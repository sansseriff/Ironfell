//! The differential harness (reactive-engine handoff §4.1, task 2; spec
//! §16.1). With the `differential` feature, the public `Store` is this
//! facade over two `StoreCore`s, `subject` and `reference`: every `&mut`
//! call goes to both, and after each the return values and the snapshots
//! (`snapshot.rs`) must be equal, or it panics naming the method and the
//! first difference. `&self` calls read the subject. So every test written
//! against `Store` is a differential script without being rewritten.
//!
//! Through phase 2b the reference ran the old engine (the whole-document
//! expansion and graph rebuild, with a working copy) and the subject the
//! new one. In phase 2c (handoff §4.6) the subject keeps copies apart from
//! the authored document, as every store does, and the reference keeps
//! them in it, as 2b did; the snapshot reads both through the joined Tree.
//! `IRON_OWNER_CHECK=1` checks the owners against the whole-document
//! expansion kept for it (`expand::expand`).
//!
//! Shared by both stores, and necessarily so: the id registries (`id.rs`),
//! so one path gets one id in both, and the text engine. Register fonts
//! before building a store, not between steps. `expr.rs`'s thread-locals
//! (`TOKENS`, `BUDGET`) are scoped to one evaluation and put back after it,
//! so the two stores, run one after the other, do not see each other's.
//!
//! **Atomicity** (a failed call leaves its store as it was) is measured:
//! set `IRON_ATOMICITY=1` and each failure of it is printed to stderr with the method and the
//! first difference. `intended::NOT_ATOMIC` lists those known.

pub mod intended;
mod snapshot;

pub use snapshot::{NOT_COMPARED, Snapshot, snapshot};

use crate::parts::Placed;
use crate::*;
use std::fmt::Debug;

/// The facade: `Store` with the `differential` feature.
pub struct Store {
    subject: StoreCore,
    reference: StoreCore,
    /// The intended difference after which the two stores legitimately
    /// differ: from then on only the subject runs and nothing is compared.
    diverged: Option<&'static str>,
}

impl std::ops::Deref for Store {
    type Target = StoreCore;
    fn deref(&self) -> &StoreCore {
        &self.subject
    }
}

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}

/// Whether a call's return value says it failed: what the atomicity check
/// looks at.
pub trait Outcome {
    fn failed(&self) -> bool {
        false
    }
}

impl<T, E> Outcome for Result<T, E> {
    fn failed(&self) -> bool {
        self.is_err()
    }
}

impl<T, E> Outcome for Option<Result<T, E>> {
    fn failed(&self) -> bool {
        matches!(self, Some(Err(_)))
    }
}

macro_rules! never_fails {
    ($($t:ty),* $(,)?) => { $(impl Outcome for $t {})* };
}
never_fails!((), bool, DragHandle, SolveReport, ProducerId, GestureId, Changes, Vec<NodeId>, Vec<(NodeId, String)>, Vec<machine::Running>, Vec<query::Request>, crate::Advanced, crate::ObserverId);

fn atomicity_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("IRON_ATOMICITY").is_some())
}

impl Store {
    /// Two stores over the same start (reactive-engine handoff §4.4, stage
    /// 5: phase 2b's engine is the reference for 2c's).
    fn pair(subject: StoreCore, reference: StoreCore, method: &str) -> Store {
        let store = Store { subject, reference, diverged: None };
        store.agree(method);
        store
    }

    /// Panic unless the two stores' snapshots are equal, or made equal by
    /// an intended difference's normaliser.
    fn agree(&self, method: &str) {
        if self.diverged.is_some() {
            return;
        }
        for (which, s) in [("subject", &self.subject), ("reference", &self.reference)] {
            if let Err(e) = s.tree().check_invariants() {
                panic!("differential: after `{method}` the {which} breaks an invariant: {e}");
            }
        }
        #[cfg(feature = "structure-oracle")]
        if let Err(e) = self.subject.check_keyed_layouts() {
            panic!("structure oracle: after `{method}`, {e}");
        }
        #[cfg(feature = "structure-oracle")]
        if let Err(e) = self.subject.check_keyed_constraints() {
            panic!("structure oracle: after `{method}`, {e}");
        }
        #[cfg(feature = "structure-oracle")]
        if let Err(e) = self.subject.check_keyed_network() {
            panic!("structure oracle: after `{method}`, {e}");
        }
        let (mut a, mut b) = (snapshot(&self.subject), snapshot(&self.reference));
        let Some(mut left) = a.diff(&b) else { return };
        for e in intended::entries() {
            if let intended::Relax::Normalise(f) = e.relax {
                f(&mut a);
                f(&mut b);
                match a.diff(&b) {
                    None => {
                        intended::count(e.name, true);
                        return;
                    }
                    Some(d) => left = d,
                }
            }
        }
        panic!("differential: after `{method}` the stores differ at {left}");
    }

    /// Stop comparing from here on, for an intended difference.
    pub fn diverged(&self) -> Option<&'static str> {
        self.diverged
    }

    /// The subject alone, for a test of the harness to make a difference
    /// the facade has to notice.
    pub fn subject_mut_for_test(&mut self) -> &mut StoreCore {
        &mut self.subject
    }

    /// One `&mut` call, made on both stores, then compared.
    fn call<R: PartialEq + Debug + Outcome>(&mut self, method: &str, f: impl Fn(&mut StoreCore) -> R) -> R {
        if self.diverged.is_some() {
            return f(&mut self.subject);
        }
        let before = atomicity_on().then(|| snapshot(&self.reference));
        let from_reference = f(&mut self.reference);
        let from_subject = f(&mut self.subject);
        if from_subject != from_reference {
            let (a, b) = (format!("{from_subject:?}"), format!("{from_reference:?}"));
            let matched = intended::entries().into_iter().find(|e| matches!(e.relax, intended::Relax::Diverge(f) if f(method, &a, &b)));
            match matched {
                Some(e) => {
                    intended::count(e.name, false);
                    self.diverged = Some(e.name);
                    return from_subject;
                }
                None => panic!("differential: `{method}` returned different values\n  subject:   {a}\n  reference: {b}"),
            }
        }
        self.agree(method);
        if let Some(before) = before
            && from_reference.failed()
            && let Some(d) = snapshot(&self.reference).diff(&before)
        {
            eprintln!("differential: not atomic: `{method}` failed with {from_reference:?} and changed the store at {d}");
        }
        from_subject
    }

    pub fn new() -> Store {
        Store::pair(StoreCore::new(), StoreCore::with_copies_in_document(Document::new()), "new")
    }

    pub fn with_document(doc: Document) -> Store {
        Store::pair(StoreCore::with_document(doc.clone()), StoreCore::with_copies_in_document(doc), "with_document")
    }

    pub fn load(text: &str) -> Result<Store, LoadError> {
        match (StoreCore::load(text), serial::load(text).map(StoreCore::with_copies_in_document)) {
            (Ok(subject), Ok(reference)) => Ok(Store::pair(subject, reference, "load")),
            (Err(a), Err(b)) => {
                assert_eq!(a.to_string(), b.to_string(), "differential: `load` failed differently");
                Err(a)
            }
            (a, b) => panic!("differential: `load` succeeded in one store only (subject ok: {}, reference ok: {})", a.is_ok(), b.is_ok()),
        }
    }

    /// The subject's document, after checking the two agree.
    pub fn into_document(self) -> Document {
        self.agree("into_document");
        self.subject.into_document()
    }

    /// The store under test, for a harness that needs it whole.
    pub fn subject(&self) -> &StoreCore {
        &self.subject
    }

    /// The reference store.
    pub fn reference(&self) -> &StoreCore {
        &self.reference
    }

    // Every public `&mut self` method of `StoreCore`, forwarded to both.
    // A method added to `StoreCore` must be added here; tests calling it
    // will not compile under the feature until it is.

    pub fn begin_drag(&mut self, id: NodeId, anchor: [f64; 2]) -> DragHandle {
        self.call("begin_drag", |s| s.begin_drag(id, anchor))
    }

    pub fn drag_to(&mut self, h: &DragHandle, world: [f64; 2]) -> SolveReport {
        self.call("drag_to", |s| s.drag_to(h, world))
    }

    pub fn unplug(&mut self, connection: NodeId, label: impl Into<String>, actor: Actor) -> Result<Version, String> {
        let label: String = label.into();
        self.call("unplug", |s| s.unplug(connection, label.clone(), actor))
    }

    pub fn begin_plug(&mut self, anchor: NodeId, moved: NodeId) -> Result<Plugging, String> {
        self.call("begin_plug", |s| s.begin_plug(anchor, moved))
    }

    pub fn plug_drag(&mut self, p: &Plugging, world: [f64; 2], target: Option<&plug::PortAnchor>) -> SolveReport {
        self.call("plug_drag", |s| s.plug_drag(p, world, target))
    }

    pub fn finish_plug(&mut self, p: Plugging, target: Option<plug::PortAnchor>) -> Result<Option<Version>, String> {
        self.call("finish_plug", |s| s.finish_plug(p.clone(), target.clone()))
    }

    pub fn cancel_plug(&mut self, p: Plugging) {
        self.call("cancel_plug", |s| s.cancel_plug(p.clone()))
    }

    pub fn end_drag(&mut self, h: &DragHandle) {
        self.call("end_drag", |s| s.end_drag(h))
    }

    pub fn set_time(&mut self, t: f64) {
        self.call("set_time", |s| s.set_time(t))
    }

    pub fn ensure_clock(&mut self) -> Result<NodeId, ApplyError> {
        self.call("ensure_clock", |s| s.ensure_clock())
    }

    pub fn register_producer(&mut self, name: &str) -> ProducerId {
        self.call("register_producer", |s| s.register_producer(name))
    }

    pub fn publish(&mut self, producer: ProducerId, values: std::collections::BTreeMap<Leaf, Value>) {
        self.call("publish", |s| s.publish(producer, values.clone()))
    }

    pub fn unpublish(&mut self, producer: ProducerId) {
        self.call("unpublish", |s| s.unpublish(producer))
    }

    pub fn freeze(&mut self, producer: ProducerId, label: String, actor: Actor, ts: u64) -> Option<Result<Applied, Vec<ApplyError>>> {
        self.call("freeze", |s| s.freeze(producer, label.clone(), actor, ts))
    }

    pub fn refresh_text(&mut self) -> bool {
        self.call("refresh_text", |s| s.refresh_text())
    }

    pub fn preview(&mut self, id: NodeId, path: LeafPath, value: Value) {
        self.call("preview", |s| s.preview(id, path, value.clone()))
    }

    pub fn unpreview(&mut self, id: NodeId, path: LeafPath) {
        self.call("unpreview", |s| s.unpreview(id, path))
    }

    pub fn preview_target(&mut self, id: NodeId, path: LeafPath, value: Value) -> Result<(), String> {
        self.call("preview_target", |s| s.preview_target(id, path, value.clone()))
    }

    pub fn begin_gesture(&mut self, nodes: &[NodeId]) -> GestureId {
        self.call("begin_gesture", |s| s.begin_gesture(nodes))
    }

    pub fn unkeyed_gesture(&mut self) {
        self.call("unkeyed_gesture", |s| s.unkeyed_gesture())
    }

    pub fn retract(&mut self, version: Version, ts: u64) -> bool {
        self.call("retract", |s| s.retract(version, ts))
    }

    pub fn merge_with_previous(&mut self, previous: Version, label: impl Into<String>) -> bool {
        let label: String = label.into();
        self.call("merge_with_previous", |s| s.merge_with_previous(previous, label.clone()))
    }

    pub fn set_through_handle(&mut self, id: NodeId, path: LeafPath, value: Value, label: impl Into<String>, actor: Actor, ts: u64) -> Result<Option<Applied>, String> {
        let label: String = label.into();
        self.call("set_through_handle", |s| s.set_through_handle(id, path, value.clone(), label.clone(), actor, ts))
    }

    pub fn commit_gesture(&mut self, label: impl Into<String>, actor: Actor, ts: u64) -> Option<Result<Applied, Vec<ApplyError>>> {
        let label: String = label.into();
        self.call("commit_gesture", |s| s.commit_gesture(label.clone(), actor, ts))
    }

    pub fn cancel_gesture(&mut self) {
        self.call("cancel_gesture", |s| s.cancel_gesture())
    }

    pub fn apply_session(&mut self, ops: Vec<Op>) -> Result<(), ApplyError> {
        self.call("apply_session", |s| s.apply_session(ops.clone()))
    }

    pub fn advance(&mut self, now: f64) -> crate::Advanced {
        self.call("advance", |s| s.advance(now))
    }

    pub fn observe(&mut self, interest: crate::Interest) -> crate::ObserverId {
        self.call("observe", |s| s.observe(interest.clone()))
    }

    pub fn reobserve(&mut self, id: crate::ObserverId, interest: crate::Interest) {
        self.call("reobserve", |s| s.reobserve(id, interest.clone()))
    }

    pub fn unobserve(&mut self, id: crate::ObserverId) {
        self.call("unobserve", |s| s.unobserve(id))
    }

    pub fn set_observe_all(&mut self, all: bool) {
        self.call("set_observe_all", |s| s.set_observe_all(all))
    }

    pub fn advance_presence(&mut self, now: f64) -> Vec<NodeId> {
        self.call("advance_presence", |s| s.advance_presence(now))
    }

    pub fn set_state(&mut self, leaf: Leaf, value: Value) -> Result<(), String> {
        self.call("set_state", |s| s.set_state(leaf, value.clone()))
    }

    pub fn fire(&mut self, node: NodeId, event: &str, actor: Actor, ts: u64) -> Result<Option<Applied>, String> {
        self.call("fire", |s| s.fire(node, event, actor, ts))
    }

    pub fn fire_at(&mut self, listens: NodeId, event: &str, actor: Actor, ts: u64) -> Result<Option<Applied>, String> {
        self.call("fire_at", |s| s.fire_at(listens, event, actor, ts))
    }

    pub fn pointer_action(&mut self, node: NodeId, event: &str, payload: [f64; 5]) -> Result<Option<String>, String> {
        self.call("pointer_action", |s| s.pointer_action(node, event, payload))
    }

    pub fn take_changes(&mut self) -> Changes {
        self.call("take_changes", |s| s.take_changes())
    }

    pub fn take_emitted(&mut self) -> Vec<(NodeId, String)> {
        self.call("take_emitted", |s| s.take_emitted())
    }

    pub fn resolve_physics(&mut self) {
        self.call("resolve_physics", |s| s.resolve_physics())
    }

    pub fn set_interaction(&mut self, states: BTreeMap<NodeId, Vec<&'static str>>) -> bool {
        self.call("set_interaction", |s| s.set_interaction(states.clone()))
    }

    pub fn set_motion(&mut self, enabled: bool) {
        self.call("set_motion", |s| s.set_motion(enabled))
    }

    pub fn animate_presentation(&mut self, leaf: Leaf, from: f64) {
        self.call("animate_presentation", |s| s.animate_presentation(leaf, from))
    }

    pub fn cancel_presentation(&mut self, leaf: Leaf) {
        self.call("cancel_presentation", |s| s.cancel_presentation(leaf))
    }

    pub fn pin_state(&mut self, pin: Option<machine::Pinned>) {
        self.call("pin_state", |s| s.pin_state(pin.clone()))
    }

    pub fn set_inside_open(&mut self, part: NodeId, open: bool) {
        self.call("set_inside_open", |s| s.set_inside_open(part, open))
    }

    pub fn advance_machines(&mut self, now: f64) -> Vec<machine::Running> {
        self.call("advance_machines", |s| s.advance_machines(now))
    }

    pub fn advance_queries(&mut self, now: f64) -> Vec<query::Request> {
        self.call("advance_queries", |s| s.advance_queries(now))
    }

    pub fn resolve_query(&mut self, node: NodeId, seq: u64, result: Result<Value, String>) -> bool {
        self.call("resolve_query", |s| s.resolve_query(node, seq, result.clone()))
    }

    pub fn advance_steps(&mut self, now: f64) -> Vec<NodeId> {
        self.call("advance_steps", |s| s.advance_steps(now))
    }

    pub fn advance_motion(&mut self, now: f64) -> Vec<NodeId> {
        self.call("advance_motion", |s| s.advance_motion(now))
    }

    pub fn set_theme(&mut self, dark: bool) {
        self.call("set_theme", |s| s.set_theme(dark))
    }

    pub fn set_layout_preview(&mut self, preview: Option<layout::Preview>) {
        self.call("set_layout_preview", |s| s.set_layout_preview(preview.clone()))
    }

    pub fn set_selection(&mut self, items: &[(NodeId, std::collections::BTreeMap<String, Value>)]) -> Result<(), ApplyError> {
        self.call("set_selection", |s| s.set_selection(items))
    }

    pub fn set_horizon(&mut self, horizon: usize) {
        self.call("set_horizon", |s| s.set_horizon(horizon))
    }

    pub fn apply(&mut self, input: TransactionInput) -> Result<Applied, Vec<ApplyError>> {
        self.call("apply", |s| s.apply(input.clone()))
    }

    pub fn replay(&mut self, tx: &Transaction) -> Result<Applied, Vec<ApplyError>> {
        self.call("replay", |s| s.replay(tx))
    }

    pub fn undo(&mut self, ts: u64) -> Option<Result<Applied, Vec<ApplyError>>> {
        self.call("undo", |s| s.undo(ts))
    }

    pub fn redo(&mut self, ts: u64) -> Option<Result<Applied, Vec<ApplyError>>> {
        self.call("redo", |s| s.redo(ts))
    }

    pub fn set_scope(&mut self, scope: Option<ScopeId>) {
        self.call("set_scope", |s| s.set_scope(scope))
    }

    pub fn set_client(&mut self, client: u32) {
        self.call("set_client", |s| s.set_client(client))
    }

    pub fn clear_history(&mut self) {
        self.call("clear_history", |s| s.clear_history())
    }

    pub fn place_part(&mut self, key: &str, composition: NodeId, at_: [f64; 2], rot: f64, actor: crate::Actor) -> Result<Placed, String> {
        self.call("place_part", |s| s.place_part(key, composition, at_, rot, actor))
    }

    pub fn place_standard(&mut self, key: &str, composition: NodeId, at: [f64; 2], actor: crate::Actor) -> Result<NodeId, String> {
        self.call("place_standard", |s| s.place_standard(key, composition, at, actor))
    }

    pub fn set_point(&mut self, point: crate::patch::Point) {
        self.call("set_point", |s| s.set_point(point.clone()))
    }

    pub fn set_session_patch(&mut self, patch: crate::patch::Patch) {
        self.call("set_session_patch", |s| s.set_session_patch(patch.clone()))
    }

    pub fn remove_session_patch(&mut self, id: crate::patch::PatchId) {
        self.call("remove_session_patch", |s| s.remove_session_patch(id))
    }

    pub fn clear_session_patches(&mut self) {
        self.call("clear_session_patches", |s| s.clear_session_patches())
    }
}

/// Layout and the bounds it measures read the subject, as every `&self` call
/// does.
impl crate::reads::Reads for Store {
    fn tree(&self) -> Tree<'_> {
        crate::reads::Reads::tree(&self.subject)
    }
    fn resolved(&self, id: NodeId, path: LeafPath) -> Option<Value> {
        crate::reads::Reads::resolved(&self.subject, id, path)
    }
    fn layout_base(&self, id: NodeId, path: LeafPath) -> Option<Value> {
        crate::reads::Reads::layout_base(&self.subject, id, path)
    }
    fn layout_preview(&self) -> Option<&crate::layout::Preview> {
        crate::reads::Reads::layout_preview(&self.subject)
    }
}
