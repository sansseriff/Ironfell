//! "Ask the node" (`EN-14`, patches.md §5): `Store::inspect` traces a bound,
//! derived or laid-out node in one query.

use iron_document::edit;
use iron_document::facet::{Name, Override};
use iron_document::inspect::{render, Inspection};
use iron_document::*;

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn apply(store: &mut Store, xml: &str) -> std::collections::BTreeMap<String, NodeId> {
    let plan = edit::plan(store, xml, None).unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("edit", plan.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    plan.aliases
}

fn inspect(store: &Store, id: NodeId) -> Inspection {
    store.inspect(id).unwrap_or_else(|| panic!("{id} is live"))
}

fn has(links: &[iron_document::inspect::Link], kind: &str, node: NodeId) -> bool {
    links.iter().any(|l| l.kind == kind && l.node == node.to_string())
}

#[test]
fn a_bound_leaf_names_what_it_reads_and_its_reader_names_it() {
    let mut store = Store::new();
    let a = apply(&mut store, r#"<edit><composition id="$c" name="scene" dims="2d">
        <slider id="$s" name="level" x="10" y="10" w="200" h="24" value="0.4"/>
        <rect id="$r" name="bar" x="{= $s.slider.value * 100}" y="50" w="10" h="10"/>
      </composition></edit>"#);
    let (s, r, c) = (a["$s"], a["$r"], a["$c"]);
    let bar = inspect(&store, r);
    assert_eq!(bar.node.name.as_deref(), Some("bar"));
    assert_eq!(bar.path.first().map(|p| p.id.clone()), Some(c.to_string()), "its page: {:?}", bar.path);
    let x = bar.why.iter().find(|w| w.leaf == "transform2d.x").expect("x is bound");
    // Its formula once, as its binding; its name is not a leaf here.
    assert_eq!(x.layers.iter().map(|l| l.kind).collect::<Vec<_>>(), ["default", "binding"], "{:?}", x.layers);
    assert!(!bar.why.iter().any(|w| w.leaf.starts_with("name.")), "{:?}", bar.why);
    assert_eq!(x.value.as_deref(), Some("40"));
    assert!(bar.driven_by.iter().any(|l| l.kind == "binding" && l.node == s.to_string() && l.detail.as_deref() == Some("slider.value")), "{:?}", bar.driven_by);
    // A leaf at its default is not a line.
    assert!(!bar.why.iter().any(|w| w.leaf == "transform2d.rot"), "{:?}", bar.why);
    let level = inspect(&store, s);
    assert!(level.drives.iter().any(|l| l.kind == "binding" && l.node == r.to_string() && l.leaf.as_deref() == Some("slider.value")), "{:?}", level.drives);
}

#[test]
fn an_instance_part_says_where_it_comes_from_and_what_overrides_it() {
    let mut store = Store::new();
    let a = apply(&mut store, r#"<edit><model name="model"/><composition id="$c" name="bench" dims="2d"/></edit>"#);
    let u = store.place_standard("meter", a["$c"], [0.0, 0.0], Actor::Human).unwrap();
    let doc = store.tree();
    let named = |n: NodeId| doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()).map(str::to_owned);
    let dial = doc.descendants(u).into_iter().find(|&d| d.is_expanded() && named(d).as_deref() == Some("dial")).expect("the meter's dial");
    let (owner, local) = dial.expanded_parts().unwrap();
    let mut o = Override::default();
    o.target = Slot::Const(Value::Ref(local));
    o.leaf = Slot::Const(Value::Str("fill.color".into()));
    o.value = Slot::Const(Value::Color([0.2, 0.2, 0.2, 1.0]));
    let id = doc.next_id();
    store.apply(tx("override", vec![Op::Create { id, ty: TypeId::lookup("override").unwrap(), parent: Some(owner), order: OrderKey::first(), facets: vec![o.wrap()] }])).unwrap();
    let i = inspect(&store, dial);
    assert!(i.derives.first().is_some_and(|d| d.contains(&local.to_string()) && d.contains(&owner.to_string()) && d.contains("an instance of")), "{:?}", i.derives);
    let fill = i.why.iter().find(|w| w.leaf == "fill.color").expect("the overridden fill");
    let kinds: Vec<&str> = fill.layers.iter().map(|l| l.kind).collect();
    assert!(kinds.ends_with(&["template", "override"]), "{kinds:?}");
    assert_eq!(fill.layers.last().and_then(|l| l.from.clone()), Some(id.to_string()), "the override node");
}

/// Relation kinds read as the view writes them (`aligns_to`, not `alignsto`).
#[test]
fn a_relation_reads_by_its_name() {
    let mut store = Store::new();
    let a = apply(&mut store, r#"<edit><composition id="$c" name="b" dims="2d"><rect id="$a" w="10" h="10"/><rect id="$b" x="20" w="10" h="10"/></composition></edit>"#);
    let id = store.authored().next_relation_id();
    store.apply(tx("align", vec![Op::Link { rel: Relation { id, from: a["$a"], to: a["$b"], rel: RelKind::AlignsTo, data: Default::default() } }])).unwrap();
    assert!(has(&inspect(&store, a["$a"]).drives, "aligns_to", a["$b"]));
    assert!(has(&inspect(&store, a["$b"]).driven_by, "aligns_to", a["$a"]));
}

#[test]
fn a_laid_out_child_names_its_layout() {
    let mut store = Store::new();
    let a = apply(&mut store, r#"<edit><composition id="$c" name="b" dims="2d"><group id="$g" x="0" y="0" layout.mode="stack_x" layout.gap="5"><rect id="$r1" w="10" h="10"/><rect id="$r2" w="10" h="10"/></group></composition></edit>"#);
    let i = inspect(&store, a["$r2"]);
    assert!(has(&i.driven_by, "layout", a["$g"]), "{:?}", i.driven_by);
    let x = i.why.iter().find(|w| w.leaf == "transform2d.x").expect("placed by the layout");
    assert_eq!(x.layers.last().map(|l| (l.kind, l.from.clone())), Some(("producer", Some("layout".into()))), "{:?}", x.layers);
    assert_eq!(x.value.as_deref(), Some("15"));
    // The first child's size is what places the second: the layout reads it.
    let first = inspect(&store, a["$r1"]);
    assert!(has(&first.drives, "layout", a["$g"]), "{:?}", first.drives);
}

#[test]
fn the_agent_reads_connections_after_a_view() {
    let mut store = Store::new();
    let a = apply(&mut store, r#"<edit><composition id="$c" name="scene" dims="2d">
        <slider id="$s" name="level" x="10" y="10" w="200" h="24" value="0.4"/>
        <rect id="$r" name="bar" x="{= $s.slider.value * 100}" y="50" w="10" h="10"/>
      </composition></edit>"#);
    let text = render(&inspect(&store, a["$r"]));
    assert!(text.starts_with(&format!("<connections id=\"{}\" type=\"rect\" name=\"bar\">", a["$r"])), "{text}");
    assert!(text.contains("<why leaf=\"transform2d.x\" value=\"40\">"), "{text}");
    assert!(text.contains(&format!("<driven-by kind=\"binding\" node=\"{}\" leaf=\"transform2d.x\" detail=\"slider.value\"/>", a["$s"])), "{text}");
    // Through the view: `connections` appends it.
    let q = ViewQuery { omit_defaults: true, expand: Vec::new(), relation: Default::default(), scope: Some(a["$r"]), fidelity: Fidelity::Full, depth: Some(0), connections: vec![a["$r"]] };
    let v = store.view(&q).unwrap();
    assert!(v.contains(&text), "{v}");
}
