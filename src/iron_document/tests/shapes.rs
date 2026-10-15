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
