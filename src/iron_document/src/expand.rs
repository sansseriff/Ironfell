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
use crate::facet::{Facet, FacetKind, Instance, LeafPath, Use};
use crate::id::{NodeId, Version};
use crate::op::{self, Op};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::Slot;
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
                out.push(Want { id, ty: node.ty, parent, order: node.order.clone(), facets: rewritten(doc, local, &follow) });
            }
            stack.extend(doc.children(local).iter().rev().copied());
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
        Op::Set { id, path, .. } => matches!(path.facet, FacetKind::Instance | FacetKind::Use) || in_library(doc, *id),
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
