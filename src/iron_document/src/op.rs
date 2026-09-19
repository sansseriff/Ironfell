//! Operations, transactions, and the applier.
//!
//! Every mutation to authored state is one of these ops (doc 04 §3). Applying
//! an op computes its inverse while the pre-state is in hand, so undo is
//! ordinary application of ops and needs no special casing downstream.

use crate::component::{Component, ComponentKind, LeafPath};
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
    /// which case this is a restore and the given placement and components
    /// replace the old ones.
    Create {
        id: NodeId,
        ty: TypeId,
        parent: Option<NodeId>,
        order: OrderKey,
        #[serde(default)]
        components: Vec<Component>,
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
    AddComp {
        id: NodeId,
        comp: Component,
    },
    RemoveComp {
        id: NodeId,
        kind: ComponentKind,
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
    /// Applying these in reverse order undoes `ops`.
    pub inverse: Vec<Op>,
}

impl Transaction {
    /// The ops that undo this transaction, in the order to apply them.
    pub fn undo_ops(&self) -> Vec<Op> {
        self.inverse.iter().rev().cloned().collect()
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
    ComponentNotAllowed,
    ComponentRequired,
    ComponentPresent,
    ComponentAbsent,
    UnknownPath,
    TypeMismatch,
    UnknownRelation,
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
            components,
        } => create(doc, &cx, *id, *ty, *parent, order, components),
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
        Op::AddComp { id, comp } => add_comp(doc, &cx, *id, comp),
        Op::RemoveComp { id, kind } => remove_comp(doc, &cx, *id, *kind),
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

fn check_components(cx: &Ctx, ty: TypeId, comps: &[Component]) -> Result<(), ApplyError> {
    let spec = ty.spec();
    for (i, c) in comps.iter().enumerate() {
        let k = c.kind();
        if !spec.allows(k) {
            return Err(cx.err(
                ErrorKind::ComponentNotAllowed,
                format!("{} does not allow {}", ty, k.name()),
            ));
        }
        if comps[..i].iter().any(|o| o.kind() == k) {
            return Err(cx.err(
                ErrorKind::ComponentPresent,
                format!("{} given twice", k.name()),
            ));
        }
    }
    for req in spec.required() {
        if !comps.iter().any(|c| c.kind() == req) {
            return Err(cx.err(
                ErrorKind::ComponentRequired,
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
    comps: &[Component],
) -> Result<Vec<Op>, ApplyError> {
    let restoring = match doc.node(id) {
        Some(n) if n.is_live() => {
            return Err(cx.err(ErrorKind::BadId, format!("{id} already exists")));
        }
        Some(_) => true,
        None => {
            if id < doc.next_id() {
                return Err(cx.err(
                    ErrorKind::BadId,
                    format!(
                        "{id} is below the allocation counter {}; reserve ids from next_id",
                        doc.next_id()
                    ),
                ));
            }
            false
        }
    };
    if parent == Some(id) {
        return Err(cx.err(ErrorKind::Cycle, format!("{id} cannot be its own parent")));
    }
    check_parent(doc, cx, parent, ty)?;
    check_components(cx, ty, comps)?;
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
    check_component_bindings(doc, cx, id, comps)?;
    Ok(vec![Op::Delete { id }])
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
        let comps: Vec<Component> = doc.components_of(n).cloned().collect();
        inverse.push(Op::Create {
            id: n,
            ty: node.ty,
            parent: node.parent,
            order: node.order,
            components: comps,
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
        if doc.component(dep.node, dep.path.component).is_none() {
            return Err(cx.err(
                ErrorKind::UnknownPath,
                format!(
                    "binding reads {dep}, but {} has no {} component",
                    dep.node,
                    dep.path.component.name()
                ),
            ));
        }
    }
    if doc.binding_cycles(target, expr) {
        return Err(cx.err(ErrorKind::Cycle, format!("binding {target} would depend on itself")));
    }
    Ok(())
}

/// Validate every bound slot in `comps` as if it were being set on `id`.
/// Runs after the components are in place so a node may bind its own leaves;
/// on failure the transaction's working copy is discarded anyway.
fn check_component_bindings(doc: &Document, cx: &Ctx, id: NodeId, comps: &[Component]) -> Result<(), ApplyError> {
    for c in comps {
        for (field, _) in c.fields() {
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
    let comp = doc.component_mut(id, path.component).ok_or_else(|| {
        cx.err(
            ErrorKind::UnknownPath,
            format!("{id} has no {} component", path.component.name()),
        )
    })?;
    let old = comp
        .set(path.field, slot.clone())
        .map_err(|e| cx.err(ErrorKind::TypeMismatch, e.to_string()))?;
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
    comp: &Component,
) -> Result<Vec<Op>, ApplyError> {
    let n = live(doc, cx, id)?;
    let kind = comp.kind();
    if !n.ty.spec().allows(kind) {
        return Err(cx.err(
            ErrorKind::ComponentNotAllowed,
            format!("{} does not allow {}", n.ty, kind.name()),
        ));
    }
    if doc.component(id, kind).is_some() {
        return Err(cx.err(
            ErrorKind::ComponentPresent,
            format!("{id} already has {}", kind.name()),
        ));
    }
    doc.insert_component(id, comp.clone());
    check_component_bindings(doc, cx, id, std::slice::from_ref(comp))?;
    Ok(vec![Op::RemoveComp { id, kind }])
}

fn remove_comp(
    doc: &mut Document,
    cx: &Ctx,
    id: NodeId,
    kind: ComponentKind,
) -> Result<Vec<Op>, ApplyError> {
    let n = live(doc, cx, id)?;
    if n.ty.spec().requires(kind) {
        return Err(cx.err(
            ErrorKind::ComponentRequired,
            format!("{} requires {}", n.ty, kind.name()),
        ));
    }
    let comp = doc.remove_component(id, kind).ok_or_else(|| {
        cx.err(
            ErrorKind::ComponentAbsent,
            format!("{id} has no {}", kind.name()),
        )
    })?;
    Ok(vec![Op::AddComp { id, comp }])
}

fn link(doc: &mut Document, cx: &Ctx, rel: &Relation) -> Result<Vec<Op>, ApplyError> {
    live(doc, cx, rel.from)?;
    live(doc, cx, rel.to)?;
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
