//! Prints the canonical file and the full view for a small scene, so the two
//! formats can be read side by side. `cargo run -p iron_document --example dump`.

use iron_document::component::{Fill, Name, Radius, Size, Transform2d};
use iron_document::*;

fn main() {
    let mut store = Store::new();
    let ids = store.document().next_ids(3);
    let k0 = OrderKey::first();
    let k1 = OrderKey::after(&k0);
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("scene".into()));
    let mut t = Transform2d::default();
    t.x = Slot::Const(Value::Number(120.0));
    t.y = Slot::Const(Value::Number(80.0));
    store
        .apply(TransactionInput {
            label: "build".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![
                Op::Create {
                    id: ids[0],
                    ty: TypeId::lookup("group").unwrap(),
                    parent: None,
                    order: k0.clone(),
                    components: vec![name.wrap()],
                },
                Op::Create {
                    id: ids[1],
                    ty: TypeId::lookup("rect").unwrap(),
                    parent: Some(ids[0]),
                    order: k0.clone(),
                    components: vec![t.wrap(), Size::default().wrap(), Fill::default().wrap()],
                },
                Op::Create {
                    id: ids[2],
                    ty: TypeId::lookup("circle").unwrap(),
                    parent: Some(ids[0]),
                    order: k1,
                    components: vec![Transform2d::default().wrap(), Radius::default().wrap()],
                },
            ],
        })
        .unwrap();
    store
        .apply(TransactionInput {
            label: "move rect".into(),
            actor: Actor::Human,
            ts: 1,
            idempotency_key: None,
            ops: vec![Op::Set {
                id: ids[1],
                path: "transform2d.x".parse().unwrap(),
                slot: Slot::Const(Value::Number(200.0)),
            }],
        })
        .unwrap();
    println!("=== file ===\n{}", store.save());
    println!(
        "=== view (full) ===\n{}",
        store
            .view(&ViewQuery {
                scope: None,
                fidelity: Fidelity::Full,
                depth: None
            })
            .unwrap()
    );
    println!(
        "=== last transaction ===\n{}",
        serde_json::to_string_pretty(store.history().past().last().unwrap()).unwrap()
    );
}
