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

fn fiber_rig() -> (Store, NodeId, NodeId, Vec<NodeId>) {
    use iron_document::facet::{PathShape, Stroke};
    use iron_document::shape::Anchor as PathAnchor;
    use kurbo::{Point, Vec2};
    let Desk { mut store, comp, body, beam } = desk();
    apply(&mut store, "clear", vec![Op::Delete { id: body }, Op::Delete { id: beam }]);
    let ids = store.document().next_ids(8);
    let (fiber, shadow, control) = (ids[0], ids[1], ids[2]);
    let mut ops = vec![];
    let mut point_ops = vec![];
    let mut pts = Vec::new();
    for (i, shape) in [fiber, shadow].into_iter().enumerate() {
        let mut t = Transform2d::default();
        t.x = Slot::Const(Value::Number(20.0));
        t.y = Slot::Const(Value::Number(i as f64 * 8.0));
        ops.push(Op::Create { id: shape, ty: ty("path"), parent: Some(comp), order: key(i as u32), facets: vec![t.wrap(), PathShape::default().wrap(), Stroke::default().wrap(), named(if i == 0 { "fiber" } else { "shadow" })] });
        let mut seq = vec![ids[3+i*2], ids[4+i*2]].into_iter();
        let a = PathAnchor { at: Point::new(0.0, 0.0), inn: Vec2::new(-30.0, 0.0), out: Vec2::new(30.0, 0.0) };
        point_ops.extend(contour::contour_ops(shape, key(0), &[a], false, &mut seq));
        pts.push(ids[4+i*2]);
    }
    ops.push(Op::Create { id: control, ty: ty("empty"), parent: Some(comp), order: key(3), facets: vec![Transform2d::default().wrap(), named("A")] });
    ops.extend(point_ops);
    apply(&mut store, "draw", ops);
    let (made, input) = store.make_component(&[fiber, shadow, control], "fiber and shadow", Actor::Human).unwrap();
    apply(&mut store, "make", input.ops);
    (store, made.picture, control, pts)
}

#[test]
fn shared_control_preserves_contours_offsets_and_follows_rotation_scale() {
    let (mut store, main, control, points) = fiber_rig();
    let parents: Vec<_> = points.iter().map(|p| store.document().live(*p).unwrap().parent).collect();
    let before: Vec<_> = points.iter().map(|p| store.world_affine(*p).translation()).collect();
    let shown = |p| NodeId::expanded(main, p).unwrap();
    let ops = attachment::attach(&store, shown(control), &points.iter().map(|p| shown(*p)).collect::<Vec<_>>()).unwrap();
    apply(&mut store, "attach", ops);
    for (i, &p) in points.iter().enumerate() {
        assert_eq!(store.document().live(p).unwrap().parent, parents[i]);
        assert_eq!(store.world_affine(p).translation(), before[i]);
    }
    apply(&mut store, "move endpoint", vec![Op::Set { id: shown(control), path: path("transform2d.x"), slot: Slot::Const(Value::Number(100.0)) }]);
    assert!((num(&store, points[1], "transform2d.y") - num(&store, points[0], "transform2d.y")).abs() < 1e-8);
    let a = store.world_affine(points[0]).translation();
    let b = store.world_affine(points[1]).translation();
    assert!((b[1] - a[1] - 8.0).abs() < 1e-8);
    apply(&mut store, "turn endpoint", vec![Op::Set { id: shown(control), path: path("transform2d.rot"), slot: Slot::Const(Value::Number(std::f64::consts::FRAC_PI_2)) }, Op::Set { id: shown(control), path: path("transform2d.sy"), slot: Slot::Const(Value::Number(2.0)) }]);
    let a = store.world_affine(points[0]).translation();
    let b = store.world_affine(points[1]).translation();
    assert!((b[0] - a[0] + 16.0).abs() < 1e-8);
    assert_eq!(num(&store, points[0], "handles.ox"), 30.0);
    // Detaching freezes each point in its own path coordinates, not world.
    let before: Vec<_> = points.iter().map(|p| store.world_affine(*p).translation()).collect();
    let ops = attachment::detach(&store, &points).unwrap();
    apply(&mut store, "detach", ops);
    for (i, &p) in points.iter().enumerate() { assert_eq!(store.world_affine(p).translation(), before[i]); }
    store.undo(0).unwrap().unwrap();
    assert!(store.document().get::<facet::Attachment>(points[0]).is_some());
    let saved = store.save();
    let loaded = Store::load(&saved).unwrap();
    assert_eq!(loaded.save(), saved);
    for (i, &p) in points.iter().enumerate() { assert_eq!(loaded.world_affine(p).translation(), before[i]); }
}

#[test]
fn attachments_survive_instances_and_port_exposure_and_reject_invalid_edits() {
    let (mut store, main, control, points) = fiber_rig();
    let shown = |p| NodeId::expanded(main, p).unwrap();
    let ops = attachment::attach(&store, shown(control), &points).unwrap();
    apply(&mut store, "attach", ops);
    let before = store.save();
    assert!(store.apply(tx("delete control", vec![Op::Delete { id: shown(control) }])).is_err());
    assert_eq!(store.save(), before);
    assert!(store.apply(tx("remove attachment metadata", vec![Op::RemoveFacet { id: points[0], kind: facet::FacetKind::Attachment }])).is_err());
    assert_eq!(store.save(), before);
    assert!(attachment::attach(&store, control, &points).is_err());
    let ops = author::expose_control(store.document(), shown(control), "in", true).unwrap();
    apply(&mut store, "expose", ops);
    assert!(store.document().get::<facet::Anchor>(control).is_some());
    let pasted = copy::duplicate(&store, &[main]).unwrap();
    let instance = pasted.roots[0];
    apply(&mut store, "instance", pasted.ops);
    let p = NodeId::expanded(instance, points[1]).unwrap();
    let c = NodeId::expanded(instance, control).unwrap();
    assert_eq!(store.document().get::<facet::Attachment>(p).unwrap().control.constant().unwrap().as_ref(), Some(c));
    assert!(attachment::attach(&store, c, &[p]).is_err());
    let before = store.world_affine(p).translation();
    apply(&mut store, "override endpoint", vec![Op::Set { id: c, path: path("transform2d.x"), slot: Slot::Const(Value::Number(90.0)) }]);
    assert_ne!(store.world_affine(p).translation(), before);
    let saved = store.save();
    let restored = Store::load(&saved).unwrap();
    assert_eq!(restored.world_affine(p).translation(), store.world_affine(p).translation());
    // A control driven by its point would create a cycle.
    assert!(store.apply(tx("cycle", vec![Op::Set { id: control, path: path("transform2d.x"), slot: Slot::Bound(Expr::parse(&format!("{}.transform2d.x", points[0])).unwrap()) }])).is_err());
}

#[test]
fn both_paths_follow_a_plugged_instance_and_unplug_without_jumping() {
    let (mut store, main, control, points) = fiber_rig();
    let shown = NodeId::expanded(main, control).unwrap();
    let ops = attachment::attach(&store, shown, &points).unwrap();
    apply(&mut store, "attach", ops);
    let ops = author::expose_control(store.document(), shown, "in", true).unwrap();
    apply(&mut store, "expose", ops);
    let spec = store.document().get::<facet::Anchor>(control).unwrap().port.constant().unwrap().as_ref().unwrap();
    let ops = author::edit_port(store.document(), spec, &PortPatch { kind: Some("optical_power".into()), ..Default::default() }).unwrap();
    apply(&mut store, "optical", ops);
    let copy = copy::duplicate(&store, &[main]).unwrap();
    let instance = copy.roots[0];
    apply(&mut store, "instance", copy.ops);
    let comp = store.document().live(main).unwrap().parent.unwrap();
    let laser = store.place_standard("laser", comp, [400.0, 300.0], Actor::Human).unwrap();
    let entity = expand::use_entity(store.document(), instance).unwrap();
    let input = NodeId::expanded(entity, spec).unwrap();
    let laser_anchor = store.port_anchors().into_iter().find(|p| p.anchor.path().first() == Some(&laser)).unwrap();
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let ops = author::connect(store.document(), &read, laser_anchor.port, input).unwrap();
    apply(&mut store, "connect", ops);
    let p = NodeId::expanded(instance, points[0]).unwrap();
    let q = NodeId::expanded(instance, points[1]).unwrap();
    let start = store.world_affine(p).translation();
    apply(&mut store, "move laser", vec![Op::Set { id: laser, path: path("transform2d.x"), slot: Slot::Const(Value::Number(450.0)) }]);
    assert!((store.world_affine(p).translation()[0] - start[0] - 50.0).abs() < 1e-8);
    assert!((store.world_affine(q).translation()[1] - store.world_affine(p).translation()[1] - 8.0).abs() < 1e-8);
    let before = store.world_affine(p).translation();
    let connection = plug::connection_of(store.document(), input).unwrap();
    store.unplug(connection.0, "unplug", Actor::Human).unwrap();
    assert_eq!(store.world_affine(p).translation(), before);
}

#[test]
fn attachment_batch_rejects_foreign_or_driven_points_without_partial_changes() {
    let (mut store, main, control, points) = fiber_rig();
    let before = store.save();
    assert!(attachment::attach(&store, control, &[points[0], main]).is_err());
    assert_eq!(store.save(), before);
    apply(&mut store, "existing driver", vec![Op::Set { id: points[1], path: path("transform2d.x"), slot: Slot::Bound(Expr::parse(&format!("{control}.transform2d.x + 3")).unwrap()) }]);
    let before = store.save();
    assert!(attachment::attach(&store, control, &points).is_err());
    assert_eq!(store.save(), before);
    assert!(store.document().get::<facet::Attachment>(points[0]).is_none());
}

#[test]
fn attaching_under_transformed_paths_preserves_world_positions() {
    let (mut store, _main, control, points) = fiber_rig();
    let contour = store.document().live(points[1]).unwrap().parent.unwrap();
    let path_node = store.document().live(contour).unwrap().parent.unwrap();
    apply(&mut store, "transform shadow", vec![Op::Set { id: path_node, path: path("transform2d.rot"), slot: Slot::Const(Value::Number(0.7)) }, Op::Set { id: path_node, path: path("transform2d.sx"), slot: Slot::Const(Value::Number(2.0)) }]);
    let before = store.world_affine(points[1]).translation();
    let ops = attachment::attach(&store, control, &points).unwrap();
    apply(&mut store, "attach", ops);
    let after = store.world_affine(points[1]).translation();
    assert!((after[0]-before[0]).abs()<1e-8 && (after[1]-before[1]).abs()<1e-8);
    let control_x = num(&store, control, "transform2d.x");
    apply(&mut store, "move", vec![Op::Set { id: control, path: path("transform2d.x"), slot: Slot::Const(Value::Number(60.0)) }]);
    let moved = store.world_affine(points[1]).translation();
    assert!((moved[0]-after[0]-(60.0-control_x)).abs()<1e-8 && (moved[1]-after[1]).abs()<1e-8);
}

/// Editing a component's states is being inside it (machines.md §6): with
/// a state pinned, a press on the main selects the part under it, as if
/// the main held the selection. An edit a state cannot hold (a part
/// deleted) is counted, so the editor can say it changed every state; a
/// move, which the state records, is not.
#[test]
fn a_pinned_state_opens_its_main_and_counts_edits_it_cannot_hold() {
    let Desk { mut store, body, beam, .. } = desk();
    let (made, input) = store.make_component(&[body, beam], "laser", Actor::Human).unwrap();
    apply(&mut store, "make", input.ops);
    let (main, def) = (made.picture, made.definition);
    let shown = NodeId::expanded(main, body).unwrap();
    let chain = [main, shown];
    assert_eq!(interaction::target(&store.resolver(), &chain, &|_| false, false), 0, "at rest a press selects the component");
    let (ops, state) = machine::add_state_ops(store.document(), def).unwrap();
    apply(&mut store, "add state", ops);
    let m = machine::machines(store.document(), def)[0].0;
    store.pin_state(Some(machine::Pinned::state(def, m, state)));
    assert_eq!(interaction::target(&store.resolver(), &chain, &|_| false, false), 1, "editing its states, a press selects the part");
    assert_eq!(store.shared_edits(), 0);
    apply(&mut store, "move", vec![Op::Set { id: body, path: path("transform2d.x"), slot: Slot::Const(Value::Number(12.0)) }]);
    assert_eq!(store.shared_edits(), 0, "a move goes into the state");
    apply(&mut store, "delete", vec![Op::Delete { id: beam }]);
    assert_eq!(store.shared_edits(), 1, "a deleted part is gone from every state");
    store.pin_state(None);
    assert_eq!(interaction::target(&store.resolver(), &chain, &|_| false, false), 0);
}
