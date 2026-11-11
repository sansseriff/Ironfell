//! The rectangle and ellipse tools (Figma's R and O): drag out a box and
//! release to make a `rect` or an `ellipse` in it. Shift makes a square or a
//! circle, Alt draws from the centre, and a click without a drag makes a
//! 100 × 100 shape at the press. One transaction per shape; the box being
//! dragged is overlay drawing. Afterwards the new shape is selected and the
//! select tool is back, as in Figma.
//!
//! The shape goes where the press lands, like a pen path: into the artboard
//! under it, otherwise into the view's own composition. On a grid (a
//! notation's `snap.grid`, or the composition's shown grid with
//! `grid_snap` on) both corners land on grid points; Ctrl or Cmd bypasses
//! it, as it does object snapping.

use bevy::prelude::*;
use iron_document::facet::{Fill, Name as DocName, Radius, Size, Transform2d};
use iron_document::composition::Dims;
use iron_document::{Actor, LeafStruct, NodeId, Op, Slot, TransactionInput, TypeId, Value};
use kurbo::{Affine, Point, Rect};

use crate::bevy_app::pen::{after_last, drop_target};
use crate::bevy_app::select::{ActiveTool, Geo};
use crate::document_bridge::{DocumentStore, PanelViews, PendingTransactions};

use crate::bevy_app::tolerances::SLOP;
/// A clicked shape's size, document px.
const CLICKED: f64 = 100.0;
/// Figma's default fill for a new shape.
const FILL: [f32; 4] = [0.851, 0.851, 0.851, 1.0];

/// A box being dragged out.
pub struct ShapePress {
    pub panel: String,
    /// Where the shape will go, and the view's document px → its space.
    pub container: NodeId,
    pub to_comp: Affine,
    /// The press, window px.
    pub start: Point,
    /// The box so far, in the container's space; `None` until it is a drag.
    pub rect: Option<Rect>,
}

#[derive(Resource, Default)]
pub struct ShapeTool {
    pub press: Option<ShapePress>,
    /// A shape just made, to select once its transaction has applied.
    select: Option<NodeId>,
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

#[allow(clippy::too_many_arguments)]
pub fn shape_tool_system(
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    views: Res<PanelViews>,
    mut active: ResMut<ActiveTool>,
    mut shapes: ResMut<ShapeTool>,
    mut sel: ResMut<crate::SelectionState>,
    store: Res<DocumentStore>,
    mut pending: ResMut<PendingTransactions>,
    spatial: Res<crate::document_bridge::SpatialScene>,
    drawables: Res<crate::document_bridge::Drawables>,
    focus: Res<crate::bevy_app::view_focus::ViewFocus>,
) {
    let doc = store.0.document();
    if let Some(id) = shapes.select.filter(|&id| doc.live(id).is_some()) {
        sel.selected = vec![id];
        shapes.select = None;
    }
    let ty = match *active {
        ActiveTool::Rect => "rect",
        ActiveTool::Ellipse => "ellipse",
        _ => {
            shapes.press = None;
            return;
        }
    };
    let screen = Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
    if pointer.just_pressed_left
        && let Some(view) = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two)
    {
        let geo = Geo { store: &store.0, drawables: &drawables, spatial: &spatial };
        // Drawn on a component's main, a shape joins its definition
        // (component-authoring.md §1).
        let main = crate::bevy_app::select::top_hit(&hits)
            .filter(|(chain, _)| !chain.iter().any(|&n| crate::document_bridge::nest::is_embed(doc, n)))
            .and_then(|(chain, _)| chain.into_iter().find(|&n| iron_document::expand::main_definition(doc, n).is_some()));
        shapes.press = match main {
            Some(m) => Some(ShapePress { panel: view.panel.clone(), container: m, to_comp: geo.world(m).inverse(), start: screen, rect: None }),
            None => drop_target(&geo, &hits, view).map(|(container, to_comp)| ShapePress { panel: view.panel.clone(), container, to_comp, start: screen, rect: None }),
        };
    }
    let Some(press) = shapes.press.as_mut() else { return };
    let Some(view) = views.get(&press.panel) else {
        shapes.press = None;
        return;
    };
    // The box in the container's space, from the press to the pointer,
    // each end on the grid (in the view's document px) when there is one.
    let grid = Some(focus.grid(&press.panel))
        .filter(|&g| g > 0.0)
        .or_else(|| crate::bevy_app::view_focus::canvas_grid(&store.0, view))
        .filter(|_| !(pointer.modifiers.ctrl || pointer.modifiers.meta));
    let on_grid = |p: Point| grid.map_or(p, |g| Point::new((p.x / g).round() * g, (p.y / g).round() * g));
    let to_doc = view.affine().inverse();
    let a = press.to_comp * on_grid(to_doc * press.start);
    let mut d = press.to_comp * on_grid(to_doc * screen) - a;
    if pointer.modifiers.shift {
        let side = d.x.abs().max(d.y.abs());
        d = kurbo::Vec2::new(side.copysign(d.x), side.copysign(d.y));
    }
    let rect = if pointer.modifiers.alt { Rect::from_points(a - d, a + d) } else { Rect::from_points(a, a + d) };
    if press.rect.is_some() || press.start.distance(screen) > SLOP as f64 {
        press.rect = Some(rect);
    }
    if !pointer.just_released_left {
        return;
    }
    let Some(press) = shapes.press.take() else { return };
    let r = press.rect.unwrap_or_else(|| Rect::from_origin_size(a, (CLICKED, CLICKED)));
    if r.width() < 1.0 && r.height() < 1.0 {
        return;
    }
    let id = doc.next_ids(1)[0];
    let mut t = Transform2d::default();
    let geometry = if ty == "rect" {
        t.x = n(r.x0);
        t.y = n(r.y0);
        let mut s = Size::default();
        s.w = n(r.width());
        s.h = n(r.height());
        s.wrap()
    } else {
        let c = r.center();
        t.x = n(c.x);
        t.y = n(c.y);
        let mut rad = Radius::default();
        rad.rx = n(r.width() * 0.5);
        rad.ry = n(r.height() * 0.5);
        rad.wrap()
    };
    let mut fill = Fill::default();
    fill.color = Slot::Const(Value::Color(FILL));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str(if ty == "rect" { "rectangle" } else { "ellipse" }.into()));
    let op = Op::Create {
        id,
        ty: TypeId::lookup(ty).expect("registered"),
        parent: Some(press.container),
        order: after_last(doc, Some(press.container)),
        facets: vec![t.wrap(), geometry, fill.wrap(), name.wrap()],
    };
    let label = format!("draw {}", if ty == "rect" { "rectangle" } else { "ellipse" });
    pending.0.push(TransactionInput { label, actor: Actor::Human, ts: 0, idempotency_key: None, ops: vec![op], exact: false });
    // Inside a main, what is drawn is the main's copy of the new node.
    shapes.select = if iron_document::expand::main_definition(doc, press.container).is_some() { NodeId::expanded(press.container, id) } else { Some(id) };
    *active = ActiveTool::Select;
}
