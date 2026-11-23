//! Virtual View Registry (engine.md §5, EN-5a): per-owner virtual views over
//! compiled [`Template`]s with sparse per-view resolved facets, cached sorted
//! virtual child lists, virtual relations, per-view scoped indexes, and
//! per-view coordinates ([`ViewCoord`]) for patch evaluation without
//! materialising copies into `Document`.

use crate::document::{Node, RelKind, Relation};
use crate::engine::template::{Env, Source, TNode, Template, Templates};
use crate::expand::{self, Problem};
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, Instance, LeafPath, Use};
use crate::id::{NodeId, RelationId};
use crate::patch::{DiscretePoint, PatchKey, PatchTarget, Point, ViewCoord};
use crate::registry::TypeId;
use crate::structure::Key;
use crate::value::Slot;
use crate::Tree;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

/// One virtual node owned by a [`ViewRecord`].
#[derive(Clone, Debug)]
pub(crate) struct ViewNode {
    /// Pre-built `Node` descriptor so `Tree::node`/`Tree::live` borrows `&Node`
    /// with zero allocations.
    pub(crate) node: Node,
    /// Index into `ViewRecord::template.nodes`.
    pub(crate) template_idx: usize,
    /// Definition-local node id copied (`template.nodes[template_idx].local`).
    #[allow(dead_code)]
    pub(crate) local: NodeId,
}

/// Effective facets of a virtual node: either borrowed straight from its
/// [`Template`] (zero heap allocations for unpatched constant nodes) or from
/// the view's sparse [`ViewRecord::resolved`] map.
pub(crate) enum VirtualFacets<'a> {
    Empty,
    Borrowed(&'a [Facet]),
    Resolved(&'a BTreeMap<FacetKind, Facet>),
}

/// One virtual view: an entity's derived body or a `use`'s derived picture/main.
#[derive(Clone)]
pub(crate) struct ViewRecord {
    /// The owner whose children this view populates (an authored or virtual `entity` or `use`).
    pub(crate) owner: NodeId,
    /// Top-level authored owner (`owner.path()[0]`).
    pub(crate) top_owner: NodeId,
    /// Top-level document root containing `top_owner` (for scoped indexes & spatial damage).
    pub(crate) spatial_root: NodeId,
    /// Compiled template shared across instances of the same definition/notation.
    pub(crate) template: Arc<Template>,
    /// Resolution environment (owner, figured entity, sized, overrides).
    #[allow(dead_code)]
    pub(crate) env: Arc<Env>,
    /// Per-view derived coordinate (placement path, enclosing scope chain, local discrete point).
    pub(crate) coord: ViewCoord,
    /// Definition instantiated by this view.
    pub(crate) def: NodeId,
    /// Active notation (for `use` pictures/mains; `None` for entity bodies).
    #[allow(dead_code)]
    pub(crate) notation: Option<String>,
    /// Virtual nodes owned directly by this view, in template (parents-first) order.
    pub(crate) nodes: Vec<ViewNode>,
    /// O(1) lookup from virtual `NodeId` to index in `nodes`.
    pub(crate) by_id: HashMap<NodeId, usize>,
    /// Cached sorted virtual child lists per parent (`owner` and virtual parents
    /// within this view), excluding absent virtual nodes, sorted by `(order, id)`.
    pub(crate) children: HashMap<NodeId, Vec<NodeId>>,
    /// Sparse per-view resolved facets: populated ONLY for virtual nodes that
    /// rewrite references, wire an `in` port arg, receive an override, or match
    /// an active authored patch. Pure constant unpatched nodes stay absent here
    /// and borrow straight from `template.nodes[idx].facets`.
    pub(crate) resolved: HashMap<NodeId, BTreeMap<FacetKind, Facet>>,
    /// Virtual nodes effectively hidden at `(global, coord)`.
    pub(crate) hidden: HashSet<NodeId>,
    /// Virtual nodes effectively absent at `(global, coord)`.
    pub(crate) absent: HashSet<NodeId>,
    /// Virtual relations produced by this view.
    pub(crate) rels: Vec<Relation>,
    pub(crate) rels_by_id: HashMap<RelationId, usize>,
    /// Per-view live type and facet indexes (excluding absent nodes).
    pub(crate) by_type: HashMap<TypeId, Vec<NodeId>>,
    pub(crate) by_facet: HashMap<FacetKind, Vec<NodeId>>,
}

impl ViewRecord {
    pub(crate) fn node(&self, id: NodeId) -> Option<&Node> {
        let &idx = self.by_id.get(&id)?;
        Some(&self.nodes[idx].node)
    }

    pub(crate) fn live(&self, id: NodeId) -> Option<&Node> {
        if self.absent.contains(&id) {
            return None;
        }
        self.node(id)
    }

    pub(crate) fn is_live(&self, id: NodeId) -> bool {
        !self.absent.contains(&id) && self.by_id.contains_key(&id)
    }

    pub(crate) fn is_hidden(&self, id: NodeId) -> bool {
        !self.absent.contains(&id) && self.hidden.contains(&id)
    }

    pub(crate) fn facet(&self, id: NodeId, kind: FacetKind) -> Option<&Facet> {
        if self.absent.contains(&id) {
            return None;
        }
        if let Some(map) = self.resolved.get(&id) {
            return map.get(&kind);
        }
        let &idx = self.by_id.get(&id)?;
        let tidx = self.nodes[idx].template_idx;
        self.template.nodes[tidx].facets.iter().find(|f| f.kind() == kind)
    }

    pub(crate) fn base_facet(&self, id: NodeId, kind: FacetKind) -> Option<&Facet> {
        let &idx = self.by_id.get(&id)?;
        let tidx = self.nodes[idx].template_idx;
        self.template.nodes[tidx].facets.iter().find(|f| f.kind() == kind)
    }

    pub(crate) fn has(&self, id: NodeId, kind: FacetKind) -> bool {
        self.facet(id, kind).is_some()
    }

    pub(crate) fn facets_of(&self, id: NodeId) -> VirtualFacets<'_> {
        if self.absent.contains(&id) {
            return VirtualFacets::Empty;
        }
        if let Some(map) = self.resolved.get(&id) {
            return VirtualFacets::Resolved(map);
        }
        let Some(&idx) = self.by_id.get(&id) else {
            return VirtualFacets::Empty;
        };
        let tidx = self.nodes[idx].template_idx;
        VirtualFacets::Borrowed(&self.template.nodes[tidx].facets)
    }

    pub(crate) fn leaf(&self, id: NodeId, path: LeafPath) -> Option<&Slot> {
        self.facet(id, path.facet)?.get(path.field)
    }

    /// Number of virtual nodes that required a sparse `resolved` allocation.
    pub(crate) fn resolved_count(&self) -> usize {
        self.resolved.len()
    }

    /// Number of virtual nodes borrowing facets straight from `Template` with
    /// zero per-view facet clones.
    pub(crate) fn borrowed_count(&self) -> usize {
        self.nodes.len().saturating_sub(self.resolved.len() + self.absent.len())
    }
}

/// Structural & spatial diff produced by [`ViewRegistry::reconcile_owner`].
#[derive(Clone, Debug, Default)]
pub(crate) struct ViewDelta {
    /// Deduplicated structural `Key`s invalidated by this view reconciliation.
    pub(crate) keys: Vec<Key>,
    /// Virtual nodes that became live/present: `(id, parent, spatial_root)`.
    pub(crate) became_present: Vec<(NodeId, Option<NodeId>, NodeId)>,
    /// Virtual nodes that disappeared or became absent: `(id, old_parent, old_spatial_root)`.
    /// Carries `old_spatial_root` so spatial scenes can invalidate the old root
    /// even after `id` is gone from `Tree` (EN-5a).
    pub(crate) became_absent: Vec<(NodeId, Option<NodeId>, NodeId)>,
    /// Virtual nodes that stayed live but whose node metadata, visibility, or
    /// effective facets changed: `(id, spatial_root)`.
    pub(crate) changed_nodes: Vec<(NodeId, NodeId)>,
}

impl ViewDelta {
    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.keys.is_empty()
            && self.became_present.is_empty()
            && self.became_absent.is_empty()
            && self.changed_nodes.is_empty()
    }
}

/// Registry of active virtual views, indexed for O(1) node/relation/child/scope
/// queries by [`Tree::with_views`].
#[derive(Clone, Default)]
pub(crate) struct ViewRegistry {
    /// Active views keyed by their direct `owner` (`entity` or `use`, authored or virtual).
    pub(crate) views: BTreeMap<NodeId, ViewRecord>,
    /// Virtual node `id` -> the `owner` of the [`ViewRecord`] containing it.
    pub(crate) node_to_view: HashMap<NodeId, NodeId>,
    /// Virtual relation `rid` -> the `owner` of the [`ViewRecord`] containing it.
    pub(crate) rel_to_view: HashMap<RelationId, NodeId>,
    /// Virtual relations leaving `from` (authored or virtual).
    rels_from: HashMap<NodeId, Vec<RelationId>>,
    /// Virtual relations arriving at `to` (authored or virtual).
    rels_to: HashMap<NodeId, Vec<RelationId>>,
    /// Top-level authored owner -> all view owners belonging to it (itself + nested).
    views_by_top: BTreeMap<NodeId, BTreeSet<NodeId>>,
    /// Top-level document root -> all view owners located under that root.
    views_by_root: BTreeMap<NodeId, BTreeSet<NodeId>>,
    /// Definition id -> top-level owners whose views read that definition.
    pub(crate) owners_by_def: BTreeMap<NodeId, BTreeSet<NodeId>>,
}

impl ViewRegistry {
    pub(crate) fn new() -> ViewRegistry {
        ViewRegistry::default()
    }

    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    pub(crate) fn view_count(&self) -> usize {
        self.views.len()
    }

    pub(crate) fn virtual_node_count(&self) -> usize {
        self.node_to_view.len()
    }

    #[allow(dead_code)]
    pub(crate) fn view(&self, owner: NodeId) -> Option<&ViewRecord> {
        self.views.get(&owner)
    }

    pub(crate) fn view_of_node(&self, id: NodeId) -> Option<&ViewRecord> {
        let owner = self.node_to_view.get(&id)?;
        self.views.get(owner)
    }

    pub(crate) fn coord_of(&self, id: NodeId) -> Option<&ViewCoord> {
        if let Some(v) = self.view_of_node(id) {
            return Some(&v.coord);
        }
        self.views.get(&id).map(|v| &v.coord)
    }

    pub(crate) fn node(&self, id: NodeId) -> Option<&Node> {
        self.view_of_node(id)?.node(id)
    }

    pub(crate) fn live(&self, id: NodeId) -> Option<&Node> {
        self.view_of_node(id)?.live(id)
    }

    pub(crate) fn is_live(&self, id: NodeId) -> bool {
        self.view_of_node(id).is_some_and(|v| v.is_live(id))
    }

    pub(crate) fn is_absent(&self, id: NodeId) -> bool {
        self.view_of_node(id).is_some_and(|v| v.absent.contains(&id))
    }

    pub(crate) fn is_hidden(&self, id: NodeId) -> bool {
        self.view_of_node(id).is_some_and(|v| v.is_hidden(id))
    }

    pub(crate) fn top_root(&self, id: NodeId) -> Option<NodeId> {
        let v = self.view_of_node(id)?;
        v.is_live(id).then_some(v.spatial_root)
    }

    pub(crate) fn facet(&self, id: NodeId, kind: FacetKind) -> Option<&Facet> {
        self.view_of_node(id)?.facet(id, kind)
    }

    pub(crate) fn base_facet(&self, id: NodeId, kind: FacetKind) -> Option<&Facet> {
        self.view_of_node(id)?.base_facet(id, kind)
    }

    pub(crate) fn has(&self, id: NodeId, kind: FacetKind) -> bool {
        self.view_of_node(id).is_some_and(|v| v.has(id, kind))
    }

    pub(crate) fn facets_of(&self, id: NodeId) -> VirtualFacets<'_> {
        match self.view_of_node(id) {
            Some(v) => v.facets_of(id),
            None => VirtualFacets::Empty,
        }
    }

    pub(crate) fn leaf(&self, id: NodeId, path: LeafPath) -> Option<&Slot> {
        self.view_of_node(id)?.leaf(id, path)
    }

    /// Virtual children of `parent` (either an authored owner or a virtual node),
    /// borrowing the cached slice with zero allocations in the common case.
    pub(crate) fn children_of(&self, parent: NodeId) -> Cow<'_, [NodeId]> {
        if self.is_absent(parent) {
            return Cow::Borrowed(&[]);
        }
        let own = self.views.get(&parent).and_then(|v| v.children.get(&parent)).map(Vec::as_slice).unwrap_or(&[]);
        let in_outer = self
            .node_to_view
            .get(&parent)
            .filter(|&&outer| outer != parent)
            .and_then(|outer| self.views.get(outer))
            .and_then(|v| v.children.get(&parent))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        match (in_outer.is_empty(), own.is_empty()) {
            (true, true) => Cow::Borrowed(&[]),
            (false, true) => Cow::Borrowed(in_outer),
            (true, false) => Cow::Borrowed(own),
            (false, false) => {
                let mut merged = Vec::with_capacity(in_outer.len() + own.len());
                let (mut a, mut b) = (in_outer.iter().peekable(), own.iter().peekable());
                let key = |id: NodeId| {
                    let n = self.node(id).expect("virtual child present");
                    (&n.order, id)
                };
                while let (Some(&&x), Some(&&y)) = (a.peek(), b.peek()) {
                    if key(x) <= key(y) {
                        merged.push(x);
                        a.next();
                    } else {
                        merged.push(y);
                        b.next();
                    }
                }
                merged.extend(a.copied());
                merged.extend(b.copied());
                Cow::Owned(merged)
            }
        }
    }

    pub(crate) fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.views.values().flat_map(|v| v.nodes.iter().map(|vn| &vn.node))
    }

    pub(crate) fn live_nodes(&self) -> impl Iterator<Item = &Node> {
        self.views.values().flat_map(|v| {
            v.nodes
                .iter()
                .filter(|vn| !v.absent.contains(&vn.node.id))
                .map(|vn| &vn.node)
        })
    }

    pub(crate) fn expanded_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.views.values().flat_map(|v| v.nodes.iter().map(|vn| vn.node.id))
    }

    pub(crate) fn nodes_of_type(&self, ty: TypeId) -> impl Iterator<Item = NodeId> + '_ {
        self.views.values().flat_map(move |v| v.by_type.get(&ty).into_iter().flatten().copied())
    }

    pub(crate) fn nodes_with(&self, kind: FacetKind) -> impl Iterator<Item = NodeId> + '_ {
        self.views.values().flat_map(move |v| v.by_facet.get(&kind).into_iter().flatten().copied())
    }

    pub(crate) fn nodes_of_type_in(&self, root: NodeId, ty: TypeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        if let Some(owners) = self.views_by_root.get(&root) {
            for owner in owners {
                if let Some(v) = self.views.get(owner)
                    && let Some(ids) = v.by_type.get(&ty)
                {
                    out.extend_from_slice(ids);
                }
            }
        }
        out
    }

    pub(crate) fn nodes_with_in(&self, root: NodeId, kind: FacetKind) -> Vec<NodeId> {
        let mut out = Vec::new();
        if let Some(owners) = self.views_by_root.get(&root) {
            for owner in owners {
                if let Some(v) = self.views.get(owner)
                    && let Some(ids) = v.by_facet.get(&kind)
                {
                    out.extend_from_slice(ids);
                }
            }
        }
        out
    }

    pub(crate) fn bound_slots(&self) -> Vec<(Leaf, &Expr)> {
        let mut out = Vec::new();
        for v in self.views.values() {
            for vn in &v.nodes {
                let id = vn.node.id;
                if v.absent.contains(&id) {
                    continue;
                }
                match v.facets_of(id) {
                    VirtualFacets::Empty => {}
                    VirtualFacets::Borrowed(slice) => {
                        for f in slice {
                            for field in f.field_names() {
                                if let Some(e) = f.get(field).and_then(Slot::expr) {
                                    out.push((Leaf { node: id, path: LeafPath::new(f.kind(), field).expect("declared") }, e));
                                }
                            }
                        }
                    }
                    VirtualFacets::Resolved(map) => {
                        for f in map.values() {
                            for field in f.field_names() {
                                if let Some(e) = f.get(field).and_then(Slot::expr) {
                                    out.push((Leaf { node: id, path: LeafPath::new(f.kind(), field).expect("declared") }, e));
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }

    pub(crate) fn relation(&self, id: RelationId) -> Option<&Relation> {
        let owner = self.rel_to_view.get(&id)?;
        let v = self.views.get(owner)?;
        let &idx = v.rels_by_id.get(&id)?;
        v.rels.get(idx)
    }

    pub(crate) fn relations(&self) -> impl Iterator<Item = &Relation> {
        self.views.values().flat_map(|v| v.rels.iter())
    }

    pub(crate) fn relations_from(&self, id: NodeId) -> impl Iterator<Item = &Relation> {
        self.rels_from
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|rid| self.relation(*rid))
    }

    pub(crate) fn relations_to(&self, id: NodeId) -> impl Iterator<Item = &Relation> {
        self.rels_to
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|rid| self.relation(*rid))
    }

    /// Insert one built `ViewRecord` into all lookup indexes.
    fn insert_view(&mut self, view: ViewRecord) {
        let owner = view.owner;
        let top = view.top_owner;
        let root = view.spatial_root;
        let def = view.def;
        for vn in &view.nodes {
            self.node_to_view.insert(vn.node.id, owner);
        }
        for r in &view.rels {
            self.rel_to_view.insert(r.id, owner);
            self.rels_from.entry(r.from).or_default().push(r.id);
            self.rels_to.entry(r.to).or_default().push(r.id);
        }
        self.views_by_top.entry(top).or_default().insert(owner);
        self.views_by_root.entry(root).or_default().insert(owner);
        self.owners_by_def.entry(def).or_default().insert(top);
        self.views.insert(owner, view);
    }

    /// Remove and return every `ViewRecord` owned by `top_owner` (itself + nested).
    #[allow(dead_code)]
    fn remove_top_owner(&mut self, top_owner: NodeId) -> Vec<ViewRecord> {
        let Some(owners) = self.views_by_top.remove(&top_owner) else {
            return Vec::new();
        };
        let mut removed = Vec::with_capacity(owners.len());
        for owner in owners {
            if let Some(view) = self.views.remove(&owner) {
                for vn in &view.nodes {
                    self.node_to_view.remove(&vn.node.id);
                }
                for r in &view.rels {
                    self.rel_to_view.remove(&r.id);
                    if let Some(list) = self.rels_from.get_mut(&r.from) {
                        list.retain(|&x| x != r.id);
                        if list.is_empty() {
                            self.rels_from.remove(&r.from);
                        }
                    }
                    if let Some(list) = self.rels_to.get_mut(&r.to) {
                        list.retain(|&x| x != r.id);
                        if list.is_empty() {
                            self.rels_to.remove(&r.to);
                        }
                    }
                }
                if let Some(set) = self.views_by_root.get_mut(&view.spatial_root) {
                    set.remove(&owner);
                    if set.is_empty() {
                        self.views_by_root.remove(&view.spatial_root);
                    }
                }
                removed.push(view);
            }
        }
        removed
    }

    /// Build a complete static [`ViewRegistry`] for all authored `entity` and
    /// `use` owners in `doc` at `global` (`EN-5a` dual-run oracle & baseline).
    ///
    /// Entities are expanded first so any `use` figuring a derived inner entity
    /// can read its definition cleanly; nested entities and inner `use`s expand
    /// recursively within `DEPTH_MAX` and `NODE_BUDGET`.
    pub(crate) fn build_static(
        doc: Tree<'_>,
        templates: &mut Templates<'_>,
        global: &Point,
        local_points: Option<&HashMap<NodeId, DiscretePoint>>,
    ) -> (ViewRegistry, Vec<Problem>) {
        let mut reg = ViewRegistry::new();
        let mut problems = Vec::new();
        let mut budget = expand::NODE_BUDGET;

        let entity_ty = TypeId::lookup("entity").expect("registered");
        let use_ty = TypeId::lookup("use").expect("registered");

        // Pass 1: all authored linked entities (in id order).
        let mut entities: Vec<NodeId> = doc
            .nodes_of_type(entity_ty)
            .filter(|&id| !id.is_expanded() && doc.is_live(id))
            .collect();
        entities.sort();
        for owner in entities {
            let local = local_points.and_then(|m| m.get(&owner)).cloned().unwrap_or_default();
            let built = build_owner_views(doc, &reg, templates, owner, global, &local, &mut budget, &mut problems);
            for v in built {
                reg.insert_view(v);
            }
        }

        // Pass 2: all authored `use` owners (in id order).
        let mut uses: Vec<NodeId> = doc
            .nodes_of_type(use_ty)
            .filter(|&id| !id.is_expanded() && doc.is_live(id))
            .collect();
        uses.sort();
        for owner in uses {
            let local = local_points.and_then(|m| m.get(&owner)).cloned().unwrap_or_default();
            let built = build_owner_views(doc, &reg, templates, owner, global, &local, &mut budget, &mut problems);
            for v in built {
                reg.insert_view(v);
            }
        }

        expand::sort_problems(&mut problems);
        (reg, problems)
    }

    /// Rebuild every view belonging to top-level authored `owner` at `global`,
    /// returning the exact [`ViewDelta`] (structural keys, `became_present`,
    /// `became_absent` with old spatial roots, and `changed_nodes`).
    #[allow(dead_code)]
    pub(crate) fn reconcile_owner(
        &mut self,
        doc: Tree<'_>,
        templates: &mut Templates<'_>,
        owner: NodeId,
        global: &Point,
        local: &DiscretePoint,
    ) -> (ViewDelta, Vec<Problem>) {
        let old_views = self.remove_top_owner(owner);
        let mut problems = Vec::new();
        let used = self.virtual_node_count();
        let mut budget = expand::NODE_BUDGET.saturating_sub(used);
        let new_views = build_owner_views(doc, self, templates, owner, global, local, &mut budget, &mut problems);
        let delta = diff_views(&old_views, &new_views);
        for v in new_views {
            self.insert_view(v);
        }
        expand::sort_problems(&mut problems);
        (delta, problems)
    }
}

/// Effective notation of `use` node `u` at `(global, coord)` (checking
/// `PatchKey::Notation` over `Use::notation`).
fn effective_notation(
    doc: Tree<'_>,
    global: &Point,
    coord: &ViewCoord,
    u: NodeId,
    local_u: Option<NodeId>,
    facets_override: Option<&BTreeMap<FacetKind, Facet>>,
    borrowed_facets: Option<&[Facet]>,
) -> Option<String> {
    let pt = doc.patches();
    if !pt.patches.is_empty() {
        let mut best: Option<(&crate::patch::Patch, PatchTarget)> = None;
        for cand in std::iter::once(u).chain(local_u.filter(|&l| l != u)) {
            let target = PatchTarget { node: cand, key: PatchKey::Notation };
            if let Some(p) = pt.winner_authored_at_view(global, coord, target)
                && best.is_none_or(|(b, _)| pt.compare_at_view(p, b, coord) == std::cmp::Ordering::Greater)
            {
                best = Some((p, target));
            }
        }
        if let Some((p, target)) = best
            && let Some(n) = p.entries.get(&target).and_then(|v| v.as_notation())
        {
            return Some(n.to_owned());
        }
    }
    if let Some(map) = facets_override {
        return map.get(&FacetKind::Use).and_then(|f| match f {
            Facet::Use(x) => x.notation.constant().and_then(|v| v.as_str()).map(str::to_owned),
            _ => None,
        });
    }
    if let Some(slice) = borrowed_facets {
        return slice.iter().find_map(|f| match f {
            Facet::Use(x) => x.notation.constant().and_then(|v| v.as_str()).map(str::to_owned),
            _ => None,
        });
    }
    doc.get::<Use>(u)
        .and_then(|x| x.notation.constant())
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

fn authored_root_of(doc: Tree<'_>, id: NodeId) -> NodeId {
    let mut cur = id;
    while let Some(p) = doc.node(cur).and_then(|n| n.parent) {
        cur = p;
    }
    cur
}

/// Resolve linked definition of an authored or already-expanded virtual entity.
fn resolve_entity_def(doc: Tree<'_>, reg: &ViewRegistry, local_views: &[ViewRecord], entity: NodeId) -> Option<NodeId> {
    if !entity.is_expanded() {
        return expand::linked_definition(doc, entity);
    }
    let inst_facet = local_views
        .iter()
        .find_map(|v| v.facet(entity, FacetKind::Instance))
        .or_else(|| reg.facet(entity, FacetKind::Instance))?;
    let Facet::Instance(inst) = inst_facet else { return None };
    if inst.mode.constant().and_then(|v| v.as_str()) != Some("linked") {
        return None;
    }
    let def = inst.def.constant()?.as_ref()?;
    doc.live(def).filter(|n| n.ty.name() == "definition").map(|_| def)
}

/// Resolve the entity figured by authored `use` `u` (authored or virtual entity).
fn resolve_use_entity(doc: Tree<'_>, reg: &ViewRegistry, u: NodeId) -> Option<NodeId> {
    doc.relations_from(u)
        .find(|r| {
            r.rel == RelKind::Figures
                && if r.to.is_expanded() { reg.is_live(r.to) } else { doc.is_live(r.to) }
        })
        .map(|r| r.to)
}

/// Build all views (top + nested) for one top-level authored owner (`entity` or `use`).
#[allow(clippy::too_many_arguments)]
fn build_owner_views(
    doc: Tree<'_>,
    reg: &ViewRegistry,
    templates: &mut Templates<'_>,
    top_owner: NodeId,
    global: &Point,
    local_dp: &DiscretePoint,
    budget: &mut usize,
    problems: &mut Vec<Problem>,
) -> Vec<ViewRecord> {
    let Some(node) = doc.live(top_owner).filter(|_| !top_owner.is_expanded()) else {
        return Vec::new();
    };
    let spatial_root = authored_root_of(doc, top_owner);
    let mut out = Vec::new();
    match node.ty.name() {
        "entity" => {
            if let Some(def) = expand::linked_definition(doc, top_owner) {
                let coord = ViewCoord { path: vec![top_owner], scopes: vec![def], local: local_dp.clone() };
                build_entity_view(doc, templates, top_owner, top_owner, spatial_root, def, coord, global, budget, problems, &mut out);
            }
        }
        "use" => {
            let sized = doc.facet(top_owner, FacetKind::Size).is_some();
            if let Some(def) = expand::main_definition(doc, top_owner) {
                let coord = ViewCoord { path: vec![top_owner], scopes: vec![def], local: local_dp.clone() };
                if let Some(notation) = effective_notation(doc, global, &coord, top_owner, None, None, None) {
                    build_use_view(doc, reg, templates, top_owner, top_owner, spatial_root, sized, None, def, &notation, coord, global, budget, problems, &mut out);
                }
            } else if let Some(entity) = resolve_use_entity(doc, reg, top_owner)
                && let Some(def) = resolve_entity_def(doc, reg, &out, entity)
            {
                let coord = ViewCoord { path: vec![top_owner], scopes: vec![def], local: local_dp.clone() };
                if let Some(notation) = effective_notation(doc, global, &coord, top_owner, None, None, None) {
                    build_use_view(doc, reg, templates, top_owner, top_owner, spatial_root, sized, Some(entity), def, &notation, coord, global, budget, problems, &mut out);
                }
            }
        }
        _ => {}
    }
    out
}

fn check_depth(owner: NodeId, problems: &mut Vec<Problem>) -> bool {
    let depth = owner.nesting() + 1;
    if depth > expand::DEPTH_MAX {
        problems.push(Problem {
            node: owner,
            kind: "depth_blocked",
            message: format!("nested more than {} deep: not expanded (does something contain itself?)", expand::DEPTH_MAX),
        });
        return false;
    }
    if depth == expand::DEPTH_REPORTED + 1 {
        problems.push(Problem {
            node: owner,
            kind: "depth",
            message: format!("nested more than {} deep", expand::DEPTH_REPORTED),
        });
    }
    true
}

/// Evaluate whether a virtual node `(copy_id, local)` has any active authored
/// patch at `(global, coord)` and, if so, compute its presence, visibility,
/// and optional facet/leaf overlays.
struct NodePatchState {
    present: bool,
    hidden: bool,
    has_facet_or_leaf_patch: bool,
    opt_facets: BTreeMap<FacetKind, bool>,
    leaf_patches: Vec<(LeafPath, Slot)>,
}

fn eval_virtual_patches(
    doc: Tree<'_>,
    global: &Point,
    coord: &ViewCoord,
    copy_id: NodeId,
    local: NodeId,
    parent_absent: bool,
    parent_hidden: bool,
) -> NodePatchState {
    let pt = doc.patches();
    if pt.patches.is_empty() {
        return NodePatchState {
            present: !parent_absent,
            hidden: !parent_absent && parent_hidden,
            has_facet_or_leaf_patch: false,
            opt_facets: BTreeMap::new(),
            leaf_patches: Vec::new(),
        };
    }

    // Winner helper across both `copy_id` (specific virtual target) and `local`
    // (definition-local target matched by `coord`).
    let winner_for = |key: PatchKey| -> Option<(&crate::patch::Patch, PatchTarget)> {
        let mut best: Option<(&crate::patch::Patch, PatchTarget)> = None;
        for cand in [copy_id, local] {
            let target = PatchTarget { node: cand, key };
            if let Some(p) = pt.winner_authored_at_view(global, coord, target)
                && best.is_none_or(|(b, _)| pt.compare_at_view(p, b, coord) == std::cmp::Ordering::Greater)
            {
                best = Some((p, target));
            }
        }
        best
    };

    let present = if parent_absent {
        false
    } else if let Some((p, t)) = winner_for(PatchKey::Present)
        && p.entries.get(&t).and_then(|v| v.as_bool()) == Some(false)
    {
        false
    } else {
        true
    };

    let hidden = if !present {
        false
    } else if parent_hidden {
        true
    } else if let Some((p, t)) = winner_for(PatchKey::Hidden)
        && p.entries.get(&t).and_then(|v| v.as_bool()) == Some(true)
    {
        true
    } else {
        false
    };

    if !present {
        return NodePatchState {
            present: false,
            hidden: false,
            has_facet_or_leaf_patch: false,
            opt_facets: BTreeMap::new(),
            leaf_patches: Vec::new(),
        };
    }

    let mut fk_seen: BTreeSet<FacetKind> = BTreeSet::new();
    let mut lp_seen: BTreeSet<LeafPath> = BTreeSet::new();
    for cand in [copy_id, local] {
        for p in pt.patches_for_target(cand) {
            if p.layer >= crate::patch::SESSION_LAYER {
                continue;
            }
            if !p.region.contains_at_view(&pt.axes, global, coord, p.anchor) {
                continue;
            }
            for t in p.entries.keys() {
                if t.node != cand {
                    continue;
                }
                match t.key {
                    PatchKey::FacetPresent(fk) => {
                        fk_seen.insert(fk);
                    }
                    PatchKey::Leaf(lp) => {
                        lp_seen.insert(lp);
                    }
                    _ => {}
                }
            }
        }
    }

    let mut opt_facets = BTreeMap::new();
    for fk in fk_seen {
        if let Some((p, t)) = winner_for(PatchKey::FacetPresent(fk))
            && let Some(on) = p.entries.get(&t).and_then(|v| v.as_bool())
        {
            opt_facets.insert(fk, on);
        }
    }

    let mut leaf_patches = Vec::new();
    for lp in lp_seen {
        if let Some((p, t)) = winner_for(PatchKey::Leaf(lp))
            && let Some(slot) = p.entries.get(&t).and_then(|v| v.as_slot())
        {
            leaf_patches.push((lp, slot.clone()));
        }
    }

    let has_facet_or_leaf_patch = !opt_facets.is_empty() || !leaf_patches.is_empty();
    NodePatchState { present, hidden, has_facet_or_leaf_patch, opt_facets, leaf_patches }
}

/// Whether `env` (or an item's outer env) has any override targeting `tnode.target`.
fn env_overrides_node(n: &TNode, env: &Env) -> bool {
    env.overrides.iter().any(|(target, _, _)| *target == n.target)
}

/// Compute sparse resolved facets for virtual node `i`, returning `None` when
/// the node can borrow directly from `template.nodes[i].facets` with 0 clones!
fn resolve_virtual_facets(
    doc: Tree<'_>,
    template: &Template,
    i: usize,
    env: &Env,
    coord: &ViewCoord,
    patch: &NodePatchState,
) -> Option<BTreeMap<FacetKind, Facet>> {
    let n = &template.nodes[i];
    // If `coord.local` overrides an axis locally and `n.local` has any patch in
    // the table, rebuild from unpatched base facets so a local point can
    // un-match a global patch.
    let local_resets_base = !coord.local.is_empty()
        && doc.patches().patches_for_target(n.local).next().is_some();

    let needs_resolved = n.rewrite
        || n.arg.is_some()
        || env_overrides_node(n, env)
        || patch.has_facet_or_leaf_patch
        || local_resets_base;
    if !needs_resolved {
        return None;
    }

    let mut map: BTreeMap<FacetKind, Facet> = if local_resets_base && !n.rewrite && n.arg.is_none() && !env_overrides_node(n, env) {
        doc.base_facets_of(n.local).map(|f| (f.kind(), f.clone())).collect()
    } else {
        let mut base_vec = template.materialise(i, env);
        if local_resets_base {
            // Re-seed unmodified slots from unpatched base before applying patches at `coord`.
            let base_map: BTreeMap<FacetKind, Facet> = doc.base_facets_of(n.local).map(|f| (f.kind(), f.clone())).collect();
            for f in &mut base_vec {
                if let Some(bf) = base_map.get(&f.kind()) {
                    for field in f.field_names() {
                        if let Some(Slot::Const(_)) = f.get(field)
                            && let Some(bs) = bf.get(field)
                            && !env.overrides.iter().any(|(t, lp, _)| *t == n.target && lp.facet == f.kind() && lp.field == field)
                        {
                            let _ = f.set(field, bs.clone());
                        }
                    }
                }
            }
        }
        base_vec.into_iter().map(|f| (f.kind(), f)).collect()
    };

    for (&fk, &on) in &patch.opt_facets {
        if on {
            if !map.contains_key(&fk) && n.ty.spec().allows(fk) {
                map.insert(fk, fk.default_facet());
            }
        } else {
            map.remove(&fk);
        }
    }
    for (lp, slot) in &patch.leaf_patches {
        if let Some(f) = map.get_mut(&lp.facet) {
            let _ = f.set(lp.field, slot.clone());
        }
    }
    Some(map)
}

/// Finalize per-view cached child lists, relation map, and type/facet indexes.
fn finalize_view(mut view: ViewRecord) -> ViewRecord {
    // Build cached child lists for live (non-absent) virtual nodes.
    let mut children: HashMap<NodeId, Vec<(crate::order::OrderKey, NodeId)>> = HashMap::new();
    let mut by_type: HashMap<TypeId, Vec<NodeId>> = HashMap::new();
    let mut by_facet: HashMap<FacetKind, Vec<NodeId>> = HashMap::new();
    for vn in &view.nodes {
        let id = vn.node.id;
        if view.absent.contains(&id) {
            continue;
        }
        if let Some(p) = vn.node.parent {
            children.entry(p).or_default().push((vn.node.order.clone(), id));
        }
        by_type.entry(vn.node.ty).or_default().push(id);
        match view.facets_of(id) {
            VirtualFacets::Empty => {}
            VirtualFacets::Borrowed(slice) => {
                for f in slice {
                    by_facet.entry(f.kind()).or_default().push(id);
                }
            }
            VirtualFacets::Resolved(map) => {
                for &k in map.keys() {
                    by_facet.entry(k).or_default().push(id);
                }
            }
        }
    }
    for (p, mut list) in children {
        list.sort();
        view.children.insert(p, list.into_iter().map(|(_, id)| id).collect());
    }
    for list in by_type.values_mut() {
        list.sort();
    }
    for list in by_facet.values_mut() {
        list.sort();
    }
    view.by_type = by_type;
    view.by_facet = by_facet;
    for (i, r) in view.rels.iter().enumerate() {
        view.rels_by_id.insert(r.id, i);
    }
    view
}

/// Build an entity's body view and recursively expand its linked inner entities.
#[allow(clippy::too_many_arguments)]
fn build_entity_view(
    doc: Tree<'_>,
    templates: &mut Templates<'_>,
    entity: NodeId,
    top_owner: NodeId,
    spatial_root: NodeId,
    def: NodeId,
    coord: ViewCoord,
    global: &Point,
    budget: &mut usize,
    problems: &mut Vec<Problem>,
    out: &mut Vec<ViewRecord>,
) {
    if *budget == 0 || !check_depth(entity, problems) {
        return;
    }
    let t = templates.body(doc, def);
    let env = Arc::new(Env {
        owner: entity,
        entity: Some(entity),
        sized: false,
        item: None,
        overrides: templates.overrides(doc, entity),
    });

    let mut nodes = Vec::with_capacity(t.nodes.len());
    let mut by_id = HashMap::with_capacity(t.nodes.len());
    let mut resolved = HashMap::new();
    let mut hidden = HashSet::new();
    let mut absent = HashSet::new();
    let mut rels = Vec::new();
    let mut nested_entities: Vec<(NodeId, NodeId, NodeId)> = Vec::new();

    for (i, n) in t.nodes.iter().enumerate() {
        let parent = if n.parent == def { Some(entity) } else { NodeId::expanded(entity, n.parent) };
        let (Some(id), Some(parent)) = (NodeId::expanded(entity, n.local), parent) else { continue };
        if *budget == 0 {
            problems.push(Problem {
                node: parent,
                kind: "budget",
                message: format!("the document derives more than {} nodes; the rest are left out, from {id} on", expand::NODE_BUDGET),
            });
            break;
        }
        *budget -= 1;

        let parent_absent = absent.contains(&parent);
        let parent_hidden = hidden.contains(&parent);
        let patch = eval_virtual_patches(doc, global, &coord, id, n.local, parent_absent, parent_hidden);
        if !patch.present {
            absent.insert(id);
        } else if patch.hidden {
            hidden.insert(id);
        }

        let res_map = if patch.present {
            resolve_virtual_facets(doc, &t, i, &env, &coord, &patch)
        } else {
            None
        };

        // Check if this is a linked inner entity to expand recursively.
        if patch.present && n.top && n.ty.name() == "entity" {
            let inst_facet = match &res_map {
                Some(m) => m.get(&FacetKind::Instance),
                None => n.facets.iter().find(|f| f.kind() == FacetKind::Instance),
            };
            if let Some(Facet::Instance(inst)) = inst_facet
                && inst.mode.constant().and_then(|v| v.as_str()) == Some("linked")
                && let Some(inner_def) = inst.def.constant().and_then(|v| v.as_ref())
                && doc.live(inner_def).is_some_and(|dn| dn.ty.name() == "definition")
            {
                nested_entities.push((id, n.local, inner_def));
            }
        }

        if let Some(map) = res_map {
            resolved.insert(id, map);
        }

        by_id.insert(id, nodes.len());
        nodes.push(ViewNode {
            node: Node { id, ty: n.ty, parent: Some(parent), order: n.order.clone(), tombstoned: None },
            template_idx: i,
            local: n.local,
        });

        if patch.present {
            for r in &n.rels {
                let to = t.follow_node(&env, r.to);
                rels.push(Relation {
                    id: RelationId::expanded(entity, r.id),
                    from: id,
                    to,
                    rel: r.rel,
                    data: r.data.clone(),
                });
            }
        }
    }

    let view = finalize_view(ViewRecord {
        owner: entity,
        top_owner,
        spatial_root,
        template: t,
        env,
        coord: coord.clone(),
        def,
        notation: None,
        nodes,
        by_id,
        children: HashMap::new(),
        resolved,
        hidden,
        absent,
        rels,
        rels_by_id: HashMap::new(),
        by_type: HashMap::new(),
        by_facet: HashMap::new(),
    });
    out.push(view);

    for (inner_id, inner_local, inner_def) in nested_entities {
        let mut inner_path = coord.path.clone();
        inner_path.push(inner_local);
        let mut inner_scopes = coord.scopes.clone();
        inner_scopes.push(inner_def);
        let inner_coord = ViewCoord { path: inner_path, scopes: inner_scopes, local: coord.local.clone() };
        build_entity_view(doc, templates, inner_id, top_owner, spatial_root, inner_def, inner_coord, global, budget, problems, out);
    }
}

/// Definition linked by definition-local entity `e` (`expand::linked_def_of_node`).
fn linked_def_of_node(doc: Tree<'_>, e: NodeId) -> Option<NodeId> {
    doc.node(e)
        .and_then(|_| doc.get::<Instance>(e))
        .and_then(|i| {
            (i.mode.constant().and_then(|v| v.as_str()) == Some("linked"))
                .then(|| i.def.constant().and_then(|v| v.as_ref()))
                .flatten()
        })
        .filter(|&d| doc.live(d).is_some_and(|n| n.ty.name() == "definition"))
}

/// Build a `use`'s picture/main view and recursively expand its inner `use`s
/// (and any inline non-repeat-template `entity` / non-`own` `use` scopes).
#[allow(clippy::too_many_arguments)]
fn build_use_view(
    doc: Tree<'_>,
    reg: &ViewRegistry,
    templates: &mut Templates<'_>,
    u: NodeId,
    top_owner: NodeId,
    spatial_root: NodeId,
    sized: bool,
    entity: Option<NodeId>,
    def: NodeId,
    notation: &str,
    coord: ViewCoord,
    global: &Point,
    budget: &mut usize,
    problems: &mut Vec<Problem>,
    out: &mut Vec<ViewRecord>,
) {
    if *budget == 0 || !check_depth(u, problems) {
        return;
    }
    let Some(t) = templates.picture(doc, def, notation) else { return };
    let Source::Picture(template) = t.source else { return };
    let env = Arc::new(Env {
        owner: u,
        entity,
        sized,
        item: None,
        overrides: templates.overrides(doc, u),
    });

    let mut nodes = Vec::with_capacity(t.nodes.len());
    let mut by_id = HashMap::with_capacity(t.nodes.len());
    let mut resolved = HashMap::new();
    let mut hidden = HashSet::new();
    let mut absent = HashSet::new();
    let mut rels = Vec::new();
    let mut in_repeat: HashSet<NodeId> = HashSet::new();
    // (inner_u_id, inner_local, inner_sized, inner_entity, inner_def, inner_notation)
    let mut inner_uses: Vec<(NodeId, NodeId, bool, NodeId, NodeId, String)> = Vec::new();
    // Non-repeat-template inline entities inside the picture template.
    let mut inline_entities: Vec<(NodeId, NodeId, NodeId)> = Vec::new();
    // Non-own uses inside the picture template (e.g. figuring an inline entity or authored entity).
    let mut other_uses: Vec<(NodeId, NodeId, bool, NodeId, String)> = Vec::new();

    for (i, n) in t.nodes.iter().enumerate() {
        let parent = if n.parent == template { Some(u) } else { NodeId::expanded(u, n.parent) };
        let (Some(id), Some(parent)) = (NodeId::expanded(u, n.local), parent) else { continue };
        if *budget == 0 {
            problems.push(Problem {
                node: parent,
                kind: "budget",
                message: format!("the document derives more than {} nodes; the rest are left out, from {id} on", expand::NODE_BUDGET),
            });
            break;
        }
        *budget -= 1;

        let under_repeat = in_repeat.contains(&parent);
        let parent_absent = absent.contains(&parent);
        let parent_hidden = hidden.contains(&parent);
        let patch = eval_virtual_patches(doc, global, &coord, id, n.local, parent_absent, parent_hidden);
        if !patch.present {
            absent.insert(id);
        } else if patch.hidden {
            hidden.insert(id);
        }

        let res_map = if patch.present {
            resolve_virtual_facets(doc, &t, i, &env, &coord, &patch)
        } else {
            None
        };

        let is_repeat_owner = match &res_map {
            Some(m) => m.keys().any(|&k| expand::owns_items(k)),
            None => n.facets.iter().any(|f| expand::owns_items(f.kind())),
        };
        if under_repeat || is_repeat_owner {
            in_repeat.insert(id);
        }

        if patch.present && !under_repeat {
            if n.ty.name() == "use" {
                if let Some(r) = &n.figures {
                    let inner_entity = t.follow_node(&env, r.to);
                    rels.push(Relation {
                        id: RelationId::expanded(u, r.id),
                        from: id,
                        to: inner_entity,
                        rel: RelKind::Figures,
                        data: r.data.clone(),
                    });
                    let inner_sized = match &res_map {
                        Some(m) => m.contains_key(&FacetKind::Size),
                        None => n.facets.iter().any(|f| matches!(f, Facet::Size(_))),
                    };
                    let mut inner_path = coord.path.clone();
                    inner_path.push(n.local);
                    if n.own {
                        if let Some(d) = linked_def_of_node(doc, r.to) {
                            let mut inner_scopes = coord.scopes.clone();
                            inner_scopes.push(d);
                            let inner_coord = ViewCoord { path: inner_path, scopes: inner_scopes, local: coord.local.clone() };
                            if let Some(inner_notation) = effective_notation(doc, global, &inner_coord, id, Some(n.local), res_map.as_ref(), Some(&n.facets)) {
                                inner_uses.push((id, n.local, inner_sized, inner_entity, d, inner_notation));
                            }
                        }
                    } else {
                        let inner_coord = ViewCoord { path: inner_path, scopes: coord.scopes.clone(), local: coord.local.clone() };
                        if let Some(inner_notation) = effective_notation(doc, global, &inner_coord, id, Some(n.local), res_map.as_ref(), Some(&n.facets)) {
                            other_uses.push((id, n.local, inner_sized, inner_entity, inner_notation));
                        }
                    }
                }
            } else if n.ty.name() == "entity" {
                let inst_facet = match &res_map {
                    Some(m) => m.get(&FacetKind::Instance),
                    None => n.facets.iter().find(|f| f.kind() == FacetKind::Instance),
                };
                if let Some(Facet::Instance(inst)) = inst_facet
                    && inst.mode.constant().and_then(|v| v.as_str()) == Some("linked")
                    && let Some(inner_def) = inst.def.constant().and_then(|v| v.as_ref())
                    && doc.live(inner_def).is_some_and(|dn| dn.ty.name() == "definition")
                {
                    inline_entities.push((id, n.local, inner_def));
                }
            }
        } else if patch.present && n.ty.name() == "use" {
            // Even inside a repeat template copy, `want_use` links its `figures` relation.
            if let Some(r) = &n.figures {
                let inner_entity = t.follow_node(&env, r.to);
                rels.push(Relation {
                    id: RelationId::expanded(u, r.id),
                    from: id,
                    to: inner_entity,
                    rel: RelKind::Figures,
                    data: r.data.clone(),
                });
            }
        }

        if let Some(map) = res_map {
            resolved.insert(id, map);
        }

        by_id.insert(id, nodes.len());
        nodes.push(ViewNode {
            node: Node { id, ty: n.ty, parent: Some(parent), order: n.order.clone(), tombstoned: None },
            template_idx: i,
            local: n.local,
        });
    }

    let view = finalize_view(ViewRecord {
        owner: u,
        top_owner,
        spatial_root,
        template: t,
        env,
        coord: coord.clone(),
        def,
        notation: Some(notation.to_owned()),
        nodes,
        by_id,
        children: HashMap::new(),
        resolved,
        hidden,
        absent,
        rels,
        rels_by_id: HashMap::new(),
        by_type: HashMap::new(),
        by_facet: HashMap::new(),
    });
    out.push(view);

    // Expand own inner pictures (recursively).
    for (inner_u, inner_local, inner_sized, inner_entity, inner_def, inner_notation) in inner_uses {
        let mut inner_path = coord.path.clone();
        inner_path.push(inner_local);
        let mut inner_scopes = coord.scopes.clone();
        inner_scopes.push(inner_def);
        let inner_coord = ViewCoord { path: inner_path, scopes: inner_scopes, local: coord.local.clone() };
        build_use_view(doc, reg, templates, inner_u, top_owner, spatial_root, inner_sized, Some(inner_entity), inner_def, &inner_notation, inner_coord, global, budget, problems, out);
    }

    // Expand inline non-repeat entities inside the template.
    for (inner_e, inner_local, inner_def) in inline_entities {
        let mut inner_path = coord.path.clone();
        inner_path.push(inner_local);
        let mut inner_scopes = coord.scopes.clone();
        inner_scopes.push(inner_def);
        let inner_coord = ViewCoord { path: inner_path, scopes: inner_scopes, local: coord.local.clone() };
        build_entity_view(doc, templates, inner_e, top_owner, spatial_root, inner_def, inner_coord, global, budget, problems, out);
    }

    // Expand non-own inner uses (e.g. figuring an inline entity or an authored entity).
    for (inner_u, inner_local, inner_sized, inner_entity, inner_notation) in other_uses {
        if let Some(inner_def) = resolve_entity_def(doc, reg, out, inner_entity) {
            let mut inner_path = coord.path.clone();
            inner_path.push(inner_local);
            let mut inner_scopes = coord.scopes.clone();
            inner_scopes.push(inner_def);
            let inner_coord = ViewCoord { path: inner_path, scopes: inner_scopes, local: coord.local.clone() };
            build_use_view(doc, reg, templates, inner_u, top_owner, spatial_root, inner_sized, Some(inner_entity), inner_def, &inner_notation, inner_coord, global, budget, problems, out);
        }
    }
}

/// Diff old vs new `ViewRecord`s of one top-level owner to produce an exact
/// [`ViewDelta`] (structural keys + spatial root tracking for disappearing IDs).
#[allow(dead_code)]
fn diff_views(old_views: &[ViewRecord], new_views: &[ViewRecord]) -> ViewDelta {
    let mut delta = ViewDelta::default();
    let mut keys: Vec<Key> = Vec::new();

    // Index live virtual nodes by id -> (&ViewRecord, &ViewNode).
    let mut old_nodes: HashMap<NodeId, (&ViewRecord, &ViewNode)> = HashMap::new();
    let mut old_rels: HashMap<RelationId, &Relation> = HashMap::new();
    let mut old_children: HashMap<NodeId, &[NodeId]> = HashMap::new();
    for v in old_views {
        for vn in &v.nodes {
            if !v.absent.contains(&vn.node.id) {
                old_nodes.insert(vn.node.id, (v, vn));
            }
        }
        for r in &v.rels {
            if !v.absent.contains(&r.from) && !v.absent.contains(&r.to) {
                old_rels.insert(r.id, r);
            }
        }
        for (&p, list) in &v.children {
            old_children.insert(p, list.as_slice());
        }
    }

    let mut new_nodes: HashMap<NodeId, (&ViewRecord, &ViewNode)> = HashMap::new();
    let mut new_rels: HashMap<RelationId, &Relation> = HashMap::new();
    let mut new_children: HashMap<NodeId, &[NodeId]> = HashMap::new();
    for v in new_views {
        for vn in &v.nodes {
            if !v.absent.contains(&vn.node.id) {
                new_nodes.insert(vn.node.id, (v, vn));
            }
        }
        for r in &v.rels {
            if !v.absent.contains(&r.from) && !v.absent.contains(&r.to) {
                new_rels.insert(r.id, r);
            }
        }
        for (&p, list) in &v.children {
            new_children.insert(p, list.as_slice());
        }
    }

    let emit_all_facet_keys = |v: &ViewRecord, id: NodeId, root: NodeId, keys: &mut Vec<Key>| {
        keys.push(Key::FacetSet(id));
        match v.facets_of(id) {
            VirtualFacets::Empty => {}
            VirtualFacets::Borrowed(slice) => {
                for f in slice {
                    let k = f.kind();
                    keys.push(Key::Facet(id, k));
                    keys.push(Key::With(root, k));
                    for field in f.field_names() {
                        if let Ok(path) = LeafPath::new(k, field) {
                            keys.push(Key::Leaf(Leaf { node: id, path }));
                        }
                    }
                }
            }
            VirtualFacets::Resolved(map) => {
                for (&k, f) in map {
                    keys.push(Key::Facet(id, k));
                    keys.push(Key::With(root, k));
                    for field in f.field_names() {
                        if let Ok(path) = LeafPath::new(k, field) {
                            keys.push(Key::Leaf(Leaf { node: id, path }));
                        }
                    }
                }
            }
        }
    };

    // Nodes that disappeared or became absent (carry old spatial_root!).
    for (&id, &(ov, ovn)) in &old_nodes {
        if !new_nodes.contains_key(&id) {
            delta.became_absent.push((id, ovn.node.parent, ov.spatial_root));
            keys.push(Key::Node(id));
            if let Some(p) = ovn.node.parent {
                keys.push(Key::Children(p));
            }
            keys.push(Key::In(ov.spatial_root, ovn.node.ty));
            emit_all_facet_keys(ov, id, ov.spatial_root, &mut keys);
        }
    }

    // Nodes that appeared or changed.
    for (&id, &(nv, nvn)) in &new_nodes {
        match old_nodes.get(&id) {
            None => {
                delta.became_present.push((id, nvn.node.parent, nv.spatial_root));
                keys.push(Key::Node(id));
                if let Some(p) = nvn.node.parent {
                    keys.push(Key::Children(p));
                }
                keys.push(Key::In(nv.spatial_root, nvn.node.ty));
                emit_all_facet_keys(nv, id, nv.spatial_root, &mut keys);
            }
            Some(&(ov, ovn)) => {
                let mut changed = false;
                if ovn.node != nvn.node || ov.is_hidden(id) != nv.is_hidden(id) {
                    changed = true;
                    keys.push(Key::Node(id));
                    if ovn.node.parent != nvn.node.parent || ovn.node.order != nvn.node.order {
                        if let Some(p) = ovn.node.parent {
                            keys.push(Key::Children(p));
                        }
                        if let Some(p) = nvn.node.parent {
                            keys.push(Key::Children(p));
                        }
                    }
                }
                if ovn.node.ty != nvn.node.ty || ov.spatial_root != nv.spatial_root {
                    changed = true;
                    keys.push(Key::In(ov.spatial_root, ovn.node.ty));
                    keys.push(Key::In(nv.spatial_root, nvn.node.ty));
                }
                // Fast path: if both borrow from the exact same Template Arc & index and neither is in `resolved`, facets are identical!
                let same_borrowed = !ov.resolved.contains_key(&id)
                    && !nv.resolved.contains_key(&id)
                    && Arc::ptr_eq(&ov.template, &nv.template)
                    && ovn.template_idx == nvn.template_idx;
                if !same_borrowed {
                    let old_kinds = facet_kinds_of(ov, id);
                    let new_kinds = facet_kinds_of(nv, id);
                    if old_kinds != new_kinds {
                        changed = true;
                        keys.push(Key::FacetSet(id));
                        for &k in old_kinds.difference(&new_kinds) {
                            keys.push(Key::With(ov.spatial_root, k));
                        }
                        for &k in new_kinds.difference(&old_kinds) {
                            keys.push(Key::With(nv.spatial_root, k));
                        }
                    }
                    for &k in old_kinds.union(&new_kinds) {
                        let before = ov.facet(id, k);
                        let after = nv.facet(id, k);
                        if before != after {
                            changed = true;
                            keys.push(Key::Facet(id, k));
                            match (before, after) {
                                (Some(b), Some(a)) => {
                                    for field in a.field_names() {
                                        if b.get(field) != a.get(field)
                                            && let Ok(path) = LeafPath::new(k, field)
                                        {
                                            keys.push(Key::Leaf(Leaf { node: id, path }));
                                        }
                                    }
                                }
                                (Some(f), None) | (None, Some(f)) => {
                                    for field in f.field_names() {
                                        if let Ok(path) = LeafPath::new(k, field) {
                                            keys.push(Key::Leaf(Leaf { node: id, path }));
                                        }
                                    }
                                }
                                (None, None) => {}
                            }
                        }
                    }
                }
                if changed {
                    delta.changed_nodes.push((id, nv.spatial_root));
                }
            }
        }
    }

    // Cached child list changes (including direct children of `owner`).
    for (&p, &old_list) in &old_children {
        if new_children.get(&p).copied() != Some(old_list) {
            keys.push(Key::Children(p));
        }
    }
    for (&p, &new_list) in &new_children {
        if old_children.get(&p).copied() != Some(new_list) {
            keys.push(Key::Children(p));
        }
    }

    // Virtual relation changes.
    for (&rid, &old_r) in &old_rels {
        if new_rels.get(&rid).copied() != Some(old_r) {
            keys.push(Key::Relation(rid));
            keys.push(Key::Relations(old_r.from));
            keys.push(Key::Relations(old_r.to));
        }
    }
    for (&rid, &new_r) in &new_rels {
        if old_rels.get(&rid).copied() != Some(new_r) {
            keys.push(Key::Relation(rid));
            keys.push(Key::Relations(new_r.from));
            keys.push(Key::Relations(new_r.to));
        }
    }

    // Deduplicate keys while keeping deterministic order.
    let mut seen = HashSet::with_capacity(keys.len());
    delta.keys = keys.into_iter().filter(|k| seen.insert(*k)).collect();
    delta.became_present.sort();
    delta.became_absent.sort();
    delta.changed_nodes.sort();
    delta
}

#[allow(dead_code)]
fn facet_kinds_of(v: &ViewRecord, id: NodeId) -> BTreeSet<FacetKind> {
    match v.facets_of(id) {
        VirtualFacets::Empty => BTreeSet::new(),
        VirtualFacets::Borrowed(slice) => slice.iter().map(Facet::kind).collect(),
        VirtualFacets::Resolved(map) => map.keys().copied().collect(),
    }
}

/// Materialized-view test oracle (`EN-5a`): compare `Tree::with_views(docs, views)`
/// bit-for-bit against `Tree::of(docs)` (materialized `Docs::copies`) across live
/// nodes, children order, facets/leaves/bound slots, relations, scoped `In`/`With`
/// indexes, and 2D geometry (`world_affine` & `local_bounds`).
///
/// Scenes containing active repeat/conditional items are skipped only for item
/// scopes (`EN-5c` migrates repeat/when items into `ViewRegistry`); for all
/// static entity/`use` scenes every query must match 100% identically.
pub(crate) fn check_parity(
    docs: &crate::tree::Docs,
    reactive: &crate::graph::Reactive,
    views: &ViewRegistry,
) -> Result<(), String> {
    let ctree = Tree::of(docs);
    let vtree = Tree::with_views(docs, views);

    // 1. Live nodes set + metadata (ty, parent, order, hidden).
    let mut cnodes: Vec<&Node> = ctree.live_nodes().filter(|n| n.id.repeat_item().is_none()).collect();
    let mut vnodes: Vec<&Node> = vtree.live_nodes().collect();
    cnodes.sort_by_key(|n| n.id);
    vnodes.sort_by_key(|n| n.id);
    if cnodes.len() != vnodes.len() {
        let cset: BTreeSet<NodeId> = cnodes.iter().map(|n| n.id).collect();
        let vset: BTreeSet<NodeId> = vnodes.iter().map(|n| n.id).collect();
        let only_c: Vec<NodeId> = cset.difference(&vset).copied().take(10).collect();
        let only_v: Vec<NodeId> = vset.difference(&cset).copied().take(10).collect();
        return Err(format!(
            "live_nodes count mismatch: copies={} views={} only_copies={only_c:?} only_views={only_v:?}",
            cnodes.len(),
            vnodes.len()
        ));
    }
    for (cn, vn) in cnodes.iter().zip(vnodes.iter()) {
        if cn != vn {
            return Err(format!("node {} differs: copies={cn:?} views={vn:?}", cn.id));
        }
        if ctree.is_hidden(cn.id) != vtree.is_hidden(vn.id) {
            return Err(format!("is_hidden({}) differs", cn.id));
        }
        if ctree.top_root(cn.id) != vtree.top_root(vn.id) {
            return Err(format!(
                "top_root({}) differs: copies={:?} views={:?}",
                cn.id,
                ctree.top_root(cn.id),
                vtree.top_root(vn.id)
            ));
        }

        // 2. Exact children order (excluding repeat items until EN-5c).
        let cc: Vec<NodeId> = ctree.children(cn.id).iter().copied().filter(|id| id.repeat_item().is_none()).collect();
        let vc: Vec<NodeId> = vtree.children(vn.id).iter().copied().collect();
        if cc != vc {
            return Err(format!("children({}) differ: copies={cc:?} views={vc:?}", cn.id));
        }

        // 3. Exact facets & leaves.
        let cf: Vec<&Facet> = ctree.facets_of(cn.id).collect();
        let vf: Vec<&Facet> = vtree.facets_of(vn.id).collect();
        if cf != vf {
            return Err(format!("facets_of({}) differ:\n copies={cf:?}\n views={vf:?}", cn.id));
        }
        for f in &cf {
            if ctree.facet(cn.id, f.kind()) != vtree.facet(vn.id, f.kind()) {
                return Err(format!("facet({}, {:?}) differs", cn.id, f.kind()));
            }
            if !vtree.has(vn.id, f.kind()) {
                return Err(format!("has({}, {:?}) false in views", cn.id, f.kind()));
            }
        }
    }

    // 4. Bound slots (excluding repeat items until EN-5c).
    let cb: Vec<(Leaf, &Expr)> = ctree.bound_slots().filter(|(l, _)| l.node.repeat_item().is_none()).collect();
    let vb: Vec<(Leaf, &Expr)> = vtree.bound_slots().collect();
    if cb != vb {
        return Err(format!("bound_slots differ: copies={} views={}", cb.len(), vb.len()));
    }

    // 5. Relations (global, per-id, from, to).
    let mut crels: Vec<&Relation> = ctree.relations().filter(|r| r.from.repeat_item().is_none() && r.to.repeat_item().is_none()).collect();
    let mut vrels: Vec<&Relation> = vtree.relations().collect();
    crels.sort_by_key(|r| r.id);
    vrels.sort_by_key(|r| r.id);
    if crels != vrels {
        return Err(format!("relations differ:\n copies={crels:?}\n views={vrels:?}"));
    }
    for r in &crels {
        if ctree.relation(r.id) != vtree.relation(r.id) {
            return Err(format!("relation({}) differs", r.id));
        }
    }
    for cn in &cnodes {
        let mut cfrom: Vec<&Relation> = ctree.relations_from(cn.id).filter(|r| r.to.repeat_item().is_none()).collect();
        let mut vfrom: Vec<&Relation> = vtree.relations_from(cn.id).collect();
        cfrom.sort_by_key(|r| r.id);
        vfrom.sort_by_key(|r| r.id);
        if cfrom != vfrom {
            return Err(format!("relations_from({}) differ: copies={cfrom:?} views={vfrom:?}", cn.id));
        }
        let mut cto: Vec<&Relation> = ctree.relations_to(cn.id).filter(|r| r.from.repeat_item().is_none()).collect();
        let mut vto: Vec<&Relation> = vtree.relations_to(cn.id).collect();
        cto.sort_by_key(|r| r.id);
        vto.sort_by_key(|r| r.id);
        if cto != vto {
            return Err(format!("relations_to({}) differ: copies={cto:?} views={vto:?}", cn.id));
        }
    }

    // 6. Scoped indexes (`nodes_of_type_in` & `nodes_with_in`) for every root.
    let types: BTreeSet<TypeId> = cnodes.iter().map(|n| n.ty).collect();
    for &root in ctree.roots().iter() {
        for &ty in &types {
            let ct: Vec<NodeId> = ctree.nodes_of_type_in(root, ty).into_iter().filter(|id| id.repeat_item().is_none()).collect();
            let vt = vtree.nodes_of_type_in(root, ty);
            if ct != vt {
                return Err(format!("nodes_of_type_in({root}, {ty}) differ: copies={ct:?} views={vt:?}"));
            }
        }
        for &fk in FacetKind::ALL {
            let cf: Vec<NodeId> = ctree.nodes_with_in(root, fk).into_iter().filter(|id| id.repeat_item().is_none()).collect();
            let vf = vtree.nodes_with_in(root, fk);
            if cf != vf {
                return Err(format!("nodes_with_in({root}, {fk:?}) differ: copies={cf:?} views={vf:?}"));
            }
        }
    }

    // 7. 2D geometry (`world_affine` & `local_bounds`) on every live node.
    struct TreeReads<'a> {
        tree: Tree<'a>,
        reactive: &'a crate::graph::Reactive,
    }
    impl<'a> crate::reads::Reads for TreeReads<'a> {
        fn tree(&self) -> Tree<'_> {
            self.tree
        }
        fn resolved(&self, id: NodeId, path: LeafPath) -> Option<crate::value::Value> {
            self.reactive.read(self.tree, Leaf { node: id, path })
        }
        fn layout_base(&self, id: NodeId, path: LeafPath) -> Option<crate::value::Value> {
            match self.reactive.signal(Leaf { node: id, path }) {
                Some(s) if s.producer == crate::graph::ProducerId::LAYOUT => crate::reads::constant_or_default(self.tree, id, path),
                _ => self.resolved(id, path),
            }
        }
        fn layout_preview(&self) -> Option<&crate::layout::Preview> {
            None
        }
    }
    let reads_c = TreeReads { tree: ctree, reactive };
    let reads_v = TreeReads { tree: vtree, reactive };
    let read_c = |leaf: Leaf| reactive.read(ctree, leaf);
    let read_v = |leaf: Leaf| reactive.read(vtree, leaf);
    for cn in &cnodes {
        let ca = crate::geom::world_affine(ctree, &read_c, cn.id);
        let va = crate::geom::world_affine(vtree, &read_v, cn.id);
        if ca != va {
            return Err(format!("world_affine({}) differs: copies={ca:?} views={va:?}", cn.id));
        }
        let cb = crate::geom::local_bounds(&reads_c, cn.id);
        let vb = crate::geom::local_bounds(&reads_v, cn.id);
        if cb != vb {
            return Err(format!("local_bounds({}) differs: copies={cb:?} views={vb:?}", cn.id));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::template::TemplateCache;
    use crate::patch::{Axis, AxisId, AxisKind, AxisOp, AxisValue, AxisValueId, Patch, PatchId, PatchOp, PatchValue, Product, Sel};
    use crate::*;

    fn apply_xml(store: &mut StoreCore, xml: &str) -> BTreeMap<String, NodeId> {
        let plan = crate::edit::plan(store, xml, None).unwrap_or_else(|f| panic!("{}", f.message()));
        let aliases = plan.aliases.clone();
        store
            .apply(TransactionInput {
                label: "edit".into(),
                actor: Actor::Human,
                ts: 0,
                idempotency_key: None,
                ops: plan.ops,
                exact: false,
            })
            .unwrap_or_else(|e| panic!("apply failed: {e:?}"));
        aliases
    }

    /// Recorded-read `TemplateCache` invalidation & canary (`EN-5a`):
    /// - editing inside `def_a` invalidates `def_a`'s cached template and leaves `def_b` intact;
    /// - adding a previously-missing notation invalidates the cached `None`;
    /// - rolling back a transaction clears templates compiled from uncommitted state.
    #[test]
    fn template_cache_recorded_reads_and_rollback() {
        let mut store = StoreCore::new();
        let ids = apply_xml(
            &mut store,
            r#"<edit>
              <library id="$lib">
                <definition id="$def_a" name="A">
                  <port_spec id="$pa" name="x" kind="value" dir="in" signal="1"/>
                  <template id="$ta" notation="free">
                    <rect id="$ra" name="ra" w="20" h="10"/>
                  </template>
                </definition>
                <definition id="$def_b" name="B">
                  <port_spec id="$pb" name="y" kind="value" dir="in" signal="2"/>
                  <template id="$tb" notation="free">
                    <rect id="$rb" name="rb" w="30" h="15"/>
                  </template>
                </definition>
              </library>
              <composition id="$comp" name="C" dims="2d">
                <entity id="$ea" name="ea" instance.def="$def_a" instance.mode="linked" args.x="1"/>
                <use id="$ua" notation="free"/>
                <entity id="$eb" name="eb" instance.def="$def_b" instance.mode="linked" args.y="2"/>
                <use id="$ub" notation="free"/>
              </composition>
              <link from="$ua" to="$ea" rel="figures"/>
              <link from="$ub" to="$eb" rel="figures"/>
            </edit>"#,
        );
        let (def_a, def_b, ra, ua) = (ids["$def_a"], ids["$def_b"], ids["$ra"], ids["$ua"]);

        let rev_a_before = store.owners.templates().picture_rev(def_a, "free").expect("A cached");
        let rev_b_before = store.owners.templates().picture_rev(def_b, "free").expect("B cached");

        // Edit inside def_a: invalidates def_a, leaves def_b completely untouched!
        store
            .apply(TransactionInput {
                label: "edit ra".into(),
                actor: Actor::Human,
                ts: 1,
                idempotency_key: None,
                ops: vec![Op::Set {
                    id: ra,
                    path: "size.w".parse().unwrap(),
                    slot: Slot::Const(Value::Number(99.0)),
                }],
                exact: false,
            })
            .unwrap();
        let rev_a_after = store.owners.templates().picture_rev(def_a, "free").expect("A recompiled");
        let rev_b_after = store.owners.templates().picture_rev(def_b, "free").expect("B still cached");
        assert_ne!(rev_a_before, rev_a_after, "editing def_a must recompile def_a template");
        assert_eq!(rev_b_before, rev_b_after, "editing def_a must NOT evict unrelated def_b template");

        // Switch ua to a missing notation "schematic": caches None with recorded reads on def_a.
        store
            .apply(TransactionInput {
                label: "missing notation".into(),
                actor: Actor::Human,
                ts: 2,
                idempotency_key: None,
                ops: vec![Op::Set {
                    id: ua,
                    path: "use.notation".parse().unwrap(),
                    slot: Slot::Const(Value::Str("schematic".into())),
                }],
                exact: false,
            })
            .unwrap();
        assert!(store.owners.templates().has_picture_entry(def_a, "schematic"));
        assert!(store.owners.templates().picture_rev(def_a, "schematic").is_none());

        // Now add a "schematic" template under def_a: invalidates the cached None automatically!
        let added = apply_xml(
            &mut store,
            &format!(
                r#"<edit>
                  <template id="$ts" in="{def_a}" notation="schematic">
                    <ellipse id="$es" name="es" w="40" h="40"/>
                  </template>
                </edit>"#
            ),
        );
        assert!(store.owners.templates().picture_rev(def_a, "schematic").is_some(), "new schematic template must invalidate cached None");
        let copy_es = NodeId::expanded(ua, added["$es"]).unwrap();
        assert!(store.tree().is_live(copy_es), "ua must now derive schematic ellipse");

        // Failed transaction that edits def_a and triggers a post-expansion check error rolls back and clears templates.
        let bad_tx = store.apply(TransactionInput {
            label: "bad scope".into(),
            actor: Actor::Human,
            ts: 3,
            idempotency_key: None,
            ops: vec![
                Op::Set { id: ra, path: "size.w".parse().unwrap(), slot: Slot::Const(Value::Number(123.0)) },
                // Out-of-scope binding inside def_a referencing def_b's node -> fails check_scope AFTER expand!
                Op::Set {
                    id: ra,
                    path: "size.h".parse().unwrap(),
                    slot: Slot::Bound(Expr::Ref(Leaf { node: ids["$rb"], path: "size.h".parse().unwrap() })),
                },
            ],
            exact: false,
        });
        assert!(bad_tx.is_err());
        assert!(store.owners.templates().picture_rev(def_a, "free").is_none(), "rollback must clear templates compiled from uncommitted state");
        assert_eq!(store.tree().leaf(ra, "size.w".parse().unwrap()), Some(&Slot::Const(Value::Number(99.0))));
    }

    /// Council Scenario 1, 2, 3 + Per-View `ViewCoord` & Derived Scope-Chain Precedence (`EN-5a`):
    /// - Outermost enclosing scope wins over inner scope without global `u32` layer renumbering;
    /// - Suffix path matching works with zero saved `AxisValue`s on the `Instance` axis;
    /// - Nested notation switch via `PatchKey::Notation` emits exact `ViewDelta` keys,
    ///   reveals initially-absent virtual targets, and reports `old_spatial_root` on disappearing IDs.
    #[test]
    fn view_coord_derived_layers_and_three_council_scenarios() {
        let mut store = StoreCore::new();
        let ids = apply_xml(
            &mut store,
            r#"<edit>
              <library id="$lib">
                <definition id="$inner_def" name="Inner">
                  <port_spec id="$ip" name="v" kind="value" dir="in" signal="1"/>
                  <template id="$it_cartoon" notation="cartoon">
                    <rect id="$inner_rect" name="r" w="20" h="20"/>
                  </template>
                  <template id="$it_schem" notation="schematic">
                    <ellipse id="$inner_ell" name="e" w="14" h="14"/>
                  </template>
                </definition>
                <definition id="$outer_def" name="Outer">
                  <port_spec id="$op" name="v" kind="value" dir="in" signal="1"/>
                  <entity id="$sub_e" name="sub" instance.def="$inner_def" instance.mode="linked" args.v="1"/>
                  <template id="$ot" notation="cartoon">
                    <use id="$sub_u" notation="cartoon"/>
                  </template>
                </definition>
              </library>
              <composition id="$comp" name="C" dims="2d">
                <entity id="$top_e" name="top" instance.def="$outer_def" instance.mode="linked" args.v="1"/>
                <use id="$top_u" notation="cartoon"/>
              </composition>
              <link from="$sub_u" to="$sub_e" rel="figures"/>
              <link from="$top_u" to="$top_e" rel="figures"/>
            </edit>"#,
        );
        let (inner_def, outer_def, inner_rect, inner_ell, sub_u, top_u, comp) = (
            ids["$inner_def"],
            ids["$outer_def"],
            ids["$inner_rect"],
            ids["$inner_ell"],
            ids["$sub_u"],
            ids["$top_u"],
            ids["$comp"],
        );

        // Add an Instance axis with ZERO saved AxisValues (derived ViewCoord matching!)
        // and a discrete Mode axis ("a", "b") to drive nested notation switching.
        let inst_ax = AxisId(1);
        let mode_ax = AxisId(2);
        let mode_a = AxisValueId(1);
        let mode_b = AxisValueId(2);
        let inner_patch = Patch {
            id: PatchId(1),
            layer: 5, // higher stored u32 than outer_patch!
            rank: OrderKey::after(&OrderKey::first()),
            region: Product::default().with(inst_ax, Sel::Suffix { tail: vec![sub_u], not: vec![] }),
            anchor: Some(inner_def),
            writable: true,
            entries: BTreeMap::from([(
                PatchTarget { node: inner_rect, key: PatchKey::Leaf("size.w".parse().unwrap()) },
                PatchValue::Slot { slot: Slot::Const(Value::Number(50.0)) },
            )]),
        };
        let outer_patch = Patch {
            id: PatchId(2),
            layer: 1, // LOWER stored u32, but anchored at outer_def -> MUST win structurally!
            rank: OrderKey::first(),
            region: Product::default().with(inst_ax, Sel::Suffix { tail: vec![top_u, sub_u], not: vec![] }),
            anchor: Some(outer_def),
            writable: true,
            entries: BTreeMap::from([(
                PatchTarget { node: inner_rect, key: PatchKey::Leaf("size.w".parse().unwrap()) },
                PatchValue::Slot { slot: Slot::Const(Value::Number(88.0)) },
            )]),
        };
        // Nested notation switch patch: on mode_b, switch sub_u from "cartoon" to "schematic".
        let switch_patch = Patch {
            id: PatchId(3),
            layer: 0,
            rank: OrderKey::first(),
            region: Product::default().with(mode_ax, Sel::Set { values: BTreeSet::from([mode_b]) }),
            anchor: Some(outer_def),
            writable: true,
            entries: BTreeMap::from([(
                PatchTarget { node: sub_u, key: PatchKey::Notation },
                PatchValue::Notation { notation: "schematic".into() },
            )]),
        };

        store
            .apply(TransactionInput {
                label: "axes+patches".into(),
                actor: Actor::Human,
                ts: 1,
                idempotency_key: None,
                ops: vec![
                    Op::Axis(AxisOp::Add {
                        axis: Axis { id: inst_ax, name: "instance".into(), kind: AxisKind::Instance, owner: None, values: vec![] },
                    }),
                    Op::Axis(AxisOp::Add {
                        axis: Axis {
                            id: mode_ax,
                            name: "mode".into(),
                            kind: AxisKind::Unordered,
                            owner: None,
                            values: vec![
                                AxisValue { id: mode_a, name: "a".into(), path: vec![], scopes: vec![], live: true },
                                AxisValue { id: mode_b, name: "b".into(), path: vec![], scopes: vec![], live: true },
                            ],
                        },
                    }),
                    Op::Patch(PatchOp::Add { patch: inner_patch }),
                    Op::Patch(PatchOp::Add { patch: outer_patch }),
                    Op::Patch(PatchOp::Add { patch: switch_patch }),
                ],
                exact: false,
            })
            .unwrap();

        let empty = ViewRegistry::new();
        let authored_doc = Tree::with_views(&store.docs, &empty);
        let mut tcache = TemplateCache::default();
        let mut tmpls = Templates::new(&mut tcache);

        let pt_a = Point::new().with_value(mode_ax, mode_a);
        let (mut reg, probs) = ViewRegistry::build_static(authored_doc, &mut tmpls, &pt_a, None);
        assert!(probs.is_empty(), "{probs:?}");

        let nested_u = NodeId::expanded(top_u, sub_u).unwrap();
        let nested_rect = NodeId::expanded(nested_u, inner_rect).unwrap();
        let nested_ell = NodeId::expanded(nested_u, inner_ell).unwrap();

        // Outermost scope (`outer_def`, stored layer 1) beats inner scope (`inner_def`, stored layer 5)!
        let vtree_a = Tree::with_views(&store.docs, &reg);
        assert_eq!(vtree_a.leaf(nested_rect, "size.w".parse().unwrap()), Some(&Slot::Const(Value::Number(88.0))));
        // Council Scenario 2: initially-absent virtual target (`nested_ell` does not exist under cartoon).
        let rec = crate::structure::Recorder::default();
        assert!(vtree_a.recording(&rec).live(nested_ell).is_none());
        let watched_keys = rec.into_inner();
        assert!(watched_keys.contains(&Key::Node(nested_ell)));

        // Council Scenario 1 + 2 + 3: switch to mode_b -> nested_u switches cartoon -> schematic!
        let pt_b = Point::new().with_value(mode_ax, mode_b);
        let (delta, probs_b) = reg.reconcile_owner(authored_doc, &mut tmpls, top_u, &pt_b, &DiscretePoint::default());
        assert!(probs_b.is_empty(), "{probs_b:?}");
        // Initially-absent target's watched key MUST be in delta.keys!
        assert!(delta.keys.contains(&Key::Node(nested_ell)), "initially-absent target must wake: {:?}", delta.keys);
        assert!(delta.became_present.iter().any(|&(id, p, r)| id == nested_ell && p == Some(nested_u) && r == comp));
        // Disappearing virtual ID MUST carry its old spatial root (`comp`)!
        assert!(delta.became_absent.iter().any(|&(id, p, r)| id == nested_rect && p == Some(nested_u) && r == comp));
        assert!(delta.keys.contains(&Key::Node(nested_rect)));
        assert!(delta.keys.contains(&Key::Children(nested_u)));

        let vtree_b = Tree::with_views(&store.docs, &reg);
        assert!(vtree_b.live(nested_rect).is_none());
        assert!(vtree_b.live(nested_ell).is_some());

        // Component-local `ViewCoord::local` overrides global point: global is pt_b (schematic),
        // local is mode_a (cartoon) -> view stays cartoon!
        let local_a: DiscretePoint = BTreeMap::from([(mode_ax, mode_a)]);
        let (delta_local, _) = reg.reconcile_owner(authored_doc, &mut tmpls, top_u, &pt_b, &local_a);
        assert!(delta_local.became_present.iter().any(|&(id, _, _)| id == nested_rect));
        assert!(delta_local.became_absent.iter().any(|&(id, _, r)| id == nested_ell && r == comp));
    }
}

