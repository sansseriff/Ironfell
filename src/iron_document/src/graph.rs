//! The reactive graph: bound slots, their dependencies, and their resolved
//! values.
//!
//! Evaluation is height-ordered with equality cutoff, so a diamond evaluates
//! each node once and an unchanged intermediate stops propagation. Structure
//! is rebuilt only when a transaction changed bindings or nodes; a plain
//! value change walks the reverse edges from the changed leaves.
//!
//! The graph also holds the *gesture overlay*: values previewed during a
//! drag, read in place of authored constants by every evaluation, and turned
//! into one transaction on commit. This is doc 04 §3.5's coalescing buffer
//! living in the store, so the manipulation loop never leaves the process
//! (doc 02 invariant 8).
//!
//! Dependencies are static (`Expr::deps`), with one exception: a **tracked
//! read**, `deref(node, 'facet.field')`, whose target is a value
//! (plans/specs/reactivity.md §2). A binding that holds one records what it
//! read each time it is evaluated; those leaves are its dependencies until
//! the next evaluation, as Svelte's and Solid's are. A leaf it starts to
//! read may be evaluated after it in the same pass, in which case it is
//! evaluated again; a tracked read that reaches its own binding is an
//! error on that leaf, since the applier cannot refuse it ahead of time.

use crate::{NodeId, Tree};
use crate::expr::{Expr, Leaf};
use crate::facet::FacetKind;
use crate::value::Slot;
use crate::value::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Who publishes a runtime signal. Three producers are built in; a host
/// registers more through `Store::register_producer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProducerId(pub u32);

impl ProducerId {
    /// Clips sampled at the playhead (`time.rs`).
    pub const CLIPS: ProducerId = ProducerId(0);
    /// The across/through network's solution (`Store::resolve_physics`).
    pub const NETWORK: ProducerId = ProducerId(1);
    /// Where groups' layouts place their children (`layout.rs`).
    pub const LAYOUT: ProducerId = ProducerId(2);
    /// Component state machines' channels (`machine.rs`).
    pub const MACHINES: ProducerId = ProducerId(3);
    /// Colour tokens' values for the theme in use (palette.rs).
    pub const THEME: ProducerId = ProducerId(4);
    /// Repeat items' values and indices (`Store::settle_repeats`).
    pub const ITEMS: ProducerId = ProducerId(5);
    /// Session state cells (`let.keep = session`; `Store::set_state`).
    pub const STATE: ProducerId = ProducerId(6);
    /// What is selected, on nodes that ask (`selection` facet).
    pub const SELECTION: ProducerId = ProducerId(7);
    /// The first id a host producer receives.
    /// Traces and accumulators (step.rs).
    pub const STEPS: ProducerId = ProducerId(8);
    /// Queries' status and answers (query.rs).
    pub const QUERIES: ProducerId = ProducerId(9);
    /// Items entering and leaving (`Store::advance_presence`).
    pub const PRESENCE: ProducerId = ProducerId(10);
    /// What the last drag or wheel event carried, on its `on` nodes.
    pub const EVENTS: ProducerId = ProducerId(11);
    pub const FIRST_HOST: ProducerId = ProducerId(12);
}

/// One published value, and who published it.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub value: Value,
    pub producer: ProducerId,
}

/// A cell of the graph (reactive-engine.md §5.1; handoff §4.10): what the
/// pass queues, orders by height, and decides necessity for. A `Leaf` cell
/// is a Formula, a binding's expression on that leaf (`BoundNode`); a
/// `Native` cell is an engine computation (`Native`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CellId {
    Leaf(Leaf),
    Native(NativeKey),
}

impl CellId {
    /// The node the cell belongs to, for `read_by`.
    pub fn node(self) -> NodeId {
        match self {
            CellId::Leaf(l) => l.node,
            CellId::Native(k) => k.node(),
        }
    }
}

/// What a native cell computes for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeKey {
    /// An island of the across/through network, by its lowest unknown.
    Island(Leaf),
    /// A native for the graph's own tests.
    #[cfg(test)]
    Test(NodeId, u32),
}

impl NativeKey {
    fn node(self) -> NodeId {
        match self {
            NativeKey::Island(l) => l.node,
            #[cfg(test)]
            NativeKey::Test(n, _) => n,
        }
    }
}

/// An engine computation in the graph (handoff §4.10): it reads values
/// through a `Cx`, which tracks every read, and writes its outputs as
/// claims in the runtime layer under its producer, inside the pass, with
/// `publish`'s rules. Its output set is fixed when it is made: a value
/// change never adds a leaf it writes.
pub(crate) trait Native: Send + Sync + std::fmt::Debug {
    fn producer(&self) -> ProducerId;
    /// Every leaf it may write.
    fn outputs(&self) -> &BTreeSet<Leaf>;
    fn compute(&self, cx: &mut Cx<'_>) -> Result<BTreeMap<Leaf, Value>, Failure>;
}

/// Why a native gave no result.
#[derive(Clone, Debug, PartialEq)]
// The network's natives use it (RE-5.3 step 4); until then, only the tests.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum Failure {
    /// Its outputs fail, and what reads them: "depends on …".
    Error(String),
    /// Its claims stand as they were (a solve that did not converge); the
    /// reason is a problem on the native.
    KeepLast(String),
}

#[derive(Clone, Debug)]
struct NativeCell {
    rule: std::sync::Arc<dyn Native>,
    /// What it read at its last run, through the layers.
    reads: BTreeSet<Leaf>,
    /// Leaves it owns and read beneath its own claim, at its last run.
    base: BTreeSet<Leaf>,
    height: u32,
    /// Run whether or not anything observed needs it.
    rooted: bool,
}

/// What a native reads through: every read is recorded.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Cx<'a> {
    doc: Tree<'a>,
    graph: &'a Reactive,
    me: NativeKey,
    reads: BTreeSet<Leaf>,
    base: BTreeSet<Leaf>,
    /// Cells nothing needed that a read reached.
    reached: BTreeSet<CellId>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl Cx<'_> {
    /// A leaf as any reader sees it. One this native owns reads beneath
    /// its own claim: the authored constant, or the facet's default (as
    /// `layout::base` reads), recorded apart so that it is no cycle.
    pub fn read(&mut self, leaf: Leaf) -> Option<Value> {
        if self.graph.owned.get(&leaf) == Some(&self.me) && !self.graph.nodes.contains_key(&leaf) {
            // Beneath its own claim; where it holds none, the leaf reads
            // as it would without this native (an overlay, a constant).
            self.base.insert(leaf);
            let producer = self.graph.natives[&self.me].rule.producer();
            if self.graph.runtime.get(&leaf).is_some_and(|s| s.producer == producer) {
                return beneath(self.doc, leaf);
            }
            return read_with(&self.graph.overlay, &self.graph.runtime, &self.graph.resolved, &self.graph.nodes, self.doc, leaf);
        }
        self.reads.insert(leaf);
        if let Some(c) = self.graph.cell_of(leaf)
            && self.graph.unneeded.contains(&c)
        {
            self.reached.insert(c);
        }
        read_with(&self.graph.overlay, &self.graph.runtime, &self.graph.resolved, &self.graph.nodes, self.doc, leaf)
    }

    /// What this native last wrote on `leaf`, if it holds it: a warm start
    /// (handoff §4.10), read outside the cycle rule and not tracked.
    pub fn previous(&self, leaf: Leaf) -> Option<Value> {
        self.graph.runtime.get(&leaf).filter(|s| s.producer == self.graph.natives[&self.me].rule.producer()).map(|s| s.value.clone())
    }
}

/// The readers of a cell: of one leaf without allocating, or of a native's
/// outputs, gathered.
enum Readers<'a> {
    One(Option<std::collections::btree_set::Iter<'a, CellId>>),
    Many(std::collections::btree_set::IntoIter<CellId>),
}

impl Iterator for Readers<'_> {
    type Item = CellId;
    fn next(&mut self) -> Option<CellId> {
        match self {
            Readers::One(it) => it.as_mut()?.next().copied(),
            Readers::Many(it) => it.next(),
        }
    }
}

/// A leaf beneath every claim and binding: its authored constant, or its
/// facet's default when the node lacks it.
#[cfg_attr(not(test), allow(dead_code))]
fn beneath(doc: Tree<'_>, leaf: Leaf) -> Option<Value> {
    doc.live(leaf.node)?;
    match doc.leaf(leaf.node, leaf.path) {
        Some(slot) => slot.constant().cloned(),
        None => leaf.path.facet.default_facet().get(leaf.path.field).and_then(|s| s.constant().cloned()),
    }
}

#[derive(Clone, Debug)]
struct BoundNode {
    /// Shared, so an evaluation borrows it for the price of a count.
    expr: std::sync::Arc<Expr>,
    deps: BTreeSet<Leaf>,
    /// What each `@name` it reads resolved to when the graph was built.
    ctx: BTreeMap<String, Leaf>,
    /// Whether the expression holds a tracked read (`deref`).
    tracks: bool,
    /// What its tracked reads read at the last evaluation, beyond `deps`.
    tracked: BTreeSet<Leaf>,
    height: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Reactive {
    nodes: BTreeMap<Leaf, BoundNode>,
    /// Any leaf -> the cells that read it.
    dependents: BTreeMap<Leaf, BTreeSet<CellId>>,
    resolved: BTreeMap<Leaf, Value>,
    overlay: BTreeMap<Leaf, Value>,
    /// The runtime layer (vision 10 §4): named signals published by
    /// producers, read beneath bindings and above authored constants, never
    /// serialised. Clips (`time.rs`) and the across/through network
    /// (`Store::resolve_physics`) are two producers; a host system (a Bevy
    /// simulation, a Model) is another. A leaf claimed by two producers keeps
    /// the first claim and reports the second (`signal_errors`).
    runtime: BTreeMap<Leaf, Signal>,
    /// Which leaves each producer currently holds, for whole-set republish.
    held: BTreeMap<ProducerId, BTreeSet<Leaf>>,
    /// Leaves a producer asked for while another held them, by producer.
    signal_errors: BTreeMap<Leaf, String>,
    changes: BTreeMap<Leaf, Value>,
    /// Every leaf whose resolved value moved since `take_moved`. Unlike
    /// `changes`, which the reconciler drains, this feeds the store's
    /// geometry revisions (`geom::Revisions`), so the two consumers never
    /// starve each other.
    touched: BTreeSet<Leaf>,
    /// Leaves reported as changed on request with no change of value
    /// (`mark`): `Changes::marks`.
    marked: BTreeSet<Leaf>,
    /// Bindings that failed, by leaf, with why. A leaf in error keeps its
    /// last good value for whoever draws it (`read`), but a binding that
    /// reads it fails too, "depends on …, which failed" (reactivity.md
    /// §4.1): nothing is computed from a failed input as if it were good.
    pub errors: BTreeMap<Leaf, String>,
    /// Expressions evaluated since construction. Tests use it to check that
    /// the schedule is glitch-free and the cutoff works.
    pub evaluations: u64,
    /// How many times a change was propagated (`evaluate`), whether or
    /// not anything ran: a publish that only turns unneeded cells stale
    /// counts. The store's idle sweep waits for a frame where it is still.
    pub(crate) propagations: u64,
    /// The signals connections and anchors derive, as last computed: what
    /// `restructure` compares against.
    derived: BTreeMap<Leaf, Expr>,
    /// Each anchor's chain of transformed ancestors as its positions in
    /// `derived` were made from (`anchor::chain`): a restructure makes them
    /// again only for an anchor whose chain changed.
    chains: BTreeMap<NodeId, Vec<NodeId>>,
    /// The rest of `derived`: connections' signals and followers'
    /// placements, made again whole by a restructure that rederives.
    rest: BTreeSet<Leaf>,
    /// Bound leaves whose expression reads context (`@name`).
    token_readers: BTreeSet<Leaf>,
    /// Bindings that failed on a tracked-read cycle or did not settle.
    /// Nothing tells them when that ends (the read that closed the cycle
    /// was never made an edge), so `restructure` runs them again (the
    /// spec's §6.3; the rebuild ran everything).
    blocked: BTreeSet<CellId>,
    /// Cells nothing observed needs (reactive-engine.md §8.2, §8.3): when
    /// what they read changes they are not evaluated but marked `stale`.
    /// Closed downwards: whatever reads a cell outside it is outside it too,
    /// so a needed cell never reads a stale one. The store sets it
    /// (`set_unneeded`); empty while everything is observed.
    unneeded: BTreeSet<CellId>,
    /// Unneeded cells whose inputs changed since they were last evaluated:
    /// they hold their last value (§8.9) until needed again.
    stale: BTreeSet<CellId>,
    /// Whether necessity is out of date: `unneeded` was emptied for
    /// exactness (a transaction, a structure change), or a tracked read
    /// left a cell only it had made needed, since the store last set it.
    relaxed: bool,
    /// Unneeded cells the store read from outside the graph (`read`, while
    /// `recording`): it makes them needed (`need`) and they stay so (§8.9's
    /// read of a stale cell brings it up to date). Layout, the network's
    /// solver, handles and the machine player read cells the graph's edges
    /// do not show. The host's reads are not recorded: what it draws of a
    /// hidden composition may be stale.
    escaped: Escaped,
    /// Cells read from outside the graph while unneeded: needed whenever
    /// the store decides necessity, until the next rebuild.
    pinned: BTreeSet<CellId>,
    /// The cells that could be unneeded (those of hidden nodes not rooted)
    /// as the store last decided: when a tracked read leaves one, it may
    /// be unneeded now, and the store decides necessity again.
    candidates: BTreeSet<CellId>,
    /// The native cells, by key.
    natives: BTreeMap<NativeKey, NativeCell>,
    /// Each leaf a native may write, to its native: a read of it is a read
    /// of that cell, for heights, priming and necessity.
    owned: BTreeMap<Leaf, NativeKey>,
    /// The leaves each native holds a claim on.
    claims: BTreeMap<NativeKey, BTreeSet<Leaf>>,
    /// Natives that read each leaf beneath their own claim.
    base_readers: BTreeMap<Leaf, BTreeSet<NativeKey>>,
    /// Natives whose last run kept their claims and why (`Failure::KeepLast`).
    pub native_problems: BTreeMap<NativeKey, String>,
}

/// The unneeded cells read from outside the graph since the store last
/// took them, and how deep the store is in code whose reads it records.
/// Behind a lock because `read` takes `&self` and the store is shared
/// across threads.
#[derive(Debug, Default)]
struct Escaped {
    /// Each with whether its value was old when read: a stale cell is
    /// upstream of it.
    read: std::sync::Mutex<BTreeMap<Leaf, bool>>,
    depth: std::sync::atomic::AtomicUsize,
}

impl Clone for Escaped {
    fn clone(&self) -> Self {
        Self { read: std::sync::Mutex::new(self.read.lock().expect("escaped lock").clone()), depth: std::sync::atomic::AtomicUsize::new(self.depth.load(std::sync::atomic::Ordering::Relaxed)) }
    }
}

impl Reactive {
    /// Value of any leaf as a reader sees it: the gesture overlay first, then
    /// the resolved value of a binding, then the authored constant.
    pub fn read(&self, doc: Tree<'_>, leaf: Leaf) -> Option<Value> {
        if !self.unneeded.is_empty()
            && self.escaped.depth.load(std::sync::atomic::Ordering::Relaxed) > 0
            && let Some(c) = self.cell_of(leaf)
            && self.unneeded.contains(&c)
        {
            let old = self.behind(c);
            *self.escaped.read.lock().expect("escaped lock").entry(leaf).or_default() |= old;
        }
        read_with(&self.overlay, &self.runtime, &self.resolved, &self.nodes, doc, leaf)
    }

    /// Whether evaluation stopped at `leaf`: its inputs changed while
    /// nothing observed needed it (reactive-engine.md §8.9). What reads it
    /// is unneeded too and holds its last value without being listed here.
    pub fn is_stale(&self, leaf: Leaf) -> bool {
        self.cell_of(leaf).is_some_and(|c| self.stale.contains(&c))
    }

    /// The cell `leaf` reads as, if one computes it.
    fn cell_of(&self, leaf: Leaf) -> Option<CellId> {
        if self.nodes.contains_key(&leaf) {
            return Some(CellId::Leaf(leaf));
        }
        self.owned.get(&leaf).map(|k| CellId::Native(*k))
    }

    /// Whether the cell exists.
    fn exists(&self, c: CellId) -> bool {
        match c {
            CellId::Leaf(l) => self.nodes.contains_key(&l),
            CellId::Native(k) => self.natives.contains_key(&k),
        }
    }

    /// A cell's height. It must exist.
    fn height_of(&self, c: CellId) -> u32 {
        match c {
            CellId::Leaf(l) => self.nodes[&l].height,
            CellId::Native(k) => self.natives[&k].height,
        }
    }

    fn set_height(&mut self, c: CellId, h: u32) {
        match c {
            CellId::Leaf(l) => self.nodes.get_mut(&l).expect("cell").height = h,
            CellId::Native(k) => self.natives.get_mut(&k).expect("cell").height = h,
        }
    }

    /// The cells `c` reads: its static and its tracked dependencies that
    /// are cells (a constant has none).
    fn reads_of(&self, c: CellId) -> Vec<CellId> {
        match c {
            CellId::Leaf(l) => self.nodes.get(&l).map(|n| n.deps.iter().chain(&n.tracked).filter_map(|d| self.cell_of(*d)).collect()).unwrap_or_default(),
            CellId::Native(k) => self.natives.get(&k).map(|n| n.reads.iter().filter_map(|d| self.cell_of(*d)).filter(|d| *d != c).collect()).unwrap_or_default(),
        }
    }

    /// The cells that read `c`'s value: a native's, any of its outputs.
    fn readers_of(&self, c: CellId) -> Readers<'_> {
        match c {
            CellId::Leaf(l) => Readers::One(self.dependents.get(&l).map(|ds| ds.iter())),
            CellId::Native(k) => Readers::Many(
                self.natives
                    .get(&k)
                    .into_iter()
                    .flat_map(|n| n.rule.outputs().iter())
                    .flat_map(|o| self.dependents.get(o).into_iter().flatten().copied())
                    .filter(|d| *d != c)
                    .collect::<BTreeSet<CellId>>()
                    .into_iter(),
            ),
        }
    }

    /// How many cells hold a value older than their inputs.
    pub fn stale_count(&self) -> usize {
        self.stale.len()
    }

    /// The cells of `nodes` nothing needs: those of nodes not `rooted`
    /// (state, and what the store reads besides drawing, §8.3) that no cell
    /// outside the set reads, directly or through others.
    pub(crate) fn necessity(&mut self, nodes: &BTreeSet<NodeId>, rooted: &dyn Fn(NodeId) -> bool) -> BTreeSet<CellId> {
        let mut candidates: BTreeSet<CellId> = BTreeSet::new();
        for &n in nodes {
            if !rooted(n) {
                candidates.extend(self.nodes.range(first_leaf(n)..).map(|(l, _)| *l).take_while(|l| l.node == n).map(CellId::Leaf));
            }
        }
        // A native nothing roots is a candidate when every node it writes is.
        for (k, n) in &self.natives {
            let outputs = n.rule.outputs();
            if !n.rooted && !outputs.is_empty() && outputs.iter().all(|o| nodes.contains(&o.node) && !rooted(o.node)) {
                candidates.insert(CellId::Native(*k));
            }
        }
        let mut stack: Vec<CellId> = candidates.iter().filter(|c| self.pinned.contains(c) || self.readers_of(**c).any(|d| !candidates.contains(&d))).copied().collect();
        let mut needed: BTreeSet<CellId> = BTreeSet::new();
        while let Some(c) = stack.pop() {
            if !needed.insert(c) {
                continue;
            }
            stack.extend(self.reads_of(c).into_iter().filter(|d| candidates.contains(d) && !needed.contains(d)));
        }
        self.candidates = candidates.clone();
        candidates.retain(|l| !needed.contains(l));
        candidates
    }

    /// Make `unneeded` the cells nothing needs, evaluating the stale ones
    /// that are needed again. Whether it evaluated anything.
    pub(crate) fn set_unneeded(&mut self, doc: Tree<'_>, unneeded: BTreeSet<CellId>) -> bool {
        self.relaxed = false;
        let wake: BTreeSet<CellId> = self.stale.iter().filter(|c| !unneeded.contains(c)).copied().collect();
        self.stale.retain(|l| unneeded.contains(l));
        self.unneeded = unneeded;
        if wake.is_empty() {
            return false;
        }
        let dirty = self.readers(&wake);
        self.evaluate(doc, &dirty, &wake);
        true
    }

    /// The unneeded cells read from outside the graph since the last take.
    pub(crate) fn take_escaped(&self) -> BTreeMap<Leaf, bool> {
        std::mem::take(&mut *self.escaped.read.lock().expect("escaped lock"))
    }

    /// Whether an unneeded cell's value is older than its inputs: a stale
    /// cell is it or upstream of it, among the unneeded (a needed cell is
    /// current).
    fn behind(&self, cell: CellId) -> bool {
        if self.stale.is_empty() {
            return false;
        }
        let mut stack = vec![cell];
        let mut seen = BTreeSet::new();
        while let Some(c) = stack.pop() {
            if !self.unneeded.contains(&c) || !seen.insert(c) {
                continue;
            }
            if self.stale.contains(&c) {
                return true;
            }
            stack.extend(self.reads_of(c));
        }
        false
    }

    /// Start (`true`) or end recording reads of unneeded cells: the store's
    /// own code that reads outside the graph runs between the two. Nests.
    pub(crate) fn recording(&self, on: bool) {
        if on {
            self.escaped.depth.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            self.escaped.depth.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Make `leaves` and what they read needed, bringing the stale ones up
    /// to date: something outside the graph read them (`take_escaped`).
    /// They stay needed when the store decides necessity again (`pinned`).
    /// Whether anything was brought up to date, so that what read them
    /// may have read an old value.
    pub(crate) fn need(&mut self, doc: Tree<'_>, leaves: &BTreeSet<Leaf>) -> bool {
        let cells: Vec<CellId> = leaves.iter().filter_map(|l| self.cell_of(*l)).collect();
        self.pinned.extend(cells.iter().filter(|c| self.unneeded.contains(c)));
        let wake = self.wake_closure(cells);
        if wake.is_empty() {
            return false;
        }
        let dirty = self.readers(&wake);
        self.evaluate(doc, &dirty, &wake);
        true
    }

    /// Take `from` and every unneeded cell it reads, directly or through
    /// others, out of `unneeded`; the stale ones among them, which the
    /// caller evaluates.
    fn wake_closure(&mut self, from: impl IntoIterator<Item = CellId>) -> BTreeSet<CellId> {
        self.wake_counted(from).0
    }

    /// `wake_closure`, and how many cells it took out of `unneeded`.
    fn wake_counted(&mut self, from: impl IntoIterator<Item = CellId>) -> (BTreeSet<CellId>, usize) {
        let mut stack: Vec<CellId> = from.into_iter().collect();
        let mut wake = BTreeSet::new();
        let mut taken = 0;
        while let Some(c) = stack.pop() {
            if !self.unneeded.remove(&c) {
                continue;
            }
            taken += 1;
            if self.stale.remove(&c) {
                wake.insert(c);
            }
            stack.extend(self.reads_of(c));
        }
        (wake, taken)
    }

    /// Whether a cell on a node `by` names reads a leaf of `nodes`.
    pub(crate) fn read_by(&self, nodes: impl IntoIterator<Item = NodeId>, by: &dyn Fn(NodeId) -> bool) -> bool {
        nodes.into_iter().any(|n| self.dependents.range(first_leaf(n)..).take_while(|(l, _)| l.node == n).any(|(_, ds)| ds.iter().any(|d| by(d.node()))))
    }

    /// Need every cell again, bringing the stale ones up to date: what a
    /// transaction and a structure change start from, so that what they
    /// read and write is exact. Whether it evaluated anything.
    pub(crate) fn relax(&mut self, doc: Tree<'_>) -> bool {
        if self.unneeded.is_empty() {
            return false;
        }
        let evaluated = self.set_unneeded(doc, BTreeSet::new());
        self.relaxed = true;
        evaluated
    }

    /// Bring up to `budget` stale cells up to date, lowest first, though
    /// nothing needs them (reactive-engine.md §8.7's idle sweep), so their
    /// problems and values do not stay old indefinitely. What reads them is
    /// unneeded too and turns stale if they changed: a later sweep takes
    /// it. `budget` counts evaluations, checked between rounds: a tracked
    /// read in one round wakes all it reads, which may overrun it once.
    /// Whether any were evaluated.
    pub(crate) fn sweep(&mut self, doc: Tree<'_>, budget: usize) -> bool {
        let mut left = budget;
        let swept = !self.stale.is_empty();
        while left > 0 && !self.stale.is_empty() {
            let mut by: Vec<(u32, CellId)> = self.stale.iter().map(|c| (self.height_of(*c), *c)).collect();
            by.sort();
            by.truncate(left);
            let picked: BTreeSet<CellId> = by.into_iter().map(|(_, c)| c).collect();
            let before = self.evaluations;
            // What this round takes out of `unneeded`: the picked cells, and
            // what a tracked read in one of them wakes (an unneeded reader's
            // reads make nothing needed).
            let was = self.unneeded.clone();
            for c in &picked {
                self.stale.remove(c);
                self.unneeded.remove(c);
            }
            let dirty = self.readers(&picked);
            self.evaluate(doc, &dirty, &picked);
            let taken: Vec<CellId> = was.difference(&self.unneeded).copied().collect();
            self.unneeded.extend(taken.iter().copied());
            // Needed after all if a needed cell reads it (a tracked read
            // from outside the round reached it): it and what it reads.
            let read: Vec<CellId> = taken.iter().filter(|c| self.readers_of(**c).any(|d| self.exists(d) && !self.unneeded.contains(&d))).copied().collect();
            let wake = self.wake_closure(read);
            if !wake.is_empty() {
                let dirty = self.readers(&wake);
                self.evaluate(doc, &dirty, &wake);
            }
            left = left.saturating_sub(usize::try_from(self.evaluations - before).unwrap_or(usize::MAX).max(1));
        }
        swept
    }

    /// Whether `relax` emptied `unneeded` since the store last set it.
    pub(crate) fn relaxed(&self) -> bool {
        self.relaxed
    }

    /// `from` and every cell that reads it, directly or through others,
    /// not going on through a cell nothing needs (what reads one is
    /// unneeded too, and is reached only if it is evaluated).
    fn readers(&self, from: &BTreeSet<CellId>) -> BTreeSet<CellId> {
        let mut dirty = from.clone();
        let mut stack: Vec<CellId> = from.iter().copied().collect();
        while let Some(c) = stack.pop() {
            if self.unneeded.contains(&c) && !from.contains(&c) {
                continue;
            }
            let ds: Vec<CellId> = self.readers_of(c).collect();
            for d in ds {
                if self.exists(d) && dirty.insert(d) {
                    stack.push(d);
                }
            }
        }
        dirty
    }

    /// The runtime layer as published.
    pub fn signals(&self) -> &BTreeMap<Leaf, Signal> {
        &self.runtime
    }

    /// The signal on a leaf, if a producer holds it.
    pub fn signal(&self, leaf: Leaf) -> Option<&Signal> {
        self.runtime.get(&leaf)
    }

    /// Whether some producer currently drives this leaf.
    pub fn is_driven(&self, leaf: Leaf) -> bool {
        self.runtime.contains_key(&leaf)
    }

    /// Whether this producer currently drives this leaf.
    pub fn driven_by(&self, leaf: Leaf, producer: ProducerId) -> bool {
        self.runtime.get(&leaf).is_some_and(|s| s.producer == producer)
    }

    /// The leaves a producer holds, with their values.
    pub fn published_by(&self, producer: ProducerId) -> BTreeMap<Leaf, Value> {
        self.held.get(&producer).map(|ls| ls.iter().filter_map(|l| self.runtime.get(l).map(|s| (*l, s.value.clone()))).collect()).unwrap_or_default()
    }

    /// Leaves a producer asked for while another held them, with why.
    pub fn signal_errors(&self) -> &BTreeMap<Leaf, String> {
        &self.signal_errors
    }

    /// Replace everything `producer` publishes with `values`: leaves it no
    /// longer names are released, leaves another producer holds are refused
    /// and reported. Leaves whose readable value moved are noted and their
    /// dependents re-evaluated, as for a constant that was set.
    pub fn publish(&mut self, doc: Tree<'_>, producer: ProducerId, values: BTreeMap<Leaf, Value>) {
        let mut changed: Vec<Leaf> = Vec::new();
        let was: BTreeSet<Leaf> = self.held.remove(&producer).unwrap_or_default();
        let mut now: BTreeSet<Leaf> = BTreeSet::new();
        for (leaf, value) in values {
            if self.nodes.contains_key(&leaf) {
                self.signal_errors.insert(leaf, format!("{leaf} is bound; producer {} cannot drive it", producer.0));
                continue;
            }
            match self.runtime.get(&leaf) {
                Some(s) if s.producer != producer => {
                    self.signal_errors.insert(leaf, format!("{leaf} is already driven by producer {}; producer {} cannot also drive it", s.producer.0, producer.0));
                    continue;
                }
                Some(s) if s.value == value => {}
                _ => {
                    self.runtime.insert(leaf, Signal { value, producer });
                    changed.push(leaf);
                }
            }
            self.signal_errors.remove(&leaf);
            now.insert(leaf);
        }
        for leaf in was.difference(&now) {
            if self.runtime.get(leaf).is_some_and(|s| s.producer == producer) {
                self.runtime.remove(leaf);
                changed.push(*leaf);
            }
        }
        // Another producer's errors on leaves this one released may now be
        // stale; they clear on that producer's next publish.
        if !now.is_empty() {
            self.held.insert(producer, now);
        }
        if changed.is_empty() {
            return;
        }
        for &l in &changed {
            // Only visible when no gesture is previewing this leaf.
            if !self.overlay.contains_key(&l)
                && let Some(v) = self.read(doc, l)
            {
                self.note(l, v);
            }
        }
        self.invalidate(doc, changed);
    }

    /// Release everything `producer` publishes.
    pub fn retract(&mut self, doc: Tree<'_>, producer: ProducerId) {
        self.publish(doc, producer, BTreeMap::new());
    }

    /// Whether any binding reads `leaf`.
    pub fn is_read(&self, leaf: Leaf) -> bool {
        self.dependents.get(&leaf).is_some_and(|d| !d.is_empty())
    }

    pub fn is_bound(&self, leaf: Leaf) -> bool {
        self.nodes.contains_key(&leaf)
    }

    /// The expression a leaf resolves through: its binding, or the signal a
    /// connection derives for it.
    pub fn expr_of(&self, leaf: Leaf) -> Option<&Expr> {
        self.nodes.get(&leaf).map(|n| &*n.expr)
    }

    /// Each bound leaf that reads context, with the provider each `@name`
    /// resolved to.
    pub fn contexts(&self) -> impl Iterator<Item = (Leaf, &BTreeMap<String, Leaf>)> {
        self.nodes.iter().filter(|(_, n)| !n.ctx.is_empty()).map(|(l, n)| (*l, &n.ctx))
    }

    /// Every leaf `leaf` reads, directly or through other bindings.
    pub fn transitive_deps(&self, leaf: Leaf) -> BTreeSet<Leaf> {
        let mut out = BTreeSet::new();
        let mut stack = vec![leaf];
        while let Some(l) = stack.pop() {
            if let Some(n) = self.nodes.get(&l) {
                for d in n.deps.iter().chain(&n.tracked) {
                    if out.insert(*d) {
                        stack.push(*d);
                    }
                }
            }
        }
        out
    }

    pub fn height(&self, leaf: Leaf) -> Option<u32> {
        self.nodes.get(&leaf).map(|n| n.height)
    }

    pub fn overlay(&self) -> &BTreeMap<Leaf, Value> {
        &self.overlay
    }

    /// Rebuild the structure from the document and evaluate every binding.
    /// Called after transactions that created, deleted, or rebound anything.
    pub fn rebuild(&mut self, doc: Tree<'_>) {
        // Everything is evaluated below.
        if !self.unneeded.is_empty() {
            self.unneeded.clear();
            self.stale.clear();
            self.relaxed = true;
        }
        self.pinned.clear();
        self.candidates.clear();
        let old_bound: BTreeSet<Leaf> = self.nodes.keys().copied().collect();
        self.nodes.clear();
        self.dependents.clear();
        self.token_readers.clear();
        // Stored bindings, then the signals connections derive (`port`):
        // an in-port reads its partner, evaluated like any binding; then
        // anchors' positions and followers' placements (`anchor`).
        let anchors = crate::anchor::anchors(doc);
        let mut derived = crate::port::derived_signals(doc);
        derived.extend(crate::anchor::followers(doc, &anchors));
        self.rest = derived.iter().map(|(l, _)| *l).collect();
        self.chains = anchors.iter().map(|&a| (a, crate::anchor::chain(doc, a))).collect();
        derived.extend(anchors.iter().flat_map(|&a| crate::anchor::positions(doc, a)));
        self.derived = derived.iter().cloned().collect();
        let tokens = crate::palette::leaves(doc);
        for (leaf, expr) in doc.bound_slots().chain(derived.iter().map(|(l, e)| (*l, e))) {
            let mut deps = expr.deps();
            // `@name` reads the nearest provider above this node, a colour
            // token at the root (palette.rs).
            let names = expr.tokens();
            if !names.is_empty() {
                self.token_readers.insert(leaf);
            }
            let ctx: BTreeMap<String, Leaf> = names.into_iter().filter_map(|n| crate::palette::provided(doc, leaf.node, &n, &tokens).map(|l| (n, l))).collect();
            deps.extend(ctx.values().copied());
            for d in &deps {
                self.dependents.entry(*d).or_default().insert(CellId::Leaf(leaf));
            }
            // A boundary's type check reads the instance's arguments
            // (`like(…)`) while it evaluates: those reads are tracked, so an
            // argument that changes checks it again.
            let tracks = tracks_reads(doc, leaf, expr);
            self.nodes.insert(
                leaf,
                BoundNode {
                    expr: std::sync::Arc::new(expr.clone()),
                    deps,
                    ctx,
                    tracks,
                    tracked: BTreeSet::new(),
                    height: 0,
                },
            );
        }
        // The natives' edges, as they last read.
        let native_reads: Vec<(NativeKey, Vec<Leaf>)> = self.natives.iter().map(|(k, n)| (*k, n.reads.iter().copied().collect())).collect();
        for (k, reads) in native_reads {
            for r in reads {
                self.dependents.entry(r).or_default().insert(CellId::Native(k));
            }
        }
        self.compute_heights();
        // A slot that stopped being bound reads as its constant again; say so.
        for leaf in old_bound {
            if !self.nodes.contains_key(&leaf) {
                self.resolved.remove(&leaf);
                self.errors.remove(&leaf);
                if let Some(v) = self.read(doc, leaf) {
                    self.note(leaf, v);
                }
            }
        }
        let all: BTreeSet<CellId> = self.nodes.keys().copied().map(CellId::Leaf).chain(self.natives.keys().map(|k| CellId::Native(*k))).collect();
        self.evaluate(doc, &all, &all);
    }

    /// Bring the graph up to date after structure changed at `nodes` (made,
    /// removed, rebound, given or stripped of facets), without the rebuild
    /// (reactive-engine handoff §4.4, stage 3a): only those nodes' bindings
    /// are read again, the signals connections and anchors derive are
    /// compared with what they were, heights are raised where an edge needs
    /// it, and what changed and what reads it are evaluated. With `rescope`
    /// (a provider may have moved), every `@name` reader is resolved again.
    /// `rederive` says whether the ops can have changed what connections and
    /// anchors derive (`port::derived_signals`, `anchor::positions`,
    /// `anchor::followers`): without it they are kept as they were; with it
    /// signals and followers are made again whole, and positions for each
    /// anchor whose chain of transformed ancestors changed.
    pub(crate) fn restructure(&mut self, doc: Tree<'_>, nodes: &BTreeSet<NodeId>, rescope: bool, rederive: bool) {
        // New cells may read unneeded ones: start from every cell current.
        self.relax(doc);
        let start = first_leaf;
        // The leaves to look at again: every binding the nodes had or have,
        // and every derived signal that came, went or changed.
        let mut look: BTreeSet<Leaf> = BTreeSet::new();
        for &n in nodes {
            look.extend(self.nodes.range(start(n)..).map(|(l, _)| *l).take_while(|l| l.node == n));
            if doc.is_live(n) {
                for f in doc.facets_of(n) {
                    for field in f.field_names() {
                        if f.get(field).and_then(Slot::expr).is_some() {
                            look.insert(Leaf { node: n, path: crate::facet::LeafPath::new(f.kind(), field).expect("declared field") });
                        }
                    }
                }
            }
        }
        // Unless the ops could change one, the derived signals and anchors
        // stand as they are: no copy of them to compare.
        if rederive {
            // Positions: made again for an anchor whose chain of transformed
            // ancestors changed (they are a function of it), dropped for one
            // gone.
            let anchors = crate::anchor::anchors(doc);
            let present: BTreeSet<NodeId> = anchors.iter().copied().collect();
            let gone: Vec<NodeId> = self.chains.keys().filter(|a| !present.contains(a)).copied().collect();
            for a in gone {
                self.chains.remove(&a);
                for l in crate::anchor::position_leaves(a) {
                    if self.derived.remove(&l).is_some() {
                        look.insert(l);
                    }
                }
            }
            for &a in &anchors {
                let chain = crate::anchor::chain(doc, a);
                if self.chains.get(&a) == Some(&chain) {
                    continue;
                }
                for (l, e) in crate::anchor::positions(doc, a) {
                    if self.derived.get(&l) != Some(&e) {
                        look.insert(l);
                        self.derived.insert(l, e);
                    }
                }
                self.chains.insert(a, chain);
            }
            // Signals and followers, which follow connections: whole.
            let mut rest: BTreeMap<Leaf, Expr> = crate::port::derived_signals(doc).into_iter().collect();
            rest.extend(crate::anchor::followers(doc, &anchors));
            for l in std::mem::take(&mut self.rest) {
                if !rest.contains_key(&l) && self.derived.remove(&l).is_some() {
                    look.insert(l);
                }
            }
            self.rest = rest.keys().copied().collect();
            for (l, e) in rest {
                if self.derived.get(&l) != Some(&e) {
                    look.insert(l);
                    self.derived.insert(l, e);
                }
            }
        }
        let tokens = crate::palette::leaves(doc);
        let mut primed: BTreeSet<CellId> = BTreeSet::new();
        let mut made: Vec<CellId> = Vec::new();
        for leaf in look {
            // A derived signal takes precedence over a stored binding, as the
            // rebuild's later insert does.
            let want: Option<Expr> = self.derived.get(&leaf).cloned().or_else(|| doc.is_live(leaf.node).then(|| doc.leaf(leaf.node, leaf.path).and_then(Slot::expr).cloned()).flatten());
            match (self.nodes.get(&leaf).map(|n| &n.expr), want) {
                (Some(old), Some(e)) if **old == e => {}
                (_, Some(e)) => {
                    self.set_cell(doc, leaf, e, &tokens);
                    made.push(CellId::Leaf(leaf));
                    primed.insert(CellId::Leaf(leaf));
                }
                (Some(_), None) => {
                    self.drop_cell(leaf);
                    // It reads as its constant again; say so, and its readers
                    // read that.
                    self.resolved.remove(&leaf);
                    self.errors.remove(&leaf);
                    if let Some(v) = self.read(doc, leaf) {
                        self.note(leaf, v);
                    }
                    primed.extend(self.dependents.get(&leaf).into_iter().flatten().copied());
                }
                (None, None) => {}
            }
        }
        if rescope {
            let readers: Vec<Leaf> = self.token_readers.iter().copied().collect();
            for leaf in readers {
                let Some(node) = self.nodes.get(&leaf) else { continue };
                let ctx: BTreeMap<String, Leaf> = node.expr.tokens().into_iter().filter_map(|n| crate::palette::provided(doc, leaf.node, &n, &tokens).map(|l| (n, l))).collect();
                if ctx != node.ctx {
                    let expr = (*node.expr).clone();
                    self.set_cell(doc, leaf, expr, &tokens);
                    made.push(CellId::Leaf(leaf));
                    primed.insert(CellId::Leaf(leaf));
                }
            }
        }
        // Every leaf of a touched node: a constant set, a node made or gone.
        // Its own bindings run again (what they check, a port's `datatype`,
        // may have changed with no change of expression), and its readers
        // read again.
        for &n in nodes {
            let own: Vec<Leaf> = self.nodes.range(start(n)..).map(|(l, _)| *l).take_while(|l| l.node == n).collect();
            // What a binding tracks follows its node (a datatype set makes
            // its check read arguments), not only its expression.
            for l in &own {
                let tracks = self.nodes.get(l).is_some_and(|b| tracks_reads(doc, *l, &b.expr));
                if let Some(b) = self.nodes.get_mut(l) {
                    b.tracks = tracks;
                }
            }
            primed.extend(own.into_iter().map(CellId::Leaf));
            let readers: Vec<CellId> = self.dependents.range(start(n)..).take_while(|(l, _)| l.node == n).flat_map(|(_, r)| r.iter().copied()).collect();
            primed.extend(readers);
        }
        primed.extend(self.blocked.iter().copied());
        primed.retain(|c| self.exists(*c));
        // A cell that tracks its reads starts again from none, as the
        // rebuild starts every cell: what it reads is found afresh, and a
        // read that now closes a cycle is caught (it would not be if the
        // read were already known).
        for c in &primed {
            let CellId::Leaf(l) = c else { continue };
            let old: Vec<Leaf> = self.nodes.get_mut(l).map(|n| std::mem::take(&mut n.tracked).into_iter().collect()).unwrap_or_default();
            for t in old {
                if let Some(ds) = self.dependents.get_mut(&t) {
                    ds.remove(c);
                    if ds.is_empty() {
                        self.dependents.remove(&t);
                    }
                }
            }
        }
        if !self.raise(&made) {
            self.compute_heights();
        }
        let dirty = self.readers(&primed);
        self.evaluate(doc, &dirty, &primed);
    }

    /// Make or replace the cell for `leaf`, reading `expr`.
    fn set_cell(&mut self, doc: Tree<'_>, leaf: Leaf, expr: Expr, tokens: &BTreeMap<String, Leaf>) {
        self.drop_cell(leaf);
        let mut deps = expr.deps();
        let names = expr.tokens();
        if !names.is_empty() {
            self.token_readers.insert(leaf);
        }
        let ctx: BTreeMap<String, Leaf> = names.into_iter().filter_map(|n| crate::palette::provided(doc, leaf.node, &n, tokens).map(|l| (n, l))).collect();
        deps.extend(ctx.values().copied());
        for d in &deps {
            self.dependents.entry(*d).or_default().insert(CellId::Leaf(leaf));
        }
        let tracks = tracks_reads(doc, leaf, &expr);
        let height = 1 + deps.iter().filter_map(|d| self.nodes.get(d).map(|n| n.height)).max().unwrap_or(0);
        self.nodes.insert(leaf, BoundNode { expr: std::sync::Arc::new(expr), deps, ctx, tracks, tracked: BTreeSet::new(), height });
    }

    /// Remove `leaf`'s cell and its edges.
    fn drop_cell(&mut self, leaf: Leaf) {
        let Some(old) = self.nodes.remove(&leaf) else { return };
        let cell = CellId::Leaf(leaf);
        for d in old.deps.iter().chain(&old.tracked) {
            if let Some(ds) = self.dependents.get_mut(d) {
                ds.remove(&cell);
                if ds.is_empty() {
                    self.dependents.remove(d);
                }
            }
        }
        self.token_readers.remove(&leaf);
        self.blocked.remove(&cell);
        self.unneeded.remove(&cell);
        self.stale.remove(&cell);
        self.pinned.remove(&cell);
        self.candidates.remove(&cell);
    }

    /// Raise the readers of `made` until every edge goes upward (spec
    /// §6.3). `false` when that would not end: the caller relaxes the whole
    /// graph instead.
    fn raise(&mut self, made: &[CellId]) -> bool {
        let mut stack: Vec<CellId> = made.to_vec();
        let mut budget = (self.nodes.len() + self.natives.len()).saturating_mul(2) + 64;
        while let Some(c) = stack.pop() {
            if !self.exists(c) {
                continue;
            }
            let h = self.height_of(c);
            let readers: Vec<CellId> = self.readers_of(c).collect();
            for r in readers {
                if self.exists(r) && self.height_of(r) <= h {
                    if budget == 0 {
                        return false;
                    }
                    budget -= 1;
                    self.set_height(r, h + 1);
                    stack.push(r);
                }
            }
        }
        true
    }

    /// The values of `changed` leaves moved (a constant was set, or the
    /// overlay changed). Re-evaluate what depends on them, in height order.
    pub fn invalidate(&mut self, doc: Tree<'_>, changed: impl IntoIterator<Item = Leaf>) {
        let mut primed: BTreeSet<CellId> = BTreeSet::new();
        let mut dirty: BTreeSet<CellId> = BTreeSet::new();
        let changed: Vec<Leaf> = changed.into_iter().collect();
        for l in &changed {
            if let Some(ds) = self.dependents.get(l) {
                primed.extend(ds.iter().copied());
            }
            // A native that reads it beneath its own claim.
            if let Some(ns) = self.base_readers.get(l) {
                primed.extend(ns.iter().map(|k| CellId::Native(*k)));
            }
        }
        let mut stack: Vec<CellId> = primed.iter().copied().collect();
        for d in &stack {
            dirty.insert(*d);
        }
        stack.retain(|d| !self.unneeded.contains(d));
        while let Some(c) = stack.pop() {
            let ds: Vec<CellId> = self.readers_of(c).collect();
            for d in ds {
                // What reads an unneeded cell is unneeded, and primed
                // only if that cell is evaluated.
                if dirty.insert(d) && !self.unneeded.contains(&d) {
                    stack.push(d);
                }
            }
        }
        self.evaluate(doc, &dirty, &primed);
    }

    /// Preview a value without writing the document.
    pub fn preview(&mut self, doc: Tree<'_>, leaf: Leaf, value: Value) {
        let before = self.read(doc, leaf);
        self.overlay.insert(leaf, value.clone());
        if before.as_ref() != Some(&value) {
            self.note(leaf, value);
            self.invalidate(doc, [leaf]);
        }
    }

    /// Drop one leaf's preview; it falls back to the document.
    pub fn unpreview(&mut self, doc: Tree<'_>, leaf: Leaf) {
        if self.overlay.remove(&leaf).is_some() {
            if let Some(v) = self.read(doc, leaf) {
                self.note(leaf, v);
            }
            self.invalidate(doc, [leaf]);
        }
    }

    /// Drop every preview; values fall back to the document.
    pub fn clear_overlay(&mut self, doc: Tree<'_>) {
        let leaves: Vec<Leaf> = self.overlay.keys().copied().collect();
        self.overlay.clear();
        for &l in &leaves {
            // A leaf of a facet the node lacks (a gesture that would have
            // added it) reads nothing now: report its default, so whatever
            // drew the preview redraws without it.
            let v = self.read(doc, l).or_else(|| l.path.facet.default_facet().get(l.path.field).and_then(|s| s.constant()).cloned());
            if let Some(v) = v {
                self.note(l, v);
            }
        }
        self.invalidate(doc, leaves);
    }

    /// Whether a value change or a mark is waiting to be taken.
    pub(crate) fn has_pending(&self) -> bool {
        !self.changes.is_empty() || !self.marked.is_empty()
    }

    /// Report a leaf as changed with no change to its value: something it
    /// derives (a text's glyphs, when a font arrives) moved.
    pub(crate) fn mark(&mut self, leaf: Leaf, _value: Value) {
        self.touched.insert(leaf);
        self.marked.insert(leaf);
    }

    /// What `take_changed`, `take_marks` and `take_moved` would return,
    /// without taking them.
    #[cfg_attr(not(feature = "differential"), allow(dead_code))]
    pub(crate) fn peek(&self) -> (BTreeSet<Leaf>, BTreeSet<Leaf>, &BTreeSet<Leaf>) {
        (self.changes.keys().copied().collect(), self.marked.clone(), &self.touched)
    }

    /// The leaves marked since the last take.
    pub(crate) fn take_marks(&mut self) -> BTreeSet<Leaf> {
        std::mem::take(&mut self.marked)
    }

    /// The one place a resolved change is recorded.
    fn note(&mut self, leaf: Leaf, value: Value) {
        self.touched.insert(leaf);
        self.changes.insert(leaf, value);
    }


    /// Leaves whose resolved value moved since the last take.
    pub(crate) fn take_moved(&mut self) -> BTreeSet<Leaf> {
        std::mem::take(&mut self.touched)
    }

    /// The leaves whose resolved value moved since the last take.
    pub(crate) fn take_changed(&mut self) -> BTreeSet<Leaf> {
        std::mem::take(&mut self.changes).into_keys().collect()
    }

    fn compute_heights(&mut self) {
        // Relaxation to a fixpoint. The applier keeps the graph acyclic, so
        // this converges within `nodes.len()` rounds; the bound is a guard.
        let keys: Vec<CellId> = self.nodes.keys().map(|l| CellId::Leaf(*l)).chain(self.natives.keys().map(|k| CellId::Native(*k))).collect();
        for _ in 0..=keys.len() {
            let mut changed = false;
            for &k in &keys {
                let h = 1 + self.reads_of(k).into_iter().map(|d| self.height_of(d)).max().unwrap_or(0);
                if self.height_of(k) != h {
                    self.set_height(k, h);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Whether the cell `from` reads `target`, directly or through others.
    fn reaches(&self, from: CellId, target: CellId) -> bool {
        let mut seen = BTreeSet::new();
        let mut stack = vec![from];
        while let Some(c) = stack.pop() {
            if c == target {
                return true;
            }
            stack.extend(self.reads_of(c).into_iter().filter(|d| seen.insert(*d)));
        }
        false
    }

    /// Evaluate `dirty` in height order. A node runs only if it is primed:
    /// a leaf it reads changed since the last pass. Primed-ness spreads from
    /// a node to its dependents only when its own value actually changed,
    /// which is the equality cutoff. A binding with a tracked read may find
    /// it depends on a leaf evaluated after it; when that leaf changes, the
    /// binding is queued again.
    fn evaluate(&mut self, doc: Tree<'_>, dirty: &BTreeSet<CellId>, primed: &BTreeSet<CellId>) {
        self.propagations += 1;
        let mut primed = primed.clone();
        let mut queue: BTreeSet<(u32, CellId)> = dirty.iter().map(|c| (self.height_of(*c), *c)).collect();
        // Static dependencies never queue a node twice; tracked ones may, a
        // bounded number of times in an acyclic graph. The limit is a guard.
        let mut budget = dirty.len().saturating_mul(4) + 1024;
        // Bindings whose cycle check waited once for the rest of the pass.
        let mut deferred: BTreeSet<CellId> = BTreeSet::new();
        // Whether an edge went in this pass: a cell blocked on a cycle may
        // be free of it now (the kernel's R6; handoff §4.10).
        let mut edge_removed = false;
        let mut retries = 0;
        loop {
        while let Some((_, cell)) = queue.pop_first() {
            if !primed.remove(&cell) {
                continue;
            }
            if self.unneeded.contains(&cell) {
                self.stale.insert(cell);
                continue;
            }
            let leaf = match cell {
                CellId::Leaf(l) => l,
                CellId::Native(k) => {
                    if budget == 0 {
                        self.native_problems.insert(k, "did not settle: tracked reads keep changing each other".into());
                        self.blocked.insert(cell);
                        continue;
                    }
                    budget -= 1;
                    let mut pass = Pass { queue: &mut queue, primed: &mut primed, deferred: &mut deferred, edge_removed: &mut edge_removed };
                    let more = self.run_native(doc, k, &mut pass);
                    budget = budget.saturating_add(more);
                    continue;
                }
            };
            if budget == 0 {
                self.errors.insert(leaf, "did not settle: tracked reads keep changing each other".into());
                self.blocked.insert(cell);
                continue;
            }
            budget -= 1;
            self.evaluations += 1;
            let expr = self.nodes[&leaf].expr.clone();
            let tracks = self.nodes[&leaf].tracks;
            let _ctx = crate::expr::TokenScope::new(self.nodes[&leaf].ctx.clone());
            let reads: std::cell::RefCell<BTreeSet<Leaf>> = Default::default();
            // Unneeded cells a tracked read reached: needed from now on.
            let reached: std::cell::RefCell<BTreeSet<CellId>> = Default::default();
            let result = {
                let unneeded = &self.unneeded;
                let overlay = &self.overlay;
                let runtime = &self.runtime;
                let resolved = &self.resolved;
                let nodes = &self.nodes;
                let errors = &self.errors;
                // The first failed input this binding read, if any.
                let upstream: std::cell::Cell<Option<Leaf>> = std::cell::Cell::new(None);
                let read = |l| {
                    if tracks {
                        reads.borrow_mut().insert(l);
                    }
                    if let Some(c) = self.cell_of(l)
                        && unneeded.contains(&c)
                    {
                        reached.borrow_mut().insert(c);
                    }
                    if errors.contains_key(&l) && !overlay.contains_key(&l) {
                        if upstream.get().is_none() {
                            upstream.set(Some(l));
                        }
                        return None;
                    }
                    read_with(overlay, runtime, resolved, nodes, doc, l)
                };
                let out = expr.eval(&read).and_then(|v| as_leaf_kind(leaf, v)).and_then(|v| match crate::types::declared(doc, leaf) {
                    // A boundary leaf with a declared type checks what it
                    // computes or receives (types-and-units.md §3.3, §3.5).
                    Some((ty, owner)) => ty
                        .check(&v, &|name| crate::facet::LeafPath::new(crate::facet::FacetKind::Args, name).ok().and_then(|path| read(Leaf { node: owner, path })))
                        .map_err(|e| crate::expr::EvalError::Type(format!("{}: {e}", boundary_name(doc, leaf)))),
                    None => Ok(v),
                });
                match (out, upstream.get()) {
                    (Err(_), Some(failed)) => {
                        // Name the input at the root of the failure.
                        let why = errors.get(&failed).map(String::as_str).unwrap_or("");
                        let root = why.strip_prefix("depends on ").unwrap_or("");
                        Err(if root.is_empty() { format!("depends on {failed}, which failed: {why}") } else { format!("depends on {root}") })
                    }
                    (Err(e), None) => Err(e.to_string()),
                    (Ok(v), _) => Ok(v),
                }
            };
            // A tracked read reached a cell nothing needed: it and what it
            // reads are needed now. The stale ones among them are evaluated
            // first and this binding after them, before what it read is
            // checked for a cycle (a stale cell's edges are what it read
            // when it was last evaluated, not what it reads now). Whether
            // or not their values move, this binding runs again.
            let reached = reached.into_inner();
            if !reached.is_empty() {
                let top = reached.iter().filter(|c| self.exists(**c)).map(|c| self.height_of(*c)).max().unwrap_or(0);
                let (wake, taken) = self.wake_counted(reached);
                if !wake.is_empty() {
                    budget = budget.saturating_add(taken.saturating_mul(4));
                    for c in wake {
                        primed.insert(c);
                        queue.insert((self.height_of(c), c));
                    }
                    // After everything waiting, nested wakes included.
                    primed.insert(cell);
                    let last = queue.last().map_or(0, |(h, _)| *h);
                    queue.insert((last.max(top).max(self.nodes[&leaf].height) + 1, cell));
                    continue;
                }
            }
            // What the tracked reads read this time becomes what this
            // binding depends on, beyond its static dependencies.
            let mut result = result;
            if tracks {
                let node = &self.nodes[&leaf];
                let tracked: BTreeSet<Leaf> = reads.into_inner().into_iter().filter(|l| !node.deps.contains(l)).collect();
                if tracked != node.tracked {
                    let old = node.tracked.clone();
                    if let Some(own) = tracked.iter().find(|t| **t == leaf || self.cell_of(**t).is_some_and(|tc| self.reaches(tc, cell))) {
                        // What it reads, or what that reads, may still be
                        // waiting in this pass (a wake beneath it): its edges
                        // are those of its last evaluation until it runs.
                        // Read this once more after everything waiting.
                        if primed.iter().any(|t| *t != cell && self.exists(*t)) && deferred.insert(cell) {
                            primed.insert(cell);
                            let last = queue.last().map_or(0, |(h, _)| *h);
                            queue.insert((last.max(self.nodes[&leaf].height) + 1, cell));
                            continue;
                        }
                        result = Err(format!("reads {own} through deref, which depends on this leaf: a cycle"));
                        self.blocked.insert(cell);
                    } else {
                        for gone in old.difference(&tracked) {
                            if let Some(ds) = self.dependents.get_mut(gone) {
                                ds.remove(&cell);
                            }
                            edge_removed = true;
                            // It may have been needed only for this read.
                            if let Some(g) = self.cell_of(*gone)
                                && self.candidates.contains(&g)
                                && !self.unneeded.contains(&g)
                            {
                                self.relaxed = true;
                            }
                        }
                        for new in tracked.difference(&old) {
                            self.dependents.entry(*new).or_default().insert(cell);
                        }
                        // A newly read binding still waiting in this pass:
                        // read it again once it has been evaluated.
                        let waiting = tracked.difference(&old).filter_map(|t| self.cell_of(*t)).any(|t| primed.contains(&t));
                        // Heights move only when it now reads something
                        // at or above its own.
                        let need = 1 + tracked.iter().filter_map(|t| self.cell_of(*t)).map(|t| self.height_of(t)).max().unwrap_or(0);
                        let node = self.nodes.get_mut(&leaf).expect("bound");
                        node.tracked = tracked;
                        if need > node.height {
                            self.compute_heights();
                        }
                        if waiting {
                            primed.insert(cell);
                            queue.insert((self.nodes[&leaf].height, cell));
                            continue;
                        }
                    }
                }
            }
            // Readers run again when this value changes, and when it starts
            // or stops failing (a recovery to the same value still clears
            // their errors).
            if result.is_ok() {
                self.blocked.remove(&cell);
            }
            let changed = match result {
                Ok(v) => {
                    let recovered = self.errors.remove(&leaf).is_some();
                    if self.resolved.get(&leaf) != Some(&v) {
                        self.resolved.insert(leaf, v.clone());
                        self.note(leaf, v);
                        true
                    } else {
                        recovered
                    }
                }
                // A new reason is a change too: readers name it ("depends
                // on …, which failed: <why>").
                Err(e) => self.errors.insert(leaf, e.clone()).as_ref() != Some(&e),
            };
            if changed {
                let ds: Vec<CellId> = self.readers_of(cell).collect();
                for d in ds {
                    primed.insert(d);
                    // A reader by a tracked read may already have run.
                    if self.exists(d) {
                        queue.insert((self.height_of(d), d));
                    }
                }
            }
        }
        if edge_removed && !self.blocked.is_empty() && retries < 4 {
            edge_removed = false;
            retries += 1;
            let blocked: Vec<CellId> = self.blocked.iter().copied().filter(|c| self.exists(*c)).collect();
            for c in blocked {
                primed.insert(c);
                queue.insert((self.height_of(c), c));
            }
            continue;
        }
        break;
        }
    }

    /// Run native `k` in the pass: its reads become its edges, its result
    /// its claims (handoff §4.10). It is queued again instead when what it
    /// read was stale and nothing needed it, or when its reads raised it
    /// above a cell still queued (the kernel's R5): its result would be
    /// provisional. The budget the wakes it made add.
    fn run_native(&mut self, doc: Tree<'_>, k: NativeKey, pass: &mut Pass<'_>) -> usize {
        let cell = CellId::Native(k);
        let rule = self.natives[&k].rule.clone();
        self.evaluations += 1;
        let (result, reads, base, reached) = {
            let mut cx = Cx { doc, graph: &*self, me: k, reads: BTreeSet::new(), base: BTreeSet::new(), reached: BTreeSet::new() };
            let result = rule.compute(&mut cx);
            (result, cx.reads, cx.base, cx.reached)
        };
        // What it read that nothing needed is needed now: evaluated first,
        // this after it.
        if !reached.is_empty() {
            let top = reached.iter().filter(|c| self.exists(**c)).map(|c| self.height_of(*c)).max().unwrap_or(0);
            let (wake, taken) = self.wake_counted(reached);
            if !wake.is_empty() {
                for c in wake {
                    pass.primed.insert(c);
                    pass.queue.insert((self.height_of(c), c));
                }
                pass.primed.insert(cell);
                let last = pass.queue.last().map_or(0, |(h, _)| *h);
                pass.queue.insert((last.max(top).max(self.height_of(cell)) + 1, cell));
                return taken.saturating_mul(4);
            }
        }
        // Its edges: what it read this time.
        let old = std::mem::take(&mut self.natives.get_mut(&k).expect("native").reads);
        for gone in old.difference(&reads) {
            if let Some(ds) = self.dependents.get_mut(gone) {
                ds.remove(&cell);
                if ds.is_empty() {
                    self.dependents.remove(gone);
                }
            }
            *pass.edge_removed = true;
            if let Some(g) = self.cell_of(*gone)
                && self.candidates.contains(&g)
                && !self.unneeded.contains(&g)
            {
                self.relaxed = true;
            }
        }
        for new in reads.difference(&old) {
            self.dependents.entry(*new).or_default().insert(cell);
        }
        let old_base = std::mem::take(&mut self.natives.get_mut(&k).expect("native").base);
        for gone in old_base.difference(&base) {
            if let Some(ns) = self.base_readers.get_mut(gone) {
                ns.remove(&k);
                if ns.is_empty() {
                    self.base_readers.remove(gone);
                }
            }
        }
        for new in base.difference(&old_base) {
            self.base_readers.entry(*new).or_default().insert(k);
        }
        let native = self.natives.get_mut(&k).expect("native");
        native.reads = reads;
        native.base = base;
        // A read that reaches this native back is a cycle; it may run
        // through an edge a cell still queued drops, so it waits once.
        let mut result = result;
        if let Some(own) = self.natives[&k].reads.iter().find(|t| self.cell_of(**t).is_some_and(|tc| self.reaches(tc, cell))).copied() {
            if pass.primed.iter().any(|t| *t != cell && self.exists(*t)) && pass.deferred.insert(cell) {
                pass.primed.insert(cell);
                let last = pass.queue.last().map_or(0, |(h, _)| *h);
                pass.queue.insert((last.max(self.height_of(cell)) + 1, cell));
                return 0;
            }
            result = Err(Failure::Error(format!("reads {own}, which depends on this: a cycle")));
            self.blocked.insert(cell);
        } else {
            // Above everything it reads now; if that raised it over a cell
            // still waiting in this pass, it runs again after it (R5).
            let need = 1 + self.reads_of(cell).into_iter().map(|d| self.height_of(d)).max().unwrap_or(0);
            if need > self.height_of(cell) {
                self.set_height(cell, need);
                if !self.raise(&[cell]) {
                    self.compute_heights();
                }
                let h = self.height_of(cell);
                if pass.queue.iter().any(|(qh, qc)| *qh < h && pass.primed.contains(qc)) {
                    pass.primed.insert(cell);
                    pass.queue.insert((h, cell));
                    return 0;
                }
            }
        }
        // Its result: claims, or a failure.
        let outputs: Vec<Leaf> = rule.outputs().iter().copied().filter(|o| !self.nodes.contains_key(o)).collect();
        let mut changed: Vec<Leaf> = Vec::new();
        match result {
            Ok(values) => {
                self.blocked.remove(&cell);
                self.native_problems.remove(&k);
                for o in &outputs {
                    if self.errors.remove(o).is_some() {
                        changed.push(*o);
                    }
                }
                changed.extend(self.claim(doc, k, values));
            }
            Err(Failure::Error(e)) => {
                self.native_problems.remove(&k);
                for o in &outputs {
                    if self.errors.insert(*o, e.clone()).as_ref() != Some(&e) {
                        changed.push(*o);
                    }
                }
            }
            Err(Failure::KeepLast(e)) => {
                self.blocked.remove(&cell);
                self.native_problems.insert(k, e);
            }
        }
        changed.sort();
        changed.dedup();
        for o in changed {
            let ds: Vec<CellId> = self.dependents.get(&o).into_iter().flatten().copied().filter(|d| *d != cell).collect();
            for d in ds {
                pass.primed.insert(d);
                if self.exists(d) {
                    pass.queue.insert((self.height_of(d), d));
                }
            }
        }
        0
    }

    /// Replace what native `k` claims with `values`, with `publish`'s rules
    /// (first claim wins, a bound leaf is refused, a leaf left out is
    /// released). The leaves whose readable value moved.
    fn claim(&mut self, doc: Tree<'_>, k: NativeKey, values: BTreeMap<Leaf, Value>) -> Vec<Leaf> {
        let producer = self.natives[&k].rule.producer();
        let was = self.claims.remove(&k).unwrap_or_default();
        let mut now: BTreeSet<Leaf> = BTreeSet::new();
        let mut changed: Vec<Leaf> = Vec::new();
        for (leaf, value) in values {
            if !self.natives[&k].rule.outputs().contains(&leaf) {
                debug_assert!(false, "{k:?} wrote {leaf}, outside its outputs");
                continue;
            }
            if self.nodes.contains_key(&leaf) {
                self.signal_errors.insert(leaf, format!("{leaf} is bound; producer {} cannot drive it", producer.0));
                continue;
            }
            match self.runtime.get(&leaf) {
                Some(s) if s.producer != producer => {
                    self.signal_errors.insert(leaf, format!("{leaf} is already driven by producer {}; producer {} cannot also drive it", s.producer.0, producer.0));
                    continue;
                }
                Some(s) if s.value == value => {}
                _ => {
                    self.runtime.insert(leaf, Signal { value, producer });
                    changed.push(leaf);
                }
            }
            self.signal_errors.remove(&leaf);
            now.insert(leaf);
        }
        for leaf in was.difference(&now) {
            if self.runtime.get(leaf).is_some_and(|s| s.producer == producer) {
                self.runtime.remove(leaf);
                changed.push(*leaf);
            }
        }
        let held = self.held.entry(producer).or_default();
        for l in &was {
            held.remove(l);
        }
        held.extend(now.iter().copied());
        if held.is_empty() {
            self.held.remove(&producer);
        }
        if !now.is_empty() {
            self.claims.insert(k, now);
        }
        for &l in &changed {
            if !self.overlay.contains_key(&l)
                && let Some(v) = self.read(doc, l)
            {
                self.note(l, v);
            }
        }
        changed
    }

    /// Make, replace or drop native cells as one batch (`None` drops), then
    /// run the ones made and what read what was dropped (handoff §4.10:
    /// natives are made where structure changes, not inside a pass). A
    /// native kept with the same rule is left as it is; `rooted` ones run
    /// whether or not anything observed needs them.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_natives(&mut self, doc: Tree<'_>, batch: Vec<(NativeKey, Option<(std::sync::Arc<dyn Native>, bool)>)>) {
        let mut primed: BTreeSet<CellId> = BTreeSet::new();
        let mut made: Vec<CellId> = Vec::new();
        for (k, new) in batch {
            let cell = CellId::Native(k);
            if let Some(old) = self.natives.remove(&k) {
                for r in &old.reads {
                    if let Some(ds) = self.dependents.get_mut(r) {
                        ds.remove(&cell);
                        if ds.is_empty() {
                            self.dependents.remove(r);
                        }
                    }
                }
                for b in &old.base {
                    if let Some(ns) = self.base_readers.get_mut(b) {
                        ns.remove(&k);
                        if ns.is_empty() {
                            self.base_readers.remove(b);
                        }
                    }
                }
                for o in old.rule.outputs() {
                    if self.owned.get(o) == Some(&k) {
                        self.owned.remove(o);
                    }
                    if self.errors.remove(o).is_some() {
                        primed.extend(self.dependents.get(o).into_iter().flatten().copied());
                    }
                }
                self.blocked.remove(&cell);
                self.unneeded.remove(&cell);
                self.stale.remove(&cell);
                self.pinned.remove(&cell);
                self.candidates.remove(&cell);
                self.native_problems.remove(&k);
                if new.is_none() {
                    // Its claims go: released leaves read as before it.
                    self.natives.insert(k, old);
                    for l in self.claim(doc, k, BTreeMap::new()) {
                        primed.extend(self.dependents.get(&l).into_iter().flatten().copied());
                    }
                    self.natives.remove(&k);
                }
            }
            if let Some((rule, rooted)) = new {
                for o in rule.outputs() {
                    debug_assert!(self.owned.get(o).is_none_or(|o2| *o2 == k), "{o} owned twice");
                    self.owned.insert(*o, k);
                }
                self.natives.insert(k, NativeCell { rule, reads: BTreeSet::new(), base: BTreeSet::new(), height: 1, rooted });
                made.push(cell);
                primed.insert(cell);
                // Readers of its outputs now read through it.
                let readers: Vec<CellId> = self.natives[&k].rule.outputs().iter().flat_map(|o| self.dependents.get(o).into_iter().flatten().copied()).collect();
                primed.extend(readers);
            }
        }
        // Readers of a native's outputs sit above it.
        if !self.raise(&made) {
            self.compute_heights();
        }
        primed.retain(|c| self.exists(*c));
        let dirty = self.readers(&primed);
        self.evaluate(doc, &dirty, &primed);
    }
}

/// The pass's state a native's run takes part in.
struct Pass<'a> {
    queue: &'a mut BTreeSet<(u32, CellId)>,
    primed: &'a mut BTreeSet<CellId>,
    deferred: &'a mut BTreeSet<CellId>,
    edge_removed: &'a mut bool,
}

/// The least leaf of `node`, where its cells start in leaf order.
fn first_leaf(node: NodeId) -> Leaf {
    let first = *FacetKind::ALL.iter().min().expect("facet kinds");
    Leaf { node, path: crate::facet::LeafPath { facet: first, field: "" } }
}

/// Whether a binding's evaluation records what it reads: it holds a
/// `deref`, or it is a boundary whose type check reads the instance's
/// arguments (`types::declared`).
fn tracks_reads(doc: Tree<'_>, leaf: Leaf, expr: &Expr) -> bool {
    let mut tracks = false;
    expr.walk(&mut |e| tracks |= matches!(e, Expr::Call(name, _) if name == "deref"));
    tracks || crate::types::declared(doc, leaf).is_some()
}

fn read_with(
    overlay: &BTreeMap<Leaf, Value>,
    runtime: &BTreeMap<Leaf, Signal>,
    resolved: &BTreeMap<Leaf, Value>,
    nodes: &BTreeMap<Leaf, BoundNode>,
    doc: Tree<'_>,
    leaf: Leaf,
) -> Option<Value> {
    if let Some(v) = overlay.get(&leaf) {
        return Some(v.clone());
    }
    if nodes.contains_key(&leaf) {
        return resolved.get(&leaf).cloned();
    }
    if let Some(s) = runtime.get(&leaf) {
        return Some(s.value.clone());
    }
    // A node that went has no stored value: a tombstone's facets are kept
    // for undo, and a copy removed outright has none (handoff §4.6).
    doc.live(leaf.node)?;
    doc.leaf(leaf.node, leaf.path)?.constant().cloned()
}

/// A bound value as its leaf holds it: SVG path data on a geometry leaf
/// (`path.d`) becomes geometry (plans/specs/plot.md §2), and a quoted word
/// on a text leaf is text.
fn as_leaf_kind(leaf: Leaf, v: Value) -> Result<Value, crate::expr::EvalError> {
    match v {
        // Text on a path is SVG path data.
        Value::Str(text) | Value::Enum(text) if leaf.path.kind() == crate::value::ValueKind::Geometry => crate::geometry::Geometry::from_svg_path(&text)
            .map(|g| Value::Geometry(std::sync::Arc::new(g)))
            .map_err(crate::expr::EvalError::Type),
        // Text on a choice leaf names the option (`{= 'evenodd'}`), and a
        // choice on a text leaf is its word.
        Value::Str(text) if leaf.path.kind() == crate::value::ValueKind::Enum => Ok(Value::Enum(text)),
        Value::Enum(text) if leaf.path.kind() == crate::value::ValueKind::Str => Ok(Value::Str(text)),
        v => Ok(v),
    }
}

/// How a boundary leaf is named in its type error: an argument by its
/// name, a port's signal by the port's name.
fn boundary_name(doc: Tree<'_>, leaf: Leaf) -> String {
    match leaf.path.facet {
        crate::facet::FacetKind::Port => doc.get::<crate::facet::Name>(leaf.node).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).unwrap_or("port").to_owned(),
        _ => leaf.path.field.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    //! Native cells (handoff §4.10, RE-5.3 step 2), with test natives over
    //! a small document.
    use super::*;
    use crate::{Actor, Op, Slot, StoreCore, TransactionInput};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn leaf(node: NodeId, path: &str) -> Leaf {
        Leaf { node, path: path.parse().expect("declared") }
    }

    fn tx(ops: Vec<Op>) -> TransactionInput {
        TransactionInput { label: "t".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false }
    }

    fn set(store: &mut StoreCore, l: Leaf, slot: Slot) {
        store.apply(tx(vec![Op::Set { id: l.node, path: l.path, slot }])).expect("applies");
    }

    fn num(store: &StoreCore, l: Leaf) -> Option<f64> {
        store.reactive.read(Tree::of(&store.docs), l).and_then(|v| v.as_f64())
    }

    /// A document of rects named in `xml` (`<rect id="$a" …/>`), by name.
    fn doc(xml: &str) -> (StoreCore, BTreeMap<String, NodeId>) {
        let mut store = StoreCore::new();
        let src = format!(r#"<edit><composition name="c" dims="2d">{xml}</composition></edit>"#);
        let plan = crate::edit::plan(&store, &src, None).unwrap_or_else(|f| panic!("{}", f.message()));
        let names = plan.aliases.clone();
        store.apply(tx(plan.ops)).expect("applies");
        (store, names)
    }

    /// `to = f(the values of from)`, counting its runs. `pick` chooses
    /// which of `from` it reads, by the value of its first: a read that
    /// changes with values.
    #[derive(Debug)]
    struct Test {
        outputs: BTreeSet<Leaf>,
        from: Vec<Leaf>,
        to: Vec<Leaf>,
        runs: Arc<AtomicUsize>,
        mode: Mode,
    }

    #[derive(Debug, Clone, Copy)]
    enum Mode {
        /// Each output is the sum of `from`, times its position plus one.
        Sum,
        /// Reads `from[0]`, then `from[1 + that value]`: the output is it.
        Pick,
        /// Fails.
        Fail(bool),
        /// The first output only, from the second `from` alone.
        Partial,
    }

    impl Native for Test {
        fn producer(&self) -> ProducerId {
            TEST
        }
        fn outputs(&self) -> &BTreeSet<Leaf> {
            &self.outputs
        }
        fn compute(&self, cx: &mut Cx<'_>) -> Result<BTreeMap<Leaf, Value>, Failure> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            let n = |v: Option<Value>| v.and_then(|v| v.as_f64()).unwrap_or(0.0);
            match self.mode {
                Mode::Sum => {
                    let sum: f64 = self.from.iter().map(|l| n(cx.read(*l))).sum();
                    Ok(self.to.iter().enumerate().map(|(i, l)| (*l, Value::Number(sum * (i + 1) as f64))).collect())
                }
                Mode::Pick => {
                    let i = n(cx.read(self.from[0])) as usize;
                    let v = n(cx.read(self.from[1 + i]));
                    Ok(self.to.iter().map(|l| (*l, Value::Number(v))).collect())
                }
                Mode::Fail(keep) => {
                    let _ = cx.read(self.from[0]);
                    Err(if keep { Failure::KeepLast("did not converge".into()) } else { Failure::Error("broken".into()) })
                }
                Mode::Partial => Ok(BTreeMap::from([(self.to[0], Value::Number(n(cx.read(self.from[1]))))])),
            }
        }
    }

    /// The test natives' producer: not one the store's pipeline publishes.
    const TEST: ProducerId = ProducerId(900);

    fn native(store: &mut StoreCore, key: u32, from: Vec<Leaf>, to: Vec<Leaf>, mode: Mode, rooted: bool) -> Arc<AtomicUsize> {
        let runs = Arc::new(AtomicUsize::new(0));
        let rule = Arc::new(Test { outputs: to.iter().copied().collect(), from, to: to.clone(), runs: runs.clone(), mode });
        let k = NativeKey::Test(to[0].node, key);
        store.reactive.set_natives(Tree::of(&store.docs), vec![(k, Some((rule as Arc<dyn Native>, rooted)))]);
        runs
    }

    #[test]
    fn a_reader_of_a_native_runs_once_per_pass_after_it() {
        // c.w reads both a.w and the native's b.h, which reads a.w: a
        // diamond through the native.
        let (mut s, n) = doc(r#"<rect id="$a" w="10" h="1"/><rect id="$b" w="1" h="1"/><rect id="$c" w="{= $b.size.h + $a.size.w}" h="1"/>"#);
        let (a, b, c) = (n["$a"], n["$b"], n["$c"]);
        let runs = native(&mut s, 0, vec![leaf(a, "size.w")], vec![leaf(b, "size.h")], Mode::Sum, true);
        assert_eq!(num(&s, leaf(b, "size.h")), Some(10.0));
        assert_eq!(num(&s, leaf(c, "size.w")), Some(20.0));
        assert!(s.reactive.height(leaf(c, "size.w")).unwrap() > s.reactive.natives[&NativeKey::Test(b, 0)].height);
        let (r0, e0) = (runs.load(Ordering::SeqCst), s.reactive.evaluations);
        // A constant-only transaction re-runs it.
        set(&mut s, leaf(a, "size.w"), Slot::Const(Value::Number(7.0)));
        assert_eq!(num(&s, leaf(b, "size.h")), Some(7.0));
        assert_eq!(num(&s, leaf(c, "size.w")), Some(14.0));
        assert_eq!(runs.load(Ordering::SeqCst) - r0, 1, "the native runs once");
        assert_eq!(s.reactive.evaluations - e0, 2, "the native and c, once each");
    }

    #[test]
    fn only_the_readers_of_an_output_that_moved_run_and_a_released_one_falls_back() {
        let (mut s, n) = doc(r#"<rect id="$a" w="0" h="1"/><rect id="$z" w="3" h="1"/><rect id="$b" w="5" h="6"/><rect id="$c" w="{= $b.size.w + 1}" h="{= $b.size.h + 1}"/>"#);
        let (a, z, b, c) = (n["$a"], n["$z"], n["$b"], n["$c"]);
        // b.w = 0·1, b.h = 0·2: both 0 at first.
        let _ = native(&mut s, 0, vec![leaf(a, "size.w")], vec![leaf(b, "size.w"), leaf(b, "size.h")], Mode::Sum, true);
        assert_eq!((num(&s, leaf(c, "size.w")), num(&s, leaf(c, "size.h"))), (Some(1.0), Some(1.0)));
        // Replaced by one that writes only b.w, from z: b.h is released and
        // reads its constant again.
        let _ = native(&mut s, 0, vec![leaf(a, "size.w"), leaf(z, "size.w")], vec![leaf(b, "size.w"), leaf(b, "size.h")], Mode::Partial, true);
        assert_eq!(num(&s, leaf(b, "size.h")), Some(6.0));
        assert_eq!(num(&s, leaf(c, "size.h")), Some(7.0));
        assert_eq!(num(&s, leaf(c, "size.w")), Some(4.0));
        // z moves b.w only: c.h does not run.
        let e0 = s.reactive.evaluations;
        set(&mut s, leaf(z, "size.w"), Slot::Const(Value::Number(8.0)));
        assert_eq!(num(&s, leaf(c, "size.w")), Some(9.0));
        assert_eq!(s.reactive.evaluations - e0, 2, "the native and c.w");
        // It no longer reads a: a change there does not run it.
        let e0 = s.reactive.evaluations;
        set(&mut s, leaf(a, "size.w"), Slot::Const(Value::Number(4.0)));
        assert_eq!(s.reactive.evaluations - e0, 0, "its reads are what it read last");
        // Dropped, its claim goes and b.w reads its constant.
        s.reactive.set_natives(Tree::of(&s.docs), vec![(NativeKey::Test(b, 0), None)]);
        assert_eq!(num(&s, leaf(c, "size.w")), Some(6.0));
        assert!(s.reactive.published_by(TEST).is_empty());
    }

    #[test]
    fn a_native_reads_beneath_its_own_claim_without_a_cycle() {
        // b.w = b.w (beneath) + a.w: it reads the leaf it writes.
        let (mut s, n) = doc(r#"<rect id="$a" w="2" h="1"/><rect id="$b" w="10" h="1"/>"#);
        let (a, b) = (n["$a"], n["$b"]);
        let runs = native(&mut s, 0, vec![leaf(b, "size.w"), leaf(a, "size.w")], vec![leaf(b, "size.w")], Mode::Sum, true);
        assert_eq!(num(&s, leaf(b, "size.w")), Some(12.0));
        assert!(s.reactive.native_problems.is_empty() && s.reactive.errors.is_empty());
        // Its own claim does not feed it back.
        let r0 = runs.load(Ordering::SeqCst);
        set(&mut s, leaf(a, "size.w"), Slot::Const(Value::Number(3.0)));
        assert_eq!(num(&s, leaf(b, "size.w")), Some(13.0));
        assert_eq!(runs.load(Ordering::SeqCst) - r0, 1);
        // The author's constant beneath it still reaches it.
        set(&mut s, leaf(b, "size.w"), Slot::Const(Value::Number(20.0)));
        assert_eq!(num(&s, leaf(b, "size.w")), Some(23.0));
    }

    #[test]
    fn a_cycle_through_a_native_is_an_error_and_recovers_when_it_goes() {
        let (mut s, n) = doc(r#"<rect id="$a" w="1" h="1"/><rect id="$b" w="1" h="1"/>"#);
        let (a, b) = (n["$a"], n["$b"]);
        let _ = native(&mut s, 0, vec![leaf(a, "size.w")], vec![leaf(b, "size.h")], Mode::Sum, true);
        // a.w reads the native's output: a cycle.
        set(&mut s, leaf(a, "size.w"), Slot::Bound(crate::expr::Expr::parse(&format!("{b}.size.h + 1")).unwrap()));
        let failed = s.reactive.errors.contains_key(&leaf(a, "size.w")) || s.reactive.errors.contains_key(&leaf(b, "size.h"));
        assert!(failed, "a cycle is reported: {:?}", s.reactive.errors);
        // Unbound again, it settles.
        set(&mut s, leaf(a, "size.w"), Slot::Const(Value::Number(5.0)));
        assert_eq!(num(&s, leaf(b, "size.h")), Some(5.0));
        assert!(s.reactive.errors.is_empty(), "{:?}", s.reactive.errors);
    }

    #[test]
    fn a_native_raised_over_a_cell_still_queued_runs_again_after_it() {
        // The native reads sel, then from[1 + sel]: at first a.w, then the
        // top of a chain of bindings that a.w also feeds. One transaction
        // moves both: the native must not keep the chain's old value.
        let (mut s, n) = doc(r#"<rect id="$sel" w="0" h="1"/><rect id="$a" w="1" h="1"/><rect id="$p" w="{= $a.size.w + 1}" h="1"/><rect id="$q" w="{= $p.size.w + 1}" h="1"/><rect id="$r" w="{= $q.size.w + 1}" h="1"/><rect id="$out" w="0" h="1"/>"#);
        let (sel, a, r, out) = (n["$sel"], n["$a"], n["$r"], n["$out"]);
        let _ = native(&mut s, 0, vec![leaf(sel, "size.w"), leaf(a, "size.w"), leaf(r, "size.w")], vec![leaf(out, "size.w")], Mode::Pick, true);
        assert_eq!(num(&s, leaf(out, "size.w")), Some(1.0));
        // A second native reads its output: it must see one value, not a
        // provisional one first.
        let reader = native(&mut s, 1, vec![leaf(out, "size.w")], vec![leaf(out, "size.h")], Mode::Sum, true);
        let r0 = reader.load(Ordering::SeqCst);
        s.apply(tx(vec![
            Op::Set { id: sel, path: "size.w".parse().unwrap(), slot: Slot::Const(Value::Number(1.0)) },
            Op::Set { id: a, path: "size.w".parse().unwrap(), slot: Slot::Const(Value::Number(10.0)) },
        ]))
        .expect("applies");
        assert_eq!(num(&s, leaf(r, "size.w")), Some(13.0));
        assert_eq!(num(&s, leaf(out, "size.w")), Some(13.0), "the native read the chain once it was current");
        assert_eq!(reader.load(Ordering::SeqCst) - r0, 1, "its reader ran once, on the final value");
    }

    #[test]
    fn a_binding_blocked_on_a_cycle_recovers_when_another_drops_the_edge() {
        // x reads y through px, y reads x through py: a cycle. Pointing py
        // elsewhere is a value change at y alone; x's read of y was never
        // made an edge, so only the retry (the kernel's R6) frees it.
        // y runs first and reads x; x then finds the cycle and is blocked.
        let (mut s, n) = doc(r#"<rect id="$y" w="{= deref($py.let.value, 'size.w', 0) + 1}" h="1"/><rect id="$x" w="{= deref($px.let.value, 'size.w', 0)}" h="1"/><rect id="$z" w="5" h="1"/><let id="$px" value="0"/><let id="$py" value="0"/>"#);
        let (x, y, z, px, py) = (n["$x"], n["$y"], n["$z"], n["$px"], n["$py"]);
        s.apply(tx(vec![
            Op::Set { id: px, path: "let.value".parse().unwrap(), slot: Slot::Const(Value::Ref(y)) },
            Op::Set { id: py, path: "let.value".parse().unwrap(), slot: Slot::Const(Value::Ref(x)) },
        ]))
        .expect("applies");
        assert!(s.reactive.errors.contains_key(&leaf(x, "size.w")), "x is blocked on the cycle: {:?}", s.reactive.errors);
        s.reactive.preview(Tree::of(&s.docs), leaf(py, "let.value"), Value::Ref(z));
        assert_eq!(num(&s, leaf(y, "size.w")), Some(6.0));
        assert_eq!(num(&s, leaf(x, "size.w")), Some(6.0), "x recovered: {:?}", s.reactive.errors);
        assert!(s.reactive.errors.is_empty(), "{:?}", s.reactive.errors);
    }

    #[test]
    fn a_native_nothing_needs_rests_and_runs_when_read() {
        let (mut s, n) = doc(r#"<rect id="$a" w="1" h="1"/><rect id="$b" w="1" h="1"/>"#);
        let (a, b) = (n["$a"], n["$b"]);
        let runs = native(&mut s, 0, vec![leaf(a, "size.w")], vec![leaf(b, "size.h")], Mode::Sum, false);
        let doc = Tree::of(&s.docs);
        let unneeded = s.reactive.necessity(&BTreeSet::from([b]), &|_| false);
        assert!(unneeded.contains(&CellId::Native(NativeKey::Test(b, 0))));
        s.reactive.set_unneeded(doc, unneeded);
        let r0 = runs.load(Ordering::SeqCst);
        s.reactive.preview(Tree::of(&s.docs), leaf(a, "size.w"), Value::Number(4.0));
        assert_eq!(runs.load(Ordering::SeqCst), r0, "not run while nothing needs it");
        assert!(s.reactive.is_stale(leaf(b, "size.h")));
        // Read from outside the graph, it is needed and brought up to date.
        s.reactive.need(Tree::of(&s.docs), &BTreeSet::from([leaf(b, "size.h")]));
        assert_eq!(num(&s, leaf(b, "size.h")), Some(4.0));
        // A rooted one runs regardless.
        let rooted = native(&mut s, 1, vec![leaf(a, "size.w")], vec![leaf(b, "size.w")], Mode::Sum, true);
        let unneeded = s.reactive.necessity(&BTreeSet::from([b]), &|_| false);
        assert!(!unneeded.contains(&CellId::Native(NativeKey::Test(b, 1))));
        let _ = rooted;
    }

    #[test]
    fn a_failing_native_fails_its_readers_or_keeps_its_last_claims() {
        let (mut s, n) = doc(r#"<rect id="$a" w="1" h="1"/><rect id="$b" w="1" h="1"/><rect id="$c" w="{= $b.size.h + 1}" h="1"/>"#);
        let (a, b, c) = (n["$a"], n["$b"], n["$c"]);
        let _ = native(&mut s, 0, vec![leaf(a, "size.w")], vec![leaf(b, "size.h")], Mode::Sum, true);
        assert_eq!(num(&s, leaf(c, "size.w")), Some(2.0));
        let _ = native(&mut s, 0, vec![leaf(a, "size.w")], vec![leaf(b, "size.h")], Mode::Fail(true), true);
        assert_eq!(num(&s, leaf(c, "size.w")), Some(2.0), "KeepLast: the claim stands");
        assert!(s.reactive.native_problems.contains_key(&NativeKey::Test(b, 0)));
        let _ = native(&mut s, 0, vec![leaf(a, "size.w")], vec![leaf(b, "size.h")], Mode::Fail(false), true);
        assert!(s.reactive.errors.get(&leaf(c, "size.w")).is_some_and(|e| e.contains("depends on")), "{:?}", s.reactive.errors);
        let _ = native(&mut s, 0, vec![leaf(a, "size.w")], vec![leaf(b, "size.h")], Mode::Sum, true);
        assert!(s.reactive.errors.is_empty(), "{:?}", s.reactive.errors);
    }

    #[test]
    fn the_first_claim_on_a_leaf_wins_between_a_host_and_a_native() {
        let (mut s, n) = doc(r#"<rect id="$a" w="1" h="1"/><rect id="$b" w="1" h="1"/><rect id="$d" w="1" h="1"/>"#);
        let (a, b, d) = (n["$a"], n["$b"], n["$d"]);
        let host = s.register_producer("host");
        // Host first.
        s.publish(host, BTreeMap::from([(leaf(b, "size.h"), Value::Number(9.0))]));
        let _ = native(&mut s, 0, vec![leaf(a, "size.w")], vec![leaf(b, "size.h")], Mode::Sum, true);
        assert_eq!(num(&s, leaf(b, "size.h")), Some(9.0));
        assert!(s.reactive.signal_errors().contains_key(&leaf(b, "size.h")));
        // Native first.
        let _ = native(&mut s, 1, vec![leaf(a, "size.w")], vec![leaf(d, "size.h")], Mode::Sum, true);
        s.publish(host, BTreeMap::from([(leaf(d, "size.h"), Value::Number(9.0))]));
        assert_eq!(num(&s, leaf(d, "size.h")), Some(1.0));
        assert!(s.reactive.driven_by(leaf(d, "size.h"), TEST));
    }
}
