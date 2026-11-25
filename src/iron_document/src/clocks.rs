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

/// Incremental owner & runtime-node index + cross-owner reactive read graph
/// (`EN-8b`, replacing `Store::paused`, `Clocks::gated`, `Reactive::reader_nodes`,
/// `Docs::reshaped`, `Reactive::wired`, and `op::reshapes`).
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
    /// Per-relation record for live `RelKind::Figures` from a composition.
    pub(crate) figure_rels: BTreeMap<RelationId, (NodeId, NodeId)>,
    /// Multiplicity of live `Figures` relations for each `(comp, entity)` pair.
    figure_edge_counts: BTreeMap<(NodeId, NodeId), usize>,

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

    /// Per-owner clips (`owner -> { clip -> target_edge_count }`),
    /// reverse `clip -> { owner -> target_edge_count }`,
    /// per-relation `Animates` record `(clip, target, target_owner)`,
    /// and `target_node -> { Animates RelationId }`.
    pub(crate) clips_by_owner: BTreeMap<Owner, BTreeMap<NodeId, usize>>,
    pub(crate) clip_targets: BTreeMap<NodeId, BTreeMap<Owner, usize>>,
    pub(crate) clip_rels: BTreeMap<RelationId, (NodeId, NodeId, Owner)>,
    pub(crate) animates_on_target: HashMap<NodeId, BTreeSet<RelationId>>,

    /// Cross-owner reactive edges per cell: only cells with >= 1 cross-owner
    /// read are stored here (`(reader_owner, src_owner)` pairs with `reader != src`).
    cell_cross_edges: HashMap<CellId, Vec<(Owner, Owner)>>,
    /// Aggregated refcounted cross-owner read graph:
    /// `reader_owner -> { src_owner -> count }`. If `reader_owner` is active,
    /// every `src_owner` it reads must also be active.
    read_by: BTreeMap<Owner, BTreeMap<Owner, usize>>,
}

impl OwnerIndex {
    /// One-time full build on `StoreCore::new` / `StoreCore::load`.
    pub(crate) fn rebuild(&mut self, doc: Tree<'_>, reactive: &Reactive) {
        *self = Self::default();
        let comp_ty = TypeId::lookup("composition").expect("registered");
        let entity_ty = TypeId::lookup("entity").expect("registered");
        let clip_ty = TypeId::lookup("clip").expect("registered");

        for c in composition::compositions(doc) {
            self.compositions.insert(c);
            if composition::run_mode(doc, c) == RunMode::Always {
                self.run_always.insert(c);
            }
        }
        // Index live `Figures` relations first so `owner_of` knows all pictured entities.
        let fig_rels: Vec<(RelationId, NodeId, NodeId)> = doc
            .relations()
            .filter(|r| r.rel == RelKind::Figures && doc.is_live(r.from) && doc.is_live(r.to))
            .filter_map(|r| Some((r.id, composition::composition_of(doc, r.from)?, r.to)))
            .collect();
        for (rid, comp, entity) in fig_rels {
            self.add_figure_rel(rid, comp, entity);
        }

        let traces: Vec<NodeId> = doc.nodes_with(FacetKind::Trace).filter(|&n| doc.is_live(n)).collect();
        for n in traces {
            let o = self.owner_of(doc, n);
            self.trace_owner.insert(n, o);
            self.traces_by_owner.entry(o).or_default().insert(n);
        }
        let accs: Vec<NodeId> = doc.nodes_with(FacetKind::Accumulate).filter(|&n| doc.is_live(n)).collect();
        for n in accs {
            let o = self.owner_of(doc, n);
            self.acc_owner.insert(n, o);
            self.accs_by_owner.entry(o).or_default().insert(n);
            if let Owner::Composition(c) = o {
                self.comp_accumulators.entry(c).or_default().insert(n);
            }
        }
        self.live_queries.extend(doc.nodes_with(FacetKind::Query).filter(|&n| doc.is_live(n)));
        let entities: Vec<NodeId> = doc.nodes_of_type(entity_ty).filter(|&n| doc.is_live(n)).collect();
        for e in entities {
            let o = self.owner_of(doc, e);
            self.entity_owner.insert(e, o);
            self.entities_by_owner.entry(o).or_default().insert(e);
        }
        let anim_rels: Vec<(RelationId, NodeId, NodeId)> = doc
            .relations()
            .filter(|r| r.rel == RelKind::Animates && doc.is_live(r.from) && doc.is_live(r.to))
            .filter(|r| doc.ty_of(r.from) == Some(clip_ty))
            .map(|r| (r.id, r.from, r.to))
            .collect();
        for (rid, clip, target) in anim_rels {
            let o = self.owner_of(doc, target);
            self.add_animates_rel(rid, clip, target, o);
        }
        let _ = comp_ty;
        self.rebuild_cross_reads(doc, reactive);
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

    /// Add one live `Figures` relation `(comp -> entity)`. Returns `true` if
    /// `entity` just became pictured (owner changed `Free -> Entity(entity)`).
    pub(crate) fn add_figure_rel(&mut self, rid: RelationId, comp: NodeId, entity: NodeId) -> bool {
        if self.figure_rels.get(&rid) == Some(&(comp, entity)) {
            return false;
        }
        let _ = self.remove_figure_rel(rid);
        self.figure_rels.insert(rid, (comp, entity));
        let cnt = self.figure_edge_counts.entry((comp, entity)).or_insert(0);
        *cnt += 1;
        if *cnt == 1 {
            let became_pictured = !self.pictured_by.contains_key(&entity);
            self.pictured_by.entry(entity).or_default().insert(comp);
            self.figures_of_comp.entry(comp).or_default().insert(entity);
            return became_pictured;
        }
        false
    }

    /// Remove a tracked `Figures` relation `rid`. Returns `(changed, unpictured_entity)`
    /// where `unpictured_entity` is `Some(e)` if `e` has no remaining compositions picturing it.
    pub(crate) fn remove_figure_rel(&mut self, rid: RelationId) -> (bool, Option<NodeId>) {
        let Some((comp, entity)) = self.figure_rels.remove(&rid) else {
            return (false, None);
        };
        let mut unpictured = None;
        if let Some(cnt) = self.figure_edge_counts.get_mut(&(comp, entity)) {
            *cnt -= 1;
            if *cnt == 0 {
                self.figure_edge_counts.remove(&(comp, entity));
                if let Some(cs) = self.pictured_by.get_mut(&entity) {
                    cs.remove(&comp);
                    if cs.is_empty() {
                        self.pictured_by.remove(&entity);
                        unpictured = Some(entity);
                    }
                }
                if let Some(es) = self.figures_of_comp.get_mut(&comp) {
                    es.remove(&entity);
                    if es.is_empty() {
                        self.figures_of_comp.remove(&comp);
                    }
                }
            }
        }
        (true, unpictured)
    }

    pub(crate) fn add_animates_rel(&mut self, rid: RelationId, clip: NodeId, target: NodeId, owner: Owner) {
        if self.clip_rels.get(&rid) == Some(&(clip, target, owner)) {
            return;
        }
        let _ = self.remove_animates_rel(rid);
        self.clip_rels.insert(rid, (clip, target, owner));
        self.animates_on_target.entry(target).or_default().insert(rid);
        *self.clips_by_owner.entry(owner).or_default().entry(clip).or_insert(0) += 1;
        *self.clip_targets.entry(clip).or_default().entry(owner).or_insert(0) += 1;
    }

    pub(crate) fn remove_animates_rel(&mut self, rid: RelationId) -> Option<NodeId> {
        let (clip, target, owner) = self.clip_rels.remove(&rid)?;
        if let Some(set) = self.animates_on_target.get_mut(&target) {
            set.remove(&rid);
            if set.is_empty() {
                self.animates_on_target.remove(&target);
            }
        }
        if let Some(map) = self.clips_by_owner.get_mut(&owner) {
            if let Some(cnt) = map.get_mut(&clip) {
                *cnt -= 1;
                if *cnt == 0 {
                    map.remove(&clip);
                }
            }
            if map.is_empty() {
                self.clips_by_owner.remove(&owner);
            }
        }
        if let Some(map) = self.clip_targets.get_mut(&clip) {
            if let Some(cnt) = map.get_mut(&owner) {
                *cnt -= 1;
                if *cnt == 0 {
                    map.remove(&owner);
                }
            }
            if map.is_empty() {
                self.clip_targets.remove(&clip);
            }
        }
        Some(clip)
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

    /// Compute the active owner set in O(|active| + |cross-owner edges out of active|).
    /// Matches `Store::paused()`'s exact semantics without scanning hidden compositions.
    pub(crate) fn compute_active(&self, doc: Tree<'_>, observed: &[NodeId]) -> BTreeSet<Owner> {
        let mut active: BTreeSet<Owner> = BTreeSet::new();
        let mut queue: VecDeque<Owner> = VecDeque::new();

        // `Owner::Free` always runs (timeline, loose roots, unpictured entities, rooted islands).
        active.insert(Owner::Free);
        queue.push_back(Owner::Free);

        // Shown compositions, excluding `on_demand` compositions holding an accumulator.
        let shown = composition::shown(doc, observed);
        let shown_holding: BTreeSet<NodeId> = shown.iter().copied().filter(|c| self.comp_accumulators.contains_key(c)).collect();
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

        // `run = always` compositions keep running while hidden (PT-1.3).
        for &c in &self.run_always {
            let o = Owner::Composition(c);
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
