//! A stress composition for the render benchmark (roadmap stage H: measure
//! per-view cost before building culling or a raster cache).
//!
//! `stress` holds `n` shapes in cells of 100, every cell a group inside one
//! `field` group, laid out over an area several times a panel's size so that
//! panning, zooming out and culling all have something to measure. The mix is
//! 60% rects (a third stroked, a quarter rotated), 30% circles and 10% closed
//! curved paths of three points. The field's rotation is bound to a `spin`
//! slider: scrubbing it moves every shape while changing one leaf, which is
//! the case a subtree raster cache exists for. Positions come from a fixed
//! xorshift seed, so every run draws the same scene. The first cell's last
//! shape, drawn on top, is a plain 20×20 rect named `sample` at a fixed
//! place, (50, 90) to (70, 110) in the composition, for a benchmark to
//! Cmd-drag: an edit inside a group, which is what invalidates a cached group.

use iron_document::component::{Composition, Fill, Handles, Name, PathShape, Radius, Size, Slider, Stroke, Transform2d};
use iron_document::{Actor, Expr, LeafStruct, NodeId, Op, OrderKey, Slot, Store, TransactionInput, TypeId, Value};

/// Shapes per cell group.
const CELL: usize = 100;
/// A cell's side, in document pixels.
const CELL_SIZE: f64 = 220.0;

/// What the benchmark driver needs to address: the composition, the slider,
/// the field group and one shape near the top-left to drag.
pub struct StressIds {
    pub composition: NodeId,
    /// The sample rect's centre in composition space.
    pub sample_at: (f64, f64),
    pub slider: NodeId,
    pub field: NodeId,
    pub sample: NodeId,
    pub nodes: usize,
    pub width: f64,
    pub height: f64,
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

fn named(s: &str) -> iron_document::Component {
    let mut c = Name::default();
    c.text = Slot::Const(Value::Str(s.into()));
    c.wrap()
}

fn at(x: f64, y: f64, rot: f64) -> iron_document::Component {
    let mut t = Transform2d::default();
    t.x = n(x);
    t.y = n(y);
    t.rot = n(rot);
    t.wrap()
}

fn fill(c: [f32; 4]) -> iron_document::Component {
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color(c));
    f.wrap()
}

fn stroke(c: [f32; 4], w: f64) -> iron_document::Component {
    let mut s = Stroke::default();
    s.color = Slot::Const(Value::Color(c));
    s.width = n(w);
    s.wrap()
}

/// A hue around the colour wheel, at a fixed saturation and value.
fn hue(h: f64) -> [f32; 4] {
    let h = (h.fract() + 1.0).fract() * 6.0;
    let x = (1.0 - ((h % 2.0) - 1.0).abs()) as f32;
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    let (lo, span) = (0.25f32, 0.6f32);
    [lo + span * r, lo + span * g, lo + span * b, 1.0]
}

/// The transaction that adds the stress composition after everything else,
/// replacing an earlier one, so a second run at another size swaps the scene
/// in one undoable step and the `stress` view keeps its name to show.
pub fn stress_scene(store: &Store, shapes: usize) -> (TransactionInput, StressIds) {
    let doc = store.document();
    let ty = |s: &str| TypeId::lookup(s).expect("registered type");
    let shapes = shapes.max(1);
    let cells = shapes.div_ceil(CELL);
    // Wider than tall, like a panel.
    let cols = ((cells as f64 * 1.6).sqrt().ceil() as usize).max(1);
    let rows = cells.div_ceil(cols);
    let (width, height) = (cols as f64 * CELL_SIZE, rows as f64 * CELL_SIZE);

    // Decide every shape first, so the id count is exact.
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    enum Kind {
        Rect { stroked: bool, rot: f64 },
        Circle,
        Path,
    }
    let kinds: Vec<Kind> = (0..shapes)
        .map(|_| {
            let r = rand();
            if r < 0.6 {
                Kind::Rect { stroked: rand() < 0.33, rot: if rand() < 0.25 { rand() * 6.283 } else { 0.0 } }
            } else if r < 0.9 {
                Kind::Circle
            } else {
                Kind::Path
            }
        })
        .collect();
    let paths = kinds.iter().filter(|k| matches!(k, Kind::Path)).count();
    // The sample rect is one extra shape.
    let total = 4 + cells + shapes + 3 * paths;
    let mut ids = doc.next_ids(total).into_iter();
    let mut next = move || ids.next().expect("reserved");

    let root_order = doc.roots().last().and_then(|&r| doc.live(r)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order));
    let mut ops = Vec::with_capacity(total + 1);
    for old in iron_document::composition::compositions(doc) {
        let name = doc.get::<Name>(old).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned)));
        if name.as_deref() == Some("stress") {
            ops.push(Op::Delete { id: old });
        }
    }
    let composition = next();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    ops.push(Op::Create { id: composition, ty: ty("composition"), parent: None, order: root_order, components: vec![named("stress"), c.wrap()] });

    // The field draws first and the slider last, on top, so their ids come
    // in that order (a create's id must not be below the counter). The
    // slider sits outside the field, so it never spins.
    let field = next();
    let slider = next();
    let mut sl = Slider::default();
    sl.min = n(0.0);
    sl.max = n(1.0);
    sl.step = n(0.001);
    sl.value = n(0.0);
    let mut sz = Size::default();
    sz.w = n(300.0);
    sz.h = n(24.0);
    let mut key = OrderKey::first();
    let mut order = move || {
        let k = key.clone();
        key = OrderKey::after(&key);
        k
    };
    let mut field_at = Transform2d::default();
    // Rotation pivots about the field's centre, which starts in view.
    field_at.x = n(40.0 + width / 2.0);
    field_at.y = n(80.0 + height / 2.0);
    field_at.rot = Slot::Bound(Expr::parse(&format!("{slider}.slider.value * 6.2832")).expect("stress binding"));
    ops.push(Op::Create { id: field, ty: ty("group"), parent: Some(composition), order: order(), components: vec![named("field"), field_at.wrap()] });
    ops.push(Op::Create { id: slider, ty: ty("slider"), parent: Some(composition), order: order(), components: vec![named("spin"), at(40.0, 30.0, 0.0), sl.wrap(), sz.wrap()] });

    let mut sample = None;
    let mut kinds = kinds.into_iter();
    let mut cell_key = OrderKey::first();
    for cell_index in 0..cells {
        let cell = next();
        let (cx, cy) = ((cell_index % cols) as f64 * CELL_SIZE - width / 2.0, (cell_index / cols) as f64 * CELL_SIZE - height / 2.0);
        ops.push(Op::Create { id: cell, ty: ty("group"), parent: Some(field), order: cell_key.clone(), components: vec![named(&format!("cell {cell_index}")), at(cx, cy, 0.0)] });
        cell_key = OrderKey::after(&cell_key);
        let mut shape_key = OrderKey::first();
        for _ in 0..CELL {
            let Some(kind) = kinds.next() else { break };
            let id = next();
            let (x, y) = (10.0 + rand() * (CELL_SIZE - 40.0), 10.0 + rand() * (CELL_SIZE - 40.0));
            let colour = hue(cell_index as f64 * 0.07 + rand() * 0.15);
            let order = shape_key.clone();
            shape_key = OrderKey::after(&shape_key);
            match kind {
                Kind::Rect { stroked, rot } => {
                    let mut s = Size::default();
                    s.w = n(8.0 + rand() * 24.0);
                    s.h = n(8.0 + rand() * 24.0);
                    let mut comps = vec![at(x, y, rot), s.wrap(), fill(colour)];
                    if stroked {
                        comps.push(stroke([0.1, 0.1, 0.12, 1.0], 1.5));
                    }
                    ops.push(Op::Create { id, ty: ty("rect"), parent: Some(cell), order, components: comps });
                }
                Kind::Circle => {
                    let mut r = Radius::default();
                    let radius = n(4.0 + rand() * 10.0);
                    r.rx = radius.clone();
                    r.ry = radius;
                    ops.push(Op::Create { id, ty: ty("ellipse"), parent: Some(cell), order, components: vec![at(x, y, 0.0), r.wrap(), fill(colour)] });
                }
                Kind::Path => {
                    let mut shape = PathShape::default();
                    shape.closed = Slot::Const(Value::Bool(true));
                    ops.push(Op::Create { id, ty: ty("path"), parent: Some(cell), order, components: vec![at(x, y, 0.0), shape.wrap(), fill(colour), stroke([0.1, 0.1, 0.12, 1.0], 1.0)] });
                    let mut point_key = OrderKey::first();
                    for (px, py) in [(0.0, 0.0), (24.0 + rand() * 10.0, 4.0), (10.0, 20.0 + rand() * 10.0)] {
                        let mut h = Handles::default();
                        h.ox = n(6.0);
                        h.oy = n(-4.0);
                        h.ix = n(-6.0);
                        h.iy = n(4.0);
                        ops.push(Op::Create { id: next(), ty: ty("point"), parent: Some(id), order: point_key.clone(), components: vec![at(px, py, 0.0), h.wrap()] });
                        point_key = OrderKey::after(&point_key);
                    }
                }
            }
        }
        // Last in cell 0, so nothing in the cell draws over it.
        if cell_index == 0 {
            let id = next();
            sample = Some(id);
            let mut s = Size::default();
            s.w = n(20.0);
            s.h = n(20.0);
            ops.push(Op::Create { id, ty: ty("rect"), parent: Some(cell), order: shape_key.clone(), components: vec![named("sample"), at(10.0, 10.0, 0.0), s.wrap(), fill([0.1, 0.1, 0.12, 1.0])] });
        }
    }
    let input = TransactionInput { label: format!("stress scene ({shapes} shapes)"), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false };
    // The field's top-left is at (40, 80); the sample sits 10 px into cell 0.
    let ids = StressIds { sample_at: (60.0, 100.0), composition, slider, field, sample: sample.expect("at least one shape"), nodes: total, width, height };
    (input, ids)
}
