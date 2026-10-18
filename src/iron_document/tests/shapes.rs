//! Shapes and paths (select-tool.md, "Shapes and paths"): the ellipse that
//! replaced the circle, schema 3 files with circles, and turning a rect or
//! an ellipse into a path.

use iron_document::facet::{Fill, Radius, Size, Slider, Transform2d};
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

fn at(x: f64, y: f64) -> Facet {
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
    // It saves as the current schema and loads back unchanged.
    let again = Store::load(&store.save()).expect("reloads");
    assert_eq!(again.save(), store.save());
    assert!(store.save().starts_with("{\"schema\": 5"));
}

#[test]
fn a_rect_becomes_a_path_with_the_same_id_and_undo_brings_it_back() {
    let mut store = Store::new();
    let id = store.document().next_ids(1)[0];
    let mut s = Size::default();
    s.w = num(40.0);
    s.h = num(20.0);
    store.apply(tx("draw", vec![Op::Create { id, ty: ty("rect"), parent: None, order: OrderKey::first(), facets: vec![at(10.0, 10.0), s.wrap(), Fill::default().wrap()] }])).unwrap();
    let facets = |s: &Store| s.document().facets_of(id).cloned().collect::<Vec<_>>();
    let before = facets(&store);

    let c = shape::to_path(&store, id).unwrap();
    assert!(c.cuts.is_empty(), "nothing reads or drives it: {:?}", c.cuts);
    store.apply(tx("convert to path", c.ops)).unwrap();
    let doc = store.document();
    assert_eq!(doc.live(id).unwrap().ty.name(), "path");
    assert!(doc.facet(id, FacetKind::Fill).is_some(), "paint carries over");
    assert_eq!(resolved(&store, id, "transform2d.x"), 10.0);
    let [contour] = doc.children(id)[..] else { panic!("one contour") };
    let pts: Vec<(f64, f64)> = doc.children(contour).iter().map(|&p| (resolved(&store, p, "transform2d.x"), resolved(&store, p, "transform2d.y"))).collect();
    assert_eq!(pts, vec![(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (0.0, 20.0)]);
    assert_eq!(store.resolved(contour, path("contour.closed")), Some(Value::Bool(true)));

    store.undo(0).unwrap().unwrap();
    assert_eq!(store.document().live(id).unwrap().ty.name(), "rect");
    assert_eq!(facets(&store), before);
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
                Op::Create { id: slider, ty: ty("slider"), parent: None, order: OrderKey::first(), facets: vec![at(0.0, 0.0), sl.wrap()] },
                Op::Create { id: ellipse, ty: ty("ellipse"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![at(100.0, 100.0), r.wrap()] },
                Op::Create { id: reader, ty: ty("rect"), parent: None, order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![at(0.0, 0.0), s.wrap()] },
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
    let contour = doc.children(ellipse)[0];
    let first = doc.children(contour)[0];
    assert_eq!((resolved(&store, first, "transform2d.x"), resolved(&store, first, "handles.oy")), (30.0, 10.0 * 0.552_284_749_830_793_4));
    assert_eq!(doc.children(contour).len(), 4);
}

#[test]
fn only_rects_and_ellipses_convert() {
    let mut store = Store::new();
    let id = store.document().next_ids(1)[0];
    store.apply(tx("group", vec![Op::Create { id, ty: ty("group"), parent: None, order: OrderKey::first(), facets: vec![] }])).unwrap();
    assert!(!shape::convertible(&store, id));
    assert!(shape::to_path(&store, id).is_err());
}

fn rect_at(store: &mut Store, w: f64, h: f64, extra: Vec<Facet>) -> NodeId {
    let id = store.document().next_ids(1)[0];
    let mut s = Size::default();
    s.w = num(w);
    s.h = num(h);
    let mut facets = vec![at(0.0, 0.0), s.wrap()];
    facets.extend(extra);
    store.apply(tx("draw", vec![Op::Create { id, ty: ty("rect"), parent: None, order: OrderKey::first(), facets }])).unwrap();
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
    use iron_document::facet::Corners;
    let mut store = Store::new();
    let mut c = Corners::default();
    for slot in [&mut c.tl, &mut c.tr, &mut c.br, &mut c.bl] {
        *slot = num(5.0);
    }
    let id = rect_at(&mut store, 40.0, 20.0, vec![c.wrap()]);
    let conv = shape::to_path(&store, id).unwrap();
    store.apply(tx("convert to path", conv.ops)).unwrap();
    let doc = store.document();
    let contour = doc.children(id)[0];
    assert_eq!(doc.children(contour).len(), 8);
    assert!(doc.facet(id, FacetKind::Corners).is_none(), "corners are a rect's parameter, not a path's");
    let second = doc.children(contour)[1];
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
    // A full ring has two outlines: a path of two contours, the hole running
    // the other way, so it stays a ring under the non-zero rule.
    assert_eq!(ellipse_outline(10.0, 10.0, Some([0.0, TAU, 0.5])).len(), 2);
    let mut store = Store::new();
    let id = store.document().next_ids(1)[0];
    let mut arc = iron_document::facet::Arc::default();
    arc.inner = num(0.5);
    store.apply(tx("ring", vec![Op::Create { id, ty: ty("ellipse"), parent: None, order: OrderKey::first(), facets: vec![at(0.0, 0.0), Radius::default().wrap(), arc.wrap()] }])).unwrap();
    let conv = shape::to_path(&store, id).unwrap();
    store.apply(tx("convert to path", conv.ops)).unwrap();
    let contours = contour::contours(&store, id);
    assert_eq!(contours.iter().map(|c| (c.points.len(), c.closed)).collect::<Vec<_>>(), [(4, true), (4, true)]);
    assert_eq!(contour::fill_rule(&store, id), contour::FillRule::NonZero);
    use kurbo::Shape as _;
    let fill = contour::fill_outline(&contours);
    assert_eq!(fill.winding(kurbo::Point::new(0.0, 0.0)), 0, "the hole is empty");
    assert_ne!(fill.winding(kurbo::Point::new(40.0, 0.0)), 0, "the ring is filled");
}

#[test]
fn a_schema_4_path_gets_one_contour_holding_its_points() {
    let text = r##"{"schema": 4, "next_id": 3, "next_rel": 0, "version": 0,
 "nodes": {
  "0": {"order": "a0", "parent": null, "ty": "path"},
  "1": {"order": "a0", "parent": "#0", "ty": "point"},
  "2": {"order": "a1", "parent": "#0", "ty": "point"}
 },
 "components": {
  "path": {"0": {"closed": true}},
  "transform2d": {"0": {}, "1": {"x": 1}, "2": {"x": 2}}
 },
 "relations": [
 ]}"##;
    let store = Store::load(text).expect("loads");
    let doc = store.document();
    let p = "#0".parse::<NodeId>().unwrap();
    let [c] = doc.children(p)[..] else { panic!("one contour") };
    assert_eq!(doc.live(c).unwrap().ty.name(), "contour");
    assert_eq!(c, "#3".parse::<NodeId>().unwrap(), "the next free id");
    assert_eq!(doc.children(c), ["#1".parse::<NodeId>().unwrap(), "#2".parse::<NodeId>().unwrap()]);
    assert_eq!(store.resolved(c, path("contour.closed")), Some(Value::Bool(true)));
    assert_eq!(contour::fill_rule(&store, p), contour::FillRule::NonZero);
    let again = Store::load(&store.save()).expect("reloads");
    assert_eq!(again.save(), store.save());
}

#[test]
fn a_gesture_on_a_missing_facet_adds_it_in_one_undoable_step() {
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
    assert!(store.document().facet(id, FacetKind::Corners).is_none());
}
