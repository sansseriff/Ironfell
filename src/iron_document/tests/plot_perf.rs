//! A timing probe, not a regression test (plans/specs/reactivity.md §13,
//! 2026-11-14): what one pan or zoom step of a `std:plot` costs the store,
//! split by whether the tick count changed (a re-expansion). `PLOTS=n`
//! places n plots, to show how a re-expansion scales with the document.
//!
//! cargo test --release -p iron_document --target aarch64-apple-darwin \
//!   --test plot_perf -- --ignored --nocapture
use iron_document::facet::{Composition, Name};
use iron_document::*;
use std::time::Instant;

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

/// Count, p50 and max of `v`, in ms.
fn stat(mut v: Vec<f64>) -> String {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    match v.len() {
        0 => "none".into(),
        n => format!("n {n}, p50 {:.3} ms, max {:.3} ms", v[n / 2], v[n - 1]),
    }
}

#[test]
#[ignore]
fn plot_pan_and_zoom_cost() {
    let mut store = Store::new();
    let ids = store.authored().next_ids(2);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("sheet".into()));
    let ty = |n: &str| TypeId::lookup(n).unwrap();
    store
        .apply(tx(
            "sheet",
            vec![
                Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] },
                Op::Create { id: ids[1], ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
            ],
        ))
        .unwrap();
    let plots: usize = std::env::var("PLOTS").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let placed: Vec<NodeId> = (0..plots).map(|k| store.place_standard("plot", ids[0], [0.0, 300.0 * k as f64], Actor::Human).unwrap()).collect();
    let first = placed[0];
    let doc = store.tree();
    let area = doc
        .descendants(first)
        .into_iter()
        .find(|&d| d.is_expanded() && !expand::in_repeat_template(doc, d) && doc.get::<Name>(d).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some("plot area"))
        .unwrap();
    println!("{plots} plot(s), {} nodes", store.tree().descendants(ids[0]).len());
    let mut step = |event: &str, payload: [f64; 5]| {
        let t = Instant::now();
        store.pointer_action(area, event, payload).unwrap().unwrap();
        let c = store.take_changes();
        (t.elapsed().as_secs_f64() * 1e3, c.created.len() + c.removed.len() + c.slots.len())
    };
    // Pan right steadily, then zoom in and out in bursts of 30.
    let pan: Vec<(f64, usize)> = (0..400).map(|_| step("drag", [-3.0, 0.0, 0.0, 0.0, 0.0])).collect();
    let zoom: Vec<(f64, usize)> = (0..300).map(|i| step("wheel", [0.0, 0.0, 150.0, 90.0, if (i / 30) % 2 == 0 { -40.0 } else { 40.0 }])).collect();
    for (label, steps) in [("pan", pan), ("zoom", zoom)] {
        let values = steps.iter().filter(|s| s.1 == 0).map(|s| s.0).collect();
        let structure = steps.iter().filter(|s| s.1 > 0).map(|s| s.0).collect();
        let ops = steps.iter().map(|s| s.1).filter(|&n| n > 0).max().unwrap_or(0);
        println!("{label}: values only {}; tick count changed {} (up to {ops} nodes made, gone or set)", stat(values), stat(structure));
    }
}
