//! Copies apart from the authored document (reactive-engine handoff §4.6,
//! phase 2c stage 1). The store keeps the authored document and the copies
//! its owners derive in two documents behind one `Tree`; under `differential`
//! each of these also runs against the reference that keeps copies in the
//! document, compared after every call.

use iron_document::facet::{Composition, Name, Size, Transform2d};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
}

/// A 2D composition `sheet`, and the store.
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

fn draw(store: &mut Store, comp: NodeId, markup: &str) -> BTreeMap<String, NodeId> {
    let built = markup::ops(markup, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("draw", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    built.named
}

/// A repeat's items are derived copies: fewer items remove the extra copies
/// outright, with no tombstone, and more bring the same ids back.
#[test]
fn a_removed_copy_leaves_nothing_and_comes_back_with_its_id() {
    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<group id="$r" name="rows" repeat.count="3"><rect name="row" x="0" y="{= $r.repeat.index * 10}" w="5" h="5"/></group>"#);
    let items = |store: &Store| -> Vec<NodeId> { store.tree().children(n["r"]).iter().copied().filter(|k| k.is_expanded()).collect() };
    let three = items(&store);
    assert_eq!(three.len(), 3, "{three:?}");
    assert!(store.authored().node(three[0]).is_none(), "the authored document holds no copy");

    store.apply(tx("one", vec![Op::Set { id: n["r"], path: path("repeat.count"), slot: num(1.0) }])).unwrap();
    assert_eq!(items(&store), three[..1]);
    for &gone in &three[1..] {
        assert!(store.tree().node(gone).is_none(), "{gone} is removed outright, not tombstoned");
    }
    // An op naming a copy is refused as one, there or removed, and changes
    // nothing.
    let before = store.save();
    for id in [three[0], three[2]] {
        let err = store.apply(tx("late", vec![Op::Set { id, path: path("transform2d.x"), slot: num(9.0) }])).expect_err("a copy");
        assert_eq!(err[0].kind, ErrorKind::BadId, "{err:?}");
    }
    assert_eq!(store.save(), before);

    store.apply(tx("three", vec![Op::Set { id: n["r"], path: path("repeat.count"), slot: num(3.0) }])).unwrap();
    assert_eq!(items(&store), three, "the items come back with the ids they had");

    // Undo and redo remove and restore them again.
    store.undo(0).unwrap().unwrap();
    assert_eq!(items(&store), three[..1]);
    assert!(store.tree().node(three[2]).is_none());
    store.redo(0).unwrap().unwrap();
    assert_eq!(items(&store), three);
    assert!(store.tree().live(three[2]).is_some());
}

/// A refusal after expansion made and removed copies restores both
/// documents: the copies the transaction removed are back, and the ones it
/// made are gone. The transaction changes the repeat's template (a rect
/// added, the old one deleted), which `run`'s own expansion copies into
/// every item; a count would not, since items settle after `run`. Then a
/// context's markup refuses it, checked only once expansion has run
/// (`check_created`).
#[test]
fn a_refused_transaction_restores_the_copies() {
    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<group id="$r" name="rows" repeat.count="3"><rect id="$row" name="row" x="0" y="{= $r.repeat.index * 10}" w="5" h="5"/></group>"#);
    let (ops, context) = contexts::put_ops(store.tree(), r#"<context name="k"></context>"#).unwrap();
    store.apply(tx("context", ops)).unwrap();
    let copies = |store: &Store| -> Vec<NodeId> {
        let doc = store.tree();
        let mut v: Vec<NodeId> = doc.descendants(n["r"]).into_iter().filter(|d| d.is_expanded()).collect();
        v.sort();
        v
    };
    let before = (copies(&store), store.save());
    assert!(!before.0.is_empty());
    let fresh = store.authored().next_id();
    let order = OrderKey::after(&store.tree().live(n["row"]).unwrap().order);
    let edit = |bad: bool| {
        let mut size = Size::default();
        size.w = num(5.0);
        size.h = num(5.0);
        let mut ops = vec![
            Op::Create { id: fresh, ty: ty("rect"), parent: Some(n["r"]), order: order.clone(), facets: vec![Transform2d::default().wrap(), size.wrap()] },
            Op::Delete { id: n["row"] },
        ];
        if bad {
            ops.push(Op::Set { id: context, path: path("context_def.markup"), slot: Slot::Const(Value::Str(r#"<context name="k"><rule match="all" affords.wobble="move"/></context>"#.into())) });
        }
        ops
    };
    let err = store.apply(tx("bad", edit(true))).expect_err("bad markup");
    assert_eq!(err[0].kind, ErrorKind::BadContext, "refused after expansion: {err:?}");
    assert_eq!((copies(&store), store.save()), before);
    store.tree().check_invariants().unwrap();
    // Without the bad markup the same edit removes each item's copy of the
    // old rect and makes one of the new, so the refusal above came after
    // expansion had made and removed them.
    store.apply(tx("good", edit(false))).unwrap();
    let after = copies(&store);
    let removed = before.0.iter().filter(|c| !after.contains(c)).count();
    let made = after.iter().filter(|c| !before.0.contains(c)).count();
    assert_eq!((removed, made), (3, 3), "{:?} → {after:?}", before.0);
}

/// An authored link cannot take a derived relation id: only an owner's
/// expansion makes one, and history would drop its inverse.
#[test]
fn an_authored_link_with_a_derived_id_is_refused() {
    let (mut store, comp) = sheet();
    let meter = store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let n = draw(&mut store, comp, r#"<rect id="$a" name="a" w="5" h="5"/><rect id="$b" name="b" w="5" h="5"/>"#);
    let local = store.authored().next_relation_ids(1)[0];
    let rel = Relation { id: RelationId::expanded(meter, local), from: n["a"], to: n["b"], rel: RelKind::Constrains, data: BTreeMap::new() };
    let before = store.save();
    let err = store.apply(tx("link", vec![Op::Link { rel }])).expect_err("a derived relation id");
    assert_eq!(err[0].kind, ErrorKind::BadId, "{err:?}");
    assert_eq!(store.save(), before);
}

/// A copy's children are its owner's: an authored node cannot be created
/// or moved under one (refused as an edit of the instance), and neither can
/// a session node (refused by `check_parent`).
#[test]
fn nothing_but_its_owner_places_children_in_a_copy() {
    let (mut store, comp) = sheet();
    let meter = store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let copy = store.tree().children(meter).iter().copied().find(|k| k.is_expanded() && store.tree().node(*k).is_some_and(|n| n.ty.name() == "group" || n.ty.name() == "empty")).expect("a copy that admits children");
    let id = store.authored().next_id();
    let err = store.apply(tx("in", vec![Op::Create { id, ty: ty("group"), parent: Some(copy), order: OrderKey::first(), facets: vec![] }])).expect_err("authored under a copy");
    assert_eq!(err[0].kind, ErrorKind::BadId, "{err:?}");
    let n = draw(&mut store, comp, r#"<group id="$g" name="g"/>"#);
    let err = store.apply(tx("move", vec![Op::Reparent { id: n["g"], parent: Some(copy), order: OrderKey::first() }])).expect_err("moved under a copy");
    assert_eq!(err[0].kind, ErrorKind::BadId, "{err:?}");

    let s = store.tree().next_session_ids(1)[0];
    store.apply_session(vec![Op::Create { id: s, ty: ty("group"), parent: None, order: OrderKey::first(), facets: vec![] }]).unwrap();
    let err = store.apply_session(vec![Op::Reparent { id: s, parent: Some(copy), order: OrderKey::first() }]).expect_err("a session node under a copy");
    assert_eq!(err.kind, ErrorKind::BadParent, "{err:?}");
    assert_eq!(store.tree().node(s).unwrap().parent, None);
}

/// The solver can write a copy's transform: an authored distance constraint
/// to a copy moves it. The write is recorded with the transaction, and undo
/// and redo take it back and replay it.
#[test]
fn a_solved_copy_transform_has_history_and_undo() {
    let (mut store, comp) = sheet();
    let meter = store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let x = path("transform2d.x");
    let doc = store.tree();
    let copy = doc
        .descendants(meter)
        .into_iter()
        .find(|&k| k.is_expanded() && doc.leaf(k, x).is_some_and(|s| s.expr().is_none()) && doc.leaf(k, path("transform2d.y")).is_some_and(|s| s.expr().is_none()))
        .expect("a copy with a constant position");
    let was = store.resolved(copy, x).and_then(|v| v.as_f64()).unwrap();
    let built = markup::ops(r#"<ellipse id="$p" x="-300" y="0" rx="5" ry="5"/><constraint id="$fix" kind="fix" value="0"/><constraint id="$d" kind="distance" value="40"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let n = built.named;
    let rels = store.authored().next_relation_ids(3);
    let constrain = |id: RelationId, from: NodeId, to: NodeId, role: &str| {
        let mut data = BTreeMap::new();
        data.insert("role".to_owned(), Value::Str(role.into()));
        data.insert("anchor".to_owned(), Value::Vec2([0.0, 0.0]));
        Op::Link { rel: Relation { id, from, to, rel: RelKind::Constrains, data } }
    };
    store
        .apply(tx("tie", built.ops.into_iter().chain([constrain(rels[0], n["fix"], n["p"], "a"), constrain(rels[1], n["d"], n["p"], "a"), constrain(rels[2], n["d"], copy, "b")]).collect()))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let last = store.history().past().last().unwrap();
    assert!(last.derived_ops.iter().any(|op| matches!(op, Op::Set { id, .. } if *id == copy)), "the solver wrote the copy: {:?}", last.derived_ops);
    let moved = store.resolved(copy, x).and_then(|v| v.as_f64()).unwrap();
    assert!((moved - was).abs() > 1.0, "the copy moved: {was} → {moved}");

    store.undo(0).unwrap().unwrap();
    assert_eq!(store.resolved(copy, x).and_then(|v| v.as_f64()), Some(was));
    store.redo(0).unwrap().unwrap();
    assert_eq!(store.resolved(copy, x).and_then(|v| v.as_f64()), Some(moved));
}

/// A store derives its own copies: a saved document never holds one, and
/// `load` refuses a file that does.
#[test]
fn load_refuses_a_document_holding_a_copy() {
    let (mut store, comp) = sheet();
    store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let saved = store.save();
    assert!(Store::load(&saved).is_ok());
    let copy = store.tree().live_nodes().map(|n| n.id).find(|id| id.is_expanded()).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&saved).unwrap();
    let nodes = json["nodes"].as_object_mut().unwrap();
    nodes.insert(copy.to_string(), serde_json::json!({ "order": "a9", "parent": null, "ty": "group" }));
    let err = Store::load(&json.to_string()).err().expect("a copy in the file");
    assert!(err.to_string().contains("derived copy"), "{err}");
}

/// The node budget (`expand::NODE_BUDGET`, 50,000 derived nodes) cuts per
/// owner in id order (spec D12): with copies apart the cut is the one the
/// in-document reference makes, compared after every call under
/// `differential`. Three repeats each wanting 20,000 nodes (10,000 items, a
/// repeat's most): the first two keep all of theirs, the third is cut, and
/// shrinking the first lets the third grow back.
///
/// Ignored by default: 60,000 wanted nodes take 11 s in release (27 s under
/// `differential`) and minutes in debug. Run it in release, with and without
/// `differential`, before a stage-1 commit:
///   cargo test -p iron_document --release --target aarch64-apple-darwin --test suite copies_apart::the_node_budget -- --ignored
#[test]
#[ignore]
fn the_node_budget_cuts_the_later_owner() {
    let (mut store, comp) = sheet();
    let repeat = |name: &str| format!(r#"<group id="${name}" name="{name}" repeat.count="10000" repeat.max="10000"><rect name="x" w="1" h="1"/></group>"#);
    let n = draw(&mut store, comp, &["a", "b", "c"].map(repeat).concat());
    let under = |store: &Store| ["a", "b", "c"].map(|k| store.tree().descendants(n[k]).into_iter().filter(|d| d.is_expanded()).count());
    let budget = |store: &Store| store.problems().iter().any(|p| p.kind == "budget");
    assert_eq!(under(&store), [20_000, 20_000, 10_000], "the budget is spent in owner order");
    assert_eq!(expand::NODE_BUDGET, 50_000);
    assert!(budget(&store), "{:?}", store.problems());

    store.apply(tx("shrink", vec![Op::Set { id: n["a"], path: path("repeat.count"), slot: num(100.0) }])).unwrap();
    assert_eq!(under(&store), [200, 20_000, 20_000], "the cut owner grows into the room");
    assert!(!budget(&store));
    store.undo(0).unwrap().unwrap();
    assert_eq!(under(&store), [20_000, 20_000, 10_000]);
    assert!(budget(&store));
}

fn constrains(id: RelationId, from: NodeId, to: NodeId, role: &str) -> Op {
    let mut data = BTreeMap::new();
    data.insert("role".to_owned(), Value::Str(role.into()));
    data.insert("anchor".to_owned(), Value::Vec2([0.0, 0.0]));
    Op::Link { rel: Relation { id, from, to, rel: RelKind::Constrains, data } }
}

/// A copy with constant x and y under `under`.
fn placed_copy(store: &Store, under: NodeId) -> NodeId {
    let doc = store.tree();
    doc.descendants(under)
        .into_iter()
        .find(|&k| k.is_expanded() && doc.leaf(k, path("transform2d.x")).is_some_and(|s| s.expr().is_none()) && doc.leaf(k, path("transform2d.y")).is_some_and(|s| s.expr().is_none()))
        .expect("a copy with a constant position")
}

/// Deleting an instance one of whose copies an authored constraint holds:
/// the operand goes with the copy, and the deletion applies.
#[test]
fn deleting_an_instance_whose_copy_is_constrained() {
    let (mut store, comp) = sheet();
    let meter = store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let copy = placed_copy(&store, meter);
    let built = markup::ops(r#"<constraint id="$fix" kind="fix" value="0"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let rel = store.authored().next_relation_ids(1)[0];
    let fix = built.named["fix"];
    store.apply(tx("fix", built.ops.into_iter().chain([constrains(rel, fix, copy, "a")]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    store.apply(tx("delete", vec![Op::Delete { id: meter }])).unwrap_or_else(|e| panic!("{e:?}"));
    store.undo(0).unwrap().unwrap();
    assert!(store.tree().live(copy).is_some());
}

/// Undo and redo of a transaction whose solve wrote a copy leave the
/// authored document as it was before and after it: the copy's values are
/// the solver's, never turned into overrides on the instance.
#[test]
fn undoing_a_solved_copy_adds_no_override() {
    let (mut store, comp) = sheet();
    let meter = store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let copy = placed_copy(&store, meter);
    let before = store.save();
    let built = markup::ops(r#"<ellipse id="$p" x="-300" y="0" rx="5" ry="5"/><constraint id="$fix" kind="fix" value="0"/><constraint id="$d" kind="distance" value="40"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let n = built.named;
    let rels = store.authored().next_relation_ids(3);
    store.apply(tx("tie", built.ops.into_iter().chain([constrains(rels[0], n["fix"], n["p"], "a"), constrains(rels[1], n["d"], n["p"], "a"), constrains(rels[2], n["d"], copy, "b")]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    let overrides = |store: &Store| store.tree().live_nodes().filter(|n| n.ty.name() == "override").count();
    let x = path("transform2d.x");
    let (was, moved) = (Store::load(&before).unwrap().resolved(copy, x), store.resolved(copy, x));
    assert_ne!(was, moved, "the solver moved the copy");
    assert_eq!(overrides(&store), 0);
    store.undo(0).unwrap().unwrap();
    assert_eq!((overrides(&store), store.resolved(copy, x)), (0, was.clone()), "undo puts the copy back and adds no override");
    store.redo(0).unwrap().unwrap();
    assert_eq!((overrides(&store), store.resolved(copy, x)), (0, moved.clone()), "redo writes the copy again, as the solver did");
    store.undo(0).unwrap().unwrap();
    assert_eq!((overrides(&store), store.resolved(copy, x)), (0, was));
}

/// An authored link to a copy nested in a repeat's item saves, and the
/// file loads.
#[test]
fn a_link_to_a_nested_copy_saves_and_loads() {
    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<group id="$r" name="rows" repeat.count="2"><rect name="row" x="0" y="{= $r.repeat.index * 10}" w="5" h="5"/></group>"#);
    let doc = store.tree();
    let nested = doc.descendants(n["r"]).into_iter().find(|d| d.expanded_parts().is_some_and(|(owner, _)| owner.is_expanded())).expect("a copy inside an item");
    let built = markup::ops(r#"<constraint id="$fix" kind="fix" value="0"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let rel = store.authored().next_relation_ids(1)[0];
    let fix = built.named["fix"];
    store.apply(tx("fix", built.ops.into_iter().chain([constrains(rel, fix, nested, "a")]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    let saved = store.save();
    let loaded = Store::load(&saved).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(loaded.save(), saved);
}

/// The same items in another order move their copies: nothing is made or
/// removed, and each move names the repeat as the parent it had.
#[test]
fn a_reorder_moves_copies_and_makes_none() {
    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<let id="$names" name="names" value="{= list('a', 'b', 'c')}"/><group id="$rows" name="rows" repeat.each="{= $names}"><rect name="row" x="0" y="{= $rows.repeat.index * 10}" w="5" h="5"/></group>"#);
    store.take_changes();
    store.apply(tx("reorder", vec![Op::Set { id: n["names"], path: path("let.value"), slot: Slot::Bound(Expr::parse("list('c', 'a', 'b')").unwrap()) }])).unwrap();
    let changes = store.take_changes();
    assert!(changes.created.is_empty() && changes.removed.is_empty(), "{changes:?}");
    assert!(!changes.moved.is_empty(), "{changes:?}");
    assert!(changes.moved.iter().all(|(_, was)| *was == Some(n["rows"])), "{:?}", changes.moved);
}

/// A new link to a copy that is not there is refused, though its
/// instance is.
#[test]
fn a_new_link_to_a_missing_copy_is_refused() {
    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<group id="$r" name="rows" repeat.count="2"><rect id="$row" name="row" x="0" y="{= $r.repeat.index * 10}" w="5" h="5"/></group>"#);
    let item = store.tree().children(n["r"]).iter().copied().find(|k| k.is_expanded()).unwrap();
    let missing = NodeId::expanded(item, store.authored().next_id().offset(30)).unwrap();
    let built = markup::ops(r#"<constraint id="$fix" kind="fix" value="0"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let rel = store.authored().next_relation_ids(1)[0];
    let fix = built.named["fix"];
    let err = store.apply(tx("fix", built.ops.into_iter().chain([constrains(rel, fix, missing, "a")]).collect())).expect_err("no such copy");
    assert_eq!(err[0].kind, ErrorKind::UnknownNode, "{err:?}");
}

/// A meter whose copy `copy` a distance constraint `d` holds 40 from a
/// fixed ellipse: the store, the meter, the copy, `d`, and the copy's x
/// before and after the solve.
fn tied() -> (Store, NodeId, NodeId, NodeId, f64, f64) {
    let (mut store, comp) = sheet();
    let meter = store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let copy = placed_copy(&store, meter);
    let x = path("transform2d.x");
    let was = store.resolved(copy, x).and_then(|v| v.as_f64()).unwrap();
    let built = markup::ops(r#"<ellipse id="$p" x="-300" y="0" rx="5" ry="5"/><constraint id="$fix" kind="fix" value="0"/><constraint id="$d" kind="distance" value="40"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let n = built.named;
    let rels = store.authored().next_relation_ids(3);
    store.apply(tx("tie", built.ops.into_iter().chain([constrains(rels[0], n["fix"], n["p"], "a"), constrains(rels[1], n["d"], n["p"], "a"), constrains(rels[2], n["d"], copy, "b")]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    let moved = store.resolved(copy, x).and_then(|v| v.as_f64()).unwrap();
    assert!((moved - was).abs() > 1.0, "the solver moved the copy: {was} → {moved}");
    (store, meter, copy, n["d"], was, moved)
}

fn x_of(store: &Store, copy: NodeId) -> f64 {
    store.resolved(copy, path("transform2d.x")).and_then(|v| v.as_f64()).unwrap()
}

/// A copy the solver moved goes back to its template's value when the
/// constraint holding it goes, and undo moves it back.
#[test]
fn a_solved_copy_returns_to_its_template_when_its_constraint_goes() {
    let (mut store, _, copy, d, was, moved) = tied();
    store.apply(tx("untie", vec![Op::Delete { id: d }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(x_of(&store, copy), was);
    store.undo(0).unwrap().unwrap();
    assert_eq!(x_of(&store, copy), moved);
}

/// Undoing the delete of an instance whose copy the solver moved brings the
/// copy back where the solver had it.
#[test]
fn undoing_the_delete_of_a_solved_copys_instance_puts_it_back_solved() {
    let (mut store, meter, copy, _, _, moved) = tied();
    store.apply(tx("delete", vec![Op::Delete { id: meter }])).unwrap_or_else(|e| panic!("{e:?}"));
    store.undo(0).unwrap().unwrap();
    assert_eq!(x_of(&store, copy), moved);
}

/// Undoing a definition edit that expanded a solved copy again puts the
/// copy back where the solver had it before the edit.
#[test]
fn undoing_a_definition_edit_puts_a_solved_copy_back() {
    let (mut store, _, copy, _, was, moved) = tied();
    let local = *copy.path().last().unwrap();
    store.apply(tx("edit the definition", vec![Op::Set { id: local, path: path("transform2d.x"), slot: num(was + 7.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    store.undo(0).unwrap().unwrap();
    assert_eq!(x_of(&store, copy), moved);
}

/// Disabling the constraint that holds a solved copy lets it go, as
/// deleting it does, and the next transaction finds the copy as its owner
/// wants it (the owners' check compares every copy).
#[test]
fn disabling_a_solved_copys_constraint_lets_it_go() {
    let (mut store, _, copy, d, was, moved) = tied();
    store.apply(tx("disable", vec![Op::Set { id: d, path: path("constraint.enabled"), slot: Slot::Const(Value::Bool(false)) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(x_of(&store, copy), was);
    store.apply(tx("anything", vec![Op::Set { id: d, path: path("constraint.value"), slot: num(50.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(x_of(&store, copy), was);
    store.undo(0).unwrap().unwrap();
    store.undo(0).unwrap().unwrap();
    assert_eq!(x_of(&store, copy), moved);
}

/// A `pin` constraint holding `copy` at `at`.
fn pin(store: &mut Store, comp: NodeId, copy: NodeId, at: [f64; 2]) -> NodeId {
    let built = markup::ops(r#"<constraint id="$pin" kind="pin" value="0"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let pin = built.named["pin"];
    let mut link = constrains(store.authored().next_relation_ids(1)[0], pin, copy, "a");
    if let Op::Link { rel } = &mut link {
        rel.data.insert("at".to_owned(), Value::Vec2(at));
    }
    store.apply(tx("pin", built.ops.into_iter().chain([link]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    pin
}

/// A repeat of one rect, and the copy in its item `key`.
fn row_copy(store: &mut Store, comp: NodeId, count: usize, key: &str) -> (NodeId, NodeId) {
    let n = draw(store, comp, &format!(r#"<group id="$r" repeat.count="{count}"><rect x="0" y="0" w="5" h="5"/></group>"#));
    let copy = store.tree().children(NodeId::item_scope(n["r"], key).unwrap())[0];
    (n["r"], copy)
}

/// Fixing a pinned copy keeps it where the solver had it: a fixed copy is
/// still in its cluster, as an input, and is not let go.
#[test]
fn fixing_a_solved_copy_keeps_its_pose() {
    let (mut store, comp) = sheet();
    let (_, copy) = row_copy(&mut store, comp, 1, "0");
    pin(&mut store, comp, copy, [-40.0, 0.0]);
    let built = markup::ops(r#"<constraint id="$fix" kind="fix" value="0"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let link = constrains(store.authored().next_relation_ids(1)[0], built.named["fix"], copy, "a");
    store.apply(tx("fix", built.ops.into_iter().chain([link]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(x_of(&store, copy), -40.0);
}

/// Undoing a repeat's shrink brings a removed item's solved copy back
/// where the solver had it: repeat settling removed it after expansion.
#[test]
fn undoing_a_repeat_shrink_puts_a_solved_item_back() {
    let (mut store, comp) = sheet();
    let (r, copy) = row_copy(&mut store, comp, 2, "1");
    pin(&mut store, comp, copy, [-260.0, 0.0]);
    store.apply(tx("one row", vec![Op::Set { id: r, path: path("repeat.count"), slot: num(1.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(!store.tree().is_live(copy));
    store.undo(0).unwrap().unwrap();
    assert_eq!(x_of(&store, copy), -260.0);
}

/// A pinned copy a producer drives stays in its cluster, as a driven input:
/// nothing lets it go, and the owners' check finds it as the solver left
/// it after undo.
#[test]
fn a_driven_solved_copy_is_not_let_go() {
    let (mut store, comp) = sheet();
    let (_, copy) = row_copy(&mut store, comp, 1, "0");
    pin(&mut store, comp, copy, [-40.0, 0.0]);
    let x = path("transform2d.x");
    let live = store.register_producer("live");
    store.publish(live, BTreeMap::from([(Leaf { node: copy, path: x }, Value::Number(-40.0))]));
    let rename = |name: &str| tx("rename", vec![Op::Set { id: comp, path: path("name.text"), slot: Slot::Const(Value::Str(name.into())) }]);
    store.apply(rename("new")).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.tree().leaf(copy, x), Some(&num(-40.0)));
    store.undo(0).unwrap().unwrap();
    store.apply(rename("newer")).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.tree().leaf(copy, x), Some(&num(-40.0)));
}

/// A solved copy a publish removed, outside history, has no solved value
/// to keep: the next transaction and its undo find the same in both stores.
#[test]
fn a_solved_copy_a_publish_removed_is_not_kept() {
    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<let id="$data" value="1 2 3"/><group id="$h" repeat.each="{= $data}"><rect name="cell" x="0" y="0" w="5" h="5"/></group><rect id="$other" x="500" y="0" w="5" h="5"/>"#);
    let items: Vec<NodeId> = store.tree().children(n["h"]).iter().copied().filter(|k| k.is_expanded()).collect();
    let copy = store.tree().children(items[2])[0];
    pin(&mut store, comp, copy, [-260.0, 0.0]);
    let live = store.register_producer("live");
    store.publish(live, BTreeMap::from([(Leaf { node: n["data"], path: path("let.value") }, Value::series(vec![1.0, 2.0], iron_document::Dim::NONE))]));
    assert!(!store.tree().is_live(copy));
    store.apply(tx("unrelated", vec![Op::Set { id: n["other"], path: path("transform2d.x"), slot: num(510.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    store.publish(live, BTreeMap::new());
    store.undo(0).unwrap().unwrap();
}

/// A `fix` constraint on `copy`.
fn fix(store: &mut Store, comp: NodeId, copy: NodeId) -> NodeId {
    let built = markup::ops(r#"<constraint id="$fix" kind="fix" value="0"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let fix = built.named["fix"];
    let link = constrains(store.authored().next_relation_ids(1)[0], fix, copy, "a");
    store.apply(tx("fix", built.ops.into_iter().chain([link]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    fix
}

/// A pinned copy, fixed where the pin put it, goes back to its template
/// when the last of its constraints goes, in one transaction or two.
#[test]
fn a_fixed_solved_copy_is_let_go_with_its_last_constraint() {
    for together in [true, false] {
        let (mut store, comp) = sheet();
        let (_, copy) = row_copy(&mut store, comp, 1, "0");
        let p = pin(&mut store, comp, copy, [-40.0, 0.0]);
        let f = fix(&mut store, comp, copy);
        if together {
            store.apply(tx("unpin, unfix", vec![Op::Delete { id: p }, Op::Delete { id: f }])).unwrap_or_else(|e| panic!("{e:?}"));
        } else {
            store.apply(tx("unpin", vec![Op::Delete { id: p }])).unwrap_or_else(|e| panic!("{e:?}"));
            assert_eq!(x_of(&store, copy), -40.0);
            store.apply(tx("unfix", vec![Op::Delete { id: f }])).unwrap_or_else(|e| panic!("{e:?}"));
        }
        assert_eq!(x_of(&store, copy), 0.0);
        store.apply(tx("rename", vec![Op::Set { id: comp, path: path("name.text"), slot: Slot::Const(Value::Str("again".into())) }])).unwrap_or_else(|e| panic!("{e:?}"));
    }
}

/// Undo after a redo puts a solved copy back as the first undo did: redo
/// records the same restores as the transaction it redoes.
#[test]
fn undo_after_redo_puts_a_solved_copy_back() {
    let (mut store, _, copy, _, was, moved) = tied();
    let local = *copy.path().last().unwrap();
    store.apply(tx("edit the definition", vec![Op::Set { id: local, path: path("transform2d.x"), slot: num(was + 7.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    for _ in 0..2 {
        store.undo(0).unwrap().unwrap();
        assert_eq!(x_of(&store, copy), moved);
        store.redo(0).unwrap().unwrap();
    }
    let (mut store, comp) = sheet();
    let (r, copy) = row_copy(&mut store, comp, 2, "1");
    pin(&mut store, comp, copy, [-260.0, 0.0]);
    store.apply(tx("one row", vec![Op::Set { id: r, path: path("repeat.count"), slot: num(1.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    for _ in 0..2 {
        store.undo(0).unwrap().unwrap();
        assert_eq!(x_of(&store, copy), -260.0);
        store.redo(0).unwrap().unwrap();
    }
}

/// An edit of a fixed copy's template that leaves its position alone
/// leaves the solved position alone: expansion does not write the leaves
/// the solver holds.
#[test]
fn a_template_edit_keeps_a_fixed_copys_solved_position() {
    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<group id="$r" repeat.count="1"><rect id="$row" x="0" y="0" w="5" h="5"/></group>"#);
    let copy = store.tree().children(NodeId::item_scope(n["r"], "0").unwrap())[0];
    pin(&mut store, comp, copy, [-40.0, 0.0]);
    fix(&mut store, comp, copy);
    store.apply(tx("wider", vec![Op::Set { id: n["row"], path: path("size.w"), slot: num(9.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(x_of(&store, copy), -40.0);
    assert_eq!(store.resolved(copy, path("size.w")), Some(Value::Number(9.0)));
}

/// Moving a held copy moves it: the edit becomes an override, which
/// changes what is wanted for the leaf, so it reaches the copy.
#[test]
fn moving_a_held_copy_moves_it() {
    let (mut store, _, copy, _, _, moved) = tied();
    store.apply(tx("move the copy", vec![Op::Set { id: copy, path: path("transform2d.x"), slot: num(moved - 80.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!((x_of(&store, copy) - moved).abs() > 1.0, "the copy did not move: {}", x_of(&store, copy));
}

/// A template edit of a leaf reaches a held copy: rotation, which a
/// distance constraint never moves, and a new binding of a bound leaf.
#[test]
fn a_template_edit_of_a_held_leaf_reaches_the_copy() {
    let (mut store, _, copy, _, _, _) = tied();
    let local = *copy.path().last().unwrap();
    let rot = path("transform2d.rot");
    store.apply(tx("turn the definition", vec![Op::Set { id: local, path: rot, slot: num(30.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.resolved(copy, rot), Some(Value::Number(30.0)));

    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<ellipse id="$p" x="-300" y="0" rx="5" ry="5"/><group id="$r" repeat.count="1"><rect id="$row" x="{= 10}" y="5" w="5" h="5"/></group>"#);
    let copy = store.tree().children(NodeId::item_scope(n["r"], "0").unwrap())[0];
    let built = markup::ops(r#"<constraint id="$fix" kind="fix" value="0"/><constraint id="$d" kind="distance" value="400"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let c = built.named;
    let rels = store.authored().next_relation_ids(3);
    store.apply(tx("tie", built.ops.into_iter().chain([constrains(rels[0], c["fix"], n["p"], "a"), constrains(rels[1], c["d"], n["p"], "a"), constrains(rels[2], c["d"], copy, "b")]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    let x = path("transform2d.x");
    store.apply(tx("rebind", vec![Op::Set { id: n["row"], path: x, slot: Slot::Bound(Expr::parse("20").unwrap()) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.resolved(copy, x), Some(Value::Number(20.0)));
}

/// A held copy let go in the transaction that binds its template: the
/// binding reaches it, and the graph reads it from then on, through undo
/// and redo.
#[test]
fn a_released_copy_reads_its_templates_binding() {
    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<let id="$k" value="70"/><group id="$r" repeat.count="1"><rect id="$row" x="0" y="0" w="5" h="5"/></group>"#);
    let copy = store.tree().children(NodeId::item_scope(n["r"], "0").unwrap())[0];
    let p = pin(&mut store, comp, copy, [-40.0, 0.0]);
    let x = path("transform2d.x");
    store.apply(tx("bind, unpin", vec![Op::Set { id: n["row"], path: x, slot: Slot::Bound(Expr::parse(&format!("{}.let.value", n["k"])).unwrap()) }, Op::Delete { id: p }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.resolved(copy, x), Some(Value::Number(70.0)));
    store.undo(0).unwrap().unwrap();
    store.redo(0).unwrap().unwrap();
    store.apply(tx("k", vec![Op::Set { id: n["k"], path: path("let.value"), slot: num(90.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.resolved(copy, x), Some(Value::Number(90.0)));
}

/// Undoing a transaction that constrained a copy and edited its template
/// puts the copy back at its template's value: undo lets it go.
#[test]
fn undoing_constraining_a_copy_and_editing_its_template() {
    for first in ["pin", "fix"] {
        let (mut store, comp) = sheet();
        let n = draw(&mut store, comp, r#"<group id="$r" repeat.count="1"><rect id="$row" x="0" y="0" w="5" h="5"/></group>"#);
        let copy = store.tree().children(NodeId::item_scope(n["r"], "0").unwrap())[0];
        let built = markup::ops(&format!(r#"<constraint id="$c" kind="{first}" value="0"/>"#), Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
        let mut link = constrains(store.authored().next_relation_ids(1)[0], built.named["c"], copy, "a");
        if let Op::Link { rel } = &mut link
            && first == "pin"
        {
            rel.data.insert("at".to_owned(), Value::Vec2([-40.0, 0.0]));
        }
        let ops: Vec<Op> = built.ops.into_iter().chain([link, Op::Set { id: n["row"], path: path("transform2d.x"), slot: num(25.0) }]).collect();
        store.apply(tx("constrain and move the template", ops)).unwrap_or_else(|e| panic!("{e:?}"));
        store.undo(0).unwrap().unwrap();
        assert_eq!(x_of(&store, copy), 0.0, "{first}");
        store.apply(tx("rename", vec![Op::Set { id: comp, path: path("name.text"), slot: Slot::Const(Value::Str("x".into())) }])).unwrap_or_else(|e| panic!("{e:?}"));
    }
}

/// Undoing a template edit of a pinned copy puts it back where the solver
/// had it, not at the edit's value.
#[test]
fn undoing_a_template_edit_of_a_pinned_copy() {
    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<group id="$r" repeat.count="1"><rect id="$row" x="0" y="0" w="5" h="5"/></group>"#);
    let copy = store.tree().children(NodeId::item_scope(n["r"], "0").unwrap())[0];
    pin(&mut store, comp, copy, [-40.0, 0.0]);
    store.apply(tx("move the template", vec![Op::Set { id: n["row"], path: path("transform2d.x"), slot: num(15.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    store.undo(0).unwrap().unwrap();
    assert_eq!(x_of(&store, copy), -40.0);
}

/// A fixed copy whose template binding is replaced by a constant takes the
/// constant: what is wanted for the leaf changed.
#[test]
fn a_fixed_copys_binding_replaced_by_a_constant() {
    let (mut store, comp) = sheet();
    let n = draw(&mut store, comp, r#"<rect id="$s" x="15" y="0" w="5" h="5"/><group id="$r" repeat.count="1"><rect id="$row" y="0" w="5" h="5"/></group>"#);
    let x = path("transform2d.x");
    store.apply(tx("bind", vec![Op::Set { id: n["row"], path: x, slot: Slot::Bound(Expr::parse(&format!("{}.transform2d.x", n["s"])).unwrap()) }])).unwrap_or_else(|e| panic!("{e:?}"));
    let copy = store.tree().children(NodeId::item_scope(n["r"], "0").unwrap())[0];
    fix(&mut store, comp, copy);
    store.apply(tx("constant, and move the sibling", vec![Op::Set { id: n["row"], path: x, slot: num(15.0) }, Op::Set { id: n["s"], path: x, slot: num(80.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(x_of(&store, copy), 15.0);
}
