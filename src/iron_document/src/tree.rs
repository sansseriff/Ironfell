//! The read interface (plans/specs/engine.md §2.3, §3.2; handoff-engine.md
//! EN-13): what answers for authored nodes and virtual derived views alike.
//! Behind [`Tree`] are `Docs::authored` (authored nodes, facets, relations, and
//! flat `Document::solved`) and `Docs::views` (`VirtualViews` over shared
//! `Arc<Template>` + sparse resolved facets + keyed `ItemTable`). `Docs::copies`
//! and `apart` are gone.

use crate::document::{Document, Node, Relation};
use crate::engine::views::{OwnerPlan, ViewDelta, VirtualViews};
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, LeafPath, LeafStruct};
use crate::id::{NodeId, RelationId};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::Slot;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// Per-top-level-root type and facet indexes across both authored nodes and
/// virtual derived nodes (engine.md §3.2). Maintained incrementally by `Docs`
/// and `VirtualViews` and journaled alongside the document journal.
#[derive(Clone, Default)]
pub(crate) struct ScopedIndexes {
    pub(crate) root_of: BTreeMap<NodeId, NodeId>,
    by_type: BTreeMap<(NodeId, TypeId), BTreeSet<NodeId>>,
    by_facet: BTreeMap<(NodeId, FacetKind), BTreeSet<NodeId>>,
    /// Global virtual-only type & facet indexes so `Tree::nodes_of_type` and
    /// `Tree::nodes_with` never scan `Document::roots()` across hidden pages.
    virtual_by_type: BTreeMap<TypeId, BTreeSet<NodeId>>,
    virtual_by_facet: BTreeMap<FacetKind, BTreeSet<NodeId>>,
    /// Direct `let` children per parent (`None`: root `let`s), keyed by
    /// `Key::LetsIn(parent)` so `@name` lookups touch only ancestor scopes in O(depth).
    lets_by_parent: BTreeMap<Option<NodeId>, BTreeSet<NodeId>>,
}

#[derive(Clone)]
pub(crate) enum ScopedRec {
    Root(NodeId, Option<NodeId>),
    ByType(NodeId, TypeId, NodeId, bool),
    ByFacet(NodeId, FacetKind, NodeId, bool),
    LetParent(Option<NodeId>, NodeId, bool),
}

impl ScopedIndexes {
    fn build(authored: &Document) -> ScopedIndexes {
        let mut out = ScopedIndexes::default();
        let let_ty = TypeId::lookup("let").expect("registered");
        let mut stack: Vec<(NodeId, NodeId)> = authored.roots().iter().map(|&r| (r, r)).collect();
        while let Some((id, root)) = stack.pop() {
            let Some(n) = authored.live(id) else { continue };
            out.root_of.insert(id, root);
            out.by_type.entry((root, n.ty)).or_default().insert(id);
            if n.ty == let_ty {
                out.lets_by_parent.entry(n.parent).or_default().insert(id);
            }
            for f in authored.facets_of(id) {
                out.by_facet.entry((root, f.kind())).or_default().insert(id);
            }
            for &c in authored.children(id) {
                stack.push((c, root));
            }
        }
        out
    }

    pub(crate) fn set_root(&mut self, id: NodeId, root: Option<NodeId>, journal: &mut Option<Vec<ScopedRec>>) {
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

    pub(crate) fn insert_let(&mut self, parent: Option<NodeId>, id: NodeId, journal: &mut Option<Vec<ScopedRec>>, touched: &mut crate::structure::Touched) {
        if self.lets_by_parent.entry(parent).or_default().insert(id) {
            if let Some(j) = journal {
                j.push(ScopedRec::LetParent(parent, id, true));
            }
            touched.note(crate::structure::Key::LetsIn(parent));
        }
    }

    pub(crate) fn remove_let(&mut self, parent: Option<NodeId>, id: NodeId, journal: &mut Option<Vec<ScopedRec>>, touched: &mut crate::structure::Touched) {
        if let Some(s) = self.lets_by_parent.get_mut(&parent)
            && s.remove(&id)
        {
            if s.is_empty() {
                self.lets_by_parent.remove(&parent);
            }
            if let Some(j) = journal {
                j.push(ScopedRec::LetParent(parent, id, false));
            }
            touched.note(crate::structure::Key::LetsIn(parent));
        }
    }

    pub(crate) fn insert_type(&mut self, root: NodeId, ty: TypeId, id: NodeId, journal: &mut Option<Vec<ScopedRec>>, touched: &mut crate::structure::Touched) {
        if self.by_type.entry((root, ty)).or_default().insert(id) {
            if id.is_expanded() {
                self.virtual_by_type.entry(ty).or_default().insert(id);
            }
            if let Some(j) = journal {
                j.push(ScopedRec::ByType(root, ty, id, true));
            }
            touched.note(crate::structure::Key::In(root, ty));
        }
    }

    pub(crate) fn remove_type(&mut self, root: NodeId, ty: TypeId, id: NodeId, journal: &mut Option<Vec<ScopedRec>>, touched: &mut crate::structure::Touched) {
        if let Some(s) = self.by_type.get_mut(&(root, ty))
            && s.remove(&id)
        {
            if s.is_empty() {
                self.by_type.remove(&(root, ty));
            }
            if id.is_expanded()
                && let Some(vs) = self.virtual_by_type.get_mut(&ty)
            {
                vs.remove(&id);
                if vs.is_empty() {
                    self.virtual_by_type.remove(&ty);
                }
            }
            if let Some(j) = journal {
                j.push(ScopedRec::ByType(root, ty, id, false));
            }
            touched.note(crate::structure::Key::In(root, ty));
        }
    }

    pub(crate) fn insert_facet(&mut self, root: NodeId, kind: FacetKind, id: NodeId, journal: &mut Option<Vec<ScopedRec>>, touched: &mut crate::structure::Touched) {
        if self.by_facet.entry((root, kind)).or_default().insert(id) {
            if id.is_expanded() {
                self.virtual_by_facet.entry(kind).or_default().insert(id);
            }
            if let Some(j) = journal {
                j.push(ScopedRec::ByFacet(root, kind, id, true));
            }
            touched.note(crate::structure::Key::With(root, kind));
        }
    }

    pub(crate) fn remove_facet(&mut self, root: NodeId, kind: FacetKind, id: NodeId, journal: &mut Option<Vec<ScopedRec>>, touched: &mut crate::structure::Touched) {
        if let Some(s) = self.by_facet.get_mut(&(root, kind))
            && s.remove(&id)
        {
            if s.is_empty() {
                self.by_facet.remove(&(root, kind));
            }
            if id.is_expanded()
                && let Some(vs) = self.virtual_by_facet.get_mut(&kind)
            {
                vs.remove(&id);
                if vs.is_empty() {
                    self.virtual_by_facet.remove(&kind);
                }
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
                        if id.is_expanded()
                            && let Some(vs) = self.virtual_by_type.get_mut(&ty)
                        {
                            vs.remove(&id);
                            if vs.is_empty() {
                                self.virtual_by_type.remove(&ty);
                            }
                        }
                    } else {
                        self.by_type.entry((root, ty)).or_default().insert(id);
                        if id.is_expanded() {
                            self.virtual_by_type.entry(ty).or_default().insert(id);
                        }
                    }
                }
                ScopedRec::LetParent(parent, id, inserted) => {
                    if inserted {
                        if let Some(s) = self.lets_by_parent.get_mut(&parent) {
                            s.remove(&id);
                            if s.is_empty() {
                                self.lets_by_parent.remove(&parent);
                            }
                        }
                    } else {
                        self.lets_by_parent.entry(parent).or_default().insert(id);
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
                        if id.is_expanded()
                            && let Some(vs) = self.virtual_by_facet.get_mut(&kind)
                        {
                            vs.remove(&id);
                            if vs.is_empty() {
                                self.virtual_by_facet.remove(&kind);
                            }
                        }
                    } else {
                        self.by_facet.entry((root, kind)).or_default().insert(id);
                        if id.is_expanded() {
                            self.virtual_by_facet.entry(kind).or_default().insert(id);
                        }
                    }
                }
            }
        }
    }
}

/// The store's authored document plus lean lexical virtual views (`EN-13`).
#[derive(Clone)]
pub(crate) struct Docs {
    pub(crate) authored: Document,
    pub(crate) views: VirtualViews,
    pub(crate) scoped: ScopedIndexes,
    scoped_journal: Option<Vec<ScopedRec>>,
    scoped_touched: crate::structure::Touched,
    /// Post-commit set of live embeds currently blocked by a repeat-induced
    /// embed cycle (`None` until synced by `StoreCore::embed_topology`, so
    /// unsynced `Docs` fall back to walking `reaches_composition`).
    pub(crate) cyclic_embeds: Option<BTreeSet<NodeId>>,
    /// Offsets into `(authored.touched, views.touched, scoped_touched)` already
    /// observed by `Owners::note_touched` since the last `take_touched`.
    view_cursor: (usize, usize, usize),
    /// Virtual teardown deltas emitted directly by `delete_node` during a
    /// transaction before `Owners::sync_dirty` runs.
    pending_view_delta: ViewDelta,
    /// Each node deleted since the changes were last taken, as it was when it went.
    pub(crate) gone: crate::changes::Gone,
    gone_journal: Option<Vec<(NodeId, Option<crate::changes::Went>)>>,
}

impl Docs {
    /// Part of `StoreCore::drop_in_parts`.
    pub(crate) fn drop_in_parts(self, after: &mut dyn FnMut(&'static str)) {
        let Docs { authored, views, scoped, .. } = self;
        drop(scoped);
        after("scoped indexes");
        drop(views);
        after("virtual views");
        drop(authored);
        after("authored document");
    }

    pub(crate) fn new(authored: Document) -> Docs {
        let scoped = ScopedIndexes::build(&authored);
        Docs {
            authored,
            views: VirtualViews::default(),
            scoped,
            scoped_journal: None,
            scoped_touched: Default::default(),
            cyclic_embeds: None,
            view_cursor: (0, 0, 0),
            pending_view_delta: ViewDelta::default(),
            gone: Default::default(),
            gone_journal: None,
        }
    }

    fn root_for(&self, id: NodeId, parent: Option<NodeId>) -> NodeId {
        match parent {
            Some(p) => self.scoped.root_of.get(&p).copied().unwrap_or_else(|| {
                let tree = Tree::of(self);
                let mut cur = p;
                while let Some(up) = tree.parent_of(cur) {
                    cur = up;
                }
                cur
            }),
            None => id,
        }
    }

    pub(crate) fn insert_node(&mut self, node: Node, comps: Vec<Facet>) {
        debug_assert!(!node.id.is_expanded(), "insert_node targets authored nodes only");
        let (id, ty, parent) = (node.id, node.ty, node.parent);
        let root = self.root_for(id, parent);
        let kinds: Vec<FacetKind> = comps.iter().map(Facet::kind).collect();
        self.authored.insert_node(node, comps);
        let on = self.scoped_touched.on;
        self.scoped_touched.on &= !crate::structure::silenced("insert_node");
        self.scoped.set_root(id, Some(root), &mut self.scoped_journal);
        self.scoped.insert_type(root, ty, id, &mut self.scoped_journal, &mut self.scoped_touched);
        if ty.name() == "let" {
            self.scoped.insert_let(parent, id, &mut self.scoped_journal, &mut self.scoped_touched);
        }
        for k in kinds {
            self.scoped.insert_facet(root, k, id, &mut self.scoped_journal, &mut self.scoped_touched);
        }
        self.scoped_touched.on = on;
    }

    pub(crate) fn restore_node(&mut self, id: NodeId, ty: TypeId, parent: Option<NodeId>, order: OrderKey, comps: Vec<Facet>) {
        debug_assert!(!id.is_expanded(), "restore_node targets authored nodes only");
        let root = self.root_for(id, parent);
        let kinds: Vec<FacetKind> = comps.iter().map(Facet::kind).collect();
        self.authored.restore_node(id, ty, parent, order, comps);
        let on = self.scoped_touched.on;
        self.scoped_touched.on &= !crate::structure::silenced("restore_node");
        self.scoped.set_root(id, Some(root), &mut self.scoped_journal);
        self.scoped.insert_type(root, ty, id, &mut self.scoped_journal, &mut self.scoped_touched);
        if ty.name() == "let" {
            self.scoped.insert_let(parent, id, &mut self.scoped_journal, &mut self.scoped_touched);
        }
        for k in kinds {
            self.scoped.insert_facet(root, k, id, &mut self.scoped_journal, &mut self.scoped_touched);
        }
        self.scoped_touched.on = on;
    }

    /// Delete one live authored node (its authored children already gone):
    /// tears down any virtual views owned by `id` immediately so mid-transaction
    /// invariants hold, records `gone`, and tombstones `id` in `authored`.
    pub(crate) fn delete_node(&mut self, id: NodeId, at: crate::id::Version) {
        debug_assert!(!id.is_expanded(), "delete_node targets authored nodes only");
        let (went, ty, kinds, depth) = {
            let tree = Tree::of(self);
            let node = tree.node(id).expect("a live node is deleted");
            let depth = tree.depth(id);
            let went = crate::changes::Went {
                parent: node.parent,
                depth,
            };
            let kinds: Vec<FacetKind> = tree.facets_of(id).map(Facet::kind).collect();
            (went, node.ty, kinds, depth)
        };
        // Tear down any virtual views owned by this authored node before tombstoning it.
        let vd = self.views.remove_top_owner(
            id,
            depth,
            &mut self.scoped,
            &mut self.scoped_journal,
            &mut self.scoped_touched,
            &mut self.gone,
            &mut self.gone_journal,
        );
        self.pending_view_delta.extend(vd);

        let del_parent = went.parent;
        let was = self.gone.insert(id, went);
        if let Some(journal) = &mut self.gone_journal {
            journal.push((id, was));
        }
        self.authored.tombstone(id, at);
        if let Some(root) = self.scoped.root_of.get(&id).copied() {
            let on = self.scoped_touched.on;
            self.scoped_touched.on &= !crate::structure::silenced("tombstone");
            self.scoped.set_root(id, None, &mut self.scoped_journal);
            self.scoped.remove_type(root, ty, id, &mut self.scoped_journal, &mut self.scoped_touched);
            if ty.name() == "let" {
                self.scoped.remove_let(del_parent, id, &mut self.scoped_journal, &mut self.scoped_touched);
            }
            for k in kinds {
                self.scoped.remove_facet(root, k, id, &mut self.scoped_journal, &mut self.scoped_touched);
            }
            self.scoped_touched.on = on;
        }
    }

    pub(crate) fn set_parent(&mut self, id: NodeId, parent: Option<NodeId>, order: OrderKey) {
        let old_root = self.scoped.root_of.get(&id).copied();
        let old_parent_and_let = self.authored.live(id).filter(|n| n.ty.name() == "let").map(|n| n.parent);
        let new_root = self.root_for(id, parent);
        self.authored.set_parent(id, parent, order);
        if let Some(old_parent) = old_parent_and_let
            && old_parent != parent
        {
            let on = self.scoped_touched.on;
            self.scoped_touched.on &= !crate::structure::silenced("set_parent");
            self.scoped.remove_let(old_parent, id, &mut self.scoped_journal, &mut self.scoped_touched);
            self.scoped.insert_let(parent, id, &mut self.scoped_journal, &mut self.scoped_touched);
            self.scoped_touched.on = on;
        }
        if old_root == Some(new_root) {
            return;
        }
        // Move `id` and every descendant (authored AND virtual!) in `ScopedIndexes`.
        let moved: Vec<(NodeId, TypeId, Vec<FacetKind>)> = {
            let tree = Tree::of(self);
            std::iter::once(id)
                .chain(tree.descendants(id))
                .filter_map(|n| {
                    let ty = tree.ty_of(n)?;
                    if !tree.is_live(n) {
                        return None;
                    }
                    let kinds = tree.facets_of(n).map(Facet::kind).collect();
                    Some((n, ty, kinds))
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

    pub(crate) fn set_order(&mut self, id: NodeId, order: OrderKey) {
        self.authored.set_order(id, order);
    }

    pub(crate) fn insert_facet(&mut self, id: NodeId, c: Facet) {
        let kind = c.kind();
        let root = self.scoped.root_of.get(&id).copied();
        self.authored.insert_facet(id, c);
        if let Some(root) = root {
            let on = self.scoped_touched.on;
            self.scoped_touched.on &= !crate::structure::silenced("insert_facet");
            self.scoped.insert_facet(root, kind, id, &mut self.scoped_journal, &mut self.scoped_touched);
            self.scoped_touched.on = on;
        }
    }

    pub(crate) fn remove_facet(&mut self, id: NodeId, kind: FacetKind) -> Option<Facet> {
        let root = self.scoped.root_of.get(&id).copied();
        let removed = self.authored.remove_facet(id, kind)?;
        if let Some(root) = root {
            let on = self.scoped_touched.on;
            self.scoped_touched.on &= !crate::structure::silenced("remove_facet");
            self.scoped.remove_facet(root, kind, id, &mut self.scoped_journal, &mut self.scoped_touched);
            self.scoped_touched.on = on;
        }
        Some(removed)
    }

    pub(crate) fn set_leaf(&mut self, id: NodeId, path: LeafPath, slot: Slot) -> Option<Result<Slot, crate::facet::LeafError>> {
        self.authored.set_leaf(id, path, slot)
    }

    pub(crate) fn set_solve(&mut self, leaf: Leaf, slot: Option<Slot>) -> Option<Slot> {
        self.authored.set_solve(leaf, slot)
    }

    pub(crate) fn insert_relation(&mut self, rel: Relation) {
        self.authored.insert_relation(rel);
    }

    pub(crate) fn remove_relation(&mut self, id: RelationId) -> Option<Relation> {
        self.authored.remove_relation(id)
    }

    /// Reconcile one top-level authored owner's virtual views, updating
    /// `VirtualViews`, `ScopedIndexes`, `Docs::gone`, and touched keys.
    pub(crate) fn reconcile_owner(&mut self, top_owner: NodeId, plan: OwnerPlan) -> ViewDelta {
        let doc_root = self.scoped.root_of.get(&top_owner).copied();
        let owner_depth = if self.authored.node(top_owner).is_some() {
            Tree::of(self).depth(top_owner)
        } else {
            self.gone.get(&top_owner).map_or(0, |w| w.depth)
        };
        let delta = self.views.reconcile_top_owner(
            top_owner,
            doc_root,
            owner_depth,
            plan,
            &mut self.scoped,
            &mut self.scoped_journal,
            &mut self.scoped_touched,
            &mut self.gone,
            &mut self.gone_journal,
        );
        delta
    }

    pub(crate) fn take_pending_view_delta(&mut self) -> ViewDelta {
        std::mem::take(&mut self.pending_view_delta)
    }

    /// Collect what writes touch, across authored, virtual views, and scoped indexes.
    pub(crate) fn log_touched(&mut self) {
        self.authored.set_touched(true);
        self.views.set_touched(true);
        self.scoped_touched.on = true;
        self.scoped_touched.keys.clear();
        self.view_cursor = (0, 0, 0);
    }

    /// Keys touched since the last `drain_view_touched`, `take_touched`, or
    /// `rollback`: `(authored_keys, view_and_scoped_keys)`. Templates depend
    /// strictly on authored keys, whereas owners may also depend on virtual
    /// views or scoped indexes.
    pub(crate) fn drain_view_touched(&mut self) -> (Vec<crate::structure::Key>, Vec<crate::structure::Key>) {
        let a = self.authored.touched_slice();
        let v = self.views.touched_slice();
        let s = &self.scoped_touched.keys;
        let (ia, iv, is) = self.view_cursor;
        let authored = if ia < a.len() { a[ia..].to_vec() } else { Vec::new() };
        let mut derived = Vec::new();
        if iv < v.len() {
            derived.extend_from_slice(&v[iv..]);
        }
        if is < s.len() {
            derived.extend_from_slice(&s[is..]);
        }
        self.view_cursor = (a.len(), v.len(), s.len());
        (authored, derived)
    }

    /// The keys all writes touched since they were last taken.
    pub(crate) fn take_touched(&mut self) -> Vec<crate::structure::Key> {
        self.view_cursor = (0, 0, 0);
        let mut keys = self.authored.take_touched();
        keys.append(&mut self.views.take_touched());
        keys.append(&mut self.scoped_touched.keys);
        keys
    }

    pub(crate) fn touched_empty(&self) -> bool {
        self.authored.touched_empty() && self.views.touched_empty() && self.scoped_touched.keys.is_empty()
    }

    pub(crate) fn begin(&mut self) {
        debug_assert!(self.touched_empty(), "a transaction begins with touched keys not handed to the graph");
        self.authored.begin();
        self.views.begin();
        self.gone_journal = Some(Vec::new());
        self.scoped_journal = Some(Vec::new());
        self.pending_view_delta = ViewDelta::default();
    }

    pub(crate) fn commit(&mut self) {
        self.authored.commit();
        self.views.commit();
        self.gone_journal = None;
        self.scoped_journal = None;
    }

    pub(crate) fn rollback(&mut self) {
        self.authored.rollback();
        self.views.rollback();
        self.scoped_touched.keys.clear();
        self.view_cursor = (0, 0, 0);
        self.pending_view_delta = ViewDelta::default();
        if let Some(recs) = self.scoped_journal.take() {
            self.scoped.undo(recs);
        }
        for (id, was) in self.gone_journal.take().into_iter().flatten().rev() {
            match was {
                Some(w) => self.gone.insert(id, w),
                None => self.gone.remove(&id),
            };
        }
        #[cfg(feature = "differential")]
        if let Err(e) = Tree::of(self).check_invariants() {
            panic!("journal rollback broke an invariant: {e}");
        }
    }
}

/// A read view of the document, authored nodes and virtual derived views alike.
#[derive(Clone, Copy)]
pub struct Tree<'a> {
    doc: &'a Document,
    views: Option<&'a VirtualViews>,
    scoped: Option<&'a ScopedIndexes>,
    cyclic_embeds: Option<&'a BTreeSet<NodeId>>,
    rec: Option<&'a crate::structure::Recorder>,
}

impl<'a> Tree<'a> {
    pub(crate) fn of(docs: &'a Docs) -> Tree<'a> {
        Tree {
            doc: &docs.authored,
            views: Some(&docs.views),
            scoped: Some(&docs.scoped),
            cyclic_embeds: docs.cyclic_embeds.as_ref(),
            rec: None,
        }
    }

    pub(crate) fn recording(self, rec: &'a crate::structure::Recorder) -> Tree<'a> {
        Tree { rec: Some(rec), ..self }
    }

    /// O(1) post-commit cycle status for an embed on non-recording `Tree`s.
    /// Returns `None` on recording `Tree`s (so `LayoutNative` walks & records
    /// `Key::In`/`Key::Facet` dependencies) and unsynced/standalone `Document`s.
    #[inline]
    pub(crate) fn is_cyclic_embed(self, id: NodeId) -> Option<bool> {
        if self.rec.is_some() {
            return None;
        }
        let set = self.cyclic_embeds?;
        Some(set.contains(&id))
    }

    fn note(self, k: crate::structure::Key) {
        if let Some(r) = self.rec {
            r.borrow_mut().push(k);
        }
    }

    fn unkeyed(self, what: &str) {
        debug_assert!(self.rec.is_none(), "a recording tree has no key for `{what}` (engine.md §3.2)");
    }

    /// Persisted solver pose on `leaf` (`Document::solved`, EN-12).
    pub fn solve_leaf(&self, leaf: Leaf) -> Option<&'a Slot> {
        self.note(crate::structure::Key::Leaf(leaf));
        self.doc.solve_leaf(leaf)
    }

    /// Effective stored slot for `leaf`: persisted solver pose if present, else
    /// the node's own facet slot.
    pub fn stored_leaf(&self, leaf: Leaf) -> Option<&'a Slot> {
        self.solve_leaf(leaf).or_else(|| self.leaf(leaf.node, leaf.path))
    }

    /// The tree over an authored document with no virtual views attached.
    pub fn authored_only(doc: &'a Document) -> Tree<'a> {
        Tree { doc, views: None, scoped: None, cyclic_embeds: None, rec: None }
    }

    pub fn check_invariants(self) -> Result<(), String> {
        self.doc.check_invariants_over(&|p| self.is_live(p))?;
        if let Some(why) = self.doc.derived_content() {
            return Err(format!("authored document holds derived content: {why}"));
        }
        if let Some(views) = self.views {
            for (&owner, v) in &views.views {
                if !self.is_live(owner) {
                    return Err(format!("virtual view owner {owner} is not live (top_owner {})", v.top_owner));
                }
            }
            for (&repeat, recs) in &views.items {
                if !self.is_live(repeat) {
                    return Err(format!("item table repeat {repeat} is not live"));
                }
                for rec in recs.iter() {
                    if !views.item_scopes.contains_key(&rec.scope) {
                        return Err(format!("item scope {} missing reverse index", rec.scope));
                    }
                }
            }
            for rel in views.relations.values() {
                if !self.is_live(rel.from) {
                    return Err(format!("derived relation {} from dead {}", rel.id, rel.from));
                }
            }
        }
        if let Some(scoped) = self.scoped {
            let mut expected_root: BTreeMap<NodeId, NodeId> = BTreeMap::new();
            let mut expected_type: BTreeMap<(NodeId, TypeId), BTreeSet<NodeId>> = BTreeMap::new();
            let mut expected_facet: BTreeMap<(NodeId, FacetKind), BTreeSet<NodeId>> = BTreeMap::new();
            let mut stack: Vec<(NodeId, NodeId)> = self.doc.roots().iter().map(|&r| (r, r)).collect();
            while let Some((id, root)) = stack.pop() {
                let Some(ty) = self.ty_of(id) else { continue };
                if !self.is_live(id) {
                    continue;
                }
                expected_root.insert(id, root);
                expected_type.entry((root, ty)).or_default().insert(id);
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

    // ---- Zero-alloc metadata helpers ----

    #[inline]
    pub fn parent_of(self, id: NodeId) -> Option<NodeId> {
        self.note(crate::structure::Key::Node(id));
        if id.is_expanded() {
            self.views.and_then(|v| v.parent_of(id))
        } else {
            self.doc.node(id).and_then(|n| n.parent)
        }
    }

    #[inline]
    pub fn ty_of(self, id: NodeId) -> Option<TypeId> {
        self.note(crate::structure::Key::Node(id));
        if id.is_expanded() {
            self.views.and_then(|v| v.ty_of(id))
        } else {
            self.doc.node(id).map(|n| n.ty)
        }
    }

    #[inline]
    pub fn order_of(self, id: NodeId) -> Option<&'a OrderKey> {
        self.note(crate::structure::Key::Node(id));
        if id.is_expanded() {
            self.views.and_then(|v| v.order_of(id))
        } else {
            self.doc.node(id).map(|n| &n.order)
        }
    }

    /// Any node, live or tombstoned.
    pub fn node(self, id: NodeId) -> Option<Cow<'a, Node>> {
        self.note(crate::structure::Key::Node(id));
        if id.is_expanded() {
            self.views.and_then(|v| v.node(id)).map(Cow::Owned)
        } else {
            self.doc.node(id).map(Cow::Borrowed)
        }
    }

    pub fn live(self, id: NodeId) -> Option<Cow<'a, Node>> {
        self.note(crate::structure::Key::Node(id));
        if id.is_expanded() {
            self.views.and_then(|v| v.node(id)).map(Cow::Owned)
        } else {
            self.doc.live(id).map(Cow::Borrowed)
        }
    }

    pub fn is_live(self, id: NodeId) -> bool {
        self.note(crate::structure::Key::Node(id));
        if id.is_expanded() {
            self.views.is_some_and(|v| v.is_live(id))
        } else {
            self.doc.is_live(id)
        }
    }

    pub fn nodes(self) -> impl Iterator<Item = Cow<'a, Node>> + 'a {
        self.unkeyed("nodes");
        let virtuals: Vec<Node> = self.expanded_ids_inner().filter_map(|id| self.views?.node(id)).collect();
        self.doc.nodes().map(Cow::Borrowed).chain(virtuals.into_iter().map(Cow::Owned))
    }

    pub fn live_nodes(self) -> impl Iterator<Item = Cow<'a, Node>> + 'a {
        self.unkeyed("live_nodes");
        let virtuals: Vec<Node> = self.expanded_ids_inner().filter_map(|id| self.views?.node(id)).collect();
        self.doc.live_nodes().map(Cow::Borrowed).chain(virtuals.into_iter().map(Cow::Owned))
    }

    pub fn roots(self) -> &'a [NodeId] {
        self.note(crate::structure::Key::Roots);
        self.doc.roots()
    }

    /// O(1) top-level root containing live node `id` (authored or expanded),
    /// via `ScopedIndexes::root_of`. Unkeyed — used only by internal indexes.
    pub(crate) fn root_of(self, id: NodeId) -> Option<NodeId> {
        let anchor = id.expanded_root().unwrap_or(id);
        self.scoped?.root_of.get(&anchor).copied()
    }

    pub fn last_root(self) -> Option<NodeId> {
        self.unkeyed("last_root");
        self.doc.last_root()
    }

    /// Authored live children of `id` only (skips virtual item scopes / derived
    /// children while still recording `Key::Children(id)`).
    pub(crate) fn authored_children(self, id: NodeId) -> &'a [NodeId] {
        self.note(crate::structure::Key::Children(id));
        if id.is_expanded() { &[] } else { self.doc.children(id) }
    }

    /// Live children in sibling `(order, id)` order.
    pub fn children(self, id: NodeId) -> Cow<'a, [NodeId]> {
        self.note(crate::structure::Key::Children(id));
        if id.is_expanded() {
            let derived = self.views.map(|v| v.derived_children(id)).unwrap_or_default();
            return Cow::Owned(derived);
        }
        let mine = self.doc.children(id);
        let Some(views) = self.views else { return Cow::Borrowed(mine) };
        let derived = views.derived_children(id);
        if derived.is_empty() {
            return Cow::Borrowed(mine);
        }
        if mine.is_empty() {
            return Cow::Owned(derived);
        }
        let key = |n: &NodeId| {
            let ord = if n.is_expanded() {
                views.order_of(*n).expect("derived child exists")
            } else {
                &self.doc.node(*n).expect("authored child exists").order
            };
            (ord, *n)
        };
        let mut out = Vec::with_capacity(mine.len() + derived.len());
        let (mut a, mut b) = (mine.iter().peekable(), derived.iter().peekable());
        while let (Some(x), Some(y)) = (a.peek(), b.peek()) {
            if key(x) <= key(y) {
                out.push(**x);
                a.next();
            } else {
                out.push(**y);
                b.next();
            }
        }
        out.extend(a.copied());
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

    /// Sibling order from the top of `id`'s tree down to `id` (zero-alloc `&'a OrderKey`).
    pub fn order_path(self, id: NodeId) -> Vec<&'a OrderKey> {
        let mut out = Vec::new();
        let mut cur = Some(id);
        while let Some(n) = cur {
            if !self.is_live(n) {
                break;
            }
            let Some(ord) = self.order_of(n) else { break };
            out.push(ord);
            cur = self.parent_of(n);
        }
        out.reverse();
        out
    }

    fn expanded_ids_inner(self) -> impl Iterator<Item = NodeId> + 'a {
        let mut ids: Vec<NodeId> = Vec::new();
        if let Some(scoped) = self.scoped {
            ids.extend(scoped.root_of.keys().copied().filter(|id| id.is_expanded()));
        } else if let Some(views) = self.views {
            for v in views.views.values() {
                for idx in 0..v.limit {
                    ids.push(v.node_id(idx));
                }
            }
            for &scope in views.item_scopes.keys() {
                ids.push(scope);
            }
            ids.sort();
        }
        ids.into_iter()
    }

    /// Every live expanded node's id, in sorted `NodeId` order.
    pub fn expanded_ids(self) -> impl Iterator<Item = NodeId> + 'a {
        self.unkeyed("expanded_ids");
        self.expanded_ids_inner()
    }

    pub fn depth(self, id: NodeId) -> usize {
        let mut d = 0;
        let mut cur = self.parent_of(id);
        while let Some(p) = cur {
            d += 1;
            cur = self.parent_of(p);
        }
        d
    }

    pub fn is_ancestor(self, a: NodeId, of: NodeId) -> bool {
        let mut cur = self.parent_of(of);
        while let Some(p) = cur {
            if p == a {
                return true;
            }
            cur = self.parent_of(p);
        }
        false
    }

    pub fn nodes_of_type(self, ty: TypeId) -> impl Iterator<Item = NodeId> + 'a {
        self.unkeyed("nodes_of_type");
        let virtuals = self.scoped.and_then(|s| s.virtual_by_type.get(&ty)).into_iter().flatten().copied();
        self.doc.nodes_of_type(ty).chain(virtuals)
    }

    pub fn nodes_with(self, kind: FacetKind) -> impl Iterator<Item = NodeId> + 'a {
        self.unkeyed("nodes_with");
        let virtuals = self.scoped.and_then(|s| s.virtual_by_facet.get(&kind)).into_iter().flatten().copied();
        self.doc.nodes_with(kind).chain(virtuals)
    }

    /// Live nodes of `ty` under top-level `root` (engine.md §3.2).
    pub fn nodes_of_type_in(self, root: NodeId, ty: TypeId) -> Vec<NodeId> {
        self.note(crate::structure::Key::In(root, ty));
        if let Some(scoped) = self.scoped {
            return scoped.by_type.get(&(root, ty)).into_iter().flatten().copied().collect();
        }
        self.doc
            .nodes_of_type(ty)
            .filter(|&id| self.top_root(id) == Some(root))
            .collect()
    }

    /// Direct live `let` children of `parent` (`None`: top-level root `let`s),
    /// keyed by `Key::LetsIn(parent)` so `@name` lookups touch only ancestor scopes.
    pub fn lets_in(self, parent: Option<NodeId>) -> Vec<NodeId> {
        self.note(crate::structure::Key::LetsIn(parent));
        if let Some(scoped) = self.scoped {
            return scoped.lets_by_parent.get(&parent).into_iter().flatten().copied().collect();
        }
        let let_ty = TypeId::lookup("let").expect("registered");
        let list = match parent {
            Some(p) => self.doc.children(p),
            None => self.doc.roots(),
        };
        list.iter()
            .copied()
            .filter(|&id| self.doc.live(id).is_some_and(|n| n.ty == let_ty))
            .collect()
    }

    /// Live nodes carrying `kind` under top-level `root` (engine.md §3.2).
    pub fn nodes_with_in(self, root: NodeId, kind: FacetKind) -> Vec<NodeId> {
        self.note(crate::structure::Key::With(root, kind));
        if let Some(scoped) = self.scoped {
            return scoped.by_facet.get(&(root, kind)).into_iter().flatten().copied().collect();
        }
        self.doc
            .nodes_with(kind)
            .filter(|&id| self.top_root(id) == Some(root))
            .collect()
    }

    pub fn top_root(self, id: NodeId) -> Option<NodeId> {
        if let Some(scoped) = self.scoped {
            return scoped.root_of.get(&id).copied();
        }
        if !self.is_live(id) {
            return None;
        }
        let mut cur = id;
        while let Some(p) = self.parent_of(cur) {
            if !self.is_live(p) {
                return None;
            }
            cur = p;
        }
        Some(cur)
    }

    /// A facet, zero-alloc borrowed from `Document`, `View::sparse_facets`, or
    /// shared `Arc<Template>`.
    pub fn facet(self, id: NodeId, kind: FacetKind) -> Option<&'a Facet> {
        self.note(crate::structure::Key::Facet(id, kind));
        if id.is_expanded() {
            self.views.and_then(|v| v.facet(id, kind))
        } else {
            self.doc.facet(id, kind)
        }
    }

    /// Whether `id` carries `kind`, keyed by `Key::FacetSet`.
    pub fn has(self, id: NodeId, kind: FacetKind) -> bool {
        self.note(crate::structure::Key::FacetSet(id));
        if id.is_expanded() {
            self.views.is_some_and(|v| v.has(id, kind))
        } else {
            self.doc.facet(id, kind).is_some()
        }
    }

    pub fn get<T: LeafStruct>(self, id: NodeId) -> Option<&'a T> {
        self.note(crate::structure::Key::Facet(id, T::KIND));
        let f = if id.is_expanded() {
            self.views.and_then(|v| v.facet(id, T::KIND))?
        } else {
            return self.doc.get::<T>(id);
        };
        T::unwrap_ref(f)
    }

    /// All facets of a node, in `FacetKind` order.
    pub fn facets_of(self, id: NodeId) -> impl Iterator<Item = &'a Facet> + 'a {
        let out: Vec<&'a Facet> = if id.is_expanded() {
            self.views.map(|v| v.facets_of(id)).unwrap_or_default()
        } else {
            self.doc.facets_of(id).collect()
        };
        if self.rec.is_some() {
            self.note(crate::structure::Key::FacetSet(id));
            for f in &out {
                self.note(crate::structure::Key::Facet(id, f.kind()));
            }
        }
        out.into_iter()
    }

    pub fn leaf(self, id: NodeId, path: LeafPath) -> Option<&'a Slot> {
        self.note(crate::structure::Key::Facet(id, path.facet));
        if id.is_expanded() {
            self.views.and_then(|v| v.leaf(id, path))
        } else {
            self.doc.leaf(id, path)
        }
    }

    /// Every bound slot on a live node, facet kind by kind, and by `NodeId`
    /// within each kind.
    pub fn bound_slots(self) -> impl Iterator<Item = (Leaf, &'a Expr)> + 'a {
        self.unkeyed("bound_slots");
        let mut kinds: BTreeSet<FacetKind> = self.doc.raw_facets().keys().copied().collect();
        if let Some(views) = self.views {
            views.bound_facet_kinds(&mut kinds);
        }
        kinds.into_iter().flat_map(move |kind| {
            let v: Vec<(Leaf, &'a Expr)> = self.views.map(|vw| vw.bound_slots_of(kind)).unwrap_or_default();
            self.doc.bound_slots_of(kind).chain(v)
        })
    }

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
        if id.is_expanded() {
            self.views.and_then(|v| v.relation(id))
        } else {
            self.doc.relation(id)
        }
    }

    pub fn relations(self) -> impl Iterator<Item = &'a Relation> + 'a {
        self.unkeyed("relations");
        self.doc.relations().chain(self.views.into_iter().flat_map(|v| v.relations()))
    }

    pub fn relations_from(self, id: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.note(crate::structure::Key::Relations(id));
        self.doc.relations_from(id).chain(self.views.into_iter().flat_map(move |v| v.relations_from(id)))
    }

    pub fn relations_to(self, id: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.note(crate::structure::Key::Relations(id));
        self.doc.relations_to(id).chain(self.views.into_iter().flat_map(move |v| v.relations_to(id)))
    }
}

#[cfg(test)]
mod tests {
    use crate::facet::{Composition, Name};
    use crate::*;

    /// `with_document` takes authored documents only: one holding a derived copy
    /// is refused.
    #[test]
    #[should_panic(expected = "with_document takes an authored document")]
    fn with_document_refuses_a_document_holding_a_copy() {
        let mut doc = Document::new();
        let comp = doc.next_id();
        let r = comp.offset(1);
        let copy = NodeId::from_path(&[comp, r]).unwrap();
        doc.insert_node(
            Node { id: comp, ty: TypeId::lookup("composition").unwrap(), parent: None, order: OrderKey::first(), tombstoned: None },
            vec![Name::default().wrap(), Composition::default().wrap()],
        );
        doc.insert_node(
            Node { id: copy, ty: TypeId::lookup("rect").unwrap(), parent: Some(comp), order: OrderKey::first(), tombstoned: None },
            vec![],
        );
        assert!(doc.derived_content().is_some_and(|why| why.contains("derived copy")));
        StoreCore::with_document(doc);
    }

    /// Scoped per-root indexes (`nodes_of_type_in`, `nodes_with_in`) partition
    /// authored nodes and virtual derived nodes by top-level root, follow
    /// reparents across roots, and roll back cleanly.
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
        let derived_in_a: Vec<NodeId> = store.tree().nodes_with_in(a, FacetKind::Transform2d).into_iter().filter(|id| id.is_expanded()).collect();
        assert!(!derived_in_a.is_empty(), "virtual copies under g belong to root A");

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
        assert!(!derived_in_b.is_empty(), "virtual copies under g moved to root B");

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

    #[test]
    #[should_panic(expected = "a recording tree has no key for `version`")]
    fn recording_tree_rejects_unkeyed_version() {
        let doc = Document::new();
        let rec = crate::structure::Recorder::default();
        let _ = Tree::authored_only(&doc).recording(&rec).version();
    }

    #[test]
    fn every_choke_point_canary_silences_its_keys() {
        use super::Docs;
        use crate::structure::{Key, silence_for_canary};
        let mut docs = Docs::new(Document::new());
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

        docs.insert_node(Node { id: a, ty: comp_ty, parent: None, order: OrderKey::first(), tombstoned: None }, vec![facet::Name::default().wrap()]);
        docs.insert_node(Node { id: b, ty: comp_ty, parent: None, order: OrderKey::after(&OrderKey::first()), tombstoned: None }, vec![facet::Name::default().wrap()]);
        docs.insert_node(Node { id: r, ty: rect_ty, parent: Some(a), order: OrderKey::first(), tombstoned: None }, vec![facet::Transform2d::default().wrap()]);
        let _ = docs.take_touched();

        let (e, s) = run(&docs, "set_leaf", |d| {
            let _ = d.set_leaf(r, "transform2d.x".parse().unwrap(), Slot::Const(Value::Number(42.0)));
        });
        assert!(e.contains(&Key::Leaf(Leaf { node: r, path: "transform2d.x".parse().unwrap() })), "{e:?}");
        assert!(s.is_empty(), "set_leaf silenced: {s:?}");

        let (e, s) = run(&docs, "set_solve", |d| {
            let _ = d.set_solve(Leaf { node: copy, path: "transform2d.x".parse().unwrap() }, Some(Slot::Const(Value::Number(17.0))));
        });
        assert!(e.contains(&Key::Leaf(Leaf { node: copy, path: "transform2d.x".parse().unwrap() })), "{e:?}");
        assert!(s.is_empty(), "set_solve silenced: {s:?}");

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
    }
}
