//! The clock and the axis (plans/specs/timeline.md phase 1): the playhead is
//! a session node's leaf, bindings read it, drags hold a view's axis fixed.

use iron_document::facet::{Axis, Clip, Size, Timing, Transform2d};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
}

fn bound(e: &str) -> Slot {
    Slot::Bound(Expr::parse(e).unwrap())
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn x_of(store: &Store, id: NodeId) -> f64 {
    store.resolved(id, path("transform2d.x")).and_then(|v| v.as_f64()).unwrap()
}

struct Scene {
    store: Store,
    rect: NodeId,
    clip: NodeId,
    view: NodeId,
}

/// rect (x=50) animated by a clip (start 1, dur 2, x 100 -> 300), and a
/// group with an axis (t0 0, pps 100, label_w 160) standing in for a
/// timeline view.
fn scene() -> Scene {
    let mut store = Store::new();
    let ids = store.document().next_ids(4);
    let (rect, timeline, clip, view) = (ids[0], ids[1], ids[2], ids[3]);
    let rel = store.document().next_relation_id();
    let mut t = Transform2d::default();
    t.x = num(50.0);
    let mut timing = Timing::default();
    timing.start = num(1.0);
    timing.dur = num(2.0);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("transform2d.x".into()));
    c.from = num(100.0);
    c.to = num(300.0);
    let k0 = OrderKey::first();
    let k1 = OrderKey::after(&k0);
    let k2 = OrderKey::after(&k1);
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: rect, ty: ty("rect"), parent: None, order: k0.clone(), facets: vec![t.wrap(), Size::default().wrap()] },
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: k1, facets: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: k0, facets: vec![timing.wrap(), c.wrap()] },
                Op::Link { rel: Relation { id: rel, from: clip, to: rect, rel: RelKind::Animates, data: BTreeMap::new() } },
                Op::Create { id: view, ty: ty("group"), parent: None, order: k2, facets: vec![Axis::default().wrap()] },
            ],
        ))
        .unwrap();
    store.drain_changes();
    Scene { store, rect, clip, view }
}

/// A node in `view` whose x is `expr`.
fn add_bar(store: &mut Store, view: NodeId, expr: &str) -> NodeId {
    let id = store.document().next_ids(1)[0];
    let mut t = Transform2d::default();
    t.x = bound(expr);
    store.apply(tx("bar", vec![Op::Create { id, ty: ty("rect"), parent: Some(view), order: OrderKey::first(), facets: vec![t.wrap(), Size::default().wrap()] }])).unwrap();
    id
}

#[test]
fn a_session_op_on_the_clock_resamples() {
    let Scene { mut store, rect, .. } = scene();
    let clock = store.ensure_clock().unwrap();
    assert!(clock.is_session());
    assert_eq!(store.ensure_clock().unwrap(), clock, "one clock per store");
    store.apply_session(vec![Op::Set { id: clock, path: path("clock.t"), slot: num(2.0) }]).unwrap();
    assert_eq!(store.time(), 2.0);
    assert_eq!(x_of(&store, rect), 200.0);
    // And set_time writes the clock.
    store.set_time(3.0);
    assert_eq!(store.document().leaf(clock, path("clock.t")).unwrap().constant(), Some(&Value::Number(3.0)));
    assert_eq!(x_of(&store, rect), 300.0);
}

#[test]
fn a_binding_reading_the_clock_follows_it() {
    let Scene { mut store, view, .. } = scene();
    let clock = store.ensure_clock().unwrap();
    let head = add_bar(&mut store, view, &format!("{view}.axis.label_w + ({clock}.clock.t - {view}.axis.t0) * {view}.axis.pps"));
    assert_eq!(x_of(&store, head), 160.0);
    store.set_time(1.5);
    assert_eq!(x_of(&store, head), 310.0);
    // Panning the axis moves the playhead line too.
    store.apply(tx("pan", vec![Op::Set { id: view, path: path("axis.t0"), slot: num(1.0) }])).unwrap();
    assert_eq!(x_of(&store, head), 210.0);
}

#[test]
fn a_drag_on_a_bar_edits_the_clip_with_the_axis_held() {
    let Scene { mut store, clip, view, .. } = scene();
    let bar = add_bar(&mut store, view, &format!("{view}.axis.label_w + ({clip}.timing.start - {view}.axis.t0) * {view}.axis.pps"));
    assert_eq!(x_of(&store, bar), 260.0);
    match store.handle(bar, path("transform2d.x")) {
        Handle::Linear { source, a, .. } => {
            assert_eq!(source, Leaf { node: clip, path: path("timing.start") });
            assert_eq!(a, 100.0);
        }
        h => panic!("expected the clip's start as the handle, got {h:?}"),
    }
    store.preview_target(bar, path("transform2d.x"), Value::Number(410.0)).unwrap();
    assert!(store.commit_gesture("retime", Actor::Human, 0).unwrap().is_ok());
    assert_eq!(store.document().leaf(clip, path("timing.start")).unwrap().constant(), Some(&Value::Number(2.5)));
    assert_eq!(store.document().leaf(view, path("axis.pps")).unwrap().constant(), Some(&Value::Number(100.0)), "the axis held");
    store.undo(0).unwrap().unwrap();
    assert_eq!(store.document().leaf(clip, path("timing.start")).unwrap().constant(), Some(&Value::Number(1.0)), "retiming is undoable");
}

#[test]
fn dragging_the_playhead_scrubs_without_history() {
    let Scene { mut store, rect, view, .. } = scene();
    let clock = store.ensure_clock().unwrap();
    let head = add_bar(&mut store, view, &format!("{view}.axis.label_w + ({clock}.clock.t - {view}.axis.t0) * {view}.axis.pps"));
    let past = store.history().past().len();
    store.preview_target(head, path("transform2d.x"), Value::Number(360.0)).unwrap();
    assert_eq!(store.time(), 2.0, "a previewed playhead samples the clips");
    assert_eq!(x_of(&store, rect), 200.0);
    store.cancel_gesture();
    assert_eq!(store.time(), 0.0, "Esc puts the playhead back");
    assert_eq!(x_of(&store, rect), 50.0);
    store.preview_target(head, path("transform2d.x"), Value::Number(360.0)).unwrap();
    assert!(store.commit_gesture("scrub", Actor::Human, 0).is_none(), "no transaction");
    assert_eq!(store.time(), 2.0);
    assert_eq!(store.history().past().len(), past, "scrubbing is not an edit");
}

#[test]
fn the_clock_is_not_saved_or_undone() {
    let Scene { mut store, rect, .. } = scene();
    let past = store.history().past().len();
    store.set_time(2.0);
    store.set_time(2.5);
    assert_eq!(store.history().past().len(), past);
    assert!(!store.save().contains("clock"), "the clock is session state");
    store.undo(0).unwrap().unwrap(); // undoes the scene's build, not the scrub
    assert_eq!(store.time(), 2.5);
    let _ = rect;
}
