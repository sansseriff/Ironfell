//! Repeats (plans/specs/repeat.md): a group drawing one copy of its
//! children per element, the copies derived and never saved.

use iron_document::facet::{Composition, Name};
use iron_document::*;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

/// A 2D composition holding `markup`; answers the store and the markup's named nodes.
fn sheet(markup: &str) -> (Store, std::collections::BTreeMap<String, NodeId>) {
    let mut store = Store::new();
    let comp = store.document().next_id();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("sheet".into()));
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] }])).unwrap();
    let built = markup::ops(markup, Some(comp), OrderKey::first(), store.document().next_id()).unwrap();
    store.apply(tx("draw", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    (store, built.named)
}

/// The copies a repeat has, in order, and each one's resolved `x`.
fn copies(store: &Store, g: NodeId) -> Vec<f64> {
    let doc = store.document();
    let mut items: Vec<NodeId> = doc.children(g).iter().copied().filter(|c| c.is_item_scope() && doc.is_live(*c)).collect();
    items.sort_by_key(|c| c.item_of(g));
    // Each item is a group holding its copy of the template's one shape.
    items.iter().map(|&i| store.resolved(doc.children(i)[0], path("transform2d.x")).unwrap().as_f64().unwrap()).collect()
}

#[test]
fn a_repeat_draws_one_copy_per_count_and_settles_with_undo() {
    let (mut store, named) = sheet(r#"<group id="g" name="row" repeat.count="3"><rect name="cell" x="{= $g.repeat.index * 20}" w="10" h="10"/></group>"#);
    let g = named["g"];
    assert_eq!(copies(&store, g), vec![0.0, 20.0, 40.0]);
    // The template is a child too, but no copy; it is never drawn.
    let template = store.document().children(g).iter().copied().find(|c| !c.is_expanded()).unwrap();
    assert!(expand::in_repeat_template(store.document(), template));
    assert!(store.document().children(g).iter().filter(|c| c.item_of(g).is_some()).all(|&c| !expand::in_repeat_template(store.document(), c)));
    store.apply(tx("more", vec![Op::Set { id: g, path: path("repeat.count"), slot: Slot::Const(Value::Number(5.0)) }])).unwrap();
    assert_eq!(copies(&store, g), vec![0.0, 20.0, 40.0, 60.0, 80.0]);
    // A copy is not edited: the error names the template node to edit.
    let item = store.document().children(g).iter().copied().find(|c| c.is_item_scope()).unwrap();
    let copy = store.document().children(item)[0];
    let refused = store.apply(tx("nudge", vec![Op::Set { id: copy, path: path("transform2d.y"), slot: Slot::Const(Value::Number(5.0)) }])).unwrap_err();
    assert!(refused[0].message.contains(&format!("edit its template, {template}")), "{}", refused[0].message);
    store.undo(0).unwrap().unwrap();
    assert_eq!(copies(&store, g), vec![0.0, 20.0, 40.0]);
    store.redo(0).unwrap().unwrap();
    assert_eq!(copies(&store, g).len(), 5);
}

#[test]
fn each_copy_reads_its_own_element_and_nothing_derived_is_saved() {
    let (mut store, named) = sheet(r#"<group id="g" name="dots" repeat.each="{= range(0, 30, 4)}"><ellipse name="dot" x="{= $g.repeat.item * 2}" rx="2" ry="2"/></group>"#);
    let g = named["g"];
    assert_eq!(copies(&store, g), vec![0.0, 20.0, 40.0, 60.0]);
    let text = store.save();
    assert!(!text.contains("~"), "copies are derived, never saved");
    let again = Store::load(&text).unwrap();
    assert_eq!(copies(&again, g), vec![0.0, 20.0, 40.0, 60.0]);
    // A count bound to another leaf follows it when that changes.
    store.apply(tx("each", vec![Op::Set { id: g, path: path("repeat.each"), slot: Slot::Bound(Expr::parse("range(0, 10, 2)").unwrap()) }])).unwrap();
    assert_eq!(copies(&store, g), vec![0.0, 20.0]);
    assert!(store.reactive().errors.is_empty(), "{:?}", store.reactive().errors);
}

/// A plot's tick labels are repeats in its definition's template: resizing
/// the picture adds labels in the same transaction, and undo takes them away.
#[test]
fn a_wider_plot_gets_more_tick_labels_in_one_transaction() {
    let (mut store, named) = sheet(r#"<group name="holder"/>"#);
    let _ = named;
    let model = store.document().next_id();
    store.apply(tx("model", vec![Op::Create { id: model, ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] }])).unwrap();
    let (ops, summary) = compose::compose_ops(&store, r#"<place def="std:plot" in="sheet" x="0" y="0"/>"#).unwrap();
    store.apply(tx("place", ops)).unwrap();
    let start = summary.find("use=\"").unwrap() + 5;
    let u: NodeId = summary[start..start + summary[start..].find('"').unwrap()].parse().unwrap();
    let count = |s: &Store| s.document().descendants(u).into_iter().filter(|n| n.to_string().contains("/~") && s.document().get::<Name>(*n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some("x tick")).count();
    let narrow = count(&store);
    store.apply(tx("wider", vec![Op::Set { id: u, path: path("size.w"), slot: Slot::Const(Value::Number(900.0)) }])).unwrap();
    let wide = count(&store);
    assert!(wide > narrow, "{narrow} → {wide}");
    store.undo(0).unwrap().unwrap();
    assert_eq!(count(&store), narrow);
}
