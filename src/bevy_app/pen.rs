//! The pen tool (roadmap stage E, §4): Figma's pen over the document's
//! `path`, `contour` and `point` nodes. It draws a path's last contour.
//!
//! P picks it, V goes back to selecting. A click places a corner; a
//! click-drag places a smooth point whose out-handle follows the drag (the
//! in-handle mirrors it). Clicking the first point closes the path; Enter or
//! Esc finishes it open, and Esc with nothing being drawn returns to the
//! select tool. Each placed point is one transaction, so undo removes one
//! point; the line to the pointer is drawn by the overlay and never stored.
//!
//! A new path goes where the press lands: inside a top-level embed's area
//! (an artboard) into the embedded composition, otherwise into the view's
//! own composition.
//!
//! On a segment of the path being drawn or edited (`PathEdit`), a click adds
//! a point there instead, splitting the curve so its shape holds; on one of
//! its points (but the first of a path being drawn, which closes it), a
//! click removes that point and the path stays joined across the gap.

use bevy::prelude::*;
use iron_document::component::{Contour, Name as DocName, PathShape, Stroke, Transform2d};
use iron_document::composition::{self, Dims};
use iron_document::{Actor, LeafStruct, NodeId, Op, OrderKey, Slot, TransactionInput, TypeId, Value};
use kurbo::{Affine, Point};

use crate::bevy_app::select::{ActiveTool, Geo, PathEdit, Source, ToolKeys, top_hit};
use crate::document_bridge::{DocumentStore, PanelView, PanelViews, PendingTransactions, nest};

/// CSS px within which a click on the first point closes the path.
const CLOSE_HIT: f64 = 8.0;
/// CSS px a press must travel to make a smooth point instead of a corner.
const DRAG: f64 = 3.0;

/// The pen's state between frames.
#[derive(Resource, Default)]
pub struct PenTool {
    /// The path being drawn: its node, the view it is drawn in, and the
    /// view's document px → the path's composition space.
    pub path: Option<(NodeId, String, Affine)>,
    /// A press not yet released: its view and where it landed and is now
    /// (window px). A drag shapes the new point's handles.
    pub press: Option<(String, Point, Point)>,
    /// The pointer, window px, for the line to it.
    pub pointer: Option<(String, Point)>,
    /// What a click at the pointer would do besides placing a point (the
    /// cursor says so).
    pub hover: PenHover,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PenHover {
    #[default]
    None,
    /// On the first point of the path being drawn: close it.
    Close,
    /// On a segment: add a point there.
    Add,
    /// On a point: remove it.
    Remove,
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

/// A point under `contour`.
fn point_op(id: NodeId, contour: NodeId, order: OrderKey, at: Point, hi: kurbo::Vec2, ho: kurbo::Vec2) -> Op {
    iron_document::contour::point_op(id, contour, order, &iron_document::shape::Anchor { at, inn: hi, out: ho })
}

/// Add point `id` to `path` on the segment from point `i` to point `j` at
/// parameter `t`, in their contour. A curve is split there (de Casteljau),
/// so the new point's handles and the trimmed handles either side keep its
/// shape; a line gets a corner.
fn insert_point(geo: &Geo, path: NodeId, i: usize, j: usize, t: f64, id: NodeId) -> Option<Vec<Op>> {
    use kurbo::ParamCurve as _;
    let doc = geo.store.document();
    let points = geo.points(path);
    let (.., c) = geo.segments(path).into_iter().find(|s| (s.0, s.1) == (i, j))?;
    let (contour, _, run) = geo.run_of(path, i)?;
    let (a, b) = (points[i], points[j]);
    // The closing segment's new point goes after the contour's last.
    let order = if j == run.start { after_last(doc, Some(contour)) } else { OrderKey::between(Some(&doc.live(a.0)?.order), Some(&doc.live(b.0)?.order)).ok()? };
    let path = contour;
    if a.3 == kurbo::Vec2::ZERO && b.2 == kurbo::Vec2::ZERO {
        return Some(vec![point_op(id, path, order, c.eval(t), kurbo::Vec2::ZERO, kurbo::Vec2::ZERO)]);
    }
    let (l, r) = (c.subsegment(0.0..t), c.subsegment(t..1.0));
    let set = |id: NodeId, leaf: &str, v: f64| Op::Set { id, path: leaf.parse().expect("declared"), slot: n(v) };
    let out = l.p1 - l.p0;
    let inn = r.p2 - r.p3;
    Some(vec![
        set(a.0, "handles.ox", out.x),
        set(a.0, "handles.oy", out.y),
        set(b.0, "handles.ix", inn.x),
        set(b.0, "handles.iy", inn.y),
        point_op(id, path, order, l.p3, l.p2 - l.p3, r.p1 - r.p0),
    ])
}

fn transaction(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

/// The order key after a parent's last child (or the first key).
pub(crate) fn after_last(doc: &iron_document::Document, parent: Option<NodeId>) -> OrderKey {
    let siblings = match parent {
        Some(p) => doc.children(p).to_vec(),
        None => doc.roots().to_vec(),
    };
    siblings.last().and_then(|&s| doc.live(s)).map_or_else(OrderKey::first, |s| OrderKey::after(&s.order))
}

/// Where something drawn at the pointer goes: the 2D artboard (top-level
/// embed) under it, else the view's own composition; and the view's
/// document px → that composition's space.
pub(crate) fn drop_target(geo: &Geo, hits: &crate::PointerHits, view: &PanelView) -> Option<(NodeId, Affine)> {
    let doc = geo.store.document();
    let (container, to_comp) = match top_hit(hits) {
        Some((chain, Source::Overlay(_))) if nest::is_embed(doc, chain[0]) && composition::embedded(doc, chain[0]).is_some_and(|s| composition::dims(doc, s) == Some(Dims::Two)) => {
            (composition::embedded(doc, chain[0]), geo.world(chain[0]).inverse())
        }
        _ => (view.composition, Affine::IDENTITY),
    };
    Some((container.or(view.composition)?, to_comp))
}

#[allow(clippy::too_many_arguments)]
pub fn pen_tool_system(
    events: Res<crate::host_events::HostEvents>,
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    views: Res<PanelViews>,
    mut active: ResMut<ActiveTool>,
    mut pen: ResMut<PenTool>,
    mut keys: ResMut<ToolKeys>,
    store: Res<DocumentStore>,
    mut pending: ResMut<PendingTransactions>,
    spatial: Res<crate::document_bridge::SpatialScene>,
    drawables: Res<crate::document_bridge::Drawables>,
    (dpr, mut shown, mut edit): (Res<crate::Dpr>, Local<Option<ActiveTool>>, ResMut<PathEdit>),
) {
    let pen = &mut *pen;
    let doc = store.0.document();
    // Undo can take the path away (its first point was its creation).
    if pen.path.as_ref().is_some_and(|(id, ..)| doc.live(*id).is_none() && !pending.0.iter().any(|t| t.ops.iter().any(|o| matches!(o, Op::Create { id: c, .. } if c == id)))) {
        pen.path = None;
    }

    // Tool keys: V, P, R and O switch (Figma's); Enter and Esc finish a
    // path (Esc with nothing drawn goes back to selecting), and Esc puts
    // the shape tools down.
    let mut rest = Vec::new();
    for (key, m) in std::mem::take(&mut keys.0) {
        match key.as_str() {
            "p" if m == 0 => *active = ActiveTool::Pen,
            "r" if m == 0 => {
                *active = ActiveTool::Rect;
                pen.path = None;
            }
            "o" if m == 0 => {
                *active = ActiveTool::Ellipse;
                pen.path = None;
            }
            "escape" if matches!(*active, ActiveTool::Rect | ActiveTool::Ellipse) => *active = ActiveTool::Select,
            "v" if m == 0 => {
                *active = ActiveTool::Select;
                pen.path = None;
            }
            "enter" | "escape" if *active == ActiveTool::Pen => {
                if pen.path.take().is_none() && key == "escape" {
                    *active = ActiveTool::Select;
                }
            }
            _ => rest.push((key, m)),
        }
    }
    keys.0 = rest;
    if *shown != Some(*active) {
        events.tool(active.name().into());
        *shown = Some(*active);
    }
    if *active != ActiveTool::Pen {
        pen.press = None;
        pen.pointer = None;
        pen.hover = PenHover::None;
        return;
    }

    let screen = Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
    let view = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two);
    pen.pointer = view.map(|v| (v.panel.clone(), screen));
    let reach = CLOSE_HIT * dpr.0;
    // The paths a click can add a point to in a view: the one being drawn
    // and the one being edited, each with its space → window px.
    let targets = |geo: &Geo, v: &PanelView| -> Vec<(NodeId, Affine)> {
        let drawn = pen.path.as_ref().filter(|(id, p, _)| p == &v.panel && doc.live(*id).is_some()).map(|(id, _, to_comp)| (*id, v.affine() * to_comp.inverse() * geo.world(*id)));
        let edited = edit.path_in(&v.panel).filter(|r| geo.ty(r.node()) == "path").and_then(|r| Some((r.node(), v.affine() * geo.space(&r.chain)? * geo.world(r.node()))));
        drawn.into_iter().chain(edited).collect()
    };
    // A point of a target a click would remove: not the first of the
    // contour being drawn (that closes it), and never leaving its contour
    // fewer than two.
    let removable = |geo: &Geo, v: &PanelView, at: Point| -> Option<NodeId> {
        let drawn = pen.path.as_ref().map(|(id, ..)| *id);
        targets(geo, v).into_iter().find_map(|(path, to_screen)| {
            let points = geo.points(path);
            let runs = geo.runs(path);
            let drawing = runs.last().filter(|_| Some(path) == drawn).map(|r| r.2.start);
            points.iter().enumerate().find(|(i, p)| Some(*i) != drawing && runs.iter().any(|r| r.2.contains(i) && r.2.len() > 2) && (to_screen * p.1).distance(at) <= reach).map(|(_, p)| p.0)
        })
    };
    pen.hover = match view {
        Some(v) => {
            let geo = Geo { store: &store.0, drawables: &drawables, spatial: &spatial };
            let closing = pen.path.as_ref().filter(|(id, p, _)| p == &v.panel && doc.live(*id).is_some()).is_some_and(|(path, _, to_comp)| {
                let points = geo.points(*path);
                let to_screen = v.affine() * to_comp.inverse() * geo.world(*path);
                geo.runs(*path).last().is_some_and(|r| r.2.len() >= 2 && (to_screen * points[r.2.start].1).distance(screen) <= reach)
            });
            if closing {
                PenHover::Close
            } else if removable(&geo, v, screen).is_some() {
                PenHover::Remove
            } else if targets(&geo, v).into_iter().any(|(path, to_screen)| geo.segment_at(path, to_screen, screen, reach).is_some()) {
                PenHover::Add
            } else {
                PenHover::None
            }
        }
        None => PenHover::None,
    };
    if pointer.just_pressed_left
        && let Some(v) = view
    {
        pen.press = Some((v.panel.clone(), screen, screen));
    }
    if let Some((_, _, now)) = pen.press.as_mut() {
        *now = screen;
    }
    if !pointer.just_released_left {
        return;
    }
    let Some((panel, start, end)) = pen.press.take() else { return };
    let Some(view) = views.get(&panel) else { return };
    let geo = Geo { store: &store.0, drawables: &drawables, spatial: &spatial };
    let to_doc = view.affine().inverse();
    let mut drag = (to_doc * end) - (to_doc * start);
    if drag.hypot() * view.zoom as f64 <= DRAG * dpr.0 {
        drag = kurbo::Vec2::ZERO;
    } else if pointer.modifiers.shift {
        // Shift keeps the handle on an axis.
        if drag.x.abs() > drag.y.abs() { drag.y = 0.0 } else { drag.x = 0.0 }
    }

    // On a point: remove it.
    if let Some(point) = removable(&geo, view, start) {
        pending.0.push(transaction("delete point", vec![Op::Delete { id: point }]));
        if edit.point == Some(point) {
            edit.point = None;
        }
        return;
    }

    // On a segment: add a point there.
    if let Some((path, (i, j, t))) = targets(&geo, view).into_iter().find_map(|(path, to_screen)| Some((path, geo.segment_at(path, to_screen, start, reach)?))) {
        let id = doc.next_ids(1)[0];
        if let Some(ops) = insert_point(&geo, path, i, j, t, id) {
            pending.0.push(transaction("add point", ops));
            if edit.path_in(&panel).is_some_and(|r| r.node() == path) {
                edit.point = Some(id);
            }
        }
        return;
    }

    // Continue the path being drawn in this view, or start one.
    if let Some((path, path_panel, to_comp)) = pen.path.clone().filter(|(id, p, _)| p == &panel && doc.live(*id).is_some())
        && let Some((contour, _, run)) = geo.runs(path).pop()
    {
        let points = geo.points(path)[run].to_vec();
        let to_local = geo.world(path).inverse() * to_comp;
        let at = to_local * (to_doc * start);
        let out = {
            let [a, b, c, d, _, _] = to_local.as_coeffs();
            kurbo::Vec2::new(a * drag.x + c * drag.y, b * drag.x + d * drag.y)
        };
        // Back on the first point: close.
        let to_screen = view.affine() * to_comp.inverse() * geo.world(path);
        if points.len() >= 2 && points.first().is_some_and(|f| (to_screen * f.1).distance(start) <= CLOSE_HIT * dpr.0) {
            let mut ops = vec![Op::Set { id: contour, path: "contour.closed".parse().expect("declared"), slot: Slot::Const(Value::Bool(true)) }];
            if out != kurbo::Vec2::ZERO {
                let first = points[0].0;
                for (leaf, v) in [("handles.ix", -out.x), ("handles.iy", -out.y), ("handles.ox", out.x), ("handles.oy", out.y)] {
                    ops.push(Op::Set { id: first, path: leaf.parse().expect("declared"), slot: Slot::Const(Value::Number(v)) });
                }
            }
            pending.0.push(transaction("close path", ops));
            pen.path = None;
            return;
        }
        let id = doc.next_ids(1)[0];
        pending.0.push(transaction("add point", vec![point_op(id, contour, after_last(doc, Some(contour)), at, -out, out)]));
        let _ = path_panel;
        return;
    }

    // A new path: into the artboard under the press, else the view's own
    // composition.
    let Some((container, to_comp)) = drop_target(&geo, &hits, view) else { return };
    let origin = to_comp * (to_doc * start);
    let out = drag;
    let ids = doc.next_ids(3);
    let mut t = Transform2d::default();
    t.x = n(origin.x);
    t.y = n(origin.y);
    let mut stroke = Stroke::default();
    stroke.width = n(2.0);
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("path".into()));
    let ops = vec![
        Op::Create {
            id: ids[0],
            ty: TypeId::lookup("path").expect("registered"),
            parent: Some(container),
            order: after_last(doc, Some(container)),
            components: vec![t.wrap(), PathShape::default().wrap(), stroke.wrap(), name.wrap()],
        },
        Op::Create { id: ids[1], ty: TypeId::lookup("contour").expect("registered"), parent: Some(ids[0]), order: OrderKey::first(), components: vec![Contour::default().wrap()] },
        point_op(ids[2], ids[1], OrderKey::first(), Point::ZERO, -out, out),
    ];
    pending.0.push(transaction("draw path", ops));
    pen.path = Some((ids[0], panel, to_comp));
}
