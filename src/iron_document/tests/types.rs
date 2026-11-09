//! Declared types at a component's boundary (plans/specs/types-and-units.md),
//! on `std:plot`: arguments checked when set and when computed, connections
//! gradual (a loose end always connects; a typed input checks what arrives).

use iron_document::facet::{Composition, Name};
use iron_document::*;
use iron_document::facet::Connection;
use std::collections::BTreeMap;

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
    TransactionInput { label: label.into(), actor: Actor::Model, ts: 0, idempotency_key: None, ops, exact: false }
}

/// A 2D composition and a model root.
fn desk() -> (Store, NodeId) {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    store
        .apply(tx(
            "desk",
            vec![
                Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: ids[1], ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
            ],
        ))
        .unwrap();
    (store, ids[0])
}

/// Place a plot; answer (entity, use).
fn place(store: &mut Store, attrs: &str) -> (NodeId, NodeId) {
    let markup = format!(r#"<place def="std:plot" in="bench" x="20" y="20" {attrs}/>"#);
    let (ops, summary) = compose::compose_ops(store, &markup).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("place", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let line = summary.lines().find(|l| l.contains("<placed")).unwrap();
    let get = |a: &str| -> NodeId {
        let start = line.find(&format!("{a}=\"")).unwrap() + a.len() + 2;
        line[start..start + line[start..].find('"').unwrap()].parse().unwrap()
    };
    (get("entity"), get("use"))
}

/// The plot's part named `name`.
fn part(store: &Store, u: NodeId, name: &str) -> NodeId {
    let doc = store.document();
    doc.descendants(u).into_iter().find(|&n| doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap_or_else(|| panic!("no part {name}"))
}

/// Every binding error inside the plot (its picture or its entity's ports).
fn errors(store: &Store, e: NodeId, u: NodeId) -> Vec<String> {
    store.reactive().errors.iter().filter(|(l, _)| l.node == u || l.node == e || l.node.expanded_parts().is_some_and(|(o, _)| o == u || o == e)).map(|(l, m)| format!("{l}: {m}")).collect()
}

fn text(store: &Store, id: NodeId) -> String {
    store.resolved(id, path("text.content")).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

fn anchors(store: &Store, id: NodeId) -> usize {
    contour::geometry(store, id).anchor_count()
}


fn set(store: &mut Store, e: NodeId, leaf: &str, slot: Slot) -> Result<Applied, Vec<ApplyError>> {
    store.apply(tx("set", vec![Op::Set { id: e, path: path(leaf), slot }]))
}

fn port_of(store: &Store, e: NodeId, name: &str) -> NodeId {
    let doc = store.document();
    doc.children(e).iter().copied().find(|&c| doc.get::<Name>(c).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
}

fn connect(store: &mut Store, from: NodeId, to: NodeId) -> Result<Applied, Vec<ApplyError>> {
    let doc = store.document();
    let model = doc.nodes_of_type(TypeId::lookup("model").unwrap()).next().unwrap();
    let id = doc.next_ids(1)[0];
    let rels = doc.next_relation_ids(2);
    let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
    let order = doc.children(model).last().map(|&c| OrderKey::after(&doc.live(c).unwrap().order)).unwrap_or_else(OrderKey::first);
    store.apply(tx(
        "connect",
        vec![
            Op::Create { id, ty: ty("connection"), parent: Some(model), order, facets: vec![Connection::default().wrap()] },
            Op::Link { rel: Relation { id: rels[0], from: id, to: from, rel: RelKind::Connects, data: end("a") } },
            Op::Link { rel: Relation { id: rels[1], from: id, to, rel: RelKind::Connects, data: end("b") } },
        ],
    ))
}

#[test]
fn arguments_are_checked_against_their_types_when_set_and_when_computed() {
    let (mut store, _) = desk();
    let (e, u) = place(&mut store, r#"xs="{= range(0 mm, 10 mm, 11)}" ys="{= range(0 mW, 5 mW, 11)}""#);
    // A choice: a typo is refused, naming the options.
    let err = set(&mut store, e, "args.vis", Slot::Const(Value::Str("hiden".into()))).unwrap_err();
    assert!(err[0].message.contains("hidden, shown"), "{err:?}");
    assert!(set(&mut store, e, "args.style", Slot::Const(Value::Str("scatter".into()))).is_ok());
    // A range is in its data's unit: a plain 0 for milliwatt data is refused
    // at the input, not as errors deep in the picture.
    let err = set(&mut store, e, "args.y_min", Slot::Const(Value::Number(0.0))).unwrap_err();
    assert!(err[0].message.contains("ys's unit"), "{err:?}");
    assert!(set(&mut store, e, "args.y_min", Slot::Const(Value::parse_quantity("0 mW").unwrap())).is_ok());
    // `auto` is NaN, typed as the word.
    assert!(set(&mut store, e, "args.y_min", Slot::Const(Value::Number(f64::NAN))).is_ok());
    // A formula's result is checked when it is computed: the error sits on
    // the argument.
    set(&mut store, e, "args.x_min", Slot::Bound(Expr::parse("3 s").unwrap())).unwrap();
    let errs = errors(&store, e, u);
    assert!(errs.iter().any(|m| m.contains("args.x_min") && m.contains("xs's unit")), "{errs:#?}");
}

#[test]
fn connections_are_gradual() {
    let (mut store, _) = desk();
    let (a, _) = place(&mut store, r#"xs="{= range(0 mm, 10 mm, 11)}""#);
    let (b, ub) = place(&mut store, r#"xs="{= range(0 s, 10 s, 11)}" ys="{= range(0, 1, 11)}""#);
    // The plot's outputs are undeclared (loose): x_lo connects to b's typed
    // cursor, and b's cursor checks what arrives (mm, not seconds).
    let (from, to) = (port_of(&store, a, "x_lo"), port_of(&store, b, "cursor"));
    connect(&mut store, from, to).expect("a loose end always connects");
    let errs = errors(&store, b, ub);
    assert!(errs.iter().any(|m| m.contains("cursor") && m.contains("xs's unit")), "{errs:#?}");
}
