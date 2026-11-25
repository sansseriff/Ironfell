//! The scaling test (plans/handoff-scaling.md §5, DS-1): every operation a
//! user makes in one small shown composition costs the same however much of
//! the document is hidden.
//!
//! A fixture is grown along two axes, keeping the shown composition and the
//! operations the same: more hidden pages (`copies`: every composition of
//! the demo twinned, and a page of queries), and one larger hidden page
//! (`big` bound rects). Each operation runs at three sizes on each axis; the
//! work it does at the store's choke points (`iron_document::work`) and what
//! it allocates (`crate::alloc_count`) must not grow with the size. A count
//! grows when it rises at both steps (beyond an absolute tolerance for
//! allocations, which B-tree splits vary).
//!
//! `SCALING` lists each (operation, count) that still grows, with the
//! plan's phase that removes it: the test fails if a count grows that is
//! not listed, or one listed stops growing, so the list stays true as phases
//! land. `save` is a canary: a whole-document walk the harness must see.
//!
//!   cargo test -p ironfell --lib --features native --target aarch64-apple-darwin scaling -- --nocapture

use iron_document::{Actor, Interest, LeafPath, NodeId, Op, Slot, Store, TransactionInput, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Each (operation, count) that still grows with the hidden document, and
/// the phase expected to stop it.
const SCALING: &[(&str, &str, &str)] = &[
    ("constant edit", "allocations", "DS-7"),
    ("cross-owner read", "allocations", "DS-7"),
    ("cross-owner read", "store.clusters", "DS-3a"),
    ("constant edit", "store.clusters", "DS-3a"),
    ("edit plan", "graph.unneeded", "EN-9: scratch() wakes every resting cell of its clone"),
    ("edit plan", "allocations", "DS-7"),
    ("edit plan", "bytes", "DS-7"),
    ("edit plan", "graph.readers", "DS-4a"),
    ("edit plan", "store.clusters", "DS-3a"),
    ("fire", "allocations", "DS-7"),
    ("fire", "nodes_of_type", "DS-3b"),
    ("fire", "store.clusters", "DS-3a"),
    ("hidden-page edit", "allocations", "DS-7"),
    ("hidden-page edit", "store.clusters", "DS-3a"),
    ("let made", "graph.unneeded", "EN-9: scratch() wakes every resting cell of its clone"),
    ("let made", "allocations", "DS-7"),
    ("let made", "bytes", "DS-7"),
    ("let made", "store.clusters", "DS-3a"),
    ("paste", "allocations", "DS-7"),
    ("paste", "bytes", "DS-7"),
    ("paste", "nodes_of_type", "DS-3b"),
    ("paste", "roots", "DS-3b"),
    ("paste", "store.clusters", "DS-3a"),
    ("rejected edit", "allocations", "DS-7"),
    ("rejected edit", "store.clusters", "DS-3a"),
    ("reparent", "allocations", "DS-7"),
    ("reparent", "store.clusters", "DS-4a"),
    ("save (canary)", "allocations", "canary: must always grow"),
    ("save (canary)", "bytes", "canary: must always grow"),
    ("save (canary)", "relations", "canary: must always grow"),
    ("structural edit", "allocations", "DS-7"),
    ("structural edit", "store.clusters", "DS-4a"),
    ("theme", "allocations", "DS-3a"),
    ("theme", "bytes", "DS-3a"),
    ("undo and redo", "allocations", "DS-7"),
    ("undo and redo", "store.clusters", "DS-3a"),
    ("unrelated relation edit", "allocations", "DS-7"),
    ("unrelated relation edit", "store.clusters", "DS-3a"),
    ("walk (canary)", "allocations", "canary: must always grow"),
    ("walk (canary)", "bytes", "canary: must always grow"),
    ("walk (canary)", "descendants", "canary: must always grow"),
    ("walk (canary)", "roots", "canary: must always grow"),
];

/// Allocations and bytes may differ by this much between sizes without
/// growing (B-tree node splits vary with the size of a map).
const ALLOCS_SLACK: u64 = 24;
const BYTES_SLACK: u64 = 24 * 1024;

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn everything(composition: NodeId) -> Interest {
    Interest::Region { composition, rect: [f64::MIN / 4.0, f64::MIN / 4.0, f64::MAX / 2.0, f64::MAX / 2.0], zoom: 1.0 }
}

/// The store and the nodes the operations use.
struct Ctx {
    store: Store,
    now: f64,
    /// The shown composition and what is in it.
    perf: NodeId,
    n: NodeId,
    r: NodeId,
    g: NodeId,
    button: NodeId,
    /// A hidden page made after the growth, so the same at every size: what
    /// a hidden edit and showing a page touch.
    quiet: NodeId,
    quiet_r: NodeId,
    /// A hidden page with a running accumulator, observed only while the
    /// accumulator operation runs.
    running: NodeId,
    acc: NodeId,
    observer: Option<iron_document::ObserverId>,
    /// What the edit-plan operation sets next.
    toggle: bool,
}

impl Ctx {
    fn frame(&mut self) -> bool {
        self.now += 1.0 / 60.0;
        self.store.advance(self.now).changed
    }

    /// Step until a frame changes nothing.
    fn settle(&mut self) {
        let mut quiet = 0;
        for _ in 0..400 {
            let changed = self.frame();
            quiet = if !changed { quiet + 1 } else { 0 };
            if quiet == 2 {
                return;
            }
        }
        panic!("the store did not settle");
    }

    fn apply(&mut self, label: &str, ops: Vec<Op>) {
        self.store.apply(tx(label, ops)).unwrap_or_else(|e| panic!("{label}: {e:?}"));
    }
}

/// The demo (`snapshot`), grown: `copies` twins of each of its compositions
/// and of a page of queries, and a hidden page of `big` bound rects; then
/// the pages the operations use, and the shown composition, observed alone.
fn fixture(snapshot: &str, copies: usize, big: usize) -> Ctx {
    let mut store = Store::load(snapshot).unwrap();
    let edit = |store: &mut Store, markup: &str| -> BTreeMap<String, NodeId> {
        let plan = iron_document::edit::plan(store, &format!("<edit>{markup}</edit>"), None).unwrap_or_else(|f| panic!("{}", f.message()));
        store.apply(tx("fixture", plan.ops)).unwrap();
        plan.aliases
    };
    // Queries are the one kind the demo lacks.
    edit(&mut store, r#"<composition name="queries" dims="2d"><let id="$k" name="k" value="1"/><query name="q1" model="sim" input="{= record(k: $k)}" debounce="0.5" initial="{= 0}"/><query name="q2" model="sim" input="{= record(k: $k + 1)}"/><text name="answer" text.content="{= 'k ' + str($k)}"/></composition>"#);
    let originals: Vec<NodeId> = iron_document::composition::compositions(store.tree()).into_iter().filter(|c| !c.is_session()).collect();
    for _ in 0..copies {
        for &c in &originals {
            twin(&mut store, c);
        }
    }
    if big > 0 {
        let rects: String = (0..big).map(|i| format!(r#"<rect name="b{i}" x="{{= $kk.let.value + {i}}}" y="10" w="5" h="5"/>"#)).collect();
        edit(&mut store, &format!(r#"<composition name="big" dims="2d"><let id="$kk" name="k" value="1"/>{rects}</composition>"#));
    }
    let quiet = edit(&mut store, r#"<composition id="$q" name="quiet" dims="2d"><rect id="$qr" name="qr" x="10" y="10" w="20" h="20"/><text name="qt" text.content="quiet"/></composition>"#);
    let running = edit(&mut store, r#"<composition id="$run" name="running" dims="2d"><accumulate id="$acc" name="acc" accumulate.next="{= $acc.accumulate.value + $acc.accumulate.dt}"/><rect name="bar" x="0" y="0" w="{= $acc.accumulate.value}" h="4"/></composition>"#);
    let perf = edit(
        &mut store,
        r#"<composition id="$c" name="perf" dims="2d">
            <let id="$n" name="n" value="3"/>
            <rect id="$r" name="r" x="100" y="100" w="150" h="100"/>
            <group id="$g" name="g" repeat.count="{= $n.let.value}"><rect name="cell" x="{= $g.repeat.index * 20}" y="300" w="10" h="10"/></group>
            <group id="$b" name="button" transform2d.x="400" transform2d.y="400"><rect name="face" size.w="40" size.h="20"/><on name="bump" write.target="$n" write.to="{= $n.let.value + 1}"/></group>
        </composition>"#,
    );
    store.clear_history();
    store.set_observe_all(false);
    store.set_motion(true);
    store.observe(everything(perf["$c"]));
    let mut ctx = Ctx {
        store,
        now: 0.0,
        perf: perf["$c"],
        n: perf["$n"],
        r: perf["$r"],
        g: perf["$g"],
        button: perf["$b"],
        quiet: quiet["$q"],
        quiet_r: quiet["$qr"],
        running: running["$run"],
        acc: running["$acc"],
        observer: None,
        toggle: false,
    };
    ctx.settle();
    ctx
}

/// Another composition holding a copy of `c`'s contents. A paste the store
/// refuses (a constraint cluster that cannot be copied whole, say) is
/// retried without the ops it named, so the rest of the page is still there;
/// `kinds` checks that every kind grew.
pub(super) fn twin(store: &mut Store, c: NodeId) {
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
        doc.last_root().and_then(|r| doc.live(r)).map_or_else(iron_document::OrderKey::first, |n| iron_document::OrderKey::after(&n.order))
    };
    store.apply(tx("twin", vec![Op::Create { id, ty: iron_document::TypeId::lookup("composition").unwrap(), parent: None, order, facets: vec![name.wrap(), comp.wrap()] }])).unwrap();
    let Ok(frag) = iron_document::copy::fragment(store, &children, "scaling") else { return };
    let place = iron_document::copy::Placement { parent: Some(id), ..Default::default() };
    let pasted = iron_document::copy::paste(store, &frag, &place, "scaling").unwrap_or_else(|e| panic!("twin of {c}: {e}"));
    if let Err(e) = store.apply(tx("paste", pasted.ops.clone())) {
        let bad: BTreeSet<usize> = e.iter().map(|x| x.op).collect();
        let ops: Vec<Op> = pasted.ops.into_iter().enumerate().filter(|(i, _)| !bad.contains(i)).map(|(_, o)| o).collect();
        let _ = store.apply(tx("paste", ops));
    }
}

/// How many of each kind the fixture holds, to check that growing it grows
/// every kind the plan names.
fn kinds(store: &Store) -> BTreeMap<&'static str, usize> {
    let doc = store.tree();
    let of = |ty: &str| iron_document::TypeId::lookup(ty).map_or(0, |t| doc.nodes_of_type(t).filter(|&n| doc.is_live(n)).count());
    use iron_document::facet::FacetKind as K;
    let with = |k: K| doc.nodes_with(k).filter(|&n| doc.is_live(n)).count();
    BTreeMap::from([
        ("constraints", with(K::Constraint)),
        ("clips", of("clip")),
        ("machines", of("machine") + with(K::Machine)),
        ("placed plots", doc.nodes_with(K::Args).filter(|&n| doc.is_live(n)).count()),
        ("queries", with(K::Query)),
        ("embeds", of("embed")),
        ("ports", with(K::Port)),
        ("accumulators", with(K::Accumulate)),
        ("repeats", with(K::Repeat)),
        ("3d compositions", iron_document::composition::compositions(doc).into_iter().filter(|&c| iron_document::composition::dims(doc, c) == Some(iron_document::composition::Dims::Three)).count()),
        ("pictured entities", doc.relations().filter(|r| r.rel == iron_document::document::RelKind::Figures).count()),
    ])
}

/// The work one operation did, by count.
#[derive(Clone, Debug, Default, PartialEq)]
struct Work(BTreeMap<&'static str, u64>);

fn take() -> Work {
    // Allocations first: building the report allocates.
    let (allocs, bytes) = crate::alloc_count::take();
    let (sites, maps) = iron_document::work::take();
    let mut w: BTreeMap<&'static str, u64> = sites.into_iter().filter(|(k, n)| *n > 0 && *k != "maps").collect();
    w.extend(maps.into_iter().filter(|(_, n)| *n > 0));
    w.insert("allocations", allocs);
    w.insert("bytes", bytes);
    Work(w)
}

/// An operation: what it does before the measurement and after, and what is
/// measured. Each leaves the store as it found it.
struct Operation {
    name: &'static str,
    setup: fn(&mut Ctx),
    run: fn(&mut Ctx),
    teardown: fn(&mut Ctx),
}

fn nothing(_: &mut Ctx) {}

fn op(name: &'static str, run: fn(&mut Ctx)) -> Operation {
    Operation { name, setup: nothing, run, teardown: nothing }
}

fn operations() -> Vec<Operation> {
    vec![
        op("idle", |c| {
            c.frame();
        }),
        Operation {
            name: "running accumulator",
            setup: |c| c.observer = Some(c.store.observe(everything(c.running))),
            run: |c| {
                c.frame();
                assert!(c.store.resolved(c.acc, path("accumulate.value")).is_some());
            },
            teardown: |c| {
                if let Some(o) = c.observer.take() {
                    c.store.unobserve(o);
                }
            },
        },
        op("drag", |c| {
            c.store.begin_gesture(&[c.r]);
            for k in 0..3 {
                c.store.preview(c.r, path("transform2d.x"), Value::Number(101.0 + k as f64));
                c.frame();
            }
            c.store.cancel_gesture();
            c.frame();
        }),
        op("structural preview", |c| {
            c.store.begin_gesture(&[c.n]);
            for v in [4.0, 3.0] {
                c.store.preview(c.n, path("let.value"), Value::Number(v));
                c.frame();
            }
            c.store.cancel_gesture();
            c.frame();
        }),
        op("constant edit", |c| {
            for x in [200.0, 100.0] {
                c.apply("x", vec![Op::Set { id: c.r, path: path("transform2d.x"), slot: Slot::Const(Value::Number(x)) }]);
                c.frame();
            }
        }),
        op("structural edit", |c| {
            let before = c.store.tree().children(c.g).len();
            for (v, d) in [(4.0, 1), (3.0, 0)] {
                c.apply("n", vec![Op::Set { id: c.n, path: path("let.value"), slot: Slot::Const(Value::Number(v)) }]);
                c.frame();
                assert_eq!(c.store.tree().children(c.g).len(), before + d, "the repeat has {v} items");
            }
        }),
        op("reparent", |c| {
            let back = c.store.tree().live(c.r).unwrap().order.clone();
            c.apply("in", vec![Op::Reparent { id: c.r, parent: Some(c.button), order: iron_document::OrderKey::first() }]);
            c.frame();
            c.apply("out", vec![Op::Reparent { id: c.r, parent: Some(c.perf), order: back }]);
            c.frame();
        }),
        op("let made", |c| {
            let plan = iron_document::edit::plan(&c.store, &format!(r#"<edit><let in="{}" name="extra" value="1"/></edit>"#, c.perf), None).unwrap_or_else(|f| panic!("{}", f.message()));
            let made: Vec<NodeId> = plan.ops.iter().filter_map(|o| match o { Op::Create { id, .. } => Some(*id), _ => None }).collect();
            c.apply("let", plan.ops);
            c.frame();
            c.apply("unlet", made.into_iter().map(|id| Op::Delete { id }).collect());
            c.frame();
        }),
        op("theme", |c| {
            c.store.set_theme(true);
            c.frame();
            c.store.set_theme(false);
            c.frame();
        }),
        op("fire", |c| {
            let fired = c.store.fire(c.button, "click", Actor::Human, 0).unwrap();
            assert!(fired.is_some(), "the button's action wrote");
            c.frame();
            c.apply("n", vec![Op::Set { id: c.n, path: path("let.value"), slot: Slot::Const(Value::Number(3.0)) }]);
            c.frame();
        }),
        // A shown binding that reads a hidden page brings that page into
        // demand and lets it rest again (council review of EN-8c).
        op("cross-owner read", |c| {
            let read = iron_document::Expr::parse(&format!("{}.transform2d.x + 1", c.quiet_r)).unwrap();
            c.apply("read", vec![Op::Set { id: c.r, path: path("transform2d.x"), slot: Slot::Bound(read) }]);
            c.frame();
            assert_eq!(c.store.resolved(c.r, path("transform2d.x")), Some(Value::Number(11.0)));
            c.apply("unread", vec![Op::Set { id: c.r, path: path("transform2d.x"), slot: Slot::Const(Value::Number(100.0)) }]);
            c.frame();
        }),
        // A link that is not `Animates` resamples no clip.
        op("unrelated relation edit", |c| {
            let id = c.store.authored().next_relation_id();
            let rel = iron_document::document::Relation { id, from: c.r, to: c.button, rel: iron_document::document::RelKind::References, data: BTreeMap::new() };
            c.apply("link", vec![Op::Link { rel }]);
            c.frame();
            c.apply("unlink", vec![Op::Unlink { id }]);
            c.frame();
        }),
        op("edit plan", |c| {
            c.toggle = !c.toggle;
            let y = if c.toggle { 120 } else { 100 };
            let plan = iron_document::edit::plan(&c.store, &format!(r#"<edit><set id="{}" transform2d.y="{y}"/></edit>"#, c.r), None).unwrap_or_else(|f| panic!("{}", f.message()));
            c.apply("plan", plan.ops);
            c.frame();
        }),
        op("paste", |c| {
            let frag = iron_document::copy::fragment(&c.store, &[c.r], "scaling").unwrap();
            let pasted = iron_document::copy::paste(&c.store, &frag, &iron_document::copy::Placement { parent: Some(c.perf), ..Default::default() }, "scaling").unwrap();
            let made = pasted.roots.clone();
            c.apply("paste", pasted.ops);
            c.frame();
            c.apply("unpaste", made.into_iter().map(|id| Op::Delete { id }).collect());
            c.frame();
        }),
        op("undo and redo", |c| {
            c.apply("x", vec![Op::Set { id: c.r, path: path("transform2d.x"), slot: Slot::Const(Value::Number(150.0)) }]);
            assert!(c.store.undo(0).is_some_and(|r| r.is_ok()));
            c.frame();
            assert!(c.store.redo(0).is_some_and(|r| r.is_ok()));
            c.frame();
            assert!(c.store.undo(0).is_some_and(|r| r.is_ok()));
            c.frame();
        }),
        op("rejected edit", |c| {
            let refused = c.store.apply(tx("bad", vec![Op::Set { id: c.r, path: path("transform2d.x"), slot: Slot::Const(Value::Str("not a number".into())) }]));
            assert!(refused.is_err());
            c.frame();
        }),
        op("hidden-page edit", |c| {
            for x in [30.0, 10.0] {
                c.apply("hidden", vec![Op::Set { id: c.quiet_r, path: path("transform2d.x"), slot: Slot::Const(Value::Number(x)) }]);
                c.frame();
            }
        }),
        op("show and hide a page", |c| {
            let o = c.store.observe(everything(c.quiet));
            c.frame();
            c.store.unobserve(o);
            c.frame();
        }),
        op("observe nodes", |c| {
            let o = c.store.observe(Interest::Nodes(vec![c.r, c.g]));
            c.frame();
            c.store.unobserve(o);
            c.frame();
        }),
        op("scrub", |c| {
            for t in [0.5, 1.0, 0.0] {
                c.store.set_time(t);
                c.frame();
            }
        }),
        op("playing", |c| {
            crate::document_bridge::clock::set(&mut c.store, &[("loop_in", Value::Number(0.0)), ("loop_out", Value::Number(4.0)), ("playing", Value::Bool(true))]).unwrap();
            for _ in 0..3 {
                c.frame();
            }
            crate::document_bridge::clock::set(&mut c.store, &[("playing", Value::Bool(false)), ("t", Value::Number(0.0))]).unwrap();
            c.frame();
        }),
        op("save (canary)", |c| {
            assert!(!c.store.save().is_empty());
        }),
        op("walk (canary)", |c| {
            let doc = c.store.tree();
            let n: usize = iron_document::composition::compositions(doc).into_iter().map(|k| doc.descendants(k).len()).sum();
            assert!(n > 0);
        }),
    ]
}

/// The fixture's kinds, and each operation's work after a warm-up run,
/// measured from a settled store.
fn measure(snapshot: &str, copies: usize, big: usize) -> (BTreeMap<&'static str, usize>, Vec<(&'static str, Work)>) {
    let mut c = fixture(snapshot, copies, big);
    let kinds = kinds(&c.store);
    let mut out = Vec::new();
    for o in operations() {
        (o.setup)(&mut c);
        (o.run)(&mut c);
        (o.teardown)(&mut c);
        c.settle();
        (o.setup)(&mut c);
        take();
        (o.run)(&mut c);
        let w = take();
        (o.teardown)(&mut c);
        c.settle();
        out.push((o.name, w));
    }
    (kinds, out)
}

/// The counts of operation `i` that grew at both steps across `sizes`.
fn growing(sizes: &[&Vec<(&'static str, Work)>; 3], i: usize) -> Vec<(&'static str, String)> {
    let at = |k: usize, key: &str| sizes[k][i].1 .0.get(key).copied().unwrap_or(0);
    let keys: BTreeSet<&'static str> = sizes.iter().flat_map(|s| s[i].1 .0.keys().copied()).collect();
    let mut out = Vec::new();
    for key in keys {
        let slack = match key {
            "allocations" => ALLOCS_SLACK,
            "bytes" => BYTES_SLACK,
            _ => 0,
        };
        let (a, b, c) = (at(0, key), at(1, key), at(2, key));
        if b > a + slack && c > b + slack {
            out.push((key, format!("{key} {a} → {b} → {c}")));
        }
    }
    out
}

/// Which passes add to one count in one operation: `WORK_TRACE=<count>`
/// `WORK_OP=<operation>` (`COPIES=n`, default 1), printing each stack and
/// the work it added. A diagnostic, ignored.
///   WORK_TRACE=nodes_of_type WORK_OP="constant edit" cargo test -p ironfell --lib --features native --target aarch64-apple-darwin trace_one_count -- --ignored --nocapture
#[test]
#[ignore]
fn trace_one_count() {
    let (Ok(count), Ok(name)) = (std::env::var("WORK_TRACE"), std::env::var("WORK_OP")) else { return };
    let copies: usize = std::env::var("COPIES").ok().and_then(|c| c.parse().ok()).unwrap_or(1);
    let snapshot = super::demo::template_snapshot("demo").unwrap();
    let mut c = fixture(&snapshot, copies, 0);
    let o = operations().into_iter().find(|o| o.name == name).expect("an operation of that name");
    (o.setup)(&mut c);
    (o.run)(&mut c);
    (o.teardown)(&mut c);
    c.settle();
    (o.setup)(&mut c);
    iron_document::work::trace(Box::leak(count.into_boxed_str()));
    (o.run)(&mut c);
    let mut stacks: Vec<(String, u64)> = iron_document::work::take_trace().into_iter().collect();
    stacks.sort_by(|a, b| b.1.cmp(&a.1));
    for (stack, n) in stacks.iter().take(15) {
        println!("{n:>8}  {stack}");
    }
}

#[test]
fn work_does_not_grow_with_the_hidden_document() {
    let snapshot = super::demo::template_snapshot("demo").unwrap();
    let sizes = [(0, 0), (1, 0), (2, 0), (0, 400), (0, 800)];
    let runs: Vec<(BTreeMap<&'static str, usize>, Vec<(&'static str, Work)>)> = std::thread::scope(|s| {
        let snapshot = &snapshot;
        let handles: Vec<_> = sizes.iter().map(|&(c, b)| s.spawn(move || measure(snapshot, c, b))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    // Growing the pages grows every kind.
    println!("kinds: {:?} → {:?} → {:?}", runs[0].0, runs[1].0, runs[2].0);
    for (kind, &n) in &runs[0].0 {
        assert!(n > 0, "the fixture holds no {kind}");
        assert!(runs[1].0[kind] > n && runs[2].0[kind] > runs[1].0[kind], "twinning did not grow {kind}: {n} → {} → {}", runs[1].0[kind], runs[2].0[kind]);
    }
    let axes = [("pages", [&runs[0].1, &runs[1].1, &runs[2].1]), ("one big page", [&runs[0].1, &runs[3].1, &runs[4].1])];
    let mut grown: BTreeMap<(&'static str, &'static str), Vec<String>> = BTreeMap::new();
    for (axis, sizes) in &axes {
        for i in 0..sizes[0].len() {
            let name = sizes[0][i].0;
            for (key, what) in growing(sizes, i) {
                grown.entry((name, key)).or_default().push(format!("{axis}: {what}"));
            }
        }
    }
    println!("counts that grow with the hidden document:");
    let mut last = "";
    for ((name, _), why) in &grown {
        if *name != last {
            println!("  {name}:");
            last = name;
        }
        println!("    {}", why.join("; "));
    }
    let expected: BTreeSet<(&str, &str)> = SCALING.iter().map(|(o, k, _)| (*o, *k)).collect();
    let unexpected: Vec<&(&str, &str)> = grown.keys().filter(|k| !expected.contains(k)).collect();
    let stopped: Vec<&(&str, &str)> = expected.iter().filter(|k| !grown.contains_key(k)).collect();
    if !unexpected.is_empty() {
        println!("not in SCALING:");
        for (o, k) in &unexpected {
            println!("    (\"{o}\", \"{k}\", \"DS-?\"),");
        }
    }
    assert!(grown.contains_key(&("save (canary)", "allocations")) && grown.contains_key(&("walk (canary)", "descendants")), "the harness no longer sees a whole-document walk");
    assert!(unexpected.is_empty() && stopped.is_empty(), "growing but not listed in SCALING: {unexpected:?}; listed but no longer growing (remove them): {stopped:?}");
}
