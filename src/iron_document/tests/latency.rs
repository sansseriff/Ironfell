//! The Latency view's session composition (plans/rendering/latency-view.md).
use iron_document::facet::Name;
use iron_document::{Actor, OrderKey, Store, composition, standard};

fn build() -> (Store, iron_document::NodeId) {
    let mut store = Store::new();
    let first = store.document().next_session_ids(1)[0];
    store.apply_session(standard::latency_ops(first, OrderKey::first())).unwrap();
    let doc = store.document();
    let comp = composition::compositions(doc).into_iter().find(|&c| doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(standard::LATENCY)).unwrap();
    let target = doc.children(comp)[0];
    (store, target)
}

#[test]
fn the_square_drags_as_session_state_never_in_history() {
    let (mut store, target) = build();
    assert!(target.is_session());
    let x = |s: &Store| s.resolved(target, "transform2d.x".parse().unwrap()).and_then(|v| v.as_f64()).unwrap();
    let before = x(&store);
    let version = store.document().version();
    let h = store.begin_drag(target, [20.0, 20.0]);
    assert!(!h.is_locked(), "the square is draggable: {h:?}");
    store.drag_to(&h, [before + 20.0 + 100.0, 220.0]);
    assert_eq!(x(&store), before + 100.0, "previewed");
    let changes = store.drain_changes();
    assert!(changes.iter().any(|c| c.leaf.node == target), "the preview is reported for redraw");
    assert!(store.commit_gesture("move", Actor::Human, 0).is_none(), "no transaction");
    assert_eq!(x(&store), before + 100.0, "kept after release");
    assert!(store.history().log().all(|tx| tx.version <= version), "nothing in history");
}

#[test]
fn recentring_reports_the_move_for_redraw_and_picking() {
    let (mut store, target) = build();
    let _ = store.drain_changes();
    let _ = store.take_geometry_changes();
    let set = |path: &str, n: f64| iron_document::Op::Set { id: target, path: path.parse().unwrap(), slot: iron_document::Slot::Const(iron_document::Value::Number(n)) };
    store.apply_session(vec![set("transform2d.x", 500.0), set("transform2d.y", 300.0)]).unwrap();
    assert!(store.drain_changes().iter().any(|c| c.leaf.node == target), "redrawn");
    assert!(!store.take_geometry_changes().is_empty(), "re-indexed for picking");
}

#[test]
fn building_it_reaches_the_spatial_scene() {
    let mut store = Store::new();
    let _ = store.take_geometry_changes();
    let first = store.document().next_session_ids(1)[0];
    store.apply_session(standard::latency_ops(first, OrderKey::first())).unwrap();
    assert!(store.take_geometry_changes().structure, "a session composition's creation is a structure change");
}
