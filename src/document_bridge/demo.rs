//! The starting scene, as a transaction. Replaces the ad-hoc demo systems: a
//! draggable rect, a field of small rects, a circle, and the torus. Until a
//! file can be loaded (plan step 4) this is what the app opens with.

use super::PendingTransactions;
use bevy::prelude::*;
use iron_document::component::{Fill, Mesh, Name as DocName, Radius, Size, Stroke, Transform2d, Transform3d};
use iron_document::{Actor, LeafStruct, Op, OrderKey, Slot, TypeId, Value};

pub(super) fn queue_demo_scene(store: Res<super::DocumentStore>, mut pending: ResMut<PendingTransactions>) {
    pending.0.push(demo_scene(&store.0));
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

pub fn demo_scene(store: &iron_document::Store) -> iron_document::TransactionInput {
    let ty = |s: &str| TypeId::lookup(s).expect("registered type");
    let mut ids = store.document().next_ids(24).into_iter();
    let mut next = move || ids.next().expect("reserved");
    let mut key = OrderKey::first();
    let mut order = move || {
        let k = key.clone();
        key = OrderKey::after(&key);
        k
    };
    let mut ops = Vec::new();

    let scene = next();
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("scene".into()));
    ops.push(Op::Create { id: scene, ty: ty("group"), parent: None, order: order(), components: vec![name.wrap()] });

    // A field of small rects, deterministic positions.
    let mut seed: u32 = 0x91E2_33AB;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f64) / (u32::MAX as f64)
    };
    for _ in 0..20 {
        let mut t = Transform2d::default();
        t.x = n((rnd() * 1100.0).round());
        t.y = n((rnd() * 640.0).round());
        let mut s = Size::default();
        s.w = n(30.0);
        s.h = n(30.0);
        let mut f = Fill::default();
        f.color = Slot::Const(Value::Color([rnd() as f32, rnd() as f32, rnd() as f32, 1.0]));
        ops.push(Op::Create {
            id: next(),
            ty: ty("rect"),
            parent: Some(scene),
            order: order(),
            components: vec![t.wrap(), s.wrap(), f.wrap()],
        });
    }

    // The draggable square.
    let mut t = Transform2d::default();
    t.x = n(500.0);
    t.y = n(420.0);
    let mut s = Size::default();
    s.w = n(80.0);
    s.h = n(80.0);
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color([0.2, 0.2, 0.2, 1.0]));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("square".into()));
    ops.push(Op::Create {
        id: next(),
        ty: ty("rect"),
        parent: Some(scene),
        order: order(),
        components: vec![t.wrap(), s.wrap(), f.wrap(), name.wrap()],
    });

    // A circle with a stroke.
    let mut t = Transform2d::default();
    t.x = n(160.0);
    t.y = n(330.0);
    let mut r = Radius::default();
    r.r = n(40.0);
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color([0.95, 0.6, 0.1, 1.0]));
    let mut st = Stroke::default();
    st.width = n(3.0);
    ops.push(Op::Create {
        id: next(),
        ty: ty("circle"),
        parent: Some(scene),
        order: order(),
        components: vec![t.wrap(), r.wrap(), f.wrap(), st.wrap()],
    });

    // The torus.
    let mut t = Transform3d::default();
    t.pos = Slot::Const(Value::Vec3([0.0, 1.5, 0.0]));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("torus".into()));
    ops.push(Op::Create {
        id: next(),
        ty: ty("mesh"),
        parent: None,
        order: order(),
        components: vec![t.wrap(), Mesh::default().wrap(), name.wrap()],
    });

    iron_document::TransactionInput { label: "open demo scene".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops }
}
