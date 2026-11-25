//! Scripted time: clips sampled at the playhead through relations.

use iron_document::facet::{Clip, Fill, Key, Size, Timing, Transform2d};
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

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn x_of(store: &Store, id: NodeId) -> f64 {
    store.resolved(id, path("transform2d.x")).and_then(|v| v.as_f64()).unwrap()
}

struct Scene {
    store: Store,
    rect: NodeId,
    bar: NodeId,
    clip: NodeId,
    rel: RelationId,
    keys: [NodeId; 2],
}

/// rect (x=50); bar.size.h bound to rect.x * 2; timeline > clip(start 1,
/// keys at 0 (100) and 2 (300) on transform2d.x) animating rect.
fn scene(easing: &str) -> Scene {
    let mut store = Store::new();
    let ids = store.authored().next_ids(6);
    let (rect, bar, timeline, clip) = (ids[0], ids[1], ids[2], ids[3]);
    let keys = [ids[4], ids[5]];
    let rel = store.authored().next_relation_id();
    let mut t = Transform2d::default();
    t.x = num(50.0);
    let mut s = Size::default();
    s.h = Slot::Bound(Expr::parse(&format!("{rect}.transform2d.x * 2")).unwrap());
    let mut timing = Timing::default();
    timing.start = num(1.0);
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("transform2d.x".into()));
    let key = |t: f64, v: f64| {
        let mut k = Key::default();
        k.t = num(t);
        k.value = num(v);
        k.easing = Slot::Const(Value::Enum(easing.into()));
        k.wrap()
    };
    let k0 = OrderKey::first();
    store
        .apply(tx(
            "build",
            vec![
                Op::Create { id: rect, ty: ty("rect"), parent: None, order: k0.clone(), facets: vec![t.wrap(), Size::default().wrap()] },
                Op::Create {
                    id: bar,
                    ty: ty("rect"),
                    parent: None,
                    order: OrderKey::after(&k0),
                    facets: vec![Transform2d::default().wrap(), s.wrap(), Fill::default().wrap()],
                },
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: OrderKey::after(&OrderKey::after(&k0)), facets: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: k0.clone(), facets: vec![timing.wrap(), c.wrap()] },
                Op::Create { id: keys[0], ty: ty("key"), parent: Some(clip), order: k0.clone(), facets: vec![key(0.0, 100.0)] },
                Op::Create { id: keys[1], ty: ty("key"), parent: Some(clip), order: OrderKey::after(&k0), facets: vec![key(2.0, 300.0)] },
                Op::Link { rel: Relation { id: rel, from: clip, to: rect, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    store.take_changes();
    Scene { store, rect, bar, clip, rel, keys }
}

#[test]
fn playhead_samples_clips_and_bindings_follow() {
    let Scene { mut store, rect, bar, .. } = scene("linear");
    assert_eq!(store.time(), 0.0);
    assert_eq!(x_of(&store, rect), 50.0, "before the clip starts the authored value shows");
    store.set_time(1.0);
    assert_eq!(x_of(&store, rect), 100.0);
    store.set_time(2.0);
    assert_eq!(x_of(&store, rect), 200.0);
    let changed: Vec<Leaf> = store.take_changes().values.into_iter().collect();
    assert!(changed.contains(&Leaf { node: rect, path: path("transform2d.x") }));
    assert!(changed.contains(&Leaf { node: bar, path: path("size.h") }), "the binding followed: {changed:?}");
    assert_eq!(store.resolved(bar, path("size.h")).unwrap(), Value::Number(400.0));
    store.set_time(10.0);
    assert_eq!(x_of(&store, rect), 300.0, "after the end the clip holds `to`");
    store.set_time(0.5);
    assert_eq!(x_of(&store, rect), 50.0);
    // The document never changed.
    assert_eq!(store.tree().leaf(rect, path("transform2d.x")).unwrap().constant(), Some(&Value::Number(50.0)));
    assert!(store.time_errors().is_empty(), "{:?}", store.time_errors());
}

/// With the gate deciding (not everything observed), scrubbing back before
/// a clip's first key puts the authored value back: the running clip
/// withdraws what it gave (council review of DS-2, Sol).
#[test]
fn scrubbing_before_the_first_key_withdraws_the_clip() {
    let Scene { mut store, rect, .. } = scene("linear");
    store.set_observe_all(false);
    store.advance(0.0);
    store.set_time(2.0);
    store.advance(0.1);
    assert_eq!(x_of(&store, rect), 200.0);
    store.set_time(0.5);
    store.advance(0.2);
    assert_eq!(x_of(&store, rect), 50.0, "before the clip starts the authored value shows");
}

/// A clip's `prop` set to another leaf: the old one goes back to its
/// authored value (council review of DS-2, Opus).
#[test]
fn a_clip_retargeted_lets_go_of_its_old_leaf() {
    let Scene { mut store, rect, clip, .. } = scene("linear");
    store.set_observe_all(false);
    store.advance(0.0);
    store.set_time(2.0);
    store.advance(0.1);
    assert_eq!(x_of(&store, rect), 200.0);
    store.apply(tx("retarget", vec![Op::Set { id: clip, path: path("clip.prop"), slot: Slot::Const(Value::Str("transform2d.y".into())) }])).unwrap();
    store.advance(0.2);
    assert_eq!(x_of(&store, rect), 50.0);
    assert_eq!(store.resolved(rect, path("transform2d.y")).and_then(|v| v.as_f64()), Some(200.0));
}

#[test]
fn easing_and_offsets_apply() {
    let Scene { mut store, rect, clip, rel, .. } = scene("ease_in");
    store.set_time(2.0); // u = 0.5 -> ease_in 0.25 -> 150
    assert_eq!(x_of(&store, rect), 150.0);
    // A stagger offset on the edge shifts the start (doc 05 §3).
    let mut data = BTreeMap::new();
    data.insert("offset".to_owned(), Value::Number(1.0));
    store
        .apply(tx(
            "stagger",
            vec![
                Op::Unlink { id: rel },
                Op::Link { rel: Relation { id: rel, from: clip, to: rect, rel: RelKind::Animates, data } },
            ],
        ))
        .unwrap();
    assert_eq!(x_of(&store, rect), 100.0, "at t=2 the shifted clip has just started");
    store.set_time(3.0);
    assert_eq!(x_of(&store, rect), 150.0);
}

#[test]
fn transactions_keep_the_sample_current() {
    let Scene { mut store, rect, clip, .. } = scene("linear");
    store.set_time(2.0);
    assert_eq!(x_of(&store, rect), 200.0);
    store.apply(tx("retime", vec![Op::Set { id: clip, path: path("timing.start"), slot: num(0.0) }])).unwrap();
    assert_eq!(x_of(&store, rect), 300.0, "clip now ends at t=2");
    store.apply(tx("delete clip", vec![Op::Delete { id: clip }])).unwrap();
    assert_eq!(x_of(&store, rect), 50.0, "no clip, authored value");
    store.undo(1).unwrap().unwrap();
    assert_eq!(x_of(&store, rect), 300.0);
}

#[test]
fn gesture_preview_wins_over_the_clip() {
    let Scene { mut store, rect, .. } = scene("linear");
    store.set_time(2.0);
    store.preview(rect, path("transform2d.x"), Value::Number(7.0));
    assert_eq!(x_of(&store, rect), 7.0);
    store.cancel_gesture();
    assert_eq!(x_of(&store, rect), 200.0);
}

#[test]
fn bad_clips_are_reported_not_applied() {
    let Scene { mut store, rect, bar, clip, keys, .. } = scene("linear");
    // Retarget onto a bound leaf: reported.
    let rel2 = store.authored().next_relation_id();
    let mut c = Clip::default();
    c.prop = Slot::Const(Value::Str("size.h".into()));
    store
        .apply(tx(
            "bad target",
            vec![
                Op::Set { id: clip, path: path("clip.prop"), slot: Slot::Const(Value::Str("size.h".into())) },
                Op::Link { rel: Relation { id: rel2, from: clip, to: bar, rel: RelKind::Animates, data: BTreeMap::new() } },
            ],
        ))
        .unwrap();
    store.set_time(2.0);
    let err = store.time_errors().get(&clip).cloned().unwrap_or_default();
    assert!(err.contains("bound"), "{err}");
    // Kind mismatch: Vec3 endpoints on a number leaf.
    store
        .apply(tx(
            "kinds",
            vec![
                Op::Set { id: clip, path: path("clip.prop"), slot: Slot::Const(Value::Str("transform2d.x".into())) },
                Op::Set { id: keys[0], path: path("key.value"), slot: Slot::Const(Value::Vec3([0.0; 3])) },
                Op::Set { id: keys[1], path: path("key.value"), slot: Slot::Const(Value::Vec3([1.0; 3])) },
            ],
        ))
        .unwrap();
    let err = store.time_errors().get(&clip).cloned().unwrap_or_default();
    assert!(err.contains("vec3"), "{err}");
    assert_eq!(x_of(&store, rect), 50.0);
}

#[test]
fn views_answer_why_is_this_moving() {
    let Scene { store, rect, clip, .. } = scene("linear");
    let v = store.view(&ViewQuery { omit_defaults: false, expand: Vec::new(), relation: Default::default(), scope: Some(rect), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains(&format!("<driven-by ref=\"{clip}\" rel=\"animates\" prop=\"transform2d.x\"/>")), "{v}");
    let v = store.view(&ViewQuery { omit_defaults: false, expand: Vec::new(), relation: Default::default(), scope: Some(clip), fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains(&format!("<animates to=\"{rect}\"/>")), "{v}");
    assert!(v.contains("value=\"100\"") && v.contains("value=\"300\""), "the clip's keys: {v}");
    assert_eq!(store.tree().relations_to(rect).count(), 1);
}

/// A clip on a running page that gives nothing yet (before its first key)
/// leaves alone a leaf a paused clip holds: it withdraws only what no other
/// clip drives.
#[test]
fn a_running_clip_leaves_a_paused_clip_s_value_alone() {
    let mut store = Store::new();
    let [shown, hidden, visible, invisible, timeline, active, paused, k0, k1] = store.authored().next_ids(9)[..] else { unreachable!() };
    let rels = store.authored().next_relation_ids(3);
    let create = |id, name, parent, facets| Op::Create { id, ty: ty(name), parent, order: OrderKey::first(), facets };
    let rel = |i: usize, from, to| Op::Link { rel: Relation { id: rels[i], from, to, rel: RelKind::Animates, data: BTreeMap::new() } };
    let key = |t, v| Key { t: num(t), value: num(v), ..Key::default() }.wrap();
    let clip = Clip { prop: Slot::Const(Value::Str("transform2d.x".into())), ..Clip::default() }.wrap();
    let name = |x: &str| facet::Name { text: Slot::Const(Value::Str(x.into())), ..Default::default() }.wrap();
    let comp = facet::Composition { dims: Slot::Const(Value::Enum("2d".into())), ..Default::default() }.wrap();
    store.apply(tx("make", vec![
        create(shown, "composition", None, vec![comp.clone(), name("shown")]),
        create(hidden, "composition", None, vec![comp, name("hidden")]),
        create(visible, "rect", Some(shown), vec![Transform2d::default().wrap(), Size::default().wrap()]),
        create(invisible, "rect", Some(hidden), vec![Transform2d::default().wrap(), Size::default().wrap()]),
        create(timeline, "timeline", None, vec![]),
        create(active, "clip", Some(timeline), vec![clip.clone(), Timing::default().wrap()]),
        create(paused, "clip", Some(timeline), vec![clip, Timing::default().wrap()]),
        create(k0, "key", Some(active), vec![key(2., 99.)]),
        create(k1, "key", Some(paused), vec![key(0., 123.)]),
        rel(0, active, visible),
        rel(1, active, invisible),
        rel(2, paused, invisible),
    ])).unwrap();
    store.ensure_clock().unwrap();
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: shown, rect: [-1000., -1000., 2000., 2000.], zoom: 1. });
    store.advance(0.);
    assert_eq!(x_of(&store, invisible), 123.);
    store.set_time(1.);
    assert_eq!(x_of(&store, invisible), 123.);
}

/// A clip on a child a stack lays out: layout drives the child's x, the
/// clip's request waits and is reported, the same after an edit as after a
/// load (DS-10a, decided by the user: layout outranks clips). Before, a
/// loaded document let the clip win and an edited one let layout win.
#[test]
fn layout_outranks_a_clip_on_a_laid_out_child_edited_or_loaded() {
    let mut store = Store::new();
    let comp = store.authored().next_id();
    let c = facet::Composition { dims: Slot::Const(Value::Enum("2d".into())), ..Default::default() }.wrap();
    let name = facet::Name { text: Slot::Const(Value::Str("sheet".into())) }.wrap();
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![c, name] }])).unwrap();
    let built = markup::ops(r#"<group id="$g" x="0" y="0" layout.mode="stack_x" layout.gap="10"><rect id="$a" w="10" h="10"/><rect id="$b" w="10" h="10"/></group><timeline id="$t"><clip id="$c" clip.prop="transform2d.x"><key key.t="0" key.value="500"/><key key.t="2" key.value="600"/></clip></timeline>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let n = built.named;
    let rel = store.authored().next_relation_id();
    let mut ops = built.ops;
    ops.push(Op::Link { rel: Relation { id: rel, from: n["c"], to: n["b"], rel: RelKind::Animates, data: BTreeMap::new() } });
    store.apply(tx("stack and clip", ops)).unwrap_or_else(|e| panic!("{e:?}"));
    store.set_time(1.0);
    let check = |store: &Store, how: &str| {
        assert_eq!(x_of(store, n["b"]), 20.0, "{how}: layout places b");
        let x = Leaf { node: n["b"], path: path("transform2d.x") };
        assert!(store.signal_errors().get(&x).is_some_and(|e| e.contains("already driven")), "{how}: the clip's request is reported: {:?}", store.signal_errors());
    };
    check(&store, "edited");
    let mut loaded = Store::load(&store.save()).unwrap();
    loaded.set_time(1.0);
    check(&loaded, "loaded");
}

/// Two shown compositions and a hidden one, a rect in each, and two clips
/// on the timeline, one animating the first shown rect and one the hidden
/// rect, each from x=10 at 0 s to x=30 at 2 s.
fn three_pages() -> (Store, [NodeId; 6], Vec<RelationId>) {
    let mut store = Store::new();
    let ids = store.authored().next_ids(13);
    let [a, b, hidden, ra, rb, rh, timeline, c, k0, k1, d, k2, k3] = ids[..] else { unreachable!() };
    let rels = store.authored().next_relation_ids(3);
    let create = |id, name, parent, facets| Op::Create { id, ty: ty(name), parent, order: OrderKey::first(), facets };
    let rel = |i: usize, from, to| Op::Link { rel: Relation { id: rels[i], from, to, rel: RelKind::Animates, data: BTreeMap::new() } };
    let key = |t, v| Key { t: num(t), value: num(v), ..Key::default() }.wrap();
    let clip = Clip { prop: Slot::Const(Value::Str("transform2d.x".into())), ..Clip::default() }.wrap();
    let name = |x: &str| facet::Name { text: Slot::Const(Value::Str(x.into())), ..Default::default() }.wrap();
    let comp = facet::Composition { dims: Slot::Const(Value::Enum("2d".into())), ..Default::default() }.wrap();
    let rect = || vec![Transform2d::default().wrap(), Size::default().wrap()];
    store
        .apply(tx("make", vec![
            create(a, "composition", None, vec![comp.clone(), name("a")]),
            create(b, "composition", None, vec![comp.clone(), name("b")]),
            create(hidden, "composition", None, vec![comp, name("hidden")]),
            create(ra, "rect", Some(a), rect()),
            create(rb, "rect", Some(b), rect()),
            create(rh, "rect", Some(hidden), rect()),
            create(timeline, "timeline", None, vec![]),
            create(c, "clip", Some(timeline), vec![clip.clone(), Timing::default().wrap()]),
            create(k0, "key", Some(c), vec![key(0., 10.)]),
            create(k1, "key", Some(c), vec![key(2., 30.)]),
            create(d, "clip", Some(timeline), vec![clip, Timing::default().wrap()]),
            create(k2, "key", Some(d), vec![key(0., 10.)]),
            create(k3, "key", Some(d), vec![key(2., 30.)]),
            rel(0, c, ra),
            rel(1, d, rh),
        ]))
        .unwrap();
    store.ensure_clock().unwrap();
    (store, [a, b, hidden, ra, rb, rh], rels)
}

/// EN-8c.0 (council 2026-11-24): a running clip that gains a target in
/// another shown composition keeps that composition live too. Before, the
/// compositions it animates were recomputed only when the set of running
/// clips changed.
#[test]
fn a_running_clip_gaining_a_target_keeps_its_new_composition_live() {
    let (mut store, [a, b, _, _, rb, _], rels) = three_pages();
    let c = store.tree().relations().find(|r| r.id == rels[0]).unwrap().from;
    store.set_observe_all(false);
    for comp in [a, b] {
        store.observe(Interest::Region { composition: comp, rect: [-1000., -1000., 2000., 2000.], zoom: 1. });
    }
    let clock = store.ensure_clock().unwrap();
    store.apply_session(vec![Op::Set { id: clock, path: path("clock.playing"), slot: Slot::Const(Value::Bool(true)) }]).unwrap();
    store.advance(0.0);
    let live = store.advance(0.1).wake.live;
    assert!(live.contains(&a) && !live.contains(&b), "only a is animated: {live:?}");
    store.apply(tx("target b", vec![Op::Link { rel: Relation { id: rels[2], from: c, to: rb, rel: RelKind::Animates, data: BTreeMap::new() } }])).unwrap();
    store.advance(0.2);
    let live = store.advance(0.3).wake.live;
    assert!(live.contains(&b), "b is animated now: {live:?}");
}

/// EN-8c.0: a relation between two shown rects is no clip structure, so it
/// resamples no clip, and a paused hidden target keeps its value. Before,
/// any relation edit resampled every clip at the current time.
#[test]
fn an_unrelated_link_leaves_a_paused_hidden_clip_target_alone() {
    let (mut store, [a, _, _, ra, rb, rh], rels) = three_pages();
    store.advance(0.);
    assert_eq!(x_of(&store, rh), 10.);
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: a, rect: [-1000., -1000., 2000., 2000.], zoom: 1. });
    let clock = store.ensure_clock().unwrap();
    store.apply_session(vec![Op::Set { id: clock, path: path("clock.playing"), slot: Slot::Const(Value::Bool(true)) }]).unwrap();
    store.advance(0.);
    store.advance(1.);
    assert!(x_of(&store, ra) > 10., "the shown target follows playback");
    assert_eq!(x_of(&store, rh), 10., "the hidden one holds");
    let id = store.authored().next_relation_id();
    store.apply(tx("refer", vec![Op::Link { rel: Relation { id, from: ra, to: rb, rel: RelKind::References, data: BTreeMap::new() } }])).unwrap();
    assert_eq!(x_of(&store, rh), 10., "still held after an unrelated link");
    // Nor does a link that is not `Animates` at the running clip itself
    // (council review of EN-8c: any relation at a clip resampled every
    // clip, the paused one included).
    let clip = store.tree().relation(rels[0]).unwrap().from;
    let id = store.authored().next_relation_id();
    store.apply(tx("refer to the clip", vec![Op::Link { rel: Relation { id, from: rb, to: clip, rel: RelKind::References, data: BTreeMap::new() } }])).unwrap();
    assert_eq!(x_of(&store, rh), 10., "still held after a link at the running clip");
}

/// Council round 3 of EN-8c: a clip on a shown `on_demand` page follows the
/// playhead (free-running.md §2: playback needs no free-running source).
/// Before, clips followed the clocks, and an `on_demand` page's are held.
#[test]
fn a_clip_on_a_shown_on_demand_page_follows_a_scrub() {
    let (mut store, [a, _, _, ra, _, _], _) = three_pages();
    store.apply(tx("on demand", vec![Op::Set { id: a, path: path("composition.run"), slot: Slot::Const(Value::Enum("on_demand".into())) }])).unwrap();
    store.set_observe_all(false);
    store.observe(Interest::Region { composition: a, rect: [-1000., -1000., 2000., 2000.], zoom: 1. });
    store.advance(0.);
    store.set_time(1.);
    store.advance(0.1);
    assert_eq!(x_of(&store, ra), 20., "scrubbed to 1 s, between keys 10 and 30");
}
