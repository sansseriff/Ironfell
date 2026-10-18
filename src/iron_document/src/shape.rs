//! Shapes: their outlines, and turning them into paths (select-tool.md,
//! "Shapes and paths").
//!
//! A `rect` (with optional `corners`) and an `ellipse` (with an optional
//! `arc`) keep their parameters so they can be edited, bound and
//! constrained. Their outline is computed here, once, for both the renderer
//! and the conversion, as anchors with Bézier handles: the same form a
//! `path`'s points take.
//!
//! Editing a shape's outline point by point first turns it into a `path`, as
//! in Figma. The conversion keeps the node's id, placement, transform, paint,
//! shadow and name, and its relations (figures, constraints, the session's
//! selection): it is a `Delete` and a restoring `Create` of the same id in
//! one transaction, so one undo brings the shape back.
//!
//! What a conversion cannot keep is the parameters. Anything that reads or
//! drives them is *cut*: a binding on the shape's own parameters, another
//! node's binding that reads one, a clip that animates one. Bindings that read
//! them are baked to their current value and such clips are deleted. A caller
//! shows the cuts and asks first; with none, nothing is lost.

use crate::facet::{Facet, FacetKind, LeafPath, LeafStruct, PathShape};
use crate::document::RelKind;
use crate::expr::Leaf;
use crate::id::NodeId;
use crate::op::Op;
use crate::registry::TypeId;
use crate::value::Slot;
use crate::Store;
use kurbo::{BezPath, Point, Vec2};
use std::f64::consts::{FRAC_PI_2, TAU};

/// The ops that turn a shape into a path, and what they cut.
#[derive(Clone, Debug, PartialEq)]
pub struct Conversion {
    pub ops: Vec<Op>,
    /// One line per binding or clip the conversion breaks, for a person.
    pub cuts: Vec<String>,
}

/// One anchor of an outline: its position, and its in- and out-handles
/// relative to it (zero for a corner), as a `point` node stores them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub at: Point,
    pub inn: Vec2,
    pub out: Vec2,
}

impl Anchor {
    fn corner(at: Point) -> Anchor {
        Anchor { at, inn: Vec2::ZERO, out: Vec2::ZERO }
    }
}

/// A cubic's handle length for a quarter circle, as a fraction of the radius.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// The leaves that are a shape's parameters, by type.
fn parameters(ty: &str) -> &'static [&'static str] {
    match ty {
        "rect" => &["size.w", "size.h", "corners.tl", "corners.tr", "corners.br", "corners.bl"],
        "ellipse" => &["radius.rx", "radius.ry", "arc.start", "arc.sweep", "arc.inner"],
        _ => &[],
    }
}

/// Whether `id` is a shape that can become a path.
pub fn convertible(store: &Store, id: NodeId) -> bool {
    store.document().live(id).is_some_and(|n| !parameters(n.ty.name()).is_empty())
}

/// Corner radii clamped as Figma draws them: none negative, none above half
/// the shorter side.
pub fn clamp_radii(w: f64, h: f64, radii: [f64; 4]) -> [f64; 4] {
    let most = (w.abs().min(h.abs()) * 0.5).max(0.0);
    radii.map(|r| r.clamp(0.0, most))
}

/// A `w`×`h` rect from the origin, its corners rounded by `radii`
/// (top-left, top-right, bottom-right, bottom-left): one closed contour,
/// clockwise on screen from the top-left corner.
pub fn rect_outline(w: f64, h: f64, radii: [f64; 4]) -> Vec<Anchor> {
    let (w, h) = (w.abs(), h.abs());
    let radii = clamp_radii(w, h, radii);
    // Each corner, with the directions of travel into and out of it.
    let corners = [
        (Point::new(0.0, 0.0), Vec2::new(0.0, -1.0), Vec2::new(1.0, 0.0)),
        (Point::new(w, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)),
        (Point::new(w, h), Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0)),
        (Point::new(0.0, h), Vec2::new(-1.0, 0.0), Vec2::new(0.0, -1.0)),
    ];
    let mut out = Vec::with_capacity(8);
    for ((p, into, from), r) in corners.into_iter().zip(radii) {
        if r <= 0.0 {
            out.push(Anchor::corner(p));
        } else {
            out.push(Anchor { at: p - into * r, inn: Vec2::ZERO, out: into * (KAPPA * r) });
            out.push(Anchor { at: p + from * r, inn: -from * (KAPPA * r), out: Vec2::ZERO });
        }
    }
    out
}

/// Anchors along the ellipse `rx`, `ry` from parameter angle `start`
/// through `sweep`, one cubic per quarter turn at most; the ends have no
/// handle outward.
fn arc_anchors(rx: f64, ry: f64, start: f64, sweep: f64) -> Vec<Anchor> {
    let n = ((sweep.abs() / FRAC_PI_2).ceil() as usize).max(1);
    let step = sweep / n as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let at = |t: f64| Point::new(rx * t.cos(), ry * t.sin());
    let tangent = |t: f64| Vec2::new(-rx * t.sin(), ry * t.cos()) * k;
    (0..=n)
        .map(|i| {
            let t = start + step * i as f64;
            Anchor { at: at(t), inn: if i == 0 { Vec2::ZERO } else { -tangent(t) }, out: if i == n { Vec2::ZERO } else { tangent(t) } }
        })
        .collect()
}

/// An open run of anchors that ends where it began, as one closed contour.
fn closed(mut run: Vec<Anchor>) -> Vec<Anchor> {
    if let Some(last) = run.pop()
        && let Some(first) = run.first_mut()
    {
        first.inn = last.inn;
    }
    run
}

/// An ellipse centred on the origin, or the part of it `arc` (start, sweep,
/// inner) keeps: one closed contour, or two for a full ring (the hole
/// running the other way, so it stays empty under the non-zero rule).
pub fn ellipse_outline(rx: f64, ry: f64, arc: Option<[f64; 3]>) -> Vec<Vec<Anchor>> {
    let [start, sweep, inner] = arc.unwrap_or([0.0, TAU, 0.0]);
    let sweep = sweep.clamp(-TAU, TAU);
    let inner = inner.clamp(0.0, 0.999);
    if sweep.abs() >= TAU - 1e-9 {
        let mut out = vec![closed(arc_anchors(rx, ry, start, TAU))];
        if inner > 0.0 {
            out.push(closed(arc_anchors(rx * inner, ry * inner, start, -TAU)));
        }
        return out;
    }
    let mut ring = arc_anchors(rx, ry, start, sweep);
    if inner > 0.0 {
        ring.extend(arc_anchors(rx * inner, ry * inner, start + sweep, -sweep));
    } else {
        ring.insert(0, Anchor::corner(Point::ORIGIN));
    }
    vec![ring]
}

/// The closed contours as a path: a line between two anchors whose facing
/// handles are both zero, a cubic otherwise.
pub fn to_bez(contours: &[Vec<Anchor>]) -> BezPath {
    let mut path = BezPath::new();
    for c in contours.iter().filter(|c| !c.is_empty()) {
        path.move_to(c[0].at);
        for (a, b) in c.iter().zip(c.iter().skip(1).chain(std::iter::once(&c[0]))) {
            if a.out == Vec2::ZERO && b.inn == Vec2::ZERO {
                path.line_to(b.at);
            } else {
                path.curve_to(a.at + a.out, b.at + b.inn, b.at);
            }
        }
        path.close_path();
    }
    path
}

/// A shape node's outline in its own space, from its resolved parameters.
pub fn outline(store: &Store, id: NodeId) -> Option<Vec<Vec<Anchor>>> {
    let num = |leaf: &str, default: f64| store.resolved(id, leaf.parse().expect("declared leaf")).and_then(|v| v.as_f64()).unwrap_or(default);
    let doc = store.document();
    match doc.live(id)?.ty.name() {
        "rect" => {
            let radii = ["tl", "tr", "br", "bl"].map(|c| num(&format!("corners.{c}"), 0.0));
            Some(vec![rect_outline(num("size.w", 100.0), num("size.h", 100.0), radii)])
        }
        "ellipse" => {
            let arc = doc.facet(id, FacetKind::Arc).map(|_| [num("arc.start", 0.0), num("arc.sweep", TAU), num("arc.inner", 0.0)]);
            Some(ellipse_outline(num("radius.rx", 50.0), num("radius.ry", 50.0), arc))
        }
        _ => None,
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
    // A ring is two outlines, the hole running the other way, so it stays
    // a ring under the path's default non-zero rule.
    let contours = outline(store, id).expect("a shape has an outline");
    let leaves: Vec<LeafPath> = params.iter().map(|p| p.parse().expect("declared leaf")).collect();

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
        let prop = prop.as_ref().and_then(|v| v.as_str()).unwrap_or_default().to_owned();
        if params.contains(&prop.as_str()) && doc.live(r.from).is_some() {
            cuts.push(format!("clip {} animates its {prop}; the clip is deleted", r.from));
            ops.push(Op::Delete { id: r.from });
        }
    }

    // The node again, as a path: everything but the parameters carries over.
    let carried: Vec<Facet> = doc
        .facets_of(id)
        .filter(|c| !matches!(c.kind(), FacetKind::Size | FacetKind::Radius | FacetKind::Corners | FacetKind::Arc))
        .cloned()
        .chain(std::iter::once(PathShape::default().wrap()))
        .collect();
    ops.push(Op::Delete { id });
    ops.push(Op::Create { id, ty: TypeId::lookup("path").expect("registered"), parent: node.parent, order: node.order.clone(), facets: carried });
    let runs: Vec<(Vec<Anchor>, bool)> = contours.into_iter().map(|c| (c, true)).collect();
    let mut ids = doc.next_ids(crate::contour::ids_needed(&runs)).into_iter();
    ops.extend(crate::contour::contours_ops(id, &runs, &mut ids));
    Ok(Conversion { ops, cuts })
}
