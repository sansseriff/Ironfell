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
#[doc(hidden)]
pub mod work;
mod structure;
#[cfg(feature = "structure-oracle")]
pub use structure::silence_for_canary;
pub mod reads;
mod engine;
mod clocks;
pub mod analysis;
pub mod composition;
pub mod compose;
pub mod constraint;
pub mod changes;
pub mod commands;
pub mod contexts;
pub mod contour;
pub mod copy;
pub mod definition;
pub mod document;
pub mod edit;
pub mod expand;
pub mod machine;
pub mod markup;
pub mod make;
pub mod expr;
pub mod geom;
pub mod slider;
mod presentation;
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
pub mod reply;
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
pub mod step;
pub mod query;
pub mod narrative;
pub mod table;
pub mod time;
pub mod palette;
pub mod types;
pub mod timeline_view;
pub mod tree;
pub mod units;
pub mod value;
pub mod view;
#[cfg(feature = "differential")]
pub mod differential;

pub use facet::{Facet, FacetKind, LeafError, LeafPath, LeafStruct};
pub use document::{Document, Node, RelKind, Relation};
pub use expr::{Expr, Leaf};
pub use geom::{Affine2, Affine3};
pub use gesture::{GestureId, GestureStatus};
pub use changes::Changes;
pub use graph::{ProducerId, Signal};
pub use handle::Handle;
pub use id::{NodeId, RelationId, ScopeId, Version};
pub use op::{Actor, ApplyError, ErrorKind, Op, Transaction, TransactionInput};
pub use order::OrderKey;
pub use registry::{ChildPolicy, Host, TypeId, TypeSpec};
pub use serial::LoadError;
pub use solve::{Finger, SolverProblem, SolverSummary, Unmet};
pub use units::Dim;
pub use value::{Slot, Value, ValueKind};
pub use view::{Fidelity, ViewError, ViewQuery, ViewRelation};
pub use tree::Tree;

/// The store. With the `differential` feature it is a facade over two
/// `StoreCore`s compared after every call (`differential.rs`); without, the
/// one store itself.
#[cfg(not(feature = "differential"))]
pub type Store = StoreCore;
#[cfg(feature = "differential")]
pub use differential::Store;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

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

/// The document's journal, open from `run` until the transaction is kept.
/// Nothing after `run` refuses (DS-7), so it is consumed exactly once, by
/// `commit`.
#[must_use]
struct Rollback(bool);

impl Rollback {
    fn take(mut self) {
        assert!(std::mem::take(&mut self.0), "a rollback token is used once");
    }
}

impl Drop for Rollback {
    fn drop(&mut self) {
        // While unwinding, a second panic would abort the test binary and
        // hide the first.
        debug_assert!(!self.0 || std::thread::panicking(), "a transaction was not committed");
    }
}

/// How many times one pass solves a cluster whose inputs another
/// cluster's write changed (through a binding or a layout): enough for a
/// chain, bounded for a loop.
const RESOLVES: usize = 4;

/// Stale cells an idle `advance` brings up to date (§8.7).
const IDLE_SWEEP: usize = 256;

/// Settles that read every drawn repeat's inputs, since the process
/// started: what RE-5.4's demonstrations count.
static SETTLE_SCANS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[doc(hidden)]
pub fn settle_scan_count() -> u64 {
    SETTLE_SCANS.load(std::sync::atomic::Ordering::Relaxed)
}

/// How many times `resolve_physics` settles repeats again after what the
/// rest of the derivation published.
const LAYOUT_ROUNDS: usize = 4;

/// How many times one repeat's items may change in one `resolve_physics`
/// before the store asks whether what it repeats over reads its own items.
/// One that does never rests, and would grow to `repeat.max`, since layout
/// runs in the settle's own passes (handoff §4.10, RE-5.6): its items are
/// held, and it is a problem (decision D6). One that reads a chain of
/// others changes once per link, and goes on.
const REPEAT_CHANGES: usize = LAYOUT_ROUNDS + 1;

/// Facets whose nodes' cells the store reads besides drawing them, so they
/// stay needed whatever is observed (reactive-engine.md §8.3): state
/// (`let`, traces, accumulators, machines and what their player reads,
/// queries, selection), structure (repeats, conditionals, items, embeds,
/// the composition itself), the network's ports and equations,
/// constraints, anchors, and the timeline. What else the store reads from
/// outside the graph is made needed when it is read (`settle_reads`).
const ROOTED: &[FacetKind] = &[
    FacetKind::Let,
    FacetKind::Args,
    FacetKind::Transition,
    FacetKind::Tween,
    FacetKind::Trace,
    FacetKind::Accumulate,
    FacetKind::Machine,
    FacetKind::Channel,
    FacetKind::Query,
    FacetKind::Selection,
    FacetKind::Write,
    FacetKind::Repeat,
    FacetKind::When,
    FacetKind::Item,
    FacetKind::Embed,
    FacetKind::EmbedCamera,
    FacetKind::Composition,
    FacetKind::Port,
    FacetKind::Equation,
    FacetKind::Connection,
    FacetKind::Constraint,
    FacetKind::Constraints,
    FacetKind::Anchor,
    FacetKind::Attachment,
    FacetKind::Clock,
    FacetKind::Clip,
    FacetKind::Key,
    FacetKind::Timing,
    FacetKind::Track,
];

/// What a host shows (reactive-engine.md §8.2): the store keeps running
/// what an observer reaches, and lets the rest rest.
#[derive(Clone, Debug, PartialEq)]
pub enum Interest {
    /// A 2D view of a composition: what is drawn in `rect` (x, y, w, h, in
    /// the composition's units) at `zoom`. A view reaches the compositions
    /// its embeds show, on an open path.
    Region { composition: NodeId, rect: [f64; 4], zoom: f64 },
    /// Nodes named by id: an inspector, a selection, open rows.
    Nodes(Vec<NodeId>),
}

/// An observer, as `observe` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObserverId(pub u64);

/// What `advance` asks of the host (reactive-engine.md §10.2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wake {
    /// Call `advance` again by this host time: `now` while anything moves
    /// continuously; `None` when nothing is due.
    pub next: Option<f64>,
    /// The compositions something moving is drawn in: they redraw.
    pub live: BTreeSet<NodeId>,
}

/// What one `advance` did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Advanced {
    pub wake: Wake,
    /// The query requests that became due, for the host to carry out and
    /// answer with `resolve_query`.
    pub queries: Vec<query::Request>,
    /// Whether anything moved, published or came to rest: the host
    /// redraws.
    pub changed: bool,
}

/// What `run` returns on success.
struct Run {
    tx: Transaction,
    /// What expansion wrote for it: derived, never history.
    expanded: Vec<Op>,
    /// The whole inverse, derived nodes included, for `Changes`.
    full_inverse: Vec<Op>,
    /// What expansion found, when `run` expanded; `None` leaves the store's
    /// as they were.
    problems: Option<engine::owners::Delta>,
    /// Whether it edited parts a pinned machine state shares, to count once
    /// the transaction is accepted.
    shared: bool,
    rollback: Rollback,
    /// The copies' leaves the solver held before the transaction, and
    /// their values then (`StoreCore::keep_solved`).
    solved: Vec<(Leaf, Slot)>,
    /// A recorded transaction's writes to copies, for `apply_recorded`.
    recorded: Vec<Op>,
}

pub struct StoreCore {
    /// The authored document, and the copies when kept apart (handoff §4.6).
    docs: tree::Docs,
    /// Who derives what (reactive-engine handoff §4.4, stage 2).
    owners: engine::owners::Owners,
    history: History,
    recent_keys: VecDeque<(String, Version)>,
    reactive: graph::Reactive,
    /// The playhead, in seconds. Runtime state, never saved.
    time: f64,
    time_errors: std::collections::BTreeMap<NodeId, String>,
    /// Constraint clusters, rebuilt after every transaction.
    clusters: solve::Clusters,
    /// The clusters, by key, whose last solve converged and whose change
    /// native has not run since: not solved again (§9.4, A6).
    committed: BTreeSet<NodeId>,
    /// The grouping native's run count when its result was last taken
    /// (`grouped`), and the error it gave then.
    grouped_at: Option<u64>,
    /// Leaves the current gesture explicitly moved (as opposed to leaves the
    /// solver moved for it): what a commit writes.
    drivers: std::collections::BTreeSet<Leaf>,
    /// Active drag drivers.
    fingers: Vec<Finger>,
    /// Bumped by every session-tier change (`apply_session`), which leaves
    /// the document's version alone.
    session_version: u64,
    machines_unsettled: Option<expand::Problem>,
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
    /// Traces and accumulators (step.rs): presentation state.
    steps: step::Steps,
    /// The traces and accumulators on running clocks, as the gate last
    /// decided: what the step player visits each frame (DS-2).
    running_steps: (Vec<NodeId>, Vec<NodeId>),
    /// The machine player visits every instance at its next advance: the
    /// structure changed (DS-2: otherwise only what changed).
    machines_full: bool,
    /// The clips animating something on a running clock, and the
    /// compositions they animate, as the gate last decided.
    running_clips: Vec<NodeId>,
    animated: BTreeSet<NodeId>,
    /// Where the last clip ends, kept until the clips or keys change.
    clips_end: Option<f64>,
    /// The tokens' stamp and the scheme the theme was last published for.
    theme_at: Option<(u64, bool)>,
    token_leaves: Option<(u64, std::sync::Arc<BTreeMap<String, Leaf>>)>,
    /// Queries to Models (query.rs): presentation state.
    queries: query::Queries,
    /// Items entering and leaving, by owner (a repeat or a conditional):
    /// presentation state (reactivity.md §13, phase 7).
    presence: work::Counted<BTreeMap<NodeId, Presence>, { work::STORE_PRESENCE }>,
    /// The repeats in `presence` with an item entering or leaving: what the
    /// presence stepper visits each frame (DS-2), not every repeat.
    presence_moving: BTreeSet<NodeId>,
    presence_at: Option<f64>,
    /// Each composition's and pictured entity's clock, which the gate
    /// pauses while nothing shows or reads it (clocks.rs).
    clocks: clocks::Clocks,
    /// The host time of the last `advance`: the playhead player's step.
    advanced_at: Option<f64>,
    /// The graph's propagation count as the last `advance` ended: when it
    /// has not moved since, nothing published or changed in between, and
    /// the store is at rest for the idle sweep.
    evaluated_at: u64,
    /// What the host shows (§8.2), and whether everything counts as shown
    /// (`observe_all`: the default, and the differential reference).
    observers: work::Counted<BTreeMap<ObserverId, Interest>, { work::STORE_OBSERVERS }>,
    next_observer: u64,
    observe_all: bool,
    /// What the last `advance` found live, and the queries then waiting:
    /// the frame something comes to rest still redraws.
    advanced: (BTreeSet<NodeId>, Vec<NodeId>),
    /// Nodes' interaction states, as the host last reported them (session;
    /// read by rules that match `state:`).
    interaction: BTreeMap<NodeId, Vec<&'static str>>,
    /// What the last expansion could not derive (component-model.md §3).
    expansion_problems: Vec<expand::Problem>,
    /// The items each drawn repeat and conditional has, by key
    /// (plans/specs/repeat.md), as last settled from its inputs.
    repeats: work::Counted<expand::Items, { work::STORE_REPEATS }>,
    /// What each repeat without enter or exit timing settled
    /// to last, and from what: a cache, a pure function of its key.
    item_memo: work::Counted<BTreeMap<NodeId, (ItemsKey, ItemsOf)>, { work::STORE_ITEM_MEMO }>,
    /// The across/through network as of a count of document
    /// writes (`Document::edits`): a cache.
    network: Option<(u64, std::sync::Arc<crate::port::Network>)>,
    /// The structural roots, each with the nodes its layout covers; each
    /// `use`'s pinned parts; the nodes structural changes shaped since the
    /// last `sync_layout`; and the x-placed nodes of each root (for glide
    /// specs). Kept, not found by a pass (DS-3b).
    layout_roots: Option<BTreeMap<NodeId, BTreeSet<NodeId>>>,
    pinned_by_use: Option<BTreeMap<NodeId, BTreeSet<NodeId>>>,
    layout_touched: BTreeSet<NodeId>,
    glide_nodes: BTreeMap<NodeId, Vec<NodeId>>,
    /// The count of document writes the repeat natives were made for.
    /// The drawn repeats and conditionals, and each one's `each`, `count`,
    /// `key` and `when.is` as last settled: what `settle_repeats` compares
    /// before doing any work. `None` after structure changed, until the
    /// next settle.
    repeat_inputs: BTreeMap<NodeId, [Option<Value>; 7]>,
    /// The drawn repeats and conditionals (`None` until first found), the
    /// ones to read at the next settle, each one's problems and the item
    /// leaves it published, and the repeat natives to make and drop
    /// (DS-3b: kept as structure changes, not found by a pass).
    drawn: Option<BTreeSet<NodeId>>,
    repeat_dirty: BTreeSet<NodeId>,
    repeat_problems: BTreeMap<NodeId, Vec<expand::Problem>>,
    item_shown: BTreeMap<NodeId, Vec<Leaf>>,
    repeat_natives: (BTreeSet<NodeId>, BTreeSet<NodeId>),
    /// Session state cells (`let.keep = session`) that differ from their
    /// initial value (reactivity.md §5.1): published over the constant,
    /// never saved, dropped with their node.
    session_state: BTreeMap<Leaf, Value>,
    /// Events actions emitted for the host (`take_emitted`).
    emitted: Vec<(NodeId, String)>,
    /// Repeats that asked for more items than `repeat.max` allows.
    /// Repeats whose inputs read their own items' layout, so that they did
    /// not come to rest (decision D6), as of the last `resolve_physics`.
    feedback: Vec<expand::Problem>,
    truncated: Vec<expand::Problem>,
    /// Derived structure made outside a transaction (a repeat that grew
    /// during a drag, or when a producer published), for the host to
    /// materialise. Never history.
    /// What moved since the last `take_changes`, from the ops applied
    /// (reactive-engine.md §12.2); the values, marks and geometry are added
    /// when it is taken.
    pending: Changes,
    settling: bool,
    /// How many times each repeat's items changed in this `resolve_physics`
    /// (`REPEAT_CHANGES`).
    repeat_changes: BTreeMap<NodeId, usize>,
    /// The repeats found reading their own items: their items are held as
    /// they are, and are the `feedback` problems, until the next
    /// transaction lets them try again.
    restless: BTreeSet<NodeId>,
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
    /// The text engine generation text was last measured under
    /// (`refresh_text`).
    text_generation: u64,
}

impl Default for StoreCore {
    fn default() -> Self {
        StoreCore::new()
    }
}

/// Declared transitions (affordances.md §8.2): the one interpolator.
#[derive(Clone, Default)]
struct Motion {
    enabled: bool,
    /// The host's clock, seconds, as of its last frame.
    clock: f64,
    values: BTreeMap<Leaf, presentation::Tween>,
}

const DEFAULT_HORIZON: usize = 200;
const RECENT_KEYS: usize = 64;

impl StoreCore {
    /// A scratch store for planning an edit (`edit`, `compose`, `parts`):
    /// this one as it stands, both documents and everything derived from
    /// them copied rather than derived again, which re-expanded every owner
    /// on each plan (115 ms at 700 meters). It has no history, gesture,
    /// previews or pending changes of its own, and no motion: its layout is
    /// settled, never part-way through a transition or a reorder's preview.
    pub(crate) fn scratch(&self) -> StoreCore {
        let mut docs = self.docs.clone();
        docs.gone.clear();
        let mut reactive = self.reactive.clone();
        reactive.clear_overlay(Tree::of(&docs));
        // A plan reads every cell exact.
        let relaxed = reactive.relax(Tree::of(&docs));
        let mut scratch = StoreCore {
            docs,
            owners: self.owners.clone(),
            history: History { horizon: DEFAULT_HORIZON, ..Default::default() },
            recent_keys: VecDeque::new(),
            reactive,
            time: self.time,
            time_errors: self.time_errors.clone(),
            clusters: self.clusters.clone(),
            committed: self.committed.clone(),
            grouped_at: self.grouped_at,
            drivers: Default::default(),
            fingers: Vec::new(),
            session_version: self.session_version,
            machines_unsettled: self.machines_unsettled.clone(),
            contexts: self.contexts.clone(),
            layout_preview: None,
            dark: self.dark,
            motion: Motion::default(),
            machines: self.machines.clone(),
            steps: self.steps.clone(),
            running_steps: self.running_steps.clone(),
            machines_full: true,
            // What runs, as the gate decided (it is copied): a plan that
            // edits a key samples the clips it runs (council review of DS-2).
            running_clips: self.running_clips.clone(),
            animated: self.animated.clone(),
            clips_end: None,
            theme_at: None,
            token_leaves: None,
            queries: self.queries.clone(),
            presence: self.presence.clone(),
            presence_moving: self.presence_moving.clone(),
            presence_at: self.presence_at,
            clocks: self.clocks.clone(),
            advanced_at: self.advanced_at,
            evaluated_at: self.evaluated_at,
            advanced: self.advanced.clone(),
            observers: self.observers.clone(),
            next_observer: self.next_observer,
            observe_all: self.observe_all,
            interaction: self.interaction.clone(),
            expansion_problems: self.expansion_problems.clone(),
            repeats: self.repeats.clone(),
            item_memo: self.item_memo.clone(),
            network: self.network.clone(),
            layout_roots: self.layout_roots.clone(),
            pinned_by_use: self.pinned_by_use.clone(),
            layout_touched: self.layout_touched.clone(),
            glide_nodes: self.glide_nodes.clone(),
            repeat_inputs: self.repeat_inputs.clone(),
            drawn: self.drawn.clone(),
            repeat_dirty: self.repeat_dirty.clone(),
            repeat_problems: self.repeat_problems.clone(),
            item_shown: self.item_shown.clone(),
            repeat_natives: self.repeat_natives.clone(),
            session_state: self.session_state.clone(),
            emitted: Vec::new(),
            truncated: self.truncated.clone(),
            feedback: self.feedback.clone(),
            pending: Changes::default(),
            settling: false,
            repeat_changes: BTreeMap::new(),
            restless: BTreeSet::new(),
            gesture: None,
            ended: None,
            next_gesture: self.next_gesture,
            geometry: self.geometry.clone(),
            producers: self.producers.clone(),
            text_generation: self.text_generation,
        };
        // What this store published for layout is eased while a transition
        // runs, previewed, or computed from a gesture's previews (cleared
        // above); the scratch publishes the targets then.
        if self.layout_preview.is_some() {
            scratch.reactive.set_input(Tree::of(&scratch.docs), graph::InputKey::LayoutPreview, None);
        }
        // No glides: what is in flight is put at its target.
        let glided = scratch.reactive.gliding();
        if glided {
            scratch.reactive.glide = None;
            let keys: Vec<graph::NativeKey> = scratch.reactive.native_keys().filter(|k| matches!(k, graph::NativeKey::Layout(_))).collect();
            scratch.reactive.prime_natives(keys);
        } else {
            scratch.reactive.glide = None;
        }
        if relaxed {
            scratch.pipeline();
        } else if glided {
            scratch.reactive.flush(Tree::of(&scratch.docs));
        }
        scratch
    }

    /// Bring the graph up to date after structure changed through `ops`:
    /// update what the ops touched (reactive-engine handoff §4.4, stage 3a).
    fn regraph<'a>(&mut self, ops: impl Iterator<Item = &'a Op>) {
        let ops: Vec<&Op> = ops.collect();
        self.update_drawn(ops.iter().copied());
        let ops = ops.into_iter();
        let mut nodes: BTreeSet<NodeId> = BTreeSet::new();
        // The nodes a structural op made, moved, removed or reshaped, and
        // their parents (the inverse ops name the old ones): what a layout
        // whose subtree holds one must lay out again.
        let mut shaped: BTreeSet<NodeId> = BTreeSet::new();
        let mut reparented = false;
        // Which `@name` readers to look at again: the names whose providers
        // a let or token made, removed, renamed or reflagged may move, and
        // the readers under a moved node; all of them only when a name
        // cannot be told (DS-3b: not every reader on every change).
        let mut names: BTreeSet<String> = BTreeSet::new();
        let mut under: Vec<NodeId> = Vec::new();
        let mut rescope_all = false;
        let mut rederive = false;
        let doc = Tree::of(&self.docs);
        let name_of = |n: NodeId| doc.get::<facet::Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()).map(str::to_owned);
        let provides_any = |n: NodeId| matches!(doc.node(n).map(|x| x.ty.name()), Some("let" | "token"));
        // What connections and anchors derive reads ports, anchors,
        // connections, the relations between them and each anchor's chain
        // of transformed ancestors (`port::derived_signals`,
        // `anchor::positions`, `anchor::followers`).
        let derives = |kind: FacetKind| matches!(kind, FacetKind::Port | FacetKind::Anchor | FacetKind::Connection | FacetKind::Transform2d);
        // A deleted node is read as it was when it went (`Docs::gone`).
        let gone = &self.docs.gone;
        // What grouping and validation read of structure, as of their last
        // run (`solve::grouping_reach`); unknown before one, when every
        // structural change makes them look again.
        let reach = self.reactive.native_extra(graph::NativeKey::Clusters).and_then(|e| e.downcast_ref::<solve::Grouping>()).map(|g| &g.reach);
        let grouping_kind = |k: FacetKind| matches!(k, FacetKind::Constraint | FacetKind::Anchor | FacetKind::Port | FacetKind::Connection);
        let in_reach = |n: NodeId| reach.is_none_or(|r| r.contains(&n));
        let mut regroup = false;
        // The nodes a move took in or out of the grouping's reach: the
        // clusters beneath them read a changed chain of ancestors.
        let mut moved: Vec<NodeId> = Vec::new();
        let made_of = |id: NodeId| match gone.get(&id) {
            Some(w) => matches!(w.ty.name(), "port" | "connection") || w.port_or_anchor,
            None => doc.node(id).is_some_and(|n| matches!(n.ty.name(), "port" | "connection")) || doc.facet(id, FacetKind::Port).is_some() || doc.facet(id, FacetKind::Anchor).is_some(),
        };
        for op in ops {
            match op {
                Op::Create { id, parent, .. } => {
                    shaped.insert(*id);
                    shaped.extend(*parent);
                }
                Op::Reparent { id, parent, .. } => {
                    reparented = true;
                    shaped.insert(*id);
                    shaped.extend(*parent);
                }
                Op::Delete { id } => {
                    shaped.insert(*id);
                    shaped.extend(gone.get(id).and_then(|w| w.parent).or_else(|| doc.live(*id).and_then(|n| n.parent)));
                }
                Op::Reorder { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => {
                    shaped.insert(*id);
                    shaped.extend(doc.live(*id).and_then(|n| n.parent));
                }
                Op::Link { rel } => {
                    shaped.insert(rel.from);
                    shaped.insert(rel.to);
                }
                Op::Unlink { .. } | Op::Set { .. } => {}
            }
            rederive |= match op {
                Op::Reparent { .. } | Op::Link { .. } | Op::Unlink { .. } => true,
                Op::AddFacet { facet, .. } => derives(facet.kind()),
                Op::RemoveFacet { kind, .. } => derives(*kind),
                // A created node is live: read it, not what an earlier node
                // with its id was when it went.
                Op::Create { id, ty, facets, .. } => matches!(ty.name(), "port" | "connection") || facets.iter().any(|f| matches!(f.kind(), FacetKind::Port | FacetKind::Anchor)) || doc.facet(*id, FacetKind::Port).is_some() || doc.facet(*id, FacetKind::Anchor).is_some(),
                Op::Delete { id } => made_of(*id),
                Op::Set { path, .. } => matches!(path.facet, FacetKind::Port | FacetKind::Anchor | FacetKind::Connection),
                Op::Reorder { .. } => false,
            };
            match op {
                Op::Create { id, .. } | Op::Delete { id } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } | Op::Set { id, .. } | Op::Reorder { id, .. } => {
                    nodes.insert(*id);
                }
                Op::Reparent { id, .. } => {
                    nodes.insert(*id);
                    under.push(*id);
                    // A let moved provides to its new siblings' readers.
                    for n in std::iter::once(*id).chain(doc.descendants(*id)) {
                        if provides_any(n) {
                            match name_of(n) {
                                Some(x) => {
                                    names.insert(x);
                                }
                                None => rescope_all = true,
                            }
                        }
                    }
                }
                Op::Link { rel } => {
                    nodes.insert(rel.from);
                    nodes.insert(rel.to);
                }
                Op::Unlink { .. } => {}
            }
            regroup |= match op {
                // A copy made again may be an operand that went.
                Op::Create { id, ty, facets, .. } => in_reach(*id) || matches!(ty.name(), "constraint" | "port" | "connection") || facets.iter().any(|f| grouping_kind(f.kind())),
                Op::AddFacet { id, facet } => grouping_kind(facet.kind()) || in_reach(*id),
                Op::RemoveFacet { id, kind } => grouping_kind(*kind) || in_reach(*id),
                Op::Delete { id } | Op::Set { id, .. } => in_reach(*id),
                Op::Reparent { id, .. } => {
                    if in_reach(*id) {
                        moved.push(*id);
                    }
                    in_reach(*id)
                }
                Op::Link { rel } => matches!(rel.rel, RelKind::Constrains | RelKind::Connects | RelKind::Exposes) || in_reach(rel.from) || in_reach(rel.to),
                // Its relation is gone; the inverse link names its ends.
                Op::Unlink { id } => doc.relation(*id).is_none_or(|r| in_reach(r.from) || in_reach(r.to)),
                Op::Reorder { .. } => false,
            };
            // What can move a provider (`@name`): a let or a token made,
            // removed or renamed, a provide flag set.
            let touches = match op {
                Op::Reorder { id, .. } => provides(doc, *id).then_some(*id),
                Op::Create { id, ty, .. } => matches!(ty.name(), "let" | "token").then_some(*id),
                Op::Delete { id } => gone.get(id).map(|w| w.ty).or_else(|| doc.node(*id).map(|n| n.ty)).is_some_and(|ty| matches!(ty.name(), "let" | "token")).then_some(*id),
                Op::Set { id, path, slot } => {
                    // A rename: the new name here, the old one in the
                    // inverse op, which comes through as well.
                    if path.facet == FacetKind::Name
                        && let Some(n) = slot.constant().and_then(|v| v.as_str())
                    {
                        names.insert(n.to_owned());
                    }
                    names_context(path).then_some(*id)
                }
                Op::AddFacet { id, facet } => matches!(facet.kind(), FacetKind::Let | FacetKind::Name).then_some(*id),
                Op::RemoveFacet { id, kind } => matches!(kind, FacetKind::Let | FacetKind::Name).then_some(*id),
                _ => None,
            };
            if let Some(n) = touches {
                // The name it has now; a node gone may have none to read.
                match name_of(n) {
                    Some(x) => {
                        names.insert(x);
                    }
                    None => {
                        if !matches!(op, Op::Set { path, .. } if path.facet == FacetKind::Name) {
                            rescope_all = true;
                        }
                    }
                }
            }
        }
        let reach_unknown = reach.is_none();
        // A boundary check on an instance's argument reads its definition's
        // port_specs (`types::declared`): an edit inside a definition
        // reaches the arguments of every entity naming it.
        let defs: BTreeSet<NodeId> = nodes.iter().filter(|n| !n.is_expanded()).filter_map(|&n| expand::definition_of(doc, n)).collect();
        if !defs.is_empty() {
            let entity = TypeId::lookup("entity").expect("registered");
            let naming: Vec<NodeId> = doc
                .nodes_of_type(entity)
                .filter(|&e| doc.is_live(e) && doc.get::<facet::Instance>(e).and_then(|i| i.def.constant()).and_then(|v| v.as_ref()).is_some_and(|d| defs.contains(&d)))
                .collect();
            nodes.extend(naming);
        }
        // The network's islands and the layout natives first, so that this
        // pass runs the new ones and not the old against the new structure
        // (handoff §4.10); and the layouts whose structure changed run.
        let (net, fresh) = self.network_now();
        if fresh {
            self.sync_islands(&net, false);
        }
        self.layout_touched.extend(shaped.iter().copied());
        self.sync_layout(false);
        self.sync_repeats(false);
        self.prime_layout(&shaped);
        // What the ops wrote reaches the keyed cells that read it, now that
        // layout's outputs are reconciled (structure-in-the-graph.md §3.3).
        self.drain_touched();
        // Constraints and their operands are structure to the grouping
        // and to validation: they look again when an op touched what they
        // read (DS-3b: not on every structural change). A reparent can move
        // an operand's ancestry without touching it (a wrapper swapped):
        // the clusters beneath what moved look again.
        if regroup {
            self.reactive.prime_natives([graph::NativeKey::Clusters]);
        }
        if reparented && !moved.is_empty() {
            let doc = Tree::of(&self.docs);
            let keys: BTreeSet<graph::NativeKey> = if reach_unknown {
                self.reactive.native_keys().filter(|k| matches!(k, graph::NativeKey::Cluster(_))).collect()
            } else {
                moved.iter().flat_map(|&m| std::iter::once(m).chain(doc.descendants(m))).filter_map(|n| self.clusters.by_node.get(&n)).map(|&k| graph::NativeKey::Cluster(k)).collect()
            };
            self.reactive.prime_natives(keys);
        }
        // What waited on the leaves of every node gone, subtrees included,
        // goes with them: nothing is promoted onto a deleted leaf (review
        // of DS-10a, Sol). A lookup in the waiting requests, which are few.
        self.reactive.forget_waiting(self.docs.gone.keys().copied());
        let rescope = if rescope_all {
            graph::Rescope::All
        } else if names.is_empty() && under.is_empty() {
            graph::Rescope::No
        } else {
            graph::Rescope::Some { names, under }
        };
        let tokens = self.token_leaves();
        self.reactive.restructure(Tree::of(&self.docs), &nodes, rescope, rederive, &tokens);
    }

    /// The theme's tokens by name, as cells read them: kept until a token
    /// changes (`Docs::tokens_shaped`), not gathered on every restructure.
    fn token_leaves(&mut self) -> std::sync::Arc<BTreeMap<String, Leaf>> {
        match &self.token_leaves {
            Some((at, t)) if *at == self.docs.tokens_shaped => t.clone(),
            _ => {
                let t = std::sync::Arc::new(palette::leaves(Tree::of(&self.docs)));
                self.token_leaves = Some((self.docs.tokens_shaped, t.clone()));
                t
            }
        }
    }

    /// Store what an accepted expansion found.
    /// Record what `ops` changed, with `inverse`, in the pending changes,
    /// with the deleted nodes' places as they were when they went
    /// (`Docs::gone`, kept until the changes are taken: a later delete of
    /// the same id replaces its entry).
    fn note_changes<'a>(&mut self, ops: impl IntoIterator<Item = &'a Op>, inverse: &[Op]) {
        self.pending.note(Tree::of(&self.docs), &self.docs.gone, ops, inverse);
    }

    fn store_found(&mut self, delta: engine::owners::Delta) {
        self.owners.apply(delta);
        self.expansion_problems = self.owners.problems();
    }

    /// Under the differential harness with `IRON_OWNER_CHECK=1`: the owners'
    /// copies equal what the whole-document expansion would make, checked
    /// after each owner update (handoff §4.4, stage 2).
    #[cfg(feature = "differential")]
    fn check_owners(&self, after: &str) {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*ON.get_or_init(|| std::env::var_os("IRON_OWNER_CHECK").is_some()) {
            return;
        }
        if let Err(e) = expand::parity(Tree::of(&self.docs), &mut self.owners.templates().clone(), &self.repeats) {
            panic!("templates and the old expansion differ after {after}: {e}");
        }
        let mut whole = self.docs.clone();
        let stamp = whole.authored.version();
        let (mut ops, _) = expand::expand(&mut whole, stamp, &self.repeats);
        // A leaf the solver writes on a copy a cluster holds keeps the
        // solved value, which expansion cannot know (`apply_wanted` leaves
        // it; `post_apply` lets go of a copy when it leaves every cluster).
        ops.retain(|op| !matches!(op, Op::Set { id, path, .. } if solve::writes(*path) && self.owners.held().contains(id)));
        assert!(ops.is_empty(), "owners missed what the whole expansion makes after {after}: {ops:?}");
    }

    #[cfg(not(feature = "differential"))]
    fn check_owners(&self, _after: &str) {}

    /// Keep what `run` applied.
    fn commit(&mut self, r: Rollback) {
        r.take();
        self.docs.commit();
    }

    pub fn new() -> StoreCore {
        StoreCore::with_document(Document::new())
    }

    /// A store over an authored document: it derives every copy itself.
    /// Panics on a document holding derived content (`derived_content`);
    /// `load` reports the same as an error.
    pub fn with_document(doc: Document) -> StoreCore {
        if let Some(why) = doc.derived_content() {
            panic!("with_document takes an authored document: {why}");
        }
        StoreCore::build(doc, true)
    }

    /// A store over `doc` with copies kept in it, as phase 2b kept them:
    /// the differential reference for 2c (handoff §4.6).
    #[cfg(feature = "differential")]
    pub(crate) fn with_copies_in_document(doc: Document) -> StoreCore {
        StoreCore::build(doc, false)
    }

    /// A store over `doc`, keeping copies apart from it or not (handoff
    /// §4.6).
    pub(crate) fn build(doc: Document, apart: bool) -> StoreCore {
        let mut store = StoreCore {
            docs: tree::Docs::new(doc, apart),
            owners: Default::default(),
            history: History {
                horizon: DEFAULT_HORIZON,
                ..Default::default()
            },
            recent_keys: VecDeque::new(),
            reactive: graph::Reactive::default(),
            time: 0.0,
            time_errors: Default::default(),
            clusters: Default::default(),
            committed: BTreeSet::new(),
            grouped_at: None,
            drivers: Default::default(),
            fingers: Vec::new(),
            session_version: 0,
            machines_unsettled: None,
            contexts: None,
            layout_preview: None,
            dark: false,
            motion: Motion::default(),
            machines: machine::Player::default(),
            steps: step::Steps::default(),
            running_steps: Default::default(),
            machines_full: true,
            running_clips: Vec::new(),
            animated: BTreeSet::new(),
            clips_end: None,
            theme_at: None,
            token_leaves: None,
            queries: query::Queries::default(),
            presence: Default::default(),
            presence_moving: BTreeSet::new(),
            presence_at: None,
            clocks: clocks::Clocks::default(),
            advanced_at: None,
            evaluated_at: 0,
            advanced: (BTreeSet::new(), Vec::new()),
            observers: Default::default(),
            next_observer: 0,
            observe_all: true,
            interaction: BTreeMap::new(),
            expansion_problems: Vec::new(),
            repeats: Default::default(),
            repeat_inputs: BTreeMap::new(),
            drawn: None,
            repeat_dirty: BTreeSet::new(),
            repeat_problems: BTreeMap::new(),
            item_shown: BTreeMap::new(),
            repeat_natives: Default::default(),
            item_memo: Default::default(),
            network: None,
            layout_roots: None,
            pinned_by_use: None,
            layout_touched: BTreeSet::new(),
            glide_nodes: BTreeMap::new(),
            session_state: BTreeMap::new(),
            emitted: Vec::new(),
            truncated: Vec::new(),
            feedback: Vec::new(),
            pending: Changes::default(),
            settling: false,
            repeat_changes: BTreeMap::new(),
            restless: BTreeSet::new(),
            gesture: None,
            ended: None,
            next_gesture: 1,
            geometry: geom::Revisions::default(),
            producers: Default::default(),
            text_generation: iron_text::engine().generation(),
        };
        // What writes touch is handed to the graph from here on (§3.3 of
        // structure-in-the-graph.md): a load is a clean baseline.
        store.docs.log_touched();
        // A loaded document holds only authored nodes: derive its instances.
        let stamp = store.docs.authored.version();
        let (_, delta) = store.owners.expand(&mut store.docs, stamp, &BTreeMap::new(), engine::owners::Which::All);
        store.store_found(delta);
        store.geometry.note_structure();
        store.reactive.rebuild(Tree::of(&store.docs));
        store.resample();
        store.settle_repeats();
        store.pending = Changes::default();
        store.docs.gone.clear();
        store.rebuild_clusters();
        store.resolve_physics();
        store.drain_touched();
        store
    }

    /// The constraint clusters as last built, in solve order.
    pub fn clusters(&self) -> &solve::Clusters {
        &self.clusters
    }

    fn rebuild_clusters(&mut self) {
        self.regroup();
    }

    /// Under the differential check: the grouping not run since it was last
    /// taken (DS-3b: no structural op touched its reach) is what grouping
    /// from scratch makes now.
    #[cfg(feature = "differential")]
    fn check_grouping(&self) {
        let fresh = solve::clusters(Tree::of(&self.docs), &self.reactive, self.contexts());
        assert!(fresh == self.clusters, "the grouping was not run, but grouping from scratch differs:\n kept {:?}\n fresh {:?}", self.clusters.list.iter().map(|c| (c.id, &c.nodes)).collect::<Vec<_>>(), fresh.list.iter().map(|c| (c.id, &c.nodes)).collect::<Vec<_>>());
    }

    /// Take the constraints as grouped now into `clusters`: the grouping
    /// native's result, made again only when what it read changed (handoff
    /// §4.10, RE-5.5), and taken only when it ran since it was last taken
    /// (DS-3b: an unchanged grouping costs nothing). Grouping is total: what
    /// it can't use or solve is in `clusters.problems` (DS-7).
    fn regroup(&mut self) {
        if self.reactive.native(graph::NativeKey::Clusters).is_none() {
            self.reactive.set_natives(Tree::of(&self.docs), vec![(graph::NativeKey::Clusters, Some((std::sync::Arc::new(solve::ClustersNative::default()), true)))], true);
        }
        // A claim that came or went on an operand waits to run it.
        self.reactive.flush(Tree::of(&self.docs));
        let runs = self.reactive.runs_of(graph::NativeKey::Clusters);
        if runs.is_some() && runs == self.grouped_at {
            #[cfg(feature = "differential")]
            self.check_grouping();
            return;
        }
        let c = match self.reactive.native_extra(graph::NativeKey::Clusters).and_then(|e| e.downcast_ref::<solve::Grouping>()) {
            Some(g) => g.result.clone(),
            None => solve::clusters(Tree::of(&self.docs), &self.reactive, self.contexts()),
        };
        self.grouped_at = runs;
        if c != self.clusters {
            self.owners.hold(c.copies().clone());
            // `committed` names current clusters only: `resolve_clusters_live`
            // counts on it.
            self.committed.retain(|k| c.at.contains_key(k));
            // A cluster in a cycle is never solved: count it as posed, so
            // `to_solve` doesn't look at every cluster for it (review of
            // DS-7, Opus).
            self.committed.extend(c.failed.iter().copied());
            self.clusters = c;
            self.sync_cluster_natives();
        }
    }

    /// The first cluster in solve order whose solve could move something
    /// and that this pass hasn't solved, asked again after each solve,
    /// with the grouping taken again first: a cluster's write can wake a
    /// later one that reads it (council review of DS-9, Sol), or make a
    /// cluster cyclic, which is then not solved (review of DS-7, Sol).
    fn next_to_solve(&mut self, drivers: &[solve::Driver], solved: &BTreeMap<NodeId, usize>) -> Option<solve::Cluster> {
        self.regroup();
        // Once each; again only if what it reads changed since (its
        // watcher ran), as through a layout another cluster's write moved
        // (review of DS-7, Sol), up to `RESOLVES` times.
        self.to_solve(drivers)
            .into_iter()
            .map(|i| &self.clusters.list[i])
            .find(|c| match solved.get(&c.key()) {
                None => true,
                Some(&n) => n < RESOLVES && self.reactive.ran(graph::NativeKey::Cluster(c.key())),
            })
            .cloned()
    }

    /// The clusters, in solve order, whose solve could move something: a
    /// driver is one of their variables or a finger holds one, their
    /// watching native ran since their last committed solve, or they have
    /// none. The rest are as their last solve left them (`unmoved`), found
    /// by index, not by a pass over every cluster (DS-3a, DS-3b).
    fn to_solve(&self, drivers: &[solve::Driver]) -> Vec<usize> {
        let mut keys: BTreeSet<NodeId> = self.reactive.ran_clusters().collect();
        keys.extend(self.fingers.iter().filter_map(|f| self.clusters.by_node.get(&f.node).copied()));
        keys.extend(drivers.iter().filter_map(|d| self.clusters.by_node.get(&d.leaf.node).copied()));
        if self.committed.len() < self.clusters.list.len() {
            keys.extend(self.clusters.at.keys().filter(|k| !self.committed.contains(k)).copied());
        }
        // A cluster in a cycle through bindings is never solved (DS-7).
        let mut order: Vec<usize> = keys.iter().filter(|k| !self.clusters.failed.contains(k)).filter_map(|k| self.clusters.at.get(k).copied()).collect();
        order.sort_unstable();
        #[cfg(feature = "differential")]
        for (i, c) in self.clusters.list.iter().enumerate() {
            assert!(order.contains(&i) || self.clusters.failed.contains(&c.key()) || self.unmoved(c, drivers), "cluster {} could move but is not solved", c.id);
        }
        order
    }

    /// A watching native per cluster (`solve::ClusterNative`): made for a
    /// cluster that is new or changed, dropped for one gone.
    fn sync_cluster_natives(&mut self) {
        let want: BTreeMap<graph::NativeKey, &solve::Cluster> = self.clusters.list.iter().map(|c| (graph::NativeKey::Cluster(c.key()), c)).collect();
        let mut batch: Vec<(graph::NativeKey, Option<(std::sync::Arc<dyn graph::Native>, bool)>)> = self
            .reactive
            .native_keys()
            .filter(|k| matches!(k, graph::NativeKey::Cluster(_)) && !want.contains_key(k))
            .map(|k| (k, None))
            .collect();
        for (k, c) in &want {
            let n = solve::ClusterNative::new((*c).clone());
            if self.reactive.native(*k).and_then(|x| x.as_any().downcast_ref::<solve::ClusterNative>()) != Some(&n) {
                batch.push((*k, Some((std::sync::Arc::new(n), true))));
            }
        }
        if !batch.is_empty() {
            self.reactive.set_natives(Tree::of(&self.docs), batch, true);
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
        // The clusters that could have moved: their watching native ran,
        // a finger holds them, or they have not been committed; the rest
        // are as their last solve left them (DS-3a: no pass over all).
        let mut solved = BTreeMap::new();
        while let Some(c) = self.next_to_solve(&[], &solved) {
            *solved.entry(c.key()).or_insert(0) += 1;
            let c = &c;
            if self.unmoved(c, &[]) {
                continue;
            }
            let result = {
                let read = |l: Leaf| self.reactive.read(Tree::of(&self.docs), l);
                solve::solve_cluster(Tree::of(&self.docs), &read, c, &[], &self.fingers)
            };
            report.clusters += 1;
            report.vars += c.vars.len();
            report.iterations += result.iterations;
            report.evaluations += result.evaluations;
            if !solve::holds(&result) {
                // Keep the previous pose rather than show a jump (CS §5.5),
                // and pose it, so that only a change to its inputs tries
                // again, not every frame (review of DS-7, Sol).
                report.converged = false;
                self.pose(c);
                continue;
            }
            for (leaf, v) in result.values {
                let value = Value::Number(v);
                if self.reactive.read(Tree::of(&self.docs), leaf) != Some(value.clone()) {
                    self.reactive.preview(Tree::of(&self.docs), leaf, value);
                }
            }
            // Its own previews ran its watcher: not a reason to solve it
            // again in this pass (a change that comes back through another
            // cluster's write does mark it again). A preview is not
            // committed: the next transaction still solves and writes it.
            self.reactive.clear_ran(graph::NativeKey::Cluster(c.key()));
            self.committed.remove(&c.key());
        }
        report
    }

    /// Whether cluster `c` can be left as it is: no driver or finger moves
    /// it, and everything its solve could read is as its last committed
    /// solve left it (§9.4, A6: such a solve returns the pose it starts
    /// from). Only `post_apply`'s solves record what they left (`pose`): a
    /// live solve's answer is a preview, which the next commit must still
    /// write. Under the owners' check, a cluster left is solved anyway and
    /// must come out where it is.
    fn unmoved(&self, c: &solve::Cluster, drivers: &[solve::Driver]) -> bool {
        if drivers.iter().any(|d| c.vars.contains(&d.leaf)) || self.fingers.iter().any(|f| c.nodes.contains(&f.node)) {
            return false;
        }
        // Its watching native ran since its last committed solve:
        // something of it changed (RE-5.5).
        if !self.committed.contains(&c.key()) || self.reactive.ran(graph::NativeKey::Cluster(c.key())) {
            return false;
        }
        #[cfg(feature = "differential")]
        self.check_unmoved(c);
        true
    }

    /// Record what cluster `c`'s converged solve left for its next one to
    /// compare with (`unmoved`).
    fn pose(&mut self, c: &solve::Cluster) {
        self.committed.insert(c.key());
        self.reactive.clear_ran(graph::NativeKey::Cluster(c.key()));
    }

    /// Under the owners' check: a cluster left unsolved solves to where it is.
    #[cfg(feature = "differential")]
    fn check_unmoved(&self, c: &solve::Cluster) {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*ON.get_or_init(|| std::env::var_os("IRON_OWNER_CHECK").is_some()) {
            return;
        }
        let doc = Tree::of(&self.docs);
        let read = |l: Leaf| self.reactive.read(doc, l);
        // A cluster whose last solve failed keeps a pose its constraints
        // don't hold at; solving it again may well move it (DS-7).
        if !solve::violated(doc, &read, c).is_empty() {
            return;
        }
        let result = solve::solve_cluster(doc, &read, c, &[], &self.fingers);
        for (leaf, v) in result.values {
            let now = read(leaf).and_then(|x| x.as_f64());
            assert!(now.is_some_and(|n| (n - v).abs() < 1e-6), "cluster {} was left unsolved, but solving it moves {leaf} from {now:?} to {v}", c.id);
        }
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
                let num = |s: &StoreCore, f: &str, d: f64| s.resolved(h.node, t2(f)).and_then(|v| v.as_f64()).unwrap_or(d);
                let parent = match Tree::of(&self.docs).live(h.node).and_then(|n| n.parent) {
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
        let read = |l: Leaf| self.reactive.read(Tree::of(&self.docs), l);
        plug::port_anchors(Tree::of(&self.docs), &read)
    }

    /// Remove a connection, as one transaction: the far ends that followed
    /// it stay where they are (their positions set, as overrides inside an
    /// instance) rather than return to their template positions.
    pub fn unplug(&mut self, connection: NodeId, label: impl Into<String>, actor: Actor) -> Result<Version, String> {
        let t2 = |f: &str| LeafPath::new(FacetKind::Transform2d, f).expect("declared");
        let ends = port::ends(Tree::of(&self.docs), connection).ok_or_else(|| format!("{connection} is not a connection"))?;
        let pins: Vec<(NodeId, f64, f64)> = self
            .port_anchors()
            .into_iter()
            .filter(|a| a.follow && (a.outer == ends.0 || a.outer == ends.1 || a.port == ends.0 || a.port == ends.1))
            .filter_map(|a| Some((a.anchor, self.resolved(a.anchor, t2("x"))?.as_f64()?, self.resolved(a.anchor, t2("y"))?.as_f64()?)))
            .collect();
        let mut ops: Vec<Op> = Tree::of(&self.docs).relations_from(connection).map(|r| Op::Unlink { id: r.id }).collect();
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
        self.port_anchors().into_iter().filter(|t| plug::compatible(Tree::of(&self.docs), &p.from, t).is_ok()).collect()
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
        let fold = |s: &mut StoreCore, last: &mut Option<Version>, now: Version| {
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
            let id = self.docs.authored.next_id();
            let rels = self.docs.authored.next_relation_ids(2);
            // Two loose ends: the one landed on holds the joint.
            let held = (p.from.follow && t.follow).then_some(t.outer);
            let ops = plug::plug_ops(Tree::of(&self.docs), p.from.outer, t.outer, held, id, [rels[0], rels[1]])?;
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
        self.resample_running();
        self.pipeline();
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
                let r = self.docs.authored.next_session_ids(1)[0];
                ops.push(Op::Create { id: r, ty: ty("session"), parent: None, order: OrderKey::first(), facets: vec![] });
                r
            }
        };
        let id = *self.docs.authored.next_session_ids(ops.len() + 1).last().expect("one id");
        let mut clock = FacetKind::Clock.default_facet();
        let _ = clock.set("t", Slot::Const(Value::Number(self.time)));
        let doc = Tree::of(&self.docs);
        let last = doc.children(root).iter().filter_map(|&n| doc.live(n)).map(|n| n.order.clone()).max();
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
        let Some(Value::Number(t)) = self.reactive.read(Tree::of(&self.docs), leaf) else { return false };
        if t == self.time || !t.is_finite() {
            return false;
        }
        self.time = t;
        self.resample_running();
        true
    }

    /// Sample every clip and publish what they drive: after the
    /// structure changed (a clip, a key or a link made or gone).
    fn resample(&mut self) {
        let sample = time::sample(Tree::of(&self.docs), self.time);
        self.time_errors = sample.errors;
        self.reactive.publish(Tree::of(&self.docs), ProducerId::CLIPS, sample.values);
    }

    /// Sample the clips animating something on a running clock (as the
    /// gate last decided), publishing what changed: the playhead moving, a
    /// constant set. A clip on a paused page holds its value until its page
    /// runs again, when the gate decides again and every clip is sampled
    /// (DS-2).
    fn resample_running(&mut self) {
        // Before the gate has decided, and with everything observed, every
        // clip runs.
        if self.observe_all || self.clocks.gated.is_none() {
            return self.resample();
        }
        let clips = self.running_clips.clone();
        let sample = time::sample_clips(Tree::of(&self.docs), self.time, clips.iter().copied());
        for c in &clips {
            self.time_errors.remove(c);
        }
        self.time_errors.extend(sample.errors);
        // What a running clip drives but gave nothing for now (before its
        // first key) goes back to what it is without the clip (council
        // review of DS-2).
        // A leaf another clip drives, one on a paused page holding its
        // value, is that clip's to give or withdraw: the clips publish as
        // one producer, so withdrawing it would drop the held value.
        let doc = Tree::of(&self.docs);
        let others = |l: &Leaf| {
            doc.relations_to(l.node).any(|r| {
                r.rel == RelKind::Animates
                    && !clips.contains(&r.from)
                    && doc.get::<facet::Clip>(r.from).and_then(|c| c.prop.constant()).and_then(|v| v.as_str()).and_then(|p| p.parse::<facet::LeafPath>().ok()) == Some(l.path)
            })
        };
        let gone: Vec<Leaf> = sample.targets.iter().filter(|l| !sample.values.contains_key(l) && !others(l)).copied().collect();
        if !sample.values.is_empty() || !gone.is_empty() {
            self.reactive.publish_delta(Tree::of(&self.docs), ProducerId::CLIPS, sample.values, &gone);
        }
    }

    /// Whether `ops` change a clip, a key, a timing or a track: what every
    /// clip's sample reads. A playhead move does not.
    fn touches_clips<'a>(ops: impl IntoIterator<Item = &'a Op>) -> bool {
        let kinds = [FacetKind::Clip, FacetKind::Key, FacetKind::Timing, FacetKind::Track];
        ops.into_iter().any(|op| match op {
            Op::Set { path, .. } => kinds.contains(&path.facet),
            Op::AddFacet { facet, .. } => kinds.contains(&facet.kind()),
            Op::RemoveFacet { kind, .. } => kinds.contains(kind),
            _ => false,
        })
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
        self.reactive.publish(Tree::of(&self.docs), producer, values);
        self.pipeline();
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
        self.reactive.read(Tree::of(&self.docs), Leaf { node: id, path })
    }

    /// Whether `take_changes` would return anything: for a consumer that
    /// polls without taking (a cache of overlay geometry, the sync's early
    /// out).
    pub fn changes_pending(&self) -> bool {
        !self.pending.is_empty() || self.reactive.has_pending() || self.geometry.has_pending()
    }

    /// What moved since the last take (reactive-engine.md §12.2): one set
    /// for every consumer, taken once. A host with several consumers keeps
    /// what each has not yet used, so none takes what another needs.
    pub fn take_changes(&mut self) -> Changes {
        let mut c = std::mem::take(&mut self.pending);
        self.docs.gone.clear();
        c.values = self.reactive.take_changed();
        c.marks = self.reactive.take_marks();
        c.geometry = self.geometry.take(Tree::of(&self.docs), self.reactive.take_moved());
        c.geometry.add_parents(&c.structure);
        c.revisions = changes::Revisions { authored: self.docs.authored.version(), session: self.session_version };
        c.sorted()
    }

    /// What `take_changes` would return now, without taking it: the
    /// differential snapshot's view of what is pending.
    #[cfg(feature = "differential")]
    pub(crate) fn peek_changes(&self) -> Changes {
        let mut c = self.pending.clone();
        let (values, marks, touched) = self.reactive.peek();
        c.values = values;
        c.marks = marks;
        c.geometry = self.geometry.classify(Tree::of(&self.docs), touched);
        c.geometry.add_parents(&c.structure);
        c.revisions = changes::Revisions { authored: self.docs.authored.version(), session: self.session_version };
        c.sorted()
    }

    /// The node's local space to document 2D space, through every live
    /// ancestor, reading resolved values (previews, bindings, clips). Equal
    /// to the materialised entity's `GlobalTransform`; see `geom`.
    pub fn world_affine(&self, id: NodeId) -> Affine2 {
        geom::world_affine(Tree::of(&self.docs), &|l| self.reactive.read(Tree::of(&self.docs), l), id)
    }

    /// The node's local 3D space to its composition's (`geom::world_affine3`).
    pub fn world_affine3(&self, id: NodeId) -> geom::Affine3 {
        geom::world_affine3(Tree::of(&self.docs), &|l| self.reactive.read(Tree::of(&self.docs), l), id)
    }

    /// The node's own transform in its parent's space, from resolved values.
    pub fn local_affine(&self, id: NodeId) -> Affine2 {
        geom::local_affine(Tree::of(&self.docs), &|l| self.reactive.read(Tree::of(&self.docs), l), id)
    }

    /// A node's extent in its parent's space: its own bounds and its
    /// children's, through its transform.
    pub fn extent(&self, id: NodeId) -> Option<kurbo::Rect> {
        let a = self.local_affine(id).0;
        let to_parent = kurbo::Affine::new([a[0], a[1], a[2], a[3], a[4], a[5]]);
        let own = self.local_bounds(id).map(|b| b.geometry);
        let kids = Tree::of(&self.docs).children(id).iter().filter(|c| !c.is_expanded() || id.is_expanded() || Tree::of(&self.docs).live(id).is_some_and(|n| n.ty.name() == "use")).filter_map(|&c| self.extent(c)).reduce(|x, y| x.union(y));
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
    /// as of the last `take_changes`.
    pub fn revisions(&self) -> &geom::Revisions {
        &self.geometry
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
        let ids: Vec<NodeId> = Tree::of(&self.docs).nodes_of_type(text).filter(|&n| Tree::of(&self.docs).is_live(n)).collect();
        for id in &ids {
            let leaf = Leaf { node: *id, path };
            let value = self.reactive.read(Tree::of(&self.docs), leaf).unwrap_or(Value::Str(String::new()));
            self.reactive.mark(leaf, value);
        }
        // New faces measure differently: layouts holding text reflow.
        self.reactive.set_input(Tree::of(&self.docs), graph::InputKey::Fonts, Some(std::sync::Arc::new(generation)));
        self.relayout();
        !ids.is_empty()
    }

    /// Preview a leaf's value for the duration of a gesture. Bindings that
    /// read it re-evaluate now; nothing is written to the document until
    /// [`Store::commit_gesture`].
    pub fn preview(&mut self, id: NodeId, path: LeafPath, value: Value) {
        self.watch(id);
        let leaf = Leaf { node: id, path };
        self.drivers.insert(leaf);
        self.reactive.preview(Tree::of(&self.docs), leaf, value);
        self.sync_clock();
        self.pipeline();
    }

    /// Withdraw one leaf's preview (it reads as the document again).
    pub fn unpreview(&mut self, id: NodeId, path: LeafPath) {
        let leaf = Leaf { node: id, path };
        self.drivers.remove(&leaf);
        self.reactive.unpreview(Tree::of(&self.docs), leaf);
        let clocked = self.sync_clock();
        self.pipeline_from(clocked);
    }

    /// How a gesture on this leaf would be realised right now: directly, by
    /// inverting a linear binding onto its one constant source, or not at
    /// all (see `handle`).
    pub fn handle(&self, id: NodeId, path: LeafPath) -> Handle {
        handle::handle(Tree::of(&self.docs), &self.reactive, Leaf { node: id, path })
    }

    /// Preview a *target* value for a leaf through its handle: a constant is
    /// previewed as is; a linearly bound leaf previews its source so the
    /// binding re-evaluates to `value`. `Err` with the reason when locked.
    pub fn preview_target(&mut self, id: NodeId, path: LeafPath, value: Value) -> Result<(), String> {
        let leaf = Leaf { node: id, path };
        let (handle, target) = self.current(|s| (s.handle(id, path), handle::resolve_target(Tree::of(&s.docs), &s.reactive, leaf, value.clone())));
        // A view that moves its subject rigidly: both leaves by the same
        // amount, each held in range, so a slice pushed against the end of
        // its transition shortens there rather than stopping (`handle::rigid`).
        if let (Some((with, (lo, hi))), Handle::Linear { source, a, b }) = (handle::rigid(Tree::of(&self.docs), id), handle)
            && let Some(y) = value.as_f64()
        {
            let authored = |l: Leaf| Tree::of(&self.docs).leaf(l.node, l.path).and_then(|s| s.constant()).and_then(Value::as_f64);
            if let Some(s0) = authored(source) {
                let d = (y - b) / a - s0;
                let moved: Vec<(Leaf, f64)> = std::iter::once(source).chain(with.into_iter().filter(|l| *l != source)).filter_map(|l| Some((l, authored(l)?))).collect();
                for (l, v) in moved {
                    self.preview(l.node, l.path, Value::Number((v + d).clamp(lo, hi)));
                }
                return Ok(());
            }
        }
        let (target, v) = target?;
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
        self.gesture = Some(gesture::ActiveGesture { id, nodes: nodes.iter().copied().collect(), unkeyed: false });
        id
    }

    /// Have the running gesture commit its leaves as themselves, never as
    /// keys, whatever auto-keyframe says: a drag in a view whose context
    /// sets `animate.keys="off"` (the timeline, where a drag edits the
    /// animation itself: a key's time, a clip's start).
    pub fn unkeyed_gesture(&mut self) {
        if let Some(g) = &mut self.gesture {
            g.unkeyed = true;
        }
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
        // What the previews moved is derived again from the document: a
        // repeat's items, layout, the network.
        let previewed = !self.reactive.overlay().is_empty();
        self.reactive.clear_overlay(Tree::of(&self.docs));
        // A previewed playhead goes back to the clock's own `t`.
        if self.sync_clock() || previewed {
            self.pipeline();
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
        self.reactive.clear_overlay(Tree::of(&self.docs));
        Some(suspended)
    }

    /// Put suspended previews back on the document as it now is.
    fn resume_gesture(&mut self, s: gesture::Suspended) {
        self.drivers = s.drivers;
        self.fingers = s.fingers;
        // A preview of a node that went is dropped with it.
        for (leaf, v) in s.values {
            let doc = Tree::of(&self.docs);
            if doc.is_live(leaf.node) && doc.leaf(leaf.node, leaf.path).is_some() {
                self.reactive.preview(Tree::of(&self.docs), leaf, v);
            }
        }
        self.pipeline();
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
        self.history.past.push(Transaction { version: b.version, label: label.into(), actor: b.actor, ts: b.ts, idempotency_key: None, ops, derived_ops: b.derived_ops, inverse, solver: b.solver });
        true
    }

    /// Set a leaf to `value` through its handle, as one transaction: a
    /// constant directly, a linearly bound leaf through its source. For
    /// one-off edits (an inspector field); a running gesture is unaffected
    /// unless the edit conflicts with it. `Ok(None)` when nothing changes.
    pub fn set_through_handle(&mut self, id: NodeId, path: LeafPath, value: Value, label: impl Into<String>, actor: Actor, ts: u64) -> Result<Option<Applied>, String> {
        let leaf = Leaf { node: id, path };
        let (target, v) = self.current(|s| handle::resolve_target(Tree::of(&s.docs), &s.reactive, leaf, value.clone()))?;
        let same = match (Tree::of(&self.docs).leaf(target.node, target.path).and_then(|s| s.constant()), &v) {
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
        // by the visible mode (timeline.md §12.2), not their authored value,
        // unless the gesture's view records none.
        let unkeyed = gesture.as_ref().is_some_and(|g| g.unkeyed);
        // A preview of a node that went during the gesture (a copy its
        // owner's expansion removed) commits nothing: the node is not there
        // to write to, whether it is a tombstone or gone outright.
        let gone = |l: &Leaf| !Tree::of(&self.docs).is_live(l.node);
        let driven: Vec<(Leaf, Value)> = self
            .reactive
            .overlay()
            .iter()
            .filter(|_| !unkeyed)
            .filter(|(l, _)| !gone(l))
            .filter(|(l, _)| self.drivers.contains(l) && Tree::of(&self.docs).leaf(l.node, l.path).is_some_and(|s| s.expr().is_none()))
            .map(|(l, v)| (*l, v.clone()))
            .collect();
        let (key_ops, keyed) = timeline_view::key_ops(self, &driven);
        ops.extend(key_ops);
        // Leaves of facets the node lacks but allows: the facet is
        // added, defaults with the previewed fields set, before the `Set`s.
        let mut added: BTreeMap<(NodeId, FacetKind), Facet> = BTreeMap::new();
        for (leaf, value) in self.reactive.overlay() {
            if !self.drivers.contains(leaf) || keyed.contains(leaf) || gone(leaf) {
                continue;
            }
            let Some(slot) = Tree::of(&self.docs).leaf(leaf.node, leaf.path) else {
                let allowed = Tree::of(&self.docs).live(leaf.node).is_some_and(|n| n.ty.spec().allows(leaf.path.facet));
                if allowed && Tree::of(&self.docs).facet(leaf.node, leaf.path.facet).is_none() {
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
        self.reactive.clear_overlay(Tree::of(&self.docs));
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
        // A binding checked for cycles needs every edge as it is now; a
        // constant written (the playhead each frame) reads nothing.
        if !ops.iter().all(|op| matches!(op, Op::Set { slot: Slot::Const(_), .. })) {
            self.exact();
        }
        let stamp = self.docs.authored.version();
        let mut undo: Vec<Op> = Vec::new();
        let mut structural = false;
        // A failure part-way puts the document back exactly, through the
        // journal (reactive-engine handoff §4.4, stage 1).
        self.docs.begin();
        for (i, op) in ops.iter().enumerate() {
            let in_session = match op {
                Op::Create { id, parent, .. } => id.is_session() && parent.is_none_or(|p| p.is_session()),
                Op::Delete { id } | Op::Reparent { id, .. } | Op::Reorder { id, .. } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => id.is_session(),
                Op::Link { rel } => rel.id.is_session() && rel.from.is_session(),
                Op::Unlink { id } => id.is_session(),
            };
            let result = if in_session {
                op::apply_op(&mut self.docs, i, op, stamp)
            } else {
                Err(ApplyError { op: i, kind: ErrorKind::BadId, message: "not a session-tier op".into() })
            };
            match result {
                Ok(inverse) => undo.extend(inverse),
                Err(e) => {
                    self.docs.rollback();
                    return Err(e);
                }
            }
            structural |= match op {
                Op::Create { .. } | Op::Delete { .. } | Op::AddFacet { .. } | Op::RemoveFacet { .. } | Op::Reparent { .. } => true,
                Op::Set { id, path, slot } => slot.expr().is_some() || self.reactive.is_bound(Leaf { node: *id, path: *path }) || names_context(path),
                _ => false,
            };
        }
        // Session ops never need expansion: a derived id cannot sit under a
        // session-tier id (`NodeId::from_path`), so no session node derives
        // anything, and session ops touch only session nodes.
        self.docs.commit();
        // The spatial scene learns of session nodes as of any other: a
        // session composition built after the last transaction (the Latency
        // view's) is otherwise never indexed, so presses miss it.
        self.note_changes(ops.iter(), &undo);
        self.geometry.note_ops(ops.iter());
        if structural {
            self.regraph(ops.iter().chain(&undo));
        } else {
            self.prime_reorders(ops.iter());
            self.prime_grouping(ops.iter());
            self.drain_touched();
            let changed: Vec<Leaf> = ops.iter().filter_map(|op| match op { Op::Set { id, path, .. } => Some(Leaf { node: *id, path: *path }), _ => None }).collect();
            if !changed.is_empty() {
                // Report each written leaf as changed, as a preview does, so
                // the reconciler and the spatial scene see a session node
                // move even when nothing binds to it (the Latency view's
                // square, recentred by `engine_api::document_latency`).
                for &leaf in &changed {
                    if let Some(v) = self.reactive.read(Tree::of(&self.docs), leaf) {
                        self.reactive.mark(leaf, v);
                    }
                }
                self.reactive.invalidate(Tree::of(&self.docs), changed);
            } else {
                // A reorder sets no leaf: what its keys marked runs now.
                self.reactive.flush(Tree::of(&self.docs));
            }
        }
        self.session_version += 1;
        if self.sync_clock() {
            self.pipeline();
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
        // The drawn repeats are kept as structure changes (`update_drawn`);
        // a repeat is read again when its native ran (an input changed) or
        // it was just drawn or undrawn. Nothing else is visited (DS-3b: no
        // pass over every repeat, nor every group).
        self.ensure_drawn();
        self.sync_repeats(true);
        let ran = self.reactive.take_ran_repeats();
        self.repeat_dirty.extend(ran);
        if self.repeat_dirty.is_empty() {
            self.settling = false;
            return Vec::new();
        }
        SETTLE_SCANS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut applied = Vec::new();
        let path = |f: &str| LeafPath::new(facet::FacetKind::Repeat, f).expect("declared leaf");
        let item = |f: &str| LeafPath::new(facet::FacetKind::Item, f).expect("declared leaf");
        let when = LeafPath::new(facet::FacetKind::When, "is").expect("declared leaf");
        // A round per level of nesting, and one more to see nothing change.
        for round in 0..=expand::DEPTH_MAX + 1 {
            let mut round_start = applied.len();
            let visit: Vec<NodeId> = std::mem::take(&mut self.repeat_dirty).into_iter().collect();
            if visit.is_empty() {
                break;
            }
            let drawn = self.drawn.as_ref().expect("ensured");
            let (here, gone): (Vec<NodeId>, Vec<NodeId>) = visit.into_iter().partition(|r| drawn.contains(r));
            // A repeat no longer drawn: what it gave goes.
            let mut changed_repeats: BTreeSet<NodeId> = BTreeSet::new();
            let mut unset: Vec<Leaf> = Vec::new();
            for r in &gone {
                self.repeat_inputs.remove(r);
                self.item_memo.remove(r);
                self.presence.remove(r);
                self.presence_moving.remove(r);
                self.repeat_problems.remove(r);
                unset.extend(self.item_shown.remove(r).unwrap_or_default());
                if self.repeats.remove(r).is_some() {
                    changed_repeats.insert(*r);
                }
            }
            let inputs: Vec<(NodeId, [Option<Value>; 7])> = here
                .iter()
                .map(|&r| (r, REPEAT_INPUTS.map(|(k, f)| self.reactive.read(Tree::of(&self.docs), Leaf { node: r, path: LeafPath::new(k, f).expect("declared leaf") }))))
                .collect();
            // `Value`'s `==` holds NaN equal to NaN ("no series").
            let inputs: Vec<(NodeId, [Option<Value>; 7])> = inputs.into_iter().filter(|(r, v)| self.repeat_inputs.get(r) != Some(v) || !self.repeats.contains_key(r)).collect();
            if inputs.is_empty() && gone.is_empty() {
                continue;
            }
            // A repeat with no enter or exit timing settles to a function
            // of its inputs: what it gave last is reused while they hold
            // (reactive-engine handoff §4.4, stage 3b).
            let mut results: Vec<(NodeId, ItemsOf)> = Vec::with_capacity(inputs.len());
            for (r, [each, count, key, shown, ..]) in &inputs {
                let r = *r;
                if self.restless.contains(&r)
                    && let Some((_, got)) = self.item_memo.get(&r)
                {
                    results.push((r, got.clone()));
                    continue;
                }
                let is_repeat = Tree::of(&self.docs).facet(r, facet::FacetKind::Repeat).is_some();
                // A conditional that fails to evaluate shows nothing.
                let shown = Tree::of(&self.docs).facet(r, facet::FacetKind::When).is_none() || (shown.as_ref().and_then(|v| v.as_bool()) == Some(true) && !self.reactive.errors.contains_key(&Leaf { node: r, path: when }));
                let timing = |f: &str| {
                    let kind = if is_repeat { facet::FacetKind::Repeat } else { facet::FacetKind::When };
                    self.reactive.read(Tree::of(&self.docs), Leaf { node: r, path: LeafPath::new(kind, f).expect("declared leaf") }).and_then(|v| v.as_f64()).filter(|m| m.is_finite()).unwrap_or(0.0).max(0.0)
                };
                let (enter_ms, exit_ms) = (timing("enter_ms"), timing("exit_ms"));
                let max = if is_repeat { self.reactive.read(Tree::of(&self.docs), Leaf { node: r, path: path("max") }).and_then(|v| v.as_f64()).filter(|m| m.is_finite()).unwrap_or(1000.0).clamp(0.0, 10_000.0) as usize } else { 0 };
                let memo_key = (enter_ms == 0.0 && exit_ms == 0.0).then(|| ItemsKey { inputs: [each.clone(), count.clone(), key.clone()], is_repeat, shown, max });
                if let Some(k) = &memo_key
                    && let Some((was, got)) = self.item_memo.get(&r)
                    && was == k
                {
                    results.push((r, got.clone()));
                    continue;
                }
                let mut got = ItemsOf { keys: Vec::new(), values: Vec::new(), problems: Vec::new() };
                if !is_repeat {
                    let wanted = if shown { vec![("on".to_owned(), Value::Bool(true))] } else { Vec::new() };
                    let all = settle_presence(&mut self.presence, &mut self.presence_moving, r, wanted, enter_ms, exit_ms);
                    for (k, (key, element)) in all.iter().enumerate() {
                        let Some(scope) = NodeId::item_scope(r, key) else { continue };
                        got.values.push((Leaf { node: scope, path: item("value") }, element.clone()));
                        got.values.push((Leaf { node: scope, path: item("index") }, Value::Number(k as f64)));
                    }
                    got.keys = all.into_iter().map(|(k, _)| k).collect();
                } else {
                    let listed = each.as_ref().and_then(|v| v.len());
                    let asked = listed.unwrap_or_else(|| count.as_ref().and_then(|v| v.as_f64()).filter(|n| n.is_finite()).map_or(0, |n| n.floor().max(0.0) as usize));
                    let n = asked.min(max);
                    if asked > max {
                        got.problems.push(expand::Problem { node: r, kind: "truncated", message: format!("{asked} items asked for; repeat.max allows {max}") });
                    }
                    let mut elements: Vec<Value> = match (listed, each) {
                        (Some(_), Some(each)) => each.elements().unwrap_or_default(),
                        _ => (0..n).map(|k| Value::Number(k as f64)).collect(),
                    };
                    elements.truncate(n);
                    // The template reads the first item's.
                    if let Some(first) = elements.first() {
                        got.values.push((Leaf { node: r, path: path("item") }, first.clone()));
                    }
                    let keys = match item_keys(&elements, listed.is_some(), key.as_ref()) {
                        Ok(keys) => keys,
                        Err(why) => {
                            got.problems.push(expand::Problem { node: r, kind: "keys", message: format!("{why}; items are keyed by index instead") });
                            (0..elements.len()).map(|k| k.to_string()).collect()
                        }
                    };
                    let wanted: Vec<(String, Value)> = if shown { keys.into_iter().zip(elements).collect() } else { Vec::new() };
                    let here = wanted.len();
                    // Those leaving come after, keeping the element and index
                    // they had.
                    let all = settle_presence(&mut self.presence, &mut self.presence_moving, r, wanted, enter_ms, exit_ms);
                    for (k, (key, element)) in all.iter().enumerate() {
                        let Some(scope) = NodeId::item_scope(r, key) else { continue };
                        got.values.push((Leaf { node: scope, path: item("value") }, element.clone()));
                        let index = if k < here { k as f64 } else { self.presence.get(&r).and_then(|p| p.items.get(key)).map_or(k as f64, |e| e.index) };
                        got.values.push((Leaf { node: scope, path: item("index") }, Value::Number(index)));
                    }
                    if let Some(p) = self.presence.get_mut(&r) {
                        for (k, (key, _)) in all.iter().enumerate().take(here) {
                            if let Some(e) = p.items.get_mut(key) {
                                e.index = k as f64;
                            }
                        }
                    }
                    got.keys = all.into_iter().map(|(k, _)| k).collect();
                }
                match memo_key {
                    Some(k) => {
                        self.item_memo.insert(r, (k, got.clone()));
                    }
                    None => {
                        self.item_memo.remove(&r);
                    }
                }
                results.push((r, got));
            }
            let mut set: BTreeMap<Leaf, Value> = BTreeMap::new();
            for ((r, input), (_, got)) in inputs.into_iter().zip(results) {
                self.repeat_inputs.insert(r, input);
                if got.problems.is_empty() {
                    self.repeat_problems.remove(&r);
                } else {
                    self.repeat_problems.insert(r, got.problems.clone());
                }
                let leaves: Vec<Leaf> = got.values.iter().map(|(l, _)| *l).collect();
                for (l, v) in got.values {
                    set.insert(l, v);
                }
                for l in self.item_shown.insert(r, leaves.clone()).unwrap_or_default() {
                    if !leaves.contains(&l) {
                        unset.push(l);
                    }
                }
                if self.repeats.get(&r) != Some(&got.keys) {
                    changed_repeats.insert(r);
                    self.repeats.insert(r, got.keys);
                }
            }
            let mut truncated: Vec<expand::Problem> = self.repeat_problems.values().flatten().cloned().collect();
            expand::sort_problems(&mut truncated);
            self.truncated = truncated;
            let structural = !changed_repeats.is_empty();
            if structural {
                // The owners of the repeats whose items changed.
                let changed: BTreeSet<NodeId> = changed_repeats.iter().filter_map(|r| r.path().first().copied()).collect();
                for &r in &changed_repeats {
                    let n = self.repeat_changes.entry(r).or_default();
                    *n += 1;
                    if *n == REPEAT_CHANGES && !self.restless.contains(&r) {
                        let doc = Tree::of(&self.docs);
                        if self.reactive.reaches_into(graph::CellId::Native(graph::NativeKey::Repeat(r)), &|n| n != r && doc.is_ancestor(r, n)) {
                            self.restless.insert(r);
                            self.feedback.push(expand::Problem { node: r, kind: "feedback", message: "its items keep changing: what it repeats over reads their own layout".into() });
                        }
                    }
                }
                let stamp = self.docs.authored.version();
                // Only the items that changed, where that matches a whole
                // expansion of their owners; else the owners.
                let items = self.owners.expand_items(&mut self.docs, stamp, &self.repeats, &changed_repeats);
                let (ops, delta) = match items {
                    Some(done) => done,
                    None => self.owners.expand(&mut self.docs, stamp, &self.repeats, engine::owners::Which::Some(changed)),
                };
                self.store_found(delta);
                self.check_owners("settling repeats");
                if round == expand::DEPTH_MAX + 1 && !ops.is_empty() {
                    let first = *changed_repeats.iter().next().expect("structural");
                    self.expansion_problems.push(expand::Problem { node: first, kind: "depth_blocked", message: "repeats did not settle: their counts keep changing each other".into() });
                    expand::sort_problems(&mut self.expansion_problems);
                }
                self.geometry.note_ops(ops.iter());
                round_start = applied.len();
                applied.extend(ops);
            }
            if structural {
                // The new structure into the graph before anything this
                // round publishes evaluates: a layout over the new items
                // has them reserved first (council review of DS-9). New
                // items may hold repeats: `regraph` draws them, and they are
                // read in the next round.
                let these: Vec<Op> = applied[round_start..].to_vec();
                self.regraph(these.iter());
            }
            unset.retain(|l| !set.contains_key(l));
            if !set.is_empty() || !unset.is_empty() {
                self.reactive.publish_delta(Tree::of(&self.docs), ProducerId::ITEMS, set, &unset);
            }
            // An item that has just joined starts unseen, in this same pass.
            self.publish_presence();
            if structural {
                self.resample();
                self.publish_state();
            }
            // What the round's publishing and structure ran.
            let ran = self.reactive.take_ran_repeats();
            self.repeat_dirty.extend(ran);
        }
        self.settling = false;
        applied
    }

    /// Every drawn repeat, found once (a load, or before the first
    /// settle); afterwards kept by `update_drawn`.
    fn ensure_drawn(&mut self) {
        if self.drawn.is_some() {
            return;
        }
        let all: BTreeSet<NodeId> = expand::drawn_repeats(Tree::of(&self.docs)).into_iter().collect();
        self.repeat_dirty.extend(all.iter().copied());
        self.repeat_natives.0.extend(all.iter().copied());
        self.drawn = Some(all);
    }

    /// Keep the drawn repeats as `ops` change the structure: a node made,
    /// moved, given or stripped of a repeat or conditional is looked at,
    /// with its subtree where its ancestors changed; a node gone leaves.
    /// What becomes drawn or stops is read again at the next settle.
    fn update_drawn<'a>(&mut self, ops: impl Iterator<Item = &'a Op>) {
        let Some(drawn) = self.drawn.as_mut() else { return };
        let doc = Tree::of(&self.docs);
        let is_drawn = |n: NodeId| {
            doc.is_live(n)
                && doc.node(n).is_some_and(|x| x.ty.name() == "group")
                && expand::is_item_owner(doc, n)
                && !n.path().first().is_some_and(|&root| expand::in_library(doc, root))
                && !expand::in_repeat_template(doc, n)
        };
        let mut look: BTreeSet<NodeId> = BTreeSet::new();
        for op in ops {
            match op {
                Op::Create { id, .. } => {
                    look.insert(*id);
                }
                Op::Reparent { id, .. } => {
                    look.insert(*id);
                    look.extend(doc.descendants(*id));
                }
                Op::AddFacet { id, facet } if expand::owns_items(facet.kind()) => {
                    look.insert(*id);
                    look.extend(doc.descendants(*id));
                }
                Op::RemoveFacet { id, kind } if expand::owns_items(*kind) => {
                    look.insert(*id);
                    look.extend(doc.descendants(*id));
                }
                Op::Delete { id } => {
                    look.insert(*id);
                }
                _ => {}
            }
        }
        // Everything deleted since the changes were last taken, subtrees
        // included.
        look.extend(self.docs.gone.keys());
        for n in look {
            let now = is_drawn(n);
            if now != drawn.contains(&n) {
                if now {
                    drawn.insert(n);
                    self.repeat_natives.0.insert(n);
                } else {
                    drawn.remove(&n);
                    self.repeat_natives.1.insert(n);
                }
                self.repeat_dirty.insert(n);
            }
        }
    }

    /// Publish where each entering or leaving item has got to.
    fn publish_presence(&mut self) {
        let leaf = |node: NodeId, f: &str| Leaf { node, path: LeafPath::new(facet::FacetKind::Item, f).expect("declared leaf") };
        let mut values = BTreeMap::new();
        for r in &self.presence_moving {
            let Some(p) = self.presence.get(r) else { continue };
            for (key, e) in &p.items {
                let Some(scope) = NodeId::item_scope(*r, key).filter(|s| Tree::of(&self.docs).is_live(*s)) else { continue };
                if e.p < 1.0 || e.leaving {
                    values.insert(leaf(scope, "presence"), Value::Number(e.p));
                    values.insert(leaf(scope, "leaving"), Value::Bool(e.leaving));
                }
            }
        }
        if values != self.reactive.published_by(ProducerId::PRESENCE) {
            self.reactive.publish(Tree::of(&self.docs), ProducerId::PRESENCE, values);
        }
    }

    /// Move the items entering and leaving to `now` (seconds, the host's
    /// clock): `presence` rises over its owner's `enter_ms` and falls over
    /// `exit_ms`, and an item that has finished leaving is removed. An
    /// owner on a paused clock holds.
    /// Returns the owners with an item still on its way.
    pub fn advance_presence(&mut self, now: f64) -> Vec<NodeId> {
        let dt = self.presence_at.map_or(0.0, |l| (now - l).clamp(0.0, 0.1)) * 1000.0;
        self.presence_at = Some(now);
        let mut gone = false;
        let mut active = Vec::new();
        let (doc, clocks) = (Tree::of(&self.docs), &self.clocks);
        let moving: Vec<NodeId> = self.presence_moving.iter().copied().collect();
        for r in moving {
            let Some(p) = self.presence.get_mut(&r) else {
                self.presence_moving.remove(&r);
                continue;
            };
            // On a paused clock (clocks.rs) items hold where they are.
            if clocks.held(doc, r) {
                continue;
            }
            let (enter_ms, exit_ms) = (p.enter_ms, p.exit_ms);
            let mut left = false;
            p.items.retain(|_, e| {
                if e.leaving {
                    e.p = if exit_ms > 0.0 { e.p - dt / exit_ms } else { 0.0 };
                    left |= e.p <= 0.0;
                    e.p > 0.0
                } else {
                    if e.p < 1.0 {
                        e.p = if enter_ms > 0.0 { (e.p + dt / enter_ms).min(1.0) } else { 1.0 };
                    }
                    true
                }
            });
            if p.items.values().any(|e| e.leaving || e.p < 1.0) {
                active.push(r);
            } else {
                self.presence_moving.remove(&r);
            }
            if left {
                // Its items change though its inputs do not: read it again.
                gone = true;
                self.repeat_inputs.remove(&r);
                self.repeat_dirty.insert(r);
            }
        }
        if gone {
            // The structure changes: settle again, without the ones gone.
            self.pipeline_from(false);
        }
        self.publish_presence();
        active
    }

    /// Publish the session state cells (`let.keep = session`) that still
    /// have a node; one whose scope went is forgotten, so it starts from
    /// its initial value if the scope returns (reactivity.md §5.1).
    fn publish_state(&mut self) {
        if self.session_state.is_empty() && self.reactive.published_by(ProducerId::STATE).is_empty() {
            return;
        }
        let doc = Tree::of(&self.docs);
        self.session_state.retain(|leaf, _| doc.is_live(leaf.node));
        if self.session_state != self.reactive.published_by(ProducerId::STATE) {
            self.reactive.publish(Tree::of(&self.docs), ProducerId::STATE, self.session_state.clone());
        }
    }

    /// Publish what is selected onto every node with a `selection` facet.
    fn publish_selection(&mut self) {
        let nodes: Vec<NodeId> = Tree::of(&self.docs).nodes_with(facet::FacetKind::Selection).filter(|&n| Tree::of(&self.docs).is_live(n)).collect();
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
            self.reactive.publish(Tree::of(&self.docs), ProducerId::SELECTION, values);
        }
    }

    /// Whether `leaf` is a session state cell: the value of a `let` whose
    /// `keep` is `session`.
    pub fn is_session_state(&self, leaf: Leaf) -> bool {
        leaf.path.facet == facet::FacetKind::Let
            && leaf.path.field == "value"
            && Tree::of(&self.docs).get::<facet::Let>(leaf.node).is_some_and(|l| l.keep.constant().and_then(|v| v.as_str()) == Some("session") && l.value.expr().is_none())
    }

    /// Set a session state cell. Not a transaction: never saved, never in
    /// undo. Refused for any other leaf.
    pub fn set_state(&mut self, leaf: Leaf, value: Value) -> Result<(), String> {
        if !self.is_session_state(leaf) {
            return Err(format!("{leaf} is not session state (a let with keep=\"session\" holding a constant)"));
        }
        // Back at its initial value, it is no longer held.
        if Tree::of(&self.docs).leaf(leaf.node, leaf.path).and_then(|s| s.constant()) == Some(&value) {
            self.session_state.remove(&leaf);
        } else {
            self.session_state.insert(leaf, value);
        }
        self.publish_state();
        self.pipeline();
        Ok(())
    }

    /// Every live action for `event`, by the node it listens on: `on.node`
    /// when set, else its parent.
    fn listeners(&self, event: &str) -> BTreeMap<NodeId, Vec<NodeId>> {
        let ty = TypeId::lookup("on").expect("registered");
        let mut by_node: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for on in Tree::of(&self.docs).nodes_of_type(ty).filter(|&n| Tree::of(&self.docs).is_live(n) && !expand::in_repeat_template(Tree::of(&self.docs), n)) {
            let Some(f) = Tree::of(&self.docs).get::<facet::On>(on) else { continue };
            if f.event.constant().and_then(|v| v.as_str()) != Some(event) {
                continue;
            }
            let named = f.node.constant().and_then(|v| v.as_ref()).filter(|n| n.raw() != 0);
            let Some(listens) = named.or_else(|| Tree::of(&self.docs).node(on).and_then(|n| n.parent)) else { continue };
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
            at = Tree::of(&self.docs).node(n).and_then(|x| x.parent);
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
            at = Tree::of(&self.docs).node(n).and_then(|x| x.parent);
        }
        out
    }

    /// Whether the document declares any action at all.
    pub fn has_actions(&self) -> bool {
        Tree::of(&self.docs).nodes_with(facet::FacetKind::On).next().is_some()
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
        self.exact();
        let Some((writes, session, label, emits)) = self.action_writes(listens, event)? else { return Ok(None) };
        let mut ops: Vec<Op> = Vec::new();
        for (leaf, value) in writes {
            let (source, v) = handle::resolve_target(Tree::of(&self.docs), &self.reactive, leaf, value)?;
            if self.reactive.read(Tree::of(&self.docs), source).as_ref() != Some(&v) {
                ops.push(Op::Set { id: source.node, path: source.path, slot: Slot::Const(v) });
            }
        }
        // The document writes first: session state is written, and the
        // events queued, only once the transaction is accepted.
        let applied = if ops.is_empty() {
            None
        } else {
            let label = label.unwrap_or_else(|| event.to_owned());
            Some(self.apply(TransactionInput { label, actor, ts, idempotency_key: None, ops, exact: false }).map_err(|errors| errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?)
        };
        for (leaf, value) in session {
            self.set_state(leaf, value)?;
        }
        self.emitted.extend(emits);
        Ok(applied)
    }

    /// A continuous event on `node`: `drag` (the pointer moved while held)
    /// or `wheel`. `payload` is what the action's formulas read on their
    /// own `on` node: `dx`, `dy` (how far the pointer moved since the last
    /// event, document px), `x`, `y` (where it is, in the listening node's
    /// own space) and `delta` (the wheel, px). The writes are **previewed**,
    /// not applied: every event of one drag reads what the one before left,
    /// and the host ends it with `commit_gesture`, one undo step. Answers
    /// the action's name when something listened.
    pub fn pointer_action(&mut self, node: NodeId, event: &str, payload: [f64; 5]) -> Result<Option<String>, String> {
        let Some((listens, actions)) = self.handlers(node, event) else { return Ok(None) };
        let mut shown = self.reactive.published_by(ProducerId::EVENTS);
        for on in actions {
            for (field, v) in ["dx", "dy", "x", "y", "delta"].into_iter().zip(payload) {
                shown.insert(Leaf { node: on, path: LeafPath::new(facet::FacetKind::On, field).expect("declared leaf") }, Value::Number(v));
            }
        }
        let doc = Tree::of(&self.docs);
        shown.retain(|l, _| doc.is_live(l.node));
        self.reactive.publish(Tree::of(&self.docs), ProducerId::EVENTS, shown);
        let Some((writes, session, label, emits)) = self.current(|s| s.action_writes(listens, event))? else { return Ok(None) };
        for (leaf, value) in writes {
            self.preview_target(leaf.node, leaf.path, value)?;
        }
        for (leaf, value) in session {
            self.set_state(leaf, value)?;
        }
        self.emitted.extend(emits);
        Ok(Some(label.unwrap_or_else(|| event.to_owned())))
    }

    /// Where `node` or something above it listens for `event`.
    pub fn listens_for(&self, node: NodeId, event: &str) -> Option<NodeId> {
        if !self.has_actions() {
            return None;
        }
        self.handlers(node, event).map(|(listens, _)| listens)
    }

    /// What the actions listening on `listens` for `event` would write
    /// now: the document leaves and their values, the session cells, the
    /// first action's name, and the events they emit. Every value is read
    /// before any is written; the caller queues the events only once the
    /// writes have applied.
    #[allow(clippy::type_complexity)]
    fn action_writes(&mut self, listens: NodeId, event: &str) -> Result<Option<(Vec<(Leaf, Value)>, Vec<(Leaf, Value)>, Option<String>, Vec<(NodeId, String)>)>, String> {
        let Some(actions) = self.listeners(event).remove(&listens) else { return Ok(None) };
        let leaf_of = |f: &str| LeafPath::new(facet::FacetKind::Write, f).expect("declared leaf");
        let mut writes: Vec<(Leaf, Value)> = Vec::new();
        let mut session: Vec<(Leaf, Value)> = Vec::new();
        let mut label = None;
        let mut emits = Vec::new();
        for on in actions {
            if let Some(name) = Tree::of(&self.docs).get::<facet::On>(on).and_then(|f| f.emit.constant()).and_then(|v| v.as_str()).filter(|n| !n.is_empty()) {
                emits.push((listens, name.to_owned()));
            }
            if label.is_none() {
                label = Tree::of(&self.docs).get::<facet::Name>(on).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).filter(|n| !n.is_empty()).map(str::to_owned);
            }
            let writers: Vec<NodeId> = std::iter::once(on).chain(Tree::of(&self.docs).children(on).iter().copied()).filter(|&w| Tree::of(&self.docs).is_live(w) && Tree::of(&self.docs).facet(w, facet::FacetKind::Write).is_some()).collect();
            for w in writers {
                let read = |f: &str| self.reactive.read(Tree::of(&self.docs), Leaf { node: w, path: leaf_of(f) });
                let Some(target) = read("target").and_then(|v| v.as_ref()).filter(|n| n.raw() != 0) else { continue };
                let prop = read("prop").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
                let path: LeafPath = prop.parse().map_err(|e| format!("{w}: write.prop {prop:?}: {e}"))?;
                let leaf = Leaf { node: target, path };
                let now = self.reactive.read(Tree::of(&self.docs), leaf);
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
                    writes.push((leaf, value));
                }
            }
        }
        Ok(Some((writes, session, label, emits)))
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
            if p.kind == "binding" && (expand::in_repeat_template(Tree::of(&self.docs), p.node) || !Tree::of(&self.docs).is_live(p.node)) {
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

    /// Everything wrong that a node can show (reactivity.md §4.3): what
    /// expansion could not derive or derived past a limit, repeats cut at
    /// their `max`, and bindings that failed.
    /// The context a picture reads (reactivity.md §6): each `@name` read
    /// under `figure` that is not a colour token, with the node providing
    /// it, or `None` where the picture's own default serves. For the
    /// inspector: what placement gives this instance.
    pub fn context_reads(&self, figure: NodeId) -> Vec<(String, Option<NodeId>, Option<Value>)> {
        let mut out: BTreeMap<String, (Option<NodeId>, Option<Value>)> = BTreeMap::new();
        for (leaf, ctx) in self.reactive.contexts() {
            if leaf.node != figure && !Tree::of(&self.docs).is_ancestor(figure, leaf.node) {
                continue;
            }
            for (name, provider) in ctx {
                if palette::is_token(Tree::of(&self.docs), name) && provider.path.facet == facet::FacetKind::Token {
                    continue;
                }
                let own = Tree::of(&self.docs).is_ancestor(figure, provider.node);
                out.entry(name.clone()).or_insert_with(|| ((!own).then_some(provider.node), self.reactive.read(Tree::of(&self.docs), *provider)));
            }
        }
        out.into_iter().map(|(n, (p, v))| (n, p, v)).collect()
    }

    pub fn problems(&self) -> Vec<expand::Problem> {
        let mut out = self.expansion_problems.clone();
        out.extend(self.truncated.iter().cloned());
        out.extend(self.feedback.iter().cloned());
        out.extend(self.reactive.errors.iter().map(|(leaf, message)| expand::Problem { node: leaf.node, kind: "binding", message: format!("{}: {message}", leaf.path) }));
        out.extend(self.machines_unsettled.iter().cloned());
        out.extend(self.cluster_problems());
        out
    }

    /// The constraints' problems (DS-7): what grouping could not use or
    /// solve (a malformed constraint, a duplicate, a cycle through
    /// bindings), and each constraint that doesn't hold at its cluster's
    /// pose, as the cluster's watching native last read it. Read from the
    /// document and the pose, never remembered from a solve.
    fn cluster_problems(&self) -> Vec<expand::Problem> {
        let mut out: Vec<expand::Problem> = self.clusters.problems.iter().map(|(id, message)| expand::Problem { node: *id, kind: "constraint", message: message.clone() }).collect();
        for c in self.clusters.list.iter().filter(|c| !self.clusters.failed.contains(&c.key())) {
            let Some(v) = self.reactive.native_extra(graph::NativeKey::Cluster(c.key())).and_then(|e| e.downcast_ref::<solve::Violated>()) else { continue };
            out.extend(v.0.iter().map(|(id, r)| expand::Problem { node: *id, kind: "constraint", message: format!("constraint {id} of cluster {} doesn't hold here (residual {r:.3e}): its cluster could not be solved", c.id) }));
        }
        out
    }

    /// Recompute what the store derives after every change: structure, the
    /// across/through network's solution, and what the graph's natives
    /// derive from them (layout, `layout.rs`).
    pub fn resolve_physics(&mut self) {
        // Structure first: what follows places and styles the new items.
        self.publish_selection();
        // Layout natives run in the graph's passes, in height order, with
        // what they read (handoff §4.10, RE-5.6), so a repeat's natives see a
        // laid-out width its count reads in the same pass. A round settles
        // the structure that made, until it rests. A count that reads its
        // own items' layout never rests: it is a problem on the repeat
        // (decision D6, `REPEAT_CHANGES`).
        self.repeat_changes.clear();
        for round in 0..=LAYOUT_ROUNDS {
            let derived = self.settle_repeats();
            if round > 0 && derived.is_empty() {
                break;
            }
            self.note_changes(derived.iter(), &[]);
            self.publish_state();
            self.solve_network();
            self.resolve_theme();
            self.sync_layout(true);
            self.reactive.flush(Tree::of(&self.docs));
        }
        self.repeat_changes.clear();
        self.resolve_contexts();
    }

    /// Merge the document's `context` nodes over the built-in library when
    /// their markup changed (contexts.md §2.2).
    fn resolve_contexts(&mut self) {
        let ty = TypeId::lookup("context").expect("registered");
        let markups: Vec<(NodeId, String)> = Tree::of(&self.docs)
            .nodes_of_type(ty)
            .filter(|&n| Tree::of(&self.docs).is_live(n))
            .filter_map(|n| Some((n, Tree::of(&self.docs).get::<facet::ContextDef>(n)?.markup.constant()?.as_str()?.to_owned())))
            .collect();
        if markups.is_empty() {
            if self.contexts.take().is_some() {
                self.glide_specs();
                self.reactive.set_input(Tree::of(&self.docs), graph::InputKey::Contexts, None);
            }
        } else if self.contexts.as_ref().is_none_or(|(m, _)| *m != markups) {
            let lib = contexts::Library::with_document(&markups);
            self.reactive.set_input(Tree::of(&self.docs), graph::InputKey::Contexts, Some(std::sync::Arc::new(lib.clone())));
            self.contexts = Some((markups, lib));
            self.glide_specs();
        }
    }

    /// A resolver over this document's contexts that sees nodes'
    /// interaction states.
    pub fn resolver(&self) -> contexts::Resolver<'_> {
        contexts::Resolver { doc: Tree::of(&self.docs), lib: self.contexts(), states: Some(&self.interaction), editing: self.machines.pinned.as_ref().map(|p| p.def) }
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
            self.clear_presentation();
        }
        if !enabled {
            self.reactive.glide_off();
        } else if self.reactive.glide.is_none() {
            self.reactive.glide = Some(graph::Glide::new(self.motion.clock, self.layout_preview.as_ref().and_then(|p| p.floating).map(|(n, _)| n)));
            self.glide_specs();
        }
    }

    /// What each laid-out node may glide by, as its placement's context
    /// declares: kept with the glides while they run (handoff §4.10).
    /// `glide_specs` for the nodes of the layout roots `rebuilt` only:
    /// their old entries go, their nodes' are found again.
    fn glide_specs_of(&mut self, rebuilt: &[NodeId]) {
        if self.reactive.glide.is_none() || rebuilt.is_empty() {
            return;
        }
        let mut gone: Vec<NodeId> = Vec::new();
        for &r in rebuilt {
            gone.extend(self.glide_nodes.remove(&r).unwrap_or_default());
        }
        let doc = Tree::of(&self.docs);
        let x: LeafPath = "transform2d.x".parse().expect("declared leaf");
        let resolver = contexts::Resolver::new(doc, self.contexts());
        let mut add = BTreeMap::new();
        let mut placed: Vec<(NodeId, Vec<NodeId>)> = Vec::new();
        for &r in rebuilt {
            let Some(n) = self.reactive.native(graph::NativeKey::Layout(r)) else { continue };
            let nodes: Vec<NodeId> = n.outputs().iter().filter(|l| l.path == x).map(|l| l.node).collect();
            for &node in &nodes {
                let chain = contexts::chain_of(doc, node);
                if chain.is_empty() {
                    continue;
                }
                let declared = resolver.get(&chain, chain.len() - 1, contexts::Field::PLACEMENT);
                if let Some(m) = contexts::motion(&declared).filter(|m| matches!(m.when, "any" | "layout") && m.ms > 0.0) {
                    add.insert(node, (m.ms, m.ease));
                }
            }
            placed.push((r, nodes));
        }
        self.glide_nodes.extend(placed);
        if let Some(g) = self.reactive.glide.as_mut() {
            for n in gone {
                g.specs.remove(&n);
            }
            g.specs.extend(add);
        }
    }

    /// Every layout root's glide specs, found afresh (motion turned on),
    /// with the nodes each root glides recorded, so a root rebuilt later
    /// drops its own (council review of DS-3b).
    fn glide_specs(&mut self) {
        let Some(g) = self.reactive.glide.as_mut() else { return };
        g.specs.clear();
        self.glide_nodes.clear();
        let roots: Vec<NodeId> = self.reactive.native_keys().filter_map(|k| match k {
            graph::NativeKey::Layout(r) => Some(r),
            _ => None,
        }).collect();
        self.glide_specs_of(&roots);
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

    /// How many edits under a pinned state changed every state instead
    /// (machine.rs `shared_edit`): session only.
    pub fn shared_edits(&self) -> u64 {
        self.machines.shared
    }

    /// The parts open inside a transition in the state editor (machines.md
    /// §6): their properties have rows of their own.
    pub fn inside_open(&self) -> &std::collections::BTreeSet<NodeId> {
        &self.machines.open
    }

    pub fn set_inside_open(&mut self, part: NodeId, open: bool) {
        if open {
            self.machines.open.insert(part);
        } else {
            self.machines.open.remove(&part);
        }
    }

    /// What the machines were running at the last advance: each instance
    /// mid-transition, which transition, how far, between which states.
    pub fn running_machines(&self) -> &[machine::Running] {
        &self.machines.running
    }

    pub fn advance_machines(&mut self, now: f64) -> Vec<machine::Running> {
        // Which instances are watched is the gate's to keep (`gate`); a
        // caller playing machines alone asks it here.
        self.gate(now);
        // A pin whose state is gone (deleted, or a rename undone) lets go.
        if let Some(pin) = &self.machines.pinned
            && !machine::state_names(Tree::of(&self.docs), pin.machine).contains(&pin.state)
        {
            self.machines.pinned = None;
        }
        // A machine driven by another's channel (`machine.at`) reads what
        // that one publishes: record and evaluate again, at the same time,
        // until nothing more moves, so a child is never a frame behind its
        // parent (reactive-engine.md §10.4: a round per link of the chain,
        // at most `ROUNDS`; one still moving then is a problem).
        const ROUNDS: usize = 8;
        let mut running = Vec::new();
        let mut moving = None;
        // Every instance after a structural change (the gate decided),
        // else those whose leaves or copies changed (the graph wakes them,
        // `watch_nodes`), and the player's own moving and clocked ones.
        let mut visit: Option<BTreeSet<NodeId>> = if std::mem::take(&mut self.machines_full) { None } else { Some(self.reactive.take_woken_nodes()) };
        for round in 0..=ROUNDS {
            let (set, unset, now_running) = {
                let reactive = &self.reactive;
                let doc = Tree::of(&self.docs);
                let read = |l: Leaf| reactive.read(doc, l);
                let clocks = &self.clocks;
                self.machines.advance(doc, &read, now, self.time(), &|e, now| (clocks.time(doc, e, now), clocks.held(doc, e)), visit.as_ref())
            };
            running = now_running;
            // A leaf the graph holds against the machines (bound, or a
            // higher-ranked producer's) is never shown: it is not still
            // moving. One the machines drive, contested or not, is (review
            // of DS-10a: a lower-ranked request froze a running fade).
            moving = set.keys().find(|l| !self.reactive.refuses(**l, ProducerId::MACHINES)).map(|l| l.node).or_else(|| unset.first().map(|l| l.node));
            if moving.is_none() {
                break;
            }
            self.reactive.publish_delta(Tree::of(&self.docs), ProducerId::MACHINES, set, &unset);
            self.pipeline();
            if round == ROUNDS {
                break;
            }
            // The next round: the instances what was just published reached.
            visit = Some(self.reactive.take_woken_nodes());
        }
        self.machines_unsettled = moving.map(|node| expand::Problem { node, kind: "machine", message: format!("machines driving one another still change after {ROUNDS} rounds at one time") });
        running
    }

    /// Publish how each query stands at `now` (seconds, the host's clock)
    /// and answer the requests that became due, for the host to carry out
    /// and answer with `resolve_query` (query.rs).
    /// The nodes whose watched leaves of `kind` changed (a held trace's
    /// input, a query's): the graph keeps one list, taken by both players.
    fn take_woken(&mut self, kind: FacetKind) -> Vec<NodeId> {
        self.reactive.take_woken(kind)
    }

    pub fn advance_queries(&mut self, now: f64) -> Vec<query::Request> {
        // Which queries are live is the gate's to keep (`gate`).
        self.gate(now);
        let woken = self.take_woken(FacetKind::Query);
        self.queries.touch(woken);
        let (values, due) = {
            let reactive = &self.reactive;
            let doc = Tree::of(&self.docs);
            let read = |l: Leaf| reactive.read(doc, l);
            let wanted = |n: NodeId| query::OUTPUTS.iter().any(|f| reactive.is_read(Leaf { node: n, path: LeafPath { facet: FacetKind::Query, field: f } }));
            self.queries.advance(doc, &read, &wanted, now)
        };
        if !values.is_empty() {
            self.reactive.publish_delta(Tree::of(&self.docs), ProducerId::QUERIES, values, &[]);
            self.pipeline();
        }
        due
    }

    /// The host's answer to a request. False when a newer request has
    /// replaced it: the answer is dropped. Shown at the next
    /// `advance_queries`.
    pub fn resolve_query(&mut self, node: NodeId, seq: u64, result: Result<Value, String>) -> bool {
        self.queries.resolve(node, seq, result)
    }

    /// The queries with a request unanswered or still to send.
    pub fn pending_queries(&self) -> Vec<NodeId> {
        self.queries.pending().collect()
    }

    /// Step the traces and accumulators to `now` (seconds, the host's
    /// clock) and publish them (step.rs). Returns the nodes still
    /// changing, so the host keeps their compositions drawing. Those on a
    /// paused clock (clocks.rs: a composition no view shows) hold still: a
    /// simulation in a hidden tab costs nothing.
    pub fn advance_steps(&mut self, now: f64) -> Vec<NodeId> {
        // Each accumulator's formula reads the step's length, so it is
        // published before the formulas are read: but only while one of
        // them is moving. Frame times jitter, and republishing a length
        // nothing uses would redraw every frame (step.rs `restless`). Only
        // the steppers on running clocks are visited (the gate decides
        // which, `gate`), and only what changed is published (DS-2).
        // Which run is the gate's to say; a caller stepping alone (not
        // through `advance`) asks it here. A cache hit costs nothing.
        self.gate(now);
        let dt = self.steps.tick(now);
        let (traces, accs) = self.running_steps.clone();
        if !accs.is_empty() {
            let first = Value::Number(1.0 / 60.0);
            let mut set = BTreeMap::new();
            for &n in &accs {
                if !self.steps.dt_of(n).is_some_and(|x| x > 0.0) {
                    set.insert(step::Steps::dt_leaf(n), first.clone());
                    self.steps.shown_dt(n, first.clone());
                }
            }
            if !set.is_empty() {
                self.reactive.publish_delta(Tree::of(&self.docs), ProducerId::STEPS, set, &[]);
            }
            let restless = {
                let (reactive, doc) = (&self.reactive, Tree::of(&self.docs));
                self.steps.restless(doc, &|l: Leaf| reactive.read(doc, l), &accs)
            };
            if restless {
                let mut set = BTreeMap::new();
                for &n in &accs {
                    if self.steps.dt_of(n) != Some(dt) {
                        set.insert(step::Steps::dt_leaf(n), Value::Number(dt));
                        self.steps.shown_dt(n, Value::Number(dt));
                    }
                }
                if !set.is_empty() {
                    self.reactive.publish_delta(Tree::of(&self.docs), ProducerId::STEPS, set, &[]);
                }
            }
        }
        let woken: Vec<NodeId> = self.take_woken(FacetKind::Trace).into_iter().filter(|n| !traces.contains(n)).collect();
        let (values, active) = {
            let reactive = &self.reactive;
            let doc = Tree::of(&self.docs);
            let read = |l: Leaf| reactive.read(doc, l);
            self.steps.advance(doc, &read, now, &self.clocks, &traces, &accs, &woken)
        };
        if !values.is_empty() {
            self.reactive.publish_delta(Tree::of(&self.docs), ProducerId::STEPS, values, &[]);
            self.pipeline();
        }
        active
    }

    /// Watch what `interest` shows (§8.2). Counts once `observe_all` is off.
    pub fn observe(&mut self, interest: Interest) -> ObserverId {
        let id = ObserverId(self.next_observer);
        self.next_observer += 1;
        self.observers.insert(id, interest);
        self.necessity();
        id
    }

    /// What observer `id` shows has changed.
    pub fn reobserve(&mut self, id: ObserverId, interest: Interest) {
        if let Some(i) = self.observers.get_mut(&id)
            && *i != interest
        {
            *i = interest;
            self.necessity();
        }
    }

    pub fn unobserve(&mut self, id: ObserverId) {
        if self.observers.remove(&id).is_some() {
            self.necessity();
        }
    }

    /// Whether everything counts as shown, observers or not: today's
    /// behaviour, a store's default and the differential reference. A host
    /// that registers what it shows turns it off.
    pub fn set_observe_all(&mut self, all: bool) {
        if self.observe_all != all {
            self.observe_all = all;
            self.necessity();
        }
    }

    /// Decide which cells nothing observed needs (reactive-engine.md §8.2,
    /// §8.3), and bring up to date the stale ones that are needed again.
    /// A cell is needed when it is outside every composition no observer
    /// shows, or on a node the store reads besides drawing it (`ROOTED`,
    /// and a constraint cluster's nodes and what it reads and solves), or
    /// read by a needed cell. The rest are pictures nothing shows: they are
    /// not evaluated until shown (or a transaction needs every cell exact).
    fn necessity(&mut self) {
        let doc = Tree::of(&self.docs);
        let hidden: BTreeSet<NodeId> = if self.observe_all {
            BTreeSet::new()
        } else {
            let shown = composition::shown(doc, &self.observed_compositions());
            let mut hidden: BTreeSet<NodeId> = composition::compositions(doc).into_iter().filter(|c| !shown.contains(c)).flat_map(|c| std::iter::once(c).chain(doc.descendants(c))).collect();
            // An entity pictured only in hidden compositions is hidden too:
            // a placed instance's entity lies outside every composition, and
            // its cells (a plot's ticks and paths) are needed only where
            // something needed reads them, as the gate runs it only where it
            // is pictured (PT-1.1). Before, a clip on a hidden page's plot
            // evaluated that plot every frame while playing (2026-11-21).
            let mut pictured: BTreeMap<NodeId, bool> = BTreeMap::new();
            for r in doc.relations().filter(|r| r.rel == RelKind::Figures && doc.is_live(r.from) && doc.is_live(r.to)) {
                if let Some(c) = composition::composition_of(doc, r.from) {
                    *pictured.entry(r.to).or_default() |= shown.contains(&c);
                }
            }
            for (e, seen) in pictured {
                if !seen {
                    hidden.insert(e);
                    hidden.extend(doc.descendants(e));
                }
            }
            hidden
        };
        let solved: BTreeSet<NodeId> = self.clusters.list.iter().flat_map(|c| c.nodes.iter().copied().chain(c.vars.iter().chain(&c.inputs).map(|l| l.node))).collect();
        let rooted = |n: NodeId| solved.contains(&n) || doc.facets_of(n).any(|f| ROOTED.contains(&f.kind()));
        let unneeded = self.reactive.necessity(&hidden, &rooted);
        if self.reactive.set_unneeded(doc, unneeded) {
            self.pipeline();
        }
    }

    /// Bring every stale cell up to date: what a transaction, an action and
    /// a session write start from, so that what they read is exact (§8.9).
    /// A host that writes what it reads (an inspector's unbind keeping the
    /// value shown) or answers a reader that may write back (the agent's
    /// view) calls it first. The store decides necessity again at the next
    /// `advance`.
    pub fn exact(&mut self) {
        if self.reactive.relax(Tree::of(&self.docs)) {
            self.pipeline();
        }
    }

    /// What follows values that moved outside a transaction, as after a
    /// publish: live solves, then what the store derives. Natives read
    /// through the graph; the store's own code between them reads leaves
    /// directly (the repeat scan, the network's structure, the live
    /// cluster solve, session state). If one was stale, the derivation runs
    /// again with it up to date (handoff §4.10, RE-5.6: in the suite only
    /// `current`'s callers still read one).
    fn pipeline(&mut self) {
        self.pipeline_from(true);
    }

    /// `pipeline`, solving the constraint clusters first only if
    /// `clusters`. Every derivation outside a transaction comes here (or
    /// to `relayout`), so that what it reads outside the graph is recorded.
    fn pipeline_from(&mut self, clusters: bool) {
        self.settle_reads();
        for _ in 0..4 {
            self.derive(clusters);
            if !self.settle_reads() {
                return;
            }
        }
        // Reads that keep reaching further: derive from an exact graph.
        self.reactive.relax(Tree::of(&self.docs));
        self.derive(clusters);
    }

    fn derive(&mut self, clusters: bool) {
        self.recorded(|s| {
            if clusters && !s.clusters.list.is_empty() {
                s.resolve_clusters_live();
            }
            s.resolve_physics();
        });
    }

    /// After a change outside the document that layout reads (a theme, a
    /// font, a gap previewed), which its natives took in that pass: what
    /// read a stale cell runs again, like `pipeline`.
    fn relayout(&mut self) {
        self.reactive.flush(Tree::of(&self.docs));
        if self.settle_reads() {
            self.pipeline();
        }
    }

    /// Run `f` recording what it reads of cells nothing needed, for
    /// `settle_reads`.
    fn recorded<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.reactive.recording(true);
        let out = f(self);
        self.reactive.recording(false);
        out
    }

    /// Make needed the cells read from outside the graph while nothing
    /// needed them, bringing them up to date (reactive-engine.md §8.9): they
    /// stay needed from now on. Whether one held an old value when it was
    /// read (a tracked read inside the graph may have woken it since), so
    /// that what read it runs again; one that was current costs no second
    /// run (a run more than the reference would make moves what reads a
    /// later stage of the derivation, a repeat counting a placement).
    fn settle_reads(&mut self) -> bool {
        let escaped = self.reactive.take_escaped();
        if escaped.is_empty() {
            return false;
        }
        let old = escaped.values().any(|&old| old);
        self.reactive.need(Tree::of(&self.docs), &escaped.into_keys().collect());
        old
    }

    /// Run `f`, which computes from values without changing the document,
    /// on values up to date: if it read a stale cell, it runs again once
    /// that cell is current (a handle's target, an action's writes).
    fn current<T>(&mut self, mut f: impl FnMut(&mut Self) -> T) -> T {
        if self.settle_reads() {
            self.pipeline();
        }
        for _ in 0..4 {
            let out = self.recorded(&mut f);
            if !self.settle_reads() {
                return out;
            }
            self.pipeline();
        }
        self.exact();
        f(self)
    }

    pub fn observing_all(&self) -> bool {
        self.observe_all
    }

    /// The compositions the observers show directly.
    fn observed_compositions(&self) -> Vec<NodeId> {
        let doc = Tree::of(&self.docs);
        let mut out = Vec::new();
        for i in self.observers.values() {
            match i {
                Interest::Region { composition, .. } => out.push(*composition),
                Interest::Nodes(nodes) => out.extend(nodes.iter().filter_map(|&n| composition::composition_of(doc, n))),
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Pause the clocks of what nothing shows or reads (clocks.rs; the
    /// gate of handoff-patches.md PT-1.2): every composition no observer
    /// shows, and every entity pictured only there, unless something
    /// running reads it or its composition's `run` is `always` (PT-1.3).
    /// Nothing is paused while everything is observed.
    fn gate(&mut self, now: f64) {
        let key: clocks::GateKey = (self.docs.reshaped, self.reactive.wired(), if self.observe_all { Vec::new() } else { self.observed_compositions() });
        // Nothing the decision reads changed: the clocks stand as they are,
        // and nothing is cloned or walked (DS-2).
        if !self.observe_all && self.clocks.gated.as_ref().is_some_and(|(k, ..)| *k == key) {
            return;
        }
        self.clocks.decisions += 1;
        let (paused, pictured) = self.paused();
        let doc = Tree::of(&self.docs);
        self.clocks.set(doc, now, &paused, &pictured);
        self.clocks.gated = Some((key, paused, pictured));
        // Which steppers run, for the step player; a held trace is woken by
        // a change to its input. Steppers gone are withdrawn.
        let live = |kind| doc.nodes_with(kind).filter(|&n| doc.is_live(n)).collect::<Vec<NodeId>>();
        let (traces, accs) = (live(FacetKind::Trace), live(FacetKind::Accumulate));
        let held = |n: &NodeId| self.clocks.held(doc, *n);
        let queries: BTreeSet<NodeId> = doc.nodes_with(FacetKind::Query).filter(|&n| doc.is_live(n)).collect();
        let mut watched: BTreeSet<Leaf> = traces.iter().filter(|n| held(n)).flat_map(|&n| step::Steps::input_leaves(n)).collect();
        watched.extend(queries.iter().flat_map(|&n| query::Queries::input_leaves(n)));
        let all: BTreeSet<NodeId> = traces.iter().chain(&accs).copied().collect();
        self.running_steps = (traces.into_iter().filter(|n| !held(n)).collect(), accs.into_iter().filter(|n| !held(n)).collect());
        self.reactive.watch(watched);
        let gone = self.steps.prune(&all);
        if !gone.is_empty() {
            self.reactive.publish_delta(Tree::of(&self.docs), ProducerId::STEPS, BTreeMap::new(), &gone);
        }
        // The clips that animate something running, and where: what the
        // playhead samples and wakes each frame. Every clip is sampled now,
        // so one on a page that just resumed shows its time.
        let clip = TypeId::lookup("clip").expect("registered");
        let mut running_clips = Vec::new();
        let mut animated = BTreeSet::new();
        for c in doc.nodes_of_type(clip).filter(|&c| doc.is_live(c)) {
            let targets: Vec<NodeId> = doc.relations_from(c).filter(|r| r.rel == RelKind::Animates && doc.is_live(r.to)).map(|r| r.to).collect();
            if targets.iter().any(|&t| !self.clocks.held(doc, t)) {
                running_clips.push(c);
                animated.extend(targets.iter().filter_map(|&t| composition::composition_of(doc, t)));
            }
        }
        self.running_clips = running_clips;
        self.animated = animated;
        // Every instance is visited once; then each is woken by a change to
        // its own leaves or its copies'.
        self.machines_full = true;
        let entity = TypeId::lookup("entity").expect("registered");
        self.reactive.watch_nodes(doc.nodes_of_type(entity).filter(|&e| doc.is_live(e)).collect());
        let gone = self.queries.prune(&queries);
        if !gone.is_empty() {
            self.reactive.publish_delta(Tree::of(&self.docs), ProducerId::QUERIES, BTreeMap::new(), &gone);
        }
        if !self.observe_all {
            self.resample();
        }
    }

    /// How many times `advance`'s gate decided which clocks to pause, rather
    /// than reusing its last decision: a cost check (a frame that changes
    /// nothing the gate reads should not decide).
    pub fn gate_decisions(&self) -> u64 {
        self.clocks.decisions
    }

    /// The clocks to pause, and the entities some composition pictures.
    fn paused(&self) -> (BTreeSet<NodeId>, BTreeSet<NodeId>) {
        if self.observe_all {
            return Default::default();
        }
        let doc = Tree::of(&self.docs);
        let comps: BTreeSet<NodeId> = composition::compositions(doc).into_iter().collect();
        let observed = self.observed_compositions();
        // What an observer shows runs, but accumulators step only on an
        // open path: one in an `on_demand` composition holds even while
        // shown (free-running.md), and so does all of that composition.
        let holding: BTreeSet<NodeId> = doc.nodes_with(FacetKind::Accumulate).filter(|&n| doc.is_live(n)).filter_map(|n| composition::composition_of(doc, n)).collect();
        let still: BTreeSet<NodeId> = holding.difference(&composition::reach(doc, &observed, &holding).running).copied().collect();
        let mut live: BTreeSet<NodeId> = composition::shown(doc, &observed).difference(&still).copied().collect();
        // `run = always` keeps a composition's clock running while hidden
        // (PT-1.3; free-running.md §2).
        live.extend(comps.iter().copied().filter(|&c| composition::run_mode(doc, c) == composition::RunMode::Always));
        let hidden: Vec<NodeId> = comps.iter().copied().filter(|c| !live.contains(c)).collect();
        if hidden.is_empty() {
            return Default::default();
        }
        let mut pictured: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
        for r in doc.relations().filter(|r| r.rel == RelKind::Figures && doc.is_live(r.from) && doc.is_live(r.to)) {
            if let Some(c) = composition::composition_of(doc, r.from) {
                pictured.entry(r.to).or_default().insert(c);
            }
        }
        // The pictured entity a model node belongs to: itself or its
        // nearest such ancestor.
        let entity_of = |n: NodeId| {
            let mut cur = Some(n);
            while let Some(node) = cur.and_then(|c| doc.live(c)) {
                if pictured.contains_key(&node.id) {
                    return Some(node.id);
                }
                cur = node.parent;
            }
            None
        };
        // A composition read from one that runs runs too: a meter in view
        // reading a simulation in a hidden tab, directly, through a trace or
        // through a stack's placements. A reader outside every composition
        // is a model node, such as a placed instance's entity reading its
        // arguments: it runs where it is pictured, when something running
        // reads it, or always when nothing pictures it (the timeline, loose
        // roots). Decided until no more join (one hidden tab read through
        // another).
        let mut entities: BTreeSet<NodeId> = pictured.iter().filter(|(_, cs)| cs.iter().any(|c| live.contains(c))).map(|(&e, _)| e).collect();
        // Who reads each hidden composition and each pictured entity not yet
        // running, found once: where each reading cell runs, as the
        // composition it is in, else the pictured entity it belongs to, else
        // nowhere in particular (it always runs). Then decided over those
        // small sets until no more join. (Walking every hidden composition's
        // readers each round cost 8-10 ms on the demo, 2026-11-21.)
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum Owner {
            Composition(NodeId),
            Entity(NodeId),
            Free,
        }
        let mut owners: BTreeMap<NodeId, Owner> = BTreeMap::new();
        let mut readers_of = |root: NodeId| -> BTreeSet<Owner> {
            let mut nodes = BTreeSet::new();
            let always = self.reactive.reader_nodes(std::iter::once(root).chain(doc.descendants(root)), &mut nodes);
            let mut out: BTreeSet<Owner> = nodes
                .into_iter()
                .map(|n| {
                    *owners.entry(n).or_insert_with(|| match composition::composition_of(doc, n) {
                        Some(c) => Owner::Composition(c),
                        None => entity_of(n).map_or(Owner::Free, Owner::Entity),
                    })
                })
                .collect();
            if always {
                out.insert(Owner::Free);
            }
            out
        };
        let hidden_readers: Vec<(NodeId, BTreeSet<Owner>)> = hidden.iter().map(|&c| (c, readers_of(c))).collect();
        let entity_readers: Vec<(NodeId, BTreeSet<Owner>)> = pictured.keys().filter(|e| !entities.contains(e)).map(|&e| (e, readers_of(e))).collect();
        loop {
            let runs = |o: &Owner| match o {
                Owner::Composition(c) => live.contains(c),
                Owner::Entity(e) => entities.contains(e),
                Owner::Free => true,
            };
            let joined: Vec<NodeId> = hidden_readers.iter().filter(|(c, rs)| !live.contains(c) && rs.iter().any(runs)).map(|(c, _)| *c).collect();
            let joined_entities: Vec<NodeId> = entity_readers
                .iter()
                .filter(|(e, rs)| !entities.contains(e) && (pictured[e].iter().any(|c| live.contains(c) || joined.contains(c)) || rs.iter().any(runs)))
                .map(|(e, _)| *e)
                .collect();
            if joined.is_empty() && joined_entities.is_empty() {
                break;
            }
            live.extend(joined);
            entities.extend(joined_entities);
        }
        let mut paused: BTreeSet<NodeId> = hidden.into_iter().filter(|c| !live.contains(c)).collect();
        paused.extend(pictured.keys().filter(|e| !entities.contains(e)));
        (paused, pictured.into_keys().collect())
    }

    /// Bring everything that runs on time to `now` (seconds, the host's
    /// clock; reactive-engine.md §10.2): layout glides, machines, traces and
    /// accumulators, queries, items entering and leaving, and the playhead
    /// while the clock plays, in that order. The accumulators of a
    /// composition no observer reaches hold still. Returns when to call
    /// again and what to redraw until then, the queries that became due,
    /// and whether anything changed.
    pub fn advance(&mut self, now: f64) -> Advanced {
        if !self.observe_all && self.reactive.relaxed() {
            self.necessity();
        }
        self.gate(now);
        let (moving, running, stepping, queries, entering, playing) = self.recorded(|s| (s.advance_motion(now), s.advance_machines(now), s.advance_steps(now), s.advance_queries(now), s.advance_presence(now), s.advance_playhead(now)));
        // What they read that nothing needed is needed from the next frame
        // (what the player reads besides is rooted: `ROOTED`).
        let read = self.settle_reads();
        if read {
            self.pipeline();
        }
        let waiting = self.pending_queries();
        let doc = Tree::of(&self.docs);
        let comp = |n: NodeId| composition::composition_of(doc, n);
        let mut live: BTreeSet<NodeId> = moving.iter().chain(&stepping).chain(&waiting).chain(&entering).filter_map(|&n| comp(n)).collect();
        // A machine is drawn where its entity is pictured.
        for r in &running {
            live.extend(doc.relations_to(r.entity).filter(|x| x.rel == RelKind::Figures).filter_map(|x| comp(x.from)));
        }
        // What the clips animate, while the playhead moves.
        if playing {
            live.extend(self.animated.iter().copied());
        }
        let (was_live, was_waiting) = std::mem::take(&mut self.advanced);
        // Anything moving keeps time going, whether or not it is drawn in a
        // composition (a trace at the root driving a shape in one; the
        // playhead with nothing animated).
        let active = !moving.is_empty() || !running.is_empty() || !stepping.is_empty() || !waiting.is_empty() || !entering.is_empty() || playing;
        // At rest, cells nothing observed needs are brought up to date a
        // few at a time (§8.7), calling again until none is stale. Not on a
        // frame something was published or written since the last: a host
        // producer moving every frame would make the sweep evaluate each
        // frame what necessity saved.
        let rest = !active && !read && self.reactive.propagations == self.evaluated_at;
        let swept = rest && self.reactive.sweep(Tree::of(&self.docs), IDLE_SWEEP);
        if swept {
            self.pipeline();
        }
        let changed = active || swept || read || !was_live.is_empty() || waiting != was_waiting;
        let next = (active || self.reactive.stale_count() > 0).then_some(now);
        self.advanced = (live.clone(), waiting);
        self.evaluated_at = self.reactive.propagations;
        Advanced { wake: Wake { next, live }, queries, changed }
    }

    /// The playhead player (timeline.md §4.1): while the clock plays, move
    /// `t` by the host time since the last advance times `rate`, wrapping
    /// between `loop_in` and `loop_out` when they differ and stopping at the
    /// end of the last clip otherwise. Whether it played.
    fn advance_playhead(&mut self, now: f64) -> bool {
        let dt = now - self.advanced_at.replace(now).unwrap_or(now);
        let Some(clock) = self.clock() else { return false };
        let field = |s: &Self, f: &str| s.resolved(clock, clock_path(f));
        if field(self, "playing") != Some(Value::Bool(true)) {
            return false;
        }
        let number = |s: &Self, f: &str| field(s, f).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let (rate, lo, hi) = (number(self, "rate"), number(self, "loop_in"), number(self, "loop_out"));
        let mut t = self.time() + dt * rate;
        let mut stop = false;
        if hi > lo {
            if t >= hi {
                t = lo + (t - hi) % (hi - lo);
            }
        } else {
            let end = match self.clips_end {
                Some(e) => e,
                None => {
                    let e = timeline_view::clips_end(self);
                    self.clips_end = Some(e);
                    e
                }
            };
            if t >= end {
                t = end;
                stop = true;
            }
        }
        self.set_time(t);
        if stop {
            let _ = self.apply_session(vec![Op::Set { id: clock, path: clock_path("playing"), slot: Slot::Const(Value::Bool(false)) }]);
        }
        true
    }

    /// Advance the transitions in flight to `now` (seconds, the host's
    /// clock) and publish where they have got to. Returns the nodes that
    /// moved, so the host renders them; empty when nothing is in flight.
    pub fn advance_motion(&mut self, now: f64) -> Vec<NodeId> {
        self.motion.clock = now;
        let mut moving = self.advance_presentation();
        moving.extend(self.reactive.glide_to(Tree::of(&self.docs), now));
        moving
    }

    /// The theme colour tokens resolve for: `dark` or light (the host's,
    /// as its canvas is drawn). Everything using a token redraws.
    pub fn set_theme(&mut self, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            self.resolve_theme();
            self.relayout();
        }
    }

    pub fn dark(&self) -> bool {
        self.dark
    }

    /// Publish each token's colour for the theme (`ProducerId::THEME`).
    fn resolve_theme(&mut self) {
        // The tokens and the scheme as last published: nothing to do (no
        // pass over the tokens on every pipeline, DS-3a).
        let key = (self.docs.tokens_shaped, self.dark);
        if self.theme_at == Some(key) {
            return;
        }
        self.theme_at = Some(key);
        let values = palette::publish(Tree::of(&self.docs), self.dark);
        if values == self.reactive.published_by(ProducerId::THEME) {
            return;
        }
        self.reactive.publish(Tree::of(&self.docs), ProducerId::THEME, values);
    }

    /// Lay a container out in a previewed order, or stop (`None`): the
    /// gap a dragged item would land in opens by ordinary layout.
    pub fn set_layout_preview(&mut self, preview: Option<layout::Preview>) {
        if self.layout_preview != preview {
            self.layout_preview = preview;
            if let Some(g) = self.reactive.glide.as_mut() {
                g.floating = self.layout_preview.as_ref().and_then(|p| p.floating).map(|(n, _)| n);
            }
            let value = self.layout_preview.clone().map(|p| std::sync::Arc::new(p) as std::sync::Arc<dyn std::any::Any + Send + Sync>);
            self.reactive.set_input(Tree::of(&self.docs), graph::InputKey::LayoutPreview, value);
            self.relayout();
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
        // The layout native of the structural root holding it.
        let doc = Tree::of(&self.docs);
        let mut at = Some(table);
        while let Some(x) = at {
            if let Some(g) = self.reactive.native_extra(graph::NativeKey::Layout(x)).and_then(|e| e.downcast_ref::<BTreeMap<NodeId, table::Grid>>()).and_then(|m| m.get(&table)) {
                return Some(g);
            }
            at = doc.live(x).and_then(|l| l.parent);
        }
        None
    }

    /// Solve the across/through network (ECM §6.1, step 3) and put its
    /// solution in the graph's physics layer: derived, never stored, read by
    /// bindings like any value. A network that does not converge keeps its
    /// previous solution and says why.
    /// The across/through network, and whether it was found again: what it
    /// is depends only on structure and slots, so it is kept until the
    /// document is written again.
    fn network_now(&mut self) -> (std::sync::Arc<crate::port::Network>, bool) {
        let edits = self.docs.network_shaped;
        match &self.network {
            Some((at, net)) if *at == edits => (net.clone(), false),
            _ => {
                let net = std::sync::Arc::new(crate::port::network(Tree::of(&self.docs)));
                self.network = Some((edits, net.clone()));
                (net, true)
            }
        }
    }

    fn solve_network(&mut self) {
        let (net, fresh) = self.network_now();
        // Its islands are native cells (handoff §4.10), made where the
        // structure changes (`regraph`); this catches a write that
        // reached none (a session edit's constant).
        if fresh {
            self.sync_islands(&net, true);
        }
    }

    /// Make the layout natives (handoff §4.10, RE-5.4): one per structural
    /// root, and the pins, keeping each that is as it would be made now.
    fn sync_layout(&mut self, run: bool) {
        // The first time, every structural root and pinned part; after
        // that, the roots the nodes a structural change shaped belong to,
        // and the uses among them (DS-3b: no pass over every group, table
        // and use on each structural change).
        let doc = Tree::of(&self.docs);
        let use_ty = TypeId::lookup("use").expect("registered");
        let (mut roots, mut pinned_by) = match (self.layout_roots.take(), self.pinned_by_use.take()) {
            (Some(r), Some(p)) => (r, p),
            _ => {
                self.layout_touched.clear();
                let roots: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
                let pinned: BTreeMap<NodeId, BTreeSet<NodeId>> = doc.nodes_of_type(use_ty).map(|u| (u, layout::pinned_parts_of(doc, u))).filter(|(_, p)| !p.is_empty()).collect();
                let all = layout::structural_roots(doc);
                self.layout_touched.extend(all);
                (roots, pinned)
            }
        };
        let touched = std::mem::take(&mut self.layout_touched);
        if touched.is_empty() {
            self.layout_roots = Some(roots);
            self.pinned_by_use = Some(pinned_by);
            return;
        }
        // The uses whose pinned parts may have changed: those touched, and
        // the owners of copies touched.
        let mut uses: BTreeSet<NodeId> = BTreeSet::new();
        for &t in &touched {
            if doc.node(t).is_some_and(|n| n.ty == use_ty) {
                uses.insert(t);
            }
            if let Some((owner, _)) = t.expanded_parts()
                && doc.node(owner).is_some_and(|n| n.ty == use_ty)
            {
                uses.insert(owner);
            }
        }
        let mut changed_parts: BTreeSet<NodeId> = BTreeSet::new();
        for u in uses {
            let now = layout::pinned_parts_of(doc, u);
            let was = pinned_by.remove(&u).unwrap_or_default();
            changed_parts.extend(now.symmetric_difference(&was).copied());
            if !now.is_empty() {
                pinned_by.insert(u, now);
            }
        }
        let pinned: BTreeSet<NodeId> = pinned_by.values().flatten().copied().collect();
        // The top of the chain of placing nodes above `n`: the structural
        // root whose layout covers it.
        let top = |n: NodeId| -> Option<NodeId> {
            let mut at = if layout::lays_out(doc, n) { Some(n) } else { doc.live(n).and_then(|x| x.parent) };
            let mut found = None;
            while let Some(x) = at.filter(|&x| layout::lays_out(doc, x)) {
                found = Some(x);
                at = doc.live(x).and_then(|y| y.parent);
            }
            found
        };
        // A node's children too: whether a child is a root depends on
        // whether its parent lays out, so a `layout` facet added to or taken
        // from `t` makes or unmakes the roots just below it.
        let mut candidates: BTreeSet<NodeId> = BTreeSet::new();
        for &t in touched.iter().chain(&changed_parts) {
            candidates.insert(t);
            candidates.extend(top(t));
            candidates.extend(doc.children(t).iter().copied());
        }
        let mut want: Vec<(graph::NativeKey, std::sync::Arc<dyn graph::Native>)> = Vec::new();
        let mut drop: Vec<graph::NativeKey> = Vec::new();
        let mut rebuilt: Vec<NodeId> = Vec::new();
        for c in candidates {
            let is_root = layout::lays_out(doc, c) && !doc.live(c).and_then(|x| x.parent).is_some_and(|p| layout::lays_out(doc, p));
            let k = graph::NativeKey::Layout(c);
            if is_root {
                let n = layout::LayoutNative::new(doc, c, &pinned);
                roots.insert(c, n.nodes());
                rebuilt.push(c);
                if self.reactive.native(k).and_then(|x| x.as_any().downcast_ref::<layout::LayoutNative>()) != Some(&n) {
                    want.push((k, std::sync::Arc::new(n)));
                }
            } else if roots.remove(&c).is_some() || self.reactive.native(k).is_some() {
                drop.push(k);
                rebuilt.push(c);
            }
        }
        // The pins of parts no layout holds.
        let uncovered: BTreeSet<NodeId> = pinned.iter().copied().filter(|&p| top(p).is_none_or(|t| !roots.get(&t).is_some_and(|ns| ns.contains(&p)))).collect();
        let pins = layout::PinsNative::new(&uncovered);
        let keep_pins = !uncovered.is_empty();
        if keep_pins && self.reactive.native(graph::NativeKey::Pins).and_then(|x| x.as_any().downcast_ref::<layout::PinsNative>()) != Some(&pins) {
            want.push((graph::NativeKey::Pins, std::sync::Arc::new(pins)));
        } else if !keep_pins && self.reactive.native(graph::NativeKey::Pins).is_some() {
            drop.push(graph::NativeKey::Pins);
        }
        self.layout_roots = Some(roots);
        self.pinned_by_use = Some(pinned_by);
        // Rooted, so a hidden composition stays laid out: an unrun native
        // claims nothing, so a host could take a placement first, and a
        // capture draws what no observer shows (handoff §4.10, RE-5.6).
        let mut batch: Vec<(graph::NativeKey, Option<(std::sync::Arc<dyn graph::Native>, bool)>)> = drop.into_iter().map(|k| (k, None)).collect();
        batch.extend(want.into_iter().map(|(k, n)| (k, Some((n, true)))));
        if !batch.is_empty() {
            self.reactive.set_natives(Tree::of(&self.docs), batch, run);
        }
        self.glide_specs_of(&rebuilt);
    }

    /// What decides whether a plugged pair mates, read from the document by
    /// the contexts resolver (a notation, an inline style, a context's name),
    /// and a session link or unlink: grouping and validation look again
    /// (RE-5.5).
    fn prime_grouping<'a>(&mut self, ops: impl Iterator<Item = &'a Op>) {
        let touches = |op: &Op| match op {
            Op::Set { path, .. } => matches!(path.facet, FacetKind::Composition | FacetKind::Template | FacetKind::Use | FacetKind::ContextOf | FacetKind::Affords | FacetKind::ContextDef),
            Op::Link { .. } | Op::Unlink { .. } => true,
            _ => false,
        };
        if ops.into_iter().any(touches) {
            self.reactive.prime_natives([graph::NativeKey::Clusters]);
        }
    }

    /// A reorder that moved nothing else (not structural to the graph)
    /// still changes how its parent lays out.
    fn prime_reorders<'a>(&mut self, ops: impl Iterator<Item = &'a Op>) {
        let doc = Tree::of(&self.docs);
        let shaped: BTreeSet<NodeId> = ops.filter_map(|op| match op { Op::Reorder { id, .. } => Some(*id), _ => None }).flat_map(|id| std::iter::once(id).chain(doc.live(id).and_then(|n| n.parent))).collect();
        self.prime_layout(&shaped);
    }

    /// Make a repeat native for each drawn repeat and conditional, and drop
    /// the rest, when the document's structure changed.
    fn sync_repeats(&mut self, run: bool) {
        // The natives of the repeats drawn and undrawn since the last sync
        // (`update_drawn`, `ensure_drawn`), not of every group.
        self.ensure_drawn();
        let (add, remove) = std::mem::take(&mut self.repeat_natives);
        if add.is_empty() && remove.is_empty() {
            return;
        }
        let mut batch: Vec<(graph::NativeKey, Option<(std::sync::Arc<dyn graph::Native>, bool)>)> = Vec::new();
        for r in remove {
            let k = graph::NativeKey::Repeat(r);
            if self.reactive.native(k).is_some() {
                batch.push((k, None));
            }
        }
        for r in add {
            let k = graph::NativeKey::Repeat(r);
            if self.reactive.native(k).is_none() {
                batch.push((k, Some((std::sync::Arc::new(RepeatNative::new(r)), true))));
            }
        }
        if !batch.is_empty() {
            self.reactive.set_natives(Tree::of(&self.docs), batch, run);
        }
    }

    /// The structure oracle (structure-in-the-graph.md §3.5): every keyed
    /// layout that is up to date, laid out again from the store as it is
    /// now, must give what its native claims, claim nothing more, and give
    /// the same tables' grids. (A layout run has no errors to compare: it
    /// always places.)
    /// What a missed key leaves stale fails here, whatever primed it or
    /// not. Skipped mid-glide, when claims are eased.
    #[cfg(feature = "structure-oracle")]
    pub(crate) fn check_keyed_layouts(&self) -> Result<(), String> {
        if self.reactive.gliding() {
            return Ok(());
        }
        let doc = Tree::of(&self.docs);
        for root in layout::structural_roots(doc) {
            let k = graph::NativeKey::Layout(root);
            let Some(native) = self.reactive.native(k) else { return Err(format!("layout root {root} has no native")) };
            if !native.keyed() || !self.reactive.native_current(k) {
                continue;
            }
            let (values, grids) = layout::lay_out(self, root);
            for (leaf, v) in values.iter().filter(|(l, _)| native.outputs().contains(l)) {
                let kept = self.reactive.signal(*leaf).filter(|s| s.producer == ProducerId::LAYOUT).map(|s| &s.value);
                let same = match (kept, v) {
                    (Some(Value::Number(a)), Value::Number(b)) => (a - b).abs() < 1e-9,
                    (Some(a), b) => a == b,
                    (None, _) => false,
                };
                if !same {
                    return Err(format!("layout of {root}: {leaf} is {kept:?}, laid out again {v:?}"));
                }
            }
            // Nor does it claim what laying out again no longer places (a
            // child gone out of flow).
            if let Some(extra) = self.reactive.claims_of(k).into_iter().find(|l| !values.contains_key(l)) {
                return Err(format!("layout of {root} claims {extra}, which laying out again does not place"));
            }
            let kept = self.reactive.native_extra_of(k).and_then(|e| e.downcast_ref::<BTreeMap<NodeId, table::Grid>>());
            if kept.map_or(!grids.is_empty(), |g| *g != grids) {
                return Err(format!("layout of {root}: its tables' grids differ laid out again"));
            }
        }
        Ok(())
    }

    /// Hand the graph what the documents' writes touched since this was
    /// last done: the keyed cells that read it run in the next pass. Marks
    /// only; called where the graph is primed (structure-in-the-graph.md
    /// §3.3), never inside `run`.
    fn drain_touched(&mut self) {
        let keys = self.docs.take_touched();
        if !keys.is_empty() {
            self.reactive.prime_keys(keys);
        }
    }

    /// Run the pinned layouts whose subtree holds one of `shaped`, and the
    /// pins: what reads `constrained`, which scans every `use`. An unpinned
    /// layout is keyed (`Native::keyed`): what it read being written runs
    /// it, with no rule (structure-in-the-graph.md DS-9).
    fn prime_layout(&mut self, shaped: &BTreeSet<NodeId>) {
        if shaped.is_empty() {
            return;
        }
        let doc = Tree::of(&self.docs);
        let gone = &self.docs.gone;
        let mut keys: BTreeSet<graph::NativeKey> = BTreeSet::from([graph::NativeKey::Pins]);
        for &n in shaped {
            let mut at = Some(n);
            while let Some(x) = at {
                let k = graph::NativeKey::Layout(x);
                if self.reactive.native(k).is_some_and(|n| !n.keyed()) {
                    keys.insert(k);
                }
                at = doc.live(x).and_then(|l| l.parent).or_else(|| gone.get(&x).and_then(|w| w.parent));
            }
        }
        self.reactive.prime_natives(keys);
    }

    /// Make the network's islands native cells, keeping each one whose
    /// unknowns and equations did not change, and dropping the rest.
    fn sync_islands(&mut self, net: &crate::port::Network, run: bool) {
        let want: std::collections::BTreeMap<graph::NativeKey, crate::port::IslandNative> = net.island_natives().into_iter().collect();
        let mut batch: Vec<(graph::NativeKey, Option<(std::sync::Arc<dyn graph::Native>, bool)>)> = Vec::new();
        for k in self.reactive.native_keys() {
            if matches!(k, graph::NativeKey::Island(_)) && !want.contains_key(&k) {
                batch.push((k, None));
            }
        }
        for (k, island) in want {
            let same = self.reactive.native(k).and_then(|n| n.as_any().downcast_ref::<crate::port::IslandNative>()) == Some(&island);
            if !same {
                batch.push((k, Some((std::sync::Arc::new(island), true))));
            }
        }
        if !batch.is_empty() {
            self.reactive.set_natives(Tree::of(&self.docs), batch, run);
        }
    }

    /// Why the across/through network kept its last solution, if it did.
    /// The transaction that makes a component named `name` from the
    /// selected pictures (component-model.md §5.5, `make`).
    pub fn make_component(&self, selection: &[NodeId], name: &str, actor: Actor) -> Result<(make::Made, TransactionInput), String> {
        let read = |l: Leaf| self.resolved(l.node, l.path);
        let shapes = selection.iter().all(|&n| Tree::of(&self.docs).live(n).is_some_and(|x| x.ty.name() != "use"));
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
            make::make_main(Tree::of(&self.docs), selection, top_left, name)?
        } else {
            make::make(Tree::of(&self.docs), &read, selection, name)?
        };
        let tx = TransactionInput { label: format!("make component {name}"), actor, ts: 0, idempotency_key: None, ops: made.ops.clone(), exact: false };
        Ok((made, tx))
    }

    /// Overrides that apply nothing, and why (component-model.md §5.3).
    pub fn stale_overrides(&self) -> Vec<(NodeId, String)> {
        expand::stale_overrides(Tree::of(&self.docs))
    }

    /// What the last expansion could not derive, if anything.
    pub fn expansion_problems(&self) -> &[expand::Problem] {
        &self.expansion_problems
    }

    pub fn physics_error(&self) -> Option<&str> {
        // The island with the lowest unknown that kept its last solution.
        self.reactive.native_problems.iter().find(|(k, _)| matches!(k, graph::NativeKey::Island(_))).map(|(_, why)| why.as_str())
    }

    pub fn session_version(&self) -> u64 {
        self.session_version
    }

    /// The first live node of a type, from the type index: the clock, the
    /// session root, the selection (each of which there is one of).
    fn live_of_type(&self, name: &str) -> Option<NodeId> {
        let ty = TypeId::lookup(name)?;
        let doc = Tree::of(&self.docs);
        doc.nodes_of_type(ty).find(|&n| doc.is_live(n))
    }

    /// The selected nodes, in pick order, with how each was reached
    /// (`data.panel`, `data.chain`). Deleted nodes drop out.
    pub fn selection(&self) -> Vec<(NodeId, std::collections::BTreeMap<String, Value>)> {
        let Some(sel) = self.live_of_type("selection") else { return Vec::new() };
        Tree::of(&self.docs)
            .relations_from(sel)
            .filter(|r| r.rel == RelKind::Selects && Tree::of(&self.docs).is_live(r.to))
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
                let ids = self.docs.authored.next_session_ids(2);
                let ty = |n: &str| TypeId::lookup(n).expect("registered");
                ops.push(Op::Create { id: ids[0], ty: ty("session"), parent: None, order: OrderKey::first(), facets: vec![] });
                ops.push(Op::Create { id: ids[1], ty: ty("selection"), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![] });
                ids[1]
            }
        };
        for r in Tree::of(&self.docs).relations_from(sel).filter(|r| r.rel == RelKind::Selects) {
            ops.push(Op::Unlink { id: r.id });
        }
        for ((to, data), id) in items.iter().zip(self.docs.authored.next_session_relation_ids(items.len())) {
            ops.push(Op::Link { rel: Relation { id, from: sel, to: *to, rel: RelKind::Selects, data: data.clone() } });
        }
        self.apply_session(ops)?;
        // Bindings that read the selection follow it.
        if Tree::of(&self.docs).nodes_with(facet::FacetKind::Selection).next().is_some() {
            self.pipeline();
        }
        Ok(())
    }

    /// Keep the graph in step with a transaction that just applied.
    /// `full_inverse` is everything the transaction took or replaced,
    /// derived nodes included (a deleted entity's ports); `expanded` is what
    /// expansion wrote for it, to which settling adds the repeats' copies.
    /// Derived ops are never history: undo, redo and replay re-derive.
    fn after_transaction(&mut self, tx: &Transaction, expanded: &mut Vec<Op>, full_inverse: &[Op]) {
        // A repeat held as feedback tries again (`restless`).
        self.feedback.clear();
        self.restless.clear();
        // Outside a gesture the overlay holds only a live solve's pose (a
        // producer's publish or a playhead move re-solves the clusters).
        // Nothing else owns it, so it would shadow what this transaction
        // wrote, even after the cluster is gone: drop it, and re-solve
        // against the new document below. A gesture's own commit has taken
        // the gesture but still holds its drivers, and clears its previews
        // itself.
        let live = self.gesture.is_none() && self.drivers.is_empty() && self.fingers.is_empty() && !self.reactive.overlay().is_empty();
        if live {
            self.reactive.clear_overlay(Tree::of(&self.docs));
        }
        self.geometry.note_ops(tx.ops.iter().chain(tx.derived_ops.iter()).chain(tx.inverse.iter()).chain(expanded.iter()));
        let structural = tx.ops.iter().chain(tx.inverse.iter()).chain(expanded.iter()).any(|op| match op {
            // A link can be a connection's end, which derives a signal.
            Op::Create { .. } | Op::Delete { .. } | Op::AddFacet { .. } | Op::RemoveFacet { .. } | Op::Link { .. } | Op::Unlink { .. } | Op::Reparent { .. } => true,
            Op::Set { slot, path, .. } => slot.expr().is_some() || names_context(path) || rederives(path),
            // The first provider among siblings answers `@name`.
            Op::Reorder { id, .. } => provides(Tree::of(&self.docs), *id),
        });
        // The clips' end reads clips, keys, relations and the transitions a
        // keyed state runs on: any edit may move it, so it is read again
        // after every transaction (lazily, and only while playing with no
        // loop range); a playhead move does not. Likewise the clocked
        // machine instances.
        self.clips_end = None;
        self.machines.reclock();
        let clips = Self::touches_clips(tx.ops.iter().chain(expanded.iter()));
        if structural {
            self.regraph(tx.ops.iter().chain(full_inverse).chain(expanded.iter()));
            self.resample();
        } else {
            // What re-expansion wrote counts too: an override updated in
            // place changes its derived node's leaf, and whatever reads it.
            let changed: Vec<Leaf> = tx
                .ops
                .iter()
                .chain(expanded.iter())
                .filter_map(|op| match op {
                    Op::Set { id, path, .. } => Some(Leaf {
                        node: *id,
                        path: *path,
                    }),
                    _ => None,
                })
                .collect();
            self.prime_reorders(tx.ops.iter().chain(expanded.iter()));
            self.prime_grouping(tx.ops.iter().chain(expanded.iter()));
            self.drain_touched();
            self.reactive.invalidate(Tree::of(&self.docs), changed);
            // A clip's own leaves set (its `prop`, a key): every clip is
            // sampled, so what one stops driving is withdrawn; otherwise
            // the running ones (a constant one reads may have changed).
            if clips {
                self.resample();
            } else {
                self.resample_running();
            }
        }
        // Repeats whose count this changed get their copies, as part of
        // this transaction's derived structure: those whose natives ran, or
        // that the transaction drew or undrew.
        let copies = self.settle_repeats();
        expanded.extend(copies);
        self.rebuild_clusters();
        self.pipeline_from(live);
    }

    /// The authored document: what is saved, and what mints ids. It holds
    /// authored nodes only from phase 2c on (reactive-engine.md §12.1); a
    /// reader that can meet a derived node reads `tree()`.
    pub fn authored(&self) -> &Document {
        &self.docs.authored
    }

    /// The read interface for authored and derived nodes alike
    /// (reactive-engine.md §12.1).
    pub fn tree(&self) -> Tree<'_> {
        Tree::of(&self.docs)
    }

    /// The document, without copying it; the store's history goes with it.
    pub fn into_document(self) -> Document {
        self.docs.authored
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
        let ops = composition::link_ops(self.tree(), a, b, keep)?;
        Ok(TransactionInput { label: "link".into(), actor, ts: 0, idempotency_key: None, ops, exact: false })
    }

    /// Apply a transaction atomically. Either every op applies and the
    /// version advances, or nothing applies and every error is reported.
    pub fn apply(&mut self, input: TransactionInput) -> Result<Applied, Vec<ApplyError>> {
        // A transaction from outside a running gesture is classified against
        // it (`gesture`) and applied without its previews.
        let conflict = self.gesture.as_ref().and_then(|g| gesture::conflict(Tree::of(&self.docs), &input.ops, g, &self.drivers));
        let glides = self.gesture.as_ref().and(self.reactive.glide.clone());
        let suspended = self.suspend_gesture();
        let result = self.apply_now(input);
        if let Some(s) = suspended {
            let moved = match (&result, &self.gesture) {
                (Ok(a), Some(g)) if !a.replayed => self.history.past.last().and_then(|tx| gesture::moved(&tx.derived_ops, g)),
                _ => None,
            };
            match conflict.filter(|_| result.is_ok()).or(moved) {
                Some(reason) => self.end_gesture(GestureStatus::Cancelled(reason)),
                None => {
                    self.resume_gesture(s);
                    self.reactive.glide_keep(Tree::of(&self.docs), glides);
                }
            }
        }
        result
    }

    fn apply_now(&mut self, mut input: TransactionInput) -> Result<Applied, Vec<ApplyError>> {
        // A built-in colour token used for the first time joins the
        // document, in the same transaction (palette.rs).
        // After the transaction's own ops: its ids come after theirs, and
        // bindings are checked once all have applied.
        let theme = palette::ensure_ops(Tree::of(&self.docs), &input.ops);
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
        // An exact edit is tried first on a scratch copy, as it would apply
        // here: if the solve leaves a request unmet, it is refused before
        // this store is touched (DS-7; review: a retract here would cancel a
        // gesture, drop redo and keep the idempotency key).
        if input.exact {
            let mut dry = input.clone();
            dry.exact = false;
            dry.idempotency_key = None;
            let applied = self.scratch().apply_now(dry)?;
            if let Some(unmet) = applied.solver.as_ref().map(|s| &s.unmet).filter(|u| !u.is_empty()) {
                let lines: Vec<String> = unmet.iter().map(|u| format!("{} requested {} but constraints allow {}", u.leaf, crate::value::fmt_num(u.requested), crate::value::fmt_num(u.solved))).collect();
                return Err(vec![ApplyError { op: 0, kind: ErrorKind::Unsatisfiable, message: format!("exact transaction not honoured: {}", lines.join("; ")) }]);
            }
        }
        // The constraints' problems before, so the reply names those this
        // transaction brought (review of DS-7: after `after_transaction` it
        // was too late to see the grouping's).
        let problems_before: Vec<(NodeId, String)> = self.cluster_problems().into_iter().map(|p| (p.node, p.message)).collect();
        let Run { mut tx, mut expanded, full_inverse, problems, shared, rollback, recorded: _, solved } = self.run(input, false)?;
        if let Some(found) = problems {
            self.store_found(found);
        }
        self.check_owners("a transaction");
        self.after_transaction(&tx, &mut expanded, &full_inverse);
        let held = self.keep_solved(&mut tx, solved, &expanded);
        let mut solver = self.post_apply(&mut tx, &held);
        // What the reply tells: the constraints' problems this transaction
        // brought (a malformed or duplicate constraint, a cycle, a pose
        // that doesn't hold), and why a solve failed, unless that cluster
        // was in trouble already.
        // By node: a constraint already in trouble whose residual moved is
        // not news (review of DS-7, Opus).
        let troubled: BTreeSet<NodeId> = problems_before.iter().map(|(n, _)| *n).collect();
        let new: Vec<solve::SolverProblem> = self.cluster_problems().into_iter().filter(|p| !troubled.contains(&p.node)).map(|p| solve::SolverProblem { node: p.node, message: p.message }).collect();
        if !new.is_empty() || solver.as_ref().is_some_and(|s| !s.problems.is_empty()) {
            let s = solver.get_or_insert_with(SolverSummary::default);
            s.problems.retain(|p| !troubled.contains(&p.node));
            s.problems.extend(new);
            if s.is_empty() {
                solver = None;
            }
        }
        if shared {
            self.machines.shared += 1;
        }
        tx.solver = solver.clone();
        self.note_changes(tx.ops.iter().chain(&tx.derived_ops).chain(&expanded), &full_inverse);
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
        self.commit(rollback);
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
        let Run { tx: mut applied, mut expanded, mut full_inverse, problems, shared, rollback, recorded, solved } = self.run(input, true)?;
        self.commit(rollback);
        if shared {
            self.machines.shared += 1;
        }
        if let Some(found) = problems {
            self.store_found(found);
        }
        self.check_owners("a transaction");
        self.after_transaction(&applied, &mut expanded, &full_inverse);
        let held = self.keep_solved(&mut applied, solved, &expanded);
        self.apply_recorded(&mut applied, recorded, &mut full_inverse);
        let _ = self.release(&mut applied, &held, 0);
        self.note_changes(applied.ops.iter().chain(&applied.derived_ops).chain(&expanded), &full_inverse);
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
        let snap = args_set(&tx.undo_ops());
        let input = TransactionInput {
            label: format!("undo: {}", tx.label),
            actor: Actor::System,
            ts,
            idempotency_key: None,
            ops: tx.undo_ops(),
            exact: false,
        };
        Some(match self.run(input, true) {
            Ok(Run { tx: mut applied, mut expanded, mut full_inverse, problems, shared, rollback, recorded, solved }) => {
                self.commit(rollback);
                self.machines.snap.extend(snap);
                if shared {
                    self.machines.shared += 1;
                }
                if let Some(found) = problems {
                    self.store_found(found);
                }
                self.check_owners("a transaction");
                self.after_transaction(&applied, &mut expanded, &full_inverse);
                self.apply_recorded(&mut applied, recorded, &mut full_inverse);
                // Undo has no solve: a copy it leaves in no cluster is let go.
                let held: Vec<Leaf> = solved.iter().map(|(l, _)| *l).collect();
                let _ = self.release(&mut applied, &held, 0);
                self.note_changes(applied.ops.iter().chain(&applied.derived_ops).chain(&expanded), &full_inverse);
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
        let snap = args_set(&tx.replay_ops());
        // Requested and derived ops, no solve: the log is the ground truth.
        let input = TransactionInput {
            label: format!("redo: {}", tx.label),
            actor: Actor::System,
            ts,
            idempotency_key: None,
            ops: tx.replay_ops(),
            exact: false,
        };
        Some(match self.run(input, true) {
            Ok(Run { tx: mut applied, mut expanded, mut full_inverse, problems, shared, rollback, recorded, solved }) => {
                self.commit(rollback);
                self.machines.snap.extend(snap);
                if shared {
                    self.machines.shared += 1;
                }
                if let Some(found) = problems {
                    self.store_found(found);
                }
                self.check_owners("a transaction");
                self.after_transaction(&applied, &mut expanded, &full_inverse);
                let held = self.keep_solved(&mut applied, solved, &expanded);
                self.apply_recorded(&mut applied, recorded, &mut full_inverse);
                let _ = self.release(&mut applied, &held, 0);
                self.note_changes(applied.ops.iter().chain(&applied.derived_ops).chain(&expanded), &full_inverse);
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

    /// The solved copy leaves `solved` held before this transaction that its
    /// expansion and repeat settling rewrote or removed: undo writes each
    /// solved value back after the expansion it undoes (`apply_recorded`),
    /// and after undoing any solver write of the same transaction, which
    /// comes later in the inverse. Returns the leaves held.
    fn keep_solved(&mut self, tx: &mut Transaction, solved: Vec<(Leaf, Slot)>, expanded: &[Op]) -> Vec<Leaf> {
        let held: Vec<Leaf> = solved.iter().map(|(l, _)| *l).collect();
        // A leaf expansion wrote, even to the value it had: undo's
        // expansion writes it again, so the restore is needed.
        let written: BTreeSet<Leaf> = expanded.iter().filter_map(|op| match op {
            Op::Set { id, path, .. } => Some(Leaf { node: *id, path: *path }),
            _ => None,
        }).collect();
        let doc = Tree::of(&self.docs);
        for (leaf, was) in solved {
            if written.contains(&leaf) || doc.live(leaf.node).and_then(|_| doc.leaf(leaf.node, leaf.path)) != Some(&was) {
                tx.inverse.push(Op::Set { id: leaf.node, path: leaf.path, slot: was });
            }
        }
        held
    }

    /// What needs the whole transaction in place (constraint-system.md §7):
    /// every constraint is well formed, bindings and clusters form no cycle,
    /// and every cluster is solved, with leaves this transaction set treated
    /// as drivers. The solver's writes are applied and recorded as
    /// `derived_ops`. Runs on the committed document; the caller rolls back
    /// on `Err`.
    /// Solve what the transaction moved and write the solutions as its
    /// derived ops. Nothing here refuses (DS-7, decided by the user): a
    /// constraint that can't be used and a cycle are problems of the
    /// grouping, and a cluster whose solve fails keeps the values it had
    /// when its solve began, the transaction's own writes included, and is
    /// posed so that only a change to its inputs solves it again. Its
    /// problem is read from the pose (`cluster_problems`).
    fn post_apply(&mut self, tx: &mut Transaction, held: &[Leaf]) -> Option<SolverSummary> {
        let last = tx.ops.len().saturating_sub(1);
        self.regroup();
        let mut summary = SolverSummary::default();
        self.release(tx, held, last);
        if self.clusters.list.is_empty() {
            return None;
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
        let mut solved = BTreeMap::new();
        while let Some(c) = self.next_to_solve(&drivers, &solved) {
            *solved.entry(c.key()).or_insert(0) += 1;
            let c = &c;
            if self.unmoved(c, &drivers) {
                continue;
            }
            let result = {
                let read = |l: Leaf| self.reactive.settled(Tree::of(&self.docs), l);
                solve::solve_cluster(Tree::of(&self.docs), &read, c, &drivers, &self.fingers)
            };
            if !solve::holds(&result) {
                let names: Vec<String> = result.culprits.iter().map(|c| c.to_string()).collect();
                let message = format!(
                    "constraints of cluster {} cannot all hold (residual {:.3e}); likely at fault: {}",
                    c.id,
                    result.max_required_residual,
                    if names.is_empty() { "unknown".to_owned() } else { names.join(", ") }
                );
                let on = if result.culprits.is_empty() { vec![c.id] } else { result.culprits.clone() };
                summary.problems.extend(on.into_iter().map(|node| solve::SolverProblem { node, message: message.clone() }));
                summary.clusters.push(c.id);
                self.pose(c);
                continue;
            }
            let mut changed = Vec::new();
            for (leaf, v) in result.values {
                let current = Tree::of(&self.docs).leaf(leaf.node, leaf.path).and_then(|s| s.constant()).and_then(|x| x.as_f64());
                if current.is_some_and(|c| (c - v).abs() < 1e-9) {
                    continue;
                }
                let op = Op::Set { id: leaf.node, path: leaf.path, slot: Slot::Const(Value::Number(v)) };
                let inverse = op::apply_op(&mut self.docs, last, &op, tx.version).expect("a solved leaf is a live constant slot");
                self.geometry.note_ops([&op]);
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
            self.drain_touched();
            self.reactive.invalidate(Tree::of(&self.docs), changed);
            self.pose(c);
            summary.unmet.extend(result.unmet);
        }
        // The watchers read the pose just written, so `problems` is current.
        self.reactive.flush(Tree::of(&self.docs));
        // Drivers that are not variables of any cluster were plain sets;
        // only a moved node is a cause worth telling the model about.
        (!summary.is_empty()).then_some(summary)
    }

    /// A recorded transaction's writes to copies (the solver's, or their
    /// inverses), applied as `post_apply` writes them: after expansion, the
    /// owners' check and `after_transaction`, never translated into
    /// overrides. A copy the transaction left gone takes none.
    fn apply_recorded(&mut self, tx: &mut Transaction, recorded: Vec<Op>, full_inverse: &mut Vec<Op>) {
        let mut changed = Vec::new();
        let mut applied = Vec::new();
        for op in recorded {
            let Op::Set { id, path, .. } = &op else { continue };
            let leaf = Leaf { node: *id, path: *path };
            if !Tree::of(&self.docs).is_live(*id) {
                continue;
            }
            if let Ok(inverse) = op::apply_op(&mut self.docs, 0, &op, tx.version) {
                self.geometry.note_ops([&op]);
                changed.push(leaf);
                tx.inverse.extend(inverse.iter().cloned());
                full_inverse.extend(inverse);
                applied.push(op);
            }
        }
        if !changed.is_empty() {
            // A recorded write can put a binding back: the graph reads it.
            self.regraph(applied.iter());
            self.reactive.invalidate(Tree::of(&self.docs), changed);
        }
        tx.derived_ops.extend(applied);
    }

    /// Let go of the copies whose solver leaves `held` were held before the
    /// transaction and that no cluster holds `now`: each such leaf goes back
    /// to what its owner's expansion wants, written as the solver writes,
    /// so undo puts the solved value back. A copy still in a cluster keeps
    /// it, as a variable or as a fixed or driven input.
    fn release(&mut self, tx: &mut Transaction, held: &[Leaf], at: usize) {
        let released: Vec<Leaf> = {
            let doc = Tree::of(&self.docs);
            let now = self.clusters.copies();
            held.iter().filter(|l| doc.is_live(l.node) && !now.contains(&l.node)).copied().collect()
        };
        if released.is_empty() {
            return;
        }
        let mut changed = Vec::new();
        let mut ops = Vec::new();
        for (leaf, slot) in self.owners.wanted_slots(Tree::of(&self.docs), &self.repeats, &released) {
            if Tree::of(&self.docs).leaf(leaf.node, leaf.path) == Some(&slot) {
                continue;
            }
            let op = Op::Set { id: leaf.node, path: leaf.path, slot };
            // A template's binding can fail `apply_op`'s checks: the copy
            // keeps the solved value rather than refuse (DS-7).
            let Ok(inverse) = op::apply_op(&mut self.docs, at, &op, tx.version) else { continue };
            self.geometry.note_ops([&op]);
            tx.inverse.extend(inverse);
            changed.push(leaf);
            ops.push(op);
        }
        if !ops.is_empty() {
            // The template's value may be a binding: the graph reads it.
            self.regraph(ops.iter());
            self.reactive.invalidate(Tree::of(&self.docs), changed);
        }
        tx.derived_ops.extend(ops);
    }

    /// Apply the ops, expand, and check; on success the document holds the
    /// transaction and the token can put it back (`commit` or `abort`). On
    /// failure the document is as it was. The ops apply to the store's own
    /// document with its journal open (reactive-engine handoff §4.3).
    /// `history`: the ops are a recorded transaction's (undo, redo, replay).
    fn run(&mut self, input: TransactionInput, history: bool) -> Result<Run, Vec<ApplyError>> {
        self.exact();
        let mut input = input;
        // A recorded transaction's sets on copies are the solver's writes
        // and their inverses, not edits: they are not translated into
        // overrides, and apply after the transaction (`apply_recorded`).
        let recorded: Vec<Op> = if history {
            let (recorded, ops) = std::mem::take(&mut input.ops).into_iter().partition(|op| matches!(op, Op::Set { id, .. } if id.is_expanded()));
            input.ops = ops;
            recorded
        } else {
            Vec::new()
        };
        // A solver leaf of a copy this transaction sets itself is written
        // even where what is wanted for it comes out as before.
        self.owners.edit(input.ops.iter().filter_map(|op| match op {
            Op::Set { id, path, .. } if id.is_expanded() && solve::writes(*path) => Some(Leaf { node: *id, path: *path }),
            _ => None,
        }).collect());
        // Edits aimed inside an instance become edits to what it derives from
        // (component-model.md §7): an argument, a slider, an override.
        input.ops = expand::translate(Tree::of(&self.docs), &self.reactive, std::mem::take(&mut input.ops)).map_err(|e| vec![e])?;
        // A state pinned on its main records number edits to the component's
        // parts as that state's pose (machine.rs `record`).
        let mut shared = false;
        if let Some(pin) = self.machines.pinned.clone() {
            // Counted once the transaction is accepted (`count_shared`).
            shared = machine::shared_edit(Tree::of(&self.docs), &pin, &input.ops);
            input.ops = machine::record(Tree::of(&self.docs), &pin, std::mem::take(&mut input.ops)).map_err(|message| vec![ApplyError { op: 0, kind: ErrorKind::BadInstance, message }])?;
        }
        // What `attachment::validate` compares against: the points attached
        // before.
        let attached: Vec<NodeId> = Tree::of(&self.docs).nodes_with(FacetKind::Attachment).filter(|p| !p.is_expanded()).collect();
        // The leaves the solver writes on the copies a cluster holds, as it
        // left them: expansion and repeat settling remove a copy without
        // knowing the solved value (`keep_solved`).
        let solved: Vec<(Leaf, Slot)> = {
            let doc = Tree::of(&self.docs);
            self.owners
                .held()
                .iter()
                .filter(|&&n| doc.is_live(n))
                .flat_map(|&n| solve::TRANSFORM_LEAVES.map(|f| Leaf { node: n, path: LeafPath::new(FacetKind::Transform2d, f).expect("declared leaf") }))
                .filter_map(|l| doc.leaf(l.node, l.path).map(|s| (l, s.clone())))
                .collect()
        };
        self.docs.begin();
        let docs = &mut self.docs;
        let stamp = docs.authored.version().next();
        let mut inverse = Vec::new();
        let mut errors = Vec::new();
        for (i, op) in input.ops.iter().enumerate() {
            if let Some(reason) = expand::refuse(op) {
                errors.push(ApplyError { op: i, kind: ErrorKind::BadId, message: reason });
                continue;
            }
            match op::apply_op(docs, i, op, stamp) {
                Ok(inv) => inverse.extend(inv),
                Err(e) => errors.push(e),
            }
        }
        // Instances re-derive what their definitions say, before the checks
        // that read them (a connection to an expanded port).
        let mut expanded = Vec::new();
        let mut problems = None;
        if errors.is_empty() {
            // Only the owners these ops can change (stage 2).
            if let Some(which) = self.owners.affected(Tree::of(docs), &input.ops, &inverse) {
                let (ops, delta) = self.owners.expand(docs, stamp, &self.repeats, which);
                expanded = ops;
                problems = Some(delta);
            }
        }
        // Only this transaction's own expansion writes its edits.
        self.owners.edit(BTreeSet::new());
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
                if let Err(e) = op::check_created(Tree::of(docs), i, op, stamp) {
                    errors.push(e);
                }
                // A new link to a copy needs the copy, once expansion has
                // run; a recorded one may meet a copy that has since gone.
                if !history
                    && let Op::Link { rel } = op
                    && rel.to.is_expanded()
                    && !Tree::of(docs).is_live(rel.to)
                {
                    errors.push(ApplyError { op: i, kind: ErrorKind::UnknownNode, message: format!("no node {}", rel.to) });
                }
                if let Err(message) = expand::check_scope(Tree::of(docs), op) {
                    errors.push(ApplyError { op: i, kind: ErrorKind::OutOfScope, message });
                }
                // What may take a context declaration away (expand.rs).
                let undeclares = match op {
                    Op::Set { id, path, .. } if names_context(path) => Some(*id),
                    Op::Delete { id } | Op::Reparent { id, .. } => Some(*id),
                    _ => None,
                };
                if let Some(id) = undeclares.filter(|id| !id.is_expanded())
                    && let Err(message) = expand::check_context(Tree::of(docs), id)
                {
                    errors.push(ApplyError { op: i, kind: ErrorKind::OutOfScope, message });
                }
            }
        }
        if errors.is_empty() {
            if let Err(message) = attachment::validate(&attached, Tree::of(docs)) {
                errors.push(ApplyError { op: 0, kind: ErrorKind::BadParent, message });
            }
        }
        if !errors.is_empty() {
            self.docs.rollback();
            return Err(errors);
        }
        docs.authored.set_version(stamp);
        let rollback = Rollback(true);
        // Derived nodes and relations come back by re-expansion, never by
        // undo: a deleted instance's expanded children are not restored by
        // its inverse (the guard would refuse them anyway).
        let full_inverse = inverse.clone();
        inverse.retain(|op| match op {
            Op::Create { id, .. } | Op::Delete { id } | Op::Reparent { id, .. } | Op::Reorder { id, .. } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => !id.is_expanded(),
            Op::Link { rel } => !rel.id.is_expanded(),
            Op::Unlink { id } => !id.is_expanded(),
        });
        Ok(Run {
            tx: Transaction {
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
            expanded,
            full_inverse,
            problems,
            shared,
            rollback,
            recorded,
            solved,
        })
    }

    pub fn view(&self, q: &ViewQuery) -> Result<String, ViewError> {
        view::with_problems(self.problem_notes(), || view::render_tree(Tree::of(&self.docs), &self.reactive, &self.clusters.var_cluster, q))
    }

    pub fn save(&self) -> String {
        serial::save(&self.docs.authored)
    }

    /// A store over a loaded document, with empty history.
    pub fn load(text: &str) -> Result<StoreCore, LoadError> {
        serial::load(text).map(StoreCore::with_document)
    }


    // ---- Scopes (plans/specs/scopes.md) ---------------------------------

    /// The scope this store's document is, if it has been given one.
    pub fn scope(&self) -> Option<ScopeId> {
        self.docs.authored.scope()
    }

    /// Name the scope (scopes.md §3). Identity metadata, not content: not an
    /// op, not in history, and it does not bump the version. It is written
    /// in the next save's header.
    pub fn set_scope(&mut self, scope: Option<ScopeId>) {
        self.docs.authored.set_scope(scope);
    }

    /// The client this store mints new ids as (`id.rs`): 0 for a scope
    /// edited locally; the number a server handed out, while syncing.
    pub fn client(&self) -> u32 {
        self.docs.authored.client()
    }

    /// Mint new ids as `client` from now on. Like the scope, who is editing
    /// is not content: not an op, not in history, never saved. Panics if
    /// `client` is not below [`id::CLIENT_LIMIT`].
    pub fn set_client(&mut self, client: u32) {
        self.docs.authored.set_client(client);
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
        if self.docs.authored.version() <= version {
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

/// An owner's items as they enter and leave (`Store::advance_presence`).
#[derive(Clone, Default)]
struct Presence {
    enter_ms: f64,
    exit_ms: f64,
    items: BTreeMap<String, Present>,
}

#[derive(Clone)]
struct Present {
    /// 0, unseen, to 1, here.
    p: f64,
    leaving: bool,
    /// The element and index it had when last wanted.
    value: Value,
    index: f64,
}

/// Fold what an owner wants now into who is present: a new key enters
/// (from 0 when the owner has an `enter_ms` and was already showing, else
/// at once), a key no longer wanted leaves (kept while `exit_ms` runs, else
/// dropped), and one wanted again while leaving turns back from where it
/// is. Answers the keys to expand, the wanted ones first, with each one's
/// element.
/// What a repeat or conditional settles from (`settle_repeats`): its items'
/// source, whether it shows, how many at most, and its enter and exit
/// timings. Its native reads them all, so a change of any settles it again
/// (council review of DS-3b: `max` and the timings were left out).
const REPEAT_INPUTS: [(facet::FacetKind, &str); 7] = [
    (facet::FacetKind::Repeat, "each"),
    (facet::FacetKind::Repeat, "count"),
    (facet::FacetKind::Repeat, "key"),
    (facet::FacetKind::When, "is"),
    (facet::FacetKind::Repeat, "max"),
    (facet::FacetKind::Repeat, "enter_ms"),
    (facet::FacetKind::Repeat, "exit_ms"),
];

/// A drawn repeat or conditional as a native cell (reactive-engine handoff
/// §4.10, RE-5.4): it reads what `settle_repeats` compares
/// (`REPEAT_INPUTS`) and writes nothing. That it ran says its inputs
/// changed, so a settle with none run looks at no repeat.
#[derive(Debug)]
struct RepeatNative {
    leaves: [Leaf; 7],
    outputs: BTreeSet<Leaf>,
}

impl RepeatNative {
    fn new(r: NodeId) -> RepeatNative {
        let leaves = REPEAT_INPUTS.map(|(k, f)| Leaf { node: r, path: LeafPath::new(k, f).expect("declared leaf") });
        RepeatNative { leaves, outputs: BTreeSet::new() }
    }
}

impl graph::Native for RepeatNative {
    fn producer(&self) -> ProducerId {
        ProducerId::ITEMS
    }
    fn outputs(&self) -> &BTreeSet<Leaf> {
        &self.outputs
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn compute(&self, cx: &mut graph::Cx<'_>) -> Result<BTreeMap<Leaf, Value>, graph::Failure> {
        for l in self.leaves {
            let _ = cx.read(l);
        }
        Ok(BTreeMap::new())
    }
}

/// One repeat's or conditional's settled items: their keys, the item values
/// it publishes, and its problems (`settle_repeats`).
#[derive(Clone, Debug, PartialEq)]
struct ItemsOf {
    keys: Vec<String>,
    values: Vec<(Leaf, Value)>,
    problems: Vec<expand::Problem>,
}

/// What a repeat with no enter or exit timing settles from: its `each`,
/// `count` and `key`, whether it is a repeat, whether it shows, its `max`.
#[derive(Clone, Debug, PartialEq)]
struct ItemsKey {
    inputs: [Option<Value>; 3],
    is_repeat: bool,
    shown: bool,
    max: usize,
}

fn settle_presence(all: &mut BTreeMap<NodeId, Presence>, moving: &mut BTreeSet<NodeId>, r: NodeId, wanted: Vec<(String, Value)>, enter_ms: f64, exit_ms: f64) -> Vec<(String, Value)> {
    let first = !all.contains_key(&r);
    let p = all.entry(r).or_default();
    (p.enter_ms, p.exit_ms) = (enter_ms, exit_ms);
    for (key, value) in &wanted {
        match p.items.get_mut(key) {
            Some(e) => {
                e.leaving = false;
                e.value = value.clone();
            }
            None => {
                p.items.insert(key.clone(), Present { p: if first || enter_ms <= 0.0 { 1.0 } else { 0.0 }, leaving: false, value: value.clone(), index: 0.0 });
            }
        }
    }
    p.items.retain(|key, e| {
        if wanted.iter().any(|(k, _)| k == key) {
            return true;
        }
        e.leaving = true;
        exit_ms > 0.0 && e.p > 0.0
    });
    if p.items.values().any(|e| e.leaving || e.p < 1.0) {
        moving.insert(r);
    } else {
        moving.remove(&r);
    }
    let mut out = wanted;
    let leaving: Vec<(String, Value)> = p.items.iter().filter(|(_, e)| e.leaving).map(|(k, e)| (k.clone(), e.value.clone())).collect();
    out.extend(leaving);
    out
}

/// Whether a set of this leaf changes what `@name` resolves to: a rename,
/// or a let starting or ceasing to provide (palette.rs `provided`).
/// Whether a set on this leaf changes what the graph derives beyond the
/// leaf's own value: the signal a connection derives (a port's `kind` and
/// `dir`), an anchor's position (`follow`, `port`), or a boundary's declared
/// type (`datatype`). Each rebuilds the graph, as a structural op does.
fn rederives(path: &LeafPath) -> bool {
    matches!((path.facet, path.field), (FacetKind::Port, "kind" | "dir" | "datatype") | (FacetKind::Anchor, "follow" | "port"))
}

/// Whether `id` may answer an `@name`: a let (it provides when its flag
/// says so) or a colour token. Their order among siblings decides which
/// answers (palette.rs `provided`).
fn provides(doc: Tree<'_>, id: NodeId) -> bool {
    doc.node(id).is_some_and(|n| matches!(n.ty.name(), "let" | "token"))
}

fn names_context(path: &LeafPath) -> bool {
    path.facet == FacetKind::Name || (path.facet == FacetKind::Let && path.field == "provide")
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

/// A number no earlier call returned, in this process: change stamps that
/// stay distinct across stores and graphs (`Docs::edited`,
/// `Reactive::wired`), so a cache keyed on them never matches state it was
/// not computed from.
pub(crate) fn stamp() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
