//! Reactivity phases 1 and 2 (plans/specs/reactivity.md): errors that
//! propagate, presentation state through lets, problems as data, and scopes:
//! items as nodes, components in repeats, structure that settles live.

use iron_document::document::{RelKind, Relation};
use iron_document::facet::{Composition, Name};
use iron_document::*;
use iron_document::edit;
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
    let ids = store.authored().next_ids(2);
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
    let built = markup::ops(markup, Some(ids[0]), OrderKey::first(), store.authored().next_id()).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("draw", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    (store, ids[0], built.named)
}

/// A repeat's items, in order.
fn items(store: &Store, g: NodeId) -> Vec<NodeId> {
    let doc = store.tree();
    let items: Vec<NodeId> = doc.children(g).iter().copied().filter(|c| c.is_item_scope() && doc.is_live(*c)).collect();
    items
}

fn x_of(store: &Store, id: NodeId) -> f64 {
    store.resolved(id, path("transform2d.x")).unwrap().as_f64().unwrap()
}

#[test]
fn a_failed_binding_fails_its_readers_and_recovers() {
    let (mut store, _, n) = sheet(r#"<let id="$a" value="{= 2 mW * 3}"/><let id="$b" value="{= $a * 2}"/><rect id="$r" x="{= $b / 1 mW}" w="10" h="10"/>"#);
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
    let (mut store, _, n) = sheet(r#"<rect id="$r" x="0" w="10" h="10" interaction.hovered="false" fill.opacity="1"/><let id="$lit" value="1"/><rect id="$s" x="0" w="10" h="10" fill.opacity="1"/>"#);
    let (r, lit, s) = (n["r"], n["lit"], n["s"]);
    let hover = format!("{r}.interaction.hovered ? 1 : 0.4");
    store.apply(tx("shared", vec![Op::Set { id: lit, path: path("let.value"), slot: bind(&hover) }])).unwrap();
    store.apply(tx("look", vec![Op::Set { id: s, path: path("fill.opacity"), slot: bind(&format!("{lit}.let.value")) }])).unwrap();
    // Content may not read it, however it gets there.
    let refused = store.apply(tx("content", vec![Op::Set { id: s, path: path("transform2d.x"), slot: bind(&format!("{lit}.let.value * 10")) }]));
    assert!(refused.is_err());
    // Nor may a let that content already reads start carrying it.
    let (mut store, _, n) = sheet(r#"<rect id="$r" x="0" w="10" h="10" interaction.hovered="false"/><let id="$k" value="1"/><rect id="$s" x="{= $k * 10}" w="10" h="10"/>"#);
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
    let doc = store.tree();
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
    let (ops, _) = place_ops(&store, r#"<place def="std:meter" in="sheet" x="400" y="0"/>"#).unwrap();
    store.apply(tx("place", ops)).unwrap();
    let def = standard::find(store.tree(), "meter").unwrap();
    let comp = store.tree().roots()[0];
    let markup = format!(
        r#"<group id="$g" name="meters" repeat.each="5 10 15">
             <entity id="$e" name="m" instance.def="{def}" instance.mode="linked" args.in="{{= $g.repeat.item * 1 mW}}" args.range="20 mW"/>
             <use id="$u" name="m" notation="free" x="{{= $g.repeat.index * 130}}" y="200"/>
           </group>"#
    );
    let built = markup::ops(&markup, Some(comp), OrderKey::after(&OrderKey::after(&OrderKey::first())), store.authored().next_id()).unwrap_or_else(|e| panic!("{e}"));
    let (g, e, u) = (built.named["g"], built.named["e"], built.named["u"]);
    let mut ops = built.ops;
    ops.push(Op::Link { rel: Relation { id: store.authored().next_relation_id(), from: u, to: e, rel: RelKind::Figures, data: BTreeMap::new() } });
    store.apply(tx("meters", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let doc = store.tree();
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
    let (mut store, _, n) = sheet(r#"<slider id="$s" x="0" y="0" min="0" max="10" step="1" value="2"/><group id="$g" repeat.count="{= $s.slider.value}"><rect x="{= $g.repeat.index * 20}" w="10" h="10"/></group><let id="$data" value="1 2"/><group id="$h" repeat.each="{= $data}"><rect x="{= $h.repeat.item}" w="5" h="5"/></group>"#);
    let (s, g, data, h) = (n["s"], n["g"], n["data"], n["h"]);
    assert_eq!(items(&store, g).len(), 2);
    let version = store.authored().version();
    let _ = store.take_changes();
    // A drag previews the slider's value.
    store.preview(s, path("slider.value"), Value::Number(5.0));
    assert_eq!(items(&store, g).len(), 5);
    assert_eq!(x_of(&store, store.tree().children(items(&store, g)[4])[0]), 80.0);
    assert!(store.take_changes().structural(), "the host is told what to materialise");
    assert_eq!(store.authored().version(), version, "no transaction");
    store.unpreview(s, path("slider.value"));
    assert_eq!(items(&store, g).len(), 2);
    // A producer publishes a longer series onto the data.
    let live = store.register_producer("live");
    store.publish(live, BTreeMap::from([(Leaf { node: data, path: path("let.value") }, Value::series(vec![1.0, 2.0, 3.0, 4.0], Dim::NONE))]));
    assert_eq!(items(&store, h).len(), 4);
    assert_eq!(store.authored().version(), version);
    assert!(store.reactive().errors.is_empty(), "{:?}", store.reactive().errors);
}

/// Each item reads its own value: changing one element re-evaluates that
/// item's copy and nothing else.
#[test]
fn changing_one_element_re_evaluates_one_item() {
    let (mut store, _, n) = sheet(r#"<let id="$data" value="10 20 30 40"/><group id="$g" repeat.each="{= $data}"><rect x="{= $g.repeat.item * 2}" w="5" h="5"/></group>"#);
    let (data, g) = (n["data"], n["g"]);
    let xs = |s: &Store| items(s, g).iter().map(|&i| x_of(s, s.tree().children(i)[0])).collect::<Vec<_>>();
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
    let doc = store.tree();
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
    let (mut store, _, n) = sheet(r#"<let id="$names" value="{= list('alpha', 'beta', 'gamma')}"/><group id="$g" name="legend" repeat.each="{= $names}"><let id="$open" name="open" value="false"/><text name="label" x="0" y="{= $g.repeat.index * 20}" content="{= $g.repeat.item}"/></group>"#);
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
    let (mut store, _, n) = sheet(r#"<let id="$lo" value="0"/><group id="$g" repeat.each="{= range($lo, $lo + 30, 4)}" repeat.key="{= t => t}"><text name="tick" x="{= $g.repeat.item}" y="0" content="{= fmt($g.repeat.item)}"/></group>"#);
    let (lo, g) = (n["lo"], n["g"]);
    let ids = |s: &Store| items(s, g).iter().map(|i| i.to_string()).collect::<Vec<_>>();
    assert_eq!(ids(&store), vec![format!("{g}/~0"), format!("{g}/~10"), format!("{g}/~20"), format!("{g}/~30")]);
    let twenty = items(&store, g)[2];
    store.apply(tx("slide", vec![Op::Set { id: lo, path: path("let.value"), slot: Slot::Const(Value::Number(10.0)) }])).unwrap();
    assert_eq!(ids(&store), vec![format!("{g}/~10"), format!("{g}/~20"), format!("{g}/~30"), format!("{g}/~40")]);
    assert_eq!(items(&store, g)[1], twenty);
    assert_eq!(store.resolved(twenty, path("item.index")), Some(Value::Number(1.0)));
    assert!(!store.tree().is_live(NodeId::item_scope(g, "0").unwrap()));
}

/// `when`: the children exist only while the condition holds.
#[test]
fn a_conditional_groups_children_exist_only_while_it_holds() {
    let (mut store, _, n) = sheet(r#"<let id="$show" value="false"/><group id="$g" name="detail" when="{= $show}"><text name="body" x="0" y="0" content="more"/></group>"#);
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

/// `on.emit` queues a named event for the host when the action fires; the
/// host takes it once (`take_emitted`).
#[test]
fn an_emitting_action_queues_its_event_for_the_host() {
    let (mut store, _, n) = sheet(r#"<group id="$g" name="button"><rect id="$bar" name="bar" x="0" y="0" w="80" h="24"/><on event="click" emit="pressed"/></group>"#);
    let (g, bar) = (n["g"], n["bar"]);
    assert!(store.take_emitted().is_empty());
    store.fire(bar, "click", Actor::Human, 0).unwrap();
    assert_eq!(store.take_emitted(), vec![(g, "pressed".to_owned())]);
    assert!(store.take_emitted().is_empty(), "taken once");
}

/// An accordion as a component, in markup: a click on its header toggles
/// its own `open` state, which a conditional shows its body by. Each
/// instance has its own; one click is one undo step.
#[test]
fn an_accordion_component_toggles_per_instance_and_undoes_one_click() {
    let (mut store, comp, _) = sheet(r#"<group name="spare"/>"#);
    let library = store.authored().next_id();
    store.apply(tx("library", vec![Op::Create { id: library, ty: ty("library"), parent: None, order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![] }])).unwrap();
    let def_markup = r##"<definition name="accordion">
        <port_spec id="$title" name="title" kind="value" dir="in" signal="Section"/>
        <template notation="free">
          <let id="$open" name="open" value="false"/>
          <let id="$hot" name="hot" keep="session" value="false"/>
          <group id="$head" name="head">
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
    let built = markup::ops(def_markup, Some(library), OrderKey::first(), store.authored().next_id()).unwrap_or_else(|e| panic!("{e}"));
    let def = built.roots[0];
    store.apply(tx("define", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let place = format!(r#"<place def="{def}" in="sheet" x="0" y="0" name="A" title="One"/><place def="{def}" in="sheet" x="0" y="100" name="B" title="Two"/>"#);
    let (ops, _) = place_ops(&store, &place).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("place", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let uses: Vec<NodeId> = store.tree().children(comp).iter().copied().filter(|&c| store.tree().live(c).is_some_and(|n| n.ty.name() == "use")).collect();
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
    let (version, steps, saved) = (store.authored().version(), store.history().past().len(), store.save());
    assert_eq!(store.hover_listeners(bar).len(), 1);
    assert!(store.fire(bar, "enter", Actor::Human, 0).unwrap().is_none());
    assert_ne!(colour(&store), cold);
    assert_eq!(store.resolved(named(&store, b, "bar")[0], path("fill.color")).unwrap(), cold, "only this instance");
    assert_eq!((store.authored().version(), store.history().past().len(), store.save()), (version, steps, saved));
    store.fire(bar, "leave", Actor::Human, 0).unwrap();
    assert_eq!(colour(&store), cold);
}

/// An action's writes are computed before any is applied, and a counter
/// steps by its `by`.
#[test]
fn an_actions_writes_are_parallel_and_one_step() {
    let (mut store, _, n) = sheet(r#"<let id="$a" value="1"/><let id="$b" value="2"/><let id="$count" value="0"/>
      <group id="$btn" name="swap"><rect name="face" x="0" y="0" w="40" h="20"/>
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
    let doc = store.tree();
    doc.descendants(under).into_iter().find(|&d| doc.get::<Name>(d).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
}

// ---- Phase 5: tracked reads ---------------------------------------------

/// An inspector line: the width of whatever is selected. It follows the
/// selection, and the selected node's width.
#[test]
fn a_readout_of_the_selected_nodes_width_follows_selection_and_resize() {
    let (mut store, _, n) = sheet(r#"<rect id="$r1" x="0" y="0" w="30" h="10"/><rect id="$r2" x="50" y="0" w="70" h="10"/><let id="$sel" value="0" selection.count="0"/><text id="$t" x="0" y="40" content="{= $sel.selection.count == 0 ? 'nothing' : 'w = ' + fmt(deref($sel.selection.node, 'size.w', 0))}"/>"#);
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
    let (mut store, _, n) = sheet(r#"<let id="$k" value="2"/><rect id="$r1" x="0" y="0" w="{= $k * 10}" h="10"/><rect id="$r2" x="0" y="20" w="{= $k * 100}" h="10"/><let id="$cur" value="0"/><let id="$w" value="{= deref($cur, 'size.w', -1)}"/><let id="$twice" value="{= $w * 2}"/>"#);
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
    let (mut store, _, n) = sheet(r#"<let id="$cur" value="0"/><let id="$w" value="{= deref($cur, 'size.w', 1)}"/><rect id="$r" x="0" y="0" w="{= $w + 1}" h="10"/>"#);
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
        r##"<group id="$plain" name="plain"/>
        <group id="$wide" name="wide"><let name="gap" provide="yes" value="40"/></group>
        <group id="$night" name="night"><let id="$ink" name="ink" provide="yes" value="#ffffffff"/><rect id="$r" name="r" w="4" h="4" fill.color="@ink"/></group>
        <rect id="$day" name="day" w="4" h="4" fill.color="@ink"/>"##,
    );
    let library = store.authored().next_id();
    store.apply(tx("library", vec![Op::Create { id: library, ty: ty("library"), parent: None, order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![] }])).unwrap();
    let def_markup = r##"<definition name="spaced">
        <template notation="free">
          <let name="gap" provide="default" value="8"/>
          <rect name="box" x="{= @gap}" y="0" w="10" h="10" fill.color="#000000ff"/>
        </template>
      </definition>"##;
    let built = markup::ops(def_markup, Some(library), OrderKey::first(), store.authored().next_id()).unwrap_or_else(|e| panic!("{e}"));
    let def = built.roots[0];
    store.apply(tx("define", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let (plain, wide) = (named_ids["plain"], named_ids["wide"]);
    let place = format!(r#"<place def="{def}" in="{plain}" name="A"/><place def="{def}" in="{wide}" name="B"/>"#);
    let (ops, _) = place_ops(&store, &place).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("place", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let box_x = |s: &Store, under: NodeId| x_of(s, named(s, under, "box")[0]);
    assert_eq!(box_x(&store, plain), 8.0, "the component's own default");
    assert_eq!(box_x(&store, wide), 40.0, "the provider above it");
    // The inspector's list: what each picture reads, and from where.
    let reads = |s: &Store, under: NodeId| s.context_reads(s.tree().children(under).iter().copied().find(|&c| s.tree().live(c).is_some_and(|n| n.ty.name() == "use")).unwrap());
    assert_eq!(reads(&store, plain), vec![("gap".to_owned(), None, Some(Value::Number(8.0)))], "its own default");
    let provider = store.tree().children(wide).iter().copied().find(|&c| store.tree().live(c).is_some_and(|n| n.ty.name() == "let"));
    assert_eq!(reads(&store, wide), vec![("gap".to_owned(), provider, Some(Value::Number(40.0)))]);
    // A definition declares what it reads: an undeclared name, a read
    // outside the template, and taking the declaration away are refused.
    let template = store.tree().children(def).iter().copied().find(|&c| store.tree().live(c).is_some_and(|n| n.ty.name() == "template")).unwrap();
    let part = |name: &str| store.tree().children(template).iter().copied().find(|&c| store.tree().get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap();
    let (the_box, the_default) = (part("box"), part("gap"));
    let refused = |s: &mut Store, ops: Vec<Op>| s.apply(tx("no", ops)).unwrap_err()[0].message.clone();
    let why = refused(&mut store, vec![Op::Set { id: the_box, path: path("transform2d.y"), slot: bind("@margin") }]);
    assert!(why.contains("does not declare"), "{why}");
    let themed = store.apply(tx("theme", vec![Op::Set { id: the_box, path: path("fill.color"), slot: bind("@ink") }]));
    assert!(themed.is_ok(), "colour tokens need no declaration: {themed:?}");
    let why = refused(&mut store, vec![Op::Set { id: the_default, path: path("let.provide"), slot: Slot::Const(Value::Enum("no".into())) }]);
    assert!(why.contains("does not declare"), "{why}");
    let why = refused(&mut store, vec![Op::Delete { id: the_default }]);
    assert!(why.contains("does not declare"), "{why}");
    let outside = markup::ops(r#"<let name="model_level" value="{= @gap}"/>"#, Some(def), OrderKey::after(&OrderKey::after(&OrderKey::first())), store.authored().next_id()).unwrap_or_else(|e| panic!("{e}"));
    let why = refused(&mut store, outside.ops);
    assert!(why.contains("outside a template"), "{why}");
    // The provider's value is live, and ceasing to provide falls back.
    let gap = store.tree().children(wide).iter().copied().find(|&c| store.tree().live(c).is_some_and(|n| n.ty.name() == "let")).unwrap();
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
        r##"<let id="$lit" name="lit" value="false"/>
        <trace id="$tr" name="tr" of="{= $lit}" delay="2"/>
        <rect id="$lamp" name="lamp" w="4" h="4" fill.opacity="{= $tr ? 1 : 0.2}"/>
        <path id="$glow" name="glow" stroke.path_length="1" stroke.cap="butt" stroke.dash="{= $tr.trace.dash}"/>
        <let id="$rate" name="rate" value="3"/>
        <accumulate id="$acc" name="acc" initial="10" next="{= $acc + $rate * $acc.accumulate.dt}"/>
        <text id="$t" name="t" content="{= fmt($acc, 2)}"/>"##,
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
    let entity = |s: &Store, u: NodeId| expand::use_entity(s.tree(), u).unwrap();
    let port = |s: &Store, u: NodeId, name: &str| {
        let e = entity(s, u);
        s.tree().children(e).iter().copied().find(|&p| s.tree().get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
    };
    let (le, fe) = (entity(&store, laser), entity(&store, fiber));
    let (from, to) = (port(&store, laser, "out"), port(&store, fiber, "in"));
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let mut ops = author::connect(store.tree(), &read, from, to).unwrap();
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

/// An accumulator in a composition no view shows holds its value and keeps
/// nothing awake; shown again, it carries on from there.
#[test]
fn an_accumulator_in_a_hidden_composition_holds_still() {
    let (mut store, comp, n) = sheet(r##"<accumulate id="$acc" name="acc" initial="0" next="{= $acc + $acc.accumulate.dt}"/>"##);
    let acc = |s: &Store| s.resolved(n["acc"], path("accumulate.value")).unwrap().as_f64().unwrap();
    store.set_observe_all(false);
    let id = store.observe(Interest::Region { composition: comp, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    store.advance(0.0);
    store.advance(0.05);
    let before = acc(&store);
    assert!(before > 0.0, "it runs while shown");
    store.unobserve(id);
    for i in 1..10 {
        assert!(store.advance(0.05 + i as f64 * 0.05).wake.live.is_empty(), "nothing moves at hidden frame {i}");
    }
    assert_eq!(acc(&store), before, "it held while hidden");
    store.observe(Interest::Region { composition: comp, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    store.advance(0.6);
    store.advance(0.65);
    assert!(acc(&store) > before, "it carries on when shown again");
}

/// A laser that is not pulsing and a fiber carrying a steady value are at
/// rest: nothing keeps the frame loop awake.
#[test]
fn steady_stepped_producers_are_at_rest() {
    let (mut store, comp, _) = sheet(r#"<group name="spare"/>"#);
    let laser = store.place_standard("laser", comp, [0.0, 0.0], Actor::Human).unwrap();
    let fiber = store.place_standard("fiber", comp, [200.0, 0.0], Actor::Human).unwrap();
    store.place_standard("plot", comp, [0.0, 200.0], Actor::Human).unwrap();
    let entity = |s: &Store, u: NodeId| expand::use_entity(s.tree(), u).unwrap();
    let port = |s: &Store, u: NodeId, name: &str| {
        let e = entity(s, u);
        s.tree().children(e).iter().copied().find(|&p| s.tree().get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
    };
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let ops = author::connect(store.tree(), &read, port(&store, laser, "out"), port(&store, fiber, "in")).unwrap();
    store.apply(tx("plug", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    store.advance_steps(0.0);
    store.advance_presence(0.0);
    store.advance_steps(0.01);
    store.take_changes();
    for i in 1..20 {
        let now = i as f64 * 0.0163 + (i % 3) as f64 * 0.001;
        assert_eq!(store.advance_steps(now), vec![], "steps at frame {i}");
        assert_eq!(store.advance_presence(now), vec![], "presence at frame {i}");
        assert!(store.advance_queries(now).is_empty() && store.pending_queries().is_empty());
        assert!(store.advance_motion(now).is_empty(), "motion at frame {i}");
        let moved: Vec<String> = store.take_changes().values.iter().map(|l| l.to_string()).collect();
        assert!(moved.is_empty(), "frame {i} changed {moved:?}");
    }
}

/// Phase 6 (reactivity.md §8): a query asks a Model while something reads
/// it, once its input rests; shows pending over the last good value; and
/// drops an answer a newer request has overtaken.
#[test]
fn a_query_asks_when_read_debounces_and_drops_a_late_answer() {
    let (mut store, _, n) = sheet(
        r##"<let id="$k" name="k" value="1"/>
        <query id="$q" name="q" model="sim" input="{= record(k: $k)}" debounce="0.5" initial="{= 0}"/>
        <query id="$unread" name="unread" model="sim" input="{= record(k: $k)}"/>
        <text id="$t" name="t" content="{= $q.query.status + ' ' + fmt($q) + ($q.query.stale ? ' (old)' : '')}"/>"##,
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

/// A query's constant input set by a transaction asks again: the edit
/// wakes the query as a value a cell wrote would (council review of DS-2).
#[test]
fn a_query_s_constant_input_set_asks_again() {
    let (mut store, comp, n) = sheet(
        r##"<query id="$q" name="q" model="sim" input="5"/>
        <text id="$t" name="t" content="{= fmt($q)}"/>"##,
    );
    // Observed as a view shows it: what wakes it is what it waits on.
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: comp, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    let first = store.advance_queries(0.0);
    assert_eq!(first.len(), 1);
    assert!(store.resolve_query(n["q"], first[0].seq, Ok(query::from_plain(&serde_json::json!(15.0)))));
    store.advance_queries(0.1);
    assert!(store.advance_queries(0.2).is_empty());
    store.apply(tx("input", vec![Op::Set { id: n["q"], path: path("query.input"), slot: Slot::Const(Value::Number(6.0)) }])).unwrap();
    // Asked once the input has rested for the default debounce (0.2 s).
    store.advance_queries(0.3);
    let again = store.advance_queries(0.6);
    assert_eq!(again.len(), 1, "the new input is asked");
    assert_eq!(query::to_plain(&again[0].input), serde_json::json!(6.0));
}

/// A repeat's `max` set, alone or with a structural op beside it, settles
/// its items again (council review of DS-3b: `max` was not compared).
#[test]
fn a_repeat_s_max_set_settles_its_items() {
    let (mut store, comp, n) = sheet(r##"<group id="$rows" name="rows" repeat.count="3"><rect name="row" w="5" h="5"/></group>"##);
    assert_eq!(named(&store, n["rows"], "row").len(), 3);
    store.apply(tx("max", vec![Op::Set { id: n["rows"], path: path("repeat.max"), slot: Slot::Const(Value::Number(1.0)) }])).unwrap();
    assert_eq!(named(&store, n["rows"], "row").len(), 1);
    let id = store.authored().next_id();
    store.apply(tx("max, and a group", vec![
        Op::Set { id: n["rows"], path: path("repeat.max"), slot: Slot::Const(Value::Number(2.0)) },
        Op::Create { id, ty: TypeId::lookup("group").unwrap(), parent: Some(comp), order: OrderKey::first(), facets: vec![] },
    ])).unwrap();
    assert_eq!(named(&store, n["rows"], "row").len(), 2);
}

/// Phase 7 (reactivity.md §13): an item that joins a repeat rises into
/// view, one that leaves is kept while it fades and then goes, and one
/// wanted again while leaving turns back from where it is. A conditional's
/// children do the same.
#[test]
fn items_enter_and_leave_over_time() {
    let (mut store, _, n) = sheet(
        r##"<let id="$names" name="names" value="{= list('a', 'b')}"/>
        <group id="$rows" name="rows" repeat.each="{= $names}" repeat.enter_ms="200" repeat.exit_ms="400">
          <text name="row" content="{= $rows.repeat.item}" appearance.opacity="{= $rows.repeat.presence}"/>
        </group>
        <let id="$open" name="open" value="false"/>
        <group id="$body" name="body" when="{= $open}" when.exit_ms="100">
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
        r##"<let id="$pan" name="pan" value="0"/>
        <let id="$zoom" name="zoom" value="1"/>
        <group id="$pad" name="pad">
          <rect id="$hit" name="hit" w="100" h="100"/>
          <on id="$d" name="pan view" event="drag" target="$pan" to="{= $pan - $d.on.dx / $zoom}"/>
          <on id="$w" name="zoom view" event="wheel" target="$zoom" to="{= $zoom * exp(-$w.on.delta / 100)}"/>
        </group>
        <text id="$t" name="t" content="{= fmt($pan, 1)}"/>"##,
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

/// A tick-count change in one plot re-expands, and writes only that
/// plot's labels: the other plot's `auto` (NaN) leaves are the same data
/// as before, not changes, so it gets no ops.
#[test]
fn a_plot_s_new_tick_writes_nothing_outside_it() {
    let (mut store, comp, _) = sheet(r#"<group name="spare"/>"#);
    let plot = store.place_standard("plot", comp, [0.0, 0.0], Actor::Human).unwrap();
    let other = store.place_standard("plot", comp, [0.0, 400.0], Actor::Human).unwrap();
    let area = named(&store, plot, "plot area")[0];
    store.take_changes();
    let mut seen = 0;
    for _ in 0..200 {
        store.pointer_action(area, "drag", [-3.0, 0.0, 0.0, 0.0, 0.0]).unwrap().unwrap();
        let c = store.take_changes();
        // What the store wrote, as opposed to values it re-evaluated: the
        // nodes made, gone and moved, and the slots set.
        let written: Vec<NodeId> = c.created.iter().copied().chain(c.removed.iter().chain(&c.moved).map(|(n, _)| *n)).chain(c.slots.iter().map(|l| l.node)).collect();
        if written.is_empty() {
            continue;
        }
        seen += 1;
        for id in written {
            assert_eq!(id.path()[0], plot, "{id} is outside the panned plot (the other is {other})");
        }
    }
    assert!(seen > 0, "the pan changed the tick count at least once");
    // A formula's `==` is still IEEE: NaN is not equal to itself.
    let read = |_: Leaf| -> Option<Value> { None };
    assert_eq!(Expr::parse("0 / 0 == 0 / 0").unwrap().eval(&read).unwrap(), Value::Bool(false));
    assert_eq!(Value::Number(f64::NAN), Value::Number(f64::NAN));
}

/// `std:plot` pans by a drag in its plot area and zooms by the wheel about
/// the pointer: its view is state on the instance, one undo step a gesture.
#[test]
fn the_plot_pans_by_drag_and_zooms_by_wheel() {
    let (mut store, comp, _) = sheet(r#"<group name="spare"/>"#);
    let plot = store.place_standard("plot", comp, [0.0, 0.0], Actor::Human).unwrap();
    let e = expand::use_entity(store.tree(), plot).unwrap();
    let out = |s: &Store, name: &str| {
        let p = s.tree().children(e).iter().copied().find(|&p| s.tree().get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap();
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

/// Component verbs as an edit (edit.rs): its ops and its summary lines.
fn place_ops(store: &iron_document::Store, markup: &str) -> Result<(Vec<iron_document::Op>, String), String> {
    let plan = iron_document::edit::plan(store, &format!("<edit>{markup}</edit>"), None).map_err(|f| f.message())?;
    Ok((plan.ops, plan.summary.join("\n")))
}

/// Phase 3's exit (reactive-engine.md §17): a document with a trace and an
/// accumulator, both at rest, asks `advance` for nothing: no wake, nothing
/// to redraw. A click wakes it until the pulse has passed.
#[test]
fn a_trace_and_an_accumulator_at_rest_schedule_no_wake() {
    let (mut store, _, n) = sheet(
        r##"<let id="$lit" name="lit" value="false"/>
        <trace id="$tr" name="tr" of="{= $lit}" delay="1"/>
        <accumulate id="$acc" name="acc" initial="10" next="{= $acc}"/>"##,
    );
    for t in [0.0, 0.1, 0.2] {
        store.advance(t);
    }
    let idle = store.advance(0.3);
    assert_eq!((idle.wake.next, idle.wake.live.len(), idle.changed), (None, 0, false));
    store.apply(tx("on", vec![Op::Set { id: n["lit"], path: path("let.value"), slot: Slot::Const(Value::Bool(true)) }])).unwrap();
    let busy = store.advance(0.4);
    assert_eq!(busy.wake.next, Some(0.4));
    assert!(!busy.wake.live.is_empty());
    for t in [1.0, 1.5, 2.0, 2.1] {
        store.advance(t);
    }
    let rest = store.advance(2.2);
    assert_eq!((rest.wake.next, rest.wake.live.len(), rest.changed), (None, 0, false));
}

/// Observers (reactive-engine.md §8.2, phase 4 step 1): with everything no
/// longer counted as shown, an accumulator in a composition no observer
/// reaches holds still; observing its composition sets it going.
#[test]
fn an_accumulator_steps_only_where_observed() {
    let (mut store, comp, n) = sheet(r##"<accumulate id="$acc" name="acc" initial="0" next="{= $acc + $acc.accumulate.dt}"/>"##);
    let acc = |s: &Store| s.resolved(n["acc"], path("accumulate.value")).unwrap().as_f64().unwrap();
    store.set_observe_all(false);
    for t in [0.0, 0.1, 0.2] {
        store.advance(t);
    }
    assert_eq!(acc(&store), 0.0, "unobserved: holds still");
    let id = store.observe(Interest::Region { composition: comp, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    for t in [0.3, 0.4, 0.5] {
        store.advance(t);
    }
    assert!(acc(&store) > 0.0, "observed: steps");
    store.unobserve(id);
    let held = acc(&store);
    for t in [0.6, 0.7] {
        store.advance(t);
    }
    assert_eq!(acc(&store), held);
}

/// Two compositions, `a` and `b`, with a `let` at the root that a host
/// producer moves, read by one rect in `a` and four in `b`.
fn two_tabs() -> (Store, NodeId, NodeId, NodeId, Vec<NodeId>) {
    let mut store = Store::new();
    let src = r#"<edit>
      <model name="model"/>
      <let id="$t" name="t" value="0"/>
      <composition id="$a" name="a" dims="2d">
        <rect id="$s" name="s" x="{= $t * 2}" y="0" w="10" h="10"/>
      </composition>
      <composition id="$b" name="b" dims="2d">
        <rect id="$p1" name="p1" x="{= $t + 1}" y="0" w="10" h="10"/>
        <rect id="$p2" name="p2" x="{= $p1.transform2d.x + 1}" y="0" w="10" h="10"/>
        <rect id="$p3" name="p3" x="{= $t + 3}" y="{= $p2.transform2d.x}" w="10" h="10"/>
        <rect id="$p4" name="p4" x="{= $t + 4}" y="0" w="10" h="10"/>
      </composition>
    </edit>"#;
    let plan = edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("tabs", plan.ops)).unwrap();
    let a = |k: &str| plan.aliases[k];
    (store, a("$t"), a("$a"), a("$b"), vec![a("$s"), a("$p1"), a("$p2"), a("$p3"), a("$p4")])
}

/// Phase 4's first exit (reactive-engine.md §17): a composition no
/// observer shows costs no evaluations while what its pictures read moves;
/// once shown, every value is what observing everything gives (§8.8).
#[test]
fn an_unobserved_compositions_pictures_cost_no_evaluations() {
    let (mut all, ..) = two_tabs();
    let (mut store, t, a, b, rects) = two_tabs();
    let p = store.register_producer("host");
    all.register_producer("host");
    let at = |v: f64| BTreeMap::from([(Leaf { node: t, path: path("let.value") }, Value::Number(v))]);
    store.set_observe_all(false);
    let id = store.observe(Interest::Region { composition: a, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    let leaves = |n: NodeId| [Leaf { node: n, path: path("transform2d.x") }, Leaf { node: n, path: path("transform2d.y") }];
    for v in [1.0, 2.0, 3.0] {
        let (before, before_all) = (store.reactive().evaluations, all.reactive().evaluations);
        store.publish(p, at(v));
        all.publish(p, at(v));
        let (spent, spent_all) = (store.reactive().evaluations - before, all.reactive().evaluations - before_all);
        assert_eq!(spent, 1, "only the shown rect");
        assert_eq!(spent_all, 6, "the reference evaluates b's pictures");
    }
    assert_eq!(store.resolved(rects[0], path("transform2d.x")), Some(Value::Number(6.0)));
    assert!(rects[1..].iter().any(|&r| leaves(r).iter().any(|l| store.reactive().is_stale(*l))));
    // Shown, b is brought up to date at once.
    store.reobserve(id, Interest::Region { composition: b, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    assert_eq!(store.reactive().stale_count(), 0);
    for &r in &rects {
        for l in leaves(r) {
            assert_eq!(store.resolved(l.node, l.path), all.resolved(l.node, l.path), "{l}");
        }
    }
    // Now a is hidden. A transaction on b costs b, not a (engine spec §4.1,
    // `EN-8c`: no transaction makes every cell exact): a rests, stale, until
    // it is shown again.
    store.publish(p, at(4.0));
    all.publish(p, at(4.0));
    assert!(store.reactive().is_stale(leaves(rects[0])[0]));
    let plan = edit::plan(&store, &format!(r#"<edit><rect in="{b}" name="q" x="1" y="1" w="1" h="1"/></edit>"#), None).unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("q", plan.ops)).unwrap();
    assert!(store.reactive().is_stale(leaves(rects[0])[0]), "a still rests after b's transaction");
    let shown_a = store.observe(Interest::Region { composition: a, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    assert_eq!(store.resolved(rects[0], path("transform2d.x")), Some(Value::Number(8.0)), "shown, a is brought up to date");
    // And hidden again, a rests.
    store.unobserve(shown_a);
    store.advance(0.0);
    let before = store.reactive().evaluations;
    store.publish(p, at(5.0));
    assert_eq!(store.reactive().evaluations - before, 5, "b's five bound leaves only");
}

/// A tracked read (`deref`) from a shown picture that turns to an
/// unobserved one reads it current: what it reaches is needed from then on.
#[test]
fn a_tracked_read_into_an_unobserved_composition_reads_it_current() {
    let (mut store, t, a, _, rects) = two_tabs();
    let plan = edit::plan(&store, &format!(r#"<edit><let id="$w" name="w" value="0"/><rect in="{a}" id="$d" name="d" x="{{= deref($w, 'transform2d.x', -1)}}" y="0" w="1" h="1"/></edit>"#), None).unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("d", plan.ops)).unwrap();
    let (w, d) = (plan.aliases["$w"], plan.aliases["$d"]);
    let p = store.register_producer("host");
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: a, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    let t_at = |v: f64| (Leaf { node: t, path: path("let.value") }, Value::Number(v));
    let w_at = |n: NodeId| (Leaf { node: w, path: path("let.value") }, Value::Ref(n));
    store.publish(p, BTreeMap::from([t_at(5.0), w_at(rects[0])]));
    let x = Leaf { node: rects[4], path: path("transform2d.x") };
    assert!(store.reactive().is_stale(x), "p4 rests unobserved");
    store.publish(p, BTreeMap::from([t_at(5.0), w_at(rects[4])]));
    assert_eq!(store.resolved(d, path("transform2d.x")), Some(Value::Number(9.0)), "p4 at t + 4");
    assert!(!store.reactive().is_stale(x));
    store.publish(p, BTreeMap::from([t_at(6.0), w_at(rects[4])]));
    assert_eq!(store.resolved(d, path("transform2d.x")), Some(Value::Number(10.0)), "and keeps it current");
}

/// Phase 4's exit (reactive-engine.md §17): a pulse from a laser in a
/// composition no observer shows reaches a meter in one that is shown, on
/// time, while the hidden fiber's light is not evaluated.
#[test]
fn a_pulse_from_an_unobserved_laser_reaches_an_observed_meter_on_time() {
    let build = || {
        let (mut store, shown, _) = sheet(r#"<group name="spare"/>"#);
        let plan = edit::plan(&store, r#"<edit><composition id="$b" name="bench" dims="2d"/></edit>"#, None).unwrap_or_else(|f| panic!("{}", f.message()));
        store.apply(tx("bench", plan.ops)).unwrap();
        let bench = plan.aliases["$b"];
        let laser = store.place_standard("laser", bench, [0.0, 0.0], Actor::Human).unwrap();
        let fiber = store.place_standard("fiber", bench, [200.0, 0.0], Actor::Human).unwrap();
        let entity = |s: &Store, u: NodeId| expand::use_entity(s.tree(), u).unwrap();
        let port = |s: &Store, u: NodeId, name: &str| {
            let e = entity(s, u);
            s.tree().children(e).iter().copied().find(|&p| s.tree().get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
        };
        let (le, fe) = (entity(&store, laser), entity(&store, fiber));
        let (from, to, out) = (port(&store, laser, "out"), port(&store, fiber, "in"), port(&store, fiber, "out"));
        let read = |l: Leaf| store.resolved(l.node, l.path);
        let mut ops = author::connect(store.tree(), &read, from, to).unwrap();
        let num = |v: f64| Slot::Const(Value::Number(v));
        ops.push(Op::Set { id: fe, path: path("args.delay_s"), slot: num(1.0) });
        ops.push(Op::Set { id: fe, path: path("args.loss_db_per_km"), slot: num(0.0) });
        ops.push(Op::Set { id: le, path: path("args.pulse_hz"), slot: num(1.0) });
        store.apply(tx("plug", ops)).unwrap_or_else(|e| panic!("{e:?}"));
        let plan = edit::plan(&store, &format!(r#"<edit><rect in="{shown}" id="$m" name="meter" x="{{= {out}.port.signal / 1 mW}}" y="0" w="1" h="1"/></edit>"#), None).unwrap_or_else(|f| panic!("{}", f.message()));
        store.apply(tx("meter", plan.ops)).unwrap();
        (store, shown, plan.aliases["$m"])
    };
    let (mut all, ..) = build();
    let (mut store, shown, meter) = build();
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: shown, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    let x = |s: &Store| s.resolved(meter, path("transform2d.x")).unwrap().as_f64().unwrap();
    let mut seen = Vec::new();
    for k in 0..=60 {
        let t = k as f64 * 0.05;
        all.advance(t);
        store.advance(t);
        assert_eq!(x(&store), x(&all), "at {t} s");
        seen.push(x(&store) > 0.0);
    }
    assert!(seen.iter().any(|on| *on) && seen.iter().any(|on| !on), "the meter sees the pulses come and go");
    assert!(store.reactive().stale_count() > 0, "the hidden fiber's light rests");
}

/// A placed instance's entity is outside every composition, but it runs
/// only where it is pictured (handoff-patches.md, PT-1): a plot on a hidden
/// page reading that page's accumulator leaves it held, as the demo's
/// Lorenz page; the same plot placed on the shown page sets it going.
#[test]
fn a_hidden_page_s_own_placed_plot_does_not_keep_it_running() {
    let acc = r#"<accumulate id="$acc" name="acc" initial="0" next="{= $acc + $acc.accumulate.dt}"/>"#;
    let plot = |into: &str| format!(r#"<place def="std:plot" in="{into}" x="0" y="0" w="200" h="100" xs="{{= range(0, 1, 3)}}" ys="{{= range(0, $acc.accumulate.value, 3)}}"/>"#);
    for (into, runs) in [("$b", false), ("$a", true)] {
        let (mut store, _all, _p, n) = split(&format!(r#"<composition id="$a" name="a" dims="2d"/><composition id="$b" name="b" dims="2d">{acc}</composition>{}"#, plot(into)));
        let at = |s: &Store| s.resolved(n["$acc"], path("accumulate.value")).unwrap().as_f64().unwrap();
        for k in 0..=6 {
            store.advance(k as f64 * 0.05);
        }
        assert_eq!(at(&store) > 0.0, runs, "plot placed in {into}");
    }
}

/// A trace's change in flight on a hidden page stays in flight
/// (handoff-patches.md PT-1.2): the page's clock stands still, so the change
/// arrives a delay after it was made in the page's own time.
#[test]
fn a_hidden_page_s_trace_keeps_its_change_in_flight() {
    let (mut store, _all, _p, n) = split(
        r#"<composition id="$a" name="a" dims="2d"/>
        <composition id="$b" name="b" dims="2d"><let id="$lit" name="lit" value="false"/><trace id="$tr" name="tr" of="{= $lit}" delay="1"/></composition>"#,
    );
    let out = |s: &Store| s.resolved(n["$tr"], path("trace.out")).unwrap();
    let b = store.observe(Interest::Region { composition: n["$b"], rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    store.advance(0.0);
    store.apply(tx("on", vec![Op::Set { id: n["$lit"], path: path("let.value"), slot: Slot::Const(Value::Bool(true)) }])).unwrap();
    store.advance(0.5);
    store.advance(0.6);
    store.unobserve(b);
    for k in 0..30 {
        let out_now = store.advance(0.7 + k as f64 * 0.1);
        assert!(out_now.wake.live.is_empty(), "the hidden trace is reported moving");
    }
    assert_eq!(out(&store), Value::Bool(false), "still in flight while hidden");
    store.observe(Interest::Region { composition: n["$b"], rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    store.advance(4.0);
    assert_eq!(out(&store), Value::Bool(false), "0.7 s on the page's clock");
    store.advance(4.75);
    assert_eq!(out(&store), Value::Bool(false), "1.45 s on the page's clock");
    store.advance(4.85);
    assert_eq!(out(&store), Value::Bool(true), "1.55 s on the page's clock: it has arrived");
}

/// `run = always` keeps a hidden composition's clock running
/// (handoff-patches.md PT-1.3): its accumulator steps, and its trace's
/// change arrives on time, with no view showing it.
#[test]
fn an_always_running_page_keeps_running_while_hidden() {
    let (mut store, _all, _p, n) = split(
        r#"<composition id="$a" name="a" dims="2d"/>
        <composition id="$b" name="b" dims="2d" run="always"><let id="$lit" name="lit" value="false"/><trace id="$tr" name="tr" of="{= $lit}" delay="1"/><accumulate id="$acc" name="acc" initial="0" next="{= $acc + $acc.accumulate.dt}"/></composition>"#,
    );
    let acc = |s: &Store| s.resolved(n["$acc"], path("accumulate.value")).unwrap().as_f64().unwrap();
    store.advance(0.0);
    store.apply(tx("on", vec![Op::Set { id: n["$lit"], path: path("let.value"), slot: Slot::Const(Value::Bool(true)) }])).unwrap();
    for k in 1..=12 {
        store.advance(k as f64 * 0.1);
    }
    assert!(acc(&store) > 0.5, "the hidden accumulator stepped: {}", acc(&store));
    assert_eq!(store.resolved(n["$tr"], path("trace.out")), Some(Value::Bool(true)), "the change arrived a second later");
}

/// The gate keeps its decision through value writes (the playhead's `t`
/// every frame while playing), and decides again when one of the two leaves
/// it reads is set: `run = always` on a hidden page set mid-run starts its
/// clock (`op::reshapes`).
#[test]
fn setting_run_to_always_on_a_hidden_page_starts_it() {
    let (mut store, _all, _p, n) = split(
        r#"<composition id="$a" name="a" dims="2d"/>
        <composition id="$b" name="b" dims="2d"><accumulate id="$acc" name="acc" initial="0" next="{= $acc + $acc.accumulate.dt}"/></composition>"#,
    );
    let acc = |s: &Store| s.resolved(n["$acc"], path("accumulate.value")).unwrap().as_f64().unwrap();
    for k in 0..=3 {
        store.set_time(k as f64 * 0.1);
        store.advance(k as f64 * 0.1);
    }
    assert_eq!(acc(&store), 0.0, "hidden and unread: held");
    let run: LeafPath = "composition.run".parse().unwrap();
    store.apply(tx("always", vec![Op::Set { id: n["$b"], path: run, slot: Slot::Const(Value::Enum("always".into())) }])).unwrap();
    for k in 4..=12 {
        store.set_time(k as f64 * 0.1);
        store.advance(k as f64 * 0.1);
    }
    assert!(acc(&store) > 0.5, "run = always set mid-run: the accumulator stepped: {}", acc(&store));
}

/// EN-8a: at rest, an unobserved composition's cells stay resting with zero
/// idle-sweep wakeups, and catch up in one pass as soon as the composition is
/// observed.
#[test]
fn at_rest_unobserved_pictures_stay_resting_until_observed() {
    let (mut all, ..) = two_tabs();
    let (mut store, t, a, b, rects) = two_tabs();
    let p = store.register_producer("host");
    all.register_producer("host");
    store.set_observe_all(false);
    let obs = store.observe(Interest::Region { composition: a, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    let at = BTreeMap::from([(Leaf { node: t, path: path("let.value") }, Value::Number(7.0))]);
    store.publish(p, at.clone());
    all.publish(p, at);
    assert!(store.reactive().stale_count() > 0);
    let before = store.reactive().evaluations;
    let out = store.advance(1.0);
    assert_eq!(out.wake.next, None, "no idle-sweep wakeup at rest");
    assert_eq!(store.reactive().evaluations, before, "no hidden evaluations at rest");
    assert!(store.reactive().stale_count() > 0, "hidden cells stay resting until needed");
    assert_eq!(store.advance(1.05).changed, false, "stays at rest");
    // Showing the hidden composition catches it up cleanly in one pass.
    store.reobserve(obs, Interest::Region { composition: b, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    assert_eq!(store.reactive().stale_count(), 0);
    for &r in &rects {
        assert_eq!(store.resolved(r, path("transform2d.x")), all.resolved(r, path("transform2d.x")));
    }
}

/// Compositions `$a` and `$b` with what `body` holds (it names them), a
/// store that observes only `$a`, and the reference that observes
/// everything, each with a host producer.
fn split(body: &str) -> (Store, Store, ProducerId, BTreeMap<String, NodeId>) {
    let build = || {
        let mut store = Store::new();
        let plan = edit::plan(&store, &format!(r#"<edit><model name="model"/>{body}</edit>"#), None).unwrap_or_else(|f| panic!("{}", f.message()));
        store.apply(tx("split", plan.ops)).unwrap();
        (store, plan.aliases)
    };
    let (mut all, _) = build();
    let (mut store, n) = build();
    let p = store.register_producer("host");
    all.register_producer("host");
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: n["$a"], rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    (store, all, p, n)
}

fn lets(pairs: &[(NodeId, Value)]) -> BTreeMap<Leaf, Value> {
    pairs.iter().map(|(n, v)| (Leaf { node: *n, path: path("let.value") }, v.clone())).collect()
}

/// Layout reads its children's sizes outside the graph: a hidden stack
/// whose first child grew still places the second where a shown picture
/// reads it (reactive-engine.md §8.9).
#[test]
fn a_hidden_stack_places_from_current_sizes() {
    let (mut store, mut all, p, n) = split(
        r#"<let id="$t" name="t" value="1"/>
        <composition id="$a" name="a" dims="2d"><rect id="$s" name="s" x="{= $r2.transform2d.x}" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d"><group id="$g" x="0" y="0" layout.mode="stack_x" layout.gap="0"><rect id="$r1" w="{= $t * 10}" h="10"/><rect id="$r2" w="10" h="10"/></group></composition>"#,
    );
    let x = |s: &Store| s.resolved(n["$s"], path("transform2d.x"));
    let before = x(&all);
    for v in [4.0, 2.0] {
        store.publish(p, lets(&[(n["$t"], Value::Number(v))]));
        all.publish(p, lets(&[(n["$t"], Value::Number(v))]));
        assert_eq!(x(&store), x(&all), "t = {v}");
    }
    assert_ne!(x(&all), before, "the second child moved");
}

/// A tracked read reaching a stale cell does not trust that cell's old
/// edges: the hidden rect stopped reading the shown one before it went
/// stale, so the shown one reading it back is no cycle.
#[test]
fn a_tracked_read_into_a_stale_cell_finds_no_false_cycle() {
    let (mut store, mut all, p, n) = split(
        r#"<let id="$u" name="u" value="0"/><let id="$v" name="v" value="0"/>
        <composition id="$a" name="a" dims="2d"><rect id="$s" name="s" x="0" y="{= deref($v, 'transform2d.x', 0)}" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d"><rect id="$h" name="h" x="{= deref($u, 'transform2d.y', 7)}" y="0" w="1" h="1"/></composition>"#,
    );
    let (u, v, s, h) = (n["$u"], n["$v"], n["$s"], n["$h"]);
    // h reads s while everything is observed, then rests.
    store.set_observe_all(true);
    let zero = Value::Number(0.0);
    for (uv, vv) in [(Value::Ref(s), zero.clone()), (zero.clone(), zero.clone()), (zero.clone(), Value::Ref(h))] {
        store.publish(p, lets(&[(u, uv.clone()), (v, vv.clone())]));
        all.publish(p, lets(&[(u, uv), (v, vv)]));
        store.set_observe_all(false);
    }
    let y = Leaf { node: s, path: path("transform2d.y") };
    assert_eq!(store.reactive().errors.get(&y), None);
    assert_eq!(store.resolved(s, path("transform2d.y")), Some(Value::Number(7.0)));
    assert_eq!(store.resolved(s, path("transform2d.y")), all.resolved(s, path("transform2d.y")));
}

/// One tracked read can wake a long hidden chain: its evaluations count
/// against no budget sized before the wake.
#[test]
fn a_tracked_read_wakes_a_long_hidden_chain() {
    let chain: String = (1..1100).map(|i| format!(r#"<rect id="$c{i}" x="{{= $c{}.transform2d.x + 1}}" y="0" w="1" h="1"/>"#, i - 1)).collect();
    let (mut store, mut all, p, n) = split(&format!(
        r#"<let id="$t" name="t" value="0"/><let id="$w" name="w" value="0"/>
        <composition id="$a" name="a" dims="2d"><rect id="$d" name="d" x="{{= deref($w, 'transform2d.x', -1)}}" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d"><rect id="$c0" x="{{= $t}}" y="0" w="1" h="1"/>{chain}</composition>"#
    ));
    let (t, w, d) = (n["$t"], n["$w"], n["$d"]);
    for wv in [Value::Number(0.0), Value::Ref(n["$c1099"])] {
        store.publish(p, lets(&[(t, Value::Number(5.0)), (w, wv.clone())]));
        all.publish(p, lets(&[(t, Value::Number(5.0)), (w, wv)]));
    }
    assert_eq!(store.reactive().errors.get(&Leaf { node: d, path: path("transform2d.x") }), None);
    assert_eq!(store.resolved(d, path("transform2d.x")), Some(Value::Number(1104.0)));
    assert_eq!(all.resolved(d, path("transform2d.x")), Some(Value::Number(1104.0)));
}

/// A handle reads its range outside the graph: an edit through it on a
/// hidden slider whose maximum moved holds the value to the new maximum.
#[test]
fn a_handle_on_a_hidden_slider_reads_its_current_range() {
    let (mut store, mut all, p, n) = split(
        r#"<let id="$t" name="t" value="10"/>
        <composition id="$a" name="a" dims="2d"><rect name="s" x="0" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d"><slider id="$k" name="k" x="0" y="0" w="100" h="20" min="0" max="{= $t}" value="1"/></composition>"#,
    );
    let k = n["$k"];
    for s in [&mut store, &mut all] {
        s.publish(p, lets(&[(n["$t"], Value::Number(20.0))]));
        s.set_through_handle(k, path("slider.value"), Value::Number(15.0), "set", Actor::Human, 1).unwrap();
    }
    assert_eq!(store.resolved(k, path("slider.value")), Some(Value::Number(15.0)));
    assert_eq!(store.resolved(k, path("slider.value")), all.resolved(k, path("slider.value")));
}

/// An accumulator in a hidden composition that a shown picture reads keeps
/// stepping: what is shown needs it (§8.8).
#[test]
fn a_hidden_accumulator_a_shown_picture_reads_keeps_stepping() {
    let (mut store, mut all, _, n) = split(
        r#"<composition id="$a" name="a" dims="2d"><rect id="$m" name="m" x="{= $acc}" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d"><accumulate id="$acc" name="acc" initial="0" next="{= $acc + $acc.accumulate.dt}"/></composition>"#,
    );
    for t in [0.0, 0.1, 0.2, 0.3] {
        store.advance(t);
        all.advance(t);
    }
    let x = |s: &Store| s.resolved(n["$m"], path("transform2d.x"));
    assert!(x(&all).and_then(|v| v.as_f64()).is_some_and(|v| v > 0.0));
    assert_eq!(x(&store), x(&all));
}

/// The gate keeps its decision while nothing it decides from changes
/// (`Clocks::gated`: a drag's frames recompute nothing), and decides again
/// when a read changes outside a transaction: a preview that points a shown
/// picture's tracked read at a hidden accumulator starts that composition's
/// clock.
#[test]
fn a_preview_that_starts_reading_a_hidden_accumulator_runs_it() {
    let (mut store, _, _, n) = split(
        r#"<let id="$w" name="w" value="0"/>
        <composition id="$a" name="a" dims="2d"><rect id="$m" name="m" x="{= deref($w, 'accumulate.value', -1)}" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d"><accumulate id="$acc" name="acc" initial="0" next="{= $acc + $acc.accumulate.dt}"/></composition>"#,
    );
    let x = |s: &Store| s.resolved(n["$m"], path("transform2d.x")).and_then(|v| v.as_f64()).unwrap();
    store.advance(0.0);
    store.advance(0.1);
    assert_eq!(x(&store), -1.0);
    store.preview(n["$w"], path("let.value"), Value::Ref(n["$acc"]));
    store.advance(0.2);
    let first = x(&store);
    store.advance(0.3);
    store.advance(0.4);
    assert!(x(&store) > first, "the hidden accumulator a preview made a shown picture read held still: {first} then {}", x(&store));
}

/// A preview's layout reads outside the graph too: a handle on a shown
/// slider whose maximum is where a hidden stack places its second child
/// holds an edit to the maximum the preview made.
#[test]
fn a_preview_lays_out_a_hidden_stack_from_current_sizes() {
    let (mut store, mut all, _, n) = split(
        r#"<let id="$t" name="t" value="10"/>
        <composition id="$b" name="b" dims="2d"><group x="0" y="0" layout.mode="stack_x" layout.gap="0"><rect id="$r1" w="{= $t}" h="10"/><rect id="$r2" w="10" h="10"/></group></composition>
        <composition id="$a" name="a" dims="2d"><slider id="$m" name="m" x="0" y="0" w="100" h="20" min="0" max="{= $r2.transform2d.x}" value="1"/></composition>"#,
    );
    for s in [&mut store, &mut all] {
        s.preview(n["$t"], path("let.value"), Value::Number(100.0));
        s.set_through_handle(n["$m"], path("slider.value"), Value::Number(50.0), "set", Actor::Human, 1).unwrap();
    }
    assert_eq!(all.resolved(n["$m"], path("slider.value")), Some(Value::Number(50.0)));
    assert_eq!(store.resolved(n["$m"], path("slider.value")), all.resolved(n["$m"], path("slider.value")));
}

/// A tracked read waking a stale cell whose own wake reaches further down
/// a hidden chain waits for that cell before checking for a cycle.
#[test]
fn a_nested_tracked_wake_finds_no_false_cycle() {
    let (mut store, mut all, p, n) = split(
        r#"<let id="$u" name="u" value="0"/><let id="$v" name="v" value="0"/><let id="$t" name="t" value="1"/>
        <composition id="$a" name="a" dims="2d"><rect id="$s" name="s" x="0" y="{= deref($v, 'transform2d.x', 0)}" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d">
          <rect id="$h" name="h" x="{= deref($u, 'transform2d.y', 7)}" y="0" w="1" h="1"/>
          <rect id="$b0" x="{= $t}" y="{= $b0.transform2d.x}" w="1" h="1"/>
          <rect id="$b1" x="{= $b0.transform2d.y}" y="{= $b1.transform2d.x}" w="1" h="1"/>
          <rect id="$b2" x="{= $b1.transform2d.y}" y="{= $b2.transform2d.x}" w="1" h="1"/>
        </composition>"#,
    );
    let (u, v, t) = (n["$u"], n["$v"], n["$t"]);
    store.set_observe_all(true);
    let zero = Value::Number(0.0);
    for (uv, vv, tv) in [(Value::Ref(n["$s"]), zero.clone(), 1.0), (Value::Ref(n["$b2"]), zero.clone(), 23.0), (Value::Ref(n["$b2"]), Value::Ref(n["$h"]), 23.0)] {
        let values = lets(&[(u, uv), (v, vv), (t, Value::Number(tv))]);
        store.publish(p, values.clone());
        all.publish(p, values);
        store.set_observe_all(false);
    }
    let y = Leaf { node: n["$s"], path: path("transform2d.y") };
    assert_eq!(store.reactive().errors.get(&y), None);
    assert_eq!(store.resolved(y.node, y.path), Some(Value::Number(23.0)));
    assert_eq!(store.resolved(y.node, y.path), all.resolved(y.node, y.path));
}

/// What a trace follows reaches its output outside the graph: a shown
/// picture reading a hidden trace of a hidden accumulator keeps it
/// stepping.
#[test]
fn a_hidden_accumulator_read_through_a_trace_keeps_stepping() {
    let (mut store, mut all, _, n) = split(
        r#"<composition id="$a" name="a" dims="2d"><rect id="$m" name="m" x="{= $delay.trace.out}" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d"><accumulate id="$acc" initial="0" next="{= $acc + $acc.accumulate.dt}"/><trace id="$delay" of="{= $acc}" delay="0.1"/></composition>"#,
    );
    for t in [0.0, 0.1, 0.2, 0.3, 0.4] {
        store.advance(t);
        all.advance(t);
    }
    let x = |s: &Store| s.resolved(n["$m"], path("transform2d.x"));
    assert!(x(&all).and_then(|v| v.as_f64()).is_some_and(|v| v > 0.0));
    assert_eq!(x(&store), x(&all));
}

/// A hidden cell a shown tracked read woke rests again once that read
/// moves away, though the observers were refreshed in between.
#[test]
fn a_hidden_cell_rests_once_the_tracked_read_that_woke_it_leaves() {
    let (mut store, _, p, n) = split(
        r#"<let id="$t" name="t" value="0"/><let id="$w" name="w" value="0"/>
        <composition id="$a" name="a" dims="2d"><rect id="$s" name="s" x="{= deref($w, 'transform2d.x', 0)}" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d"><rect id="$h" name="h" x="{= $t}" y="0" w="1" h="1"/></composition>"#,
    );
    let (t, w, h) = (n["$t"], n["$w"], n["$h"]);
    store.publish(p, lets(&[(w, Value::Ref(h)), (t, Value::Number(1.0))]));
    let id = store.observe(Interest::Region { composition: n["$a"], rect: [1.0, 1.0, 100.0, 100.0], zoom: 1.0 });
    store.unobserve(id);
    store.publish(p, lets(&[(w, Value::Number(0.0)), (t, Value::Number(1.0))]));
    store.advance(0.0);
    store.publish(p, lets(&[(w, Value::Number(0.0)), (t, Value::Number(2.0))]));
    assert_eq!(store.resolved(h, path("transform2d.x")), Some(Value::Number(1.0)), "rests at its last value");
    assert!(store.reactive().is_stale(Leaf { node: h, path: path("transform2d.x") }));
}

/// The idle sweep keeps necessity closed: a cell it brought up to date
/// that a woken reader now reads stays needed, so a shown read of that
/// reader later is current.
#[test]
fn the_idle_sweep_keeps_what_a_woken_cell_reads_needed() {
    let (mut store, mut all, p, n) = split(
        r#"<let id="$t" name="t" value="0"/><let id="$w" name="w" value="0"/><let id="$v" name="v" value="0"/>
        <composition id="$a" name="a" dims="2d"><rect id="$s" name="s" x="{= deref($v, 'transform2d.x', -1)}" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d">
          <rect id="$h0" x="{= $t}" y="0" w="1" h="1"/>
          <rect id="$h1" x="{= $h0.transform2d.x + 1}" y="0" w="1" h="1"/>
          <rect id="$d" x="{= deref($w, 'transform2d.x', -1)}" y="0" w="1" h="1"/>
        </composition>"#,
    );
    let tail = Value::Ref(n["$h1"]);
    for s in [&mut store, &mut all] {
        s.publish(p, lets(&[(n["$t"], Value::Number(5.0)), (n["$w"], tail.clone())]));
        s.advance(0.0);
        s.publish(p, lets(&[(n["$t"], Value::Number(6.0)), (n["$w"], tail.clone()), (n["$v"], tail.clone())]));
    }
    assert_eq!(store.resolved(n["$s"], path("transform2d.x")), Some(Value::Number(7.0)));
    assert_eq!(store.resolved(n["$s"], path("transform2d.x")), all.resolved(n["$s"], path("transform2d.x")));
}

/// The network's solver reads a hidden rect through a branch its own
/// solution opens (`Q = P > 0.5 V ? deref(w).x × 1 V : 0 V`, `P = t × 1 V`):
/// a shown meter of `Q` reaches what observing everything gives.
#[test]
fn the_network_reads_a_hidden_rect_through_the_branch_its_solution_opens() {
    use iron_document::facet::{Args, Equation, Instance, Port, Size, Transform2d};
    let net = || {
        let mut store = Store::new();
        let plan = edit::plan(
            &store,
            r#"<edit><model id="$model" name="model"/><library><definition id="$def" name="test"/></library>
            <let id="$t" name="t" value="0"/><let id="$w" name="w" value="0"/>
            <composition id="$a" name="a" dims="2d"/>
            <composition id="$b" name="b" dims="2d"><rect id="$h" name="h" x="{= $t * 10}" y="0" w="1" h="1"/></composition></edit>"#,
            None,
        )
        .unwrap_or_else(|f| panic!("{}", f.message()));
        store.apply(tx("tabs", plan.ops)).unwrap();
        let n = plan.aliases;
        let ids = store.authored().next_ids(6);
        let [entity, p, q, eq_p, eq_q, meter] = ids[..] else { unreachable!() };
        let create = |id, t: &str, parent, facets| Op::Create { id, ty: ty(t), parent: Some(parent), order: OrderKey::first(), facets };
        let port = |name: &str| vec![Port { kind: Slot::Const(Value::Str("electrical".into())), ..Default::default() }.wrap(), Name { text: Slot::Const(Value::Str(name.into())) }.wrap()];
        let bound = |s: String| Slot::Bound(Expr::parse(&s).unwrap());
        let equation = |l: String, r: String| vec![Equation { lhs: bound(l), rhs: bound(r) }.wrap()];
        let mut at = Transform2d::default();
        at.x = bound(format!("{q}.port.across / (1 V)"));
        let ops = vec![
            create(entity, "entity", n["$model"], vec![Instance { def: Slot::Const(Value::Ref(n["$def"])), ..Default::default() }.wrap(), Args::default().wrap()]),
            create(p, "port", entity, port("p")),
            create(q, "port", entity, port("q")),
            create(eq_p, "equation", entity, equation(format!("{p}.port.across"), format!("{}.let.value * (1 V)", n["$t"]))),
            create(eq_q, "equation", entity, equation(format!("{q}.port.across"), format!("{p}.port.across > 0.5 V ? deref({}.let.value, 'transform2d.x', 0) * (1 V) : 0 V", n["$w"]))),
            create(meter, "rect", n["$a"], vec![at.wrap(), Size::default().wrap()]),
        ];
        store.apply(tx("network", ops)).unwrap();
        (store, n, meter)
    };
    let (mut store, n, meter) = net();
    let (mut all, _, _) = net();
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: n["$a"], rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    for s in [&mut store, &mut all] {
        let p = s.register_producer("host");
        s.publish(p, lets(&[(n["$t"], Value::Number(1.0)), (n["$w"], Value::Ref(n["$h"]))]));
    }
    let read = |s: &Store| s.resolved(meter, path("transform2d.x")).and_then(|v| v.as_f64()).unwrap();
    assert!((read(&all) - 10.0).abs() < 1e-5, "{}", read(&all));
    assert!((read(&store) - 10.0).abs() < 1e-5, "{}", read(&store));
}

/// A host producer that publishes every frame keeps the store from resting:
/// the idle sweep waits, so hidden pictures cost nothing meanwhile (not one
/// frame later). Once swept, they rest unneeded again.
#[test]
fn a_publish_every_frame_leaves_hidden_pictures_resting() {
    let (mut store, t, a, _, _) = two_tabs();
    let p = store.register_producer("host");
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: a, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    let at = |v: f64| BTreeMap::from([(Leaf { node: t, path: path("let.value") }, Value::Number(v))]);
    store.advance(0.0);
    let before = store.reactive().evaluations;
    for i in 1..=10 {
        store.publish(p, at(f64::from(i)));
        store.advance(f64::from(i) / 60.0);
    }
    assert_eq!(store.reactive().evaluations - before, 10, "the shown rect, once a frame");
    // At rest: no idle sweep wakes hidden pictures, and another publish still costs the shown rect only.
    let rest_before = store.reactive().evaluations;
    assert_eq!(store.advance(1.0).wake.next, None);
    assert_eq!(store.reactive().evaluations, rest_before);
    let before = store.reactive().evaluations;
    store.publish(p, at(11.0));
    assert_eq!(store.reactive().evaluations - before, 1, "hidden pictures stayed resting");
}

/// A cycle check waits for the whole pass, not only what it reads
/// directly: the hidden `K` that `H` reads is still to switch its tracked
/// read away from the shown `S`.
#[test]
fn a_tracked_wake_two_hops_down_finds_no_false_cycle() {
    let (mut store, mut all, p, n) = split(
        r#"<let id="$u" name="u" value="0"/><let id="$v" name="v" value="0"/><let id="$t" name="t" value="1"/>
        <composition id="$a" name="a" dims="2d"><rect id="$s" name="s" x="0" y="{= deref($v, 'transform2d.x', 0)}" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d">
          <rect id="$h" name="h" x="{= $k.transform2d.x + 1}" y="0" w="1" h="1"/>
          <rect id="$k" name="k" x="{= deref($u, 'transform2d.y', 7)}" y="0" w="1" h="1"/>
          <rect id="$b0" x="{= $t}" y="{= $b0.transform2d.x}" w="1" h="1"/>
          <rect id="$b1" x="{= $b0.transform2d.y}" y="{= $b1.transform2d.x}" w="1" h="1"/>
          <rect id="$b2" x="{= $b1.transform2d.y}" y="{= $b2.transform2d.x}" w="1" h="1"/>
        </composition>"#,
    );
    let (u, v, t) = (n["$u"], n["$v"], n["$t"]);
    store.set_observe_all(true);
    let zero = Value::Number(0.0);
    for (uv, vv, tv) in [(Value::Ref(n["$s"]), zero.clone(), 1.0), (Value::Ref(n["$b2"]), zero.clone(), 23.0), (Value::Ref(n["$b2"]), Value::Ref(n["$h"]), 23.0)] {
        let values = lets(&[(u, uv), (v, vv), (t, Value::Number(tv))]);
        store.publish(p, values.clone());
        all.publish(p, values);
        store.set_observe_all(false);
    }
    let y = Leaf { node: n["$s"], path: path("transform2d.y") };
    assert_eq!(store.reactive().errors.get(&y), None);
    assert_eq!(store.resolved(y.node, y.path), Some(Value::Number(24.0)));
    assert_eq!(store.resolved(y.node, y.path), all.resolved(y.node, y.path));
}

/// A hidden accumulator that reaches a shown picture through a stack's
/// placements keeps stepping: its composition is read from outside.
#[test]
fn a_hidden_accumulator_read_through_layout_keeps_stepping() {
    let (mut store, mut all, _, n) = split(
        r#"<composition id="$a" name="a" dims="2d"><rect id="$m" name="m" x="{= $r2.transform2d.x}" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d"><accumulate id="$acc" initial="0" next="{= $acc + $acc.accumulate.dt * 100}"/>
          <group x="0" y="0" layout.mode="stack_x" layout.gap="0"><rect id="$r1" w="{= $acc}" h="1"/><rect id="$r2" w="1" h="1"/></group></composition>"#,
    );
    for t in [0.0, 0.1, 0.2, 0.3] {
        store.advance(t);
        all.advance(t);
    }
    let x = |s: &Store| s.resolved(n["$m"], path("transform2d.x"));
    assert!(x(&all).and_then(|v| v.as_f64()).is_some_and(|v| v > 0.0));
    assert_eq!(x(&store), x(&all));
}

/// A publish read only by hidden pictures evaluates nothing, yet keeps the
/// store from resting: the idle sweep does not evaluate them each frame.
#[test]
fn a_publish_only_hidden_pictures_read_leaves_them_resting() {
    let (mut store, _, p, n) = split(
        r#"<let id="$h" name="h" value="0"/>
        <composition id="$a" name="a" dims="2d"><rect name="s" x="0" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d">
          <rect id="$p1" x="{= $h + 1}" y="0" w="1" h="1"/>
          <rect id="$p2" x="{= $p1.transform2d.x + 1}" y="0" w="1" h="1"/>
          <rect id="$p3" x="{= $h + 3}" y="{= $p2.transform2d.x}" w="1" h="1"/>
        </composition>"#,
    );
    store.advance(0.0);
    let before = store.reactive().evaluations;
    for i in 1..=10 {
        store.publish(p, lets(&[(n["$h"], Value::Number(f64::from(i)))]));
        store.advance(f64::from(i) / 60.0);
    }
    assert_eq!(store.reactive().evaluations - before, 0, "nothing shown reads h");
}

/// EN-8b: a dynamic cross-owner `deref` activates a hidden composition's clock
/// while reading it, pauses it when the read drops, and resumes without jumping
/// over the paused interval when re-attached.
#[test]
fn cross_owner_deref_activates_and_pauses_hidden_accumulator() {
    let (mut store, _, p, n) = split(
        r#"<let id="$sel" name="sel" value="0"/>
        <composition id="$a" name="a" dims="2d">
          <rect id="$m" x="{= deref($sel, 'accumulate.value', 0)}" y="0" w="1" h="1"/>
        </composition>
        <composition id="$b" name="b" dims="2d">
          <accumulate id="$acc" initial="0" next="{= $acc + $acc.accumulate.dt * 10}"/>
        </composition>"#,
    );
    let acc = |s: &Store| s.resolved(n["$acc"], path("accumulate.value")).and_then(|v| v.as_f64()).unwrap();
    let mx = |s: &Store| s.resolved(n["$m"], path("transform2d.x")).and_then(|v| v.as_f64()).unwrap();

    // Initially $sel = 0: $b is hidden and unread, so $acc stays at 0.
    store.advance(0.0);
    store.advance(0.5);
    assert_eq!(acc(&store), 0.0, "unobserved and unread: paused");
    assert_eq!(mx(&store), 0.0);

    // Point $sel at $acc: cross-owner read activates $b.
    store.publish(p, lets(&[(n["$sel"], Value::Ref(n["$acc"]))]));
    for k in 6..=11 {
        store.advance(k as f64 * 0.1);
    }
    let after_run = acc(&store);
    assert!(after_run > 4.0 && after_run < 7.0, "active via cross-owner deref: {after_run}");
    assert!((mx(&store) - after_run).abs() < 1e-6);

    // Drop the cross-owner read: $b pauses again and does not advance.
    store.publish(p, lets(&[(n["$sel"], Value::Number(0.0))]));
    for k in 12..=50 {
        store.advance(k as f64 * 0.1);
    }
    assert!((acc(&store) - after_run).abs() < 1e-6, "paused again after read dropped");

    // Re-attach the cross-owner read: $b resumes cleanly without jumping over the 3.9s pause.
    store.publish(p, lets(&[(n["$sel"], Value::Ref(n["$acc"]))]));
    for k in 51..=56 {
        store.advance(k as f64 * 0.1);
    }
    let after_resume = acc(&store);
    let delta = after_resume - after_run;
    assert!(delta > 4.0 && delta < 7.0, "resumed without jumping across pause: delta={delta}");
}

/// EN-8b: reparenting an accumulator from a hidden composition into an observed
/// composition re-owns it and starts it immediately; undoing pauses it again.
#[test]
fn reparenting_an_accumulator_into_an_observed_composition_starts_it() {
    let (mut store, _, _, n) = split(
        r#"<composition id="$a" name="a" dims="2d"><rect x="0" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d">
          <accumulate id="$acc" initial="0" next="{= $acc + $acc.accumulate.dt * 10}"/>
        </composition>"#,
    );
    let acc = |s: &Store| s.resolved(n["$acc"], path("accumulate.value")).and_then(|v| v.as_f64()).unwrap();

    for k in 0..=5 {
        store.advance(k as f64 * 0.1);
    }
    assert_eq!(acc(&store), 0.0, "in hidden $b: paused");

    // Reparent $acc into observed $a.
    store.apply(tx("move", vec![Op::Reparent { id: n["$acc"], parent: Some(n["$a"]), order: OrderKey::first() }])).unwrap();
    for k in 6..=11 {
        store.advance(k as f64 * 0.1);
    }
    let moved = acc(&store);
    assert!(moved > 4.0, "in observed $a: running ({moved})");

    // Undo moves $acc back to hidden $b and pauses it again.
    store.undo(1).unwrap().unwrap();
    for k in 12..=20 {
        store.advance(k as f64 * 0.1);
    }
    assert!((acc(&store) - moved).abs() < 1e-6, "back in hidden $b: paused");
}

/// EN-8b: unpicturing a model entity (removing its `Figures` relation from a
/// hidden composition) transitions its owner from `Entity(e)` to `Free` so it
/// runs freely; undoing restores `Entity(e)` ownership and pauses it again.
#[test]
fn unpicturing_an_entity_transitions_its_owner_to_free() {
    let mut store = Store::new();
    let plan = edit::plan(
        &store,
        r#"<edit>
          <definition id="$def" name="d"/>
          <model name="model">
            <entity id="$e" name="e" instance.def="$def">
              <accumulate id="$acc" initial="0" next="{= $acc + $acc.accumulate.dt * 10}"/>
            </entity>
          </model>
          <composition id="$a" name="a" dims="2d"><rect x="0" y="0" w="1" h="1"/></composition>
          <composition id="$b" name="b" dims="2d"><rect id="$fig" x="0" y="0" w="1" h="1"/></composition>
          <link rel="figures" from="$fig" to="$e"/>
        </edit>"#,
        None,
    )
    .unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("setup", plan.ops)).unwrap();
    let n = plan.aliases;
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: n["$a"], rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    let acc = |s: &Store| s.resolved(n["$acc"], path("accumulate.value")).and_then(|v| v.as_f64()).unwrap();

    // $e is pictured only in hidden $b, so its owner Entity($e) is paused.
    for k in 0..=5 {
        store.advance(k as f64 * 0.1);
    }
    assert_eq!(acc(&store), 0.0, "pictured only in hidden $b: paused");

    // Unlink the Figures relation: $e becomes Free and runs immediately.
    let fig_rel = store
        .tree()
        .relations()
        .find(|r| r.rel == RelKind::Figures && r.from == n["$fig"] && r.to == n["$e"])
        .expect("figures relation")
        .id;
    store.apply(tx("unfigure", vec![Op::Unlink { id: fig_rel }])).unwrap();
    for k in 6..=11 {
        store.advance(k as f64 * 0.1);
    }
    let free_val = acc(&store);
    assert!(free_val > 4.0, "unpictured entity is Free and runs ({free_val})");

    // Undo restores the Figures link to hidden $b, re-owning $e as Entity($e) and pausing it.
    store.undo(1).unwrap().unwrap();
    for k in 12..=20 {
        store.advance(k as f64 * 0.1);
    }
    assert!((acc(&store) - free_val).abs() < 1e-6, "re-pictured in hidden $b: paused again");
}

/// EN-8c.0 (council 2026-11-24): switching a shown composition that holds an
/// accumulator from Auto to On Demand stops it at once. Before, the owner
/// index noticed only changes into or out of `always`, so it kept stepping.
#[test]
fn a_shown_composition_switched_to_on_demand_stops_its_accumulator() {
    let (mut store, _, _, n) = split(
        r#"<composition id="$a" name="a" dims="2d">
          <accumulate id="$acc" initial="0" next="{= $acc + $acc.accumulate.dt * 10}"/>
        </composition>"#,
    );
    let acc = |s: &Store| s.resolved(n["$acc"], path("accumulate.value")).and_then(|v| v.as_f64()).unwrap();
    for k in 0..=5 {
        store.advance(k as f64 * 0.1);
    }
    assert!(acc(&store) > 4.0, "shown and auto: running");
    store.apply(tx("on demand", vec![Op::Set { id: n["$a"], path: path("composition.run"), slot: Slot::Const(Value::Enum("on_demand".into())) }])).unwrap();
    store.advance(0.6);
    let held = acc(&store);
    for k in 7..=12 {
        store.advance(k as f64 * 0.1);
    }
    assert!((acc(&store) - held).abs() < 1e-9, "on demand: holds ({held} → {})", acc(&store));
}

/// EN-8c.0: a `Figures` link made and unmade in one transaction leaves its
/// entity unpictured, so it stays `Free` and keeps stepping. Before, the
/// owner index applied the removal first and the link after it.
#[test]
fn a_figures_link_made_and_unmade_in_one_transaction_leaves_the_entity_free() {
    let mut store = Store::new();
    let plan = edit::plan(
        &store,
        r#"<edit>
          <definition id="$def" name="d"/>
          <model name="model">
            <entity id="$e" name="e" instance.def="$def">
              <accumulate id="$acc" initial="0" next="{= $acc + $acc.accumulate.dt * 10}"/>
            </entity>
          </model>
          <composition id="$a" name="a" dims="2d"><rect x="0" y="0" w="1" h="1"/></composition>
          <composition id="$b" name="b" dims="2d"><rect id="$fig" x="0" y="0" w="1" h="1"/></composition>
        </edit>"#,
        None,
    )
    .unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("setup", plan.ops)).unwrap();
    let n = plan.aliases;
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: n["$a"], rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    let acc = |s: &Store| s.resolved(n["$acc"], path("accumulate.value")).and_then(|v| v.as_f64()).unwrap();
    for k in 0..=5 {
        store.advance(k as f64 * 0.1);
    }
    let before = acc(&store);
    assert!(before > 4.0, "unpictured: Free, running ({before})");
    let id = store.authored().next_relation_id();
    let rel = Relation { id, from: n["$fig"], to: n["$e"], rel: RelKind::Figures, data: BTreeMap::new() };
    store.apply(tx("link and unlink", vec![Op::Link { rel }, Op::Unlink { id }])).unwrap();
    for k in 6..=11 {
        store.advance(k as f64 * 0.1);
    }
    assert!(acc(&store) > before + 4.0, "still Free, still running ({})", acc(&store));
}

/// EN-8c.0: undoing a move of a group into a new hidden composition deletes
/// that composition and moves the group back to model space, where it is
/// `Free` and runs. Before, the old owner was read from the tree after the
/// undo, where the deleted composition no longer existed, so the group's
/// accumulator stayed paused under a dead owner.
#[test]
fn undoing_a_move_into_a_new_hidden_composition_frees_what_moved() {
    let (mut store, _, _, n) = split(
        r#"<composition id="$a" name="a" dims="2d"><rect x="0" y="0" w="1" h="1"/></composition>
        <group id="$s" name="s"><accumulate id="$acc" initial="0" next="{= $acc + $acc.accumulate.dt * 10}"/></group>"#,
    );
    let acc = |s: &Store| s.resolved(n["$acc"], path("accumulate.value")).and_then(|v| v.as_f64()).unwrap();
    for k in 0..=5 {
        store.advance(k as f64 * 0.1);
    }
    assert!(acc(&store) > 4.0, "in model space: Free, running");
    let c = store.authored().next_id();
    let comp = Composition { dims: Slot::Const(Value::Enum("2d".into())), ..Default::default() }.wrap();
    let name = Name { text: Slot::Const(Value::Str("c".into())), ..Default::default() }.wrap();
    store
        .apply(tx("move into new", vec![
            Op::Create { id: c, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![comp, name] },
            Op::Reparent { id: n["$s"], parent: Some(c), order: OrderKey::first() },
        ]))
        .unwrap();
    store.advance(0.6);
    let paused = acc(&store);
    for k in 7..=10 {
        store.advance(k as f64 * 0.1);
    }
    assert!((acc(&store) - paused).abs() < 1e-9, "in the hidden new composition: paused");
    store.undo(1).unwrap().unwrap();
    for k in 11..=17 {
        store.advance(k as f64 * 0.1);
    }
    assert!(acc(&store) > paused + 4.0, "moved back to model space: Free, running ({paused} → {})", acc(&store));
}

/// Council review of EN-8c: a bound rect moved from a hidden group into a
/// shown one, while its old group moves into the shown composition in the
/// same transaction, wakes with its new owner, in the frame of the move.
/// Before, whether a move changed owner was judged from the final tree,
/// where both parents are in the shown composition, and what moved was
/// placed only at the next advance.
#[test]
fn a_rect_moved_out_of_a_group_that_moves_too_wakes_where_it_lands() {
    let (mut store, mut all, p, n) = split(
        r#"<let id="$t" name="t" value="0"/>
        <composition id="$a" name="a" dims="2d"><group id="$h" name="h"/></composition>
        <composition id="$b" name="b" dims="2d"><group id="$g" name="g"><rect id="$r" name="r" x="{= $t + 1}" y="0" w="1" h="1"/></group></composition>"#,
    );
    let at = BTreeMap::from([(Leaf { node: n["$t"], path: path("let.value") }, Value::Number(10.0))]);
    store.publish(p, at.clone());
    all.publish(p, at);
    let x = Leaf { node: n["$r"], path: path("transform2d.x") };
    assert!(store.reactive().is_stale(x), "hidden, r rests");
    let moves = vec![
        Op::Reparent { id: n["$r"], parent: Some(n["$h"]), order: OrderKey::first() },
        Op::Reparent { id: n["$g"], parent: Some(n["$a"]), order: OrderKey::first() },
    ];
    store.apply(tx("moves", moves.clone())).unwrap();
    all.apply(tx("moves", moves)).unwrap();
    // In the frame of the move, before any advance.
    assert!(!store.reactive().is_stale(x), "shown, r is current");
    assert_eq!(store.resolved(x.node, x.path), Some(Value::Number(11.0)));
    assert_eq!(store.resolved(x.node, x.path), all.resolved(x.node, x.path));
}

/// Council review of EN-8c: a query in a hidden composition asks with its
/// input as it is now. Before, the request was made from the resting
/// input's old value.
#[test]
fn a_hidden_query_asks_with_its_current_input() {
    let (mut store, _, p, n) = split(
        r#"<let id="$k" name="k" value="1"/>
        <composition id="$a" name="a" dims="2d"><rect x="0" y="0" w="1" h="1"/></composition>
        <composition id="$h" name="h" dims="2d"><query id="$q" name="q" model="sim" input="{= $k * 1}" debounce="0"/></composition>"#,
    );
    store.advance(0.0);
    store.publish(p, BTreeMap::from([(Leaf { node: n["$k"], path: path("let.value") }, Value::Number(11.0))]));
    let (h, q) = (n["$h"], n["$q"]);
    assert!(store.reactive().is_stale(Leaf { node: q, path: path("query.input") }), "hidden, the input rests");
    let plan = edit::plan(&store, &format!(r#"<edit><text in="{h}" name="reader" content="{{= fmt({q}.query.value)}}"/></edit>"#), None).unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("reader", plan.ops)).unwrap();
    let due = store.advance_queries(1.0);
    assert_eq!(due.len(), 1, "read, the query asks");
    assert_eq!(query::to_plain(&due[0].input), serde_json::json!(11.0));
}

/// Council review of EN-8c: what a pull brings up to date, and what is made
/// in a hidden composition, rests with its owner afterwards. Before, both
/// stayed awake, evaluated on every publish, until their owner was shown
/// and hidden again.
#[test]
fn pulled_and_new_cells_of_a_hidden_composition_rest() {
    let (mut store, t, a, b, rects) = two_tabs();
    let p = store.register_producer("host");
    let at = |v: f64| BTreeMap::from([(Leaf { node: t, path: path("let.value") }, Value::Number(v))]);
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: a, rect: [0.0, 0.0, 100.0, 100.0], zoom: 1.0 });
    store.publish(p, at(1.0));
    store.pull_nodes(&[rects[1]]);
    assert_eq!(store.resolved(rects[1], path("transform2d.x")), Some(Value::Number(2.0)), "pulled, p1 is current");
    let spent = |store: &mut Store, v: f64| {
        let before = store.reactive().evaluations;
        store.publish(p, at(v));
        store.reactive().evaluations - before
    };
    assert_eq!(spent(&mut store, 2.0), 1, "only the shown rect, after a pull");
    let plan = edit::plan(&store, &format!(r#"<edit><let id="$tt" name="tt" value="0"/><rect in="{b}" name="new" x="{{= {t}.let.value + 9}}" y="0" w="1" h="1"/></edit>"#), None).unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("new", plan.ops)).unwrap();
    store.advance(0.0);
    assert_eq!(spent(&mut store, 3.0), 1, "only the shown rect, after a rect made in b");
}

/// Council review of EN-8c: a constraint made on a hidden page is solved
/// from its operands' values as they are now. A pin's operand sits in a
/// group whose bound `x` moved while the page rested.
#[test]
fn a_pin_made_on_a_hidden_page_solves_from_current_values() {
    let (mut store, mut all, p, n) = split(
        r#"<let id="$t" name="t" value="0"/>
        <composition id="$a" name="a" dims="2d"><rect x="0" y="0" w="1" h="1"/></composition>
        <composition id="$h" name="h" dims="2d"><group id="$g" name="g" x="{= $t * 1}" y="0"><rect id="$r" name="r" x="0" y="0" w="5" h="5"/></group></composition>"#,
    );
    let at = BTreeMap::from([(Leaf { node: n["$t"], path: path("let.value") }, Value::Number(10.0))]);
    store.publish(p, at.clone());
    all.publish(p, at);
    let h = n["$h"];
    for s in [&mut store, &mut all] {
        let plan = edit::plan(s, &format!(r#"<edit><constraint in="{h}" id="$p" name="p" kind="pin" value="0"/></edit>"#), None).unwrap_or_else(|f| panic!("{}", f.message()));
        let mut data = BTreeMap::new();
        data.insert("role".to_owned(), Value::Str("a".into()));
        data.insert("anchor".to_owned(), Value::Vec2([0.0, 0.0]));
        data.insert("at".to_owned(), Value::Vec2([15.0, 0.0]));
        let link = Op::Link { rel: Relation { id: s.authored().next_relation_id(), from: plan.aliases["$p"], to: n["$r"], rel: RelKind::Constrains, data } };
        s.apply(tx("pin", plan.ops.into_iter().chain([link]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    }
    let x = |s: &Store| s.tree().leaf(n["$r"], path("transform2d.x")).and_then(|s| s.constant().cloned());
    assert_eq!(x(&all), Some(Value::Number(5.0)), "pinned at 15 in the world, in a group at 10");
    assert_eq!(x(&store), x(&all));
}

/// Council review of EN-8c: an entity's simulation pauses when the only
/// composition picturing it is `on_demand` (free-running.md §2: what is seen
/// only through it is paused). Before, only accumulators directly in the
/// composition were held, so a pictured entity kept stepping.
#[test]
fn an_entity_pictured_only_on_demand_holds_still() {
    let mut store = Store::new();
    let plan = edit::plan(
        &store,
        r#"<edit><definition id="$def" name="d"/>
        <model name="model"><entity id="$e" instance.def="$def"><accumulate id="$acc" initial="0" next="{= $acc + $acc.accumulate.dt * 10}"/></entity></model>
        <composition id="$shown" name="shown" dims="2d" run="on_demand"><rect id="$fig" w="1" h="1"/></composition></edit>"#,
        None,
    )
    .unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("make", plan.ops)).unwrap();
    let n = plan.aliases;
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: n["$shown"], rect: [0., 0., 100., 100.], zoom: 1. });
    let id = store.authored().next_relation_id();
    store.apply(tx("figure", vec![Op::Link { rel: Relation { id, from: n["$fig"], to: n["$e"], rel: RelKind::Figures, data: BTreeMap::new() } }])).unwrap();
    let acc = |s: &Store| s.resolved(n["$acc"], path("accumulate.value"));
    store.advance(0.0);
    let before = acc(&store);
    store.advance(0.1);
    store.advance(0.2);
    assert_eq!(acc(&store), before, "pictured only on demand: paused");
    // Run as usual, the same picture lets it step.
    store.apply(tx("auto", vec![Op::Set { id: n["$shown"], path: path("composition.run"), slot: Slot::Const(Value::Enum("auto".into())) }])).unwrap();
    store.advance(0.3);
    store.advance(0.4);
    assert_ne!(acc(&store), before, "pictured on an open path: running");
}

/// Council review of EN-8c, pull on ask: a shown binding that reads, by id,
/// an item a hidden page's repeat has not made yet (its count moved while
/// the page rested) brings that repeat up to date and reads the item.
/// Before, it read its fallback until the page was shown.
#[test]
fn a_reference_to_an_item_a_hidden_repeat_has_not_made_brings_it_up_to_date() {
    let (mut store, mut all, p, n) = split(
        r#"<let id="$n" name="n" value="2"/><let id="$pick" name="pick" value="0"/>
        <composition id="$a" name="a" dims="2d"><rect id="$v" name="v" x="{= deref($pick, 'transform2d.x', -1)}" y="0" w="1" h="1"/></composition>
        <composition id="$b" name="b" dims="2d"><group id="$g" name="g" repeat.count="{= $n.let.value}"><rect id="$c" name="cell" x="{= $g.repeat.item * 10 + 5}" y="0" w="1" h="1"/></group></composition>"#,
    );
    let third = NodeId::item(n["$g"], "2", n["$c"]).unwrap();
    let count = vec![Op::Set { id: n["$n"], path: path("let.value"), slot: Slot::Const(Value::Number(3.0)) }];
    store.apply(tx("count", count.clone())).unwrap();
    all.apply(tx("count", count)).unwrap();
    let items = |s: &Store| s.tree().children(n["$g"]).len();
    assert_eq!((items(&all), items(&store)), (4, 3), "hidden, b's repeat still holds two items (and its template)");
    let pick = BTreeMap::from([(Leaf { node: n["$pick"], path: path("let.value") }, Value::Ref(third))]);
    store.publish(p, pick.clone());
    all.publish(p, pick);
    let x = |s: &Store| s.resolved(n["$v"], path("transform2d.x"));
    assert_eq!(x(&all), Some(Value::Number(25.0)));
    assert_eq!(x(&store), x(&all), "asked for, the third item is made");
}

/// Council round 2 of EN-8c: a page with a stacked group and a repeat whose
/// count is set while it rests (page `b`; `a` is shown).
fn resting_bench() -> (Store, BTreeMap<String, NodeId>) {
    let mut store = Store::new();
    let plan = edit::plan(
        &store,
        r#"<edit><let id="$n" name="n" value="2"/><let id="$k" name="k" value="1"/>
        <composition id="$a" name="a" dims="2d"/>
        <composition id="$b" name="b" dims="2d"><group id="$g" name="g" repeat.count="{= $n.let.value}"><rect id="$c" name="cell" x="17" y="0" w="1" h="1"/></group>
          <group id="$s" x="0" y="50" layout.mode="stack_x" layout.gap="0"><rect id="$r1" w="{= $k * 10}" h="10"/><rect id="$r2" w="10" h="10"/></group></composition></edit>"#,
        None,
    )
    .unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("bench", plan.ops)).unwrap();
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: plan.aliases["$a"], rect: [0., 0., 100., 100.], zoom: 1. });
    store.advance(0.0);
    (store, plan.aliases)
}

fn set_let(store: &mut Store, n: NodeId, v: f64) {
    store.apply(tx("set", vec![Op::Set { id: n, path: path("let.value"), slot: Slot::Const(Value::Number(v)) }])).unwrap();
}

/// Pull on ask by page: asking about a node on a resting page (the
/// inspector, a host command) brings that page up to date, so an item its
/// repeat should have made exists, one it should have dropped is gone, and
/// what its layout places is current. Before, only the node's own cone was
/// pulled: the repeat and the layout run from the page's resting cells.
#[test]
fn asking_about_a_node_on_a_resting_page_brings_the_page_up_to_date() {
    let (mut store, n) = resting_bench();
    let third = NodeId::item(n["$g"], "2", n["$c"]).unwrap();
    let second = NodeId::item(n["$g"], "1", n["$c"]).unwrap();
    set_let(&mut store, n["$n"], 3.0);
    assert!(!store.tree().is_live(third), "resting, b's repeat has two items");
    store.pull_nodes(&[third]);
    assert!(store.tree().is_live(third), "asked for, the third item is made");
    set_let(&mut store, n["$n"], 1.0);
    store.pull_nodes(&[second]);
    assert!(!store.tree().is_live(second), "asked for, the second item is gone");
    set_let(&mut store, n["$k"], 4.0);
    store.pull_nodes(&[n["$r2"]]);
    assert_eq!(store.resolved(n["$r2"], path("transform2d.x")), Some(Value::Number(40.0)), "the stack places r2 after r1's new width");
}

/// A transaction that moves a resting page's repeat onto a shown page and
/// adds a template child its count (0) never copies reports no item made or
/// gone. Before, the wake ran after the transaction's net view delta
/// closed, so the copy made and dropped showed as both.
#[test]
fn waking_what_a_transaction_moved_stays_inside_its_net_delta() {
    let (mut store, n) = resting_bench();
    set_let(&mut store, n["$n"], 0.0);
    store.take_changes();
    let mut plan = edit::plan(&store, &format!(r#"<edit><rect in="{}" id="$new" x="20" y="0" w="1" h="1"/></edit>"#, n["$g"]), None).unwrap_or_else(|f| panic!("{}", f.message()));
    plan.ops.push(Op::Reparent { id: n["$g"], parent: Some(n["$a"]), order: OrderKey::first() });
    store.apply(tx("move", plan.ops)).unwrap();
    let changes = store.take_changes();
    let item = NodeId::item(n["$g"], "0", plan.aliases["$new"]).unwrap();
    assert!(!store.tree().is_live(item));
    assert!(!changes.created.contains(&item) && !changes.removed.iter().any(|(id, _)| *id == item), "made and dropped in one transaction: neither");
}

/// Demand is decided at the end of every transaction: a page set to run
/// always, or newly embedded in a shown one, is current in the frame of
/// the change, not at the next advance.
#[test]
fn a_page_newly_run_always_or_embedded_is_current_at_once() {
    for embed in [false, true] {
        let (mut store, n) = resting_bench();
        set_let(&mut store, n["$k"], 4.0);
        if embed {
            let plan = edit::plan(&store, &format!(r#"<edit><embed in="{}" source="{}" x="0" y="0" w="100" h="100"/></edit>"#, n["$a"], n["$b"]), None).unwrap_or_else(|f| panic!("{}", f.message()));
            store.apply(tx("embed", plan.ops)).unwrap();
        } else {
            store.apply(tx("always", vec![Op::Set { id: n["$b"], path: path("composition.run"), slot: Slot::Const(Value::Enum("always".into())) }])).unwrap();
        }
        assert_eq!(store.resolved(n["$r1"], path("size.w")), Some(Value::Number(40.0)), "embed {embed}: b is current at once");
    }
}

/// A shown binding that reads an item a resting repeat should have dropped
/// reads its fallback in the frame of the publish that points it there.
#[test]
fn a_reference_to_an_item_a_resting_repeat_dropped_reads_the_fallback_at_once() {
    let (mut store, n) = resting_bench();
    let plan = edit::plan(&store, &format!(r#"<edit><let id="$pick" name="pick" value="0"/><rect in="{}" id="$v" x="{{= deref($pick, 'transform2d.x', -1)}}" w="1" h="1"/></edit>"#, n["$a"]), None).unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("reader", plan.ops)).unwrap();
    set_let(&mut store, n["$n"], 1.0);
    let second = NodeId::item(n["$g"], "1", n["$c"]).unwrap();
    assert!(store.tree().is_live(second), "resting, b's repeat still has the second item");
    let p = store.register_producer("host");
    store.publish(p, BTreeMap::from([(Leaf { node: plan.aliases["$pick"], path: path("let.value") }, Value::Ref(second))]));
    assert_eq!(store.resolved(plan.aliases["$v"], path("transform2d.x")), Some(Value::Number(-1.0)));
}

/// An `on_demand` composition's bindings do not run what they read on
/// time: the entity its figure reads holds still (free-running.md §2).
/// Before, the composition still seeded the clock closure, which reached
/// the entity through the read.
#[test]
fn an_on_demand_figure_reading_an_entity_does_not_run_it() {
    let mut store = Store::new();
    let plan = edit::plan(
        &store,
        r#"<edit><definition id="$def" name="d"/>
        <model name="model"><entity id="$e" instance.def="$def"><accumulate id="$acc" initial="0" next="{= $acc + $acc.accumulate.dt * 10}"/></entity></model>
        <composition id="$shown" name="shown" dims="2d" run="on_demand"><rect id="$fig" x="{= $acc.accumulate.value}" w="1" h="1"/></composition></edit>"#,
        None,
    )
    .unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("make", plan.ops)).unwrap();
    let n = plan.aliases;
    let id = store.authored().next_relation_id();
    store.apply(tx("figure", vec![Op::Link { rel: Relation { id, from: n["$fig"], to: n["$e"], rel: RelKind::Figures, data: BTreeMap::new() } }])).unwrap();
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: n["$shown"], rect: [0., 0., 100., 100.], zoom: 1. });
    let acc = |s: &Store| s.resolved(n["$acc"], path("accumulate.value"));
    store.advance(0.0);
    let before = acc(&store);
    store.advance(0.1);
    store.advance(0.2);
    assert_eq!(acc(&store), before);
}

/// A transaction that names an item of a resting page asks about it (the
/// agent's pattern: set a count on one page, then edit an item on a hidden
/// one): the item exists, or not, as where everything is observed. Before,
/// a set on an item the count had just made failed as a bad id, and one on
/// an item it had just dropped succeeded.
#[test]
fn a_transaction_naming_an_item_of_a_resting_page_sees_its_current_items() {
    let (mut store, n) = resting_bench();
    let x = path("transform2d.x");
    set_let(&mut store, n["$n"], 3.0);
    let third = NodeId::item(n["$g"], "2", n["$c"]).unwrap();
    store.apply(tx("third", vec![Op::Set { id: third, path: x, slot: Slot::Const(Value::Number(30.0)) }])).unwrap_or_else(|e| panic!("the third item exists: {e:?}"));
    set_let(&mut store, n["$n"], 1.0);
    let second = NodeId::item(n["$g"], "1", n["$c"]).unwrap();
    assert!(store.apply(tx("second", vec![Op::Set { id: second, path: x, slot: Slot::Const(Value::Number(30.0)) }])).is_err(), "the second item is gone");
}
