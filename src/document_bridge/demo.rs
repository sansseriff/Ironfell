//! The starting scene, as a transaction. Replaces the ad-hoc demo systems: a
//! draggable rect, a field of small rects, a circle, and the torus. Until a
//! file can be loaded (plan step 4) this is what the app opens with.

use super::PendingTransactions;
use bevy::prelude::*;
use iron_document::component::{Fill, Mesh, Name as DocName, Radius, Size, Stroke, Transform2d, Transform3d};
use iron_document::component::{Clip, Slider, Timing};
use iron_document::{Actor, Expr, LeafStruct, Op, OrderKey, RelKind, Relation, Slot, TypeId, Value};
use std::collections::BTreeMap;

pub(super) fn queue_demo_scene(store: Res<super::DocumentStore>, mut pending: ResMut<PendingTransactions>) {
    pending.0.push(demo_scene(&store.0));
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

pub fn demo_scene(store: &iron_document::Store) -> iron_document::TransactionInput {
    let ty = |s: &str| TypeId::lookup(s).expect("registered type");
    let mut ids = store.document().next_ids(29).into_iter();
    let rel_ids = store.document().next_relation_ids(2);
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
    let square = next();
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
        id: square,
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

    // A slider, and a bar whose height and top follow it through bindings:
    // the first reactive edge in the document (doc 02 §4).
    let slider = next();
    let mut t = Transform2d::default();
    t.x = n(100.0);
    t.y = n(600.0);
    let mut s = Size::default();
    s.w = n(300.0);
    s.h = n(24.0);
    let mut sl = Slider::default();
    sl.value = n(0.4);
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("level".into()));
    ops.push(Op::Create {
        id: slider,
        ty: ty("slider"),
        parent: Some(scene),
        order: order(),
        components: vec![t.wrap(), s.wrap(), sl.wrap(), name.wrap()],
    });
    let bar = next();
    let mut t = Transform2d::default();
    t.x = n(780.0);
    t.y = Slot::Bound(Expr::parse(&format!("640 - {slider}.slider.value * 300")).expect("demo binding"));
    let mut s = Size::default();
    s.w = n(60.0);
    s.h = Slot::Bound(Expr::parse(&format!("{slider}.slider.value * 300")).expect("demo binding"));
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color([0.2, 0.5, 0.9, 1.0]));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("bar".into()));
    ops.push(Op::Create {
        id: bar,
        ty: ty("bar"),
        parent: Some(scene),
        order: order(),
        components: vec![t.wrap(), s.wrap(), f.wrap(), name.wrap()],
    });

    // The torus.
    let torus = next();
    let mut t = Transform3d::default();
    t.pos = Slot::Const(Value::Vec3([0.0, 1.5, 0.0]));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("torus".into()));
    ops.push(Op::Create {
        id: torus,
        ty: ty("mesh"),
        parent: None,
        order: order(),
        components: vec![t.wrap(), Mesh::default().wrap(), name.wrap()],
    });

    // A timeline with two clips (doc 05: scripted time). The clip names its
    // target through an `animates` relation, not a field.
    let timeline = next();
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("main".into()));
    ops.push(Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: order(), components: vec![name.wrap()] });
    let mut clip_order = OrderKey::first();
    let mut clip = |id, label: &str, start: f64, dur: f64, easing: &str, prop: &str, from: Value, to: Value| {
        let mut timing = Timing::default();
        timing.start = n(start);
        timing.dur = n(dur);
        timing.easing = Slot::Const(Value::Enum(easing.into()));
        let mut c = Clip::default();
        c.prop = Slot::Const(Value::Str(prop.into()));
        c.from = Slot::Const(from);
        c.to = Slot::Const(to);
        let mut name = DocName::default();
        name.text = Slot::Const(Value::Str(label.into()));
        let op = Op::Create {
            id,
            ty: ty("clip"),
            parent: Some(timeline),
            order: clip_order.clone(),
            components: vec![timing.wrap(), c.wrap(), name.wrap()],
        };
        clip_order = OrderKey::after(&clip_order);
        op
    };
    let spin = next();
    ops.push(clip(spin, "spin", 1.0, 3.0, "ease_in_out", "transform3d.rot", Value::Vec3([0.0, 0.0, 0.0]), Value::Vec3([0.0, std::f64::consts::TAU, 0.0])));
    let slide = next();
    ops.push(clip(slide, "slide", 0.5, 2.0, "linear", "transform2d.x", Value::Number(500.0), Value::Number(700.0)));
    ops.push(Op::Link { rel: Relation { id: rel_ids[0], from: spin, to: torus, rel: RelKind::Animates, data: BTreeMap::new() } });
    ops.push(Op::Link { rel: Relation { id: rel_ids[1], from: slide, to: square, rel: RelKind::Animates, data: BTreeMap::new() } });

    iron_document::TransactionInput { label: "open demo scene".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops }
}
