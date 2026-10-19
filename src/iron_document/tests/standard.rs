//! The standard components (component-authoring.md §6): each places as a
//! linked instance, its definition copied in once; the slider drives a
//! laser by a line; the Library tab's session composition bakes every
//! preview with no bindings left.

use iron_document::facet::{Composition, Name};
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

#[test]
fn standard_components_place_once_and_a_slider_drives_a_laser() {
    let (mut store, comp) = desk();
    for e in standard::ENTRIES {
        let u = store.place_standard(e.key, comp, [100.0, 100.0], Actor::Human).unwrap_or_else(|err| panic!("{}: {err}", e.key));
        assert!(expand::use_entity(store.document(), u).is_some(), "{}", e.key);
        assert_eq!(store.resolved(u, "transform2d.x".parse().unwrap()), Some(Value::Number(100.0)));
    }
    // One history entry per placement; the definition comes in once.
    let entries = store.history().past().len();
    let second = store.place_standard("laser", comp, [400.0, 100.0], Actor::Human).unwrap();
    assert_eq!(store.history().past().len(), entries + 1);
    let lasers = store.document().nodes_of_type(ty("definition")).filter(|&d| store.document().is_live(d)).filter(|&d| standard::find(store.document(), "laser") == Some(d)).count();
    assert_eq!(lasers, 1);
    // The slider's value (10 × 1 mW) drives the second laser's power by a
    // line; a readout of plain numbers takes it too.
    let doc = store.document();
    let port = |u: NodeId, name: &str| {
        let e = expand::use_entity(doc, u).unwrap();
        doc.children(e).iter().copied().find(|&p| doc.get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
    };
    let slider = doc.children(comp).iter().copied().find(|&u| doc.get::<Name>(u).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some("slider")).unwrap();
    let (from, to) = (port(slider, "value"), port(second, "power"));
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let ops = author::connect(store.document(), &read, from, to).unwrap();
    store.apply(TransactionInput { label: "connect".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }).unwrap();
    assert_eq!(store.resolved(to, "port.signal".parse().unwrap()), Some(Value::parse_quantity("10 mW").unwrap()));
}

#[test]
fn the_library_composition_bakes_every_preview() {
    let mut store = Store::new();
    let first = store.document().next_session_ids(1)[0];
    let ops = standard::library_ops(first, OrderKey::first());
    assert!(ops.iter().all(|op| match op {
        Op::Create { facets, .. } => facets.iter().all(|f| f.field_names().into_iter().all(|n| f.get(n).is_none_or(|s| s.expr().is_none()))),
        _ => true,
    }));
    store.apply_session(ops).unwrap();
    let doc = store.document();
    let lib = composition::compositions(doc).into_iter().find(|&c| doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(standard::LIBRARY)).unwrap();
    assert!(lib.is_session());
    assert_eq!(doc.children(lib).len(), standard::ENTRIES.len());
    assert!(!store.save().contains("Dial meter"), "never saved");
}
