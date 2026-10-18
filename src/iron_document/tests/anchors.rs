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

fn world(store: &Store, anchor: NodeId) -> (f64, f64) {
    (num(store, anchor, "anchor.wx"), num(store, anchor, "anchor.wy"))
}
fn close(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
}

#[test]
fn dragging_a_loose_end_off_unplugs_it_in_one_step() {
    let Bench { mut store, jack_use, lead_anchor, .. } = bench();
    let lead_use = lead_anchor.expanded_root().unwrap();
    let connected = |s: &Store| s.port_anchors().iter().any(|a| a.anchor == lead_anchor && a.connection.is_some());
    assert!(connected(&store));
    let entries = store.history().past().len();
    // Unplugged for the drag: the end stays where it was, the jack too.
    let p = store.begin_plug(lead_anchor, lead_use).unwrap();
    assert!(!connected(&store));
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0));
    store.plug_drag(&p, [500.0, 480.0], None);
    assert!(store.finish_plug(p, None).unwrap().is_some());
    assert!(close(world(&store, lead_anchor), (500.0, 480.0)), "{:?}", world(&store, lead_anchor));
    assert_eq!(num(&store, jack_use, "transform2d.x"), 100.0);
    assert_eq!(store.history().past().len(), entries + 1);
    // One undo plugs it back in.
    store.undo(1).unwrap().unwrap();
    assert!(connected(&store));
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0));
    // Released back on the port it came from, nothing changes.
    let p = store.begin_plug(lead_anchor, lead_use).unwrap();
    let targets = store.plug_targets(&p);
    assert_eq!(targets.len(), 1, "{targets:?}");
    store.plug_drag(&p, [0.0, 0.0], Some(&targets[0]));
    assert_eq!(store.finish_plug(p, Some(targets[0].clone())).unwrap(), None);
    assert!(connected(&store));
    assert_eq!(store.history().past().len(), entries);
}

#[test]
fn plugging_two_rigid_ports_mates_their_parts() {
    let Bench { mut store, jack_use, lead_anchor, .. } = bench();
    // Free the jack's out: unplug the lead.
    let c = store.port_anchors().into_iter().find(|a| a.anchor == lead_anchor).unwrap().connection.unwrap();
    store.unplug(c, "unplug", Actor::Human).unwrap();
    // A socket: an in port at (0, 5) facing down (+y), placed at (400, 0).
    let library = store.document().roots()[0];
    let comp = store.document().roots()[1];
    let model = store.document().roots()[2];
    let ids = store.document().next_ids(5);
    let [socket, socket_in, socket_t, socket_body, socket_anchor] = ids.try_into().unwrap();
    let mut facing = at(0.0, 5.0);
    facing.rot = Slot::Const(Value::Number(std::f64::consts::FRAC_PI_2));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("c".into()));
    store
        .apply(tx("socket", vec![
            Op::Create { id: socket, ty: ty("definition"), parent: Some(library), order: key(5), facets: vec![named("socket")] },
            Op::Create { id: socket_in, ty: ty("port_spec"), parent: Some(socket), order: key(0), facets: vec![named("in"), port("in", Slot::Const(Value::Number(0.0)))] },
            Op::Create { id: socket_t, ty: ty("template"), parent: Some(socket), order: key(1), facets: vec![t.wrap()] },
            Op::Create { id: socket_body, ty: ty("rect"), parent: Some(socket_t), order: key(0), facets: vec![at(-10.0, 0.0).wrap(), Size::default().wrap()] },
            Op::Create { id: socket_anchor, ty: ty("empty"), parent: Some(socket_t), order: key(1), facets: vec![facing.wrap(), anchor(socket_in, false), named("in")] },
        ]))
        .unwrap();
    let place = Placement {
        entity_parent: Some(model),
        entity_after: store.document().children(model).last().map(|&c| store.document().live(c).unwrap().order.clone()),
        figure_parent: Some(comp),
        figure_after: store.document().children(comp).last().map(|&c| store.document().live(c).unwrap().order.clone()),
    };
    let (_, ops) = definition::instantiate(store.document(), socket, "c", "socket", &place).unwrap();
    store.apply(tx("place socket", ops)).unwrap();
    let socket_use = *store.document().children(comp).last().unwrap();
    store.apply(tx("move socket", vec![Op::Set { id: socket_use, path: path("transform2d.x"), slot: Slot::Const(Value::Number(400.0)) }])).unwrap();
    let socket_anchor = NodeId::expanded(socket_use, socket_anchor).unwrap();
    let jack_anchor = store.port_anchors().into_iter().find(|a| a.body == jack_use).unwrap().anchor;
    assert_eq!(world(&store, socket_anchor), (400.0, 5.0));

    // Dragged by its port onto the jack's: it snaps there, turned to face it.
    let p = store.begin_plug(socket_anchor, socket_use).unwrap();
    let targets = store.plug_targets(&p);
    assert_eq!(targets.iter().map(|t| t.anchor).collect::<Vec<_>>(), vec![jack_anchor]);
    store.plug_drag(&p, [140.0, 215.0], Some(&targets[0]));
    store.finish_plug(p, Some(targets[0].clone())).unwrap().unwrap();
    assert!(close(world(&store, socket_anchor), (150.0, 210.0)), "{:?}", world(&store, socket_anchor));
    let facing = num(&store, socket_anchor, "anchor.wrot");
    assert!((facing.abs() - std::f64::consts::PI).abs() < 1e-6, "{facing}");
    assert!(store.clusters().containing(socket_use).is_some(), "mated");

    // Mated: dragging either part moves the other.
    let h = store.begin_drag(jack_use, [0.0, 0.0]);
    store.drag_to(&h, [130.0, 200.0]);
    store.commit_gesture("move jack", Actor::Human, 0).unwrap().unwrap();
    // The mate holds exactly; the drag's target within the solver's
    // strong-over-weak weighting (a few thousandths of a unit here).
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 0.05 && (a.1 - b.1).abs() < 0.05;
    assert!(close(world(&store, socket_anchor), world(&store, jack_anchor)));
    assert!(near(world(&store, socket_anchor), (180.0, 210.0)), "{:?}", world(&store, socket_anchor));
    let h = store.begin_drag(socket_use, [0.0, 0.0]);
    let [sx, sy] = [num(&store, socket_use, "transform2d.x"), num(&store, socket_use, "transform2d.y")];
    store.drag_to(&h, [sx + 10.0, sy]);
    store.commit_gesture("move socket", Actor::Human, 0).unwrap().unwrap();
    assert!((num(&store, jack_use, "transform2d.x") - 140.0).abs() < 0.05, "{}", num(&store, jack_use, "transform2d.x"));
    assert!(close(world(&store, socket_anchor), world(&store, jack_anchor)));

    // Undo the two moves and the plug: apart again, where it was.
    for _ in 0..3 {
        store.undo(1).unwrap().unwrap();
    }
    assert_eq!(world(&store, socket_anchor), (400.0, 5.0));
    assert!(store.clusters().containing(socket_use).is_none());
}

#[test]
fn pulling_a_part_off_a_loose_end_leaves_the_end_where_it_was() {
    let Bench { mut store, jack_use, lead_anchor, .. } = bench();
    let jack_anchor = store.port_anchors().into_iter().find(|a| a.body == jack_use).unwrap().anchor;
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0));
    let p = store.begin_plug(jack_anchor, jack_use).unwrap();
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0), "unplugged");
    store.plug_drag(&p, [400.0, 400.0], None);
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0), "dragging");
    store.finish_plug(p, None).unwrap();
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0), "released");
}

#[test]
fn a_loose_end_dropped_once_and_plugged_again_stays_put_when_its_part_leaves() {
    let Bench { mut store, jack_use, lead_anchor, .. } = bench();
    let lead_use = lead_anchor.expanded_root().unwrap();
    let jack_anchor = store.port_anchors().into_iter().find(|a| a.body == jack_use).unwrap().anchor;
    // Dropped far away: its place is now an override.
    let p = store.begin_plug(lead_anchor, lead_use).unwrap();
    store.plug_drag(&p, [900.0, 700.0], None);
    store.finish_plug(p, None).unwrap();
    assert!(close(world(&store, lead_anchor), (900.0, 700.0)));
    // Plugged back into the jack.
    let p = store.begin_plug(lead_anchor, lead_use).unwrap();
    let t = store.plug_targets(&p).into_iter().find(|t| t.anchor == jack_anchor).unwrap();
    store.plug_drag(&p, t.world, Some(&t));
    store.finish_plug(p, Some(t)).unwrap();
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0));
    // The jack pulled off by its port: the end stays where the jack was.
    let p = store.begin_plug(jack_anchor, jack_use).unwrap();
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0), "unplugged");
    store.plug_drag(&p, [400.0, 400.0], None);
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0), "dragging");
    store.finish_plug(p, None).unwrap();
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0), "released");
}

/// A `tail`: one out port whose anchor is a loose end, placed at (600, 50).
fn add_tail(store: &mut Store) -> (NodeId, NodeId, NodeId) {
    let library = store.document().roots()[0];
    let comp = store.document().roots()[1];
    let model = store.document().roots()[2];
    let ids = store.document().next_ids(4);
    let [tail, tail_out, tail_t, tail_anchor] = ids.try_into().unwrap();
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("c".into()));
    store
        .apply(tx("tail", vec![
            Op::Create { id: tail, ty: ty("definition"), parent: Some(library), order: key(6), facets: vec![named("tail")] },
            Op::Create { id: tail_out, ty: ty("port_spec"), parent: Some(tail), order: key(0), facets: vec![named("out"), port("out", Slot::Const(Value::Number(2.0)))] },
            Op::Create { id: tail_t, ty: ty("template"), parent: Some(tail), order: key(1), facets: vec![t.wrap()] },
            Op::Create { id: tail_anchor, ty: ty("empty"), parent: Some(tail_t), order: key(0), facets: vec![at(0.0, 0.0).wrap(), anchor(tail_out, true), named("out")] },
        ]))
        .unwrap();
    let place = Placement {
        entity_parent: Some(model),
        entity_after: store.document().children(model).last().map(|&c| store.document().live(c).unwrap().order.clone()),
        figure_parent: Some(comp),
        figure_after: store.document().children(comp).last().map(|&c| store.document().live(c).unwrap().order.clone()),
    };
    let (entity, ops) = definition::instantiate(store.document(), tail, "c", "tail", &place).unwrap();
    store.apply(tx("place tail", ops)).unwrap();
    let tail_use = *store.document().children(comp).last().unwrap();
    store.apply(tx("move tail", vec![
        Op::Set { id: tail_use, path: path("transform2d.x"), slot: Slot::Const(Value::Number(600.0)) },
        Op::Set { id: tail_use, path: path("transform2d.y"), slot: Slot::Const(Value::Number(50.0)) },
    ])).unwrap();
    (entity, tail_use, NodeId::expanded(tail_use, tail_anchor).unwrap())
}

#[test]
fn two_loose_ends_join_and_the_one_landed_on_holds() {
    let Bench { mut store, lead_anchor, .. } = bench();
    let c = store.port_anchors().into_iter().find(|a| a.anchor == lead_anchor).unwrap().connection.unwrap();
    store.unplug(c, "unplug", Actor::Human).unwrap();
    let (tail, tail_use, tail_anchor) = add_tail(&mut store);
    assert_eq!(world(&store, tail_anchor), (600.0, 50.0));
    // The tail's end dropped on the lead's: it joins there.
    let p = store.begin_plug(tail_anchor, tail_use).unwrap();
    let t = store.plug_targets(&p).into_iter().find(|t| t.anchor == lead_anchor).expect("a loose end is a target");
    store.plug_drag(&p, t.world, Some(&t));
    store.finish_plug(p, Some(t)).unwrap();
    assert_eq!(world(&store, tail_anchor), (150.0, 210.0));
    assert_eq!(world(&store, lead_anchor), (150.0, 210.0));
    // The lead's end holds the joint: dragging it carries the tail's end.
    let lead_use = lead_anchor.expanded_root().unwrap();
    let h = store.begin_drag(lead_anchor, [0.0, 0.0]);
    store.drag_to(&h, [200.0, 260.0]);
    store.commit_gesture("move joint", Actor::Human, 0).unwrap().unwrap();
    assert!(close(world(&store, tail_anchor), (200.0, 260.0)), "{:?}", world(&store, tail_anchor));
    let _ = lead_use;
    // Gender: two male connectors do not mate.
    store.undo(1).unwrap().unwrap();
    store.undo(1).unwrap().unwrap();
    let lead_port = store.port_anchors().into_iter().find(|a| a.anchor == lead_anchor).unwrap().outer;
    let tail_port = store.port_anchors().into_iter().find(|a| a.anchor == tail_anchor).unwrap().outer;
    let _ = tail;
    let lead_entity = lead_port.expanded_root().unwrap();
    let (_, lead_spec) = lead_port.expanded_parts().unwrap();
    let (_, tail_spec) = tail_port.expanded_parts().unwrap();
    let _ = lead_entity;
    store.apply(tx("male", vec![
        Op::Set { id: lead_spec, path: path("port.gender"), slot: Slot::Const(Value::Enum("male".into())) },
        Op::Set { id: tail_spec, path: path("port.gender"), slot: Slot::Const(Value::Enum("male".into())) },
    ])).unwrap();
    let p = store.begin_plug(tail_anchor, tail_use).unwrap();
    assert!(store.plug_targets(&p).iter().all(|t| t.anchor != lead_anchor));
    store.cancel_plug(p);
    store.apply(tx("female", vec![Op::Set { id: lead_spec, path: path("port.gender"), slot: Slot::Const(Value::Enum("female".into())) }])).unwrap();
    let p = store.begin_plug(tail_anchor, tail_use).unwrap();
    assert!(store.plug_targets(&p).iter().any(|t| t.anchor == lead_anchor));
    store.cancel_plug(p);
}
