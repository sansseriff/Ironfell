//! The session tier (plans/specs/panels-and-session.md): the selection as a
//! `selection` node with `selects` relations, in its own id range, never
//! saved, never in undo, leaving the document's version and ids alone.

use iron_document::facet::{Size, Transform2d};
use iron_document::*;
use std::collections::BTreeMap;

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn rect(store: &Store, n: usize) -> (Vec<NodeId>, Vec<Op>) {
    let ids = store.document().next_ids(n);
    let mut key = OrderKey::first();
    let ops = ids
        .iter()
        .map(|&id| {
            let op = Op::Create { id, ty: TypeId::lookup("rect").unwrap(), parent: None, order: key.clone(), facets: vec![Transform2d::default().wrap(), Size::default().wrap()] };
            key = OrderKey::after(&key);
            op
        })
        .collect();
    (ids, ops)
}

fn reached(panel: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([("panel".to_owned(), Value::Str(panel.into()))])
}

#[test]
fn the_selection_lives_in_the_session_and_nowhere_else() {
    let mut store = Store::new();
    let (ids, ops) = rect(&store, 2);
    store.apply(tx("two rects", ops)).unwrap();
    let saved = store.save();
    let version = store.document().version();
    let next = store.document().next_id();

    store.set_selection(&[(ids[1], reached("viewer")), (ids[0], reached("viewer"))]).unwrap();
    // In pick order, with how each was reached.
    let sel = store.selection();
    assert_eq!(sel.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![ids[1], ids[0]]);
    assert_eq!(sel[0].1.get("panel"), Some(&Value::Str("viewer".into())));
    // Not content: the version, the next content id, the file and the views
    // are as they were, and there is nothing new to undo.
    assert_eq!(store.document().version(), version);
    assert_eq!(store.document().next_id(), next);
    assert_eq!(store.save(), saved);
    let view = store.view(&ViewQuery { expand: Vec::new(), scope: None, fidelity: Fidelity::Full, depth: None, relation: ViewRelation::Tree }).unwrap();
    assert!(!view.contains("selection"), "{view}");
    assert!(store.session_version() > 0);

    // A content transaction keeps the selection; undoing it does too.
    let (more, ops) = rect(&store, 1);
    assert_eq!(more[0], next, "session ids did not take content ids");
    store.apply(tx("third", ops)).unwrap();
    store.undo(0).unwrap().unwrap();
    assert_eq!(store.selection().len(), 2);

    // Deleting a selected node drops it from the selection.
    store.apply(tx("delete", vec![Op::Delete { id: ids[0] }])).unwrap();
    assert_eq!(store.selection().iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![ids[1]]);

    // Setting the same selection again is no change.
    let before = store.session_version();
    store.set_selection(&store.selection()).unwrap();
    assert_eq!(store.session_version(), before);
    store.set_selection(&[]).unwrap();
    assert!(store.selection().is_empty());
}

#[test]
fn content_ops_are_refused_as_session_ops() {
    let mut store = Store::new();
    let (ids, ops) = rect(&store, 1);
    store.apply(tx("rect", ops)).unwrap();
    let err = store.apply_session(vec![Op::Delete { id: ids[0] }]).unwrap_err();
    assert_eq!(err.kind, ErrorKind::BadId);
    assert!(store.document().is_live(ids[0]));
}
