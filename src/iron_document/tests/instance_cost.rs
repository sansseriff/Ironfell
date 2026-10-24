//! What holding linked instances' expanded nodes costs (instance-cache.md
//! phase 0, the measurement phase D waits on): `n` dial meters, their
//! definition given 60 tick marks, then the time of a structural edit
//! elsewhere (every one re-runs expansion over every instance), of a
//! constant edit, of a document clone, and the node count. Ignored; run with
//!   cargo test -p iron_document --release --target aarch64-apple-darwin --test instance_cost -- --ignored --nocapture

use iron_document::facet::{Composition, Name, Size, Transform2d};
use iron_document::*;
use std::time::Instant;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

#[test]
#[ignore]
fn expanded_instances_cost() {
    for count in [100, 1000] {
        let mut store = Store::new();
        let ids = store.document().next_ids(2);
        let mut c = Composition::default();
        c.dims = Slot::Const(Value::Enum("2d".into()));
        let mut name = Name::default();
        name.text = Slot::Const(Value::Str("bench".into()));
        store
            .apply(tx("desk", vec![
                Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] },
                Op::Create { id: ids[1], ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
            ]))
            .unwrap();
        let comp = ids[0];
        store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
        // 60 ticks in the template, as the app's `meters` bench has.
        let doc = store.document();
        let def = standard::find(doc, "meter").unwrap();
        let template = definition::template_of(doc, def, "free").unwrap();
        let last = doc.children(template).iter().filter_map(|&k| doc.live(k)).map(|x| x.order.clone()).max();
        let tick_ids = doc.next_ids(60);
        let mut prev = last;
        let mut ops = Vec::new();
        for &id in &tick_ids {
            let order = OrderKey::between(prev.as_ref(), None).unwrap();
            prev = Some(order.clone());
            let mut s = Size::default();
            s.w = n(6.0);
            s.h = n(1.2);
            ops.push(Op::Create { id, ty: ty("rect"), parent: Some(template), order, facets: vec![Transform2d::default().wrap(), s.wrap()] });
        }
        store.apply(tx("ticks", ops)).unwrap();
        let started = Instant::now();
        for i in 1..count {
            store.place_standard("meter", comp, [(i % 30) as f64 * 130.0, (i / 30) as f64 * 140.0], Actor::Human).unwrap();
        }
        let place_ms = started.elapsed().as_secs_f64() * 1000.0 / (count - 1) as f64;
        let nodes = store.document().live_nodes().count();
        let expanded = store.document().expanded_ids().count();
        // A structural edit elsewhere: a rect added to the composition.
        let mut times = Vec::new();
        for _ in 0..5 {
            let id = store.document().next_id();
            let order = OrderKey::after(&store.document().children(comp).iter().filter_map(|&k| store.document().live(k)).map(|x| x.order.clone()).max().unwrap());
            let mut s = Size::default();
            s.w = n(10.0);
            s.h = n(10.0);
            let t = Instant::now();
            store.apply(tx("rect", vec![Op::Create { id, ty: ty("rect"), parent: Some(comp), order, facets: vec![Transform2d::default().wrap(), s.wrap()] }])).unwrap();
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        // A constant edit on that rect: no expansion.
        let rect = *store.document().children(comp).last().unwrap();
        let t = Instant::now();
        store.apply(tx("move", vec![Op::Set { id: rect, path: "transform2d.x".parse().unwrap(), slot: n(50.0) }])).unwrap();
        let set_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let clone = store.document().clone();
        let clone_ms = t.elapsed().as_secs_f64() * 1000.0;
        drop(clone);
        println!("{count} meters: {nodes} live nodes ({expanded} expanded); place {place_ms:.2} ms each; structural edit p50 {:.2} ms; constant edit {set_ms:.2} ms; clone {clone_ms:.2} ms", times[2]);
    }
}
