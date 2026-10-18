//! Operations, transactions, and the applier.
//!
//! Every mutation to authored state is one of these ops (doc 04 §3). Applying
//! an op computes its inverse while the pre-state is in hand, so undo is
//! ordinary application of ops and needs no special casing downstream.

use crate::facet::{Facet, FacetKind, LeafPath};
use crate::document::{Document, Node, Relation};
use crate::id::{NodeId, RelationId, Version};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::Slot;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Op {
    /// Mint a node. `id` must be at or past the document's allocation counter
    /// (reserve with [`Document::next_ids`]), or name a tombstoned node, in
    /// which case this is a restore and the given placement and facets
    /// replace the old ones.
    Create {
        id: NodeId,
        ty: TypeId,
        parent: Option<NodeId>,
        order: OrderKey,
        #[serde(default)]
        facets: Vec<Facet>,
    },
    /// Tombstone a node and its live descendants.
    Delete {
        id: NodeId,
    },
    Reparent {
        id: NodeId,
        parent: Option<NodeId>,
        order: OrderKey,
    },
    Reorder {
        id: NodeId,
        order: OrderKey,
    },
    /// Replace one leaf's slot.
    Set {
        id: NodeId,
        path: LeafPath,
        slot: Slot,
    },
    AddFacet {
        id: NodeId,
        facet: Facet,
    },
    RemoveFacet {
        id: NodeId,
        kind: FacetKind,
    },
    /// `rel.id` must not be in use; take fresh ids from `next_relation_id`.
    Link {
        rel: Relation,
    },
    Unlink {
        id: RelationId,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Human,
    Model,
    System,
}

/// What a caller submits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransactionInput {
    /// Semantic, in the user's vocabulary: "move 3 objects", never "set x14".
    pub label: String,
    pub actor: Actor,
    /// Caller's clock, milliseconds. The store has no clock of its own.
    pub ts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub ops: Vec<Op>,
    /// Refuse the transaction if the solver cannot honour every value it
    /// sets (constraint-system.md §9.2). Off, the value is adjusted and
    /// reported as `unmet`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exact: bool,
}

/// What the log records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    pub version: Version,
    pub label: String,
    pub actor: Actor,
    pub ts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub ops: Vec<Op>,
    /// What the solver changed to keep every constraint holding after
    /// `ops` (constraint-system.md §7). Redo and replay apply `ops` then
    /// these, without solving; `inverse` covers both.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_ops: Vec<Op>,
    /// Applying these in reverse order undoes `ops` and `derived_ops`.
    pub inverse: Vec<Op>,
    /// What expansion changed in the instances' derived nodes
    /// (component-model.md §3), for consumers that follow ops. Not history:
    /// undo, redo and replay re-expand from the authored state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expanded: Vec<Op>,
    /// Causes the model is told about: which clusters re-solved and which
    /// requested values were not honoured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solver: Option<crate::solve::SolverSummary>,
}

impl Transaction {
    /// The ops that undo this transaction, in the order to apply them.
    pub fn undo_ops(&self) -> Vec<Op> {
        self.inverse.iter().rev().cloned().collect()
    }

    /// The ops that redo or replay it: requested, then derived, no solve.
    pub fn replay_ops(&self) -> Vec<Op> {
        self.ops.iter().chain(self.derived_ops.iter()).cloned().collect()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    UnknownNode,
    Tombstoned,
    BadId,
    BadParent,
    Cycle,
    ChildNotAdmitted,
    FacetNotAllowed,
    FacetRequired,
    FacetPresent,
    FacetAbsent,
    UnknownPath,
    TypeMismatch,
    UnknownRelation,
    /// An entity's instance or params disagree with its definition.
    BadInstance,
    /// A constraint's kind, roles, anchors or operands are wrong, or it
    /// duplicates another.
    BadConstraint,
    /// A connection joins ports that cannot be joined: two kinds, two
    /// inputs, an input already fed, or a signal loop (ECM §6.1).
    BadConnection,
    /// A binding reaches across a component boundary it may not cross
    /// (component-model.md §8): a definition reading outside itself, or
    /// outside reading an instance's insides other than its ports.
    OutOfScope,
    /// The required constraints cannot all hold after this transaction (or,
    /// with `exact`, a requested value could not be honoured).
    Unsatisfiable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplyError {
    /// Index of the failing op within the transaction.
    pub op: usize,
    pub kind: ErrorKind,
    pub message: String,
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "op {}: {:?}: {}", self.op, self.kind, self.message)
    }
}
impl std::error::Error for ApplyError {}

struct Ctx {
    index: usize,
    /// The version the transaction will carry; tombstones are stamped with it.
    stamp: Version,
}

impl Ctx {
    fn err(&self, kind: ErrorKind, message: impl Into<String>) -> ApplyError {
        ApplyError {
            op: self.index,
            kind,
            message: message.into(),
        }
    }
}

/// Apply one op to `doc`, returning the ops that invert it. On error `doc` is
/// untouched: every check precedes every mutation.
pub(crate) fn apply_op(
    doc: &mut Document,
    index: usize,
    op: &Op,
    stamp: Version,
) -> Result<Vec<Op>, ApplyError> {
    let cx = Ctx { index, stamp };
    match op {
        Op::Create {
            id,
            ty,
            parent,
            order,
            facets,
        } => create(doc, &cx, *id, *ty, *parent, order, facets),
        Op::Delete { id } => delete(doc, &cx, *id),
        Op::Reparent { id, parent, order } => reparent(doc, &cx, *id, *parent, order),
        Op::Reorder { id, order } => {
            let n = live(doc, &cx, *id)?;
            let old = n.order.clone();
            doc.set_order(*id, order.clone());
            Ok(vec![Op::Reorder {
                id: *id,
                order: old,
            }])
        }
        Op::Set { id, path, slot } => set(doc, &cx, *id, *path, slot),
        Op::AddFacet { id, facet } => add_comp(doc, &cx, *id, facet),
        Op::RemoveFacet { id, kind } => remove_comp(doc, &cx, *id, *kind),
        Op::Link { rel } => link(doc, &cx, rel),
        Op::Unlink { id } => {
            let r = doc
                .remove_relation(*id)
                .ok_or_else(|| cx.err(ErrorKind::UnknownRelation, format!("no relation {id}")))?;
            Ok(vec![Op::Link { rel: r }])
        }
    }
}

fn live<'d>(doc: &'d Document, cx: &Ctx, id: NodeId) -> Result<&'d Node, ApplyError> {
    match doc.node(id) {
        Some(n) if n.is_live() => Ok(n),
        Some(n) => Err(cx.err(
            ErrorKind::Tombstoned,
            format!(
                "{id} was deleted at version {}",
                n.tombstoned.expect("tombstoned").0
            ),
        )),
        None => Err(cx.err(ErrorKind::UnknownNode, format!("no node {id}"))),
    }
}

fn check_parent(
    doc: &Document,
    cx: &Ctx,
    parent: Option<NodeId>,
    child_ty: TypeId,
) -> Result<(), ApplyError> {
    let Some(p) = parent else { return Ok(()) };
    let pn = live(doc, cx, p).map_err(|e| cx.err(ErrorKind::BadParent, e.message))?;
    if !pn.ty.spec().admits_child(child_ty) {
        return Err(cx.err(
            ErrorKind::ChildNotAdmitted,
            format!("{} ({}) does not admit a {} child", p, pn.ty, child_ty),
        ));
    }
    Ok(())
}

fn check_facets(cx: &Ctx, ty: TypeId, comps: &[Facet]) -> Result<(), ApplyError> {
    let spec = ty.spec();
    for (i, c) in comps.iter().enumerate() {
        let k = c.kind();
        if !spec.allows(k) {
            return Err(cx.err(
                ErrorKind::FacetNotAllowed,
                format!("{} does not allow {}", ty, k.name()),
            ));
        }
        if comps[..i].iter().any(|o| o.kind() == k) {
            return Err(cx.err(
                ErrorKind::FacetPresent,
                format!("{} given twice", k.name()),
            ));
        }
    }
    for req in spec.required() {
        if !comps.iter().any(|c| c.kind() == req) {
            return Err(cx.err(
                ErrorKind::FacetRequired,
                format!("{} requires {}", ty, req.name()),
            ));
        }
    }
    Ok(())
}

fn create(
    doc: &mut Document,
    cx: &Ctx,
    id: NodeId,
    ty: TypeId,
    parent: Option<NodeId>,
    order: &OrderKey,
    comps: &[Facet],
) -> Result<Vec<Op>, ApplyError> {
    let restoring = match doc.node(id) {
        Some(n) if n.is_live() => {
            return Err(cx.err(ErrorKind::BadId, format!("{id} already exists")));
        }
        Some(_) => true,
        None => {
            let next = if id.is_session() { doc.next_session_ids(1)[0] } else { doc.next_id() };
            if id < next {
                return Err(cx.err(
                    ErrorKind::BadId,
                    format!("{id} is below the allocation counter {next}; reserve ids from next_id"),
                ));
            }
            false
        }
    };
    if parent == Some(id) {
        return Err(cx.err(ErrorKind::Cycle, format!("{id} cannot be its own parent")));
    }
    check_parent(doc, cx, parent, ty)?;
    check_facets(cx, ty, comps)?;
    if restoring {
        doc.restore_node(id, ty, parent, order.clone(), comps.to_vec());
    } else {
        doc.insert_node(
            Node {
                id,
                ty,
                parent,
                order: order.clone(),
                tombstoned: None,
            },
            comps.to_vec(),
        );
        doc.advance_next_id(id);
    }
    // Bindings and entity rules are checked once the whole transaction has
    // applied (`check_created`): a restored subtree's nodes may read each
    // other in any order.
    Ok(vec![Op::Delete { id }])
}

/// The checks a `Create` defers to the end of its transaction: its bindings
/// read live leaves without a cycle, and an entity or embed is valid.
pub(crate) fn check_created(doc: &Document, index: usize, op: &Op, stamp: Version) -> Result<(), ApplyError> {
    // A connection is checked once both of its ends exist (ECM §6.1).
    if let Op::Link { rel } = op
        && rel.rel == crate::document::RelKind::Connects
    {
        let cx = Ctx { index, stamp };
        if doc.is_live(rel.from) {
            crate::port::validate_connection(doc, rel.from).map_err(|e| cx.err(ErrorKind::BadConnection, e))?;
        }
        return Ok(());
    }
    let Op::Create { id, .. } = op else { return Ok(()) };
    let cx = Ctx { index, stamp };
    if doc.live(*id).is_none() {
        return Ok(());
    }
    let comps: Vec<Facet> = doc.facets_of(*id).cloned().collect();
    check_facet_bindings(doc, &cx, *id, &comps)?;
    check_entity(doc, &cx, *id)
}

/// An entity's instance and params must agree with its definition; checked
/// whenever either is created, set, or added (ECM §4.3).
fn check_entity(doc: &Document, cx: &Ctx, id: NodeId) -> Result<(), ApplyError> {
    match doc.live(id).map(|n| n.ty.name()) {
        Some("entity") => crate::definition::validate_entity(doc, id).map_err(|e| cx.err(ErrorKind::BadInstance, e))?,
        Some("embed") => check_embeds(doc, cx, id)?,
        _ => {}
    }
    Ok(())
}

/// Every embed at or under `id` must name a live composition other than its
/// own, without a cycle (ECM §7.4); checked on create, set, add and
/// reparent, since a move can close a loop too.
fn check_embeds(doc: &Document, cx: &Ctx, id: NodeId) -> Result<(), ApplyError> {
    for n in std::iter::once(id).chain(doc.descendants(id)) {
        if doc.live(n).is_some_and(|x| x.ty.name() == "embed") {
            crate::composition::validate_embed(doc, n).map_err(|e| cx.err(ErrorKind::Cycle, e))?;
        }
    }
    Ok(())
}

fn delete(doc: &mut Document, cx: &Ctx, id: NodeId) -> Result<Vec<Op>, ApplyError> {
    live(doc, cx, id)?;
    let mut order = vec![id];
    order.extend(doc.descendants(id));
    // Inverses are pushed bottom-up so that the reversed list restores parents
    // before children.
    let mut inverse = Vec::with_capacity(order.len());
    for &n in order.iter().rev() {
        let node = doc.node(n).expect("descendant exists").clone();
        let comps: Vec<Facet> = doc.facets_of(n).cloned().collect();
        inverse.push(Op::Create {
            id: n,
            ty: node.ty,
            parent: node.parent,
            order: node.order,
            facets: comps,
        });
        doc.tombstone(n, cx.stamp);
    }
    Ok(inverse)
}

fn reparent(
    doc: &mut Document,
    cx: &Ctx,
    id: NodeId,
    parent: Option<NodeId>,
    order: &OrderKey,
) -> Result<Vec<Op>, ApplyError> {
    let n = live(doc, cx, id)?;
    let (old_parent, old_order, ty) = (n.parent, n.order.clone(), n.ty);
    if let Some(p) = parent {
        if p == id {
            return Err(cx.err(ErrorKind::Cycle, format!("{id} cannot be its own parent")));
        }
        if doc.is_ancestor(id, p) {
            return Err(cx.err(ErrorKind::Cycle, format!("{p} is a descendant of {id}")));
        }
    }
    check_parent(doc, cx, parent, ty)?;
    doc.set_parent(id, parent, order.clone());
    check_embeds(doc, cx, id)?;
    Ok(vec![Op::Reparent {
        id,
        parent: old_parent,
        order: old_order,
    }])
}

/// A binding may only read leaves that exist, and may not reach itself.
fn check_binding(
    doc: &Document,
    cx: &Ctx,
    target: crate::expr::Leaf,
    expr: &crate::expr::Expr,
) -> Result<(), ApplyError> {
    for dep in expr.deps() {
        live(doc, cx, dep.node).map_err(|e| cx.err(e.kind, format!("binding reads {dep}: {}", e.message)))?;
        let Some(comp) = doc.facet(dep.node, dep.path.facet) else {
            return Err(cx.err(
                ErrorKind::UnknownPath,
                format!(
                    "binding reads {dep}, but {} has no {} component",
                    dep.node,
                    dep.path.facet.name()
                ),
            ));
        };
        if comp.get(dep.path.field).is_none() {
            return Err(cx.err(ErrorKind::UnknownPath, format!("binding reads {dep}, but {} has no such argument", dep.node)));
        }
    }
    if doc.binding_cycles(target, expr) {
        return Err(cx.err(ErrorKind::Cycle, format!("binding {target} would depend on itself")));
    }
    Ok(())
}

/// Validate every bound slot in `comps` as if it were being set on `id`.
/// Runs after the facets are in place so a node may bind its own leaves;
/// on failure the transaction's working copy is discarded anyway.
fn check_facet_bindings(doc: &Document, cx: &Ctx, id: NodeId, comps: &[Facet]) -> Result<(), ApplyError> {
    for c in comps {
        for field in c.field_names() {
            if let Some(expr) = c.get(field).and_then(|s| s.expr()) {
                let path = LeafPath::new(c.kind(), field).expect("declared field");
                check_binding(doc, cx, crate::expr::Leaf { node: id, path }, expr)?;
            }
        }
    }
    Ok(())
}

fn set(
    doc: &mut Document,
    cx: &Ctx,
    id: NodeId,
    path: LeafPath,
    slot: &Slot,
) -> Result<Vec<Op>, ApplyError> {
    live(doc, cx, id)?;
    if let Some(expr) = slot.expr() {
        check_binding(doc, cx, crate::expr::Leaf { node: id, path }, expr)?;
    }
    let comp = doc.facet_mut(id, path.facet).ok_or_else(|| {
        cx.err(
            ErrorKind::UnknownPath,
            format!("{id} has no {} component", path.facet.name()),
        )
    })?;
    let old = comp
        .set(path.field, slot.clone())
        .map_err(|e| cx.err(ErrorKind::TypeMismatch, e.to_string()))?;
    if matches!(path.facet, FacetKind::Args | FacetKind::Instance | FacetKind::Embed | FacetKind::EmbedCamera) {
        check_entity(doc, cx, id)?;
    }
    Ok(vec![Op::Set {
        id,
        path,
        slot: old,
    }])
}

fn add_comp(
    doc: &mut Document,
    cx: &Ctx,
    id: NodeId,
    comp: &Facet,
) -> Result<Vec<Op>, ApplyError> {
    let n = live(doc, cx, id)?;
    let kind = comp.kind();
    if !n.ty.spec().allows(kind) {
        return Err(cx.err(
            ErrorKind::FacetNotAllowed,
            format!("{} does not allow {}", n.ty, kind.name()),
        ));
    }
    // A dynamic facet (an instance's `args`) is replaced whole: its fields
    // are the definition's inputs, which can gain, lose or rename one at
    // once (component-authoring.md §2).
    let replaced = match doc.facet(id, kind) {
        Some(old) if kind.is_dynamic() => Some(old.clone()),
        Some(_) => {
            return Err(cx.err(
                ErrorKind::FacetPresent,
                format!("{id} already has {}", kind.name()),
            ));
        }
        None => None,
    };
    if replaced.is_some() {
        doc.remove_facet(id, kind);
    }
    doc.insert_facet(id, comp.clone());
    check_facet_bindings(doc, cx, id, std::slice::from_ref(comp))?;
    check_entity(doc, cx, id)?;
    Ok(vec![match replaced {
        Some(old) => Op::AddFacet { id, facet: old },
        None => Op::RemoveFacet { id, kind },
    }])
}

fn remove_comp(
    doc: &mut Document,
    cx: &Ctx,
    id: NodeId,
    kind: FacetKind,
) -> Result<Vec<Op>, ApplyError> {
    let n = live(doc, cx, id)?;
    if n.ty.spec().requires(kind) {
        return Err(cx.err(
            ErrorKind::FacetRequired,
            format!("{} requires {}", n.ty, kind.name()),
        ));
    }
    let comp = doc.remove_facet(id, kind).ok_or_else(|| {
        cx.err(
            ErrorKind::FacetAbsent,
            format!("{id} has no {}", kind.name()),
        )
    })?;
    Ok(vec![Op::AddFacet { id, facet: comp }])
}

fn link(doc: &mut Document, cx: &Ctx, rel: &Relation) -> Result<Vec<Op>, ApplyError> {
    live(doc, cx, rel.from)?;
    // An instance's expanded node appears when the transaction's expansion
    // runs, so a link to one needs only its instance to exist here (an undo
    // may relink before it restores the instance); the checks after
    // expansion (a connection's) see the node itself.
    match rel.to.expanded_root() {
        Some(root) if doc.node(root).is_some() => {}
        Some(root) => return Err(cx.err(ErrorKind::UnknownNode, format!("no node {root}"))),
        None => live(doc, cx, rel.to).map(|_| ())?,
    }
    // Any unused id is accepted, not only ids past the counter: the inverse of
    // an unlink re-links with the original id, and relations are not
    // tombstoned. Fresh links should still take ids from `next_relation_id`.
    if doc.relation(rel.id).is_some() {
        return Err(cx.err(
            ErrorKind::BadId,
            format!("relation {} already exists", rel.id),
        ));
    }
    doc.insert_relation(rel.clone());
    doc.advance_next_relation_id(rel.id);
    Ok(vec![Op::Unlink { id: rel.id }])
}
