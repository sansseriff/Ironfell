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
pub mod analysis;
pub mod composition;
pub mod compose;
pub mod constraint;
pub mod contexts;
pub mod contour;
pub mod copy;
pub mod definition;
pub mod document;
pub mod expand;
pub mod machine;
pub mod markup;
pub mod make;
pub mod expr;
pub mod geom;
pub mod geometry;
pub mod interaction;
pub mod gesture;
pub mod anchor;
pub mod author;
pub mod attachment;
pub mod appearance;
pub mod graph;
pub mod handle;
pub mod id;
pub mod op;
pub mod order;
pub mod parts;
pub mod port;
pub mod registry;
pub mod serial;
pub mod series;
pub mod shape;
pub mod plug;
pub mod solve;
pub mod standard;
pub mod sym;
pub mod text;
pub mod layers;
pub mod layout;
pub mod list;
pub mod narrative;
pub mod table;
pub mod time;
pub mod palette;
pub mod types;
pub mod timeline_view;
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
pub use id::{NodeId, RelationId, ScopeId, Version};
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
    /// Each table's grid from the last layout (`layout.rs`, `table.rs`).
    tables: std::collections::BTreeMap<NodeId, table::Grid>,
    /// The built-in contexts with this document's `context` nodes over
    /// them, and the markups they were made from; `None` while it has none.
    contexts: Option<(Vec<(NodeId, String)>, contexts::Library)>,
    /// A reorder being dragged (affordances.md §6): its container laid out
    /// in a previewed order, the dragged item where the pointer holds it.
    /// Session state, never saved; a release commits one `Reorder`.
    layout_preview: Option<layout::Preview>,
    /// The theme colour tokens resolve for (`set_theme`; palette.rs).
    dark: bool,
    /// Declared transitions in flight (affordances.md §8.2), and what layout
    /// last asked for, which they ease towards. Off until a host with a
    /// clock turns it on (`set_motion`): a document read without one sees
    /// where things are, never part-way.
    motion: Motion,
    /// Component state machines' player (machine.rs): presentation state.
    machines: machine::Player,
    /// Nodes' interaction states, as the host last reported them (session;
    /// read by rules that match `state:`).
    interaction: BTreeMap<NodeId, Vec<&'static str>>,
    /// What the last expansion could not derive (component-model.md §3).
    expansion_problems: Vec<expand::Problem>,
    /// The items each drawn repeat and conditional has, by key
    /// (plans/specs/repeat.md), as last settled from its inputs.
    repeats: expand::Items,
    /// The drawn repeats and conditionals, and each one's `each`, `count`,
    /// `key` and `when.is` as last settled: what `settle_repeats` compares
    /// before doing any work. `None` after structure changed, until the
    /// next settle.
    repeat_inputs: Option<Vec<(NodeId, [Option<Value>; 4])>>,
    /// Session state cells (`let.keep = session`) that differ from their
    /// initial value (reactivity.md §5.1): published over the constant,
    /// never saved, dropped with their node.
    session_state: BTreeMap<Leaf, Value>,
    /// Events actions emitted for the host (`take_emitted`).
    emitted: Vec<(NodeId, String)>,
    /// Repeats that asked for more items than `repeat.max` allows.
    truncated: Vec<expand::Problem>,
    /// Derived structure made outside a transaction (a repeat that grew
    /// during a drag, or when a producer published), for the host to
    /// materialise (`take_derived_ops`). Never history.
    derived_ops: Vec<Op>,
    settling: bool,
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

/// Declared transitions (affordances.md §8.2): the one interpolator.
#[derive(Default)]
struct Motion {
    enabled: bool,
    /// The host's clock, seconds, as of its last frame.
    clock: f64,
    anims: BTreeMap<NodeId, Anim>,
    /// What layout last asked for: where the transitions are heading.
    targets: BTreeMap<Leaf, Value>,
}

struct Anim {
    from: [f64; 2],
    to: [f64; 2],
    t0: f64,
    ms: f64,
    ease: &'static str,
}

impl Motion {
    /// Put each node in flight where its transition has got to; drop the
    /// finished ones (they rest where layout put them).
    fn apply(&mut self, values: &mut BTreeMap<Leaf, Value>) {
        let (xp, yp): (LeafPath, LeafPath) = ("transform2d.x".parse().expect("declared leaf"), "transform2d.y".parse().expect("declared leaf"));
        let clock = self.clock;
        self.anims.retain(|&node, a| {
            let u = ((clock - a.t0) * 1000.0 / a.ms).clamp(0.0, 1.0);
            if u >= 1.0 {
                return false;
            }
            let name = match a.ease {
                "in" => "ease_in",
                "out" => "ease_out",
                "in_out" => "ease_in_out",
                _ => "linear",
            };
            let e = time::ease(name, u);
            values.insert(Leaf { node, path: xp }, Value::Number(a.from[0] + (a.to[0] - a.from[0]) * e));
            values.insert(Leaf { node, path: yp }, Value::Number(a.from[1] + (a.to[1] - a.from[1]) * e));
            true
        });
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
            tables: Default::default(),
            contexts: None,
            layout_preview: None,
            dark: false,
            motion: Motion::default(),
            machines: machine::Player::default(),
            interaction: BTreeMap::new(),
            expansion_problems: Vec::new(),
            repeats: BTreeMap::new(),
            repeat_inputs: None,
            session_state: BTreeMap::new(),
            emitted: Vec::new(),
            truncated: Vec::new(),
            derived_ops: Vec::new(),
            settling: false,
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
        store.expansion_problems = expand::expand(&mut store.doc, stamp, &BTreeMap::new()).1;
        store.geometry.note_structure();
        store.reactive.rebuild(&store.doc);
        store.resample();
        store.settle_repeats();
        store.derived_ops.clear();
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
        if let Ok(c) = solve::clusters(&self.doc, &self.reactive, self.contexts()) {
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

    /// The playhead: the session clock's `t` as read (a gesture's preview
    /// of it counts), timeline.md §4.1.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Move the playhead: a session op on the clock's `t` (the clock is
    /// created when missing), so it is never saved or undone. Every clip is
    /// resampled; leaves whose animated value moved are reported like any
    /// other resolved change.
    pub fn set_time(&mut self, t: f64) {
        // Taken first, so the op's own sync finds nothing to do and the
        // resample below runs once.
        self.time = t;
        if let Ok(c) = self.ensure_clock() {
            let _ = self.apply_session(vec![Op::Set { id: c, path: clock_path("t"), slot: Slot::Const(Value::Number(t)) }]);
        }
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

    /// The session clock every timeline view shares (timeline.md §4.1).
    pub fn clock(&self) -> Option<NodeId> {
        self.live_of_type("clock")
    }

    /// The session clock, created (with the session root, if that is
    /// missing too) at the current time when there is none.
    pub fn ensure_clock(&mut self) -> Result<NodeId, ApplyError> {
        if let Some(c) = self.clock() {
            return Ok(c);
        }
        let ty = |n: &str| TypeId::lookup(n).expect("registered");
        let mut ops = Vec::new();
        let root = match self.live_of_type("session") {
            Some(r) => r,
            None => {
                let r = self.doc.next_session_ids(1)[0];
                ops.push(Op::Create { id: r, ty: ty("session"), parent: None, order: OrderKey::first(), facets: vec![] });
                r
            }
        };
        let id = *self.doc.next_session_ids(ops.len() + 1).last().expect("one id");
        let mut clock = FacetKind::Clock.default_facet();
        let _ = clock.set("t", Slot::Const(Value::Number(self.time)));
        let last = self.doc.live_nodes().filter(|n| n.parent == Some(root)).map(|n| n.order.clone()).max();
        let order = last.map_or_else(OrderKey::first, |o| OrderKey::after(&o));
        ops.push(Op::Create { id, ty: ty("clock"), parent: Some(root), order, facets: vec![clock] });
        self.apply_session(ops)?;
        Ok(id)
    }

    /// Follow the clock: when its `t` as read moved from the playhead the
    /// clips were last sampled at, resample. True when it did; callers
    /// re-solve.
    fn sync_clock(&mut self) -> bool {
        let Some(c) = self.clock() else { return false };
        let leaf = Leaf { node: c, path: clock_path("t") };
        let Some(Value::Number(t)) = self.reactive.read(&self.doc, leaf) else { return false };
        if t == self.time || !t.is_finite() {
            return false;
        }
        self.time = t;
        self.resample();
        true
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
            ProducerId::LAYOUT => "layout".to_owned(),
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
        // New faces measure differently: layouts holding text reflow.
        self.resolve_layout();
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
        self.sync_clock();
        if !self.clusters.list.is_empty() {
            self.resolve_clusters_live();
        }
        self.resolve_physics();
    }

    /// Withdraw one leaf's preview (it reads as the document again).
    pub fn unpreview(&mut self, id: NodeId, path: LeafPath) {
        let leaf = Leaf { node: id, path };
        self.drivers.remove(&leaf);
        self.reactive.unpreview(&self.doc, leaf);
        if self.sync_clock() && !self.clusters.list.is_empty() {
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
        // A previewed playhead goes back to the clock's own `t`.
        if self.sync_clock() {
            if !self.clusters.list.is_empty() {
                self.resolve_clusters_live();
            }
            self.resolve_physics();
        }
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
        // Animated leaves (and with auto-keyframe on, any leaf) write keys,
        // by the visible mode (timeline.md §12.2), not their authored value.
        let driven: Vec<(Leaf, Value)> = self
            .reactive
            .overlay()
            .iter()
            .filter(|(l, _)| self.drivers.contains(l) && self.doc.leaf(l.node, l.path).is_some_and(|s| s.expr().is_none()))
            .map(|(l, v)| (*l, v.clone()))
            .collect();
        let (key_ops, keyed) = timeline_view::key_ops(self, &driven);
        ops.extend(key_ops);
        // Leaves of facets the node lacks but allows: the facet is
        // added, defaults with the previewed fields set, before the `Set`s.
        let mut added: BTreeMap<(NodeId, FacetKind), Facet> = BTreeMap::new();
        for (leaf, value) in self.reactive.overlay() {
            if !self.drivers.contains(leaf) || keyed.contains(leaf) {
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
        // Leaves of session nodes (the clock's `t`, a timeline's axis) are
        // session ops: never in history (timeline.md §4.1).
        let (session, ops): (Vec<Op>, Vec<Op>) = ops.into_iter().partition(|op| matches!(op, Op::Set { id, .. } | Op::AddFacet { id, .. } if id.is_session()));
        if !session.is_empty() {
            let _ = self.apply_session(session);
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
            structural |= match op {
                Op::Create { .. } | Op::Delete { .. } | Op::AddFacet { .. } | Op::RemoveFacet { .. } | Op::Reparent { .. } => true,
                Op::Set { id, path, slot } => slot.expr().is_some() || self.reactive.is_bound(Leaf { node: *id, path: *path }),
                _ => false,
            };
        }
        // The spatial scene learns of session nodes as of any other: a
        // session composition built after the last transaction (the Latency
        // view's) is otherwise never indexed, so presses miss it.
        self.geometry.note_ops(ops.iter());
        if structural {
            self.repeat_inputs = None;
            self.reactive.rebuild(&self.doc);
        } else {
            let changed: Vec<Leaf> = ops.iter().filter_map(|op| match op { Op::Set { id, path, .. } => Some(Leaf { node: *id, path: *path }), _ => None }).collect();
            if !changed.is_empty() {
                // Report each written leaf as changed, as a preview does, so
                // the reconciler and the spatial scene see a session node
                // move even when nothing binds to it (the Latency view's
                // square, recentred by `engine_api::document_latency`).
                for &leaf in &changed {
                    if let Some(v) = self.reactive.read(&self.doc, leaf) {
                        self.reactive.mark(leaf, v);
                    }
                }
                self.reactive.invalidate(&self.doc, changed);
            }
        }
        self.session_version += 1;
        if self.sync_clock() {
            if !self.clusters.list.is_empty() {
                self.resolve_clusters_live();
            }
            self.resolve_physics();
        }
        Ok(())
    }

    /// Bring every drawn repeat's and conditional's items to what its
    /// inputs say now (reactivity.md §3.2, §3.3). A repeat has an item per
    /// element of `each` (a series or a list), else `count` items, at most
    /// `max` (and 10,000); a conditional has one item while `when.is` is
    /// true. Each item's key (its place in ids) comes from `repeat.key`, or
    /// by default: a node reference, a record's `id`, text when all texts
    /// differ, else the index. Each item's value and index are published
    /// on its node. Runs whenever values may have moved: after a
    /// transaction, a publish, a preview, the clock. Where the items
    /// changed it re-expands and rebuilds the graph; a repeat inside new
    /// items settles in the next round. Returns the structural ops applied.
    fn settle_repeats(&mut self) -> Vec<Op> {
        if self.settling {
            return Vec::new();
        }
        self.settling = true;
        let mut applied = Vec::new();
        let path = |f: &str| LeafPath::new(facet::FacetKind::Repeat, f).expect("declared leaf");
        let item = |f: &str| LeafPath::new(facet::FacetKind::Item, f).expect("declared leaf");
        let when = LeafPath::new(facet::FacetKind::When, "is").expect("declared leaf");
        // A round per level of nesting, and one more to see nothing change.
        for round in 0..=expand::DEPTH_MAX + 1 {
            let repeats: Vec<NodeId> = match &self.repeat_inputs {
                Some(known) => known.iter().map(|k| k.0).collect(),
                None => expand::drawn_repeats(&self.doc),
            };
            let inputs: Vec<(NodeId, [Option<Value>; 4])> = repeats
                .iter()
                .map(|&r| (r, [path("each"), path("count"), path("key"), when].map(|p| self.reactive.read(&self.doc, Leaf { node: r, path: p }))))
                .collect();
            // NaN is a value here ("no series"), so compare by text too.
            if self.repeat_inputs.as_ref().is_some_and(|known| known.len() == inputs.len() && known.iter().zip(&inputs).all(|(a, b)| a == b || format!("{a:?}") == format!("{b:?}"))) {
                break;
            }
            let mut counts = expand::Items::new();
            let mut values = BTreeMap::new();
            let mut truncated = Vec::new();
            for (r, [each, count, key, shown]) in &inputs {
                let r = *r;
                let is_repeat = self.doc.facet(r, facet::FacetKind::Repeat).is_some();
                // A conditional that fails to evaluate shows nothing.
                let shown = self.doc.facet(r, facet::FacetKind::When).is_none() || (shown.as_ref().and_then(|v| v.as_bool()) == Some(true) && !self.reactive.errors.contains_key(&Leaf { node: r, path: when }));
                if !is_repeat {
                    if shown && let Some(scope) = NodeId::item_scope(r, "on") {
                        counts.insert(r, vec!["on".to_owned()]);
                        values.insert(Leaf { node: scope, path: item("value") }, Value::Bool(true));
                        values.insert(Leaf { node: scope, path: item("index") }, Value::Number(0.0));
                    } else {
                        counts.insert(r, Vec::new());
                    }
                    continue;
                }
                let max = self.reactive.read(&self.doc, Leaf { node: r, path: path("max") }).and_then(|v| v.as_f64()).filter(|m| m.is_finite()).unwrap_or(1000.0).clamp(0.0, 10_000.0) as usize;
                let listed = each.as_ref().and_then(|v| v.len());
                let asked = listed.unwrap_or_else(|| count.as_ref().and_then(|v| v.as_f64()).filter(|n| n.is_finite()).map_or(0, |n| n.floor().max(0.0) as usize));
                let n = asked.min(max);
                if asked > max {
                    truncated.push(expand::Problem { node: r, kind: "truncated", message: format!("{asked} items asked for; repeat.max allows {max}") });
                }
                let mut elements: Vec<Value> = match (listed, each) {
                    (Some(_), Some(each)) => each.elements().unwrap_or_default(),
                    _ => (0..n).map(|k| Value::Number(k as f64)).collect(),
                };
                elements.truncate(n);
                // The template reads the first item's.
                if let Some(first) = elements.first() {
                    values.insert(Leaf { node: r, path: path("item") }, first.clone());
                }
                let keys = match item_keys(&elements, listed.is_some(), key.as_ref()) {
                    Ok(keys) => keys,
                    Err(why) => {
                        truncated.push(expand::Problem { node: r, kind: "keys", message: format!("{why}; items are keyed by index instead") });
                        (0..elements.len()).map(|k| k.to_string()).collect()
                    }
                };
                if shown {
                    for (k, (key, element)) in keys.iter().zip(elements).enumerate() {
                        let Some(scope) = NodeId::item_scope(r, key) else { continue };
                        values.insert(Leaf { node: scope, path: item("value") }, element);
                        values.insert(Leaf { node: scope, path: item("index") }, Value::Number(k as f64));
                    }
                    counts.insert(r, keys);
                } else {
                    counts.insert(r, Vec::new());
                }
            }
            self.truncated = truncated;
            self.repeat_inputs = Some(inputs);
            let structural = counts != self.repeats;
            if structural {
                self.repeats = counts;
                let stamp = self.doc.version();
                let (ops, problems) = expand::expand(&mut self.doc, stamp, &self.repeats);
                self.expansion_problems = problems;
                if round == expand::DEPTH_MAX + 1 && !ops.is_empty() {
                    self.expansion_problems.push(expand::Problem { node: repeats[0], kind: "depth_blocked", message: "repeats did not settle: their counts keep changing each other".into() });
                }
                self.geometry.note_ops(ops.iter());
                applied.extend(ops);
            }
            if values != self.reactive.published_by(ProducerId::ITEMS) {
                self.signal_stamp += 1;
                self.reactive.publish(&self.doc, ProducerId::ITEMS, values, self.signal_stamp);
            }
            if structural {
                // New items may hold repeats: look again.
                self.repeat_inputs = None;
                self.reactive.rebuild(&self.doc);
                self.resample();
                self.publish_state();
            }
        }
        self.settling = false;
        applied
    }

    /// Publish the session state cells (`let.keep = session`) that still
    /// have a node; one whose scope went is forgotten, so it starts from
    /// its initial value if the scope returns (reactivity.md §5.1).
    fn publish_state(&mut self) {
        if self.session_state.is_empty() && self.reactive.published_by(ProducerId::STATE).is_empty() {
            return;
        }
        let doc = &self.doc;
        self.session_state.retain(|leaf, _| doc.is_live(leaf.node));
        if self.session_state != self.reactive.published_by(ProducerId::STATE) {
            self.signal_stamp += 1;
            self.reactive.publish(&self.doc, ProducerId::STATE, self.session_state.clone(), self.signal_stamp);
        }
    }

    /// Publish what is selected onto every node with a `selection` facet.
    fn publish_selection(&mut self) {
        let nodes: Vec<NodeId> = self.doc.nodes_with(facet::FacetKind::Selection).filter(|&n| self.doc.is_live(n)).collect();
        if nodes.is_empty() && self.reactive.published_by(ProducerId::SELECTION).is_empty() {
            return;
        }
        let picked: Vec<NodeId> = self.selection().into_iter().map(|(n, _)| n).collect();
        let leaf = |f: &str| LeafPath::new(facet::FacetKind::Selection, f).expect("declared leaf");
        let mut values = BTreeMap::new();
        for n in nodes {
            values.insert(Leaf { node: n, path: leaf("node") }, Value::Ref(picked.first().copied().unwrap_or(NodeId::from_raw(0))));
            values.insert(Leaf { node: n, path: leaf("nodes") }, Value::list(picked.iter().map(|&p| Value::Ref(p)).collect::<Vec<_>>()));
            values.insert(Leaf { node: n, path: leaf("count") }, Value::Number(picked.len() as f64));
        }
        if values != self.reactive.published_by(ProducerId::SELECTION) {
            self.signal_stamp += 1;
            self.reactive.publish(&self.doc, ProducerId::SELECTION, values, self.signal_stamp);
        }
    }

    /// Whether `leaf` is a session state cell: the value of a `let` whose
    /// `keep` is `session`.
    pub fn is_session_state(&self, leaf: Leaf) -> bool {
        leaf.path.facet == facet::FacetKind::Let
            && leaf.path.field == "value"
            && self.doc.get::<facet::Let>(leaf.node).is_some_and(|l| l.keep.constant().and_then(|v| v.as_str()) == Some("session") && l.value.expr().is_none())
    }

    /// Set a session state cell. Not a transaction: never saved, never in
    /// undo. Refused for any other leaf.
    pub fn set_state(&mut self, leaf: Leaf, value: Value) -> Result<(), String> {
        if !self.is_session_state(leaf) {
            return Err(format!("{leaf} is not session state (a let with keep=\"session\" holding a constant)"));
        }
        // Back at its initial value, it is no longer held.
        if self.doc.leaf(leaf.node, leaf.path).and_then(|s| s.constant()) == Some(&value) {
            self.session_state.remove(&leaf);
        } else {
            self.session_state.insert(leaf, value);
        }
        self.publish_state();
        if !self.clusters.list.is_empty() {
            self.resolve_clusters_live();
        }
        self.resolve_physics();
        Ok(())
    }

    /// Every live action for `event`, by the node it listens on: `on.node`
    /// when set, else its parent.
    fn listeners(&self, event: &str) -> BTreeMap<NodeId, Vec<NodeId>> {
        let ty = TypeId::lookup("on").expect("registered");
        let mut by_node: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for on in self.doc.nodes_of_type(ty).filter(|&n| self.doc.is_live(n) && !expand::in_repeat_template(&self.doc, n)) {
            let Some(f) = self.doc.get::<facet::On>(on) else { continue };
            if f.event.constant().and_then(|v| v.as_str()) != Some(event) {
                continue;
            }
            let named = f.node.constant().and_then(|v| v.as_ref()).filter(|n| n.raw() != 0);
            let Some(listens) = named.or_else(|| self.doc.node(on).and_then(|n| n.parent)) else { continue };
            by_node.entry(listens).or_default().push(on);
        }
        by_node
    }

    /// The `on` nodes that `event` on `node` runs (reactivity.md §5.2): those
    /// of the nearest node at or above `node` that has any for this event,
    /// as a click on a button's label reaches the button. With that node.
    pub fn handlers(&self, node: NodeId, event: &str) -> Option<(NodeId, Vec<NodeId>)> {
        let mut by_node = self.listeners(event);
        if by_node.is_empty() {
            return None;
        }
        let mut at = Some(node);
        while let Some(n) = at {
            if let Some(found) = by_node.remove(&n) {
                return Some((n, found));
            }
            at = self.doc.node(n).and_then(|x| x.parent);
        }
        None
    }

    /// Every node at or above `node` that listens for the pointer coming
    /// or going (`enter`, `leave`): what the pointer is inside, for a host
    /// to compare from one frame to the next.
    pub fn hover_listeners(&self, node: NodeId) -> Vec<NodeId> {
        let (enter, leave) = (self.listeners("enter"), self.listeners("leave"));
        if enter.is_empty() && leave.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut at = Some(node);
        while let Some(n) = at {
            if enter.contains_key(&n) || leave.contains_key(&n) {
                out.push(n);
            }
            at = self.doc.node(n).and_then(|x| x.parent);
        }
        out
    }

    /// Whether the document declares any action at all.
    pub fn has_actions(&self) -> bool {
        self.doc.nodes_with(facet::FacetKind::On).next().is_some()
    }

    /// Run what `event` on `node`, or on the nearest node above it that
    /// listens, declares (`handlers`); see `fire_at`.
    pub fn fire(&mut self, node: NodeId, event: &str, actor: Actor, ts: u64) -> Result<Option<Applied>, String> {
        match self.handlers(node, event) {
            Some((listens, _)) => self.fire_at(listens, event, actor, ts),
            None => Ok(None),
        }
    }

    /// Run what `event` declares on exactly `listens`. Every write is
    /// computed from the values before any of them applies, so the result
    /// does not depend on their order. Writes to the document are one
    /// transaction, one undo step; writes to session state leave no
    /// history. `Ok(None)` when nothing listens or nothing changes the
    /// document.
    pub fn fire_at(&mut self, listens: NodeId, event: &str, actor: Actor, ts: u64) -> Result<Option<Applied>, String> {
        let Some(actions) = self.listeners(event).remove(&listens) else { return Ok(None) };
        let leaf_of = |f: &str| LeafPath::new(facet::FacetKind::Write, f).expect("declared leaf");
        let mut ops: Vec<Op> = Vec::new();
        let mut session: Vec<(Leaf, Value)> = Vec::new();
        let mut label = None;
        for on in actions {
            if let Some(name) = self.doc.get::<facet::On>(on).and_then(|f| f.emit.constant()).and_then(|v| v.as_str()).filter(|n| !n.is_empty()) {
                self.emitted.push((listens, name.to_owned()));
            }
            if label.is_none() {
                label = self.doc.get::<facet::Name>(on).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).filter(|n| !n.is_empty()).map(str::to_owned);
            }
            let writers: Vec<NodeId> = std::iter::once(on).chain(self.doc.children(on).iter().copied()).filter(|&w| self.doc.is_live(w) && self.doc.facet(w, facet::FacetKind::Write).is_some()).collect();
            for w in writers {
                let read = |f: &str| self.reactive.read(&self.doc, Leaf { node: w, path: leaf_of(f) });
                let Some(target) = read("target").and_then(|v| v.as_ref()).filter(|n| n.raw() != 0) else { continue };
                let prop = read("prop").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
                let path: LeafPath = prop.parse().map_err(|e| format!("{w}: write.prop {prop:?}: {e}"))?;
                let leaf = Leaf { node: target, path };
                let now = self.reactive.read(&self.doc, leaf);
                let value = match read("verb").as_ref().and_then(|v| v.as_str()) {
                    Some("toggle") => match now {
                        Some(Value::Bool(b)) => Value::Bool(!b),
                        other => return Err(format!("{w}: toggle needs a yes/no value at {leaf}, found {}", other.map_or("nothing".to_owned(), |v| v.kind().name().to_owned()))),
                    },
                    Some("step") => {
                        let by = read("by").and_then(|v| v.as_quantity()).ok_or_else(|| format!("{w}: write.by is not a number"))?;
                        match now.as_ref().and_then(|v| v.as_quantity()) {
                            Some((x, dim)) if dim == by.1 || by.1.is_none() => Value::quantity(x + by.0, dim),
                            _ => return Err(format!("{w}: step needs a number at {leaf}, in the unit of write.by")),
                        }
                    }
                    _ => {
                        if let Some(why) = self.reactive.errors.get(&Leaf { node: w, path: leaf_of("to") }) {
                            return Err(format!("{w}: write.to failed: {why}"));
                        }
                        read("to").ok_or_else(|| format!("{w}: write.to has no value"))?
                    }
                };
                if self.is_session_state(leaf) {
                    session.push((leaf, value));
                } else {
                    let (source, v) = handle::resolve_target(&self.doc, &self.reactive, leaf, value)?;
                    if self.reactive.read(&self.doc, source).as_ref() != Some(&v) {
                        ops.push(Op::Set { id: source.node, path: source.path, slot: Slot::Const(v) });
                    }
                }
            }
        }
        for (leaf, value) in session {
            self.set_state(leaf, value)?;
        }
        if ops.is_empty() {
            return Ok(None);
        }
        let label = label.unwrap_or_else(|| event.to_owned());
        self.apply(TransactionInput { label, actor, ts, idempotency_key: None, ops, exact: false }).map(Some).map_err(|errors| errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))
    }

    /// Events actions asked the host to hear (`on.emit`), with the node
    /// each fired on, since the last take.
    pub fn take_emitted(&mut self) -> Vec<(NodeId, String)> {
        std::mem::take(&mut self.emitted)
    }

    /// Structure derived since the last take, outside any transaction
    /// (`settle_repeats`): the host materialises it as it does a
    /// transaction's ops.
    /// `problems`, by the node that shows each: the node itself, or for a
    /// derived node the authored one it is derived for (an instance, a
    /// repeat), named in the note. At most three notes a node.
    pub fn problem_notes(&self) -> BTreeMap<NodeId, Vec<String>> {
        let mut out: BTreeMap<NodeId, Vec<String>> = BTreeMap::new();
        let mut more: BTreeMap<NodeId, usize> = BTreeMap::new();
        for p in self.problems() {
            let shown = p.node.path()[0];
            // A repeat's template is evaluated for item 0 only so that it
            // has values; what fails there fails in the items too.
            if p.kind == "binding" && (expand::in_repeat_template(&self.doc, p.node) || !self.doc.is_live(p.node)) {
                continue;
            }
            let note = if shown == p.node { p.message } else { format!("{}: {}", p.node, p.message) };
            let notes = out.entry(shown).or_default();
            if notes.len() < 3 {
                notes.push(note);
            } else {
                *more.entry(shown).or_default() += 1;
            }
        }
        for (node, n) in more {
            out.entry(node).or_default().push(format!("and {n} more"));
        }
        out
    }

    pub fn has_derived_ops(&self) -> bool {
        !self.derived_ops.is_empty()
    }

    pub fn take_derived_ops(&mut self) -> Vec<Op> {
        std::mem::take(&mut self.derived_ops)
    }

    /// Everything wrong that a node can show (reactivity.md §4.3): what
    /// expansion could not derive or derived past a limit, repeats cut at
    /// their `max`, and bindings that failed.
    pub fn problems(&self) -> Vec<expand::Problem> {
        let mut out = self.expansion_problems.clone();
        out.extend(self.truncated.iter().cloned());
        out.extend(self.reactive.errors.iter().map(|(leaf, message)| expand::Problem { node: leaf.node, kind: "binding", message: format!("{}: {message}", leaf.path) }));
        out
    }

    /// Recompute what the store derives after every change: the
    /// across/through network's solution, then the layouts that place
    /// groups' children (`layout.rs`), which may read it.
    pub fn resolve_physics(&mut self) {
        // Structure first: what follows places and styles the new items.
        self.publish_selection();
        let derived = self.settle_repeats();
        self.derived_ops.extend(derived);
        self.publish_state();
        self.solve_network();
        self.resolve_theme();
        self.resolve_layout();
        self.resolve_contexts();
    }

    /// Merge the document's `context` nodes over the built-in library when
    /// their markup changed (contexts.md §2.2).
    fn resolve_contexts(&mut self) {
        let ty = TypeId::lookup("context").expect("registered");
        let markups: Vec<(NodeId, String)> = self
            .doc
            .nodes_of_type(ty)
            .filter(|&n| self.doc.is_live(n))
            .filter_map(|n| Some((n, self.doc.get::<facet::ContextDef>(n)?.markup.constant()?.as_str()?.to_owned())))
            .collect();
        if markups.is_empty() {
            self.contexts = None;
        } else if self.contexts.as_ref().is_none_or(|(m, _)| *m != markups) {
            let lib = contexts::Library::with_document(&markups);
            self.contexts = Some((markups, lib));
        }
    }

    /// Publish every laid-out group's placements (layout-and-tables.md §2.3)
    /// under `ProducerId::LAYOUT`: derived, never stored, locked against
    /// direct edits like any published leaf.
    pub fn resolve_layout(&mut self) {
        let (mut values, grids) = layout::compute(self);
        if self.motion.enabled {
            self.start_motion(&values);
            self.motion.targets = values.clone();
            self.motion.apply(&mut values);
        }
        // A table whose tracks moved redraws its lines: report it changed.
        if grids != self.tables {
            let columns: LeafPath = "table.columns".parse().expect("declared leaf");
            let moved: Vec<NodeId> = grids.iter().filter(|(t, g)| self.tables.get(t) != Some(g)).map(|(t, _)| *t).collect();
            self.tables = grids;
            for t in moved {
                let leaf = Leaf { node: t, path: columns };
                let value = self.reactive.read(&self.doc, leaf).unwrap_or(Value::Str(String::new()));
                self.reactive.mark(leaf, value);
            }
        }
        if values.is_empty() && self.reactive.published_by(ProducerId::LAYOUT).is_empty() {
            return;
        }
        if values == self.reactive.published_by(ProducerId::LAYOUT) {
            return;
        }
        self.signal_stamp += 1;
        self.reactive.publish(&self.doc, ProducerId::LAYOUT, values, self.signal_stamp);
    }

    /// A resolver over this document's contexts that sees nodes'
    /// interaction states.
    pub fn resolver(&self) -> contexts::Resolver<'_> {
        contexts::Resolver { doc: &self.doc, lib: self.contexts(), states: Some(&self.interaction) }
    }

    /// The nodes that are hovered, selected, pressed or dragged now, as the
    /// host sees them. Returns whether anything changed.
    pub fn set_interaction(&mut self, states: BTreeMap<NodeId, Vec<&'static str>>) -> bool {
        if self.interaction == states {
            return false;
        }
        self.interaction = states;
        true
    }

    pub fn interaction(&self) -> &BTreeMap<NodeId, Vec<&'static str>> {
        &self.interaction
    }

    pub fn motion_enabled(&self) -> bool {
        self.motion.enabled
    }

    /// Turn declared transitions on (a host with a clock) or off.
    pub fn set_motion(&mut self, enabled: bool) {
        self.motion.enabled = enabled;
        if !enabled {
            self.motion.anims.clear();
        }
    }

    /// Advance components' state machines to `now` (seconds, the host's
    /// clock; plans/specs/machines.md): start a transition where an
    /// instance's state argument changed, move the ones running, and publish
    /// channel values. Returns what is running, so the host renders it.
    /// Pin a state on its main for editing, or unpin (`None`): session
    /// only, never saved or undone. A pinned machine's channels show the
    /// state's pose on the main, and number edits record into it.
    pub fn pin_state(&mut self, pin: Option<machine::Pinned>) {
        self.machines.pinned = pin;
        self.advance_machines(self.motion.clock);
    }

    pub fn pinned_state(&self) -> Option<&machine::Pinned> {
        self.machines.pinned.as_ref()
    }

    /// What the machines were running at the last advance: each instance
    /// mid-transition, which transition, how far, between which states.
    pub fn running_machines(&self) -> &[machine::Running] {
        &self.machines.running
    }

    pub fn advance_machines(&mut self, now: f64) -> Vec<machine::Running> {
        // A pin whose state is gone (deleted, or a rename undone) lets go.
        if let Some(pin) = &self.machines.pinned
            && !machine::state_names(&self.doc, pin.machine).contains(&pin.state)
        {
            self.machines.pinned = None;
        }
        let (values, running) = {
            let reactive = &self.reactive;
            let doc = &self.doc;
            let read = |l: Leaf| reactive.read(doc, l);
            self.machines.advance(doc, &read, now, self.time())
        };
        if values != self.reactive.published_by(ProducerId::MACHINES) {
            self.publish(ProducerId::MACHINES, values);
        }
        running
    }

    /// Advance the transitions in flight to `now` (seconds, the host's
    /// clock) and publish where they have got to. Returns the nodes that
    /// moved, so the host renders them; empty when nothing is in flight.
    pub fn advance_motion(&mut self, now: f64) -> Vec<NodeId> {
        self.motion.clock = now;
        if self.motion.anims.is_empty() {
            return Vec::new();
        }
        let moving: Vec<NodeId> = self.motion.anims.keys().copied().collect();
        let mut values = self.motion.targets.clone();
        self.motion.apply(&mut values);
        if values != self.reactive.published_by(ProducerId::LAYOUT) {
            self.signal_stamp += 1;
            self.reactive.publish(&self.doc, ProducerId::LAYOUT, values, self.signal_stamp);
        }
        moving
    }

    /// Start a transition for each node layout moved whose context declares
    /// one for its placement: from where it is drawn to where layout puts
    /// it. A dragged item floating at the pointer does not ease.
    fn start_motion(&mut self, values: &BTreeMap<Leaf, Value>) {
        let (xp, yp): (LeafPath, LeafPath) = ("transform2d.x".parse().expect("declared leaf"), "transform2d.y".parse().expect("declared leaf"));
        let shown = self.reactive.published_by(ProducerId::LAYOUT);
        let num = |m: &BTreeMap<Leaf, Value>, node: NodeId, path: LeafPath| m.get(&Leaf { node, path }).and_then(|v| v.as_f64());
        let floating = self.layout_preview.as_ref().and_then(|p| p.floating).map(|(n, _)| n);
        let nodes: Vec<NodeId> = values.keys().filter(|l| l.path == xp).map(|l| l.node).collect();
        for node in nodes {
            let (Some(tx), Some(ty)) = (num(values, node, xp), num(values, node, yp)) else { continue };
            let before = (num(&self.motion.targets, node, xp), num(&self.motion.targets, node, yp));
            if before == (Some(tx), Some(ty)) || Some(node) == floating {
                continue;
            }
            let (Some(sx), Some(sy)) = (num(&shown, node, xp), num(&shown, node, yp)) else { continue };
            let chain = contexts::chain_of(&self.doc, node);
            if chain.is_empty() {
                continue;
            }
            let declared = contexts::Resolver::new(&self.doc, self.contexts()).get(&chain, chain.len() - 1, contexts::Field::PLACEMENT);
            let Some(m) = contexts::motion(&declared).filter(|m| matches!(m.when, "any" | "layout") && m.ms > 0.0) else { continue };
            self.motion.anims.insert(node, Anim { from: [sx, sy], to: [tx, ty], t0: self.motion.clock, ms: m.ms, ease: m.ease });
        }
    }

    /// The theme colour tokens resolve for: `dark` or light (the host's,
    /// as its canvas is drawn). Everything using a token redraws.
    pub fn set_theme(&mut self, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            self.resolve_theme();
            self.resolve_layout();
        }
    }

    pub fn dark(&self) -> bool {
        self.dark
    }

    /// Publish each token's colour for the theme (`ProducerId::THEME`).
    fn resolve_theme(&mut self) {
        let values = palette::publish(&self.doc, self.dark);
        if values == self.reactive.published_by(ProducerId::THEME) {
            return;
        }
        self.signal_stamp += 1;
        self.reactive.publish(&self.doc, ProducerId::THEME, values, self.signal_stamp);
    }

    /// Lay a container out in a previewed order, or stop (`None`): the
    /// gap a dragged item would land in opens by ordinary layout.
    pub fn set_layout_preview(&mut self, preview: Option<layout::Preview>) {
        if self.layout_preview != preview {
            self.layout_preview = preview;
            self.resolve_layout();
        }
    }

    pub fn layout_preview(&self) -> Option<&layout::Preview> {
        self.layout_preview.as_ref()
    }

    /// The contexts this document's nodes resolve against
    /// (plans/specs/contexts.md): the built-in library, with the document's
    /// own `context` nodes merged over it (`resolve_contexts`).
    pub fn contexts(&self) -> &contexts::Library {
        match &self.contexts {
            Some((_, lib)) => lib,
            None => contexts::Library::builtin(),
        }
    }

    /// Where a table's cells fell, as of the last layout (`table::Grid`).
    pub fn table_grid(&self, table: NodeId) -> Option<&table::Grid> {
        self.tables.get(&table)
    }

    /// Solve the across/through network (ECM §6.1, step 3) and put its
    /// solution in the graph's physics layer: derived, never stored, read by
    /// bindings like any value. A network that does not converge keeps its
    /// previous solution and says why.
    fn solve_network(&mut self) {
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
            // A network is a set of equations, not a drag: nothing should
            // stay where it was at the expense of an equation (the weak
            // rows stalled a kΩ–mA circuit short of the tolerance).
            let options = iron_solve::Options { max_iterations: 100, least_change: false, ..Default::default() };
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
    pub fn expansion_problems(&self) -> &[expand::Problem] {
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
        self.apply_session(ops)?;
        // Bindings that read the selection follow it.
        if self.doc.nodes_with(facet::FacetKind::Selection).next().is_some() {
            if !self.clusters.list.is_empty() {
                self.resolve_clusters_live();
            }
            self.resolve_physics();
        }
        Ok(())
    }

    /// Keep the graph in step with a transaction that just applied.
    fn after_transaction(&mut self, tx: &mut Transaction) {
        // Outside a gesture the overlay holds only a live solve's pose (a
        // producer's publish or a playhead move re-solves the clusters).
        // Nothing else owns it, so it would shadow what this transaction
        // wrote, even after the cluster is gone: drop it, and re-solve
        // against the new document below. A gesture's own commit has taken
        // the gesture but still holds its drivers, and clears its previews
        // itself.
        let live = self.gesture.is_none() && self.drivers.is_empty() && self.fingers.is_empty() && !self.reactive.overlay().is_empty();
        if live {
            self.reactive.clear_overlay(&self.doc);
        }
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
        // Repeats whose count this changed get their copies, as part of
        // this transaction's derived structure.
        self.repeat_inputs = None;
        let copies = self.settle_repeats();
        tx.expanded.extend(copies);
        self.rebuild_clusters();
        if live && !self.clusters.list.is_empty() {
            self.resolve_clusters_live();
        }
        self.resolve_physics();
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// The document, without copying it; the store's history goes with it.
    pub fn into_document(self) -> Document {
        self.doc
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

    fn apply_now(&mut self, mut input: TransactionInput) -> Result<Applied, Vec<ApplyError>> {
        // A built-in colour token used for the first time joins the
        // document, in the same transaction (palette.rs).
        // After the transaction's own ops: its ids come after theirs, and
        // bindings are checked once all have applied.
        let theme = palette::ensure_ops(&self.doc, &input.ops);
        input.ops.extend(theme);
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
        self.after_transaction(&mut tx);
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
        let (mut applied, doc) = self.run(input)?;
        self.doc = doc;
        self.after_transaction(&mut applied);
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
        // An undone state change snaps rather than playing back
        // (plans/specs/machines.md §6), on the instances it touches.
        self.machines.snap.extend(args_set(&tx.undo_ops()));
        let input = TransactionInput {
            label: format!("undo: {}", tx.label),
            actor: Actor::System,
            ts,
            idempotency_key: None,
            ops: tx.undo_ops(),
            exact: false,
        };
        Some(match self.run(input) {
            Ok((mut applied, doc)) => {
                self.doc = doc;
                self.after_transaction(&mut applied);
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
        self.machines.snap.extend(args_set(&tx.replay_ops()));
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
            Ok((mut applied, doc)) => {
                self.doc = doc;
                self.after_transaction(&mut applied);
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
        let clusters = solve::clusters(&self.doc, &self.reactive, self.contexts()).map_err(|m| err(ErrorKind::Cycle, m))?;
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
        // A state pinned on its main records number edits to the component's
        // parts as that state's pose (machine.rs `record`).
        if let Some(pin) = self.machines.pinned.clone() {
            input.ops = machine::record(&self.doc, &pin, std::mem::take(&mut input.ops));
        }
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
            let (ops, problems) = expand::expand(&mut work, stamp, &self.repeats);
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
        if errors.is_empty() {
            if let Err(message) = attachment::validate(&self.doc, &work) {
                errors.push(ApplyError { op: 0, kind: ErrorKind::BadParent, message });
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
        view::with_problems(self.problem_notes(), || view::render_tree(&self.doc, &self.reactive, &self.clusters.var_cluster, q))
    }

    pub fn save(&self) -> String {
        serial::save(&self.doc)
    }

    /// A store over a loaded document, with empty history.
    pub fn load(text: &str) -> Result<Store, LoadError> {
        serial::load(text).map(Store::with_document)
    }

    // ---- Scopes (plans/specs/scopes.md) ---------------------------------

    /// The scope this store's document is, if it has been given one.
    pub fn scope(&self) -> Option<ScopeId> {
        self.doc.scope()
    }

    /// Name the scope (scopes.md §3). Identity metadata, not content: not an
    /// op, not in history, and it does not bump the version. It is written
    /// in the next save's header.
    pub fn set_scope(&mut self, scope: Option<ScopeId>) {
        self.doc.set_scope(scope);
    }

    /// Forget undo, redo and the log, keeping the document (scopes.md §4.3).
    /// Opening a scope replays its log onto the snapshot, which pushes every
    /// replayed transaction onto the undo stack; a reopened scope starts
    /// with nothing to undo, as any editor's freshly opened file does.
    pub fn clear_history(&mut self) {
        self.history.past.clear();
        self.history.future.clear();
        self.history.log.clear();
    }

    /// The logged transactions after `version`, oldest first, for the
    /// durable log's feed (scopes.md §4.2). `None` when the log no longer
    /// reaches back that far (the horizon dropped some): the caller cannot
    /// append, and must write a fresh snapshot instead.
    pub fn log_since(&self, version: Version) -> Option<Vec<&Transaction>> {
        if self.doc.version() <= version {
            return Some(Vec::new());
        }
        let first = self.history.log.front()?;
        // Versions are consecutive across the log, so the first one after
        // `version` must still be in it.
        if first.version.0 > version.0 + 1 {
            return None;
        }
        Some(self.history.log.iter().filter(|t| t.version > version).collect())
    }
}

/// A field of the `clock` facet.
fn clock_path(field: &str) -> LeafPath {
    LeafPath::new(FacetKind::Clock, field).expect("a clock field")
}

/// The nodes whose arguments `ops` set: an undo or redo of a state change
/// snaps those instances' machines.
fn args_set(ops: &[Op]) -> Vec<NodeId> {
    ops.iter()
        .filter_map(|o| match o {
            Op::Set { id, path, .. } if path.facet == facet::FacetKind::Args => Some(*id),
            Op::AddFacet { id, facet } if facet.kind() == facet::FacetKind::Args => Some(*id),
            _ => None,
        })
        .collect()
}

/// An item's key from a value (reactivity.md §3.2): a number or quantity
/// by its digits, text by its characters, a node by its id.
fn key_text(v: &Value) -> String {
    let text = match v {
        Value::Number(n) => value::fmt_num(*n),
        Value::Quantity { si, .. } => value::fmt_num(*si),
        Value::Ref(id) => id.to_string().trim_start_matches('#').to_owned(),
        v => v.to_string(),
    };
    id::item_key(&text)
}

/// The key of each element of a repeat. With a `repeat.key` function, what
/// it gives for each; otherwise a node reference is its own key, a record's
/// `id` is, text is when the repeat is over a list of texts, and anything
/// else is keyed by index (numbers too: a series that slides would remake
/// every item on every change; give `key` to key numbers by value). An
/// error when two items share a key or the function fails.
fn item_keys(elements: &[Value], listed: bool, key: Option<&Value>) -> Result<Vec<String>, String> {
    let by_index = || (0..elements.len()).map(|k| k.to_string()).collect::<Vec<_>>();
    let keys: Vec<String> = match key {
        Some(Value::Fn(f)) if f.params.len() == 1 => {
            let mut out = Vec::with_capacity(elements.len());
            for e in elements {
                out.push(key_text(&f.apply(std::slice::from_ref(e)).map_err(|err| format!("repeat.key failed: {err}"))?));
            }
            out
        }
        Some(Value::Fn(f)) => return Err(format!("repeat.key takes one item; this function takes {}", f.params.len())),
        _ if !listed || elements.is_empty() => return Ok(by_index()),
        _ if elements.iter().all(|e| matches!(e, Value::Ref(_))) => elements.iter().map(key_text).collect(),
        _ if elements.iter().all(|e| e.field("id").is_some()) => elements.iter().map(|e| key_text(e.field("id").expect("checked"))).collect(),
        _ if elements.iter().all(|e| matches!(e, Value::Str(_) | Value::Enum(_))) => {
            let keys: Vec<String> = elements.iter().map(key_text).collect();
            // Repeated words are legitimate in a list of labels.
            let distinct: std::collections::BTreeSet<&String> = keys.iter().collect();
            if distinct.len() != keys.len() {
                return Ok(by_index());
            }
            keys
        }
        _ => return Ok(by_index()),
    };
    let distinct: std::collections::BTreeSet<&String> = keys.iter().collect();
    if distinct.len() != keys.len() {
        let twice = keys.iter().find(|k| keys.iter().filter(|x| x == k).count() > 1).cloned().unwrap_or_default();
        return Err(format!("two items share the key {twice}"));
    }
    Ok(keys)
}
