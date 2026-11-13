//! Making a component's states by hand (machines.md §9 Q8): Add state on
//! the main, pin a state, edit a part, and the edit becomes that state's
//! pose through a new channel; instances then animate between them.

use iron_document::facet::{Appearance, Composition, Name, Port, Size, Template, Transform2d};
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

#[test]
fn a_state_made_by_hand_animates_its_instances() {
    let mut store = Store::new();
    let ids = store.document().next_ids(6);
    let [bench, model, library, def, template, dot] = ids[..] else { unreachable!() };
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("free".into()));
    store
        .apply(tx(
            "card",
            vec![
                Op::Create { id: bench, ty: ty("composition"), parent: None, order: k(0), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: k(1), facets: vec![] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(2), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(0), facets: vec![t.wrap()] },
                Op::Create { id: dot, ty: ty("rect"), parent: Some(template), order: k(0), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), Appearance::default().wrap(), named("dot")] },
            ],
        ))
        .unwrap();
    // Add state: a machine following a new `state` input, two states.
    let (ops, second) = machine::add_state_ops(store.document(), def).unwrap();
    store.apply(tx("add state", ops)).unwrap();
    assert_eq!(second, "state_2");
    let doc = store.document();
    let (m, port) = machine::machines(doc, def)[0].clone();
    assert_eq!(port, "state");
    let spec = doc.children(def).iter().copied().find(|&c| doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some("state") && doc.get::<Port>(c).is_some()).unwrap();
    assert_eq!(doc.get::<Port>(spec).unwrap().datatype.constant(), Some(&Value::Str("choice(default|state_2)".into())));
    // Pin state_2; dim the dot. One transaction makes a channel, binds the
    // dot to it and records 0.2 as state_2's pose.
    store.pin_state(Some(machine::Pinned::state(def, m, "state_2")));
    store.apply(tx("dim", vec![Op::Set { id: dot, path: path("appearance.opacity"), slot: Slot::Const(Value::Number(0.2)) }])).unwrap();
    store.advance_machines(0.5);
    let chans = machine::channels(store.document(), def);
    assert_eq!(chans.len(), 1);
    assert_eq!(chans[0].2, Value::Number(1.0), "the channel rests at the dot's value before the edit");
    assert!(store.document().leaf(dot, path("appearance.opacity")).unwrap().expr().is_some(), "the dot reads the channel");
    assert_eq!(store.resolved(dot, path("appearance.opacity")), Some(Value::Number(0.2)), "the pinned state shows on the definition");
    // Unpinned, the definition is at rest.
    store.pin_state(None);
    assert_eq!(store.resolved(dot, path("appearance.opacity")), Some(Value::Number(1.0)));
    // Rename the state; the port's choice follows.
    let s2 = store.document().children(m).iter().copied().find(|&s| store.document().get::<Name>(s).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some("state_2")).unwrap();
    let ops = machine::rename_state_ops(store.document(), s2, "dim").unwrap();
    store.apply(tx("rename", ops)).unwrap();
    assert_eq!(store.document().get::<Port>(spec).unwrap().datatype.constant(), Some(&Value::Str("choice(default|dim)".into())));
    // An instance animates between them: 300 ms, Easy Ease.
    let (ops, summary) = compose::compose_ops(&store, &format!(r#"<place def="{def}" in="bench" x="0" y="0"/>"#)).unwrap();
    store.apply(tx("place", ops)).unwrap();
    let line = summary.lines().find(|l| l.contains("<placed")).unwrap();
    let get = |a: &str| -> NodeId {
        let start = line.find(&format!("{a}=\"")).unwrap() + a.len() + 2;
        line[start..start + line[start..].find('"').unwrap()].parse().unwrap()
    };
    let (e, u) = (get("entity"), get("use"));
    let opacity = |s: &Store| s.resolved(NodeId::expanded(u, dot).unwrap(), path("appearance.opacity")).unwrap().as_f64().unwrap();
    store.advance_machines(0.0);
    assert_eq!(opacity(&store), 1.0);
    let err = store.apply(tx("typo", vec![Op::Set { id: e, path: path("args.state"), slot: Slot::Const(Value::Str("dimm".into())) }])).unwrap_err();
    assert!(err[0].message.contains("default, dim"), "{err:?}");
    store.apply(tx("dim it", vec![Op::Set { id: e, path: path("args.state"), slot: Slot::Const(Value::Str("dim".into())) }])).unwrap();
    store.advance_machines(1.0);
    store.advance_machines(1.15);
    let mid = opacity(&store);
    assert!(mid < 1.0 && mid > 0.2, "mid-way: {mid}");
    store.advance_machines(2.0);
    assert!((opacity(&store) - 0.2).abs() < 1e-9);
    // Undo the recorded edit, all of it in one step: no channel, the dot
    // a plain 1 again (after undoing the place, rename too).
    for _ in 0..4 {
        store.undo(0).unwrap().unwrap();
    }
    assert!(machine::channels(store.document(), def).is_empty());
    assert_eq!(store.document().leaf(dot, path("appearance.opacity")).unwrap().constant(), Some(&Value::Number(1.0)));
}

/// The machine panel's verbs (machines.md §6): a scrubbed transition shows
/// on the main, a pair gets its own transition, a channel its own slice;
/// and every instance with states has a lane on the timeline.
#[test]
fn the_machine_panel_scrubs_retimes_and_lanes_show_state() {
    let mut store = Store::new();
    let ids = store.document().next_ids(6);
    let [bench, model, library, def, template, dot] = ids[..] else { unreachable!() };
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("free".into()));
    store
        .apply(tx(
            "card",
            vec![
                Op::Create { id: bench, ty: ty("composition"), parent: None, order: k(0), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: k(1), facets: vec![] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(2), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(0), facets: vec![t.wrap()] },
                Op::Create { id: dot, ty: ty("rect"), parent: Some(template), order: k(0), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), Appearance::default().wrap(), named("dot")] },
            ],
        ))
        .unwrap();
    let (ops, _) = machine::add_state_ops(store.document(), def).unwrap();
    store.apply(tx("add state", ops)).unwrap();
    let m = machine::machines(store.document(), def)[0].0;
    store.pin_state(Some(machine::Pinned::state(def, m, "state_2")));
    store.apply(tx("dim", vec![Op::Set { id: dot, path: path("appearance.opacity"), slot: Slot::Const(Value::Number(0.2)) }])).unwrap();
    let opacity = |s: &Store| s.resolved(dot, path("appearance.opacity")).unwrap().as_f64().unwrap();
    // Scrubbing default → state_2 half-way shows a moment between on the main.
    let scrub = |s: &mut Store, p: f64| s.pin_state(Some(machine::Pinned { def, machine: m, state: "state_2".into(), scrub: Some(("default".into(), "state_2".into(), p)) }));
    scrub(&mut store, 0.5);
    let half = opacity(&store);
    assert!(half < 1.0 && half > 0.2, "half-way: {half}");
    scrub(&mut store, 1.0);
    assert!((opacity(&store) - 0.2).abs() < 1e-9);
    // One "any change" transition: a strip, and default → state_2 uses it.
    {
        let read = |l: Leaf| store.resolved(l.node, l.path);
        assert!(machine::is_strip(store.document(), &read, m));
    }
    // Own timing for default → state_2: its own transition, 300 ms like the shared one.
    let (ops, own) = {
        let read = |l: Leaf| store.resolved(l.node, l.path);
        machine::add_transition_ops(store.document(), &read, def, m, "default", "state_2").unwrap()
    };
    store.apply(tx("own", ops)).unwrap();
    {
        let read = |l: Leaf| store.resolved(l.node, l.path);
        let plan = machine::plan(store.document(), &read, def, m, "default", "state_2").unwrap();
        assert_eq!(plan.transition, own);
        assert_eq!(plan.ms, 300.0);
        assert!(machine::add_transition_ops(store.document(), &read, def, m, "default", "state_2").is_err(), "twice is refused");
    }
    // The channel's own slice, the second half: at a quarter it has not moved.
    let chan = machine::channels(store.document(), def)[0].1.clone();
    let (ops, tween) = machine::add_tween_ops(store.document(), own, &chan).unwrap();
    store.apply(tx("tween", ops)).unwrap();
    store.apply(tx("narrow", vec![Op::Set { id: tween, path: path("tween.start"), slot: Slot::Const(Value::Number(0.5)) }])).unwrap();
    scrub(&mut store, 0.25);
    assert_eq!(opacity(&store), 1.0, "before its slice");
    scrub(&mut store, 0.75);
    assert!(opacity(&store) < 1.0);
    store.pin_state(None);
    // An instance, never keyed, has a state lane on the timeline.
    let (ops, _) = compose::compose_ops(&store, &format!(r#"<place def="{def}" in="bench" x="0" y="0"/>"#)).unwrap();
    store.apply(tx("place", ops)).unwrap();
    let clock = store.ensure_clock().unwrap();
    let plan = timeline_view::plan(&store, clock, None);
    let lane = plan.objects.iter().flat_map(|o| &o.props).find(|p| p.prop == "args.state").expect("a lane");
    assert!(lane.lane);
    assert_eq!(lane.rest, Some(("default".into(), 0)));
    assert!(timeline_view::rows(&plan).iter().any(|r| r.lane));
}

/// The council's fixes: no pose is recorded while a transition is
/// scrubbed; a pin whose state is gone lets go; the strip follows the
/// chain; a transition an event started is reported with its states.
#[test]
fn scrubbing_records_nothing_and_events_report_their_states() {
    let mut store = Store::new();
    let ids = store.document().next_ids(6);
    let [bench, model, library, def, template, dot] = ids[..] else { unreachable!() };
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("free".into()));
    store
        .apply(tx(
            "card",
            vec![
                Op::Create { id: bench, ty: ty("composition"), parent: None, order: k(0), facets: vec![named("bench"), c.wrap()] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: k(1), facets: vec![] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(2), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(0), facets: vec![t.wrap()] },
                Op::Create { id: dot, ty: ty("rect"), parent: Some(template), order: k(0), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), Appearance::default().wrap(), named("dot")] },
            ],
        ))
        .unwrap();
    let (ops, _) = machine::add_state_ops(store.document(), def).unwrap();
    store.apply(tx("add state", ops)).unwrap();
    let m = machine::machines(store.document(), def)[0].0;
    store.pin_state(Some(machine::Pinned::state(def, m, "state_2")));
    store.apply(tx("dim", vec![Op::Set { id: dot, path: path("appearance.opacity"), slot: Slot::Const(Value::Number(0.2)) }])).unwrap();
    assert_eq!(machine::channels(store.document(), def).len(), 1);
    // Previewing a transition: an edit to a part belongs to neither state,
    // so it is refused, and nothing changes.
    store.pin_state(Some(machine::Pinned { def, machine: m, state: "default".into(), scrub: Some(("default".into(), "state_2".into(), 0.5)) }));
    let refused = store.apply(tx("move", vec![Op::Set { id: dot, path: path("transform2d.x"), slot: Slot::Const(Value::Number(40.0)) }])).unwrap_err();
    assert!(refused[0].message.contains("pick default or state_2"), "{refused:?}");
    assert_eq!(machine::channels(store.document(), def).len(), 1, "nothing recorded while previewing");
    assert_ne!(store.document().leaf(dot, path("transform2d.x")).and_then(|s| s.constant()), Some(&Value::Number(40.0)));
    // A third state, and explicit transitions a: state_3 → default → state_2: the strip follows them.
    let (ops, third) = machine::add_state_ops(store.document(), def).unwrap();
    store.apply(tx("third", ops)).unwrap();
    for (f, g) in [(third.as_str(), "default"), ("default", "state_2")] {
        let read = |l: Leaf| store.resolved(l.node, l.path);
        let (ops, _) = machine::add_transition_ops(store.document(), &read, def, m, f, g).unwrap();
        store.apply(tx("own", ops)).unwrap();
    }
    {
        let read = |l: Leaf| store.resolved(l.node, l.path);
        assert_eq!(machine::strip_order(store.document(), &read, m), (vec![third.clone(), "default".into(), "state_2".into()], false));
    }
    // A pin on a state that is gone lets go.
    store.pin_state(Some(machine::Pinned::state(def, m, "nope")));
    assert!(store.pinned_state().is_none());
    // An instance's state changed by an event reports its transition and states.
    let (ops, summary) = compose::compose_ops(&store, &format!(r#"<place def="{def}" in="bench" x="0" y="0"/>"#)).unwrap();
    store.apply(tx("place", ops)).unwrap();
    let line = summary.lines().find(|l| l.contains("<placed")).unwrap();
    let start = line.find("entity=\"").unwrap() + 8;
    let e: NodeId = line[start..start + line[start..].find('"').unwrap()].parse().unwrap();
    store.advance_machines(0.0);
    store.apply(tx("go", vec![Op::Set { id: e, path: path("args.state"), slot: Slot::Const(Value::Str("state_2".into())) }])).unwrap();
    store.advance_machines(1.0);
    store.advance_machines(1.1);
    let r = store.running_machines().iter().find(|r| r.entity == e).expect("running");
    assert_eq!((r.from.as_str(), r.to.as_str()), ("default", "state_2"));
    assert!(r.progress > 0.0 && r.progress < 1.0);
}

/// The state menu: making a state the default, and deleting one (its
/// transitions go, the port's choice loses it, a default moves on).
#[test]
fn a_state_is_made_default_and_deleted() {
    let mut store = Store::new();
    let ids = store.document().next_ids(4);
    let [library, def, template, dot] = ids[..] else { unreachable!() };
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("free".into()));
    store
        .apply(tx(
            "card",
            vec![
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(0), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(0), facets: vec![t.wrap()] },
                Op::Create { id: dot, ty: ty("rect"), parent: Some(template), order: k(0), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), Appearance::default().wrap(), named("dot")] },
            ],
        ))
        .unwrap();
    let (ops, _) = machine::add_state_ops(store.document(), def).unwrap();
    store.apply(tx("add", ops)).unwrap();
    let m = machine::machines(store.document(), def)[0].0;
    let state = |s: &Store, n: &str| s.document().children(m).iter().copied().find(|&c| s.document().get::<Name>(c).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(n)).unwrap();
    let spec = store.document().children(def).iter().copied().find(|&c| store.document().get::<Port>(c).is_some()).unwrap();
    let ops = machine::set_default_state_ops(store.document(), state(&store, "state_2")).unwrap();
    store.apply(tx("default", ops)).unwrap();
    assert_eq!(store.document().get::<Port>(spec).unwrap().signal.constant(), Some(&Value::Enum("state_2".into())));
    // Own timing for default → state_2, then delete state_2: the transition goes too.
    {
        let read = |l: Leaf| store.resolved(l.node, l.path);
        let (ops, _) = machine::add_transition_ops(store.document(), &read, def, m, "default", "state_2").unwrap();
        store.apply(tx("own", ops)).unwrap();
    }
    let ops = machine::delete_state_ops(store.document(), state(&store, "state_2")).unwrap();
    store.apply(tx("delete", ops)).unwrap();
    assert_eq!(machine::state_names(store.document(), m), vec!["default".to_owned()]);
    assert_eq!(store.document().get::<Port>(spec).unwrap().datatype.constant(), Some(&Value::Str("choice(default)".into())));
    assert_eq!(store.document().get::<Port>(spec).unwrap().signal.constant(), Some(&Value::Enum("default".into())));
    assert_eq!(store.document().children(m).iter().filter(|&&c| store.document().live(c).is_some_and(|n| n.ty.name() == "transition")).count(), 1, "the pair's own transition went with it");
    assert!(machine::delete_state_ops(store.document(), state(&store, "default")).is_err(), "the last state stays");
}

/// A part already bound to a channel: a drag inverts the binding and writes
/// the channel's rest value. Pinned, that write is the pinned state's pose,
/// so the states that do not set the channel (default) stay where they were.
#[test]
fn an_edit_to_a_part_bound_to_a_channel_records_into_the_pinned_state() {
    let mut store = Store::new();
    let ids = store.document().next_ids(4);
    let [library, def, template, dot] = ids[..] else { unreachable!() };
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("free".into()));
    store
        .apply(tx(
            "card",
            vec![
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(0), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(0), facets: vec![t.wrap()] },
                Op::Create { id: dot, ty: ty("rect"), parent: Some(template), order: k(0), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), Appearance::default().wrap(), named("dot")] },
            ],
        ))
        .unwrap();
    let (ops, _) = machine::add_state_ops(store.document(), def).unwrap();
    store.apply(tx("add", ops)).unwrap();
    let m = machine::machines(store.document(), def)[0].0;
    // state_2 moves the dot: a channel, the dot bound to it.
    store.pin_state(Some(machine::Pinned::state(def, m, "state_2")));
    store.apply(tx("move", vec![Op::Set { id: dot, path: path("transform2d.x"), slot: Slot::Const(Value::Number(-80.0)) }])).unwrap();
    let (ch, name, rest) = machine::channels(store.document(), def)[0].clone();
    assert_eq!(rest, Value::Number(0.0));
    // A third state, pinned; the drag writes the channel's value (inverted).
    let (ops, third) = machine::add_state_ops(store.document(), def).unwrap();
    store.apply(tx("add", ops)).unwrap();
    store.pin_state(Some(machine::Pinned::state(def, m, third.clone())));
    store.apply(tx("move", vec![Op::Set { id: ch, path: path("channel.value"), slot: Slot::Const(Value::Number(100.0)) }])).unwrap();
    store.advance_machines(0.5);
    let view = machine::channels(store.document(), def);
    assert_eq!(view[0].2, Value::Number(0.0), "the rest value, which default shows, is unchanged");
    assert_eq!(store.resolved(dot, path("transform2d.x")), Some(Value::Number(100.0)), "the pinned state shows");
    let s3 = store.document().children(m).iter().copied().find(|&c| store.document().get::<Name>(c).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(third.as_str())).unwrap();
    assert_eq!(store.document().leaf(s3, LeafPath::new(facet::FacetKind::Args, &name).unwrap()).and_then(|s| s.constant()).and_then(Value::as_f64), Some(100.0));
    store.pin_state(None);
    store.advance_machines(0.6);
    assert_eq!(store.resolved(dot, path("transform2d.x")), Some(Value::Number(0.0)), "at rest: default's place");
}

/// States hold colours and words as well as numbers (machines.md "Values
/// other than numbers"): a colour blends through a transition, words switch
/// half way through their slice. Editing a pair that uses the default
/// timing gives it its own in the same step, leaving the default alone; a
/// reverse leg can take that timing mirrored; a state can stop setting a
/// value.
#[test]
fn states_hold_colours_and_words_and_pairs_take_their_own_timing_on_edit() {
    use facet::{Fill, Font, Text};
    let mut store = Store::new();
    let ids = store.document().next_ids(5);
    let [library, def, template, dot, label] = ids[..] else { unreachable!() };
    let k = |i: u32| (0..i).fold(OrderKey::first(), |k, _| OrderKey::after(&k));
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str("free".into()));
    let mut words = Text::default();
    words.content = Slot::Const(Value::Str("calm".into()));
    store
        .apply(tx(
            "card",
            vec![
                Op::Create { id: library, ty: ty("library"), parent: None, order: k(0), facets: vec![] },
                Op::Create { id: def, ty: ty("definition"), parent: Some(library), order: k(0), facets: vec![named("card")] },
                Op::Create { id: template, ty: ty("template"), parent: Some(def), order: k(0), facets: vec![t.wrap()] },
                Op::Create { id: dot, ty: ty("rect"), parent: Some(template), order: k(0), facets: vec![Transform2d::default().wrap(), Size::default().wrap(), Fill::default().wrap(), Appearance::default().wrap(), named("dot")] },
                Op::Create { id: label, ty: ty("text"), parent: Some(template), order: k(1), facets: vec![Transform2d::default().wrap(), words.wrap(), Font::default().wrap(), named("label")] },
            ],
        ))
        .unwrap();
    // Named on the way in.
    let (ops, second) = machine::add_named_state_ops(store.document(), def, Some("calm"), Some("alert")).unwrap();
    store.apply(tx("add", ops)).unwrap();
    assert_eq!(second, "alert");
    let m = machine::machines(store.document(), def)[0].0;
    assert_eq!(machine::state_names(store.document(), m), vec!["calm".to_owned(), "alert".to_owned()]);
    assert!(machine::add_named_state_ops(store.document(), def, None, Some("alert")).is_err(), "names are unique");
    // alert: red, louder words, half opacity.
    let red = Value::Color([1.0, 0.0, 0.0, 1.0]);
    store.pin_state(Some(machine::Pinned::state(def, m, "alert")));
    store
        .apply(tx(
            "alert",
            vec![
                Op::Set { id: dot, path: path("fill.color"), slot: Slot::Const(red.clone()) },
                Op::Set { id: label, path: path("text.content"), slot: Slot::Const(Value::Str("ALERT".into())) },
                Op::Set { id: dot, path: path("appearance.opacity"), slot: Slot::Const(Value::Number(0.5)) },
            ],
        ))
        .unwrap();
    let chans = machine::channels(store.document(), def);
    assert_eq!(chans.len(), 3, "a channel each: colour, words, opacity");
    let by = |n: &str| chans.iter().find(|(_, c, _)| c.contains(n)).unwrap().clone();
    let (colour, words_ch) = (by("color"), by("content"));
    assert_eq!(colour.2, Value::Color([0.5, 0.5, 0.5, 1.0]), "the colour rests at the drawing's");
    assert_eq!(words_ch.2, Value::Str("calm".into()));
    // The parts that read each channel, for the editor's rows.
    let readers = machine::readers(store.document(), def);
    assert_eq!(readers[&colour.0], vec![(dot, path("fill.color"))]);
    assert_eq!(readers[&words_ch.0], vec![(label, path("text.content"))]);
    store.advance_machines(0.1);
    assert_eq!(store.resolved(dot, path("fill.color")), Some(red.clone()));
    assert_eq!(store.resolved(label, path("text.content")), Some(Value::Str("ALERT".into())));
    // Edited again through the Inspector while alert is pinned: the fields
    // are bound to channels the player publishes, yet they edit alert's
    // values (a colour and words through their binding, a number too).
    assert!(matches!(store.handle(dot, path("fill.color")), Handle::Alias { .. }), "{:?}", store.handle(dot, path("fill.color")));
    assert!(matches!(store.handle(dot, path("appearance.opacity")), Handle::Linear { .. }), "{:?}", store.handle(dot, path("appearance.opacity")));
    let blue = Value::Color([0.0, 0.0, 1.0, 1.0]);
    store.set_through_handle(dot, path("fill.color"), blue.clone(), "blue", Actor::Human, 0).unwrap();
    store.set_through_handle(label, path("text.content"), Value::Str("ALARM".into()), "words", Actor::Human, 0).unwrap();
    store.set_through_handle(dot, path("appearance.opacity"), Value::Number(0.4), "dim", Actor::Human, 0).unwrap();
    store.advance_machines(0.15);
    assert_eq!(store.resolved(dot, path("fill.color")), Some(blue.clone()));
    assert_eq!(store.resolved(label, path("text.content")), Some(Value::Str("ALARM".into())));
    assert_eq!(store.resolved(dot, path("appearance.opacity")), Some(Value::Number(0.4)));
    assert_eq!(machine::channels(store.document(), def).iter().find(|c| c.1.contains("color")).unwrap().2, Value::Color([0.5, 0.5, 0.5, 1.0]), "the drawing's colour is unchanged");
    store.set_through_handle(label, path("text.content"), Value::Str("ALERT".into()), "words", Actor::Human, 0).unwrap();
    store.set_through_handle(dot, path("fill.color"), red.clone(), "red", Actor::Human, 0).unwrap();
    store.set_through_handle(dot, path("appearance.opacity"), Value::Number(0.5), "dim", Actor::Human, 0).unwrap();
    // calm → alert used the default (300 ms); timing the words late gives
    // the pair its own transition in one step, and the default stays.
    let default_t = {
        let read = |l: Leaf| store.resolved(l.node, l.path);
        machine::plan(store.document(), &read, def, m, "calm", "alert").unwrap().transition
    };
    let edit = machine::PairEdit { ms: Some(1000.0), name: Some("raise".into()), slices: vec![machine::Slice { channel: words_ch.1.clone(), start: Some(0.8), end: Some(1.0), ease: Some("linear".into()) }], ..Default::default() };
    let (ops, own) = {
        let read = |l: Leaf| store.resolved(l.node, l.path);
        machine::edit_pair_ops(store.document(), &read, def, m, "calm", "alert", &edit).unwrap()
    };
    store.apply(tx("time words", ops)).unwrap();
    assert_ne!(own, default_t);
    assert_eq!(store.resolved(default_t, path("transition.ms")), Some(Value::Number(300.0)), "the default is untouched");
    assert_eq!(store.resolved(own, path("transition.ms")), Some(Value::Number(1000.0)));
    assert_eq!(store.document().get::<Name>(own).and_then(|n| n.text.constant()).and_then(|v| v.as_str()), Some("raise"));
    // A second edit changes the same transition.
    let (ops, again) = {
        let read = |l: Leaf| store.resolved(l.node, l.path);
        machine::edit_pair_ops(store.document(), &read, def, m, "calm", "alert", &machine::PairEdit { ease: Some("linear".into()), ..Default::default() }).unwrap()
    };
    store.apply(tx("linear", ops)).unwrap();
    assert_eq!(again, own);
    // Half way: the colour half way, the words not yet (their slice is the
    // last fifth); near the end the words have switched.
    store.pin_state(Some(machine::Pinned { def, machine: m, state: "calm".into(), scrub: Some(("calm".into(), "alert".into(), 0.5)) }));
    store.advance_machines(0.2);
    assert_eq!(store.resolved(dot, path("fill.color")), Some(Value::Color([0.75, 0.25, 0.25, 1.0])));
    assert_eq!(store.resolved(label, path("text.content")), Some(Value::Str("calm".into())));
    store.pin_state(Some(machine::Pinned { def, machine: m, state: "calm".into(), scrub: Some(("calm".into(), "alert".into(), 0.95)) }));
    store.advance_machines(0.3);
    assert_eq!(store.resolved(label, path("text.content")), Some(Value::Str("ALERT".into())));
    // The reverse leg takes the timing mirrored: the words first.
    let (ops, back) = {
        let read = |l: Leaf| store.resolved(l.node, l.path);
        let e = machine::copied_timing(store.document(), &read, own, true).unwrap();
        machine::edit_pair_ops(store.document(), &read, def, m, "alert", "calm", &e).unwrap()
    };
    store.apply(tx("mirror", ops)).unwrap();
    let w = store.document().children(back).iter().copied().find(|&c| store.document().live(c).is_some_and(|n| n.ty.name() == "tween")).unwrap();
    assert_eq!((store.resolved(w, path("tween.start")), store.resolved(w, path("tween.end"))), (Some(Value::Number(0.0)), Some(Value::Number(0.19999999999999996))));
    assert_eq!(machine::mirror_ease("ease_in"), "ease_out");
    assert_eq!(machine::mirror_ease("cubic-bezier(0.1, 0.2, 0.3, 1)"), "cubic-bezier(0.7, 0, 0.9, 0.8)");
    // alert stops setting the opacity: it shows the drawing's again.
    let alert = store.document().children(m).iter().copied().find(|&c| store.document().get::<Name>(c).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some("alert")).unwrap();
    let opacity = by("opacity").1;
    store.apply(tx("reset", machine::reset_value_ops(store.document(), alert, &opacity).unwrap())).unwrap();
    store.pin_state(Some(machine::Pinned::state(def, m, "alert")));
    store.advance_machines(0.4);
    assert_eq!(store.resolved(dot, path("appearance.opacity")), Some(Value::Number(1.0)));
    assert!(machine::reset_value_ops(store.document(), alert, &opacity).is_err());
}
