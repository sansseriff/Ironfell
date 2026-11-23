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
    let ids = store.authored().next_ids(7);
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
    let (ops, summary) = place_ops(&store, &format!(r#"<place def="{def}" in="bench" x="100" y="100"/>"#)).unwrap();
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
    let ops = author::resizable(store.tree(), def, true, &|id| store.extent(id)).unwrap();
    store.apply(tx("make resizable", ops)).unwrap();
    // The box is the parts' bounds (10..210 × 20..110), moved to start at 0, 0.
    assert_eq!(store.resolved(template, path("size.w")), Some(Value::Number(200.0)));
    assert_eq!(store.resolved(template, path("size.h")), Some(Value::Number(90.0)));
    assert_eq!(store.resolved(u, path("size.w")), Some(Value::Number(200.0)));
    let after = at(&store, u, button, "transform2d.x") + store.resolved(u, path("transform2d.x")).unwrap().as_f64().unwrap();
    assert!((after - before).abs() < 1e-9, "the button stays where it was on screen");
    // Undo in one step; off again removes the sizes.
    store.undo(0).unwrap().unwrap();
    assert!(store.tree().facet(template, FacetKind::Size).is_none());
}

#[test]
fn parts_follow_a_resized_instance_by_their_constraints() {
    let Card { mut store, def, button, title, u, .. } = card();
    let ops = author::resizable(store.tree(), def, true, &|id| store.extent(id)).unwrap();
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
    // A part's constraint edited on the template moves its copy at once.
    store.apply(tx("left", vec![Op::Set { id: button, path: path("constraints.horizontal"), slot: Slot::Const(Value::Enum("left".into())) }])).unwrap();
    assert_eq!(at(&store, u, button, "transform2d.x"), 160.0, "kept to the left now");
    store.undo(0).unwrap().unwrap();
    assert_eq!(at(&store, u, button, "transform2d.x"), 260.0);
    // Undo puts the instance back at the template's size.
    store.undo(0).unwrap().unwrap();
    assert_eq!(at(&store, u, button, "transform2d.x"), 160.0);
}

/// EN-2a: `PinsNative(root)` and pinned `LayoutNative` are keyed and scoped per
/// top-level root. Resizing an instance or editing a constraint mode runs zero
/// layout makers and wakes only the affected root's natives; adding a
/// `constraints` facet in the library wakes the instance's composition maker.
#[test]
fn pinned_parts_and_pinned_layout_are_per_root_and_keyed() {
    let mut store = Store::new();
    let ids = store.authored().next_ids(10);
    let [comp_a, comp_b, model, library, def, template, button, bar, bar_item, badge] = ids[..] else { unreachable!() };
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("free".into()));
    let mut bar_layout = facet::Layout::default();
    bar_layout.mode = Slot::Const(Value::Enum("stack_x".into()));
    bar_layout.sizing_w = Slot::Const(Value::Enum("fixed".into()));
    bar_layout.sizing_h = Slot::Const(Value::Enum("fixed".into()));
    let mut bar_facets = rect(0.0, 0.0, 200.0, 24.0, "bar", Some(("left_right", "top")));
    bar_facets.push(bar_layout.wrap());
    store
        .apply(tx(
            "two comps + card",
            vec![
                Op::Create { id: comp_a, ty: ty("composition"), parent: None, order: k(0), facets: vec![named("a"), c.clone().wrap()] },
                Op::Create { id: comp_b, ty: ty("composition"), parent: None, order: k(1), facets: vec![named("b"), c.wrap()] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: k(2), facets: vec![] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(3), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(0), facets: vec![t.wrap()] },
                Op::Create { id: button, ty: ty("rect"), parent: Some(template), order: k(0), facets: rect(170.0, 70.0, 30.0, 20.0, "button", Some(("right", "bottom"))) },
                Op::Create { id: bar, ty: ty("group"), parent: Some(template), order: k(1), facets: bar_facets },
                Op::Create { id: bar_item, ty: ty("rect"), parent: Some(bar), order: k(0), facets: rect(0.0, 0.0, 20.0, 20.0, "item", None) },
                Op::Create { id: badge, ty: ty("rect"), parent: Some(template), order: k(2), facets: rect(10.0, 70.0, 20.0, 20.0, "badge", None) },
            ],
        ))
        .unwrap();

    let place_in = |s: &mut Store, comp: &str| -> NodeId {
        let (ops, summary) = place_ops(s, &format!(r#"<place def="{def}" in="{comp}" x="0" y="0"/>"#)).unwrap();
        s.apply(tx("place", ops)).unwrap();
        let line = summary.lines().find(|l| l.contains("<placed")).unwrap();
        let start = line.find("use=\"").unwrap() + 5;
        line[start..start + line[start..].find('"').unwrap()].parse().unwrap()
    };
    let u_a = place_in(&mut store, "a");
    let u_b = place_in(&mut store, "b");
    let ops = author::resizable(store.tree(), def, true, &|id| store.extent(id)).unwrap();
    store.apply(tx("make resizable", ops)).unwrap();

    let bar_a = NodeId::expanded(u_a, bar).unwrap();
    let bar_b = NodeId::expanded(u_b, bar).unwrap();
    assert!(store.reactive().runs_of(graph::NativeKey::Pins(comp_a)).is_some());
    assert!(store.reactive().runs_of(graph::NativeKey::Pins(comp_b)).is_some());
    assert!(store.reactive().runs_of(graph::NativeKey::Layout(bar_a)).is_some());
    assert!(store.reactive().runs_of(graph::NativeKey::Layout(bar_b)).is_some());

    let (ma0, mb0) = (store.layout_maker_runs(comp_a), store.layout_maker_runs(comp_b));
    let (pins_a0, pins_b0) = (store.reactive().runs_of(graph::NativeKey::Pins(comp_a)).unwrap(), store.reactive().runs_of(graph::NativeKey::Pins(comp_b)).unwrap());
    let (lay_a0, lay_b0) = (store.reactive().runs_of(graph::NativeKey::Layout(bar_a)).unwrap(), store.reactive().runs_of(graph::NativeKey::Layout(bar_b)).unwrap());

    // Resizing u_a runs 0 layout makers, updates both Pins(comp_a) and Layout(bar_a),
    // and leaves comp_b's Pins and Layout natives untouched.
    store.apply(tx("resize a", vec![Op::Set { id: u_a, path: path("size.w"), slot: n(300.0) }, Op::Set { id: u_a, path: path("size.h"), slot: n(140.0) }])).unwrap();
    assert_eq!(store.layout_maker_runs(comp_a), ma0, "resize must not rerun comp_a's maker");
    assert_eq!(store.layout_maker_runs(comp_b), mb0, "resize must not rerun comp_b's maker");
    assert!(store.reactive().runs_of(graph::NativeKey::Pins(comp_a)).unwrap() > pins_a0);
    assert!(store.reactive().runs_of(graph::NativeKey::Layout(bar_a)).unwrap() > lay_a0);
    assert_eq!(store.reactive().runs_of(graph::NativeKey::Pins(comp_b)).unwrap(), pins_b0, "comp_b pins must stay idle");
    assert_eq!(store.reactive().runs_of(graph::NativeKey::Layout(bar_b)).unwrap(), lay_b0, "comp_b layout must stay idle");
    assert_eq!(at(&store, u_a, button, "transform2d.x"), 270.0);
    assert_eq!(at(&store, u_a, bar, "size.w"), 300.0);

    // Editing a constraint mode in the library runs 0 layout makers and updates u_a.
    store.apply(tx("mode", vec![Op::Set { id: button, path: path("constraints.horizontal"), slot: Slot::Const(Value::Enum("left".into())) }])).unwrap();
    assert_eq!(store.layout_maker_runs(comp_a), ma0);
    assert_eq!(store.layout_maker_runs(comp_b), mb0);
    assert_eq!(at(&store, u_a, button, "transform2d.x"), 170.0);

    // Adding a Constraints facet to `badge` in the library wakes comp_a's maker
    // across top-level roots and pins `badge` in u_a immediately.
    let mut pin_right = Constraints::default();
    pin_right.horizontal = Slot::Const(Value::Enum("right".into()));
    pin_right.vertical = Slot::Const(Value::Enum("bottom".into()));
    store.apply(tx("pin badge", vec![Op::AddFacet { id: badge, facet: pin_right.wrap() }])).unwrap();
    assert!(store.layout_maker_runs(comp_a) > ma0, "adding Constraints in library must wake comp_a's layout maker");
    assert_eq!(at(&store, u_a, badge, "transform2d.x"), 110.0, "badge shifts +100 with u_a's wider box");
}

/// Component verbs as an edit (edit.rs): its ops and its summary lines.
fn place_ops(store: &iron_document::Store, markup: &str) -> Result<(Vec<iron_document::Op>, String), String> {
    let plan = iron_document::edit::plan(store, &format!("<edit>{markup}</edit>"), None).map_err(|f| f.message())?;
    Ok((plan.ops, plan.summary.join("\n")))
}
