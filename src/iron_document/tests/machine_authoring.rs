//! Making a component's states by hand (machines.md §9 Q8): Add state on
//! the main, pin a state, edit a part, and the edit becomes that state's
//! pose through a new channel; instances then animate between them.

use iron_document::facet::{Appearance, Composition, Name, Port, Size, Template, Transform2d};
use iron_document::*;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn named(t: &str) -> facet::Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(t.into()));
    n.wrap()
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

#[test]
fn a_state_made_by_hand_animates_its_instances() {
    let mut store = Store::new();
    let ids = store.document().next_ids(6);
    let [bench, model, library, def, template, dot] = ids[..] else { unreachable!() };
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("free".into()));
    store
        .apply(tx(
            "card",
            vec![
                Op::Create { id: bench, ty: ty("composition"), parent: None, order: k(0), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: k(1), facets: vec![] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(2), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(0), facets: vec![t.wrap()] },
                Op::Create { id: dot, ty: ty("rect"), parent: Some(template), order: k(0), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), Appearance::default().wrap(), named("dot")] },
            ],
        ))
        .unwrap();
    // Add state: a machine following a new `state` input, two states.
    let (ops, second) = machine::add_state_ops(store.document(), def).unwrap();
    store.apply(tx("add state", ops)).unwrap();
    assert_eq!(second, "state_2");
    let doc = store.document();
    let (m, port) = machine::machines(doc, def)[0].clone();
    assert_eq!(port, "state");
    let spec = doc.children(def).iter().copied().find(|&c| doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some("state") && doc.get::<Port>(c).is_some()).unwrap();
    assert_eq!(doc.get::<Port>(spec).unwrap().datatype.constant(), Some(&Value::Str("choice(default|state_2)".into())));
    // Pin state_2; dim the dot. One transaction makes a channel, binds the
    // dot to it and records 0.2 as state_2's pose.
    store.pin_state(Some(machine::Pinned { def, machine: m, state: "state_2".into() }));
    store.apply(tx("dim", vec![Op::Set { id: dot, path: path("appearance.opacity"), slot: Slot::Const(Value::Number(0.2)) }])).unwrap();
    store.advance_machines(0.5);
    let chans = machine::channels(store.document(), def);
    assert_eq!(chans.len(), 1);
    assert_eq!(chans[0].2, 1.0, "the channel rests at the dot's value before the edit");
    assert!(store.document().leaf(dot, path("appearance.opacity")).unwrap().expr().is_some(), "the dot reads the channel");
    assert_eq!(store.resolved(dot, path("appearance.opacity")), Some(Value::Number(0.2)), "the pinned state shows on the definition");
    // Unpinned, the definition is at rest.
    store.pin_state(None);
    assert_eq!(store.resolved(dot, path("appearance.opacity")), Some(Value::Number(1.0)));
    // Rename the state; the port's choice follows.
    let s2 = store.document().children(m).iter().copied().find(|&s| store.document().get::<Name>(s).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some("state_2")).unwrap();
    let ops = machine::rename_state_ops(store.document(), s2, "dim").unwrap();
    store.apply(tx("rename", ops)).unwrap();
    assert_eq!(store.document().get::<Port>(spec).unwrap().datatype.constant(), Some(&Value::Str("choice(default|dim)".into())));
    // An instance animates between them: 300 ms, Easy Ease.
    let (ops, summary) = compose::compose_ops(&store, &format!(r#"<place def="{def}" in="bench" x="0" y="0"/>"#)).unwrap();
    store.apply(tx("place", ops)).unwrap();
    let line = summary.lines().find(|l| l.contains("<placed")).unwrap();
    let get = |a: &str| -> NodeId {
        let start = line.find(&format!("{a}=\"")).unwrap() + a.len() + 2;
        line[start..start + line[start..].find('"').unwrap()].parse().unwrap()
    };
    let (e, u) = (get("entity"), get("use"));
    let opacity = |s: &Store| s.resolved(NodeId::expanded(u, dot).unwrap(), path("appearance.opacity")).unwrap().as_f64().unwrap();
    store.advance_machines(0.0);
    assert_eq!(opacity(&store), 1.0);
    let err = store.apply(tx("typo", vec![Op::Set { id: e, path: path("args.state"), slot: Slot::Const(Value::Str("dimm".into())) }])).unwrap_err();
    assert!(err[0].message.contains("default, dim"), "{err:?}");
    store.apply(tx("dim it", vec![Op::Set { id: e, path: path("args.state"), slot: Slot::Const(Value::Str("dim".into())) }])).unwrap();
    store.advance_machines(1.0);
    store.advance_machines(1.15);
    let mid = opacity(&store);
    assert!(mid < 1.0 && mid > 0.2, "mid-way: {mid}");
    store.advance_machines(2.0);
    assert!((opacity(&store) - 0.2).abs() < 1e-9);
    // Undo the recorded edit, all of it in one step: no channel, the dot
    // a plain 1 again (after undoing the place, rename too).
    for _ in 0..4 {
        store.undo(0).unwrap().unwrap();
    }
    assert!(machine::channels(store.document(), def).is_empty());
    assert_eq!(store.document().leaf(dot, path("appearance.opacity")).unwrap().constant(), Some(&Value::Number(1.0)));
}
