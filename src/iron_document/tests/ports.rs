//! Ports, connections and one-way signals (ECM §6, §6.1): laser → fiber →
//! power meter. Signals ride the reactive graph; an in-port's is derived
//! from its connection; a reading inverts back to its source.

use iron_document::component::{Composition, Connection, Name, ParamSpec, Port, Size, Template, Transform2d};
use iron_document::definition::{self, Placement};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn named(s: &str) -> Component {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(s.into()));
    n.wrap()
}

fn port(kind: &str, dir: &str, signal: Option<String>) -> Component {
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
                Op::Create { id, ty: ty("connection"), parent: Some(self.model), order: key, components: vec![Connection::default().wrap()] },
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
        let mut p = ParamSpec::default();
        p.kind = Slot::Const(Value::Enum(match &default {
            Value::Quantity { .. } => "quantity".into(),
            _ => "number".into(),
        }));
        p.default = Slot::Const(default);
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
        Op::Create { id: library, ty: ty("library"), parent: None, order: k(0), components: vec![named("library")] },
        Op::Create { id: cartoon, ty: ty("composition"), parent: None, order: k(1), components: vec![named("cartoon"), comp.wrap()] },
        Op::Create { id: model, ty: ty("model"), parent: None, order: k(2), components: vec![named("model")] },
        Op::Create { id: laser_def, ty: ty("definition"), parent: Some(library), order: k(0), components: vec![named("laser")] },
    ];
    let (pid, pc) = param(power, "power", Value::parse_quantity("10 mW").unwrap());
    ops.push(Op::Create { id: pid, ty: ty("param_spec"), parent: Some(laser_def), order: k(0), components: pc });
    ops.push(Op::Create { id: laser_out, ty: ty("port_spec"), parent: Some(laser_def), order: k(1), components: vec![named("out"), port("optical_power", "out", Some(format!("{power}.param_spec.default")))] });
    ops.push(Op::Create { id: laser_t, ty: ty("template"), parent: Some(laser_def), order: k(2), components: vec![template("cartoon")] });
    ops.push(Op::Create { id: laser_box, ty: ty("rect"), parent: Some(laser_t), order: k(0), components: vec![Transform2d::default().wrap(), Size::default().wrap()] });
    ops.push(Op::Create { id: fiber_def, ty: ty("definition"), parent: Some(library), order: k(1), components: vec![named("fiber")] });
    let (lid, lc) = param(loss, "loss_db_per_km", Value::Number(0.5));
    ops.push(Op::Create { id: lid, ty: ty("param_spec"), parent: Some(fiber_def), order: k(0), components: lc });
    let (nid, nc) = param(length, "length_km", Value::Number(2.0));
    ops.push(Op::Create { id: nid, ty: ty("param_spec"), parent: Some(fiber_def), order: k(1), components: nc });
    ops.push(Op::Create { id: fiber_in, ty: ty("port_spec"), parent: Some(fiber_def), order: k(2), components: vec![named("in"), port("optical_power", "in", None)] });
    ops.push(Op::Create {
        id: fiber_out,
        ty: ty("port_spec"),
        parent: Some(fiber_def),
        order: k(3),
        components: vec![named("out"), port("optical_power", "out", Some(format!("{fiber_in}.port.signal * pow(10, -{loss}.param_spec.default * {length}.param_spec.default / 10)")))],
    });
    ops.push(Op::Create { id: meter_def, ty: ty("definition"), parent: Some(library), order: k(2), components: vec![named("meter")] });
    ops.push(Op::Create { id: meter_in, ty: ty("port_spec"), parent: Some(meter_def), order: k(0), components: vec![named("in"), port("optical_power", "in", None)] });
    ops.push(Op::Create { id: meter_t, ty: ty("template"), parent: Some(meter_def), order: k(1), components: vec![template("cartoon")] });
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
                Op::Create { id: fiber_t, ty: ty("template"), parent: Some(fiber_def), order: k(4), components: vec![template("cartoon")] },
                Op::Create { id: more[1], ty: ty("rect"), parent: Some(meter_t), order: k(0), components: vec![Transform2d::default().wrap(), s.wrap()] },
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
    lab.store.apply(tx("longer", vec![Op::Set { id: lab.fiber, path: path("params.length_km"), slot: Slot::Const(Value::Number(10.0)) }])).unwrap();
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
        Handle::Linear { source, .. } => assert_eq!(source, Leaf { node: lab.laser, path: path("params.power") }),
        other => panic!("expected a linear handle, got {other:?}"),
    }
    // Ask for 5 mW at the meter: the laser is set to 5 mW / 10^-0.1.
    lab.store.preview_target(reading.node, reading.path, Value::parse_quantity("5 mW").unwrap()).unwrap();
    lab.store.commit_gesture("set reading", Actor::Human, 0).unwrap().unwrap();
    assert!((watts(&lab.store.resolved(lab.laser, path("params.power")).unwrap()) - 0.005 / 10f64.powf(-0.1)).abs() < 1e-12);
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
    lab.store.apply(tx("rf port", vec![Op::Create { id: rf, ty: ty("port"), parent: Some(lab.laser), order: key, components: vec![named("rf"), port("rf_voltage", "out", None)] }])).unwrap();
    let spare = lab.store.document().next_ids(1)[0];
    let key = OrderKey::after(&lab.store.document().live(meter_in).unwrap().order);
    lab.store.apply(tx("spare in", vec![Op::Create { id: spare, ty: ty("port"), parent: Some(lab.meter), order: key, components: vec![named("in2"), port("optical_power", "in", None)] }])).unwrap();
    let err = lab.connect(rf, spare).unwrap_err();
    assert!(err[0].message.contains("cannot connect rf_voltage"), "{err:?}");
}
