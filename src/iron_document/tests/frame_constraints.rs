//! Figma's constraints on a resizable component (plans/specs/frame-constraints.md):
//! the main's Resizable toggle gives the picture a box, and each part with
//! `constraints` follows a resized instance, published by the layout step.

use iron_document::facet::{Composition, Constraints, Name, Size, Template, Transform2d};
use iron_document::*;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn named(t: &str) -> facet::Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(t.into()));
    n.wrap()
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn n(x: f64) -> Slot {
    Slot::Const(Value::Number(x))
}

fn rect(x: f64, y: f64, w: f64, h: f64, name: &str, c: Option<(&str, &str)>) -> Vec<facet::Facet> {
    let mut t = Transform2d::default();
    t.x = n(x);
    t.y = n(y);
    let mut s = Size::default();
    s.w = n(w);
    s.h = n(h);
    let mut f = vec![t.wrap(), s.wrap(), named(name)];
    if let Some((h, v)) = c {
        let mut k = Constraints::default();
        k.horizontal = Slot::Const(Value::Enum(h.into()));
        k.vertical = Slot::Const(Value::Enum(v.into()));
        f.push(k.wrap());
    }
    f
}

struct Card {
    store: Store,
    def: NodeId,
    template: NodeId,
    button: NodeId,
    title: NodeId,
    u: NodeId,
}

/// A card definition (not yet resizable): a title bar along the top and a
/// button in the bottom-right corner, its picture offset by (10, 20) inside
/// the template; one instance placed.
fn card() -> Card {
    let mut store = Store::new();
    let ids = store.document().next_ids(7);
    let [bench, model, library, def, template, title, button] = ids[..] else { unreachable!() };
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("free".into()));
    store
        .apply(tx(
            "card",
            vec![
                Op::Create { id: bench, ty: ty("composition"), parent: None, order: k(0), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: k(1), facets: vec![] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(2), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(0), facets: vec![t.wrap()] },
                Op::Create { id: title, ty: ty("rect"), parent: Some(template), order: k(0), facets: rect(10.0, 20.0, 200.0, 20.0, "title", Some(("left_right", "top"))) },
                Op::Create { id: button, ty: ty("rect"), parent: Some(template), order: k(1), facets: rect(170.0, 90.0, 30.0, 20.0, "button", Some(("right", "bottom"))) },
            ],
        ))
        .unwrap();
    let (ops, summary) = compose::compose_ops(&store, &format!(r#"<place def="{def}" in="bench" x="100" y="100"/>"#)).unwrap();
    store.apply(tx("place", ops)).unwrap();
    let line = summary.lines().find(|l| l.contains("<placed")).unwrap();
    let start = line.find("use=\"").unwrap() + 5;
    let u: NodeId = line[start..start + line[start..].find('"').unwrap()].parse().unwrap();
    Card { store, def, template, button, title, u }
}

fn at(store: &Store, u: NodeId, part: NodeId, leaf: &str) -> f64 {
    store.resolved(NodeId::expanded(u, part).unwrap(), path(leaf)).unwrap().as_f64().unwrap()
}

#[test]
fn resizable_gives_the_picture_its_parts_bounds_and_nothing_moves() {
    let Card { mut store, def, template, button, u, .. } = card();
    let before = at(&store, u, button, "transform2d.x") + store.resolved(u, path("transform2d.x")).unwrap().as_f64().unwrap();
    let ops = author::resizable(store.document(), def, true, &|id| store.extent(id)).unwrap();
    store.apply(tx("make resizable", ops)).unwrap();
    // The box is the parts' bounds (10..210 × 20..110), moved to start at 0, 0.
    assert_eq!(store.resolved(template, path("size.w")), Some(Value::Number(200.0)));
    assert_eq!(store.resolved(template, path("size.h")), Some(Value::Number(90.0)));
    assert_eq!(store.resolved(u, path("size.w")), Some(Value::Number(200.0)));
    let after = at(&store, u, button, "transform2d.x") + store.resolved(u, path("transform2d.x")).unwrap().as_f64().unwrap();
    assert!((after - before).abs() < 1e-9, "the button stays where it was on screen");
    // Undo in one step; off again removes the sizes.
    store.undo(0).unwrap().unwrap();
    assert!(store.document().facet(template, FacetKind::Size).is_none());
}

#[test]
fn parts_follow_a_resized_instance_by_their_constraints() {
    let Card { mut store, def, button, title, u, .. } = card();
    let ops = author::resizable(store.document(), def, true, &|id| store.extent(id)).unwrap();
    store.apply(tx("make resizable", ops)).unwrap();
    // At the template's size nothing is published: the parts are as drawn.
    assert_eq!(at(&store, u, button, "transform2d.x"), 160.0);
    // 100 wider and 50 taller.
    store.apply(tx("resize", vec![Op::Set { id: u, path: path("size.w"), slot: n(300.0) }, Op::Set { id: u, path: path("size.h"), slot: n(140.0) }])).unwrap();
    assert_eq!((at(&store, u, button, "transform2d.x"), at(&store, u, button, "transform2d.y")), (260.0, 120.0), "the button keeps to the corner");
    assert_eq!(at(&store, u, button, "size.w"), 30.0, "and keeps its size");
    assert_eq!((at(&store, u, title, "transform2d.x"), at(&store, u, title, "size.w")), (0.0, 300.0), "the title stretches");
    assert_eq!(at(&store, u, title, "transform2d.y"), 0.0);
    // Nothing of it is saved: the file holds the parts as drawn.
    assert!(!store.save().contains("260"));
    // Undo puts the instance back at the template's size.
    store.undo(0).unwrap().unwrap();
    assert_eq!(at(&store, u, button, "transform2d.x"), 160.0);
}
