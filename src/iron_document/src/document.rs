//! The authored store: nodes, facets, relations, and the derived indexes
//! over them.
//!
//! Every mutating method is `pub(crate)` and reachable only from the op
//! applier. That is the structural form of the rule in
//! plans/specs/document-spine.md §6.1: no code path mutates the store directly.

use crate::facet::{Facet, FacetKind, LeafPath, LeafStruct};
use crate::expr::{Expr, Leaf};
use crate::id::{NodeId, RelationId, ScopeId, Version};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub ty: TypeId,
    pub parent: Option<NodeId>,
    pub order: OrderKey,
    /// The version at which this node was deleted. Tombstoned nodes keep their
    /// record and facets so that references fail loudly and undo restores
    /// the same id (doc 04 §2.4).
    pub tombstoned: Option<Version>,
}

impl Node {
    pub fn is_live(&self) -> bool {
        self.tombstoned.is_none()
    }
}

/// A non-containment edge. First-class storage; `target="…"` in a view is a
/// rendering of one of these (doc 05 §3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub id: RelationId,
    pub from: NodeId,
    pub to: NodeId,
    pub rel: RelKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, Value>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelKind {
    /// A clip drives a property of its target.
    Animates,
    After,
    During,
    AlignsTo,
    References,
    /// A visual node is a picture of an entity (ECM §5).
    Figures,
    /// A constraint node to one of its operands; `data.role` names the
    /// operand's role and `data.anchor` the point in its local space.
    Constrains,
    /// A connection to one of its two ports; `data.end` is `a` or `b`.
    Connects,
    /// A session's `selection` node to a selected node (session tier, never
    /// saved): `data.panel` and `data.chain` say how it was reached.
    Selects,
    /// A component's port to the inner port it exposes (component-model.md
    /// §4.1, §5.5): signals cross by bindings; this says where the inner end
    /// is, so an anchor following one finds the other.
    Exposes,
    /// A session node to the content node it stands for (timeline.md
    /// §4.3): a timeline segment to its clip. Selection and the probe follow
    /// it. Session to content only; never saved.
    Projects,
}

impl RelKind {
    pub const ALL: [RelKind; 11] = [
        RelKind::Animates,
        RelKind::After,
        RelKind::During,
        RelKind::AlignsTo,
        RelKind::References,
        RelKind::Figures,
        RelKind::Constrains,
        RelKind::Connects,
        RelKind::Selects,
        RelKind::Exposes,
        RelKind::Projects,
    ];

    /// The kind [`RelKind::name`] names.
    pub fn from_name(name: &str) -> Option<RelKind> {
        RelKind::ALL.into_iter().find(|k| k.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            RelKind::Animates => "animates",
            RelKind::After => "after",
            RelKind::During => "during",
            RelKind::AlignsTo => "aligns_to",
            RelKind::References => "references",
            RelKind::Figures => "figures",
            RelKind::Constrains => "constrains",
            RelKind::Connects => "connects",
            RelKind::Selects => "selects",
            RelKind::Exposes => "exposes",
            RelKind::Projects => "projects",
        }
    }
}

/// Each client's next node and relation counter, as a file's header holds
/// them. A client missing from a map is at 1.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Counters {
    pub nodes: BTreeMap<u32, u64>,
    pub relations: BTreeMap<u32, u64>,
}

fn counter(next: &BTreeMap<u32, u64>, client: u32) -> u64 {
    next.get(&client).copied().unwrap_or(1)
}

/// Move `client`'s counter past `n`.
fn advance(next: &mut BTreeMap<u32, u64>, client: u32, n: u64) {
    let c = next.entry(client).or_insert(1);
    *c = (*c).max(n + 1);
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    nodes: BTreeMap<NodeId, Node>,
    facets: BTreeMap<FacetKind, BTreeMap<NodeId, Facet>>,
    relations: BTreeMap<RelationId, Relation>,
    /// Each client's next counter for nodes and for relations (a client
    /// missing here is at 1): ids are never reused, so these outlive the
    /// nodes and relations they counted past.
    next_ids: BTreeMap<u32, u64>,
    next_rels: BTreeMap<u32, u64>,
    /// The client new ids are minted as (`id.rs`): 0 unless a server has
    /// handed this client one for the scope. Who is editing, not content:
    /// never saved.
    client: u32,
    /// The session tier's own counters (`id::SESSION_BASE` and up).
    next_session_id: u64,
    next_session_rel: u64,
    version: Version,
    /// Which scope this document is (scopes.md §3). Identity metadata, not
    /// content: set by the store, saved in the header, never an op.
    scope: Option<ScopeId>,

    // Derived. Rebuilt incrementally by the mutators, wholesale by `reindex`.
    // Only live nodes appear in these.
    children: BTreeMap<NodeId, Vec<NodeId>>,
    roots: Vec<NodeId>,
    by_type: BTreeMap<TypeId, BTreeSet<NodeId>>,
    by_facet: BTreeMap<FacetKind, BTreeSet<NodeId>>,
    by_source: BTreeMap<NodeId, BTreeSet<RelationId>>,
    by_target: BTreeMap<NodeId, BTreeSet<RelationId>>,

    /// The open transaction's undo records (reactive-engine handoff §4.3,
    /// task 2). Outside equality and cloning: a clone has none open.
    journal: JournalCell,
    /// Bumped by every write of structure or facets: a cache of something
    /// derived from them is current while this is unchanged. Not content.
    edits: Edits,
    /// Each definition's and repeat's (or conditional's) last write under
    /// it ([`Document::scope_stamp`]). Not content.
    scopes: Scopes,
}

/// When each scope was last written under: a stamp from one counter for
/// the process, so no stamp is ever given twice, whether by a rollback, a
/// clone going its own way, or another document. A scope never written is
/// at the document's `base`. Equal to any other.
#[derive(Clone, Debug)]
struct Scopes {
    base: u64,
    last: std::collections::HashMap<NodeId, u64>,
}

fn next_stamp() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Default for Scopes {
    fn default() -> Scopes {
        Scopes { base: next_stamp(), last: Default::default() }
    }
}

impl PartialEq for Scopes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// A count of writes, equal to any other: a `Document`'s equality is about
/// its content.
#[derive(Clone, Copy, Debug, Default)]
struct Edits(u64);

impl PartialEq for Edits {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// One write, as what puts it back. Recorded in the order of the writes and
/// undone in reverse, on the raw fields, so the indexes come back exactly as
/// they were, empty keys included.
#[derive(Debug)]
enum Rec {
    Node(NodeId, Option<Node>),
    /// `facets[kind]` was created.
    FacetTable(FacetKind),
    Facet(FacetKind, NodeId, Option<Facet>),
    Relation(RelationId, Option<Relation>),
    NextId(u32, Option<u64>),
    NextRel(u32, Option<u64>),
    SessionId(u64),
    SessionRel(u64),
    Version(Version),
    TypeKey(TypeId),
    /// `by_type[ty]` gained (`true`) or lost `id`.
    ByType(TypeId, NodeId, bool),
    FacetKey(FacetKind),
    ByFacet(FacetKind, NodeId, bool),
    SourceKey(NodeId),
    BySource(NodeId, RelationId, bool),
    TargetKey(NodeId),
    ByTarget(NodeId, RelationId, bool),
    ChildrenKey(NodeId),
    /// `children[id]`, empty, was removed.
    ChildrenUnkeyed(NodeId),
    /// An id was inserted into a child list (`None`: the roots) at `pos`.
    Placed(Option<NodeId>, usize),
    /// `id` was removed from a child list at `pos`.
    Unplaced(Option<NodeId>, usize, NodeId),
}

#[derive(Default)]
struct Journal {
    recs: Vec<Rec>,
    /// Under the differential harness, the document at `begin`: every
    /// rollback is checked against it.
    #[cfg(feature = "differential")]
    shadow: Option<Box<Document>>,
}

/// A journal, or none open. Equal to any other and cloned as none, so a
/// `Document`'s equality and clones are about its content.
#[derive(Default)]
struct JournalCell(Option<Journal>);

impl Clone for JournalCell {
    fn clone(&self) -> Self {
        JournalCell(None)
    }
}

impl PartialEq for JournalCell {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for JournalCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(j) => write!(f, "journal open ({} records)", j.recs.len()),
            None => write!(f, "no journal"),
        }
    }
}

impl Document {
    pub fn new() -> Document {
        Document::default()
    }

    // ---- reads --------------------------------------------------------

    pub fn version(&self) -> Version {
        self.version
    }

    /// The scope this document belongs to, once it has been given one.
    pub fn scope(&self) -> Option<ScopeId> {
        self.scope
    }

    pub(crate) fn set_scope(&mut self, scope: Option<ScopeId>) {
        debug_assert!(self.journal.0.is_none(), "the scope is not a transaction's to change");
        self.scope = scope;
    }

    /// The id the next `create` will be given. Peek only; nothing is reserved
    /// until an op is applied. A batch creating several nodes uses
    /// consecutive ids from here, see [`Document::next_ids`].
    pub fn next_id(&self) -> NodeId {
        NodeId::authored(self.client, counter(&self.next_ids, self.client))
    }

    pub fn next_ids(&self, n: usize) -> Vec<NodeId> {
        (0..n as u64).map(|i| self.next_id().offset(i)).collect()
    }

    pub fn next_relation_id(&self) -> RelationId {
        RelationId::authored(self.client, counter(&self.next_rels, self.client))
    }

    pub fn next_relation_ids(&self, n: usize) -> Vec<RelationId> {
        (0..n as u64).map(|i| self.next_relation_id().offset(i)).collect()
    }

    /// Whether an authored `id`'s client has already counted past it: it
    /// was handed out before (live, tombstoned, or created and undone), so
    /// creating it restores a node rather than minting one. A sync server
    /// checks only uncounted ids against the client that sent them.
    pub fn counted(&self, id: NodeId) -> bool {
        id.minted().is_some_and(|(client, n)| n < counter(&self.next_ids, client))
    }

    /// [`Document::counted`], for a relation id.
    pub fn counted_relation(&self, id: RelationId) -> bool {
        id.minted().is_some_and(|(client, n)| n < counter(&self.next_rels, client))
    }

    /// Every client that has minted an id in this document, ascending.
    pub fn clients(&self) -> Vec<u32> {
        let mut all: Vec<u32> = self.next_ids.keys().chain(self.next_rels.keys()).copied().collect();
        all.sort_unstable();
        all.dedup();
        all
    }

    /// The client this document mints new ids as.
    pub fn client(&self) -> u32 {
        self.client
    }

    pub(crate) fn set_client(&mut self, client: u32) {
        assert!(client < crate::id::CLIENT_LIMIT, "client {client} is out of range");
        debug_assert!(self.journal.0.is_none(), "the client is not a transaction's to change");
        self.client = client;
    }

    /// Ids for session-tier nodes, from their own range.
    pub fn next_session_ids(&self, n: usize) -> Vec<NodeId> {
        let base = self.next_session_id.max(crate::id::SESSION_BASE);
        (0..n as u64).map(|i| NodeId::from_raw(base + i)).collect()
    }

    pub fn next_session_relation_ids(&self, n: usize) -> Vec<RelationId> {
        let base = self.next_session_rel.max(crate::id::SESSION_BASE);
        (0..n as u64).map(|i| RelationId::from_raw(base + i)).collect()
    }

    /// Any node, live or tombstoned.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn live(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id).filter(|n| n.is_live())
    }

    pub fn is_live(&self, id: NodeId) -> bool {
        self.live(id).is_some()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn live_nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values().filter(|n| n.is_live())
    }

    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    /// Live children in sibling order.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.children.get(&id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Live descendants, preorder.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
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
    /// uses instead of [`Document::descendants`], which walks all of it.
    pub fn descendants_within(&self, id: NodeId, limit: usize) -> Option<Vec<NodeId>> {
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
    pub fn order_path(&self, id: NodeId) -> Vec<&OrderKey> {
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
    pub fn expanded_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.range(NodeId::from_raw(crate::id::EXPANDED)..).map(|(id, _)| *id)
    }

    /// How many ancestors a node has.
    pub fn depth(&self, id: NodeId) -> usize {
        let mut d = 0;
        let mut cur = self.nodes.get(&id).and_then(|n| n.parent);
        while let Some(p) = cur {
            d += 1;
            cur = self.nodes.get(&p).and_then(|n| n.parent);
        }
        d
    }

    /// Whether `a` is a strict ancestor of `of`.
    pub fn is_ancestor(&self, a: NodeId, of: NodeId) -> bool {
        let mut cur = self.nodes.get(&of).and_then(|n| n.parent);
        while let Some(p) = cur {
            if p == a {
                return true;
            }
            cur = self.nodes.get(&p).and_then(|n| n.parent);
        }
        false
    }

    pub fn nodes_of_type(&self, ty: TypeId) -> impl Iterator<Item = NodeId> + '_ {
        self.by_type.get(&ty).into_iter().flatten().copied()
    }

    pub fn nodes_with(&self, kind: FacetKind) -> impl Iterator<Item = NodeId> + '_ {
        self.by_facet.get(&kind).into_iter().flatten().copied()
    }

    pub fn facet(&self, id: NodeId, kind: FacetKind) -> Option<&Facet> {
        self.facets.get(&kind)?.get(&id)
    }

    pub fn get<T: LeafStruct>(&self, id: NodeId) -> Option<&T> {
        self.facet(id, T::KIND).and_then(T::unwrap_ref)
    }

    /// All facets of a node, in kind order.
    pub fn facets_of(&self, id: NodeId) -> impl Iterator<Item = &Facet> {
        self.facets.values().filter_map(move |m| m.get(&id))
    }

    pub fn leaf(&self, id: NodeId, path: LeafPath) -> Option<&Slot> {
        self.facet(id, path.facet)?.get(path.field)
    }

    /// Every bound slot on a live node, with its expression.
    pub fn bound_slots(&self) -> impl Iterator<Item = (Leaf, &Expr)> {
        self.facets.iter().flat_map(move |(kind, table)| {
            table
                .iter()
                .filter(move |(id, _)| self.is_live(**id))
                .flat_map(move |(id, comp)| {
                    comp.field_names().into_iter().filter_map(move |field| {
                        let expr = comp.get(field)?.expr()?;
                        let path = LeafPath::new(*kind, field).expect("declared field");
                        Some((Leaf { node: *id, path }, expr))
                    })
                })
        })
    }

    /// `bound_slots` for one facet kind.
    pub(crate) fn bound_slots_of(&self, kind: FacetKind) -> impl Iterator<Item = (Leaf, &Expr)> {
        self.facets.get(&kind).into_iter().flat_map(move |table| {
            table.iter().filter(move |(id, _)| self.is_live(**id)).flat_map(move |(id, comp)| {
                comp.field_names().into_iter().filter_map(move |field| {
                    let expr = comp.get(field)?.expr()?;
                    let path = LeafPath::new(kind, field).expect("declared field");
                    Some((Leaf { node: *id, path }, expr))
                })
            })
        })
    }

    /// Whether binding `target` to `expr` would let a value depend on itself,
    /// following existing bindings. Checked at apply time so the graph is
    /// always a DAG.
    pub fn binding_cycles(&self, target: Leaf, expr: &Expr) -> bool {
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
    pub fn reads_interaction(&self, expr: &Expr) -> bool {
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

    /// How many writes of structure or facets this document has had, a key
    /// for caches of what is derived from them. Not content.
    pub(crate) fn edits(&self) -> u64 {
        self.edits.0
    }

    /// When anything under definition, repeat or conditional `id` was last
    /// written: what is compiled from its content (`engine::template`) is
    /// current while this is unchanged. Written means a node in it made,
    /// removed, moved, reordered, its facets changed, or a relation from it
    /// linked or unlinked; a rollback writes again what it puts back.
    pub(crate) fn scope_stamp(&self, id: NodeId) -> u64 {
        self.scopes.last.get(&id).copied().unwrap_or(self.scopes.base)
    }

    /// A write at `id`: a new stamp for every definition, repeat and
    /// conditional it is in, itself included. Derived nodes are in no
    /// template. A session node is copied by none, but a compiler can
    /// still see it (a picture's classes, a body's inputs), so it stamps.
    fn touch(&mut self, id: NodeId) {
        if id.is_expanded() {
            return;
        }
        let mut stamp = None;
        let mut at = Some(id);
        while let Some(n) = at {
            let Some(node) = self.nodes.get(&n) else { break };
            let scope = node.ty.name() == "definition" || [FacetKind::Repeat, FacetKind::When].iter().any(|k| self.facets.get(k).is_some_and(|m| m.contains_key(&n)));
            if scope {
                let s = *stamp.get_or_insert_with(next_stamp);
                self.scopes.last.insert(n, s);
            }
            at = node.parent;
        }
    }

    pub fn relation(&self, id: RelationId) -> Option<&Relation> {
        self.relations.get(&id)
    }

    pub fn relations(&self) -> impl Iterator<Item = &Relation> {
        self.relations.values()
    }

    /// Edges leaving `id`.
    pub fn relations_from(&self, id: NodeId) -> impl Iterator<Item = &Relation> {
        self.by_source
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|r| self.relations.get(r))
    }

    /// Edges arriving at `id`: "what drives this node" (doc 05 §3.1).
    pub fn relations_to(&self, id: NodeId) -> impl Iterator<Item = &Relation> {
        self.by_target
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|r| self.relations.get(r))
    }

    // ---- the journal ---------------------------------------------------

    /// Open a transaction's journal: from here every write is recorded until
    /// [`Document::commit`] or [`Document::rollback`]. One at a time.
    pub(crate) fn begin(&mut self) {
        debug_assert!(self.journal.0.is_none(), "a journal is already open");
        self.journal.0 = Some(Journal {
            recs: Vec::new(),
            #[cfg(feature = "differential")]
            shadow: Some(Box::new(self.clone())),
        });
    }

    /// Keep everything written since [`Document::begin`], and close the journal.
    pub(crate) fn commit(&mut self) {
        debug_assert!(self.journal.0.is_some(), "no journal to commit");
        self.journal.0 = None;
    }

    /// Put back everything written since [`Document::begin`], bit for bit,
    /// and close the journal.
    pub(crate) fn rollback(&mut self) {
        self.edits.0 += 1;
        let journal = self.journal.0.take().expect("no journal to roll back");
        // What is put back is written again: the scopes it is in now, and
        // those it is back in after.
        let mut touched = BTreeSet::new();
        for r in &journal.recs {
            match r {
                Rec::Node(id, _) | Rec::Facet(_, id, _) => {
                    touched.insert(*id);
                }
                Rec::Relation(id, before) => {
                    touched.extend(self.relations.get(id).map(|x| x.from));
                    touched.extend(before.as_ref().map(|x| x.from));
                }
                _ => {}
            }
        }
        for &id in &touched {
            self.touch(id);
        }
        for r in journal.recs.into_iter().rev() {
            self.undo(r);
        }
        for &id in &touched {
            self.touch(id);
        }
        // Invariants are checked over both documents together, by
        // `Docs::rollback`: a copy's parent is in the other one.
        #[cfg(feature = "differential")]
        if let Some(shadow) = journal.shadow {
            assert!(*self == *shadow, "journal rollback did not restore the document");
        }
    }

    #[cfg(test)]
    pub(crate) fn journal_open(&self) -> bool {
        self.journal.0.is_some()
    }

    fn rec(&mut self, r: Rec) {
        if let Some(j) = &mut self.journal.0 {
            j.recs.push(r);
        }
    }

    fn recording(&self) -> bool {
        self.journal.0.is_some()
    }

    fn undo(&mut self, r: Rec) {
        fn put<K: Ord, V>(m: &mut BTreeMap<K, V>, k: K, v: Option<V>) {
            match v {
                Some(v) => {
                    m.insert(k, v);
                }
                None => {
                    m.remove(&k);
                }
            }
        }
        fn flip<K: Ord, T: Ord>(m: &mut BTreeMap<K, BTreeSet<T>>, k: &K, id: T, inserted: bool) {
            let set = m.get_mut(k).expect("journaled index key");
            if inserted {
                set.remove(&id);
            } else {
                set.insert(id);
            }
        }
        match r {
            Rec::Node(id, n) => put(&mut self.nodes, id, n),
            Rec::FacetTable(kind) => {
                self.facets.remove(&kind);
            }
            Rec::Facet(kind, id, f) => put(self.facets.get_mut(&kind).expect("journaled facet table"), id, f),
            Rec::Relation(id, r) => put(&mut self.relations, id, r),
            Rec::NextId(client, n) => put(&mut self.next_ids, client, n),
            Rec::NextRel(client, n) => put(&mut self.next_rels, client, n),
            Rec::SessionId(n) => self.next_session_id = n,
            Rec::SessionRel(n) => self.next_session_rel = n,
            Rec::Version(v) => self.version = v,
            Rec::TypeKey(ty) => {
                self.by_type.remove(&ty);
            }
            Rec::ByType(ty, id, inserted) => flip(&mut self.by_type, &ty, id, inserted),
            Rec::FacetKey(kind) => {
                self.by_facet.remove(&kind);
            }
            Rec::ByFacet(kind, id, inserted) => flip(&mut self.by_facet, &kind, id, inserted),
            Rec::SourceKey(n) => {
                self.by_source.remove(&n);
            }
            Rec::BySource(n, r, inserted) => flip(&mut self.by_source, &n, r, inserted),
            Rec::TargetKey(n) => {
                self.by_target.remove(&n);
            }
            Rec::ByTarget(n, r, inserted) => flip(&mut self.by_target, &n, r, inserted),
            Rec::ChildrenKey(id) => {
                self.children.remove(&id);
            }
            Rec::ChildrenUnkeyed(id) => {
                self.children.insert(id, Vec::new());
            }
            Rec::Placed(parent, pos) => {
                self.container(parent).remove(pos);
            }
            Rec::Unplaced(parent, pos, id) => self.container(parent).insert(pos, id),
        }
    }

    fn container(&mut self, parent: Option<NodeId>) -> &mut Vec<NodeId> {
        match parent {
            Some(p) => self.children.get_mut(&p).expect("journaled child list"),
            None => &mut self.roots,
        }
    }

    // ---- recorded primitive writes ------------------------------------

    /// Replace `nodes[id]`, recording what it was.
    fn write_node(&mut self, id: NodeId, f: impl FnOnce(&mut Node)) {
        let n = self.nodes.get_mut(&id).expect("known node");
        let before = self.journal.0.is_some().then(|| n.clone());
        f(n);
        if let Some(before) = before {
            self.rec(Rec::Node(id, Some(before)));
        }
    }

    fn type_insert(&mut self, ty: TypeId, id: NodeId) {
        let created = !self.by_type.contains_key(&ty);
        let inserted = self.by_type.entry(ty).or_default().insert(id);
        if created {
            self.rec(Rec::TypeKey(ty));
        }
        if inserted {
            self.rec(Rec::ByType(ty, id, true));
        }
    }

    fn type_remove(&mut self, ty: TypeId, id: NodeId) {
        if self.by_type.get_mut(&ty).is_some_and(|s| s.remove(&id)) {
            self.rec(Rec::ByType(ty, id, false));
        }
    }

    fn facet_index_insert(&mut self, kind: FacetKind, id: NodeId) {
        let created = !self.by_facet.contains_key(&kind);
        let inserted = self.by_facet.entry(kind).or_default().insert(id);
        if created {
            self.rec(Rec::FacetKey(kind));
        }
        if inserted {
            self.rec(Rec::ByFacet(kind, id, true));
        }
    }

    fn facet_index_remove(&mut self, kind: FacetKind, id: NodeId) {
        if self.by_facet.get_mut(&kind).is_some_and(|s| s.remove(&id)) {
            self.rec(Rec::ByFacet(kind, id, false));
        }
    }

    fn children_entry(&mut self, id: NodeId) {
        if !self.children.contains_key(&id) {
            self.children.insert(id, Vec::new());
            self.rec(Rec::ChildrenKey(id));
        }
    }

    // ---- crate-private mutators ---------------------------------------

    pub(crate) fn set_version(&mut self, v: Version) {
        let before = self.version;
        self.version = v;
        if before != v {
            self.rec(Rec::Version(before));
        }
    }

    pub(crate) fn advance_next_id(&mut self, past: NodeId) {
        if past.is_expanded() {
            // Derived ids are computed, never allocated.
        } else if past.is_session() {
            let before = self.next_session_id;
            self.next_session_id = before.max(past.raw() + 1);
            if self.next_session_id != before {
                self.rec(Rec::SessionId(before));
            }
        } else if let Some((client, n)) = past.minted() {
            let before = self.next_ids.get(&client).copied();
            advance(&mut self.next_ids, client, n);
            if self.next_ids.get(&client).copied() != before {
                self.rec(Rec::NextId(client, before));
            }
        }
    }

    pub(crate) fn advance_next_relation_id(&mut self, past: RelationId) {
        if past.is_expanded() {
            // Derived relations' ids are computed, never allocated.
        } else if past.is_session() {
            let before = self.next_session_rel;
            self.next_session_rel = before.max(past.raw() + 1);
            if self.next_session_rel != before {
                self.rec(Rec::SessionRel(before));
            }
        } else if let Some((client, n)) = past.minted() {
            let before = self.next_rels.get(&client).copied();
            advance(&mut self.next_rels, client, n);
            if self.next_rels.get(&client).copied() != before {
                self.rec(Rec::NextRel(client, before));
            }
        }
    }

    pub(crate) fn insert_node(&mut self, node: Node, comps: Vec<Facet>) {
        self.edits.0 += 1;
        let id = node.id;
        debug_assert!(node.is_live());
        self.type_insert(node.ty, id);
        let before = self.nodes.insert(id, node);
        if self.recording() {
            self.rec(Rec::Node(id, before));
        }
        self.children_entry(id);
        for c in comps {
            self.insert_facet(id, c);
        }
        self.place(id);
        self.touch(id);
    }

    /// Bring a tombstoned node back, replacing its placement and facets
    /// with the given ones.
    pub(crate) fn restore_node(
        &mut self,
        id: NodeId,
        ty: TypeId,
        parent: Option<NodeId>,
        order: OrderKey,
        comps: Vec<Facet>,
    ) {
        self.edits.0 += 1;
        // Where the tombstone was, and (below) where it comes back.
        self.touch(id);
        for kind in FacetKind::ALL {
            if let Some(f) = self.facets.get_mut(kind).and_then(|m| m.remove(&id)) {
                self.rec(Rec::Facet(*kind, id, Some(f)));
            }
        }
        self.write_node(id, |n| {
            n.ty = ty;
            n.parent = parent;
            n.order = order;
            n.tombstoned = None;
        });
        self.type_insert(ty, id);
        self.children_entry(id);
        for c in comps {
            self.insert_facet(id, c);
        }
        self.place(id);
        self.touch(id);
    }

    /// Tombstone one node. The caller tombstones descendants separately so
    /// each gets its own inverse op.
    pub(crate) fn tombstone(&mut self, id: NodeId, at: Version) {
        self.edits.0 += 1;
        self.touch(id);
        self.unplace(id);
        self.write_node(id, |n| n.tombstoned = Some(at));
        let ty = self.nodes[&id].ty;
        self.type_remove(ty, id);
        let kinds: Vec<FacetKind> = self.facets.iter().filter(|(_, m)| m.contains_key(&id)).map(|(k, _)| *k).collect();
        for kind in kinds {
            self.facet_index_remove(kind, id);
        }
    }

    /// Remove a live node outright, leaving no tombstone: a derived copy,
    /// which its owner makes again when it is wanted (handoff §4.6). Its
    /// children go first; relations to it are the caller's.
    pub(crate) fn remove_node(&mut self, id: NodeId) {
        self.edits.0 += 1;
        self.touch(id);
        debug_assert!(self.is_live(id), "only a live node is removed");
        self.unplace(id);
        let ty = self.nodes[&id].ty;
        self.type_remove(ty, id);
        let kinds: Vec<FacetKind> = self.facets.iter().filter(|(_, m)| m.contains_key(&id)).map(|(k, _)| *k).collect();
        for kind in kinds {
            self.remove_facet(id, kind);
        }
        if self.children.get(&id).is_some_and(Vec::is_empty) {
            self.children.remove(&id);
            self.rec(Rec::ChildrenUnkeyed(id));
        }
        let before = self.nodes.remove(&id);
        if self.recording() {
            self.rec(Rec::Node(id, before));
        }
    }

    pub(crate) fn set_parent(&mut self, id: NodeId, parent: Option<NodeId>, order: OrderKey) {
        self.edits.0 += 1;
        self.touch(id);
        self.unplace(id);
        self.write_node(id, |n| {
            n.parent = parent;
            n.order = order;
        });
        self.place(id);
        self.touch(id);
    }

    pub(crate) fn set_order(&mut self, id: NodeId, order: OrderKey) {
        self.edits.0 += 1;
        self.touch(id);
        self.unplace(id);
        self.write_node(id, |n| n.order = order);
        self.place(id);
    }

    pub(crate) fn insert_facet(&mut self, id: NodeId, c: Facet) {
        self.edits.0 += 1;
        self.touch(id);
        let kind = c.kind();
        let created = !self.facets.contains_key(&kind);
        let before = self.facets.entry(kind).or_default().insert(id, c);
        if self.recording() {
            if created {
                self.rec(Rec::FacetTable(kind));
            }
            self.rec(Rec::Facet(kind, id, before));
        }
        if self.is_live(id) {
            self.facet_index_insert(kind, id);
        }
    }

    pub(crate) fn remove_facet(
        &mut self,
        id: NodeId,
        kind: FacetKind,
    ) -> Option<Facet> {
        self.edits.0 += 1;
        self.touch(id);
        let c = self.facets.get_mut(&kind)?.remove(&id)?;
        if self.recording() {
            self.rec(Rec::Facet(kind, id, Some(c.clone())));
        }
        self.facet_index_remove(kind, id);
        Some(c)
    }

    /// The facet to change in place. While a journal is open it is recorded
    /// first, since what the caller writes through `&mut` cannot be seen.
    pub(crate) fn facet_mut(
        &mut self,
        id: NodeId,
        kind: FacetKind,
    ) -> Option<&mut Facet> {
        self.edits.0 += 1;
        self.touch(id);
        if self.recording() {
            let before = self.facets.get(&kind)?.get(&id)?.clone();
            self.rec(Rec::Facet(kind, id, Some(before)));
        }
        self.facets.get_mut(&kind)?.get_mut(&id)
    }

    pub(crate) fn insert_relation(&mut self, r: Relation) {
        self.edits.0 += 1;
        self.touch(r.from);
        let (id, from, to) = (r.id, r.from, r.to);
        let created = !self.by_source.contains_key(&from);
        let inserted = self.by_source.entry(from).or_default().insert(id);
        if created {
            self.rec(Rec::SourceKey(from));
        }
        if inserted {
            self.rec(Rec::BySource(from, id, true));
        }
        let created = !self.by_target.contains_key(&to);
        let inserted = self.by_target.entry(to).or_default().insert(id);
        if created {
            self.rec(Rec::TargetKey(to));
        }
        if inserted {
            self.rec(Rec::ByTarget(to, id, true));
        }
        let before = self.relations.insert(id, r);
        if self.recording() {
            self.rec(Rec::Relation(id, before));
        }
    }

    pub(crate) fn remove_relation(&mut self, id: RelationId) -> Option<Relation> {
        self.edits.0 += 1;
        let r = self.relations.remove(&id)?;
        self.touch(r.from);
        if self.recording() {
            self.rec(Rec::Relation(id, Some(r.clone())));
        }
        if self.by_source.get_mut(&r.from).is_some_and(|s| s.remove(&id)) {
            self.rec(Rec::BySource(r.from, id, false));
        }
        if self.by_target.get_mut(&r.to).is_some_and(|s| s.remove(&id)) {
            self.rec(Rec::ByTarget(r.to, id, false));
        }
        Some(r)
    }

    /// Insert `id` into its parent's child list (or the roots) at the position
    /// its order key dictates. Ties on key break by id so order is total.
    fn place(&mut self, id: NodeId) {
        let (parent, key) = {
            let n = &self.nodes[&id];
            (n.parent, n.order.clone())
        };
        if let Some(p) = parent {
            self.children_entry(p);
        }
        let nodes = &self.nodes;
        let container = match parent {
            Some(p) => self.children.get_mut(&p).expect("made above"),
            None => &mut self.roots,
        };
        let pos = container.partition_point(|&c| {
            let cn = &nodes[&c];
            (&cn.order, c) < (&key, id)
        });
        container.insert(pos, id);
        self.rec(Rec::Placed(parent, pos));
    }

    fn unplace(&mut self, id: NodeId) {
        let parent = self.nodes[&id].parent;
        let container = match parent {
            Some(p) => match self.children.get_mut(&p) {
                Some(c) => c,
                None => return,
            },
            None => &mut self.roots,
        };
        if let Some(pos) = container.iter().position(|&c| c == id) {
            container.remove(pos);
            self.rec(Rec::Unplaced(parent, pos, id));
        }
    }

    // ---- bulk construction (used by load) -----------------------------

    /// Build from raw tables without validation beyond what `reindex` checks.
    /// Used by deserialisation; ops never go through here. Each client's
    /// counters are `next`'s, moved past any id the tables hold.
    pub(crate) fn from_parts(
        nodes: BTreeMap<NodeId, Node>,
        facets: BTreeMap<FacetKind, BTreeMap<NodeId, Facet>>,
        relations: BTreeMap<RelationId, Relation>,
        next: Counters,
        version: Version,
    ) -> Document {
        let mut d = Document {
            nodes,
            facets,
            relations,
            next_ids: next.nodes,
            next_rels: next.relations,
            version,
            ..Default::default()
        };
        for (client, n) in d.nodes.keys().filter_map(|id| id.minted()).collect::<Vec<_>>() {
            advance(&mut d.next_ids, client, n);
        }
        for (client, n) in d.relations.keys().filter_map(|id| id.minted()).collect::<Vec<_>>() {
            advance(&mut d.next_rels, client, n);
        }
        d.reindex();
        d
    }

    pub(crate) fn raw_nodes(&self) -> &BTreeMap<NodeId, Node> {
        &self.nodes
    }

    /// Why this is not an authored document, if it is not: it holds a
    /// derived node or relation, or an authored node or relation hangs from
    /// a copy. A store derives its copies itself (reactive-engine handoff
    /// §4.6), so `load` and `with_document` refuse such a document. A
    /// relation may still end at a copy (a wire bound to a pin in an
    /// instance).
    pub fn derived_content(&self) -> Option<String> {
        if let Some((id, n)) = self.nodes.iter().find(|(id, n)| id.is_expanded() || n.parent.is_some_and(|p| p.is_expanded())) {
            return Some(if id.is_expanded() { format!("node {id} is a derived copy") } else { format!("node {id} is placed under the copy {}", n.parent.expect("a parent")) });
        }
        self.relations.values().find(|r| r.id.is_expanded() || r.from.is_expanded()).map(|r| {
            if r.id.is_expanded() { format!("relation {} is derived", r.id) } else { format!("relation {} leaves the copy {}", r.id, r.from) }
        })
    }

    pub(crate) fn raw_facets(&self) -> &BTreeMap<FacetKind, BTreeMap<NodeId, Facet>> {
        &self.facets
    }

    pub(crate) fn counters(&self) -> Counters {
        Counters { nodes: self.next_ids.clone(), relations: self.next_rels.clone() }
    }

    fn reindex(&mut self) {
        self.children.clear();
        self.roots.clear();
        self.by_type.clear();
        self.by_facet.clear();
        self.by_source.clear();
        self.by_target.clear();
        let live: Vec<NodeId> = self
            .nodes
            .values()
            .filter(|n| n.is_live())
            .map(|n| n.id)
            .collect();
        for &id in &live {
            self.children.entry(id).or_default();
            self.by_type
                .entry(self.nodes[&id].ty)
                .or_default()
                .insert(id);
        }
        for &id in &live {
            self.place(id);
        }
        for (kind, m) in &self.facets {
            for id in m.keys() {
                if self.nodes.get(id).is_some_and(|n| n.is_live()) {
                    self.by_facet.entry(*kind).or_default().insert(*id);
                }
            }
        }
        let rels: Vec<Relation> = self.relations.values().cloned().collect();
        for r in rels {
            self.by_source.entry(r.from).or_default().insert(r.id);
            self.by_target.entry(r.to).or_default().insert(r.id);
        }
    }

    /// Cross-check the indexes against the tables. Cheap enough to run in
    /// tests after every transaction; not run in production.
    pub fn check_invariants(&self) -> Result<(), String> {
        self.check_invariants_over(&|p| self.is_live(p))
    }

    /// The invariants, with `parent_live` answering for parents: a copy
    /// kept apart has its owner in the other document (`Tree::check_invariants`).
    pub(crate) fn check_invariants_over(&self, parent_live: &dyn Fn(NodeId) -> bool) -> Result<(), String> {
        for n in self.nodes.values() {
            if n.is_live() {
                let container: &[NodeId] = match n.parent {
                    Some(p) => {
                        if !parent_live(p) {
                            return Err(format!("{} has non-live parent {p}", n.id));
                        }
                        self.children(p)
                    }
                    None => &self.roots,
                };
                if !container.contains(&n.id) {
                    return Err(format!("{} missing from its sibling list", n.id));
                }
                if !self.by_type.get(&n.ty).is_some_and(|s| s.contains(&n.id)) {
                    return Err(format!("{} missing from by_type", n.id));
                }
                if self.is_ancestor(n.id, n.id) {
                    return Err(format!("{} is its own ancestor", n.id));
                }
            } else {
                if self.roots.contains(&n.id) || self.children.values().any(|c| c.contains(&n.id)) {
                    return Err(format!("tombstoned {} still placed", n.id));
                }
                if self.by_type.get(&n.ty).is_some_and(|s| s.contains(&n.id)) {
                    return Err(format!("tombstoned {} still in by_type", n.id));
                }
            }
        }
        for (parent, kids) in &self.children {
            let mut prev: Option<(&OrderKey, NodeId)> = None;
            for &k in kids {
                let kn = &self.nodes[&k];
                if kn.parent != Some(*parent) {
                    return Err(format!(
                        "{k} listed under {parent} but parent is {:?}",
                        kn.parent
                    ));
                }
                if let Some(p) = prev
                    && p >= (&kn.order, k)
                {
                    return Err(format!("children of {parent} out of order at {k}"));
                }
                prev = Some((&kn.order, k));
            }
        }
        for (kind, set) in &self.by_facet {
            for id in set {
                if !self.is_live(*id) {
                    return Err(format!("by_component[{}] holds non-live {id}", kind.name()));
                }
                if self.facet(*id, *kind).is_none() {
                    return Err(format!(
                        "by_component[{}] holds {id} without the component",
                        kind.name()
                    ));
                }
            }
        }
        for (kind, m) in &self.facets {
            for id in m.keys() {
                if !self.nodes.contains_key(id) {
                    return Err(format!("component {} on unknown node {id}", kind.name()));
                }
            }
        }
        for r in self.relations.values() {
            if !self
                .by_source
                .get(&r.from)
                .is_some_and(|s| s.contains(&r.id))
                || !self.by_target.get(&r.to).is_some_and(|s| s.contains(&r.id))
            {
                return Err(format!("relation {} not indexed", r.id));
            }
        }
        Ok(())
    }
}

/// The journal (reactive-engine handoff §4.3, task 2): whatever the mutators
/// write while it is open, a rollback puts the document back exactly, its
/// indexes and empty map keys included.
#[cfg(test)]
mod journal_tests {
    use super::*;
    use crate::facet::{Name, Transform2d};

    /// A small deterministic generator (xorshift64*).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn pick<T: Copy>(&mut self, xs: &[T]) -> Option<T> {
            (!xs.is_empty()).then(|| xs[self.below(xs.len())])
        }
    }

    fn ty(n: &str) -> TypeId {
        TypeId::lookup(n).unwrap()
    }

    fn name(text: &str) -> Facet {
        let mut n = Name::default();
        n.text = Slot::Const(Value::Str(text.into()));
        n.wrap()
    }

    fn transform(x: f64) -> Facet {
        let mut t = Transform2d::default();
        t.x = Slot::Const(Value::Number(x));
        t.wrap()
    }

    fn node(id: NodeId, parent: Option<NodeId>, order: OrderKey) -> Node {
        Node { id, ty: ty("group"), parent, order, tombstoned: None }
    }

    fn live(doc: &Document) -> Vec<NodeId> {
        doc.nodes.values().filter(|n| n.is_live()).map(|n| n.id).collect()
    }

    /// A live node with no live children: what a delete tombstones first.
    fn leaves(doc: &Document) -> Vec<NodeId> {
        live(doc).into_iter().filter(|&id| doc.children(id).is_empty()).collect()
    }

    fn new_id(doc: &Document) -> NodeId {
        let id = doc.next_id();
        id
    }

    /// One random mutator call, as the op applier makes them.
    fn mutate(doc: &mut Document, rng: &mut Rng, at: Version) {
        let live = live(doc);
        match rng.below(12) {
            0 | 1 => {
                let id = new_id(doc);
                doc.advance_next_id(id);
                let parent = if rng.below(4) == 0 { None } else { rng.pick(&live) };
                let order = OrderKey::between(None, None).unwrap_or_else(|_| OrderKey::first());
                let comps = if rng.below(2) == 0 { vec![name("new")] } else { vec![] };
                doc.insert_node(node(id, parent, order), comps);
            }
            2 => {
                if let Some(id) = rng.pick(&leaves(doc)) {
                    doc.tombstone(id, at);
                }
            }
            3 => {
                let dead: Vec<NodeId> = doc.nodes.values().filter(|n| !n.is_live()).map(|n| n.id).collect();
                if let Some(id) = rng.pick(&dead) {
                    let parent = rng.pick(&live).filter(|_| rng.below(3) != 0);
                    doc.restore_node(id, ty("group"), parent, OrderKey::first(), vec![transform(rng.below(9) as f64)]);
                }
            }
            4 => {
                if let Some(id) = rng.pick(&live) {
                    let candidates: Vec<NodeId> = live.iter().copied().filter(|&p| p != id && !doc.is_ancestor(id, p)).collect();
                    let parent = if rng.below(3) == 0 { None } else { rng.pick(&candidates) };
                    doc.set_parent(id, parent, OrderKey::first());
                }
            }
            5 => {
                if let Some(id) = rng.pick(&live) {
                    let key = doc.nodes[&id].order.clone();
                    doc.set_order(id, if rng.below(2) == 0 { OrderKey::after(&key) } else { OrderKey::first() });
                }
            }
            6 => {
                if let Some(id) = rng.pick(&doc.nodes.keys().copied().collect::<Vec<_>>()) {
                    let f = if rng.below(2) == 0 { name(&format!("n{}", rng.below(5))) } else { transform(rng.below(5) as f64) };
                    doc.insert_facet(id, f);
                }
            }
            7 => {
                if let Some(id) = rng.pick(&doc.nodes.keys().copied().collect::<Vec<_>>()) {
                    let kind = if rng.below(2) == 0 { FacetKind::Name } else { FacetKind::Transform2d };
                    doc.remove_facet(id, kind);
                }
            }
            8 => {
                if let Some(id) = rng.pick(&doc.nodes.keys().copied().collect::<Vec<_>>())
                    && let Some(f) = doc.facet_mut(id, FacetKind::Name)
                {
                    f.set("text", Slot::Const(Value::Str(format!("set{}", rng.below(5))))).unwrap();
                }
            }
            9 => {
                if let (Some(from), Some(to)) = (rng.pick(&live), rng.pick(&live)) {
                    let id = doc.next_relation_id();
                    doc.advance_next_relation_id(id);
                    doc.insert_relation(Relation { id, from, to, rel: RelKind::References, data: BTreeMap::new() });
                }
            }
            10 => {
                if let Some(id) = rng.pick(&doc.relations.keys().copied().collect::<Vec<_>>()) {
                    doc.remove_relation(id);
                }
            }
            _ => {
                let session = NodeId::from_raw(crate::id::SESSION_BASE + rng.below(50) as u64);
                doc.advance_next_id(session);
                doc.advance_next_relation_id(RelationId::from_raw(crate::id::SESSION_BASE + rng.below(50) as u64));
                doc.set_version(Version(doc.version().0 + 1));
            }
        }
    }

    #[test]
    fn random_writes_roll_back_exactly() {
        for seed in 1..=300u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut doc = Document::new();
            for _ in 0..rng.below(25) {
                mutate(&mut doc, &mut rng, Version(1));
            }
            doc.check_invariants().unwrap_or_else(|e| panic!("seed {seed}: base: {e}"));
            let before = doc.clone();
            doc.begin();
            for _ in 0..1 + rng.below(40) {
                mutate(&mut doc, &mut rng, Version(2));
            }
            doc.rollback();
            assert!(doc == before, "seed {seed}: the rollback differs\nafter:  {doc:?}\nbefore: {before:?}");
            doc.check_invariants().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            assert!(!doc.journal_open());
        }
    }

    #[test]
    fn committed_writes_stay() {
        let mut doc = Document::new();
        doc.begin();
        let id = new_id(&doc);
        doc.advance_next_id(id);
        doc.insert_node(node(id, None, OrderKey::first()), vec![name("kept")]);
        doc.commit();
        assert!(doc.is_live(id));
        assert!(!doc.journal_open());
    }

    /// A facet kind's first insertion makes its table, which a rollback takes
    /// away; a table that exists empty stays.
    #[test]
    fn facet_tables_come_back_as_they_were() {
        let mut doc = Document::new();
        let id = new_id(&doc);
        doc.advance_next_id(id);
        doc.insert_node(node(id, None, OrderKey::first()), vec![]);
        let before = doc.clone();
        doc.begin();
        doc.insert_facet(id, name("first"));
        doc.rollback();
        assert!(!doc.facets.contains_key(&FacetKind::Name));
        assert!(doc == before);

        doc.insert_facet(id, name("there"));
        doc.remove_facet(id, FacetKind::Name);
        assert!(doc.facets[&FacetKind::Name].is_empty());
        let before = doc.clone();
        doc.begin();
        doc.insert_facet(id, name("again"));
        doc.rollback();
        assert!(doc.facets[&FacetKind::Name].is_empty(), "the empty table stays");
        assert!(doc == before);
    }

    /// A tombstoned node loaded from a file has no child list; restoring it
    /// makes one, and a rollback takes it away again.
    #[test]
    fn restoring_a_loaded_tombstone_rolls_back() {
        let id = NodeId::authored(0, 1);
        let mut nodes = BTreeMap::new();
        nodes.insert(id, Node { id, ty: ty("group"), parent: None, order: OrderKey::first(), tombstoned: Some(Version(1)) });
        let mut facets = BTreeMap::new();
        facets.insert(FacetKind::Name, BTreeMap::from([(id, name("old"))]));
        let mut doc = Document::from_parts(nodes, facets, BTreeMap::new(), Counters::default(), Version(1));
        assert!(!doc.children.contains_key(&id));
        let before = doc.clone();
        doc.begin();
        doc.restore_node(id, ty("group"), None, OrderKey::first(), vec![transform(3.0)]);
        assert!(doc.is_live(id) && doc.children.contains_key(&id));
        doc.rollback();
        assert!(doc == before);
        doc.check_invariants().unwrap();
    }

    /// One node written several times: each record undoes one write, in
    /// reverse, so the first value comes back.
    #[test]
    fn a_node_touched_several_times_rolls_back() {
        let mut doc = Document::new();
        let a = new_id(&doc);
        doc.advance_next_id(a);
        doc.insert_node(node(a, None, OrderKey::first()), vec![name("a")]);
        let b = new_id(&doc);
        doc.advance_next_id(b);
        doc.insert_node(node(b, None, OrderKey::after(&OrderKey::first())), vec![]);
        let before = doc.clone();
        doc.begin();
        for i in 0..3 {
            doc.set_order(a, OrderKey::after(&doc.nodes[&b].order.clone()));
            doc.set_order(b, OrderKey::after(&doc.nodes[&a].order.clone()));
            doc.facet_mut(a, FacetKind::Name).unwrap().set("text", Slot::Const(Value::Str(format!("x{i}")))).unwrap();
        }
        doc.set_parent(b, Some(a), OrderKey::first());
        doc.tombstone(b, Version(2));
        doc.rollback();
        assert!(doc == before);
        doc.check_invariants().unwrap();
    }
}
