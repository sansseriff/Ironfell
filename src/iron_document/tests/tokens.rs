//! Colour tokens (palette.rs): `@name` in a formula, a light and a dark
//! value, the built-in theme joining a document on first use.

use iron_document::facet::{Composition, Fill, Name, Size, Transform2d};
use iron_document::*;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Model, ts: 0, idempotency_key: None, ops, exact: false }
}

fn named(t: &str) -> facet::Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(t.into()));
    n.wrap()
}

/// A composition with one rect; answers (store, rect).
fn bench() -> (Store, NodeId) {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    store
        .apply(tx(
            "bench",
            vec![
                Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: ids[1], ty: ty("rect"), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), Fill::default().wrap()] },
            ],
        ))
        .unwrap();
    (store, ids[1])
}

fn fill(store: &Store, r: NodeId) -> [f32; 4] {
    match store.resolved(r, path("fill.color")) {
        Some(Value::Color(c)) => c,
        v => panic!("{v:?}"),
    }
}

fn close(a: [f32; 4], hex: &str) -> bool {
    let b = palette::hex(hex);
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

#[test]
fn a_token_follows_the_theme_and_the_built_in_palette_joins_on_first_use() {
    let (mut store, r) = bench();
    assert!(palette::tokens(store.document()).is_empty());
    store.apply(tx("token", vec![Op::Set { id: r, path: path("fill.color"), slot: Slot::Bound(Expr::parse("@series-2").unwrap()) }])).unwrap();
    assert!(palette::tokens(store.document()).contains_key("series-8"), "the whole built-in theme joined");
    assert!(close(fill(&store, r), "#eb6834"), "{:?}", fill(&store, r));
    store.set_theme(true);
    assert!(close(fill(&store, r), "#d95926"), "dark");
    store.set_theme(false);
    // Editing the token recolours what uses it.
    let t = palette::tokens(store.document())["series-2"];
    store.apply(tx("recolour", vec![Op::Set { id: t, path: path("token.light"), slot: Slot::Const(Value::Color([0.0, 0.0, 1.0, 1.0])) }])).unwrap();
    assert_eq!(fill(&store, r), [0.0, 0.0, 1.0, 1.0]);
    // Formulas print the token by name; a save round-trips.
    let text = store.save();
    assert!(text.contains("@series-2"));
    let again = Store::load(&text).unwrap();
    assert_eq!(fill(&again, r), [0.0, 0.0, 1.0, 1.0]);
    // Undo takes the theme away with the first use.
    store.undo(0).unwrap().unwrap();
    store.undo(0).unwrap().unwrap();
    assert!(palette::tokens(store.document()).is_empty());
}

#[test]
fn an_unknown_token_is_a_binding_error() {
    let (mut store, r) = bench();
    store.apply(tx("token", vec![Op::Set { id: r, path: path("fill.color"), slot: Slot::Bound(Expr::parse("@nope").unwrap()) }])).unwrap();
    let errs: Vec<String> = store.reactive().errors.values().cloned().collect();
    assert!(errs.iter().any(|e| e.contains("@nope")), "{errs:?}");
    assert!(palette::tokens(store.document()).is_empty(), "no theme for a name it does not have");
}

#[test]
fn tokens_mix_in_formulas() {
    let (mut store, r) = bench();
    store.apply(tx("token", vec![Op::Set { id: r, path: path("fill.color"), slot: Slot::Bound(Expr::parse("mix(@surface, @series-1, 0.2)").unwrap()) }])).unwrap();
    assert!(store.reactive().errors.is_empty(), "{:?}", store.reactive().errors);
    let before = fill(&store, r);
    store.set_theme(true);
    assert_ne!(fill(&store, r), before, "a mixed colour follows the theme too");
}
