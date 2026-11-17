//! The read interface (reactive-engine.md §12.1; handoff §4.2): what
//! answers for authored and derived nodes alike. In phase 1 it forwards to
//! the document, which still stores derived copies; it is a type of its own,
//! not `&Document`, so that later phases can change what is behind it
//! without its callers changing. Get one from `Store::tree()`, or inside the
//! crate from any document with `Tree::over`.
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

/// A read view of the document's nodes, authored and derived.
#[derive(Clone, Copy)]
pub struct Tree<'a> {
    doc: &'a Document,
}

impl<'a> Tree<'a> {
    /// The tree over `doc`: what the store reads through, and what a check
    /// against a working document that is not the store's reads through.
    pub fn over(doc: &'a Document) -> Tree<'a> {
        Tree { doc }
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
        self.doc.node(id)
    }

    pub fn live(self, id: NodeId) -> Option<&'a Node> {
        self.doc.live(id)
    }

    pub fn is_live(self, id: NodeId) -> bool {
        self.doc.is_live(id)
    }

    pub fn nodes(self) -> impl Iterator<Item = &'a Node> + 'a {
        self.doc.nodes()
    }

    pub fn live_nodes(self) -> impl Iterator<Item = &'a Node> + 'a {
        self.doc.live_nodes()
    }

    pub fn roots(self) -> &'a [NodeId] {
        self.doc.roots()
    }

    /// Live children in sibling order.
    pub fn children(self, id: NodeId) -> &'a [NodeId] {
        self.doc.children(id)
    }

    /// Live descendants, preorder.
    pub fn descendants(self, id: NodeId) -> Vec<NodeId> {
        self.doc.descendants(id)
    }

    /// The preorder of `id` and its descendants, up to `limit` nodes: `None`
    /// once there are more. What a caller that only wants a small subtree
    /// uses instead of [`Document::descendants`], which walks all of it.
    pub fn descendants_within(self, id: NodeId, limit: usize) -> Option<Vec<NodeId>> {
        self.doc.descendants_within(id, limit)
    }

    /// Sibling order from the top of `id`'s tree down to `id`: sorting
    /// nodes by it puts them in preorder (painter order), whatever set they
    /// come from, with no walk of the content around them.
    pub fn order_path(self, id: NodeId) -> Vec<&'a OrderKey> {
        self.doc.order_path(id)
    }

    /// Every expanded node's id, live or tombstoned (component-model.md §6).
    /// Expanded ids sort after every other id, so this is a range read.
    pub fn expanded_ids(self) -> impl Iterator<Item = NodeId> + 'a {
        self.doc.expanded_ids()
    }

    /// How many ancestors a node has.
    pub fn depth(self, id: NodeId) -> usize {
        self.doc.depth(id)
    }

    /// Whether `a` is a strict ancestor of `of`.
    pub fn is_ancestor(self, a: NodeId, of: NodeId) -> bool {
        self.doc.is_ancestor(a, of)
    }

    pub fn nodes_of_type(self, ty: TypeId) -> impl Iterator<Item = NodeId> + 'a {
        self.doc.nodes_of_type(ty)
    }

    pub fn nodes_with(self, kind: FacetKind) -> impl Iterator<Item = NodeId> + 'a {
        self.doc.nodes_with(kind)
    }

    pub fn facet(self, id: NodeId, kind: FacetKind) -> Option<&'a Facet> {
        self.doc.facet(id, kind)
    }

    pub fn get<T: LeafStruct>(self, id: NodeId) -> Option<&'a T> {
        self.doc.get::<T>(id)
    }

    /// All facets of a node, in kind order.
    pub fn facets_of(self, id: NodeId) -> impl Iterator<Item = &'a Facet> + 'a {
        self.doc.facets_of(id)
    }

    pub fn leaf(self, id: NodeId, path: LeafPath) -> Option<&'a Slot> {
        self.doc.leaf(id, path)
    }

    /// Every bound slot on a live node, with its expression.
    pub fn bound_slots(self) -> impl Iterator<Item = (Leaf, &'a Expr)> + 'a {
        self.doc.bound_slots()
    }

    /// Whether binding `target` to `expr` would let a value depend on itself,
    /// following existing bindings. Checked at apply time so the graph is
    /// always a DAG.
    pub fn binding_cycles(self, target: Leaf, expr: &Expr) -> bool {
        self.doc.binding_cycles(target, expr)
    }

    /// Whether `expr` reads interaction state (hover, pressed, selected),
    /// directly or through other bindings: its value is then presentation,
    /// not content (reactivity.md §4.2).
    pub fn reads_interaction(self, expr: &Expr) -> bool {
        self.doc.reads_interaction(expr)
    }

    pub fn relation(self, id: RelationId) -> Option<&'a Relation> {
        self.doc.relation(id)
    }

    pub fn relations(self) -> impl Iterator<Item = &'a Relation> + 'a {
        self.doc.relations()
    }

    /// Edges leaving `id`.
    pub fn relations_from(self, id: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.doc.relations_from(id)
    }

    /// Edges arriving at `id`: "what drives this node" (doc 05 §3.1).
    pub fn relations_to(self, id: NodeId) -> impl Iterator<Item = &'a Relation> + 'a {
        self.doc.relations_to(id)
    }
}
