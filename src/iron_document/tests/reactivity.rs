//! Reactivity phases 1 and 2 (plans/specs/reactivity.md): errors that
//! propagate, presentation state through lets, problems as data, and scopes:
//! items as nodes, components in repeats, structure that settles live.

use iron_document::document::{RelKind, Relation};
use iron_document::facet::{Composition, Name};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn bind(e: &str) -> Slot {
    Slot::Bound(Expr::parse(e).unwrap())
}

/// A 2D composition named `sheet` holding `markup`, and a model root.
fn sheet(markup: &str) -> (Store, NodeId, BTreeMap<String, NodeId>) {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("sheet".into()));
    store
        .apply(tx(
            "sheet",
            vec![
                Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] },
                Op::Create { id: ids[1], ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
            ],
        ))
        .unwrap();
    let built = markup::ops(markup, Some(ids[0]), OrderKey::first(), store.document().next_id()).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("draw", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    (store, ids[0], built.named)
}

/// A repeat's items, in order.
fn items(store: &Store, g: NodeId) -> Vec<NodeId> {
    let doc = store.document();
    let mut items: Vec<NodeId> = doc.children(g).iter().copied().filter(|c| c.is_item_scope() && doc.is_live(*c)).collect();
    items.sort_by_key(|c| c.item_of(g));
    items
}

fn x_of(store: &Store, id: NodeId) -> f64 {
    store.resolved(id, path("transform2d.x")).unwrap().as_f64().unwrap()
}

#[test]
fn a_failed_binding_fails_its_readers_and_recovers() {
    let (mut store, _, n) = sheet(r#"<let id="a" value="{= 2 mW * 3}"/><let id="b" value="{= $a * 2}"/><rect id="r" x="{= $b / 1 mW}" w="10" h="10"/>"#);
    let (a, b, r) = (n["a"], n["b"], n["r"]);
    assert_eq!(x_of(&store, r), 12.0);
    assert!(store.reactive().errors.is_empty());
    // Adding a unit to a plain number fails; what reads it fails with it.
    store.apply(tx("break", vec![Op::Set { id: a, path: path("let.value"), slot: bind("2 mW + 3") }])).unwrap();
    let errors = &store.reactive().errors;
    assert!(errors.contains_key(&Leaf { node: a, path: path("let.value") }));
    let downstream = &errors[&Leaf { node: r, path: path("transform2d.x") }];
    assert!(downstream.starts_with(&format!("depends on {a}.let.value")), "{downstream}");
    assert!(errors.contains_key(&Leaf { node: b, path: path("let.value") }));
    // What is drawn keeps its last good value, and the view says so.
    assert_eq!(x_of(&store, r), 12.0);
    let view = store.view(&ViewQuery { omit_defaults: true, relation: Default::default(), scope: None, fidelity: Fidelity::Full, depth: None, expand: vec![] }).unwrap();
    assert!(view.contains("problem=") && view.contains("depends on"), "{view}");
    // Mending it to the same value clears every error.
    store.apply(tx("mend", vec![Op::Set { id: a, path: path("let.value"), slot: bind("3 mW * 2") }])).unwrap();
    assert!(store.reactive().errors.is_empty(), "{:?}", store.reactive().errors);
    assert!(store.problems().is_empty());
}

#[test]
fn hover_reaches_a_look_through_a_let_and_never_content() {
    let (mut store, _, n) = sheet(r#"<rect id="r" x="0" w="10" h="10" interaction.hovered="false" fill.opacity="1"/><let id="lit" value="1"/><rect id="s" x="0" w="10" h="10" fill.opacity="1"/>"#);
    let (r, lit, s) = (n["r"], n["lit"], n["s"]);
    let hover = format!("{r}.interaction.hovered ? 1 : 0.4");
    store.apply(tx("shared", vec![Op::Set { id: lit, path: path("let.value"), slot: bind(&hover) }])).unwrap();
    store.apply(tx("look", vec![Op::Set { id: s, path: path("fill.opacity"), slot: bind(&format!("{lit}.let.value")) }])).unwrap();
    // Content may not read it, however it gets there.
    let refused = store.apply(tx("content", vec![Op::Set { id: s, path: path("transform2d.x"), slot: bind(&format!("{lit}.let.value * 10")) }]));
    assert!(refused.is_err());
    // Nor may a let that content already reads start carrying it.
    let (mut store, _, n) = sheet(r#"<rect id="r" x="0" w="10" h="10" interaction.hovered="false"/><let id="k" value="1"/><rect id="s" x="{= $k * 10}" w="10" h="10"/>"#);
    let refused = store.apply(tx("taint", vec![Op::Set { id: n["k"], path: path("let.value"), slot: bind(&format!("{}.interaction.hovered ? 1 : 0", n["r"])) }]));
    assert!(refused.is_err());
}

#[test]
fn nesting_past_sixteen_is_reported_and_still_drawn() {
    // 17 repeats, each inside the last, one item each.
    let depth = 17;
    let mut markup = String::new();
    for i in 0..depth {
        markup.push_str(&format!(r#"<group name="level {i}" repeat.count="1">"#));
    }
    markup.push_str(r#"<rect name="leaf" x="1" w="10" h="10"/>"#);
    markup.push_str(&"</group>".repeat(depth));
    let (store, comp, _) = sheet(&markup);
    let doc = store.document();
    let top = doc.children(comp)[0];
    let leaves: Vec<NodeId> = doc.descendants(top).into_iter().filter(|&d| d.nesting() == depth && doc.get::<Name>(d).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some("leaf")).collect();
    assert_eq!(leaves.len(), 1, "the deepest copy is drawn");
    let problems = store.problems();
    assert!(problems.iter().any(|p| p.kind == "depth"), "{problems:?}");
    assert!(store.problem_notes().get(&top).is_some_and(|notes| notes.iter().any(|m| m.contains("16 deep"))), "{:?}", store.problem_notes());
}

/// A component in a repeat's template: each item has its own instance,
/// reading the item, and its own picture.
#[test]
fn a_repeat_of_a_component_gives_each_item_its_own_instance() {
    let (mut store, _, _) = sheet(r#"<group name="spare"/>"#);
    // Copy std:meter's definition in by placing one.
    let (ops, _) = compose::compose_ops(&store, r#"<place def="std:meter" in="sheet" x="400" y="0"/>"#).unwrap();
    store.apply(tx("place", ops)).unwrap();
    let def = standard::find(store.document(), "meter").unwrap();
    let comp = store.document().roots()[0];
    let markup = format!(
        r#"<group id="g" name="meters" repeat.each="5 10 15">
             <entity id="e" name="m" instance.def="{def}" instance.mode="linked" args.in="{{= $g.repeat.item * 1 mW}}" args.range="20 mW"/>
             <use id="u" name="m" notation="free" x="{{= $g.repeat.index * 130}}" y="200"/>
           </group>"#
    );
    let built = markup::ops(&markup, Some(comp), OrderKey::after(&OrderKey::after(&OrderKey::first())), store.document().next_id()).unwrap_or_else(|e| panic!("{e}"));
    let (g, e, u) = (built.named["g"], built.named["e"], built.named["u"]);
    let mut ops = built.ops;
    ops.push(Op::Link { rel: Relation { id: store.document().next_relation_id(), from: u, to: e, rel: RelKind::Figures, data: BTreeMap::new() } });
    store.apply(tx("meters", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let doc = store.document();
    let items = items(&store, g);
    assert_eq!(items.len(), 3);
    let readouts: Vec<String> = items
        .iter()
        .map(|&i| {
            let readout = doc.descendants(i).into_iter().find(|&d| doc.get::<Name>(d).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some("readout")).expect("the item's picture has a readout");
            store.resolved(readout, path("text.content")).unwrap().as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(readouts, vec!["5.00 mW", "10.00 mW", "15.00 mW"]);
    let own: Vec<String> = store.problems().into_iter().filter(|p| p.node.item_of(g).is_some()).map(|p| format!("{}: {}", p.node, p.message)).collect();
    assert!(own.is_empty(), "{own:#?}");
}

/// Structure follows values outside transactions: a preview (a drag) and a
/// producer's publish both add items, with no history entry.
#[test]
fn items_follow_a_drag_and_a_publish_without_a_transaction() {
    let (mut store, _, n) = sheet(r#"<slider id="s" x="0" y="0" min="0" max="10" step="1" value="2"/><group id="g" repeat.count="{= $s.slider.value}"><rect x="{= $g.repeat.index * 20}" w="10" h="10"/></group><let id="data" value="1 2"/><group id="h" repeat.each="{= $data}"><rect x="{= $h.repeat.item}" w="5" h="5"/></group>"#);
    let (s, g, data, h) = (n["s"], n["g"], n["data"], n["h"]);
    assert_eq!(items(&store, g).len(), 2);
    let version = store.document().version();
    let _ = store.take_derived_ops();
    // A drag previews the slider's value.
    store.preview(s, path("slider.value"), Value::Number(5.0));
    assert_eq!(items(&store, g).len(), 5);
    assert_eq!(x_of(&store, store.document().children(items(&store, g)[4])[0]), 80.0);
    assert!(!store.take_derived_ops().is_empty(), "the host is told what to materialise");
    assert_eq!(store.document().version(), version, "no transaction");
    store.unpreview(s, path("slider.value"));
    assert_eq!(items(&store, g).len(), 2);
    // A producer publishes a longer series onto the data.
    let live = store.register_producer("live");
    store.publish(live, BTreeMap::from([(Leaf { node: data, path: path("let.value") }, Value::series(vec![1.0, 2.0, 3.0, 4.0], Dim::NONE))]));
    assert_eq!(items(&store, h).len(), 4);
    assert_eq!(store.document().version(), version);
    assert!(store.reactive().errors.is_empty(), "{:?}", store.reactive().errors);
}

/// Each item reads its own value: changing one element re-evaluates that
/// item's copy and nothing else.
#[test]
fn changing_one_element_re_evaluates_one_item() {
    let (mut store, _, n) = sheet(r#"<let id="data" value="10 20 30 40"/><group id="g" repeat.each="{= $data}"><rect x="{= $g.repeat.item * 2}" w="5" h="5"/></group>"#);
    let (data, g) = (n["data"], n["g"]);
    let xs = |s: &Store| items(s, g).iter().map(|&i| x_of(s, s.document().children(i)[0])).collect::<Vec<_>>();
    assert_eq!(xs(&store), vec![20.0, 40.0, 60.0, 80.0]);
    let before = store.reactive().evaluations;
    store.apply(tx("one", vec![Op::Set { id: data, path: path("let.value"), slot: Slot::Const(Value::series(vec![10.0, 20.0, 99.0, 40.0], Dim::NONE)) }])).unwrap();
    assert_eq!(xs(&store), vec![20.0, 40.0, 198.0, 80.0]);
    // The repeat's `each`, and the third item's x.
    assert_eq!(store.reactive().evaluations - before, 2);
}
