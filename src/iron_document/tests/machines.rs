//! Component state machines driven by events (plans/specs/machines.md
//! phase 1): a state argument changes, the transition plays over its length,
//! tweens give channels their own slices, an interruption starts from where
//! things are, undo snaps, and nothing reaches the file.

use iron_document::facet::{Appearance, Args, Channel, Composition, Name, Port, Template, Transition, Tween};
use iron_document::*;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).unwrap()
}

fn named(t: &str) -> facet::Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(t.into()));
    n.wrap()
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn str_(s: &str) -> Slot {
    Slot::Const(Value::Str(s.into()))
}

fn n(x: f64) -> Slot {
    Slot::Const(Value::Number(x))
}

struct Card {
    store: Store,
    entity: NodeId,
    fade: NodeId,
    reveal: NodeId,
    panel: NodeId,
    u: NodeId,
}

/// A card with channels fade and reveal (both resting at 1, "shown"), a
/// `vis` input, and a machine: hidden (both 0) and shown; appear (hidden →
/// shown, 1000 ms linear, reveal tweened over its second half) and
/// disappear (shown → hidden, 400 ms linear). Its panel's opacity is fade.
fn card() -> Card {
    let mut store = Store::new();
    let ids = store.document().next_ids(14);
    let [bench, model, library, def, vis, fade, reveal, machine, hidden, shown, appear, tween, disappear, template] = ids[..] else { unreachable!() };
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut vis_port = Port::default();
    vis_port.kind = str_("value");
    vis_port.dir = Slot::Const(Value::Enum("in".into()));
    vis_port.signal = Slot::Const(Value::Enum("shown".into()));
    let channel = |rest: f64| {
        let mut ch = Channel::default();
        ch.value = n(rest);
        ch.wrap()
    };
    let pose = |fade_v: f64, reveal_v: f64| {
        let mut a = Args::default();
        a.insert("fade", n(fade_v));
        a.insert("reveal", n(reveal_v));
        a.wrap()
    };
    let transition = |from: &str, to: &str, ms: f64| {
        let mut t = Transition::default();
        t.from = str_(from);
        t.to = str_(to);
        t.ms = n(ms);
        t.ease = str_("linear");
        t.wrap()
    };
    let mut tw = Tween::default();
    tw.channel = str_("reveal");
    tw.start = n(0.5);
    tw.end = n(1.0);
    tw.ease = str_("linear");
    let mut t = Template::default();
    t.notation = str_("free");
    store
        .apply(tx(
            "card",
            vec![
                Op::Create { id: bench, ty: ty("composition"), parent: None, order: k(0), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: k(1), facets: vec![] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(2), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: vis, ty: ty("port_spec"), parent: Some(def), order: k(0), facets: vec![named("vis"), vis_port.wrap()] },
                Op::Create { id: fade, ty: ty("channel"), parent: Some(def), order: k(1), facets: vec![named("fade"), channel(1.0)] },
                Op::Create { id: reveal, ty: ty("channel"), parent: Some(def), order: k(2), facets: vec![named("reveal"), channel(1.0)] },
                Op::Create { id: machine, ty: ty("machine"), parent: Some(def), order: k(3), facets: vec![named("vis")] },
                Op::Create { id: hidden, ty: ty("state"), parent: Some(machine), order: k(0), facets: vec![named("hidden"), pose(0.0, 0.0)] },
                Op::Create { id: shown, ty: ty("state"), parent: Some(machine), order: k(1), facets: vec![named("shown"), pose(1.0, 1.0)] },
                Op::Create { id: appear, ty: ty("transition"), parent: Some(machine), order: k(2), facets: vec![transition("hidden", "shown", 1000.0), named("appear")] },
                Op::Create { id: tween, ty: ty("tween"), parent: Some(appear), order: k(0), facets: vec![tw.wrap()] },
                Op::Create { id: disappear, ty: ty("transition"), parent: Some(machine), order: k(3), facets: vec![transition("shown", "hidden", 400.0), named("disappear")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(4), facets: vec![t.wrap()] },
            ],
        ))
        .unwrap();
    // The template: one panel whose opacity is the fade channel.
    let panel = store.document().next_id();
    let mut op = Appearance::default();
    op.opacity = Slot::Bound(Expr::parse(&format!("{fade}.channel.value")).unwrap());
    store
        .apply(tx("panel", vec![Op::Create { id: panel, ty: ty("rect"), parent: Some(template), order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), facet::Size::default().wrap(), op.wrap(), named("panel")] }]))
        .unwrap();
    let (ops, summary) = compose::compose_ops(&store, &format!(r#"<place def="{def}" in="bench" x="0" y="0"/>"#)).unwrap();
    store.apply(tx("place", ops)).unwrap();
    let get = |a: &str| -> NodeId {
        let line = summary.lines().find(|l| l.contains("<placed")).unwrap();
        let start = line.find(&format!("{a}=\"")).unwrap() + a.len() + 2;
        line[start..start + line[start..].find('"').unwrap()].parse().unwrap()
    };
    Card { store, entity: get("entity"), fade, reveal, panel, u: get("use") }
}

fn set_vis(store: &mut Store, entity: NodeId, state: &str) {
    store.apply(tx("vis", vec![Op::Set { id: entity, path: path("args.vis"), slot: Slot::Const(Value::Enum(state.into())) }])).unwrap();
}

fn channel(store: &Store, entity: NodeId, c: NodeId) -> f64 {
    store.resolved(NodeId::expanded(entity, c).unwrap(), path("channel.value")).unwrap().as_f64().unwrap()
}

#[test]
fn a_state_change_plays_its_transition_and_interruptions_start_where_things_are() {
    let Card { mut store, entity, fade, reveal, panel, u } = card();
    let opacity = |s: &Store| s.resolved(NodeId::expanded(u, panel).unwrap(), path("appearance.opacity")).unwrap().as_f64().unwrap();
    // First sight snaps; resting at the channels' rest values publishes nothing.
    assert!(store.advance_machines(0.0).is_empty());
    assert_eq!(opacity(&store), 1.0);
    assert!(store.signals().keys().all(|l| l.path != path("channel.value")));
    // Disappear: 400 ms, linear.
    set_vis(&mut store, entity, "hidden");
    assert_eq!(store.advance_machines(1.0).len(), 1);
    store.advance_machines(1.2);
    assert!((channel(&store, entity, fade) - 0.5).abs() < 1e-9);
    assert!((opacity(&store) - 0.5).abs() < 1e-9, "the template follows its channel");
    assert!(store.advance_machines(1.5).is_empty());
    assert_eq!(channel(&store, entity, fade), 0.0);
    // Appear: 1000 ms; reveal only in the second half (its tween).
    set_vis(&mut store, entity, "shown");
    store.advance_machines(2.0);
    store.advance_machines(2.25);
    assert!((channel(&store, entity, fade) - 0.25).abs() < 1e-9);
    assert_eq!(channel(&store, entity, reveal), 0.0);
    store.advance_machines(2.75);
    assert!((channel(&store, entity, fade) - 0.75).abs() < 1e-9);
    assert!((channel(&store, entity, reveal) - 0.5).abs() < 1e-9);
    // Interrupted: disappear starts from 0.75 and 0.5, not from shown's pose.
    set_vis(&mut store, entity, "hidden");
    store.advance_machines(2.75);
    store.advance_machines(2.95);
    assert!((channel(&store, entity, fade) - 0.375).abs() < 1e-9, "{}", channel(&store, entity, fade));
    assert!((channel(&store, entity, reveal) - 0.25).abs() < 1e-9);
    // Undo snaps to the state it restores (shown), with nothing published.
    store.undo(0).unwrap().unwrap();
    assert!(store.advance_machines(3.0).is_empty());
    assert_eq!((channel(&store, entity, fade), channel(&store, entity, reveal)), (1.0, 1.0));
    assert!(store.signals().keys().all(|l| l.path != path("channel.value")));
    // Nothing of it is saved: the file holds the channels' rest values.
    set_vis(&mut store, entity, "hidden");
    store.advance_machines(4.0);
    store.advance_machines(4.2);
    assert!((channel(&store, entity, fade) - 0.5).abs() < 1e-9, "mid-way now");
    let saved = Store::load(&store.save()).unwrap();
    assert_eq!(saved.document().get::<Channel>(fade).unwrap().value.constant(), Some(&Value::Number(1.0)), "channel values are presentation state");
}

#[test]
fn a_change_without_a_transition_snaps() {
    let Card { mut store, entity, fade, .. } = card();
    store.advance_machines(0.0);
    // Remove disappear: shown → hidden has no transition now.
    let doc = store.document();
    let disappear = doc.nodes_of_type(ty("transition")).find(|&t| doc.get::<Name>(t).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some("disappear")).unwrap();
    store.apply(tx("drop", vec![Op::Delete { id: disappear }])).unwrap();
    set_vis(&mut store, entity, "hidden");
    assert!(store.advance_machines(1.0).is_empty());
    assert_eq!(channel(&store, entity, fade), 0.0);
}

/// An undo snaps only the instances whose state it changes: undoing an
/// unrelated edit, then changing state, still plays the transition.
#[test]
fn an_unrelated_undo_does_not_snap_the_next_transition() {
    let Card { mut store, entity, fade, .. } = card();
    store.advance_machines(0.0);
    store.apply(tx("rename", vec![Op::Set { id: entity, path: path("name.text"), slot: str_("renamed") }])).unwrap();
    store.undo(0).unwrap().unwrap();
    set_vis(&mut store, entity, "hidden");
    store.advance_machines(1.0);
    store.advance_machines(1.2);
    assert!((channel(&store, entity, fade) - 0.5).abs() < 1e-9, "played, not snapped: {}", channel(&store, entity, fade));
}

fn named_child(store: &Store, parent: NodeId, ty_name: &str, name: &str) -> NodeId {
    let doc = store.document();
    doc.nodes_of_type(ty(ty_name)).find(|&n| !n.is_expanded() && doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name) && doc.is_ancestor(parent, n)).unwrap()
}

/// Timing customised from outside (machines.md §9 Q12): (b) the author
/// binds a transition's length to an input, so each instance sets its own;
/// (c) an instance overrides a transition without the author planning it.
#[test]
fn an_instance_times_its_transitions_by_an_input_or_an_override() {
    let Card { mut store, entity, fade, .. } = card();
    let doc = store.document();
    let def = doc.get::<facet::Instance>(entity).unwrap().def.constant().unwrap().as_ref().unwrap();
    let disappear = named_child(&store, def, "transition", "disappear");
    // (b) An input `out_ms` (default 400) the transition's length reads.
    let spec = store.document().next_id();
    let mut p = Port::default();
    p.kind = str_("value");
    p.dir = Slot::Const(Value::Enum("in".into()));
    p.signal = n(400.0);
    let after = store.document().children(def).iter().filter_map(|&c| store.document().live(c)).map(|x| x.order.clone()).max().unwrap();
    store
        .apply(tx(
            "input",
            vec![
                Op::Create { id: spec, ty: ty("port_spec"), parent: Some(def), order: OrderKey::after(&after), facets: vec![named("out_ms"), p.wrap()] },
                Op::Set { id: disappear, path: path("transition.ms"), slot: Slot::Bound(Expr::parse(&format!("{spec}.port.signal")).unwrap()) },
            ],
        ))
        .unwrap();
    store.apply(tx("slow", vec![Op::Set { id: entity, path: path("args.out_ms"), slot: n(1000.0) }])).unwrap();
    store.advance_machines(0.0);
    set_vis(&mut store, entity, "hidden");
    store.advance_machines(1.0);
    store.advance_machines(1.2);
    assert!((channel(&store, entity, fade) - 0.8).abs() < 1e-9, "1000 ms from the argument: {}", channel(&store, entity, fade));
    store.advance_machines(2.5);
    // (c) The instance overrides its copy of appear (1000 ms in the
    // definition): 100 ms, here only.
    let appear = named_child(&store, def, "transition", "appear");
    let copy = NodeId::expanded(entity, appear).unwrap();
    store.apply(tx("override", vec![Op::Set { id: copy, path: path("transition.ms"), slot: n(100.0) }])).unwrap();
    assert!(store.document().nodes_of_type(ty("override")).any(|o| store.document().is_live(o)), "an override on the instance");
    assert_eq!(store.resolved(appear, path("transition.ms")), Some(Value::Number(1000.0)), "the definition keeps its own");
    set_vis(&mut store, entity, "shown");
    store.advance_machines(6.0);
    store.advance_machines(6.05);
    assert!((channel(&store, entity, fade) - 0.5).abs() < 1e-9, "100 ms on this instance: {}", channel(&store, entity, fade));
}

/// State keyed on the timeline (machines.md §4.2): the clock drives the
/// transition, so any playhead time shows the same picture, forwards or
/// backwards; a key's span retimes that one occurrence; undo restores it.
#[test]
fn keyed_state_scrubs_and_one_occurrence_retimes() {
    let Card { mut store, entity, fade, .. } = card();
    let ids = store.document().next_ids(4);
    let (timeline, clip, k0, k1) = (ids[0], ids[1], ids[2], ids[3]);
    let key = |t: f64, v: &str| {
        let mut k = facet::Key::default();
        k.t = n(t);
        k.value = Slot::Const(Value::Enum(v.into()));
        k.easing = Slot::Const(Value::Enum("hold".into()));
        k.wrap()
    };
    let mut c = facet::Clip::default();
    c.prop = str_("args.vis");
    let first = OrderKey::first();
    let last = store.document().roots().iter().filter_map(|&r| store.document().live(r)).map(|x| x.order.clone()).max().unwrap();
    store
        .apply(tx(
            "key the state",
            vec![
                Op::Create { id: timeline, ty: ty("timeline"), parent: None, order: OrderKey::after(&last), facets: vec![] },
                Op::Create { id: clip, ty: ty("clip"), parent: Some(timeline), order: first.clone(), facets: vec![facet::Timing::default().wrap(), c.wrap()] },
                Op::Create { id: k0, ty: ty("key"), parent: Some(clip), order: first.clone(), facets: vec![key(0.0, "shown")] },
                Op::Create { id: k1, ty: ty("key"), parent: Some(clip), order: OrderKey::after(&first), facets: vec![key(2.0, "hidden")] },
                Op::Link { rel: Relation { id: store.document().next_relation_id(), from: clip, to: entity, rel: RelKind::Animates, data: Default::default() } },
            ],
        ))
        .unwrap();
    let at = |s: &mut Store, t: f64| {
        s.set_time(t);
        s.advance_machines(100.0 + t);
        channel(s, entity, fade)
    };
    // disappear is 400 ms, linear: half-way at 2.2 s.
    assert_eq!(at(&mut store, 1.0), 1.0);
    assert!((at(&mut store, 2.2) - 0.5).abs() < 1e-9);
    assert_eq!(at(&mut store, 3.0), 0.0);
    // Backwards: the same picture at the same time.
    assert!((at(&mut store, 2.1) - 0.75).abs() < 1e-9, "scrubbing back");
    assert_eq!(at(&mut store, 0.5), 1.0);
    // Retime this occurrence: 2 s instead of 0.4.
    store.apply(tx("retime", vec![Op::Set { id: k1, path: path("key.span"), slot: n(2.0) }])).unwrap();
    assert!((at(&mut store, 3.0) - 0.5).abs() < 1e-9, "half-way through the slow occurrence");
    store.undo(0).unwrap().unwrap();
    assert_eq!(at(&mut store, 3.0), 0.0, "undo restores the transition's own length");
}
