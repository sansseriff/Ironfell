//! Anchors (component-model.md, "anchors"): a port's place on a picture,
//! derived into the graph. A `jack` has an out port anchored on its
//! picture; a `lead` has an in port whose anchor follows whatever it is
//! connected to, and a tip drawn at that anchor. Moving or turning the jack
//! carries the lead's tip along; dragging the tip moves the jack.

use iron_document::definition::{self, Placement};
use iron_document::facet::{Anchor, Composition, Connection, Name, Port, Size, Template, Transform2d};
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
fn named(s: &str) -> Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(s.into()));
    n.wrap()
}
fn at(x: f64, y: f64) -> Transform2d {
    let mut t = Transform2d::default();
    t.x = Slot::Const(Value::Number(x));
    t.y = Slot::Const(Value::Number(y));
    t
}
fn port(dir: &str, signal: Slot) -> Facet {
    let mut p = Port::default();
    p.kind = Slot::Const(Value::Str("value".into()));
    p.dir = Slot::Const(Value::Enum(dir.into()));
    p.signal = signal;
    p.wrap()
}
fn anchor(port: NodeId, follow: bool) -> Facet {
    let mut a = Anchor::default();
    a.port = Slot::Const(Value::Ref(port));
    a.follow = Slot::Const(Value::Bool(follow));
    a.wrap()
}
fn num(store: &Store, id: NodeId, p: &str) -> f64 {
    store.resolved(id, path(p)).unwrap_or_else(|| panic!("{id}.{p}")).as_f64().unwrap()
}
fn key(n: u32) -> OrderKey {
    (0..n).fold(OrderKey::first(), |k, _| OrderKey::after(&k))
}

struct Bench {
    store: Store,
    jack_use: NodeId,
    tip: NodeId,
    lead_anchor: NodeId,
}

fn bench() -> Bench {
    let mut store = Store::new();
    let ids = store.document().next_ids(12);
    let [library, comp, model, jack, jack_out, jack_t, jack_body, jack_anchor, lead, lead_in, lead_t, lead_anchor] = ids.try_into().unwrap();
    let template = || {
        let mut t = Template::default();
        t.notation = Slot::Const(Value::Str("c".into()));
        t.wrap()
    };
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    store
        .apply(tx(
            "library",
            vec![
                Op::Create { id: library, ty: ty("library"), parent: None, order: key(0), facets: vec![] },
                Op::Create { id: comp, ty: ty("composition"), parent: None, order: key(1), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: key(2), facets: vec![] },
                // A jack: a body, and its out port anchored at (50, 10).
                Op::Create { id: jack, ty: ty("definition"), parent: Some(library), order: key(0), facets: vec![named("jack")] },
                Op::Create { id: jack_out, ty: ty("port_spec"), parent: Some(jack), order: key(0), facets: vec![named("out"), port("out", Slot::Const(Value::Number(1.0)))] },
                Op::Create { id: jack_t, ty: ty("template"), parent: Some(jack), order: key(1), facets: vec![template()] },
                Op::Create { id: jack_body, ty: ty("rect"), parent: Some(jack_t), order: key(0), facets: vec![at(0.0, 0.0).wrap(), Size::default().wrap()] },
                Op::Create { id: jack_anchor, ty: ty("empty"), parent: Some(jack_t), order: key(1), facets: vec![at(50.0, 10.0).wrap(), anchor(jack_out, false), named("out")] },
                // A lead: its in port's anchor follows its partner.
                Op::Create { id: lead, ty: ty("definition"), parent: Some(library), order: key(1), facets: vec![named("lead")] },
                Op::Create { id: lead_in, ty: ty("port_spec"), parent: Some(lead), order: key(0), facets: vec![named("in"), port("in", Slot::Const(Value::Number(0.0)))] },
                Op::Create { id: lead_t, ty: ty("template"), parent: Some(lead), order: key(1), facets: vec![template()] },
                Op::Create { id: lead_anchor, ty: ty("empty"), parent: Some(lead_t), order: key(0), facets: vec![at(0.0, 0.0).wrap(), anchor(lead_in, true), named("in")] },
            ],
        ))
        .unwrap();
    // The lead's tip sits on its anchor.
    let tip = store.document().next_id();
    let mut t = Transform2d::default();
    t.x = Slot::Bound(Expr::parse(&format!("{lead_anchor}.transform2d.x")).unwrap());
    t.y = Slot::Bound(Expr::parse(&format!("{lead_anchor}.transform2d.y")).unwrap());
    store.apply(tx("tip", vec![Op::Create { id: tip, ty: ty("rect"), parent: Some(lead_t), order: key(1), facets: vec![t.wrap(), Size::default().wrap(), named("tip")] }])).unwrap();

    let place = |s: &Store| Placement {
        entity_parent: Some(model),
        entity_after: s.document().children(model).last().map(|&c| s.document().live(c).unwrap().order.clone()),
        figure_parent: Some(comp),
        figure_after: s.document().children(comp).last().map(|&c| s.document().live(c).unwrap().order.clone()),
    };
    let (j, ops) = definition::instantiate(store.document(), jack, "c", "jack", &place(&store)).unwrap();
    store.apply(tx("jack", ops)).unwrap();
    let (l, ops) = definition::instantiate(store.document(), lead, "c", "lead", &place(&store)).unwrap();
    store.apply(tx("lead", ops)).unwrap();
    let uses: Vec<NodeId> = store.document().children(comp).to_vec();
    let (jack_use, lead_use) = (uses[0], uses[1]);
    // The jack placed at (100, 200); the lead at (300, 0), not yet plugged in.
    store.apply(tx("place", vec![
        Op::Set { id: jack_use, path: path("transform2d.x"), slot: Slot::Const(Value::Number(100.0)) },
        Op::Set { id: jack_use, path: path("transform2d.y"), slot: Slot::Const(Value::Number(200.0)) },
        Op::Set { id: lead_use, path: path("transform2d.x"), slot: Slot::Const(Value::Number(300.0)) },
    ])).unwrap();
    let tip = NodeId::expanded(lead_use, tip).unwrap();
    let lead_anchor = NodeId::expanded(lead_use, lead_anchor).unwrap();
    assert_eq!((num(&store, lead_anchor, "anchor.wx"), num(&store, lead_anchor, "anchor.wy")), (300.0, 0.0));
    // Plugged in: a connection from the jack's out to the lead's in.
    let (a, b) = (NodeId::expanded(j, jack_out).unwrap(), NodeId::expanded(l, lead_in).unwrap());
    let id = store.document().next_id();
    let rels = store.document().next_relation_ids(2);
    let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
    store
        .apply(tx("plug", vec![
            Op::Create { id, ty: ty("connection"), parent: Some(model), order: key(5), facets: vec![Connection::default().wrap()] },
            Op::Link { rel: Relation { id: rels[0], from: id, to: a, rel: RelKind::Connects, data: end("a") } },
            Op::Link { rel: Relation { id: rels[1], from: id, to: b, rel: RelKind::Connects, data: end("b") } },
        ]))
        .unwrap();
    Bench { store, jack_use, tip, lead_anchor }
}

#[test]
fn a_following_anchor_sits_on_its_partner_and_follows_it() {
    let Bench { mut store, jack_use, tip, lead_anchor } = bench();
    // The tip, in the lead's space, lands on the jack's anchor at
    // (100 + 50, 200 + 10).
    let world = |s: &Store| (num(s, lead_anchor, "anchor.wx"), num(s, lead_anchor, "anchor.wy"));
    assert_eq!(world(&store), (150.0, 210.0));
    assert_eq!((num(&store, tip, "transform2d.x"), num(&store, tip, "transform2d.y")), (-150.0, 210.0));
    // Move the jack: the tip follows in the same transaction.
    store.apply(tx("move jack", vec![Op::Set { id: jack_use, path: path("transform2d.x"), slot: Slot::Const(Value::Number(130.0)) }])).unwrap();
    assert_eq!(world(&store), (180.0, 210.0));
    // Turn it a quarter turn: its anchor swings to (130 - 10, 200 + 50).
    store.apply(tx("turn jack", vec![Op::Set { id: jack_use, path: path("transform2d.rot"), slot: Slot::Const(Value::Number(std::f64::consts::FRAC_PI_2)) }])).unwrap();
    let (x, y) = world(&store);
    assert!((x - 120.0).abs() < 1e-9 && (y - 250.0).abs() < 1e-9, "{x} {y}");
    store.undo(1).unwrap().unwrap();
    // Dragging the lead's end inverts onto what placed the jack: the jack moves.
    match store.handle(lead_anchor, path("transform2d.x")) {
        Handle::Linear { source, .. } => assert_eq!(source, Leaf { node: jack_use, path: path("transform2d.x") }),
        other => panic!("expected the jack's x, got {other:?}"),
    }
    store.preview_target(lead_anchor, path("transform2d.x"), Value::Number(-100.0)).unwrap();
    assert_eq!(num(&store, jack_use, "transform2d.x"), 150.0);
    assert_eq!(world(&store), (200.0, 210.0));
}
