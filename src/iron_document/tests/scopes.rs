//! plans/specs/scopes.md phases 1–2: a document names its scope, only the
//! current schema loads, and a snapshot plus the logged transactions after it
//! replays to the live document, through undo and redo, with nothing to undo
//! afterwards.

use iron_document::facet::{Radius, Transform2d};
use iron_document::*;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput {
        label: label.into(),
        actor: Actor::Human,
        ts: 0,
        idempotency_key: None,
        ops,
        exact: false,
    }
}

fn set_x(id: NodeId, x: f64) -> Op {
    Op::Set {
        id,
        path: "transform2d.x".parse().unwrap(),
        slot: Slot::Const(Value::Number(x)),
    }
}

fn ellipse(store: &mut Store) -> NodeId {
    let id = store.document().next_id();
    store
        .apply(tx(
            "add",
            vec![Op::Create {
                id,
                ty: ty("ellipse"),
                parent: None,
                order: OrderKey::first(),
                facets: vec![Transform2d::default().wrap(), Radius::default().wrap()],
            }],
        ))
        .unwrap();
    id
}

const ID: &str = "01JBXQ7Z3M4T8V2K9N6P5R0SWA";

#[test]
fn scope_ids_round_trip_as_26_crockford_characters() {
    let id: ScopeId = ID.parse().unwrap();
    assert_eq!(id.to_string(), ID);
    assert_eq!(id.to_string().len(), 26);
    // Lower case and the Crockford look-alikes read as the same id.
    assert_eq!(ID.to_lowercase().parse::<ScopeId>().unwrap(), id);
    assert_eq!("O1JBXQ7Z3M4T8V2K9N6P5R0SWA".parse::<ScopeId>().unwrap(), id);
    // Extremes, and what is not an id.
    for raw in [0u128, 1, u128::MAX] {
        let s = ScopeId::from_u128(raw);
        assert_eq!(s.to_string().parse::<ScopeId>().unwrap(), s);
    }
    assert!("8ZZZZZZZZZZZZZZZZZZZZZZZZZ".parse::<ScopeId>().is_err(), "131 bits");
    assert!("01JBXQ7Z3M4T8V2K9N6P5R0SW".parse::<ScopeId>().is_err(), "25 chars");
    assert!("01JBXQ7Z3M4T8V2K9N6P5R0SWU".parse::<ScopeId>().is_err(), "U is excluded");
}

#[test]
fn the_scope_is_saved_in_the_header_and_is_not_an_op() {
    let mut store = Store::new();
    ellipse(&mut store);
    let before = store.document().version();
    let undoable = store.history().past().len();
    store.set_scope(Some(ID.parse().unwrap()));
    assert_eq!(store.document().version(), before, "naming a scope is not a change");
    assert_eq!(store.history().past().len(), undoable);

    let text = store.save();
    assert!(text.starts_with(&format!("{{\"schema\": 12, \"scope\": \"{ID}\",")), "{text}");
    let loaded = Store::load(&text).unwrap();
    assert_eq!(loaded.scope(), store.scope());
    assert_eq!(loaded.save(), text);
}

#[test]
fn only_the_current_schema_loads() {
    let mut store = Store::new();
    ellipse(&mut store);
    let text = store.save();
    // A document need not name a scope (an unsaved demo does not).
    assert!(text.starts_with("{\"schema\": 12, \"next_ids\""));
    assert_eq!(Store::load(&text).unwrap().scope(), None);
    // No migrations: an older schema is refused, as is a bad scope.
    let old = text.replacen("\"schema\": 12", "\"schema\": 11", 1);
    assert!(Store::load(&old).err().is_some_and(|e| e.to_string().contains("reads only 12")));
    let bad = text.replacen("\"schema\": 12,", "\"schema\": 12, \"scope\": \"nope\",", 1);
    assert!(Store::load(&bad).is_err());
}

#[test]
fn snapshot_plus_the_log_after_it_replays_to_the_live_document() {
    let mut store = Store::new();
    store.set_scope(Some(ID.parse().unwrap()));
    let a = ellipse(&mut store);
    store.apply(tx("move", vec![set_x(a, 5.0)])).unwrap();

    // The snapshot a scope writes: then everything after it goes to the log.
    let snapshot = store.save();
    let at = store.document().version();

    let b = ellipse(&mut store);
    store.apply(tx("move b", vec![set_x(b, 7.0)])).unwrap();
    store.apply(tx("move a", vec![set_x(a, 9.0)])).unwrap();
    store.undo(1).unwrap().unwrap();
    store.undo(2).unwrap().unwrap();
    store.redo(3).unwrap().unwrap();
    store.apply(tx("delete a", vec![Op::Delete { id: a }])).unwrap();
    store.undo(4).unwrap().unwrap();

    // What the feed hands the durable log, through its text form.
    let lines: Vec<String> = store
        .log_since(at)
        .expect("the horizon still reaches the snapshot")
        .into_iter()
        .map(|t| serde_json::to_string(t).unwrap())
        .collect();
    assert_eq!(lines.len() as u64, store.document().version().0 - at.0);

    let mut opened = Store::load(&snapshot).unwrap();
    for line in &lines {
        let t: Transaction = serde_json::from_str(line).unwrap();
        let applied = opened.replay(&t).unwrap();
        assert_eq!(applied.version, t.version, "{}", t.label);
    }
    assert_eq!(opened.save(), store.save());
    opened.document().check_invariants().unwrap();

    // A reopened scope has nothing to undo.
    opened.clear_history();
    assert!(!opened.history().can_undo());
    assert!(!opened.history().can_redo());
    assert_eq!(opened.history().log().count(), 0);
    assert_eq!(opened.log_since(opened.document().version()).unwrap().len(), 0);

    // And it carries on from there: a new edit lands at the next version,
    // and the feed resumes from it.
    let v = opened.document().version();
    opened.apply(tx("move b again", vec![set_x(b, 1.0)])).unwrap();
    let next = opened.log_since(v).unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].version, v.next());
    // Before the clear, nothing is there to feed: the feed must snapshot.
    assert!(opened.log_since(at).is_none());
}

#[test]
fn the_feed_reports_a_gap_once_the_horizon_drops_transactions() {
    let mut store = Store::new();
    let a = ellipse(&mut store);
    let at = store.document().version();
    for i in 0..250 {
        store.apply(tx("move", vec![set_x(a, i as f64)])).unwrap();
    }
    assert!(store.log_since(at).is_none(), "200 kept, 250 written");
    let recent = Version(store.document().version().0 - 10);
    assert_eq!(store.log_since(recent).unwrap().len(), 10);
}
