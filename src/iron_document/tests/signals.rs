//! The runtime signal layer (vision 10 §4): host producers publish values
//! beneath bindings and above constants, conflicts are reported, and a
//! freeze commits what was published.

use iron_document::facet::{Clip, Fill, Size, Slider, Timing, Transform2d};
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

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn leaf(node: NodeId, p: &str) -> Leaf {
    Leaf { node, path: path(p) }
}

fn f64_of(store: &Store, id: NodeId, p: &str) -> f64 {
    store.resolved(id, path(p)).and_then(|v| v.as_f64()).unwrap()
}

struct Scene {
    store: Store,
    gauge: NodeId,
    bar: NodeId,
    rect: NodeId,
}

/// gauge (slider, value 0.25); bar.size.w bound to gauge.value * 200;
/// rect (x=50) animated by a clip (x 100 -> 300 over 1..3 s).
fn scene() -> Scene {
    let mut store = Store::new();
    let ids = store.document().next_ids(5);
    let (gauge, bar, rect, timeline, clip) = (ids[0], ids[1], ids[2], ids[3], ids[4]);
    let rel = store.document().next_relation_id();
    let mut sl = Slider::default();
    sl.value = num(0.25);
    let mut s = Size::default();
    s.w = Slot::Bound(Expr::parse(&format!("{gauge}.slider.value * 200")).unwrap());
    let mut t = Transform2d::default();
    t.x = num(50.0);
    let mut timing = Timing::default();
    timing.start = num(1.0);
    timing.dur = num(2.0);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("transform2d.x".into()));
    c.from = num(100.0);
    c.to = num(300.0);
    let mut k = OrderKey::first();
    let mut next = || {
        let out = k.clone();
        k = OrderKey::after(&k);
        out
    };
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: gauge, ty: ty("slider"), parent: None, order: next(), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), sl.wrap()] },
                Op::Create { id: bar, ty: ty("rect"), parent: None, order: next(), facets: vec![Transform2d::default().wrap(), s.wrap(), Fill::default().wrap()] },
                Op::Create { id: rect, ty: ty("rect"), parent: None, order: next(), facets: vec![t.wrap(), Size::default().wrap()] },
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: next(), facets: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: OrderKey::first(), facets: vec![timing.wrap(), c.wrap()] },
                Op::Link { rel: Relation { id: rel, from: clip, to: rect, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    store.drain_changes();
    Scene { store, gauge, bar, rect }
}

#[test]
fn a_published_signal_reads_beneath_bindings_and_above_constants() {
    let Scene { mut store, gauge, bar, .. } = scene();
    let gas = store.register_producer("gas");
    assert_eq!(gas, ProducerId::FIRST_HOST);
    assert_eq!(store.producer_name(gas), "gas");
    assert_eq!(f64_of(&store, bar, "size.w"), 50.0);

    store.publish(gas, BTreeMap::from([(leaf(gauge, "slider.value"), Value::Number(0.5))]));
    assert_eq!(f64_of(&store, gauge, "slider.value"), 0.5);
    assert_eq!(f64_of(&store, bar, "size.w"), 100.0, "the binding re-evaluated");
    let changed: Vec<Leaf> = store.drain_changes().into_iter().map(|c| c.leaf).collect();
    assert_eq!(changed, vec![leaf(gauge, "slider.value"), leaf(bar, "size.w")]);
    assert!(store.signal_errors().is_empty());
    let signal = store.signals()[&leaf(gauge, "slider.value")].clone();
    assert_eq!((signal.producer, signal.value), (gas, Value::Number(0.5)));

    // The document never saw it.
    assert_eq!(store.document().leaf(gauge, path("slider.value")).unwrap().constant(), Some(&Value::Number(0.25)));
    assert_eq!(store.history().log().count(), 1);

    // Republishing the same value is silent; a moved value reports once.
    store.publish(gas, BTreeMap::from([(leaf(gauge, "slider.value"), Value::Number(0.5))]));
    assert!(!store.has_changes());
    store.publish(gas, BTreeMap::from([(leaf(gauge, "slider.value"), Value::Number(0.75))]));
    assert_eq!(f64_of(&store, bar, "size.w"), 150.0);
    assert_eq!(store.drain_changes().len(), 2);

    // A driven leaf is locked to gestures, naming the producer.
    assert!(matches!(store.handle(gauge, path("slider.value")), Handle::Locked(reason) if reason.contains("producer 2")));

    // Releasing it reads as authored again.
    store.unpublish(gas);
    assert_eq!(f64_of(&store, bar, "size.w"), 50.0);
    assert!(store.signals().is_empty());
    assert!(matches!(store.handle(gauge, path("slider.value")), Handle::Direct));
}

#[test]
fn two_producers_on_one_leaf_is_an_error_the_first_claim_wins() {
    let Scene { mut store, rect, gauge, .. } = scene();
    let gas = store.register_producer("gas");
    store.set_time(2.0);
    assert_eq!(f64_of(&store, rect, "transform2d.x"), 200.0, "the clip drives x");
    store.drain_changes();

    store.publish(gas, BTreeMap::from([(leaf(rect, "transform2d.x"), Value::Number(0.0)), (leaf(gauge, "slider.value"), Value::Number(1.0))]));
    assert_eq!(f64_of(&store, rect, "transform2d.x"), 200.0, "the clip keeps it");
    assert_eq!(f64_of(&store, gauge, "slider.value"), 1.0, "the free leaf was taken");
    let err = &store.signal_errors()[&leaf(rect, "transform2d.x")];
    assert!(err.contains("already driven by producer 0"), "{err}");

    // Once the clip releases the leaf, the next publish takes it.
    store.set_time(0.0);
    assert_eq!(f64_of(&store, rect, "transform2d.x"), 50.0);
    store.publish(gas, BTreeMap::from([(leaf(rect, "transform2d.x"), Value::Number(0.0))]));
    assert_eq!(f64_of(&store, rect, "transform2d.x"), 0.0);
    assert!(store.signal_errors().is_empty());
    // And the gauge, no longer named, was released.
    assert_eq!(f64_of(&store, gauge, "slider.value"), 0.25);
}

#[test]
fn a_bound_leaf_cannot_be_driven() {
    let Scene { mut store, bar, .. } = scene();
    let gas = store.register_producer("gas");
    store.publish(gas, BTreeMap::from([(leaf(bar, "size.w"), Value::Number(1.0))]));
    assert_eq!(f64_of(&store, bar, "size.w"), 50.0);
    assert!(store.signal_errors()[&leaf(bar, "size.w")].contains("is bound"));
    assert!(store.signals().is_empty());
}

#[test]
fn a_freeze_commits_the_published_values_as_one_transaction() {
    let Scene { mut store, gauge, bar, .. } = scene();
    let gas = store.register_producer("gas");
    assert!(store.freeze(gas, "freeze".into(), Actor::Human, 1).is_none(), "nothing to freeze");
    store.publish(gas, BTreeMap::from([(leaf(gauge, "slider.value"), Value::Number(0.6))]));
    store.drain_changes();

    let applied = store.freeze(gas, "freeze gas".into(), Actor::Human, 1).unwrap().unwrap();
    assert_eq!(applied.version, Version(2));
    assert!(store.signals().is_empty(), "the producer holds nothing after a freeze");
    assert_eq!(store.document().leaf(gauge, path("slider.value")).unwrap().constant(), Some(&Value::Number(0.6)));
    assert_eq!(f64_of(&store, bar, "size.w"), 120.0);
    // The release reports the gauge (as a resolved change) and the set
    // reaches readers through the transaction, like any authored change;
    // either way they rewrite from the resolved value, which is 0.6.
    let changes = store.drain_changes();
    assert!(changes.iter().any(|c| c.leaf == leaf(gauge, "slider.value")));
    assert_eq!(f64_of(&store, gauge, "slider.value"), 0.6);
    let last = store.history().log().last().unwrap();
    assert_eq!((last.label.as_str(), last.ops.len()), ("freeze gas", 1));

    // Undo restores the authored value; the producer stays released.
    store.undo(2).unwrap().unwrap();
    assert_eq!(f64_of(&store, bar, "size.w"), 50.0);
}
