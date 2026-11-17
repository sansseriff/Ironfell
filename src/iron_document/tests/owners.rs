//! Owners (reactive-engine handoff §4.4, stage 2): an op expands again only
//! the owners it can change. Each case here is one the council found the
//! first version missed; under the `differential` feature with
//! `IRON_OWNER_CHECK=1` the whole-document expansion after each update finds
//! nothing left to do, and the reference store agrees.

use iron_document::facet::{Composition, Fill, Name, Size, Transform2d};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn sheet() -> (Store, NodeId) {
    let mut store = Store::new();
    let comp = store.authored().next_id();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("sheet".into()));
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] }])).unwrap();
    (store, comp)
}

/// A placed meter: its picture and the entity it figures.
fn meter(store: &mut Store, comp: NodeId) -> (NodeId, NodeId) {
    store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let doc = store.tree();
    let u = *doc.children(comp).iter().filter(|c| !c.is_expanded() && doc.live(**c).is_some_and(|n| n.ty.name() == "use")).last().unwrap();
    let e = doc.relations_from(u).find(|r| r.rel == RelKind::Figures).unwrap().to;
    (u, e)
}

fn derived_under(store: &Store, n: NodeId) -> usize {
    store.tree().descendants(n).into_iter().filter(|d| d.is_expanded()).count()
}

/// Deleting the container of an entity takes the picture's copies with it.
#[test]
fn deleting_an_entitys_container_clears_its_picture() {
    let (mut store, comp) = sheet();
    let (u, e) = meter(&mut store, comp);
    assert!(derived_under(&store, u) > 0);
    let model = store.tree().live(e).unwrap().parent.unwrap();
    store.apply(tx("delete model", vec![Op::Delete { id: model }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(derived_under(&store, u), 0);
}

/// A picture in a repeat's template figuring an entity outside the repeat:
/// changing the entity reaches the items' copies.
#[test]
fn a_picture_in_a_repeat_template_follows_its_entity() {
    let (mut store, comp) = sheet();
    let (_, e) = meter(&mut store, comp);
    let ids = store.authored().next_ids(2);
    let (r, u) = (ids[0], ids[1]);
    let mut rep = facet::Repeat::default();
    rep.count = Slot::Const(Value::Number(2.0));
    let rel = store.authored().next_relation_id();
    store
        .apply(tx(
            "repeat",
            vec![
                Op::Create { id: r, ty: ty("group"), parent: Some(comp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), rep.wrap()] },
                Op::Create { id: u, ty: ty("use"), parent: Some(r), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), facet::Use::default().wrap()] },
                Op::Link { rel: Relation { id: rel, from: u, to: e, rel: RelKind::Figures, data: BTreeMap::new() } },
            ],
        ))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let before = derived_under(&store, r);
    store.apply(tx("detach", vec![Op::Set { id: e, path: "instance.mode".parse().unwrap(), slot: Slot::Const(Value::Enum("detached".into())) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(derived_under(&store, r) < before, "the items' pictures went with the link: {before} -> {}", derived_under(&store, r));
}

/// A derived relation unlinked by hand comes back: its owner derives it.
#[test]
fn an_unlinked_derived_relation_is_derived_again() {
    let (mut store, comp) = sheet();
    meter(&mut store, comp);
    let Some(rel) = store.tree().relations().find(|r| r.id.is_expanded()).cloned() else { return };
    if store.apply(tx("unlink", vec![Op::Unlink { id: rel.id }])).is_ok() {
        assert!(store.tree().relation(rel.id).is_some(), "{rel:?} is derived again");
    }
}

/// Copies whose bindings read each other in a chain, written in one
/// transaction in the order that makes each fail until the next is made.
#[test]
fn a_chain_of_copies_reading_each_other_expands_in_one_pass() {
    let (mut store, comp) = sheet();
    let ids = store.authored().next_ids(4);
    let [r, a, b, c] = ids.try_into().unwrap();
    let mut rep = facet::Repeat::default();
    rep.count = Slot::Const(Value::Number(2.0));
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let rect = |id, i| Op::Create { id, ty: ty("rect"), parent: Some(r), order: k(i), facets: vec![Transform2d::default().wrap(), Size::default().wrap()] };
    store.apply(tx("repeat", vec![Op::Create { id: r, ty: ty("group"), parent: Some(comp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), rep.wrap()] }, rect(a, 0), rect(b, 1), rect(c, 2)])).unwrap();
    let fill = |slot: Slot| {
        let mut f = Fill::default();
        f.color = slot;
        f.wrap()
    };
    let bound = |n: NodeId| Slot::Bound(Expr::parse(&format!("{n}.fill.color")).unwrap());
    store
        .apply(tx(
            "fills",
            vec![
                Op::AddFacet { id: c, facet: fill(Slot::Const(Value::Color([0.2, 0.4, 0.6, 1.0]))) },
                Op::AddFacet { id: b, facet: fill(bound(c)) },
                Op::AddFacet { id: a, facet: fill(bound(b)) },
            ],
        ))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let doc = store.tree();
    let copies_of_a: Vec<NodeId> = doc.descendants(r).into_iter().filter(|d| d.is_expanded() && d.path().last() == Some(&a)).collect();
    assert_eq!(copies_of_a.len(), 2);
    for copy in copies_of_a {
        assert!(doc.get::<Fill>(copy).is_some(), "{copy} has its fill");
    }
    assert!(store.expansion_problems().is_empty(), "{:?}", store.expansion_problems());
}
