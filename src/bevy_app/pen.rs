//! The pen tool (roadmap stage E, §4): Figma's pen over the document's
//! `path` and `point` nodes.
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

use bevy::prelude::*;
use iron_document::component::{Handles, Name as DocName, PathShape, Stroke, Transform2d};
use iron_document::composition::{self, Dims};
use iron_document::{Actor, LeafStruct, NodeId, Op, OrderKey, Slot, TransactionInput, TypeId, Value};
use kurbo::{Affine, Point};

use crate::bevy_app::select::{ActiveTool, Geo, Source, ToolKeys, top_hit};
use crate::document_bridge::{Doc2d, DocumentStore, NodeMap, PanelView, PanelViews, PendingTransactions, nest};

/// Window px within which a click on the first point closes the path.
const CLOSE_HIT: f64 = 8.0;
/// Window px a press must travel to make a smooth point instead of a corner.
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
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

fn point_op(id: NodeId, parent: NodeId, order: OrderKey, at: Point, out: kurbo::Vec2) -> Op {
    let mut t = Transform2d::default();
    t.x = n(at.x);
    t.y = n(at.y);
    let mut h = Handles::default();
    h.ix = n(-out.x);
    h.iy = n(-out.y);
    h.ox = n(out.x);
    h.oy = n(out.y);
    Op::Create { id, ty: TypeId::lookup("point").expect("registered"), parent: Some(parent), order, components: vec![t.wrap(), h.wrap()] }
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
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    views: Res<PanelViews>,
    mut active: ResMut<ActiveTool>,
    mut pen: ResMut<PenTool>,
    mut keys: ResMut<ToolKeys>,
    store: Res<DocumentStore>,
    mut pending: ResMut<PendingTransactions>,
    map: Res<NodeMap>,
    globals: Query<&'static GlobalTransform>,
    nodes: Query<(&'static Doc2d, &'static GlobalTransform)>,
    mut shown: Local<Option<ActiveTool>>,
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
        crate::web_ffi::send_tool_from_worker(active.name().into());
        *shown = Some(*active);
    }
    if *active != ActiveTool::Pen {
        pen.press = None;
        pen.pointer = None;
        return;
    }

    let screen = Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
    let view = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two);
    pen.pointer = view.map(|v| (v.panel.clone(), screen));
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
    let geo = Geo { store: &store.0, map: &map, globals: &globals, nodes: &nodes };
    let to_doc = view.affine().inverse();
    let mut drag = (to_doc * end) - (to_doc * start);
    if drag.hypot() * view.zoom as f64 <= DRAG {
        drag = kurbo::Vec2::ZERO;
    } else if pointer.modifiers.shift {
        // Shift keeps the handle on an axis.
        if drag.x.abs() > drag.y.abs() { drag.y = 0.0 } else { drag.x = 0.0 }
    }

    // Continue the path being drawn in this view, or start one.
    if let Some((path, path_panel, to_comp)) = pen.path.clone().filter(|(id, p, _)| p == &panel && doc.live(*id).is_some()) {
        let points = geo.points(path);
        let to_local = geo.world(path).inverse() * to_comp;
        let at = to_local * (to_doc * start);
        let out = {
            let [a, b, c, d, _, _] = to_local.as_coeffs();
            kurbo::Vec2::new(a * drag.x + c * drag.y, b * drag.x + d * drag.y)
        };
        // Back on the first point: close.
        let to_screen = view.affine() * to_comp.inverse() * geo.world(path);
        if points.len() >= 2 && points.first().is_some_and(|f| (to_screen * f.1).distance(start) <= CLOSE_HIT) {
            let mut ops = vec![Op::Set { id: path, path: "path.closed".parse().expect("declared"), slot: Slot::Const(Value::Bool(true)) }];
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
        pending.0.push(transaction("add point", vec![point_op(id, path, after_last(doc, Some(path)), at, out)]));
        let _ = path_panel;
        return;
    }

    // A new path: into the artboard under the press, else the view's own
    // composition.
    let Some((container, to_comp)) = drop_target(&geo, &hits, view) else { return };
    let origin = to_comp * (to_doc * start);
    let out = drag;
    let ids = doc.next_ids(2);
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
        point_op(ids[1], ids[0], OrderKey::first(), Point::ZERO, out),
    ];
    pending.0.push(transaction("draw path", ops));
    pen.path = Some((ids[0], panel, to_comp));
}
