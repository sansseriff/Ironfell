//! The graph updated in place (reactive-engine handoff §4.4, stage 3a)
//! against the rebuild it replaces: cases the council's review of stages 3a
//! and 3b found (2026-11-17), each where `New` and `Old` disagreed. Run
//! through the differential facade they compare the two engines; each also
//! asserts the right outcome directly.

use iron_document::facet::{Composition, Name, Port};
use iron_document::*;

fn ty(n: &str) -> TypeId { TypeId::lookup(n).unwrap() }
fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}
fn path(s: &str) -> LeafPath { s.parse().unwrap() }
fn name(text: &str) -> Facet { let mut n = Name::default(); n.text = Slot::Const(Value::Str(text.into())); n.wrap() }
fn q(s: &str) -> Value { Value::parse_quantity(s).unwrap() }
fn sheet() -> (Store, NodeId) {
    let mut store = Store::new();
    let comp = store.authored().next_id();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name("sheet"), c.wrap()] }])).unwrap();
    (store, comp)
}
fn library_def(store: &mut Store) -> (NodeId, NodeId) {
    let ids = store.authored().next_ids(2);
    store.apply(tx("library", vec![
        Op::Create { id: ids[0], ty: ty("library"), parent: None, order: OrderKey::first(), facets: vec![name("library")] },
        Op::Create { id: ids[1], ty: ty("definition"), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![name("thing")] },
    ])).unwrap_or_else(|e| panic!("{e:?}"));
    (ids[0], ids[1])
}
fn inst(def: NodeId, mode: &str) -> Facet {
    let mut i = facet::Instance::default();
    i.def = Slot::Const(Value::Ref(def));
    i.mode = Slot::Const(Value::Enum(mode.into()));
    i.wrap()
}
fn errs(store: &Store) -> Vec<String> { store.reactive().errors.iter().map(|(l, m)| format!("{l}: {m}")).collect() }

/// T1: a port's datatype set to `like(xs)` must make its check track args.xs.
#[test]
fn a_datatype_set_to_like_tracks_the_argument_it_reads() {
    let mut store = Store::new();
    let (_, def) = library_def(&mut store);
    let ids = store.authored().next_ids(5);
    let [spec, model, l, e, p] = ids.try_into().unwrap();
    let mut lf = facet::Let::default();
    lf.value = Slot::Const(q("1 mm"));
    let mut sp = Port::default();
    sp.kind = Slot::Const(Value::Str("value".into()));
    sp.dir = Slot::Const(Value::Enum("in".into()));

    sp.signal = Slot::Const(q("1 mm"));
    let mut args = facet::Args::default();
    args.insert("xs", Slot::Bound(Expr::parse(&format!("{l}.let.value")).unwrap()));
    let mut port = Port::default();
    port.kind = Slot::Const(Value::Str("value".into()));
    port.dir = Slot::Const(Value::Enum("out".into()));
    port.signal = Slot::Bound(Expr::parse("2 mm").unwrap());
    store.apply(tx("make", vec![
        Op::Create { id: spec, ty: ty("port_spec"), parent: Some(def), order: OrderKey::first(), facets: vec![name("xs"), sp.wrap()] },
        Op::Create { id: model, ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
        Op::Create { id: l, ty: ty("let"), parent: None, order: OrderKey::before(&OrderKey::first()), facets: vec![name("L"), lf.wrap()] },
        Op::Create { id: e, ty: ty("entity"), parent: Some(model), order: OrderKey::first(), facets: vec![inst(def, "detached"), args.wrap()] },
        Op::Create { id: p, ty: ty("port"), parent: Some(e), order: OrderKey::first(), facets: vec![port.wrap(), name("p")] },
    ])).unwrap_or_else(|e| panic!("{e:?}"));
    store.apply(tx("type", vec![Op::Set { id: p, path: path("port.datatype"), slot: Slot::Const(Value::Str("like(xs)".into())) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(errs(&store).is_empty(), "2 mm is like 1 mm: {:?}", errs(&store));
    store.apply(tx("unit", vec![Op::Set { id: l, path: path("let.value"), slot: Slot::Const(q("1 s")) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(errs(&store).iter().any(|m| m.contains("unit")), "the check reads args.xs, now in seconds: {:?}", errs(&store));
}

/// T2: a definition's port_spec datatype set: the instance's bound argument is checked again.
#[test]
fn a_port_spec_datatype_set_checks_instances_arguments_again() {
    let mut store = Store::new();
    let (_, def) = library_def(&mut store);
    let ids = store.authored().next_ids(3);
    let [spec, model, e] = ids.try_into().unwrap();
    let mut port = Port::default();
    port.kind = Slot::Const(Value::Str("value".into()));
    port.dir = Slot::Const(Value::Enum("in".into()));
    port.signal = Slot::Const(Value::Number(2.0));
    port.datatype = Slot::Const(Value::Str("number".into()));
    store.apply(tx("make", vec![
        Op::Create { id: spec, ty: ty("port_spec"), parent: Some(def), order: OrderKey::first(), facets: vec![name("n"), port.wrap()] },
        Op::Create { id: model, ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
        Op::Create { id: e, ty: ty("entity"), parent: Some(model), order: OrderKey::first(), facets: vec![inst(def, "linked"), { let mut a = facet::Args::default(); a.insert("n", Slot::Bound(Expr::parse("2 + 3").unwrap())); a.wrap() }] },
    ])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(errs(&store).is_empty(), "{:?}", errs(&store));
    store.apply(tx("retype", vec![Op::Set { id: spec, path: path("port.datatype"), slot: Slot::Const(Value::Str("bool".into())) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(errs(&store).iter().any(|m| m.contains("expects bool")), "the instance's argument is checked again: {:?}", errs(&store));
}

/// T3: a deref cycle broken by rebinding the other end.
#[test]
fn a_deref_cycle_broken_by_a_rebind_recovers() {
    let (mut store, comp) = sheet();
    let built = markup::ops(r#"<rect id="$a" w="1" h="1"/><rect id="$b" w="1" h="1"/>"#, Some(comp), OrderKey::first(), store.authored().next_id()).unwrap();
    let (a, b) = (built.named["a"], built.named["b"]);
    store.apply(tx("rects", built.ops)).unwrap();
    store.apply(tx("cycle", vec![
        Op::Set { id: a, path: path("size.w"), slot: Slot::Bound(Expr::parse(&format!("deref({b}, 'size.w')")).unwrap()) },
        Op::Set { id: b, path: path("size.w"), slot: Slot::Bound(Expr::parse(&format!("{a}.size.w + 1")).unwrap()) },
    ])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(errs(&store).iter().any(|m| m.contains("cycle")), "{:?}", errs(&store));
    store.apply(tx("break", vec![Op::Set { id: b, path: path("size.w"), slot: Slot::Bound(Expr::parse("5").unwrap()) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(errs(&store).is_empty(), "the cycle has ended: {:?}", errs(&store));
    assert_eq!(store.resolved(a, path("size.w")), Some(Value::Number(5.0)));
}

/// T4: two providers reordered, alongside an unrelated create.
#[test]
fn reordering_providers_with_another_op_resolves_the_name_again() {
    let (mut store, comp) = sheet();
    let built = markup::ops(r#"<group id="$g" name="g"><let id="$a" name="c" provide="yes" value="1"/><let id="$b" name="c" provide="yes" value="2"/><rect id="$r" w="{= @c}" h="1"/></group>"#, Some(comp), OrderKey::first(), store.authored().next_id()).unwrap();
    let (a, b, r) = (built.named["a"], built.named["b"], built.named["r"]);
    store.apply(tx("make", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.resolved(r, path("size.w")), Some(Value::Number(1.0)));
    let first = store.tree().live(a).unwrap().order.clone();
    let id = store.authored().next_id();
    store.apply(tx("reorder", vec![
        Op::Reorder { id: b, order: OrderKey::before(&first) },
        Op::Create { id, ty: ty("group"), parent: Some(comp), order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![] },
    ])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.resolved(r, path("size.w")), Some(Value::Number(2.0)), "the first provider is now b");
}

/// A reorder alone that puts another provider first resolves `@name` again
/// (a shared fix: the old engine treated a reorder as no structure).
#[test]
fn reordering_providers_alone_resolves_the_name_again() {
    let (mut store, comp) = sheet();
    let built = markup::ops(r#"<group id="$g" name="g"><let id="$a" name="c" provide="yes" value="1"/><let id="$b" name="c" provide="yes" value="2"/><rect id="$r" w="{= @c}" h="1"/></group>"#, Some(comp), OrderKey::first(), store.authored().next_id()).unwrap();
    let (a, b, r) = (built.named["a"], built.named["b"], built.named["r"]);
    store.apply(tx("make", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let first = store.tree().live(a).unwrap().order.clone();
    store.apply(tx("reorder", vec![Op::Reorder { id: b, order: OrderKey::before(&first) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.resolved(r, path("size.w")), Some(Value::Number(2.0)));
}
