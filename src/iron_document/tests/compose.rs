//! The model's component verbs (`compose::compose_ops`): make a component
//! from shapes, place an instance of it in one 2D composition and picture
//! the same entity in a second, as one transaction; standard components by
//! `std:<key>`, with arguments; and the refusals a model can act on.

use iron_document::facet::{Composition, Name, Size, Transform2d};
use iron_document::*;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn named(t: &str) -> facet::Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(t.into()));
    n.wrap()
}

fn comp(name: &str, dims: &str) -> Vec<facet::Facet> {
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum(dims.into()));
    vec![named(name), c.wrap()]
}

/// Two 2D compositions (bench, overview), a 3D one (scene), a model root,
/// and a rect and an ellipse on the bench.
fn desk() -> (Store, [NodeId; 6]) {
    let mut store = Store::new();
    let ids = store.document().next_ids(6);
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut at = Transform2d::default();
    at.x = Slot::Const(Value::Number(100.0));
    at.y = Slot::Const(Value::Number(50.0));
    let mut size = Size::default();
    size.w = Slot::Const(Value::Number(40.0));
    size.h = Slot::Const(Value::Number(30.0));
    let mut r = facet::Radius::default();
    r.rx = Slot::Const(Value::Number(10.0));
    r.ry = Slot::Const(Value::Number(10.0));
    let ops = vec![
        Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: k(0), facets: comp("bench", "2d") },
        Op::Create { id: ids[1], ty: ty("composition"), parent: None, order: k(1), facets: comp("overview", "2d") },
        Op::Create { id: ids[2], ty: ty("composition"), parent: None, order: k(2), facets: comp("scene", "3d") },
        Op::Create { id: ids[3], ty: ty("model"), parent: None, order: k(3), facets: vec![] },
        Op::Create { id: ids[4], ty: ty("rect"), parent: Some(ids[0]), order: k(0), facets: vec![at.clone().wrap(), size.wrap(), named("body")] },
        Op::Create { id: ids[5], ty: ty("ellipse"), parent: Some(ids[0]), order: k(1), facets: vec![at.wrap(), r.wrap(), named("dot")] },
    ];
    store.apply(TransactionInput { label: "desk".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }).unwrap();
    (store, [ids[0], ids[1], ids[2], ids[3], ids[4], ids[5]])
}

fn run(store: &mut Store, markup: &str) -> Result<String, String> {
    let (ops, summary) = compose::compose_ops(store, markup)?;
    store
        .apply(TransactionInput { label: "compose".into(), actor: Actor::Model, ts: 0, idempotency_key: None, ops, exact: false })
        .map_err(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))?;
    Ok(summary)
}

/// The value of `attr="…"` in the summary line starting `<tag`.
fn field(summary: &str, tag: &str, n: usize, attr: &str) -> NodeId {
    let line = summary.lines().filter(|l| l.trim_start().starts_with(&format!("<{tag} "))).nth(n).unwrap_or_else(|| panic!("no <{tag}> {n} in {summary}"));
    let start = line.find(&format!("{attr}=\"")).unwrap() + attr.len() + 2;
    line[start..start + line[start..].find('"').unwrap()].parse().unwrap()
}

#[test]
fn a_component_from_shapes_is_pictured_in_two_compositions_in_one_transaction() {
    let (mut store, [bench, overview, _, model, rect, dot]) = desk();
    let before = store.history().past().len();
    let summary = run(
        &mut store,
        &format!(
            r#"<make-component of="{rect} {dot}" name="pump" as="$pump"/>
               <place def="$pump" in="bench" x="300" y="80" name="P1" as="$p1"/>
               <place entity="$p1" in="{overview}" x="40" y="40"/>"#
        ),
    )
    .unwrap();
    assert_eq!(store.history().past().len(), before + 1, "one history entry");
    let doc = store.document();
    let def = field(&summary, "made", 0, "definition");
    let entity = field(&summary, "placed", 0, "entity");
    assert_eq!(field(&summary, "placed", 1, "entity"), entity, "the second picture is of the same entity");
    let (u1, u2) = (field(&summary, "placed", 0, "use"), field(&summary, "placed", 1, "use"));
    assert_eq!(doc.live(entity).unwrap().parent, Some(model));
    assert_eq!(expand::linked_definition(doc, entity), Some(def));
    assert_eq!(composition::composition_of(doc, u1), Some(bench));
    assert_eq!(composition::composition_of(doc, u2), Some(overview));
    for u in [u1, u2] {
        assert_eq!(expand::use_entity(doc, u), Some(entity));
        assert_eq!(doc.children(u).iter().filter(|c| c.is_expanded()).count(), 2, "each picture derives the rect and the ellipse");
    }
    assert_eq!(store.resolved(u2, "transform2d.x".parse().unwrap()), Some(Value::Number(40.0)));
    assert_eq!(doc.get::<Name>(entity).and_then(|n| n.text.constant()).and_then(|v| v.as_str()), Some("P1"));
    // Undo takes it all back: the shapes return, nothing placed remains.
    store.undo(1).unwrap().unwrap();
    assert!(store.document().is_live(rect) && !store.document().is_live(entity));
}

#[test]
fn a_standard_component_comes_in_once_and_takes_arguments() {
    let (mut store, [bench, overview, ..]) = desk();
    let summary = run(
        &mut store,
        &format!(r#"<place def="std:laser" in="{bench}" x="40" y="200" power="5 mW" as="$l"/><place def="std:laser" in="{overview}" args.power="{{= 2 mW * 3}}"/>"#),
    )
    .unwrap();
    let doc = store.document();
    let (a, b) = (field(&summary, "placed", 0, "entity"), field(&summary, "placed", 1, "entity"));
    assert_eq!(field(&summary, "placed", 0, "definition"), field(&summary, "placed", 1, "definition"), "the definition is copied in once");
    assert_eq!(standard::find(doc, "laser"), Some(field(&summary, "placed", 0, "definition")));
    assert_eq!(store.resolved(a, "args.power".parse().unwrap()), Some(Value::parse_quantity("5 mW").unwrap()));
    assert_eq!(store.resolved(b, "args.power".parse().unwrap()), Some(Value::parse_quantity("6 mW").unwrap()));
}

#[test]
fn refusals_say_what_to_do_instead() {
    let (mut store, [_, overview, scene, ..]) = desk();
    let err = |store: &mut Store, m: &str| run(store, m).unwrap_err();
    assert!(err(&mut store, &format!(r#"<place def="std:laser" in="{scene}"/>"#)).contains("<link-figures>"));
    assert!(err(&mut store, &format!(r#"<place def="std:laser" in="{overview}" wattage="3"/>"#)).contains("no input \"wattage\" (power)"));
    assert!(err(&mut store, &format!(r#"<place def="std:laser" in="{overview}" notation="schematic"/>"#)).contains("it has free"));
    assert!(err(&mut store, &format!(r#"<place def="std:lamp" in="{overview}"/>"#)).contains("std:laser"));
    assert!(err(&mut store, r#"<place entity="$nope" in="overview"/>"#).contains("not named by an earlier verb"));
    // A refused edit applies nothing, even when an earlier verb was fine.
    let version = store.document().version();
    assert!(err(&mut store, &format!(r#"<place def="std:laser" in="{overview}"/><place def="std:laser" in="{scene}"/>"#)).starts_with("<place> 2"));
    assert_eq!(store.document().version(), version);
}

#[test]
fn the_schema_lists_each_standard_component_s_ports() {
    let schema = registry::schema_json();
    let laser = schema["standard"].as_array().unwrap().iter().find(|e| e["key"] == "laser").unwrap();
    let ports: Vec<String> = laser["ports"].as_array().unwrap().iter().map(|p| format!("{} {} {} {}", p["name"].as_str().unwrap(), p["dir"].as_str().unwrap(), p["kind"].as_str().unwrap(), p["default"])).collect();
    assert_eq!(ports, ["power in value \"10 mW\"", "out out optical_power null"]);
}
