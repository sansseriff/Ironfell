//! Calls that once returned an error and left the store other than it was
//! (reactive-engine handoff §4.1, task 3). Phase 0 found four; phase 2a fixed
//! expansion problems and phase 2b's stage 1 the rest, in both engines. Each
//! test now asserts the failed call left the store as it was.

use iron_document::facet::{Composition, Name};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
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

/// A session op list that fails part-way is rolled back by the document's
/// journal: no tombstone of the created session node, no moved counter.
#[test]
fn a_failed_session_op_list_leaves_no_trace() {
    let (mut store, comp) = sheet();
    let before = store.tree().nodes().count();
    let id = store.authored().next_session_ids(1)[0];
    let err = store
        .apply_session(vec![
            Op::Create { id, ty: ty("group"), parent: None, order: OrderKey::first(), facets: vec![] },
            Op::Set { id: comp, path: "name.text".parse().unwrap(), slot: Slot::Const(Value::Str("not session".into())) },
        ])
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::BadId);
    assert!(!store.tree().is_live(id));
    assert_eq!(store.tree().nodes().count(), before, "no tombstone");
    assert_eq!(store.authored().next_session_ids(1)[0], id, "the session counter did not move");
}

/// Seventeen nested repeats of one item, deeper than expansion reports
/// quietly, settled and expanded: the store reports a `depth` problem.
fn deep_nest(store: &mut Store, comp: NodeId) -> BTreeMap<String, NodeId> {
    // Seventeen nested repeats of one item: deeper than expansion reports.
    let mut nest = String::from(r#"<group id="$top" repeat.count="1">"#);
    for _ in 0..16 {
        nest.push_str(r#"<group repeat.count="1">"#);
    }
    nest.push_str(r#"<rect w="1" h="1"/>"#);
    for _ in 0..17 {
        nest.push_str("</group>");
    }
    let (ops, n) = draw(store, comp, &nest);
    store.apply(tx("nest", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    // Settling makes items, then a structural transaction expands them.
    let (ops, _) = draw(store, comp, r#"<rect w="2" h="2"/>"#);
    store.apply(tx("more", ops)).unwrap();
    assert!(store.expansion_problems().iter().any(|p| p.kind == "depth"), "{:?}", store.expansion_problems());
    n
}

/// A transaction that expands and is then refused (here by a binding cycle
/// found after expansion) leaves the store's expansion problems as they
/// were: `run` hands them back only on success (reactive-engine handoff
/// §4.3, task 3; until phase 2a it stored them before its checks). Here the
/// refused transaction deletes the nest, whose problem it would have taken
/// away.
#[test]
fn a_refused_transaction_leaves_expansion_problems_alone() {
    let (mut store, comp) = sheet();
    let n = deep_nest(&mut store, comp);
    let before = store.expansion_problems().to_vec();
    let (mut ops, _) = draw(&store, comp, r#"<let id="$a" value="{= $b}"/><let id="$b" value="{= $a}"/>"#);
    ops.push(Op::Delete { id: n["top"] });
    let errs = store.apply(tx("delete and cycle", ops)).unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::Cycle, "{errs:?}");
    assert!(store.tree().is_live(n["top"]), "the nest is still there");
    assert_eq!(store.expansion_problems(), &before[..]);
}

/// A transaction that does not expand keeps the store's expansion problems:
/// `run` reports none rather than an empty list.
#[test]
fn a_transaction_that_does_not_expand_leaves_expansion_problems_alone() {
    let (mut store, comp) = sheet();
    deep_nest(&mut store, comp);
    let before = store.expansion_problems().to_vec();
    store.apply(tx("rename", vec![Op::Set { id: comp, path: "name.text".parse().unwrap(), slot: Slot::Const(Value::Str("renamed".into())) }])).unwrap();
    assert_eq!(store.expansion_problems(), &before[..]);
}

/// An action that emits an event and then fails on a later write queues
/// nothing: events are queued once the writes apply.
#[test]
fn a_failed_action_queues_no_emit() {
    let (mut store, comp) = sheet();
    let (ops, n) = draw(&store, comp, r#"<let id="$n" value="3"/><group name="button"><rect id="$bar" x="0" y="0" w="80" h="24"/><on event="click" emit="pressed" target="$n" verb="toggle"/></group>"#);
    store.apply(tx("button", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let err = store.fire(n["bar"], "click", Actor::Human, 0).unwrap_err();
    assert!(err.contains("toggle needs a yes/no value"), "{err}");
    assert!(store.take_emitted().is_empty(), "the failed action's event is not queued");
}

/// An action whose document write fails writes no session state: the
/// session writes wait for the transaction (Opus review of stage 2).
#[test]
fn a_refused_action_writes_no_session_state() {
    let (mut store, comp) = sheet();
    let (ops, n) = draw(&store, comp, r#"<let id="$hot" name="hot" keep="session" value="false"/><let id="$n" value="3"/>
        <group id="$g" name="button"><rect id="$bar" x="0" y="0" w="80" h="24"/>
          <on event="click" target="$hot" to="true"/>
          <on event="click" target="$n" verb="toggle"/>
        </group>"#);
    store.apply(tx("button", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let hot: LeafPath = "let.value".parse().unwrap();
    assert_eq!(store.resolved(n["hot"], hot), Some(Value::Bool(false)));
    let err = store.fire(n["bar"], "click", Actor::Human, 0).unwrap_err();
    assert!(err.contains("toggle needs a yes/no value"), "{err}");
    assert_eq!(store.resolved(n["hot"], hot), Some(Value::Bool(false)), "the failed action wrote no session state");
}
