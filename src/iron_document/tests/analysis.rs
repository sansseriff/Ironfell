//! Render analysis (instance-cache.md phase A, component-model.md §11): the
//! dial meter's face is static, its needle and readout depend on `in`; a
//! connection makes them live while the face stays static; an override keys
//! the part it touches.

use iron_document::analysis::{self, Channel, Class, Input};
use iron_document::facet::{Composition, Name, Override};
use iron_document::*;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn desk() -> (Store, NodeId) {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str("bench".into()));
    store
        .apply(TransactionInput { label: "desk".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops: vec![
            Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![n.wrap(), c.wrap()] },
            Op::Create { id: ids[1], ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
        ], exact: false })
        .unwrap();
    (store, ids[0])
}

fn name(doc: &Document, n: NodeId) -> String {
    doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()).unwrap_or("").to_owned()
}

/// Each part by name: (class, shape, placement).
fn classes(store: &Store, u: NodeId) -> Vec<(String, Class, Class, Class)> {
    let doc = store.document();
    let a = analysis::classify(doc, store.reactive(), u).expect("an instance");
    a.parts.iter().map(|p| (name(doc, p.node), p.class(), p.classes[&Channel::Shape], p.classes[&Channel::Placement])).collect()
}

fn part<'a>(c: &'a [(String, Class, Class, Class)], n: &str) -> &'a (String, Class, Class, Class) {
    c.iter().find(|p| p.0 == n).unwrap_or_else(|| panic!("no part {n} in {c:?}"))
}

#[test]
fn a_dial_meters_face_is_static_and_its_needle_depends_on_in() {
    let (mut store, comp) = desk();
    let u = store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let c = classes(&store, u);
    assert_eq!(part(&c, "dial").1, Class::Static);
    // The pivot's rotation reads `in` and `range`, constants here.
    let needle = part(&c, "needle");
    assert_eq!((needle.1, needle.2, needle.3), (Class::Keyed, Class::Static, Class::Keyed));
    assert_eq!(part(&c, "readout").2, Class::Keyed);
    let doc = store.document();
    let a = analysis::classify(doc, store.reactive(), u).unwrap();
    let n = a.parts.iter().find(|p| name(doc, p.node) == "needle").unwrap();
    assert_eq!(n.key.keys().cloned().collect::<Vec<_>>(), ["in", "range"]);
    assert!(n.reasons.iter().any(|r| r == "placement depends on `in`, `range`"), "{:?}", n.reasons);
    assert!(matches!(a.inputs["in"], Input::Constant(_)));
    // The face runs alone; the needle and readout share a keyed run.
    let runs: Vec<(Class, Vec<String>)> = analysis::runs(&a).into_iter().map(|r| (r.class, r.parts.iter().map(|&p| name(doc, p)).collect())).collect();
    assert_eq!(runs, [(Class::Static, vec!["dial".to_owned()]), (Class::Keyed, vec!["needle".to_owned(), "readout".to_owned()])]);
}

#[test]
fn connecting_in_makes_the_needle_live_and_leaves_the_face_static() {
    let (mut store, comp) = desk();
    let laser = store.place_standard("laser", comp, [0.0, 0.0], Actor::Human).unwrap();
    let meter = store.place_standard("meter", comp, [300.0, 0.0], Actor::Human).unwrap();
    let doc = store.document();
    let port = |u: NodeId, n: &str| {
        let e = expand::use_entity(doc, u).unwrap();
        doc.children(e).iter().copied().find(|&p| name(doc, p) == n).unwrap()
    };
    let (from, to) = (port(laser, "out"), port(meter, "in"));
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let ops = author::connect(store.document(), &read, from, to).unwrap();
    store.apply(TransactionInput { label: "connect".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }).unwrap();
    let c = classes(&store, meter);
    assert_eq!(part(&c, "dial").1, Class::Static);
    let needle = part(&c, "needle");
    assert_eq!((needle.1, needle.2, needle.3), (Class::Live, Class::Static, Class::Live));
    assert_eq!(part(&c, "readout").1, Class::Live);
    let doc = store.document();
    let a = analysis::classify(doc, store.reactive(), meter).unwrap();
    assert_eq!(a.inputs["in"], Input::Wired("connected".into()));
    let n = a.parts.iter().find(|p| name(doc, p.node) == "needle").unwrap();
    assert!(n.movable(), "baked once and turned");
    assert!(n.reasons.iter().any(|r| r == "placement depends on `in`, connected"), "{:?}", n.reasons);
    // The laser's own picture reads nothing that changes.
    assert!(classes(&store, laser).iter().all(|p| p.1 != Class::Live), "{:?}", classes(&store, laser));
}

#[test]
fn an_override_keys_the_part_it_touches() {
    let (mut store, comp) = desk();
    let u = store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let doc = store.document();
    let a = analysis::classify(doc, store.reactive(), u).unwrap();
    let dial = a.parts.iter().find(|p| name(doc, p.node) == "dial").unwrap().node;
    let mut o = Override::default();
    o.target = Slot::Const(Value::Ref(dial));
    o.leaf = Slot::Const(Value::Str("fill.color".into()));
    o.value = Slot::Const(Value::Color([0.2, 0.2, 0.2, 1.0]));
    let id = doc.next_id();
    store
        .apply(TransactionInput { label: "override".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops: vec![Op::Create { id, ty: ty("override"), parent: Some(u), order: OrderKey::first(), facets: vec![o.wrap()] }], exact: false })
        .unwrap();
    let doc = store.document();
    let a = analysis::classify(doc, store.reactive(), u).unwrap();
    let d = a.parts.iter().find(|p| p.node == dial).unwrap();
    assert_eq!((d.class(), d.classes[&Channel::Shape]), (Class::Keyed, Class::Keyed));
    assert_eq!(d.key.len(), 1);
    assert!(d.reasons.iter().any(|r| r == "shape overridden (fill.color)"), "{:?}", d.reasons);
}

#[test]
fn every_standard_part_classifies_without_opaque_reads() {
    let (mut store, comp) = desk();
    for e in standard::ENTRIES {
        let u = store.place_standard(e.key, comp, [0.0, 0.0], Actor::Human).unwrap();
        let doc = store.document();
        let a = analysis::classify(doc, store.reactive(), u).unwrap_or_else(|| panic!("{}: not an instance", e.key));
        assert!(!a.parts.is_empty(), "{}", e.key);
        for p in &a.parts {
            assert!(p.class() != Class::Live, "{} {}: {:?}", e.key, name(doc, p.node), p.reasons);
        }
    }
}

#[test]
fn a_plugged_fiber_follows_its_partner_live() {
    let (mut store, comp) = desk();
    let laser = store.place_standard("laser", comp, [0.0, 0.0], Actor::Human).unwrap();
    let fiber = store.place_standard("fiber", comp, [200.0, 0.0], Actor::Human).unwrap();
    let before = classes(&store, fiber);
    assert!(before.iter().all(|p| p.1 != Class::Live), "{before:?}");
    let doc = store.document();
    let port = |u: NodeId, n: &str| {
        let e = expand::use_entity(doc, u).unwrap();
        doc.children(e).iter().copied().find(|&p| name(doc, p) == n).unwrap()
    };
    let (from, to) = (port(laser, "out"), port(fiber, "in"));
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let ops = author::connect(store.document(), &read, from, to).unwrap();
    store.apply(TransactionInput { label: "connect".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }).unwrap();
    // Its `in` end sits on the laser's anchor, so the curve reads outside.
    let after = classes(&store, fiber);
    let curve = part(&after, "fiber");
    assert_eq!((curve.1, curve.2), (Class::Live, Class::Live), "{after:?}");
    let doc = store.document();
    let a = analysis::classify(doc, store.reactive(), fiber).unwrap();
    let c = a.parts.iter().find(|p| name(doc, p.node) == "fiber").unwrap();
    assert!(c.reasons.iter().any(|r| r.contains("outside the instance")), "{:?}", c.reasons);
}

#[test]
fn an_argument_bound_in_the_parent_is_wired() {
    let (mut store, comp) = desk();
    let slider = store.place_standard("slider", comp, [0.0, 0.0], Actor::Human).unwrap();
    let meter = store.place_standard("meter", comp, [300.0, 0.0], Actor::Human).unwrap();
    let doc = store.document();
    let e = expand::use_entity(doc, meter).unwrap();
    let s = expand::use_entity(doc, slider).unwrap();
    let level = doc.children(s).iter().copied().find(|&p| name(doc, p) == "value").unwrap();
    let path: LeafPath = "args.range".parse().unwrap();
    let expr = Expr::parse(&format!("{level}.port.signal * 1 mW")).unwrap();
    store.apply(TransactionInput { label: "bind".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops: vec![Op::Set { id: e, path, slot: Slot::Bound(expr) }], exact: false }).unwrap();
    let doc = store.document();
    let a = analysis::classify(doc, store.reactive(), meter).unwrap();
    assert!(matches!(&a.inputs["range"], Input::Wired(w) if w.starts_with("bound to")), "{:?}", a.inputs);
    let c = classes(&store, meter);
    assert_eq!(part(&c, "needle").1, Class::Live);
    // The readout reads only `in`, still constant.
    assert_eq!(part(&c, "readout").1, Class::Keyed);
    assert_eq!(part(&c, "dial").1, Class::Static);
}
