//! Definitions as data (roadmap stage B, ECM §4; component-model.md §4): a
//! library definition with two `in` ports and a template, instantiated into
//! an entity, its ports and a figure whose bindings read the ports; an
//! argument bound to a slider; validation against the definition.

use iron_document::facet::{Template, Fill, Name, Port, Size, Slider, Transform2d};
use iron_document::definition::{self, Placement};
use iron_document::*;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Model, ts: 0, idempotency_key: None, ops, exact: false }
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
}

fn named(text: &str) -> Name {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(text.into()));
    n
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

/// An `in` port_spec's facet with its default.
fn input(default: Value) -> Port {
    let mut p = Port::default();
    p.kind = Slot::Const(Value::Str("value".into()));
    p.dir = Slot::Const(Value::Enum("in".into()));
    p.signal = Slot::Const(default);
    p
}

/// The entity's port named `name`.
fn port_named(store: &Store, entity: NodeId, name: &str) -> NodeId {
    let doc = store.document();
    doc.children(entity).iter().copied().find(|&c| doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
}

fn resolved(store: &Store, id: NodeId, p: &str) -> Value {
    store.resolved(id, path(p)).unwrap_or_else(|| panic!("{id}.{p} unresolved"))
}

struct Lib {
    store: Store,
    scene: NodeId,
    slider: NodeId,
    def: NodeId,
    level: NodeId,
    power: NodeId,
}

/// scene { slider }, library { definition "bar" { in port level (0.4), in
/// port power (1 mW), template "free" { rect "bar" with y and h bound to
/// level's port } } }.
fn library() -> Lib {
    let mut store = Store::new();
    let ids = store.document().next_ids(8);
    let (scene, slider, library, def, level, power, template, bar) = (ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], ids[6], ids[7]);
    let mut sl = Slider::default();
    sl.value = num(0.4);
    let level_spec = input(Value::Number(0.4));
    let power_spec = input(Value::parse_quantity("1 mW").unwrap());
    let mut t = Transform2d::default();
    t.x = num(780.0);
    t.y = Slot::Bound(Expr::parse(&format!("640 - {level}.port.signal * 300")).unwrap());
    let mut s = Size::default();
    s.w = num(60.0);
    s.h = Slot::Bound(Expr::parse(&format!("{level}.port.signal * 300")).unwrap());
    let k0 = OrderKey::first();
    let k1 = OrderKey::after(&k0);
    let k2 = OrderKey::after(&k1);
    store
        .apply(tx(
            "library",
            vec![
                Op::Create { id: scene, ty: ty("group"), parent: None, order: k0.clone(), facets: vec![named("scene").wrap()] },
                Op::Create { id: slider, ty: ty("slider"), parent: Some(scene), order: k0.clone(), facets: vec![Transform2d::default().wrap(), sl.wrap(), named("level").wrap()] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k1.clone(), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k0.clone(), facets: vec![named("bar").wrap()] },
                Op::Create { id: level, ty: ty("port_spec"), parent: Some(def), order: k0.clone(), facets: vec![level_spec.wrap(), named("level").wrap()] },
                Op::Create { id: power, ty: ty("port_spec"), parent: Some(def), order: k1.clone(), facets: vec![power_spec.wrap(), named("power").wrap()] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k2, facets: vec![Template::default().wrap()] },
                Op::Create { id: bar, ty: ty("rect"), parent: Some(template), order: k0, facets: vec![t.wrap(), s.wrap(), Fill::default().wrap(), named("bar").wrap()] },
            ],
        ))
        .unwrap();
    Lib { store, scene, slider, def, level, power }
}

fn place(scene: NodeId) -> Placement {
    Placement { entity_parent: Some(scene), entity_after: Some(OrderKey::first()), figure_parent: Some(scene), figure_after: Some(OrderKey::after(&OrderKey::first())) }
}

#[test]
fn a_definition_instantiates_into_an_entity_and_a_figure() {
    let Lib { mut store, scene, def, level, .. } = library();
    let specs = definition::inputs_of(store.document(), def).unwrap();
    assert_eq!(specs.iter().map(|s| s.name).collect::<Vec<_>>(), vec!["level", "power"]);
    assert_eq!(specs[0].kind, ValueKind::Number);
    assert_eq!(specs[1].kind, ValueKind::Quantity);
    // The template itself evaluates with the defaults.
    let template_rect = store.document().children(definition::template_of(store.document(), def, "free").unwrap())[0];
    assert_eq!(resolved(&store, template_rect, "size.h"), Value::Number(120.0));

    let (entity, ops) = definition::instantiate(store.document(), def, "free", "bar", &place(scene)).unwrap();
    store.apply(tx("instantiate bar", ops)).unwrap();
    store.document().check_invariants().unwrap();
    let e = store.document().live(entity).unwrap();
    assert_eq!((e.ty.name(), e.parent), ("entity", Some(scene)));
    assert_eq!(resolved(&store, entity, "args.level"), Value::Number(0.4));
    assert_eq!(resolved(&store, entity, "args.power"), Value::parse_quantity("1 mW").unwrap());
    // Each input became a port on the entity that reads its argument.
    let level_port = port_named(&store, entity, "level");
    let signal = store.document().leaf(level_port, path("port.signal")).unwrap().expr().unwrap().to_string();
    assert_eq!(signal, format!("{entity}.args.level"));
    assert_eq!(resolved(&store, level_port, "port.signal"), Value::Number(0.4));
    // The copy is a plain rect under the scene whose bindings read the
    // entity's port, with a figures relation back to it.
    let copy = store.document().children(scene).iter().copied().find(|&c| store.document().live(c).unwrap().ty.name() == "rect").unwrap();
    assert_ne!(copy, template_rect);
    let h = store.document().leaf(copy, path("size.h")).unwrap().expr().unwrap().to_string();
    assert_eq!(h, format!("{level_port}.port.signal * 300"));
    assert!(!h.contains(&level.to_string()));
    assert_eq!(resolved(&store, copy, "size.h"), Value::Number(120.0));
    assert_eq!(resolved(&store, copy, "transform2d.y"), Value::Number(520.0));
    let rels: Vec<_> = store.document().relations_to(entity).collect();
    assert_eq!(rels.len(), 1);
    assert_eq!((rels[0].from, rels[0].rel), (copy, RelKind::Figures));
    // A schema-2 file spelled the template `depiction` and the relation
    // `depicts`; it loads under the new names.
    let text = store.save();
    let old = text
        .replacen(&format!("\"schema\": {}", iron_document::serial::SCHEMA), "\"schema\": 2", 1)
        .replace("\"template\"", "\"depiction\"")
        .replace("\"figures\"", "\"depicts\"");
    assert!(old.contains("\"depicts\"") && old.contains("\"depiction\""));
    assert_eq!(Store::load(&old).unwrap().save(), text);
    // A schema-5 file declared parameters (`param_spec`) and held `params`;
    // they load as in ports and arguments.
    let v5 = text
        .replacen(&format!("\"schema\": {}", iron_document::serial::SCHEMA), "\"schema\": 5", 1)
        .replace("\"facets\": {", "\"components\": {")
        .replace(&format!(".port.signal"), ".param_spec.default")
        .replace(&format!("{entity}.param_spec.default"), &format!("{entity}.args.level"))
        .replace("\"args\":", "\"params\":");
    let migrated = Store::load(&v5).unwrap();
    assert_eq!(resolved(&migrated, copy, "size.h"), Value::Number(120.0));
    assert_eq!(resolved(&migrated, entity, "args.power"), Value::parse_quantity("1 mW").unwrap());
    // Undo removes the whole instance; redo restores it.
    store.undo(1).unwrap().unwrap();
    assert!(store.document().live(entity).is_none() && store.document().live(copy).is_none());
    store.redo(2).unwrap().unwrap();
    assert_eq!(resolved(&store, copy, "size.h"), Value::Number(120.0));
}

#[test]
fn an_entity_parameter_bound_to_a_slider_drives_the_figure() {
    let Lib { mut store, scene, slider, def, .. } = library();
    let (entity, ops) = definition::instantiate(store.document(), def, "free", "bar", &place(scene)).unwrap();
    store.apply(tx("instantiate bar", ops)).unwrap();
    store
        .apply(tx("bind level", vec![Op::Set { id: entity, path: path("args.level"), slot: Slot::Bound(Expr::parse(&format!("{slider}.slider.value")).unwrap()) }]))
        .unwrap();
    let copy = store.document().children(scene).iter().copied().find(|&c| store.document().live(c).unwrap().ty.name() == "rect").unwrap();
    store.drain_changes();
    store.preview(slider, path("slider.value"), Value::Number(0.9));
    assert_eq!(resolved(&store, entity, "args.level"), Value::Number(0.9));
    assert_eq!(resolved(&store, copy, "size.h"), Value::Number(270.0));
    assert_eq!(resolved(&store, copy, "transform2d.y"), Value::Number(370.0));
    // Dragging the figure's y inverts through the port and the argument
    // onto the slider.
    assert!(matches!(store.handle(copy, path("transform2d.y")), Handle::Linear { source, .. } if source == Leaf { node: slider, path: path("slider.value") }));
    store.commit_gesture("set slider", Actor::Human, 1).unwrap().unwrap();
    // The view names the definition and shows the argument and its binding.
    let v = store.view(&ViewQuery { relation: Default::default(), scope: Some(entity), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains(&format!(r#"<entity id="{entity}" name="bar" def="{def}" level="0.9" level.bind="{slider}.slider.value" power="1 mW""#)), "{v}");
    assert!(v.contains(&format!(r#"<driven-by ref="{copy}" rel="figures"/>"#)), "{v}");
    // Everything round-trips through the file.
    let text = store.save();
    assert!(text.contains(r#""schema": 6"#), "{text}");
    let loaded = Store::load(&text).unwrap();
    assert_eq!(loaded.save(), text);
    assert_eq!(resolved(&loaded, copy, "size.h"), Value::Number(270.0));
}

#[test]
fn entities_are_validated_against_their_definition() {
    let Lib { mut store, scene, slider, def, power, .. } = library();
    let (entity, ops) = definition::instantiate(store.document(), def, "free", "bar", &place(scene)).unwrap();
    store.apply(tx("instantiate bar", ops)).unwrap();
    let bad = |store: &mut Store, label: &str, ops: Vec<Op>| -> String {
        let errs = store.apply(tx(label, ops)).unwrap_err();
        assert_eq!(errs[0].kind, ErrorKind::BadInstance, "{label}: {}", errs[0].message);
        errs[0].message.clone()
    };
    // Wrong kind for an argument.
    let m = bad(&mut store, "kind", vec![Op::Set { id: entity, path: path("args.level"), slot: Slot::Const(Value::Str("high".into())) }]);
    assert!(m.contains("expects number"), "{m}");
    // Wrong dimension for a quantity argument; the right one is fine.
    let m = bad(&mut store, "dim", vec![Op::Set { id: entity, path: path("args.power"), slot: Slot::Const(Value::parse_quantity("3 V").unwrap()) }]);
    assert!(m.contains("expects W"), "{m}");
    store.apply(tx("ok", vec![Op::Set { id: entity, path: path("args.power"), slot: Slot::Const(Value::parse_quantity("2 W").unwrap()) }])).unwrap();
    // An unknown argument is not a field at all (Set), and an args facet
    // with an extra or missing argument is rejected (Create).
    let errs = store.apply(tx("unknown", vec![Op::Set { id: entity, path: path("args.gain"), slot: num(1.0) }])).unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::TypeMismatch, "{}", errs[0].message);
    assert!(errs[0].message.contains("no argument"), "{}", errs[0].message);
    let id = store.document().next_id();
    let mut inst = iron_document::facet::Instance::default();
    inst.def = Slot::Const(Value::Ref(def));
    let mut params = iron_document::facet::Args::default();
    params.insert("level", num(1.0));
    let m = bad(&mut store, "missing", vec![Op::Create { id, ty: ty("entity"), parent: Some(scene), order: OrderKey::first(), facets: vec![inst.clone().wrap(), params.clone().wrap()] }]);
    assert!(m.contains("missing argument \"power\""), "{m}");
    params.insert("power", Slot::Const(Value::parse_quantity("1 W").unwrap()));
    params.insert("gain", num(2.0));
    let m = bad(&mut store, "extra", vec![Op::Create { id, ty: ty("entity"), parent: Some(scene), order: OrderKey::first(), facets: vec![inst.clone().wrap(), params.clone().wrap()] }]);
    assert!(m.contains("declares no in port \"gain\""), "{m}");
    // The instance must name a definition.
    let m = bad(&mut store, "not a def", vec![Op::Set { id: entity, path: path("instance.def"), slot: Slot::Const(Value::Ref(slider)) }]);
    assert!(m.contains("not a definition"), "{m}");
    // No template for a notation; a definition with a bad spec.
    assert!(definition::instantiate(store.document(), def, "schematic", "bar3", &place(scene)).unwrap_err().contains("no template"));
    store.apply(tx("break spec", vec![Op::Set { id: power, path: path("port.signal"), slot: Slot::Bound(Expr::parse(&format!("{slider}.slider.value")).unwrap()) }])).unwrap();
    let e = definition::instantiate(store.document(), def, "free", "bar2", &place(scene)).unwrap_err();
    assert!(e.contains("default must be a constant"), "{e}");
    store.document().check_invariants().unwrap();
}
