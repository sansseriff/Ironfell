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
use std::collections::{BTreeMap, BTreeSet};

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
        Docs {
            authored,
            copies: Document::new(),
            apart,
            scoped,
            scoped_journal: None,
            scoped_touched: Default::default(),
            template_cursor: (0, 0, 0),
            gone: Default::default(),
            gone_journal: None,
            reshaped: crate::stamp(),
            tokens_shaped: crate::stamp(),
        }
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
    }

    pub(crate) fn set_parent(&mut self, id: NodeId, parent: Option<NodeId>, order: OrderKey) {
        let old_root = self.scoped.root_of.get(&id).copied();
        let new_root = self.root_for(id, parent);
        self.of_node(id).set_parent(id, parent, order);
        if old_root == Some(new_root) {
            return;
        }
        let moved: Vec<(NodeId, TypeId, Vec<FacetKind>)> = {
            let tree = Tree::of(self);
            std::iter::once(id)
                .chain(tree.descendants(id))
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

    pub(crate) fn set_order(&mut self, id: NodeId, order: OrderKey) {
        self.of_node(id).set_order(id, order);
    }

    pub(crate) fn insert_facet(&mut self, id: NodeId, c: Facet) {
        let kind = c.kind();
        let root = self.scoped.root_of.get(&id).copied();
        self.of_node(id).insert_facet(id, c);
        if let Some(root) = root {
            let on = self.scoped_touched.on;
            self.scoped_touched.on &= !crate::structure::silenced("insert_facet");
            self.scoped.insert_facet(root, kind, id, &mut self.scoped_journal, &mut self.scoped_touched);
            self.scoped_touched.on = on;
        }
    }

    pub(crate) fn remove_facet(&mut self, id: NodeId, kind: FacetKind) -> Option<Facet> {
        let root = self.scoped.root_of.get(&id).copied();
        let removed = self.of_node(id).remove_facet(id, kind)?;
        if let Some(root) = root {
            let on = self.scoped_touched.on;
            self.scoped_touched.on &= !crate::structure::silenced("remove_facet");
            self.scoped.remove_facet(root, kind, id, &mut self.scoped_journal, &mut self.scoped_touched);
            self.scoped_touched.on = on;
        }
        Some(removed)
    }

    pub(crate) fn set_leaf(&mut self, id: NodeId, path: LeafPath, slot: Slot) -> Option<Result<Slot, crate::facet::LeafError>> {
        self.of_node(id).set_leaf(id, path, slot)
    }

    pub(crate) fn set_solve(&mut self, leaf: Leaf, slot: Option<Slot>) -> Option<Slot> {
        self.authored.set_solve(leaf, slot)
    }

    pub(crate) fn insert_relation(&mut self, rel: Relation) {
        self.of_relation(rel.id).insert_relation(rel);
    }

    pub(crate) fn remove_relation(&mut self, id: RelationId) -> Option<Relation> {
        self.of_relation(id).remove_relation(id)
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
        self.template_cursor = (0, 0, 0);
    }

    /// Keys touched since the last `drain_template_touched`, `take_touched`,
    /// or `rollback` (for recorded-read template invalidation without removing
    /// keys from the main `take_touched` log, EN-12).
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
        #[cfg(feature = "differential")]
        if let Err(e) = Tree::of(self).check_invariants() {
            panic!("journal rollback broke an invariant: {e}");
        }
    }

}

/// A read view of the nodes, authored and derived.
#[derive(Clone, Copy)]
pub struct Tree<'a> {
    doc: &'a Document,
    /// The copies, when they are kept apart from `doc`.
    copies: Option<&'a Document>,
    /// Per-root indexes when backed by `Docs`.
    scoped: Option<&'a ScopedIndexes>,
    /// Where a cell's reads are noted, when it reads through a recording
    /// tree (engine.md §3.2).
    rec: Option<&'a crate::structure::Recorder>,
}

impl<'a> Tree<'a> {
    /// The tree over a store's documents.
    pub(crate) fn of(docs: &'a Docs) -> Tree<'a> {
        Tree { doc: &docs.authored, copies: docs.apart.then_some(&docs.copies), scoped: Some(&docs.scoped), rec: None }
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

    /// A read a recording tree has no key for: caught where debug assertions
    /// run, so it is never missed (engine.md §3.2).
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

    /// The tree over a document that holds no copies apart from it: a
    /// scratch document cloned for planning, a fragment, a document being
    /// loaded. A derived node it does not hold is not there.
    pub fn authored_only(doc: &'a Document) -> Tree<'a> {
        Tree { doc, copies: None, scoped: None, rec: None }
    }

    /// Each document's invariants, with parents answered across both (a
    /// copy's owner is authored), and that each node is in the document its
    /// id says. A live node's parent is live, so no live copy is under a
    /// dead authored ancestor.
    pub fn check_invariants(self) -> Result<(), String> {
        self.doc.check_invariants_over(&|p| self.is_live(p))?;
        if let Some(copies) = self.copies {
            if let Some(n) = self.doc.nodes().find(|n| n.id.is_expanded()) {
                return Err(format!("derived {} is in the authored document", n.id));
            }
            if let Some(n) = copies.nodes().find(|n| !n.id.is_expanded()) {
                return Err(format!("{} is in the copies document", n.id));
            }
            copies.check_invariants_over(&|p| self.is_live(p))?;
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

    /// Any node, live or tombstoned.
    pub fn node(self, id: NodeId) -> Option<&'a Node> {
        self.note(crate::structure::Key::Node(id));
        self.at(id).node(id)
    }

    pub fn live(self, id: NodeId) -> Option<&'a Node> {
        self.note(crate::structure::Key::Node(id));
        self.at(id).live(id)
    }

    pub fn is_live(self, id: NodeId) -> bool {
        self.note(crate::structure::Key::Node(id));
        self.at(id).is_live(id)
    }

    pub fn nodes(self) -> impl Iterator<Item = &'a Node> + 'a {
        self.unkeyed("nodes");
        self.both().flat_map(Document::nodes)
    }

    pub fn live_nodes(self) -> impl Iterator<Item = &'a Node> + 'a {
        self.unkeyed("live_nodes");
        self.both().flat_map(Document::live_nodes)
    }

    pub fn roots(self) -> &'a [NodeId] {
        self.note(crate::structure::Key::Roots);
        self.doc.roots()
    }

    /// The last root in root order, without visiting the others.
    pub fn last_root(self) -> Option<NodeId> {
        self.unkeyed("last_root");
        self.doc.last_root()
    }

    /// Live children in sibling order. Merged by (order, id), the
    /// documents' own tie-break, where both hold children of `id`: only at
    /// an owner, the authored parent of its copies.
    pub fn children(self, id: NodeId) -> Cow<'a, [NodeId]> {
        self.note(crate::structure::Key::Children(id));
        let mine = self.at(id).children(id);
        let Some(copies) = self.copies.filter(|_| !id.is_expanded()) else { return Cow::Borrowed(mine) };
        let derived = copies.children(id);
        if derived.is_empty() {
            return Cow::Borrowed(mine);
        }
        if mine.is_empty() {
            return Cow::Borrowed(derived);
        }
        let key = |n: &NodeId| (&self.at(*n).node(*n).expect("a child is held").order, *n);
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
        self.both().flat_map(Document::expanded_ids)
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
        self.both().flat_map(move |d| d.nodes_of_type(ty))
    }

    pub fn nodes_with(self, kind: FacetKind) -> impl Iterator<Item = NodeId> + 'a {
        self.unkeyed("nodes_with");
        self.both().flat_map(move |d| d.nodes_with(kind))
    }

    /// Live nodes of `ty` under top-level `root` (engine.md §3.2): O(matches
    /// in root) and keyed by `Key::In(root, ty)`.
    pub fn nodes_of_type_in(self, root: NodeId, ty: TypeId) -> Vec<NodeId> {
        self.note(crate::structure::Key::In(root, ty));
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
        if let Some(scoped) = self.scoped {
            return scoped.root_of.get(&id).copied();
        }
        let mut cur = self.live(id)?;
        while let Some(p) = cur.parent {
            cur = self.live(p)?;
        }
        Some(cur.id)
    }

    /// A facet, what it holds included: a recording tree notes the whole
    /// facet, so any write to it reaches the reader (§3.1).
    pub fn facet(self, id: NodeId, kind: FacetKind) -> Option<&'a Facet> {
        self.note(crate::structure::Key::Facet(id, kind));
        self.at(id).facet(id, kind)
    }

    /// Whether `id` carries `kind`, and nothing of what it holds: a reader
    /// that asks only this is not reached by a write to the facet's fields.
    pub fn has(self, id: NodeId, kind: FacetKind) -> bool {
        self.note(crate::structure::Key::FacetSet(id));
        self.at(id).facet(id, kind).is_some()
    }

    pub fn get<T: LeafStruct>(self, id: NodeId) -> Option<&'a T> {
        self.note(crate::structure::Key::Facet(id, T::KIND));
        self.at(id).get::<T>(id)
    }

    /// All facets of a node, in kind order.
    pub fn facets_of(self, id: NodeId) -> impl Iterator<Item = &'a Facet> + 'a {
        if self.rec.is_some() {
            self.note(crate::structure::Key::FacetSet(id));
            for f in self.at(id).facets_of(id) {
                self.note(crate::structure::Key::Facet(id, f.kind()));
            }
        }
        self.at(id).facets_of(id)
    }

    pub fn leaf(self, id: NodeId, path: LeafPath) -> Option<&'a Slot> {
        self.note(crate::structure::Key::Facet(id, path.facet));
        self.at(id).leaf(id, path)
    }

    /// Every bound slot on a live node, with its expression: facet kind by
    /// kind, and by id within a kind, as one document gives them.
    pub fn bound_slots(self) -> impl Iterator<Item = (Leaf, &'a Expr)> + 'a {
        self.unkeyed("bound_slots");
        let kinds: BTreeSet<FacetKind> = self.both().flat_map(|d| d.raw_facets().keys().copied()).collect();
        kinds.into_iter().flat_map(move |kind| self.both().flat_map(move |d| d.bound_slots_of(kind)))
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
        match self.copies {
            Some(c) if id.is_expanded() => c.relation(id),
            _ => self.doc.relation(id),
        }
    }

    /// The derived relations whose id names `owner`, from either document.
    pub(crate) fn rooted(self, owner: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.unkeyed("rooted");
        self.both().flat_map(move |d| d.rooted(owner))
    }

    pub fn relations(self) -> impl Iterator<Item = &'a Relation> + 'a {
        self.unkeyed("relations");
        self.both().flat_map(Document::relations)
    }

    /// Edges leaving `id`, from either document: an authored relation can
    /// end at a copy and a derived one start at an authored node.
    pub fn relations_from(self, id: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.note(crate::structure::Key::Relations(id));
        self.both().flat_map(move |d| d.relations_from(id))
    }

    /// Edges arriving at `id`: "what drives this node" (doc 05 §3.1).
    pub fn relations_to(self, id: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.note(crate::structure::Key::Relations(id));
        self.both().flat_map(move |d| d.relations_to(id))
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
    }
}
