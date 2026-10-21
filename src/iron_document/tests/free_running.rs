//! Free-running compositions (free-running.md §2): the run mode, bubbling
//! through embeds, and `on_demand` stopping it.

use iron_document::composition::{self, RunMode};
use iron_document::standard;
use iron_document::facet::{Composition, Embed, Name, Transform2d};
use iron_document::*;
use std::collections::BTreeSet;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn comp(name: &str, dims: &str) -> Vec<Facet> {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(name.into()));
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum(dims.into()));
    vec![n.wrap(), c.wrap()]
}

fn embed_of(c: NodeId) -> Vec<Facet> {
    let mut e = Embed::default();
    e.source = Slot::Const(Value::Ref(c));
    vec![e.wrap(), Transform2d::default().wrap()]
}

/// outer (2d) shows middle (2d) shows gas (3d); `lone` (2d) shows nothing.
struct Nest {
    store: Store,
    outer: NodeId,
    middle: NodeId,
    gas: NodeId,
    lone: NodeId,
}

fn nest() -> Nest {
    let mut store = Store::new();
    let ids = store.document().next_ids(6);
    let (outer, middle, gas, lone, e1, e2) = (ids[0], ids[1], ids[2], ids[3], ids[4], ids[5]);
    let k = OrderKey::first();
    let k1 = OrderKey::after(&k);
    let k2 = OrderKey::after(&k1);
    let k3 = OrderKey::after(&k2);
    store
        .apply(tx(
            "setup",
            vec![
                Op::Create { id: outer, ty: ty("composition"), parent: None, order: k.clone(), facets: comp("outer", "2d") },
                Op::Create { id: middle, ty: ty("composition"), parent: None, order: k1, facets: comp("middle", "2d") },
                Op::Create { id: gas, ty: ty("composition"), parent: None, order: k2, facets: comp("gas", "3d") },
                Op::Create { id: lone, ty: ty("composition"), parent: None, order: k3, facets: comp("lone", "2d") },
                Op::Create { id: e1, ty: ty("embed"), parent: Some(outer), order: k.clone(), facets: embed_of(middle) },
                Op::Create { id: e2, ty: ty("embed"), parent: Some(middle), order: k, facets: embed_of(gas) },
            ],
        ))
        .expect("setup applies");
    Nest { store, outer, middle, gas, lone }
}

fn set_run(store: &mut Store, c: NodeId, mode: &str) {
    store
        .apply(tx("run", vec![Op::Set { id: c, path: "composition.run".parse().unwrap(), slot: Slot::Const(Value::Enum(mode.into())) }]))
        .expect("run mode applies");
}

#[test]
fn default_is_auto_and_nothing_is_live_without_a_source() {
    let n = nest();
    let doc = n.store.document();
    assert_eq!(composition::run_mode(doc, n.outer), RunMode::Auto);
    let r = composition::reach(doc, &[n.outer], &BTreeSet::new());
    assert!(!r.live);
    assert!(r.running.is_empty());
}

#[test]
fn a_source_bubbles_out_through_nested_embeds() {
    let n = nest();
    let sources = BTreeSet::from([n.gas]);
    let doc = n.store.document();
    let r = composition::reach(doc, &[n.outer], &sources);
    assert!(r.live, "outer shows middle shows gas");
    assert_eq!(r.running, sources);
    assert!(composition::reach(doc, &[n.middle], &sources).live);
    assert!(composition::reach(doc, &[n.gas], &sources).live);
    assert!(!composition::reach(doc, &[n.lone], &sources).live, "lone shows nothing live");
}

#[test]
fn on_demand_stops_bubbling_and_pauses_what_it_hides() {
    let mut n = nest();
    set_run(&mut n.store, n.middle, "on_demand");
    let sources = BTreeSet::from([n.gas]);
    let doc = n.store.document();
    let r = composition::reach(doc, &[n.outer], &sources);
    assert!(!r.live, "middle is on demand, so the gas does not reach outer");
    assert!(r.running.is_empty(), "seen only through middle, the gas pauses");
    assert!(!composition::reach(doc, &[n.middle], &sources).live, "on demand freezes its own view");
    assert!(composition::reach(doc, &[n.gas], &sources).live, "the gas's own view still runs");
}

#[test]
fn on_demand_at_the_root_freezes_the_view() {
    let mut n = nest();
    set_run(&mut n.store, n.outer, "on_demand");
    let sources = BTreeSet::from([n.gas]);
    let r = composition::reach(n.store.document(), &[n.outer], &sources);
    assert!(!r.live);
    assert!(r.running.is_empty());
}

#[test]
fn always_runs_without_a_source_but_not_behind_on_demand() {
    let mut n = nest();
    set_run(&mut n.store, n.middle, "always");
    let none = BTreeSet::new();
    let r = composition::reach(n.store.document(), &[n.outer], &none);
    assert!(r.live, "middle is always, and outer shows it");
    assert!(r.running.is_empty(), "always runs the view, not a producer");
    set_run(&mut n.store, n.outer, "on_demand");
    assert!(!composition::reach(n.store.document(), &[n.outer], &none).live);
}

#[test]
fn a_view_with_several_roots_is_live_when_any_is() {
    let n = nest();
    let sources = BTreeSet::from([n.gas]);
    assert!(composition::reach(n.store.document(), &[n.lone, n.gas], &sources).live);
}

#[test]
fn the_run_mode_is_undoable_and_saved() {
    let mut n = nest();
    set_run(&mut n.store, n.middle, "on_demand");
    let saved = n.store.save();
    let loaded = Store::load(&saved).expect("loads");
    assert_eq!(composition::run_mode(loaded.document(), n.middle), RunMode::OnDemand);
    n.store.undo(1).expect("something to undo").expect("undo applies");
    assert_eq!(composition::run_mode(n.store.document(), n.middle), RunMode::Auto);
}

#[test]
fn the_stats_composition_is_session_state_its_lines_take_signals() {
    let mut store = Store::new();
    let first = store.document().next_session_ids(1)[0];
    let ops = standard::stats_ops(first, OrderKey::first());
    store.apply_session(ops).expect("stats composition applies as session ops");
    let doc = store.document();
    let comp = composition::compositions(doc).into_iter().find(|&c| c.is_session()).expect("a session composition");
    let line = |name: &str| doc.children(comp).iter().copied().find(|&k| doc.get::<Name>(k).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).expect("line");
    let fps = line("fps");
    assert_eq!(standard::STATS_LINES.len(), 4);
    assert!(!store.save().contains("JetBrains Mono"), "session nodes are never saved");
    let producer = store.register_producer("stats");
    store.publish(producer, std::collections::BTreeMap::from([(Leaf { node: fps, path: "text.content".parse().unwrap() }, Value::Str("FPS 60.0".into()))]));
    assert!(store.signal_errors().is_empty(), "{:?}", store.signal_errors());
    assert_eq!(store.signals().len(), 1);
}
