//! The read interface (reactive-engine.md §12.1; handoff §4.2, §4.6): what
//! answers for authored and derived nodes alike. Behind it are two
//! documents (`Docs`): the authored one, and, once copies are kept apart
//! (phase 2c), the one holding derived copies. A node is answered by the
//! document its id belongs to; enumerations chain the two, which keeps their
//! order since every derived id sorts after every other; children merge
//! where an owner has both. Get one from `Store::tree()`, inside the crate
//! from `Tree::of(&docs)`, or over a document known to hold no copies with
//! `Tree::authored_only`.
//!
//! The authored document's version and id counters are answered here too,
//! for code that reads the tree and builds a transaction from it: ids are
//! minted by the authored document in every phase. Its saved form and its
//! mutators are not here.

use crate::document::{Document, Node, Relation};
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, LeafPath, LeafStruct};
use crate::id::{NodeId, RelationId};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::Slot;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Per-top-level-root type and facet indexes across both authored nodes and
/// derived copies (engine.md §3.2). Maintained incrementally by `Docs`'s
/// mutators and journaled alongside the document journal.
#[derive(Clone, Default)]
pub(crate) struct ScopedIndexes {
    root_of: BTreeMap<NodeId, NodeId>,
    by_type: BTreeMap<(NodeId, TypeId), BTreeSet<NodeId>>,
    by_facet: BTreeMap<(NodeId, FacetKind), BTreeSet<NodeId>>,
}

#[derive(Clone)]
enum ScopedRec {
    Root(NodeId, Option<NodeId>),
    ByType(NodeId, TypeId, NodeId, bool),
    ByFacet(NodeId, FacetKind, NodeId, bool),
}

impl ScopedIndexes {
    fn build(authored: &Document) -> ScopedIndexes {
        let mut out = ScopedIndexes::default();
        let mut stack: Vec<(NodeId, NodeId)> = authored.roots().iter().map(|&r| (r, r)).collect();
        while let Some((id, root)) = stack.pop() {
            let Some(n) = authored.live(id) else { continue };
            out.root_of.insert(id, root);
            out.by_type.entry((root, n.ty)).or_default().insert(id);
            for f in authored.facets_of(id) {
                out.by_facet.entry((root, f.kind())).or_default().insert(id);
            }
            for &c in authored.children(id) {
                stack.push((c, root));
            }
        }
        out
    }

    fn set_root(&mut self, id: NodeId, root: Option<NodeId>, journal: &mut Option<Vec<ScopedRec>>) {
        let prev = match root {
            Some(r) => self.root_of.insert(id, r),
            None => self.root_of.remove(&id),
        };
        if prev != root
            && let Some(j) = journal
        {
            j.push(ScopedRec::Root(id, prev));
        }
    }

    fn insert_type(&mut self, root: NodeId, ty: TypeId, id: NodeId, journal: &mut Option<Vec<ScopedRec>>, touched: &mut crate::structure::Touched) {
        if self.by_type.entry((root, ty)).or_default().insert(id) {
            if let Some(j) = journal {
                j.push(ScopedRec::ByType(root, ty, id, true));
            }
            touched.note(crate::structure::Key::In(root, ty));
        }
    }

    fn remove_type(&mut self, root: NodeId, ty: TypeId, id: NodeId, journal: &mut Option<Vec<ScopedRec>>, touched: &mut crate::structure::Touched) {
        if let Some(s) = self.by_type.get_mut(&(root, ty))
            && s.remove(&id)
        {
            if s.is_empty() {
                self.by_type.remove(&(root, ty));
            }
            if let Some(j) = journal {
                j.push(ScopedRec::ByType(root, ty, id, false));
            }
            touched.note(crate::structure::Key::In(root, ty));
        }
    }

    fn insert_facet(&mut self, root: NodeId, kind: FacetKind, id: NodeId, journal: &mut Option<Vec<ScopedRec>>, touched: &mut crate::structure::Touched) {
        if self.by_facet.entry((root, kind)).or_default().insert(id) {
            if let Some(j) = journal {
                j.push(ScopedRec::ByFacet(root, kind, id, true));
            }
            touched.note(crate::structure::Key::With(root, kind));
        }
    }

    fn remove_facet(&mut self, root: NodeId, kind: FacetKind, id: NodeId, journal: &mut Option<Vec<ScopedRec>>, touched: &mut crate::structure::Touched) {
        if let Some(s) = self.by_facet.get_mut(&(root, kind))
            && s.remove(&id)
        {
            if s.is_empty() {
                self.by_facet.remove(&(root, kind));
            }
            if let Some(j) = journal {
                j.push(ScopedRec::ByFacet(root, kind, id, false));
            }
            touched.note(crate::structure::Key::With(root, kind));
        }
    }

    fn undo(&mut self, recs: Vec<ScopedRec>) {
        for r in recs.into_iter().rev() {
            match r {
                ScopedRec::Root(id, prev) => match prev {
                    Some(root) => {
                        self.root_of.insert(id, root);
                    }
                    None => {
                        self.root_of.remove(&id);
                    }
                },
                ScopedRec::ByType(root, ty, id, inserted) => {
                    if inserted {
                        if let Some(s) = self.by_type.get_mut(&(root, ty)) {
                            s.remove(&id);
                            if s.is_empty() {
                                self.by_type.remove(&(root, ty));
                            }
                        }
                    } else {
                        self.by_type.entry((root, ty)).or_default().insert(id);
                    }
                }
                ScopedRec::ByFacet(root, kind, id, inserted) => {
                    if inserted {
                        if let Some(s) = self.by_facet.get_mut(&(root, kind)) {
                            s.remove(&id);
                            if s.is_empty() {
                                self.by_facet.remove(&(root, kind));
                            }
                        }
                    } else {
                        self.by_facet.entry((root, kind)).or_default().insert(id);
                    }
                }
            }
        }
    }
}

/// Sparse non-journaled store of effective authored state at the active point
/// (engine.md §2.3, EN-3/EN-4). Empty when no authored patches exist, so
/// unpatched reads are a single branch and zero allocations.
#[derive(Clone, Default)]
pub(crate) struct ResolvedStore {
    /// Merged facets for nodes that have at least one active authored patch
    /// affecting their facets or leaves at the active point.
    pub(crate) facets: HashMap<NodeId, BTreeMap<FacetKind, Facet>>,
    /// Authored nodes whose effective `Present` is false at the active point
    /// (including descendants of absent ancestors, P6).
    pub(crate) absent: HashSet<NodeId>,
    /// Authored nodes whose effective `Hidden` is true at the active point
    /// (including descendants of hidden ancestors, P6).
    pub(crate) hidden: HashSet<NodeId>,
}

impl ResolvedStore {
    pub(crate) fn is_empty(&self) -> bool {
        self.facets.is_empty() && self.absent.is_empty() && self.hidden.is_empty()
    }
}

/// Effective presence & node changes accumulated by `Docs::reconcile_resolved`
/// (engine.md §2.3, EN-4b): drained once by `StoreCore` into `Changes`,
/// `geometry`, and `Reactive::restructure`.
#[derive(Clone, Debug, Default)]
pub(crate) struct ResolvedDelta {
    pub(crate) became_present: Vec<(NodeId, Option<NodeId>)>,
    pub(crate) became_absent: Vec<(NodeId, Option<NodeId>)>,
    pub(crate) changed_nodes: BTreeSet<NodeId>,
}

impl ResolvedDelta {
    pub(crate) fn is_empty(&self) -> bool {
        self.became_present.is_empty() && self.became_absent.is_empty() && self.changed_nodes.is_empty()
    }

    pub(crate) fn has_presence_change(&self) -> bool {
        !self.became_present.is_empty() || !self.became_absent.is_empty()
    }
}

/// The store's documents (handoff §4.6): the authored one, and the one
/// holding derived copies when they are kept apart. With `apart` false every
/// node, copies included, is in `authored` and `copies` stays empty: 2b's
/// layout, the differential reference for phase 2c.
#[derive(Clone)]
pub(crate) struct Docs {
    pub(crate) authored: Document,
    pub(crate) copies: Document,
    pub(crate) apart: bool,
    /// Per-top-level-root type and facet indexes across authored and copies.
    pub(crate) scoped: ScopedIndexes,
    scoped_journal: Option<Vec<ScopedRec>>,
    scoped_touched: crate::structure::Touched,
    /// Offsets into `(authored.touched, copies.touched, scoped_touched)` already
    /// observed by `TemplateCache::note_touched` since the last `take_touched`.
    template_cursor: (usize, usize, usize),
    /// Active point on variation axes (`Point::default()` when no axes are set).
    pub(crate) point: crate::patch::Point,
    /// Session-layer patches (`SESSION_LAYER`, P8; never saved, never journaled).
    pub(crate) session_patches: BTreeMap<crate::patch::PatchId, crate::patch::Patch>,
    /// Sparse non-journaled resolved state at `point` (engine.md §2.3).
    pub(crate) resolved: ResolvedStore,
    /// Effective presence & node changes from `reconcile_resolved` waiting to
    /// be drained by `StoreCore` into `Changes` and `Reactive`.
    pending_delta: ResolvedDelta,
    /// Each node deleted since the changes were last taken, as it was when
    /// it went: a copy leaves no tombstone to read it from afterwards
    /// (`Changes::note`, `Store::regraph`).
    pub(crate) gone: crate::changes::Gone,
    /// While a transaction is open, what each of its deletes replaced in
    /// `gone`, so a rollback takes them back out.
    gone_journal: Option<Vec<(NodeId, Option<crate::changes::Went>)>>,
    /// A stamp taken whenever an op that can change what the gate reads is
    /// applied (`op::reshapes`: structure, facets, relations, a
    /// composition's `run`, an embed's `source`), and on every rollback;
    /// unique across stores (`crate::stamp`). The gate's cache key
    /// (`Store::gate`).
    pub(crate) reshaped: u64,
    /// Likewise for the theme's tokens (`op::retokens`): what the theme's
    /// published colours are cached against (`Store::resolve_theme`).
    pub(crate) tokens_shaped: u64,
}

impl Docs {
    pub(crate) fn new(authored: Document, apart: bool) -> Docs {
        let scoped = ScopedIndexes::build(&authored);
        let mut docs = Docs {
            authored,
            copies: Document::new(),
            apart,
            scoped,
            scoped_journal: None,
            scoped_touched: Default::default(),
            template_cursor: (0, 0, 0),
            point: crate::patch::Point::default(),
            session_patches: BTreeMap::new(),
            resolved: ResolvedStore::default(),
            pending_delta: ResolvedDelta::default(),
            gone: Default::default(),
            gone_journal: None,
            reshaped: crate::stamp(),
            tokens_shaped: crate::stamp(),
        };
        if !docs.authored.patches().patches.is_empty() {
            docs.reconcile_resolved(None);
            docs.pending_delta = ResolvedDelta::default();
        }
        docs
    }

    pub(crate) fn take_resolved_delta(&mut self) -> ResolvedDelta {
        std::mem::take(&mut self.pending_delta)
    }

    /// Walk raw/base live descendants of `id` across authored + copies without
    /// filtering by `resolved.absent` (used by `Op::Delete` and `set_parent`
    /// so authored topology edits never skip patch-absent children, EN-4b).
    pub(crate) fn base_descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = Vec::new();
        let push_raw = |n: NodeId, stack: &mut Vec<NodeId>| {
            if self.apart {
                if n.is_expanded() {
                    stack.extend(self.copies.children(n).iter().rev().copied());
                } else {
                    stack.extend(self.copies.children(n).iter().rev().copied());
                    stack.extend(self.authored.children(n).iter().rev().copied());
                }
            } else {
                stack.extend(self.authored.children(n).iter().rev().copied());
            }
        };
        push_raw(id, &mut stack);
        while let Some(cur) = stack.pop() {
            out.push(cur);
            push_raw(cur, &mut stack);
        }
        out
    }

    fn root_for(&self, id: NodeId, parent: Option<NodeId>) -> NodeId {
        match parent {
            Some(p) => self.scoped.root_of.get(&p).copied().unwrap_or_else(|| {
                let tree = Tree::of(self);
                let mut cur = p;
                while let Some(up) = tree.node(cur).and_then(|n| n.parent) {
                    cur = up;
                }
                cur
            }),
            None => id,
        }
    }

    pub(crate) fn insert_node(&mut self, node: Node, comps: Vec<Facet>) {
        let (id, ty, parent) = (node.id, node.ty, node.parent);
        let root = self.root_for(id, parent);
        let kinds: Vec<FacetKind> = comps.iter().map(Facet::kind).collect();
        self.of_node(id).insert_node(node, comps);
        let on = self.scoped_touched.on;
        self.scoped_touched.on &= !crate::structure::silenced("insert_node");
        self.scoped.set_root(id, Some(root), &mut self.scoped_journal);
        self.scoped.insert_type(root, ty, id, &mut self.scoped_journal, &mut self.scoped_touched);
        for k in kinds {
            self.scoped.insert_facet(root, k, id, &mut self.scoped_journal, &mut self.scoped_touched);
        }
        self.scoped_touched.on = on;
        if self.needs_resolved_sync(id) || parent.is_some_and(|p| self.resolved.absent.contains(&p) || self.resolved.hidden.contains(&p)) {
            self.reconcile_resolved(Some(&BTreeSet::from([id])));
        }
    }

    pub(crate) fn restore_node(&mut self, id: NodeId, ty: TypeId, parent: Option<NodeId>, order: OrderKey, comps: Vec<Facet>) {
        let root = self.root_for(id, parent);
        let kinds: Vec<FacetKind> = comps.iter().map(Facet::kind).collect();
        self.of_node(id).restore_node(id, ty, parent, order, comps);
        let on = self.scoped_touched.on;
        self.scoped_touched.on &= !crate::structure::silenced("restore_node");
        self.scoped.set_root(id, Some(root), &mut self.scoped_journal);
        self.scoped.insert_type(root, ty, id, &mut self.scoped_journal, &mut self.scoped_touched);
        for k in kinds {
            self.scoped.insert_facet(root, k, id, &mut self.scoped_journal, &mut self.scoped_touched);
        }
        self.scoped_touched.on = on;
        if self.needs_resolved_sync(id) || parent.is_some_and(|p| self.resolved.absent.contains(&p) || self.resolved.hidden.contains(&p)) {
            self.reconcile_resolved(Some(&BTreeSet::from([id])));
        }
    }

    /// Delete one live node, its children already gone: a copy kept apart
    /// is removed outright, anything else tombstoned. What it was is kept
    /// in `gone`.
    pub(crate) fn delete_node(&mut self, id: NodeId, at: crate::id::Version) {
        let (went, ty, kinds) = {
            let tree = Tree::of(self);
            let node = tree.node(id).expect("a live node is deleted");
            let went = crate::changes::Went {
                parent: node.parent,
                depth: tree.depth(id),
                ty: node.ty,
            };
            let kinds: Vec<FacetKind> = tree.facets_of(id).map(Facet::kind).collect();
            (went, node.ty, kinds)
        };
        let was = self.gone.insert(id, went);
        if let Some(journal) = &mut self.gone_journal {
            journal.push((id, was));
        }
        let site = if self.apart && id.is_expanded() {
            self.copies.remove_node(id);
            "remove_node"
        } else {
            self.authored.tombstone(id, at);
            "tombstone"
        };
        if let Some(root) = self.scoped.root_of.get(&id).copied() {
            let on = self.scoped_touched.on;
            self.scoped_touched.on &= !crate::structure::silenced(site);
            self.scoped.set_root(id, None, &mut self.scoped_journal);
            self.scoped.remove_type(root, ty, id, &mut self.scoped_journal, &mut self.scoped_touched);
            for k in kinds {
                self.scoped.remove_facet(root, k, id, &mut self.scoped_journal, &mut self.scoped_touched);
            }
            self.scoped_touched.on = on;
        }
        self.resolved.facets.remove(&id);
        self.resolved.absent.remove(&id);
        self.resolved.hidden.remove(&id);
    }

    pub(crate) fn set_parent(&mut self, id: NodeId, parent: Option<NodeId>, order: OrderKey) {
        let old_root = self.scoped.root_of.get(&id).copied();
        let new_root = self.root_for(id, parent);
        self.of_node(id).set_parent(id, parent, order);
        if old_root != Some(new_root) {
            let base_desc = self.base_descendants(id);
            let moved: Vec<(NodeId, TypeId, Vec<FacetKind>)> = {
                let tree = Tree::of(self);
                std::iter::once(id)
                    .chain(base_desc)
                    .filter_map(|n| {
                        let node = tree.live(n)?;
                        let kinds = tree.facets_of(n).map(Facet::kind).collect();
                        Some((n, node.ty, kinds))
                    })
                    .collect()
            };
            let on = self.scoped_touched.on;
            self.scoped_touched.on &= !crate::structure::silenced("set_parent");
            for (n, ty, kinds) in moved {
                if let Some(prev) = self.scoped.root_of.get(&n).copied() {
                    self.scoped.remove_type(prev, ty, n, &mut self.scoped_journal, &mut self.scoped_touched);
                    for &k in &kinds {
                        self.scoped.remove_facet(prev, k, n, &mut self.scoped_journal, &mut self.scoped_touched);
                    }
                }
                self.scoped.set_root(n, Some(new_root), &mut self.scoped_journal);
                self.scoped.insert_type(new_root, ty, n, &mut self.scoped_journal, &mut self.scoped_touched);
                for k in kinds {
                    self.scoped.insert_facet(new_root, k, n, &mut self.scoped_journal, &mut self.scoped_touched);
                }
            }
            self.scoped_touched.on = on;
        }
        if self.needs_resolved_sync(id) || parent.is_some_and(|p| self.resolved.absent.contains(&p) || self.resolved.hidden.contains(&p)) {
            self.reconcile_resolved(Some(&BTreeSet::from([id])));
        }
    }

    pub(crate) fn set_order(&mut self, id: NodeId, order: OrderKey) {
        self.of_node(id).set_order(id, order);
    }

    pub(crate) fn insert_facet(&mut self, id: NodeId, c: Facet) {
        let kind = c.kind();
        let root = self.scoped.root_of.get(&id).copied();
        let patched_here = self.resolved.absent.contains(&id) || self.resolved.facets.contains_key(&id);
        self.of_node(id).insert_facet(id, c);
        if !patched_here && let Some(root) = root {
            let on = self.scoped_touched.on;
            self.scoped_touched.on &= !crate::structure::silenced("insert_facet");
            self.scoped.insert_facet(root, kind, id, &mut self.scoped_journal, &mut self.scoped_touched);
            self.scoped_touched.on = on;
        }
        if self.needs_resolved_sync(id) {
            self.reconcile_resolved(Some(&BTreeSet::from([id])));
        }
    }

    pub(crate) fn remove_facet(&mut self, id: NodeId, kind: FacetKind) -> Option<Facet> {
        let root = self.scoped.root_of.get(&id).copied();
        let patched_here = self.resolved.absent.contains(&id) || self.resolved.facets.contains_key(&id);
        let removed = self.of_node(id).remove_facet(id, kind)?;
        if !patched_here && let Some(root) = root {
            let on = self.scoped_touched.on;
            self.scoped_touched.on &= !crate::structure::silenced("remove_facet");
            self.scoped.remove_facet(root, kind, id, &mut self.scoped_journal, &mut self.scoped_touched);
            self.scoped_touched.on = on;
        }
        if self.needs_resolved_sync(id) {
            self.reconcile_resolved(Some(&BTreeSet::from([id])));
        }
        Some(removed)
    }

    pub(crate) fn set_leaf(&mut self, id: NodeId, path: LeafPath, slot: Slot) -> Option<Result<Slot, crate::facet::LeafError>> {
        let res = self.of_node(id).set_leaf(id, path, slot);
        if res.as_ref().is_some_and(Result::is_ok) && self.needs_resolved_sync(id) {
            self.reconcile_resolved(Some(&BTreeSet::from([id])));
        }
        res
    }

    pub(crate) fn insert_relation(&mut self, rel: Relation) {
        self.of_relation(rel.id).insert_relation(rel);
    }

    pub(crate) fn remove_relation(&mut self, id: RelationId) -> Option<Relation> {
        self.of_relation(id).remove_relation(id)
    }

    /// All live derived descendants currently stored under `owner` in raw
    /// storage (ignoring `ResolvedStore::absent` on `owner`), so an authored
    /// owner made absent by a patch still cleans up its derived copies (`EN-5b`).
    pub(crate) fn raw_derived_descendants(&self, owner: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![owner];
        while let Some(cur) = stack.pop() {
            let children_a = self.authored.children(cur);
            let children_c = if self.apart { self.copies.children(cur) } else { &[][..] };
            for &c in children_a.iter().chain(children_c.iter()) {
                let live = if self.apart && c.is_expanded() {
                    self.copies.is_live(c)
                } else {
                    self.authored.is_live(c)
                };
                if live {
                    if c.is_expanded() && c.path().first() == Some(&owner) {
                        out.push(c);
                    }
                    stack.push(c);
                }
            }
        }
        out
    }

    pub(crate) fn apply_axis_op(&mut self, op: &crate::patch::AxisOp) -> Result<crate::patch::AxisOp, String> {
        let (inv, targets) = self.authored.apply_axis_op(op)?;
        if !targets.is_empty() {
            for &t in &targets {
                self.scoped_touched.note(crate::structure::Key::Patch(t));
                if let Some(def) = self.authored.definition_of_raw(t) {
                    self.scoped_touched.note(crate::structure::Key::Patch(def));
                }
            }
            // Also wake any anchored patches referencing the mutated axis.
            let axis_id = match op {
                crate::patch::AxisOp::Add { axis } => Some(axis.id),
                crate::patch::AxisOp::Remove { id } => Some(*id),
                crate::patch::AxisOp::AddValue { axis, .. }
                | crate::patch::AxisOp::RemoveValue { axis, .. }
                | crate::patch::AxisOp::DeleteValue { axis, .. }
                | crate::patch::AxisOp::RestoreValue { axis, .. }
                | crate::patch::AxisOp::MoveValue { axis, .. } => Some(*axis),
                crate::patch::AxisOp::SetOrder { .. } => None,
            };
            let wake_nodes: Vec<NodeId> = match axis_id {
                Some(a) => self
                    .authored
                    .patches()
                    .patches
                    .values()
                    .filter(|p| p.region.sels.contains_key(&a))
                    .flat_map(|p| self.authored.patch_wake_nodes(p))
                    .collect(),
                None => self
                    .authored
                    .patches()
                    .patches
                    .values()
                    .flat_map(|p| self.authored.patch_wake_nodes(p))
                    .collect(),
            };
            for n in wake_nodes {
                self.scoped_touched.note(crate::structure::Key::Patch(n));
            }
            self.reconcile_resolved(Some(&targets));
        }
        Ok(inv)
    }

    pub(crate) fn apply_patch_op(&mut self, op: &crate::patch::PatchOp) -> Result<crate::patch::PatchOp, String> {
        let (inv, targets) = self.authored.apply_patch_op(op)?;
        if !targets.is_empty() {
            self.reconcile_resolved(Some(&targets));
        }
        Ok(inv)
    }

    pub(crate) fn apply_solve(&mut self, point: crate::patch::DiscretePoint, leaf: crate::expr::Leaf, slot: Option<Slot>) -> Result<Option<Slot>, String> {
        self.authored.apply_solve(point, leaf, slot)
    }

    /// Move the active point on variation axes, reconciling only nodes whose
    /// patches reference the changed axes (O(targets on changed axes), EN-4).
    pub(crate) fn set_point(&mut self, point: crate::patch::Point) -> BTreeSet<NodeId> {
        let changed = self.point.changed_axes(&point);
        if changed.is_empty() {
            return BTreeSet::new();
        }
        let old_dp = self.point.discrete();
        let new_dp = point.discrete();
        self.point = point;
        let mut targets = BTreeSet::new();
        let mut wake_nodes = Vec::new();
        for a in &changed {
            targets.extend(self.authored.patches().targets_for_axis(*a));
            let is_instance = self.authored.patches().axes.get(a).is_some_and(|ax| ax.kind == crate::patch::AxisKind::Instance);
            for p in self.authored.patches().patches.values() {
                if p.region.sels.contains_key(a) || (is_instance && p.anchor.is_some()) {
                    wake_nodes.extend(self.authored.patch_wake_nodes(p));
                }
            }
            for p in self.session_patches.values() {
                if p.region.sels.contains_key(a) || (is_instance && p.anchor.is_some()) {
                    targets.extend(p.entries.keys().map(|t| t.node));
                }
            }
        }
        // Emit Leaf keys (never Patch keys) for solve-layer entries at old/new discrete point (O(delta)).
        if old_dp != new_dp && !self.authored.patches().solve.is_empty() {
            let mut solve_leaves: Vec<crate::expr::Leaf> = Vec::new();
            if let Some(m) = self.authored.patches().solve.get(&old_dp) {
                solve_leaves.extend(m.keys().copied());
            }
            if let Some(m) = self.authored.patches().solve.get(&new_dp) {
                solve_leaves.extend(m.keys().copied());
            }
            for leaf in solve_leaves {
                targets.insert(leaf.node);
                self.scoped_touched.note(crate::structure::Key::Leaf(leaf));
            }
        }
        for n in wake_nodes {
            self.scoped_touched.note(crate::structure::Key::Patch(n));
        }
        if !targets.is_empty() {
            self.reconcile_resolved(Some(&targets));
            // Also emit Leaf keys for session-patched leaves on changed axes.
            for p in self.session_patches.values() {
                if p.region.sels.keys().any(|a| changed.contains(a))
                    || (p.anchor.is_some()
                        && changed.iter().any(|a| {
                            self.authored.patches().axes.get(a).is_some_and(|ax| ax.kind == crate::patch::AxisKind::Instance)
                        }))
                {
                    for t in p.entries.keys() {
                        if let crate::patch::PatchKey::Leaf(lp) = t.key {
                            self.scoped_touched.note(crate::structure::Key::Leaf(crate::expr::Leaf { node: t.node, path: lp }));
                        }
                    }
                }
            }
        }
        targets
    }

    /// Install or replace a session-layer patch (`SESSION_LAYER`, P8; never saved).
    /// Validated before mutation so an invalid patch never leaves queued keys.
    /// Session patches are leaf-only and emit `Key::Leaf` only (not `Key::Patch`).
    pub(crate) fn set_session_patch(&mut self, mut patch: crate::patch::Patch) -> Result<Vec<crate::expr::Leaf>, String> {
        patch.layer = crate::patch::SESSION_LAYER;
        self.authored
            .patches()
            .validate_session_patch(&patch, &|id| {
                let ty_node = id.expanded_parts().map(|(_, local)| local).unwrap_or(id);
                self.authored.node(ty_node).map(|n| n.ty)
            })?;
        let mut leaves = Vec::new();
        if let Some(old) = self.session_patches.insert(patch.id, patch.clone()) {
            for t in old.entries.keys() {
                if let crate::patch::PatchKey::Leaf(lp) = t.key {
                    leaves.push(crate::expr::Leaf { node: t.node, path: lp });
                }
            }
        }
        for t in patch.entries.keys() {
            if let crate::patch::PatchKey::Leaf(lp) = t.key {
                leaves.push(crate::expr::Leaf { node: t.node, path: lp });
            }
        }
        for &l in &leaves {
            self.scoped_touched.note(crate::structure::Key::Leaf(l));
        }
        Ok(leaves)
    }

    /// Remove a session-layer patch by id, returning the leaves it touched.
    pub(crate) fn remove_session_patch(&mut self, id: crate::patch::PatchId) -> Vec<crate::expr::Leaf> {
        let mut leaves = Vec::new();
        if let Some(old) = self.session_patches.remove(&id) {
            for t in old.entries.keys() {
                if let crate::patch::PatchKey::Leaf(lp) = t.key {
                    let l = crate::expr::Leaf { node: t.node, path: lp };
                    leaves.push(l);
                    self.scoped_touched.note(crate::structure::Key::Leaf(l));
                }
            }
        }
        leaves
    }

    /// Clear every session-layer patch, returning the leaves they touched.
    pub(crate) fn clear_session_patches(&mut self) -> Vec<crate::expr::Leaf> {
        let ids: Vec<_> = self.session_patches.keys().copied().collect();
        let mut leaves = Vec::new();
        for id in ids {
            leaves.extend(self.remove_session_patch(id));
        }
        leaves
    }

    fn needs_resolved_sync(&self, id: NodeId) -> bool {
        !self.authored.patches().patches.is_empty()
            && (self.authored.patches().patches_for_target(id).next().is_some()
                || self.resolved.facets.contains_key(&id)
                || self.resolved.absent.contains(&id)
                || self.resolved.hidden.contains(&id))
    }

    /// Evaluate whether authored node `id` is effectively present at `point`
    /// (recursive over authored ancestors, P6).
    pub(crate) fn authored_present_at(&self, point: &crate::patch::Point, id: NodeId) -> bool {
        let Some(node) = self.authored.live(id) else { return false };
        let pt = self.authored.patches();
        if let Some(p) = pt.winner_authored(point, crate::patch::PatchTarget { node: id, key: crate::patch::PatchKey::Present })
            && p.entries.get(&crate::patch::PatchTarget { node: id, key: crate::patch::PatchKey::Present }).and_then(|v| v.as_bool()) == Some(false)
        {
            return false;
        }
        node.parent.is_none_or(|p| self.authored_present_at(point, p))
    }

    /// Evaluate whether authored node `id` is effectively hidden at `point`
    /// (cascades down from hidden ancestors, P6).
    pub(crate) fn authored_hidden_at(&self, point: &crate::patch::Point, id: NodeId) -> bool {
        let Some(node) = self.authored.live(id) else { return false };
        let pt = self.authored.patches();
        let own = pt
            .winner_authored(point, crate::patch::PatchTarget { node: id, key: crate::patch::PatchKey::Hidden })
            .and_then(|p| p.entries.get(&crate::patch::PatchTarget { node: id, key: crate::patch::PatchKey::Hidden }))
            .and_then(|v| v.as_bool())
            == Some(true);
        own || node.parent.is_some_and(|p| self.authored_hidden_at(point, p))
    }

    /// Evaluate whether authored node `id` carries `kind` at `point` (base or
    /// explicit `FacetPresent(kind)` patch, P21).
    pub(crate) fn authored_has_facet_at(&self, point: &crate::patch::Point, id: NodeId, kind: FacetKind) -> bool {
        if !self.authored_present_at(point, id) {
            return false;
        }
        let target = crate::patch::PatchTarget { node: id, key: crate::patch::PatchKey::FacetPresent(kind) };
        if let Some(p) = self.authored.patches().winner_authored(point, target)
            && let Some(on) = p.entries.get(&target).and_then(|v| v.as_bool())
        {
            return on;
        }
        self.authored.facet(id, kind).is_some()
    }

    /// Stage 1 eager reconciliation of the sparse `ResolvedStore` at
    /// `self.point` (engine.md §2.3, EN-4). Fast-paths to O(1) no-op when no
    /// authored patches exist. Emits exact effective `structure::Key`s and
    /// keeps `ScopedIndexes` in sync with effective presence & optional facets.
    pub(crate) fn reconcile_resolved(&mut self, dirty_targets: Option<&BTreeSet<NodeId>>) {
        if self.authored.patches().patches.is_empty() && self.resolved.is_empty() {
            return;
        }
        let mut work: Vec<NodeId> = match dirty_targets {
            Some(set) => set.iter().copied().collect(),
            None => {
                let mut all = self.authored.patches().all_targets();
                all.extend(self.resolved.facets.keys().copied());
                all.extend(self.resolved.absent.iter().copied());
                all.extend(self.resolved.hidden.iter().copied());
                all.into_iter().collect()
            }
        };
        let mut seen: HashSet<NodeId> = HashSet::with_capacity(work.len());
        let point = self.point.clone();
        while let Some(id) = work.pop() {
            if !seen.insert(id) {
                continue;
            }
            let base_live = self.authored.live(id).cloned();
            let old_absent = self.resolved.absent.contains(&id);
            let old_hidden = self.resolved.hidden.contains(&id);
            let old_facets = self.resolved.facets.remove(&id);

            let Some(node) = base_live else {
                self.resolved.absent.remove(&id);
                self.resolved.hidden.remove(&id);
                continue;
            };
            let present = self.authored_present_at(&point, id);
            let hidden = present && self.authored_hidden_at(&point, id);
            if !present {
                self.resolved.absent.insert(id);
                self.resolved.hidden.remove(&id);
            } else {
                self.resolved.absent.remove(&id);
                if hidden {
                    self.resolved.hidden.insert(id);
                } else {
                    self.resolved.hidden.remove(&id);
                }
            }
            // Cascade down authored children (and derived copies if apart) only
            // when effective presence or visibility changed (keeps leaf edits O(1)).
            if old_absent != !present || old_hidden != hidden {
                work.extend_from_slice(self.authored.children(id));
                if self.apart {
                    work.extend_from_slice(self.copies.children(id));
                }
            }

            // Compute effective facets if present and touched by any active patch on `id`.
            let new_facets: Option<BTreeMap<FacetKind, Facet>> = if !present {
                None
            } else {
                let pt = self.authored.patches();
                let mut opt_facets: BTreeMap<FacetKind, bool> = BTreeMap::new();
                let mut leaf_paths: BTreeSet<LeafPath> = BTreeSet::new();
                for p in pt.patches_for_target(id) {
                    if !p.region.contains_anchored(&pt.axes, &point, p.anchor) {
                        continue;
                    }
                    for target in p.entries.keys() {
                        if target.node != id {
                            continue;
                        }
                        match target.key {
                            crate::patch::PatchKey::FacetPresent(fk) => {
                                if let Some(w) = pt.winner_authored(&point, *target)
                                    && let Some(on) = w.entries.get(target).and_then(|v| v.as_bool())
                                {
                                    opt_facets.insert(fk, on);
                                }
                            }
                            crate::patch::PatchKey::Leaf(lp) => {
                                leaf_paths.insert(lp);
                            }
                            _ => {}
                        }
                    }
                }
                if opt_facets.is_empty() && leaf_paths.is_empty() {
                    None
                } else {
                    let mut map: BTreeMap<FacetKind, Facet> = self.authored.facets_of(id).map(|f| (f.kind(), f.clone())).collect();
                    let mut differs = false;
                    for (fk, on) in opt_facets {
                        if on && !map.contains_key(&fk) && node.ty.spec().allows(fk) {
                            map.insert(fk, fk.default_facet());
                            differs = true;
                        } else if !on && map.remove(&fk).is_some() {
                            differs = true;
                        }
                    }
                    for lp in leaf_paths {
                        let target = crate::patch::PatchTarget { node: id, key: crate::patch::PatchKey::Leaf(lp) };
                        if let Some(w) = pt.winner_authored(&point, target)
                            && let Some(slot) = w.entries.get(&target).and_then(|v| v.as_slot())
                            && let Some(f) = map.get_mut(&lp.facet)
                            && f.set(lp.field, slot.clone()).is_ok()
                        {
                            differs = true;
                        }
                    }
                    differs.then_some(map)
                }
            };

            let old_kinds: BTreeSet<FacetKind> = if old_absent {
                BTreeSet::new()
            } else if let Some(m) = &old_facets {
                m.keys().copied().collect()
            } else {
                self.authored.facets_of(id).map(Facet::kind).collect()
            };
            let new_kinds: BTreeSet<FacetKind> = if !present {
                BTreeSet::new()
            } else if let Some(m) = &new_facets {
                m.keys().copied().collect()
            } else {
                self.authored.facets_of(id).map(Facet::kind).collect()
            };

            // Update ScopedIndexes when effective presence or facet set changed.
            let root = self.scoped.root_of.get(&id).copied().or_else(|| Some(self.root_for(id, node.parent)));
            if let Some(root) = root {
                if old_absent != !present {
                    if !present {
                        self.scoped.set_root(id, None, &mut self.scoped_journal);
                        self.scoped.remove_type(root, node.ty, id, &mut self.scoped_journal, &mut self.scoped_touched);
                        for &k in &old_kinds {
                            self.scoped.remove_facet(root, k, id, &mut self.scoped_journal, &mut self.scoped_touched);
                        }
                    } else {
                        self.scoped.set_root(id, Some(root), &mut self.scoped_journal);
                        self.scoped.insert_type(root, node.ty, id, &mut self.scoped_journal, &mut self.scoped_touched);
                        for &k in &new_kinds {
                            self.scoped.insert_facet(root, k, id, &mut self.scoped_journal, &mut self.scoped_touched);
                        }
                    }
                } else if present && old_kinds != new_kinds {
                    for &k in old_kinds.difference(&new_kinds) {
                        self.scoped.remove_facet(root, k, id, &mut self.scoped_journal, &mut self.scoped_touched);
                    }
                    for &k in new_kinds.difference(&old_kinds) {
                        self.scoped.insert_facet(root, k, id, &mut self.scoped_journal, &mut self.scoped_touched);
                    }
                }
            }

            // Emit effective structural keys and record ResolvedDelta for anything that changed.
            if old_absent != !present || old_hidden != hidden || old_facets != new_facets {
                self.pending_delta.changed_nodes.insert(id);
            }
            if old_absent != !present {
                if present {
                    self.pending_delta.became_present.push((id, node.parent));
                } else {
                    self.pending_delta.became_absent.push((id, node.parent));
                }
                // Notify relations touching `id` so network/constraint/anchor makers react.
                self.scoped_touched.note(crate::structure::Key::Relations(id));
                let rel_touch: Vec<(RelationId, NodeId, NodeId)> = self
                    .authored
                    .relations_from(id)
                    .chain(self.authored.relations_to(id))
                    .chain(self.copies.relations_from(id))
                    .chain(self.copies.relations_to(id))
                    .map(|r| (r.id, r.from, r.to))
                    .collect();
                for (rid, from, to) in rel_touch {
                    self.scoped_touched.note(crate::structure::Key::Relation(rid));
                    self.scoped_touched.note(crate::structure::Key::Relations(from));
                    self.scoped_touched.note(crate::structure::Key::Relations(to));
                }
            }
            if old_absent != !present || old_hidden != hidden {
                self.scoped_touched.note(crate::structure::Key::Node(id));
                if let Some(p) = node.parent {
                    self.scoped_touched.note(crate::structure::Key::Children(p));
                } else {
                    self.scoped_touched.note(crate::structure::Key::Roots);
                }
            }
            if old_kinds != new_kinds {
                self.scoped_touched.note(crate::structure::Key::FacetSet(id));
            }
            if old_absent != !present || old_facets != new_facets {
                for &k in old_kinds.union(&new_kinds) {
                    let before = if old_absent {
                        None
                    } else {
                        match &old_facets {
                            Some(m) => m.get(&k),
                            None => self.authored.facet(id, k),
                        }
                    };
                    let after = if !present {
                        None
                    } else {
                        match &new_facets {
                            Some(m) => m.get(&k),
                            None => self.authored.facet(id, k),
                        }
                    };
                    if before != after {
                        self.scoped_touched.note(crate::structure::Key::Facet(id, k));
                        match (before, after) {
                            (Some(b), Some(a)) => {
                                for field in a.field_names() {
                                    if b.get(field) != a.get(field)
                                        && let Ok(lp) = LeafPath::new(k, field)
                                    {
                                        self.scoped_touched.note(crate::structure::Key::Leaf(crate::expr::Leaf { node: id, path: lp }));
                                    }
                                }
                            }
                            (Some(f), None) | (None, Some(f)) => {
                                for field in f.field_names() {
                                    if let Ok(lp) = LeafPath::new(k, field) {
                                        self.scoped_touched.note(crate::structure::Key::Leaf(crate::expr::Leaf { node: id, path: lp }));
                                    }
                                }
                            }
                            (None, None) => {}
                        }
                    }
                }
            }
            if let Some(m) = new_facets {
                self.resolved.facets.insert(id, m);
            }
        }
    }

    /// The document a write to node `id` goes to.
    pub(crate) fn of_node(&mut self, id: NodeId) -> &mut Document {
        if self.apart && id.is_expanded() { &mut self.copies } else { &mut self.authored }
    }

    /// The document a write to relation `id` goes to.
    pub(crate) fn of_relation(&mut self, id: RelationId) -> &mut Document {
        if self.apart && id.is_expanded() { &mut self.copies } else { &mut self.authored }
    }

    /// Collect what writes touch, in both documents and scoped indexes
    /// (engine.md §3.2). Starts empty.
    pub(crate) fn log_touched(&mut self) {
        self.authored.set_touched(true);
        self.copies.set_touched(true);
        self.scoped_touched.on = true;
        self.scoped_touched.keys.clear();
    }

    /// Keys touched since the last `drain_template_touched`, `take_touched`,
    /// or `rollback` (for recorded-read template invalidation without removing
    /// keys from the main `take_touched` log, `EN-5a`).
    pub(crate) fn drain_template_touched(&mut self) -> Vec<crate::structure::Key> {
        let a = self.authored.touched_slice();
        let c = self.copies.touched_slice();
        let s = &self.scoped_touched.keys;
        let (ia, ic, is) = self.template_cursor;
        let mut out = Vec::new();
        if ia < a.len() {
            out.extend_from_slice(&a[ia..]);
        }
        if ic < c.len() {
            out.extend_from_slice(&c[ic..]);
        }
        if is < s.len() {
            out.extend_from_slice(&s[is..]);
        }
        self.template_cursor = (a.len(), c.len(), s.len());
        out
    }

    /// The keys both documents' writes and scoped index updates touched since
    /// they were last taken.
    pub(crate) fn take_touched(&mut self) -> Vec<crate::structure::Key> {
        self.template_cursor = (0, 0, 0);
        let mut keys = self.authored.take_touched();
        keys.append(&mut self.copies.take_touched());
        keys.append(&mut self.scoped_touched.keys);
        keys
    }

    pub(crate) fn touched_empty(&self) -> bool {
        self.authored.touched_empty() && self.copies.touched_empty() && self.scoped_touched.keys.is_empty()
    }

    /// Open both documents' journals. What earlier writes touched has been
    /// handed to the graph: a rollback clears what this transaction's do.
    pub(crate) fn begin(&mut self) {
        debug_assert!(self.touched_empty(), "a transaction begins with touched keys not handed to the graph");
        self.authored.begin();
        self.copies.begin();
        self.gone_journal = Some(Vec::new());
        self.scoped_journal = Some(Vec::new());
    }

    pub(crate) fn commit(&mut self) {
        self.authored.commit();
        self.copies.commit();
        self.gone_journal = None;
        self.scoped_journal = None;
    }

    pub(crate) fn rollback(&mut self) {
        self.reshaped = crate::stamp();
        self.tokens_shaped = crate::stamp();
        self.authored.rollback();
        self.copies.rollback();
        self.scoped_touched.keys.clear();
        self.template_cursor = (0, 0, 0);
        if let Some(recs) = self.scoped_journal.take() {
            self.scoped.undo(recs);
        }
        for (id, was) in self.gone_journal.take().into_iter().flatten().rev() {
            match was {
                Some(w) => self.gone.insert(id, w),
                None => self.gone.remove(&id),
            };
        }
        self.pending_delta = ResolvedDelta::default();
        if !self.resolved.is_empty() || !self.authored.patches().patches.is_empty() {
            let on = self.scoped_touched.on;
            self.scoped_touched.on = false;
            self.resolved = ResolvedStore::default();
            self.reconcile_resolved(None);
            self.pending_delta = ResolvedDelta::default();
            self.scoped_touched.on = on;
        }
        #[cfg(feature = "differential")]
        if let Err(e) = Tree::of(self).check_invariants() {
            panic!("journal rollback broke an invariant: {e}");
        }
    }

}

/// Zero-allocation iterator over a node's facets (either from `ResolvedStore`,
/// a borrowed `Template` slice in `ViewRegistry`, or straight from `Document`).
enum FacetsIter<'a, I> {
    Empty,
    Resolved(std::collections::btree_map::Values<'a, FacetKind, Facet>),
    Slice(std::slice::Iter<'a, Facet>),
    Base(I),
}

impl<'a, I: Iterator<Item = &'a Facet>> Iterator for FacetsIter<'a, I> {
    type Item = &'a Facet;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            FacetsIter::Empty => None,
            FacetsIter::Resolved(it) => it.next(),
            FacetsIter::Slice(it) => it.next(),
            FacetsIter::Base(it) => it.next(),
        }
    }
}

enum EitherIter<A, B> {
    Left(A),
    Right(B),
}

impl<T, A: Iterator<Item = T>, B: Iterator<Item = T>> Iterator for EitherIter<A, B> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        match self {
            EitherIter::Left(a) => a.next(),
            EitherIter::Right(b) => b.next(),
        }
    }
}

/// A read view of the nodes, authored and derived.
#[derive(Clone, Copy)]
pub struct Tree<'a> {
    doc: &'a Document,
    /// The copies, when they are kept apart from `doc`.
    copies: Option<&'a Document>,
    /// Virtual views registry (`EN-5a`); when `Some`, derived/expanded ids are
    /// answered from virtual views instead of materialized `copies`.
    views: Option<&'a crate::engine::views::ViewRegistry>,
    /// Per-root indexes when backed by `Docs`.
    scoped: Option<&'a ScopedIndexes>,
    /// Sparse non-journaled resolved state at `point` (engine.md §2.3, EN-4).
    resolved: Option<&'a ResolvedStore>,
    /// Active point on variation axes.
    point: Option<&'a crate::patch::Point>,
    /// Session-layer patches (`SESSION_LAYER`, P8).
    session_patches: Option<&'a BTreeMap<crate::patch::PatchId, crate::patch::Patch>>,
    /// Where a cell's reads are noted, when it reads through a recording
    /// tree (engine.md §3.2).
    rec: Option<&'a crate::structure::Recorder>,
}

impl<'a> Tree<'a> {
    /// The tree over a store's documents.
    pub(crate) fn of(docs: &'a Docs) -> Tree<'a> {
        Tree {
            doc: &docs.authored,
            copies: docs.apart.then_some(&docs.copies),
            views: None,
            scoped: Some(&docs.scoped),
            resolved: Some(&docs.resolved),
            point: Some(&docs.point),
            session_patches: Some(&docs.session_patches),
            rec: None,
        }
    }

    /// Read view over authored `docs` + virtual `views`, ignoring materialized
    /// `docs.copies` (`EN-5a` dual-run oracle & virtual read path).
    pub(crate) fn with_views(docs: &'a Docs, views: &'a crate::engine::views::ViewRegistry) -> Tree<'a> {
        Tree {
            doc: &docs.authored,
            copies: None,
            views: Some(views),
            scoped: Some(&docs.scoped),
            resolved: Some(&docs.resolved),
            point: Some(&docs.point),
            session_patches: Some(&docs.session_patches),
            rec: None,
        }
    }

    /// This tree, noting what is read through it in `rec`.
    pub(crate) fn recording(self, rec: &'a crate::structure::Recorder) -> Tree<'a> {
        Tree { rec: Some(rec), ..self }
    }

    fn note(self, k: crate::structure::Key) {
        if let Some(r) = self.rec {
            r.borrow_mut().push(k);
        }
    }

    pub(crate) fn note_key(self, k: crate::structure::Key) {
        self.note(k);
    }

    pub(crate) fn note_all(self, keys: &[crate::structure::Key]) {
        if let Some(r) = self.rec {
            r.borrow_mut().extend_from_slice(keys);
        }
    }

    /// A read a recording tree has no key for: caught where debug assertions
    /// run, so it is never missed (engine.md §3.2).
    fn unkeyed(self, what: &str) {
        debug_assert!(self.rec.is_none(), "a recording tree has no key for `{what}` (engine.md §3.2)");
    }

    /// The tree over a document that holds no copies apart from it: a
    /// scratch document cloned for planning, a fragment, a document being
    /// loaded. A derived node it does not hold is not there.
    pub fn authored_only(doc: &'a Document) -> Tree<'a> {
        Tree { doc, copies: None, views: None, scoped: None, resolved: None, point: None, session_patches: None, rec: None }
    }

    /// Each document's invariants, with parents answered across both (a
    /// copy's owner is authored), and that each node is in the document its
    /// id says. A live node's parent is live, so no live copy is under a
    /// dead authored ancestor.
    pub fn check_invariants(self) -> Result<(), String> {
        self.doc.check_invariants_over(&|p| self.at(p).is_live(p))?;
        if let Some(copies) = self.copies {
            if let Some(n) = self.doc.nodes().find(|n| n.id.is_expanded()) {
                return Err(format!("derived {} is in the authored document", n.id));
            }
            if let Some(n) = copies.nodes().find(|n| !n.id.is_expanded()) {
                return Err(format!("{} is in the copies document", n.id));
            }
            copies.check_invariants_over(&|p| self.at(p).is_live(p))?;
        }
        if let Some(scoped) = self.scoped {
            let mut expected_root: BTreeMap<NodeId, NodeId> = BTreeMap::new();
            let mut expected_type: BTreeMap<(NodeId, TypeId), BTreeSet<NodeId>> = BTreeMap::new();
            let mut expected_facet: BTreeMap<(NodeId, FacetKind), BTreeSet<NodeId>> = BTreeMap::new();
            let mut stack: Vec<(NodeId, NodeId)> = self.doc.roots().iter().map(|&r| (r, r)).collect();
            while let Some((id, root)) = stack.pop() {
                let Some(n) = self.live(id) else { continue };
                expected_root.insert(id, root);
                expected_type.entry((root, n.ty)).or_default().insert(id);
                for f in self.facets_of(id) {
                    expected_facet.entry((root, f.kind())).or_default().insert(id);
                }
                for &c in self.children(id).iter() {
                    stack.push((c, root));
                }
            }
            if scoped.root_of != expected_root {
                return Err(format!("scoped.root_of mismatch: {} vs {}", scoped.root_of.len(), expected_root.len()));
            }
            if scoped.by_type != expected_type {
                return Err("scoped.by_type mismatch".into());
            }
            if scoped.by_facet != expected_facet {
                return Err("scoped.by_facet mismatch".into());
            }
        }
        Ok(())
    }

    /// The document that holds node `id`.
    fn at(self, id: NodeId) -> &'a Document {
        match self.copies {
            Some(c) if id.is_expanded() => c,
            _ => self.doc,
        }
    }

    /// Both documents, authored first.
    fn both(self) -> impl Iterator<Item = &'a Document> + 'a {
        std::iter::once(self.doc).chain(self.copies)
    }

    // The authored document's version and counters.

    pub fn version(self) -> crate::id::Version {
        self.unkeyed("version");
        self.doc.version()
    }

    pub fn client(self) -> u32 {
        self.doc.client()
    }

    pub fn next_id(self) -> NodeId {
        self.doc.next_id()
    }

    pub fn next_ids(self, n: usize) -> Vec<NodeId> {
        self.doc.next_ids(n)
    }

    pub fn next_relation_id(self) -> RelationId {
        self.doc.next_relation_id()
    }

    pub fn next_relation_ids(self, n: usize) -> Vec<RelationId> {
        self.doc.next_relation_ids(n)
    }

    pub fn next_session_ids(self, n: usize) -> Vec<NodeId> {
        self.doc.next_session_ids(n)
    }

    pub fn next_session_relation_ids(self, n: usize) -> Vec<RelationId> {
        self.doc.next_session_relation_ids(n)
    }

    fn is_absent(self, id: NodeId) -> bool {
        if id.is_expanded()
            && let Some(v) = self.views
        {
            return v.is_absent(id);
        }
        self.resolved.is_some_and(|r| r.absent.contains(&id))
    }

    /// Any node, live or tombstoned.
    pub fn node(self, id: NodeId) -> Option<&'a Node> {
        self.note(crate::structure::Key::Node(id));
        if id.is_expanded()
            && let Some(v) = self.views
        {
            return v.node(id);
        }
        self.at(id).node(id)
    }

    pub fn live(self, id: NodeId) -> Option<&'a Node> {
        self.note(crate::structure::Key::Node(id));
        if id.is_expanded()
            && let Some(v) = self.views
        {
            return v.live(id);
        }
        if self.resolved.is_some_and(|r| r.absent.contains(&id)) {
            return None;
        }
        self.at(id).live(id)
    }

    pub fn is_live(self, id: NodeId) -> bool {
        self.note(crate::structure::Key::Node(id));
        if id.is_expanded()
            && let Some(v) = self.views
        {
            return v.is_live(id);
        }
        if self.resolved.is_some_and(|r| r.absent.contains(&id)) {
            return false;
        }
        self.at(id).is_live(id)
    }

    /// Whether `id` is hidden at the active point (cascading `PatchKey::Hidden`, P6).
    pub fn is_hidden(self, id: NodeId) -> bool {
        self.note(crate::structure::Key::Node(id));
        if id.is_expanded()
            && let Some(v) = self.views
        {
            return v.is_hidden(id);
        }
        self.resolved.is_some_and(|r| r.hidden.contains(&id))
    }

    /// The authored patch table (axes, patches, solve layer).
    pub fn patches(self) -> &'a crate::patch::PatchTable {
        self.doc.patches()
    }

    /// The active point on variation axes, when backed by `Docs`.
    pub fn point(self) -> Option<&'a crate::patch::Point> {
        self.point
    }

    pub fn nodes(self) -> impl Iterator<Item = &'a Node> + 'a {
        self.unkeyed("nodes");
        let has_views = self.views.is_some();
        self.both()
            .flat_map(Document::nodes)
            .filter(move |n| !has_views || !n.id.is_expanded())
            .chain(self.views.into_iter().flat_map(|v| v.nodes()))
    }

    pub fn live_nodes(self) -> impl Iterator<Item = &'a Node> + 'a {
        self.unkeyed("live_nodes");
        let resolved = self.resolved;
        let has_views = self.views.is_some();
        self.both()
            .flat_map(Document::live_nodes)
            .filter(move |n| (!has_views || !n.id.is_expanded()) && !resolved.is_some_and(|r| r.absent.contains(&n.id)))
            .chain(self.views.into_iter().flat_map(|v| v.live_nodes()))
    }

    pub fn roots(self) -> Cow<'a, [NodeId]> {
        self.note(crate::structure::Key::Roots);
        let raw = self.doc.roots();
        match self.resolved.filter(|r| !r.absent.is_empty()) {
            Some(r) if raw.iter().any(|id| r.absent.contains(id)) => {
                Cow::Owned(raw.iter().copied().filter(|id| !r.absent.contains(id)).collect())
            }
            _ => Cow::Borrowed(raw),
        }
    }

    /// The last root in root order, without visiting the others.
    pub fn last_root(self) -> Option<NodeId> {
        self.unkeyed("last_root");
        match self.resolved.filter(|r| !r.absent.is_empty()) {
            Some(r) => self.doc.roots().iter().rev().copied().find(|id| !r.absent.contains(id)),
            None => self.doc.last_root(),
        }
    }

    /// Live children in sibling order. Merged by (order, id), the
    /// documents' own tie-break, where both hold children of `id`: only at
    /// an owner, the authored parent of its copies.
    pub fn children(self, id: NodeId) -> Cow<'a, [NodeId]> {
        self.note(crate::structure::Key::Children(id));
        if self.is_absent(id) {
            return Cow::Borrowed(&[]);
        }
        let absent = self.resolved.filter(|r| !r.absent.is_empty());
        if let Some(views) = self.views {
            if id.is_expanded() {
                return views.children_of(id);
            }
            let mine_raw = self.doc.children(id);
            let derived = views.children_of(id);
            let filter_mine = |c: &NodeId| !c.is_expanded() && absent.is_none_or(|r| !r.absent.contains(c));
            if derived.is_empty() {
                return if mine_raw.iter().all(filter_mine) {
                    Cow::Borrowed(mine_raw)
                } else {
                    Cow::Owned(mine_raw.iter().copied().filter(filter_mine).collect())
                };
            }
            if mine_raw.iter().all(|c| !filter_mine(c)) {
                return derived;
            }
            let mut out = Vec::with_capacity(mine_raw.len() + derived.len());
            let mut a = mine_raw.iter().copied().filter(filter_mine).peekable();
            let mut b = derived.iter().copied().peekable();
            let key_a = |n: NodeId| (&self.doc.node(n).expect("authored child").order, n);
            let key_b = |n: NodeId| (&views.node(n).expect("virtual child").order, n);
            while let (Some(&x), Some(&y)) = (a.peek(), b.peek()) {
                if key_a(x) <= key_b(y) {
                    out.push(x);
                    a.next();
                } else {
                    out.push(y);
                    b.next();
                }
            }
            out.extend(a);
            out.extend(b);
            return Cow::Owned(out);
        }
        let mine = self.at(id).children(id);
        let Some(copies) = self.copies.filter(|_| !id.is_expanded()) else {
            return match absent {
                Some(r) if mine.iter().any(|c| r.absent.contains(c)) => {
                    Cow::Owned(mine.iter().copied().filter(|c| !r.absent.contains(c)).collect())
                }
                _ => Cow::Borrowed(mine),
            };
        };
        let derived = copies.children(id);
        if derived.is_empty() {
            return match absent {
                Some(r) if mine.iter().any(|c| r.absent.contains(c)) => {
                    Cow::Owned(mine.iter().copied().filter(|c| !r.absent.contains(c)).collect())
                }
                _ => Cow::Borrowed(mine),
            };
        }
        if mine.is_empty() {
            return Cow::Borrowed(derived);
        }
        let key = |n: &NodeId| (&self.at(*n).node(*n).expect("a child is held").order, *n);
        let mut out = Vec::with_capacity(mine.len() + derived.len());
        let (mut a, mut b) = (mine.iter().peekable(), derived.iter().peekable());
        while let (Some(x), Some(y)) = (a.peek(), b.peek()) {
            if key(x) <= key(y) {
                if absent.is_none_or(|r| !r.absent.contains(*x)) {
                    out.push(**x);
                }
                a.next();
            } else {
                out.push(**y);
                b.next();
            }
        }
        for &x in a {
            if absent.is_none_or(|r| !r.absent.contains(&x)) {
                out.push(x);
            }
        }
        out.extend(b.copied());
        Cow::Owned(out)
    }

    /// Live descendants, preorder.
    pub fn descendants(self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.children(id).iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.children(n).iter().rev().copied());
        }
        crate::work::visit(crate::work::Site::Descendants, out.len());
        out
    }

    /// The preorder of `id` and its descendants, up to `limit` nodes: `None`
    /// once there are more. What a caller that only wants a small subtree
    /// uses instead of [`Tree::descendants`], which walks all of it.
    pub fn descendants_within(self, id: NodeId, limit: usize) -> Option<Vec<NodeId>> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.children(id).iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            if out.len() == limit {
                return None;
            }
            out.push(n);
            stack.extend(self.children(n).iter().rev().copied());
        }
        crate::work::visit(crate::work::Site::Descendants, out.len());
        Some(out)
    }

    /// Sibling order from the top of `id`'s tree down to `id`: sorting
    /// nodes by it puts them in preorder (painter order), whatever set they
    /// come from, with no walk of the content around them.
    pub fn order_path(self, id: NodeId) -> Vec<&'a OrderKey> {
        let mut out = Vec::new();
        let mut at = self.live(id);
        while let Some(n) = at {
            out.push(&n.order);
            at = n.parent.and_then(|p| self.live(p));
        }
        out.reverse();
        out
    }

    /// Every expanded node's id, live or tombstoned (component-model.md §6).
    /// Expanded ids sort after every other id, so this is a range read.
    pub fn expanded_ids(self) -> impl Iterator<Item = NodeId> + 'a {
        self.unkeyed("expanded_ids");
        match self.views {
            Some(v) => EitherIter::Left(v.expanded_ids()),
            None => EitherIter::Right(self.both().flat_map(Document::expanded_ids)),
        }
    }

    /// How many ancestors a node has.
    pub fn depth(self, id: NodeId) -> usize {
        let mut d = 0;
        let mut cur = self.node(id).and_then(|n| n.parent);
        while let Some(p) = cur {
            d += 1;
            cur = self.node(p).and_then(|n| n.parent);
        }
        d
    }

    /// Whether `a` is a strict ancestor of `of`.
    pub fn is_ancestor(self, a: NodeId, of: NodeId) -> bool {
        let mut cur = self.node(of).and_then(|n| n.parent);
        while let Some(p) = cur {
            if p == a {
                return true;
            }
            cur = self.node(p).and_then(|n| n.parent);
        }
        false
    }

    pub fn nodes_of_type(self, ty: TypeId) -> impl Iterator<Item = NodeId> + 'a {
        self.unkeyed("nodes_of_type");
        let resolved = self.resolved;
        let has_views = self.views.is_some();
        self.both()
            .flat_map(move |d| d.nodes_of_type(ty))
            .filter(move |id| (!has_views || !id.is_expanded()) && !resolved.is_some_and(|r| r.absent.contains(id)))
            .chain(self.views.into_iter().flat_map(move |v| v.nodes_of_type(ty)))
    }

    pub fn nodes_with(self, kind: FacetKind) -> impl Iterator<Item = NodeId> + 'a {
        self.unkeyed("nodes_with");
        let resolved = self.resolved;
        let has_views = self.views.is_some();
        let base = self.both().flat_map(move |d| d.nodes_with(kind)).filter(move |id| {
            if has_views && id.is_expanded() {
                return false;
            }
            let Some(r) = resolved else { return true };
            if r.absent.contains(id) {
                return false;
            }
            r.facets.get(id).is_none_or(|m| m.contains_key(&kind))
        });
        let added = resolved
            .into_iter()
            .flat_map(|r| r.facets.iter())
            .filter(move |(id, map)| {
                (!has_views || !id.is_expanded())
                    && !resolved.is_some_and(|r| r.absent.contains(id))
                    && map.contains_key(&kind)
                    && self.at(**id).facet(**id, kind).is_none()
            })
            .map(|(&id, _)| id);
        base.chain(added).chain(self.views.into_iter().flat_map(move |v| v.nodes_with(kind)))
    }

    /// Live nodes of `ty` under top-level `root` (engine.md §3.2): O(matches
    /// in root) and keyed by `Key::In(root, ty)`.
    pub fn nodes_of_type_in(self, root: NodeId, ty: TypeId) -> Vec<NodeId> {
        self.note(crate::structure::Key::In(root, ty));
        if let Some(views) = self.views {
            let mut out: Vec<NodeId> = match self.scoped {
                Some(scoped) => scoped
                    .by_type
                    .get(&(root, ty))
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|id| !id.is_expanded())
                    .collect(),
                None => self
                    .doc
                    .nodes_of_type(ty)
                    .filter(|&id| !id.is_expanded() && !self.is_absent(id) && self.top_root(id) == Some(root))
                    .collect(),
            };
            out.extend(views.nodes_of_type_in(root, ty));
            out.sort();
            return out;
        }
        if let Some(scoped) = self.scoped {
            return scoped.by_type.get(&(root, ty)).into_iter().flatten().copied().collect();
        }
        self.both()
            .flat_map(move |d| d.nodes_of_type(ty))
            .filter(|&id| self.top_root(id) == Some(root))
            .collect()
    }

    /// Live nodes carrying `kind` under top-level `root` (engine.md §3.2):
    /// O(matches in root) and keyed by `Key::With(root, kind)`.
    pub fn nodes_with_in(self, root: NodeId, kind: FacetKind) -> Vec<NodeId> {
        self.note(crate::structure::Key::With(root, kind));
        if let Some(views) = self.views {
            let mut out: Vec<NodeId> = match self.scoped {
                Some(scoped) => scoped
                    .by_facet
                    .get(&(root, kind))
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|id| !id.is_expanded())
                    .collect(),
                None => self
                    .doc
                    .nodes_with(kind)
                    .filter(|&id| !id.is_expanded() && !self.is_absent(id) && self.top_root(id) == Some(root))
                    .collect(),
            };
            out.extend(views.nodes_with_in(root, kind));
            out.sort();
            return out;
        }
        if let Some(scoped) = self.scoped {
            return scoped.by_facet.get(&(root, kind)).into_iter().flatten().copied().collect();
        }
        self.both()
            .flat_map(move |d| d.nodes_with(kind))
            .filter(|&id| self.top_root(id) == Some(root))
            .collect()
    }

    /// Top-level root of live node `id`, if live.
    pub fn top_root(self, id: NodeId) -> Option<NodeId> {
        if id.is_expanded()
            && let Some(views) = self.views
        {
            return views.top_root(id);
        }
        if let Some(scoped) = self.scoped {
            return scoped.root_of.get(&id).copied();
        }
        let mut cur = self.live(id)?;
        while let Some(p) = cur.parent {
            cur = self.live(p)?;
        }
        Some(cur.id)
    }

    /// Effective facet at the active point (sparse `ResolvedStore` / virtual
    /// `ViewRegistry`, 0 heap allocations): a recording tree notes the whole facet (§3.1).
    pub fn facet(self, id: NodeId, kind: FacetKind) -> Option<&'a Facet> {
        self.note(crate::structure::Key::Facet(id, kind));
        if id.is_expanded()
            && let Some(views) = self.views
        {
            return views.facet(id, kind);
        }
        if let Some(r) = self.resolved {
            if r.absent.contains(&id) {
                return None;
            }
            if let Some(map) = r.facets.get(&id) {
                return map.get(&kind);
            }
        }
        self.at(id).facet(id, kind)
    }

    /// Unpatched base facet from the authored document or template (for inspector
    /// and drag planner, engine.md §2.3, EN-4/EN-5a).
    pub fn base_facet(self, id: NodeId, kind: FacetKind) -> Option<&'a Facet> {
        self.note(crate::structure::Key::Facet(id, kind));
        if id.is_expanded()
            && let Some(views) = self.views
        {
            return views.base_facet(id, kind);
        }
        self.at(id).facet(id, kind)
    }

    /// Unpatched base leaf slot from the authored document or template (EN-4/EN-5a).
    pub fn base_leaf(self, id: NodeId, path: LeafPath) -> Option<&'a Slot> {
        self.note(crate::structure::Key::Facet(id, path.facet));
        if id.is_expanded()
            && let Some(views) = self.views
        {
            return views.base_facet(id, path.facet)?.get(path.field);
        }
        self.at(id).leaf(id, path)
    }

    /// All unpatched base facets of a node from the authored document (EN-4b),
    /// recording `FacetSet` + per-facet keys on a recording tree so `Template`
    /// compilation stays point-independent and properly invalidated (`EN-5b`).
    pub fn base_facets_of(self, id: NodeId) -> impl Iterator<Item = &'a Facet> + 'a {
        if self.rec.is_some() {
            self.note(crate::structure::Key::FacetSet(id));
            for f in self.at(id).facets_of(id) {
                self.note(crate::structure::Key::Facet(id, f.kind()));
            }
        }
        self.at(id).facets_of(id)
    }

    /// Unpatched authored liveness of `id` (ignoring `ResolvedStore::absent`),
    /// recording `Key::Node(id)` (`EN-5b` point-independent template compilation).
    pub(crate) fn base_live(self, id: NodeId) -> Option<&'a Node> {
        self.note(crate::structure::Key::Node(id));
        self.at(id).live(id)
    }

    /// Unpatched authored children of `id` (ignoring `ResolvedStore::absent`),
    /// recording `Key::Children(id)` (`EN-5b`).
    pub(crate) fn base_children(self, id: NodeId) -> &'a [NodeId] {
        self.note(crate::structure::Key::Children(id));
        self.at(id).children(id)
    }

    /// Unpatched authored descendants of `id` (ignoring `ResolvedStore::absent`),
    /// recording `Key::Children` along the walk (`EN-5b`).
    pub(crate) fn base_descendants(self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            for &c in self.base_children(cur) {
                if self.base_live(c).is_some() {
                    out.push(c);
                    stack.push(c);
                }
            }
        }
        out
    }

    /// Dedicated system-owned solve-layer value for `leaf` at the active
    /// discrete point (engine.md §2.4 Tier 4, P17) — zero heap allocations.
    pub(crate) fn solve_leaf(self, leaf: Leaf) -> Option<&'a crate::value::Value> {
        if self.doc.patches().solve.is_empty() {
            return None;
        }
        if !self.is_live(leaf.node) || !self.has(leaf.node, leaf.path.facet) {
            return None;
        }
        static EMPTY_DISCRETE: crate::patch::DiscretePoint = BTreeMap::new();
        let dp = self.point.map(|p| &p.values).unwrap_or(&EMPTY_DISCRETE);
        self.doc.patches().solve_get(dp, leaf)?.constant()
    }

    /// Session-layer patch value for `leaf` at the active point (engine.md §2.4 Tier 1, P8).
    pub(crate) fn session_leaf(self, leaf: Leaf) -> Option<&'a crate::value::Value> {
        let session = self.session_patches.filter(|s| !s.is_empty())?;
        let point = self.point?;
        if !self.is_live(leaf.node) || !self.has(leaf.node, leaf.path.facet) {
            return None;
        }
        let target = crate::patch::PatchTarget { node: leaf.node, key: crate::patch::PatchKey::Leaf(leaf.path) };
        let win = if leaf.node.is_expanded()
            && let Some(views) = self.views
            && let Some(coord) = views.coord_of(leaf.node)
        {
            self.doc.patches().winner_session_at_view(session, point, coord, target)?
        } else {
            self.doc.patches().winner_session(session, point, target)?
        };
        win.entries.get(&target)?.as_slot()?.constant()
    }

    /// Whether `id` carries `kind`, and nothing of what it holds: a reader
    /// that asks only this is not reached by a write to the facet's fields.
    pub fn has(self, id: NodeId, kind: FacetKind) -> bool {
        self.note(crate::structure::Key::FacetSet(id));
        if id.is_expanded()
            && let Some(views) = self.views
        {
            return views.has(id, kind);
        }
        if let Some(r) = self.resolved {
            if r.absent.contains(&id) {
                return false;
            }
            if let Some(map) = r.facets.get(&id) {
                return map.contains_key(&kind);
            }
        }
        self.at(id).facet(id, kind).is_some()
    }

    pub fn get<T: LeafStruct>(self, id: NodeId) -> Option<&'a T> {
        self.note(crate::structure::Key::Facet(id, T::KIND));
        if id.is_expanded()
            && let Some(views) = self.views
        {
            return views.facet(id, T::KIND).and_then(T::unwrap_ref);
        }
        if let Some(r) = self.resolved {
            if r.absent.contains(&id) {
                return None;
            }
            if let Some(map) = r.facets.get(&id) {
                return map.get(&T::KIND).and_then(T::unwrap_ref);
            }
        }
        self.at(id).get::<T>(id)
    }

    /// All effective facets of a node at the active point, in kind order.
    pub fn facets_of(self, id: NodeId) -> impl Iterator<Item = &'a Facet> + 'a {
        if id.is_expanded()
            && let Some(views) = self.views
        {
            if self.rec.is_some() {
                self.note(crate::structure::Key::FacetSet(id));
            }
            return match views.facets_of(id) {
                crate::engine::views::VirtualFacets::Empty => FacetsIter::Empty,
                crate::engine::views::VirtualFacets::Borrowed(slice) => {
                    if self.rec.is_some() {
                        for f in slice {
                            self.note(crate::structure::Key::Facet(id, f.kind()));
                        }
                    }
                    FacetsIter::Slice(slice.iter())
                }
                crate::engine::views::VirtualFacets::Resolved(map) => {
                    if self.rec.is_some() {
                        for &k in map.keys() {
                            self.note(crate::structure::Key::Facet(id, k));
                        }
                    }
                    FacetsIter::Resolved(map.values())
                }
            };
        }
        if let Some(r) = self.resolved {
            if r.absent.contains(&id) {
                if self.rec.is_some() {
                    self.note(crate::structure::Key::FacetSet(id));
                }
                return FacetsIter::Empty;
            }
            if let Some(map) = r.facets.get(&id) {
                if self.rec.is_some() {
                    self.note(crate::structure::Key::FacetSet(id));
                    for &k in map.keys() {
                        self.note(crate::structure::Key::Facet(id, k));
                    }
                }
                return FacetsIter::Resolved(map.values());
            }
        }
        if self.rec.is_some() {
            self.note(crate::structure::Key::FacetSet(id));
            for f in self.at(id).facets_of(id) {
                self.note(crate::structure::Key::Facet(id, f.kind()));
            }
        }
        FacetsIter::Base(self.at(id).facets_of(id))
    }

    pub fn leaf(self, id: NodeId, path: LeafPath) -> Option<&'a Slot> {
        self.note(crate::structure::Key::Facet(id, path.facet));
        if id.is_expanded()
            && let Some(views) = self.views
        {
            return views.leaf(id, path);
        }
        if let Some(r) = self.resolved {
            if r.absent.contains(&id) {
                return None;
            }
            if let Some(map) = r.facets.get(&id) {
                return map.get(&path.facet)?.get(path.field);
            }
        }
        self.at(id).leaf(id, path)
    }

    /// Every effective bound slot on a live node, with its expression: facet
    /// kind by kind, and by id within a kind, as one document gives them.
    pub fn bound_slots(self) -> impl Iterator<Item = (Leaf, &'a Expr)> + 'a {
        self.unkeyed("bound_slots");
        let resolved = self.resolved.filter(|r| !r.is_empty());
        let mut out: Vec<(Leaf, &'a Expr)> = Vec::new();
        if resolved.is_some() || self.views.is_some() {
            let has_views = self.views.is_some();
            for n in self.both().flat_map(Document::live_nodes) {
                if has_views && n.id.is_expanded() {
                    continue;
                }
                if resolved.is_some_and(|r| r.absent.contains(&n.id)) {
                    continue;
                }
                if let Some(map) = resolved.and_then(|r| r.facets.get(&n.id)) {
                    for f in map.values() {
                        for field in f.field_names() {
                            if let Some(e) = f.get(field).and_then(Slot::expr) {
                                out.push((Leaf { node: n.id, path: LeafPath::new(f.kind(), field).expect("declared") }, e));
                            }
                        }
                    }
                } else {
                    for f in self.at(n.id).facets_of(n.id) {
                        for field in f.field_names() {
                            if let Some(e) = f.get(field).and_then(Slot::expr) {
                                out.push((Leaf { node: n.id, path: LeafPath::new(f.kind(), field).expect("declared") }, e));
                            }
                        }
                    }
                }
            }
            if let Some(views) = self.views {
                out.extend(views.bound_slots());
            }
            // Stable sort by (facet, node) preserves each facet's declared
            // `field_names()` order, matching `Document::bound_slots_of`.
            out.sort_by_key(|(l, _)| (l.path.facet, l.node));
            return out.into_iter();
        }
        let kinds: BTreeSet<FacetKind> = self.both().flat_map(|d| d.raw_facets().keys().copied()).collect();
        for kind in kinds {
            for d in self.both() {
                out.extend(d.bound_slots_of(kind));
            }
        }
        out.into_iter()
    }

    /// Whether binding `target` to `expr` would let a value depend on itself,
    /// following existing bindings. Checked at apply time so the graph is
    /// always a DAG.
    pub fn binding_cycles(self, target: Leaf, expr: &Expr) -> bool {
        let mut stack: Vec<Leaf> = expr.deps().into_iter().collect();
        let mut seen = BTreeSet::new();
        while let Some(l) = stack.pop() {
            if l == target {
                return true;
            }
            if !seen.insert(l) {
                continue;
            }
            if let Some(e) = self.leaf(l.node, l.path).and_then(|s| s.expr()) {
                stack.extend(e.deps());
            }
        }
        false
    }

    /// Whether `expr` reads interaction state (hover, pressed, selected),
    /// directly or through other bindings: its value is then presentation,
    /// not content (reactivity.md §4.2).
    pub fn reads_interaction(self, expr: &Expr) -> bool {
        let mut stack: Vec<Leaf> = expr.deps().into_iter().collect();
        let mut seen = BTreeSet::new();
        while let Some(l) = stack.pop() {
            if l.path.facet == FacetKind::Interaction {
                return true;
            }
            if !seen.insert(l) {
                continue;
            }
            if let Some(e) = self.leaf(l.node, l.path).and_then(|s| s.expr()) {
                stack.extend(e.deps());
            }
        }
        false
    }

    pub fn relation(self, id: RelationId) -> Option<&'a Relation> {
        self.note(crate::structure::Key::Relation(id));
        let rel = if id.is_expanded()
            && let Some(views) = self.views
        {
            views.relation(id)
        } else {
            match self.copies {
                Some(c) if id.is_expanded() => c.relation(id),
                _ => self.doc.relation(id),
            }
        }?;
        if self.is_absent(rel.from) || self.is_absent(rel.to) {
            return None;
        }
        Some(rel)
    }

    /// The derived relations whose id names `owner`, from either document.
    pub(crate) fn rooted(self, owner: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.unkeyed("rooted");
        self.both().flat_map(move |d| d.rooted(owner))
    }

    pub fn relations(self) -> impl Iterator<Item = &'a Relation> + 'a {
        self.unkeyed("relations");
        let has_views = self.views.is_some();
        self.both()
            .flat_map(Document::relations)
            .filter(move |rel| !has_views || !rel.id.is_expanded())
            .chain(self.views.into_iter().flat_map(|v| v.relations()))
            .filter(move |rel| !self.is_absent(rel.from) && !self.is_absent(rel.to))
    }

    /// Edges leaving `id`, from either document: an authored relation can
    /// end at a copy and a derived one start at an authored node.
    pub fn relations_from(self, id: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.note(crate::structure::Key::Relations(id));
        let has_views = self.views.is_some();
        self.both()
            .flat_map(move |d| d.relations_from(id))
            .filter(move |rel| !has_views || !rel.id.is_expanded())
            .chain(self.views.into_iter().flat_map(move |v| v.relations_from(id)))
            .filter(move |rel| !self.is_absent(rel.from) && !self.is_absent(rel.to))
    }

    /// Edges arriving at `id`: "what drives this node" (doc 05 §3.1).
    pub fn relations_to(self, id: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.note(crate::structure::Key::Relations(id));
        let has_views = self.views.is_some();
        self.both()
            .flat_map(move |d| d.relations_to(id))
            .filter(move |rel| !has_views || !rel.id.is_expanded())
            .chain(self.views.into_iter().flat_map(move |v| v.relations_to(id)))
            .filter(move |rel| !self.is_absent(rel.from) && !self.is_absent(rel.to))
    }
}

#[cfg(test)]
mod tests {
    use crate::facet::{Composition, Name};
    use crate::*;

    /// `with_document` takes authored documents only: one holding copies (as
    /// a store that keeps them in the document has) is refused.
    #[test]
    #[should_panic(expected = "with_document takes an authored document")]
    fn with_document_refuses_a_document_holding_a_copy() {
        let mut store = StoreCore::build(Document::new(), false);
        let comp = store.authored().next_id();
        let mut c = Composition::default();
        c.dims = Slot::Const(Value::Enum("2d".into()));
        let mut name = Name::default();
        name.text = Slot::Const(Value::Str("sheet".into()));
        let ops = vec![Op::Create { id: comp, ty: TypeId::lookup("composition").unwrap(), parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] }];
        store.apply(TransactionInput { label: "sheet".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }).unwrap();
        store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
        let doc = store.authored().clone();
        assert!(doc.derived_content().is_some_and(|why| why.contains("derived copy")));
        StoreCore::with_document(doc);
    }

    /// Scoped per-root indexes (`nodes_of_type_in`, `nodes_with_in`) partition
    /// authored nodes and derived copies by top-level root, follow reparents
    /// across roots (emitting keys for both old and new roots), and roll back
    /// cleanly (EN-1).
    #[test]
    fn scoped_indexes_follow_moves_and_rollback() {
        let mut store = StoreCore::new();
        let src = r#"<edit>
          <composition id="$a" name="A" dims="2d">
            <group id="$g" name="g" layout.mode="stack_x"><rect id="$r" name="r" w="10" h="10"/></group>
          </composition>
          <composition id="$b" name="B" dims="2d">
            <rect id="$rb" name="rb" w="20" h="20"/>
          </composition>
        </edit>"#;
        let plan = crate::edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
        let (a, b, g, r, rb) = (plan.aliases["$a"], plan.aliases["$b"], plan.aliases["$g"], plan.aliases["$r"], plan.aliases["$rb"]);
        store.apply(TransactionInput { label: "init".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops: plan.ops, exact: false }).unwrap();
        store.place_standard("meter", g, [0.0, 0.0], Actor::Human).unwrap();

        let rect_ty = TypeId::lookup("rect").unwrap();
        let in_a = store.tree().nodes_of_type_in(a, rect_ty);
        let in_b = store.tree().nodes_of_type_in(b, rect_ty);
        assert!(in_a.contains(&r) && !in_a.contains(&rb), "A holds r only: {in_a:?}");
        assert!(in_b.contains(&rb) && !in_b.contains(&r), "B holds rb only: {in_b:?}");
        assert_eq!(store.tree().nodes_with_in(a, FacetKind::Layout), vec![g]);
        assert!(store.tree().nodes_with_in(b, FacetKind::Layout).is_empty());
        // Meter's derived copies under g are indexed under A.
        let derived_in_a: Vec<NodeId> = store.tree().nodes_with_in(a, FacetKind::Transform2d).into_iter().filter(|id| id.is_expanded()).collect();
        assert!(!derived_in_a.is_empty(), "derived copies under g belong to root A");

        // Reparent g from A to B: authored children AND derived copies move to B.
        store.apply(TransactionInput {
            label: "move g".into(),
            actor: Actor::Human,
            ts: 1,
            idempotency_key: None,
            ops: vec![Op::Reparent { id: g, parent: Some(b), order: OrderKey::first() }],
            exact: false,
        }).unwrap();
        store.tree().check_invariants().unwrap();
        assert!(store.tree().nodes_with_in(a, FacetKind::Layout).is_empty());
        assert_eq!(store.tree().nodes_with_in(b, FacetKind::Layout), vec![g]);
        assert!(!store.tree().nodes_of_type_in(a, rect_ty).contains(&r));
        assert!(store.tree().nodes_of_type_in(b, rect_ty).contains(&r));
        let derived_in_b: Vec<NodeId> = store.tree().nodes_with_in(b, FacetKind::Transform2d).into_iter().filter(|id| id.is_expanded()).collect();
        assert!(!derived_in_b.is_empty(), "derived copies under g moved to root B");

        // Refused transaction rolling back after reparent leaves scoped indexes intact.
        let bad = store.apply(TransactionInput {
            label: "bad".into(),
            actor: Actor::Human,
            ts: 2,
            idempotency_key: None,
            ops: vec![
                Op::Reparent { id: g, parent: Some(a), order: OrderKey::first() },
                Op::Delete { id: NodeId::authored(0, 999_999) },
            ],
            exact: false,
        });
        assert!(bad.is_err());
        store.tree().check_invariants().unwrap();
        assert_eq!(store.tree().nodes_with_in(b, FacetKind::Layout), vec![g]);
        assert!(store.tree().nodes_with_in(a, FacetKind::Layout).is_empty());
    }

    /// Recording tree enforces keys: reading `version()` under a recording
    /// tree fails a debug assertion (EN-1).
    #[test]
    #[should_panic(expected = "a recording tree has no key for `version`")]
    fn recording_tree_rejects_unkeyed_version() {
        let doc = Document::new();
        let rec = crate::structure::Recorder::default();
        let _ = Tree::authored_only(&doc).recording(&rec).version();
    }

    /// Choke-point canary sweep (EN-1): every mutation choke point emits keys
    /// when active and emits none when silenced via `silence_for_canary`.
    #[test]
    fn every_choke_point_canary_silences_its_keys() {
        use super::Docs;
        use crate::structure::{Key, silence_for_canary};
        let mut docs = Docs::new(Document::new(), true);
        docs.log_touched();
        let comp_ty = TypeId::lookup("composition").unwrap();
        let rect_ty = TypeId::lookup("rect").unwrap();
        let a = docs.authored.next_id();
        let b = a.offset(1);
        let r = a.offset(2);
        let copy = NodeId::from_path(&[a, r]).unwrap();
        let rel_id = docs.authored.next_relation_id();

        fn run(base: &Docs, site: &'static str, mut f: impl FnMut(&mut Docs)) -> (Vec<Key>, Vec<Key>) {
            let mut active = base.clone();
            active.log_touched();
            f(&mut active);
            let emitted = active.take_touched();

            let mut muted = base.clone();
            muted.log_touched();
            silence_for_canary(Some(site));
            f(&mut muted);
            silence_for_canary(None);
            let silenced = muted.take_touched();
            (emitted, silenced)
        }

        let (e, s) = run(&docs, "insert_node", |d| {
            d.insert_node(Node { id: a, ty: comp_ty, parent: None, order: OrderKey::first(), tombstoned: None }, vec![facet::Name::default().wrap()]);
        });
        assert!(!e.is_empty() && e.contains(&Key::Roots) && e.contains(&Key::In(a, comp_ty)), "{e:?}");
        assert!(s.is_empty(), "insert_node silenced: {s:?}");

        // Populate base nodes for remaining choke points.
        docs.insert_node(Node { id: a, ty: comp_ty, parent: None, order: OrderKey::first(), tombstoned: None }, vec![facet::Name::default().wrap()]);
        docs.insert_node(Node { id: b, ty: comp_ty, parent: None, order: OrderKey::after(&OrderKey::first()), tombstoned: None }, vec![facet::Name::default().wrap()]);
        docs.insert_node(Node { id: r, ty: rect_ty, parent: Some(a), order: OrderKey::first(), tombstoned: None }, vec![facet::Transform2d::default().wrap()]);
        docs.insert_node(Node { id: copy, ty: rect_ty, parent: Some(a), order: OrderKey::after(&OrderKey::first()), tombstoned: None }, vec![facet::Transform2d::default().wrap()]);
        let _ = docs.take_touched();

        let (e, s) = run(&docs, "set_leaf", |d| {
            let _ = d.set_leaf(r, "transform2d.x".parse().unwrap(), Slot::Const(Value::Number(42.0)));
        });
        assert!(e.contains(&Key::Leaf(Leaf { node: r, path: "transform2d.x".parse().unwrap() })), "{e:?}");
        assert!(s.is_empty(), "set_leaf silenced: {s:?}");

        let (e, s) = run(&docs, "set_order", |d| {
            d.set_order(r, OrderKey::after(&OrderKey::after(&OrderKey::first())));
        });
        assert!(e.contains(&Key::Children(a)), "{e:?}");
        assert!(s.is_empty(), "set_order silenced: {s:?}");

        let (e, s) = run(&docs, "set_parent", |d| {
            d.set_parent(r, Some(b), OrderKey::first());
        });
        assert!(e.contains(&Key::In(a, rect_ty)) && e.contains(&Key::In(b, rect_ty)), "{e:?}");
        assert!(s.is_empty(), "set_parent silenced: {s:?}");

        let (e, s) = run(&docs, "insert_facet", |d| {
            d.insert_facet(r, facet::Size::default().wrap());
        });
        assert!(e.contains(&Key::With(a, FacetKind::Size)) && e.contains(&Key::FacetSet(r)), "{e:?}");
        assert!(s.is_empty(), "insert_facet silenced: {s:?}");

        let (e, s) = run(&docs, "remove_facet", |d| {
            d.remove_facet(r, FacetKind::Transform2d);
        });
        assert!(e.contains(&Key::With(a, FacetKind::Transform2d)) && e.contains(&Key::FacetSet(r)), "{e:?}");
        assert!(s.is_empty(), "remove_facet silenced: {s:?}");

        let (e, s) = run(&docs, "tombstone", |d| {
            d.delete_node(r, Version(2));
        });
        assert!(e.contains(&Key::Node(r)) && e.contains(&Key::In(a, rect_ty)), "{e:?}");
        assert!(s.is_empty(), "tombstone silenced: {s:?}");

        let (e, s) = run(&docs, "remove_node", |d| {
            d.delete_node(copy, Version(2));
        });
        assert!(e.contains(&Key::Node(copy)) && e.contains(&Key::In(a, rect_ty)), "{e:?}");
        assert!(s.is_empty(), "remove_node silenced: {s:?}");

        let (e, s) = run(&docs, "insert_relation", |d| {
            d.insert_relation(Relation { id: rel_id, from: r, to: b, rel: crate::document::RelKind::References, data: BTreeMap::new() });
        });
        assert!(e.contains(&Key::Relation(rel_id)) && e.contains(&Key::Relations(r)) && e.contains(&Key::Relations(b)), "{e:?}");
        assert!(s.is_empty(), "insert_relation silenced: {s:?}");

        docs.insert_relation(Relation { id: rel_id, from: r, to: b, rel: crate::document::RelKind::References, data: BTreeMap::new() });
        docs.delete_node(r, Version(2));
        let _ = docs.take_touched();

        let (e, s) = run(&docs, "remove_relation", |d| {
            d.remove_relation(rel_id);
        });
        assert!(e.contains(&Key::Relation(rel_id)) && e.contains(&Key::Relations(r)), "{e:?}");
        assert!(s.is_empty(), "remove_relation silenced: {s:?}");

        let (e, s) = run(&docs, "restore_node", |d| {
            d.restore_node(r, rect_ty, Some(a), OrderKey::first(), vec![facet::Transform2d::default().wrap()]);
        });
        assert!(e.contains(&Key::Node(r)) && e.contains(&Key::In(a, rect_ty)), "{e:?}");
        assert!(s.is_empty(), "restore_node silenced: {s:?}");

        let (e, s) = run(&docs, "apply_patch", |d| {
            let p = crate::patch::Patch {
                id: crate::patch::PatchId(1),
                layer: 0,
                rank: OrderKey::first(),
                region: crate::patch::Product::default(),
                anchor: None,
                writable: true,
                entries: BTreeMap::from([(
                    crate::patch::PatchTarget { node: a, key: crate::patch::PatchKey::Hidden },
                    crate::patch::PatchValue::Bool { v: true },
                )]),
            };
            let _ = d.authored.apply_patch_op(&crate::patch::PatchOp::Add { patch: p });
        });
        assert!(e.contains(&Key::Patch(a)), "{e:?}");
        assert!(s.is_empty(), "apply_patch silenced: {s:?}");
    }
}
