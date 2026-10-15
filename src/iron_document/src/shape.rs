//! Shapes into paths (select-tool.md, "Shapes and paths").
//!
//! A `rect` or an `ellipse` keeps its parameters (`size`, `radius`) so they
//! can be edited, bound and constrained; editing its outline point by point
//! first turns it into a `path`, as in Figma. The conversion keeps the node's
//! id, placement, transform, paint and name, and its relations (figures,
//! constraints, the session's selection): it is a `Delete` and a restoring
//! `Create` of the same id in one transaction, so one undo brings the shape
//! back.
//!
//! What a conversion cannot keep is the parameters. Anything that reads or
//! drives them is *cut*: a binding on the shape's own size or radius, another
//! node's binding that reads it, a clip that animates it. Bindings that read
//! it are baked to their current value and such clips are deleted. A caller
//! shows the cuts and asks first; with none, nothing is lost.

use crate::component::{Component, ComponentKind, Handles, LeafPath, LeafStruct, PathShape, Transform2d};
use crate::document::RelKind;
use crate::expr::Leaf;
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use crate::Store;

/// The ops that turn a shape into a path, and what they cut.
#[derive(Clone, Debug, PartialEq)]
pub struct Conversion {
    pub ops: Vec<Op>,
    /// One line per binding or clip the conversion breaks, for a person.
    pub cuts: Vec<String>,
}

/// A cubic's handle length for a quarter ellipse, as a fraction of the radius.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// The leaves that are a shape's parameters, by type.
fn parameters(ty: &str) -> &'static [&'static str] {
    match ty {
        "rect" => &["size.w", "size.h"],
        "ellipse" => &["radius.rx", "radius.ry"],
        _ => &[],
    }
}

/// Whether `id` is a shape that can become a path.
pub fn convertible(store: &Store, id: NodeId) -> bool {
    store.document().live(id).is_some_and(|n| !parameters(n.ty.name()).is_empty())
}

/// The points of a shape's outline in its own space: position, in-handle,
/// out-handle (relative), clockwise on screen from the first.
pub fn outline_points(ty: &str, a: f64, b: f64) -> Vec<([f64; 2], [f64; 2], [f64; 2])> {
    match ty {
        "rect" => [[0.0, 0.0], [a, 0.0], [a, b], [0.0, b]].into_iter().map(|p| (p, [0.0; 2], [0.0; 2])).collect(),
        "ellipse" => {
            let (kx, ky) = (KAPPA * a, KAPPA * b);
            vec![
                ([a, 0.0], [0.0, -ky], [0.0, ky]),
                ([0.0, b], [kx, 0.0], [-kx, 0.0]),
                ([-a, 0.0], [0.0, ky], [0.0, -ky]),
                ([0.0, -b], [-kx, 0.0], [kx, 0.0]),
            ]
        }
        _ => Vec::new(),
    }
}

/// The transaction's ops that turn the shape `id` into a closed path with the
/// same outline, or why it cannot be.
pub fn to_path(store: &Store, id: NodeId) -> Result<Conversion, String> {
    let doc = store.document();
    let node = doc.live(id).ok_or_else(|| format!("{id} is not live"))?;
    let ty = node.ty.name();
    let params = parameters(ty);
    if params.is_empty() {
        return Err(format!("{id} is a {ty}; only a rect or an ellipse becomes a path"));
    }
    let leaves: Vec<LeafPath> = params.iter().map(|p| p.parse().expect("declared leaf")).collect();
    let value = |path: LeafPath| store.resolved(id, path).and_then(|v| v.as_f64()).ok_or_else(|| format!("{id} has no {path}"));
    let (a, b) = (value(leaves[0])?, value(leaves[1])?);

    let mut ops = Vec::new();
    let mut cuts = Vec::new();
    // Its own parameters, if driven.
    for &path in &leaves {
        if let Some(e) = doc.leaf(id, path).and_then(|s| s.expr()) {
            cuts.push(format!("its {} is bound to {e}; the path keeps the current value", path.field));
        }
    }
    // Other nodes' bindings that read them: baked to what they are now.
    let mut readers: Vec<(Leaf, String)> = doc
        .bound_slots()
        .filter(|(target, _)| target.node != id)
        .filter(|(_, e)| e.deps().iter().any(|d| d.node == id && leaves.contains(&d.path)))
        .map(|(target, e)| (target, e.to_string()))
        .collect();
    readers.sort_by_key(|(l, _)| (l.node, l.path));
    for (target, e) in readers {
        let Some(now) = store.resolved(target.node, target.path) else { continue };
        cuts.push(format!("{target} reads it ({e}); it keeps its current value"));
        ops.push(Op::Set { id: target.node, path: target.path, slot: Slot::Const(now) });
    }
    // Clips that animate them.
    for r in doc.relations_to(id).filter(|r| r.rel == RelKind::Animates) {
        let prop = store.resolved(r.from, "clip.prop".parse().expect("declared leaf"));
        if prop.as_ref().and_then(|v| v.as_str()).is_some_and(|p| params.contains(&p)) && doc.live(r.from).is_some() {
            cuts.push(format!("clip {} animates its {}; the clip is deleted", r.from, prop.and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()));
            ops.push(Op::Delete { id: r.from });
        }
    }

    // The node again, as a path: everything but the parameters carries over.
    let carried: Vec<Component> = doc
        .components_of(id)
        .filter(|c| !matches!(c.kind(), ComponentKind::Size | ComponentKind::Radius))
        .cloned()
        .chain(std::iter::once({
            let mut s = PathShape::default();
            s.closed = Slot::Const(Value::Bool(true));
            s.wrap()
        }))
        .collect();
    ops.push(Op::Delete { id });
    ops.push(Op::Create { id, ty: TypeId::lookup("path").expect("registered"), parent: node.parent, order: node.order.clone(), components: carried });
    let points = outline_points(ty, a, b);
    let ids = doc.next_ids(points.len());
    let mut order = OrderKey::first();
    for (&pid, (p, i, o)) in ids.iter().zip(points) {
        let n = |v: f64| Slot::Const(Value::Number(v));
        let mut t = Transform2d::default();
        t.x = n(p[0]);
        t.y = n(p[1]);
        let mut h = Handles::default();
        h.ix = n(i[0]);
        h.iy = n(i[1]);
        h.ox = n(o[0]);
        h.oy = n(o[1]);
        ops.push(Op::Create { id: pid, ty: TypeId::lookup("point").expect("registered"), parent: Some(id), order: order.clone(), components: vec![t.wrap(), h.wrap()] });
        order = OrderKey::after(&order);
    }
    Ok(Conversion { ops, cuts })
}
