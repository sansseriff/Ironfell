//! A path's outlines (plans/specs/svg-import-export.md phase 3,
//! plans/specs/packed-paths.md).
//!
//! A `path` takes one of two forms. Packed, its outlines are one value,
//! `path.d` ([`Geometry`]). Expanded, it holds `contour` children, each one
//! outline: its `point` children in order, closed or open. Several contours
//! make a compound path (a donut, the letter "O"), and the path's
//! `fill_rule` decides which enclosed areas are inside. Reading and building
//! them lives here, so the renderer, the editors, shape conversion and SVG
//! agree: what only needs the shape reads [`geometry`], whichever the form;
//! what edits points needs their nodes, [`contours`] of an expanded path.

use crate::facet::{Contour as ContourShape, Handles, LeafStruct, PathShape, Transform2d};
use crate::document::Document;
use crate::geometry::{Geometry, PackedContour};
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::shape::Anchor;
use crate::value::{Slot, Value};
use crate::Store;
use kurbo::{Point, Vec2};
use std::sync::Arc;

/// One outline of a path, from resolved values.
#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    pub id: NodeId,
    pub closed: bool,
    /// Each point's node and its anchor, in order.
    pub points: Vec<(NodeId, Anchor)>,
}

/// How overlapping outlines fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

fn of_type(doc: &Document, id: NodeId, ty: &str) -> bool {
    doc.live(id).is_some_and(|n| n.ty.name() == ty)
}

/// `path`'s authored packed geometry, if it is packed (a non-empty
/// `path.d`): what decides its form.
pub fn packed(doc: &Document, path: NodeId) -> Option<&Arc<Geometry>> {
    doc.get::<PathShape>(path).and_then(|p| p.d.constant()).and_then(Value::as_geometry).filter(|g| !g.is_empty())
}

/// `path`'s outlines whichever form it takes, as resolved (a resize
/// previews a packed path's `d`): its `path.d` when packed, shared, with its
/// outlines cached; built from its `contour` and `point` children when
/// expanded.
pub fn geometry(store: &Store, path: NodeId) -> Arc<Geometry> {
    if let Some(Value::Geometry(g)) = store.resolved(path, "path.d".parse().expect("declared leaf"))
        && !g.is_empty()
    {
        return g;
    }
    Arc::new(Geometry::new(contours(store, path).into_iter().map(|c| PackedContour { closed: c.closed, anchors: c.points.into_iter().map(|p| p.1).collect() }).collect()))
}

/// The ops that give a packed `path` point nodes, so its anchors can be
/// addressed one by one (packed-paths.md §4): `path.d` emptied, and a
/// `contour` per packed contour with a `point` per anchor. Empty when the
/// path is not packed.
pub fn expand_points(store: &Store, path: NodeId) -> Vec<Op> {
    let doc = store.document();
    let Some(g) = packed(doc, path) else { return Vec::new() };
    let runs: Vec<(Vec<Anchor>, bool)> = g.contours().iter().map(|c| (c.anchors.clone(), c.closed)).collect();
    let mut ids = doc.next_ids(ids_needed(&runs)).into_iter();
    let mut ops = vec![Op::Set { id: path, path: "path.d".parse().expect("declared leaf"), slot: Slot::Const(Value::Geometry(Geometry::empty())) }];
    ops.extend(contours_ops(path, &runs, &mut ids));
    ops
}

/// `path`'s contours in order, each with its points: an expanded path's,
/// none for a packed one (see [`expand_points`]).
pub fn contours(store: &Store, path: NodeId) -> Vec<Contour> {
    let doc = store.document();
    let num = |n: NodeId, leaf: &str| store.resolved(n, leaf.parse().expect("declared leaf")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    doc.children(path)
        .iter()
        .filter(|&&c| of_type(doc, c, "contour"))
        .map(|&c| Contour {
            id: c,
            closed: store.resolved(c, "contour.closed".parse().expect("declared")) == Some(Value::Bool(true)),
            points: doc
                .children(c)
                .iter()
                .filter(|&&p| of_type(doc, p, "point"))
                .map(|&p| {
                    let a = Anchor {
                        at: Point::new(num(p, "transform2d.x"), num(p, "transform2d.y")),
                        inn: Vec2::new(num(p, "handles.ix"), num(p, "handles.iy")),
                        out: Vec2::new(num(p, "handles.ox"), num(p, "handles.oy")),
                    };
                    (p, a)
                })
                .collect(),
        })
        .collect()
}

pub fn fill_rule(store: &Store, path: NodeId) -> FillRule {
    match store.resolved(path, "path.fill_rule".parse().expect("declared")) {
        Some(Value::Enum(r)) if r == "evenodd" => FillRule::EvenOdd,
        _ => FillRule::NonZero,
    }
}

/// Whether a contour is drawn closed: closed, with three points or more (two
/// points close onto themselves, which draws nothing more).
pub fn draws_closed(c: &Contour) -> bool {
    c.closed && c.points.len() > 2
}

/// The ops that create one contour of `points` under `path`, with ids from
/// `ids` (one for the contour, one per point).
pub fn contour_ops(path: NodeId, order: OrderKey, points: &[Anchor], closed: bool, ids: &mut impl Iterator<Item = NodeId>) -> Vec<Op> {
    let id = ids.next().expect("an id for the contour");
    let mut c = ContourShape::default();
    c.closed = Slot::Const(Value::Bool(closed));
    let mut ops = vec![Op::Create { id, ty: TypeId::lookup("contour").expect("registered"), parent: Some(path), order, facets: vec![c.wrap()] }];
    let mut key = OrderKey::first();
    for a in points {
        ops.push(point_op(ids.next().expect("an id per point"), id, key.clone(), a));
        key = OrderKey::after(&key);
    }
    ops
}

/// A `point` under `contour` at `order`.
pub fn point_op(id: NodeId, contour: NodeId, order: OrderKey, a: &Anchor) -> Op {
    let n = |v: f64| Slot::Const(Value::Number(v));
    let mut t = Transform2d::default();
    (t.x, t.y) = (n(a.at.x), n(a.at.y));
    let mut h = Handles::default();
    (h.ix, h.iy, h.ox, h.oy) = (n(a.inn.x), n(a.inn.y), n(a.out.x), n(a.out.y));
    Op::Create { id, ty: TypeId::lookup("point").expect("registered"), parent: Some(contour), order, facets: vec![t.wrap(), h.wrap()] }
}

/// The ops that give `path` these contours, each `(points, closed)`, with
/// ids from `ids`: one per contour and one per point.
pub fn contours_ops(path: NodeId, contours: &[(Vec<Anchor>, bool)], ids: &mut impl Iterator<Item = NodeId>) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut key = OrderKey::first();
    for (points, closed) in contours {
        ops.extend(contour_ops(path, key.clone(), points, *closed, ids));
        key = OrderKey::after(&key);
    }
    ops
}

/// How many ids `contours_ops` takes.
pub fn ids_needed(contours: &[(Vec<Anchor>, bool)]) -> usize {
    contours.iter().map(|(p, _)| 1 + p.len()).sum()
}
