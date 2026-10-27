//! The schematic notation's store side (plans/specs/schematic.md): parts as
//! markup, placed as linked instances, wired by connections between
//! `electrical` pins and solved as one network.

use iron_document::expr::Expr;
use iron_document::facet::{self, LeafStruct};
use iron_document::op::Op;
use iron_document::order::OrderKey;
use iron_document::parts;
use iron_document::registry::TypeId;
use iron_document::value::{Slot, Value};
use iron_document::{Actor, NodeId, Store, TransactionInput};

fn tx(ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: "test".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn sheet(store: &mut Store) -> NodeId {
    let id = store.document().next_id();
    let mut c = facet::Composition::default();
    c.notation = Slot::Const(Value::Str("schematic".into()));
    store.apply(tx(vec![Op::Create { id, ty: TypeId::lookup("composition").unwrap(), parent: None, order: OrderKey::first(), facets: vec![c.wrap(), facet::Name { text: Slot::Const(Value::Str("sheet".into())) }.wrap()] }])).unwrap();
    id
}

fn si(store: &Store, id: NodeId, path: &str) -> f64 {
    match store.resolved(id, path.parse().unwrap()) {
        Some(Value::Quantity { si, .. }) => si,
        Some(Value::Number(n)) => n,
        other => panic!("{id}.{path} is {other:?}"),
    }
}

fn wire(store: &mut Store, a: NodeId, b: NodeId) {
    let model = store.document().roots().iter().copied().find(|&r| store.document().live(r).is_some_and(|n| n.ty.name() == "model")).unwrap();
    let (ops, _) = parts::connect_ops(store.document(), a, b, model);
    store.apply(tx(ops)).unwrap();
}

#[test]
fn the_standard_parts_parse_and_say_what_is_wrong() {
    let keys: Vec<&str> = parts::library().iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["resistor", "capacitor", "inductor", "vsource", "isource", "ground", "junction", "probe"]);
    for p in parts::library() {
        assert!(!p.symbol.is_empty(), "{} has no symbol", p.key);
        for (name, v) in &p.inputs {
            assert!(matches!(v, Value::Quantity { .. }), "{}.{name} = {v:?} is not a quantity", p.key);
        }
        for pin in &p.pins {
            assert!(pin.at[0] % 10.0 == 0.0 && pin.at[1] % 10.0 == 0.0, "{}.{} is off the grid", p.key, pin.name);
        }
    }
    assert!(parts::parse(r#"<part key="x" name="X"><in name="R" value="1"/></part>"#).unwrap_err().contains("needs a pin"));
    assert!(parts::parse(r#"<part key="x" name="X"><pin name="a"/><equation lhs="a.across +" rhs="0"/></part>"#).unwrap_err().starts_with("part x: a.across +"));
    assert!(parts::parse(r#"<part key="x" name="X"><pin name="a"/><symbol><path d="M0 0 Q1 1 2 2"/></symbol></part>"#).unwrap_err().contains("Q is not one of"));
}

#[test]
fn a_divider_solves_and_its_probe_reads_the_middle() {
    let mut store = Store::new();
    let s = sheet(&mut store);
    let v1 = store.place_part("vsource", s, [100.0, 200.0], 0.0, Actor::Human).unwrap();
    let r1 = store.place_part("resistor", s, [200.0, 100.0], 0.0, Actor::Human).unwrap();
    let r2 = store.place_part("resistor", s, [300.0, 200.0], std::f64::consts::FRAC_PI_2, Actor::Human).unwrap();
    let gnd = store.place_part("ground", s, [100.0, 300.0], 0.0, Actor::Human).unwrap();
    let probe = store.place_part("probe", s, [240.0, 100.0], 0.0, Actor::Human).unwrap();
    let doc = store.document();
    let name = |e: NodeId| doc.get::<facet::Name>(e).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).map(str::to_owned).unwrap();
    assert_eq!([v1, r1, r2, gnd, probe].map(|p| name(p.entity)), ["V1", "R1", "R2", "GND1", "P1"]);
    // One definition per part, however many are placed.
    assert_eq!(doc.nodes_of_type(TypeId::lookup("definition").unwrap()).filter(|&d| doc.is_live(d)).count(), 4);
    let pin = |p: parts::Placed, n: &str| parts::pin(store.document(), p.entity, n).unwrap_or_else(|| panic!("no pin {n}"));
    let (vp, vn, r1a, r1b, r2a, r2b, g, t) = (pin(v1, "p"), pin(v1, "n"), pin(r1, "a"), pin(r1, "b"), pin(r2, "a"), pin(r2, "b"), pin(gnd, "g"), pin(probe, "t"));
    store.apply(tx(vec![
        Op::Set { id: v1.entity, path: "args.V".parse().unwrap(), slot: Slot::Const(Value::parse_quantity("9 V").unwrap()) },
        Op::Set { id: r2.entity, path: "args.R".parse().unwrap(), slot: Slot::Const(Value::parse_quantity("2 kΩ").unwrap()) },
    ])).unwrap();
    for (a, b) in [(vp, r1a), (r1b, r2a), (r2b, vn), (vn, g), (r1b, t)] {
        wire(&mut store, a, b);
    }
    assert!(store.physics_error().is_none(), "{:?}", store.physics_error());
    assert!((si(&store, t, "port.across") - 6.0).abs() < 1e-6, "{}", si(&store, t, "port.across"));
    assert!((si(&store, r1a, "port.through") - 0.003).abs() < 1e-9, "{}", si(&store, r1a, "port.through"));
    // The probe's label reads its pin, and the resistor's its value.
    let words = |figure: NodeId| -> Vec<String> {
        let doc = store.document();
        doc.children(figure).iter().filter(|&&c| doc.live(c).is_some_and(|n| n.ty.name() == "text")).filter_map(|&c| store.resolved(c, "text.content".parse().unwrap())).filter_map(|v| v.as_str().map(str::to_owned)).collect()
    };
    assert_eq!(words(probe.figure), ["6.000 V"]);
    assert_eq!(words(r2.figure), ["R2", "2 kΩ"]);
}

#[test]
fn a_wire_bound_to_a_pin_follows_its_part() {
    let mut store = Store::new();
    let s = sheet(&mut store);
    let r1 = store.place_part("resistor", s, [200.0, 100.0], 0.0, Actor::Human).unwrap();
    let b = parts::pin(store.document(), r1.entity, "b").unwrap();
    let anchor = parts::pin_anchor(store.document(), r1.figure, b).expect("pin b's anchor");
    assert_eq!((si(&store, anchor, "anchor.wx"), si(&store, anchor, "anchor.wy")), (230.0, 100.0));
    // A point outside the instance may read where its pin is.
    let id = store.document().next_id();
    let mut t = facet::Transform2d::default();
    t.x = Slot::Bound(Expr::parse(&format!("{anchor}.anchor.wx")).unwrap());
    t.y = Slot::Bound(Expr::parse(&format!("{anchor}.anchor.wy")).unwrap());
    store.apply(tx(vec![Op::Create { id, ty: TypeId::lookup("empty").unwrap(), parent: Some(s), order: OrderKey::after(&OrderKey::first()), facets: vec![t.wrap()] }])).unwrap();
    store.apply(tx(vec![Op::Set { id: r1.figure, path: "transform2d.x".parse().unwrap(), slot: Slot::Const(Value::Number(260.0)) }])).unwrap();
    assert_eq!((si(&store, id, "transform2d.x"), si(&store, id, "transform2d.y")), (290.0, 100.0));
    // But not the rest of its picture.
    let inside = store.document().children(r1.figure)[0];
    let mut t = facet::Transform2d::default();
    t.x = Slot::Bound(Expr::parse(&format!("{inside}.transform2d.x")).unwrap());
    let err = store.apply(tx(vec![Op::Create { id: store.document().next_id(), ty: TypeId::lookup("empty").unwrap(), parent: Some(s), order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![t.wrap()] }])).unwrap_err();
    assert!(err[0].message.contains("inside instance"), "{err:?}");
}

#[test]
fn a_wires_corner_keeps_its_legs_straight_when_either_part_moves() {
    let mut store = Store::new();
    let s = sheet(&mut store);
    let r1 = store.place_part("resistor", s, [100.0, 100.0], 0.0, Actor::Human).unwrap();
    let r2 = store.place_part("resistor", s, [200.0, 200.0], 0.0, Actor::Human).unwrap();
    let pins = parts::pins_on(&store, s);
    let end = |p: parts::Placed, n: &str| {
        let pin = parts::pin(store.document(), p.entity, n).unwrap();
        pins.iter().find(|(e, _)| e.pin == pin).map(|(e, at)| (*e, *at)).unwrap()
    };
    let ((a, pa), (b, pb)) = (end(r1, "b"), end(r2, "a"));
    assert_eq!((pa, pb), ([130.0, 100.0], [170.0, 200.0]));
    let corner = parts::elbow(pa, pb, false).unwrap();
    let (ops, path) = parts::wire_ops(store.document(), s, a, b, &[pa, corner, pb]).unwrap();
    store.apply(tx(ops)).unwrap();
    let points = |store: &Store| -> Vec<[f64; 2]> {
        let doc = store.document();
        let contour = doc.children(path)[0];
        doc.children(contour).iter().map(|&p| [si(store, p, "transform2d.x"), si(store, p, "transform2d.y")]).collect()
    };
    assert_eq!(points(&store), [[130.0, 100.0], [170.0, 100.0], [170.0, 200.0]]);
    store.apply(tx(vec![Op::Set { id: r2.figure, path: "transform2d.x".parse().unwrap(), slot: Slot::Const(Value::Number(220.0)) }])).unwrap();
    store.apply(tx(vec![Op::Set { id: r1.figure, path: "transform2d.y".parse().unwrap(), slot: Slot::Const(Value::Number(110.0)) }])).unwrap();
    assert_eq!(points(&store), [[130.0, 110.0], [190.0, 110.0], [190.0, 200.0]]);
    // The wire is a figure of the connection that joins the pins.
    let doc = store.document();
    let c = doc.relations_from(path).find(|r| r.rel == iron_document::document::RelKind::Figures).unwrap().to;
    assert_eq!(iron_document::port::ends(doc, c), Some((a.pin, b.pin)));
}

#[test]
fn deleting_a_parts_symbol_deletes_the_part_and_its_wires() {
    let mut store = Store::new();
    let s = sheet(&mut store);
    let r1 = store.place_part("resistor", s, [100.0, 100.0], 0.0, Actor::Human).unwrap();
    let r2 = store.place_part("resistor", s, [200.0, 100.0], 0.0, Actor::Human).unwrap();
    let r3 = store.place_part("resistor", s, [300.0, 100.0], 0.0, Actor::Human).unwrap();
    let pins = parts::pins_on(&store, s);
    let end = |p: parts::Placed, n: &str| {
        let pin = parts::pin(store.document(), p.entity, n).unwrap();
        pins.iter().find(|(e, _)| e.pin == pin).map(|(e, at)| (*e, *at)).unwrap()
    };
    let ends = [(end(r1, "b"), end(r2, "a")), (end(r2, "b"), end(r3, "a"))];
    let mut wires = Vec::new();
    for ((a, pa), (b, pb)) in ends {
        let (ops, path) = parts::wire_ops(store.document(), s, a, b, &[pa, pb]).unwrap();
        store.apply(tx(ops)).unwrap();
        wires.push(path);
    }
    // Paths count only wires (the symbols are paths too).
    let count = |store: &Store, t: &str| {
        let doc = store.document();
        doc.nodes_of_type(TypeId::lookup(t).unwrap()).filter(|&n| doc.is_live(n) && (t != "path" || wires.contains(&n))).count()
    };
    assert_eq!((count(&store, "entity"), count(&store, "connection"), count(&store, "path")), (3, 2, 2));
    // R2 sits between both wires: they go with it.
    let ops = parts::delete_subject_ops(store.document(), r2.figure);
    store.apply(tx(ops)).unwrap();
    assert_eq!((count(&store, "entity"), count(&store, "connection"), count(&store, "path")), (2, 0, 0));
    store.undo(0).unwrap().unwrap();
    assert_eq!((count(&store, "entity"), count(&store, "connection"), count(&store, "path")), (3, 2, 2));
    // A wire takes only its connection.
    let ops = parts::delete_subject_ops(store.document(), wires[0]);
    store.apply(tx(ops)).unwrap();
    assert_eq!((count(&store, "entity"), count(&store, "connection"), count(&store, "path")), (3, 1, 1));
}

#[test]
fn a_wire_ending_on_a_wire_makes_a_junction_and_one_net() {
    let mut store = Store::new();
    let s = sheet(&mut store);
    let r1 = store.place_part("resistor", s, [100.0, 100.0], 0.0, Actor::Human).unwrap();
    let r2 = store.place_part("resistor", s, [300.0, 100.0], 0.0, Actor::Human).unwrap();
    let r3 = store.place_part("resistor", s, [200.0, 200.0], std::f64::consts::FRAC_PI_2, Actor::Human).unwrap();
    let pins = parts::pins_on(&store, s);
    let end = |p: parts::Placed, n: &str| {
        let pin = parts::pin(store.document(), p.entity, n).unwrap();
        pins.iter().find(|(e, _)| e.pin == pin).map(|(e, at)| (*e, *at)).unwrap()
    };
    let ((a, pa), (b, pb), (c, pc)) = (end(r1, "b"), end(r2, "a"), end(r3, "a"));
    assert_eq!((pa, pb, pc), ([130.0, 100.0], [270.0, 100.0], [200.0, 170.0]));
    let ops = parts::wire_between(&store, s, parts::Terminal::Pin(a), parts::Terminal::Pin(b), &[pa, pb]).unwrap();
    store.apply(tx(ops)).unwrap();
    let (wire, on) = parts::wire_at(&store, s, [201.0, 104.0], 6.0).expect("the wire under the point");
    assert_eq!(on, [201.0, 100.0]);
    // R3's pin a up to the wire, at 200: one transaction.
    let v = store.document().version();
    let ops = parts::wire_between(&store, s, parts::Terminal::Pin(c), parts::Terminal::Wire { path: wire, at: [200.0, 100.0] }, &[pc, [200.0, 100.0]]).unwrap();
    store.apply(tx(ops)).unwrap();
    assert_eq!(store.document().version().0, v.0 + 1);
    let doc = store.document();
    let live = |t: &str| doc.nodes_of_type(TypeId::lookup(t).unwrap()).filter(|&n| doc.is_live(n)).count();
    // The old wire is two now, through junction J1, and R3's is the third.
    assert_eq!((live("connection"), doc.is_live(wire)), (3, false));
    let names: Vec<String> = doc.nodes_of_type(TypeId::lookup("entity").unwrap()).filter(|&e| doc.is_live(e)).filter_map(|e| doc.get::<facet::Name>(e)?.text.constant()?.as_str().map(str::to_owned)).collect();
    assert!(names.contains(&"J1".to_owned()), "{names:?}");
    let net = iron_document::port::network(doc);
    let joined = |x: NodeId, y: NodeId| {
        // Each connection's ends; a, b, c share a net through the junction.
        let conns: Vec<(NodeId, NodeId)> = doc.nodes_of_type(TypeId::lookup("connection").unwrap()).filter(|&n| doc.is_live(n)).filter_map(|n| iron_document::port::ends(doc, n)).collect();
        let mut reach = vec![x];
        for _ in 0..4 {
            for &(p, q) in &conns {
                if reach.contains(&p) && !reach.contains(&q) { reach.push(q); }
                if reach.contains(&q) && !reach.contains(&p) { reach.push(p); }
            }
        }
        reach.contains(&y)
    };
    assert!(joined(a.pin, b.pin) && joined(a.pin, c.pin));
    assert!(!net.unknowns.is_empty());
}

#[test]
fn a_model_writes_a_circuit_in_one_transaction_and_reads_its_netlist() {
    let mut store = Store::new();
    let s = sheet(&mut store);
    let markup = r#"<circuit sheet="sheet">
  <place part="vsource" name="V1" x="120" y="220" V="9 V"/>
  <place part="resistor" name="R1" x="220" y="160" R="1 kΩ"/>
  <place part="resistor" name="R2" x="300" y="220" rot="90" R="2 kΩ"/>
  <place part="ground" x="120" y="300"/>
  <wire from="V1.p" to="R1.a" via="120,160"/>
  <wire from="R1.b" to="R2.a"/>
  <wire from="R2.b" to="V1.n"/>
  <wire from="V1.n" to="GND1.g"/>
</circuit>"#;
    let (ops, summary) = parts::circuit_ops(&store, markup).unwrap();
    assert_eq!(summary, "placed V1 R1 R2 GND1; 4 wires");
    let v = store.document().version();
    store.apply(tx(ops)).unwrap();
    assert_eq!(store.document().version().0, v.0 + 1);
    assert_eq!(parts::netlist(&store, s), "* sheet (netlist from Iron)\nV1 N1 0 9 V\nR1 N1 N2 1 kΩ\nR2 N2 0 2 kΩ\n.end\n* 0 = 0 V\n* N1 = 9 V\n* N2 = 6 V");
    // Mistakes are named, and nothing is applied.
    let bad = |m: &str| parts::circuit_ops(&store, &format!("<circuit sheet=\"sheet\">{m}</circuit>")).unwrap_err();
    assert!(bad(r#"<place part="resistor" x="15" y="20"/>"#).contains("off the 10 px grid"));
    assert!(bad(r#"<place part="resistor" x="10" y="20" C="1 nF"/>"#).contains("has no input C (R)"));
    assert!(bad(r#"<place part="diode" x="10" y="20"/>"#).contains("no part diode"));
    assert!(bad(r#"<wire from="R1.c" to="R2.a"/>"#).contains("R1 has no pin c"));
    assert!(bad(r#"<place part="resistor" name="R1" x="10" y="20"/>"#).contains("R1 is placed already"));
}

#[test]
fn a_turned_parts_labels_stay_upright_beside_it() {
    let mut store = Store::new();
    let s = sheet(&mut store);
    let r = store.place_part("resistor", s, [200.0, 200.0], std::f64::consts::FRAC_PI_2, Actor::Human).unwrap();
    let doc = store.document();
    let labels: Vec<NodeId> = doc.children(r.figure).iter().copied().filter(|&c| doc.live(c).is_some_and(|n| n.ty.name() == "text")).collect();
    assert_eq!(labels.len(), 2);
    for (label, at) in labels.into_iter().zip([[212.0, 184.0], [212.0, 198.0]]) {
        let w = iron_document::geom::world_affine(doc, &|l| store.resolved(l.node, l.path), label);
        let [a, b, _, _, x, y] = w.0;
        assert!(b.atan2(a).abs() < 1e-9, "turned {}", b.atan2(a));
        assert!((x - at[0]).abs() < 1e-9 && (y - at[1]).abs() < 1e-9, "at {x}, {y}");
    }
}
