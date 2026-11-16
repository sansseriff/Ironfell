//! The old engine's failures of atomicity (reactive-engine handoff §4.1,
//! task 3): calls that return an error and leave the store other than it
//! was. Each test shows one, as it is today, and is listed in
//! `src/differential/intended.rs` (`NOT_ATOMIC`). When the new engine takes
//! a call over and meets atomicity for it (spec §11), the entry goes and
//! its test is turned round to assert the store is unchanged.

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
    let comp = store.document().next_id();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("sheet".into()));
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] }])).unwrap();
    (store, comp)
}

fn draw(store: &Store, comp: NodeId, markup: &str) -> (Vec<Op>, BTreeMap<String, NodeId>) {
    let built = markup::ops(markup, Some(comp), OrderKey::after(&OrderKey::first()), store.document().next_id()).unwrap_or_else(|e| panic!("{e}"));
    (built.ops, built.named)
}

/// A session op list that fails part-way is rolled back by applying the
/// inverses of the ops before it: a created session node comes back as a
/// tombstone, and the session id counter has moved (`lib.rs` `apply_session`,
/// `document.rs`).
#[test]
fn a_failed_session_op_list_leaves_a_tombstone() {
    let (mut store, comp) = sheet();
    let before = store.document().nodes().count();
    let id = store.document().next_session_ids(1)[0];
    let err = store
        .apply_session(vec![
            Op::Create { id, ty: ty("group"), parent: None, order: OrderKey::first(), facets: vec![] },
            Op::Set { id: comp, path: "name.text".parse().unwrap(), slot: Slot::Const(Value::Str("not session".into())) },
        ])
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::BadId);
    assert!(!store.document().is_live(id));
    assert_eq!(store.document().nodes().count(), before + 1, "the rolled-back create is kept as a tombstone");
    assert_ne!(store.document().next_session_ids(1)[0], id, "the session counter moved");
}

/// A transaction that expands and is then refused (here by a binding cycle
/// found after expansion) keeps what that expansion found: `run` sets
/// `self.expansion_problems` before the checks. Here the refused
/// transaction deletes a nest too deep to expand quietly, so the problem it
/// reported goes though the nest stays.
#[test]
fn a_refused_transaction_keeps_its_expansion_problems() {
    let (mut store, comp) = sheet();
    // Seventeen nested repeats of one item: deeper than expansion reports.
    let mut nest = String::from(r#"<group id="$top" repeat.count="1">"#);
    for _ in 0..16 {
        nest.push_str(r#"<group repeat.count="1">"#);
    }
    nest.push_str(r#"<rect w="1" h="1"/>"#);
    for _ in 0..17 {
        nest.push_str("</group>");
    }
    let (ops, n) = draw(&store, comp, &nest);
    store.apply(tx("nest", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    // Settling makes items, then a structural transaction expands them.
    let (ops, _) = draw(&store, comp, r#"<rect w="2" h="2"/>"#);
    store.apply(tx("more", ops)).unwrap();
    let before = store.expansion_problems().to_vec();
    assert!(before.iter().any(|p| p.kind == "depth"), "{before:?}");
    let (mut ops, _) = draw(&store, comp, r#"<let id="$a" value="{= $b}"/><let id="$b" value="{= $a}"/>"#);
    ops.push(Op::Delete { id: n["top"] });
    let errs = store.apply(tx("delete and cycle", ops)).unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::Cycle, "{errs:?}");
    assert!(store.document().is_live(n["top"]), "the nest is still there");
    assert_ne!(store.expansion_problems(), &before[..], "but the refused transaction's expansion replaced its problems");
}

/// A transaction refused after it applied (by `post_apply`, here a binding
/// that reads and feeds one constraint cluster) is undone by putting the old
/// document back and rebuilding the graph, which reports the leaf it
/// touched as changed (`lib.rs` `apply_now`).
#[test]
fn a_transaction_refused_by_the_solver_leaves_a_change_pending() {
    let (mut store, comp) = sheet();
    let markup = r#"<ellipse id="$p" x="0" y="0" rx="5" ry="5"/><ellipse id="$q" x="40" y="0" rx="5" ry="5"/><constraint id="$fix" kind="fix" value="0"/><constraint id="$d" kind="distance" value="50"/>"#;
    let (mut ops, n) = draw(&store, comp, markup);
    let rels = store.document().next_relation_ids(3);
    let link = |id: RelationId, from: NodeId, to: NodeId, role: &str| {
        let mut data = BTreeMap::new();
        data.insert("role".to_owned(), Value::Str(role.into()));
        data.insert("anchor".to_owned(), Value::Vec2([0.0, 0.0]));
        Op::Link { rel: Relation { id, from, to, rel: RelKind::Constrains, data } }
    };
    ops.push(link(rels[0], n["fix"], n["p"], "a"));
    ops.push(link(rels[1], n["d"], n["p"], "a"));
    ops.push(link(rels[2], n["d"], n["q"], "b"));
    store.apply(tx("pair", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    store.drain_changes();
    assert!(!store.has_changes());
    let errs = store
        .apply(tx("loop", vec![Op::Set { id: n["d"], path: "constraint.value".parse().unwrap(), slot: Slot::Bound(Expr::parse(&format!("{}.transform2d.x + 50", n["q"])).unwrap()) }]))
        .unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::Cycle, "{errs:?}");
    assert!(store.has_changes(), "the refused transaction left a change to drain");
}

/// An action that emits an event and then fails on a later write has
/// already queued the event (`lib.rs` `action_writes`).
#[test]
fn a_failed_action_has_already_queued_its_emit() {
    let (mut store, comp) = sheet();
    let (ops, n) = draw(&store, comp, r#"<let id="$n" value="3"/><group name="button"><rect id="$bar" x="0" y="0" w="80" h="24"/><on event="click" emit="pressed" target="$n" verb="toggle"/></group>"#);
    store.apply(tx("button", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    let err = store.fire(n["bar"], "click", Actor::Human, 0).unwrap_err();
    assert!(err.contains("toggle needs a yes/no value"), "{err}");
    assert_eq!(store.take_emitted().len(), 1, "the failed action's event is queued");
}
