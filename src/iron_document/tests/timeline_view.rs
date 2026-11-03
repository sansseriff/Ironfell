//! The generated timeline composition (plans/specs/timeline.md phase 2).

use iron_document::facet::{Axis, Clip, Name, Size, Timing, Transform2d};
use iron_document::timeline_view;
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


fn node_named(store: &Store, comp: NodeId, key: &str) -> NodeId {
    let doc = store.document();
    *doc.children(comp)
        .iter()
        .find(|&&k| doc.get::<Name>(k).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(key))
        .unwrap_or_else(|| panic!("no {key}"))
}

fn build(store: &mut Store, last: &mut Option<(NodeId, timeline_view::Plan)>) -> NodeId {
    timeline_view::sync(store, last).unwrap();
    timeline_view::find(store).expect("built")
}

#[test]
fn the_timeline_is_built_from_the_clips() {
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    assert!(comp.is_session());
    let seg = node_named(&store, comp, &format!("segment {clip} {rect}"));
    assert_eq!(x_of(&store, seg), 260.0, "label_w + start * pps");
    assert_eq!(store.resolved(seg, path("size.w")).unwrap(), Value::Number(200.0));
    assert!(store.document().relations_from(seg).any(|r| r.rel == RelKind::Projects && r.to == clip), "a segment projects its clip");
    let rows: Vec<String> = store.document().children(comp).iter().filter_map(|&k| store.document().get::<Name>(k).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).map(str::to_owned)).filter(|k| k.starts_with("object") || k.starts_with("property")).collect();
    assert_eq!(rows, vec![format!("object {rect}"), format!("property {rect} transform2d.x")]);
    assert!(timeline_view::sync(&mut store, &mut last).unwrap().is_none(), "nothing changed, nothing regenerated");
    assert!(!store.save().contains("segment"), "the timeline is session state");
}

#[test]
fn dragging_a_segment_retimes_its_clip() {
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let seg = node_named(&store, comp, &format!("segment {clip} {rect}"));
    let h = store.begin_drag(seg, [0.0, 0.0]);
    match &h.mode {
        DragMode::Axes { free, .. } => assert_eq!(*free, [true, false], "segments move only in time"),
        m => panic!("{m:?}"),
    }
    store.drag_to(&h, [410.0, 500.0]);
    store.commit_gesture("retime", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(store.document().leaf(clip, path("timing.start")).unwrap().constant(), Some(&Value::Number(2.5)));
    assert_eq!(x_of(&store, seg), 410.0);
    assert!(timeline_view::sync(&mut store, &mut last).unwrap().is_none(), "retiming does not regenerate");
    store.undo(0).unwrap().unwrap();
    assert_eq!(x_of(&store, seg), 260.0);
}

#[test]
fn the_static_pieces_do_not_move_and_the_playhead_scrubs() {
    let Scene { mut store, rect, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    for key in ["background", "header", "ruler", "tick 1", "tick label 1", &format!("object {rect}")] {
        let id = node_named(&store, comp, key);
        assert!(matches!(store.begin_drag(id, [0.0, 0.0]).mode, DragMode::Locked(_)), "{key} is locked");
    }
    let grip = node_named(&store, comp, "playhead grip");
    let past = store.history().past().len();
    let h = store.begin_drag(grip, [6.0, 0.0]);
    store.drag_to(&h, [360.0, 0.0]);
    assert_eq!(store.time(), 2.0);
    assert_eq!(x_of(&store, rect), 200.0, "the clip samples at the dragged playhead");
    assert!(store.commit_gesture("scrub", Actor::Human, 0).is_none());
    assert_eq!(store.history().past().len(), past, "scrubbing is not an edit");
}

#[test]
fn regeneration_keeps_what_survives() {
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let seg = node_named(&store, comp, &format!("segment {clip} {rect}"));
    // A second clip on the rect, on another property.
    let ids = store.document().next_ids(1);
    let timeline = store.document().live(clip).unwrap().parent.unwrap();
    let mut timing = Timing::default();
    timing.start = num(3.0);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("transform2d.y".into()));
    let rel = store.document().next_relation_id();
    store
        .apply(tx(
            "add clip",
            vec![
                Op::Create { id: ids[0], ty: ty("clip"), parent: Some(timeline), order: OrderKey::after(&OrderKey::first()), facets: vec![timing.wrap(), c.wrap()] },
                Op::Link { rel: Relation { id: rel, from: ids[0], to: rect, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    assert!(timeline_view::sync(&mut store, &mut last).unwrap().is_some());
    assert_eq!(node_named(&store, comp, &format!("segment {clip} {rect}")), seg, "the old segment kept its id");
    let second = node_named(&store, comp, &format!("segment {} {rect}", ids[0]));
    assert_eq!(x_of(&store, second), 460.0);
    // Deleting the clip removes its segment.
    store.apply(tx("delete", vec![Op::Delete { id: ids[0] }])).unwrap();
    timeline_view::sync(&mut store, &mut last).unwrap();
    assert!(!store.document().is_live(second));
    let _ = Axis::default();
}
