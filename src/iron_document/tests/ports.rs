//! Ports, connections and one-way signals (ECM §6, §6.1): laser → fiber →
//! power meter. Signals ride the reactive graph; an in-port's is derived
//! from its connection; a reading inverts back to its source.

use iron_document::facet::{Composition, Connection, Name, Port, Size, Template, Transform2d};
use iron_document::definition::{self, Placement};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn named(s: &str) -> Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(s.into()));
    n.wrap()
}

fn port(kind: &str, dir: &str, signal: Option<String>) -> Facet {
    let mut p = Port::default();
    p.kind = Slot::Const(Value::Str(kind.into()));
    p.dir = Slot::Const(Value::Enum(dir.into()));
    if let Some(e) = signal {
        p.signal = Slot::Bound(Expr::parse(&e).unwrap());
    }
    p.wrap()
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

struct Lab {
    store: Store,
    laser: NodeId,
    fiber: NodeId,
    meter: NodeId,
    model: NodeId,
}

impl Lab {
    fn port(&self, entity: NodeId, name: &str) -> NodeId {
        let doc = self.store.document();
        doc.children(entity).iter().copied().find(|&p| doc.get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
    }
    fn signal(&self, entity: NodeId, name: &str) -> Value {
        self.store.resolved(self.port(entity, name), path("port.signal")).unwrap()
    }
    fn connect(&mut self, from: NodeId, to: NodeId) -> Result<Applied, Vec<ApplyError>> {
        let id = self.store.document().next_ids(1)[0];
        let rels = self.store.document().next_relation_ids(2);
        let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
        let key = self.store.document().children(self.model).last().map(|&c| OrderKey::after(&self.store.document().live(c).unwrap().order)).unwrap_or_else(OrderKey::first);
        self.store.apply(tx(
            "connect",
            vec![
                Op::Create { id, ty: ty("connection"), parent: Some(self.model), order: key, facets: vec![Connection::default().wrap()] },
                Op::Link { rel: Relation { id: rels[0], from: id, to: from, rel: RelKind::Connects, data: end("a") } },
                Op::Link { rel: Relation { id: rels[1], from: id, to, rel: RelKind::Connects, data: end("b") } },
            ],
        ))
    }
}

/// A library of three definitions, three entities in `model`, figures in a
/// "cartoon" composition, and the two connections of the chain.
fn lab() -> Lab {
    let mut store = Store::new();
    let ids = store.document().next_ids(16);
    let [library, cartoon, model, laser_def, power, laser_out, laser_t, laser_box, fiber_def, loss, length, fiber_in, fiber_out, meter_def, meter_in, meter_t] =
        ids.clone().try_into().unwrap();
    let k = |n: u32| {
        let mut key = OrderKey::first();
        for _ in 0..n {
            key = OrderKey::after(&key);
        }
        key
    };
    let param = |id, name: &str, default: Value| {
        let mut p = Port::default();
        p.kind = Slot::Const(Value::Str("value".into()));
        p.dir = Slot::Const(Value::Enum("in".into()));
        p.signal = Slot::Const(default);
        (id, vec![p.wrap(), named(name)])
    };
    let template = |notation: &str| {
        let mut t = Template::default();
        t.notation = Slot::Const(Value::Str(notation.into()));
        t.wrap()
    };
    let mut comp = Composition::default();
    comp.dims = Slot::Const(Value::Enum("2d".into()));
    let mut ops = vec![
        Op::Create { id: library, ty: ty("library"), parent: None, order: k(0), facets: vec![named("library")] },
        Op::Create { id: cartoon, ty: ty("composition"), parent: None, order: k(1), facets: vec![named("cartoon"), comp.wrap()] },
        Op::Create { id: model, ty: ty("model"), parent: None, order: k(2), facets: vec![named("model")] },
        Op::Create { id: laser_def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("laser")] },
    ];
    let (pid, pc) = param(power, "power", Value::parse_quantity("10 mW").unwrap());
    ops.push(Op::Create { id: pid, ty: ty("port_spec"), parent: Some(laser_def), order: k(0), facets: pc });
    ops.push(Op::Create { id: laser_out, ty: ty("port_spec"), parent: Some(laser_def), order: k(1), facets: vec![named("out"), port("optical_power", "out", Some(format!("{power}.port.signal")))] });
    ops.push(Op::Create { id: laser_t, ty: ty("template"), parent: Some(laser_def), order: k(2), facets: vec![template("cartoon")] });
    ops.push(Op::Create { id: laser_box, ty: ty("rect"), parent: Some(laser_t), order: k(0), facets: vec![Transform2d::default().wrap(), Size::default().wrap()] });
    ops.push(Op::Create { id: fiber_def, ty: ty("definition"), parent: Some(library), order: k(1), facets: vec![named("fiber")] });
    let (lid, lc) = param(loss, "loss_db_per_km", Value::Number(0.5));
    ops.push(Op::Create { id: lid, ty: ty("port_spec"), parent: Some(fiber_def), order: k(0), facets: lc });
    let (nid, nc) = param(length, "length_km", Value::Number(2.0));
    ops.push(Op::Create { id: nid, ty: ty("port_spec"), parent: Some(fiber_def), order: k(1), facets: nc });
    ops.push(Op::Create { id: fiber_in, ty: ty("port_spec"), parent: Some(fiber_def), order: k(2), facets: vec![named("in"), port("optical_power", "in", None)] });
    ops.push(Op::Create {
        id: fiber_out,
        ty: ty("port_spec"),
        parent: Some(fiber_def),
        order: k(3),
        facets: vec![named("out"), port("optical_power", "out", Some(format!("{fiber_in}.port.signal * pow(10, -{loss}.port.signal * {length}.port.signal / 10)")))],
    });
    ops.push(Op::Create { id: meter_def, ty: ty("definition"), parent: Some(library), order: k(2), facets: vec![named("meter")] });
    ops.push(Op::Create { id: meter_in, ty: ty("port_spec"), parent: Some(meter_def), order: k(0), facets: vec![named("in"), port("optical_power", "in", None)] });
    ops.push(Op::Create { id: meter_t, ty: ty("template"), parent: Some(meter_def), order: k(1), facets: vec![template("cartoon")] });
    store.apply(tx("library", ops)).unwrap();
    // The fiber and meter templates draw one bar each; the meter's width
    // reads its in-port.
    let more = store.document().next_ids(2);
    let mut s = Size::default();
    s.w = Slot::Bound(Expr::parse(&format!("{meter_in}.port.signal / 1 mW * 20")).unwrap());
    let fiber_t = more[0];
    store
        .apply(tx(
            "templates",
            vec![
                Op::Create { id: fiber_t, ty: ty("template"), parent: Some(fiber_def), order: k(4), facets: vec![template("cartoon")] },
                Op::Create { id: more[1], ty: ty("rect"), parent: Some(meter_t), order: k(0), facets: vec![Transform2d::default().wrap(), s.wrap()] },
            ],
        ))
        .unwrap();
    let mut entities = Vec::new();
    for (def, name) in [(laser_def, "laser"), (fiber_def, "fiber"), (meter_def, "meter")] {
        let after = store.document().children(model).last().map(|&c| store.document().live(c).unwrap().order.clone());
        let fafter = store.document().children(cartoon).last().map(|&c| store.document().live(c).unwrap().order.clone());
        let place = Placement { entity_parent: Some(model), entity_after: after, figure_parent: Some(cartoon), figure_after: fafter };
        let (e, ops) = definition::instantiate(store.document(), def, "cartoon", name, &place).unwrap();
        store.apply(tx(name, ops)).unwrap();
        entities.push(e);
    }
    let mut lab = Lab { store, laser: entities[0], fiber: entities[1], meter: entities[2], model };
    let (lo, fi, fo, mi) = (lab.port(lab.laser, "out"), lab.port(lab.fiber, "in"), lab.port(lab.fiber, "out"), lab.port(lab.meter, "in"));
    lab.connect(lo, fi).unwrap();
    lab.connect(fo, mi).unwrap();
    lab
}

fn watts(v: &Value) -> f64 {
    match v {
        Value::Quantity { si, .. } => *si,
        other => panic!("not a quantity: {other:?}"),
    }
}

#[test]
fn power_flows_down_the_chain_and_follows_every_parameter() {
    let mut lab = lab();
    // 10 mW through 2 km at 0.5 dB/km: 1 dB down.
    let expected = 0.010 * 10f64.powf(-0.1);
    assert!((watts(&lab.signal(lab.meter, "in")) - expected).abs() < 1e-12);
    // Longer fiber, less power, in the same transaction's resolve.
    lab.store.apply(tx("longer", vec![Op::Set { id: lab.fiber, path: path("args.length_km"), slot: Slot::Const(Value::Number(10.0)) }])).unwrap();
    assert!((watts(&lab.signal(lab.meter, "in")) - 0.010 * 10f64.powf(-0.5)).abs() < 1e-12);
    // The meter's figure reads its port: width = reading / 1 mW · 20.
    let bar = lab.store.document().children(lab.store.document().roots()[1]).iter().copied().find(|&n| composition::figured_by(lab.store.document(), n) == Some(lab.meter)).unwrap();
    let w = lab.store.resolved(bar, path("size.w")).unwrap().as_f64().unwrap();
    assert!((w - 0.010 * 10f64.powf(-0.5) / 0.001 * 20.0).abs() < 1e-9, "{w}");
    // The model view shows the chain.
    let view = lab.store.view(&ViewQuery { scope: None, fidelity: Fidelity::Full, depth: None, relation: ViewRelation::Model }).unwrap();
    assert!(view.contains(r#"a="laser.out" b="fiber.in" kind="optical_power""#), "{view}");
    assert!(view.contains(r#"<port"#) && view.contains("signal="), "{view}");
}

#[test]
fn the_meter_reading_inverts_to_the_laser_power() {
    let mut lab = lab();
    let reading = Leaf { node: lab.port(lab.meter, "in"), path: path("port.signal") };
    match lab.store.handle(reading.node, reading.path) {
        Handle::Linear { source, .. } => assert_eq!(source, Leaf { node: lab.laser, path: path("args.power") }),
        other => panic!("expected a linear handle, got {other:?}"),
    }
    // Ask for 5 mW at the meter: the laser is set to 5 mW / 10^-0.1.
    lab.store.preview_target(reading.node, reading.path, Value::parse_quantity("5 mW").unwrap()).unwrap();
    lab.store.commit_gesture("set reading", Actor::Human, 0).unwrap().unwrap();
    assert!((watts(&lab.store.resolved(lab.laser, path("args.power")).unwrap()) - 0.005 / 10f64.powf(-0.1)).abs() < 1e-12);
    assert!((watts(&lab.signal(lab.meter, "in")) - 0.005).abs() < 1e-12);
}

#[test]
fn connections_must_join_one_kind_one_way_once() {
    let mut lab = lab();
    let meter_in = lab.port(lab.meter, "in");
    let fiber_in = lab.port(lab.fiber, "in");
    let laser_out = lab.port(lab.laser, "out");
    // The meter's input is already fed by the fiber.
    let err = lab.connect(laser_out, meter_in).unwrap_err();
    assert!(err[0].message.contains("already has a connection feeding it"), "{err:?}");
    // Two inputs do not make a one-way connection.
    let err = lab.connect(fiber_in, meter_in).unwrap_err();
    assert!(err[0].message.contains("out port to an in port"), "{err:?}");
    // An rf_voltage output cannot drive an optical input.
    let rf = lab.store.document().next_ids(1)[0];
    let key = OrderKey::after(&lab.store.document().live(laser_out).unwrap().order);
    lab.store.apply(tx("rf port", vec![Op::Create { id: rf, ty: ty("port"), parent: Some(lab.laser), order: key, facets: vec![named("rf"), port("rf_voltage", "out", None)] }])).unwrap();
    let spare = lab.store.document().next_ids(1)[0];
    let key = OrderKey::after(&lab.store.document().live(meter_in).unwrap().order);
    lab.store.apply(tx("spare in", vec![Op::Create { id: spare, ty: ty("port"), parent: Some(lab.meter), order: key, facets: vec![named("in2"), port("optical_power", "in", None)] }])).unwrap();
    let err = lab.connect(rf, spare).unwrap_err();
    assert!(err[0].message.contains("cannot connect rf_voltage"), "{err:?}");
}

/// Across/through pairs (ECM §6.1 step 3): a 5 W heater, a 2 K/W thermal
/// link, and an ambient at 293.15 K. The connections' equations (equal
/// temperature, heat summing to zero) and each part's behaviour equation
/// are solved together: the heater sits at 303.15 K and 5 W flows.
#[test]
fn a_heater_settles_through_its_thermal_link() {
    use iron_document::facet::Equation;
    let mut store = Store::new();
    let q = |s: &str| Value::parse_quantity(s).unwrap();
    let k = |n: u32| (0..n).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let ids = store.document().next_ids(19);
    let [library, model, heater, power, h_out, h_eq, h_t, link, r, l_a, l_b, l_eq1, l_eq2, l_t, ambient, t, a_p, a_eq, a_t] = ids.try_into().unwrap();
    let param = |id, parent, order, name: &str, v: Value| {
        let mut p = Port::default();
        p.kind = Slot::Const(Value::Str("value".into()));
        p.dir = Slot::Const(Value::Enum("in".into()));
        p.signal = Slot::Const(v);
        Op::Create { id, ty: ty("port_spec"), parent: Some(parent), order, facets: vec![p.wrap(), named(name)] }
    };
    let pspec = |id, parent, order, name: &str| Op::Create { id, ty: ty("port_spec"), parent: Some(parent), order, facets: vec![named(name), port("heat_flow", "in", None)] };
    let eq = |id, parent, order, lhs: String, rhs: String| {
        let mut e = Equation::default();
        e.lhs = Slot::Bound(Expr::parse(&lhs).unwrap());
        e.rhs = Slot::Bound(Expr::parse(&rhs).unwrap());
        Op::Create { id, ty: ty("equation"), parent: Some(parent), order, facets: vec![e.wrap()] }
    };
    let template = |id, parent, order| {
        let mut t = Template::default();
        t.notation = Slot::Const(Value::Str("cartoon".into()));
        Op::Create { id, ty: ty("template"), parent: Some(parent), order, facets: vec![t.wrap()] }
    };
    let def = |id, name: &str, order| Op::Create { id, ty: ty("definition"), parent: Some(library), order, facets: vec![named(name)] };
    store
        .apply(tx(
            "thermal library",
            vec![
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(0), facets: vec![named("library")] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: k(1), facets: vec![named("model")] },
                def(heater, "heater", k(0)),
                param(power, heater, k(0), "power", q("5 W")),
                pspec(h_out, heater, k(1), "out"),
                eq(h_eq, heater, k(2), format!("{h_out}.port.through"), format!("-{power}.port.signal")),
                template(h_t, heater, k(3)),
                def(link, "link", k(1)),
                param(r, link, k(0), "r", q("2 K/W")),
                pspec(l_a, link, k(1), "a"),
                pspec(l_b, link, k(2), "b"),
                eq(l_eq1, link, k(3), format!("{l_a}.port.through + {l_b}.port.through"), "0 W".into()),
                eq(l_eq2, link, k(4), format!("{l_a}.port.across - {l_b}.port.across"), format!("{r}.port.signal * {l_a}.port.through")),
                template(l_t, link, k(5)),
                def(ambient, "ambient", k(2)),
                param(t, ambient, k(0), "t", q("293.15 K")),
                pspec(a_p, ambient, k(1), "p"),
                eq(a_eq, ambient, k(2), format!("{a_p}.port.across"), format!("{t}.port.signal")),
                template(a_t, ambient, k(3)),
            ],
        ))
        .unwrap();
    let mut entities = Vec::new();
    for (def, name) in [(heater, "heater"), (link, "link"), (ambient, "ambient")] {
        let after = store.document().children(model).last().map(|&c| store.document().live(c).unwrap().order.clone());
        let place = Placement { entity_parent: Some(model), entity_after: after, figure_parent: None, figure_after: None };
        let (e, ops) = definition::instantiate(store.document(), def, "cartoon", name, &place).unwrap();
        store.apply(tx(name, ops)).unwrap();
        entities.push(e);
    }
    let mut lab = Lab { store, laser: entities[0], fiber: entities[1], meter: entities[2], model };
    let (h, la, lb, amb) = (lab.port(lab.laser, "out"), lab.port(lab.fiber, "a"), lab.port(lab.fiber, "b"), lab.port(lab.meter, "p"));
    lab.connect(h, la).unwrap();
    lab.connect(lb, amb).unwrap();
    let across = |s: &Store, p: NodeId| watts(&s.resolved(p, path("port.across")).unwrap());
    let through = |s: &Store, p: NodeId| watts(&s.resolved(p, path("port.through")).unwrap());
    assert_eq!(lab.store.physics_error(), None);
    assert!((across(&lab.store, h) - 303.15).abs() < 1e-6, "{}", across(&lab.store, h));
    assert!((through(&lab.store, la) - 5.0).abs() < 1e-6, "{}", through(&lab.store, la));
    // More power, hotter heater, in the same transaction.
    lab.store.apply(tx("10 W", vec![Op::Set { id: lab.laser, path: path("args.power"), slot: Slot::Const(q("10 W")) }])).unwrap();
    assert!((across(&lab.store, h) - 313.15).abs() < 1e-6, "{}", across(&lab.store, h));
    // A solved value is not edited directly.
    assert!(lab.store.handle(h, path("port.across")).is_locked());
    // The model view shows the pair.
    let view = lab.store.view(&ViewQuery { scope: None, fidelity: Fidelity::Full, depth: None, relation: ViewRelation::Model }).unwrap();
    assert!(view.contains(r#"kind="heat_flow""#) && view.contains("across=") && view.contains("through="), "{view}");
}

/// The entity's first figure; the lab's fiber template draws nothing, so
/// the fiber gets a plain rect for one.
fn figure_of_entity(store: &mut Store, entity: NodeId) -> NodeId {
    if let Some(&f) = composition::figures_of(store.document(), entity).first() {
        return f;
    }
    let doc = store.document();
    let cartoon = doc.roots()[1];
    let id = doc.next_ids(1)[0];
    let rel = doc.next_relation_id();
    let order = OrderKey::after(&doc.live(*doc.children(cartoon).last().unwrap()).unwrap().order);
    store
        .apply(tx(
            "figure",
            vec![
                Op::Create { id, ty: ty("rect"), parent: Some(cartoon), order, facets: vec![Transform2d::default().wrap(), Size::default().wrap()] },
                Op::Link { rel: Relation { id: rel, from: id, to: entity, rel: RelKind::Figures, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    id
}

#[test]
fn copying_a_figure_copies_its_entity_with_fresh_ports() {
    let mut lab = lab();
    let fiber_box = figure_of_entity(&mut lab.store, lab.fiber);
    let pasted = copy::duplicate(&lab.store, &[fiber_box]).unwrap();
    // Both of the fiber's connections reach outside: neither is copied.
    assert_eq!(pasted.notes.len(), 2, "{:?}", pasted.notes);
    lab.store.apply(tx("duplicate", pasted.ops)).unwrap();
    let copy = pasted.roots[0];
    let doc = lab.store.document();
    let entity = composition::figured_by(doc, copy).unwrap();
    assert_ne!(entity, lab.fiber);
    // A second fiber in the model, right after the first, with its own ports.
    let model: Vec<NodeId> = doc.children(lab.model).to_vec();
    assert_eq!(model[model.iter().position(|&e| e == lab.fiber).unwrap() + 1], entity);
    let ports: Vec<NodeId> = doc.children(entity).to_vec();
    assert_eq!(ports.len(), doc.children(lab.fiber).len());
    assert!(ports.iter().all(|p| !doc.children(lab.fiber).contains(p)));
    // Its out-port's behaviour reads its own parameters and in-port.
    let out = lab.port(entity, "out");
    let expr = doc.leaf(out, path("port.signal")).unwrap().expr().unwrap().clone();
    assert!(expr.deps().iter().all(|d| d.node == entity || ports.contains(&d.node)), "{expr:?}");
    // Unconnected, it carries nothing; the original chain is untouched.
    let expected = 0.010 * 10f64.powf(-0.1);
    assert!((watts(&lab.signal(lab.meter, "in")) - expected).abs() < 1e-12);
    // Its parameters are its own.
    lab.store.apply(tx("longer", vec![Op::Set { id: entity, path: path("args.length_km"), slot: Slot::Const(Value::Number(10.0)) }])).unwrap();
    assert!((watts(&lab.signal(lab.meter, "in")) - expected).abs() < 1e-12);
}

#[test]
fn copying_a_whole_chain_keeps_its_connections() {
    let mut lab = lab();
    let figures: Vec<NodeId> = [lab.laser, lab.fiber, lab.meter].iter().map(|&e| figure_of_entity(&mut lab.store, e)).collect();
    let pasted = copy::duplicate(&lab.store, &figures).unwrap();
    assert!(pasted.notes.is_empty(), "{:?}", pasted.notes);
    let connections = lab.store.document().nodes_of_type(ty("connection")).count();
    lab.store.apply(tx("duplicate", pasted.ops)).unwrap();
    let doc = lab.store.document();
    assert_eq!(doc.nodes_of_type(ty("connection")).count(), connections * 2);
    // The copied meter reads the copied laser: raise it, only its meter moves.
    let def = |e: NodeId| doc.get::<iron_document::facet::Instance>(e).unwrap().def.constant().cloned();
    let copy_of = |original: NodeId| pasted.roots.iter().map(|&r| composition::figured_by(doc, r).unwrap()).find(|&e| def(e) == def(original)).unwrap();
    let (laser, meter) = (copy_of(lab.laser), copy_of(lab.meter));
    assert!(laser != lab.laser && meter != lab.meter);
    lab.store.apply(tx("brighter", vec![Op::Set { id: laser, path: path("args.power"), slot: Slot::Const(Value::parse_quantity("20 mW").unwrap()) }])).unwrap();
    assert!((watts(&lab.signal(meter, "in")) - 0.020 * 10f64.powf(-0.1)).abs() < 1e-12);
    assert!((watts(&lab.signal(lab.meter, "in")) - 0.010 * 10f64.powf(-0.1)).abs() < 1e-12);
}

#[test]
fn pasting_after_a_cut_relinks_the_figure_to_its_entity() {
    let mut lab = lab();
    let fiber_box = figure_of_entity(&mut lab.store, lab.fiber);
    let frag = copy::fragment(&lab.store, &[fiber_box], "here").unwrap();
    lab.store.apply(tx("cut", vec![Op::Delete { id: fiber_box }])).unwrap();
    let place = copy::Placement { share_entities: true, ..Default::default() };
    let pasted = copy::paste(&lab.store, &frag, &place, "here").unwrap();
    lab.store.apply(tx("paste", pasted.ops)).unwrap();
    // The same fiber, still connected: no second entity.
    let doc = lab.store.document();
    assert_eq!(composition::figured_by(doc, pasted.roots[0]), Some(lab.fiber));
    assert_eq!(doc.nodes_of_type(ty("entity")).count(), 3);
}
