//! Lean Lexical Virtual Views + minimal `ItemTable` (plans/specs/engine.md §2.3,
//! handoff-engine.md EN-13).
//!
//! Replaces materialized `Docs::copies: Document` with virtual views over shared
//! `Arc<Template>` plus sparse resolved facets (`View::sparse_facets`), a keyed
//! `ItemTable` (`items`) for `repeat`/`when` scopes, and a derived-relations
//! index. Unpatched constant facets are borrowed directly from `Arc<Template>`
//! with zero per-instance heap allocation; `reconcile_owners` diffs old vs new
//! views per top-level owner, emits exact `structure::Key`s + `ScopedIndexes`
//! updates + `Docs::gone` entries + [`ViewDelta`], and journals replaced owner
//! slices for O(touched owners) transaction rollback.

use crate::changes::{Gone, Went};
use crate::document::{Node, Relation};
use crate::engine::template::{Env, Template};
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, LeafPath};
use crate::id::{NodeId, RelationId};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::structure::{Key, Touched};
use crate::tree::{ScopedIndexes, ScopedRec};
use crate::value::Slot;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};

fn item_facet() -> &'static Facet {
    static F: OnceLock<Facet> = OnceLock::new();
    F.get_or_init(|| FacetKind::Item.default_facet())
}

fn group_ty() -> TypeId {
    TypeId::lookup("group").expect("registered")
}

/// One active repeat/when item scope (`#repeat/~key`) in the keyed `ItemTable`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ItemRecord {
    pub(crate) scope: NodeId,
    pub(crate) repeat: NodeId,
    pub(crate) key: String,
    pub(crate) order: OrderKey,
}

/// One virtual view owned by a piece (`entity`, `use`, or `item_scope`).
#[derive(Clone)]
pub(crate) struct View {
    pub(crate) owner: NodeId,
    pub(crate) top_owner: NodeId,
    pub(crate) template: Arc<Template>,
    pub(crate) env: Arc<Env>,
    /// Number of nodes from `template.nodes` admitted under `NODE_BUDGET`.
    pub(crate) limit: usize,
    /// Only `(node_idx, kind)` facets that differ from `template.nodes[node_idx]`
    /// (rewritten refs/bindings, `in`-port signal bindings, or lexical overrides)
    /// or optional facets added by overrides.
    pub(crate) sparse_facets: BTreeMap<(usize, FacetKind), Facet>,
    /// Extra optional facet kinds added by overrides for `node_idx` that are
    /// absent from `template.nodes[node_idx]`, sorted by `FacetKind`.
    pub(crate) extra_facets: BTreeMap<usize, Vec<FacetKind>>,
}

impl View {
    pub(crate) fn new(owner: NodeId, top_owner: NodeId, template: Arc<Template>, env: Arc<Env>, limit: usize) -> View {
        let (sparse_facets, extra_facets) = template.sparse_facets(&env, limit);
        View { owner, top_owner, template, env, limit, sparse_facets, extra_facets }
    }

    #[inline]
    pub(crate) fn same_as(&self, other: &View) -> bool {
        self.owner == other.owner
            && self.top_owner == other.top_owner
            && self.limit == other.limit
            && Arc::ptr_eq(&self.template, &other.template)
            && *self.env == *other.env
    }

    #[inline]
    pub(crate) fn node_id(&self, idx: usize) -> NodeId {
        let local = self.template.nodes[idx].local;
        NodeId::expanded(self.owner, local).expect("template local is authored")
    }

    #[inline]
    pub(crate) fn parent_of_idx(&self, idx: usize) -> NodeId {
        let p = self.template.nodes[idx].parent;
        if p == self.template.source_root() {
            self.owner
        } else {
            NodeId::expanded(self.owner, p).expect("template parent is authored")
        }
    }

    #[inline]
    pub(crate) fn facet_at(&self, idx: usize, kind: FacetKind) -> Option<&Facet> {
        if let Some(f) = self.sparse_facets.get(&(idx, kind)) {
            return Some(f);
        }
        self.template.nodes[idx].facet(kind)
    }

    pub(crate) fn has_at(&self, idx: usize, kind: FacetKind) -> bool {
        self.template.nodes[idx].facet(kind).is_some()
            || self.extra_facets.get(&idx).is_some_and(|ks| ks.contains(&kind))
    }

    pub(crate) fn facets_at(&self, idx: usize) -> Vec<&Facet> {
        let tnode = &self.template.nodes[idx];
        let extra = self.extra_facets.get(&idx).map(Vec::as_slice).unwrap_or(&[]);
        if extra.is_empty() {
            return tnode
                .facets
                .iter()
                .map(|f| self.sparse_facets.get(&(idx, f.kind())).unwrap_or(f))
                .collect();
        }
        // Merge sorted template facets and sorted extra override facets by `FacetKind`.
        let mut out = Vec::with_capacity(tnode.facets.len() + extra.len());
        let (mut ti, mut ei) = (0, 0);
        while ti < tnode.facets.len() && ei < extra.len() {
            let tk = tnode.facets[ti].kind();
            let ek = extra[ei];
            if tk < ek {
                out.push(self.sparse_facets.get(&(idx, tk)).unwrap_or(&tnode.facets[ti]));
                ti += 1;
            } else {
                out.push(&self.sparse_facets[&(idx, ek)]);
                ei += 1;
            }
        }
        while ti < tnode.facets.len() {
            let tk = tnode.facets[ti].kind();
            out.push(self.sparse_facets.get(&(idx, tk)).unwrap_or(&tnode.facets[ti]));
            ti += 1;
        }
        while ei < extra.len() {
            out.push(&self.sparse_facets[&(idx, extra[ei])]);
            ei += 1;
        }
        out
    }

    pub(crate) fn facet_kinds_at(&self, idx: usize) -> Vec<FacetKind> {
        let tnode = &self.template.nodes[idx];
        let extra = self.extra_facets.get(&idx).map(Vec::as_slice).unwrap_or(&[]);
        let mut out: Vec<FacetKind> = tnode.facets.iter().map(Facet::kind).chain(extra.iter().copied()).collect();
        if !extra.is_empty() {
            out.sort();
        }
        out
    }
}

/// Desired derived state produced by an owner derivation pass for one top-level
/// owner.
pub(crate) struct OwnerPlan {
    pub(crate) views: Vec<Arc<View>>,
    pub(crate) items: Vec<(NodeId, Arc<[ItemRecord]>)>,
    pub(crate) rels: Arc<[Relation]>,
}

impl Default for OwnerPlan {
    fn default() -> Self {
        OwnerPlan {
            views: Vec::new(),
            items: Vec::new(),
            rels: Arc::from([]),
        }
    }
}

/// Summary of virtual view changes produced by reconciliation, consumed by
/// `Changes` and `GeometryRevisions` without synthesizing `Op::Create`/`Op::Set`.
#[derive(Clone, Debug, Default)]
pub(crate) struct ViewDelta {
    /// Created virtual nodes: `(id, parent, depth, ty)`.
    pub(crate) created: Vec<(NodeId, NodeId, usize, TypeId)>,
    /// Removed virtual nodes: `(id, parent, depth, ty)`.
    pub(crate) removed: Vec<(NodeId, NodeId, usize, TypeId)>,
    /// Reparented or reordered virtual nodes: `(id, old_parent, new_parent)`.
    pub(crate) moved: Vec<(NodeId, NodeId, NodeId)>,
    /// Virtual leaves whose slot or facet changed.
    pub(crate) slots: Vec<Leaf>,
    /// Virtual nodes whose facets were added or removed (for geometry structure bump).
    pub(crate) facet_shaped: Vec<NodeId>,
    /// Derived relations added (preserving `RelationId`, `RelKind`, `from`, `to`).
    pub(crate) rels_added: Vec<Relation>,
    /// Derived relations removed (preserving `RelationId`, `RelKind`, `from`, `to`).
    pub(crate) rels_removed: Vec<Relation>,
}

impl ViewDelta {
    pub(crate) fn is_empty(&self) -> bool {
        self.created.is_empty()
            && self.removed.is_empty()
            && self.moved.is_empty()
            && self.slots.is_empty()
            && self.facet_shaped.is_empty()
            && self.rels_added.is_empty()
            && self.rels_removed.is_empty()
    }

    pub(crate) fn extend(&mut self, other: ViewDelta) {
        self.created.extend(other.created);
        self.removed.extend(other.removed);
        self.moved.extend(other.moved);
        self.slots.extend(other.slots);
        self.facet_shaped.extend(other.facet_shaped);
        self.rels_added.extend(other.rels_added);
        self.rels_removed.extend(other.rels_removed);
    }
}

/// Journal entry restoring one top-level owner's previous virtual state on
/// transaction rollback, and capturing first-before `Arc` snapshots for net
/// multi-wave `ViewDelta` computation in O(1).
#[derive(Clone)]
struct OwnerUndo {
    top_owner: NodeId,
    views: Vec<Arc<View>>,
    items: Vec<(NodeId, Arc<[ItemRecord]>)>,
    rels: Arc<[Relation]>,
}

/// All derived virtual views, repeat/when `ItemTable` records, and derived
/// relations for a store. Replaces `Docs::copies: Document` (`EN-13`).
#[derive(Clone, Default)]
pub(crate) struct VirtualViews {
    /// Piece owner (`entity`, `use`, or `item_scope`) -> its shared `Arc<View>`.
    pub(crate) views: BTreeMap<NodeId, Arc<View>>,
    /// Top-level authored owner -> piece owners belonging to it.
    pub(crate) by_top: BTreeMap<NodeId, Vec<NodeId>>,
    /// Keyed `ItemTable`: repeat/when `NodeId` -> shared ordered `ItemRecord` slice.
    pub(crate) items: BTreeMap<NodeId, Arc<[ItemRecord]>>,
    /// Top-level authored owner -> repeat/when ids belonging to it that have
    /// entries in `items`.
    pub(crate) items_by_top: BTreeMap<NodeId, Vec<NodeId>>,
    /// Reverse lookup: `item_scope` (`#r/~key`) -> `(repeat, index)`.
    pub(crate) item_scopes: HashMap<NodeId, (NodeId, usize)>,
    /// Derived relations indexed by id, from, to, and top-level owner.
    pub(crate) relations: BTreeMap<RelationId, Relation>,
    rels_from: BTreeMap<NodeId, Vec<RelationId>>,
    rels_to: BTreeMap<NodeId, Vec<RelationId>>,
    pub(crate) rels_by_top: BTreeMap<NodeId, Arc<[Relation]>>,
    /// Structural keys touched by view mutations since the last `take_touched`.
    pub(crate) touched: Touched,
    /// Per-transaction rollback journal (O(touched owners)).
    journal: Option<Vec<OwnerUndo>>,
    /// Optional multi-wave net delta capture: first-before snapshot + depth per
    /// touched `top_owner` across a transaction / `settle_repeats` sequence.
    net_before: Option<BTreeMap<NodeId, (OwnerUndo, usize)>>,
    pub(crate) template_diffs: TemplateDiffs,
}

impl VirtualViews {
    pub(crate) fn set_touched(&mut self, on: bool) {
        self.touched.on = on;
        self.touched.keys.clear();
    }

    pub(crate) fn touched_slice(&self) -> &[Key] {
        &self.touched.keys
    }

    pub(crate) fn take_touched(&mut self) -> Vec<Key> {
        std::mem::take(&mut self.touched.keys)
    }

    pub(crate) fn touched_empty(&self) -> bool {
        self.touched.keys.is_empty()
    }

    pub(crate) fn begin(&mut self) {
        self.journal = Some(Vec::new());
    }

    pub(crate) fn commit(&mut self) {
        self.journal = None;
    }

    pub(crate) fn rollback(&mut self) {
        self.touched.keys.clear();
        self.net_before = None;
        let Some(recs) = self.journal.take() else { return };
        for undo in recs.into_iter().rev() {
            self.clear_top_raw(undo.top_owner);
            self.install_top_raw(undo.top_owner, undo.views, undo.items, undo.rels);
        }
    }

    /// Open a net before-vs-after `ViewDelta` window if one is not already open.
    /// Returns `true` if this call opened the window (and owns calling `finish_net`).
    pub(crate) fn begin_net(&mut self) -> bool {
        if self.net_before.is_some() {
            return false;
        }
        self.net_before = Some(BTreeMap::new());
        true
    }

    /// Close the net `ViewDelta` window and compute one exact before-vs-after
    /// `ViewDelta` across all touched top-level owners (cancelling intermediate
    /// multi-wave / multi-round create+delete and relation churn).
    pub(crate) fn finish_net(&mut self) -> ViewDelta {
        let Some(before) = self.net_before.take() else {
            return ViewDelta::default();
        };
        let mut net = ViewDelta::default();
        let mut diffs = std::mem::take(&mut self.template_diffs);
        for (top_owner, (first_old, owner_depth)) in before {
            let final_now = self.snapshot_top(top_owner);
            let mut d = self.diff_top_snapshots(top_owner, owner_depth, &first_old, &final_now, &mut diffs);
            net.extend(std::mem::take(&mut d));
        }
        self.template_diffs = diffs;
        net
    }

    // ---- O(1) Virtual Read Path (0 heap allocs for facets/leaves/metadata) ----

    #[inline]
    fn locate(&self, id: NodeId) -> Option<(&View, usize)> {
        let (owner, local) = id.expanded_parts()?;
        let view = self.views.get(&owner)?;
        let &idx = view.template.local_index.get(&local)?;
        (idx < view.limit).then_some((view.as_ref(), idx))
    }

    #[inline]
    pub(crate) fn is_live(&self, id: NodeId) -> bool {
        self.item_scopes.contains_key(&id) || self.locate(id).is_some()
    }

    #[inline]
    pub(crate) fn ty_of(&self, id: NodeId) -> Option<TypeId> {
        if self.item_scopes.contains_key(&id) {
            return Some(group_ty());
        }
        let (view, idx) = self.locate(id)?;
        Some(view.template.nodes[idx].ty)
    }

    #[inline]
    pub(crate) fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        if let Some(&(repeat, _)) = self.item_scopes.get(&id) {
            return Some(repeat);
        }
        let (view, idx) = self.locate(id)?;
        Some(view.parent_of_idx(idx))
    }

    #[inline]
    pub(crate) fn order_of(&self, id: NodeId) -> Option<&OrderKey> {
        if let Some(&(repeat, idx)) = self.item_scopes.get(&id) {
            return self.items.get(&repeat).and_then(|v| v.get(idx)).map(|r| &r.order);
        }
        let (view, idx) = self.locate(id)?;
        Some(&view.template.nodes[idx].order)
    }

    pub(crate) fn node(&self, id: NodeId) -> Option<Node> {
        if let Some(&(repeat, idx)) = self.item_scopes.get(&id) {
            let rec = self.items.get(&repeat)?.get(idx)?;
            return Some(Node {
                id,
                ty: group_ty(),
                parent: Some(rec.repeat),
                order: rec.order.clone(),
                tombstoned: None,
            });
        }
        let (view, idx) = self.locate(id)?;
        let t = &view.template.nodes[idx];
        Some(Node {
            id,
            ty: t.ty,
            parent: Some(view.parent_of_idx(idx)),
            order: t.order.clone(),
            tombstoned: None,
        })
    }

    #[inline]
    pub(crate) fn facet(&self, id: NodeId, kind: FacetKind) -> Option<&Facet> {
        if self.item_scopes.contains_key(&id) {
            return (kind == FacetKind::Item).then(|| item_facet());
        }
        let (view, idx) = self.locate(id)?;
        view.facet_at(idx, kind)
    }

    #[inline]
    pub(crate) fn has(&self, id: NodeId, kind: FacetKind) -> bool {
        if self.item_scopes.contains_key(&id) {
            return kind == FacetKind::Item;
        }
        self.locate(id).is_some_and(|(view, idx)| view.has_at(idx, kind))
    }

    #[inline]
    pub(crate) fn leaf(&self, id: NodeId, path: LeafPath) -> Option<&Slot> {
        self.facet(id, path.facet)?.get(path.field)
    }

    pub(crate) fn facets_of(&self, id: NodeId) -> Vec<&Facet> {
        if self.item_scopes.contains_key(&id) {
            return vec![item_facet()];
        }
        match self.locate(id) {
            Some((view, idx)) => view.facets_at(idx),
            None => Vec::new(),
        }
    }

    /// Derived children of `id` (authored or virtual), sorted by `(order, id)`.
    pub(crate) fn derived_children(&self, id: NodeId) -> Vec<NodeId> {
        let mut out: Vec<(&OrderKey, NodeId)> = Vec::new();
        // 1. If `id` is a virtual template node, include its template children.
        if let Some((view, idx)) = self.locate(id) {
            for &ci in &view.template.nodes[idx].children {
                if ci < view.limit {
                    out.push((&view.template.nodes[ci].order, view.node_id(ci)));
                }
            }
        }
        // 2. If `id` is a piece owner (authored entity/use, nested entity/use, or item scope),
        //    include its own view's top children.
        if let Some(view) = self.views.get(&id) {
            for &ci in &view.template.top_children {
                if ci < view.limit {
                    out.push((&view.template.nodes[ci].order, view.node_id(ci)));
                }
            }
        }
        // 3. If `id` is a repeat or conditional with active items, include its item scopes.
        if let Some(recs) = self.items.get(&id) {
            for rec in recs.iter() {
                out.push((&rec.order, rec.scope));
            }
        }
        if out.len() > 1 {
            // Fast check: only sort if out of `(order, id)` order (e.g. equal-order tie or mixed sources).
            let sorted = out.windows(2).all(|w| (w[0].0, w[0].1) <= (w[1].0, w[1].1));
            if !sorted {
                out.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
            }
        }
        out.into_iter().map(|(_, n)| n).collect()
    }

    /// Every bound slot on live virtual nodes of `kind`, sorted by `NodeId`.
    pub(crate) fn bound_slots_of(&self, kind: FacetKind) -> Vec<(Leaf, &Expr)> {
        let mut out = Vec::new();
        for view in self.views.values() {
            // Template-declared bound facets (reuses sparse override/rewrite when present).
            for &(idx, k) in &view.template.bound_facets {
                if k != kind || idx >= view.limit {
                    continue;
                }
                let id = view.node_id(idx);
                if let Some(f) = view.facet_at(idx, kind) {
                    for field in f.field_names() {
                        if let Some(Slot::Bound(e)) = f.get(field) {
                            let path = LeafPath::new(kind, field).expect("declared field");
                            out.push((Leaf { node: id, path }, e));
                        }
                    }
                }
            }
        }
        out.sort_by_key(|(l, _)| *l);
        out
    }

    /// All facet kinds that may hold bound slots across live views.
    pub(crate) fn bound_facet_kinds(&self, out: &mut BTreeSet<FacetKind>) {
        for view in self.views.values() {
            for &(_, k) in &view.template.bound_facets {
                out.insert(k);
            }
        }
    }

    pub(crate) fn relation(&self, id: RelationId) -> Option<&Relation> {
        self.relations.get(&id)
    }

    pub(crate) fn relations(&self) -> impl Iterator<Item = &Relation> {
        self.relations.values()
    }

    pub(crate) fn relations_from(&self, id: NodeId) -> impl Iterator<Item = &Relation> {
        self.rels_from.get(&id).into_iter().flatten().filter_map(|rid| self.relations.get(rid))
    }

    pub(crate) fn relations_to(&self, id: NodeId) -> impl Iterator<Item = &Relation> {
        self.rels_to.get(&id).into_iter().flatten().filter_map(|rid| self.relations.get(rid))
    }

    pub(crate) fn top_owners_held(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.by_top
            .keys()
            .copied()
            .chain(self.items_by_top.keys().copied())
            .chain(self.rels_by_top.keys().copied())
    }

    // ---- Internal Raw Install / Teardown (for rollback and reconciliation) ----

    fn snapshot_top(&self, top_owner: NodeId) -> OwnerUndo {
        let views = self.by_top.get(&top_owner).into_iter().flatten().filter_map(|o| self.views.get(o).cloned()).collect();
        let items = self.items_by_top.get(&top_owner).into_iter().flatten().filter_map(|r| self.items.get(r).map(|v| (*r, Arc::clone(v)))).collect();
        let rels = self.rels_by_top.get(&top_owner).map(Arc::clone).unwrap_or_else(|| Arc::from([]));
        OwnerUndo { top_owner, views, items, rels }
    }

    fn clear_top_raw(&mut self, top_owner: NodeId) {
        self.clear_top_views(top_owner);
        self.clear_top_items(top_owner);
        self.clear_top_rels(top_owner);
    }

    fn clear_top_views(&mut self, top_owner: NodeId) {
        if let Some(owners) = self.by_top.remove(&top_owner) {
            for o in owners {
                self.views.remove(&o);
            }
        }
    }

    fn clear_top_items(&mut self, top_owner: NodeId) {
        if let Some(repeats) = self.items_by_top.remove(&top_owner) {
            for r in repeats {
                if let Some(recs) = self.items.remove(&r) {
                    for rec in recs.iter() {
                        self.item_scopes.remove(&rec.scope);
                    }
                }
            }
        }
    }

    fn clear_top_rels(&mut self, top_owner: NodeId) {
        if let Some(rels) = self.rels_by_top.remove(&top_owner) {
            for rel in rels.iter() {
                let rid = rel.id;
                if let Some(removed) = self.relations.remove(&rid) {
                    remove_rel_idx(&mut self.rels_from, removed.from, rid);
                    remove_rel_idx(&mut self.rels_to, removed.to, rid);
                }
            }
        }
    }

    fn install_top_raw(&mut self, top_owner: NodeId, views: Vec<Arc<View>>, items: Vec<(NodeId, Arc<[ItemRecord]>)>, rels: Arc<[Relation]>) {
        self.install_top_views(top_owner, views);
        self.install_top_items(top_owner, items);
        self.install_top_rels(top_owner, rels);
    }

    fn install_top_views(&mut self, top_owner: NodeId, views: Vec<Arc<View>>) {
        if !views.is_empty() {
            let owners: Vec<NodeId> = views.iter().map(|v| v.owner).collect();
            for v in views {
                self.views.insert(v.owner, v);
            }
            self.by_top.insert(top_owner, owners);
        }
    }

    fn install_top_items(&mut self, top_owner: NodeId, items: Vec<(NodeId, Arc<[ItemRecord]>)>) {
        if !items.is_empty() {
            let mut repeats = Vec::with_capacity(items.len());
            for (r, recs) in items {
                if recs.is_empty() {
                    continue;
                }
                for (idx, rec) in recs.iter().enumerate() {
                    self.item_scopes.insert(rec.scope, (r, idx));
                }
                self.items.insert(r, recs);
                repeats.push(r);
            }
            if !repeats.is_empty() {
                self.items_by_top.insert(top_owner, repeats);
            }
        }
    }

    fn install_top_rels(&mut self, top_owner: NodeId, rels: Arc<[Relation]>) {
        if !rels.is_empty() {
            for rel in rels.iter() {
                let rid = rel.id;
                insert_rel_idx(&mut self.rels_from, rel.from, rid);
                insert_rel_idx(&mut self.rels_to, rel.to, rid);
                self.relations.insert(rid, rel.clone());
            }
            self.rels_by_top.insert(top_owner, rels);
        }
    }

    // ---- Reconciliation: Diff Old vs New, Emit Keys + ScopedIndexes + Gone + ViewDelta ----

    /// Tear down every virtual view, item scope, and derived relation belonging
    /// to `top_owner` (called when `top_owner` is deleted in `Docs::delete_node`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn remove_top_owner(
        &mut self,
        top_owner: NodeId,
        top_depth: usize,
        scoped: &mut ScopedIndexes,
        scoped_journal: &mut Option<Vec<ScopedRec>>,
        scoped_touched: &mut Touched,
        gone: &mut Gone,
        gone_journal: &mut Option<Vec<(NodeId, Option<Went>)>>,
    ) -> ViewDelta {
        if !self.by_top.contains_key(&top_owner)
            && !self.items_by_top.contains_key(&top_owner)
            && !self.rels_by_top.contains_key(&top_owner)
        {
            return ViewDelta::default();
        }
        self.reconcile_top_owner(
            top_owner,
            None,
            top_depth,
            OwnerPlan::default(),
            scoped,
            scoped_journal,
            scoped_touched,
            gone,
            gone_journal,
        )
    }

    /// Reconcile `top_owner` to `plan`, updating `VirtualViews`, `ScopedIndexes`,
    /// `Docs::gone`, and touched keys in one pass. Fast-skips any unchanged
    /// `View` (`Arc::ptr_eq` + equal `Env` + equal `limit`) in O(1), diffs same-
    /// template pieces in O(|sparse_facets|), and skips unchanged item/relation
    /// index teardown via `Arc::ptr_eq`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reconcile_top_owner(
        &mut self,
        top_owner: NodeId,
        doc_root: Option<NodeId>,
        owner_depth: usize,
        plan: OwnerPlan,
        scoped: &mut ScopedIndexes,
        scoped_journal: &mut Option<Vec<ScopedRec>>,
        scoped_touched: &mut Touched,
        gone: &mut Gone,
        gone_journal: &mut Option<Vec<(NodeId, Option<Went>)>>,
    ) -> ViewDelta {
        let mut delta = ViewDelta::default();
        let old = self.snapshot_top(top_owner);
        if old.views.is_empty() && old.items.is_empty() && old.rels.is_empty()
            && plan.views.is_empty() && plan.items.is_empty() && plan.rels.is_empty()
        {
            return delta;
        }
        if let Some(j) = &mut self.journal {
            j.push(old.clone());
        }

        // Index old and new views by piece owner.
        let old_views: HashMap<NodeId, &Arc<View>> = old.views.iter().map(|v| (v.owner, v)).collect();
        let new_views: HashMap<NodeId, &Arc<View>> = plan.views.iter().map(|v| (v.owner, v)).collect();
        let old_items: HashMap<NodeId, &Arc<[ItemRecord]>> = old.items.iter().map(|(r, v)| (*r, v)).collect();
        let new_items: HashMap<NodeId, &Arc<[ItemRecord]>> = plan.items.iter().map(|(r, v)| (*r, v)).collect();

        // Fast check: if every view, item list, and relation is identical, nothing changed!
        let views_identical = old_views.len() == new_views.len()
            && old_views.iter().all(|(o, ov)| new_views.get(o).is_some_and(|nv| Arc::ptr_eq(ov, nv) || ov.same_as(nv)));
        let items_identical = old_items.len() == new_items.len()
            && old_items.iter().all(|(r, oi)| new_items.get(r).is_some_and(|ni| Arc::ptr_eq(oi, ni) || oi.as_ref() == ni.as_ref()));
        let rels_identical = Arc::ptr_eq(&old.rels, &plan.rels) || old.rels.as_ref() == plan.rels.as_ref();
        if views_identical && items_identical && rels_identical {
            // No structural or content change — pop the redundant undo entry.
            if let Some(j) = &mut self.journal {
                j.pop();
            }
            return delta;
        }
        // Record the first-before snapshot for net multi-wave `ViewDelta` once we know
        // this `top_owner` actually changed.
        if let Some(net) = &mut self.net_before {
            net.entry(top_owner).or_insert_with(|| (old.clone(), owner_depth));
        }

        // Collect node snapshots ONLY for pieces whose view or item list actually changed.
        // Unchanged pieces (`Arc::ptr_eq`) are skipped in O(1); same-template same-limit
        // pieces diff ONLY their sparse/extra facet keys in O(|sparse|)!
        let mut old_nodes: BTreeMap<NodeId, NodeView<'_>> = BTreeMap::new();
        let mut new_nodes: BTreeMap<NodeId, NodeView<'_>> = BTreeMap::new();
        let mut old_parents: HashMap<NodeId, NodeId> = HashMap::new();
        let mut new_parents: HashMap<NodeId, NodeId> = HashMap::new();
        collect_changed_nodes(
            &old.views,
            &plan.views,
            &old_views,
            &new_views,
            &old.items,
            &plan.items,
            &old_items,
            &new_items,
            &mut old_nodes,
            &mut new_nodes,
            &mut old_parents,
            &mut new_parents,
            &mut self.template_diffs,
        );

        // 1. Removed nodes (or nodes whose type changed, which are removed then created).
        let mut removed_list: Vec<(usize, NodeId, NodeId, TypeId)> = Vec::new();
        for (&id, old_n) in &old_nodes {
            if new_nodes.get(&id).is_none_or(|nn| nn.ty() != old_n.ty()) {
                let depth = depth_in(self, top_owner, owner_depth, &old_parents, id);
                removed_list.push((depth, id, old_n.parent(), old_n.ty()));
            }
        }
        removed_list.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        // Precompute created node depths while `self` is borrowed immutably.
        let mut created_list: Vec<(usize, NodeId, NodeId, TypeId)> = Vec::new();
        for (&id, new_n) in &new_nodes {
            if old_nodes.get(&id).is_none_or(|on| on.ty() != new_n.ty()) {
                let depth = depth_in(self, top_owner, owner_depth, &new_parents, id);
                created_list.push((depth, id, new_n.parent(), new_n.ty()));
            }
        }
        created_list.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        for (depth, id, parent, ty) in removed_list {
            let old_n = &old_nodes[&id];
            let prev_gone = gone.insert(id, Went { parent: Some(parent), depth });
            if let Some(gj) = gone_journal {
                gj.push((id, prev_gone));
            }
            if let Some(root) = scoped.root_of.get(&id).copied() {
                scoped.set_root(id, None, scoped_journal);
                scoped.remove_type(root, ty, id, scoped_journal, scoped_touched);
                if ty.name() == "let" {
                    scoped.remove_let(Some(parent), id, scoped_journal, scoped_touched);
                }
                for k in old_n.facet_kinds() {
                    scoped.remove_facet(root, k, id, scoped_journal, scoped_touched);
                }
            }
            note_node_keys(&mut self.touched, id, parent, &old_n.facets());
            delta.removed.push((id, parent, depth, ty));
        }

        // 2. Surviving modified nodes.
        for (&id, new_n) in &new_nodes {
            let Some(old_n) = old_nodes.get(&id).filter(|on| on.ty() == new_n.ty()) else {
                continue;
            };
            let (op, np) = (old_n.parent(), new_n.parent());
            if op != np && new_n.ty().name() == "let" {
                scoped.remove_let(Some(op), id, scoped_journal, scoped_touched);
                scoped.insert_let(Some(np), id, scoped_journal, scoped_touched);
            }
            if op != np || old_n.order() != new_n.order() {
                self.touched.note(Key::Node(id));
                self.touched.note(Key::Children(op));
                if np != op {
                    self.touched.note(Key::Children(np));
                }
                delta.moved.push((id, op, np));
            }
            let mut facet_set_changed = false;
            for k in new_n.facet_kinds() {
                let nf = new_n.facet(k).expect("listed facet");
                match old_n.facet(k) {
                    None => {
                        facet_set_changed = true;
                        if let Some(root) = doc_root {
                            scoped.insert_facet(root, k, id, scoped_journal, scoped_touched);
                        }
                        note_facet_leaves(&mut self.touched, id, nf, Some(&mut delta.slots));
                        delta.facet_shaped.push(id);
                    }
                    Some(of) if of != nf => {
                        self.touched.note(Key::Facet(id, k));
                        note_diff_fields(&mut self.touched, id, k, of, nf, &mut delta);
                    }
                    Some(_) => {}
                }
            }
            for k in old_n.facet_kinds() {
                if new_n.facet(k).is_none() {
                    facet_set_changed = true;
                    if let Some(root) = doc_root {
                        scoped.remove_facet(root, k, id, scoped_journal, scoped_touched);
                    }
                    if let Some(of) = old_n.facet(k) {
                        note_facet_leaves(&mut self.touched, id, of, Some(&mut delta.slots));
                    } else {
                        self.touched.note(Key::Facet(id, k));
                    }
                    delta.facet_shaped.push(id);
                }
            }
            if facet_set_changed {
                self.touched.note(Key::FacetSet(id));
            }
        }

        // 3. Newly created nodes (shallowest first).
        for (depth, id, parent, ty) in created_list {
            let new_n = &new_nodes[&id];
            if let Some(root) = doc_root {
                scoped.set_root(id, Some(root), scoped_journal);
                scoped.insert_type(root, ty, id, scoped_journal, scoped_touched);
                if ty.name() == "let" {
                    scoped.insert_let(Some(parent), id, scoped_journal, scoped_touched);
                }
                for k in new_n.facet_kinds() {
                    scoped.insert_facet(root, k, id, scoped_journal, scoped_touched);
                }
            }
            note_node_keys(&mut self.touched, id, parent, &new_n.facets());
            delta.created.push((id, parent, depth, ty));
        }

        // 4. Diff derived relations (skipped in O(1) when `rels_identical`).
        if !rels_identical {
            let old_rels: BTreeMap<RelationId, &Relation> = old.rels.iter().map(|r| (r.id, r)).collect();
            let new_rels: BTreeMap<RelationId, &Relation> = plan.rels.iter().map(|r| (r.id, r)).collect();
            for (&rid, &r) in old_rels.iter().filter(|(rid, r)| new_rels.get(rid) != Some(r)) {
                self.touched.note(Key::Relation(rid));
                self.touched.note(Key::Relations(r.from));
                self.touched.note(Key::Relations(r.to));
                delta.rels_removed.push(r.clone());
            }
            for (&rid, &r) in new_rels.iter().filter(|(rid, r)| old_rels.get(rid) != Some(r)) {
                self.touched.note(Key::Relation(rid));
                self.touched.note(Key::Relations(r.from));
                self.touched.note(Key::Relations(r.to));
                delta.rels_added.push(r.clone());
            }
        }

        // Release borrows of `old` and `plan` before mutating `self`.
        drop(old_nodes);
        drop(new_nodes);
        let removed_piece_owners: Vec<NodeId> = if !views_identical {
            old.views.iter().map(|v| v.owner).filter(|o| !new_views.contains_key(o)).collect()
        } else {
            Vec::new()
        };
        let changed_pieces: Vec<Arc<View>> = if !views_identical {
            plan.views
                .iter()
                .filter(|nv| old_views.get(&nv.owner).is_none_or(|ov| !Arc::ptr_eq(ov, nv)))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let new_piece_owners: Vec<NodeId> = if !views_identical {
            plan.views.iter().map(|v| v.owner).collect()
        } else {
            Vec::new()
        };
        drop(old_views);
        drop(new_views);
        drop(old_items);
        drop(new_items);

        // Install only the pieces/slices that actually changed:
        // - Update `self.views` in place for changed/added/removed piece owners (never
        //   tearing down unchanged sibling pieces in a large repeat).
        // - Skip unchanged item/relation index teardown & reinstall in O(1).
        if !views_identical {
            for o in removed_piece_owners {
                self.views.remove(&o);
            }
            for v in changed_pieces {
                self.views.insert(v.owner, v);
            }
            if new_piece_owners.is_empty() {
                self.by_top.remove(&top_owner);
            } else {
                self.by_top.insert(top_owner, new_piece_owners);
            }
        }
        if !items_identical {
            self.clear_top_items(top_owner);
            self.install_top_items(top_owner, plan.items);
        }
        if !rels_identical {
            self.clear_top_rels(top_owner);
            self.install_top_rels(top_owner, plan.rels);
        }

        delta
    }

    /// Read-only net diff between a first-before `OwnerUndo` snapshot and the
    /// final `OwnerUndo` snapshot for `top_owner` after all waves/rounds settle.
    fn diff_top_snapshots(&self, top_owner: NodeId, owner_depth: usize, old: &OwnerUndo, now: &OwnerUndo, template_diffs: &mut TemplateDiffs) -> ViewDelta {
        let mut delta = ViewDelta::default();
        let old_views: HashMap<NodeId, &Arc<View>> = old.views.iter().map(|v| (v.owner, v)).collect();
        let new_views: HashMap<NodeId, &Arc<View>> = now.views.iter().map(|v| (v.owner, v)).collect();
        let old_items: HashMap<NodeId, &Arc<[ItemRecord]>> = old.items.iter().map(|(r, v)| (*r, v)).collect();
        let new_items: HashMap<NodeId, &Arc<[ItemRecord]>> = now.items.iter().map(|(r, v)| (*r, v)).collect();

        let views_identical = old_views.len() == new_views.len()
            && old_views.iter().all(|(o, ov)| new_views.get(o).is_some_and(|nv| Arc::ptr_eq(ov, nv) || ov.same_as(nv)));
        let items_identical = old_items.len() == new_items.len()
            && old_items.iter().all(|(r, oi)| new_items.get(r).is_some_and(|ni| Arc::ptr_eq(oi, ni) || oi.as_ref() == ni.as_ref()));
        let rels_identical = Arc::ptr_eq(&old.rels, &now.rels) || old.rels.as_ref() == now.rels.as_ref();
        if views_identical && items_identical && rels_identical {
            return delta;
        }

        let mut old_nodes: BTreeMap<NodeId, NodeView<'_>> = BTreeMap::new();
        let mut new_nodes: BTreeMap<NodeId, NodeView<'_>> = BTreeMap::new();
        let mut old_parents: HashMap<NodeId, NodeId> = HashMap::new();
        let mut new_parents: HashMap<NodeId, NodeId> = HashMap::new();
        collect_changed_nodes(
            &old.views,
            &now.views,
            &old_views,
            &new_views,
            &old.items,
            &now.items,
            &old_items,
            &new_items,
            &mut old_nodes,
            &mut new_nodes,
            &mut old_parents,
            &mut new_parents,
            template_diffs,
        );

        let mut removed_list: Vec<(usize, NodeId, NodeId, TypeId)> = Vec::new();
        for (&id, old_n) in &old_nodes {
            if new_nodes.get(&id).is_none_or(|nn| nn.ty() != old_n.ty()) {
                let depth = depth_in(self, top_owner, owner_depth, &old_parents, id);
                removed_list.push((depth, id, old_n.parent(), old_n.ty()));
            }
        }
        removed_list.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        for (depth, id, parent, ty) in removed_list {
            delta.removed.push((id, parent, depth, ty));
        }

        let mut dummy = Touched::default();
        for (&id, new_n) in &new_nodes {
            let Some(old_n) = old_nodes.get(&id).filter(|on| on.ty() == new_n.ty()) else {
                continue;
            };
            let (op, np) = (old_n.parent(), new_n.parent());
            if op != np || old_n.order() != new_n.order() {
                delta.moved.push((id, op, np));
            }
            for k in new_n.facet_kinds() {
                let nf = new_n.facet(k).expect("listed facet");
                match old_n.facet(k) {
                    None => {
                        note_facet_leaves(&mut dummy, id, nf, Some(&mut delta.slots));
                        delta.facet_shaped.push(id);
                    }
                    Some(of) if of != nf => {
                        note_diff_fields(&mut dummy, id, k, of, nf, &mut delta);
                    }
                    Some(_) => {}
                }
            }
            for k in old_n.facet_kinds() {
                if new_n.facet(k).is_none() {
                    if let Some(of) = old_n.facet(k) {
                        note_facet_leaves(&mut dummy, id, of, Some(&mut delta.slots));
                    }
                    delta.facet_shaped.push(id);
                }
            }
        }

        let mut created_list: Vec<(usize, NodeId, NodeId, TypeId)> = Vec::new();
        for (&id, new_n) in &new_nodes {
            if old_nodes.get(&id).is_none_or(|on| on.ty() != new_n.ty()) {
                let depth = depth_in(self, top_owner, owner_depth, &new_parents, id);
                created_list.push((depth, id, new_n.parent(), new_n.ty()));
            }
        }
        created_list.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        for (depth, id, parent, ty) in created_list {
            delta.created.push((id, parent, depth, ty));
        }

        if !rels_identical {
            let old_rels: BTreeMap<RelationId, &Relation> = old.rels.iter().map(|r| (r.id, r)).collect();
            let new_rels: BTreeMap<RelationId, &Relation> = now.rels.iter().map(|r| (r.id, r)).collect();
            for (rid, &r) in old_rels.iter().filter(|(rid, r)| new_rels.get(rid) != Some(r)) {
                let _ = rid;
                delta.rels_removed.push(r.clone());
            }
            for (rid, &r) in new_rels.iter().filter(|(rid, r)| old_rels.get(rid) != Some(r)) {
                let _ = rid;
                delta.rels_added.push(r.clone());
            }
        }

        delta
    }
}

/// What a definition edit changed in its template (`Template::changed_nodes`),
/// by the old and new template's revision: every instance of the definition
/// gets the same pair, so it is worked out once, not once per instance (`EN-17`).
/// `off` in the differential reference (`full_views`), which walks every
/// node of a view whose template changed, so it checks this shortcut rather
/// than sharing it (council review of `EN-17`).
#[derive(Clone, Default)]
pub(crate) struct TemplateDiffs {
    diffs: HashMap<(u64, u64), Option<Arc<[usize]>>>,
    pub(crate) off: bool,
}

impl TemplateDiffs {
    /// The template nodes whose facets differ, when `old` and `new` have the
    /// same nodes; `None` when they are not two revisions of one shape.
    fn get(&mut self, old: &Arc<Template>, new: &Arc<Template>) -> Option<Arc<[usize]>> {
        if Arc::ptr_eq(old, new) || self.off {
            return None;
        }
        let diff = || old.changed_nodes(new).map(Arc::from);
        if old.rev == 0 || new.rev == 0 {
            return diff();
        }
        if self.diffs.len() > 64 {
            self.diffs.clear();
        }
        self.diffs.entry((old.rev, new.rev)).or_insert_with(diff).clone()
    }
}

/// Collect ONLY changed node snapshots between old and new piece views/items.
/// When a piece survives with the exact same `Arc<Template>` and `limit`, only
/// the union of `old.sparse_facets ∪ new.sparse_facets ∪ old.extra_facets ∪ new.extra_facets`
/// node indices is visited (O(|sparse|), never walking `0..limit`); with a
/// new template of the same shape, those and the indices it changed.
#[allow(clippy::too_many_arguments)]
fn collect_changed_nodes<'a>(
    old_view_list: &'a [Arc<View>],
    new_view_list: &'a [Arc<View>],
    old_views: &HashMap<NodeId, &'a Arc<View>>,
    new_views: &HashMap<NodeId, &'a Arc<View>>,
    old_item_list: &'a [(NodeId, Arc<[ItemRecord]>)],
    new_item_list: &'a [(NodeId, Arc<[ItemRecord]>)],
    old_items: &HashMap<NodeId, &'a Arc<[ItemRecord]>>,
    new_items: &HashMap<NodeId, &'a Arc<[ItemRecord]>>,
    old_nodes: &mut BTreeMap<NodeId, NodeView<'a>>,
    new_nodes: &mut BTreeMap<NodeId, NodeView<'a>>,
    old_parents: &mut HashMap<NodeId, NodeId>,
    new_parents: &mut HashMap<NodeId, NodeId>,
    template_diffs: &mut TemplateDiffs,
) {
    for (r, recs) in old_item_list {
        if new_items.get(r).is_some_and(|ni| Arc::ptr_eq(ni, recs) || ni.as_ref() == recs.as_ref()) {
            continue;
        }
        for rec in recs.iter() {
            old_parents.insert(rec.scope, rec.repeat);
            old_nodes.insert(rec.scope, NodeView::ItemScope(rec));
        }
    }
    for (r, recs) in new_item_list {
        if old_items.get(r).is_some_and(|oi| Arc::ptr_eq(oi, recs) || oi.as_ref() == recs.as_ref()) {
            continue;
        }
        for rec in recs.iter() {
            new_parents.insert(rec.scope, rec.repeat);
            new_nodes.insert(rec.scope, NodeView::ItemScope(rec));
        }
    }
    for ov in old_view_list {
        let nv = new_views.get(&ov.owner).copied();
        let template_changed = nv.filter(|nv| ov.limit == nv.limit).and_then(|nv| template_diffs.get(&ov.template, &nv.template));
        match nv {
            Some(nv) if Arc::ptr_eq(ov, nv) || ov.same_as(nv) => continue,
            Some(nv) if ov.limit == nv.limit && (Arc::ptr_eq(&ov.template, &nv.template) || template_changed.is_some()) => {
                // Fast sparse diff: every node `0..limit` has identical id/ty/parent/order;
                // only indices present in either view's `sparse_facets` or `extra_facets`,
                // or whose template facets changed, can differ.
                let mut touched_idxs: BTreeSet<usize> = template_changed.iter().flat_map(|c| c.iter().copied()).collect();
                touched_idxs.extend(ov.sparse_facets.keys().map(|&(i, _)| i));
                touched_idxs.extend(nv.sparse_facets.keys().map(|&(i, _)| i));
                touched_idxs.extend(ov.extra_facets.keys().copied());
                touched_idxs.extend(nv.extra_facets.keys().copied());
                for idx in touched_idxs {
                    if idx >= ov.limit {
                        continue;
                    }
                    let id = ov.node_id(idx);
                    let parent = ov.parent_of_idx(idx);
                    old_parents.insert(id, parent);
                    new_parents.insert(id, parent);
                    old_nodes.insert(id, NodeView::Template(ov.as_ref(), idx));
                    new_nodes.insert(id, NodeView::Template(nv.as_ref(), idx));
                }
            }
            _ => {
                for idx in 0..ov.limit {
                    let id = ov.node_id(idx);
                    let parent = ov.parent_of_idx(idx);
                    old_parents.insert(id, parent);
                    old_nodes.insert(id, NodeView::Template(ov.as_ref(), idx));
                }
            }
        }
    }
    for nv in new_view_list {
        match old_views.get(&nv.owner) {
            Some(&ov) if Arc::ptr_eq(ov, nv) || ov.same_as(nv) => continue,
            Some(&ov)
                if ov.limit == nv.limit
                    && (Arc::ptr_eq(&ov.template, &nv.template) || template_diffs.get(&ov.template, &nv.template).is_some()) =>
            {
                // Already handled symmetrically in the sparse fast path above.
                continue;
            }
            _ => {
                for idx in 0..nv.limit {
                    let id = nv.node_id(idx);
                    let parent = nv.parent_of_idx(idx);
                    new_parents.insert(id, parent);
                    new_nodes.insert(id, NodeView::Template(nv.as_ref(), idx));
                }
            }
        }
    }
}

fn depth_in(views: &VirtualViews, top_owner: NodeId, owner_depth: usize, parents: &HashMap<NodeId, NodeId>, mut id: NodeId) -> usize {
    let mut d = 0;
    while id != top_owner {
        if let Some(&p) = parents.get(&id) {
            d += 1;
            id = p;
        } else if let Some(p) = views.parent_of(id) {
            d += 1;
            id = p;
        } else {
            break;
        }
    }
    owner_depth + d
}

fn note_facet_leaves(touched: &mut Touched, id: NodeId, f: &Facet, mut slots: Option<&mut Vec<Leaf>>) {
    touched.note(Key::Facet(id, f.kind()));
    for field in f.field_names() {
        if let Ok(path) = LeafPath::new(f.kind(), field) {
            let leaf = Leaf { node: id, path };
            touched.note(Key::Leaf(leaf));
            if let Some(out) = slots.as_deref_mut() {
                out.push(leaf);
            }
        }
    }
}

fn note_node_keys(touched: &mut Touched, id: NodeId, parent: NodeId, facets: &[&Facet]) {
    touched.note(Key::Node(id));
    touched.note(Key::Children(parent));
    touched.note(Key::FacetSet(id));
    for f in facets {
        note_facet_leaves(touched, id, f, None);
    }
}

fn note_diff_fields(touched: &mut Touched, id: NodeId, k: FacetKind, of: &Facet, nf: &Facet, delta: &mut ViewDelta) {
    let mut fields: Vec<&'static str> = nf.field_names();
    if k.is_dynamic() {
        let mut all: BTreeSet<&'static str> = of.field_names().into_iter().collect();
        all.extend(fields);
        fields = all.into_iter().collect();
    }
    for field in fields {
        if of.get(field) != nf.get(field)
            && let Ok(path) = LeafPath::new(k, field)
        {
            let leaf = Leaf { node: id, path };
            touched.note(Key::Leaf(leaf));
            delta.slots.push(leaf);
        }
    }
}

enum NodeView<'a> {
    ItemScope(&'a ItemRecord),
    Template(&'a View, usize),
}

impl<'a> NodeView<'a> {
    fn ty(&self) -> TypeId {
        match self {
            Self::ItemScope(_) => group_ty(),
            Self::Template(v, i) => v.template.nodes[*i].ty,
        }
    }
    fn parent(&self) -> NodeId {
        match self {
            Self::ItemScope(r) => r.repeat,
            Self::Template(v, i) => v.parent_of_idx(*i),
        }
    }
    fn order(&self) -> &'a OrderKey {
        match self {
            Self::ItemScope(r) => &r.order,
            Self::Template(v, i) => &v.template.nodes[*i].order,
        }
    }
    fn facet(&self, kind: FacetKind) -> Option<&'a Facet> {
        match self {
            Self::ItemScope(_) => (kind == FacetKind::Item).then(|| item_facet()),
            Self::Template(v, i) => v.facet_at(*i, kind),
        }
    }
    fn facets(&self) -> Vec<&'a Facet> {
        match self {
            Self::ItemScope(_) => vec![item_facet()],
            Self::Template(v, i) => v.facets_at(*i),
        }
    }
    fn facet_kinds(&self) -> Vec<FacetKind> {
        match self {
            Self::ItemScope(_) => vec![FacetKind::Item],
            Self::Template(v, i) => v.facet_kinds_at(*i),
        }
    }
}

fn insert_rel_idx(map: &mut BTreeMap<NodeId, Vec<RelationId>>, node: NodeId, rid: RelationId) {
    let list = map.entry(node).or_default();
    if let Err(pos) = list.binary_search(&rid) {
        list.insert(pos, rid);
    }
}

fn remove_rel_idx(map: &mut BTreeMap<NodeId, Vec<RelationId>>, node: NodeId, rid: RelationId) {
    if let Some(list) = map.get_mut(&node) {
        if let Ok(pos) = list.binary_search(&rid) {
            list.remove(pos);
        }
        if list.is_empty() {
            map.remove(&node);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::RelKind;
    use crate::facet::{Args, Composition, Fill, Instance, LeafStruct, Name, Override, Size, Template as TemplateFacet, Transform2d, Use};
    use crate::op::Op;
    use crate::value::Value;
    use crate::{Actor, StoreCore, TransactionInput};

    fn ty(n: &str) -> TypeId {
        TypeId::lookup(n).unwrap()
    }

    fn tx(ops: Vec<Op>) -> TransactionInput {
        TransactionInput { label: "test".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
    }

    fn name(text: &str) -> Facet {
        let mut n = Name::default();
        n.text = Slot::Const(Value::Str(text.into()));
        n.wrap()
    }

    fn linked(def: NodeId) -> Facet {
        let mut i = Instance::default();
        i.def = Slot::Const(Value::Ref(def));
        i.mode = Slot::Const(Value::Enum("linked".into()));
        i.wrap()
    }

    fn use_icon() -> Facet {
        let mut u = Use::default();
        u.notation = Slot::Const(Value::Str("icon".into()));
        u.wrap()
    }

    fn tmpl_icon() -> Facet {
        let mut t = TemplateFacet::default();
        t.notation = Slot::Const(Value::Str("icon".into()));
        t.wrap()
    }

    fn override_facet(target: NodeId, leaf: &str, value: Value) -> Facet {
        let mut o = Override::default();
        o.target = Slot::Const(Value::Ref(target));
        o.leaf = Slot::Const(Value::Str(leaf.into()));
        o.value = Slot::Const(value);
        o.wrap()
    }

    fn make_def_with_rect(store: &mut StoreCore) -> (NodeId, NodeId, NodeId, NodeId) {
        let ids = store.authored().next_ids(5);
        let [lib, def, tmpl, rect, comp] = ids.try_into().unwrap();
        let mut size = Size::default();
        size.w = Slot::Const(Value::Number(40.0));
        size.h = Slot::Const(Value::Number(20.0));
        let mut fill = Fill::default();
        fill.color = Slot::Const(Value::Color([1.0, 0.0, 0.0, 1.0]));
        store
            .apply(tx(vec![
                Op::Create { id: lib, ty: ty("library"), parent: None, order: OrderKey::first(), facets: vec![name("lib")] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(lib), order: OrderKey::first(), facets: vec![name("box")] },
                Op::Create { id: tmpl, ty: ty("template"), parent: Some(def), order: OrderKey::first(), facets: vec![tmpl_icon()] },
                Op::Create {
                    id: rect,
                    ty: ty("rect"),
                    parent: Some(tmpl),
                    order: OrderKey::first(),
                    facets: vec![Transform2d::default().wrap(), size.wrap(), fill.wrap()],
                },
                Op::Create {
                    id: comp,
                    ty: ty("composition"),
                    parent: None,
                    order: OrderKey::after(&OrderKey::first()),
                    facets: vec![name("sheet"), Composition::default().wrap()],
                },
            ]))
            .unwrap();
        (def, rect, comp, lib)
    }

    #[test]
    fn virtual_views_share_template_and_borrow_unpatched_facets() {
        let mut store = StoreCore::new();
        let (def, rect, comp, _) = make_def_with_rect(&mut store);
        let ids = store.authored().next_ids(6);
        let [e1, u1, e2, u2, ov, extra_rect] = ids.try_into().unwrap();
        let rels = store.authored().next_relation_ids(2);
        let green = Value::Color([0.0, 1.0, 0.0, 1.0]);
        let mut ord = OrderKey::first();
        let mut next_ord = || {
            let cur = ord.clone();
            ord = OrderKey::after(&ord);
            cur
        };
        store
            .apply(tx(vec![
                Op::Create { id: e1, ty: ty("entity"), parent: Some(comp), order: next_ord(), facets: vec![linked(def), Args::default().wrap()] },
                Op::Create { id: u1, ty: ty("use"), parent: Some(comp), order: next_ord(), facets: vec![Transform2d::default().wrap(), use_icon()] },
                Op::Link { rel: Relation { id: rels[0], from: u1, to: e1, rel: RelKind::Figures, data: BTreeMap::new() } },
                Op::Create { id: e2, ty: ty("entity"), parent: Some(comp), order: next_ord(), facets: vec![linked(def), Args::default().wrap()] },
                Op::Create { id: u2, ty: ty("use"), parent: Some(comp), order: next_ord(), facets: vec![Transform2d::default().wrap(), use_icon()] },
                Op::Link { rel: Relation { id: rels[1], from: u2, to: e2, rel: RelKind::Figures, data: BTreeMap::new() } },
                Op::Create { id: ov, ty: ty("override"), parent: Some(u2), order: OrderKey::first(), facets: vec![override_facet(rect, "fill.color", green.clone())] },
            ]))
            .unwrap();

        let v1 = store.docs.views.views.get(&u1).expect("view 1").clone();
        let v2 = store.docs.views.views.get(&u2).expect("view 2").clone();
        assert!(Arc::ptr_eq(&v1.template, &v2.template), "instances must share one Arc<Template>");
        assert!(v1.sparse_facets.is_empty(), "unpatched constant template view should allocate zero sparse facets");

        let r1 = NodeId::expanded(u1, rect).unwrap();
        let r2 = NodeId::expanded(u2, rect).unwrap();
        let idx = v1.template.local_index[&rect];
        let tree = store.tree();
        // Zero-copy borrow directly into shared Arc<Template>.
        assert!(std::ptr::eq(
            tree.facet(r1, FacetKind::Fill).unwrap() as *const Facet,
            v1.template.nodes[idx].facet(FacetKind::Fill).unwrap() as *const Facet,
        ));
        assert!(std::ptr::eq(
            tree.facet(r1, FacetKind::Size).unwrap() as *const Facet,
            v1.template.nodes[idx].facet(FacetKind::Size).unwrap() as *const Facet,
        ));
        // Overridden instance overrides only Fill and still zero-copy borrows Size.
        assert_eq!(v2.sparse_facets.len(), 1);
        assert_eq!(
            tree.leaf(r2, LeafPath::new(FacetKind::Fill, "color").unwrap()),
            Some(&Slot::Const(green))
        );
        assert!(std::ptr::eq(
            tree.facet(r2, FacetKind::Size).unwrap() as *const Facet,
            v2.template.nodes[idx].facet(FacetKind::Size).unwrap() as *const Facet,
        ));

        // Unrelated edit leaves both Arc<View> pointers untouched in O(1).
        store
            .apply(tx(vec![Op::Create {
                id: extra_rect,
                ty: ty("rect"),
                parent: Some(comp),
                order: next_ord(),
                facets: vec![Transform2d::default().wrap(), Size::default().wrap()],
            }]))
            .unwrap();
        assert!(Arc::ptr_eq(&v1, store.docs.views.views.get(&u1).unwrap()));
        assert!(Arc::ptr_eq(&v2, store.docs.views.views.get(&u2).unwrap()));
    }

    #[test]
    fn authored_override_on_outer_use_targets_nested_virtual_rect() {
        let mut store = StoreCore::new();
        let (leaf_def, rect, comp, lib) = make_def_with_rect(&mut store);
        let ids = store.authored().next_ids(7);
        let [outer_def, inner_ent, outer_tmpl, inner_use_local, outer_ent, outer_use, ov] = ids.try_into().unwrap();
        let rels = store.authored().next_relation_ids(2);
        let red = Value::Color([1.0, 0.0, 0.0, 1.0]);
        let blue = Value::Color([0.0, 0.0, 1.0, 1.0]);
        store
            .apply(tx(vec![
                Op::Create { id: outer_def, ty: ty("definition"), parent: Some(lib), order: OrderKey::after(&OrderKey::first()), facets: vec![name("outer")] },
                Op::Create { id: inner_ent, ty: ty("entity"), parent: Some(outer_def), order: OrderKey::first(), facets: vec![linked(leaf_def), Args::default().wrap()] },
                Op::Create { id: outer_tmpl, ty: ty("template"), parent: Some(outer_def), order: OrderKey::after(&OrderKey::first()), facets: vec![tmpl_icon()] },
                Op::Create { id: inner_use_local, ty: ty("use"), parent: Some(outer_tmpl), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), use_icon()] },
                Op::Link { rel: Relation { id: rels[0], from: inner_use_local, to: inner_ent, rel: RelKind::Figures, data: BTreeMap::new() } },
                Op::Create { id: outer_ent, ty: ty("entity"), parent: Some(comp), order: OrderKey::first(), facets: vec![linked(outer_def), Args::default().wrap()] },
                Op::Create { id: outer_use, ty: ty("use"), parent: Some(comp), order: OrderKey::after(&OrderKey::first()), facets: vec![Transform2d::default().wrap(), use_icon()] },
                Op::Link { rel: Relation { id: rels[1], from: outer_use, to: outer_ent, rel: RelKind::Figures, data: BTreeMap::new() } },
            ]))
            .unwrap();

        let nested_use = NodeId::expanded(outer_use, inner_use_local).unwrap();
        let nested_rect = NodeId::expanded(nested_use, rect).unwrap();
        let color_path = LeafPath::new(FacetKind::Fill, "color").unwrap();
        assert_eq!(store.tree().leaf(nested_rect, color_path), Some(&Slot::Const(red)));

        // Override the nested virtual rect via an authored override on `outer_use`.
        let target_in_outer = NodeId::expanded(inner_use_local, rect).unwrap();
        store
            .apply(tx(vec![Op::Create {
                id: ov,
                ty: ty("override"),
                parent: Some(outer_use),
                order: OrderKey::first(),
                facets: vec![override_facet(target_in_outer, "fill.color", blue.clone())],
            }]))
            .unwrap();

        assert_eq!(store.tree().leaf(nested_rect, color_path), Some(&Slot::Const(blue)));
    }

    #[test]
    fn earlier_id_use_figuring_later_id_derived_entity_converges_when_both_dirty() {
        let mut store = StoreCore::new();
        let (def_a, rect_a, comp, lib) = make_def_with_rect(&mut store);
        let ids = store.authored().next_ids(8);
        let [def_b, tmpl_b, rect_b, outer_def, inner_ent, early_use, later_ent, ov] = ids.try_into().unwrap();
        let rel = store.authored().next_relation_ids(1)[0];
        let mut fill_b = Fill::default();
        fill_b.color = Slot::Const(Value::Color([0.0, 0.0, 1.0, 1.0]));
        let mut size_b = Size::default();
        size_b.w = Slot::Const(Value::Number(10.0));
        size_b.h = Slot::Const(Value::Number(10.0));
        let derived_inner = NodeId::expanded(later_ent, inner_ent).unwrap();
        // `early_use` has a SMALLER NodeId than `later_ent`, yet figures `later_ent`'s derived inner entity!
        assert!(early_use < later_ent);
        store
            .apply(tx(vec![
                Op::Create { id: def_b, ty: ty("definition"), parent: Some(lib), order: OrderKey::after(&OrderKey::first()), facets: vec![name("b")] },
                Op::Create { id: tmpl_b, ty: ty("template"), parent: Some(def_b), order: OrderKey::first(), facets: vec![tmpl_icon()] },
                Op::Create { id: rect_b, ty: ty("rect"), parent: Some(tmpl_b), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), size_b.wrap(), fill_b.wrap()] },
                Op::Create { id: outer_def, ty: ty("definition"), parent: Some(lib), order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![name("outer")] },
                Op::Create { id: inner_ent, ty: ty("entity"), parent: Some(outer_def), order: OrderKey::first(), facets: vec![linked(def_a), Args::default().wrap()] },
                Op::Create { id: early_use, ty: ty("use"), parent: Some(comp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), use_icon()] },
                Op::Create { id: later_ent, ty: ty("entity"), parent: Some(comp), order: OrderKey::after(&OrderKey::first()), facets: vec![linked(outer_def), Args::default().wrap()] },
                Op::Link { rel: Relation { id: rel, from: early_use, to: derived_inner, rel: RelKind::Figures, data: BTreeMap::new() } },
            ]))
            .unwrap();

        let copy_a = NodeId::expanded(early_use, rect_a).unwrap();
        let copy_b = NodeId::expanded(early_use, rect_b).unwrap();
        assert!(store.tree().is_live(copy_a));
        assert!(!store.tree().is_live(copy_b));

        // Now dirty BOTH `early_use` (via an override) AND `later_ent` (switching
        // `inner_ent` from `def_a` to `def_b`) in ONE transaction: `early_use`
        // runs before `later_ent` in wave 1, and MUST wake in wave 2 to pick up `def_b`!
        let yellow = Value::Color([1.0, 1.0, 0.0, 1.0]);
        store
            .apply(tx(vec![
                Op::Set { id: inner_ent, path: LeafPath::new(FacetKind::Instance, "def").unwrap(), slot: Slot::Const(Value::Ref(def_b)) },
                Op::Create {
                    id: ov,
                    ty: ty("override"),
                    parent: Some(early_use),
                    order: OrderKey::first(),
                    facets: vec![override_facet(rect_b, "fill.color", yellow.clone())],
                },
            ]))
            .unwrap();
        assert!(!store.tree().is_live(copy_a));
        assert!(store.tree().is_live(copy_b));
        assert_eq!(store.tree().leaf(copy_b, LeafPath::new(FacetKind::Fill, "color").unwrap()), Some(&Slot::Const(yellow)));

        // Reloading from authored document must converge to the exact same views!
        let reloaded = StoreCore::with_document(store.authored().clone());
        assert!(!reloaded.tree().is_live(copy_a));
        assert!(reloaded.tree().is_live(copy_b));
    }

    #[test]
    fn override_on_repeated_embed_cannot_close_composition_cycle() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(6);
        let [c1, c2, grp, emb, ov, _] = ids.try_into().unwrap();
        let mut rep = crate::facet::Repeat::default();
        rep.count = Slot::Const(Value::Number(1.0));
        let mut e = crate::facet::Embed::default();
        e.source = Slot::Const(Value::Ref(c2));
        store
            .apply(tx(vec![
                Op::Create { id: c1, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name("c1"), Composition::default().wrap()] },
                Op::Create { id: c2, ty: ty("composition"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![name("c2"), Composition::default().wrap()] },
                Op::Create { id: grp, ty: ty("group"), parent: Some(c1), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), rep.wrap()] },
                Op::Create { id: emb, ty: ty("embed"), parent: Some(grp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), e.wrap()] },
            ]))
            .unwrap();

        // Overriding the repeated embed's `embed.source` to its own composition `c1` must be refused!
        let item_emb = NodeId::item(grp, "0", emb).unwrap();
        assert!(store.tree().is_live(item_emb));
        let target = NodeId::from_path(&[NodeId::item_scope(grp, "0").unwrap().path()[1], emb]).unwrap();
        let res = store.apply(tx(vec![Op::Create {
            id: ov,
            ty: ty("override"),
            parent: Some(grp),
            order: OrderKey::after(&OrderKey::first()),
            facets: vec![override_facet(target, "embed.source", Value::Ref(c1))],
        }]));
        assert!(res.is_err(), "override creating an embed cycle must be rejected");
        assert_eq!(
            store.tree().leaf(item_emb, LeafPath::new(FacetKind::Embed, "source").unwrap()),
            Some(&Slot::Const(Value::Ref(c2)))
        );
    }

    #[test]
    fn rejected_session_edit_preserves_template_invalidation() {
        let mut store = StoreCore::new();
        let (def, rect, comp, _) = make_def_with_rect(&mut store);
        let ids = store.authored().next_ids(2);
        let [ent, u] = ids.try_into().unwrap();
        let rel = store.authored().next_relation_ids(1)[0];
        store
            .apply(tx(vec![
                Op::Create { id: ent, ty: ty("entity"), parent: Some(comp), order: OrderKey::first(), facets: vec![linked(def), Args::default().wrap()] },
                Op::Create { id: u, ty: ty("use"), parent: Some(comp), order: OrderKey::after(&OrderKey::first()), facets: vec![Transform2d::default().wrap(), use_icon()] },
                Op::Link { rel: Relation { id: rel, from: u, to: ent, rel: RelKind::Figures, data: BTreeMap::new() } },
            ]))
            .unwrap();

        // Rejected session op must NOT clear TemplateCache subscriptions.
        let bad_session = store.apply_session(vec![Op::Delete { id: ent }]);
        assert!(bad_session.is_err());

        // Subsequent template-only edit must still evict the cached template and update the view!
        let green = Value::Color([0.0, 1.0, 0.0, 1.0]);
        let color_path = LeafPath::new(FacetKind::Fill, "color").unwrap();
        store
            .apply(tx(vec![Op::Set { id: rect, path: color_path, slot: Slot::Const(green.clone()) }]))
            .unwrap();
        let copy = NodeId::expanded(u, rect).unwrap();
        assert_eq!(store.tree().leaf(copy, color_path), Some(&Slot::Const(green)));
    }

    #[test]
    fn deferred_repeat_embed_cycle_reports_problem_and_clears_when_hidden() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(6);
        let [c1, c2, grp, emb, ov, _] = ids.try_into().unwrap();
        let mut rep = crate::facet::Repeat::default();
        rep.count = Slot::Const(Value::Number(0.0));
        let mut e = crate::facet::Embed::default();
        e.source = Slot::Const(Value::Ref(c2));
        let item_emb = NodeId::item(grp, "0", emb).unwrap();
        let target = NodeId::from_path(&[NodeId::item_scope(grp, "0").unwrap().path()[1], emb]).unwrap();
        // Install the self-embedding override while item ~0 is absent (count=0).
        store
            .apply(tx(vec![
                Op::Create { id: c1, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name("c1"), Composition::default().wrap()] },
                Op::Create { id: c2, ty: ty("composition"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![name("c2"), Composition::default().wrap()] },
                Op::Create { id: grp, ty: ty("group"), parent: Some(c1), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), rep.wrap()] },
                Op::Create { id: emb, ty: ty("embed"), parent: Some(grp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), e.wrap()] },
                Op::Create {
                    id: ov,
                    ty: ty("override"),
                    parent: Some(grp),
                    order: OrderKey::after(&OrderKey::first()),
                    facets: vec![override_facet(target, "embed.source", Value::Ref(c1))],
                },
            ]))
            .unwrap();
        assert!(store.problems().is_empty());

        // Reveal item ~0 via repeat.count = 1 -> settle_repeats detects the
        // invalid self-embedding virtual embed and reports an expansion problem,
        // and composition::embedded suppresses direct self-recursion.
        store
            .apply(tx(vec![Op::Set {
                id: grp,
                path: LeafPath::new(FacetKind::Repeat, "count").unwrap(),
                slot: Slot::Const(Value::Number(1.0)),
            }]))
            .unwrap();
        assert!(store.problems().iter().any(|p| p.node == item_emb && p.kind == "expansion"), "{:?}", store.problems());
        assert_eq!(crate::composition::embedded(store.tree(), item_emb), None);

        // Hide item ~0 again -> problem clears automatically.
        store
            .apply(tx(vec![Op::Set {
                id: grp,
                path: LeafPath::new(FacetKind::Repeat, "count").unwrap(),
                slot: Slot::Const(Value::Number(0.0)),
            }]))
            .unwrap();
        assert!(store.problems().is_empty(), "{:?}", store.problems());
    }

    #[test]
    fn virtual_definition_target_is_ignored_without_panicking() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(5);
        let [comp, grp, vdef, eq, ent] = ids.try_into().unwrap();
        let mut rep = crate::facet::Repeat::default();
        rep.count = Slot::Const(Value::Number(1.0));
        store
            .apply(tx(vec![
                Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name("c"), Composition::default().wrap()] },
                Op::Create { id: grp, ty: ty("group"), parent: Some(comp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), rep.wrap()] },
                Op::Create { id: vdef, ty: ty("definition"), parent: Some(grp), order: OrderKey::first(), facets: vec![name("d")] },
                Op::Create { id: eq, ty: ty("equation"), parent: Some(vdef), order: OrderKey::first(), facets: vec![crate::facet::Equation::default().wrap()] },
            ]))
            .unwrap();
        let derived_def = NodeId::item(grp, "0", vdef).unwrap();
        assert!(store.tree().is_live(derived_def));
        // Pointing an authored entity at a virtual definition must not panic `Template::body`/`View::node_id`.
        let _ = store.apply(tx(vec![Op::Create {
            id: ent,
            ty: ty("entity"),
            parent: Some(comp),
            order: OrderKey::after(&OrderKey::first()),
            facets: vec![linked(derived_def), Args::default().wrap()],
        }]));
        assert!(store.tree().is_live(derived_def));
    }

    #[test]
    fn theme_maker_and_recorded_read_context_rescope_without_regraph() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(8);
        let [comp, g1, g2, let_a, let_b, rect, pal, tok] = ids.try_into().unwrap();
        let mut la = crate::facet::Let::default();
        la.provide = Slot::Const(Value::Enum("yes".into()));
        la.value = Slot::Const(Value::Number(10.0));
        let mut lb = crate::facet::Let::default();
        lb.provide = Slot::Const(Value::Enum("yes".into()));
        lb.value = Slot::Const(Value::Number(25.0));
        let mut size = Size::default();
        size.w = Slot::Bound(Expr::parse("@gap").unwrap());
        size.h = Slot::Const(Value::Number(5.0));
        let mut fill = Fill::default();
        fill.color = Slot::Bound(Expr::parse("@brand").unwrap());
        let mut tf = crate::facet::Token::default();
        let light_c = Value::Color([0.1, 0.2, 0.3, 1.0]);
        let dark_c = Value::Color([0.9, 0.8, 0.7, 1.0]);
        tf.light = Slot::Const(light_c.clone());
        tf.dark = Slot::Const(dark_c.clone());
        store
            .apply(tx(vec![
                Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name("c"), Composition::default().wrap()] },
                Op::Create { id: g1, ty: ty("group"), parent: Some(comp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap()] },
                Op::Create { id: g2, ty: ty("group"), parent: Some(comp), order: OrderKey::after(&OrderKey::first()), facets: vec![Transform2d::default().wrap()] },
                Op::Create { id: let_a, ty: ty("let"), parent: Some(g1), order: OrderKey::first(), facets: vec![name("gap"), la.wrap()] },
                Op::Create { id: let_b, ty: ty("let"), parent: Some(g2), order: OrderKey::first(), facets: vec![name("gap"), lb.wrap()] },
                Op::Create { id: rect, ty: ty("rect"), parent: Some(g1), order: OrderKey::after(&OrderKey::first()), facets: vec![Transform2d::default().wrap(), size.wrap(), fill.wrap()] },
                Op::Create { id: pal, ty: ty("palette"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![name("Theme")] },
                Op::Create { id: tok, ty: ty("token"), parent: Some(pal), order: OrderKey::first(), facets: vec![name("brand"), tf.wrap()] },
            ]))
            .unwrap();

        let w_path = LeafPath::new(FacetKind::Size, "w").unwrap();
        let c_path = LeafPath::new(FacetKind::Fill, "color").unwrap();
        assert_eq!(store.resolved(rect, w_path), Some(Value::Number(10.0)));
        assert_eq!(store.resolved(rect, c_path), Some(light_c));

        // Reparent rect from g1 to g2 -> recorded-read `@gap` wakes and switches to 25.0.
        store
            .apply(tx(vec![Op::Reparent { id: rect, parent: Some(g2), order: OrderKey::after(&OrderKey::first()) }]))
            .unwrap();
        assert_eq!(store.resolved(rect, w_path), Some(Value::Number(25.0)));

        // Switch theme to dark -> ThemeMaker publishes dark token color.
        store.set_theme(true);
        assert_eq!(store.resolved(rect, c_path), Some(dark_c));

        // Rename token away -> `@brand` binding re-resolves via changed_tokens and reports error.
        store
            .apply(tx(vec![Op::Set { id: tok, path: LeafPath::new(FacetKind::Name, "text").unwrap(), slot: Slot::Const(Value::Str("other".into())) }]))
            .unwrap();
        assert!(store.reactive().errors.contains_key(&Leaf { node: rect, path: c_path }));
    }

    #[test]
    fn indirect_repeat_embed_cycle_is_blocked_and_repairing_other_edge_clears_problem() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(8);
        let [c1, c2, c3, emb_b_to_a, grp, emb_a_to_c3, ov, _] = ids.try_into().unwrap();
        let mut e_ba = crate::facet::Embed::default();
        e_ba.source = Slot::Const(Value::Ref(c1));
        let mut e_ac = crate::facet::Embed::default();
        e_ac.source = Slot::Const(Value::Ref(c3));
        let mut rep = crate::facet::Repeat::default();
        rep.count = Slot::Const(Value::Number(0.0));
        let item_emb = NodeId::item(grp, "0", emb_a_to_c3).unwrap();
        let target = NodeId::from_path(&[NodeId::item_scope(grp, "0").unwrap().path()[1], emb_a_to_c3]).unwrap();
        // Existing edge c2 -> c1; repeat in c1 starts hidden with override targeting c2.
        store
            .apply(tx(vec![
                Op::Create { id: c1, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name("c1"), Composition::default().wrap()] },
                Op::Create { id: c2, ty: ty("composition"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![name("c2"), Composition::default().wrap()] },
                Op::Create { id: c3, ty: ty("composition"), parent: None, order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![name("c3"), Composition::default().wrap()] },
                Op::Create { id: emb_b_to_a, ty: ty("embed"), parent: Some(c2), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), e_ba.wrap()] },
                Op::Create { id: grp, ty: ty("group"), parent: Some(c1), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), rep.wrap()] },
                Op::Create { id: emb_a_to_c3, ty: ty("embed"), parent: Some(grp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), e_ac.wrap()] },
                Op::Create {
                    id: ov,
                    ty: ty("override"),
                    parent: Some(grp),
                    order: OrderKey::after(&OrderKey::first()),
                    facets: vec![override_facet(target, "embed.source", Value::Ref(c2))],
                },
            ]))
            .unwrap();
        assert!(store.problems().is_empty());

        // Reveal item ~0 -> creates indirect cycle c1 -> c2 -> c1.
        store
            .apply(tx(vec![Op::Set { id: grp, path: LeafPath::new(FacetKind::Repeat, "count").unwrap(), slot: Slot::Const(Value::Number(1.0)) }]))
            .unwrap();
        assert!(store.problems().iter().any(|p| p.node == item_emb && p.kind == "expansion"), "{:?}", store.problems());
        // Both edges of the cycle must be blocked by `composition::embedded`.
        assert_eq!(crate::composition::embedded(store.tree(), item_emb), None);
        assert_eq!(crate::composition::embedded(store.tree(), emb_b_to_a), None);

        // Repair the authored c2 -> c1 edge to c2 -> c3 -> cycle breaks and diagnostic clears immediately!
        store
            .apply(tx(vec![Op::Set { id: emb_b_to_a, path: LeafPath::new(FacetKind::Embed, "source").unwrap(), slot: Slot::Const(Value::Ref(c3)) }]))
            .unwrap();
        assert!(store.problems().is_empty(), "{:?}", store.problems());
        assert_eq!(crate::composition::embedded(store.tree(), item_emb), Some(c2));
        assert_eq!(crate::composition::embedded(store.tree(), emb_b_to_a), Some(c3));
    }

    #[test]
    fn adding_repeat_to_when_group_wakes_keyed_repeat_native() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(3);
        let [comp, grp, rect] = ids.try_into().unwrap();
        let mut when = crate::facet::When::default();
        when.is = Slot::Const(Value::Bool(true));
        store
            .apply(tx(vec![
                Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name("c"), Composition::default().wrap()] },
                Op::Create { id: grp, ty: ty("group"), parent: Some(comp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), when.wrap()] },
                Op::Create { id: rect, ty: ty("rect"), parent: Some(grp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap()] },
            ]))
            .unwrap();
        assert!(store.tree().is_live(NodeId::item(grp, "on", rect).unwrap()));

        // Add `Repeat` facet to the existing `When` group -> keyed RepeatNative must wake and expand 3 items!
        let mut rep = crate::facet::Repeat::default();
        rep.count = Slot::Const(Value::Number(3.0));
        store.apply(tx(vec![Op::AddFacet { id: grp, facet: rep.wrap() }])).unwrap();
        for k in ["0", "1", "2"] {
            assert!(store.tree().is_live(NodeId::item(grp, k, rect).unwrap()), "missing item {k}");
        }
    }

    #[test]
    fn authored_set_breaking_deref_cycle_recovers_blocked_cell() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(5);
        let [comp, ptr, x, y, z] = ids.try_into().unwrap();
        let mut lf = crate::facet::Let::default();
        lf.value = Slot::Const(Value::Ref(x));
        let mut sx = Size::default();
        sx.w = Slot::Bound(Expr::parse(&format!("deref({y}, 'size.w') + 1")).unwrap());
        let mut sy = Size::default();
        sy.w = Slot::Bound(Expr::parse(&format!("deref({ptr}.let.value, 'size.w') + 1")).unwrap());
        let mut sz = Size::default();
        sz.w = Slot::Const(Value::Number(10.0));
        store
            .apply(tx(vec![
                Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name("c"), Composition::default().wrap()] },
                Op::Create { id: ptr, ty: ty("let"), parent: Some(comp), order: OrderKey::first(), facets: vec![name("p"), lf.wrap()] },
                Op::Create { id: x, ty: ty("rect"), parent: Some(comp), order: OrderKey::after(&OrderKey::first()), facets: vec![Transform2d::default().wrap(), sx.wrap()] },
                Op::Create { id: y, ty: ty("rect"), parent: Some(comp), order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![Transform2d::default().wrap(), sy.wrap()] },
                Op::Create { id: z, ty: ty("rect"), parent: Some(comp), order: OrderKey::after(&OrderKey::after(&OrderKey::after(&OrderKey::first()))), facets: vec![Transform2d::default().wrap(), sz.wrap()] },
            ]))
            .unwrap();
        let w = LeafPath::new(FacetKind::Size, "w").unwrap();
        assert!(store.reactive().errors.contains_key(&Leaf { node: x, path: w }) || store.reactive().errors.contains_key(&Leaf { node: y, path: w }));

        // Break the cycle with an authored `Op::Set` (not a preview) retargeting `ptr` from `x` to `z`.
        store
            .apply(tx(vec![Op::Set { id: ptr, path: LeafPath::new(FacetKind::Let, "value").unwrap(), slot: Slot::Const(Value::Ref(z)) }]))
            .unwrap();
        assert!(store.reactive().errors.is_empty(), "{:?}", store.reactive().errors);
        assert_eq!(store.resolved(y, w), Some(Value::Number(11.0)));
        assert_eq!(store.resolved(x, w), Some(Value::Number(12.0)));
    }

    #[test]
    fn duplicate_theme_tokens_across_palettes_agree_after_reload() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(6);
        let [comp, rect, pal_hi_id, tok_hi_id, pal_lo_id, tok_lo_id] = ids.try_into().unwrap();
        // Put `pal_lo_id` (higher NodeId) FIRST in root order, `pal_hi_id` (lower NodeId) SECOND.
        let c_first = Value::Color([0.2, 0.4, 0.6, 1.0]);
        let c_second = Value::Color([0.8, 0.1, 0.1, 1.0]);
        let mut t1 = crate::facet::Token::default();
        t1.light = Slot::Const(c_first.clone());
        let mut t2 = crate::facet::Token::default();
        t2.light = Slot::Const(c_second.clone());
        let mut fill = Fill::default();
        fill.color = Slot::Bound(Expr::parse("@dup").unwrap());
        let first_ord = OrderKey::first();
        let second_ord = OrderKey::after(&first_ord);
        let third_ord = OrderKey::after(&second_ord);
        store
            .apply(tx(vec![
                Op::Create { id: comp, ty: ty("composition"), parent: None, order: third_ord, facets: vec![name("c"), Composition::default().wrap()] },
                Op::Create { id: rect, ty: ty("rect"), parent: Some(comp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), fill.wrap()] },
                Op::Create { id: pal_hi_id, ty: ty("palette"), parent: None, order: second_ord, facets: vec![name("P2")] },
                Op::Create { id: tok_hi_id, ty: ty("token"), parent: Some(pal_hi_id), order: OrderKey::first(), facets: vec![name("dup"), t2.wrap()] },
                Op::Create { id: pal_lo_id, ty: ty("palette"), parent: None, order: first_ord, facets: vec![name("P1")] },
                Op::Create { id: tok_lo_id, ty: ty("token"), parent: Some(pal_lo_id), order: OrderKey::first(), facets: vec![name("dup"), t1.wrap()] },
            ]))
            .unwrap();
        let c_path = LeafPath::new(FacetKind::Fill, "color").unwrap();
        let live_color = store.resolved(rect, c_path);
        assert_eq!(live_color, Some(c_first.clone()));
        let reloaded = StoreCore::with_document(store.authored().clone());
        assert_eq!(reloaded.resolved(rect, c_path), live_color);
    }

    #[test]
    fn sparse_override_on_large_piece_reuses_arcs_and_borrows_constants() {
        let mut store = StoreCore::new();
        let count = 1_000usize;
        let ids = store.authored().next_ids(8 + count);
        let [lib, def, tmpl, comp, ent, u, rep_grp, rep_rect] = ids[..8].try_into().unwrap();
        let rects = &ids[8..];
        let rel = store.authored().next_relation_ids(1)[0];
        let mut fill = Fill::default();
        fill.color = Slot::Const(Value::Color([1.0, 0.0, 0.0, 1.0]));
        let mut rep = crate::facet::Repeat::default();
        rep.count = Slot::Const(Value::Number(10.0));
        let mut ops = vec![
            Op::Create { id: lib, ty: ty("library"), parent: None, order: OrderKey::first(), facets: vec![name("lib")] },
            Op::Create { id: def, ty: ty("definition"), parent: Some(lib), order: OrderKey::first(), facets: vec![name("big")] },
            Op::Create { id: tmpl, ty: ty("template"), parent: Some(def), order: OrderKey::first(), facets: vec![tmpl_icon()] },
            Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![name("c"), Composition::default().wrap()] },
            Op::Create { id: ent, ty: ty("entity"), parent: Some(comp), order: OrderKey::first(), facets: vec![linked(def), Args::default().wrap()] },
            Op::Create { id: u, ty: ty("use"), parent: Some(comp), order: OrderKey::after(&OrderKey::first()), facets: vec![Transform2d::default().wrap(), use_icon()] },
            Op::Link { rel: Relation { id: rel, from: u, to: ent, rel: RelKind::Figures, data: BTreeMap::new() } },
            Op::Create { id: rep_grp, ty: ty("group"), parent: Some(comp), order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![Transform2d::default().wrap(), rep.wrap()] },
            Op::Create { id: rep_rect, ty: ty("rect"), parent: Some(rep_grp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap()] },
        ];
        let mut ord = OrderKey::first();
        for &r in rects {
            ops.push(Op::Create {
                id: r,
                ty: ty("rect"),
                parent: Some(tmpl),
                order: ord.clone(),
                facets: vec![Transform2d::default().wrap(), Size::default().wrap(), fill.clone().wrap()],
            });
            ord = OrderKey::after(&ord);
        }
        store.apply(tx(ops)).unwrap();

        let v_before = store.docs.views.views.get(&u).unwrap().clone();
        let rep_items_before = store.docs.views.items.get(&rep_grp).unwrap().clone();
        assert!(v_before.sparse_facets.is_empty());

        // Override 1 rect inside the 1,000-rect template piece.
        let ov = store.authored().next_id();
        let blue = Value::Color([0.0, 0.0, 1.0, 1.0]);
        store
            .apply(tx(vec![Op::Create {
                id: ov,
                ty: ty("override"),
                parent: Some(u),
                order: OrderKey::first(),
                facets: vec![override_facet(rects[500], "fill.color", blue.clone())],
            }]))
            .unwrap();

        let v_after = store.docs.views.views.get(&u).unwrap().clone();
        assert!(Arc::ptr_eq(&v_before.template, &v_after.template));
        assert_eq!(v_after.sparse_facets.len(), 1, "only the 1 overridden facet should allocate");
        // Untouched rect still borrows directly from the shared template.
        let untouched = NodeId::expanded(u, rects[0]).unwrap();
        let idx0 = v_after.template.local_index[&rects[0]];
        assert!(std::ptr::eq(
            store.tree().facet(untouched, FacetKind::Fill).unwrap() as *const Facet,
            v_after.template.nodes[idx0].facet(FacetKind::Fill).unwrap() as *const Facet,
        ));
        let overridden = NodeId::expanded(u, rects[500]).unwrap();
        assert_eq!(
            store.tree().leaf(overridden, LeafPath::new(FacetKind::Fill, "color").unwrap()),
            Some(&Slot::Const(blue))
        );
        // Sibling repeat's `Arc<[ItemRecord]>` is untouched in O(1).
        assert!(Arc::ptr_eq(&rep_items_before, store.docs.views.items.get(&rep_grp).unwrap()));
    }

    #[test]
    fn live_owner_reparented_across_compositions_keeps_subscriptions() {
        let mut store = StoreCore::new();
        let (def, rect, comp_a, _) = make_def_with_rect(&mut store);
        let ids = store.authored().next_ids(3);
        let [comp_b, ent, u] = ids.try_into().unwrap();
        let rel = store.authored().next_relation_ids(1)[0];
        store
            .apply(tx(vec![
                Op::Create {
                    id: comp_b,
                    ty: ty("composition"),
                    parent: None,
                    order: OrderKey::after(&OrderKey::after(&OrderKey::first())),
                    facets: vec![name("b"), Composition::default().wrap()],
                },
                Op::Create { id: ent, ty: ty("entity"), parent: Some(comp_a), order: OrderKey::first(), facets: vec![linked(def), Args::default().wrap()] },
                Op::Create { id: u, ty: ty("use"), parent: Some(comp_a), order: OrderKey::after(&OrderKey::first()), facets: vec![Transform2d::default().wrap(), use_icon()] },
                Op::Link { rel: Relation { id: rel, from: u, to: ent, rel: RelKind::Figures, data: BTreeMap::new() } },
            ]))
            .unwrap();

        let copy = NodeId::expanded(u, rect).unwrap();
        assert!(store.tree().is_live(copy));

        // Reparent live `use` owner from `comp_a` to `comp_b`.
        store
            .apply(tx(vec![Op::Reparent { id: u, parent: Some(comp_b), order: OrderKey::first() }]))
            .unwrap();
        assert!(store.tree().is_live(copy));
        assert_eq!(store.tree().top_root(copy), Some(comp_b));

        // Template edit must still wake the moved `use` owner!
        let green = Value::Color([0.0, 1.0, 0.0, 1.0]);
        let color_path = LeafPath::new(FacetKind::Fill, "color").unwrap();
        store
            .apply(tx(vec![Op::Set { id: rect, path: color_path, slot: Slot::Const(green.clone()) }]))
            .unwrap();
        assert_eq!(store.tree().leaf(copy, color_path), Some(&Slot::Const(green)));
    }

    #[test]
    fn embeds_inside_auto_layout_and_indirect_cycle_break_wakes_layout() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(10);
        let [comp_a, comp_b_3d, comp_c_2d, layout_grp, emb_2d, emb_3d, tail, rep_grp, emb_back, ov] = ids.try_into().unwrap();
        let mut c3d = Composition::default();
        c3d.dims = Slot::Const(Value::Enum("3d".into()));
        let mut lay = crate::facet::Layout::default();
        lay.mode = Slot::Const(Value::Enum("stack_x".into()));
        lay.gap = Slot::Const(Value::Number(10.0));
        let mut e2 = crate::facet::Embed::default();
        e2.source = Slot::Const(Value::Ref(comp_c_2d));
        let mut e3 = crate::facet::Embed::default();
        e3.source = Slot::Const(Value::Ref(comp_b_3d));
        let mut eb = crate::facet::Embed::default();
        eb.source = Slot::Const(Value::Ref(comp_c_2d));
        let mut rep = crate::facet::Repeat::default();
        rep.count = Slot::Const(Value::Number(0.0));
        let mut s_tail = Size::default();
        s_tail.w = Slot::Const(Value::Number(20.0));
        s_tail.h = Slot::Const(Value::Number(20.0));
        let target = NodeId::from_path(&[NodeId::item_scope(rep_grp, "0").unwrap().path()[1], emb_back]).unwrap();

        let k0 = OrderKey::first();
        let k1 = OrderKey::after(&k0);
        let k2 = OrderKey::after(&k1);
        store
            .apply(tx(vec![
                Op::Create { id: comp_a, ty: ty("composition"), parent: None, order: k0.clone(), facets: vec![name("a"), Composition::default().wrap()] },
                Op::Create { id: comp_b_3d, ty: ty("composition"), parent: None, order: k1.clone(), facets: vec![name("b3d"), c3d.wrap()] },
                Op::Create { id: comp_c_2d, ty: ty("composition"), parent: None, order: k2.clone(), facets: vec![name("c2d"), Composition::default().wrap()] },
                Op::Create { id: layout_grp, ty: ty("group"), parent: Some(comp_a), order: k0.clone(), facets: vec![Transform2d::default().wrap(), lay.wrap()] },
                Op::Create { id: emb_2d, ty: ty("embed"), parent: Some(layout_grp), order: k0.clone(), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), e2.wrap()] },
                Op::Create { id: emb_3d, ty: ty("embed"), parent: Some(layout_grp), order: k1.clone(), facets: vec![Transform2d::default().wrap(), e3.wrap()] },
                Op::Create { id: tail, ty: ty("rect"), parent: Some(layout_grp), order: k2.clone(), facets: vec![Transform2d::default().wrap(), s_tail.wrap()] },
                Op::Create { id: rep_grp, ty: ty("group"), parent: Some(comp_b_3d), order: k0.clone(), facets: vec![Transform2d::default().wrap(), rep.wrap()] },
                Op::Create { id: emb_back, ty: ty("embed"), parent: Some(rep_grp), order: k0.clone(), facets: vec![Transform2d::default().wrap(), eb.wrap()] },
                Op::Create {
                    id: ov,
                    ty: ty("override"),
                    parent: Some(rep_grp),
                    order: k1,
                    facets: vec![override_facet(target, "embed.source", Value::Ref(comp_a))],
                },
            ]))
            .unwrap();

        let x_path = LeafPath::new(FacetKind::Transform2d, "x").unwrap();
        let x_open = store.resolved(tail, x_path).and_then(|v| v.as_f64()).unwrap();
        // 3D embed default width is EMBED_3D_SIZE.0 (320), so `tail` sits past 320.
        assert!(x_open >= 320.0, "expected 3D embed width in layout, got tail.x={x_open}");

        // Reveal `comp_b_3d`'s repeat -> indirect cycle `comp_a -> comp_b_3d -> comp_a`.
        // `LayoutNative` wakes via recorded `Key::In(comp_b_3d, embed)` and collapses the blocked 3D embed.
        store
            .apply(tx(vec![Op::Set { id: rep_grp, path: LeafPath::new(FacetKind::Repeat, "count").unwrap(), slot: Slot::Const(Value::Number(1.0)) }]))
            .unwrap();
        assert_eq!(crate::composition::embedded(store.tree(), emb_3d), None);
        let x_blocked = store.resolved(tail, x_path).and_then(|v| v.as_f64()).unwrap();
        assert!(x_blocked < x_open - 200.0, "expected blocked 3D embed to shrink layout: open={x_open}, blocked={x_blocked}");

        // Hide `comp_b_3d`'s repeat -> cycle breaks -> `LayoutNative` automatically wakes and restores `x_open`!
        store
            .apply(tx(vec![Op::Set { id: rep_grp, path: LeafPath::new(FacetKind::Repeat, "count").unwrap(), slot: Slot::Const(Value::Number(0.0)) }]))
            .unwrap();
        assert_eq!(crate::composition::embedded(store.tree(), emb_3d), Some(comp_b_3d));
        let x_recovered = store.resolved(tail, x_path).and_then(|v| v.as_f64()).unwrap();
        assert!((x_recovered - x_open).abs() < 1e-6, "expected layout recovery to {x_open}, got {x_recovered}");
    }

    #[test]
    fn multi_wave_and_settle_repeats_emit_single_net_view_delta() {
        let mut store = StoreCore::new();
        let (def_a, rect_a, comp, lib) = make_def_with_rect(&mut store);
        let ids = store.authored().next_ids(8);
        let [def_b, tmpl_b, rect_b, outer_def, inner_ent, early_use, later_ent, _] = ids.try_into().unwrap();
        let rel = store.authored().next_relation_ids(1)[0];
        let derived_inner = NodeId::expanded(later_ent, inner_ent).unwrap();
        assert!(early_use < later_ent);
        store
            .apply(tx(vec![
                Op::Create { id: def_b, ty: ty("definition"), parent: Some(lib), order: OrderKey::after(&OrderKey::first()), facets: vec![name("b")] },
                Op::Create { id: tmpl_b, ty: ty("template"), parent: Some(def_b), order: OrderKey::first(), facets: vec![tmpl_icon()] },
                Op::Create { id: rect_b, ty: ty("ellipse"), parent: Some(tmpl_b), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), crate::facet::Radius::default().wrap()] },
                Op::Create { id: outer_def, ty: ty("definition"), parent: Some(lib), order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![name("outer")] },
                Op::Create { id: inner_ent, ty: ty("entity"), parent: Some(outer_def), order: OrderKey::first(), facets: vec![linked(def_a), Args::default().wrap()] },
                Op::Create { id: early_use, ty: ty("use"), parent: Some(comp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), use_icon()] },
                Op::Create { id: later_ent, ty: ty("entity"), parent: Some(comp), order: OrderKey::after(&OrderKey::first()), facets: vec![linked(outer_def), Args::default().wrap()] },
                Op::Link { rel: Relation { id: rel, from: early_use, to: derived_inner, rel: RelKind::Figures, data: BTreeMap::new() } },
            ]))
            .unwrap();
        let _ = store.take_changes();

        // Switch `inner_ent` from `def_a` to `def_b`: `later_ent` updates in wave 1,
        // `early_use` updates in wave 2 -> net Changes must remove `copy_a` and create `copy_b` once.
        store
            .apply(tx(vec![Op::Set {
                id: inner_ent,
                path: LeafPath::new(FacetKind::Instance, "def").unwrap(),
                slot: Slot::Const(Value::Ref(def_b)),
            }]))
            .unwrap();
        let ch = store.take_changes();
        let copy_a = NodeId::expanded(early_use, rect_a).unwrap();
        let copy_b = NodeId::expanded(early_use, rect_b).unwrap();
        assert_eq!(ch.removed.iter().filter(|(id, _)| *id == copy_a).count(), 1, "{:?}", ch.removed);
        assert_eq!(ch.created.iter().filter(|&&id| id == copy_b).count(), 1, "{:?}", ch.created);
        assert!(!ch.created.contains(&copy_a));
        assert!(!ch.removed.iter().any(|(id, _)| *id == copy_b));
        assert!(ch.geometry.structure);
    }

    #[test]
    fn undrawn_repeat_prototype_embed_does_not_block_valid_reverse_embed() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(8);
        let [comp_a, comp_b, comp_c, rep_a, proto_emb_ab, rep_b, proto_emb_bc, ov_ba] = ids.try_into().unwrap();
        let mut rep0 = crate::facet::Repeat::default();
        rep0.count = Slot::Const(Value::Number(0.0));
        let mut rep1 = crate::facet::Repeat::default();
        rep1.count = Slot::Const(Value::Number(1.0));
        let mut e_ab = crate::facet::Embed::default();
        e_ab.source = Slot::Const(Value::Ref(comp_b));
        let mut e_bc = crate::facet::Embed::default();
        e_bc.source = Slot::Const(Value::Ref(comp_c));
        let target_b = NodeId::from_path(&[NodeId::item_scope(rep_b, "0").unwrap().path()[1], proto_emb_bc]).unwrap();
        let k0 = OrderKey::first();
        let k1 = OrderKey::after(&k0);
        let k2 = OrderKey::after(&k1);
        store
            .apply(tx(vec![
                Op::Create { id: comp_a, ty: ty("composition"), parent: None, order: k0.clone(), facets: vec![name("a"), Composition::default().wrap()] },
                Op::Create { id: comp_b, ty: ty("composition"), parent: None, order: k1.clone(), facets: vec![name("b"), Composition::default().wrap()] },
                Op::Create { id: comp_c, ty: ty("composition"), parent: None, order: k2, facets: vec![name("c"), Composition::default().wrap()] },
                // A has a zero-count repeat whose undrawn prototype embeds B.
                Op::Create { id: rep_a, ty: ty("group"), parent: Some(comp_a), order: k0.clone(), facets: vec![Transform2d::default().wrap(), rep0.wrap()] },
                Op::Create { id: proto_emb_ab, ty: ty("embed"), parent: Some(rep_a), order: k0.clone(), facets: vec![Transform2d::default().wrap(), e_ab.wrap()] },
                // B has an active 1-item repeat whose item ~0 embeds A.
                Op::Create { id: rep_b, ty: ty("group"), parent: Some(comp_b), order: k0.clone(), facets: vec![Transform2d::default().wrap(), rep1.wrap()] },
                Op::Create { id: proto_emb_bc, ty: ty("embed"), parent: Some(rep_b), order: k0.clone(), facets: vec![Transform2d::default().wrap(), e_bc.wrap()] },
                Op::Create {
                    id: ov_ba,
                    ty: ty("override"),
                    parent: Some(rep_b),
                    order: k1,
                    facets: vec![override_facet(target_b, "embed.source", Value::Ref(comp_a))],
                },
            ]))
            .unwrap();

        let item_emb_ba = NodeId::item(rep_b, "0", proto_emb_bc).unwrap();
        // Because `proto_emb_ab` is an undrawn repeat prototype (count=0), B -> A is valid and NOT cyclic!
        assert_eq!(crate::composition::embedded(store.tree(), item_emb_ba), Some(comp_a));
        assert!(store.problems().is_empty(), "{:?}", store.problems());
    }

    #[test]
    fn non_edge_composition_edit_preserves_active_embed_cycle() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(7);
        let [comp_a, comp_b, comp_c, emb_ab, rep_b, proto_emb_bc, ov_ba] = ids.try_into().unwrap();
        let mut rep1 = crate::facet::Repeat::default();
        rep1.count = Slot::Const(Value::Number(1.0));
        let mut e_ab = crate::facet::Embed::default();
        e_ab.source = Slot::Const(Value::Ref(comp_b));
        let mut e_bc = crate::facet::Embed::default();
        e_bc.source = Slot::Const(Value::Ref(comp_c));
        let target_b = NodeId::from_path(&[NodeId::item_scope(rep_b, "0").unwrap().path()[1], proto_emb_bc]).unwrap();
        let k0 = OrderKey::first();
        let k1 = OrderKey::after(&k0);
        let k2 = OrderKey::after(&k1);
        store
            .apply(tx(vec![
                Op::Create { id: comp_a, ty: ty("composition"), parent: None, order: k0.clone(), facets: vec![name("a"), Composition::default().wrap()] },
                Op::Create { id: comp_b, ty: ty("composition"), parent: None, order: k1.clone(), facets: vec![name("b"), Composition::default().wrap()] },
                Op::Create { id: comp_c, ty: ty("composition"), parent: None, order: k2, facets: vec![name("c"), Composition::default().wrap()] },
                Op::Create { id: emb_ab, ty: ty("embed"), parent: Some(comp_a), order: k0.clone(), facets: vec![Transform2d::default().wrap(), e_ab.wrap()] },
                Op::Create { id: rep_b, ty: ty("group"), parent: Some(comp_b), order: k0.clone(), facets: vec![Transform2d::default().wrap(), rep1.wrap()] },
                Op::Create { id: proto_emb_bc, ty: ty("embed"), parent: Some(rep_b), order: k0.clone(), facets: vec![Transform2d::default().wrap(), e_bc.wrap()] },
                Op::Create {
                    id: ov_ba,
                    ty: ty("override"),
                    parent: Some(rep_b),
                    order: k1,
                    facets: vec![override_facet(target_b, "embed.source", Value::Ref(comp_a))],
                },
            ]))
            .unwrap();

        let item_emb_ba = NodeId::item(rep_b, "0", proto_emb_bc).unwrap();
        assert_eq!(crate::composition::embedded(store.tree(), emb_ab), None);
        assert_eq!(crate::composition::embedded(store.tree(), item_emb_ba), None);

        // Edit a non-edge Composition field on both A and B in one transaction.
        let gs = LeafPath::new(FacetKind::Composition, "grid_spacing").unwrap();
        store
            .apply(tx(vec![
                Op::Set { id: comp_a, path: gs, slot: Slot::Const(Value::Number(16.0)) },
                Op::Set { id: comp_b, path: gs, slot: Slot::Const(Value::Number(32.0)) },
            ]))
            .unwrap();

        assert_eq!(crate::composition::embedded(store.tree(), emb_ab), None);
        assert_eq!(crate::composition::embedded(store.tree(), item_emb_ba), None);
        let expected: BTreeSet<NodeId> = [comp_a].into_iter().collect();
        let sources: BTreeSet<NodeId> = [comp_b].into_iter().collect();
        assert!(crate::composition::reach(store.tree(), &[comp_a], &sources).running.is_empty());
        assert_eq!(crate::composition::shown(store.tree(), &[comp_a]), expected);
    }

    #[test]
    fn deleting_camera_refreshes_virtual_embed_problem_and_caller_geometry() {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(8);
        let [comp_a, comp_b_3d, comp_c, comp_d, cam_b, rep_a, proto_emb, emb_other] = ids.try_into().unwrap();
        let mut c3d = Composition::default();
        c3d.dims = Slot::Const(Value::Enum("3d".into()));
        let mut rep1 = crate::facet::Repeat::default();
        rep1.count = Slot::Const(Value::Number(1.0));
        let mut e_ab = crate::facet::Embed::default();
        e_ab.source = Slot::Const(Value::Ref(comp_b_3d));
        let mut e_cam = crate::facet::EmbedCamera::default();
        e_cam.camera = Slot::Const(Value::Ref(cam_b));
        let mut e_other = crate::facet::Embed::default();
        e_other.source = Slot::Const(Value::Ref(comp_d));
        let k0 = OrderKey::first();
        let k1 = OrderKey::after(&k0);
        let k2 = OrderKey::after(&k1);
        let k3 = OrderKey::after(&k2);
        store
            .apply(tx(vec![
                Op::Create { id: comp_a, ty: ty("composition"), parent: None, order: k0.clone(), facets: vec![name("a"), Composition::default().wrap()] },
                Op::Create { id: comp_b_3d, ty: ty("composition"), parent: None, order: k1.clone(), facets: vec![name("b3d"), c3d.wrap()] },
                Op::Create { id: comp_c, ty: ty("composition"), parent: None, order: k2, facets: vec![name("c"), Composition::default().wrap()] },
                Op::Create { id: comp_d, ty: ty("composition"), parent: None, order: k3, facets: vec![name("d"), Composition::default().wrap()] },
                Op::Create { id: cam_b, ty: ty("camera"), parent: Some(comp_b_3d), order: k0.clone(), facets: vec![crate::facet::Transform3d::default().wrap(), crate::facet::Camera::default().wrap()] },
                Op::Create { id: rep_a, ty: ty("group"), parent: Some(comp_a), order: k0.clone(), facets: vec![Transform2d::default().wrap(), rep1.wrap()] },
                Op::Create { id: proto_emb, ty: ty("embed"), parent: Some(rep_a), order: k0.clone(), facets: vec![Transform2d::default().wrap(), e_ab.wrap(), e_cam.wrap()] },
                Op::Create { id: emb_other, ty: ty("embed"), parent: Some(comp_c), order: k0, facets: vec![Transform2d::default().wrap(), e_other.wrap()] },
            ]))
            .unwrap();
        let _ = store.take_changes();
        assert!(store.problems().is_empty(), "{:?}", store.problems());

        // Delete `cam_b` AND change an unrelated embed edge (`emb_other` in `comp_c`) in one tx.
        store
            .apply(tx(vec![
                Op::Delete { id: cam_b },
                Op::Set { id: emb_other, path: LeafPath::new(FacetKind::Embed, "source").unwrap(), slot: Slot::Const(Value::Ref(comp_a)) },
            ]))
            .unwrap();
        let ch = store.take_changes();
        assert!(ch.geometry.shaped.contains(&Some(comp_a)), "deleting camera must mark caller comp_a shaped: {:?}", ch.geometry.shaped);
        assert!(
            store.problems().iter().any(|p| p.message.contains("embed_camera.camera")),
            "expected missing camera problem, got {:?}",
            store.problems()
        );

        // Undo restores `cam_b`, clears the problem, and marks `comp_a` shaped again.
        store.undo(1).unwrap().unwrap();
        let ch = store.take_changes();
        assert!(ch.geometry.shaped.contains(&Some(comp_a)), "restoring camera must mark caller comp_a shaped: {:?}", ch.geometry.shaped);
        assert!(store.problems().is_empty(), "{:?}", store.problems());

        // Camera-only deletion (empty `seeds` path) must also mark `comp_a` shaped and report the problem.
        store.apply(tx(vec![Op::Delete { id: cam_b }])).unwrap();
        let ch = store.take_changes();
        assert!(ch.geometry.shaped.contains(&Some(comp_a)), "camera-only delete must mark caller comp_a shaped: {:?}", ch.geometry.shaped);
        assert!(
            store.problems().iter().any(|p| p.message.contains("embed_camera.camera")),
            "expected missing camera problem on camera-only delete, got {:?}",
            store.problems()
        );
    }
}
