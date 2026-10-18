//! A path's outlines (plans/specs/svg-import-export.md phase 3).
//!
//! A `path` holds `contour` children, each one outline: its `point`
//! children in order, closed or open. Several contours make a compound path
//! (a donut, the letter "O"), and the path's `fill_rule` decides which
//! enclosed areas are inside. Reading and building them lives here, so the
//! renderer, the editors, shape conversion and SVG agree.

use crate::facet::{Contour as ContourShape, Handles, LeafStruct, Transform2d};
use crate::document::Document;
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::shape::Anchor;
use crate::value::{Slot, Value};
use crate::Store;
use kurbo::{BezPath, Point, Vec2};

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

/// `path`'s contours in order, each with its points.
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

/// One contour's outline: a line between two points whose facing handles
/// are both zero, a cubic otherwise.
fn append(path: &mut BezPath, points: &[Anchor], closed: bool) {
    let Some(first) = points.first() else { return };
    path.move_to(first.at);
    let seg = |path: &mut BezPath, a: &Anchor, b: &Anchor| {
        if a.out == Vec2::ZERO && b.inn == Vec2::ZERO {
            path.line_to(b.at);
        } else {
            path.curve_to(a.at + a.out, b.at + b.inn, b.at);
        }
    };
    for w in points.windows(2) {
        seg(path, &w[0], &w[1]);
    }
    if closed && points.len() > 2 {
        seg(path, points.last().expect("non-empty"), first);
        path.close_path();
    }
}

/// Every contour's outline: what is stroked.
pub fn outline(contours: &[Contour]) -> BezPath {
    let mut path = BezPath::new();
    for c in contours {
        append(&mut path, &c.points.iter().map(|p| p.1).collect::<Vec<_>>(), c.closed);
    }
    path
}

/// The closed contours' outline: what is filled. An open contour has no
/// inside.
pub fn fill_outline(contours: &[Contour]) -> BezPath {
    let mut path = BezPath::new();
    for c in contours.iter().filter(|c| draws_closed(c)) {
        append(&mut path, &c.points.iter().map(|p| p.1).collect::<Vec<_>>(), true);
    }
    path
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
