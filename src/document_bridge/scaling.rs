//! The scaling test (plans/handoff-scaling.md §5, DS-1): every operation a
//! user makes in one small shown composition costs the same however much of
//! the document is hidden.
//!
//! A fixture is grown along two axes, keeping the shown composition and the
//! operations the same: more hidden pages (`copies`: every composition of
//! the demo twinned, constraints, islands, 3D, embeds, clips, machines and
//! pictured entities included), and one larger hidden page (`big`). Each
//! operation runs at each size; the work it does at the store's choke
//! points (`iron_document::work`) and what it allocates
//! (`crate::alloc_count`) must not grow with the size.
//!
//! `SCALING` lists the operations that still grow, with the plan's phase
//! that removes each: the test fails if one grows that is not listed, or one
//! listed stops growing (so the list stays true as phases land).
//!
//!   cargo test -p ironfell --lib --features native --target aarch64-apple-darwin scaling -- --nocapture

use iron_document::{Actor, Interest, LeafPath, NodeId, Op, Slot, Store, TransactionInput, Value};
use std::collections::BTreeMap;

/// The operations that still grow with the hidden document, and the phase
/// expected to stop it (DS-1's starting point: §3's list).
const SCALING: &[(&str, &str)] = &[
    ("idle", "DS-2: the clock found by a scan of every node; machines, steps, presence, clocks each frame"),
    ("accumulator", "DS-2: as idle"),
    ("drag", "DS-2: as idle, each frame"),
    ("theme", "DS-2, DS-3a: as idle; palette and contexts published whole"),
    ("scrub", "DS-2: every clip sampled; DS-6b: clip-driven cells on hidden pages"),
    ("playing", "DS-2: every clip sampled, `Animates` walked; DS-6b"),
    ("structural preview", "DS-3b: repeats, layout and `regraph` scanned; DS-5: the gate decides again"),
    ("reparent", "DS-3b: `regraph`'s rederive and rescope (every token reader's provider, every root)"),
    ("let made", "DS-3b: as reparent"),
    ("observe nodes", "DS-4a: necessity swept on an observer change"),
    ("show and hide a page", "DS-4a: as observe nodes"),
    ("constant edit", "DS-4b: `exact` and the sweep; DS-7: the snapshot"),
    ("structural edit", "DS-4b, DS-7: as constant edit, with DS-3b's scans"),
    ("hidden-page edit", "DS-4b, DS-7: as constant edit"),
    ("edit plan", "DS-7: `scratch` clones the store; DS-4b"),
    ("fire", "DS-4b: `fire_at`'s `exact`; DS-7"),
    ("paste", "DS-4b, DS-7: as constant edit"),
    ("undo and redo", "DS-4b, DS-7: as constant edit"),
    ("rejected edit", "DS-7: the snapshot and its restore"),
];

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

/// What the shown composition holds, and the ids the operations use.
struct Shown {
    comp: NodeId,
    n: NodeId,
    r: NodeId,
    g: NodeId,
    button: NodeId,
    acc: NodeId,
}

/// The demo, grown: `copies` twins of each of its compositions, and a hidden
/// page of `big` bound rects; then the shown composition, observed alone.
fn fixture(copies: usize, big: usize) -> (Store, Shown, NodeId) {
    let mut store = Store::load(&super::demo::template_snapshot("demo").unwrap()).unwrap();
    let originals: Vec<NodeId> = iron_document::composition::compositions(store.tree()).into_iter().filter(|c| !c.is_session()).collect();
    for _ in 0..copies {
        for &c in &originals {
            twin(&mut store, c);
        }
    }
    if big > 0 {
        let rects: String = (0..big).map(|i| format!(r#"<rect name="b{i}" x="{{= $k.let.value + {i}}}" y="10" w="5" h="5"/>"#)).collect();
        let plan = iron_document::edit::plan(&store, &format!(r#"<edit><composition name="big" dims="2d"><let id="$k" name="k" value="1"/>{rects}</composition></edit>"#), None).unwrap_or_else(|f| panic!("{}", f.message()));
        store.apply(tx("big", plan.ops)).unwrap();
    }
    let plan = iron_document::edit::plan(
        &store,
        r#"<edit><composition id="$c" name="perf" dims="2d">
            <let id="$n" name="n" value="3"/>
            <accumulate id="$acc" name="acc" accumulate.next="{= $acc.accumulate.value + $acc.accumulate.dt}"/>
            <rect id="$r" name="r" x="100" y="100" w="150" h="100"/>
            <group id="$g" name="g" repeat.count="{= $n.let.value}"><rect name="cell" x="{= $g.repeat.index * 20}" y="300" w="10" h="10"/></group>
            <group id="$b" name="button" transform2d.x="400" transform2d.y="400"><rect name="face" size.w="40" size.h="20"/><on name="bump" write.target="$n" write.to="{= $n.let.value + 1}"/></group>
        </composition></edit>"#,
        None,
    )
    .unwrap_or_else(|f| panic!("{}", f.message()));
    store.apply(tx("perf", plan.ops)).unwrap();
    let shown = Shown { comp: plan.aliases["$c"], n: plan.aliases["$n"], r: plan.aliases["$r"], g: plan.aliases["$g"], button: plan.aliases["$b"], acc: plan.aliases["$acc"] };
    store.clear_history();
    store.set_observe_all(false);
    store.set_motion(true);
    store.observe(Interest::Region { composition: shown.comp, rect: [f64::MIN / 4.0, f64::MIN / 4.0, f64::MAX / 2.0, f64::MAX / 2.0], zoom: 1.0 });
    // A hidden page the operations touch: the demo's first composition,
    // the same at every size.
    let hidden = originals[0];
    (store, shown, hidden)
}

/// Another composition holding a copy of `c`'s contents.
fn twin(store: &mut Store, c: NodeId) {
    use iron_document::LeafStruct;
    let (children, dims) = {
        let doc = store.tree();
        (doc.children(c).iter().copied().filter(|&k| doc.is_live(k)).collect::<Vec<NodeId>>(), iron_document::composition::dims(doc, c))
    };
    if children.is_empty() {
        return;
    }
    let id = store.authored().next_id();
    let mut comp = iron_document::facet::Composition::default();
    comp.dims = Slot::Const(Value::Enum(if dims == Some(iron_document::composition::Dims::Three) { "3d" } else { "2d" }.into()));
    let mut name = iron_document::facet::Name::default();
    name.text = Slot::Const(Value::Str("twin".into()));
    let order = {
        let doc = store.tree();
        doc.roots().iter().filter(|r| !r.is_session()).filter_map(|&r| doc.live(r)).map(|r| r.order.clone()).max().map_or_else(iron_document::OrderKey::first, |k| iron_document::OrderKey::after(&k))
    };
    store.apply(tx("twin", vec![Op::Create { id, ty: iron_document::TypeId::lookup("composition").unwrap(), parent: None, order, facets: vec![name.wrap(), comp.wrap()] }])).unwrap();
    if let Ok(frag) = iron_document::copy::fragment(store, &children, "scaling")
        && let Ok(pasted) = iron_document::copy::paste(store, &frag, &iron_document::copy::Placement { parent: Some(id), ..Default::default() }, "scaling")
    {
        let _ = store.apply(tx("paste", pasted.ops));
    }
}

/// The work one operation did.
#[derive(Clone, Debug, Default, PartialEq)]
struct Work {
    sites: BTreeMap<&'static str, u64>,
    maps: BTreeMap<&'static str, u64>,
    allocs: u64,
    bytes: u64,
}

fn take() -> Work {
    let (sites, maps) = iron_document::work::take();
    let (allocs, bytes) = crate::alloc_count::take();
    Work { sites: sites.into_iter().filter(|(_, n)| *n > 0).collect(), maps: maps.into_iter().filter(|(_, n)| *n > 0).collect(), allocs, bytes }
}

type Operation = (&'static str, fn(&mut Store, &Shown, NodeId, &mut f64));

fn frame(store: &mut Store, now: &mut f64) {
    *now += 1.0 / 60.0;
    store.advance(*now);
}

/// Every operation, each leaving the store as it found it where it can, so
/// the next starts from the same state at every size.
fn operations() -> Vec<Operation> {
    vec![
        ("idle", |s, _, _, now| frame(s, now)),
        ("accumulator", |s, sh, _, now| {
            frame(s, now);
            let _ = s.resolved(sh.acc, path("accumulate.value"));
        }),
        ("drag", |s, sh, _, now| {
            s.begin_gesture(&[sh.r]);
            for k in 0..3 {
                s.preview(sh.r, path("transform2d.x"), Value::Number(100.0 + k as f64));
                frame(s, now);
            }
            s.cancel_gesture();
            frame(s, now);
        }),
        ("structural preview", |s, sh, _, now| {
            s.begin_gesture(&[sh.n]);
            for v in [4.0, 3.0] {
                s.preview(sh.n, path("let.value"), Value::Number(v));
                frame(s, now);
            }
            s.cancel_gesture();
            frame(s, now);
        }),
        ("constant edit", |s, sh, _, now| {
            for x in [200.0, 100.0] {
                s.apply(tx("x", vec![Op::Set { id: sh.r, path: path("transform2d.x"), slot: Slot::Const(Value::Number(x)) }])).unwrap();
                frame(s, now);
            }
        }),
        ("structural edit", |s, sh, _, now| {
            for v in [4.0, 3.0] {
                s.apply(tx("n", vec![Op::Set { id: sh.n, path: path("let.value"), slot: Slot::Const(Value::Number(v)) }])).unwrap();
                frame(s, now);
            }
        }),
        ("reparent", |s, sh, _, now| {
            let doc = s.tree();
            let back = doc.live(sh.r).unwrap().order.clone();
            let into = iron_document::OrderKey::first();
            s.apply(tx("in", vec![Op::Reparent { id: sh.r, parent: Some(sh.button), order: into }])).unwrap();
            frame(s, now);
            s.apply(tx("out", vec![Op::Reparent { id: sh.r, parent: Some(sh.comp), order: back }])).unwrap();
            frame(s, now);
        }),
        ("let made", |s, sh, _, now| {
            let plan = iron_document::edit::plan(s, &format!(r#"<edit><let in="{}" name="extra" value="1"/></edit>"#, sh.comp), None).unwrap_or_else(|f| panic!("{}", f.message()));
            let made: Vec<NodeId> = plan.ops.iter().filter_map(|o| match o { Op::Create { id, .. } => Some(*id), _ => None }).collect();
            s.apply(tx("let", plan.ops)).unwrap();
            frame(s, now);
            s.apply(tx("unlet", made.into_iter().map(|id| Op::Delete { id }).collect())).unwrap();
            frame(s, now);
        }),
        ("theme", |s, _, _, now| {
            s.set_theme(true);
            frame(s, now);
            s.set_theme(false);
            frame(s, now);
        }),
        ("fire", |s, sh, _, now| {
            let _ = s.fire(sh.button, "click", Actor::Human, 0);
            frame(s, now);
            s.apply(tx("n", vec![Op::Set { id: sh.n, path: path("let.value"), slot: Slot::Const(Value::Number(3.0)) }])).unwrap();
            frame(s, now);
        }),
        ("edit plan", |s, sh, _, now| {
            let plan = iron_document::edit::plan(s, &format!(r#"<edit><set id="{}" transform2d.y="120"/></edit>"#, sh.r), None).unwrap_or_else(|f| panic!("{}", f.message()));
            s.apply(tx("plan", plan.ops)).unwrap();
            frame(s, now);
        }),
        ("paste", |s, sh, _, now| {
            let frag = iron_document::copy::fragment(s, &[sh.r], "scaling").unwrap();
            let pasted = iron_document::copy::paste(s, &frag, &iron_document::copy::Placement { parent: Some(sh.comp), ..Default::default() }, "scaling").unwrap();
            let made = pasted.roots.clone();
            s.apply(tx("paste", pasted.ops)).unwrap();
            frame(s, now);
            s.apply(tx("unpaste", made.into_iter().map(|id| Op::Delete { id }).collect())).unwrap();
            frame(s, now);
        }),
        ("undo and redo", |s, sh, _, now| {
            s.apply(tx("x", vec![Op::Set { id: sh.r, path: path("transform2d.x"), slot: Slot::Const(Value::Number(150.0)) }])).unwrap();
            let _ = s.undo(0);
            frame(s, now);
            let _ = s.redo(0);
            frame(s, now);
            let _ = s.undo(0);
            frame(s, now);
        }),
        ("rejected edit", |s, sh, _, now| {
            let _ = s.apply(tx("bad", vec![Op::Set { id: sh.r, path: path("transform2d.x"), slot: Slot::Const(Value::Str("not a number".into())) }]));
            frame(s, now);
        }),
        ("hidden-page edit", |s, _, hidden, now| {
            let target = s.tree().descendants(hidden).into_iter().find(|&n| s.tree().leaf(n, path("transform2d.x")).and_then(|l| l.constant()).is_some());
            if let Some(t) = target {
                let x = s.tree().leaf(t, path("transform2d.x")).and_then(|l| l.constant()).cloned().unwrap();
                s.apply(tx("hidden", vec![Op::Set { id: t, path: path("transform2d.x"), slot: Slot::Const(Value::Number(x.as_f64().unwrap_or(0.0) + 1.0)) }])).unwrap();
                frame(s, now);
                s.apply(tx("hidden back", vec![Op::Set { id: t, path: path("transform2d.x"), slot: Slot::Const(x) }])).unwrap();
                frame(s, now);
            }
        }),
        ("show and hide a page", |s, _, hidden, now| {
            let id = s.observe(Interest::Region { composition: hidden, rect: [f64::MIN / 4.0, f64::MIN / 4.0, f64::MAX / 2.0, f64::MAX / 2.0], zoom: 1.0 });
            frame(s, now);
            s.unobserve(id);
            frame(s, now);
        }),
        ("observe nodes", |s, sh, _, now| {
            let id = s.observe(Interest::Nodes(vec![sh.r, sh.g]));
            frame(s, now);
            s.unobserve(id);
            frame(s, now);
        }),
        ("scrub", |s, _, _, now| {
            for t in [0.5, 1.0, 0.0] {
                s.set_time(t);
                frame(s, now);
            }
        }),
        ("playing", |s, _, _, now| {
            let _ = crate::document_bridge::clock::set(s, &[("loop_in", Value::Number(0.0)), ("loop_out", Value::Number(4.0)), ("playing", Value::Bool(true))]);
            for _ in 0..3 {
                frame(s, now);
            }
            let _ = crate::document_bridge::clock::set(s, &[("playing", Value::Bool(false)), ("t", Value::Number(0.0))]);
            frame(s, now);
        }),
    ]
}

/// Each operation's work at each size, after a warm-up run.
fn measure(copies: usize, big: usize) -> Vec<(&'static str, Work)> {
    let (mut store, shown, hidden) = fixture(copies, big);
    let mut now = 0.0;
    for _ in 0..30 {
        frame(&mut store, &mut now);
    }
    let mut out = Vec::new();
    for (name, op) in operations() {
        op(&mut store, &shown, hidden, &mut now);
        take();
        op(&mut store, &shown, hidden, &mut now);
        out.push((name, take()));
    }
    out
}

/// What grew from `a` to `b`, largest growth first: any site or map visited
/// more, or allocations beyond a small tolerance (B-tree splits vary with
/// size). Empty when nothing did.
fn grew(a: &Work, b: &Work) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    for (k, &n) in b.sites.iter().filter(|(k, _)| **k != "maps").chain(&b.maps) {
        let was = a.sites.get(k).or(a.maps.get(k)).copied().unwrap_or(0);
        if n > was {
            out.push((n - was, format!("{k} {was} → {n}")));
        }
    }
    let slack = |x: u64| x + x / 20 + 16;
    if b.allocs > slack(a.allocs) {
        out.push((0, format!("allocations {} → {}", a.allocs, b.allocs)));
    }
    if b.bytes > slack(a.bytes) {
        out.push((0, format!("bytes {} → {}", a.bytes, b.bytes)));
    }
    out.sort_by(|x, y| y.0.cmp(&x.0));
    out
}

#[test]
fn work_does_not_grow_with_the_hidden_document() {
    // Two axes from one base: more hidden pages, and one larger hidden page.
    let base = measure(0, 0);
    let axes = [("pages", measure(1, 0)), ("one big page", measure(0, 600))];
    let mut growing: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for (axis, run) in &axes {
        for (i, (name, work)) in run.iter().enumerate() {
            let steps = grew(&base[i].1, work);
            if !steps.is_empty() {
                let top: Vec<String> = steps.into_iter().take(5).map(|(_, s)| s).collect();
                growing.entry(name).or_default().push(format!("{axis}: {}", top.join(", ")));
            }
        }
    }
    println!("operations whose work grows with the hidden document (largest growth first):");
    for (name, why) in &growing {
        println!("  {name}: {}", why.join("; "));
    }
    let expected: BTreeMap<&str, &str> = SCALING.iter().copied().collect();
    let unexpected: Vec<&&str> = growing.keys().filter(|k| !expected.contains_key(*k)).collect();
    let stopped: Vec<&&str> = expected.keys().filter(|k| !growing.contains_key(*k)).collect();
    assert!(unexpected.is_empty() && stopped.is_empty(), "growing but not listed in SCALING: {unexpected:?}; listed but no longer growing (remove them): {stopped:?}");
}
