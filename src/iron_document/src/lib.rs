//! `iron_document`: the authored store.
//!
//! One identity-stable, renderer-neutral document holds all authored intent.
//! Everything downstream is derived and rebuildable. This crate is that
//! document: nodes with stable ids, typed facets, first-class relations,
//! an operation log with computed inverses, undo/redo, canonical JSON, and
//! elided views. It depends on nothing renderer-shaped and tests natively.
//!
//! The design record is `plans/specs/document-spine.md`.
//!
//! ```text
//!   TransactionInput ──apply──▶ Store ──▶ Transaction (with inverse) ──▶ log
//!                                  │
//!                                  ├── document(): reads, indexes, leaves
//!                                  ├── view(): elided tree for a reader
//!                                  └── save()/load(): canonical JSON
//! ```

pub mod facet;
pub mod composition;
pub mod constraint;
pub mod contour;
pub mod copy;
pub mod definition;
pub mod document;
pub mod expand;
pub mod make;
pub mod expr;
pub mod geom;
pub mod gesture;
pub mod anchor;
pub mod author;
pub mod appearance;
pub mod graph;
pub mod handle;
pub mod id;
pub mod op;
pub mod order;
pub mod port;
pub mod registry;
pub mod serial;
pub mod shape;
pub mod plug;
pub mod solve;
pub mod standard;
pub mod sym;
pub mod text;
pub mod time;
pub mod units;
pub mod value;
pub mod view;

pub use facet::{Facet, FacetKind, LeafError, LeafPath, LeafStruct};
pub use document::{Document, Node, RelKind, Relation};
pub use expr::{Expr, Leaf};
pub use geom::{Affine2, Affine3};
pub use gesture::{GestureId, GestureStatus};
pub use graph::{ProducerId, ResolvedChange, Signal};
pub use handle::Handle;
pub use id::{NodeId, RelationId, Version};
pub use op::{Actor, ApplyError, ErrorKind, Op, Transaction, TransactionInput};
pub use order::OrderKey;
pub use registry::{ChildPolicy, Host, TypeId, TypeSpec};
pub use serial::LoadError;
pub use solve::{Finger, SolverSummary, Unmet};
pub use units::Dim;
pub use value::{Slot, Value, ValueKind};
pub use view::{Fidelity, ViewError, ViewQuery, ViewRelation};

use std::collections::{BTreeMap, VecDeque};

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

/// A plug drag in progress (component-model.md §4.3), from
/// `Store::begin_plug`: a port dragged to connect or disconnect it.
#[derive(Clone, Debug, PartialEq)]
pub struct Plugging {
    /// The port dragged from, as it was when the drag began.
    pub from: plug::PortAnchor,
    /// What follows the pointer: the loose end itself, or the instance a
    /// rigid port belongs to.
    pub moved: NodeId,
    pub drag: DragHandle,
    /// `moved`'s rotation when the drag began.
    pub rot0: f64,
    /// The port it was connected to, and the unplug applied for the drag.
    pub was: Option<NodeId>,
    unplugged: Option<Version>,
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
    /// Bumped by every session-tier change (`apply_session`), which leaves
    /// the document's version alone.
    session_version: u64,
    physics_error: Option<String>,
    /// What the last expansion could not derive (component-model.md §3).
    expansion_problems: Vec<String>,
    /// The gesture whose previews are in the overlay (`gesture`).
    gesture: Option<gesture::ActiveGesture>,
    /// How the last gesture ended, for its tool to find out.
    ended: Option<(GestureId, GestureStatus)>,
    next_gesture: u64,
    /// What moved, for spatial consumers (`geom::Revisions`).
    geometry: geom::Revisions,
    /// Host producers of runtime signals, by id (vision 10 §4). The two
    /// built-in producers (clips, the network) are not listed.
    producers: std::collections::BTreeMap<ProducerId, String>,
    /// Bumped on every publish: the stamp signals carry.
    signal_stamp: u64,
    /// The text engine generation text was last measured under
    /// (`refresh_text`).
    text_generation: u64,
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
            session_version: 0,
            physics_error: None,
            expansion_problems: Vec::new(),
            gesture: None,
            ended: None,
            next_gesture: 1,
            geometry: geom::Revisions::default(),
            producers: Default::default(),
            signal_stamp: 0,
            text_generation: iron_text::engine().generation(),
        };
        // A loaded document holds only authored nodes: derive its instances.
        let stamp = store.doc.version();
        store.expansion_problems = expand::expand(&mut store.doc, stamp).1;
        store.geometry.note_structure();
        store.reactive.rebuild(&store.doc);
        store.resample();
        store.rebuild_clusters();
        store.resolve_physics();
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
        self.watch(id);
        let t2 = |f: &str| LeafPath::new(FacetKind::Transform2d, f).expect("declared");
        if let Some(cluster) = self.clusters.containing(id) {
            let free: Vec<Leaf> = cluster.vars.iter().copied().filter(|v| v.node == id).collect();
            if free.is_empty() {
                return DragHandle { node: id, anchor, mode: DragMode::Locked(vec![format!("{id} is fixed or bound in cluster {}", cluster.id)]) };
            }
            let cluster_id = cluster.id;
            let target = self.world_affine(id).apply(anchor);
            let mated = cluster.constraints.iter().any(|c| c.spec.kind == "mate" && c.nodes().any(|n| n == id));
            let hold_rot = mated.then(|| self.resolved(id, t2("rot")).and_then(|v| v.as_f64()).unwrap_or(0.0));
            self.fingers.push(Finger { node: id, anchor, target, hold_rot });
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
                let t2 = |f: &str| LeafPath::new(FacetKind::Transform2d, f).expect("declared");
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

    // ---- Plugging (component-model.md §4.3) --------------------------------

    /// Every port anchor, with where it is and what it is connected to.
    pub fn port_anchors(&self) -> Vec<plug::PortAnchor> {
        let read = |l: Leaf| self.reactive.read(&self.doc, l);
        plug::port_anchors(&self.doc, &read)
    }

    /// Remove a connection, as one transaction: the far ends that followed
    /// it stay where they are (their positions set, as overrides inside an
    /// instance) rather than return to their template positions.
    pub fn unplug(&mut self, connection: NodeId, label: impl Into<String>, actor: Actor) -> Result<Version, String> {
        let t2 = |f: &str| LeafPath::new(FacetKind::Transform2d, f).expect("declared");
        let ends = port::ends(&self.doc, connection).ok_or_else(|| format!("{connection} is not a connection"))?;
        let pins: Vec<(NodeId, f64, f64)> = self
            .port_anchors()
            .into_iter()
            .filter(|a| a.follow && (a.outer == ends.0 || a.outer == ends.1 || a.port == ends.0 || a.port == ends.1))
            .filter_map(|a| Some((a.anchor, self.resolved(a.anchor, t2("x"))?.as_f64()?, self.resolved(a.anchor, t2("y"))?.as_f64()?)))
            .collect();
        let mut ops: Vec<Op> = self.doc.relations_from(connection).map(|r| Op::Unlink { id: r.id }).collect();
        ops.push(Op::Delete { id: connection });
        let label = label.into();
        let tx = |label: &str, ops| TransactionInput { label: label.to_owned(), actor, ts: 0, idempotency_key: None, ops, exact: false };
        let first = self.apply(tx(&label, ops)).map_err(|e| e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?.version;
        let sets: Vec<Op> = pins
            .into_iter()
            .flat_map(|(a, x, y)| [Op::Set { id: a, path: t2("x"), slot: Slot::Const(Value::Number(x)) }, Op::Set { id: a, path: t2("y"), slot: Slot::Const(Value::Number(y)) }])
            .collect();
        if sets.is_empty() {
            return Ok(first);
        }
        match self.apply(tx(&label, sets)) {
            Ok(a) => {
                self.merge_with_previous(first, label);
                Ok(a.version)
            }
            Err(e) => {
                self.retract(first, 0);
                Err(e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))
            }
        }
    }

    /// Start dragging the port at `anchor` to plug it somewhere: a loose
    /// end moves itself, a rigid port moves `moved` (the instance it
    /// belongs to, as the pointer reached it). A connected port is unplugged
    /// at once, so the drag shows it free; `finish_plug` or `cancel_plug`
    /// settles it.
    pub fn begin_plug(&mut self, anchor: NodeId, moved: NodeId) -> Result<Plugging, String> {
        let from = self.port_anchors().into_iter().find(|a| a.anchor == anchor).ok_or_else(|| format!("{anchor} is not a port's anchor"))?;
        if from.sealed {
            return Err(format!("{} is inside a component that does not expose it", from.port));
        }
        let unplugged = match from.connection {
            Some(c) => Some(self.unplug(c, format!("disconnect {}", from.name), Actor::Human)?),
            None => None,
        };
        let moved = if from.follow { anchor } else { moved };
        self.begin_gesture(&[moved]);
        let grab = if from.follow {
            [0.0, 0.0]
        } else {
            let m = self.world_affine(moved).inverse().unwrap_or(Affine2::IDENTITY).then(self.world_affine(anchor));
            m.apply([0.0, 0.0])
        };
        let drag = self.begin_drag(moved, grab);
        let rot0 = self.resolved(moved, LeafPath::new(FacetKind::Transform2d, "rot").expect("declared")).and_then(|v| v.as_f64()).unwrap_or(0.0);
        Ok(Plugging { was: from.partner, from, moved, drag, rot0, unplugged })
    }

    /// The ports the drag could plug into now.
    pub fn plug_targets(&self, p: &Plugging) -> Vec<plug::PortAnchor> {
        self.port_anchors().into_iter().filter(|t| plug::compatible(&self.doc, &p.from, t).is_ok()).collect()
    }

    /// Move the drag's port to `world` (its composition), or snap it onto
    /// `target`: onto its anchor, and turned to face it when it is rigid.
    pub fn plug_drag(&mut self, p: &Plugging, world: [f64; 2], target: Option<&plug::PortAnchor>) -> SolveReport {
        let at = target.map_or(world, |t| t.world);
        if !p.from.follow {
            let rot = match target {
                Some(t) if !t.follow => {
                    let turn = t.facing + std::f64::consts::PI - p.from.facing;
                    p.rot0 + (turn + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
                }
                _ => p.rot0,
            };
            let path = LeafPath::new(FacetKind::Transform2d, "rot").expect("declared");
            if self.resolved(p.moved, path).and_then(|v| v.as_f64()).is_none_or(|r| (r - rot).abs() > 1e-12) {
                let _ = self.preview_target(p.moved, path, Value::Number(rot));
            }
        }
        self.drag_to(&p.drag, at)
    }

    /// Release a plug drag: plugged into `target` when given, else left
    /// where it was dropped. One history entry with the drag's unplug;
    /// released back on the port it came from, nothing changes. Returns the
    /// entry's version, `None` when nothing changed.
    pub fn finish_plug(&mut self, p: Plugging, target: Option<plug::PortAnchor>) -> Result<Option<Version>, String> {
        self.end_drag(&p.drag);
        if p.was.is_some() && target.as_ref().map(|t| t.outer) == p.was {
            self.cancel_gesture();
            if let Some(v) = p.unplugged {
                self.retract(v, 0);
            }
            return Ok(None);
        }
        let label = match &target {
            Some(t) => format!("connect {} to {}", p.from.name, t.name),
            None if p.was.is_some() => format!("disconnect {}", p.from.name),
            None => format!("move {}", p.from.name),
        };
        let mut last = p.unplugged;
        let fold = |s: &mut Store, last: &mut Option<Version>, now: Version| {
            if let Some(v) = *last {
                s.merge_with_previous(v, label.clone());
            }
            *last = Some(now);
        };
        if p.from.follow && target.is_some() {
            // The end will sit on its partner; where the pointer left it
            // is not kept.
            self.cancel_gesture();
        } else {
            match self.commit_gesture(label.clone(), Actor::Human, 0) {
                Some(Ok(a)) => fold(self, &mut last, a.version),
                Some(Err(e)) => return Err(e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")),
                None => {}
            }
        }
        if let Some(t) = target {
            let id = self.doc.next_id();
            let rels = self.doc.next_relation_ids(2);
            // Two loose ends: the one landed on holds the joint.
            let held = (p.from.follow && t.follow).then_some(t.outer);
            let ops = plug::plug_ops(&self.doc, p.from.outer, t.outer, held, id, [rels[0], rels[1]])?;
            let input = TransactionInput { label: label.clone(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false };
            let a = self.apply(input).map_err(|e| e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
            fold(self, &mut last, a.version);
        }
        Ok(last)
    }

    /// Abandon a plug drag (Esc): nothing moves, and the port is connected
    /// as it was.
    pub fn cancel_plug(&mut self, p: Plugging) {
        self.end_drag(&p.drag);
        self.cancel_gesture();
        if let Some(v) = p.unplugged {
            self.retract(v, 0);
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
        self.resolve_physics();
    }

    /// Clips that could not be sampled at the current time, by clip node.
    pub fn time_errors(&self) -> &std::collections::BTreeMap<NodeId, String> {
        &self.time_errors
    }

    fn resample(&mut self) {
        let sample = time::sample(&self.doc, self.time);
        self.time_errors = sample.errors;
        self.signal_stamp += 1;
        self.reactive.publish(&self.doc, ProducerId::CLIPS, sample.values, self.signal_stamp);
    }

    /// Name a host producer of runtime signals (a simulation, a Model) and
    /// get the id its publishes carry. Names are for reporting only.
    pub fn register_producer(&mut self, name: &str) -> ProducerId {
        let id = ProducerId(self.producers.keys().next_back().map_or(ProducerId::FIRST_HOST.0, |p| p.0 + 1));
        self.producers.insert(id, name.to_owned());
        id
    }

    /// The host producers registered so far, by id.
    pub fn producers(&self) -> impl Iterator<Item = (ProducerId, &str)> {
        self.producers.iter().map(|(p, n)| (*p, n.as_str()))
    }

    /// The host producer registered under `name`, if any.
    pub fn producer(&self, name: &str) -> Option<ProducerId> {
        self.producers.iter().find(|(_, n)| n.as_str() == name).map(|(p, _)| *p)
    }

    /// A producer's name: the two built-in ones, or what the host registered.
    pub fn producer_name(&self, producer: ProducerId) -> String {
        match producer {
            ProducerId::CLIPS => "clips".to_owned(),
            ProducerId::NETWORK => "network".to_owned(),
            p => self.producers.get(&p).cloned().unwrap_or_else(|| format!("producer {}", p.0)),
        }
    }

    /// Publish a host producer's signals for this frame (vision 10 §4):
    /// `values` replaces everything the producer published before. The
    /// values are read beneath bindings and above authored constants, never
    /// enter the document or its history, and re-evaluate what reads them.
    /// A leaf another producer holds, or a bound leaf, is refused and listed
    /// in `signal_errors`.
    pub fn publish(&mut self, producer: ProducerId, values: std::collections::BTreeMap<Leaf, Value>) {
        self.signal_stamp += 1;
        self.reactive.publish(&self.doc, producer, values, self.signal_stamp);
        if !self.clusters.list.is_empty() {
            self.resolve_clusters_live();
        }
        self.resolve_physics();
    }

    /// Release everything a producer published; its leaves read as authored.
    pub fn unpublish(&mut self, producer: ProducerId) {
        self.publish(producer, Default::default());
    }

    /// Every runtime signal currently published, by leaf.
    pub fn signals(&self) -> &std::collections::BTreeMap<Leaf, Signal> {
        self.reactive.signals()
    }

    /// Leaves a producer asked for while another held them, or while bound.
    pub fn signal_errors(&self) -> &std::collections::BTreeMap<Leaf, String> {
        self.reactive.signal_errors()
    }

    /// Freeze a producer's signals into the document: one transaction that
    /// sets every leaf it holds to the published value as a constant, after
    /// which the producer holds nothing (the host stops it or it republishes,
    /// and takes the leaves back). `None` when it holds nothing.
    pub fn freeze(&mut self, producer: ProducerId, label: String, actor: Actor, ts: u64) -> Option<Result<Applied, Vec<ApplyError>>> {
        let held = self.reactive.published_by(producer);
        if held.is_empty() {
            return None;
        }
        let ops: Vec<Op> = held.into_iter().map(|(leaf, value)| Op::Set { id: leaf.node, path: leaf.path, slot: Slot::Const(value) }).collect();
        self.unpublish(producer);
        Some(self.apply(TransactionInput { label, actor, ts, idempotency_key: None, ops, exact: false }))
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

    /// The node's local 3D space to its composition's (`geom::world_affine3`).
    pub fn world_affine3(&self, id: NodeId) -> geom::Affine3 {
        geom::world_affine3(&self.doc, &|l| self.reactive.read(&self.doc, l), id)
    }

    /// The node's own transform in its parent's space, from resolved values.
    pub fn local_affine(&self, id: NodeId) -> Affine2 {
        geom::local_affine(&self.doc, &|l| self.reactive.read(&self.doc, l), id)
    }

    /// A node's extent in its parent's space: its own bounds and its
    /// children's, through its transform.
    pub fn extent(&self, id: NodeId) -> Option<kurbo::Rect> {
        let a = self.local_affine(id).0;
        let to_parent = kurbo::Affine::new([a[0], a[1], a[2], a[3], a[4], a[5]]);
        let own = self.local_bounds(id).map(|b| b.geometry);
        let kids = self.doc.children(id).iter().filter(|c| !c.is_expanded() || id.is_expanded() || self.doc.live(id).is_some_and(|n| n.ty.name() == "use")).filter_map(|&c| self.extent(c)).reduce(|x, y| x.union(y));
        let r = match (own, kids) {
            (Some(a), Some(b)) => a.union(b),
            (a, b) => a.or(b)?,
        };
        Some(to_parent.transform_rect_bbox(r))
    }

    /// A drawable node's extent in its own space (`geom::local_bounds`).
    pub fn local_bounds(&self, id: NodeId) -> Option<geom::LocalBounds> {
        geom::local_bounds(self, id)
    }

    /// Per-node placement and local revisions and the structure revision,
    /// as of the last `take_geometry_changes`.
    pub fn revisions(&self) -> &geom::Revisions {
        &self.geometry
    }

    /// Everything that moved spatially since the last take, classified
    /// (`geom::GeometryChanges`), with the revisions bumped to match. One
    /// consumer owns this: the spatial scene. It is independent of
    /// `drain_changes`, which the reconciler owns.
    pub fn take_geometry_changes(&mut self) -> geom::GeometryChanges {
        let touched = self.reactive.take_touched();
        self.geometry.take(&self.doc, touched)
    }

    /// Re-measure every text node if the fonts changed since the last
    /// call (a family arrived): their bounds and glyphs move with no
    /// document change, so they are reported as changed here. Cheap when
    /// nothing changed; a host calls it once a frame.
    pub fn refresh_text(&mut self) -> bool {
        let generation = iron_text::engine().generation();
        if generation == self.text_generation {
            return false;
        }
        self.text_generation = generation;
        let text = TypeId::lookup("text").expect("registered");
        let path: LeafPath = "text.content".parse().expect("declared leaf");
        let ids: Vec<NodeId> = self.doc.nodes_of_type(text).filter(|&n| self.doc.is_live(n)).collect();
        for id in &ids {
            let leaf = Leaf { node: *id, path };
            let value = self.reactive.read(&self.doc, leaf).unwrap_or(Value::Str(String::new()));
            self.reactive.mark(leaf, value);
        }
        !ids.is_empty()
    }

    /// Leaves whose resolved value changed since the last drain.
    pub fn drain_changes(&mut self) -> Vec<ResolvedChange> {
        self.reactive.drain_changes()
    }

    /// Preview a leaf's value for the duration of a gesture. Bindings that
    /// read it re-evaluate now; nothing is written to the document until
    /// [`Store::commit_gesture`].
    pub fn preview(&mut self, id: NodeId, path: LeafPath, value: Value) {
        self.watch(id);
        let leaf = Leaf { node: id, path };
        self.drivers.insert(leaf);
        self.reactive.preview(&self.doc, leaf, value);
        if !self.clusters.list.is_empty() {
            self.resolve_clusters_live();
        }
        self.resolve_physics();
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

    // ---- Gesture ownership (`gesture`) ---------------------------------------

    /// Start a gesture on `nodes`, the ones whose pose it reads. A gesture
    /// already running is cancelled: there is one pointer. Previews and drags
    /// without a gesture start one implicitly.
    pub fn begin_gesture(&mut self, nodes: &[NodeId]) -> GestureId {
        if self.gesture.is_some() {
            self.end_gesture(GestureStatus::Cancelled("a newer gesture began".into()));
        }
        let id = GestureId(self.next_gesture);
        self.next_gesture += 1;
        self.gesture = Some(gesture::ActiveGesture { id, nodes: nodes.iter().copied().collect() });
        id
    }

    /// The gesture running now, if any.
    pub fn active_gesture(&self) -> Option<GestureId> {
        self.gesture.as_ref().map(|g| g.id)
    }

    /// Whether `id` is still running, and if not, how it ended.
    pub fn gesture_status(&self, id: GestureId) -> GestureStatus {
        match (&self.gesture, &self.ended) {
            (Some(g), _) if g.id == id => GestureStatus::Active,
            (_, Some((e, status))) if *e == id => status.clone(),
            _ => GestureStatus::Unknown,
        }
    }

    /// Add `id` to the running gesture's watched nodes, starting one if
    /// none runs.
    fn watch(&mut self, id: NodeId) {
        match &mut self.gesture {
            Some(g) => {
                g.nodes.insert(id);
            }
            None => {
                self.begin_gesture(&[id]);
            }
        }
    }

    /// Drop the running gesture's previews and record how it ended.
    fn end_gesture(&mut self, status: GestureStatus) {
        self.drivers.clear();
        self.fingers.clear();
        self.reactive.clear_overlay(&self.doc);
        if let Some(g) = self.gesture.take() {
            self.ended = Some((g.id, status));
        }
    }

    /// Set the running gesture's previews aside, so another transaction
    /// applies and solves against the document alone.
    fn suspend_gesture(&mut self) -> Option<gesture::Suspended> {
        self.gesture.as_ref()?;
        let values = self.reactive.overlay().iter().filter(|(l, _)| self.drivers.contains(l)).map(|(l, v)| (*l, v.clone())).collect();
        let suspended = gesture::Suspended { values, drivers: std::mem::take(&mut self.drivers), fingers: std::mem::take(&mut self.fingers) };
        self.reactive.clear_overlay(&self.doc);
        Some(suspended)
    }

    /// Put suspended previews back on the document as it now is.
    fn resume_gesture(&mut self, s: gesture::Suspended) {
        self.drivers = s.drivers;
        self.fingers = s.fingers;
        for (leaf, v) in s.values {
            if self.doc.leaf(leaf.node, leaf.path).is_some() {
                self.reactive.preview(&self.doc, leaf, v);
            }
        }
        if !self.clusters.list.is_empty() {
            self.resolve_clusters_live();
        }
        self.resolve_physics();
    }

    /// Take back transaction `version` if it is the latest: undo it and drop
    /// it from redo, as if it had never been applied. For a gesture that
    /// made something and then thought better of it (releasing Alt during an
    /// Alt-drag removes the copies). `false` when it is not the latest.
    pub fn retract(&mut self, version: Version, ts: u64) -> bool {
        if self.history.past.last().is_none_or(|t| t.version != version) {
            return false;
        }
        match self.undo(ts) {
            Some(Ok(_)) => {
                self.history.future.pop();
                true
            }
            _ => false,
        }
    }

    /// Fold the latest transaction into the one before it, when that one is
    /// `previous`: one history entry, labelled `label`, undone in one step.
    /// An Alt-drag's duplicate and its move are one edit. The log keeps both.
    pub fn merge_with_previous(&mut self, previous: Version, label: impl Into<String>) -> bool {
        let n = self.history.past.len();
        if n < 2 || self.history.past[n - 2].version != previous {
            return false;
        }
        let b = self.history.past.pop().expect("two entries");
        let a = self.history.past.pop().expect("two entries");
        let mut ops = a.ops;
        ops.extend(a.derived_ops);
        ops.extend(b.ops);
        let mut inverse = a.inverse;
        inverse.extend(b.inverse);
        self.history.past.push(Transaction { version: b.version, label: label.into(), actor: b.actor, ts: b.ts, idempotency_key: None, ops, derived_ops: b.derived_ops, inverse, expanded: b.expanded, solver: b.solver });
        true
    }

    /// Set a leaf to `value` through its handle, as one transaction: a
    /// constant directly, a linearly bound leaf through its source. For
    /// one-off edits (an inspector field); a running gesture is unaffected
    /// unless the edit conflicts with it. `Ok(None)` when nothing changes.
    pub fn set_through_handle(&mut self, id: NodeId, path: LeafPath, value: Value, label: impl Into<String>, actor: Actor, ts: u64) -> Result<Option<Applied>, String> {
        let (target, v) = handle::resolve_target(&self.doc, &self.reactive, Leaf { node: id, path }, value)?;
        let same = match (self.doc.leaf(target.node, target.path).and_then(|s| s.constant()), &v) {
            (Some(Value::Number(a)), Value::Number(b)) => (a - b).abs() < 1e-9,
            (c, v) => c == Some(v),
        };
        if same {
            return Ok(None);
        }
        let input = TransactionInput { label: label.into(), actor, ts, idempotency_key: None, ops: vec![Op::Set { id: target.node, path: target.path, slot: Slot::Const(v) }], exact: false };
        self.apply(input).map(Some).map_err(|errors| errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))
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
        // The gesture's own commit: taken first, so `apply` neither suspends
        // its previews nor finds it in conflict with itself.
        let gesture = self.gesture.take();
        if let Some(g) = &gesture {
            self.ended = Some((g.id, GestureStatus::Committed));
        }
        let mut ops = Vec::new();
        // Leaves of facets the node lacks but allows: the facet is
        // added, defaults with the previewed fields set, before the `Set`s.
        let mut added: BTreeMap<(NodeId, FacetKind), Facet> = BTreeMap::new();
        for (leaf, value) in self.reactive.overlay() {
            if !self.drivers.contains(leaf) {
                continue;
            }
            let Some(slot) = self.doc.leaf(leaf.node, leaf.path) else {
                let allowed = self.doc.live(leaf.node).is_some_and(|n| n.ty.spec().allows(leaf.path.facet));
                if allowed && self.doc.facet(leaf.node, leaf.path.facet).is_none() {
                    let c = added.entry((leaf.node, leaf.path.facet)).or_insert_with(|| leaf.path.facet.default_facet());
                    let _ = c.set(leaf.path.field, Slot::Const(value.clone()));
                }
                continue;
            };
            if slot.expr().is_some() {
                continue;
            }
            let same = match (slot.constant(), value) {
                (Some(Value::Number(a)), Value::Number(b)) => (a - b).abs() < 1e-9,
                (c, v) => c == Some(v),
            };
            if !same {
                ops.push(Op::Set {
                    id: leaf.node,
                    path: leaf.path,
                    slot: Slot::Const(value.clone()),
                });
            }
        }
        let adds = added.into_iter().map(|((id, _), comp)| Op::AddFacet { id, facet: comp });
        ops = adds.chain(ops).collect();
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

    /// Drop the gesture's previews without a transaction (Esc).
    pub fn cancel_gesture(&mut self) {
        self.end_gesture(GestureStatus::Cancelled("cancelled".into()));
    }

    // ---- The session tier (plans/specs/panels-and-session.md) ----------------

    /// Apply session-tier ops: nodes, relations and leaves in the session's
    /// id range only. Not a transaction: the document's version, history and
    /// undo never see it, and it is never saved. All or nothing: an error
    /// rolls back the ops already applied.
    pub fn apply_session(&mut self, ops: Vec<Op>) -> Result<(), ApplyError> {
        let stamp = self.doc.version();
        let mut undo: Vec<Op> = Vec::new();
        let mut structural = false;
        for (i, op) in ops.iter().enumerate() {
            let in_session = match op {
                Op::Create { id, parent, .. } => id.is_session() && parent.is_none_or(|p| p.is_session()),
                Op::Delete { id } | Op::Reparent { id, .. } | Op::Reorder { id, .. } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => id.is_session(),
                Op::Link { rel } => rel.id.is_session() && rel.from.is_session(),
                Op::Unlink { id } => id.is_session(),
            };
            let result = if in_session {
                op::apply_op(&mut self.doc, i, op, stamp)
            } else {
                Err(ApplyError { op: i, kind: ErrorKind::BadId, message: "not a session-tier op".into() })
            };
            match result {
                Ok(inverse) => undo.extend(inverse),
                Err(e) => {
                    for inv in undo.iter().rev() {
                        let _ = op::apply_op(&mut self.doc, i, inv, stamp);
                    }
                    return Err(e);
                }
            }
            structural |= matches!(op, Op::Create { .. } | Op::Delete { .. });
        }
        if structural {
            self.reactive.rebuild(&self.doc);
        }
        self.session_version += 1;
        Ok(())
    }

    /// Solve the across/through network (ECM §6.1, step 3) and put its
    /// solution in the graph's physics layer: derived, never stored, read by
    /// bindings like any value. Run after every change. A network that does
    /// not converge keeps its previous solution and says why.
    pub fn resolve_physics(&mut self) {
        let net = crate::port::network(&self.doc);
        if net.unknowns.is_empty() {
            if !self.reactive.published_by(ProducerId::NETWORK).is_empty() {
                self.reactive.retract(&self.doc, ProducerId::NETWORK);
            }
            self.physics_error = None;
            return;
        }
        let magnitude = |v: &Value| match v {
            Value::Number(n) => Some(*n),
            Value::Quantity { si, .. } => Some(*si),
            _ => None,
        };
        let index: std::collections::BTreeMap<Leaf, usize> = net.unknowns.iter().enumerate().map(|(i, (l, _))| (*l, i)).collect();
        let x0: Vec<f64> = net.unknowns.iter().map(|(l, _)| self.reactive.signal(*l).map(|s| &s.value).and_then(magnitude).unwrap_or(0.0)).collect();
        let solution = {
            let doc = &self.doc;
            let reactive = &self.reactive;
            let unknowns = &net.unknowns;
            let mut problem = iron_solve::Problem::uniform(x0, 1.0);
            for eq in &net.equations {
                match eq {
                    crate::port::Equation::Linear(coefs) => {
                        let vars: Vec<usize> = coefs.iter().map(|(i, _)| *i).collect();
                        let k: Vec<f64> = coefs.iter().map(|(_, c)| *c).collect();
                        problem.term(iron_solve::Term::new(iron_solve::Tier::Required, vars, 1, move |v, out| {
                            out[0] = v.iter().zip(&k).map(|(x, c)| x * c).sum();
                        }));
                    }
                    crate::port::Equation::Expr { lhs, rhs, .. } => {
                        let mut vars: Vec<usize> = lhs.deps().into_iter().chain(rhs.deps()).filter_map(|l| index.get(&l).copied()).collect();
                        vars.sort();
                        vars.dedup();
                        if vars.is_empty() {
                            continue;
                        }
                        let (lhs, rhs, index) = (lhs.clone(), rhs.clone(), index.clone());
                        problem.term(iron_solve::Term::new(iron_solve::Tier::Required, vars.clone(), 1, move |v, out| {
                            let read = |l: Leaf| match index.get(&l).and_then(|i| vars.iter().position(|j| j == i)) {
                                Some(pos) => Some(Value::quantity(v[pos], unknowns[vars[pos]].1)),
                                None => reactive.read(doc, l),
                            };
                            let side = |e: &Expr| e.eval(&read).ok().as_ref().and_then(magnitude);
                            out[0] = match (side(&lhs), side(&rhs)) {
                                (Some(a), Some(b)) => a - b,
                                _ => 1e6,
                            };
                        }));
                    }
                }
            }
            let options = iron_solve::Options { max_iterations: 100, ..Default::default() };
            iron_solve::solve(&problem, &options)
        };
        if solution.status != iron_solve::Status::Converged {
            self.physics_error = Some(format!("the across/through network does not settle (residual {:.3e})", solution.max_required_residual));
            return;
        }
        self.physics_error = None;
        let values = net.unknowns.iter().zip(&solution.vars).map(|((l, dim), x)| (*l, Value::quantity(*x, *dim))).collect();
        self.signal_stamp += 1;
        self.reactive.publish(&self.doc, ProducerId::NETWORK, values, self.signal_stamp);
    }

    /// Why the across/through network kept its last solution, if it did.
    /// The transaction that makes a component named `name` from the
    /// selected pictures (component-model.md §5.5, `make`).
    pub fn make_component(&self, selection: &[NodeId], name: &str, actor: Actor) -> Result<(make::Made, TransactionInput), String> {
        let read = |l: Leaf| self.resolved(l.node, l.path);
        let shapes = selection.iter().all(|&n| self.doc.live(n).is_some_and(|x| x.ty.name() != "use"));
        let made = if shapes {
            // Plain shapes: they become a definition shown by its main.
            let mut top_left = [f64::INFINITY, f64::INFINITY];
            for &n in selection {
                if let Some(r) = self.extent(n) {
                    top_left = [top_left[0].min(r.x0), top_left[1].min(r.y0)];
                }
            }
            if !top_left[0].is_finite() {
                top_left = [0.0, 0.0];
            }
            make::make_main(&self.doc, selection, top_left, name)?
        } else {
            make::make(&self.doc, &read, selection, name)?
        };
        let tx = TransactionInput { label: format!("make component {name}"), actor, ts: 0, idempotency_key: None, ops: made.ops.clone(), exact: false };
        Ok((made, tx))
    }

    /// Overrides that apply nothing, and why (component-model.md §5.3).
    pub fn stale_overrides(&self) -> Vec<(NodeId, String)> {
        expand::stale_overrides(&self.doc)
    }

    /// What the last expansion could not derive, if anything.
    pub fn expansion_problems(&self) -> &[String] {
        &self.expansion_problems
    }

    pub fn physics_error(&self) -> Option<&str> {
        self.physics_error.as_deref()
    }

    pub fn session_version(&self) -> u64 {
        self.session_version
    }

    fn live_of_type(&self, name: &str) -> Option<NodeId> {
        let ty = TypeId::lookup(name)?;
        self.doc.live_nodes().find(|n| n.ty == ty).map(|n| n.id)
    }

    /// The selected nodes, in pick order, with how each was reached
    /// (`data.panel`, `data.chain`). Deleted nodes drop out.
    pub fn selection(&self) -> Vec<(NodeId, std::collections::BTreeMap<String, Value>)> {
        let Some(sel) = self.live_of_type("selection") else { return Vec::new() };
        self.doc
            .relations_from(sel)
            .filter(|r| r.rel == RelKind::Selects && self.doc.is_live(r.to))
            .map(|r| (r.to, r.data.clone()))
            .collect()
    }

    /// Replace the selection: one session change, nothing when it is
    /// already this. The session root and its `selection` node are created
    /// on first use.
    pub fn set_selection(&mut self, items: &[(NodeId, std::collections::BTreeMap<String, Value>)]) -> Result<(), ApplyError> {
        if self.selection() == items {
            return Ok(());
        }
        let mut ops = Vec::new();
        let sel = match self.live_of_type("selection") {
            Some(sel) => sel,
            None => {
                let ids = self.doc.next_session_ids(2);
                let ty = |n: &str| TypeId::lookup(n).expect("registered");
                ops.push(Op::Create { id: ids[0], ty: ty("session"), parent: None, order: OrderKey::first(), facets: vec![] });
                ops.push(Op::Create { id: ids[1], ty: ty("selection"), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![] });
                ids[1]
            }
        };
        for r in self.doc.relations_from(sel).filter(|r| r.rel == RelKind::Selects) {
            ops.push(Op::Unlink { id: r.id });
        }
        for ((to, data), id) in items.iter().zip(self.doc.next_session_relation_ids(items.len())) {
            ops.push(Op::Link { rel: Relation { id, from: sel, to: *to, rel: RelKind::Selects, data: data.clone() } });
        }
        self.apply_session(ops)
    }

    /// Keep the graph in step with a transaction that just applied.
    fn after_transaction(&mut self, tx: &Transaction) {
        self.geometry.note_ops(tx.ops.iter().chain(tx.derived_ops.iter()).chain(tx.inverse.iter()).chain(tx.expanded.iter()));
        let structural = tx.ops.iter().chain(tx.inverse.iter()).chain(tx.expanded.iter()).any(|op| match op {
            // A link can be a connection's end, which derives a signal.
            Op::Create { .. } | Op::Delete { .. } | Op::AddFacet { .. } | Op::RemoveFacet { .. } | Op::Link { .. } | Op::Unlink { .. } => true,
            Op::Set { slot, .. } => slot.expr().is_some(),
            _ => false,
        });
        if structural {
            self.reactive.rebuild(&self.doc);
            self.resample();
        } else {
            // What re-expansion wrote counts too: an override updated in
            // place changes its derived node's leaf, and whatever reads it.
            let changed: Vec<Leaf> = tx
                .ops
                .iter()
                .chain(tx.expanded.iter())
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
        self.resolve_physics();
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

    /// The transaction that links two pictures to one entity (ECM §7.2,
    /// `composition::link_ops`). Both the UI and the model surface ask the
    /// same question when the pictures already have different entities:
    /// `Err(LinkError::Question)` until `keep` names the survivor.
    pub fn link(&self, a: NodeId, b: NodeId, keep: Option<NodeId>, actor: Actor) -> Result<TransactionInput, composition::LinkError> {
        let ops = composition::link_ops(self.document(), a, b, keep)?;
        Ok(TransactionInput { label: "link".into(), actor, ts: 0, idempotency_key: None, ops, exact: false })
    }

    /// Apply a transaction atomically. Either every op applies and the
    /// version advances, or nothing applies and every error is reported.
    pub fn apply(&mut self, input: TransactionInput) -> Result<Applied, Vec<ApplyError>> {
        // A transaction from outside a running gesture is classified against
        // it (`gesture`) and applied without its previews.
        let conflict = self.gesture.as_ref().and_then(|g| gesture::conflict(&self.doc, &input.ops, g, &self.drivers));
        let suspended = self.suspend_gesture();
        let result = self.apply_now(input);
        if let Some(s) = suspended {
            let moved = match (&result, &self.gesture) {
                (Ok(a), Some(g)) if !a.replayed => self.history.past.last().and_then(|tx| gesture::moved(&tx.derived_ops, g)),
                _ => None,
            };
            match conflict.filter(|_| result.is_ok()).or(moved) {
                Some(reason) => self.end_gesture(GestureStatus::Cancelled(reason)),
                None => self.resume_gesture(s),
            }
        }
        result
    }

    fn apply_now(&mut self, input: TransactionInput) -> Result<Applied, Vec<ApplyError>> {
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
        if self.gesture.is_some() && self.history.can_undo() {
            self.end_gesture(GestureStatus::Cancelled("undo".into()));
        }
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
        if self.gesture.is_some() && self.history.can_redo() {
            self.end_gesture(GestureStatus::Cancelled("redo".into()));
        }
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
                if current.is_some_and(|c| (c - v).abs() < 1e-9) {
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
        // Edits aimed inside an instance become edits to what it derives from
        // (component-model.md §7): an argument, a slider, an override.
        let mut input = input;
        input.ops = expand::translate(&self.doc, &self.reactive, std::mem::take(&mut input.ops)).map_err(|e| vec![e])?;
        let mut work = self.doc.clone();
        let stamp = work.version().next();
        let mut inverse = Vec::new();
        let mut errors = Vec::new();
        for (i, op) in input.ops.iter().enumerate() {
            if let Some(reason) = expand::refuse(op) {
                errors.push(ApplyError { op: i, kind: ErrorKind::BadId, message: reason });
                continue;
            }
            match op::apply_op(&mut work, i, op, stamp) {
                Ok(inv) => inverse.extend(inv),
                Err(e) => errors.push(e),
            }
        }
        // Instances re-derive what their definitions say, before the checks
        // that read them (a connection to an expanded port).
        let mut expanded = Vec::new();
        if errors.is_empty() && expand::affected_by(&work, &input.ops) {
            let (ops, problems) = expand::expand(&mut work, stamp);
            expanded = ops;
            self.expansion_problems = problems;
        }
        if errors.is_empty() {
            // A connection is reported once, however many of its links the
            // transaction holds.
            let mut connections = std::collections::BTreeSet::new();
            for (i, op) in input.ops.iter().enumerate() {
                if let Op::Link { rel } = op
                    && rel.rel == RelKind::Connects
                    && !connections.insert(rel.from)
                {
                    continue;
                }
                if let Err(e) = op::check_created(&work, i, op, stamp) {
                    errors.push(e);
                }
                if let Err(message) = expand::check_scope(&work, op) {
                    errors.push(ApplyError { op: i, kind: ErrorKind::OutOfScope, message });
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        work.set_version(stamp);
        // Derived nodes and relations come back by re-expansion, never by
        // undo: a deleted instance's expanded children are not restored by
        // its inverse (the guard would refuse them anyway).
        inverse.retain(|op| match op {
            Op::Create { id, .. } | Op::Delete { id } | Op::Reparent { id, .. } | Op::Reorder { id, .. } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => !id.is_expanded(),
            Op::Link { rel } => !rel.id.is_expanded(),
            Op::Unlink { id } => !id.is_expanded(),
        });
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
                expanded,
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
