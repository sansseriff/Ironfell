//! Shapes and paths (select-tool.md, "Shapes and paths"): the ellipse that
//! replaced the circle, schema 3 files with circles, and turning a rect or
//! an ellipse into a path.

use iron_document::component::{Fill, Radius, Size, Slider, Transform2d};
use iron_document::*;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn resolved(store: &Store, id: NodeId, p: &str) -> f64 {
    store.resolved(id, path(p)).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("{id}.{p} unresolved"))
}

fn at(x: f64, y: f64) -> Component {
    let mut t = Transform2d::default();
    t.x = num(x);
    t.y = num(y);
    t.wrap()
}

#[test]
fn a_schema_3_circle_loads_as_an_ellipse_and_its_readers_follow() {
    // A circle, and a rect whose width reads the circle's radius.
    let text = r##"{"schema": 3, "next_id": 2, "next_rel": 0, "version": 0,
 "nodes": {
  "0": {"order": "a0", "parent": null, "ty": "circle"},
  "1": {"order": "a1", "parent": null, "ty": "rect"}
 },
 "components": {
  "radius": {"0": {"r": 12}},
  "size": {"1": {"h": 10, "w": {"bind": "#0.radius.r * 2"}}},
  "transform2d": {"0": {"x": 5, "y": 6}, "1": {}}
 },
 "relations": [
 ]}"##;
    let store = Store::load(text).expect("loads");
    let doc = store.document();
    let c = "#0".parse::<NodeId>().unwrap();
    assert_eq!(doc.live(c).unwrap().ty.name(), "ellipse");
    assert_eq!(resolved(&store, c, "radius.rx"), 12.0);
    assert_eq!(resolved(&store, c, "radius.ry"), 12.0);
    assert_eq!(resolved(&store, "#1".parse::<NodeId>().unwrap(), "size.w"), 24.0);
    // It saves as schema 4 and loads back unchanged.
    let again = Store::load(&store.save()).expect("reloads");
    assert_eq!(again.save(), store.save());
    assert!(store.save().starts_with("{\"schema\": 4"));
}

#[test]
fn a_rect_becomes_a_path_with_the_same_id_and_undo_brings_it_back() {
    let mut store = Store::new();
    let id = store.document().next_ids(1)[0];
    let mut s = Size::default();
    s.w = num(40.0);
    s.h = num(20.0);
    store.apply(tx("draw", vec![Op::Create { id, ty: ty("rect"), parent: None, order: OrderKey::first(), components: vec![at(10.0, 10.0), s.wrap(), Fill::default().wrap()] }])).unwrap();
    let components = |s: &Store| s.document().components_of(id).cloned().collect::<Vec<_>>();
    let before = components(&store);

    let c = shape::to_path(&store, id).unwrap();
    assert!(c.cuts.is_empty(), "nothing reads or drives it: {:?}", c.cuts);
    store.apply(tx("convert to path", c.ops)).unwrap();
    let doc = store.document();
    assert_eq!(doc.live(id).unwrap().ty.name(), "path");
    assert!(doc.component(id, ComponentKind::Fill).is_some(), "paint carries over");
    assert_eq!(resolved(&store, id, "transform2d.x"), 10.0);
    let pts: Vec<(f64, f64)> = doc.children(id).iter().map(|&p| (resolved(&store, p, "transform2d.x"), resolved(&store, p, "transform2d.y"))).collect();
    assert_eq!(pts, vec![(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (0.0, 20.0)]);
    assert_eq!(store.resolved(id, path("path.closed")), Some(Value::Bool(true)));

    store.undo(0).unwrap().unwrap();
    assert_eq!(store.document().live(id).unwrap().ty.name(), "rect");
    assert_eq!(components(&store), before);
    assert!(store.document().children(id).iter().all(|&p| store.document().live(p).is_none()), "its points went with the undo");
    store.redo(0).unwrap().unwrap();
    assert_eq!(store.document().live(id).unwrap().ty.name(), "path");
}

#[test]
fn converting_an_ellipse_reports_and_bakes_what_reads_or_drives_its_radii() {
    let mut store = Store::new();
    let ids = store.document().next_ids(3);
    let (slider, ellipse, reader) = (ids[0], ids[1], ids[2]);
    let mut sl = Slider::default();
    sl.value = num(30.0);
    let mut r = Radius::default();
    r.rx = Slot::Bound(Expr::parse(&format!("{slider}.slider.value")).unwrap());
    r.ry = num(10.0);
    let mut s = Size::default();
    s.w = Slot::Bound(Expr::parse(&format!("{ellipse}.radius.ry * 3")).unwrap());
    store
        .apply(tx(
            "scene",
            vec![
                Op::Create { id: slider, ty: ty("slider"), parent: None, order: OrderKey::first(), components: vec![at(0.0, 0.0), sl.wrap()] },
                Op::Create { id: ellipse, ty: ty("ellipse"), parent: None, order: OrderKey::after(&OrderKey::first()), components: vec![at(100.0, 100.0), r.wrap()] },
                Op::Create { id: reader, ty: ty("rect"), parent: None, order: OrderKey::after(&OrderKey::after(&OrderKey::first())), components: vec![at(0.0, 0.0), s.wrap()] },
            ],
        ))
        .unwrap();

    let c = shape::to_path(&store, ellipse).unwrap();
    assert_eq!(c.cuts.len(), 2, "{:?}", c.cuts);
    assert!(c.cuts[0].contains("rx is bound"), "{}", c.cuts[0]);
    assert!(c.cuts[1].contains(&format!("{reader}.size.w reads it")), "{}", c.cuts[1]);
    store.apply(tx("convert to path", c.ops)).unwrap();
    // The reader keeps its value as a constant; the outline matches the radii.
    assert_eq!(store.document().leaf(reader, path("size.w")), Some(&num(30.0)));
    let doc = store.document();
    let first = doc.children(ellipse)[0];
    assert_eq!((resolved(&store, first, "transform2d.x"), resolved(&store, first, "handles.oy")), (30.0, 10.0 * 0.552_284_749_830_793_4));
    assert_eq!(doc.children(ellipse).len(), 4);
}

#[test]
fn only_rects_and_ellipses_convert() {
    let mut store = Store::new();
    let id = store.document().next_ids(1)[0];
    store.apply(tx("group", vec![Op::Create { id, ty: ty("group"), parent: None, order: OrderKey::first(), components: vec![] }])).unwrap();
    assert!(!shape::convertible(&store, id));
    assert!(shape::to_path(&store, id).is_err());
}

fn rect_at(store: &mut Store, w: f64, h: f64, extra: Vec<Component>) -> NodeId {
    let id = store.document().next_ids(1)[0];
    let mut s = Size::default();
    s.w = num(w);
    s.h = num(h);
    let mut components = vec![at(0.0, 0.0), s.wrap()];
    components.extend(extra);
    store.apply(tx("draw", vec![Op::Create { id, ty: ty("rect"), parent: None, order: OrderKey::first(), components }])).unwrap();
    id
}

#[test]
fn corner_radii_round_a_rect_and_are_clamped_to_half_the_shorter_side() {
    use iron_document::shape::rect_outline;
    let plain = rect_outline(40.0, 20.0, [0.0; 4]);
    assert_eq!(plain.len(), 4);
    let round = rect_outline(40.0, 20.0, [4.0, 0.0, 50.0, 0.0]);
    // Two anchors per rounded corner; 50 is clamped to 10.
    assert_eq!(round.len(), 6);
    assert_eq!((round[0].at.x, round[0].at.y, round[1].at.x, round[1].at.y), (0.0, 4.0, 4.0, 0.0));
    assert_eq!((round[3].at.x, round[3].at.y, round[4].at.x, round[4].at.y), (40.0, 10.0, 30.0, 20.0));
}

#[test]
fn a_rounded_rect_converts_to_its_eight_point_outline() {
    use iron_document::component::Corners;
    let mut store = Store::new();
    let mut c = Corners::default();
    for slot in [&mut c.tl, &mut c.tr, &mut c.br, &mut c.bl] {
        *slot = num(5.0);
    }
    let id = rect_at(&mut store, 40.0, 20.0, vec![c.wrap()]);
    let conv = shape::to_path(&store, id).unwrap();
    store.apply(tx("convert to path", conv.ops)).unwrap();
    let doc = store.document();
    assert_eq!(doc.children(id).len(), 8);
    assert!(doc.component(id, ComponentKind::Corners).is_none(), "corners are a rect's parameter, not a path's");
    let second = doc.children(id)[1];
    assert_eq!((resolved(&store, second, "transform2d.x"), resolved(&store, second, "handles.ix")), (5.0, -5.0 * 0.552_284_749_830_793_4));
}

#[test]
fn an_arc_is_a_pie_a_ring_segment_or_a_ring() {
    use iron_document::shape::ellipse_outline;
    use std::f64::consts::{PI, TAU};
    // A half pie: the centre, then three anchors over two quarter turns.
    let pie = ellipse_outline(10.0, 10.0, Some([0.0, PI, 0.0]));
    assert_eq!(pie.len(), 1);
    assert_eq!(pie[0].len(), 4);
    assert_eq!((pie[0][0].at.x, pie[0][0].at.y), (0.0, 0.0));
    // A half ring segment: out along the outer edge, back along the inner.
    let segment = ellipse_outline(10.0, 10.0, Some([0.0, PI, 0.5]));
    assert_eq!(segment[0].len(), 6);
    assert!((segment[0][3].at.x + 5.0).abs() < 1e-9);
    // A full ring has two contours, so it cannot become one path.
    assert_eq!(ellipse_outline(10.0, 10.0, Some([0.0, TAU, 0.5])).len(), 2);
    let mut store = Store::new();
    let id = store.document().next_ids(1)[0];
    let mut arc = iron_document::component::Arc::default();
    arc.inner = num(0.5);
    store.apply(tx("ring", vec![Op::Create { id, ty: ty("ellipse"), parent: None, order: OrderKey::first(), components: vec![at(0.0, 0.0), Radius::default().wrap(), arc.wrap()] }])).unwrap();
    assert!(shape::to_path(&store, id).unwrap_err().contains("ring"));
}

#[test]
fn a_gesture_on_a_missing_component_adds_it_in_one_undoable_step() {
    let mut store = Store::new();
    let id = rect_at(&mut store, 40.0, 20.0, vec![]);
    assert!(matches!(store.handle(id, path("corners.tl")), handle::Handle::Direct));
    assert!(matches!(store.handle(id, path("arc.start")), handle::Handle::Locked(_)), "a rect has no arc");
    for c in ["tl", "tr", "br", "bl"] {
        store.preview_target(id, path(&format!("corners.{c}")), Value::Number(6.0)).unwrap();
    }
    assert_eq!(resolved(&store, id, "corners.br"), 6.0, "previewed before it exists");
    let applied = store.commit_gesture("round corners", Actor::Human, 1).unwrap().unwrap();
    let _ = applied;
    assert_eq!(store.document().leaf(id, path("corners.tl")), Some(&num(6.0)));
    store.undo(2).unwrap().unwrap();
    assert!(store.document().component(id, ComponentKind::Corners).is_none());
}
