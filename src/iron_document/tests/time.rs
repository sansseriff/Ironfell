//! Scripted time: clips sampled at the playhead through relations.

use iron_document::component::{Clip, Fill, Size, Timing, Transform2d};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops }
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
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
    bar: NodeId,
    clip: NodeId,
    rel: RelationId,
}

/// rect (x=50); bar.size.h bound to rect.x * 2; timeline > clip(start 1,
/// dur 2, transform2d.x 100 -> 300) animating rect.
fn scene(easing: &str) -> Scene {
    let mut store = Store::new();
    let ids = store.document().next_ids(4);
    let (rect, bar, timeline, clip) = (ids[0], ids[1], ids[2], ids[3]);
    let rel = store.document().next_relation_id();
    let mut t = Transform2d::default();
    t.x = num(50.0);
    let mut s = Size::default();
    s.h = Slot::Bound(Expr::parse(&format!("{rect}.transform2d.x * 2")).unwrap());
    let mut timing = Timing::default();
    timing.start = num(1.0);
    timing.dur = num(2.0);
    timing.easing = Slot::Const(Value::Enum(easing.into()));
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("transform2d.x".into()));
    c.from = num(100.0);
    c.to = num(300.0);
    let k0 = OrderKey::first();
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: rect, ty: ty("rect"), parent: None, order: k0.clone(), components: vec![t.wrap(), Size::default().wrap()] },
                Op::Create {
                    id: bar,
                    ty: ty("bar"),
                    parent: None,
                    order: OrderKey::after(&k0),
                    components: vec![Transform2d::default().wrap(), s.wrap(), Fill::default().wrap()],
                },
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: OrderKey::after(&OrderKey::after(&k0)), components: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: k0, components: vec![timing.wrap(), c.wrap()] },
                Op::Link { rel: Relation { id: rel, from: clip, to: rect, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    store.drain_changes();
    Scene { store, rect, bar, clip, rel }
}

#[test]
fn playhead_samples_clips_and_bindings_follow() {
    let Scene { mut store, rect, bar, .. } = scene("linear");
    assert_eq!(store.time(), 0.0);
    assert_eq!(x_of(&store, rect), 50.0, "before the clip starts the authored value shows");
    store.set_time(1.0);
    assert_eq!(x_of(&store, rect), 100.0);
    store.set_time(2.0);
    assert_eq!(x_of(&store, rect), 200.0);
    let changed: Vec<Leaf> = store.drain_changes().into_iter().map(|c| c.leaf).collect();
    assert!(changed.contains(&Leaf { node: rect, path: path("transform2d.x") }));
    assert!(changed.contains(&Leaf { node: bar, path: path("size.h") }), "the binding followed: {changed:?}");
    assert_eq!(store.resolved(bar, path("size.h")).unwrap(), Value::Number(400.0));
    store.set_time(10.0);
    assert_eq!(x_of(&store, rect), 300.0, "after the end the clip holds `to`");
    store.set_time(0.5);
    assert_eq!(x_of(&store, rect), 50.0);
    // The document never changed.
    assert_eq!(store.document().leaf(rect, path("transform2d.x")).unwrap().constant(), Some(&Value::Number(50.0)));
    assert!(store.time_errors().is_empty(), "{:?}", store.time_errors());
}

#[test]
fn easing_and_offsets_apply() {
    let Scene { mut store, rect, clip, rel, .. } = scene("ease_in");
    store.set_time(2.0); // u = 0.5 -> ease_in 0.25 -> 150
    assert_eq!(x_of(&store, rect), 150.0);
    // A stagger offset on the edge shifts the start (doc 05 §3).
    let mut data = BTreeMap::new();
    data.insert("offset".to_owned(), Value::Number(1.0));
    store
        .apply(tx(
            "stagger",
            vec![
                Op::Unlink { id: rel },
                Op::Link { rel: Relation { id: rel, from: clip, to: rect, rel: RelKind::Animates, data } },
            ],
        ))
        .unwrap();
    assert_eq!(x_of(&store, rect), 100.0, "at t=2 the shifted clip has just started");
    store.set_time(3.0);
    assert_eq!(x_of(&store, rect), 150.0);
}

#[test]
fn transactions_keep_the_sample_current() {
    let Scene { mut store, rect, clip, .. } = scene("linear");
    store.set_time(2.0);
    assert_eq!(x_of(&store, rect), 200.0);
    store.apply(tx("retime", vec![Op::Set { id: clip, path: path("timing.start"), slot: num(0.0) }])).unwrap();
    assert_eq!(x_of(&store, rect), 300.0, "clip now ends at t=2");
    store.apply(tx("delete clip", vec![Op::Delete { id: clip }])).unwrap();
    assert_eq!(x_of(&store, rect), 50.0, "no clip, authored value");
    store.undo(1).unwrap().unwrap();
    assert_eq!(x_of(&store, rect), 300.0);
}

#[test]
fn gesture_preview_wins_over_the_clip() {
    let Scene { mut store, rect, .. } = scene("linear");
    store.set_time(2.0);
    store.preview(rect, path("transform2d.x"), Value::Number(7.0));
    assert_eq!(x_of(&store, rect), 7.0);
    store.cancel_gesture();
    assert_eq!(x_of(&store, rect), 200.0);
}

#[test]
fn bad_clips_are_reported_not_applied() {
    let Scene { mut store, rect, bar, clip, .. } = scene("linear");
    // Retarget onto a bound leaf: reported.
    let rel2 = store.document().next_relation_id();
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("size.h".into()));
    store
        .apply(tx(
            "bad target",
            vec![
                Op::Set { id: clip, path: path("clip.prop"), slot: Slot::Const(Value::Str("size.h".into())) },
                Op::Link { rel: Relation { id: rel2, from: clip, to: bar, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    store.set_time(2.0);
    let err = store.time_errors().get(&clip).cloned().unwrap_or_default();
    assert!(err.contains("bound"), "{err}");
    // Kind mismatch: Vec3 endpoints on a number leaf.
    store
        .apply(tx(
            "kinds",
            vec![
                Op::Set { id: clip, path: path("clip.prop"), slot: Slot::Const(Value::Str("transform2d.x".into())) },
                Op::Set { id: clip, path: path("clip.from"), slot: Slot::Const(Value::Vec3([0.0; 3])) },
                Op::Set { id: clip, path: path("clip.to"), slot: Slot::Const(Value::Vec3([1.0; 3])) },
            ],
        ))
        .unwrap();
    let err = store.time_errors().get(&clip).cloned().unwrap_or_default();
    assert!(err.contains("vec3"), "{err}");
    assert_eq!(x_of(&store, rect), 50.0);
}

#[test]
fn views_answer_why_is_this_moving() {
    let Scene { store, rect, clip, .. } = scene("linear");
    let v = store.view(&ViewQuery { scope: Some(rect), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains(&format!("<driven-by ref=\"{clip}\" rel=\"animates\" prop=\"transform2d.x\"/>")), "{v}");
    let v = store.view(&ViewQuery { scope: Some(clip), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains(&format!("<animates to=\"{rect}\"/>")), "{v}");
    assert!(v.contains("from=\"100\" to=\"300\""), "{v}");
    assert_eq!(store.document().relations_to(rect).count(), 1);
}
