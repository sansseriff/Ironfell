//! Authoring a component from the UI's verbs (component-authoring.md):
//! shapes become a definition shown by its main; edits in the main change
//! the definition; a copy of the main is an instance; ports and inputs are
//! added and edited with every instance's arguments kept valid; shapes are
//! wrapped in an empty; an input is linked to a part; two instances are
//! connected by a line.

use iron_document::author::{self, PortPatch};
use iron_document::facet::{Composition, Name, Size, Transform2d};
use iron_document::*;

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
fn key(n: u32) -> OrderKey {
    (0..n).fold(OrderKey::first(), |k, _| OrderKey::after(&k))
}
fn num(store: &Store, id: NodeId, p: &str) -> f64 {
    store.resolved(id, path(p)).unwrap_or_else(|| panic!("{id}.{p}")).as_f64().unwrap()
}
fn rect(x: f64, y: f64, w: f64, h: f64, name: &str) -> Vec<Facet> {
    let mut t = Transform2d::default();
    t.x = Slot::Const(Value::Number(x));
    t.y = Slot::Const(Value::Number(y));
    let mut s = Size::default();
    s.w = Slot::Const(Value::Number(w));
    s.h = Slot::Const(Value::Number(h));
    vec![t.wrap(), s.wrap(), named(name)]
}
fn apply(store: &mut Store, label: &str, ops: Vec<Op>) {
    store.apply(tx(label, ops)).unwrap_or_else(|e| panic!("{label}: {e:?}"));
}

struct Desk {
    store: Store,
    comp: NodeId,
    body: NodeId,
    beam: NodeId,
}

fn desk() -> Desk {
    let mut store = Store::new();
    let ids = store.document().next_ids(5);
    let [library, comp, model, body, beam] = ids.try_into().unwrap();
    let mut c = Composition::default();
    c.notation = Slot::Const(Value::Str("cartoon".into()));
    apply(&mut store, "desk", vec![
        Op::Create { id: library, ty: ty("library"), parent: None, order: key(0), facets: vec![] },
        Op::Create { id: comp, ty: ty("composition"), parent: None, order: key(1), facets: vec![named("bench"), c.wrap()] },
        Op::Create { id: model, ty: ty("model"), parent: None, order: key(2), facets: vec![] },
        Op::Create { id: body, ty: ty("rect"), parent: Some(comp), order: key(0), facets: rect(60.0, 140.0, 130.0, 70.0, "body") },
        Op::Create { id: beam, ty: ty("rect"), parent: Some(comp), order: key(1), facets: rect(190.0, 168.0, 50.0, 14.0, "beam") },
    ]);
    Desk { store, comp, body, beam }
}

#[test]
fn shapes_become_a_component_edited_through_its_main_and_copied_as_instances() {
    let Desk { mut store, comp, body, beam } = desk();
    // Make component: the shapes move into a definition; a main takes their
    // place at their top-left, and nothing moves on screen.
    let (made, input) = store.make_component(&[body, beam], "laser", Actor::Human).unwrap();
    apply(&mut store, "make", input.ops);
    let main = made.picture;
    let doc = store.document();
    assert_eq!(expand::main_definition(doc, main), Some(made.definition));
    assert_eq!((num(&store, main, "transform2d.x"), num(&store, main, "transform2d.y")), (60.0, 140.0));
    assert_eq!((num(&store, body, "transform2d.x"), num(&store, body, "transform2d.y")), (0.0, 0.0));
    let shown_body = NodeId::expanded(main, body).unwrap();
    assert_eq!(store.world_affine(shown_body).translation(), [60.0, 140.0]);
    assert!(doc.children(comp).contains(&main) && !doc.children(comp).contains(&body));

    // Edits inside the main are edits to the definition.
    apply(&mut store, "wider", vec![Op::Set { id: shown_body, path: path("size.w"), slot: Slot::Const(Value::Number(150.0)) }]);
    assert_eq!(num(&store, body, "size.w"), 150.0);
    assert!(store.document().children(shown_body).is_empty());

    // A copy of the main is an instance, which follows the definition.
    let pasted = copy::duplicate(&store, &[main]).unwrap();
    apply(&mut store, "duplicate", pasted.ops);
    let instance = pasted.roots[0];
    let doc = store.document();
    assert!(expand::main_definition(doc, instance).is_none());
    let entity = expand::use_entity(doc, instance).expect("an instance figures an entity");
    assert_eq!(expand::linked_definition(doc, entity), Some(made.definition));
    apply(&mut store, "taller", vec![Op::Set { id: shown_body, path: path("size.h"), slot: Slot::Const(Value::Number(90.0)) }]);
    assert_eq!(num(&store, NodeId::expanded(instance, body).unwrap(), "size.h"), 90.0);

    // Undo the edit, the duplicate and the make: the shapes are back.
    for _ in 0..4 {
        store.undo(0).unwrap().unwrap();
    }
    let doc = store.document();
    assert!(doc.live(main).is_none() && doc.children(comp).contains(&body));
    assert_eq!(num(&store, body, "transform2d.x"), 60.0);
}

#[test]
fn ports_inputs_wraps_links_and_lines_are_authored_on_the_main() {
    let Desk { mut store, body, beam, .. } = desk();
    let (made, input) = store.make_component(&[body, beam], "laser", Actor::Human).unwrap();
    apply(&mut store, "make", input.ops);
    let (main, def) = (made.picture, made.definition);
    let pasted = copy::duplicate(&store, &[main]).unwrap();
    apply(&mut store, "instance", pasted.ops);
    let instance = pasted.roots[0];
    let entity = expand::use_entity(store.document(), instance).unwrap();

    // A port placed on the main, at the beam's end.
    let (ops, out, out_anchor) = author::add_port(store.document(), main, [180.0, 35.0], "out", false).unwrap();
    apply(&mut store, "add port", ops);
    let shown = NodeId::expanded(main, out_anchor).unwrap();
    assert_eq!(store.world_affine(shown).translation(), [240.0, 175.0]);
    // Edited: an optical output, a male FC/PC connector.
    let patch = PortPatch { name: Some("light out".into()), kind: Some("optical_power".into()), connector: Some("FC/PC".into()), gender: Some("male".into()), ..Default::default() };
    { let ops = author::edit_port(store.document(), out, &patch).unwrap(); apply(&mut store, "edit port", ops); }
    let port = NodeId::expanded(entity, out).unwrap();
    assert_eq!(store.resolved(port, path("port.kind")), Some(Value::Str("optical_power".into())));
    assert_eq!(store.resolved(out_anchor, path("name.text")), Some(Value::Str("light_out".into())));

    // A setting: power, 10 mW by default; the instance gets the argument.
    let (ops, power) = author::add_input(store.document(), def, "power", Value::parse_quantity("10 mW").unwrap()).unwrap();
    apply(&mut store, "add input", ops);
    assert_eq!(store.resolved(entity, path("args.power")), Some(Value::parse_quantity("10 mW").unwrap()));
    // The output says the power.
    let patch = PortPatch { signal: Some(format!("{{= {power}.port.signal}}")), ..Default::default() };
    { let ops = author::edit_port(store.document(), out, &patch).unwrap(); apply(&mut store, "out = power", ops); }
    assert_eq!(store.resolved(port, path("port.signal")), Some(Value::parse_quantity("10 mW").unwrap()));
    // Renamed, the instance keeps its value under the new name.
    apply(&mut store, "set power", vec![Op::Set { id: entity, path: path("args.power"), slot: Slot::Const(Value::parse_quantity("4 mW").unwrap()) }]);
    let patch = PortPatch { name: Some("setpoint".into()), ..Default::default() };
    { let ops = author::edit_port(store.document(), power, &patch).unwrap(); apply(&mut store, "rename", ops); }
    assert_eq!(store.resolved(entity, path("args.setpoint")), Some(Value::parse_quantity("4 mW").unwrap()));
    assert_eq!(store.resolved(port, path("port.signal")), Some(Value::parse_quantity("4 mW").unwrap()));

    // The beam wrapped in an empty, inside the main: the definition gains
    // the empty, the beam inside it, nothing moving on screen.
    let shown_beam = NodeId::expanded(main, beam).unwrap();
    let before = store.world_affine(shown_beam).translation();
    let extent = |n: NodeId| store.extent(n);
    let (ops, empty) = author::wrap_in_empty(store.document(), &extent, &[shown_beam]).unwrap();
    apply(&mut store, "wrap", ops);
    let doc = store.document();
    assert_eq!(doc.live(beam).unwrap().parent, Some(empty));
    assert_eq!(doc.live(empty).map(|n| n.ty.name()), Some("empty"));
    let shown_beam = NodeId::expanded(main, beam).unwrap();
    let after = store.world_affine(shown_beam).translation();
    assert!((before[0] - after[0]).abs() < 1e-9 && (before[1] - after[1]).abs() < 1e-9, "{before:?} {after:?}");

    // A link from the setting to the empty's rotation, through the main:
    // the definition's binding, so each instance turns with its own power.
    let shown_empty = NodeId::expanded(main, empty).unwrap();
    let link = Expr::parse(&format!("scale({power}.port.signal, 0 mW, 20 mW, 0, 1.5)")).unwrap();
    apply(&mut store, "link", vec![Op::Set { id: shown_empty, path: path("transform2d.rot"), slot: Slot::Bound(link) }]);
    assert!(store.document().leaf(empty, path("transform2d.rot")).unwrap().expr().is_some());
    assert!((num(&store, NodeId::expanded(instance, empty).unwrap(), "transform2d.rot") - 0.3).abs() < 1e-12);

    // A second instance's light connected by a line to a meter-like input:
    // make a "sink" with one optical input, and connect.
    let ids = store.document().next_ids(1);
    let comp = store.document().roots()[1];
    apply(&mut store, "sink shape", vec![Op::Create { id: ids[0], ty: ty("rect"), parent: Some(comp), order: key(9), facets: rect(400.0, 140.0, 40.0, 40.0, "sink") }]);
    let (sink, input) = store.make_component(&[ids[0]], "sink", Actor::Human).unwrap();
    apply(&mut store, "make sink", input.ops);
    let (ops, sink_in) = author::add_input(store.document(), sink.definition, "light", Value::parse_quantity("0 W").unwrap()).unwrap();
    apply(&mut store, "sink input", ops);
    { let ops = author::edit_port(store.document(), sink_in, &PortPatch { kind: Some("optical_power".into()), ..Default::default() }).unwrap(); apply(&mut store, "optical", ops); }
    let pasted = copy::duplicate(&store, &[sink.picture]).unwrap();
    apply(&mut store, "sink instance", pasted.ops);
    let sink_entity = expand::use_entity(store.document(), pasted.roots[0]).unwrap();
    let to = NodeId::expanded(sink_entity, sink_in).unwrap();
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let ops = author::connect(store.document(), &read, port, to).unwrap();
    apply(&mut store, "connect", ops);
    assert_eq!(store.resolved(to, path("port.signal")), Some(Value::parse_quantity("4 mW").unwrap()));
}
