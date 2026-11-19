//! Templates (reactive-engine handoff §4.6, stage 2): each owner's copies
//! are materialised from a template compiled once per pass. These are the
//! parity cases the handoff names: override precedence, an override that
//! changes which template a nested owner gets, a repeat's prototype in a
//! picture carrying that picture's overrides into its items, and
//! references that follow to each owner's own copies. Under the
//! `differential` feature with `IRON_OWNER_CHECK=1`, the code templates
//! replaced also expands the whole document after every update and must
//! find nothing to change.

use iron_document::edit;
use iron_document::facet::{Name, Override};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn apply(store: &mut Store, xml: &str) -> BTreeMap<String, NodeId> {
    let plan = edit::plan(store, &format!("<edit>{xml}</edit>"), None).unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("edit", plan.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    plan.aliases
}

fn number(store: &Store, id: NodeId, leaf: &str) -> f64 {
    store.resolved(id, leaf.parse().unwrap()).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("{id}.{leaf}"))
}

/// The live copies under `under` named `name`, in id order.
fn copies(store: &Store, under: NodeId, name: &str) -> Vec<NodeId> {
    let doc = store.tree();
    doc.descendants(under).into_iter().filter(|&d| d.is_expanded() && doc.is_live(d) && doc.get::<Name>(d).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).collect()
}

/// An override node under `owner`: `target`'s `leaf` set to `value`.
fn override_of(store: &Store, owner: NodeId, target: NodeId, leaf: &str, value: Value) -> (NodeId, Op) {
    let id = store.authored().next_id();
    let doc = store.tree();
    let after = doc.children(owner).iter().filter(|c| !c.is_expanded()).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max();
    let mut o = Override::default();
    o.target = Slot::Const(Value::Ref(target));
    o.leaf = Slot::Const(Value::Str(leaf.into()));
    o.value = Slot::Const(value);
    (id, Op::Create { id, ty: ty("override"), parent: Some(owner), order: after.map_or_else(OrderKey::first, |k| OrderKey::after(&k)), facets: vec![o.wrap()] })
}

/// A scene and two definitions: `cell`, drawn as a 10-wide box in `free`
/// and a dot in `alt`; and `pair`, whose body holds a `cell` and whose
/// template pictures it. Then one `pair` placed in the scene.
struct Pair {
    store: Store,
    names: BTreeMap<String, NodeId>,
    /// The placed `pair`'s picture.
    u: NodeId,
}

fn pair() -> Pair {
    let mut store = Store::new();
    let mut names = apply(
        &mut store,
        r#"<model name="model"/>
        <composition id="$c" name="scene" dims="2d"/>
        <library name="library">
          <definition id="$cell" name="cell">
            <template notation="free"><rect id="$box" name="box" w="10" h="4"/></template>
            <template notation="alt"><ellipse id="$dot" name="dot" w="3" h="3"/></template>
          </definition>
        </library>"#,
    );
    let cell = names["$cell"];
    let library = store.tree().live(cell).unwrap().parent.unwrap();
    names.extend(apply(
        &mut store,
        &format!(
            r#"<definition in="{library}" id="$pair" name="pair">
              <entity id="$ie" name="inner" instance.def="{cell}" instance.mode="linked"/>
              <template notation="free"><use id="$iu" name="iu" notation="free" x="5"/></template>
            </definition>"#
        ),
    ));
    let (iu, ie) = (names["$iu"], names["$ie"]);
    let rel = store.authored().next_relation_id();
    store.apply(tx("figure", vec![Op::Link { rel: Relation { id: rel, from: iu, to: ie, rel: RelKind::Figures, data: BTreeMap::new() } }])).unwrap_or_else(|e| panic!("{e:?}"));
    let c = names["$c"];
    names.extend(apply(&mut store, &format!(r#"<place def="{}" in="{c}" id="$p" x="0" y="0"/>"#, names["$pair"])));
    let doc = store.tree();
    let u = *doc.children(c).iter().find(|&&n| !n.is_expanded() && doc.live(n).is_some_and(|x| x.ty.name() == "use")).expect("the pair's picture");
    Pair { store, names, u }
}

/// The overrides reaching a nested picture's copy apply innermost first:
/// the definition's own opinion, then the placed instance's, which wins.
/// Taking each away restores the one beneath it.
#[test]
fn nested_overrides_apply_innermost_first_and_the_outermost_wins() {
    let Pair { mut store, names, u } = pair();
    let (iu, bx) = (names["$iu"], names["$box"]);
    let copy = || NodeId::from_path(&[u, iu, bx]).unwrap();
    assert_eq!(number(&store, copy(), "size.w"), 10.0);
    // The definition's: an override under its inner picture.
    let (inner, op) = override_of(&store, iu, bx, "size.w", Value::Number(20.0));
    store.apply(tx("definition's", vec![op])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(number(&store, copy(), "size.w"), 20.0);
    // The instance's, through the inner picture to the box.
    let (outer, op) = override_of(&store, u, NodeId::from_path(&[iu, bx]).unwrap(), "size.w", Value::Number(30.0));
    store.apply(tx("instance's", vec![op])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(number(&store, copy(), "size.w"), 30.0);
    // An override may add a facet the copy lacks, where its type admits one.
    let (_, op) = override_of(&store, u, NodeId::from_path(&[iu, bx]).unwrap(), "stroke.width", Value::Number(2.0));
    store.apply(tx("outline", vec![op])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(number(&store, copy(), "stroke.width"), 2.0);
    store.apply(tx("drop the instance's", vec![Op::Delete { id: outer }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(number(&store, copy(), "size.w"), 20.0);
    store.apply(tx("drop the definition's", vec![Op::Delete { id: inner }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(number(&store, copy(), "size.w"), 10.0);
}

/// An override of an inner picture's notation changes which template its
/// copy is made from: the nested owner is found from its effective slots.
#[test]
fn an_override_of_an_inner_pictures_notation_changes_its_copies() {
    let Pair { mut store, names, u } = pair();
    let iu = names["$iu"];
    let inner = NodeId::from_path(&[u, iu]).unwrap();
    assert_eq!((copies(&store, inner, "box").len(), copies(&store, inner, "dot").len()), (1, 0));
    let (o, op) = override_of(&store, u, iu, "use.notation", Value::Str("alt".into()));
    store.apply(tx("alt", vec![op])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!((copies(&store, inner, "box").len(), copies(&store, inner, "dot").len()), (0, 1));
    store.apply(tx("free again", vec![Op::Delete { id: o }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!((copies(&store, inner, "box").len(), copies(&store, inner, "dot").len()), (1, 0));
    store.undo(0).unwrap().unwrap();
    assert_eq!((copies(&store, inner, "box").len(), copies(&store, inner, "dot").len()), (0, 1));
}

/// A repeat in a definition's template is copied into each picture, and
/// the picture's copy is the one that repeats: its items copy the
/// prototype as that picture's overrides left it, and a second picture's
/// items are its own.
#[test]
fn a_repeat_in_a_template_carries_its_pictures_overrides_into_its_items() {
    let mut store = Store::new();
    let names = apply(
        &mut store,
        r#"<model name="model"/>
        <composition id="$c" name="scene" dims="2d"/>
        <library name="library">
          <definition id="$row" name="row">
            <template notation="free">
              <group id="$rep" name="rep" repeat.count="3"><rect id="$cell" name="cell" x="{= $rep.repeat.index * 10}" w="5" h="5"/></group>
            </template>
          </definition>
        </library>"#,
    );
    let (c, row, rep, cell) = (names["$c"], names["$row"], names["$rep"], names["$cell"]);
    apply(&mut store, &format!(r#"<place def="{row}" in="{c}" x="0" y="0"/><place def="{row}" in="{c}" x="0" y="50"/>"#));
    let doc = store.tree();
    let uses: Vec<NodeId> = doc.children(c).iter().copied().filter(|&n| !n.is_expanded() && doc.live(n).is_some_and(|x| x.ty.name() == "use")).collect();
    let (a, b) = (uses[0], uses[1]);
    let cells = |store: &Store, u: NodeId| -> Vec<(f64, f64)> {
        let r = NodeId::from_path(&[u, rep]).unwrap();
        ["0", "1", "2"].iter().map(|k| NodeId::item(r, k, NodeId::from_path(&[u, cell]).unwrap()).unwrap()).map(|n| (number(store, n, "transform2d.x"), number(store, n, "size.w"))).collect()
    };
    assert_eq!(cells(&store, a), vec![(0.0, 5.0), (10.0, 5.0), (20.0, 5.0)]);
    let (_, op) = override_of(&store, a, cell, "size.w", Value::Number(9.0));
    store.apply(tx("wider in a", vec![op])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(cells(&store, a), vec![(0.0, 9.0), (10.0, 9.0), (20.0, 9.0)]);
    assert_eq!(cells(&store, b), vec![(0.0, 5.0), (10.0, 5.0), (20.0, 5.0)]);
    // The definition's edit reaches both; the override still wins in a.
    store.apply(tx("taller", vec![Op::Set { id: cell, path: "size.w".parse().unwrap(), slot: Slot::Const(Value::Number(6.0)) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(cells(&store, a), vec![(0.0, 9.0), (10.0, 9.0), (20.0, 9.0)]);
    assert_eq!(cells(&store, b), vec![(0.0, 6.0), (10.0, 6.0), (20.0, 6.0)]);
}

/// References in a template follow to each owner's own copies: in a
/// binding, as a node named in a formula (`list(…)`), and to a port of the
/// definition's body, which reads the figured entity's port.
#[test]
fn references_follow_to_each_owners_copies() {
    let mut store = Store::new();
    let names = apply(
        &mut store,
        r#"<model name="model"/>
        <composition id="$c" name="scene" dims="2d"/>
        <library name="library">
          <definition id="$d" name="tag">
            <port_spec id="$n" name="n" kind="value" dir="in" signal="2" datatype="number"/>
            <template notation="free">
              <rect id="$a" name="a" w="{= $n.port.signal * 10}" h="4"/>
              <rect id="$b" name="b" x="{= $a.size.w + 1}" w="3" h="3"/>
              <let id="$both" name="both" value="{= list($a, $b)}"/>
            </template>
          </definition>
        </library>"#,
    );
    let (c, d, n, a, b, both) = (names["$c"], names["$d"], names["$n"], names["$a"], names["$b"], names["$both"]);
    apply(&mut store, &format!(r#"<place def="{d}" in="{c}" x="0" y="0" n="3"/><place def="{d}" in="{c}" x="0" y="50" n="5"/>"#));
    let doc = store.tree();
    let uses: Vec<NodeId> = doc.children(c).iter().copied().filter(|&x| !x.is_expanded() && doc.live(x).is_some_and(|y| y.ty.name() == "use")).collect();
    for (&u, k) in uses.iter().zip([3.0, 5.0]) {
        let entity = doc.relations_from(u).find(|r| r.rel == RelKind::Figures).unwrap().to;
        let [ca, cb, cboth] = [a, b, both].map(|x| NodeId::from_path(&[u, x]).unwrap());
        let port = NodeId::from_path(&[entity, n]).unwrap();
        let deps = |id: NodeId, leaf: &str| -> Vec<Leaf> { doc.leaf(id, leaf.parse().unwrap()).and_then(Slot::expr).map(|e| e.deps().into_iter().collect()).unwrap_or_default() };
        assert_eq!(deps(ca, "size.w").iter().map(|l| l.node).collect::<Vec<_>>(), vec![port], "a picture reads its entity's port");
        assert_eq!(deps(cb, "transform2d.x").iter().map(|l| l.node).collect::<Vec<_>>(), vec![ca]);
        let list = doc.leaf(cboth, "let.value".parse().unwrap()).and_then(Slot::expr).unwrap().to_string();
        assert!(list.contains(&ca.to_string()) && list.contains(&cb.to_string()), "{list}");
        assert_eq!(number(&store, cb, "transform2d.x"), k * 10.0 + 1.0);
    }
}
