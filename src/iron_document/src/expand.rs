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
use crate::document::{Document, RelKind, Relation};
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, Instance, LeafPath, LeafStruct, Override, Use};
use crate::id::{NodeId, RelationId, Version};
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

/// One relation an instance should have: a derived connection's ends, a
/// derived picture's `figures`.
struct RelWant {
    rel: Relation,
}

/// What every instance derives, nodes parents first, then relations.
#[derive(Default)]
struct Wanted {
    nodes: Vec<Want>,
    rels: Vec<RelWant>,
}

/// Instances nest at most this deep; deeper is a definition containing
/// itself, which `check_nesting` refuses, so this only guards expansion.
const MAX_DEPTH: usize = 16;

/// `n`, a node of a definition's content, as instance `owner` derives it:
/// `owner`'s path followed by `n`'s.
fn relocate(owner: NodeId, n: NodeId) -> Option<NodeId> {
    let mut path = owner.path();
    path.extend(n.path());
    NodeId::from_path(&path)
}

/// A definition's own content an entity derives: its port_specs,
/// equations, inner entities and connections.
fn is_body(doc: &Document, def: NodeId, node: NodeId) -> bool {
    doc.node(node).is_some_and(|n| n.parent == Some(def) && matches!(n.ty.name(), "port_spec" | "equation" | "entity" | "connection"))
}

/// The definition an entity instantiates and is linked to, read from an
/// authored or derived entity's facets.
fn linked_def_of(doc: &Document, facets: &[Facet]) -> Option<NodeId> {
    facets.iter().find_map(|f| match f {
        Facet::Instance(inst) if inst.mode.constant().and_then(|v| v.as_str()) == Some("linked") => inst.def.constant()?.as_ref(),
        _ => None,
    }).filter(|&d| doc.live(d).is_some_and(|n| n.ty.name() == "definition"))
}

/// Every expanded node and relation a document should have.
fn wanted(doc: &Document) -> Wanted {
    let mut out = Wanted::default();
    let entity_ty = TypeId::lookup("entity").expect("registered");
    for entity in doc.nodes_of_type(entity_ty).filter(|&e| doc.is_live(e) && !e.is_expanded()).collect::<Vec<_>>() {
        if let Some(def) = linked_definition(doc, entity) {
            want_entity(doc, entity, def, 0, &mut out);
        }
    }
    let use_ty = TypeId::lookup("use").expect("registered");
    for u in doc.nodes_of_type(use_ty).filter(|&u| doc.is_live(u) && !u.is_expanded()).collect::<Vec<_>>() {
        let Some(entity) = use_entity(doc, u) else { continue };
        let Some(def) = linked_definition(doc, entity) else { continue };
        let Some(notation) = doc.get::<Use>(u).and_then(|x| x.notation.constant()).and_then(|v| v.as_str()).map(str::to_owned) else { continue };
        want_use(doc, u, entity, def, &notation, 0, &mut out);
    }
    out
}

/// What entity `entity` (authored, or derived for an outer instance)
/// derives from `def`: its ports and equations, and its inner entities and
/// connections, each inner entity in turn deriving its own.
fn want_entity(doc: &Document, entity: NodeId, def: NodeId, depth: usize, out: &mut Wanted) {
    if depth > MAX_DEPTH {
        return;
    }
    let inputs = definition::inputs_of(doc, def).unwrap_or_default();
    let follow = |leaf: Leaf| -> Leaf {
        match leaf.node.path().first().copied().filter(|&root| is_body(doc, def, root)).and_then(|_| relocate(entity, leaf.node)) {
            Some(n) => Leaf { node: n, path: leaf.path },
            None => leaf,
        }
    };
    for &child in doc.children(def) {
        let Some(node) = doc.live(child) else { continue };
        let ty = match node.ty.name() {
            "port_spec" => TypeId::lookup("port").expect("registered"),
            "equation" | "entity" | "connection" => node.ty,
            _ => continue,
        };
        let Some(id) = NodeId::expanded(entity, child) else { continue };
        let mut facets = rewritten(doc, child, &follow);
        if let Some(input) = inputs.iter().find(|i| i.spec == child) {
            let arg = Leaf { node: entity, path: LeafPath::new(FacetKind::Args, input.name).expect("argument path") };
            for f in &mut facets {
                if let Facet::Port(p) = f {
                    p.signal = Slot::Bound(Expr::Ref(arg));
                }
            }
        }
        apply_overrides(doc, entity, child, ty, &mut facets);
        let inner_def = (node.ty.name() == "entity").then(|| linked_def_of(doc, &facets)).flatten();
        out.nodes.push(Want { id, ty, parent: entity, order: node.order.clone(), facets });
        // A connection's ends, a port's `exposes`: re-pointed for this entity.
        if matches!(node.ty.name(), "connection" | "port_spec") {
            for r in doc.relations_from(child) {
                let to = follow(Leaf { node: r.to, path: LeafPath::new(FacetKind::Name, "text").expect("declared") }).node;
                out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(entity, r.id), from: id, to, rel: r.rel, data: r.data.clone() } });
            }
        }
        if let Some(inner) = inner_def {
            want_entity(doc, id, inner, depth + 1, out);
        }
    }
}

/// What `use` `u`, figuring `entity` (an instance of `def`), derives: the
/// definition's template for `notation`, as its children. A `use` inside
/// the template pictures one of the definition's inner entities, so its
/// copy figures that entity's copy and derives that entity's picture.
fn want_use(doc: &Document, u: NodeId, entity: NodeId, def: NodeId, notation: &str, depth: usize, out: &mut Wanted) {
    if depth > MAX_DEPTH {
        return;
    }
    let Some(template) = definition::template_of(doc, def, notation) else { return };
    let inside: BTreeSet<NodeId> = doc.descendants(template).into_iter().filter(|d| !d.is_expanded()).collect();
    let follow = |leaf: Leaf| -> Leaf {
        let root = leaf.node.path()[0];
        let owner = if inside.contains(&root) {
            u
        } else if is_body(doc, def, root) {
            entity
        } else {
            return leaf;
        };
        match relocate(owner, leaf.node) {
            Some(n) => Leaf { node: n, path: leaf.path },
            None => leaf,
        }
    };
    // Preorder, so every parent is wanted before its children. A template's
    // own derived children (its uses' previews) and overrides are not copied.
    let copied = |n: NodeId| !n.is_expanded() && doc.live(n).is_some_and(|x| x.ty.name() != "override");
    let mut stack: Vec<NodeId> = doc.children(template).iter().rev().copied().filter(|&c| copied(c)).collect();
    while let Some(local) = stack.pop() {
        let Some(node) = doc.live(local) else { continue };
        let parent = match node.parent {
            Some(p) if p == template => Some(u),
            Some(p) => NodeId::expanded(u, p),
            None => None,
        };
        let (Some(id), Some(parent)) = (NodeId::expanded(u, local), parent) else { continue };
        let mut facets = rewritten(doc, local, &follow);
        apply_overrides(doc, u, local, node.ty, &mut facets);
        out.nodes.push(Want { id, ty: node.ty, parent, order: node.order.clone(), facets: facets.clone() });
        if node.ty.name() == "use" {
            // An inner picture: it figures the inner entity's copy, and
            // derives that entity's picture in turn.
            if let Some(r) = doc.relations_from(local).find(|r| r.rel == RelKind::Figures) {
                let inner_entity = follow(Leaf { node: r.to, path: LeafPath::new(FacetKind::Name, "text").expect("declared") }).node;
                out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(u, r.id), from: id, to: inner_entity, rel: RelKind::Figures, data: r.data.clone() } });
                let inner_notation = facets.iter().find_map(|f| match f {
                    Facet::Use(x) => x.notation.constant().and_then(|v| v.as_str()).map(str::to_owned),
                    _ => None,
                });
                let inner_def = doc.node(r.to).and_then(|_| doc.get::<Instance>(r.to)).and_then(|i| {
                    (i.mode.constant().and_then(|v| v.as_str()) == Some("linked")).then(|| i.def.constant().and_then(|v| v.as_ref())).flatten()
                });
                if let (Some(n), Some(d)) = (inner_notation, inner_def) {
                    want_use(doc, id, inner_entity, d, &n, depth + 1, out);
                }
            }
            continue;
        }
        stack.extend(doc.children(local).iter().rev().copied().filter(|&c| copied(c)));
    }
}

/// The overrides that reach `owner`'s copies, as (node, target, leaf, value)
/// with `target` the definition-local node copied, weakest first
/// (§5.3, "Nested targets"). `owner`'s path runs from an authored node
/// through the definition-local nodes each enclosing instance copies; each
/// step's own overrides apply when their target path is the rest of the
/// way down. So the innermost definition's opinion comes first and the
/// outermost instance's last, and the last applied wins.
fn overrides(doc: &Document, owner: NodeId) -> Vec<(NodeId, NodeId, LeafPath, Value)> {
    let path = owner.path();
    let mut out = Vec::new();
    for j in (0..path.len()).rev() {
        let rest = &path[j + 1..];
        for (o, target, leaf, value) in own_overrides(doc, path[j]) {
            let steps = target.path();
            if steps.len() == rest.len() + 1 && steps[..rest.len()] == *rest {
                out.push((o, steps[rest.len()], leaf, value));
            }
        }
    }
    out
}

fn own_overrides(doc: &Document, owner: NodeId) -> Vec<(NodeId, NodeId, LeafPath, Value)> {
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
        let derived = NodeId::from_path(&[owner.path(), target.path()].concat()).filter(|&d| doc.is_live(d));
        match derived {
            None => out.push((o, format!("{def} has no {target} for {owner} to derive"))),
            Some(d) if doc.leaf(d, leaf).is_none() => out.push((o, format!("{target} has no {leaf}"))),
            Some(_) => {}
        }
    }
    out
}

/// `node`'s facets with every reference passed through `follow`: those in
/// bindings, and node references held as constants (an anchor's `port`).
fn rewritten(doc: &Document, node: NodeId, follow: &dyn Fn(Leaf) -> Leaf) -> Vec<Facet> {
    doc.facets_of(node)
        .map(|c| {
            let mut c = c.clone();
            for field in c.field_names() {
                match c.get(field) {
                    Some(Slot::Bound(expr)) => {
                        let e = expr.map_refs(follow);
                        c.set(field, Slot::Bound(e)).expect("field exists");
                    }
                    Some(Slot::Const(Value::Ref(r))) => {
                        let any = LeafPath::new(FacetKind::Name, "text").expect("declared");
                        let to = follow(Leaf { node: *r, path: any }).node;
                        c.set(field, Slot::Const(Value::Ref(to))).expect("field exists");
                    }
                    _ => {}
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
    let Wanted { nodes: want, rels: want_rels } = wanted(doc);
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
    // Relations: stale ones unlinked, changed ones relinked, new ones linked.
    let wanted_rels: BTreeMap<RelationId, Relation> = want_rels.into_iter().map(|w| (w.rel.id, w.rel)).collect();
    let have_rels: Vec<Relation> = doc.relations().filter(|r| r.id.is_expanded()).cloned().collect();
    for r in &have_rels {
        if wanted_rels.get(&r.id) != Some(r) {
            run(doc, Op::Unlink { id: r.id }, &mut ops);
        }
    }
    for (id, rel) in wanted_rels {
        if doc.relation(id) != Some(&rel) && doc.is_live(rel.from) {
            run(doc, Op::Link { rel }, &mut ops);
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
    let mut out: Vec<Op> = Vec::with_capacity(ops.len());
    // An override of `path` on `owner`'s copy of `local`: the one already
    // there (or created earlier in this transaction) updated, else a new one.
    let mut made: Vec<(NodeId, NodeId, LeafPath, NodeId)> = Vec::new();
    let mut override_op = |owner: NodeId, local: NodeId, path: LeafPath, value: Value, out: &mut Vec<Op>| -> Result<(), String> {
        // A part of a nested instance is overridden from the outermost one,
        // its target the path down to the part (§5.3, "Nested targets").
        let mut steps = owner.path();
        let owner = steps.remove(0);
        steps.push(local);
        let local = NodeId::from_path(&steps).ok_or_else(|| format!("{owner}: no path to {local}"))?;
        let existing = own_overrides(doc, owner).into_iter().find(|(_, t, l, _)| *t == local && *l == path).map(|(o, ..)| o).or_else(|| made.iter().find(|m| m.0 == owner && m.1 == local && m.2 == path).map(|m| m.3));
        match existing {
            Some(o) => out.push(Op::Set { id: o, path: "override.value".parse().expect("declared leaf"), slot: Slot::Const(value) }),
            None => {
                let o = NodeId::from_raw(fresh);
                fresh += 1;
                let after = doc.children(owner).iter().rev().find(|c| !c.is_expanded()).and_then(|&c| doc.live(c)).map(|n| n.order.clone());
                let mut order = after.map_or_else(OrderKey::first, |k| OrderKey::after(&k));
                for _ in made.iter().filter(|m| m.0 == owner) {
                    order = OrderKey::after(&order);
                }
                let mut f = Override::default();
                f.target = Slot::Const(Value::Ref(local));
                f.leaf = Slot::Const(Value::Str(path.to_string()));
                f.value = Slot::Const(value);
                made.push((owner, local, path, o));
                out.push(Op::Create { id: o, ty: TypeId::lookup("override").expect("registered"), parent: Some(owner), order, facets: vec![f.wrap()] });
            }
        }
        Ok(())
    };
    for (i, op) in ops.into_iter().enumerate() {
        // A facet added to what an instance derives (an ellipse gaining an
        // arc) is an override of each of its fields.
        if let Op::AddFacet { id, facet } = &op
            && let Some((owner, local)) = id.expanded_parts()
        {
            for field in facet.field_names() {
                if let Some(v) = facet.get(field).and_then(|s| s.constant()) {
                    override_op(owner, local, LeafPath::new(facet.kind(), field).expect("own field"), v.clone(), &mut out).map_err(|m| ApplyError { op: i, kind: ErrorKind::BadId, message: m })?;
                }
            }
            continue;
        }
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
            crate::handle::Handle::Direct => override_op(owner, local, *path, value, &mut out).map_err(err)?,
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
