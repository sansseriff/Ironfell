//! The harness's own rules (reactive-engine handoff §4.4, stage 1): a
//! difference no intended entry explains fails; a normaliser relaxes only the
//! respect it names; a divergence ends comparison for its script and no
//! other.
#![cfg(feature = "differential")]

use iron_document::differential::intended::{Intended, Relax, with_intended};
use iron_document::differential::Snapshot;
use iron_document::*;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn group(store: &Store) -> (NodeId, Op) {
    let id = store.authored().next_id();
    (id, Op::Create { id, ty: TypeId::lookup("group").unwrap(), parent: None, order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap()] })
}

/// A published value in the subject only: the stores now differ in one
/// signal on `node`.
fn differ(store: &mut Store, node: NodeId, field: &str) {
    let s = store.subject_mut_for_test();
    let p = s.register_producer("harness test");
    let leaf = Leaf { node, path: format!("transform2d.{field}").parse().unwrap() };
    s.publish(p, BTreeMap::from([(leaf, Value::Number(7.0))]));
}

fn fails(f: impl FnOnce()) -> bool {
    catch_unwind(AssertUnwindSafe(f)).is_err()
}

/// Drops the harness test's producer from both snapshots, and what it moved
/// through `transform2d.x` (values, changes, the view, placements). A
/// difference in `y` still shows in values and signals.
fn drop_x(s: &mut Snapshot) {
    let x = |l: &Leaf| l.path.field == "x";
    s.signals.retain(|(l, _)| !x(l));
    s.values.retain(|(l, _, _)| !x(l));
    s.other.producers.retain(|(_, n)| n != "harness test");
    s.pending.changes.values.retain(|l| !x(l));
    s.pending.changes.geometry = Default::default();
    // What the published x moves: the view's attribute and the placement.
    s.view = Ok(String::new());
    s.other.geometry.clear();
}

static NORMALISE_X: [Intended; 1] = [Intended { name: "harness test: x", relax: Relax::Normalise(drop_x), test: "tests/differential_harness.rs" }];

fn accepted_by_subject_only(method: &str, subject: &str, reference: &str) -> bool {
    method == "apply" && subject.starts_with("Ok") && reference.starts_with("Err")
}

static DIVERGE: [Intended; 1] = [Intended { name: "harness test: accepted by one", relax: Relax::Diverge(accepted_by_subject_only), test: "tests/differential_harness.rs" }];

#[test]
fn an_unexplained_difference_fails() {
    let mut store = Store::new();
    let (g, op) = group(&store);
    store.apply(tx("group", vec![op])).unwrap();
    differ(&mut store, g, "x");
    assert!(fails(|| store.set_time(1.0)));
}

#[test]
fn a_normaliser_relaxes_only_what_it_names() {
    with_intended(&NORMALISE_X, || {
        let mut store = Store::new();
        let (g, op) = group(&store);
        store.apply(tx("group", vec![op])).unwrap();
        differ(&mut store, g, "x");
        assert!(!fails(|| store.set_time(1.0)), "the normaliser explains a difference in x");
        let mut store = Store::new();
        let (g, op) = group(&store);
        store.apply(tx("group", vec![op])).unwrap();
        differ(&mut store, g, "y");
        assert!(fails(|| store.set_time(1.0)), "but not one in y");
    });
}

#[test]
fn a_divergence_ends_comparison_for_its_script_only() {
    with_intended(&DIVERGE, || {
        let mut store = Store::new();
        let (g, op) = group(&store);
        // The subject alone has the group, so a set on it is accepted there
        // and refused by the reference.
        store.subject_mut_for_test().apply(tx("group", vec![op])).unwrap();
        store.apply(tx("move", vec![Op::Set { id: g, path: "transform2d.x".parse().unwrap(), slot: Slot::Const(Value::Number(3.0)) }])).unwrap();
        assert_eq!(store.diverged(), Some("harness test: accepted by one"));
        store.set_time(2.0);
        // A new script is compared again.
        let mut store = Store::new();
        let (g, op) = group(&store);
        store.apply(tx("group", vec![op])).unwrap();
        differ(&mut store, g, "x");
        assert!(fails(|| store.set_time(1.0)));
    });
    assert!(iron_document::differential::intended::counts().iter().any(|(n, _, d)| *n == "harness test: accepted by one" && *d >= 1));
}

#[test]
fn a_divergence_must_match_its_case() {
    with_intended(&DIVERGE, || {
        let mut store = Store::new();
        let (g, op) = group(&store);
        // The reference alone would accept: not the entry's case.
        store.apply(tx("group", vec![op])).unwrap();
        let s = store.subject_mut_for_test();
        s.apply(tx("delete", vec![Op::Delete { id: g }])).unwrap();
        assert!(fails(|| {
            let _ = store.apply(tx("move", vec![Op::Set { id: g, path: "transform2d.x".parse().unwrap(), slot: Slot::Const(Value::Number(3.0)) }]));
        }));
    });
}
