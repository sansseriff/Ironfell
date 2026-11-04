//! The generated timeline composition (plans/specs/timeline.md phase 2).

use iron_document::facet::{Axis, Clip, Name, Size, Timing, Transform2d};
use iron_document::timeline_view;
use iron_document::*;
use std::collections::BTreeMap;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
}

fn bound(e: &str) -> Slot {
    Slot::Bound(Expr::parse(e).unwrap())
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn x_of(store: &Store, id: NodeId) -> f64 {
    store.resolved(id, path("transform2d.x")).and_then(|v| v.as_f64()).unwrap()
}

struct Scene {
    store: Store,
    rect: NodeId,
    clip: NodeId,
    view: NodeId,
    keys: [NodeId; 2],
}

/// rect (x=50) animated by a clip (start 1; keys at 0 (x 100) and 2 (x 300)),
/// and a
/// group with an axis (t0 0, pps 100, label_w 160) standing in for a
/// timeline view.
fn scene() -> Scene {
    let mut store = Store::new();
    let ids = store.document().next_ids(6);
    let key_ids = [ids[3], ids[4]];
    let (rect, timeline, clip, view) = (ids[0], ids[1], ids[2], ids[5]);
    let rel = store.document().next_relation_id();
    let mut t = Transform2d::default();
    t.x = num(50.0);
    let mut timing = Timing::default();
    timing.start = num(1.0);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("transform2d.x".into()));
    let key = |t: f64, v: f64| {
        let mut k = iron_document::facet::Key::default();
        k.t = num(t);
        k.value = num(v);
        k.wrap()
    };
    let k0 = OrderKey::first();
    let k1 = OrderKey::after(&k0);
    let k2 = OrderKey::after(&k1);
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: rect, ty: ty("rect"), parent: None, order: k0.clone(), facets: vec![t.wrap(), Size::default().wrap()] },
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: k1, facets: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: OrderKey::first(), facets: vec![timing.wrap(), c.wrap()] },
                Op::Create { id: key_ids[0], ty: ty("key"), parent: Some(clip), order: OrderKey::first(), facets: vec![key(0.0, 100.0)] },
                Op::Create { id: key_ids[1], ty: ty("key"), parent: Some(clip), order: OrderKey::after(&OrderKey::first()), facets: vec![key(2.0, 300.0)] },
                Op::Link { rel: Relation { id: rel, from: clip, to: rect, rel: RelKind::Animates, data: BTreeMap::new() } },
                Op::Create { id: view, ty: ty("group"), parent: None, order: k2, facets: vec![Axis::default().wrap()] },
            ],
        ))
        .unwrap();
    store.drain_changes();
    Scene { store, rect, clip, view, keys: key_ids }
}

/// A node in `view` whose x is `expr`.
fn add_bar(store: &mut Store, view: NodeId, expr: &str) -> NodeId {
    let id = store.document().next_ids(1)[0];
    let mut t = Transform2d::default();
    t.x = bound(expr);
    store.apply(tx("bar", vec![Op::Create { id, ty: ty("rect"), parent: Some(view), order: OrderKey::first(), facets: vec![t.wrap(), Size::default().wrap()] }])).unwrap();
    id
}


fn node_named(store: &Store, comp: NodeId, key: &str) -> NodeId {
    let doc = store.document();
    let mut stack = doc.children(comp).to_vec();
    while let Some(k) = stack.pop() {
        if doc.get::<Name>(k).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(key) {
            return k;
        }
        stack.extend(doc.children(k).iter().copied());
    }
    panic!("no {key}")
}

fn names_under(store: &Store, comp: NodeId) -> Vec<String> {
    let doc = store.document();
    let mut out = Vec::new();
    let mut stack = doc.children(comp).to_vec();
    while let Some(k) = stack.pop() {
        if let Some(n) = doc.get::<Name>(k).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) {
            out.push(n.to_owned());
        }
        stack.extend(doc.children(k).iter().copied());
    }
    out
}

fn build(store: &mut Store, last: &mut Option<(NodeId, timeline_view::Plan)>) -> NodeId {
    timeline_view::sync(store, last).unwrap();
    timeline_view::find(store).expect("built")
}

fn key_of(store: &Store, key: NodeId, f: &str) -> Value {
    store.document().leaf(key, path(&format!("key.{f}"))).unwrap().constant().cloned().unwrap()
}

/// Preview `x` on the rect as a drag does, at clock time `t`, and commit.
fn drag_rect_to(store: &mut Store, rect: NodeId, field: &str, t: f64, x: f64) -> bool {
    store.set_time(t);
    store.begin_gesture(&[rect]);
    store.preview_target(rect, path(&format!("transform2d.{field}")), Value::Number(x)).unwrap();
    store.commit_gesture("move rect", Actor::Human, 0).is_some_and(|r| r.is_ok())
}

fn set_autokey(store: &mut Store, on: bool) {
    let clock = store.ensure_clock().unwrap();
    store.apply_session(vec![Op::Set { id: clock, path: path("clock.autokey"), slot: Slot::Const(Value::Bool(on)) }]).unwrap();
}

#[test]
fn the_timeline_is_built_from_the_clips_and_their_keys() {
    let Scene { mut store, rect, clip, keys, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    assert!(comp.is_session());
    let bar = node_named(&store, comp, &format!("bar {clip} {rect}"));
    assert_eq!(x_of(&store, bar), 260.0, "label_w + (start + first key) * pps");
    assert_eq!(store.resolved(bar, path("size.w")).unwrap(), Value::Number(200.0), "first key to last");
    assert!(store.document().relations_from(bar).any(|r| r.rel == RelKind::Projects && r.to == clip), "a bar projects its clip");
    let diamond = node_named(&store, comp, &format!("key {} {rect}", keys[1]));
    assert_eq!(x_of(&store, diamond), 460.0, "a key's diamond at its time");
    assert!(store.document().relations_from(diamond).any(|r| r.rel == RelKind::Projects && r.to == keys[1]), "a diamond projects its key");
    // The rows the DOM column draws (§12.3).
    let rows = timeline_view::rows(&last.as_ref().unwrap().1);
    let shape: Vec<(&str, String)> = rows.iter().map(|r| (r.kind, r.label.clone())).collect();
    assert_eq!(shape, vec![("object", format!("rect {rect}")), ("property", "x".to_owned())]);
    assert_eq!(rows[1].clip, Some(clip));
    assert!(timeline_view::sync(&mut store, &mut last).unwrap().is_none(), "nothing changed, nothing regenerated");
    assert!(!store.save().contains("summary"), "the timeline is session state");
}

#[test]
fn a_bar_drag_retimes_the_clip_and_a_diamond_drag_moves_its_key() {
    let Scene { mut store, rect, clip, keys, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let bar = node_named(&store, comp, &format!("bar {clip} {rect}"));
    let h = store.begin_drag(bar, [0.0, 0.0]);
    match &h.mode {
        DragMode::Axes { free, .. } => assert_eq!(*free, [true, false], "bars move only in time"),
        m => panic!("{m:?}"),
    }
    store.drag_to(&h, [410.0, 500.0]);
    store.commit_gesture("retime", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(store.document().leaf(clip, path("timing.start")).unwrap().constant(), Some(&Value::Number(2.5)), "the bar moves the clip");
    assert_eq!(key_of(&store, keys[1], "t"), Value::Number(2.0), "its keys keep their times in it");
    store.undo(0).unwrap().unwrap();
    // A diamond reads the clip's start and its key's time; it projects the
    // key, so the drag moves the key.
    let diamond = node_named(&store, comp, &format!("key {} {rect}", keys[1]));
    match store.handle(diamond, path("transform2d.x")) {
        Handle::Linear { source, .. } => assert_eq!(source, Leaf { node: keys[1], path: path("key.t") }),
        h => panic!("{h:?}"),
    }
    store.preview_target(diamond, path("transform2d.x"), Value::Number(510.0)).unwrap();
    store.commit_gesture("move key", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(key_of(&store, keys[1], "t"), Value::Number(2.5));
    assert!(timeline_view::sync(&mut store, &mut last).unwrap().is_none(), "the key order is the same: no regeneration");
    let bar = node_named(&store, comp, &format!("bar {clip} {rect}"));
    assert_eq!(store.resolved(bar, path("size.w")).unwrap(), Value::Number(250.0), "the bar widens through its binding");
}

#[test]
fn the_static_pieces_do_not_move_and_the_playhead_scrubs() {
    let Scene { mut store, rect, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    for key in ["background", "header", "ruler bar", "tick 1", "tick label 1", &format!("summary {rect}")] {
        let id = node_named(&store, comp, key);
        assert!(matches!(store.begin_drag(id, [0.0, 0.0]).mode, DragMode::Locked(_)), "{key} is locked");
    }
    let grip = node_named(&store, comp, "playhead grip");
    let past = store.history().past().len();
    let h = store.begin_drag(grip, [6.0, 0.0]);
    store.drag_to(&h, [360.0, 0.0]);
    assert_eq!(store.time(), 2.0);
    assert_eq!(x_of(&store, rect), 200.0, "the clip samples at the dragged playhead");
    assert!(store.commit_gesture("scrub", Actor::Human, 0).is_none());
    assert_eq!(store.history().past().len(), past, "scrubbing is not an edit");
}

#[test]
fn regeneration_keeps_what_survives() {
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let bar = node_named(&store, comp, &format!("bar {clip} {rect}"));
    // A second clip on the rect, on another property.
    let ids = store.document().next_ids(3);
    let timeline = store.document().live(clip).unwrap().parent.unwrap();
    let mut timing = Timing::default();
    timing.start = num(3.0);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("transform2d.y".into()));
    let key = |t: f64, v: f64| {
        let mut k = iron_document::facet::Key::default();
        k.t = num(t);
        k.value = num(v);
        k.wrap()
    };
    let rel = store.document().next_relation_id();
    store
        .apply(tx(
            "add clip",
            vec![
                Op::Create { id: ids[0], ty: ty("clip"), parent: Some(timeline), order: OrderKey::after(&OrderKey::first()), facets: vec![timing.wrap(), c.wrap()] },
                Op::Create { id: ids[1], ty: ty("key"), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![key(0.0, 0.0)] },
                Op::Create { id: ids[2], ty: ty("key"), parent: Some(ids[0]), order: OrderKey::after(&OrderKey::first()), facets: vec![key(1.0, 10.0)] },
                Op::Link { rel: Relation { id: rel, from: ids[0], to: rect, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    assert!(timeline_view::sync(&mut store, &mut last).unwrap().is_some());
    assert_eq!(node_named(&store, comp, &format!("bar {clip} {rect}")), bar, "the old bar kept its id");
    let second = node_named(&store, comp, &format!("bar {} {rect}", ids[0]));
    assert_eq!(x_of(&store, second), 460.0);
    store.apply(tx("delete", vec![Op::Delete { id: ids[0] }])).unwrap();
    timeline_view::sync(&mut store, &mut last).unwrap();
    assert!(!store.document().is_live(second));
}

#[test]
fn the_axis_pans_zooms_and_fits_without_history() {
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let bar = node_named(&store, comp, &format!("bar {clip} {rect}"));
    let past = store.history().past().len();
    timeline_view::navigate(&mut store, 0.0, -(2f64.ln()), 360.0).unwrap();
    assert!((x_of(&store, bar) - 160.0).abs() < 1e-9, "{}", x_of(&store, bar));
    assert!((store.resolved(bar, path("size.w")).unwrap().as_f64().unwrap() - 400.0).abs() < 1e-9);
    timeline_view::navigate(&mut store, 100.0, 0.0, 0.0).unwrap();
    assert!((x_of(&store, bar) - 60.0).abs() < 1e-9, "{}", x_of(&store, bar));
    timeline_view::fit(&mut store, 1184.0).unwrap();
    assert_eq!(store.resolved(comp, path("axis.t0")).unwrap(), Value::Number(0.0));
    assert_eq!(store.resolved(comp, path("axis.pps")).unwrap(), Value::Number(1000.0 / 3.0), "3 s of keys in 1000 px");
    assert_eq!(store.history().past().len(), past, "navigation is not an edit");
    assert!(timeline_view::sync(&mut store, &mut last).unwrap().is_some());
}

#[test]
fn the_loop_band_follows_the_clock() {
    let Scene { mut store, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let band = node_named(&store, comp, "loop");
    assert_eq!(store.resolved(band, path("size.w")).unwrap(), Value::Number(0.0), "no loop, no band");
    let clock = store.clock().unwrap();
    store.apply_session(vec![Op::Set { id: clock, path: path("clock.loop_in"), slot: num(1.0) }, Op::Set { id: clock, path: path("clock.loop_out"), slot: num(2.5) }]).unwrap();
    assert_eq!(x_of(&store, band), 260.0);
    assert_eq!(store.resolved(band, path("size.w")).unwrap(), Value::Number(150.0));
}

#[test]
fn what_each_piece_affords_is_its_groups_context() {
    use iron_document::contexts::{chain_of, Field};
    let Scene { mut store, rect, clip, keys, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let values = |key: &str| {
        let id = node_named(&store, comp, key);
        let chain = chain_of(store.document(), id);
        store.resolver().values(&chain, chain.len() - 1)
    };
    let bar = values(&format!("bar {clip} {rect}"));
    assert!(bar.selectable() && bar.is(Field::DRAG, "point") && bar.is(Field::DELETE, "subject"), "a bar selects, drags and deletes its clip");
    let key = values(&format!("key {} {rect}", keys[0]));
    assert!(key.selectable() && key.is(Field::DRAG, "point") && key.is(Field::DELETE, "subject"), "a diamond selects, drags and deletes its key");
    assert_eq!((key.get(Field::CURSOR), values(&format!("key {} {rect}", keys[1])).get(Field::CURSOR)), ("trim-start", "trim-end"), "a clip's first and last keys say they trim it");
    assert_eq!(bar.get(Field::CURSOR), "auto");
    for k in ["background", "header", "grid 1", &format!("summary {rect}")] {
        let v = values(k);
        assert!(v.is(Field::SELECT, "none") && v.source(Field::SELECT).is_some(), "{k} is declared no target");
    }
    for k in [format!("bar label {clip} {rect}"), "playhead".to_owned()] {
        assert!(values(&k).is(Field::SELECT, "inert"), "{k} is transparent to the pointer");
    }
    let ruler = node_named(&store, comp, "ruler bar");
    let ruler_group = store.document().live(ruler).unwrap().parent.unwrap();
    let chain = chain_of(store.document(), ruler_group);
    let v = store.resolver().values(&chain, chain.len() - 1);
    assert!(v.is(Field::DRAG, "track") && v.get(Field::DRIVES) == "track.value");
    let n = |f: &str| store.resolved(ruler_group, path(f)).and_then(|v| v.as_f64()).unwrap();
    assert_eq!(n("track.origin") + 360.0 * n("track.scale"), 2.0);
}

#[test]
fn deleting_a_bar_deletes_its_clip_and_a_diamond_its_key() {
    let Scene { mut store, rect, clip, keys, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let diamond = node_named(&store, comp, &format!("key {} {rect}", keys[1]));
    assert_eq!(iron_document::parts::delete_subject_ops(store.document(), diamond), vec![Op::Delete { id: keys[1] }]);
    let bar = node_named(&store, comp, &format!("bar {clip} {rect}"));
    let ops = iron_document::parts::delete_subject_ops(store.document(), bar);
    assert_eq!(ops, vec![Op::Delete { id: clip }]);
    store.apply(tx("delete clip", ops)).unwrap();
    timeline_view::sync(&mut store, &mut last).unwrap();
    assert!(!store.document().is_live(bar));
    store.undo(0).unwrap().unwrap();
    timeline_view::sync(&mut store, &mut last).unwrap();
    assert!(names_under(&store, comp).contains(&format!("bar {clip} {rect}")), "undo brings the clip and its bar back");
}

// ---- §12.2: what a drag on an animated object writes ------------------------

#[test]
fn on_a_key_a_drag_sets_that_key() {
    let Scene { mut store, rect, keys, .. } = scene();
    assert!(drag_rect_to(&mut store, rect, "x", 3.0, 350.0));
    assert_eq!(key_of(&store, keys[1], "value"), Value::Number(350.0));
    assert_eq!(key_of(&store, keys[0], "value"), Value::Number(100.0));
    assert_eq!(store.document().leaf(rect, path("transform2d.x")).unwrap().constant(), Some(&Value::Number(50.0)), "the authored value is untouched");
    assert_eq!(x_of(&store, rect), 350.0);
}

#[test]
fn between_keys_with_auto_keyframe_off_the_whole_animation_moves() {
    let Scene { mut store, rect, keys, .. } = scene();
    // At 2 s the rect shows 200; dragged to 250, every key moves by 50.
    assert!(drag_rect_to(&mut store, rect, "x", 2.0, 250.0));
    assert_eq!((key_of(&store, keys[0], "value"), key_of(&store, keys[1], "value")), (Value::Number(150.0), Value::Number(350.0)));
    assert_eq!(x_of(&store, rect), 250.0);
    // After the last key too: the animation, not just its end, moves.
    assert!(drag_rect_to(&mut store, rect, "x", 9.0, 300.0));
    assert_eq!((key_of(&store, keys[0], "value"), key_of(&store, keys[1], "value")), (Value::Number(100.0), Value::Number(300.0)));
    // Before the clip starts, nothing drives the leaf: the authored value.
    assert!(drag_rect_to(&mut store, rect, "x", 0.5, 70.0));
    assert_eq!(store.document().leaf(rect, path("transform2d.x")).unwrap().constant(), Some(&Value::Number(70.0)));
    store.undo(0).unwrap().unwrap();
    store.undo(0).unwrap().unwrap();
    assert_eq!(key_of(&store, keys[1], "value"), Value::Number(350.0), "each drag is one undo");
}

#[test]
fn with_auto_keyframe_on_a_drag_adds_a_key_at_the_playhead() {
    let Scene { mut store, rect, clip, .. } = scene();
    set_autokey(&mut store, true);
    assert!(drag_rect_to(&mut store, rect, "x", 2.0, 260.0));
    let ks = iron_document::time::keys(store.document(), clip);
    assert_eq!(ks.iter().map(|k| (k.t, k.value.clone())).collect::<Vec<_>>(), vec![(0.0, Value::Number(100.0)), (1.0, Value::Number(260.0)), (2.0, Value::Number(300.0))]);
    // Before the clip: its start moves to the playhead, the keys keep their times.
    assert!(drag_rect_to(&mut store, rect, "x", 0.5, 80.0));
    assert_eq!(store.document().leaf(clip, path("timing.start")).unwrap().constant(), Some(&Value::Number(0.5)));
    let ks = iron_document::time::keys(store.document(), clip);
    assert_eq!(ks.iter().map(|k| k.t).collect::<Vec<_>>(), vec![0.0, 0.5, 1.5, 2.5]);
    store.set_time(0.5);
    assert_eq!(x_of(&store, rect), 80.0);
    store.set_time(3.0);
    assert_eq!(x_of(&store, rect), 300.0, "the old keys play as before");
}

#[test]
fn with_auto_keyframe_on_an_unanimated_leaf_starts_an_animation() {
    let Scene { mut store, rect, .. } = scene();
    set_autokey(&mut store, true);
    // y is not animated: at 2 s a drag makes a clip from y's value at 0 to
    // the dragged one at 2 s. The authored y stays.
    assert!(drag_rect_to(&mut store, rect, "y", 2.0, 40.0));
    let clip = timeline_view::clip_of(store.document(), Leaf { node: rect, path: path("transform2d.y") }).unwrap().0;
    let ks = iron_document::time::keys(store.document(), clip);
    assert_eq!(ks.iter().map(|k| (k.t, k.value.clone())).collect::<Vec<_>>(), vec![(0.0, Value::Number(0.0)), (2.0, Value::Number(40.0))]);
    assert_eq!(store.document().leaf(rect, path("transform2d.y")).unwrap().constant(), Some(&Value::Number(0.0)));
    store.set_time(1.0);
    assert_eq!(store.resolved(rect, path("transform2d.y")).unwrap(), Value::Number(20.0));
    // With auto-keyframe off the same drag is an ordinary edit.
    set_autokey(&mut store, false);
    let Scene { store: mut plain, rect: r2, .. } = scene();
    assert!(drag_rect_to(&mut plain, r2, "y", 2.0, 40.0));
    assert_eq!(plain.document().leaf(r2, path("transform2d.y")).unwrap().constant(), Some(&Value::Number(40.0)));
}

#[test]
fn the_rows_buttons_step_and_toggle_keys() {
    let Scene { mut store, rect, clip, keys, .. } = scene();
    store.set_time(1.5);
    assert_eq!(timeline_view::step_key(&store, clip, 1), Some(3.0), "› the next key, on the clock");
    assert_eq!(timeline_view::step_key(&store, clip, -1), Some(1.0), "‹ the previous");
    // ◇ between keys: a key here, holding what shows (175 at 1.5 s).
    let ops = timeline_view::toggle_key_ops(&store, clip).unwrap();
    store.apply(tx("add key", ops)).unwrap();
    let ks = iron_document::time::keys(store.document(), clip);
    assert_eq!(ks.len(), 3);
    assert_eq!((ks[1].t, ks[1].value.clone()), (0.5, Value::Number(150.0)));
    // ◇ on a key: removes it.
    store.set_time(3.0);
    let ops = timeline_view::toggle_key_ops(&store, clip).unwrap();
    assert_eq!(ops, vec![Op::Delete { id: keys[1] }]);
    let _ = rect;
}

#[test]
fn scrolling_moves_the_rows_under_a_ruler_that_stays() {
    let Scene { mut store, rect, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let y = |store: &Store, key: &str| store.resolved(node_named(store, comp, key), path("transform2d.y")).and_then(|v| v.as_f64()).unwrap();
    let summary = format!("summary {rect}");
    let (row, ruler) = (y(&store, &summary), y(&store, "ruler bar"));
    let past = store.history().past().len();
    // Only one row pair: the scroll stops with the last row under the ruler.
    timeline_view::scroll(&mut store, 1000.0).unwrap();
    let scrolled = y(&store, &summary);
    assert!(scrolled < row, "the rows moved up: {row} → {scrolled}");
    assert_eq!(y(&store, "ruler bar"), ruler, "the ruler stays");
    timeline_view::scroll(&mut store, -5000.0).unwrap();
    assert_eq!(y(&store, &summary), row, "back to the top, and no further");
    assert_eq!(store.history().past().len(), past, "scrolling is not an edit");
}
