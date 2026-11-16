//! A probe (reactive-engine handoff §4.1, task 1; spec D5, A10): how the
//! process-wide id registries grow when a repeat keyed by value churns. Its
//! values move through 10,000 distinct numbers; every new key and every
//! item's copy path is interned and never freed. A plot's ticks are keyed by
//! index today, so panning a plot does not show this. Ignored; run with
//!   cargo test -p iron_document --release --target aarch64-apple-darwin --test id_growth -- --ignored --nocapture

use iron_document::facet::{Composition, Name};
use iron_document::*;

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

#[test]
#[ignore]
fn value_keyed_items_grow_the_registries() {
    let mut store = Store::new();
    let comp = store.document().next_id();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("sheet".into()));
    let ty = TypeId::lookup("composition").unwrap();
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty, parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] }])).unwrap();
    // Eight ticks keyed by their value, as tests/reactivity.rs's
    // `ticks_keyed_by_value_keep_the_labels_that_stay`.
    let markup = r#"<let id="$lo" value="0"/><group id="$g" repeat.each="{= range($lo, $lo + 30, 4)}" repeat.key="{= t => t}"><text name="tick" x="{= $g.repeat.item}" y="0" content="{= fmt($g.repeat.item)}"/></group>"#;
    let built = markup::ops(markup, Some(comp), OrderKey::first(), store.document().next_id()).unwrap();
    store.apply(tx("draw", built.ops)).unwrap();
    let lo = built.named["lo"];
    let before = id::registry_sizes();
    // Steps of 4/9 never land on an earlier value of the range's grid, so
    // each step brings new keys.
    for i in 1..=10_000 {
        let v = i as f64 * 4.0 / 9.0;
        store.apply(tx("slide", vec![Op::Set { id: lo, path: "let.value".parse().unwrap(), slot: Slot::Const(Value::Number(v)) }])).unwrap();
    }
    let after = id::registry_sizes();
    let (paths, steps, keys, bytes) = (after.0 - before.0, after.1 - before.1, after.2 - before.2, after.3 - before.3);
    // Each path is held twice (the list and the map's key), 8 bytes a step,
    // plus a Vec header (24) each time; each key twice as a String (24 + bytes).
    let estimate = 2 * (paths * 24 + steps * 8) + 2 * (keys * 24 + bytes);
    println!(
        "10,000 moves of a value-keyed repeat: +{paths} paths ({steps} steps), +{keys} keys ({bytes} bytes); about {:.1} MiB never freed, not counting hash tables",
        estimate as f64 / (1024.0 * 1024.0)
    );
}
