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
use iron_document::facet::{self, Name, Override};
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

/// A repeat in a repeat in a picture: when the instance's input changes
/// how many items the inner repeat has, only those items are made again,
/// from the definition's template (not the picture's copies), with the
/// picture's override and each level's index.
#[test]
fn items_of_a_repeat_in_a_repeat_in_a_picture_come_from_the_definition() {
    let mut store = Store::new();
    let names = apply(
        &mut store,
        r#"<model name="model"/>
        <composition id="$c" name="scene" dims="2d"/>
        <library name="library">
          <definition id="$d" name="grid">
            <port_spec id="$n" name="n" kind="value" dir="in" signal="2" datatype="number"/>
            <template notation="free">
              <group id="$rows" name="rows" repeat.count="2">
                <group id="$cols" name="cols" repeat.count="{= $n.port.signal}">
                  <rect id="$cell" name="cell" x="{= $cols.repeat.index * 10}" y="{= $rows.repeat.index * 10}" w="5" h="5"/>
                </group>
              </group>
            </template>
          </definition>
        </library>"#,
    );
    let (c, d, rows, cols, cell) = (names["$c"], names["$d"], names["$rows"], names["$cols"], names["$cell"]);
    apply(&mut store, &format!(r#"<place def="{d}" in="{c}" x="0" y="0" n="2"/>"#));
    let doc = store.tree();
    let u = *doc.children(c).iter().find(|&&x| !x.is_expanded() && doc.live(x).is_some_and(|y| y.ty.name() == "use")).unwrap();
    let entity = doc.relations_from(u).find(|r| r.rel == RelKind::Figures).unwrap().to;
    let (_, op) = override_of(&store, u, cell, "size.w", Value::Number(9.0));
    store.apply(tx("wider", vec![op])).unwrap_or_else(|e| panic!("{e:?}"));
    let cells = |store: &Store| -> Vec<(f64, f64, f64)> {
        let outer = NodeId::from_path(&[u, rows]).unwrap();
        let mut out = Vec::new();
        for k in ["0", "1"] {
            let inner = NodeId::item(outer, k, cols).unwrap();
            for j in ["0", "1", "2", "3"] {
                let n = NodeId::item(inner, j, cell).unwrap();
                if store.tree().is_live(n) {
                    out.push((number(store, n, "transform2d.x"), number(store, n, "transform2d.y"), number(store, n, "size.w")));
                }
            }
        }
        out
    };
    assert_eq!(cells(&store), vec![(0.0, 0.0, 9.0), (10.0, 0.0, 9.0), (0.0, 10.0, 9.0), (10.0, 10.0, 9.0)]);
    store.apply(tx("three", vec![Op::Set { id: entity, path: "args.n".parse().unwrap(), slot: Slot::Const(Value::Number(3.0)) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(cells(&store), vec![(0.0, 0.0, 9.0), (10.0, 0.0, 9.0), (20.0, 0.0, 9.0), (0.0, 10.0, 9.0), (10.0, 10.0, 9.0), (20.0, 10.0, 9.0)]);
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

/// One picture template serves a sized and an unsized `use` in the same
/// pass: in the sized one, a read of the template's own `size` is the
/// `use`'s; in the other it stays on the template.
#[test]
fn a_sized_and_an_unsized_picture_share_a_template() {
    let mut store = Store::new();
    let names = apply(
        &mut store,
        r#"<model name="model"/>
        <composition id="$c" name="scene" dims="2d"/>
        <library name="library">
          <definition id="$d" name="bar">
            <template id="$t" notation="free" w="100" h="40"><rect id="$half" name="half" w="{= $t.size.w / 2}" h="4"/></template>
          </definition>
        </library>"#,
    );
    let (c, d, t, half) = (names["$c"], names["$d"], names["$t"], names["$half"]);
    apply(&mut store, &format!(r#"<place def="{d}" in="{c}" x="0" y="0"/><place def="{d}" in="{c}" x="0" y="50" w="300" h="40"/>"#));
    let doc = store.tree();
    let uses: Vec<NodeId> = doc.children(c).iter().copied().filter(|&x| !x.is_expanded() && doc.live(x).is_some_and(|y| y.ty.name() == "use")).collect();
    // `place` sizes every picture; the first is made unsized.
    store.apply(tx("unsized", vec![Op::RemoveFacet { id: uses[0], kind: FacetKind::Size }])).unwrap_or_else(|e| panic!("{e:?}"));
    let widths = |store: &Store| -> Vec<(bool, f64)> { uses.iter().map(|&u| (store.tree().facet(u, FacetKind::Size).is_some(), number(store, NodeId::from_path(&[u, half]).unwrap(), "size.w"))).collect() };
    assert_eq!(widths(&store), vec![(false, 50.0), (true, 150.0)]);
    // A definition edit expands both again, from one template.
    store.apply(tx("narrower", vec![Op::Set { id: t, path: "size.w".parse().unwrap(), slot: Slot::Const(Value::Number(80.0)) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(widths(&store), vec![(false, 40.0), (true, 150.0)]);
}

/// A main pictures its definition with no entity: a read of the
/// definition's body stays on the definition's own node.
#[test]
fn a_mains_reads_of_the_body_stay_on_the_definition() {
    let mut store = Store::new();
    let names = apply(
        &mut store,
        r#"<model name="model"/>
        <composition id="$c" name="scene" dims="2d"/>
        <library name="library">
          <definition id="$d" name="tag">
            <port_spec id="$n" name="n" kind="value" dir="in" signal="2" datatype="number"/>
            <template notation="free"><rect id="$a" name="a" w="{= $n.port.signal * 10}" h="4"/></template>
          </definition>
        </library>"#,
    );
    let (c, d, n, a) = (names["$c"], names["$d"], names["$n"], names["$a"]);
    let u = store.authored().next_id();
    let mut usef = facet::Use::default();
    usef.notation = Slot::Const(Value::Str("free".into()));
    let mut main = facet::Instance::default();
    main.def = Slot::Const(Value::Ref(d));
    main.mode = Slot::Const(Value::Str("main".into()));
    store.apply(tx("main", vec![Op::Create { id: u, ty: ty("use"), parent: Some(c), order: OrderKey::first(), facets: vec![usef.wrap(), main.wrap(), facet::Transform2d::default().wrap()] }])).unwrap_or_else(|e| panic!("{e:?}"));
    let copy = NodeId::from_path(&[u, a]).unwrap();
    let deps: Vec<NodeId> = store.tree().leaf(copy, "size.w".parse().unwrap()).and_then(Slot::expr).unwrap().deps().into_iter().map(|l| l.node).collect();
    assert_eq!(deps, vec![n]);
    assert_eq!(number(&store, copy, "size.w"), 20.0);
}

/// A session node in a picture template gets no copy, and nothing under it
/// does: no copy is wanted under a parent that has none, so no problem is
/// reported (the council, round 1).
#[test]
fn nothing_under_a_session_node_in_a_template_is_copied() {
    let mut store = Store::new();
    let names = apply(
        &mut store,
        r#"<model name="model"/>
        <composition id="$c" name="scene" dims="2d"/>
        <library name="library">
          <definition id="$d" name="box">
            <template id="$t" notation="free"><rect name="frame" w="10" h="10"/><group id="$g" name="g"><rect id="$inner" name="inner" w="2" h="2"/></group></template>
          </definition>
        </library>"#,
    );
    let (c, d, t, g, inner) = (names["$c"], names["$d"], names["$t"], names["$g"], names["$inner"]);
    let s = store.tree().next_session_ids(1)[0];
    store.apply_session(vec![Op::Create { id: s, ty: ty("group"), parent: None, order: OrderKey::first(), facets: vec![] }]).unwrap();
    store.apply_session(vec![Op::Reparent { id: s, parent: Some(t), order: OrderKey::first() }]).unwrap_or_else(|e| panic!("{e:?}"));
    store.apply(tx("into the session group", vec![Op::Reparent { id: g, parent: Some(s), order: OrderKey::first() }])).unwrap_or_else(|e| panic!("{e:?}"));
    apply(&mut store, &format!(r#"<place def="{d}" in="{c}" x="0" y="0"/>"#));
    let doc = store.tree();
    let u = *doc.children(c).iter().find(|&&x| !x.is_expanded() && doc.live(x).is_some_and(|y| y.ty.name() == "use")).unwrap();
    assert_eq!(copies(&store, u, "frame").len(), 1);
    assert!(NodeId::from_path(&[u, g]).is_none_or(|x| !doc.is_live(x)) && NodeId::from_path(&[u, inner]).is_none_or(|x| !doc.is_live(x)));
    let problems: Vec<String> = store.problems().into_iter().map(|p| format!("{}: {}", p.node, p.message)).collect();
    assert!(problems.is_empty(), "{problems:#?}");
}

/// A definition edit reaches every copy through the diff: a facet removed
/// from a template node leaves each copy, and a changed field changes in
/// each, with the rest of the copy as it was.
#[test]
fn a_definition_edit_removes_a_facet_and_changes_a_field_in_every_copy() {
    let mut store = Store::new();
    let names = apply(
        &mut store,
        r#"<model name="model"/>
        <composition id="$c" name="scene" dims="2d"/>
        <library name="library">
          <definition id="$d" name="box">
            <template notation="free"><rect id="$r" name="r" w="10" h="5" stroke.width="2"/></template>
          </definition>
        </library>"#,
    );
    let (c, d, r) = (names["$c"], names["$d"], names["$r"]);
    apply(&mut store, &format!(r#"<place def="{d}" in="{c}" x="0" y="0"/><place def="{d}" in="{c}" x="0" y="50"/>"#));
    let doc = store.tree();
    let rects: Vec<NodeId> = doc.children(c).iter().copied().filter(|&x| !x.is_expanded() && doc.live(x).is_some_and(|y| y.ty.name() == "use")).map(|u| NodeId::from_path(&[u, r]).unwrap()).collect();
    assert!(rects.iter().all(|&x| store.tree().facet(x, FacetKind::Stroke).is_some()));
    store.take_changes();
    store
        .apply(tx("edit", vec![Op::RemoveFacet { id: r, kind: FacetKind::Stroke }, Op::Set { id: r, path: "size.h".parse().unwrap(), slot: Slot::Const(Value::Number(7.0)) }]))
        .unwrap_or_else(|e| panic!("{e:?}"));
    for &x in &rects {
        assert!(store.tree().facet(x, FacetKind::Stroke).is_none(), "{x} kept its stroke");
        assert_eq!((number(&store, x, "size.w"), number(&store, x, "size.h")), (10.0, 7.0));
    }
    let changes = store.take_changes();
    assert!(changes.created.is_empty() && changes.removed.is_empty(), "{changes:?}");
}

/// Templates are kept between passes. A transaction refused after its
/// expansion ran compiled a template from what it would have made; once
/// it is rolled back, the next owner to expand reads the definition as it
/// is, not that template.
#[test]
fn a_refused_definition_edit_leaves_no_template_behind() {
    let Pair { mut store, names, u } = pair();
    let (c, iu, bx) = (names["$c"], names["$iu"], names["$box"]);
    let copy = NodeId::from_path(&[u, iu, bx]).unwrap();
    assert_eq!(number(&store, copy, "size.w"), 10.0);
    // A link to a copy that does not exist is refused after expansion.
    let rel = store.authored().next_relation_id();
    let bogus = NodeId::expanded(u, c).unwrap();
    let refused = store.apply(tx(
        "wider, refused",
        vec![
            Op::Set { id: bx, path: "size.w".parse().unwrap(), slot: Slot::Const(Value::Number(99.0)) },
            Op::Link { rel: Relation { id: rel, from: c, to: bogus, rel: RelKind::References, data: BTreeMap::new() } },
        ],
    ));
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(number(&store, copy, "size.w"), 10.0);
    // The picture expands again for something else.
    let (_, op) = override_of(&store, u, iu, "transform2d.x", Value::Number(7.0));
    store.apply(tx("moved", vec![op])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(number(&store, NodeId::from_path(&[u, iu]).unwrap(), "transform2d.x"), 7.0);
    assert_eq!(number(&store, copy, "size.w"), 10.0);
}

/// A definition edited while nothing instantiates it, then placed: its
/// copies come from the definition as edited.
#[test]
fn a_definition_edited_without_instances_is_placed_as_edited() {
    let Pair { mut store, names, u } = pair();
    let (c, pair_def, iu, bx) = (names["$c"], names["$pair"], names["$iu"], names["$box"]);
    let doc = store.tree();
    let entity = doc.relations_from(u).find(|r| r.rel == RelKind::Figures).unwrap().to;
    store.apply(tx("remove", vec![Op::Delete { id: u }, Op::Delete { id: entity }])).unwrap_or_else(|e| panic!("{e:?}"));
    store.apply(tx("wider", vec![Op::Set { id: bx, path: "size.w".parse().unwrap(), slot: Slot::Const(Value::Number(20.0)) }])).unwrap_or_else(|e| panic!("{e:?}"));
    apply(&mut store, &format!(r#"<place def="{pair_def}" in="{c}" x="0" y="0"/>"#));
    let doc = store.tree();
    let u = *doc.children(c).iter().find(|&&n| !n.is_expanded() && doc.is_live(n) && doc.live(n).is_some_and(|x| x.ty.name() == "use")).expect("the new picture");
    assert_eq!(number(&store, NodeId::from_path(&[u, iu, bx]).unwrap(), "size.w"), 20.0);
}

/// A template's inner `use` may figure an entity outside its definition:
/// that entity's definition is read when the picture expands, not kept
/// with the template, so changing it redraws the picture.
#[test]
fn an_inner_pictures_definition_is_read_when_it_expands() {
    let Pair { mut store, names, u } = pair();
    let (c, cell, ie, iu, bx) = (names["$c"], names["$cell"], names["$ie"], names["$iu"], names["$box"]);
    let lib = store.tree().live(cell).unwrap().parent.unwrap();
    let other = apply(&mut store, &format!(r#"<definition in="{lib}" id="$other" name="other"><template notation="free"><rect id="$otherbox" name="otherbox" w="20" h="4"/></template></definition>"#));
    let model = *store.tree().roots().iter().find(|&&n| store.tree().live(n).unwrap().ty.name() == "model").unwrap();
    let ext = apply(&mut store, &format!(r#"<entity in="{model}" id="$ext" name="external" instance.def="{cell}" instance.mode="linked"/>"#))["$ext"];
    let old = store.tree().relations_from(iu).find(|r| r.rel == RelKind::Figures).unwrap().id;
    let new = store.authored().next_relation_id();
    assert_ne!(ext, ie);
    assert_ne!(ext, c);
    store.apply(tx("figure external", vec![Op::Unlink { id: old }, Op::Link { rel: Relation { id: new, from: iu, to: ext, rel: RelKind::Figures, data: BTreeMap::new() } }])).unwrap();
    let old_box = NodeId::from_path(&[u, iu, bx]).unwrap();
    let new_box = NodeId::from_path(&[u, iu, other["$otherbox"]]).unwrap();
    assert_eq!(number(&store, old_box, "size.w"), 10.0);
    store.apply(tx("change external's definition", vec![Op::Set { id: ext, path: "instance.def".parse().unwrap(), slot: Slot::Const(Value::Ref(other["$other"])) }])).unwrap();
    assert!(store.tree().is_live(new_box));
    assert!(!store.tree().is_live(old_box));
}

/// A `use` of one of the definition's own entities inside a repeat in the
/// template: each item's copy of the `use` derives that entity's picture
/// under itself, and nothing else does (the identity contract: the old
/// code also copied the picture's prototype into each item).
#[test]
fn an_own_inner_picture_in_a_repeat_is_derived_once_per_item() {
    let Pair { mut store, names, u } = pair();
    let (iu, bx) = (names["$iu"], names["$box"]);
    let template = store.tree().live(iu).unwrap().parent.unwrap();
    let r = apply(&mut store, &format!(r#"<group in="{template}" id="$r" name="repeat" repeat.count="1"/>"#))["$r"];
    store.apply(tx("repeat the inner picture", vec![Op::Reparent { id: iu, parent: Some(r), order: OrderKey::first() }])).unwrap();
    let copied_repeat = NodeId::expanded(u, r).unwrap();
    let copied_pic = NodeId::item(copied_repeat, "0", iu).unwrap();
    let expected = NodeId::expanded(copied_pic, bx).unwrap();
    assert!(store.tree().is_live(expected));
    let flattened = NodeId::item(copied_repeat, "0", NodeId::from_path(&[u, iu, bx]).unwrap()).unwrap();
    assert!(!store.tree().is_live(flattened), "the prototype's picture is not copied into the item");
}

/// A picture's override that sets a node of a repeat's prototype, as the
/// picture copies it, names each item's own copy of that node.
#[test]
fn an_outer_override_naming_a_prototype_node_names_the_items_copy() {
    let mut store = Store::new();
    let names = apply(&mut store, r#"<model name="model"/><composition id="$c" name="scene" dims="2d"/><library name="library"><definition id="$d" name="ref"><template notation="free"><group id="$r" repeat.count="1"><rect id="$cell" w="5" h="5"/><let id="$saved" name="saved" value="0"/></group></template></definition></library>"#);
    let (c, d, r, cell, saved) = (names["$c"], names["$d"], names["$r"], names["$cell"], names["$saved"]);
    apply(&mut store, &format!(r#"<place def="{d}" in="{c}" x="0" y="0"/>"#));
    let u = *store.tree().children(c).iter().find(|&&n| !n.is_expanded() && store.tree().live(n).is_some_and(|x| x.ty.name() == "use")).unwrap();
    let prototype = NodeId::expanded(u, cell).unwrap();
    let (_, op) = override_of(&store, u, saved, "let.value", Value::Ref(prototype));
    store.apply(tx("store sibling ref", vec![op])).unwrap();
    let copied_repeat = NodeId::expanded(u, r).unwrap();
    let item_saved = NodeId::item(copied_repeat, "0", saved).unwrap();
    let item_cell = NodeId::item(copied_repeat, "0", cell).unwrap();
    assert_eq!(store.tree().leaf(item_saved, "let.value".parse().unwrap()).and_then(Slot::constant).and_then(Value::as_ref), Some(item_cell));
}

/// An inner picture's `use` that also carries a `main` instance: its
/// picture is the inner entity's, as whole expansion draws it, when only
/// its repeat's items are made again.
#[test]
fn a_repeat_in_an_inner_picture_settles_as_its_owner_expands_it() {
    let Pair { mut store, names, u } = pair();
    let (cell, iu, bx) = (names["$cell"], names["$iu"], names["$box"]);
    let lib = store.tree().live(cell).unwrap().parent.unwrap();
    let other = apply(&mut store, &format!(r#"<definition in="{lib}" id="$other" name="other"><template notation="free"><rect name="otherbox" w="20" h="4"/></template></definition>"#))["$other"];
    let mut main = facet::Instance::default();
    main.def = Slot::Const(Value::Ref(other));
    main.mode = Slot::Const(Value::Str("main".into()));
    store.apply(tx("declare the inner use main as well", vec![Op::AddFacet { id: iu, facet: main.wrap() }])).unwrap();
    let template = store.tree().live(bx).unwrap().parent.unwrap();
    let r = apply(&mut store, &format!(r#"<group in="{template}" id="$r" repeat.count="1"><rect id="$part" name="part" w="5" h="4"/></group>"#));
    let dr = NodeId::from_path(&[u, iu, r["$r"]]).unwrap();
    assert!(store.tree().is_live(NodeId::item(dr, "0", r["$part"]).unwrap()));
}

/// A definition outside a library: its template's repeat is drawn in place
/// and copied into a picture, from two templates that are not confused.
#[test]
fn a_repeat_drawn_in_place_and_copied_into_a_picture_keeps_two_templates() {
    let mut store = Store::new();
    let names = apply(&mut store, r#"<model name="model"/><composition id="$c" name="scene" dims="2d"/><definition id="$d" name="def"><template notation="free"><group id="$r" repeat.count="1"><rect id="$a" w="5" h="4"/><rect id="$b" w="6" h="4"/></group></template></definition>"#);
    let (r, a, b) = (names["$r"], names["$a"], names["$b"]);
    let id = store.authored().next_relation_id();
    store.apply(tx("link the repeat prototype's children", vec![Op::Link { rel: Relation { id, from: a, to: b, rel: RelKind::References, data: BTreeMap::new() } }])).unwrap();
    let c = names["$c"];
    apply(&mut store, &format!(r#"<place def="{}" in="{c}" x="0" y="0"/>"#, names["$d"]));
    let a0 = NodeId::item(r, "0", a).unwrap();
    assert_eq!(store.tree().relations_from(a0).count(), 1);
    store.preview(r, "repeat.count".parse().unwrap(), Value::Number(2.0));
    let a1 = NodeId::item(r, "1", a).unwrap();
    assert_eq!(store.tree().relations_from(a1).count(), 1);
}

/// A session node moved into a drawn repeat is no part of its template:
/// it gets no copy and does not order the items.
#[test]
fn a_session_node_in_a_repeat_does_not_order_its_items() {
    let mut store = Store::new();
    let names = apply(&mut store, r#"<composition dims="2d"><group id="$r" repeat.count="1"><rect w="5" h="4"/></group></composition>"#);
    let r = names["$r"];
    let s = store.tree().next_session_ids(1)[0];
    let a0 = NodeId::item_scope(r, "0").unwrap();
    let late = OrderKey::after(&OrderKey::after(&OrderKey::after(&store.tree().live(a0).unwrap().order)));
    store.apply_session(vec![Op::Create { id: s, ty: ty("group"), parent: None, order: late.clone(), facets: vec![] },
        Op::Reparent { id: s, parent: Some(r), order: late.clone() }]).unwrap();
    store.preview(r, "repeat.count".parse().unwrap(), Value::Number(2.0));
    let a1 = NodeId::item_scope(r, "1").unwrap();
    for item in [a0, a1] {
        assert!(store.tree().live(item).unwrap().order < late, "the items order after the template's copied children only");
    }
}

/// An override of an inner entity's definition: when only an inner
/// picture's repeat's items are made again, they come from the picture
/// whole expansion draws.
#[test]
fn an_override_of_an_inner_entitys_definition_settles_as_it_expands() {
    let Pair { mut store, names, u } = pair();
    let (iu, ie, bx) = (names["$iu"], names["$ie"], names["$box"]);
    let cell = names["$cell"];
    let lib = store.tree().live(cell).unwrap().parent.unwrap();
    let other = apply(&mut store, &format!(r#"<definition in="{lib}" id="$other" name="other"><template notation="free"><rect name="otherbox" w="20" h="4"/></template></definition>"#))["$other"];
    let outer = store.tree().relations_from(u).find(|rel| rel.rel == RelKind::Figures).unwrap().to;
    let (_, op) = override_of(&store, outer, ie, "instance.def", Value::Ref(other));
    store.apply(tx("override inner entity's definition", vec![op])).unwrap();
    let template = store.tree().live(bx).unwrap().parent.unwrap();
    let r = apply(&mut store, &format!(r#"<group in="{template}" id="$r" repeat.count="1"><rect id="$part" name="part" w="5" h="4"/></group>"#));
    let dr = NodeId::from_path(&[u, iu, r["$r"]]).unwrap();
    assert!(store.tree().is_live(NodeId::item(dr, "0", r["$part"]).unwrap()));
}

/// A deleted body node restored outside its definition: the picture that
/// read it as the definition's body reads it as it is.
#[test]
fn a_body_node_restored_elsewhere_is_no_longer_the_bodys() {
    let mut store = Store::new();
    let names = apply(&mut store, r#"<model name="model"/><composition id="$c" name="scene" dims="2d"/><library name="library"><definition id="$d" name="ref"><let id="$body" name="body" value="7"/><template notation="free"><let id="$reader" name="reader" value="0"/></template></definition></library>"#);
    let (c, d, body, reader) = (names["$c"], names["$d"], names["$body"], names["$reader"]);
    store.apply(tx("hold body ref", vec![Op::Set { id: reader, path: "let.value".parse().unwrap(), slot: Slot::Const(Value::Ref(body)) }])).unwrap();
    apply(&mut store, &format!(r#"<place def="{d}" in="{c}" x="0" y="0"/>"#));
    let u = *store.tree().children(c).iter().find(|&&n| !n.is_expanded() && store.tree().live(n).is_some_and(|x| x.ty.name() == "use")).unwrap();
    let copy = NodeId::expanded(u, reader).unwrap();
    let facets = store.tree().facets_of(body).cloned().collect();
    store.apply(tx("delete body let", vec![Op::Delete { id: body }])).unwrap();
    store.apply(tx("restore that id outside the definition", vec![
        Op::Create { id: body, ty: ty("let"), parent: None, order: OrderKey::first(), facets },
        Op::Reparent { id: c, parent: None, order: OrderKey::first() },
    ])).unwrap();
    assert_eq!(store.tree().leaf(copy, "let.value".parse().unwrap()).and_then(Slot::constant).and_then(Value::as_ref), Some(body));
}

/// A node in a pictured repeat holding the picture's copy of another
/// node there holds the item's copy, as the old code had it (it copied the
/// item from the picture's copies).
#[test]
fn a_repeat_node_naming_the_pictures_copy_of_another_names_the_items() {
    let mut store = Store::new();
    let names = apply(&mut store, r#"<model name="model"/><composition id="$c" dims="2d"/><library name="library"><definition id="$d" name="ref"><template notation="free"><group id="$r" repeat.count="1"><rect id="$part" w="10" h="4"/><let id="$read" name="reader" value="0"/></group></template></definition></library>"#);
    let (c,d,r,part,reader) = (names["$c"], names["$d"], names["$r"], names["$part"], names["$read"]);
    apply(&mut store, &format!(r#"<place def="{d}" in="{c}" x="0" y="0"/>"#));
    let u = *store.tree().children(c).iter().find(|&&x| store.tree().live(x).unwrap().ty.name() == "use").unwrap();
    let outer_part = NodeId::expanded(u, part).unwrap();
    let repeat = NodeId::expanded(u, r).unwrap();
    let item_part = NodeId::item(repeat, "0", part).unwrap();
    let item_reader = NodeId::item(repeat, "0", reader).unwrap();
    let value = "let.value".parse().unwrap();
    store.apply(tx("hold reference to picture prototype", vec![Op::Set { id: reader, path: value, slot: Slot::Const(Value::Ref(outer_part)) }])).unwrap();
    assert_eq!(store.tree().leaf(item_reader, value).unwrap().constant().unwrap().as_ref(), Some(item_part));
}

/// A binding in a repeat to a part of an inner picture the repeat holds
/// reads that part of the item's copy of the picture.
#[test]
fn a_binding_into_an_inner_picture_in_a_repeat_reads_the_items_copy() {
    let Pair { mut store, names, u } = pair();
    let (iu, bx) = (names["$iu"], names["$box"]);
    let template = store.tree().live(iu).unwrap().parent.unwrap();
    let probe = apply(&mut store, &format!(r#"<group in="{template}" id="$r" repeat.count="1"><rect id="$reader" w="1" h="1"/></group>"#));
    let (r, reader) = (probe["$r"], probe["$reader"]);
    store.apply(tx("repeat the inner picture", vec![Op::Reparent { id: iu, parent: Some(r), order: OrderKey::first() }])).unwrap();
    let w = "size.w".parse().unwrap();
    let authored_picture_box = NodeId::expanded(iu, bx).unwrap();
    store.apply(tx("bind repeat rect to the inner picture box", vec![Op::Set { id: reader, path: w, slot: Slot::Bound(Expr::Ref(Leaf { node: authored_picture_box, path: w })) }])).unwrap();
    let repeat = NodeId::expanded(u, r).unwrap();
    let prototype_box = NodeId::from_path(&[u, iu, bx]).unwrap();
    let item_scope = NodeId::item_scope(repeat, "0").unwrap();
    let item_pic = NodeId::item(repeat, "0", iu).unwrap();
    let item_box = NodeId::expanded(item_pic, bx).unwrap();
    let item_reader = NodeId::item(repeat, "0", reader).unwrap();
    let target = NodeId::from_path(&[*item_scope.path().last().unwrap(), iu, bx]).unwrap();
    let (_, op) = override_of(&store, r, target, "size.w", Value::Number(29.0));
    store.apply(tx("make the first item's inner box wider", vec![op])).unwrap();
    assert_eq!(number(&store, prototype_box, "size.w"), 10.0);
    assert_eq!(number(&store, item_box, "size.w"), 29.0);
    assert_eq!(number(&store, item_reader, "size.w"), 29.0);
}

/// A session template is a definition's notation while it is in the
/// definition; moving it out (a session op) is a write to the definition.
#[test]
fn a_session_template_moved_out_of_a_definition_is_no_longer_drawn() {
    let mut store = Store::new();
    let names = apply(&mut store, r#"<model/><composition id="$c" dims="2d"/><library name="library"><definition id="$d" name="ref"><template notation="alt"><rect id="$a" w="10" h="4"/></template></definition></library>"#);
    let (c,d,a) = (names["$c"], names["$d"], names["$a"]);
    let s = store.tree().next_session_ids(1)[0];
    let mut template = facet::Template::default();
    template.notation = Slot::Const(Value::Str("free".into()));
    store.apply_session(vec![
        Op::Create { id: s, ty: ty("template"), parent: None, order: OrderKey::first(), facets: vec![template.wrap()] },
        Op::Reparent { id: s, parent: Some(d), order: OrderKey::first() },
    ]).unwrap();
    store.apply(tx("put authored rect in session template", vec![Op::Reparent { id: a, parent: Some(s), order: OrderKey::first() }])).unwrap();
    apply(&mut store, &format!(r#"<place def="{d}" in="{c}" x="0" y="0"/>"#));
    let u = *store.tree().children(c).iter().find(|&&x| store.tree().live(x).unwrap().ty.name() == "use").unwrap();
    assert!(store.tree().is_live(NodeId::expanded(u, a).unwrap()));
    store.apply_session(vec![Op::Reparent { id: s, parent: None, order: OrderKey::first() }]).unwrap();
    assert_eq!(iron_document::definition::template_of(store.tree(), d, "free"), None);
    store.apply(tx("force picture expansion", vec![Op::Set { id: u, path: "use.notation".parse().unwrap(), slot: Slot::Const(Value::Str("free".into())) }])).unwrap();
    assert!(!store.tree().is_live(NodeId::expanded(u, a).unwrap()));
}

/// A session group in a picture's template: moving it out, a session op,
/// takes the authored nodes under it out of the template.
#[test]
fn a_session_group_moved_out_of_a_template_takes_its_nodes_out() {
    let mut store = Store::new();
    let names = apply(&mut store, r#"<model name="model"/><composition id="$c" name="scene" dims="2d"/><library name="library"><definition id="$d" name="ref"><template notation="free"><let id="$reader" name="reader" value="0"/></template></definition></library>"#);
    let (c, d, reader) = (names["$c"], names["$d"], names["$reader"]);
    let t = store.tree().live(reader).unwrap().parent.unwrap();
    let s = store.tree().next_session_ids(1)[0];
    let late = OrderKey::after(&store.tree().live(reader).unwrap().order);
    store.apply_session(vec![Op::Create { id: s, ty: ty("group"), parent: None, order: late.clone(), facets: vec![] },
        Op::Reparent { id: s, parent: Some(t), order: late.clone() }]).unwrap();
    let x = apply(&mut store, &format!(r#"<let in="{s}" id="$x" name="x" value="1"/>"#))["$x"];
    store.apply(tx("reader holds x", vec![Op::Set { id: reader, path: "let.value".parse().unwrap(), slot: Slot::Const(Value::Ref(x)) }])).unwrap();
    apply(&mut store, &format!(r#"<place def="{d}" in="{c}" x="0" y="0"/>"#));
    let u = *store.tree().children(c).iter().find(|&&n| !n.is_expanded() && store.tree().live(n).is_some_and(|x| x.ty.name() == "use")).unwrap();
    let copy = NodeId::expanded(u, reader).unwrap();
    assert_eq!(store.tree().leaf(copy, "let.value".parse().unwrap()).and_then(Slot::constant).and_then(Value::as_ref), NodeId::expanded(u, x));
    // Move the session group out of the template: a session op.
    store.apply_session(vec![Op::Reparent { id: s, parent: None, order: OrderKey::first() }]).unwrap();
    // Anything that re-expands u, and runs the owner check.
    let (_, op) = override_of(&store, u, reader, "name.text", Value::Str("renamed".into()));
    store.apply(tx("re-expand u", vec![op])).unwrap();
    let after = store.tree().leaf(copy, "let.value".parse().unwrap()).and_then(Slot::constant).and_then(Value::as_ref);
    assert_eq!(after, Some(x), "the oracle leaves a reference to a node outside the template as it is");
}

/// A node in an authored repeat holding a part of a picture the repeat
/// holds holds that part of the item's copy of the picture.
#[test]
fn a_reference_into_a_picture_in_an_authored_repeat_names_the_items_copy() {
    let Pair { mut store, names, u } = pair();
    let (c, iu, bx) = (names["$c"], names["$iu"], names["$box"]);
    let probe = apply(&mut store, &format!(r#"<group in="{c}" id="$r" repeat.count="1"><let id="$reader" name="reader" value="0"/></group>"#));
    let (r, reader) = (probe["$r"], probe["$reader"]);
    store.apply(tx("repeat the picture", vec![Op::Reparent { id: u, parent: Some(r), order: OrderKey::first() }])).unwrap();
    let value: LeafPath = "let.value".parse().unwrap();
    let part = NodeId::from_path(&[u, iu, bx]).unwrap();
    store.apply(tx("hold the picture's box", vec![Op::Set { id: reader, path: value, slot: Slot::Const(Value::Ref(part)) }])).unwrap();
    let item_u = NodeId::item(r, "0", u).unwrap();
    let item_part = NodeId::from_path(&[item_u.path(), vec![iu, bx]].concat()).unwrap();
    assert!(store.tree().is_live(item_part));
    let item_reader = NodeId::item(r, "0", reader).unwrap();
    assert_eq!(store.tree().leaf(item_reader, value).and_then(Slot::constant).and_then(Value::as_ref), Some(item_part));
}
