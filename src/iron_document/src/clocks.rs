//! Composition clocks and incremental owner index (`EN-8b`, PT-1.2,
//! reactive-engine.md §10.1).
//!
//! Every live node belongs to one `Owner`:
//! - `Owner::Composition(c)` — a composition and everything drawn under it.
//! - `Owner::Entity(e)` — a model entity pictured by at least one composition
//!   via `RelKind::Figures` (and everything under it outside compositions).
//! - `Owner::Free` — anything outside every composition and pictured entity
//!   (timeline, loose roots, unpictured model entities, rooted network islands).
//!
//! `OwnerIndex` maintains per-owner runtime node membership and cross-owner
//! reactive read counts incrementally, so computing `ActiveOwners` and applying
//! delta clock transitions runs in O(|active| + |cross-owner edges| + |delta|)
//! and never scans hidden compositions or their descendants.

use crate::composition::{self, RunMode};
use crate::document::RelKind;
use crate::expr::Leaf;
use crate::facet::FacetKind;
use crate::graph::{CellId, Reactive};
use crate::id::{NodeId, RelationId};
use crate::registry::TypeId;
use crate::tree::Tree;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

/// Ownership partition of live nodes for clock gating and demand activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Owner {
    Composition(NodeId),
    Entity(NodeId),
    Free,
}

impl Owner {
    /// The clock key for this owner, if it has a pausable clock (`Free` always
    /// reads presentation time).
    pub(crate) fn clock_key(self) -> Option<NodeId> {
        match self {
            Owner::Composition(c) | Owner::Entity(c) => Some(c),
            Owner::Free => None,
        }
    }
}

/// Delta-maintained composition and pictured-entity clocks (`EN-8b`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Clocks {
    /// When each paused clock stopped, in presentation time.
    since: crate::work::Counted<BTreeMap<NodeId, f64>, { crate::work::CLOCKS }>,
    /// How long each clock spent paused before its current pause.
    behind: crate::work::Counted<BTreeMap<NodeId, f64>, { crate::work::CLOCKS }>,
    /// How many times the gate recomputed active owners (`Store::gate_decisions`).
    pub(crate) decisions: u64,
}

impl Clocks {
    /// Pause clock `k` at presentation time `now` (idempotent if already paused).
    pub(crate) fn pause(&mut self, k: NodeId, now: f64) {
        self.since.entry(k).or_insert(now);
    }

    /// Resume clock `k` at presentation time `now`, folding its pause duration
    /// into `behind`.
    pub(crate) fn resume(&mut self, k: NodeId, now: f64) {
        if let Some(stopped) = self.since.remove(&k) {
            *self.behind.entry(k).or_default() += (now - stopped).max(0.0);
        }
    }

    /// Resume every currently paused clock (used only when switching
    /// `observe_all` from `false` to `true`).
    pub(crate) fn resume_all(&mut self, now: f64) {
        let keys: Vec<NodeId> = self.since.keys().copied().collect();
        for k in keys {
            self.resume(k, now);
        }
    }

    /// Forget a deleted or unpictured clock key in O(log N).
    pub(crate) fn forget(&mut self, k: NodeId) {
        self.since.remove(&k);
        self.behind.remove(&k);
    }

    /// Whether `node`'s clock is paused.
    pub(crate) fn held(&self, doc: Tree<'_>, owners: &OwnerIndex, node: NodeId) -> bool {
        if self.since.is_empty() {
            return false;
        }
        owners.owner_of(doc, node).clock_key().is_some_and(|k| self.since.contains_key(&k))
    }

    /// `node`'s local clock time at presentation time `now`.
    pub(crate) fn time(&self, doc: Tree<'_>, owners: &OwnerIndex, node: NodeId, now: f64) -> f64 {
        if self.since.is_empty() && self.behind.is_empty() {
            return now;
        }
        let Some(k) = owners.owner_of(doc, node).clock_key() else {
            return now;
        };
        self.since.get(&k).copied().unwrap_or(now) - self.behind.get(&k).copied().unwrap_or(0.0)
    }
}

/// What one top-level root holds that runs on time or decides who owns what
/// (`EN-8c.0`). A per-root maker makes it from scratch through a recording
/// `Tree` (`root_runtime`), and `OwnerIndex` is the sum of every root's: no
/// op is classified to keep it current, and a re-made root's old summary is
/// subtracted exactly as it was added.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RootRuntime {
    /// The root, if it is a live composition, and its run mode.
    pub(crate) compositions: BTreeMap<NodeId, RunMode>,
    pub(crate) traces: BTreeMap<NodeId, Owner>,
    pub(crate) accs: BTreeMap<NodeId, Owner>,
    pub(crate) queries: BTreeSet<NodeId>,
    pub(crate) entities: BTreeMap<NodeId, Owner>,
    /// Entities under the root that live `Figures` picture, and the
    /// compositions picturing each.
    pub(crate) pictured: BTreeMap<NodeId, BTreeSet<NodeId>>,
    /// `Animates` from live clips under the root: clip, target, and the
    /// target's owner.
    pub(crate) animates: BTreeMap<RelationId, (NodeId, NodeId, Owner)>,
}

impl RootRuntime {
    pub(crate) fn extend(&mut self, o: &RootRuntime) {
        self.compositions.extend(o.compositions.iter().map(|(k, v)| (*k, *v)));
        self.traces.extend(o.traces.iter().map(|(k, v)| (*k, *v)));
        self.accs.extend(o.accs.iter().map(|(k, v)| (*k, *v)));
        self.queries.extend(o.queries.iter().copied());
        self.entities.extend(o.entities.iter().map(|(k, v)| (*k, *v)));
        self.pictured.extend(o.pictured.iter().map(|(k, v)| (*k, v.clone())));
        self.animates.extend(o.animates.iter().map(|(k, v)| (*k, *v)));
    }
}

/// Whether live `Figures` from a node in a composition picture `n`.
fn pictured_in(doc: Tree<'_>, n: NodeId) -> BTreeSet<NodeId> {
    doc.relations_to(n)
        .filter(|r| r.rel == RelKind::Figures && r.to == n && doc.is_live(r.from))
        .filter_map(|r| composition::composition_of(doc, r.from))
        .collect()
}

/// `n`'s owner, read through `doc` so a recording tree notes what decides it:
/// its composition root, else its nearest pictured ancestor, else `Free`.
fn owner_through(doc: Tree<'_>, n: NodeId) -> Owner {
    let comp_ty = TypeId::lookup("composition").expect("registered");
    let mut chain: Vec<NodeId> = Vec::new();
    let mut top = None;
    let mut cur = Some(n);
    while let Some(x) = cur.and_then(|c| doc.live(c)) {
        chain.push(x.id);
        top = Some(x.ty);
        cur = x.parent;
    }
    if top == Some(comp_ty) {
        return Owner::Composition(*chain.last().expect("a live node"));
    }
    for &a in &chain {
        if doc.live(a).is_some_and(|x| x.ty == comp_ty) {
            return Owner::Composition(a);
        }
        if !pictured_in(doc, a).is_empty() {
            return Owner::Entity(a);
        }
    }
    Owner::Free
}

/// `root`'s summary, read only through `doc` (a recording `Tree` in the
/// maker, a plain one in the check).
pub(crate) fn root_runtime(doc: Tree<'_>, root: NodeId) -> RootRuntime {
    let ty = |n: &str| TypeId::lookup(n).expect("registered");
    let (comp_ty, entity_ty, clip_ty) = (ty("composition"), ty("entity"), ty("clip"));
    let mut out = RootRuntime::default();
    let Some(r) = doc.live(root) else { return out };
    if r.parent.is_some() {
        return out;
    }
    let comp = r.ty == comp_ty;
    if comp {
        out.compositions.insert(root, composition::run_mode(doc, root));
    }
    let owner = |n: NodeId| if comp { Owner::Composition(root) } else { owner_through(doc, n) };
    for n in doc.nodes_with_in(root, FacetKind::Trace).into_iter().filter(|&n| doc.is_live(n)) {
        out.traces.insert(n, owner(n));
    }
    for n in doc.nodes_with_in(root, FacetKind::Accumulate).into_iter().filter(|&n| doc.is_live(n)) {
        out.accs.insert(n, owner(n));
    }
    out.queries.extend(doc.nodes_with_in(root, FacetKind::Query).into_iter().filter(|&n| doc.is_live(n)));
    for e in doc.nodes_of_type_in(root, entity_ty).into_iter().filter(|&n| doc.is_live(n)) {
        out.entities.insert(e, owner(e));
        let by = pictured_in(doc, e);
        if !by.is_empty() {
            out.pictured.insert(e, by);
        }
    }
    for c in doc.nodes_of_type_in(root, clip_ty).into_iter().filter(|&n| doc.is_live(n)) {
        let rels: Vec<(RelationId, NodeId)> = doc.relations_from(c).filter(|r| r.rel == RelKind::Animates && r.from == c).map(|r| (r.id, r.to)).collect();
        for (rid, t) in rels {
            if doc.is_live(t) {
                out.animates.insert(rid, (c, t, owner_through(doc, t)));
            }
        }
    }
    out
}

/// What a change of the owner index asks of the runtime systems.
#[derive(Default)]
pub(crate) struct RuntimeDiff {
    /// Traces, accumulators and entities that appeared or changed owner
    /// (`Some`), or are gone (`None`).
    pub(crate) traces: Vec<(NodeId, Option<Owner>)>,
    pub(crate) accs: Vec<(NodeId, Option<Owner>)>,
    pub(crate) entities: Vec<(NodeId, Option<Owner>)>,
    pub(crate) queries_added: Vec<NodeId>,
    pub(crate) queries_gone: Vec<NodeId>,
    /// Clips whose targets changed.
    pub(crate) clips: BTreeSet<NodeId>,
    /// What decides the active set changed: compositions, run modes,
    /// compositions holding accumulators, `Figures`.
    pub(crate) gate: bool,
    pub(crate) new_owners: Vec<Owner>,
    pub(crate) gone_owners: Vec<Owner>,
    /// Entities that became or stopped being pictured: their subtrees
    /// changed owner.
    pub(crate) reowned: Vec<NodeId>,
}

/// The owner and runtime-node index (`EN-8b`), kept as the sum of every
/// root's `RootRuntime` (`EN-8c.0`), plus the cross-owner reactive read
/// graph, which follows the cells whose reads changed.
#[derive(Clone, Debug, Default)]
pub(crate) struct OwnerIndex {
    /// Every live composition root.
    pub(crate) compositions: BTreeSet<NodeId>,
    /// Live compositions with `composition.run == always`.
    pub(crate) run_always: BTreeSet<NodeId>,
    /// Live compositions containing at least one live `Accumulate` node.
    pub(crate) comp_accumulators: BTreeMap<NodeId, BTreeSet<NodeId>>,

    /// `entity -> { compositions picturing it via live RelKind::Figures }`.
    pub(crate) pictured_by: BTreeMap<NodeId, BTreeSet<NodeId>>,
    /// `composition -> { entities it pictures via live RelKind::Figures }`.
    pub(crate) figures_of_comp: BTreeMap<NodeId, BTreeSet<NodeId>>,

    /// Per-owner live `Trace` nodes and reverse owner lookup.
    pub(crate) traces_by_owner: BTreeMap<Owner, BTreeSet<NodeId>>,
    pub(crate) trace_owner: HashMap<NodeId, Owner>,
    /// Per-owner live `Accumulate` nodes and reverse owner lookup.
    pub(crate) accs_by_owner: BTreeMap<Owner, BTreeSet<NodeId>>,
    pub(crate) acc_owner: HashMap<NodeId, Owner>,
    /// Every live `Query` node.
    pub(crate) live_queries: BTreeSet<NodeId>,
    /// Per-owner live `entity` nodes and reverse owner lookup (for machines).
    pub(crate) entities_by_owner: BTreeMap<Owner, BTreeSet<NodeId>>,
    pub(crate) entity_owner: HashMap<NodeId, Owner>,

    /// Per-owner clips (`owner -> { clip -> target_edge_count }`), reverse
    /// `clip -> { owner -> target_edge_count }`, and `target -> { Animates }`.
    pub(crate) clips_by_owner: BTreeMap<Owner, BTreeMap<NodeId, usize>>,
    pub(crate) clip_targets: BTreeMap<NodeId, BTreeMap<Owner, usize>>,
    pub(crate) animates_on_target: HashMap<NodeId, BTreeSet<RelationId>>,

    /// Cross-owner reactive edges per cell: only cells with >= 1 cross-owner
    /// read are stored here (`(reader_owner, src_owner)` pairs with `reader != src`).
    cell_cross_edges: HashMap<CellId, Vec<(Owner, Owner)>>,
    /// Aggregated refcounted cross-owner read graph:
    /// `reader_owner -> { src_owner -> count }`. If `reader_owner` is active,
    /// every `src_owner` it reads must also be active.
    read_by: BTreeMap<Owner, BTreeMap<Owner, usize>>,
}

fn move_member(by_owner: &mut BTreeMap<Owner, BTreeSet<NodeId>>, of: &mut HashMap<NodeId, Owner>, n: NodeId, to: Option<Owner>) {
    if let Some(old) = of.remove(&n)
        && let Some(set) = by_owner.get_mut(&old)
    {
        set.remove(&n);
        if set.is_empty() {
            by_owner.remove(&old);
        }
    }
    if let Some(o) = to {
        of.insert(n, o);
        by_owner.entry(o).or_default().insert(n);
    }
}

fn keys<V>(a: &BTreeMap<NodeId, V>, b: &BTreeMap<NodeId, V>) -> BTreeSet<NodeId> {
    a.keys().chain(b.keys()).copied().collect()
}

fn count(map: &mut BTreeMap<NodeId, usize>, k: NodeId, up: bool) {
    let c = map.entry(k).or_insert(0);
    if up {
        *c += 1;
    } else {
        *c -= 1;
        if *c == 0 {
            map.remove(&k);
        }
    }
}

impl OwnerIndex {
    /// Replace `old` (what the re-made roots contributed) with `new` (what
    /// they contribute now), and say what the runtime systems must do.
    pub(crate) fn apply(&mut self, old: &RootRuntime, new: &RootRuntime) -> RuntimeDiff {
        let mut d = RuntimeDiff::default();
        for c in keys(&old.compositions, &new.compositions) {
            let (was, now) = (old.compositions.get(&c), new.compositions.get(&c));
            if was == now {
                continue;
            }
            d.gate = true;
            match now {
                Some(m) => {
                    if self.compositions.insert(c) {
                        d.new_owners.push(Owner::Composition(c));
                    }
                    if *m == RunMode::Always {
                        self.run_always.insert(c);
                    } else {
                        self.run_always.remove(&c);
                    }
                }
                None => {
                    self.compositions.remove(&c);
                    self.run_always.remove(&c);
                    d.gone_owners.push(Owner::Composition(c));
                }
            }
        }
        for n in keys(&old.traces, &new.traces) {
            let now = new.traces.get(&n).copied();
            if old.traces.get(&n).copied() != now {
                move_member(&mut self.traces_by_owner, &mut self.trace_owner, n, now);
                d.traces.push((n, now));
            }
        }
        for n in keys(&old.accs, &new.accs) {
            let (was, now) = (old.accs.get(&n).copied(), new.accs.get(&n).copied());
            if was == now {
                continue;
            }
            move_member(&mut self.accs_by_owner, &mut self.acc_owner, n, now);
            if let Some(Owner::Composition(c)) = was
                && let Some(set) = self.comp_accumulators.get_mut(&c)
            {
                set.remove(&n);
                if set.is_empty() {
                    self.comp_accumulators.remove(&c);
                }
            }
            if let Some(Owner::Composition(c)) = now {
                self.comp_accumulators.entry(c).or_default().insert(n);
            }
            d.gate = true;
            d.accs.push((n, now));
        }
        for n in keys(&old.entities, &new.entities) {
            let now = new.entities.get(&n).copied();
            if old.entities.get(&n).copied() != now {
                move_member(&mut self.entities_by_owner, &mut self.entity_owner, n, now);
                d.entities.push((n, now));
            }
        }
        for &q in old.queries.difference(&new.queries) {
            self.live_queries.remove(&q);
            d.queries_gone.push(q);
        }
        for &q in new.queries.difference(&old.queries) {
            self.live_queries.insert(q);
            d.queries_added.push(q);
        }
        let empty = BTreeSet::new();
        for e in keys(&old.pictured, &new.pictured) {
            let (was, now) = (old.pictured.get(&e).unwrap_or(&empty), new.pictured.get(&e).unwrap_or(&empty));
            if was == now {
                continue;
            }
            d.gate = true;
            for c in was.difference(now) {
                if let Some(es) = self.figures_of_comp.get_mut(c) {
                    es.remove(&e);
                    if es.is_empty() {
                        self.figures_of_comp.remove(c);
                    }
                }
            }
            for &c in now.difference(was) {
                self.figures_of_comp.entry(c).or_default().insert(e);
            }
            if now.is_empty() {
                self.pictured_by.remove(&e);
                d.gone_owners.push(Owner::Entity(e));
                d.reowned.push(e);
            } else {
                self.pictured_by.insert(e, now.clone());
                if was.is_empty() {
                    d.new_owners.push(Owner::Entity(e));
                    d.reowned.push(e);
                }
            }
        }
        let rels: BTreeSet<RelationId> = old.animates.keys().chain(new.animates.keys()).copied().collect();
        for rid in rels {
            let (was, now) = (old.animates.get(&rid), new.animates.get(&rid));
            if was == now {
                continue;
            }
            for (&(clip, target, owner), up) in was.map(|w| (w, false)).into_iter().chain(now.map(|n| (n, true))) {
                count(self.clips_by_owner.entry(owner).or_default(), clip, up);
                if self.clips_by_owner.get(&owner).is_some_and(BTreeMap::is_empty) {
                    self.clips_by_owner.remove(&owner);
                }
                let by_owner = self.clip_targets.entry(clip).or_default();
                let c = by_owner.entry(owner).or_insert(0);
                if up {
                    *c += 1;
                } else {
                    *c -= 1;
                    if *c == 0 {
                        by_owner.remove(&owner);
                    }
                }
                if by_owner.is_empty() {
                    self.clip_targets.remove(&clip);
                }
                let on = self.animates_on_target.entry(target).or_default();
                if up {
                    on.insert(rid);
                } else {
                    on.remove(&rid);
                }
                if on.is_empty() {
                    self.animates_on_target.remove(&target);
                }
                d.clips.insert(clip);
            }
        }
        d
    }

    /// Whether the runtime part (everything but the cross-owner reads) is
    /// the same as `other`'s: `IRON_OWNER_CHECK` against a fresh build.
    #[cfg(feature = "differential")]
    pub(crate) fn same_runtime(&self, other: &OwnerIndex) -> bool {
        self.compositions == other.compositions
            && self.run_always == other.run_always
            && self.comp_accumulators == other.comp_accumulators
            && self.pictured_by == other.pictured_by
            && self.figures_of_comp == other.figures_of_comp
            && self.traces_by_owner == other.traces_by_owner
            && self.trace_owner == other.trace_owner
            && self.accs_by_owner == other.accs_by_owner
            && self.acc_owner == other.acc_owner
            && self.live_queries == other.live_queries
            && self.entities_by_owner == other.entities_by_owner
            && self.entity_owner == other.entity_owner
            && self.clips_by_owner == other.clips_by_owner
            && self.clip_targets == other.clip_targets
            && self.animates_on_target == other.animates_on_target
    }

    /// O(1) ownership lookup for a live node (via `root_of` for compositions,
    /// and a short parent walk outside compositions for pictured entities).
    pub(crate) fn owner_of(&self, doc: Tree<'_>, node: NodeId) -> Owner {
        if let Some(root) = doc.root_of(node)
            && self.compositions.contains(&root)
        {
            return Owner::Composition(root);
        }
        if self.pictured_by.is_empty() && self.compositions.is_empty() {
            return Owner::Free;
        }
        let comp_ty = TypeId::lookup("composition").expect("registered");
        let mut cur = Some(node);
        while let Some(n) = cur.and_then(|c| doc.live(c)) {
            if n.ty == comp_ty {
                return Owner::Composition(n.id);
            }
            if self.pictured_by.contains_key(&n.id) {
                return Owner::Entity(n.id);
            }
            cur = n.parent;
        }
        Owner::Free
    }

    /// Every currently-pausable clock key (live compositions + pictured
    /// entities). Used only on the `observe_all: true -> false` mode switch.
    pub(crate) fn all_clock_keys(&self) -> Vec<NodeId> {
        self.compositions.iter().chain(self.pictured_by.keys()).copied().collect()
    }

    /// Whether clip `c` animates at least one target whose owner is active.
    pub(crate) fn clip_is_active(&self, clip: NodeId, active: &BTreeSet<Owner>) -> bool {
        self.clip_targets.get(&clip).is_some_and(|owners| owners.keys().any(|o| active.contains(o)))
    }

    /// Rebuild `cell_cross_edges` and `read_by` from scratch (only on `load` / `Reactive::rebuild`).
    pub(crate) fn rebuild_cross_reads(&mut self, doc: Tree<'_>, reactive: &Reactive) {
        self.cell_cross_edges.clear();
        self.read_by.clear();
        let all_cells = reactive.all_cell_ids();
        for c in all_cells {
            self.reconcile_cell_cross_edges(doc, reactive, c);
        }
    }

    /// Incrementally update `cell_cross_edges` and `read_by` for `dirty_cells`
    /// (or full rebuild if `rebuilt`). Returns `true` iff any `(reader, src)`
    /// cross-owner reachability edge appeared or disappeared (`0 <-> 1`).
    pub(crate) fn sync_cross_reads(&mut self, doc: Tree<'_>, reactive: &Reactive, dirty_cells: BTreeSet<CellId>, rebuilt: bool) -> bool {
        if rebuilt {
            let before = self.read_by.clone();
            self.rebuild_cross_reads(doc, reactive);
            return before.len() != self.read_by.len()
                || before.iter().any(|(r, srcs)| self.read_by.get(r).map(|m| m.keys().collect::<Vec<_>>()) != Some(srcs.keys().collect::<Vec<_>>()));
        }
        let mut changed = false;
        for c in dirty_cells {
            changed |= self.reconcile_cell_cross_edges(doc, reactive, c);
        }
        changed
    }

    fn reconcile_cell_cross_edges(&mut self, doc: Tree<'_>, reactive: &Reactive, cell: CellId) -> bool {
        let mut topology_changed = false;
        if let Some(old) = self.cell_cross_edges.remove(&cell) {
            for (reader, src) in old {
                if let Some(srcs) = self.read_by.get_mut(&reader) {
                    if let Some(cnt) = srcs.get_mut(&src) {
                        *cnt -= 1;
                        if *cnt == 0 {
                            srcs.remove(&src);
                            topology_changed = true;
                        }
                    }
                    if srcs.is_empty() {
                        self.read_by.remove(&reader);
                    }
                }
            }
        }
        if !reactive.cell_exists(cell) {
            return topology_changed;
        }
        let (reader_nodes, always_free) = reactive.cell_reader_nodes(cell);
        let mut reader_owners: Vec<Owner> = Vec::with_capacity(reader_nodes.len() + usize::from(always_free));
        if always_free {
            reader_owners.push(Owner::Free);
        }
        for n in reader_nodes {
            let o = self.owner_of(doc, n);
            if !reader_owners.contains(&o) {
                reader_owners.push(o);
            }
        }
        let reads = reactive.cell_read_leaves(cell);
        let mut new_edges: Vec<(Owner, Owner)> = Vec::new();
        for l in reads {
            let src = self.owner_of(doc, l.node);
            if src == Owner::Free {
                // `Free` is always active, so cross-edges into `Free` never activate anything new.
                continue;
            }
            for &reader in &reader_owners {
                if reader != src {
                    new_edges.push((reader, src));
                }
            }
        }
        if !new_edges.is_empty() {
            new_edges.sort_unstable();
            new_edges.dedup();
            for &(reader, src) in &new_edges {
                let cnt = self.read_by.entry(reader).or_default().entry(src).or_insert(0);
                *cnt += 1;
                if *cnt == 1 {
                    topology_changed = true;
                }
            }
            self.cell_cross_edges.insert(cell, new_edges);
        }
        topology_changed
    }

    /// Compute the active owner set in O(|active| + |cross-owner edges out of active|),
    /// never scanning hidden compositions. Without `demand`, the owners whose clocks
    /// run. With it, the owners whose cells are evaluated (`EN-8c`): every shown
    /// composition counts, `on_demand` or not (what is shown is evaluated even while
    /// its clock holds), and `held` owners seed it too.
    pub(crate) fn compute_active(&self, doc: Tree<'_>, observed: &[NodeId], held: &[Owner], demand: bool) -> BTreeSet<Owner> {
        let mut active: BTreeSet<Owner> = BTreeSet::new();
        let mut queue: VecDeque<Owner> = VecDeque::new();

        // `Owner::Free` always runs (timeline, loose roots, unpictured entities, rooted islands).
        active.insert(Owner::Free);
        queue.push_back(Owner::Free);

        // Shown compositions, excluding `on_demand` compositions holding an accumulator.
        let shown = composition::shown(doc, observed);
        let shown_holding: BTreeSet<NodeId> = if demand { BTreeSet::new() } else { shown.iter().copied().filter(|c| self.comp_accumulators.contains_key(c)).collect() };
        let open_running = if shown_holding.is_empty() {
            BTreeSet::new()
        } else {
            composition::reach(doc, observed, &shown_holding).running
        };
        for c in shown {
            if !shown_holding.contains(&c) || open_running.contains(&c) {
                let o = Owner::Composition(c);
                if active.insert(o) {
                    queue.push_back(o);
                }
            }
        }

        // `run = always` compositions keep running while hidden (PT-1.3),
        // and what runs is evaluated: their steppers, items and machines
        // read their own cells.
        for &c in &self.run_always {
            let o = Owner::Composition(c);
            if active.insert(o) {
                queue.push_back(o);
            }
        }
        for &o in held {
            if active.insert(o) {
                queue.push_back(o);
            }
        }

        // Propagate forward from active readers to the owners they read (and from
        // active compositions to the entities they picture).
        while let Some(owner) = queue.pop_front() {
            if let Owner::Composition(c) = owner
                && let Some(entities) = self.figures_of_comp.get(&c)
            {
                for &e in entities {
                    let eo = Owner::Entity(e);
                    if active.insert(eo) {
                        queue.push_back(eo);
                    }
                }
            }
            if let Some(srcs) = self.read_by.get(&owner) {
                for &src in srcs.keys() {
                    if active.insert(src) {
                        queue.push_back(src);
                    }
                }
            }
        }

        active
    }
}

/// Read-only helper used by `Store` to list leaves of a node to watch/unwatch.
pub(crate) fn trace_input_leaves(n: NodeId) -> [Leaf; 2] {
    crate::step::Steps::input_leaves(n)
}
