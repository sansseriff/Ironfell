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
        let doc = self.store.tree();
        doc.children(entity).iter().copied().find(|&p| doc.get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
    }
    fn signal(&self, entity: NodeId, name: &str) -> Value {
        self.store.resolved(self.port(entity, name), path("port.signal")).unwrap()
    }
    fn connect(&mut self, from: NodeId, to: NodeId) -> Result<Applied, Vec<ApplyError>> {
        let id = self.store.authored().next_ids(1)[0];
        let rels = self.store.authored().next_relation_ids(2);
        let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
        let key = self.store.tree().children(self.model).last().map(|&c| OrderKey::after(&self.store.tree().live(c).unwrap().order)).unwrap_or_else(OrderKey::first);
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
    let ids = store.authored().next_ids(16);
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
    let more = store.authored().next_ids(2);
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
        let after = store.tree().children(model).last().map(|&c| store.tree().live(c).unwrap().order.clone());
        let fafter = store.tree().children(cartoon).last().map(|&c| store.tree().live(c).unwrap().order.clone());
        let place = Placement { entity_parent: Some(model), entity_after: after, figure_parent: Some(cartoon), figure_after: fafter };
        let (e, ops) = definition::instantiate(store.tree(), def, "cartoon", name, &place).unwrap();
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
    // The meter's figure (a `use`, its rect expanded from the template)
    // reads its port: width = reading / 1 mW · 20.
    let u = lab.store.tree().children(lab.store.tree().roots()[1]).iter().copied().find(|&n| composition::figured_by(lab.store.tree(), n) == Some(lab.meter)).unwrap();
    let bar = lab.store.tree().children(u)[0];
    assert!(bar.is_expanded());
    let w = lab.store.resolved(bar, path("size.w")).unwrap().as_f64().unwrap();
    assert!((w - 0.010 * 10f64.powf(-0.5) / 0.001 * 20.0).abs() < 1e-9, "{w}");
    // The model view shows the chain.
    let view = lab.store.view(&ViewQuery { connections: Vec::new(), omit_defaults: false, expand: Vec::new(), scope: None, fidelity: Fidelity::Full, depth: None, relation: ViewRelation::Model }).unwrap();
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
    let rf = lab.store.authored().next_ids(1)[0];
    let key = OrderKey::after(&lab.store.tree().live(laser_out).unwrap().order);
    lab.store.apply(tx("rf port", vec![Op::Create { id: rf, ty: ty("port"), parent: Some(lab.laser), order: key, facets: vec![named("rf"), port("rf_voltage", "out", None)] }])).unwrap();
    let spare = lab.store.authored().next_ids(1)[0];
    let key = OrderKey::after(&lab.store.tree().live(meter_in).unwrap().order);
    lab.store.apply(tx("spare in", vec![Op::Create { id: spare, ty: ty("port"), parent: Some(lab.meter), order: key, facets: vec![named("in2"), port("optical_power", "in", None)] }])).unwrap();
    let err = lab.connect(rf, spare).unwrap_err();
    assert!(err[0].message.contains("cannot connect rf_voltage"), "{err:?}");
}

/// Across/through pairs (ECM §6.1 step 3): a 5 W heater, a 2 K/W thermal
/// link, and an ambient at 293.15 K. The connections' equations (equal
/// temperature, heat summing to zero) and each part's behaviour equation
/// are solved together: the heater sits at 303.15 K and 5 W flows.
/// A thermal library (a heater, a link, an ambient) and a model, as
/// definitions: `(store, model, [heater, link, ambient])`.
fn thermal() -> (Store, NodeId, [NodeId; 3]) {
    use iron_document::facet::Equation;
    let mut store = Store::new();
    let q = |s: &str| Value::parse_quantity(s).unwrap();
    let k = |n: u32| (0..n).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let ids = store.authored().next_ids(19);
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
    (store, model, [heater, link, ambient])
}

/// An instance of `def` in `model`, by its entity.
fn place(store: &mut Store, model: NodeId, def: NodeId, name: &str) -> NodeId {
    let after = store.tree().children(model).last().map(|&c| store.tree().live(c).unwrap().order.clone());
    let place = Placement { entity_parent: Some(model), entity_after: after, figure_parent: None, figure_after: None };
    let (e, ops) = definition::instantiate(store.tree(), def, "cartoon", name, &place).unwrap();
    store.apply(tx(name, ops)).unwrap();
    e
}

#[test]
fn a_heater_settles_through_its_thermal_link() {
    let (mut store, model, [heater, link, ambient]) = thermal();
    let q = |s: &str| Value::parse_quantity(s).unwrap();
    let entities: Vec<NodeId> = [(heater, "heater"), (link, "link"), (ambient, "ambient")].into_iter().map(|(d, n)| place(&mut store, model, d, n)).collect();
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
    let view = lab.store.view(&ViewQuery { connections: Vec::new(), omit_defaults: false, expand: Vec::new(), scope: None, fidelity: Fidelity::Full, depth: None, relation: ViewRelation::Model }).unwrap();
    assert!(view.contains(r#"kind="heat_flow""#) && view.contains("across=") && view.contains("through="), "{view}");
}

/// Islands are solved apart (reactive-engine handoff §4.10, RE-5.3): a
/// circuit that cannot settle keeps its last solution and says so, and a
/// circuit beside it still follows its parameters.
#[test]
fn a_circuit_that_cannot_settle_does_not_hold_back_another() {
    let (mut store, model, [heater, link, ambient]) = thermal();
    let q = |s: &str| Value::parse_quantity(s).unwrap();
    let [h, l, a] = [(heater, "heater"), (link, "link"), (ambient, "ambient")].map(|(d, n)| place(&mut store, model, d, n));
    let [c1, c2] = [ambient, ambient].map(|d| place(&mut store, model, d, "cold"));
    let mut lab = Lab { store, laser: h, fiber: l, meter: a, model };
    let (h_out, la, lb, amb) = (lab.port(h, "out"), lab.port(l, "a"), lab.port(l, "b"), lab.port(a, "p"));
    lab.connect(h_out, la).unwrap();
    lab.connect(lb, amb).unwrap();
    assert_eq!(lab.store.physics_error(), None);
    let across = |s: &Store, p: NodeId| watts(&s.resolved(p, path("port.across")).unwrap());
    assert!((across(&lab.store, h_out) - 303.15).abs() < 1e-6);
    // Two ambients joined at one temperature each: no solution.
    lab.store.apply(tx("colder", vec![Op::Set { id: c2, path: path("args.t"), slot: Slot::Const(q("250 K")) }])).unwrap();
    let (p1, p2) = (lab.port(c1, "p"), lab.port(c2, "p"));
    let _ = lab.connect(p1, p2);
    assert!(lab.store.physics_error().is_some_and(|e| e.contains("does not settle")), "{:?}", lab.store.physics_error());
    // The heater's circuit still follows its power.
    lab.store.apply(tx("10 W", vec![Op::Set { id: h, path: path("args.power"), slot: Slot::Const(q("10 W")) }])).unwrap();
    assert!((across(&lab.store, h_out) - 313.15).abs() < 1e-6, "{}", across(&lab.store, h_out));
    // Undone, the power and the joint: the islands split again and settle.
    lab.store.undo(0).unwrap().unwrap();
    lab.store.undo(0).unwrap().unwrap();
    assert_eq!(lab.store.physics_error(), None);
    assert!((across(&lab.store, h_out) - 303.15).abs() < 1e-6, "{}", across(&lab.store, h_out));
    assert!((across(&lab.store, p2) - 250.0).abs() < 1e-6, "{}", across(&lab.store, p2));
}

/// Each island is a native cell (reactive-engine handoff §4.10, RE-5.3):
/// a parameter of one circuit solves that circuit and no other.
#[test]
fn an_edit_in_one_circuit_solves_no_other() {
    let (mut store, model, [heater, link, ambient]) = thermal();
    let q = |s: &str| Value::parse_quantity(s).unwrap();
    let one = [(heater, "heater"), (link, "link"), (ambient, "ambient")].map(|(d, n)| place(&mut store, model, d, n));
    let two = [(heater, "heater"), (link, "link"), (ambient, "ambient")].map(|(d, n)| place(&mut store, model, d, n));
    let mut lab = Lab { store, laser: one[0], fiber: one[1], meter: one[2], model };
    for [h, l, a] in [one, two] {
        let (h_out, la, lb, amb) = (lab.port(h, "out"), lab.port(l, "a"), lab.port(l, "b"), lab.port(a, "p"));
        lab.connect(h_out, la).unwrap();
        lab.connect(lb, amb).unwrap();
    }
    let (out1, out2) = (lab.port(one[0], "out"), lab.port(two[0], "out"));
    let across = |s: &Store, p: NodeId| watts(&s.resolved(p, path("port.across")).unwrap());
    let runs = |s: &Store, p: NodeId| s.reactive().native_runs(Leaf { node: p, path: path("port.across") }).expect("solved by a native");
    let (r1, r2) = (runs(&lab.store, out1), runs(&lab.store, out2));
    lab.store.apply(tx("10 W", vec![Op::Set { id: one[0], path: path("args.power"), slot: Slot::Const(q("10 W")) }])).unwrap();
    assert!((across(&lab.store, out1) - 313.15).abs() < 1e-6, "{}", across(&lab.store, out1));
    assert!((across(&lab.store, out2) - 303.15).abs() < 1e-6, "{}", across(&lab.store, out2));
    assert!(runs(&lab.store, out1) > r1, "the edited circuit solved");
    assert_eq!(runs(&lab.store, out2), r2, "the other did not");
}

/// The entity's first figure; the lab's fiber template draws nothing, so
/// the fiber gets a plain rect for one.
fn figure_of_entity(store: &mut Store, entity: NodeId) -> NodeId {
    if let Some(&f) = composition::figures_of(store.tree(), entity).first() {
        return f;
    }
    let doc = store.tree();
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
    let doc = lab.store.tree();
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
    let connections = lab.store.tree().nodes_of_type(ty("connection")).count();
    lab.store.apply(tx("duplicate", pasted.ops)).unwrap();
    let doc = lab.store.tree();
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
    let doc = lab.store.tree();
    assert_eq!(composition::figured_by(doc, pasted.roots[0]), Some(lab.fiber));
    assert_eq!(doc.nodes_of_type(ty("entity")).count(), 3);
}

/// Detaching a linked instance (component-model.md §5.2): its ports and
/// picture become authored copies, the connection into it follows its port,
/// the reading is unchanged, and editing the definition no longer reaches
/// it. Undo links it again.
#[test]
fn a_detached_meter_keeps_its_wiring_and_stops_following_its_definition() {
    let mut lab = lab();
    let doc = lab.store.tree();
    let meter_in = lab.port(lab.meter, "in");
    assert!(meter_in.is_expanded());
    let reading = watts(&lab.signal(lab.meter, "in"));
    let u = doc.relations_to(lab.meter).find(|r| r.rel == RelKind::Figures).unwrap().from;
    let bar = doc.children(u)[0];
    let (_, template_bar) = bar.expanded_parts().unwrap();
    let ops = definition::detach_ops(lab.store.tree(), lab.meter).unwrap();
    lab.store.apply(tx("detach meter", ops)).unwrap();
    let doc = lab.store.tree();
    // Authored now: a port and a picture with fresh ids.
    let port = lab.port(lab.meter, "in");
    assert!(!port.is_expanded() && port != meter_in);
    let copy = doc.children(u)[0];
    assert!(!copy.is_expanded());
    assert!(doc.live(meter_in).is_none() && doc.live(bar).is_none());
    // The connection feeds the copy; the reading and the picture's width hold.
    assert!(doc.relations_to(port).any(|r| r.rel == RelKind::Connects));
    assert!((watts(&lab.signal(lab.meter, "in")) - reading).abs() < 1e-15);
    let w = lab.store.resolved(copy, path("size.w")).unwrap().as_f64().unwrap();
    assert!((w - reading / 0.001 * 20.0).abs() < 1e-9, "{w}");
    // Editing the definition's picture no longer reaches it.
    lab.store.apply(tx("taller", vec![Op::Set { id: template_bar, path: path("size.h"), slot: Slot::Const(Value::Number(99.0)) }])).unwrap();
    assert_ne!(lab.store.resolved(copy, path("size.h")), Some(Value::Number(99.0)));
    // Undo (twice: the edit, then the detach) links it again.
    lab.store.undo(1).unwrap().unwrap();
    lab.store.undo(2).unwrap().unwrap();
    let doc = lab.store.tree();
    assert!(doc.live(meter_in).is_some() && doc.live(port).is_none());
    assert!(doc.relations_to(meter_in).any(|r| r.rel == RelKind::Connects));
    assert!((watts(&lab.signal(lab.meter, "in")) - reading).abs() < 1e-15);
}

/// Make component (component-model.md §5.5; ECM §9's black-boxing law): the
/// laser and the fiber, the laser's power bound to a slider outside, become a
/// "source" component. The meter's reading is unchanged; the slider still
/// drives it, through the source's new `power` port; the fiber's light leaves
/// through its `out` port. The inner laser and fiber stay linked to their own
/// definitions, so editing the laser's picture reaches the laser inside every
/// source. Undo restores the selection.
#[test]
fn making_a_component_keeps_every_signal_and_nests_live_instances() {
    let mut lab = lab();
    let cartoon = lab.store.tree().roots()[1];
    let picture_of = |s: &Store, e: NodeId| s.tree().relations_to(e).find(|r| r.rel == RelKind::Figures).unwrap().from;
    let (laser_use, fiber_use) = (picture_of(&lab.store, lab.laser), picture_of(&lab.store, lab.fiber));
    // A slider outside drives the laser's power.
    let slider = lab.store.authored().next_id();
    let mut sl = iron_document::facet::Slider::default();
    sl.value = Slot::Const(Value::Number(0.01));
    let key = OrderKey::after(&lab.store.tree().live(*lab.store.tree().children(cartoon).last().unwrap()).unwrap().order);
    lab.store.apply(tx("slider", vec![
        Op::Create { id: slider, ty: ty("slider"), parent: Some(cartoon), order: key, facets: vec![Transform2d::default().wrap(), sl.wrap()] },
        Op::Set { id: lab.laser, path: path("args.power"), slot: Slot::Bound(Expr::parse(&format!("{slider}.slider.value * 1 W")).unwrap()) },
    ])).unwrap();
    let reading = watts(&lab.signal(lab.meter, "in"));
    assert!((reading - 0.010 * 10f64.powf(-0.1)).abs() < 1e-12);

    let (made, input) = lab.store.make_component(&[laser_use, fiber_use], "source", Actor::Human).unwrap();
    lab.store.apply(input).unwrap();
    let doc = lab.store.tree();
    assert!(doc.live(lab.laser).is_none() && doc.live(lab.fiber).is_none() && doc.live(laser_use).is_none());
    // Derived relations (the inner connection, `exposes`) leave the
    // allocator alone: a fresh relation is still an authored one.
    assert!(!doc.next_relation_id().is_expanded());
    // Black-boxing: the meter reads what it read.
    assert!((watts(&lab.signal(lab.meter, "in")) - reading).abs() < 1e-15, "{}", watts(&lab.signal(lab.meter, "in")));
    // The source has a power input bound to the slider and a light output.
    let port = |name: &str| lab.port(made.entity, name);
    assert_eq!(lab.store.tree().leaf(made.entity, path("args.power")).unwrap().expr().unwrap().to_string(), format!("{slider}.slider.value * 1 W"));
    assert!((watts(&lab.signal(made.entity, "out")) - reading).abs() < 1e-15);
    assert!(port("power").is_expanded());
    // The model view lists the source, not its insides.
    let view = lab.store.view(&ViewQuery { connections: Vec::new(), omit_defaults: false, expand: Vec::new(), scope: None, fidelity: Fidelity::Full, depth: None, relation: ViewRelation::Model }).unwrap();
    assert!(view.contains(r#"name="source""#), "{view}");
    assert!(!view.contains(r#"name="laser""#), "{view}");
    // The slider still drives the meter, through the source.
    lab.store.apply(tx("brighter", vec![Op::Set { id: slider, path: path("slider.value"), slot: Slot::Const(Value::Number(0.02)) }])).unwrap();
    assert!((watts(&lab.signal(lab.meter, "in")) - 2.0 * reading).abs() < 1e-15);
    // Nested and live: the source's picture holds the laser's picture, and
    // editing the laser definition's box reaches it.
    let laser_def = made.definition;
    let _ = laser_def;
    let source_picture = made.picture;
    let inner_pictures: Vec<NodeId> = lab.store.tree().children(source_picture).to_vec();
    assert_eq!(inner_pictures.len(), 2, "{inner_pictures:?}");
    let laser_box = lab.store.tree().children(inner_pictures[0])[0];
    assert_eq!(laser_box.path().len(), 3, "{laser_box}");
    let (_, template_box) = laser_box.expanded_parts().unwrap();
    lab.store.apply(tx("taller laser", vec![Op::Set { id: template_box, path: path("size.h"), slot: Slot::Const(Value::Number(77.0)) }])).unwrap();
    assert_eq!(lab.store.resolved(laser_box, path("size.h")), Some(Value::Number(77.0)));
    // A part of the nested laser is overridden from the outermost instance,
    // its target the path down to the part, and the override outlives an
    // edit to the laser definition (§5.3, "Nested targets").
    lab.store.apply(tx("wider laser here", vec![Op::Set { id: laser_box, path: path("size.w"), slot: Slot::Const(Value::Number(140.0)) }])).unwrap();
    let doc = lab.store.tree();
    let o = doc.children(source_picture).iter().copied().find(|&c| doc.live(c).unwrap().ty.name() == "override").expect("override on the source's picture");
    let target = doc.get::<iron_document::facet::Override>(o).unwrap().target.constant().unwrap().as_ref().unwrap();
    assert_eq!(target.path(), laser_box.path()[1..].to_vec());
    lab.store.apply(tx("wider lasers", vec![Op::Set { id: template_box, path: path("size.w"), slot: Slot::Const(Value::Number(90.0)) }])).unwrap();
    assert_eq!(lab.store.resolved(laser_box, path("size.w")), Some(Value::Number(140.0)));
    assert!(lab.store.stale_overrides().is_empty(), "{:?}", lab.store.stale_overrides());
    // Saved and loaded, the nesting re-derives with the same ids.
    let text = lab.store.save();
    let loaded = Store::load(&text).unwrap();
    assert_eq!(loaded.resolved(laser_box, path("size.h")), Some(Value::Number(77.0)));
    assert_eq!(loaded.resolved(laser_box, path("size.w")), Some(Value::Number(140.0)));
    assert_eq!(loaded.save(), text);
    // Undo (two widths, the edit, the slider move, then the make) restores
    // the selection.
    lab.store.undo(1).unwrap().unwrap();
    lab.store.undo(1).unwrap().unwrap();
    lab.store.undo(1).unwrap().unwrap();
    lab.store.undo(1).unwrap().unwrap();
    lab.store.undo(1).unwrap().unwrap();
    let doc = lab.store.tree();
    assert!(doc.live(lab.laser).is_some() && doc.live(made.entity).is_none());
    assert!((watts(&lab.signal(lab.meter, "in")) - reading).abs() < 1e-15);
}

fn key(n: u32) -> OrderKey {
    (0..n).fold(OrderKey::first(), |k, _| OrderKey::after(&k))
}

/// EN-2c: `NetworkMakers` runs per top-level root, `deref` in one model's
/// equation does not merge an unrelated model's island, constant edits on an
/// equation solve without replacing islands or waking other models, and
/// non-structural `port.dir` edits update derived one-way signals via
/// `sync_touched`.
#[test]
fn independent_models_and_deref_equations_stay_isolated() {
    use iron_document::facet::{Args, Connection, Equation, Instance, Let, Port, Size, Transform2d};
    let mut store = Store::new();
    let ids = store.authored().next_ids(15);
    let [lib, def, m1, m2, comp, w_let, h_rect, e1, p1, eq1, e2, p2, eq2, p_sig_a, p_sig_b] = ids.try_into().unwrap();
    let elec = |name: &str| vec![
        Port { kind: Slot::Const(Value::Str("electrical".into())), ..Default::default() }.wrap(),
        named(name),
    ];
    let val_port = |name: &str, dir: &str, sig: f64| vec![
        Port {
            kind: Slot::Const(Value::Str("value".into())),
            dir: Slot::Const(Value::Enum(dir.into())),
            signal: Slot::Const(Value::Number(sig)),
            ..Default::default()
        }
        .wrap(),
        named(name),
    ];
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut at = Transform2d::default();
    at.x = Slot::Const(Value::Number(7.0));
    let mut w = Let::default();
    w.value = Slot::Const(Value::Ref(h_rect));
    let bound = |s: String| Slot::Bound(Expr::parse(&s).unwrap());
    let mut eq1_f = Equation::default();
    eq1_f.lhs = bound(format!("{p1}.port.across"));
    eq1_f.rhs = bound(format!("deref({w_let}.let.value, 'transform2d.x', 0) * (1 V)"));
    let mut eq2_f = Equation::default();
    eq2_f.lhs = bound(format!("{p2}.port.across"));
    eq2_f.rhs = Slot::Const(Value::parse_quantity("3 V").unwrap());
    let mut inst = Instance::default();
    inst.def = Slot::Const(Value::Ref(def));
    store
        .apply(tx(
            "two models",
            vec![
                Op::Create { id: lib, ty: ty("library"), parent: None, order: key(0), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(lib), order: key(0), facets: vec![named("part")] },
                Op::Create { id: m1, ty: ty("model"), parent: None, order: key(1), facets: vec![named("m1")] },
                Op::Create { id: m2, ty: ty("model"), parent: None, order: key(2), facets: vec![named("m2")] },
                Op::Create { id: comp, ty: ty("composition"), parent: None, order: key(3), facets: vec![named("c"), c.wrap()] },
                Op::Create { id: w_let, ty: ty("let"), parent: Some(comp), order: key(0), facets: vec![named("w"), w.wrap()] },
                Op::Create { id: h_rect, ty: ty("rect"), parent: Some(comp), order: key(1), facets: vec![at.wrap(), Size::default().wrap()] },
                Op::Create { id: e1, ty: ty("entity"), parent: Some(m1), order: key(0), facets: vec![inst.clone().wrap(), Args::default().wrap()] },
                Op::Create { id: p1, ty: ty("port"), parent: Some(e1), order: key(0), facets: elec("p1") },
                Op::Create { id: eq1, ty: ty("equation"), parent: Some(e1), order: key(1), facets: vec![eq1_f.wrap()] },
                Op::Create { id: e2, ty: ty("entity"), parent: Some(m2), order: key(0), facets: vec![inst.wrap(), Args::default().wrap()] },
                Op::Create { id: p2, ty: ty("port"), parent: Some(e2), order: key(0), facets: elec("p2") },
                Op::Create { id: eq2, ty: ty("equation"), parent: Some(e2), order: key(1), facets: vec![eq2_f.wrap()] },
                Op::Create { id: p_sig_a, ty: ty("port"), parent: Some(e1), order: key(2), facets: val_port("sa", "out", 42.0) },
                Op::Create { id: p_sig_b, ty: ty("port"), parent: Some(e1), order: key(3), facets: val_port("sb", "in", 0.0) },
            ],
        ))
        .unwrap();

    // Scoped deref: `eq1` in `m1` uses `deref(...)`, yet `m1` and `m2` stay
    // separate solver islands (`m1` has 1 island over `{p1.across, p1.through}`,
    // `m2` has 2 islands `{p2.across}` and `{p2.through}`, instead of 1 merged).
    let net = iron_document::port::network(store.tree());
    let islands = net.islands();
    assert_eq!(islands.len(), 3, "deref in m1 must not merge m2's islands");
    for island in &islands {
        let roots: std::collections::BTreeSet<_> = island.unknowns.iter().map(|&i| store.tree().top_root(net.unknowns[i].0.node)).collect();
        assert_eq!(roots.len(), 1, "each island must stay inside one model root: {roots:?}");
    }
    let volts = |s: &Store, p: NodeId| s.resolved(p, path("port.across")).unwrap().as_quantity().unwrap().0;
    assert!((volts(&store, p1) - 7.0).abs() < 1e-6);
    assert!((volts(&store, p2) - 3.0).abs() < 1e-6);

    // Editing m2's equation constant updates p2 without touching m1's network maker.
    let m1_runs = store.network_maker_runs(m1);
    let m2_runs = store.network_maker_runs(m2);
    store
        .apply(tx("set eq2", vec![Op::Set { id: eq2, path: path("equation.rhs"), slot: Slot::Const(Value::parse_quantity("9 V").unwrap()) }]))
        .unwrap();
    assert!((volts(&store, p2) - 9.0).abs() < 1e-6);
    assert_eq!(store.network_maker_runs(m1), m1_runs, "m1 network maker must not run on m2 edit");
    assert!(store.network_maker_runs(m2) >= m2_runs);

    // Connecting a one-way signal in m1 does not wake m2's derived-binding maker,
    // and flipping `port.dir` via non-structural Op::Set updates the derived
    // signal through `sync_touched`.
    let conn = store.authored().next_id();
    let rels = store.authored().next_relation_ids(2);
    let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
    let m2_derived = store.derived_maker_runs(m2);
    store
        .apply(tx(
            "wire signal",
            vec![
                Op::Create { id: conn, ty: ty("connection"), parent: Some(m1), order: key(5), facets: vec![Connection::default().wrap()] },
                Op::Link { rel: Relation { id: rels[0], from: conn, to: p_sig_a, rel: RelKind::Connects, data: end("a") } },
                Op::Link { rel: Relation { id: rels[1], from: conn, to: p_sig_b, rel: RelKind::Connects, data: end("b") } },
            ],
        ))
        .unwrap();
    assert_eq!(store.derived_maker_runs(m2), m2_derived, "m2 derived maker must not run on m1 connection");
    assert_eq!(store.resolved(p_sig_b, path("port.signal")), Some(Value::Number(42.0)));

    // Reverse direction with pure constant Sets (`sync_touched`, not `regraph`).
    store
        .apply(tx(
            "reverse dir",
            vec![
                Op::Set { id: p_sig_a, path: path("port.dir"), slot: Slot::Const(Value::Enum("in".into())) },
                Op::Set { id: p_sig_b, path: path("port.dir"), slot: Slot::Const(Value::Enum("out".into())) },
                Op::Set { id: p_sig_b, path: path("port.signal"), slot: Slot::Const(Value::Number(13.0)) },
            ],
        ))
        .unwrap();
    assert_eq!(store.derived_maker_runs(m2), m2_derived, "m2 derived maker must stay quiet");
    assert_eq!(store.resolved(p_sig_a, path("port.signal")), Some(Value::Number(13.0)));
}

/// EN-2c canary: silencing `insert_relation` stops `Key::Relations(conn)` from
/// waking `NetworkMakers` when two pair ports are connected, and the oracle catches it.
#[cfg(feature = "structure-oracle")]
#[test]
#[should_panic(expected = "structure oracle")]
fn canary_a_silenced_connection_link_fails_the_network_oracle() {
    use iron_document::facet::{Args, Connection, Instance, Port};
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            iron_document::silence_for_canary(None);
        }
    }
    let _reset = Reset;
    let mut store = Store::new();
    let ids = store.authored().next_ids(7);
    let [lib, def, model, entity, pa, pb, conn] = ids.try_into().unwrap();
    let elec = |name: &str| vec![Port { kind: Slot::Const(Value::Str("electrical".into())), ..Default::default() }.wrap(), named(name)];
    let mut inst = Instance::default();
    inst.def = Slot::Const(Value::Ref(def));
    store
        .apply(tx(
            "setup",
            vec![
                Op::Create { id: lib, ty: ty("library"), parent: None, order: key(0), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(lib), order: key(0), facets: vec![named("part")] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: key(1), facets: vec![] },
                Op::Create { id: entity, ty: ty("entity"), parent: Some(model), order: key(0), facets: vec![inst.wrap(), Args::default().wrap()] },
                Op::Create { id: pa, ty: ty("port"), parent: Some(entity), order: key(0), facets: elec("a") },
                Op::Create { id: pb, ty: ty("port"), parent: Some(entity), order: key(1), facets: elec("b") },
                Op::Create { id: conn, ty: ty("connection"), parent: Some(model), order: key(2), facets: vec![Connection::default().wrap()] },
            ],
        ))
        .unwrap();
    let rels = store.authored().next_relation_ids(2);
    let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
    iron_document::silence_for_canary(Some("insert_relation"));
    store
        .apply(tx(
            "connect",
            vec![
                Op::Link { rel: Relation { id: rels[0], from: conn, to: pa, rel: RelKind::Connects, data: end("a") } },
                Op::Link { rel: Relation { id: rels[1], from: conn, to: pb, rel: RelKind::Connects, data: end("b") } },
            ],
        ))
        .unwrap();
}
