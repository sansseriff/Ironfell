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
use std::collections::BTreeSet;

/// The store's documents (handoff §4.6): the authored one, and the one
/// holding derived copies when they are kept apart. With `apart` false every
/// node, copies included, is in `authored` and `copies` stays empty: 2b's
/// layout, the differential reference for phase 2c.
#[derive(Clone)]
pub(crate) struct Docs {
    pub(crate) authored: Document,
    pub(crate) copies: Document,
    pub(crate) apart: bool,
    /// Each node deleted since the changes were last taken, as it was when
    /// it went: a copy leaves no tombstone to read it from afterwards
    /// (`Changes::note`, `Store::regraph`).
    pub(crate) gone: crate::changes::Gone,
    /// While a transaction is open, what each of its deletes replaced in
    /// `gone`, so a rollback takes them back out.
    gone_journal: Option<Vec<(NodeId, Option<crate::changes::Went>)>>,
}

impl Docs {
    pub(crate) fn new(authored: Document, apart: bool) -> Docs {
        Docs { authored, copies: Document::new(), apart, gone: Default::default(), gone_journal: None }
    }

    /// Delete one live node, its children already gone: a copy kept apart
    /// is removed outright, anything else tombstoned. What it was is kept
    /// in `gone`.
    pub(crate) fn delete_node(&mut self, id: NodeId, at: crate::id::Version) {
        let tree = Tree::of(self);
        let node = tree.node(id).expect("a live node is deleted");
        let went = crate::changes::Went {
            parent: node.parent,
            depth: tree.depth(id),
            ty: node.ty,
            port_or_anchor: tree.facet(id, FacetKind::Port).is_some() || tree.facet(id, FacetKind::Anchor).is_some(),
        };
        let was = self.gone.insert(id, went);
        if let Some(journal) = &mut self.gone_journal {
            journal.push((id, was));
        }
        if self.apart && id.is_expanded() {
            self.copies.remove_node(id);
        } else {
            self.authored.tombstone(id, at);
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

    /// Open both documents' journals.
    pub(crate) fn begin(&mut self) {
        self.authored.begin();
        self.copies.begin();
        self.gone_journal = Some(Vec::new());
    }

    pub(crate) fn commit(&mut self) {
        self.authored.commit();
        self.copies.commit();
        self.gone_journal = None;
    }

    pub(crate) fn rollback(&mut self) {
        self.authored.rollback();
        self.copies.rollback();
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

    /// Writes to either document, a key for caches of what is derived from
    /// them. A rollback counts as a write.
    pub(crate) fn edits(&self) -> u64 {
        self.authored.edits() + self.copies.edits()
    }
}

/// A read view of the nodes, authored and derived.
#[derive(Clone, Copy)]
pub struct Tree<'a> {
    doc: &'a Document,
    /// The copies, when they are kept apart from `doc`.
    copies: Option<&'a Document>,
}

impl<'a> Tree<'a> {
    /// The tree over a store's documents.
    pub(crate) fn of(docs: &'a Docs) -> Tree<'a> {
        Tree { doc: &docs.authored, copies: docs.apart.then_some(&docs.copies) }
    }

    /// The tree over a document that holds no copies apart from it: a
    /// scratch document cloned for planning, a fragment, a document being
    /// loaded. A derived node it does not hold is not there.
    pub fn authored_only(doc: &'a Document) -> Tree<'a> {
        Tree { doc, copies: None }
    }

    /// Each document's invariants, with parents answered across both (a
    /// copy's owner is authored), and that each node is in the document its
    /// id says. A live node's parent is live, so no live copy is under a
    /// dead authored ancestor.
    pub fn check_invariants(self) -> Result<(), String> {
        self.doc.check_invariants_over(&|p| self.is_live(p))?;
        let Some(copies) = self.copies else { return Ok(()) };
        if let Some(n) = self.doc.nodes().find(|n| n.id.is_expanded()) {
            return Err(format!("derived {} is in the authored document", n.id));
        }
        if let Some(n) = copies.nodes().find(|n| !n.id.is_expanded()) {
            return Err(format!("{} is in the copies document", n.id));
        }
        copies.check_invariants_over(&|p| self.is_live(p))
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
        self.at(id).node(id)
    }

    pub fn live(self, id: NodeId) -> Option<&'a Node> {
        self.at(id).live(id)
    }

    pub fn is_live(self, id: NodeId) -> bool {
        self.at(id).is_live(id)
    }

    pub fn nodes(self) -> impl Iterator<Item = &'a Node> + 'a {
        self.both().flat_map(Document::nodes)
    }

    pub fn live_nodes(self) -> impl Iterator<Item = &'a Node> + 'a {
        self.both().flat_map(Document::live_nodes)
    }

    pub fn roots(self) -> &'a [NodeId] {
        self.doc.roots()
    }

    /// Live children in sibling order. Merged by (order, id), the
    /// documents' own tie-break, where both hold children of `id`: only at
    /// an owner, the authored parent of its copies.
    pub fn children(self, id: NodeId) -> Cow<'a, [NodeId]> {
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
        self.both().flat_map(move |d| d.nodes_of_type(ty))
    }

    pub fn nodes_with(self, kind: FacetKind) -> impl Iterator<Item = NodeId> + 'a {
        self.both().flat_map(move |d| d.nodes_with(kind))
    }

    pub fn facet(self, id: NodeId, kind: FacetKind) -> Option<&'a Facet> {
        self.at(id).facet(id, kind)
    }

    pub fn get<T: LeafStruct>(self, id: NodeId) -> Option<&'a T> {
        self.at(id).get::<T>(id)
    }

    /// All facets of a node, in kind order.
    pub fn facets_of(self, id: NodeId) -> impl Iterator<Item = &'a Facet> + 'a {
        self.at(id).facets_of(id)
    }

    pub fn leaf(self, id: NodeId, path: LeafPath) -> Option<&'a Slot> {
        self.at(id).leaf(id, path)
    }

    /// Every bound slot on a live node, with its expression: facet kind by
    /// kind, and by id within a kind, as one document gives them.
    pub fn bound_slots(self) -> impl Iterator<Item = (Leaf, &'a Expr)> + 'a {
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
        match self.copies {
            Some(c) if id.is_expanded() => c.relation(id),
            _ => self.doc.relation(id),
        }
    }

    pub fn relations(self) -> impl Iterator<Item = &'a Relation> + 'a {
        self.both().flat_map(Document::relations)
    }

    /// Edges leaving `id`, from either document: an authored relation can
    /// end at a copy and a derived one start at an authored node.
    pub fn relations_from(self, id: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.both().flat_map(move |d| d.relations_from(id))
    }

    /// Edges arriving at `id`: "what drives this node" (doc 05 §3.1).
    pub fn relations_to(self, id: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
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
}
