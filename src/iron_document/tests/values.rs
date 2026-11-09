//! Quantities in the store (roadmap stage B, ECM §3): the file form, views,
//! bindings over them, and clips interpolating them.

use iron_document::facet::{Clip, Key, Timing};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn q(text: &str) -> Value {
    Value::parse_quantity(text).unwrap()
}

/// A timeline with one clip whose two keys hold powers. A key's `value` is
/// an `Any` field, so it is where a quantity can live before `params` arrive.
fn scene() -> (Store, NodeId, NodeId) {
    let mut store = Store::new();
    let ids = store.document().next_ids(4);
    let (timeline, clip, a, b) = (ids[0], ids[1], ids[2], ids[3]);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("timing.start".into()));
    let key = |t: f64, v: Value| {
        let mut k = Key::default();
        k.t = Slot::Const(Value::Number(t));
        k.value = Slot::Const(v);
        k.wrap()
    };
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: OrderKey::first(), facets: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: OrderKey::first(), facets: vec![Timing::default().wrap(), c.wrap()] },
                Op::Create { id: a, ty: ty("key"), parent: Some(clip), order: OrderKey::first(), facets: vec![key(0.0, q("9.58 mW"))] },
                Op::Create { id: b, ty: ty("key"), parent: Some(clip), order: OrderKey::after(&OrderKey::first()), facets: vec![key(1.0, q("1 W"))] },
            ],
        ))
        .unwrap();
    (store, a, b)
}

#[test]
fn quantities_round_trip_through_the_file_with_their_unit() {
    let (store, a, _) = scene();
    let text = store.save();
    assert!(text.contains(r#""value":{"q":[0.00958,"W"]}"#), "{text}");
    assert!(text.contains(r#""value":{"q":[1.0,"W"]}"#), "{text}");
    let loaded = Store::load(&text).unwrap();
    assert_eq!(loaded.save(), text);
    assert_eq!(loaded.document().leaf(a, "key.value".parse().unwrap()).unwrap().constant(), Some(&q("9.58 mW")));
    // Views show the engineering form.
    let v = loaded.view(&ViewQuery { omit_defaults: false, expand: Vec::new(), relation: Default::default(), scope: Some(a), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains(r#"value="9.58 mW""#), "{v}");
}

#[test]
fn bindings_evaluate_quantities_and_report_dimension_errors() {
    let (mut store, a, b) = scene();
    let path = |s: &str| -> LeafPath { s.parse().unwrap() };
    // b = a * 2: a quantity binding.
    store
        .apply(tx("bind", vec![Op::Set { id: b, path: path("key.value"), slot: Slot::Bound(Expr::parse(&format!("{a}.key.value * 2")).unwrap()) }]))
        .unwrap();
    assert_eq!(store.resolved(b, path("key.value")), Some(q("19.16 mW")));
    // to = from + 1 nm: a dimension error, reported on the leaf, which keeps
    // its last good value.
    store
        .apply(tx("bad", vec![Op::Set { id: b, path: path("key.value"), slot: Slot::Bound(Expr::parse(&format!("{a}.key.value + 1 nm")).unwrap()) }]))
        .unwrap();
    let leaf = Leaf { node: b, path: path("key.value") };
    let err = store.reactive().errors.get(&leaf).cloned().unwrap_or_default();
    assert!(err.contains("cannot combine"), "{err}");
    assert_eq!(store.resolved(b, path("key.value")), Some(q("19.16 mW")));
    // Binding text round-trips through the file in canonical units.
    let text = store.save();
    assert!(text.contains(&format!(r#""value":{{"bind":"{a}.key.value + 0.000000001 m"}}"#)), "{text}");
    assert_eq!(Store::load(&text).unwrap().save(), text);
}

#[test]
fn clips_interpolate_quantities() {
    let mut store = Store::new();
    let ids = store.document().next_ids(7);
    let (timeline, driver, d0, d1, holder, h0, target) = (ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], ids[6]);
    let rel = store.document().next_relation_id();
    let key = |t: f64, v: Value| {
        let mut k = Key::default();
        k.t = Slot::Const(Value::Number(t));
        k.value = Slot::Const(v);
        k.wrap()
    };
    // The driver clip animates another clip's key's `value` (an Any leaf)
    // from 0 W at 0 s to 10 W at 10 s.
    let mut dc = Clip::default();
    dc.prop = Slot::Const(Value::Str("key.value".into()));
    let mut hc = Clip::default();
    hc.prop = Slot::Const(Value::Str("timing.start".into()));
    let k0 = OrderKey::first();
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: k0.clone(), facets: vec![] },
                Op::Create { id: driver, ty: ty("clip"), parent: Some(timeline), order: k0.clone(), facets: vec![Timing::default().wrap(), dc.wrap()] },
                Op::Create { id: d0, ty: ty("key"), parent: Some(driver), order: k0.clone(), facets: vec![key(0.0, q("0 W"))] },
                Op::Create { id: d1, ty: ty("key"), parent: Some(driver), order: OrderKey::after(&k0), facets: vec![key(10.0, q("10 W"))] },
                Op::Create { id: holder, ty: ty("clip"), parent: Some(timeline), order: OrderKey::after(&k0), facets: vec![Timing::default().wrap(), hc.wrap()] },
                Op::Create { id: h0, ty: ty("key"), parent: Some(holder), order: k0.clone(), facets: vec![key(0.0, q("2 W"))] },
                Op::Create { id: target, ty: ty("key"), parent: Some(holder), order: OrderKey::after(&k0), facets: vec![key(1.0, q("2 W"))] },
                Op::Link { rel: Relation { id: rel, from: driver, to: target, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    store.set_time(2.5);
    assert_eq!(store.resolved(target, "key.value".parse().unwrap()), Some(q("2.5 W")));
    let _ = (d0, d1, h0);
}

#[test]
fn numbers_needing_every_digit_survive_save_and_load() {
    // 620 / 340 prints as 1.8235294117647058; a parser that is not
    // correctly rounded reads it back as …056 (serde_json without
    // `float_roundtrip`), and the second save differs from the first.
    let mut store = Store::new();
    let id = store.document().next_ids(1)[0];
    let mut lens = iron_document::facet::Camera::default();
    lens.aspect = Slot::Const(Value::Number(620.0 / 340.0));
    store.apply(tx("camera", vec![Op::Create { id, ty: ty("camera"), parent: None, order: OrderKey::first(), facets: vec![iron_document::facet::Transform3d::default().wrap(), lens.wrap()] }])).unwrap();
    let text = store.save();
    assert!(text.contains("1.8235294117647058"), "{text}");
    assert_eq!(Store::load(&text).unwrap().save(), text);
}
