//! Edits after which the old engine left derived state stale until its next
//! whole-document pass (reactive-engine handoff §4.4, stage 1). Each now
//! re-derives at once, in both engines, so the new engine, which works per
//! edit, agrees with the old.

use iron_document::facet::{Composition, Name, Port, Size, Transform2d};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn name(text: &str) -> Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(text.into()));
    n.wrap()
}

fn sheet() -> (Store, NodeId) {
    let mut store = Store::new();
    let comp = store.authored().next_id();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("sheet".into()));
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] }])).unwrap();
    (store, comp)
}

/// A set on a repeat's template child reaches its items at once.
#[test]
fn a_set_on_a_repeat_template_child_reaches_the_items() {
    let (mut store, comp) = sheet();
    let built = markup::ops(r#"<group id="$g" repeat.count="2"><rect id="$r" w="4" h="4"/></group>"#, Some(comp), OrderKey::first(), store.authored().next_id()).unwrap();
    let (g, r) = (built.named["g"], built.named["r"]);
    store.apply(tx("repeat", built.ops)).unwrap();
    let copies = |s: &Store| -> Vec<Value> {
        let doc = s.tree();
        doc.descendants(g).into_iter().filter(|&d| d.is_expanded() && doc.live(d).is_some_and(|n| n.ty.name() == "rect")).map(|d| s.resolved(d, path("size.w")).unwrap()).collect()
    };
    assert_eq!(copies(&store), vec![Value::Number(4.0); 2]);
    store.apply(tx("wider", vec![Op::Set { id: r, path: path("size.w"), slot: Slot::Const(Value::Number(9.0)) }])).unwrap();
    assert_eq!(copies(&store), vec![Value::Number(9.0); 2]);
    store.apply(tx("bound", vec![Op::Set { id: r, path: path("size.w"), slot: Slot::Bound(Expr::parse("3 + 4").unwrap()) }])).unwrap();
    assert_eq!(copies(&store), vec![Value::Number(7.0); 2]);
}

/// An empty definition in a library, for entities that hold their own
/// ports (`detached`).
fn definition(store: &mut Store) -> NodeId {
    let ids = store.authored().next_ids(2);
    store
        .apply(tx(
            "library",
            vec![
                Op::Create { id: ids[0], ty: ty("library"), parent: None, order: OrderKey::first(), facets: vec![name("library")] },
                Op::Create { id: ids[1], ty: ty("definition"), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![name("thing")] },
            ],
        ))
        .unwrap_or_else(|e| panic!("{e:?}"));
    ids[1]
}

fn detached(def: NodeId) -> Facet {
    let mut i = facet::Instance::default();
    i.def = Slot::Const(Value::Ref(def));
    i.mode = Slot::Const(Value::Enum("detached".into()));
    i.wrap()
}

fn port(kind: &str, dir: &str, signal: Slot) -> Facet {
    let mut p = Port::default();
    p.kind = Slot::Const(Value::Str(kind.into()));
    p.dir = Slot::Const(Value::Enum(dir.into()));
    p.signal = signal;
    p.wrap()
}

/// Two entities with one port each, joined by a connection: `a` out, `b`
/// in, so `b` reads `a`'s signal.
fn wired() -> (Store, NodeId, NodeId) {
    let mut store = Store::new();
    let def = definition(&mut store);
    let ids = store.authored().next_ids(6);
    let [model, e1, e2, a, b, c] = ids.try_into().unwrap();
    let rels = store.authored().next_relation_ids(2);
    let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
    store
        .apply(tx(
            "wired",
            vec![
                Op::Create { id: model, ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
                Op::Create { id: e1, ty: ty("entity"), parent: Some(model), order: OrderKey::first(), facets: vec![detached(def), facet::Args::default().wrap()] },
                Op::Create { id: e2, ty: ty("entity"), parent: Some(model), order: OrderKey::after(&OrderKey::first()), facets: vec![detached(def), facet::Args::default().wrap()] },
                Op::Create { id: a, ty: ty("port"), parent: Some(e1), order: OrderKey::first(), facets: vec![port("value", "out", Slot::Const(Value::Number(5.0))), name("a")] },
                Op::Create { id: b, ty: ty("port"), parent: Some(e2), order: OrderKey::first(), facets: vec![port("value", "in", Slot::Const(Value::Number(1.0))), name("b")] },
                Op::Create { id: c, ty: ty("connection"), parent: Some(model), order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![facet::Connection::default().wrap()] },
                Op::Link { rel: Relation { id: rels[0], from: c, to: a, rel: RelKind::Connects, data: end("a") } },
                Op::Link { rel: Relation { id: rels[1], from: c, to: b, rel: RelKind::Connects, data: end("b") } },
            ],
        ))
        .unwrap_or_else(|e| panic!("{e:?}"));
    (store, a, b)
}

/// A port's direction decides which end a connection feeds: changing it
/// re-derives the signal at once.
#[test]
fn a_port_direction_set_rederives_the_connection() {
    let (mut store, a, b) = wired();
    assert_eq!(store.resolved(b, path("port.signal")), Some(Value::Number(5.0)));
    // Both ends out: the connection feeds neither.
    store.apply(tx("turn", vec![Op::Set { id: b, path: path("port.dir"), slot: Slot::Const(Value::Enum("out".into())) }])).unwrap();
    assert_eq!(store.resolved(b, path("port.signal")), Some(Value::Number(1.0)));
    // Reversed: `a` reads `b`.
    store.apply(tx("reverse", vec![Op::Set { id: a, path: path("port.dir"), slot: Slot::Const(Value::Enum("in".into())) }])).unwrap();
    assert_eq!(store.resolved(a, path("port.signal")), Some(Value::Number(1.0)));
}

/// A port's declared type is checked against what it computes: changing
/// the type checks again at once.
#[test]
fn a_datatype_set_checks_the_signal_again() {
    let mut store = Store::new();
    let def = definition(&mut store);
    let ids = store.authored().next_ids(3);
    let [model, e, p] = ids.try_into().unwrap();
    let mut port = Port::default();
    port.kind = Slot::Const(Value::Str("value".into()));
    port.dir = Slot::Const(Value::Enum("out".into()));
    port.signal = Slot::Bound(Expr::parse("2 + 3").unwrap());
    port.datatype = Slot::Const(Value::Str("bool".into()));
    store
        .apply(tx(
            "port",
            vec![
                Op::Create { id: model, ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
                Op::Create { id: e, ty: ty("entity"), parent: Some(model), order: OrderKey::first(), facets: vec![detached(def), facet::Args::default().wrap()] },
                Op::Create { id: p, ty: ty("port"), parent: Some(e), order: OrderKey::first(), facets: vec![port.wrap(), name("p")] },
            ],
        ))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let leaf = Leaf { node: p, path: path("port.signal") };
    assert!(store.reactive().errors.contains_key(&leaf), "5 is not a bool");
    store.apply(tx("number", vec![Op::Set { id: p, path: path("port.datatype"), slot: Slot::Const(Value::Str("number".into())) }])).unwrap();
    assert!(!store.reactive().errors.contains_key(&leaf), "{:?}", store.reactive().errors.get(&leaf));
}

/// Session nodes derive nothing, so `apply_session` needs no expansion: a
/// derived id cannot sit under a session-tier id. A session main `use` stays
/// as it was made (the council's round 2 thought it went stale; it never
/// derives).
#[test]
fn a_session_use_derives_nothing() {
    let (mut store, comp) = sheet();
    store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let def = standard::find(store.tree(), "meter").unwrap();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let ids = store.authored().next_session_ids(2);
    let mut u = facet::Use::default();
    u.notation = Slot::Const(Value::Str("free".into()));
    let mut main = facet::Instance::default();
    main.def = Slot::Const(Value::Ref(def));
    main.mode = Slot::Const(Value::Str("main".into()));
    store
        .apply_session(vec![
            Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![c.wrap(), name("session sheet")] },
            Op::Create { id: ids[1], ty: ty("use"), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![u.wrap(), main.wrap(), Transform2d::default().wrap(), Size::default().wrap()] },
        ])
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(NodeId::from_path(&[ids[1], def]), None);
    // A later transaction does not derive it either.
    let id = store.authored().next_id();
    store.apply(tx("group", vec![Op::Create { id, ty: ty("group"), parent: Some(comp), order: OrderKey::first(), facets: vec![] }])).unwrap();
    assert!(store.tree().descendants(ids[1]).is_empty());
}
