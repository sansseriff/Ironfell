//! Quantities in the store (roadmap stage B, ECM §3): the file form, views,
//! bindings over them, and clips interpolating them.

use iron_document::component::{Clip, Timing};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops }
}

fn q(text: &str) -> Value {
    Value::parse_quantity(text).unwrap()
}

/// A timeline with one clip whose endpoints are powers. Clip endpoints are
/// the one `Any` field today, so they are where a quantity can live before
/// `params` arrive.
fn scene() -> (Store, NodeId) {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    let (timeline, clip) = (ids[0], ids[1]);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("timing.start".into()));
    c.from = Slot::Const(q("9.58 mW"));
    c.to = Slot::Const(q("1 W"));
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: OrderKey::first(), components: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: OrderKey::first(), components: vec![Timing::default().wrap(), c.wrap()] },
            ],
        ))
        .unwrap();
    (store, clip)
}

#[test]
fn quantities_round_trip_through_the_file_with_their_unit() {
    let (store, clip) = scene();
    let text = store.save();
    assert!(text.contains(r#""from":{"q":[0.00958,"W"]}"#), "{text}");
    assert!(text.contains(r#""to":{"q":[1.0,"W"]}"#), "{text}");
    let loaded = Store::load(&text).unwrap();
    assert_eq!(loaded.save(), text);
    assert_eq!(loaded.document().leaf(clip, "clip.from".parse().unwrap()).unwrap().constant(), Some(&q("9.58 mW")));
    // Views show the engineering form.
    let v = loaded.view(&ViewQuery { scope: Some(clip), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains(r#"from="9.58 mW" to="1 W""#), "{v}");
}

#[test]
fn bindings_evaluate_quantities_and_report_dimension_errors() {
    let (mut store, clip) = scene();
    let path = |s: &str| -> LeafPath { s.parse().unwrap() };
    // to = from * 2: a quantity binding.
    store
        .apply(tx("bind", vec![Op::Set { id: clip, path: path("clip.to"), slot: Slot::Bound(Expr::parse(&format!("{clip}.clip.from * 2")).unwrap()) }]))
        .unwrap();
    assert_eq!(store.resolved(clip, path("clip.to")), Some(q("19.16 mW")));
    // to = from + 1 nm: a dimension error, reported on the leaf, which keeps
    // its last good value.
    store
        .apply(tx("bad", vec![Op::Set { id: clip, path: path("clip.to"), slot: Slot::Bound(Expr::parse(&format!("{clip}.clip.from + 1 nm")).unwrap()) }]))
        .unwrap();
    let leaf = Leaf { node: clip, path: path("clip.to") };
    let err = store.reactive().errors.get(&leaf).cloned().unwrap_or_default();
    assert!(err.contains("cannot combine"), "{err}");
    assert_eq!(store.resolved(clip, path("clip.to")), Some(q("19.16 mW")));
    // Binding text round-trips through the file in canonical units.
    let text = store.save();
    assert!(text.contains(&format!(r#""to":{{"bind":"{clip}.clip.from + 0.000000001 m"}}"#)), "{text}");
    assert_eq!(Store::load(&text).unwrap().save(), text);
}

#[test]
fn clips_interpolate_quantities() {
    let mut store = Store::new();
    let ids = store.document().next_ids(3);
    let (timeline, driver, target) = (ids[0], ids[1], ids[2]);
    let rel = store.document().next_relation_id();
    // The driver clip animates the target clip's `from` (an Any leaf) from
    // 0 W to 10 W over 10 s.
    let mut dc = Clip::default();
    dc.prop = Slot::Const(Value::Str("clip.from".into()));
    dc.from = Slot::Const(q("0 W"));
    dc.to = Slot::Const(q("10 W"));
    let mut dt = Timing::default();
    dt.dur = Slot::Const(Value::Number(10.0));
    let mut tc = Clip::default();
    tc.prop = Slot::Const(Value::Str("timing.start".into()));
    tc.from = Slot::Const(q("2 W"));
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: OrderKey::first(), components: vec![] },
                Op::Create { id: driver, ty: ty("clip"), parent: Some(timeline), order: OrderKey::first(), components: vec![dt.wrap(), dc.wrap()] },
                Op::Create { id: target, ty: ty("clip"), parent: Some(timeline), order: OrderKey::after(&OrderKey::first()), components: vec![Timing::default().wrap(), tc.wrap()] },
                Op::Link { rel: Relation { id: rel, from: driver, to: target, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    store.set_time(2.5);
    assert_eq!(store.resolved(target, "clip.from".parse().unwrap()), Some(q("2.5 W")));
    assert!(store.time_errors().is_empty(), "{:?}", store.time_errors());
}
