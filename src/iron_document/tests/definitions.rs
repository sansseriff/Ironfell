//! Definitions as data (roadmap stage B, ECM §4 and phase 1's exit): a
//! library definition with a parameter and a template, instantiated into an
//! entity plus a depiction whose bindings read the entity's params; the
//! parameter bound to a slider; validation against the definition.

use iron_document::component::{Depiction, Fill, Name, ParamSpec, Size, Slider, Transform2d};
use iron_document::definition::{self, Placement};
use iron_document::*;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Model, ts: 0, idempotency_key: None, ops }
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

/// scene { slider }, library { definition "bar" { param_spec level (number
/// 0.4), param_spec power (quantity 1 mW), depiction "free" { rect "bar"
/// with y and h bound to level's default } } }.
fn library() -> Lib {
    let mut store = Store::new();
    let ids = store.document().next_ids(8);
    let (scene, slider, library, def, level, power, depiction, template) = (ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], ids[6], ids[7]);
    let mut sl = Slider::default();
    sl.value = num(0.4);
    let mut level_spec = ParamSpec::default();
    level_spec.kind = Slot::Const(Value::Enum("number".into()));
    level_spec.default = num(0.4);
    let mut power_spec = ParamSpec::default();
    power_spec.kind = Slot::Const(Value::Enum("quantity".into()));
    power_spec.default = Slot::Const(Value::parse_quantity("1 mW").unwrap());
    let mut t = Transform2d::default();
    t.x = num(780.0);
    t.y = Slot::Bound(Expr::parse(&format!("640 - {level}.param_spec.default * 300")).unwrap());
    let mut s = Size::default();
    s.w = num(60.0);
    s.h = Slot::Bound(Expr::parse(&format!("{level}.param_spec.default * 300")).unwrap());
    let k0 = OrderKey::first();
    let k1 = OrderKey::after(&k0);
    let k2 = OrderKey::after(&k1);
    store
        .apply(tx(
            "library",
            vec![
                Op::Create { id: scene, ty: ty("group"), parent: None, order: k0.clone(), components: vec![named("scene").wrap()] },
                Op::Create { id: slider, ty: ty("slider"), parent: Some(scene), order: k0.clone(), components: vec![Transform2d::default().wrap(), sl.wrap(), named("level").wrap()] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k1.clone(), components: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k0.clone(), components: vec![named("bar").wrap()] },
                Op::Create { id: level, ty: ty("param_spec"), parent: Some(def), order: k0.clone(), components: vec![level_spec.wrap(), named("level").wrap()] },
                Op::Create { id: power, ty: ty("param_spec"), parent: Some(def), order: k1.clone(), components: vec![power_spec.wrap(), named("power").wrap()] },
                Op::Create { id: depiction, ty: ty("depiction"), parent: Some(def), order: k2, components: vec![Depiction::default().wrap()] },
                Op::Create { id: template, ty: ty("rect"), parent: Some(depiction), order: k0, components: vec![t.wrap(), s.wrap(), Fill::default().wrap(), named("bar").wrap()] },
            ],
        ))
        .unwrap();
    Lib { store, scene, slider, def, level, power }
}

fn place(scene: NodeId) -> Placement {
    Placement { entity_parent: Some(scene), entity_after: Some(OrderKey::first()), depiction_parent: Some(scene), depiction_after: Some(OrderKey::after(&OrderKey::first())) }
}

#[test]
fn a_definition_instantiates_into_an_entity_and_a_depiction() {
    let Lib { mut store, scene, def, level, .. } = library();
    let specs = definition::params_of(store.document(), def).unwrap();
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
    assert_eq!(resolved(&store, entity, "params.level"), Value::Number(0.4));
    assert_eq!(resolved(&store, entity, "params.power"), Value::parse_quantity("1 mW").unwrap());
    // The copy is a plain rect under the scene whose bindings read the
    // entity's params, with a depicts relation back to it.
    let copy = store.document().children(scene).iter().copied().find(|&c| store.document().live(c).unwrap().ty.name() == "rect").unwrap();
    assert_ne!(copy, template_rect);
    let h = store.document().leaf(copy, path("size.h")).unwrap().expr().unwrap().to_string();
    assert_eq!(h, format!("{entity}.params.level * 300"));
    assert!(!h.contains(&level.to_string()));
    assert_eq!(resolved(&store, copy, "size.h"), Value::Number(120.0));
    assert_eq!(resolved(&store, copy, "transform2d.y"), Value::Number(520.0));
    let rels: Vec<_> = store.document().relations_to(entity).collect();
    assert_eq!(rels.len(), 1);
    assert_eq!((rels[0].from, rels[0].rel), (copy, RelKind::Depicts));
    // Undo removes the whole instance; redo restores it.
    store.undo(1).unwrap().unwrap();
    assert!(store.document().live(entity).is_none() && store.document().live(copy).is_none());
    store.redo(2).unwrap().unwrap();
    assert_eq!(resolved(&store, copy, "size.h"), Value::Number(120.0));
}

#[test]
fn an_entity_parameter_bound_to_a_slider_drives_the_depiction() {
    let Lib { mut store, scene, slider, def, .. } = library();
    let (entity, ops) = definition::instantiate(store.document(), def, "free", "bar", &place(scene)).unwrap();
    store.apply(tx("instantiate bar", ops)).unwrap();
    store
        .apply(tx("bind level", vec![Op::Set { id: entity, path: path("params.level"), slot: Slot::Bound(Expr::parse(&format!("{slider}.slider.value")).unwrap()) }]))
        .unwrap();
    let copy = store.document().children(scene).iter().copied().find(|&c| store.document().live(c).unwrap().ty.name() == "rect").unwrap();
    store.drain_changes();
    store.preview(slider, path("slider.value"), Value::Number(0.9));
    assert_eq!(resolved(&store, entity, "params.level"), Value::Number(0.9));
    assert_eq!(resolved(&store, copy, "size.h"), Value::Number(270.0));
    assert_eq!(resolved(&store, copy, "transform2d.y"), Value::Number(370.0));
    // Dragging the depiction's y inverts through the parameter onto the slider.
    assert!(matches!(store.handle(copy, path("transform2d.y")), Handle::Linear { source, .. } if source == Leaf { node: slider, path: path("slider.value") }));
    store.commit_gesture("set slider", Actor::Human, 1).unwrap().unwrap();
    // The view names the definition and shows the parameter and its binding.
    let v = store.view(&ViewQuery { scope: Some(entity), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains(&format!(r#"<entity id="{entity}" name="bar" def="{def}" level="0.9" level.bind="{slider}.slider.value" power="1 mW">"#)), "{v}");
    assert!(v.contains(&format!(r#"<driven-by ref="{copy}" rel="depicts"/>"#)), "{v}");
    // Everything round-trips through the file.
    let text = store.save();
    assert!(text.contains(r#""schema": 2"#), "{text}");
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
    // Wrong kind for a parameter.
    let m = bad(&mut store, "kind", vec![Op::Set { id: entity, path: path("params.level"), slot: Slot::Const(Value::Str("high".into())) }]);
    assert!(m.contains("expects number"), "{m}");
    // Wrong dimension for a quantity parameter; the right one is fine.
    let m = bad(&mut store, "dim", vec![Op::Set { id: entity, path: path("params.power"), slot: Slot::Const(Value::parse_quantity("3 V").unwrap()) }]);
    assert!(m.contains("expects W"), "{m}");
    store.apply(tx("ok", vec![Op::Set { id: entity, path: path("params.power"), slot: Slot::Const(Value::parse_quantity("2 W").unwrap()) }])).unwrap();
    // An unknown parameter is not a field at all (Set), and a params
    // component with an extra or missing parameter is rejected (Create).
    let errs = store.apply(tx("unknown", vec![Op::Set { id: entity, path: path("params.gain"), slot: num(1.0) }])).unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::TypeMismatch, "{}", errs[0].message);
    assert!(errs[0].message.contains("no parameter"), "{}", errs[0].message);
    let id = store.document().next_id();
    let mut inst = iron_document::component::Instance::default();
    inst.def = Slot::Const(Value::Ref(def));
    let mut params = iron_document::component::Params::default();
    params.insert("level", num(1.0));
    let m = bad(&mut store, "missing", vec![Op::Create { id, ty: ty("entity"), parent: Some(scene), order: OrderKey::first(), components: vec![inst.clone().wrap(), params.clone().wrap()] }]);
    assert!(m.contains("missing parameter \"power\""), "{m}");
    params.insert("power", Slot::Const(Value::parse_quantity("1 W").unwrap()));
    params.insert("gain", num(2.0));
    let m = bad(&mut store, "extra", vec![Op::Create { id, ty: ty("entity"), parent: Some(scene), order: OrderKey::first(), components: vec![inst.clone().wrap(), params.clone().wrap()] }]);
    assert!(m.contains("declares no parameter \"gain\""), "{m}");
    // The instance must name a definition.
    let m = bad(&mut store, "not a def", vec![Op::Set { id: entity, path: path("instance.def"), slot: Slot::Const(Value::Ref(slider)) }]);
    assert!(m.contains("not a definition"), "{m}");
    // No template for a notation; a definition with a bad spec.
    assert!(definition::instantiate(store.document(), def, "schematic", "bar3", &place(scene)).unwrap_err().contains("no depiction"));
    store.apply(tx("break spec", vec![Op::Set { id: power, path: path("param_spec.default"), slot: num(1.0) }])).unwrap();
    let e = definition::instantiate(store.document(), def, "free", "bar2", &place(scene)).unwrap_err();
    assert!(e.contains("kind says quantity"), "{e}");
    store.document().check_invariants().unwrap();
}
