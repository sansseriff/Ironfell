//! Expansion (component-model.md §3, §6): the nodes every linked instance
//! derives from its definition, kept in the document beside the authored
//! nodes and never stored.
//!
//! A linked `entity` (`instance.mode = linked`) derives its definition's
//! interface and behaviour: one `port` per `port_spec` and one `equation`
//! per equation, as its children. An `in` port reads the entity's argument
//! (`#entity.args.<name>`) unless a connection feeds it. Each `use` that
//! figures a linked entity derives the definition's template for its
//! notation, as its children. References inside the copies follow: to a
//! `port_spec` or equation, the entity's copy; to a template node, the
//! `use`'s copy.
//!
//! An expanded node's id is `NodeId::expanded(owner, local)`, the owner
//! being the entity or `use` and `local` the definition node it copies, so
//! re-expanding gives every node the id it had and references to expanded
//! nodes (a connection to `#m/7`, a readout bound to `#m/7.port.signal`)
//! survive edits to the definition, undo and reload.
//!
//! [`expand`] computes the whole wanted set and reconciles the document with
//! it through the op applier, returning the ops it applied so consumers that
//! follow ops (the Bevy bridge, geometry revisions) see the change. They are
//! not history: undo and redo re-expand from the authored state.

use crate::definition;
use crate::document::{Document, RelKind};
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, Instance, LeafPath, LeafStruct, Override, Use};
use crate::id::{NodeId, Version};
use crate::op::{self, Op};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use std::collections::{BTreeMap, BTreeSet};

/// One node an instance should have.
struct Want {
    id: NodeId,
    ty: TypeId,
    parent: NodeId,
    order: OrderKey,
    facets: Vec<Facet>,
}

/// The definition a linked entity instantiates, if it is one.
pub fn linked_definition(doc: &Document, entity: NodeId) -> Option<NodeId> {
    let inst = doc.get::<Instance>(entity)?;
    if inst.mode.constant().and_then(|v| v.as_str()) != Some("linked") {
        return None;
    }
    let def = inst.def.constant()?.as_ref()?;
    doc.live(def).filter(|n| n.ty.name() == "definition").map(|_| def)
}

/// The entity a `use` figures.
pub fn use_entity(doc: &Document, u: NodeId) -> Option<NodeId> {
    doc.relations_from(u).find(|r| r.rel == RelKind::Figures && doc.is_live(r.to)).map(|r| r.to)
}

/// Every expanded node a document should have, parents before children.
fn wanted(doc: &Document) -> Vec<Want> {
    let mut out = Vec::new();
    let entity_ty = TypeId::lookup("entity").expect("registered");
    for entity in doc.nodes_of_type(entity_ty).filter(|&e| doc.is_live(e)).collect::<Vec<_>>() {
        let Some(def) = linked_definition(doc, entity) else { continue };
        let inputs = definition::inputs_of(doc, def).unwrap_or_default();
        // Interface and behaviour, under the entity.
        let follow = |leaf: Leaf| -> Leaf {
            match NodeId::expanded(entity, leaf.node) {
                Some(n) if is_interface(doc, def, leaf.node) => Leaf { node: n, path: leaf.path },
                _ => leaf,
            }
        };
        for &spec in doc.children(def) {
            let Some(node) = doc.live(spec) else { continue };
            let ty = match node.ty.name() {
                "port_spec" => TypeId::lookup("port").expect("registered"),
                "equation" => node.ty,
                _ => continue,
            };
            let Some(id) = NodeId::expanded(entity, spec) else { continue };
            let mut facets = rewritten(doc, spec, &follow);
            if let Some(input) = inputs.iter().find(|i| i.spec == spec) {
                let arg = Leaf { node: entity, path: LeafPath::new(FacetKind::Args, input.name).expect("argument path") };
                for f in &mut facets {
                    if let Facet::Port(p) = f {
                        p.signal = Slot::Bound(Expr::Ref(arg));
                    }
                }
            }
            apply_overrides(doc, entity, spec, ty, &mut facets);
            out.push(Want { id, ty, parent: entity, order: node.order.clone(), facets });
        }
    }
    let use_ty = TypeId::lookup("use").expect("registered");
    for u in doc.nodes_of_type(use_ty).filter(|&u| doc.is_live(u)).collect::<Vec<_>>() {
        let Some(entity) = use_entity(doc, u) else { continue };
        let Some(def) = linked_definition(doc, entity) else { continue };
        let Some(notation) = doc.get::<Use>(u).and_then(|x| x.notation.constant()).and_then(|v| v.as_str()) else { continue };
        let Some(template) = definition::template_of(doc, def, notation) else { continue };
        let inside: BTreeSet<NodeId> = doc.descendants(template).into_iter().collect();
        let follow = |leaf: Leaf| -> Leaf {
            let owner = if inside.contains(&leaf.node) {
                u
            } else if is_interface(doc, def, leaf.node) {
                entity
            } else {
                return leaf;
            };
            match NodeId::expanded(owner, leaf.node) {
                Some(n) => Leaf { node: n, path: leaf.path },
                None => leaf,
            }
        };
        // Preorder, so every parent is wanted before its children.
        let mut stack: Vec<NodeId> = doc.children(template).iter().rev().copied().collect();
        while let Some(local) = stack.pop() {
            let Some(node) = doc.live(local) else { continue };
            let parent = match node.parent {
                Some(p) if p == template => Some(u),
                Some(p) => NodeId::expanded(u, p),
                None => None,
            };
            if let (Some(id), Some(parent)) = (NodeId::expanded(u, local), parent) {
                let mut facets = rewritten(doc, local, &follow);
                apply_overrides(doc, u, local, node.ty, &mut facets);
                out.push(Want { id, ty: node.ty, parent, order: node.order.clone(), facets });
            }
            stack.extend(doc.children(local).iter().rev().copied());
        }
    }
    out
}

/// The live `override` children of an instance, as (node, target, leaf,
/// value); one whose leaf does not parse is left out (it is stale).
fn overrides(doc: &Document, owner: NodeId) -> Vec<(NodeId, NodeId, LeafPath, Value)> {
    doc.children(owner)
        .iter()
        .copied()
        .filter(|&c| doc.live(c).is_some_and(|n| n.ty.name() == "override"))
        .filter_map(|c| {
            let o = doc.get::<Override>(c)?;
            let target = o.target.constant()?.as_ref()?;
            let leaf: LeafPath = o.leaf.constant()?.as_str()?.parse().ok()?;
            Some((c, target, leaf, o.value.constant()?.clone()))
        })
        .collect()
}

/// Apply `owner`'s overrides of `local` to the facets its copy will have
/// (component-model.md §5.3): each sets one leaf to a constant, adding the
/// facet when the node type admits it and the copy lacks it.
fn apply_overrides(doc: &Document, owner: NodeId, local: NodeId, ty: TypeId, facets: &mut Vec<Facet>) {
    for (_, target, leaf, value) in overrides(doc, owner) {
        if target != local {
            continue;
        }
        if !facets.iter().any(|f| f.kind() == leaf.facet) {
            if !ty.spec().allows(leaf.facet) {
                continue;
            }
            facets.push(leaf.facet.default_facet());
        }
        if let Some(f) = facets.iter_mut().find(|f| f.kind() == leaf.facet) {
            let _ = f.set(leaf.field, Slot::Const(value));
        }
    }
}

/// Overrides that apply nothing (component-model.md §5.3): their instance
/// is not linked, their target is gone from the definition, or their leaf
/// is not one the target can have. Kept, inert, and listed with why.
pub fn stale_overrides(doc: &Document) -> Vec<(NodeId, String)> {
    let ty = TypeId::lookup("override").expect("registered");
    let mut out = Vec::new();
    for o in doc.nodes_of_type(ty).filter(|&o| doc.is_live(o)) {
        let Some(owner) = doc.live(o).and_then(|n| n.parent) else { continue };
        let entity = if doc.live(owner).is_some_and(|n| n.ty.name() == "use") { use_entity(doc, owner) } else { Some(owner) };
        let Some(def) = entity.and_then(|e| linked_definition(doc, e)) else {
            out.push((o, format!("{owner} is not a linked instance")));
            continue;
        };
        let Some(facet) = doc.get::<Override>(o) else { continue };
        let target = facet.target.constant().and_then(|v| v.as_ref());
        let leaf = facet.leaf.constant().and_then(|v| v.as_str()).and_then(|l| l.parse::<LeafPath>().ok());
        let (Some(target), Some(leaf)) = (target, leaf) else {
            out.push((o, "its target or leaf is not well formed".into()));
            continue;
        };
        let derived = NodeId::expanded(owner, target).filter(|&d| doc.is_live(d));
        match derived {
            None => out.push((o, format!("{def} has no {target} for {owner} to derive"))),
            Some(d) if doc.leaf(d, leaf).is_none() => out.push((o, format!("{target} has no {leaf}"))),
            Some(_) => {}
        }
    }
    out
}

/// A definition's `port_spec`s and equations: what its entity copies.
fn is_interface(doc: &Document, def: NodeId, node: NodeId) -> bool {
    doc.live(node).is_some_and(|n| n.parent == Some(def) && matches!(n.ty.name(), "port_spec" | "equation"))
}

/// `node`'s facets with every binding's references passed through `follow`.
fn rewritten(doc: &Document, node: NodeId, follow: &dyn Fn(Leaf) -> Leaf) -> Vec<Facet> {
    doc.facets_of(node)
        .map(|c| {
            let mut c = c.clone();
            for field in c.field_names() {
                if let Some(expr) = c.get(field).and_then(|s| s.expr()) {
                    let e = expr.map_refs(follow);
                    c.set(field, Slot::Bound(e)).expect("field exists");
                }
            }
            c
        })
        .collect()
}

/// Bring the document's expanded nodes to what its linked instances derive.
/// Returns the ops applied (for consumers that follow ops) and the problems
/// met (a template node the registry no longer admits, say), which leave
/// that node out rather than failing the transaction.
pub fn expand(doc: &mut Document, stamp: Version) -> (Vec<Op>, Vec<String>) {
    let want = wanted(doc);
    let wanted_ids: BTreeSet<NodeId> = want.iter().map(|w| w.id).collect();
    let mut ops = Vec::new();
    let mut problems = Vec::new();
    let mut run = |doc: &mut Document, op: Op, ops: &mut Vec<Op>| match op::apply_op(doc, 0, &op, stamp) {
        Ok(_) => ops.push(op),
        Err(e) => problems.push(format!("expanding {}: {}", op_target(&op), e.message)),
    };
    // Gone: deepest first, so a parent's delete never meets a dead child.
    let stale: Vec<NodeId> = doc.expanded_ids().filter(|id| doc.is_live(*id) && !wanted_ids.contains(id)).collect();
    let mut stale: Vec<(usize, NodeId)> = stale.into_iter().map(|id| (doc.depth(id), id)).collect();
    stale.sort_by(|a, b| b.cmp(a));
    for (_, id) in stale {
        if doc.is_live(id) {
            run(doc, Op::Delete { id }, &mut ops);
        }
    }
    for w in want {
        let Some(node) = doc.live(w.id) else {
            run(doc, Op::Create { id: w.id, ty: w.ty, parent: Some(w.parent), order: w.order, facets: w.facets }, &mut ops);
            continue;
        };
        if node.ty != w.ty {
            run(doc, Op::Delete { id: w.id }, &mut ops);
            run(doc, Op::Create { id: w.id, ty: w.ty, parent: Some(w.parent), order: w.order, facets: w.facets }, &mut ops);
            continue;
        }
        if node.parent != Some(w.parent) || node.order != w.order {
            run(doc, Op::Reparent { id: w.id, parent: Some(w.parent), order: w.order }, &mut ops);
        }
        let have: BTreeMap<FacetKind, Facet> = doc.facets_of(w.id).map(|f| (f.kind(), f.clone())).collect();
        let mut kept = BTreeSet::new();
        for f in w.facets {
            kept.insert(f.kind());
            match have.get(&f.kind()) {
                None => run(doc, Op::AddFacet { id: w.id, facet: f }, &mut ops),
                Some(old) if *old != f => {
                    if f.kind().is_dynamic() {
                        run(doc, Op::RemoveFacet { id: w.id, kind: f.kind() }, &mut ops);
                        run(doc, Op::AddFacet { id: w.id, facet: f }, &mut ops);
                    } else {
                        for field in f.field_names() {
                            let slot = f.get(field).expect("own field").clone();
                            if old.get(field) != Some(&slot) {
                                let path = LeafPath::new(f.kind(), field).expect("declared field");
                                run(doc, Op::Set { id: w.id, path, slot }, &mut ops);
                            }
                        }
                    }
                }
                Some(_) => {}
            }
        }
        for kind in have.keys().filter(|k| !kept.contains(k)) {
            run(doc, Op::RemoveFacet { id: w.id, kind: *kind }, &mut ops);
        }
    }
    (ops, problems)
}

fn op_target(op: &Op) -> String {
    match op {
        Op::Create { id, .. } | Op::Delete { id } | Op::Reparent { id, .. } | Op::Reorder { id, .. } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => id.to_string(),
        Op::Link { rel } => rel.id.to_string(),
        Op::Unlink { id } => id.to_string(),
    }
}

/// Whether a transaction's ops can change what instances derive: anything
/// structural, or a set on an instance, a `use`, or a node of a definition.
pub fn affected_by(doc: &Document, ops: &[Op]) -> bool {
    ops.iter().any(|op| match op {
        Op::Set { id, path, .. } => matches!(path.facet, FacetKind::Instance | FacetKind::Use | FacetKind::Override) || in_library(doc, *id),
        _ => true,
    })
}

fn in_library(doc: &Document, mut id: NodeId) -> bool {
    while let Some(n) = doc.node(id) {
        if n.ty.name() == "library" {
            return true;
        }
        match n.parent {
            Some(p) => id = p,
            None => return false,
        }
    }
    false
}

/// The authored op an edit to an expanded node would need; until edits are
/// translated (component-model.md §7) such ops are refused with this reason.
pub fn refuse(op: &Op) -> Option<String> {
    let target = match op {
        Op::Create { id, parent, .. } => [Some(*id), *parent].into_iter().flatten().find(|n| n.is_expanded()),
        Op::Delete { id } | Op::Reorder { id, .. } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => Some(*id).filter(|n| n.is_expanded()),
        Op::Reparent { id, parent, .. } => [Some(*id), *parent].into_iter().flatten().find(|n| n.is_expanded()),
        Op::Link { rel } => Some(rel.from).filter(|n| n.is_expanded()),
        Op::Unlink { .. } => None,
    }?;
    let (owner, _) = target.expanded_parts()?;
    Some(format!("{target} is part of instance {owner}, derived from its definition; edit the definition or detach the instance"))
}

/// Translate edits aimed at expanded nodes into authored ones
/// (component-model.md §7), before a transaction applies:
///
/// - a constant set on a leaf bound to something it can be inverted onto
///   (an `in` port's argument, a slider) sets that source instead;
/// - a constant set on a leaf with a value of its own becomes an override
///   on the instance, created or updated;
/// - a binding set on one is refused: a binding from outside may not reach
///   inside an instance; the definition needs an `in` port for it.
///
/// Every other op passes through; `refuse` turns away structural edits.
pub fn translate(doc: &Document, reactive: &crate::graph::Reactive, ops: Vec<Op>) -> Result<Vec<Op>, crate::op::ApplyError> {
    use crate::op::{ApplyError, ErrorKind};
    // Fresh ids for created overrides go past anything the transaction itself
    // reserved.
    let mut fresh = doc.next_id().raw().max(
        ops.iter()
            .filter_map(|op| match op {
                Op::Create { id, .. } if !id.is_expanded() && !id.is_session() => Some(id.raw() + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0),
    );
    let mut out = Vec::with_capacity(ops.len());
    for (i, op) in ops.into_iter().enumerate() {
        let Op::Set { id, path, slot } = &op else {
            out.push(op);
            continue;
        };
        let Some((owner, local)) = id.expanded_parts() else {
            out.push(op);
            continue;
        };
        let err = |m: String| ApplyError { op: i, kind: ErrorKind::BadId, message: m };
        let value = match slot {
            Slot::Const(v) => v.clone(),
            _ => return Err(err(format!("{id} is inside instance {owner}: a binding from outside cannot reach it; give its definition an `in` port and bind that"))),
        };
        let leaf = Leaf { node: *id, path: *path };
        match crate::handle::handle(doc, reactive, leaf) {
            crate::handle::Handle::Locked(reason) => return Err(err(reason)),
            crate::handle::Handle::Linear { .. } => {
                let (source, v) = crate::handle::resolve_target(doc, reactive, leaf, value).map_err(err)?;
                if source.node.is_expanded() {
                    return Err(err(format!("{leaf} resolves onto {source}, inside an instance")));
                }
                out.push(Op::Set { id: source.node, path: source.path, slot: Slot::Const(v) });
            }
            crate::handle::Handle::Direct => {
                let existing = overrides(doc, owner).into_iter().find(|(_, t, l, _)| *t == local && l == path).map(|(o, ..)| o);
                match existing {
                    Some(o) => out.push(Op::Set { id: o, path: "override.value".parse().expect("declared leaf"), slot: Slot::Const(value) }),
                    None => {
                        let o = NodeId::from_raw(fresh);
                        fresh += 1;
                        let after = doc.children(owner).iter().rev().find(|c| !c.is_expanded()).and_then(|&c| doc.live(c)).map(|n| n.order.clone());
                        let order = after.map_or_else(OrderKey::first, |k| OrderKey::after(&k));
                        let mut f = Override::default();
                        f.target = Slot::Const(Value::Ref(local));
                        f.leaf = Slot::Const(Value::Str(path.to_string()));
                        f.value = Slot::Const(value);
                        out.push(Op::Create { id: o, ty: TypeId::lookup("override").expect("registered"), parent: Some(owner), order, facets: vec![f.wrap()] });
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The scope rules (component-model.md §8), for a binding written by `op`:
/// inside a definition, a binding reads only that definition's own nodes;
/// outside, a binding reads an instance's insides only through its ports.
pub fn check_scope(doc: &Document, op: &Op) -> Result<(), String> {
    let (node, exprs): (NodeId, Vec<&Expr>) = match op {
        Op::Create { id, facets, .. } => (*id, facets.iter().flat_map(|f| f.field_names().into_iter().filter_map(move |n| f.get(n).and_then(|s| s.expr()))).collect()),
        Op::Set { id, slot, .. } => (*id, slot.expr().into_iter().collect()),
        Op::AddFacet { id, facet } => (*id, facet.field_names().into_iter().filter_map(|n| facet.get(n).and_then(|s| s.expr())).collect()),
        _ => return Ok(()),
    };
    if node.is_expanded() || exprs.is_empty() {
        return Ok(());
    }
    let home = definition_of(doc, node);
    for dep in exprs.iter().flat_map(|e| e.deps()) {
        match home {
            Some(def) => {
                if dep.node != def && !doc.is_ancestor(def, dep.node) {
                    return Err(format!("{node} is part of definition {def}: its bindings read only that definition's own nodes and ports, not {}", dep.node));
                }
            }
            None => {
                if let Some((owner, _)) = dep.node.expanded_parts()
                    && !doc.live(dep.node).is_some_and(|n| n.ty.name() == "port")
                {
                    return Err(format!("{node} reads {dep}, inside instance {owner}; read one of its ports, or give its definition an `out` port for this"));
                }
            }
        }
    }
    Ok(())
}

/// The definition a node belongs to, if it is part of one.
fn definition_of(doc: &Document, mut id: NodeId) -> Option<NodeId> {
    while let Some(n) = doc.node(id) {
        if n.ty.name() == "definition" {
            return Some(id);
        }
        id = n.parent?;
    }
    None
}
