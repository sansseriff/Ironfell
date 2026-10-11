//! `iron_document`: the authored store.
//!
//! One identity-stable, renderer-neutral document holds all authored intent.
//! Everything downstream is derived and rebuildable. This crate is that
//! document: nodes with stable ids, typed components, first-class relations,
//! an operation log with computed inverses, undo/redo, canonical JSON, and
//! elided views. It depends on nothing renderer-shaped and tests natively.
//!
//! The design record is `plans/document-spine.md`.
//!
//! ```text
//!   TransactionInput ──apply──▶ Store ──▶ Transaction (with inverse) ──▶ log
//!                                  │
//!                                  ├── document(): reads, indexes, leaves
//!                                  ├── view(): elided tree for a reader
//!                                  └── save()/load(): canonical JSON
//! ```

pub mod component;
pub mod constraint;
pub mod definition;
pub mod document;
pub mod expr;
pub mod geom;
pub mod graph;
pub mod handle;
pub mod id;
pub mod op;
pub mod order;
pub mod registry;
pub mod serial;
pub mod solve;
pub mod sym;
pub mod time;
pub mod units;
pub mod value;
pub mod view;

pub use component::{Component, ComponentKind, LeafError, LeafPath, LeafStruct};
pub use document::{Document, Node, RelKind, Relation};
pub use expr::{Expr, Leaf};
pub use geom::Affine2;
pub use graph::ResolvedChange;
pub use handle::Handle;
pub use id::{NodeId, RelationId, Version};
pub use op::{Actor, ApplyError, ErrorKind, Op, Transaction, TransactionInput};
pub use order::OrderKey;
pub use registry::{ChildPolicy, Host, TypeId, TypeSpec};
pub use serial::LoadError;
pub use solve::{Finger, SolverSummary, Unmet};
pub use units::Dim;
pub use value::{Slot, Value, ValueKind};
pub use view::{Fidelity, ViewError, ViewQuery};

use std::collections::VecDeque;

/// Result of a successful apply.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub version: Version,
    /// True when an idempotency key matched an earlier transaction and nothing
    /// was applied.
    pub replayed: bool,
    /// What the solver did, when constraints were involved (CS §9.2).
    pub solver: Option<SolverSummary>,
}

/// A drag in progress on one node (constraint-system.md §8.1).
#[derive(Clone, Debug, PartialEq)]
pub struct DragHandle {
    pub node: NodeId,
    /// The grab point in the node's local space.
    pub anchor: [f64; 2],
    pub mode: DragMode,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DragMode {
    /// The node is a cluster operand: a finger on the anchor drives the solve.
    Cluster(NodeId),
    /// No constraints: each axis previews through its own handle; a locked
    /// axis holds still, with the reason.
    Axes { free: [bool; 2], locked: Vec<String> },
    /// Nothing may move; the reasons say why.
    Locked(Vec<String>),
}

impl DragHandle {
    pub fn is_locked(&self) -> bool {
        matches!(self.mode, DragMode::Locked(_))
    }
}

/// What one frame of a drag cost (CS phase 3 exit 9).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolveReport {
    pub clusters: usize,
    pub vars: usize,
    pub iterations: u32,
    pub evaluations: u64,
    pub converged: bool,
}

/// Undo/redo stacks plus the log of everything applied, in order.
#[derive(Clone, Debug, Default)]
pub struct History {
    past: Vec<Transaction>,
    future: Vec<Transaction>,
    log: VecDeque<Transaction>,
    horizon: usize,
}

impl History {
    pub fn past(&self) -> &[Transaction] {
        &self.past
    }

    pub fn future(&self) -> &[Transaction] {
        &self.future
    }

    /// Every applied transaction, including undo and redo ones, oldest first.
    /// Bounded by the horizon; the append-only file is the durable log.
    pub fn log(&self) -> impl Iterator<Item = &Transaction> {
        self.log.iter()
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    fn record(&mut self, tx: Transaction) {
        self.log.push_back(tx);
        while self.log.len() > self.horizon {
            self.log.pop_front();
        }
    }
}

pub struct Store {
    doc: Document,
    history: History,
    recent_keys: VecDeque<(String, Version)>,
    reactive: graph::Reactive,
    /// The playhead, in seconds. Runtime state, never saved.
    time: f64,
    time_errors: std::collections::BTreeMap<NodeId, String>,
    /// Constraint clusters, rebuilt after every transaction.
    clusters: solve::Clusters,
    /// Leaves the current gesture explicitly moved (as opposed to leaves the
    /// solver moved for it): what a commit writes.
    drivers: std::collections::BTreeSet<Leaf>,
    /// Active drag drivers.
    fingers: Vec<Finger>,
}

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}

const DEFAULT_HORIZON: usize = 200;
const RECENT_KEYS: usize = 64;

impl Store {
    pub fn new() -> Store {
        Store::with_document(Document::new())
    }

    pub fn with_document(doc: Document) -> Store {
        let mut store = Store {
            doc,
            history: History {
                horizon: DEFAULT_HORIZON,
                ..Default::default()
            },
            recent_keys: VecDeque::new(),
            reactive: graph::Reactive::default(),
            time: 0.0,
            time_errors: Default::default(),
            clusters: Default::default(),
            drivers: Default::default(),
            fingers: Vec::new(),
        };
        store.reactive.rebuild(&store.doc);
        store.resample();
        store.rebuild_clusters();
        store
    }

    /// The constraint clusters as last built, in solve order.
    pub fn clusters(&self) -> &solve::Clusters {
        &self.clusters
    }

    fn rebuild_clusters(&mut self) {
        // A malformed document (which `apply` refuses) or a cycle leaves the
        // last good clusters; `post_apply` reports the error.
        if let Ok(c) = solve::clusters(&self.doc, &self.reactive) {
            self.clusters = c;
        }
    }

    /// Re-solve every cluster from the current resolved values, with the
    /// active fingers, previewing the results. Used during gestures and when
    /// the playhead moves a cluster's input.
    fn resolve_clusters_live(&mut self) -> SolveReport {
        let mut report = SolveReport { converged: true, ..Default::default() };
        if self.clusters.list.is_empty() {
            return report;
        }
        let clusters = self.clusters.list.clone();
        for c in &clusters {
            let result = {
                let read = |l: Leaf| self.reactive.read(&self.doc, l);
                solve::solve_cluster(&self.doc, &read, c, &[], &self.fingers)
            };
            report.clusters += 1;
            report.vars += c.vars.len();
            report.iterations += result.iterations;
            report.evaluations += result.evaluations;
            if result.status != iron_solve::Status::Converged {
                // Keep the previous pose rather than show a jump (CS §5.5).
                report.converged = false;
                continue;
            }
            for (leaf, v) in result.values {
                let value = Value::Number(v);
                if self.reactive.read(&self.doc, leaf) != Some(value.clone()) {
                    self.reactive.preview(&self.doc, leaf, value);
                }
            }
        }
        report
    }

    /// Start dragging `id` by `anchor` (local coordinates). Decides the
    /// drag's mode from the roles at this moment.
    pub fn begin_drag(&mut self, id: NodeId, anchor: [f64; 2]) -> DragHandle {
        let t2 = |f: &str| LeafPath::new(ComponentKind::Transform2d, f).expect("declared");
        if let Some(cluster) = self.clusters.containing(id) {
            let free: Vec<Leaf> = cluster.vars.iter().copied().filter(|v| v.node == id).collect();
            if free.is_empty() {
                return DragHandle { node: id, anchor, mode: DragMode::Locked(vec![format!("{id} is fixed or bound in cluster {}", cluster.id)]) };
            }
            let cluster_id = cluster.id;
            let target = self.world_affine(id).apply(anchor);
            self.fingers.push(Finger { node: id, anchor, target });
            self.drivers.extend(free);
            return DragHandle { node: id, anchor, mode: DragMode::Cluster(cluster_id) };
        }
        let mut free = [false, false];
        let mut locked = Vec::new();
        for (i, f) in ["x", "y"].iter().enumerate() {
            match self.handle(id, t2(f)) {
                Handle::Locked(reason) => locked.push(reason),
                _ => free[i] = true,
            }
        }
        if free == [false, false] {
            return DragHandle { node: id, anchor, mode: DragMode::Locked(locked) };
        }
        DragHandle { node: id, anchor, mode: DragMode::Axes { free, locked } }
    }

    /// Move the drag so that the anchor lands at `world` (document space,
    /// as far as the constraints and handles allow). Results are previewed.
    pub fn drag_to(&mut self, h: &DragHandle, world: [f64; 2]) -> SolveReport {
        match &h.mode {
            DragMode::Locked(_) => SolveReport { converged: true, ..Default::default() },
            DragMode::Cluster(_) => {
                for f in &mut self.fingers {
                    if f.node == h.node && f.anchor == h.anchor {
                        f.target = world;
                    }
                }
                self.resolve_clusters_live()
            }
            DragMode::Axes { free, .. } => {
                // The origin that puts the anchor at `world`: undo the
                // parent's transform, then the node's own rotation and scale.
                let t2 = |f: &str| LeafPath::new(ComponentKind::Transform2d, f).expect("declared");
                let num = |s: &Store, f: &str, d: f64| s.resolved(h.node, t2(f)).and_then(|v| v.as_f64()).unwrap_or(d);
                let parent = match self.doc.live(h.node).and_then(|n| n.parent) {
                    Some(p) => self.world_affine(p),
                    None => Affine2::IDENTITY,
                };
                let local_target = parent.inverse().unwrap_or(Affine2::IDENTITY).apply(world);
                let m = Affine2::from_trs(0.0, 0.0, num(self, "rot", 0.0), num(self, "sx", 1.0), num(self, "sy", 1.0));
                let offset = m.apply(h.anchor);
                let origin = [local_target[0] - offset[0], local_target[1] - offset[1]];
                for (i, f) in ["x", "y"].iter().enumerate() {
                    if free[i] {
                        let _ = self.preview_target(h.node, t2(f), Value::Number(origin[i]));
                    }
                }
                SolveReport { converged: true, ..Default::default() }
            }
        }
    }

    /// Forget the drag's finger without committing or cancelling the gesture.
    pub fn end_drag(&mut self, h: &DragHandle) {
        self.fingers.retain(|f| !(f.node == h.node && f.anchor == h.anchor));
    }

    pub fn time(&self) -> f64 {
        self.time
    }

    /// Move the playhead. Every clip is resampled; leaves whose animated
    /// value moved are reported like any other resolved change.
    pub fn set_time(&mut self, t: f64) {
        self.time = t;
        self.resample();
        if !self.clusters.list.is_empty() {
            self.resolve_clusters_live();
        }
    }

    /// Clips that could not be sampled at the current time, by clip node.
    pub fn time_errors(&self) -> &std::collections::BTreeMap<NodeId, String> {
        &self.time_errors
    }

    fn resample(&mut self) {
        let sample = time::sample(&self.doc, self.time);
        self.time_errors = sample.errors;
        self.reactive.set_animated(&self.doc, sample.values);
    }

    pub fn reactive(&self) -> &graph::Reactive {
        &self.reactive
    }

    /// The value a reader sees for a leaf: the gesture preview if one is in
    /// progress, else the binding's resolved value, else the constant.
    pub fn resolved(&self, id: NodeId, path: LeafPath) -> Option<Value> {
        self.reactive.read(&self.doc, Leaf { node: id, path })
    }

    pub fn has_changes(&self) -> bool {
        self.reactive.has_changes()
    }

    /// The node's local space to document 2D space, through every live
    /// ancestor, reading resolved values (previews, bindings, clips). Equal
    /// to the materialised entity's `GlobalTransform`; see `geom`.
    pub fn world_affine(&self, id: NodeId) -> Affine2 {
        geom::world_affine(&self.doc, &|l| self.reactive.read(&self.doc, l), id)
    }

    /// Leaves whose resolved value changed since the last drain.
    pub fn drain_changes(&mut self) -> Vec<ResolvedChange> {
        self.reactive.drain_changes()
    }

    /// Preview a leaf's value for the duration of a gesture. Bindings that
    /// read it re-evaluate now; nothing is written to the document until
    /// [`Store::commit_gesture`].
    pub fn preview(&mut self, id: NodeId, path: LeafPath, value: Value) {
        let leaf = Leaf { node: id, path };
        self.drivers.insert(leaf);
        self.reactive.preview(&self.doc, leaf, value);
        if !self.clusters.list.is_empty() {
            self.resolve_clusters_live();
        }
    }

    /// How a gesture on this leaf would be realised right now: directly, by
    /// inverting a linear binding onto its one constant source, or not at
    /// all (see `handle`).
    pub fn handle(&self, id: NodeId, path: LeafPath) -> Handle {
        handle::handle(&self.doc, &self.reactive, Leaf { node: id, path })
    }

    /// Preview a *target* value for a leaf through its handle: a constant is
    /// previewed as is; a linearly bound leaf previews its source so the
    /// binding re-evaluates to `value`. `Err` with the reason when locked.
    pub fn preview_target(&mut self, id: NodeId, path: LeafPath, value: Value) -> Result<(), String> {
        let (target, v) = handle::resolve_target(&self.doc, &self.reactive, Leaf { node: id, path }, value)?;
        self.preview(target.node, target.path, v);
        Ok(())
    }

    pub fn gesture_in_progress(&self) -> bool {
        !self.reactive.overlay().is_empty()
    }

    /// Turn the gesture into one transaction: a `Set` for every leaf the
    /// gesture drove (not the leaves the solver moved for it: solve-on-apply
    /// reproduces those as `derived_ops`, warm-started from the previewed
    /// pose). Previews on bound leaves are dropped: a derived value has no
    /// handle without a disambiguation policy (doc 02 invariant 2). `None`
    /// when nothing was previewed or nothing differs from the document.
    pub fn commit_gesture(
        &mut self,
        label: impl Into<String>,
        actor: Actor,
        ts: u64,
    ) -> Option<Result<Applied, Vec<ApplyError>>> {
        let mut ops = Vec::new();
        for (leaf, value) in self.reactive.overlay() {
            if !self.drivers.contains(leaf) {
                continue;
            }
            let Some(slot) = self.doc.leaf(leaf.node, leaf.path) else {
                continue;
            };
            if slot.expr().is_some() {
                continue;
            }
            if slot.constant() != Some(value) {
                ops.push(Op::Set {
                    id: leaf.node,
                    path: leaf.path,
                    slot: Slot::Const(value.clone()),
                });
            }
        }
        if ops.is_empty() {
            self.cancel_gesture();
            return None;
        }
        let result = self.apply(TransactionInput {
            label: label.into(),
            actor,
            ts,
            idempotency_key: None,
            ops,
            exact: false,
        });
        self.drivers.clear();
        self.fingers.clear();
        self.reactive.clear_overlay(&self.doc);
        Some(result)
    }

    pub fn cancel_gesture(&mut self) {
        self.drivers.clear();
        self.fingers.clear();
        self.reactive.clear_overlay(&self.doc);
    }

    /// Keep the graph in step with a transaction that just applied.
    fn after_transaction(&mut self, tx: &Transaction) {
        let structural = tx.ops.iter().chain(tx.inverse.iter()).any(|op| match op {
            Op::Create { .. } | Op::Delete { .. } | Op::AddComp { .. } | Op::RemoveComp { .. } => true,
            Op::Set { slot, .. } => slot.expr().is_some(),
            _ => false,
        });
        if structural {
            self.reactive.rebuild(&self.doc);
            self.resample();
        } else {
            let changed: Vec<Leaf> = tx
                .ops
                .iter()
                .filter_map(|op| match op {
                    Op::Set { id, path, .. } => Some(Leaf {
                        node: *id,
                        path: *path,
                    }),
                    _ => None,
                })
                .collect();
            self.reactive.invalidate(&self.doc, changed);
            // A set on a clip's timing, or a link, changes the sample too.
            // Cheap enough to do unconditionally: clips are few.
            self.resample();
        }
        self.rebuild_clusters();
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn set_horizon(&mut self, horizon: usize) {
        self.history.horizon = horizon.max(1);
        while self.history.past.len() > self.history.horizon {
            self.history.past.remove(0);
        }
    }

    /// Apply a transaction atomically. Either every op applies and the
    /// version advances, or nothing applies and every error is reported.
    pub fn apply(&mut self, input: TransactionInput) -> Result<Applied, Vec<ApplyError>> {
        if let Some(k) = &input.idempotency_key
            && let Some((_, v)) = self.recent_keys.iter().find(|(key, _)| key == k)
        {
            return Ok(Applied {
                version: *v,
                replayed: true,
                solver: None,
            });
        }
        let exact = input.exact;
        let (mut tx, doc) = self.run(input)?;
        let old = std::mem::replace(&mut self.doc, doc);
        self.after_transaction(&tx);
        let solver = match self.post_apply(&mut tx, exact) {
            Ok(s) => s,
            Err(e) => {
                // Everything after `run` is undone by putting the old
                // document back; the graph is rebuilt from it.
                self.doc = old;
                self.reactive.rebuild(&self.doc);
                self.resample();
                self.rebuild_clusters();
                return Err(e);
            }
        };
        tx.solver = solver.clone();
        let version = tx.version;
        if let Some(k) = &tx.idempotency_key {
            self.recent_keys.push_back((k.clone(), version));
            while self.recent_keys.len() > RECENT_KEYS {
                self.recent_keys.pop_front();
            }
        }
        self.history.future.clear();
        self.history.past.push(tx.clone());
        while self.history.past.len() > self.history.horizon {
            self.history.past.remove(0);
        }
        self.history.record(tx);
        Ok(Applied {
            version,
            replayed: false,
            solver,
        })
    }

    /// Apply a recorded transaction's requested and derived ops without
    /// solving: replaying a log must not depend on the solver (CS §7).
    pub fn replay(&mut self, tx: &Transaction) -> Result<Applied, Vec<ApplyError>> {
        let input = TransactionInput {
            label: tx.label.clone(),
            actor: tx.actor,
            ts: tx.ts,
            idempotency_key: None,
            ops: tx.replay_ops(),
            exact: false,
        };
        let (applied, doc) = self.run(input)?;
        self.doc = doc;
        self.after_transaction(&applied);
        let version = applied.version;
        self.history.future.clear();
        self.history.past.push(applied.clone());
        self.history.record(applied);
        Ok(Applied { version, replayed: false, solver: None })
    }

    /// Undo the most recent transaction. `ts` is the caller's clock.
    /// `None` when there is nothing to undo.
    pub fn undo(&mut self, ts: u64) -> Option<Result<Applied, Vec<ApplyError>>> {
        let tx = self.history.past.pop()?;
        let input = TransactionInput {
            label: format!("undo: {}", tx.label),
            actor: Actor::System,
            ts,
            idempotency_key: None,
            ops: tx.undo_ops(),
            exact: false,
        };
        Some(match self.run(input) {
            Ok((applied, doc)) => {
                self.doc = doc;
                self.after_transaction(&applied);
                let version = applied.version;
                self.history.record(applied);
                self.history.future.push(tx);
                Ok(Applied {
                    version,
                    replayed: false,
                    solver: None,
                })
            }
            Err(e) => {
                self.history.past.push(tx);
                Err(e)
            }
        })
    }

    pub fn redo(&mut self, ts: u64) -> Option<Result<Applied, Vec<ApplyError>>> {
        let tx = self.history.future.pop()?;
        // Requested and derived ops, no solve: the log is the ground truth.
        let input = TransactionInput {
            label: format!("redo: {}", tx.label),
            actor: Actor::System,
            ts,
            idempotency_key: None,
            ops: tx.replay_ops(),
            exact: false,
        };
        Some(match self.run(input) {
            Ok((applied, doc)) => {
                self.doc = doc;
                self.after_transaction(&applied);
                let version = applied.version;
                self.history.past.push(applied.clone());
                self.history.record(applied);
                Ok(Applied {
                    version,
                    replayed: false,
                    solver: None,
                })
            }
            Err(e) => {
                self.history.future.push(tx);
                Err(e)
            }
        })
    }

    /// What needs the whole transaction in place (constraint-system.md §7):
    /// every constraint is well formed, bindings and clusters form no cycle,
    /// and every cluster is solved, with leaves this transaction set treated
    /// as drivers. The solver's writes are applied and recorded as
    /// `derived_ops`. Runs on the committed document; the caller rolls back
    /// on `Err`.
    fn post_apply(&mut self, tx: &mut Transaction, exact: bool) -> Result<Option<SolverSummary>, Vec<ApplyError>> {
        let last = tx.ops.len().saturating_sub(1);
        let err = |kind: ErrorKind, message: String| vec![ApplyError { op: last, kind, message }];
        {
            let read = |l: Leaf| self.reactive.read(&self.doc, l);
            constraint::validate_all(&self.doc, &read).map_err(|m| err(ErrorKind::BadConstraint, m))?;
        }
        let clusters = solve::clusters(&self.doc, &self.reactive).map_err(|m| err(ErrorKind::Cycle, m))?;
        self.clusters = clusters.clone();
        if clusters.list.is_empty() {
            return Ok(None);
        }
        let drivers: Vec<solve::Driver> = tx
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Set { id, path, slot } => slot.constant().and_then(|v| v.as_f64()).map(|requested| solve::Driver {
                    leaf: Leaf { node: *id, path: *path },
                    requested,
                }),
                _ => None,
            })
            .collect();
        let mut summary = SolverSummary::default();
        for c in &clusters.list {
            let result = {
                let read = |l: Leaf| self.reactive.read(&self.doc, l);
                solve::solve_cluster(&self.doc, &read, c, &drivers, &self.fingers)
            };
            if result.status != iron_solve::Status::Converged {
                let names: Vec<String> = result.culprits.iter().map(|c| c.to_string()).collect();
                return Err(err(
                    ErrorKind::Unsatisfiable,
                    format!(
                        "constraints of cluster {} cannot all hold (residual {:.3e}); likely at fault: {}",
                        c.id,
                        result.max_required_residual,
                        if names.is_empty() { "unknown".to_owned() } else { names.join(", ") }
                    ),
                ));
            }
            let mut changed = Vec::new();
            for (leaf, v) in result.values {
                let current = self.doc.leaf(leaf.node, leaf.path).and_then(|s| s.constant()).and_then(|x| x.as_f64());
                if current == Some(v) {
                    continue;
                }
                let op = Op::Set { id: leaf.node, path: leaf.path, slot: Slot::Const(Value::Number(v)) };
                let inverse = op::apply_op(&mut self.doc, last, &op, tx.version).map_err(|e| vec![e])?;
                tx.derived_ops.push(op);
                tx.inverse.extend(inverse);
                changed.push(leaf);
                if !summary.moved.contains(&leaf.node) {
                    summary.moved.push(leaf.node);
                }
            }
            if !changed.is_empty() || !result.unmet.is_empty() {
                summary.clusters.push(c.id);
            }
            self.reactive.invalidate(&self.doc, changed);
            summary.unmet.extend(result.unmet);
        }
        if exact && !summary.unmet.is_empty() {
            let lines: Vec<String> = summary.unmet.iter().map(|u| format!("{} requested {} but constraints allow {}", u.leaf, crate::value::fmt_num(u.requested), crate::value::fmt_num(u.solved))).collect();
            return Err(err(ErrorKind::Unsatisfiable, format!("exact transaction not honoured: {}", lines.join("; "))));
        }
        // Drivers that are not variables of any cluster were plain sets;
        // only a moved node is a cause worth telling the model about.
        Ok((!summary.is_empty()).then_some(summary))
    }

    /// Run the ops against a working copy and return it with the
    /// transaction; the caller commits the copy.
    ///
    /// Cloning per transaction is the simplest way to report every error in a
    /// batch and leave the store untouched on failure. Transactions are
    /// coalesced per gesture, so this is one clone per commit, not per frame.
    /// If it ever shows up in a profile, the alternative is applying with
    /// rollback through the inverses.
    fn run(&mut self, input: TransactionInput) -> Result<(Transaction, Document), Vec<ApplyError>> {
        let mut work = self.doc.clone();
        let stamp = work.version().next();
        let mut inverse = Vec::new();
        let mut errors = Vec::new();
        for (i, op) in input.ops.iter().enumerate() {
            match op::apply_op(&mut work, i, op, stamp) {
                Ok(inv) => inverse.extend(inv),
                Err(e) => errors.push(e),
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        work.set_version(stamp);
        Ok((
            Transaction {
                version: stamp,
                label: input.label,
                actor: input.actor,
                ts: input.ts,
                idempotency_key: input.idempotency_key,
                ops: input.ops,
                derived_ops: Vec::new(),
                inverse,
                solver: None,
            },
            work,
        ))
    }

    pub fn view(&self, q: &ViewQuery) -> Result<String, ViewError> {
        view::render_tree(&self.doc, &self.reactive, &self.clusters.var_cluster, q)
    }

    pub fn save(&self) -> String {
        serial::save(&self.doc)
    }

    /// A store over a loaded document, with empty history.
    pub fn load(text: &str) -> Result<Store, LoadError> {
        serial::load(text).map(Store::with_document)
    }
}
