//! Copy, paste and duplicate (`iron_document::copy`): which references a
//! copy keeps, where a paste lands, and the clipboard's text form. Copies of
//! entities and connections are in `ports.rs`, of constraints in
//! `constraints.rs`.

use iron_document::facet::{Clip, Composition, Fill, Name, Size, Slider, Timing, Transform2d};
use iron_document::copy::{self, Fragment, Placement};
use iron_document::*;

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

fn get(store: &Store, id: NodeId, p: &str) -> f64 {
    store.resolved(id, path(p)).and_then(|v| v.as_f64()).unwrap()
}

fn rect(x: f64, y: f64, w: Slot) -> Vec<Facet> {
    let mut t = Transform2d::default();
    t.x = num(x);
    t.y = num(y);
    let mut s = Size::default();
    s.w = w;
    s.h = num(10.0);
    vec![t.wrap(), s.wrap(), Fill::default().wrap()]
}

struct Scene {
    store: Store,
    comp: NodeId,
    slider: NodeId,
    /// `w` reads the slider.
    a: NodeId,
    /// `w` reads `a`.
    b: NodeId,
    /// A group at (100, 0), rotated a quarter turn, holding `inner`.
    group: NodeId,
    inner: NodeId,
    clip: NodeId,
}

fn scene() -> Scene {
    let mut store = Store::new();
    let ids = store.document().next_ids(8);
    let [comp, slider, a, b, group, inner, timeline, clip] = ids.try_into().unwrap();
    let mut key = OrderKey::first();
    let mut order = || {
        let k = key.clone();
        key = OrderKey::after(&key);
        k
    };
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("scene".into()));
    let mut s = Slider::default();
    s.value = num(0.5);
    let mut g = Transform2d::default();
    g.x = num(100.0);
    g.rot = num(std::f64::consts::FRAC_PI_2);
    let mut timing = Timing::default();
    timing.dur = num(2.0);
    let mut cl = Clip::default();
    cl.prop = Slot::Const(Value::Str("transform2d.x".into()));
    cl.from = num(0.0);
    cl.to = num(50.0);
    let rel = store.document().next_relation_id();
    store
        .apply(tx(
            "scene",
            vec![
                Op::Create { id: comp, ty: ty("composition"), parent: None, order: order(), facets: vec![name.wrap(), c.wrap()] },
                Op::Create { id: slider, ty: ty("slider"), parent: Some(comp), order: order(), facets: vec![Transform2d::default().wrap(), s.wrap()] },
                Op::Create { id: a, ty: ty("rect"), parent: Some(comp), order: order(), facets: rect(10.0, 20.0, Slot::Bound(Expr::parse(&format!("{slider}.slider.value * 100")).unwrap())) },
                Op::Create { id: b, ty: ty("rect"), parent: Some(comp), order: order(), facets: rect(30.0, 40.0, Slot::Bound(Expr::parse(&format!("{a}.size.w + 1")).unwrap())) },
                Op::Create { id: group, ty: ty("group"), parent: Some(comp), order: order(), facets: vec![g.wrap()] },
                Op::Create { id: inner, ty: ty("rect"), parent: Some(group), order: order(), facets: rect(5.0, 0.0, num(10.0)) },
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: order(), facets: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: order(), facets: vec![timing.wrap(), cl.wrap()] },
                Op::Link { rel: Relation { id: rel, from: clip, to: b, rel: RelKind::Animates, data: Default::default() } },
            ],
        ))
        .unwrap();
    Scene { store, comp, slider, a, b, group, inner, clip }
}

fn expr_of(store: &Store, id: NodeId, p: &str) -> String {
    store.document().leaf(id, path(p)).unwrap().expr().unwrap().to_string()
}

#[test]
fn a_copy_reads_what_its_original_read_and_follows_its_own_copies() {
    let mut s = scene();
    let pasted = copy::duplicate(&s.store, &[s.a, s.b]).unwrap();
    s.store.apply(tx("duplicate", pasted.ops)).unwrap();
    let (a2, b2) = (pasted.roots[0], pasted.roots[1]);
    // `a`'s copy still reads the slider; `b`'s copy reads `a`'s copy.
    assert_eq!(expr_of(&s.store, a2, "size.w"), format!("{}.slider.value * 100", s.slider));
    assert_eq!(expr_of(&s.store, b2, "size.w"), format!("{a2}.size.w + 1"));
    // Each copy sits right above its original, in place.
    let kids = s.store.document().children(s.comp).to_vec();
    let at = |n: NodeId| kids.iter().position(|&k| k == n).unwrap();
    assert_eq!(at(a2), at(s.a) + 1);
    assert_eq!(at(b2), at(s.b) + 1);
    assert_eq!(get(&s.store, a2, "transform2d.x"), 10.0);
    // The clip animating `b` came along and animates `b`'s copy.
    let clips: Vec<NodeId> = s.store.document().nodes_of_type(ty("clip")).collect();
    assert_eq!(clips.len(), 2);
    let new_clip = clips.into_iter().find(|&c| c != s.clip).unwrap();
    let targets: Vec<NodeId> = s.store.document().relations_from(new_clip).map(|r| r.to).collect();
    assert_eq!(targets, vec![b2]);
    // Moving the slider moves both.
    s.store.apply(tx("slide", vec![Op::Set { id: s.slider, path: path("slider.value"), slot: num(0.2) }])).unwrap();
    assert!((get(&s.store, a2, "size.w") - 20.0).abs() < 1e-9);
    assert!((get(&s.store, b2, "size.w") - 21.0).abs() < 1e-9);
}

#[test]
fn a_paste_elsewhere_freezes_reads_it_cannot_keep() {
    let mut s = scene();
    let frag = copy::fragment(&s.store, &[s.a], "tab 1").unwrap();
    // Another document (or session): the slider's id means nothing there.
    let pasted = copy::paste(&s.store, &frag, &Placement { parent: Some(s.comp), ..Default::default() }, "tab 2").unwrap();
    assert_eq!(pasted.notes.len(), 1, "{:?}", pasted.notes);
    s.store.apply(tx("paste", pasted.ops)).unwrap();
    let a2 = pasted.roots[0];
    assert_eq!(expr_of(&s.store, a2, "size.w"), "0.5 * 100");
    assert_eq!(get(&s.store, a2, "size.w"), 50.0);
}

#[test]
fn a_paste_into_another_container_keeps_its_place_on_screen() {
    let mut s = scene();
    let world = s.store.world_affine(s.a).apply([0.0, 0.0]);
    let frag = copy::fragment(&s.store, &[s.a], "here").unwrap();
    let place = Placement { parent: Some(s.group), offset: [7.0, 0.0], ..Default::default() };
    let pasted = copy::paste(&s.store, &frag, &place, "here").unwrap();
    s.store.apply(tx("paste", pasted.ops)).unwrap();
    let a2 = pasted.roots[0];
    assert_eq!(s.store.document().live(a2).unwrap().parent, Some(s.group));
    // On top of the group's children, offset by (7, 0) in document space.
    assert_eq!(s.store.document().children(s.group).last(), Some(&a2));
    let now = s.store.world_affine(a2).apply([0.0, 0.0]);
    assert!((now[0] - world[0] - 7.0).abs() < 1e-9 && (now[1] - world[1]).abs() < 1e-9, "{now:?} vs {world:?}");
    // Unrotated on screen: the group's quarter turn is undone locally.
    assert!((get(&s.store, a2, "transform2d.rot") + std::f64::consts::FRAC_PI_2).abs() < 1e-9);
}

#[test]
fn a_fragment_survives_the_clipboard() {
    let mut s = scene();
    let frag = copy::fragment(&s.store, &[s.group, s.b], "here").unwrap();
    let text = frag.to_text();
    assert!(Fragment::is_fragment_text(&text));
    let back = Fragment::from_text(&text).unwrap();
    assert_eq!(back.roots, frag.roots);
    assert_eq!(back.extras, frag.extras);
    assert_eq!(back.externals, frag.externals);
    assert_eq!(serial::save(&back.doc), serial::save(&frag.doc));
    let direct = copy::paste(&s.store, &frag, &Placement::default(), "here").unwrap();
    let via_text = copy::paste(&s.store, &back, &Placement::default(), "here").unwrap();
    assert_eq!(direct, via_text);
    s.store.apply(tx("paste", via_text.ops)).unwrap();
    // Roots come back in painter order (`b` sits below the group), and the
    // group came with its child.
    assert_eq!(frag.roots, vec![s.b, s.group]);
    assert_eq!(s.store.document().children(via_text.roots[1]).len(), 1);
    assert!(Fragment::from_text("{\"hello\": 1}").is_err());
}

#[test]
fn a_child_of_a_selected_group_is_copied_once_and_undo_removes_everything() {
    let mut s = scene();
    let before = s.store.save();
    let pasted = copy::duplicate(&s.store, &[s.inner, s.group]).unwrap();
    assert_eq!(pasted.roots.len(), 1);
    s.store.apply(tx("duplicate", pasted.ops)).unwrap();
    assert_eq!(s.store.document().nodes_of_type(ty("rect")).count(), 4);
    s.store.undo(0).unwrap().unwrap();
    let after: serde_json::Value = serde_json::from_str(&s.store.save()).unwrap();
    let before: serde_json::Value = serde_json::from_str(&before).unwrap();
    // Same live content; the copies remain only as tombstones.
    let live = |v: &serde_json::Value| v["nodes"].as_object().unwrap().iter().filter(|(_, n)| n.get("tombstoned").is_none()).count();
    assert_eq!(live(&after), live(&before));
}

#[test]
fn nothing_copyable_is_an_error() {
    let s = scene();
    assert!(copy::fragment(&s.store, &[s.comp], "here").is_err());
    assert!(copy::fragment(&s.store, &[], "here").is_err());
}
