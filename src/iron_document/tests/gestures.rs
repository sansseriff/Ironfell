//! Gesture ownership (infinite-canvas-architecture-patterns.md §4, exit
//! criteria): what a transaction arriving mid-gesture does to the gesture.

use iron_document::facet::{Constraint, Fill, Radius, Size, Slider, Transform2d};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, actor: Actor, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor, ts: 0, idempotency_key: None, ops, exact: false }
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn get(store: &Store, id: NodeId, p: &str) -> f64 {
    store.resolved(id, path(p)).and_then(|v| v.as_f64()).unwrap()
}

fn set(id: NodeId, p: &str, v: f64) -> Op {
    Op::Set { id, path: path(p), slot: num(v) }
}

struct Scene {
    store: Store,
    group: NodeId,
    /// A free rect at (10, 20).
    a: NodeId,
    /// Another rect, unrelated to `a`.
    other: NodeId,
    slider: NodeId,
    /// A bar whose x is bound to the slider: `x = slider * 100`.
    bar: NodeId,
    /// Two circles 100 apart by a distance constraint.
    p: NodeId,
    q: NodeId,
    link: NodeId,
}

fn scene() -> Scene {
    let mut store = Store::new();
    let ids = store.document().next_ids(8);
    let [group, a, other, slider, bar, p, q, link] = ids.try_into().unwrap();
    let rels = store.document().next_relation_ids(2);
    let mut key = OrderKey::first();
    let mut order = || {
        let k = key.clone();
        key = OrderKey::after(&key);
        k
    };
    let at = |x: f64, y: f64| {
        let mut t = Transform2d::default();
        t.x = num(x);
        t.y = num(y);
        t.wrap()
    };
    let size = || {
        let mut s = Size::default();
        s.w = num(10.0);
        s.h = num(10.0);
        s.wrap()
    };
    let circle = || {
        let mut r = Radius::default();
        r.rx = num(5.0);
        r.ry = num(5.0);
        r.wrap()
    };
    let mut sl = Slider::default();
    sl.value = num(0.5);
    let mut bt = Transform2d::default();
    bt.x = Slot::Bound(Expr::parse(&format!("{slider}.slider.value * 100")).unwrap());
    let mut c = Constraint::default();
    c.kind = Slot::Const(Value::Enum("distance".into()));
    c.value = num(100.0);
    let role = |r: &str| BTreeMap::from([("role".to_owned(), Value::Str(r.into()))]);
    store
        .apply(tx(
            "scene",
            Actor::System,
            vec![
                Op::Create { id: group, ty: ty("group"), parent: None, order: order(), facets: vec![Transform2d::default().wrap()] },
                Op::Create { id: a, ty: ty("rect"), parent: Some(group), order: order(), facets: vec![at(10.0, 20.0), size(), Fill::default().wrap()] },
                Op::Create { id: other, ty: ty("rect"), parent: Some(group), order: order(), facets: vec![at(300.0, 20.0), size(), Fill::default().wrap()] },
                Op::Create { id: slider, ty: ty("slider"), parent: Some(group), order: order(), facets: vec![at(0.0, 200.0), sl.wrap()] },
                Op::Create { id: bar, ty: ty("rect"), parent: Some(group), order: order(), facets: vec![bt.wrap(), size()] },
                Op::Create { id: p, ty: ty("ellipse"), parent: Some(group), order: order(), facets: vec![at(0.0, 400.0), circle()] },
                Op::Create { id: q, ty: ty("ellipse"), parent: Some(group), order: order(), facets: vec![at(100.0, 400.0), circle()] },
                Op::Create { id: link, ty: ty("constraint"), parent: Some(group), order: order(), facets: vec![c.wrap()] },
                Op::Link { rel: Relation { id: rels[0], from: link, to: p, rel: RelKind::Constrains, data: role("a") } },
                Op::Link { rel: Relation { id: rels[1], from: link, to: q, rel: RelKind::Constrains, data: role("b") } },
            ],
        ))
        .unwrap();
    Scene { store, group, a, other, slider, bar, p, q, link }
}

/// Drag `id` by its origin to `to`, leaving the gesture open.
fn drag(store: &mut Store, id: NodeId, to: [f64; 2]) -> (GestureId, DragHandle) {
    let g = store.begin_gesture(&[id]);
    let h = store.begin_drag(id, [0.0, 0.0]);
    store.drag_to(&h, to);
    (g, h)
}

#[test]
fn a_model_pose_edit_mid_drag_survives_cancel_and_release() {
    let mut s = scene();
    let (g, _) = drag(&mut s.store, s.a, [200.0, 200.0]);
    assert_eq!(get(&s.store, s.a, "transform2d.x"), 200.0);
    s.store.apply(tx("model moves a", Actor::Model, vec![set(s.a, "transform2d.x", 500.0)])).unwrap();
    assert!(matches!(s.store.gesture_status(g), GestureStatus::Cancelled(ref why) if why.contains("transform2d.x")), "{:?}", s.store.gesture_status(g));
    // The edit shows, not the drag.
    assert_eq!(get(&s.store, s.a, "transform2d.x"), 500.0);
    assert_eq!(get(&s.store, s.a, "transform2d.y"), 20.0);
    // Release commits nothing.
    let version = s.store.document().version();
    assert!(s.store.commit_gesture("move rect", Actor::Human, 0).is_none());
    assert_eq!(s.store.document().version(), version);
    assert_eq!(get(&s.store, s.a, "transform2d.x"), 500.0);
}

#[test]
fn an_unrelated_edit_mid_drag_leaves_the_drag_running() {
    let mut s = scene();
    let (g, h) = drag(&mut s.store, s.a, [200.0, 200.0]);
    // A colour on the dragged node, and a move of another one.
    let red = Slot::Const(Value::Color([1.0, 0.0, 0.0, 1.0]));
    s.store.apply(tx("recolour", Actor::Model, vec![Op::Set { id: s.a, path: path("fill.color"), slot: red }, set(s.other, "transform2d.x", 400.0)])).unwrap();
    assert_eq!(s.store.gesture_status(g), GestureStatus::Active);
    // The preview is back on top of the edit, and the drag goes on.
    assert_eq!(get(&s.store, s.a, "transform2d.x"), 200.0);
    s.store.drag_to(&h, [250.0, 200.0]);
    s.store.end_drag(&h);
    s.store.commit_gesture("move rect", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(s.store.gesture_status(g), GestureStatus::Committed);
    assert_eq!(get(&s.store, s.a, "transform2d.x"), 250.0);
    assert_eq!(get(&s.store, s.other, "transform2d.x"), 400.0);
    // The other edit's transaction did not carry the drag.
    let recolour = s.store.history().past().iter().find(|t| t.label == "recolour").unwrap();
    assert!(recolour.ops.iter().chain(&recolour.derived_ops).all(|op| !matches!(op, Op::Set { id, .. } if *id == s.a && op != &recolour.ops[0])));
}

#[test]
fn structural_and_binding_changes_cancel() {
    let cases: Vec<(&str, Box<dyn Fn(&Scene) -> Vec<Op>>)> = vec![
        ("delete", Box::new(|s: &Scene| vec![Op::Delete { id: s.a }])),
        ("delete parent", Box::new(|s: &Scene| vec![Op::Delete { id: s.group }])),
        ("reparent", Box::new(|s: &Scene| vec![Op::Reparent { id: s.a, parent: None, order: OrderKey::first() }])),
        ("parent moves", Box::new(|s: &Scene| vec![set(s.group, "transform2d.x", 50.0)])),
        ("rebind", Box::new(|s: &Scene| vec![Op::Set { id: s.a, path: path("transform2d.y"), slot: Slot::Bound(Expr::parse(&format!("{}.transform2d.y", s.other)).unwrap()) }])),
        ("resize", Box::new(|s: &Scene| vec![set(s.a, "size.w", 40.0)])),
    ];
    for (name, ops) in cases {
        let mut s = scene();
        let (g, _) = drag(&mut s.store, s.a, [200.0, 200.0]);
        let ops = ops(&s);
        s.store.apply(tx(name, Actor::Model, ops)).unwrap();
        assert!(matches!(s.store.gesture_status(g), GestureStatus::Cancelled(_)), "{name}: {:?}", s.store.gesture_status(g));
        assert!(!s.store.gesture_in_progress(), "{name}");
        assert!(s.store.commit_gesture("move rect", Actor::Human, 0).is_none(), "{name}");
    }
}

#[test]
fn a_bound_source_gesture_cancels_when_its_source_is_set() {
    let mut s = scene();
    // Dragging the bar sets the slider through the binding's inverse.
    let (g, _) = drag(&mut s.store, s.bar, [80.0, 0.0]);
    assert!((get(&s.store, s.slider, "slider.value") - 0.8).abs() < 1e-9);
    s.store.apply(tx("model sets slider", Actor::Model, vec![set(s.slider, "slider.value", 0.1)])).unwrap();
    assert!(matches!(s.store.gesture_status(g), GestureStatus::Cancelled(ref why) if why.contains("drives")), "{:?}", s.store.gesture_status(g));
    assert!((get(&s.store, s.bar, "transform2d.x") - 10.0).abs() < 1e-9);
}

#[test]
fn a_constrained_gesture_cancels_when_the_solver_moves_its_node() {
    let mut s = scene();
    let (g, h) = drag(&mut s.store, s.q, [100.0, 500.0]);
    assert!(matches!(h.mode, DragMode::Cluster(_)), "{:?}", h.mode);
    // An unrelated edit: the cluster is solved without the drag's finger,
    // and nothing of the drag is written.
    s.store.apply(tx("move other", Actor::Model, vec![set(s.other, "transform2d.x", 400.0)])).unwrap();
    assert_eq!(s.store.gesture_status(g), GestureStatus::Active);
    let last = s.store.history().past().last().unwrap();
    assert!(last.derived_ops.is_empty(), "{:?}", last.derived_ops);
    // A longer link moves the dragged node: the gesture is cancelled.
    s.store.apply(tx("longer link", Actor::Model, vec![set(s.link, "constraint.value", 150.0)])).unwrap();
    assert!(matches!(s.store.gesture_status(g), GestureStatus::Cancelled(_)), "{:?}", s.store.gesture_status(g));
    let (p, q) = ([get(&s.store, s.p, "transform2d.x"), get(&s.store, s.p, "transform2d.y")], [get(&s.store, s.q, "transform2d.x"), get(&s.store, s.q, "transform2d.y")]);
    assert!(((p[0] - q[0]).hypot(p[1] - q[1]) - 150.0).abs() < 1e-6);
}

#[test]
fn undo_mid_gesture_cancels_it_first() {
    let mut s = scene();
    s.store.apply(tx("nudge", Actor::Human, vec![set(s.other, "transform2d.x", 310.0)])).unwrap();
    let (g, _) = drag(&mut s.store, s.a, [200.0, 200.0]);
    s.store.undo(0).unwrap().unwrap();
    assert_eq!(s.store.gesture_status(g), GestureStatus::Cancelled("undo".into()));
    assert_eq!(get(&s.store, s.a, "transform2d.x"), 10.0);
    assert_eq!(get(&s.store, s.other, "transform2d.x"), 300.0);
    assert!(s.store.commit_gesture("move rect", Actor::Human, 0).is_none());
}

#[test]
fn cancel_writes_nothing_and_a_release_writes_one_transaction() {
    let mut s = scene();
    let before = s.store.history().past().len();
    let (g, _) = drag(&mut s.store, s.a, [200.0, 200.0]);
    s.store.cancel_gesture();
    assert_eq!(s.store.gesture_status(g), GestureStatus::Cancelled("cancelled".into()));
    assert_eq!(s.store.history().past().len(), before);
    assert_eq!(get(&s.store, s.a, "transform2d.x"), 10.0);
    let (g2, _) = drag(&mut s.store, s.a, [200.0, 200.0]);
    assert_ne!(g, g2);
    assert_eq!(s.store.gesture_status(g), GestureStatus::Cancelled("cancelled".into()));
    s.store.commit_gesture("move rect", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(s.store.history().past().len(), before + 1);
    assert_eq!(s.store.gesture_status(g2), GestureStatus::Committed);
}

#[test]
fn a_one_off_edit_through_a_handle_leaves_a_drag_alone() {
    let mut s = scene();
    let (g, h) = drag(&mut s.store, s.a, [200.0, 200.0]);
    // The inspector sets the bar's bound x: the slider moves, the drag stays.
    s.store.set_through_handle(s.bar, path("transform2d.x"), Value::Number(30.0), "set x", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(s.store.gesture_status(g), GestureStatus::Active);
    assert!((get(&s.store, s.slider, "slider.value") - 0.3).abs() < 1e-9);
    s.store.end_drag(&h);
    let applied = s.store.commit_gesture("move rect", Actor::Human, 0).unwrap().unwrap();
    // Only the drag's own leaves are in its transaction.
    let tx = s.store.history().past().last().unwrap();
    assert_eq!(tx.version, applied.version);
    assert!(tx.ops.iter().all(|op| matches!(op, Op::Set { id, .. } if *id == s.a)), "{:?}", tx.ops);
}

#[test]
fn an_alt_drag_duplicates_moves_the_copy_and_undoes_in_one_step() {
    let mut s = scene();
    let (a, other) = (s.a, s.other);
    // The drag starts on the original...
    let (g, _) = drag(&mut s.store, a, [200.0, 200.0]);
    // ...Alt goes down: the original goes back, a copy takes the drag.
    s.store.cancel_gesture();
    assert!(matches!(s.store.gesture_status(g), GestureStatus::Cancelled(_)));
    let pasted = copy::duplicate(&s.store, &[a]).unwrap();
    assert_eq!(pasted.sources, vec![a]);
    let dup = s.store.apply(tx("duplicate rect", Actor::Human, pasted.ops)).unwrap().version;
    let copy = pasted.roots[0];
    s.store.begin_gesture(&[copy]);
    let h = s.store.begin_drag(copy, [0.0, 0.0]);
    s.store.drag_to(&h, [220.0, 210.0]);
    assert_eq!(get(&s.store, a, "transform2d.x"), 10.0);
    // Alt comes up: the copy is taken back, the original drags again.
    s.store.cancel_gesture();
    assert!(s.store.retract(dup, 0));
    assert!(!s.store.document().is_live(copy));
    assert!(!s.store.history().can_redo());
    // And down again, then release.
    let pasted = copy::duplicate(&s.store, &[a]).unwrap();
    let dup = s.store.apply(tx("duplicate rect", Actor::Human, pasted.ops)).unwrap().version;
    let copy = pasted.roots[0];
    s.store.begin_gesture(&[copy]);
    let h = s.store.begin_drag(copy, [0.0, 0.0]);
    s.store.drag_to(&h, [240.0, 220.0]);
    s.store.end_drag(&h);
    s.store.commit_gesture("move rect", Actor::Human, 0).unwrap().unwrap();
    assert!(s.store.merge_with_previous(dup, "duplicate rect"));
    assert_eq!(s.store.history().past().last().unwrap().label, "duplicate rect");
    assert_eq!(get(&s.store, copy, "transform2d.x"), 240.0);
    assert_eq!(get(&s.store, a, "transform2d.x"), 10.0);
    // One undo removes the copy; one redo brings it back where it was left.
    s.store.undo(0).unwrap().unwrap();
    assert!(!s.store.document().is_live(copy));
    s.store.redo(0).unwrap().unwrap();
    assert_eq!(get(&s.store, copy, "transform2d.x"), 240.0);
    // Merging is refused when the previous entry is another.
    s.store.apply(tx("nudge", Actor::Human, vec![set(other, "transform2d.x", 1.0)])).unwrap();
    assert!(!s.store.merge_with_previous(dup, "x"));
}
