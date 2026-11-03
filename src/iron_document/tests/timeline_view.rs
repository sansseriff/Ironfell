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
}

/// rect (x=50) animated by a clip (start 1, dur 2, x 100 -> 300), and a
/// group with an axis (t0 0, pps 100, label_w 160) standing in for a
/// timeline view.
fn scene() -> Scene {
    let mut store = Store::new();
    let ids = store.document().next_ids(4);
    let (rect, timeline, clip, view) = (ids[0], ids[1], ids[2], ids[3]);
    let rel = store.document().next_relation_id();
    let mut t = Transform2d::default();
    t.x = num(50.0);
    let mut timing = Timing::default();
    timing.start = num(1.0);
    timing.dur = num(2.0);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("transform2d.x".into()));
    c.from = num(100.0);
    c.to = num(300.0);
    let k0 = OrderKey::first();
    let k1 = OrderKey::after(&k0);
    let k2 = OrderKey::after(&k1);
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: rect, ty: ty("rect"), parent: None, order: k0.clone(), facets: vec![t.wrap(), Size::default().wrap()] },
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: k1, facets: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: k0, facets: vec![timing.wrap(), c.wrap()] },
                Op::Link { rel: Relation { id: rel, from: clip, to: rect, rel: RelKind::Animates, data: BTreeMap::new() } },
                Op::Create { id: view, ty: ty("group"), parent: None, order: k2, facets: vec![Axis::default().wrap()] },
            ],
        ))
        .unwrap();
    store.drain_changes();
    Scene { store, rect, clip, view }
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

#[test]
fn the_timeline_is_built_from_the_clips() {
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    assert!(comp.is_session());
    let seg = node_named(&store, comp, &format!("bar {clip} {rect}"));
    assert_eq!(x_of(&store, seg), 260.0, "label_w + start * pps");
    assert_eq!(store.resolved(seg, path("size.w")).unwrap(), Value::Number(200.0));
    assert!(store.document().relations_from(seg).any(|r| r.rel == RelKind::Projects && r.to == clip), "a segment projects its clip");
    let mut rows: Vec<String> = names_under(&store, comp).into_iter().filter(|k| k.starts_with("object") || k.starts_with("property")).collect();
    rows.sort();
    assert_eq!(rows, vec![format!("object {rect}"), format!("property {rect} transform2d.x")]);
    assert!(timeline_view::sync(&mut store, &mut last).unwrap().is_none(), "nothing changed, nothing regenerated");
    assert!(!store.save().contains("segment"), "the timeline is session state");
}

#[test]
fn dragging_a_segment_retimes_its_clip() {
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let seg = node_named(&store, comp, &format!("bar {clip} {rect}"));
    let h = store.begin_drag(seg, [0.0, 0.0]);
    match &h.mode {
        DragMode::Axes { free, .. } => assert_eq!(*free, [true, false], "segments move only in time"),
        m => panic!("{m:?}"),
    }
    store.drag_to(&h, [410.0, 500.0]);
    store.commit_gesture("retime", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(store.document().leaf(clip, path("timing.start")).unwrap().constant(), Some(&Value::Number(2.5)));
    assert_eq!(x_of(&store, seg), 410.0);
    assert!(timeline_view::sync(&mut store, &mut last).unwrap().is_none(), "retiming does not regenerate");
    store.undo(0).unwrap().unwrap();
    assert_eq!(x_of(&store, seg), 260.0);
}

#[test]
fn the_static_pieces_do_not_move_and_the_playhead_scrubs() {
    let Scene { mut store, rect, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    for key in ["background", "header", "ruler bar", "tick 1", "tick label 1", &format!("object {rect}")] {
        let id = node_named(&store, comp, key);
        assert!(matches!(store.begin_drag(id, [0.0, 0.0]).mode, DragMode::Locked(_)), "{key} is locked");
    }
    let grip = node_named(&store, comp, "playhead grip");
    let past = store.history().past().len();
    let h = store.begin_drag(grip, [8.0, 0.0]);
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
    let seg = node_named(&store, comp, &format!("bar {clip} {rect}"));
    // A second clip on the rect, on another property.
    let ids = store.document().next_ids(1);
    let timeline = store.document().live(clip).unwrap().parent.unwrap();
    let mut timing = Timing::default();
    timing.start = num(3.0);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("transform2d.y".into()));
    let rel = store.document().next_relation_id();
    store
        .apply(tx(
            "add clip",
            vec![
                Op::Create { id: ids[0], ty: ty("clip"), parent: Some(timeline), order: OrderKey::after(&OrderKey::first()), facets: vec![timing.wrap(), c.wrap()] },
                Op::Link { rel: Relation { id: rel, from: ids[0], to: rect, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    assert!(timeline_view::sync(&mut store, &mut last).unwrap().is_some());
    assert_eq!(node_named(&store, comp, &format!("bar {clip} {rect}")), seg, "the old bar kept its id");
    let second = node_named(&store, comp, &format!("bar {} {rect}", ids[0]));
    assert_eq!(x_of(&store, second), 460.0);
    // Deleting the clip removes its segment.
    store.apply(tx("delete", vec![Op::Delete { id: ids[0] }])).unwrap();
    timeline_view::sync(&mut store, &mut last).unwrap();
    assert!(!store.document().is_live(second));
    let _ = Axis::default();
}

#[test]
fn an_end_value_types_into_the_clip() {
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let to = node_named(&store, comp, &format!("to {clip} {rect}"));
    assert_eq!(store.resolved(to, path("text.content")).unwrap(), Value::Str("300".into()));
    assert_eq!(x_of(&store, to), 200.0 + 4.0, "just past the bar's end, in the bar");
    store.preview_target(to, path("text.content"), Value::Str("650".into())).unwrap();
    store.commit_gesture("set to", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(store.document().leaf(clip, path("clip.to")).unwrap().constant(), Some(&Value::Number(650.0)));
}

#[test]
fn the_axis_pans_zooms_and_fits_without_history() {
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let seg = node_named(&store, comp, &format!("bar {clip} {rect}"));
    let past = store.history().past().len();
    // Zoom in 2x about x = 360 (t = 2 s): 2 s stays at 360, the clip's
    // start (1 s) moves from 260 to 160.
    timeline_view::navigate(&mut store, 0.0, -(2f64.ln()), 360.0).unwrap();
    assert!((x_of(&store, seg) - 160.0).abs() < 1e-9, "{}", x_of(&store, seg));
    assert!((store.resolved(seg, path("size.w")).unwrap().as_f64().unwrap() - 400.0).abs() < 1e-9);
    // Pan 100 px later: 0.5 s at 200 px/s.
    timeline_view::navigate(&mut store, 100.0, 0.0, 0.0).unwrap();
    assert!((x_of(&store, seg) - 60.0).abs() < 1e-9, "{}", x_of(&store, seg));
    timeline_view::fit(&mut store, 1184.0).unwrap();
    assert_eq!(store.resolved(comp, path("axis.t0")).unwrap(), Value::Number(0.0));
    assert_eq!(store.resolved(comp, path("axis.pps")).unwrap(), Value::Number(1000.0 / 3.0), "3 s of clips in 1000 px");
    assert_eq!(store.history().past().len(), past, "navigation is not an edit");
    // A zoom changes the tick spacing, so the ruler regenerates.
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
fn animate_change_turns_the_last_edit_into_a_clip() {
    let Scene { mut store, rect, .. } = scene();
    store.set_time(4.0);
    store.apply(tx("move rect", vec![Op::Set { id: rect, path: path("transform2d.y"), slot: num(80.0) }])).unwrap();
    let ops = timeline_view::animate_change_ops(&store, &[rect], store.time()).unwrap();
    store.apply(tx("animate change", ops)).unwrap();
    assert_eq!(store.document().leaf(rect, path("transform2d.y")).unwrap().constant(), Some(&Value::Number(0.0)), "the authored value went back");
    store.set_time(4.0);
    assert_eq!(store.resolved(rect, path("transform2d.y")).unwrap(), Value::Number(0.0), "at the start the clip holds the old value");
    store.set_time(5.0);
    assert_eq!(store.resolved(rect, path("transform2d.y")).unwrap(), Value::Number(80.0), "after a second, the new one");
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let rows: usize = names_under(&store, comp).iter().filter(|s| s.starts_with("property ")).count();
    assert_eq!(rows, 2, "the rect has an x row and now a y row");
    // One undo takes the whole thing back.
    store.undo(0).unwrap().unwrap();
    assert_eq!(store.document().leaf(rect, path("transform2d.y")).unwrap().constant(), Some(&Value::Number(80.0)));
    // Nothing of the selection changed: it says so.
    let Scene { clip, .. } = scene();
    assert!(timeline_view::animate_change_ops(&store, &[clip], 0.0).is_err());
}

#[test]
fn what_each_piece_affords_is_its_groups_context() {
    use iron_document::contexts::{chain_of, Field};
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let values = |key: &str| {
        let id = node_named(&store, comp, key);
        let chain = chain_of(store.document(), id);
        store.resolver().values(&chain, chain.len() - 1)
    };
    let bar = values(&format!("bar {clip} {rect}"));
    assert!(bar.selectable() && bar.is(Field::DRAG, "point") && bar.has(Field::GRIPS, "ends") && bar.is(Field::DELETE, "subject"), "a bar selects, drags, trims by its ends and deletes its clip");
    for key in ["background", "header", "grid 1", &format!("object {rect}"), &format!("segment {clip} {rect}")] {
        let v = values(key);
        assert!(v.is(Field::SELECT, "none") && v.source(Field::SELECT).is_some(), "{key} is declared no target");
    }
    for key in [format!("segment label {clip} {rect}"), "playhead".to_owned()] {
        assert!(values(&key).is(Field::SELECT, "inert"), "{key} is transparent to the pointer");
    }
    let to = values(&format!("to {clip} {rect}"));
    assert!(to.selectable() && to.is(Field::EDIT, "text") && to.is(Field::DRAG, "none"), "an end value, entered from its bar, edits and never drags");
    // The ruler group is the scrub control: a track whose value is the clock.
    let ruler = node_named(&store, comp, "ruler bar");
    let ruler_group = store.document().live(ruler).unwrap().parent.unwrap();
    let chain = chain_of(store.document(), ruler_group);
    let v = store.resolver().values(&chain, chain.len() - 1);
    assert!(v.is(Field::DRAG, "track") && v.get(Field::DRIVES) == "track.value");
    for key in ["ruler bar", "tick 3", "tick label 1", "playhead grip"] {
        assert!(!values(key).selectable(), "{key} is part of the ruler, not a target");
    }
    let n = |f: &str| store.resolved(ruler_group, path(f)).and_then(|v| v.as_f64()).unwrap();
    // Local x 360 is 2 s at the default axis (label_w 160, 100 px/s).
    assert_eq!(n("track.origin") + 360.0 * n("track.scale"), 2.0);
    store.preview_target(ruler_group, path("track.value"), Value::Number(2.0)).unwrap();
    assert_eq!(store.time(), 2.0, "the track's value is the playhead");
    assert!(store.commit_gesture("scrub", Actor::Human, 0).is_none(), "scrubbing is not an edit");
}

#[test]
fn a_bar_trims_by_its_ends_and_deleting_it_deletes_the_clip() {
    let Scene { mut store, rect, clip, .. } = scene();
    let mut last = None;
    let comp = build(&mut store, &mut last);
    let bar = node_named(&store, comp, &format!("bar {clip} {rect}"));
    // The right end: width 200 -> 300 is dur 2 -> 3.
    store.preview_target(bar, path("size.w"), Value::Number(300.0)).unwrap();
    store.commit_gesture("trim slide", Actor::Human, 0).unwrap().unwrap();
    assert_eq!(store.document().leaf(clip, path("timing.dur")).unwrap().constant(), Some(&Value::Number(3.0)));
    // The left end, 1 s later: x 260 -> 360 and width 300 -> 200, the
    // right end staying: start 1 -> 2, dur 3 -> 2.
    store.preview_target(bar, path("transform2d.x"), Value::Number(360.0)).unwrap();
    store.preview_target(bar, path("size.w"), Value::Number(200.0)).unwrap();
    store.commit_gesture("trim slide", Actor::Human, 0).unwrap().unwrap();
    let n = |f: &str| store.document().leaf(clip, path(f)).unwrap().constant().cloned();
    assert_eq!((n("timing.start"), n("timing.dur")), (Some(Value::Number(2.0)), Some(Value::Number(2.0))));
    // Deleting the bar is deleting the clip; the timeline drops the bar.
    let ops = iron_document::parts::delete_subject_ops(store.document(), bar);
    assert_eq!(ops, vec![Op::Delete { id: clip }]);
    store.apply(tx("delete clip", ops)).unwrap();
    timeline_view::sync(&mut store, &mut last).unwrap();
    assert!(!store.document().is_live(bar));
    store.undo(0).unwrap().unwrap();
    timeline_view::sync(&mut store, &mut last).unwrap();
    assert!(names_under(&store, comp).contains(&format!("bar {clip} {rect}")), "undo brings the clip and its bar back");
}
