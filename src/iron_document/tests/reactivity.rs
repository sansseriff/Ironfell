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
    let items: Vec<NodeId> = doc.children(g).iter().copied().filter(|c| c.is_item_scope() && doc.is_live(*c)).collect();
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

// ---- Phase 3: lists, records, keys, `when` -------------------------------

fn text_of(store: &Store, id: NodeId) -> String {
    store.resolved(id, path("text.content")).unwrap().as_str().unwrap().to_owned()
}

fn named(store: &Store, under: NodeId, name: &str) -> Vec<NodeId> {
    let doc = store.document();
    // Drawn copies only: a derived repeat's or conditional's own template is not one.
    doc.descendants(under).into_iter().filter(|&d| d.is_expanded() && !expand::in_repeat_template(doc, d) && doc.get::<Name>(d).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).collect()
}

#[test]
fn lists_and_records_are_values_of_the_floor() {
    let read = |_: Leaf| -> Option<Value> { None };
    let ev = |t: &str| Expr::parse(t).unwrap_or_else(|e| panic!("{t}: {e}")).eval(&read).unwrap_or_else(|e| panic!("{t}: {e}"));
    assert_eq!(ev("len(list('a', 'b', 'c'))"), Value::Number(3.0));
    assert_eq!(ev("at(list('a', 'b', 'c'), 1)"), Value::Str("b".into()));
    assert_eq!(ev("record(id: 7, label: 'seven').label"), Value::Str("seven".into()));
    assert_eq!(ev("get(record(id: 7), 'missing', 0)"), Value::Number(0.0));
    assert_eq!(ev("map(list(1, 2, 3), x => x * 2)"), Value::list(vec![Value::Number(2.0), Value::Number(4.0), Value::Number(6.0)]));
    // Over a series, numbers in one unit stay a series.
    assert_eq!(ev("filter(range(0, 5, 6), x => x > 2)"), Value::series(vec![3.0, 4.0, 5.0], Dim::NONE));
    assert_eq!(ev("first(sort_by(list(record(n: 3), record(n: 1)), r => r.n)).n"), Value::Number(1.0));
    assert_eq!(ev("len(unique(list('a', 'b', 'a')))"), Value::Number(2.0));
    assert_eq!(ev("at(enumerate(list('x', 'y')), 1).index"), Value::Number(1.0));
    assert_eq!(ev("at(at(zip(list('x', 'y'), series(1, 2)), 1), 1)"), Value::Number(2.0));
    // Written and read back the same.
    for text in ["list(#3, #4/~beta/7)", "list('a', 'b')", "record(id: 7, label: 'seven')", "map(list(1, 2), x => x * 2)", "deref(#3.let.value, 'size.w', 0)"] {
        assert_eq!(Expr::parse(text).unwrap().to_string(), text);
    }
    // A list survives a save.
    let v = ev("list('a', record(id: 1), 2 mW)");
    assert_eq!(Value::from_json(&v.to_json()).unwrap(), v);
}

/// A legend over a list of words: one label per word, keyed by the word,
/// so a word added at the front leaves every other item's node, and what
/// is attached to it, where it was.
#[test]
fn a_legend_over_a_list_keeps_each_items_identity_when_one_is_added_in_front() {
    let (mut store, _, n) = sheet(r#"<let id="names" value="{= list('alpha', 'beta', 'gamma')}"/><group id="g" name="legend" repeat.each="{= $names}"><let id="open" name="open" value="false"/><text name="label" x="0" y="{= $g.repeat.index * 20}" content="{= $g.repeat.item}"/></group>"#);
    let (names, g) = (n["names"], n["g"]);
    let labels = |s: &Store| items(s, g).iter().map(|&i| text_of(s, named(s, i, "label")[0])).collect::<Vec<_>>();
    assert_eq!(labels(&store), vec!["alpha", "beta", "gamma"]);
    let beta = items(&store, g)[1];
    assert_eq!(beta.to_string(), format!("{g}/~beta"));
    // Per-item state: set on the copy, stored as the repeat's override.
    let beta_open = named(&store, beta, "open")[0];
    store.apply(tx("open beta", vec![Op::Set { id: beta_open, path: path("let.value"), slot: Slot::Const(Value::Bool(true)) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.resolved(beta_open, path("let.value")), Some(Value::Bool(true)));
    store.set_selection(&[(beta, BTreeMap::new())]).unwrap();
    // A word goes in at the front.
    store.apply(tx("add", vec![Op::Set { id: names, path: path("let.value"), slot: bind("list('zero', 'alpha', 'beta', 'gamma')") }])).unwrap();
    assert_eq!(labels(&store), vec!["zero", "alpha", "beta", "gamma"]);
    assert_eq!(items(&store, g)[2], beta, "the same node, one place on");
    assert_eq!(store.resolved(beta, path("item.index")), Some(Value::Number(2.0)));
    assert_eq!(store.resolved(beta_open, path("let.value")), Some(Value::Bool(true)), "its state came with it");
    assert_eq!(store.selection().iter().map(|s| s.0).collect::<Vec<_>>(), vec![beta], "and its selection");
    let others: Vec<bool> = items(&store, g).iter().map(|&i| store.resolved(named(&store, i, "open")[0], path("let.value")) == Some(Value::Bool(true))).collect();
    assert_eq!(others, vec![false, false, true, false]);
    // The override is saved; derived nodes are not.
    let again = Store::load(&store.save()).unwrap();
    assert_eq!(again.resolved(beta_open, path("let.value")), Some(Value::Bool(true)));
    // Two items with one key: reported, and drawn by index.
    store.apply(tx("dup", vec![Op::Set { id: g, path: path("repeat.key"), slot: bind("w => 'same'") }])).unwrap();
    assert_eq!(labels(&store).len(), 4);
    assert!(store.problem_notes().get(&g).is_some_and(|m| m.iter().any(|m| m.contains("share the key"))), "{:?}", store.problem_notes());
}

/// Tick labels keyed by value: when the range moves, a tick that is still
/// there keeps its node; new ones are made and old ones go.
#[test]
fn ticks_keyed_by_value_keep_the_labels_that_stay() {
    let (mut store, _, n) = sheet(r#"<let id="lo" value="0"/><group id="g" repeat.each="{= range($lo, $lo + 30, 4)}" repeat.key="{= t => t}"><text name="tick" x="{= $g.repeat.item}" y="0" content="{= fmt($g.repeat.item)}"/></group>"#);
    let (lo, g) = (n["lo"], n["g"]);
    let ids = |s: &Store| items(s, g).iter().map(|i| i.to_string()).collect::<Vec<_>>();
    assert_eq!(ids(&store), vec![format!("{g}/~0"), format!("{g}/~10"), format!("{g}/~20"), format!("{g}/~30")]);
    let twenty = items(&store, g)[2];
    store.apply(tx("slide", vec![Op::Set { id: lo, path: path("let.value"), slot: Slot::Const(Value::Number(10.0)) }])).unwrap();
    assert_eq!(ids(&store), vec![format!("{g}/~10"), format!("{g}/~20"), format!("{g}/~30"), format!("{g}/~40")]);
    assert_eq!(items(&store, g)[1], twenty);
    assert_eq!(store.resolved(twenty, path("item.index")), Some(Value::Number(1.0)));
    assert!(!store.document().is_live(NodeId::item_scope(g, "0").unwrap()));
}

/// `when`: the children exist only while the condition holds.
#[test]
fn a_conditional_groups_children_exist_only_while_it_holds() {
    let (mut store, _, n) = sheet(r#"<let id="show" value="false"/><group id="g" name="detail" when="{= $show}"><text name="body" x="0" y="0" content="more"/></group>"#);
    let (show, g) = (n["show"], n["g"]);
    assert!(named(&store, g, "body").is_empty());
    store.apply(tx("show", vec![Op::Set { id: show, path: path("let.value"), slot: Slot::Const(Value::Bool(true)) }])).unwrap();
    let body = named(&store, g, "body");
    assert_eq!(body.len(), 1);
    assert_eq!(body[0].to_string().split('/').nth(1), Some("~on"));
    let view = store.view(&ViewQuery { omit_defaults: true, relation: Default::default(), scope: None, fidelity: Fidelity::Full, depth: None, expand: vec![] }).unwrap();
    assert!(view.contains(r#"copies="1""#), "{view}");
    store.undo(0).unwrap().unwrap();
    assert!(named(&store, g, "body").is_empty());
    assert!(!store.save().contains('~'), "nothing derived is saved");
}

// ---- Phase 4: state and actions -----------------------------------------

/// An accordion as a component, in markup: a click on its header toggles
/// its own `open` state, which a conditional shows its body by. Each
/// instance has its own; one click is one undo step.
#[test]
fn an_accordion_component_toggles_per_instance_and_undoes_one_click() {
    let (mut store, comp, _) = sheet(r#"<group name="spare"/>"#);
    let library = store.document().next_id();
    store.apply(tx("library", vec![Op::Create { id: library, ty: ty("library"), parent: None, order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![] }])).unwrap();
    let def_markup = r##"<definition name="accordion">
        <port_spec id="title" name="title" kind="value" dir="in" signal="Section"/>
        <template notation="free">
          <let id="open" name="open" value="false"/>
          <let id="hot" name="hot" keep="session" value="false"/>
          <group id="head" name="head">
            <rect name="bar" x="0" y="0" w="200" h="24" fill.color="{= $hot ? rgb(0.8, 0.9, 1) : rgb(0.9, 0.9, 0.9)}"/>
            <text name="title" x="8" y="4" content="{= $title}"/>
            <on name="toggle section" event="click" target="$open" to="{= !$open}"/>
            <on event="enter" target="$hot" to="true"/>
            <on event="leave" target="$hot" to="false"/>
          </group>
          <group name="body" when="{= $open}">
            <text name="content" x="8" y="30" content="{= 'About ' + $title}"/>
          </group>
        </template>
      </definition>"##;
    let built = markup::ops(def_markup, Some(library), OrderKey::first(), store.document().next_id()).unwrap_or_else(|e| panic!("{e}"));
    let def = built.roots[0];
    store.apply(tx("define", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let place = format!(r#"<place def="{def}" in="sheet" x="0" y="0" name="A" title="One"/><place def="{def}" in="sheet" x="0" y="100" name="B" title="Two"/>"#);
    let (ops, _) = compose::compose_ops(&store, &place).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("place", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let uses: Vec<NodeId> = store.document().children(comp).iter().copied().filter(|&c| store.document().live(c).is_some_and(|n| n.ty.name() == "use")).collect();
    assert_eq!(uses.len(), 2);
    let (a, b) = (uses[0], uses[1]);
    let content = |s: &Store, u: NodeId| named(s, u, "content").iter().map(|&c| text_of(s, c)).collect::<Vec<_>>();
    assert!(content(&store, a).is_empty() && content(&store, b).is_empty());
    // A click on the bar reaches the header's action.
    let bar = named(&store, a, "bar")[0];
    let before = store.history().past().len();
    store.fire(bar, "click", Actor::Human, 0).unwrap().expect("a transaction");
    assert_eq!(content(&store, a), vec!["About One"]);
    assert!(content(&store, b).is_empty(), "the other instance has its own state");
    assert_eq!(store.history().past().len(), before + 1);
    assert_eq!(store.history().past().last().unwrap().label, "toggle section");
    // B opens too; then one undo closes only B.
    store.fire(named(&store, b, "title")[0], "click", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(content(&store, b), vec!["About Two"]);
    store.undo(0).unwrap().unwrap();
    assert!(content(&store, b).is_empty());
    assert_eq!(content(&store, a), vec!["About One"]);
    // A second click closes A. The state survives a save.
    let again = Store::load(&store.save()).unwrap();
    assert_eq!(content(&again, a), vec!["About One"]);
    store.fire(bar, "click", Actor::Human, 0).unwrap().unwrap();
    assert!(content(&store, a).is_empty());

    // Hover: session state. It changes the look, with no transaction and
    // nothing saved.
    let colour = |s: &Store| s.resolved(bar, path("fill.color")).unwrap();
    let cold = colour(&store);
    let (version, steps, saved) = (store.document().version(), store.history().past().len(), store.save());
    assert_eq!(store.hover_listeners(bar).len(), 1);
    assert!(store.fire(bar, "enter", Actor::Human, 0).unwrap().is_none());
    assert_ne!(colour(&store), cold);
    assert_eq!(store.resolved(named(&store, b, "bar")[0], path("fill.color")).unwrap(), cold, "only this instance");
    assert_eq!((store.document().version(), store.history().past().len(), store.save()), (version, steps, saved));
    store.fire(bar, "leave", Actor::Human, 0).unwrap();
    assert_eq!(colour(&store), cold);
}

/// An action's writes are computed before any is applied, and a counter
/// steps by its `by`.
#[test]
fn an_actions_writes_are_parallel_and_one_step() {
    let (mut store, _, n) = sheet(r#"<let id="a" value="1"/><let id="b" value="2"/><let id="count" value="0"/>
      <group id="btn" name="swap"><rect name="face" x="0" y="0" w="40" h="20"/>
        <on name="swap" event="click"><set target="$a" to="{= $b}"/><set target="$b" to="{= $a}"/><set target="$count" verb="step" by="5"/></on>
      </group>"#);
    let val = |s: &Store, id: NodeId| s.resolved(id, path("let.value")).unwrap().as_f64().unwrap();
    let face = named_any(&store, n["btn"], "face");
    store.fire(face, "click", Actor::Human, 0).unwrap().unwrap();
    assert_eq!((val(&store, n["a"]), val(&store, n["b"]), val(&store, n["count"])), (2.0, 1.0, 5.0));
    store.undo(0).unwrap().unwrap();
    assert_eq!((val(&store, n["a"]), val(&store, n["b"]), val(&store, n["count"])), (1.0, 2.0, 0.0));
}

fn named_any(store: &Store, under: NodeId, name: &str) -> NodeId {
    let doc = store.document();
    doc.descendants(under).into_iter().find(|&d| doc.get::<Name>(d).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
}

// ---- Phase 5: tracked reads ---------------------------------------------

/// An inspector line: the width of whatever is selected. It follows the
/// selection, and the selected node's width.
#[test]
fn a_readout_of_the_selected_nodes_width_follows_selection_and_resize() {
    let (mut store, _, n) = sheet(r#"<rect id="r1" x="0" y="0" w="30" h="10"/><rect id="r2" x="50" y="0" w="70" h="10"/><let id="sel" value="0" selection.count="0"/><text id="t" x="0" y="40" content="{= $sel.selection.count == 0 ? 'nothing' : 'w = ' + fmt(deref($sel.selection.node, 'size.w', 0))}"/>"#);
    let (r1, r2, t) = (n["r1"], n["r2"], n["t"]);
    assert_eq!(text_of(&store, t), "nothing");
    store.set_selection(&[(r1, BTreeMap::new())]).unwrap();
    assert_eq!(text_of(&store, t), "w = 30");
    // The selected node is resized: the readout follows, by the tracked
    // dependency, with one evaluation.
    let before = store.reactive().evaluations;
    store.apply(tx("resize", vec![Op::Set { id: r1, path: path("size.w"), slot: Slot::Const(Value::Number(45.0)) }])).unwrap();
    assert_eq!(text_of(&store, t), "w = 45");
    assert_eq!(store.reactive().evaluations - before, 1);
    // During a drag too.
    store.preview(r1, path("size.w"), Value::Number(60.0));
    assert_eq!(text_of(&store, t), "w = 60");
    store.unpreview(r1, path("size.w"));
    // Another node is selected: the readout no longer follows the first.
    store.set_selection(&[(r2, BTreeMap::new())]).unwrap();
    assert_eq!(text_of(&store, t), "w = 70");
    let before = store.reactive().evaluations;
    store.apply(tx("resize the other", vec![Op::Set { id: r1, path: path("size.w"), slot: Slot::Const(Value::Number(5.0)) }])).unwrap();
    assert_eq!(store.reactive().evaluations, before, "no longer a dependency");
    store.set_selection(&[]).unwrap();
    assert_eq!(text_of(&store, t), "nothing");
    assert!(store.reactive().errors.is_empty(), "{:?}", store.reactive().errors);
}

/// A tracked read follows a reference held in state, reads bound leaves in
/// the right order, and a read that reaches itself is an error on the leaf.
#[test]
fn a_tracked_read_follows_a_reference_and_reports_a_cycle() {
    let (mut store, _, n) = sheet(r#"<let id="k" value="2"/><rect id="r1" x="0" y="0" w="{= $k * 10}" h="10"/><rect id="r2" x="0" y="20" w="{= $k * 100}" h="10"/><let id="cur" value="0"/><let id="w" value="{= deref($cur, 'size.w', -1)}"/><let id="twice" value="{= $w * 2}"/>"#);
    let (k, r1, r2, cur, w, twice) = (n["k"], n["r1"], n["r2"], n["cur"], n["w"], n["twice"]);
    let val = |s: &Store, id: NodeId| s.resolved(id, path("let.value")).unwrap().as_f64().unwrap();
    assert_eq!(val(&store, w), -1.0, "the fallback: no node");
    store.apply(tx("point", vec![Op::Set { id: cur, path: path("let.value"), slot: Slot::Const(Value::Ref(r1)) }])).unwrap();
    assert_eq!((val(&store, w), val(&store, twice)), (20.0, 40.0));
    // What it reads is itself bound: one edit upstream flows through.
    store.apply(tx("k", vec![Op::Set { id: k, path: path("let.value"), slot: Slot::Const(Value::Number(3.0)) }])).unwrap();
    assert_eq!((val(&store, w), val(&store, twice)), (30.0, 60.0));
    store.apply(tx("repoint", vec![Op::Set { id: cur, path: path("let.value"), slot: Slot::Const(Value::Ref(r2)) }])).unwrap();
    assert_eq!((val(&store, w), val(&store, twice)), (300.0, 600.0));
    assert!(store.reactive().transitive_deps(Leaf { node: twice, path: path("let.value") }).contains(&Leaf { node: r2, path: path("size.w") }));
    // A node whose own width reads the tracked value: pointing at it is a
    // cycle, reported on the leaf, not a hang.
    let (mut store, _, n) = sheet(r#"<let id="cur" value="0"/><let id="w" value="{= deref($cur, 'size.w', 1)}"/><rect id="r" x="0" y="0" w="{= $w + 1}" h="10"/>"#);
    store.apply(tx("loop", vec![Op::Set { id: n["cur"], path: path("let.value"), slot: Slot::Const(Value::Ref(n["r"])) }])).unwrap();
    let why = store.reactive().errors.get(&Leaf { node: n["w"], path: path("let.value") }).cloned().unwrap_or_default();
    assert!(why.contains("cycle"), "{:?}", store.reactive().errors);
}

/// Phase 6 (reactivity.md §6): `@name` reads the nearest provider above the
/// reader. A component reads it with no port in between; a default inside
/// the component serves where nothing above provides; a group may shadow a
/// colour token for what is under it.
#[test]
fn a_component_reads_context_from_wherever_it_is_placed() {
    let (mut store, _, named_ids) = sheet(
        r##"<group id="plain" name="plain"/>
        <group id="wide" name="wide"><let name="gap" provide="yes" value="40"/></group>
        <group id="night" name="night"><let id="ink" name="ink" provide="yes" value="#ffffffff"/><rect id="r" name="r" w="4" h="4" fill.color="@ink"/></group>
        <rect id="day" name="day" w="4" h="4" fill.color="@ink"/>"##,
    );
    let library = store.document().next_id();
    store.apply(tx("library", vec![Op::Create { id: library, ty: ty("library"), parent: None, order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![] }])).unwrap();
    let def_markup = r##"<definition name="spaced">
        <template notation="free">
          <let name="gap" provide="default" value="8"/>
          <rect name="box" x="{= @gap}" y="0" w="10" h="10"/>
        </template>
      </definition>"##;
    let built = markup::ops(def_markup, Some(library), OrderKey::first(), store.document().next_id()).unwrap_or_else(|e| panic!("{e}"));
    let def = built.roots[0];
    store.apply(tx("define", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let (plain, wide) = (named_ids["plain"], named_ids["wide"]);
    let place = format!(r#"<place def="{def}" in="{plain}" name="A"/><place def="{def}" in="{wide}" name="B"/>"#);
    let (ops, _) = compose::compose_ops(&store, &place).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("place", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let box_x = |s: &Store, under: NodeId| x_of(s, named(s, under, "box")[0]);
    assert_eq!(box_x(&store, plain), 8.0, "the component's own default");
    assert_eq!(box_x(&store, wide), 40.0, "the provider above it");
    // The provider's value is live, and ceasing to provide falls back.
    let gap = store.document().children(wide).iter().copied().find(|&c| store.document().live(c).is_some_and(|n| n.ty.name() == "let")).unwrap();
    store.apply(tx("gap", vec![Op::Set { id: gap, path: path("let.value"), slot: Slot::Const(Value::Number(60.0)) }])).unwrap();
    assert_eq!(box_x(&store, wide), 60.0);
    store.apply(tx("stop", vec![Op::Set { id: gap, path: path("let.provide"), slot: Slot::Const(Value::Enum("no".into())) }])).unwrap();
    assert_eq!(box_x(&store, wide), 8.0);
    // A group's own `ink` shadows the theme's for what is under it.
    let fill = |s: &Store, n: NodeId| s.resolved(n, path("fill.color")).unwrap();
    assert_eq!(fill(&store, named_ids["r"]), Value::Color([1.0, 1.0, 1.0, 1.0]));
    assert_ne!(fill(&store, named_ids["day"]), Value::Color([1.0, 1.0, 1.0, 1.0]), "the theme's ink");
    // Nothing provides it: a problem on the leaf, not a silent value.
    let err = store.apply(tx("lost", vec![Op::Set { id: named_ids["day"], path: path("transform2d.x"), slot: bind("@nowhere") }]));
    assert!(err.is_err() || store.problems().iter().any(|p| p.message.contains("nothing provides @nowhere")), "{:?}", store.problems());
}

/// Phase 6 (reactivity.md §8): a trace gives a value as it was a while
/// ago, and where along a path it was on; an accumulator steps a formula
/// of its own past. Both run on presentation time and save nothing.
#[test]
fn a_trace_delays_a_click_and_an_accumulator_integrates() {
    let (mut store, _, n) = sheet(
        r##"<let id="lit" name="lit" value="false"/>
        <trace id="tr" name="tr" of="{= $lit}" delay="2"/>
        <rect id="lamp" name="lamp" w="4" h="4" fill.opacity="{= $tr ? 1 : 0.2}"/>
        <path id="glow" name="glow" stroke.path_length="1" stroke.cap="butt" stroke.dash="{= $tr.trace.dash}"/>
        <let id="rate" name="rate" value="3"/>
        <accumulate id="acc" name="acc" initial="10" next="{= $acc + $rate * $acc.accumulate.dt}"/>
        <text id="t" name="t" content="{= fmt($acc, 2)}"/>"##,
    );
    let version = store.history().past().len();
    let out = |s: &Store| s.resolved(n["tr"], path("trace.out")).unwrap();
    let dash = |s: &Store| s.resolved(n["glow"], path("stroke.dash")).unwrap().as_str().unwrap().to_owned();
    let acc = |s: &Store| s.resolved(n["acc"], path("accumulate.value")).unwrap().as_f64().unwrap();
    assert!(store.advance_steps(0.0).contains(&n["acc"]), "the accumulator starts");
    assert_eq!((out(&store), dash(&store).as_str()), (Value::Bool(false), "0 1"));
    assert_eq!(acc(&store), 10.0);
    // On at 1 s: the far end still shows off; the lit run grows from the start.
    store.apply(tx("on", vec![Op::Set { id: n["lit"], path: path("let.value"), slot: Slot::Const(Value::Bool(true)) }])).unwrap();
    assert!(store.advance_steps(1.0).contains(&n["tr"]));
    store.advance_steps(1.05);
    let a = acc(&store);
    store.advance_steps(1.1);
    assert!((acc(&store) - (a + 3.0 * 0.05)).abs() < 1e-9, "a step of 0.05 s at 3 a second");
    store.advance_steps(1.5);
    assert_eq!(out(&store), Value::Bool(false));
    assert_eq!(dash(&store), "0.2500 1");
    // Off at 2 s: a pulse one second long travels the path.
    store.apply(tx("off", vec![Op::Set { id: n["lit"], path: path("let.value"), slot: Slot::Const(Value::Bool(false)) }])).unwrap();
    store.advance_steps(2.0);
    store.advance_steps(2.5);
    assert_eq!(dash(&store), "0 0.2500 0.5000 1");
    // 2 s after the click the far end turns on, and what reads it follows.
    store.advance_steps(3.1);
    assert_eq!(out(&store), Value::Bool(true));
    assert_eq!(store.resolved(n["lamp"], path("fill.opacity")), Some(Value::Number(1.0)));
    store.advance_steps(4.1);
    assert_eq!(out(&store), Value::Bool(false));
    // The pulse has left: the trace rests, and nothing was ever committed.
    assert!(!store.advance_steps(4.2).contains(&n["tr"]));
    assert_eq!(dash(&store), "0 1");
    assert_eq!(store.history().past().len(), version + 2, "only the two clicks");
    // Reset puts the accumulator back.
    store.apply(tx("reset", vec![Op::Set { id: n["acc"], path: path("accumulate.reset"), slot: Slot::Const(Value::Bool(true)) }])).unwrap();
    store.advance_steps(4.3);
    assert_eq!(acc(&store), 10.0);
}

/// The fiber with a delay: a laser switched on at one end reaches the
/// other end that many seconds later, lit along the way.
#[test]
fn a_delayed_fiber_carries_a_pulse_to_its_far_end() {
    let (mut store, comp, _) = sheet(r#"<group name="spare"/>"#);
    let laser = store.place_standard("laser", comp, [0.0, 0.0], Actor::Human).unwrap();
    let fiber = store.place_standard("fiber", comp, [200.0, 0.0], Actor::Human).unwrap();
    let entity = |s: &Store, u: NodeId| expand::use_entity(s.document(), u).unwrap();
    let port = |s: &Store, u: NodeId, name: &str| {
        let e = entity(s, u);
        s.document().children(e).iter().copied().find(|&p| s.document().get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
    };
    let (le, fe) = (entity(&store, laser), entity(&store, fiber));
    let (from, to) = (port(&store, laser, "out"), port(&store, fiber, "in"));
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let mut ops = author::connect(store.document(), &read, from, to).unwrap();
    ops.push(Op::Set { id: fe, path: path("args.delay_s"), slot: Slot::Const(Value::Number(1.0)) });
    ops.push(Op::Set { id: fe, path: path("args.loss_db_per_km"), slot: Slot::Const(Value::Number(0.0)) });
    let watts = |t: &str| Slot::Const(Expr::parse(t).unwrap().eval(&|_| None).unwrap());
    ops.push(Op::Set { id: le, path: path("args.power"), slot: watts("0 mW") });
    store.apply(tx("plug", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let out = |s: &Store| s.resolved(port(s, fiber, "out"), path("port.signal")).unwrap();
    let light = |s: &Store| s.resolved(named(s, fiber, "light")[0], path("stroke.dash")).unwrap().as_str().unwrap().to_owned();
    store.advance_steps(0.0);
    assert_eq!(light(&store), "0 1");
    store.apply(tx("on", vec![Op::Set { id: le, path: path("args.power"), slot: watts("5 mW") }])).unwrap_or_else(|e| panic!("{e:?}"));
    store.advance_steps(1.0);
    store.advance_steps(1.5);
    assert_eq!(light(&store), "0.5000 1", "half way along");
    assert_eq!(format!("{}", out(&store)), "0 W");
    store.advance_steps(2.01);
    assert_eq!(light(&store), "", "lit throughout");
    assert_eq!(format!("{}", out(&store)), "5 mW", "the far end has it");
}

/// Phase 6 (reactivity.md §8): a query asks a Model while something reads
/// it, once its input rests; shows pending over the last good value; and
/// drops an answer a newer request has overtaken.
#[test]
fn a_query_asks_when_read_debounces_and_drops_a_late_answer() {
    let (mut store, _, n) = sheet(
        r##"<let id="k" name="k" value="1"/>
        <query id="q" name="q" model="sim" input="{= record(k: $k)}" debounce="0.5" initial="{= 0}"/>
        <query id="unread" name="unread" model="sim" input="{= record(k: $k)}"/>
        <text id="t" name="t" content="{= $q.query.status + ' ' + fmt($q) + ($q.query.stale ? ' (old)' : '')}"/>"##,
    );
    let text = |s: &Store| text_of(s, n["t"]);
    let set_k = |s: &mut Store, k: f64| s.apply(tx("k", vec![Op::Set { id: n["k"], path: path("let.value"), slot: Slot::Const(Value::Number(k)) }])).unwrap();
    let answer = |k: f64| Ok(query::from_plain(&serde_json::json!(k * 3.0)));
    let history = store.history().past().len();
    // The first request goes at once, and only for the query that is read.
    let due = store.advance_queries(0.0);
    assert_eq!(due.len(), 1);
    assert_eq!((due[0].node, due[0].model.as_str()), (n["q"], "sim"));
    assert_eq!(query::to_plain(&due[0].input), serde_json::json!({"k": 1.0}));
    assert!(store.advance_queries(0.1).is_empty());
    assert_eq!(text(&store), "pending 0", "the placeholder shows until an answer");
    assert!(store.resolve_query(n["q"], due[0].seq, answer(1.0)));
    store.advance_queries(0.2);
    assert_eq!(text(&store), "ready 3");
    assert!(store.pending_queries().is_empty());
    // A change waits for the input to rest; meanwhile the old value is stale.
    set_k(&mut store, 2.0);
    assert!(store.advance_queries(1.0).is_empty());
    assert_eq!(text(&store), "ready 3 (old)");
    set_k(&mut store, 3.0);
    assert!(store.advance_queries(1.3).is_empty(), "moved again: the wait restarts");
    assert!(store.advance_queries(1.7).is_empty());
    let second = store.advance_queries(1.9);
    assert_eq!(query::to_plain(&second[0].input), serde_json::json!({"k": 3.0}));
    assert_eq!(text(&store), "pending 3 (old)");
    // A newer request overtakes it: the late answer is dropped.
    set_k(&mut store, 4.0);
    assert!(store.advance_queries(3.0).is_empty());
    let third = store.advance_queries(3.6);
    assert_eq!(third.len(), 1);
    assert!(!store.resolve_query(n["q"], second[0].seq, answer(3.0)), "overtaken");
    store.advance_queries(3.7);
    assert_eq!(text(&store), "pending 3 (old)");
    // A failure keeps the last good value and says why.
    assert!(store.resolve_query(n["q"], third[0].seq, Err("the model is down".into())));
    store.advance_queries(3.8);
    assert_eq!(text(&store), "error 3 (old)");
    assert_eq!(store.resolved(n["q"], path("query.error")), Some(Value::Str("the model is down".into())));
    assert_eq!(store.history().past().len(), history + 3, "answers are never transactions");
}

/// Phase 7 (reactivity.md §13): an item that joins a repeat rises into
/// view, one that leaves is kept while it fades and then goes, and one
/// wanted again while leaving turns back from where it is. A conditional's
/// children do the same.
#[test]
fn items_enter_and_leave_over_time() {
    let (mut store, _, n) = sheet(
        r##"<let id="names" name="names" value="{= list('a', 'b')}"/>
        <group id="rows" name="rows" repeat.each="{= $names}" repeat.enter_ms="200" repeat.exit_ms="400">
          <text name="row" content="{= $rows.repeat.item}" appearance.opacity="{= $rows.repeat.presence}"/>
        </group>
        <let id="open" name="open" value="false"/>
        <group id="body" name="body" when="{= $open}" when.exit_ms="100">
          <rect name="panel" w="10" h="10" appearance.opacity="{= $body.when.presence}"/>
        </group>"##,
    );
    let rows = |s: &Store| -> Vec<(String, f64)> {
        named(s, n["rows"], "row").iter().map(|&r| (text_of(s, r), s.resolved(r, path("appearance.opacity")).unwrap().as_f64().unwrap())).collect()
    };
    let set_names = |s: &mut Store, e: &str| s.apply(tx("names", vec![Op::Set { id: n["names"], path: path("let.value"), slot: bind(e) }])).unwrap();
    store.advance_presence(0.0);
    assert_eq!(rows(&store), vec![("a".into(), 1.0), ("b".into(), 1.0)], "what is there at the start is simply there");
    // `c` joins unseen and rises; `a` is kept, leaving.
    set_names(&mut store, "list('b', 'c')");
    assert_eq!(rows(&store), vec![("b".into(), 1.0), ("c".into(), 0.0), ("a".into(), 1.0)]);
    assert_eq!(store.advance_presence(0.1), vec![n["rows"]]);
    assert_eq!(rows(&store), vec![("b".into(), 1.0), ("c".into(), 0.5), ("a".into(), 0.75)]);
    // `a` is wanted again half way out: it turns back, the same node.
    let a = named(&store, n["rows"], "row")[2];
    store.advance_presence(0.2);
    set_names(&mut store, "list('a', 'b')");
    assert_eq!(named(&store, n["rows"], "row")[0], a);
    assert_eq!(rows(&store), vec![("a".into(), 0.5), ("b".into(), 1.0), ("c".into(), 1.0)]);
    for t in [0.3, 0.4, 0.5, 0.6, 0.7] {
        store.advance_presence(t);
    }
    assert_eq!(rows(&store), vec![("a".into(), 1.0), ("b".into(), 1.0)], "c has gone, a is back");
    assert!(store.advance_presence(0.8).is_empty());
    // A conditional with only an exit: opens at once, closes over 100 ms.
    let panel = |s: &Store| named(s, n["body"], "panel").iter().map(|&p| (s.resolved(p, path("appearance.opacity")).unwrap().as_f64().unwrap() * 1000.0).round() / 1000.0).collect::<Vec<_>>();
    let set_open = |s: &mut Store, v: bool| s.apply(tx("open", vec![Op::Set { id: n["open"], path: path("let.value"), slot: Slot::Const(Value::Bool(v)) }])).unwrap();
    set_open(&mut store, true);
    assert_eq!(panel(&store), vec![1.0]);
    set_open(&mut store, false);
    store.advance_presence(0.85);
    assert_eq!(panel(&store), vec![0.5]);
    store.advance_presence(0.95);
    assert!(panel(&store).is_empty());
}

/// Continuous events (reactivity.md §13): a drag's and a wheel's writes are
/// previewed as they happen, each reading what the last left, and commit
/// as one undo step.
#[test]
fn a_drag_previews_its_writes_and_commits_one_step() {
    let (mut store, _, n) = sheet(
        r##"<let id="pan" name="pan" value="0"/>
        <let id="zoom" name="zoom" value="1"/>
        <group id="pad" name="pad">
          <rect id="hit" name="hit" w="100" h="100"/>
          <on id="d" name="pan view" event="drag" target="$pan" to="{= $pan - $d.on.dx / $zoom}"/>
          <on id="w" name="zoom view" event="wheel" target="$zoom" to="{= $zoom * exp(-$w.on.delta / 100)}"/>
        </group>
        <text id="t" name="t" content="{= fmt($pan, 1)}"/>"##,
    );
    let history = store.history().past().len();
    assert_eq!(store.listens_for(n["hit"], "drag"), Some(n["pad"]));
    assert_eq!(store.pointer_action(n["hit"], "drag", [10.0, 0.0, 10.0, 5.0, 0.0]).unwrap().as_deref(), Some("pan view"));
    store.pointer_action(n["hit"], "drag", [5.0, 0.0, 15.0, 5.0, 0.0]).unwrap();
    assert_eq!(text_of(&store, n["t"]), "-15.0", "previewed, each move from where the last left");
    assert_eq!(store.history().past().len(), history);
    store.commit_gesture("pan view", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(store.history().past().len(), history + 1);
    assert_eq!(store.resolved(n["pan"], path("let.value")), Some(Value::Number(-15.0)));
    store.undo(0).unwrap().unwrap();
    assert_eq!(text_of(&store, n["t"]), "0.0");
    store.pointer_action(n["hit"], "wheel", [0.0, 0.0, 50.0, 50.0, -100.0]).unwrap();
    store.commit_gesture("zoom view", Actor::Human, 0).unwrap().unwrap();
    assert!((store.resolved(n["zoom"], path("let.value")).unwrap().as_f64().unwrap() - std::f64::consts::E).abs() < 1e-9);
}

/// `std:plot` pans by a drag in its plot area and zooms by the wheel about
/// the pointer: its view is state on the instance, one undo step a gesture.
#[test]
fn the_plot_pans_by_drag_and_zooms_by_wheel() {
    let (mut store, comp, _) = sheet(r#"<group name="spare"/>"#);
    let plot = store.place_standard("plot", comp, [0.0, 0.0], Actor::Human).unwrap();
    let e = expand::use_entity(store.document(), plot).unwrap();
    let out = |s: &Store, name: &str| {
        let p = s.document().children(e).iter().copied().find(|&p| s.document().get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap();
        s.resolved(p, path("port.signal")).unwrap().as_f64().unwrap()
    };
    assert_eq!((out(&store, "x_lo"), out(&store, "x_hi")), (0.0, 10.0));
    let area = named(&store, plot, "plot area")[0];
    let history = store.history().past().len();
    // The area is 290 px wide (360 less the margins): half of it is 5 units.
    store.pointer_action(area, "drag", [-100.0, 0.0, 0.0, 0.0, 0.0]).unwrap().unwrap();
    store.pointer_action(area, "drag", [-45.0, 0.0, 0.0, 0.0, 0.0]).unwrap().unwrap();
    assert!((out(&store, "x_lo") - 5.0).abs() < 1e-9 && (out(&store, "x_hi") - 15.0).abs() < 1e-9, "{} {}", out(&store, "x_lo"), out(&store, "x_hi"));
    store.commit_gesture("pan plot", Actor::Human, 0).unwrap().unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.history().past().len(), history + 1);
    assert!((out(&store, "x_lo") - 5.0).abs() < 1e-9);
    // Zoom in 2x about the left edge: the left edge stays, the span halves.
    let delta = -(2f64.ln()) / 0.002;
    store.pointer_action(area, "wheel", [0.0, 0.0, 0.0, 0.0, delta]).unwrap().unwrap();
    store.commit_gesture("zoom plot", Actor::Human, 0).unwrap().unwrap_or_else(|e| panic!("{e:?}"));
    assert!((out(&store, "x_lo") - 5.0).abs() < 1e-9 && (out(&store, "x_hi") - 10.0).abs() < 1e-9, "{} {}", out(&store, "x_lo"), out(&store, "x_hi"));
    store.undo(0).unwrap().unwrap();
    store.undo(0).unwrap().unwrap();
    assert_eq!((out(&store, "x_lo"), out(&store, "x_hi")), (0.0, 10.0));
}
