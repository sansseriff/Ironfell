//! A refused transaction leaves the document bit-identical (reactive-engine
//! handoff §4.3, task 5; spec §17, phase 2a's exit). The store applies ops to
//! its own document with the journal open, and a refusal at any stage rolls
//! it back: the document compares equal field by field (its indexes and empty
//! map keys included), saves the same text, keeps its invariants, and mints
//! the same ids next. A successful transaction after each refusal shows the
//! journal was closed (`Document::begin` asserts none is open).
//!
//! Under the `differential` feature every rollback is also checked against a
//! shadow copy taken when the journal opened, across the whole suite.

use iron_document::facet::{Composition, Constraint, Name, Size, Transform2d};
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

fn bind(e: &str) -> Slot {
    Slot::Bound(Expr::parse(e).unwrap())
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

fn draw(store: &Store, comp: NodeId, markup: &str) -> (Vec<Op>, BTreeMap<String, NodeId>) {
    let built = markup::ops(markup, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap_or_else(|e| panic!("{e}"));
    (built.ops, built.named)
}

fn place(store: &mut Store, markup: &str) {
    let plan = edit::plan(store, &format!("<edit>{markup}</edit>"), None).unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("place", plan.ops)).unwrap_or_else(|e| panic!("{e:?}"));
}

/// Two lets that read each other, refused by the checks after expansion,
/// with ids after any `ops` already creates.
fn cycle(store: &Store, comp: NodeId, ops: &[Op]) -> Vec<Op> {
    let mut first = store.authored().next_id();
    for op in ops {
        if let Op::Create { id, .. } = op
            && id.minted().is_some()
            && *id >= first
        {
            first = id.offset(1);
        }
    }
    let markup = r#"<let id="$a" value="{= $b}"/><let id="$b" value="{= $a}"/>"#;
    markup::ops(markup, Some(comp), OrderKey::after(&OrderKey::first()), first).unwrap_or_else(|e| panic!("{e}")).ops
}

/// What must come back after a refusal.
struct Before {
    doc: Document,
    saved: String,
    next: (NodeId, RelationId, Vec<NodeId>),
}

fn before(store: &Store) -> Before {
    let doc = store.authored().clone();
    let next = (doc.next_id(), doc.next_relation_id(), doc.next_session_ids(1));
    Before { saved: store.save(), doc, next }
}

/// `f` fails, and the document is as it was; then a transaction applies.
fn refused<T: std::fmt::Debug, E: std::fmt::Debug>(store: &mut Store, comp: NodeId, what: &str, f: impl FnOnce(&mut Store) -> Result<T, E>) -> E {
    let b = before(store);
    let err = f(store).expect_err(what);
    let doc = store.authored();
    assert!(*doc == b.doc, "{what}: the document differs after the refusal ({err:?})");
    assert_eq!(store.save(), b.saved, "{what}");
    doc.check_invariants().unwrap_or_else(|e| panic!("{what}: {e}"));
    assert_eq!((doc.next_id(), doc.next_relation_id(), doc.next_session_ids(1)), b.next, "{what}: the counters moved");
    // The journal is closed: the next transaction can open one.
    let id = store.authored().next_id();
    store.apply(tx("after", vec![Op::Create { id, ty: ty("group"), parent: Some(comp), order: OrderKey::first(), facets: vec![] }])).unwrap_or_else(|e| panic!("{what}: {e:?}"));
    store.undo(0).unwrap().unwrap();
    err
}

fn rect(store: &Store, parent: NodeId) -> Op {
    Op::Create { id: store.authored().next_id(), ty: ty("rect"), parent: Some(parent), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap()] }
}

#[test]
fn an_op_error_mid_list_rolls_back_the_ops_before_it() {
    let (mut store, comp) = sheet();
    // A rect, then a set on an id never handed out.
    let missing = store.authored().next_id().offset(40);
    let ops = vec![rect(&store, comp), Op::Set { id: missing, path: path("size.w"), slot: num(1.0) }];
    refused(&mut store, comp, "a set on a node that does not exist", |s| s.apply(tx("bad", ops)));
}

/// `add_facet` inserts the facet before it checks the bindings in it.
#[test]
fn a_facet_added_then_refused_is_taken_back() {
    let (mut store, comp) = sheet();
    let (ops, n) = draw(&store, comp, r#"<group id="$g"/>"#);
    store.apply(tx("group", ops)).unwrap();
    let mut t = Transform2d::default();
    t.x = bind(&format!("{}.transform2d.x", n["g"].offset(500)));
    let rename = Op::Set { id: comp, path: path("name.text"), slot: Slot::Const(Value::Str("renamed".into())) };
    refused(&mut store, comp, "a facet bound to a missing node", |s| s.apply(tx("bad", vec![rename, Op::AddFacet { id: n["g"], facet: t.wrap() }])));
}

#[test]
fn a_refusal_after_expansion_made_derived_nodes() {
    let (mut store, comp) = sheet();
    place(&mut store, r#"<place def="std:meter" in="sheet" x="400" y="0"/>"#);
    let derived_before = store.tree().live_nodes().filter(|n| n.id.is_expanded()).count();
    let plan = edit::plan(&store, r#"<edit><place def="std:meter" in="sheet" x="0" y="0"/></edit>"#, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let mut ops = plan.ops;
    let more = cycle(&store, comp, &ops);
    ops.extend(more);
    let errs = refused(&mut store, comp, "a second meter and a cycle", |s| s.apply(tx("bad", ops)));
    assert_eq!(errs[0].kind, ErrorKind::Cycle, "{errs:?}");
    assert_eq!(store.tree().live_nodes().filter(|n| n.id.is_expanded()).count(), derived_before);
}

#[test]
fn a_refusal_after_expansion_removed_derived_nodes() {
    let (mut store, comp) = sheet();
    place(&mut store, r#"<place def="std:meter" in="sheet" x="400" y="0"/>"#);
    let uses: Vec<NodeId> = store.tree().children(comp).iter().copied().filter(|&c| store.tree().live(c).is_some_and(|n| n.ty.name() == "use")).collect();
    assert_eq!(uses.len(), 1);
    let mut ops = vec![Op::Delete { id: uses[0] }];
    let more = cycle(&store, comp, &ops);
    ops.extend(more);
    refused(&mut store, comp, "the meter's picture deleted, and a cycle", |s| s.apply(tx("bad", ops)));
    assert!(store.tree().is_live(uses[0]));
}

/// A new input on a definition rewrites every instance's arguments
/// (`op.rs` `new_input_args`) before a later check refuses.
#[test]
fn a_refusal_after_an_input_rewrote_every_instance() {
    let (mut store, comp) = sheet();
    place(&mut store, r#"<place def="std:meter" in="sheet" x="400" y="0"/><place def="std:meter" in="sheet" x="0" y="0"/>"#);
    let def = standard::find(store.tree(), "meter").unwrap();
    let built = markup::ops(r#"<port_spec name="extra" kind="value" dir="in" signal="Number"/>"#, Some(def), OrderKey::first(), store.authored().next_id()).unwrap_or_else(|e| panic!("{e}"));
    let mut ops = built.ops;
    let more = cycle(&store, comp, &ops);
    ops.extend(more);
    let errs = refused(&mut store, comp, "a new input, and a cycle", |s| s.apply(tx("bad", ops)));
    assert_eq!(errs[0].kind, ErrorKind::Cycle, "{errs:?}");
}

/// An op aimed at a derived node is refused (`expand::refuse`), after an
/// earlier op of the same transaction applied.
#[test]
fn an_op_on_a_derived_node_is_refused_mid_list() {
    let (mut store, comp) = sheet();
    place(&mut store, r#"<place def="std:meter" in="sheet" x="400" y="0"/>"#);
    let derived = store.tree().live_nodes().map(|n| n.id).find(|id| id.is_expanded()).unwrap();
    let ops = vec![rect(&store, comp), Op::Delete { id: derived }];
    refused(&mut store, comp, "a delete of a derived node", |s| s.apply(tx("bad", ops)));
}

/// Interaction state may not drive what the document holds: `check_created`
/// or `check_scope` refuses after the ops applied.
#[test]
fn a_scope_refusal_after_the_ops_applied() {
    let (mut store, comp) = sheet();
    let id = store.authored().next_id();
    let mut fill = facet::Fill::default();
    fill.opacity = num(1.0);
    store.apply(tx("rect", vec![Op::Create { id, ty: ty("rect"), parent: Some(comp), order: OrderKey::first(), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), fill.wrap(), facet::Interaction::default().wrap()] }])).unwrap();
    let ops = vec![rect(&store, comp), Op::Set { id, path: path("transform2d.x"), slot: bind(&format!("{id}.interaction.hovered ? 10 : 0")) }];
    refused(&mut store, comp, "state driving a position", |s| s.apply(tx("bad", ops)));
}

/// Two points `p` and `q`, `p` fixed at the origin, `q` 50 from it: one
/// cluster.
fn pair(store: &mut Store, comp: NodeId) -> BTreeMap<String, NodeId> {
    let markup = r#"<ellipse id="$p" x="0" y="0" rx="5" ry="5"/><ellipse id="$q" x="40" y="0" rx="5" ry="5"/><constraint id="$fix" kind="fix" value="0"/><constraint id="$d" kind="distance" value="50"/>"#;
    let (mut ops, n) = draw(store, comp, markup);
    let rels = store.authored().next_relation_ids(3);
    ops.push(constrain(rels[0], n["fix"], n["p"], "a"));
    ops.push(constrain(rels[1], n["d"], n["p"], "a"));
    ops.push(constrain(rels[2], n["d"], n["q"], "b"));
    store.apply(tx("pair", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    n
}

fn constrain(id: RelationId, from: NodeId, to: NodeId, role: &str) -> Op {
    let mut data = BTreeMap::new();
    data.insert("role".to_owned(), Value::Str(role.into()));
    data.insert("anchor".to_owned(), Value::Vec2([0.0, 0.0]));
    Op::Link { rel: Relation { id, from, to, rel: RelKind::Constrains, data } }
}


/// A cluster that can't be solved, beside one that can, in one
/// transaction: the first solves and writes; the second keeps its pose and
/// reports, and undo and redo bring its problem back and forth (DS-7: no
/// refusal after derivation).
#[test]
fn an_unsolvable_cluster_beside_one_that_solves() {
    let (mut store, comp) = sheet();
    let first = pair(&mut store, comp);
    let second = pair(&mut store, comp);
    // The first cluster solves and writes `q`. The second gains a point `r`
    // 10 from its fixed `p` and 100 from its `q`, which is 50 from `p`:
    // no pose satisfies both.
    let rels = store.authored().next_relation_ids(4);
    let ids = store.authored().next_ids(3);
    let distance = |v: f64| {
        let mut c = Constraint::default();
        c.kind = Slot::Const(Value::Enum("distance".into()));
        c.value = num(v);
        c.wrap()
    };
    let mut t = Transform2d::default();
    t.x = num(5.0);
    let ops = vec![
        Op::Set { id: first["q"], path: path("transform2d.x"), slot: num(-60.0) },
        Op::Create { id: ids[0], ty: ty("ellipse"), parent: Some(comp), order: OrderKey::first(), facets: vec![t.wrap(), facet::Radius::default().wrap()] },
        Op::Create { id: ids[1], ty: ty("constraint"), parent: Some(comp), order: OrderKey::first(), facets: vec![distance(10.0)] },
        Op::Create { id: ids[2], ty: ty("constraint"), parent: Some(comp), order: OrderKey::first(), facets: vec![distance(100.0)] },
        constrain(rels[0], ids[1], second["p"], "a"),
        constrain(rels[1], ids[1], ids[0], "b"),
        constrain(rels[2], ids[2], second["q"], "a"),
        constrain(rels[3], ids[2], ids[0], "b"),
    ];
    let x = |s: &Store, id: NodeId| s.resolved(id, path("transform2d.x")).and_then(|v| v.as_f64()).unwrap();
    let second_q = x(&store, second["q"]);
    store.apply(tx("bad", ops)).unwrap_or_else(|e| panic!("applied, not refused: {e:?}"));
    assert!((x(&store, first["q"]) + 50.0).abs() < 1e-6, "the first cluster solved: q at {}", x(&store, first["q"]));
    assert_eq!(x(&store, second["q"]), second_q, "the second keeps its pose");
    let problems = |s: &Store| s.problems().into_iter().filter(|p| p.kind == "constraint").map(|p| p.node).collect::<Vec<_>>();
    assert!(problems(&store).iter().any(|n| *n == ids[1] || *n == ids[2]), "{:?}", store.problems());
    store.undo(0).unwrap().unwrap();
    assert!(problems(&store).is_empty(), "{:?}", store.problems());
    store.redo(0).unwrap().unwrap();
    assert!(!problems(&store).is_empty(), "redo shows the problem again");
}

/// An exact request the constraints would move is refused: tried on a
/// scratch copy first, it never reaches this store's document or history
/// (DS-7, after review).
#[test]
fn an_exact_request_unmet_is_refused() {
    let (mut store, comp) = sheet();
    let n = pair(&mut store, comp);
    let x = |s: &Store| s.resolved(n["q"], path("transform2d.x")).and_then(|v| v.as_f64());
    let (q, past) = (x(&store), store.history().past().len());
    let mut strict = tx("push q exactly", vec![Op::Set { id: n["q"], path: path("transform2d.x"), slot: num(900.0) }]);
    strict.exact = true;
    let errs = store.apply(strict).unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::Unsatisfiable, "{errs:?}");
    assert!(errs[0].message.contains("exact transaction not honoured"), "{errs:?}");
    assert_eq!(x(&store), q, "q is where it was");
    assert_eq!(store.history().past().len(), past, "nothing was kept in history");
    assert!(store.history().future().is_empty(), "nor left to redo");
    store.authored().check_invariants().unwrap();
    let _ = comp;
}


/// One node written several times by a refused transaction.
#[test]
fn a_node_written_several_times_then_refused() {
    let (mut store, comp) = sheet();
    let (ops, n) = draw(&store, comp, r#"<rect id="$r" x="1" y="1" w="4" h="4"/><group id="$g"/>"#);
    store.apply(tx("rect", ops)).unwrap();
    let mut ops = vec![
        Op::Set { id: n["r"], path: path("transform2d.x"), slot: num(2.0) },
        Op::Set { id: n["r"], path: path("transform2d.x"), slot: num(3.0) },
        Op::Reparent { id: n["r"], parent: Some(n["g"]), order: OrderKey::first() },
        Op::Set { id: n["r"], path: path("size.w"), slot: num(9.0) },
    ];
    let more = cycle(&store, comp, &ops);
    ops.extend(more);
    refused(&mut store, comp, "many writes to one node, and a cycle", |s| s.apply(tx("bad", ops)));
}

/// `replay` takes the stages before and inside `run`: a recorded transaction
/// whose create now collides is refused after its other ops applied.
#[test]
fn a_refused_replay_rolls_back() {
    let (mut store, comp) = sheet();
    let ids = store.authored().next_ids(2);
    store
        .apply(tx(
            "two",
            vec![
                Op::Create { id: ids[0], ty: ty("group"), parent: Some(comp), order: OrderKey::first(), facets: vec![] },
                Op::Create { id: ids[1], ty: ty("group"), parent: Some(comp), order: OrderKey::first(), facets: vec![] },
            ],
        ))
        .unwrap();
    let recorded = store.history().past().last().unwrap().clone();
    store.apply(tx("delete one", vec![Op::Delete { id: ids[1] }])).unwrap();
    // Replaying restores the deleted one, then finds the other still there.
    refused(&mut store, comp, "a replay that collides", |s| s.replay(&recorded));
}

/// A stack made in a transaction the solver refuses, then made again with
/// the same ids: its layout runs, so the store's layout roots were put back
/// with the graph (DS-3b part 2 kept them; `Saved` must hold them too).
#[test]
fn a_layout_made_in_a_refused_transaction_lays_out_when_made_again() {
    let (mut store, comp) = sheet();
    let n = pair(&mut store, comp);
    let stack = r#"<group id="$g" x="0" y="0" layout.mode="stack_x" layout.gap="10"><rect id="$a" w="10" h="10"/><rect id="$b" w="10" h="10"/></group>"#;
    let (mut ops, _) = draw(&store, comp, stack);
    ops.push(Op::Set { id: n["q"], path: path("transform2d.x"), slot: num(900.0) });
    let mut strict = tx("a stack, and q pushed exactly", ops);
    strict.exact = true;
    let errs = store.apply(strict).unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::Unsatisfiable, "{errs:?}");
    let (ops, s) = draw(&store, comp, stack);
    store.apply(tx("the stack again", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let x = |id: NodeId| store.resolved(id, path("transform2d.x")).and_then(|v| v.as_f64());
    assert_eq!((x(s["a"]), x(s["b"])), (Some(0.0), Some(20.0)), "the stack laid out");
}

/// A reorder in a stack, run by the keys it touched, in a transaction the
/// solver refuses: the stack is laid out as before it, and a reorder after
/// lays it out again (structure-in-the-graph.md §3.3).
#[test]
fn a_keyed_layout_run_in_a_refused_transaction_is_put_back() {
    let (mut store, comp) = sheet();
    let n = pair(&mut store, comp);
    let (ops, s) = draw(&store, comp, r#"<group id="$g" x="0" y="0" layout.mode="stack_x" layout.gap="10"><rect id="$a" w="10" h="10"/><rect id="$b" w="10" h="10"/></group>"#);
    store.apply(tx("a stack", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let x = |store: &Store, id: NodeId| store.resolved(id, path("transform2d.x")).and_then(|v| v.as_f64());
    assert_eq!((x(&store, s["a"]), x(&store, s["b"])), (Some(0.0), Some(20.0)));
    let first = store.tree().live(s["a"]).unwrap().order.clone();
    let swap = Op::Reorder { id: s["b"], order: OrderKey::before(&first) };
    let mut strict = tx("swap, and q pushed exactly", vec![swap.clone(), Op::Set { id: n["q"], path: path("transform2d.x"), slot: num(900.0) }]);
    strict.exact = true;
    let errs = store.apply(strict).unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::Unsatisfiable, "{errs:?}");
    assert_eq!((x(&store, s["a"]), x(&store, s["b"])), (Some(0.0), Some(20.0)), "laid out as before");
    store.apply(tx("swap", vec![swap])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!((x(&store, s["a"]), x(&store, s["b"])), (Some(20.0), Some(0.0)), "laid out again");
}

/// Two clusters, the second's distance bound to where the first puts its
/// point: a change to the first's distance moves its point, and the second
/// solves again in the same commit (council review of DS-9, Sol: the
/// clusters to solve were taken before the first's write woke the second).
#[test]
fn a_cluster_that_reads_another_s_variable_solves_after_it() {
    let (mut store, comp) = sheet();
    let a = pair(&mut store, comp);
    let b = pair(&mut store, comp);
    store.apply(tx("tie", vec![Op::Set { id: b["d"], path: path("constraint.value"), slot: bind(&format!("{}.transform2d.x", a["q"])) }])).unwrap_or_else(|e| panic!("{e:?}"));
    let x = |store: &Store, id: NodeId| store.resolved(id, path("transform2d.x")).and_then(|v| v.as_f64()).unwrap();
    let y = |store: &Store, id: NodeId| store.resolved(id, path("transform2d.y")).and_then(|v| v.as_f64()).unwrap();
    let dist = |store: &Store, p: NodeId, q: NodeId| ((x(store, q) - x(store, p)).powi(2) + (y(store, q) - y(store, p)).powi(2)).sqrt();
    store.apply(tx("wider", vec![Op::Set { id: a["d"], path: path("constraint.value"), slot: num(80.0) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!((dist(&store, a["p"], a["q"]) - 80.0).abs() < 1e-6);
    let want = x(&store, a["q"]);
    assert!((dist(&store, b["p"], b["q"]) - want).abs() < 1e-6, "the second cluster at {}, its distance {want}", dist(&store, b["p"], b["q"]));
}

/// An exact edit refused while a gesture runs leaves the gesture and its
/// preview as they were, and history untouched: the edit is tried on a
/// scratch copy first, never applied and taken back (review of DS-7).
#[test]
fn an_exact_edit_refused_mid_gesture_leaves_the_gesture() {
    let (mut store, comp) = sheet();
    let n = pair(&mut store, comp);
    let (ops, r) = draw(&store, comp, r#"<rect id="$r" x="0" y="0" w="4" h="4"/>"#);
    store.apply(tx("free", ops)).unwrap();
    let y = path("transform2d.y");
    store.begin_gesture(&[r["r"]]);
    store.preview(r["r"], y, Value::Number(77.0));
    let (version, past) = (store.authored().version(), store.history().past().len());
    let mut strict = tx("push q exactly", vec![Op::Set { id: n["q"], path: path("transform2d.x"), slot: num(900.0) }]);
    strict.exact = true;
    strict.idempotency_key = Some("k".into());
    assert_eq!(store.apply(strict.clone()).unwrap_err()[0].kind, ErrorKind::Unsatisfiable);
    assert!(store.gesture_in_progress(), "the gesture runs on");
    assert_eq!(store.resolved(r["r"], y), Some(Value::Number(77.0)), "its preview stands");
    assert_eq!((store.authored().version(), store.history().past().len()), (version, past), "nothing applied");
    // A retry with the same key is refused again, not replayed as applied.
    assert_eq!(store.apply(strict).unwrap_err()[0].kind, ErrorKind::Unsatisfiable);
}

/// The reply names what a transaction brought: a duplicate constraint, and
/// a binding that closes a cycle through a cluster (review of DS-7: they
/// were missed, the grouping having been taken before the comparison).
#[test]
fn the_reply_names_the_problems_a_transaction_brought() {
    let (mut store, comp) = sheet();
    let n = pair(&mut store, comp);
    let id = store.authored().next_id();
    let rels = store.authored().next_relation_ids(2);
    let mut dup = Constraint::default();
    dup.kind = Slot::Const(Value::Enum("distance".into()));
    dup.value = num(30.0);
    let applied = store
        .apply(tx("dup", vec![Op::Create { id, ty: ty("constraint"), parent: Some(comp), order: OrderKey::first(), facets: vec![dup.wrap()] }, constrain(rels[0], id, n["p"], "a"), constrain(rels[1], id, n["q"], "b")]))
        .unwrap();
    let told = applied.solver.map(|s| s.problems).unwrap_or_default();
    assert!(told.iter().any(|p| p.node == id && p.message.contains("already constrained by")), "{told:?}");
    store.undo(0).unwrap().unwrap();
    let applied = store.apply(tx("loop", vec![Op::Set { id: n["d"], path: path("constraint.value"), slot: bind(&format!("{}.transform2d.x + 50", n["q"])) }])).unwrap();
    let told = applied.solver.map(|s| s.problems).unwrap_or_default();
    assert!(told.iter().any(|p| p.node == n["d"] && p.message.contains("feeds the same cluster")), "{told:?}");
    // An unrelated edit brings nothing, and tells nothing of the cycle again.
    let applied = store.apply(tx("rename", vec![Op::Set { id: comp, path: path("name.text"), slot: Slot::Const(Value::Str("renamed".into())) }])).unwrap();
    assert!(applied.solver.is_none_or(|s| s.problems.is_empty()));
}

/// A disabled constraint doesn't hide an enabled one over the same
/// operands: only what the solver would use is a duplicate (review of DS-7).
#[test]
fn a_disabled_constraint_hides_no_duplicate() {
    let (mut store, comp) = sheet();
    let n = pair(&mut store, comp);
    store.apply(tx("off", vec![Op::Set { id: n["d"], path: path("constraint.enabled"), slot: Slot::Const(Value::Bool(false)) }])).unwrap();
    let id = store.authored().next_id();
    let rels = store.authored().next_relation_ids(2);
    let mut again = Constraint::default();
    again.kind = Slot::Const(Value::Enum("distance".into()));
    again.value = num(30.0);
    store.apply(tx("again", vec![Op::Create { id, ty: ty("constraint"), parent: Some(comp), order: OrderKey::first(), facets: vec![again.wrap()] }, constrain(rels[0], id, n["p"], "a"), constrain(rels[1], id, n["q"], "b")])).unwrap();
    let x = store.resolved(n["q"], path("transform2d.x")).and_then(|v| v.as_f64()).unwrap();
    assert!((x - 30.0).abs() < 1e-6, "the enabled one holds: q at {x}");
    assert!(store.problems().iter().all(|p| p.kind != "constraint"), "{:?}", store.problems());
}

/// Two clusters that read each other's variables through bindings are a
/// ring: both fail and are not solved; a third that only reads one of them
/// is solved after it as usual (DS-7, strongly connected components).
#[test]
fn a_ring_of_clusters_fails_and_what_reads_it_still_solves() {
    let (mut store, comp) = sheet();
    let a = pair(&mut store, comp);
    let b = pair(&mut store, comp);
    let c = pair(&mut store, comp);
    store
        .apply(tx(
            "ring",
            vec![
                Op::Set { id: a["d"], path: path("constraint.value"), slot: bind(&format!("{}.transform2d.x", b["q"])) },
                Op::Set { id: b["d"], path: path("constraint.value"), slot: bind(&format!("{}.transform2d.x", a["q"])) },
                Op::Set { id: c["d"], path: path("constraint.value"), slot: bind(&format!("{}.transform2d.x + 10", a["q"])) },
            ],
        ))
        .unwrap();
    let ring: Vec<_> = store.problems().into_iter().filter(|p| p.kind == "constraint" && p.message.contains("depend on each other")).map(|p| p.node).collect();
    assert!(ring.contains(&a["d"]) && ring.contains(&b["d"]) && !ring.contains(&c["d"]), "{:?}", store.problems());
    let x = |id: NodeId| store.resolved(id, path("transform2d.x")).and_then(|v| v.as_f64()).unwrap();
    assert!((x(c["q"]) - (x(a["q"]) + 10.0)).abs() < 1e-6, "the cluster that reads the ring is solved: {} against {}", x(c["q"]), x(a["q"]));
}

/// A live solve that fails is posed: dragging an independent cluster
/// doesn't try it again each frame (review of DS-7, Sol).
#[test]
fn a_failed_live_solve_is_not_retried_by_an_unrelated_drag() {
    let (mut store, comp) = sheet();
    let a = pair(&mut store, comp);
    let b = pair(&mut store, comp);
    let live = store.register_producer("live");
    // A negative distance can't hold.
    store.publish(live, BTreeMap::from([(Leaf { node: a["d"], path: path("constraint.value") }, Value::Number(-10.0))]));
    let h = store.begin_drag(b["q"], [0.0, 0.0]);
    let first = store.drag_to(&h, [45.0, 5.0]);
    let second = store.drag_to(&h, [46.0, 6.0]);
    assert!(first.converged || second.converged, "{first:?}");
    assert!(second.converged, "the failed cluster is not tried again: {second:?}");
    store.end_drag(&h);
}
