//! Instances with a size (plans/specs/plot.md §3): a template's `size` is
//! its picture's default box; its parts bind to it; each sized `use` holds
//! its own box, so resizing a picture reflows its parts and never another
//! picture's.

use iron_document::facet::{Composition, Name, Size, Template, Text, Transform2d};
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
    TransactionInput { label: label.into(), actor: Actor::Model, ts: 0, idempotency_key: None, ops, exact: false }
}

fn bound(e: String) -> Slot {
    Slot::Bound(Expr::parse(&e).unwrap())
}

fn num(store: &Store, id: NodeId, p: &str) -> f64 {
    store.resolved(id, path(p)).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("{id}.{p} unresolved"))
}

struct Bench {
    store: Store,
    bench: NodeId,
    def: NodeId,
    frame: NodeId,
    label: NodeId,
}

/// A composition, a model root, and a definition "card" whose template is
/// 200 × 100: a frame filling the box and a label 20 px above its bottom.
fn bench() -> Bench {
    let mut store = Store::new();
    let ids = store.document().next_ids(7);
    let (bench, model, library, def, template, frame, label) = (ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], ids[6]);
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("free".into()));
    let mut box_ = Size::default();
    box_.w = Slot::Const(Value::Number(200.0));
    box_.h = Slot::Const(Value::Number(100.0));
    let mut fill = Size::default();
    fill.w = bound(format!("{template}.size.w"));
    fill.h = bound(format!("{template}.size.h"));
    let mut at = Transform2d::default();
    at.x = Slot::Const(Value::Number(10.0));
    at.y = bound(format!("{template}.size.h - 20"));
    let mut text = Text::default();
    text.content = Slot::Const(Value::Str("card".into()));
    store
        .apply(tx(
            "bench",
            vec![
                Op::Create { id: bench, ty: ty("composition"), parent: None, order: k(0), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: k(1), facets: vec![] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(2), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(0), facets: vec![t.wrap(), box_.wrap()] },
                Op::Create { id: frame, ty: ty("rect"), parent: Some(template), order: k(0), facets: vec![Transform2d::default().wrap(), fill.wrap(), named("frame")] },
                Op::Create { id: label, ty: ty("text"), parent: Some(template), order: k(1), facets: vec![at.wrap(), text.wrap(), facet::Font::default().wrap(), named("label")] },
            ],
        ))
        .unwrap();
    Bench { store, bench, def, frame, label }
}

fn place(store: &mut Store, markup: &str) -> NodeId {
    let (ops, summary) = compose::compose_ops(store, markup).unwrap();
    store.apply(tx("place", ops)).unwrap();
    let line = summary.lines().find(|l| l.contains("<placed")).unwrap();
    let start = line.find("use=\"").unwrap() + 5;
    line[start..start + line[start..].find('"').unwrap()].parse().unwrap()
}

#[test]
fn a_sized_picture_reflows_its_parts_and_only_its_own() {
    let Bench { mut store, bench, def, frame, label } = bench();
    // The template itself shows its default box.
    assert_eq!(num(&store, frame, "size.w"), 200.0);
    let a = place(&mut store, &format!(r#"<place def="{def}" in="{bench}" x="0" y="0"/>"#));
    let b = place(&mut store, &format!(r#"<place def="{def}" in="{bench}" x="0" y="200" w="320" h="140"/>"#));
    store.document().check_invariants().unwrap();
    // Each picture starts at the template's box, or the box it was given.
    assert_eq!((num(&store, a, "size.w"), num(&store, a, "size.h")), (200.0, 100.0));
    assert_eq!((num(&store, b, "size.w"), num(&store, b, "size.h")), (320.0, 140.0));
    let (fa, fb) = (NodeId::expanded(a, frame).unwrap(), NodeId::expanded(b, frame).unwrap());
    let (la, lb) = (NodeId::expanded(a, label).unwrap(), NodeId::expanded(b, label).unwrap());
    assert_eq!((num(&store, fa, "size.w"), num(&store, fb, "size.w")), (200.0, 320.0));
    assert_eq!((num(&store, la, "transform2d.y"), num(&store, lb, "transform2d.y")), (80.0, 120.0));
    // Resizing one picture reflows its parts, not the other's; text keeps its size.
    let font = store.resolved(la, path("font.size"));
    store.apply(tx("resize", vec![Op::Set { id: a, path: path("size.w"), slot: Slot::Const(Value::Number(260.0)) }, Op::Set { id: a, path: path("size.h"), slot: Slot::Const(Value::Number(60.0)) }])).unwrap();
    assert_eq!((num(&store, fa, "size.w"), num(&store, fa, "size.h")), (260.0, 60.0));
    assert_eq!(num(&store, la, "transform2d.y"), 40.0);
    assert_eq!(store.resolved(la, path("font.size")), font);
    assert_eq!(num(&store, fb, "size.w"), 320.0);
    // The parts that read the box are keyed on it: pictures of different
    // sizes never share a drawing.
    let key = |u: NodeId| {
        let an = analysis::classify(store.document(), store.reactive(), u).unwrap();
        an.parts.iter().find(|p| p.node == frame).unwrap().key.clone()
    };
    assert!(key(a).contains_key("size"), "{:?}", key(a));
    assert_ne!(key(a), key(b));
    // Undo restores the box in one step.
    store.undo(0).unwrap().unwrap();
    assert_eq!((num(&store, fa, "size.w"), num(&store, la, "transform2d.y")), (200.0, 80.0));
}

#[test]
fn an_unsized_template_refuses_a_box() {
    let Bench { mut store, bench, def, .. } = bench();
    let template = store.document().children(def)[0];
    store.apply(tx("unsize", vec![Op::RemoveFacet { id: template, kind: FacetKind::Size }])).unwrap();
    let err = compose::compose_ops(&store, &format!(r#"<place def="{def}" in="{bench}" x="0" y="0" w="10"/>"#)).unwrap_err();
    assert!(err.contains("has no size"), "{err}");
    // Without w, it places, unsized.
    let u = place(&mut store, &format!(r#"<place def="{def}" in="{bench}" x="0" y="0"/>"#));
    assert!(store.document().facet(u, FacetKind::Size).is_none());
}
