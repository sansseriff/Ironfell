//! Make component (component-model.md §5.5): a selection of placed
//! instances becomes a new definition, and one linked instance of it takes
//! the selection's place.
//!
//! The new definition holds the selected instances as *inner* entities (each
//! still linked to its own definition) and a template of their pictures.
//! Connections wholly inside move in. What crosses the boundary gets a port
//! on the new component, wired through inside (§4.1):
//!
//! - a connection out of the selection: an `out` port whose signal reads the
//!   inner port;
//! - a connection into it: an `in` port the inner entity's argument reads;
//! - an across/through pair: a terminal joined to the inner one by a
//!   connection inside;
//! - an argument bound to something outside (a slider): an `in` port the
//!   inner argument reads, the outer instance's argument taking the binding.
//!
//! The outside connections are re-pointed at the new instance's ports, so
//! every signal outside is what it was (ECM §9's black-boxing law). One
//! transaction; undo restores the selection.

use crate::composition;
use crate::document::{Document, RelKind, Relation};
use crate::expr::{Expr, Leaf};
use crate::facet::{Args, Facet, FacetKind, Instance, LeafPath, LeafStruct, Name, Port, Template, Transform2d, Use};
use crate::id::{NodeId, RelationId};
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).expect("registered type")
}

fn named(text: &str) -> Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(text.to_owned()));
    n.wrap()
}

/// What `make` built: the new definition, the instance that replaced the
/// selection, and its picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    pub definition: NodeId,
    pub entity: NodeId,
    pub picture: NodeId,
    pub ops: Vec<Op>,
}

/// The ops that make a component named `name` from `selection`: `use`
/// nodes of linked instances, all in one composition.
pub fn make(doc: &Document, resolved: &dyn Fn(Leaf) -> Option<Value>, selection: &[NodeId], name: &str) -> Result<Made, String> {
    // The selection: pictures, and the instances they picture.
    let mut uses: Vec<NodeId> = Vec::new();
    for &n in selection {
        let node = doc.live(n).ok_or_else(|| format!("{n} is not a live node"))?;
        if node.ty.name() != "use" || n.is_expanded() {
            return Err(format!("{n} is a {}: make a component from placed instances (their pictures)", node.ty.name()));
        }
        if !uses.contains(&n) {
            uses.push(n);
        }
    }
    if uses.is_empty() {
        return Err("nothing selected to make a component of".into());
    }
    let comp = composition::composition_of(doc, uses[0]).ok_or("the selection is not in a composition")?;
    if uses.iter().any(|&u| composition::composition_of(doc, u) != Some(comp)) {
        return Err("the selection spans compositions; pick pictures in one".into());
    }
    let notation = doc.get::<Use>(uses[0]).and_then(|u| u.notation.constant()).and_then(|v| v.as_str()).unwrap_or("free").to_owned();
    let mut inner: Vec<NodeId> = Vec::new();
    for &u in &uses {
        let e = crate::expand::use_entity(doc, u).ok_or_else(|| format!("{u} pictures nothing"))?;
        if crate::expand::linked_definition(doc, e).is_none() || e.is_expanded() {
            return Err(format!("{e} is not a linked instance"));
        }
        if !inner.contains(&e) {
            inner.push(e);
        }
    }
    let inner_set: BTreeSet<NodeId> = inner.iter().copied().collect();
    let library = doc.roots().iter().copied().find(|&r| doc.live(r).is_some_and(|n| n.ty.name() == "library")).ok_or("the document has no library")?;
    let model = doc.live(inner[0]).and_then(|n| n.parent);

    // Fresh ids, handed out in order.
    let next_id = Cell::new(doc.next_id().raw());
    let fresh = || {
        let id = NodeId::from_raw(next_id.get());
        next_id.set(next_id.get() + 1);
        id
    };
    let next_rel = Cell::new(doc.next_relation_id().raw());
    let fresh_rel = || {
        let id = RelationId::from_raw(next_rel.get());
        next_rel.set(next_rel.get() + 1);
        id
    };
    let key = |i: usize| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let after_last = |parent: NodeId| doc.children(parent).iter().rev().find(|c| !c.is_expanded()).and_then(|&c| doc.live(c)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order));

    let def = fresh();
    let template = fresh();
    let mut ops = vec![Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: after_last(library), facets: vec![named(name)] }];
    let body_key = Cell::new(0usize);
    let next_body_key = || {
        body_key.set(body_key.get() + 1);
        key(body_key.get())
    };

    // Inner entities: the selected instances, moved into the definition.
    let moved: BTreeMap<NodeId, NodeId> = inner.iter().map(|&e| (e, fresh())).collect();
    // A node of the selection's (an inner entity, or one it derives) as the
    // definition holds it.
    let inside = |n: NodeId| -> Option<NodeId> {
        let path = n.path();
        let to = moved.get(&path[0])?;
        let mut p = vec![*to];
        p.extend_from_slice(&path[1..]);
        NodeId::from_path(&p)
    };
    let rewrite_inside = |e: &Expr| e.map_refs(&|l: Leaf| inside(l.node).map_or(l, |n| Leaf { node: n, path: l.path }));
    let reaches_out = |e: &Expr| e.deps().iter().any(|d| !inner_set.contains(&d.node.path()[0]));

    // Ports of the new component, with their default (or, for an argument
    // bound outside, the binding the outer instance takes).
    let port_names: RefCell<BTreeSet<String>> = RefCell::new(BTreeSet::new());
    let unique = |base: &str| {
        let mut n = base.to_owned();
        let mut i = 2;
        while !port_names.borrow_mut().insert(n.clone()) {
            n = format!("{base}{i}");
            i += 1;
        }
        n
    };
    let outer_args: RefCell<Vec<(String, Slot)>> = RefCell::new(Vec::new());
    let port_ops: RefCell<Vec<Op>> = RefCell::new(Vec::new());
    let new_port = |name: &str, port: Port, outer: Option<Slot>| -> (NodeId, String) {
        let id = fresh();
        let name = unique(name);
        if let Some(slot) = outer {
            outer_args.borrow_mut().push((name.clone(), slot));
        }
        port_ops.borrow_mut().push(Op::Create { id, ty: ty("port_spec"), parent: Some(def), order: next_body_key(), facets: vec![named(&name), port.wrap()] });
        (id, name)
    };

    // The inner entities, their arguments read through: a binding to
    // something outside becomes an `in` port of the component.
    let mut inner_ops = Vec::new();
    for &e in &inner {
        let mut facets: Vec<Facet> = Vec::new();
        for f in doc.facets_of(e) {
            let mut f = f.clone();
            if let Facet::Args(args) = &mut f {
                for field in args.field_names() {
                    let Some(Slot::Bound(expr)) = args.get(field).cloned() else { continue };
                    if reaches_out(&expr) {
                        let mut p = Port::default();
                        p.kind = Slot::Const(Value::Str("value".into()));
                        p.dir = Slot::Const(Value::Enum("in".into()));
                        p.signal = Slot::Const(resolved(Leaf { node: e, path: LeafPath::new(FacetKind::Args, field).expect("argument path") }).unwrap_or(Value::Number(0.0)));
                        let (id, _) = new_port(field, p, Some(Slot::Bound(expr)));
                        args.insert(field, Slot::Bound(Expr::Ref(Leaf { node: id, path: "port.signal".parse().expect("declared") })));
                    } else {
                        args.insert(field, Slot::Bound(rewrite_inside(&expr)));
                    }
                }
            }
            facets.push(f);
        }
        inner_ops.push(Op::Create { id: moved[&e], ty: ty("entity"), parent: Some(def), order: next_body_key(), facets });
        for (i, &o) in doc.children(e).iter().filter(|&&c| doc.live(c).is_some_and(|n| n.ty.name() == "override")).enumerate() {
            inner_ops.push(Op::Create { id: fresh(), ty: ty("override"), parent: Some(moved[&e]), order: key(i), facets: doc.facets_of(o).cloned().collect() });
        }
    }

    // Connections: inside ones move in; crossing ones get a port.
    let mut wiring = Vec::new();
    let mut relink: Vec<(Relation, NodeId)> = Vec::new();
    let conn_ty = ty("connection");
    for c in doc.nodes_of_type(conn_ty).filter(|&c| doc.is_live(c) && !c.is_expanded()).collect::<Vec<_>>() {
        let ends: Vec<&Relation> = doc.relations_from(c).filter(|r| r.rel == RelKind::Connects).collect();
        let ins: Vec<&&Relation> = ends.iter().filter(|r| inner_set.contains(&r.to.path()[0])).collect();
        if ins.is_empty() {
            continue;
        }
        if ins.len() == ends.len() {
            let moved_c = fresh();
            wiring.push(Op::Create { id: moved_c, ty: conn_ty, parent: Some(def), order: next_body_key(), facets: doc.facets_of(c).cloned().collect() });
            for r in &ends {
                wiring.push(Op::Link { rel: Relation { id: fresh_rel(), from: moved_c, to: inside(r.to).ok_or("an inner port has no place in the definition")?, rel: RelKind::Connects, data: r.data.clone() } });
            }
            wiring.push(Op::Delete { id: c });
            continue;
        }
        // One end inside: expose it.
        let r = ins[0];
        let at = doc.get::<Port>(r.to).ok_or_else(|| format!("{} is not a port", r.to))?;
        let kind = at.kind.constant().and_then(|v| v.as_str()).unwrap_or("value").to_owned();
        let dir = at.dir.constant().and_then(|v| v.as_str()).unwrap_or("in").to_owned();
        let flow = crate::port::kind(&kind).map(|k| k.flow);
        let port_name = doc.get::<Name>(r.to).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).unwrap_or("port").to_owned();
        let inner_port = inside(r.to).ok_or("an inner port has no place in the definition")?;
        let mut p = Port::default();
        p.kind = Slot::Const(Value::Str(kind.clone()));
        p.dir = Slot::Const(Value::Enum(dir.clone()));
        match (flow, dir.as_str()) {
            (Some(crate::port::Flow::Signal), "out") => {
                p.signal = Slot::Bound(Expr::Ref(Leaf { node: inner_port, path: "port.signal".parse().expect("declared") }));
                let (id, _) = new_port(&port_name, p, None);
                wiring.push(Op::Link { rel: Relation { id: fresh_rel(), from: id, to: inner_port, rel: RelKind::Exposes, data: BTreeMap::new() } });
                relink.push(((*r).clone(), id));
            }
            (Some(crate::port::Flow::Signal), _) => {
                p.signal = at.signal.constant().cloned().map_or(Slot::Const(Value::Number(0.0)), Slot::Const);
                if !matches!(p.signal, Slot::Const(_)) {
                    p.signal = Slot::Const(Value::Number(0.0));
                }
                let default = resolved(Leaf { node: r.to, path: "port.signal".parse().expect("declared") }).unwrap_or(Value::Number(0.0));
                p.signal = Slot::Const(match default {
                    Value::Quantity { dim, .. } => Value::Quantity { si: 0.0, dim },
                    _ => Value::Number(0.0),
                });
                let (id, _) = new_port(&port_name, p, None);
                // The inner entity's argument for that port reads the new one.
                let (owner, _) = r.to.expanded_parts().ok_or("an inner port is not derived")?;
                if let Some(Op::Create { facets, .. }) = inner_ops.iter_mut().find(|op| matches!(op, Op::Create { id, .. } if Some(*id) == moved.get(&owner).copied())) {
                    for f in facets.iter_mut() {
                        if let Facet::Args(args) = f {
                            args.insert(&port_name, Slot::Bound(Expr::Ref(Leaf { node: id, path: "port.signal".parse().expect("declared") })));
                        }
                    }
                }
                wiring.push(Op::Link { rel: Relation { id: fresh_rel(), from: id, to: inner_port, rel: RelKind::Exposes, data: BTreeMap::new() } });
                relink.push(((*r).clone(), id));
            }
            _ => {
                let (id, _) = new_port(&port_name, p, None);
                let joint = fresh();
                let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
                wiring.push(Op::Create { id: joint, ty: conn_ty, parent: Some(def), order: next_body_key(), facets: doc.facets_of(c).cloned().collect() });
                wiring.push(Op::Link { rel: Relation { id: fresh_rel(), from: joint, to: id, rel: RelKind::Connects, data: end("a") } });
                wiring.push(Op::Link { rel: Relation { id: fresh_rel(), from: joint, to: inner_port, rel: RelKind::Connects, data: end("b") } });
                wiring.push(Op::Link { rel: Relation { id: fresh_rel(), from: id, to: inner_port, rel: RelKind::Exposes, data: BTreeMap::new() } });
                relink.push(((*r).clone(), id));
            }
        }
    }

    // The template: the selection's pictures, each picturing its inner
    // entity.
    let mut template_facet = Template::default();
    template_facet.notation = Slot::Const(Value::Str(notation.clone()));
    let mut picture_ops = vec![Op::Create { id: template, ty: ty("template"), parent: Some(def), order: next_body_key(), facets: vec![template_facet.wrap()] }];
    let mut ordered = uses.clone();
    ordered.sort_by_key(|&u| doc.live(u).map(|n| n.order.clone()));
    for (i, &u) in ordered.iter().enumerate() {
        let copy = fresh();
        picture_ops.push(Op::Create { id: copy, ty: ty("use"), parent: Some(template), order: key(i), facets: doc.facets_of(u).cloned().collect() });
        let e = crate::expand::use_entity(doc, u).expect("checked");
        picture_ops.push(Op::Link { rel: Relation { id: fresh_rel(), from: copy, to: moved[&e], rel: RelKind::Figures, data: BTreeMap::new() } });
        for (j, &o) in doc.children(u).iter().filter(|&&c| !c.is_expanded() && doc.live(c).is_some_and(|n| n.ty.name() == "override")).enumerate() {
            picture_ops.push(Op::Create { id: fresh(), ty: ty("override"), parent: Some(copy), order: key(j), facets: doc.facets_of(o).cloned().collect() });
        }
    }

    // The instance that takes the selection's place, and its picture where
    // the first selected picture was.
    let entity = fresh();
    let picture = fresh();
    let mut instance = Instance::default();
    instance.def = Slot::Const(Value::Ref(def));
    let mut args = Args::default();
    for (n, slot) in outer_args.borrow().iter() {
        args.insert(n, slot.clone());
    }
    let first = ordered[0];
    let mut placed = Use::default();
    placed.notation = Slot::Const(Value::Str(notation));
    let mut replace = vec![
        Op::Create { id: entity, ty: ty("entity"), parent: model, order: model.map_or_else(OrderKey::first, after_last), facets: vec![instance.wrap(), args.wrap(), named(name)] },
        Op::Create { id: picture, ty: ty("use"), parent: doc.live(first).and_then(|n| n.parent), order: doc.live(first).map(|n| n.order.clone()).unwrap_or_else(OrderKey::first), facets: vec![placed.wrap(), Transform2d::default().wrap(), named(name)] },
        Op::Link { rel: Relation { id: fresh_rel(), from: picture, to: entity, rel: RelKind::Figures, data: BTreeMap::new() } },
    ];
    // Crossing connections now end on the instance's ports.
    for (r, port) in relink {
        replace.push(Op::Unlink { id: r.id });
        replace.push(Op::Link { rel: Relation { id: fresh_rel(), from: r.from, to: NodeId::expanded(entity, port).expect("authored ids"), rel: r.rel, data: r.data } });
    }
    // The selection goes: pictures first (the new picture took the first's
    // place, so it is deleted before the new one is created), then entities.
    let mut removal: Vec<Op> = uses.iter().map(|&u| Op::Delete { id: u }).collect();
    removal.extend(inner.iter().map(|&e| Op::Delete { id: e }));

    let mut all = ops;
    all.extend(port_ops.into_inner());
    all.extend(inner_ops);
    all.extend(wiring);
    all.extend(picture_ops);
    all.extend(removal);
    all.extend(replace);
    // Ids were handed out as they were needed, not in the order their nodes
    // are created, and the store takes new ids in ascending order: renumber
    // the fresh ones by creation order, everywhere they appear.
    let first_fresh = doc.next_id().raw();
    let mut order: Vec<NodeId> = Vec::new();
    for op in &all {
        if let Op::Create { id, .. } = op
            && id.raw() >= first_fresh
            && !id.is_expanded()
        {
            order.push(*id);
        }
    }
    let map: BTreeMap<NodeId, NodeId> = order.iter().enumerate().map(|(i, &old)| (old, NodeId::from_raw(first_fresh + i as u64))).collect();
    ops = all.into_iter().map(|op| remap_op(op, &map)).collect();
    let m = |n: NodeId| remap(n, &map);
    Ok(Made { definition: m(def), entity: m(entity), picture: m(picture), ops })
}

/// `n` with every fresh id on its path renumbered.
fn remap(n: NodeId, map: &BTreeMap<NodeId, NodeId>) -> NodeId {
    let path: Vec<NodeId> = n.path().into_iter().map(|p| map.get(&p).copied().unwrap_or(p)).collect();
    NodeId::from_path(&path).unwrap_or(n)
}

fn remap_facet(mut f: Facet, map: &BTreeMap<NodeId, NodeId>) -> Facet {
    for field in f.field_names() {
        match f.get(field).cloned() {
            Some(Slot::Bound(e)) => {
                let e = e.map_refs(&|l: Leaf| Leaf { node: remap(l.node, map), path: l.path });
                let _ = f.set(field, Slot::Bound(e));
            }
            Some(Slot::Const(Value::Ref(r))) => {
                let _ = f.set(field, Slot::Const(Value::Ref(remap(r, map))));
            }
            _ => {}
        }
    }
    f
}

fn remap_op(op: Op, map: &BTreeMap<NodeId, NodeId>) -> Op {
    let m = |n: NodeId| remap(n, map);
    match op {
        Op::Create { id, ty, parent, order, facets } => Op::Create { id: m(id), ty, parent: parent.map(m), order, facets: facets.into_iter().map(|f| remap_facet(f, map)).collect() },
        Op::Delete { id } => Op::Delete { id: m(id) },
        Op::Reparent { id, parent, order } => Op::Reparent { id: m(id), parent: parent.map(m), order },
        Op::Reorder { id, order } => Op::Reorder { id: m(id), order },
        Op::Set { id, path, slot } => {
            let slot = match slot {
                Slot::Bound(e) => Slot::Bound(e.map_refs(&|l: Leaf| Leaf { node: m(l.node), path: l.path })),
                Slot::Const(Value::Ref(r)) => Slot::Const(Value::Ref(m(r))),
                other => other,
            };
            Op::Set { id: m(id), path, slot }
        }
        Op::AddFacet { id, facet } => Op::AddFacet { id: m(id), facet: remap_facet(facet, map) },
        Op::RemoveFacet { id, kind } => Op::RemoveFacet { id: m(id), kind },
        Op::Link { rel } => Op::Link { rel: Relation { from: m(rel.from), to: m(rel.to), ..rel } },
        Op::Unlink { id } => Op::Unlink { id },
    }
}
