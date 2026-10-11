//! Linear binding inversion (roadmap stage A): dragging a leaf bound to an
//! expression linear in one constant leaf sets that leaf.

use iron_document::component::{Clip, Fill, Size, Slider, Timing, Transform2d};
use iron_document::handle::Handle;
use iron_document::*;
use std::collections::BTreeMap;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops }
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
}

fn bind(text: &str) -> Slot {
    Slot::Bound(Expr::parse(text).unwrap())
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn leaf(id: NodeId, p: &str) -> Leaf {
    Leaf { node: id, path: path(p) }
}

fn resolved(store: &Store, id: NodeId, p: &str) -> f64 {
    store.resolved(id, path(p)).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("{id}.{p} unresolved"))
}

/// slider (value 0.4); bar with y = 640 - value*300 and h = value*300;
/// rect with x = 2*v + 10 where v is the rect's own sx leaf... no: v is a
/// second slider's value, so the test reads naturally.
struct Scene {
    store: Store,
    slider: NodeId,
    bar: NodeId,
    rect: NodeId,
    v: NodeId,
}

fn scene() -> Scene {
    let mut store = Store::new();
    let ids = store.document().next_ids(4);
    let (slider, bar, v, rect) = (ids[0], ids[1], ids[2], ids[3]);
    let mut sl = Slider::default();
    sl.value = num(0.4);
    let mut bt = Transform2d::default();
    bt.x = num(780.0);
    bt.y = bind(&format!("640 - {slider}.slider.value * 300"));
    let mut bs = Size::default();
    bs.h = bind(&format!("{slider}.slider.value * 300"));
    let mut vs = Slider::default();
    vs.value = num(5.0);
    vs.max = num(1000.0);
    let mut rt = Transform2d::default();
    rt.x = bind(&format!("2 * {v}.slider.value + 10"));
    rt.y = num(50.0);
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: slider, ty: ty("slider"), parent: None, order: OrderKey::first(), components: vec![Transform2d::default().wrap(), sl.wrap()] },
                Op::Create { id: bar, ty: ty("bar"), parent: None, order: OrderKey::first(), components: vec![bt.wrap(), bs.wrap(), Fill::default().wrap()] },
                Op::Create { id: v, ty: ty("slider"), parent: None, order: OrderKey::first(), components: vec![Transform2d::default().wrap(), vs.wrap()] },
                Op::Create { id: rect, ty: ty("rect"), parent: None, order: OrderKey::first(), components: vec![rt.wrap(), Size::default().wrap()] },
            ],
        ))
        .unwrap();
    store.drain_changes();
    Scene { store, slider, bar, rect, v }
}

#[test]
fn x_equals_two_v_plus_ten_sets_v() {
    let Scene { mut store, rect, v, .. } = scene();
    assert_eq!(resolved(&store, rect, "transform2d.x"), 20.0);
    match store.handle(rect, path("transform2d.x")) {
        Handle::Linear { source, a, b } => {
            assert_eq!(source, leaf(v, "slider.value"));
            assert_eq!((a, b), (2.0, 10.0));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(store.handle(rect, path("transform2d.y")), Handle::Direct);

    // Drag x to 110: v becomes 50, x reads 110, and the document is untouched.
    store.preview_target(rect, path("transform2d.x"), Value::Number(110.0)).unwrap();
    assert_eq!(resolved(&store, v, "slider.value"), 50.0);
    assert_eq!(resolved(&store, rect, "transform2d.x"), 110.0);
    assert_eq!(store.document().leaf(v, path("slider.value")).unwrap().constant(), Some(&Value::Number(5.0)));
    let changed: Vec<Leaf> = store.drain_changes().into_iter().map(|c| c.leaf).collect();
    assert!(changed.contains(&leaf(v, "slider.value")) && changed.contains(&leaf(rect, "transform2d.x")), "{changed:?}");

    // The commit writes the source, once, and undo restores it.
    let applied = store.commit_gesture("move rect", Actor::Human, 1).unwrap().unwrap();
    let last = store.history().past().last().unwrap();
    assert_eq!(last.ops.len(), 1, "{:?}", last.ops);
    assert!(matches!(&last.ops[0], Op::Set { id, path: p, slot } if *id == v && *p == path("slider.value") && slot.constant() == Some(&Value::Number(50.0))));
    assert_eq!(applied.version, Version(2));
    assert!(store.document().leaf(rect, path("transform2d.x")).unwrap().expr().is_some(), "the binding stays");
    store.undo(2).unwrap().unwrap();
    assert_eq!(resolved(&store, rect, "transform2d.x"), 20.0);
}

#[test]
fn the_demo_bar_drags_its_slider() {
    let Scene { mut store, slider, bar, .. } = scene();
    // y = 640 - value*300 is linear with a negative slope.
    match store.handle(bar, path("transform2d.y")) {
        Handle::Linear { source, a, b } => {
            assert_eq!(source, leaf(slider, "slider.value"));
            assert_eq!((a, b), (-300.0, 640.0));
        }
        other => panic!("{other:?}"),
    }
    store.preview_target(bar, path("transform2d.y"), Value::Number(400.0)).unwrap();
    assert!((resolved(&store, slider, "slider.value") - 0.8).abs() < 1e-12);
    assert!((resolved(&store, bar, "transform2d.y") - 400.0).abs() < 1e-9);
    assert!((resolved(&store, bar, "size.h") - 240.0).abs() < 1e-9, "the other binding follows");
    // x is a constant: previewed directly, and both land in one transaction.
    store.preview_target(bar, path("transform2d.x"), Value::Number(700.0)).unwrap();
    store.commit_gesture("move bar", Actor::Human, 1).unwrap().unwrap();
    let last = store.history().past().last().unwrap();
    assert_eq!(last.ops.len(), 2, "{:?}", last.ops);
}

#[test]
fn inversion_follows_a_chain_of_bindings() {
    let Scene { mut store, slider, bar, rect, .. } = scene();
    // rect.y = bar.size.h + 10 = slider.value*300 + 10: linear in the slider through the bar.
    store.apply(tx("chain", vec![Op::Set { id: rect, path: path("transform2d.y"), slot: bind(&format!("{bar}.size.h + 10")) }])).unwrap();
    match store.handle(rect, path("transform2d.y")) {
        Handle::Linear { source, a, b } => {
            assert_eq!(source, leaf(slider, "slider.value"));
            assert_eq!((a, b), (300.0, 10.0));
        }
        other => panic!("{other:?}"),
    }
    store.preview_target(rect, path("transform2d.y"), Value::Number(160.0)).unwrap();
    assert!((resolved(&store, slider, "slider.value") - 0.5).abs() < 1e-12);
}

#[test]
fn linear_floor_functions_and_constant_conditions_invert() {
    let Scene { mut store, rect, v, slider, .. } = scene();
    let cases: &[(&str, Option<(f64, f64)>)] = &[
        (&format!("lerp(100, 400, {v}.slider.value)"), Some((300.0, 100.0))),
        (&format!("scale({v}.slider.value, 0, 1, 100, 400)"), Some((300.0, 100.0))),
        (&format!("pi > 3 ? {v}.slider.value * 2 : {v}.slider.value / 2 + 1"), Some((2.0, 0.0))),
        (&format!("pi < 3 ? {v}.slider.value * 2 : {v}.slider.value / 2 + 1"), Some((0.5, 1.0))),
        (&format!("-({v}.slider.value - 4) / 2"), Some((-0.5, 2.0))),
        (&format!("{v}.slider.value * pi"), Some((std::f64::consts::PI, 0.0))),
    ];
    let _ = slider;
    for (text, expect) in cases {
        store.apply(tx("rebind", vec![Op::Set { id: rect, path: path("transform2d.x"), slot: bind(text) }])).unwrap();
        match (store.handle(rect, path("transform2d.x")), expect) {
            (Handle::Linear { a, b, .. }, Some((ea, eb))) => {
                assert!((a - ea).abs() < 1e-12 && (b - eb).abs() < 1e-12, "{text}: got ({a}, {b})");
            }
            (h, e) => panic!("{text}: {h:?} vs {e:?}"),
        }
    }
}

#[test]
fn everything_else_is_locked_with_a_reason() {
    let Scene { mut store, rect, v, slider, .. } = scene();
    let locked = [
        format!("{v}.slider.value * {v}.slider.value"),
        format!("sin({v}.slider.value)"),
        format!("clamp({v}.slider.value, 0, 1)"),
        // Two free leaves, even where the expression is linear in each.
        format!("{v}.slider.value + {slider}.slider.value"),
        format!("{v}.slider.value * {slider}.slider.value"),
        format!("{slider}.slider.value > 0.5 ? {v}.slider.value * 2 : 0"),
        format!("{v}.slider.value > 3 ? {v}.slider.value : 0"),
        format!("1 / {v}.slider.value"),
        format!("{v}.slider.value % 2"),
        format!("{v}.slider.value * 0"),
    ];
    for text in locked {
        store.apply(tx("rebind", vec![Op::Set { id: rect, path: path("transform2d.x"), slot: bind(&text) }])).unwrap();
        let h = store.handle(rect, path("transform2d.x"));
        assert!(h.is_locked(), "{text}: {h:?}");
        let err = store.preview_target(rect, path("transform2d.x"), Value::Number(1.0)).unwrap_err();
        assert!(err.contains(&rect.to_string()), "{err}");
    }
    // Two constant leaves: the reason names both.
    store
        .apply(tx("two", vec![Op::Set { id: rect, path: path("transform2d.x"), slot: bind(&format!("{v}.slider.value + {slider}.slider.value")) }]))
        .unwrap();
    let Handle::Locked(reason) = store.handle(rect, path("transform2d.x")) else { panic!() };
    assert!(reason.contains("2 free leaves") && reason.contains(&v.to_string()) && reason.contains(&slider.to_string()), "{reason}");
    // A missing leaf.
    assert!(store.handle(rect, path("slider.value")).is_locked());
}

#[test]
fn clip_driven_leaves_lock_the_target_and_the_source() {
    let Scene { mut store, rect, v, .. } = scene();
    let ids = store.document().next_ids(2);
    let (timeline, clip) = (ids[0], ids[1]);
    let rel = store.document().next_relation_id();
    let mut timing = Timing::default();
    timing.start = num(0.0);
    timing.dur = num(10.0);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("slider.value".into()));
    c.from = num(0.0);
    c.to = num(100.0);
    store
        .apply(tx(
            "animate v",
            vec![
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: OrderKey::first(), components: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: OrderKey::first(), components: vec![timing.wrap(), c.wrap()] },
                Op::Link { rel: Relation { id: rel, from: clip, to: v, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    store.set_time(5.0);
    // v is clip-driven: the rect's x (bound to v) has no free leaf, and v itself is locked.
    assert!(store.handle(rect, path("transform2d.x")).is_locked());
    let Handle::Locked(reason) = store.handle(v, path("slider.value")) else { panic!() };
    assert!(reason.contains("clip"), "{reason}");
    store.set_time(20.0);
    // After the clip ends it still holds `to`, so the leaf stays locked.
    assert!(store.handle(v, path("slider.value")).is_locked());
    // Before the clip starts nothing drives it.
    store
        .apply(tx("later", vec![Op::Set { id: clip, path: path("timing.start"), slot: num(30.0) }]))
        .unwrap();
    assert_eq!(store.handle(v, path("slider.value")), Handle::Direct);
    assert!(matches!(store.handle(rect, path("transform2d.x")), Handle::Linear { .. }));
}
