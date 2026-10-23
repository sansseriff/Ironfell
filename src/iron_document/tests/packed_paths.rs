//! Packed paths (plans/specs/packed-paths.md): a path's outlines as one
//! `path.d` value, read like an expanded path, kept in files exactly, and
//! expanded into point nodes on demand.

use iron_document::facet::{PathShape, Transform2d};
use iron_document::geometry::{Geometry, PackedContour};
use iron_document::shape::Anchor;
use iron_document::*;
use kurbo::{Point, Vec2};
use std::sync::Arc;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn anchor(x: f64, y: f64, ix: f64, iy: f64, ox: f64, oy: f64) -> Anchor {
    Anchor { at: Point::new(x, y), inn: Vec2::new(ix, iy), out: Vec2::new(ox, oy) }
}

/// A square with a curved side, and an open stroke with end handles.
fn sample() -> Geometry {
    Geometry::new(vec![
        PackedContour { closed: true, anchors: vec![anchor(0.0, 0.0, 0.0, 0.0, 0.0, 0.0), anchor(10.0, 0.0, 0.0, 0.0, 2.5, 0.1), anchor(10.0, 10.0, 0.3, -2.0, 0.0, 0.0), anchor(0.0, 10.0, 0.0, 0.0, 0.0, 0.0)] },
        PackedContour { closed: false, anchors: vec![anchor(20.0, 0.1, -1.0, 0.0, 1.0, 0.0), anchor(30.0, 5.0, 0.2, 0.2, 0.7, 0.0)] },
    ])
}

fn packed_facets(g: Geometry) -> Vec<Facet> {
    let mut p = PathShape::default();
    p.d = Slot::Const(Value::Geometry(Arc::new(g)));
    vec![Transform2d::default().wrap(), p.wrap()]
}

/// A store with one packed path of `sample()`.
fn with_packed() -> (Store, NodeId) {
    let mut store = Store::new();
    let id = store.document().next_ids(1)[0];
    store.apply(tx("draw", vec![Op::Create { id, ty: ty("path"), parent: None, order: OrderKey::first(), facets: packed_facets(sample()) }])).unwrap();
    (store, id)
}

#[test]
fn a_packed_path_reads_as_its_expanded_twin() {
    let (store, packed) = with_packed();
    // The twin, built the expanded way.
    let mut store2 = Store::new();
    let twin = store2.document().next_ids(1)[0];
    let runs: Vec<(Vec<Anchor>, bool)> = sample().contours().iter().map(|c| (c.anchors.clone(), c.closed)).collect();
    let mut ids = store2.document().next_ids(1 + contour::ids_needed(&runs)).into_iter().skip(1);
    let mut ops = vec![Op::Create { id: twin, ty: ty("path"), parent: None, order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), PathShape::default().wrap()] }];
    ops.extend(contour::contours_ops(twin, &runs, &mut ids));
    store2.apply(tx("draw", ops)).unwrap();

    assert_eq!(*contour::geometry(&store, packed), *contour::geometry(&store2, twin));
    assert_eq!(contour::geometry(&store, packed).outline(), contour::geometry(&store2, twin).outline());
    assert_eq!(store.local_bounds(packed), store2.local_bounds(twin));
    assert!(contour::contours(&store, packed).is_empty(), "no point nodes");
    assert_eq!(store.document().children(packed).len(), 0);
    // The shared value is handed out, not rebuilt.
    assert!(Arc::ptr_eq(&contour::geometry(&store, packed), &contour::geometry(&store, packed)));
}

#[test]
fn a_packed_path_saves_and_loads_exactly() {
    let (store, id) = with_packed();
    let text = store.save();
    assert!(text.contains("\"geo\""), "{text}");
    let back = Store::load(&text).unwrap();
    assert_eq!(*contour::geometry(&back, id), sample());
    assert_eq!(back.save(), text);
}

#[test]
fn the_view_summarises_a_packed_path() {
    let (store, _) = with_packed();
    let q = ViewQuery { relation: ViewRelation::Tree, scope: None, fidelity: Fidelity::Full, depth: None, expand: Vec::new() };
    let view = store.view(&q).unwrap();
    assert!(view.contains(r#"d="2 contours, 6 anchors""#), "{view}");
}

#[test]
fn path_d_cannot_be_bound() {
    let (mut store, id) = with_packed();
    let bound = Slot::Bound(Expr::parse("1 + 1").unwrap());
    let err = store.apply(tx("bind", vec![Op::Set { id, path: path("path.d"), slot: bound.clone() }])).unwrap_err();
    assert_eq!(err[0].kind, ErrorKind::TypeMismatch, "{err:?}");
    // Nor on a created path.
    let id2 = store.document().next_ids(1)[0];
    let mut p = PathShape::default();
    p.d = bound;
    let err = store.apply(tx("draw", vec![Op::Create { id: id2, ty: ty("path"), parent: None, order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), p.wrap()] }])).unwrap_err();
    assert_eq!(err[0].kind, ErrorKind::TypeMismatch, "{err:?}");
}

#[test]
fn a_path_is_packed_or_expanded_never_both() {
    let (mut store, id) = with_packed();
    let ids = store.document().next_ids(3);
    let ops = contour::contour_ops(id, OrderKey::first(), &[anchor(0.0, 0.0, 0.0, 0.0, 0.0, 0.0), anchor(5.0, 5.0, 0.0, 0.0, 0.0, 0.0)], false, &mut ids.into_iter());
    let err = store.apply(tx("add contour", ops)).unwrap_err();
    assert_eq!(err[0].kind, ErrorKind::MixedPath, "{err:?}");
}

#[test]
fn expanding_is_one_undoable_transaction() {
    let (mut store, id) = with_packed();
    store.apply(tx("edit points", contour::expand_points(&store, id))).unwrap();
    assert!(contour::packed(store.document(), id).is_none());
    let cs = contour::contours(&store, id);
    assert_eq!(cs.iter().map(|c| (c.points.len(), c.closed)).collect::<Vec<_>>(), [(4, true), (2, false)]);
    assert_eq!(*contour::geometry(&store, id), sample(), "the same anchors, as nodes");
    // Expanding an expanded path does nothing.
    assert!(contour::expand_points(&store, id).is_empty());

    // A point edit, then undo both.
    let p = cs[0].points[1].0;
    store.apply(tx("move point", vec![Op::Set { id: p, path: path("transform2d.x"), slot: Slot::Const(Value::Number(12.0)) }])).unwrap();
    store.undo(1).unwrap().unwrap();
    store.undo(2).unwrap().unwrap();
    assert_eq!(*contour::geometry(&store, id), sample());
    assert!(contour::packed(store.document(), id).is_some());
    assert!(contour::contours(&store, id).is_empty(), "the point nodes are tombstones again");
    store.redo(3).unwrap().unwrap();
    assert_eq!(contour::contours(&store, id).len(), 2);
}

#[test]
fn setting_path_d_undoes() {
    let (mut store, id) = with_packed();
    let moved = sample().map_anchors(|a| Anchor { at: a.at + Vec2::new(1.0, 2.0), ..a });
    store.apply(tx("resize", vec![Op::Set { id, path: path("path.d"), slot: Slot::Const(Value::Geometry(Arc::new(moved.clone()))) }])).unwrap();
    assert_eq!(*contour::geometry(&store, id), moved);
    store.undo(1).unwrap().unwrap();
    assert_eq!(*contour::geometry(&store, id), sample());
    store.redo(2).unwrap().unwrap();
    assert_eq!(*contour::geometry(&store, id), moved);
}
