//! Bindings, the reactive graph, and gestures (plans/document-spine.md step 5).

use iron_document::component::{Fill, Size, Slider, Transform2d};
use iron_document::*;

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

fn resolved_num(store: &Store, id: NodeId, p: &str) -> f64 {
    store.resolved(id, path(p)).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("{id}.{p} unresolved"))
}

/// A slider and a bar whose height and y are bound to it.
struct Scene {
    store: Store,
    slider: NodeId,
    bar: NodeId,
}

fn scene() -> Scene {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    let (slider, bar) = (ids[0], ids[1]);
    let mut sl = Slider::default();
    sl.value = num(0.25);
    let mut t = Transform2d::default();
    t.x = num(1000.0);
    t.y = bind(&format!("640 - {slider}.slider.value * 300"));
    let mut s = Size::default();
    s.w = num(60.0);
    s.h = bind(&format!("{slider}.slider.value * 300"));
    store
        .apply(tx(
            "build",
            vec![
                Op::Create {
                    id: slider,
                    ty: ty("slider"),
                    parent: None,
                    order: OrderKey::first(),
                    components: vec![Transform2d::default().wrap(), sl.wrap()],
                },
                Op::Create {
                    id: bar,
                    ty: ty("bar"),
                    parent: None,
                    order: OrderKey::after(&OrderKey::first()),
                    components: vec![t.wrap(), s.wrap(), Fill::default().wrap()],
                },
            ],
        ))
        .unwrap();
    Scene { store, slider, bar }
}

#[test]
fn bindings_resolve_on_apply_and_on_change() {
    let Scene { mut store, slider, bar } = scene();
    assert_eq!(resolved_num(&store, bar, "size.h"), 75.0);
    assert_eq!(resolved_num(&store, bar, "transform2d.y"), 565.0);
    // The authored slot is still the binding, not the value.
    assert!(store.document().leaf(bar, path("size.h")).unwrap().expr().is_some());
    // Changes for the two bound leaves were reported once.
    let changes = store.drain_changes();
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert!(!store.has_changes());

    store.apply(tx("set", vec![Op::Set { id: slider, path: path("slider.value"), slot: num(0.5) }])).unwrap();
    assert_eq!(resolved_num(&store, bar, "size.h"), 150.0);
    assert_eq!(resolved_num(&store, bar, "transform2d.y"), 490.0);
    let changes = store.drain_changes();
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().all(|c| c.leaf.node == bar));
}

#[test]
fn views_show_value_and_binding() {
    let Scene { store, bar, .. } = scene();
    let v = store.view(&ViewQuery { scope: Some(bar), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains("y=\"565\" y.bind=\"640 - #1.slider.value * 300\""), "{v}");
    assert!(v.contains("h=\"75\" h.bind=\"#1.slider.value * 300\""), "{v}");
}

#[test]
fn bindings_survive_the_file_and_replay() {
    let Scene { store, bar, .. } = scene();
    let text = store.save();
    assert!(text.contains(r##""h":{"bind":"#1.slider.value * 300"}"##), "{text}");
    let loaded = Store::load(&text).unwrap();
    assert_eq!(loaded.save(), text);
    assert_eq!(resolved_num(&loaded, bar, "size.h"), 75.0);
}

#[test]
fn diamond_is_glitch_free_and_cutoff_stops_propagation() {
    // a = slider.value; b = a * 2 (bar.size.w); c = a + b (bar.size.h).
    let Scene { mut store, slider, bar } = scene();
    store
        .apply(tx(
            "rebind",
            vec![
                Op::Set { id: bar, path: path("size.w"), slot: bind(&format!("{slider}.slider.value * 2")) },
                Op::Set { id: bar, path: path("size.h"), slot: bind(&format!("{slider}.slider.value + {bar}.size.w")) },
                Op::Set { id: bar, path: path("transform2d.y"), slot: num(0.0) },
            ],
        ))
        .unwrap();
    assert_eq!(store.reactive().height(Leaf { node: bar, path: path("size.w") }), Some(1));
    assert_eq!(store.reactive().height(Leaf { node: bar, path: path("size.h") }), Some(2));
    store.drain_changes();

    let before = store.reactive().evaluations;
    store.apply(tx("set", vec![Op::Set { id: slider, path: path("slider.value"), slot: num(1.0) }])).unwrap();
    // Each of b and c evaluated exactly once, c after b.
    assert_eq!(store.reactive().evaluations - before, 2);
    assert_eq!(resolved_num(&store, bar, "size.w"), 2.0);
    assert_eq!(resolved_num(&store, bar, "size.h"), 3.0);

    // Cutoff: d = w > 100 ? 1 : 0 (fill.opacity); e depends on d via a
    // second node. Changing w without crossing the threshold evaluates d
    // only; e is never run.
    let other = store.document().next_id();
    let mut f = Fill::default();
    f.opacity = bind(&format!("{bar}.fill.opacity * 0.5"));
    let mut t = Transform2d::default();
    t.x = num(0.0);
    store
        .apply(tx(
            "threshold",
            vec![
                Op::Set { id: bar, path: path("size.w"), slot: num(150.0) },
                Op::Set { id: bar, path: path("fill.opacity"), slot: bind(&format!("{bar}.size.w > 100 ? 1 : 0")) },
                Op::Create {
                    id: other,
                    ty: ty("rect"),
                    parent: None,
                    order: OrderKey::first(),
                    components: vec![t.wrap(), Size::default().wrap(), f.wrap()],
                },
            ],
        ))
        .unwrap();
    assert_eq!(resolved_num(&store, other, "fill.opacity"), 0.5);
    store.drain_changes();
    let before = store.reactive().evaluations;
    store.apply(tx("nudge", vec![Op::Set { id: bar, path: path("size.w"), slot: num(160.0) }])).unwrap();
    // Two readers of size.w ran: the threshold (unchanged result) and size.h.
    // The threshold's dependent on the other node did not, which is the cutoff.
    assert_eq!(store.reactive().evaluations - before, 2);
    assert_eq!(resolved_num(&store, other, "fill.opacity"), 0.5);
    let changed: Vec<Leaf> = store.drain_changes().into_iter().map(|c| c.leaf).collect();
    assert_eq!(changed, vec![Leaf { node: bar, path: path("size.h") }]);
}

#[test]
fn invalid_bindings_are_rejected() {
    let Scene { mut store, slider, bar } = scene();
    // Self-dependency through another leaf.
    let e = store
        .apply(tx("cycle", vec![Op::Set { id: slider, path: path("slider.value"), slot: bind(&format!("{bar}.size.h / 300")) }]))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::Cycle, "{}", e[0].message);
    // Direct self-reference.
    let e = store
        .apply(tx("self", vec![Op::Set { id: bar, path: path("size.w"), slot: bind(&format!("{bar}.size.w + 1")) }]))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::Cycle);
    // Dangling reference.
    let e = store
        .apply(tx("dangling", vec![Op::Set { id: bar, path: path("size.w"), slot: bind("#zz.size.w") }]))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::UnknownNode);
    // Reference to a component the node does not have.
    let e = store
        .apply(tx("no comp", vec![Op::Set { id: bar, path: path("size.w"), slot: bind(&format!("{slider}.size.w")) }]))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::UnknownPath);
    // A create carrying a bad binding is rejected too.
    let id = store.document().next_id();
    let mut s = Size::default();
    s.w = bind("#zz.size.w");
    let e = store
        .apply(tx(
            "create bad",
            vec![Op::Create {
                id,
                ty: ty("rect"),
                parent: None,
                order: OrderKey::first(),
                components: vec![Transform2d::default().wrap(), s.wrap()],
            }],
        ))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::UnknownNode);
    store.document().check_invariants().unwrap();
}

#[test]
fn gestures_preview_then_commit_once() {
    let Scene { mut store, slider, bar } = scene();
    store.drain_changes();
    let v0 = store.document().version();

    // Preview: dependents follow, nothing is written, changes are reported.
    for v in [0.3, 0.4, 0.5] {
        store.preview(slider, path("slider.value"), Value::Number(v));
    }
    assert!(store.gesture_in_progress());
    assert_eq!(resolved_num(&store, slider, "slider.value"), 0.5);
    assert_eq!(resolved_num(&store, bar, "size.h"), 150.0);
    assert_eq!(store.document().leaf(slider, path("slider.value")).unwrap().constant(), Some(&Value::Number(0.25)));
    assert_eq!(store.document().version(), v0);
    let changed: Vec<Leaf> = store.drain_changes().into_iter().map(|c| c.leaf).collect();
    assert_eq!(changed.len(), 3, "{changed:?}");

    // Commit: one labelled transaction with one op; overlay gone.
    let applied = store.commit_gesture("set slider", Actor::Human, 7).unwrap().unwrap();
    assert_eq!(applied.version, Version(v0.0 + 1));
    let last = store.history().past().last().unwrap();
    assert_eq!(last.label, "set slider");
    assert_eq!(last.ops.len(), 1);
    assert!(!store.gesture_in_progress());
    assert_eq!(store.document().leaf(slider, path("slider.value")).unwrap().constant(), Some(&Value::Number(0.5)));
    assert_eq!(resolved_num(&store, bar, "size.h"), 150.0);

    // Undo restores the constant and the dependents.
    store.undo(8).unwrap().unwrap();
    assert_eq!(resolved_num(&store, bar, "size.h"), 75.0);

    // Cancel reverts previews without a transaction.
    store.drain_changes();
    let v1 = store.document().version();
    store.preview(slider, path("slider.value"), Value::Number(0.9));
    assert_eq!(resolved_num(&store, bar, "size.h"), 270.0);
    store.cancel_gesture();
    assert_eq!(store.document().version(), v1);
    assert_eq!(resolved_num(&store, bar, "size.h"), 75.0);
    let changed = store.drain_changes();
    assert!(changed.iter().any(|c| c.leaf.node == bar && c.value == Value::Number(75.0)));

    // A preview on a bound leaf never becomes a write.
    store.preview(bar, path("size.h"), Value::Number(1.0));
    assert!(store.commit_gesture("no-op", Actor::Human, 9).is_none());
    assert!(store.document().leaf(bar, path("size.h")).unwrap().expr().is_some());
}

#[test]
fn undoing_a_binding_makes_the_slot_constant_again() {
    let Scene { mut store, slider, bar } = scene();
    store.apply(tx("unbind", vec![Op::Set { id: bar, path: path("size.h"), slot: num(10.0) }])).unwrap();
    assert_eq!(resolved_num(&store, bar, "size.h"), 10.0);
    store.drain_changes();
    store.apply(tx("set", vec![Op::Set { id: slider, path: path("slider.value"), slot: num(0.9) }])).unwrap();
    assert_eq!(resolved_num(&store, bar, "size.h"), 10.0, "unbound slot ignores the slider");
    store.undo(1).unwrap().unwrap(); // slider back to 0.25
    store.undo(2).unwrap().unwrap(); // binding restored
    assert_eq!(resolved_num(&store, bar, "size.h"), 75.0);
    let v = store.view(&ViewQuery { scope: Some(bar), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains("h.bind="), "{v}");
    store.redo(3).unwrap().unwrap();
    let v = store.view(&ViewQuery { scope: Some(bar), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(!v.contains("h.bind="), "{v}");
    assert_eq!(resolved_num(&store, bar, "size.h"), 10.0);
}
