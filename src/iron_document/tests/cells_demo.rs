//! Demonstrations of the native cells' efficiency (reactive-engine handoff
//! §4.10, RE-5.4; the user's request, 2026-11-21). Measured against the old
//! pipeline while it was the differential reference (the handoff has that
//! table); since RE-5.6 removed it, this reports the cells alone, for a
//! later change to compare with. Each case reports p50 and p95 per change,
//! structural roots laid out, expressions evaluated, and settles that read
//! every repeat's inputs. Run in release:
//! `cargo test --release -p iron_document --target aarch64-apple-darwin --test suite -- --ignored --nocapture cells_demo`

use iron_document::*;
use std::time::Instant;

fn tx(ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: "demo".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false }
}

fn build(store: &mut StoreCore, body: &str) -> std::collections::BTreeMap<String, NodeId> {
    let src = format!(r#"<edit><composition name="demo" dims="2d">{body}</composition></edit>"#);
    let plan = iron_document::edit::plan(store, &src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let names = plan.aliases.clone();
    store.apply(tx(plan.ops)).unwrap();
    names
}

struct Run {
    p50: f64,
    p95: f64,
    lay_outs: f64,
    evaluations: f64,
    scans: f64,
}

/// Time `change(i)` over `n` rounds, with what each did on average.
fn measure(store: &mut StoreCore, n: usize, mut change: impl FnMut(&mut StoreCore, usize)) -> Run {
    let (l0, e0, s0) = (layout::lay_out_count(), store.reactive().evaluations, settle_scan_count());
    let mut times: Vec<f64> = (0..n)
        .map(|i| {
            let t = Instant::now();
            change(store, i);
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let per = |d: u64| d as f64 / n as f64;
    Run { p50: times[n / 2], p95: times[(n * 95 / 100).min(n - 1)], lay_outs: per(layout::lay_out_count() - l0), evaluations: per(store.reactive().evaluations - e0), scans: per(settle_scan_count() - s0) }
}

fn report(case: &str, new: &Run) {
    println!("{case:<44} {:>8.3} / {:>8.3} ms, {:>6.1} lay-outs, {:>7.1} evals, {:>4.1} scans", new.p50, new.p95, new.lay_outs, new.evaluations, new.scans);
}

fn both(case: &str, body: &str, n: usize, change: impl Fn(&mut StoreCore, &std::collections::BTreeMap<String, NodeId>, usize)) -> StoreCore {
    let mut new = StoreCore::new();
    let names = build(&mut new, body);
    let b = measure(&mut new, n, |s, i| change(s, &names, i));
    report(case, &b);
    new
}

fn set_w(s: &mut StoreCore, id: NodeId, w: f64) {
    s.apply(tx(vec![Op::Set { id, path: "size.w".parse().unwrap(), slot: Slot::Const(Value::Number(w)) }])).unwrap();
}

/// N stacks of three rects: one edit in the first.
fn containers(n: usize) -> String {
    (0..n)
        .map(|i| format!(r#"<group id="$g{i}" x="0" y="{y}" size.w="10" size.h="10" layout.mode="stack_x"><rect id="$r{i}" w="10" h="10"/><rect w="10" h="10"/><rect w="10" h="10"/></group>"#, y = i * 20))
        .collect()
}

/// K stacks, each one's gap bound to the last one's laid-out width.
fn chain(k: usize) -> String {
    (0..k)
        .map(|i| {
            let gap = if i == 0 { "0".to_owned() } else { format!("{{= $g{}.size.w / 10}}", i - 1) };
            format!(r#"<group id="$g{i}" x="0" y="{y}" size.w="10" size.h="10" layout.mode="stack_x" layout.gap="{gap}"><rect id="$r{i}" w="10" h="10"/><rect id="$s{i}" w="10" h="10"/></group>"#, y = i * 20)
        })
        .collect()
}

#[test]
#[ignore]
fn cells_demo() {
    println!("p50 / p95 per change; lay-outs = structural roots laid out per change; scans = settles reading every repeat's inputs");
    for n in [10, 100, 1000] {
        both(&format!("{n} containers, edit one"), &containers(n), 40, |s, names, i| set_w(s, names["$r0"], if i % 2 == 0 { 30.0 } else { 10.0 }));
    }
    for n in [10, 100] {
        both(&format!("{n} containers, preview a width in one"), &containers(n), 40, |s, names, i| s.preview(names["$r0"], "size.w".parse().unwrap(), Value::Number(10.0 + (i % 7) as f64)));
    }
    for k in [4, 8] {
        let new = both(&format!("a chain of {k} layout -> binding -> layout"), &chain(k), 40, |s, names, i| set_w(s, names["$r0"], if i % 2 == 0 { 30.0 } else { 10.0 }));
        // Where the last link's second rect ends up, after an edit making the
        // first wider: the chain followed to its end, or not.
        let last = |s: &StoreCore| {
            let names: Vec<NodeId> = s.tree().nodes().filter(|n| n.is_live()).map(|n| n.id).collect();
            names.into_iter().filter_map(|id| s.resolved(id, "transform2d.x".parse().unwrap()).and_then(|v| v.as_f64())).fold(0.0f64, f64::max)
        };
        println!("    furthest placement after the last change: {:.3}", last(&new));
    }
    let big = format!(r#"<group x="0" y="0" size.w="10" size.h="10" layout.mode="stack_y"><rect id="$r0" w="10" h="10"/>{}</group>"#, (0..999).map(|_| r#"<rect w="10" h="10"/>"#).collect::<String>());
    both("one container of 1,000 items, edit one", &big, 40, |s, names, i| set_w(s, names["$r0"], if i % 2 == 0 { 30.0 } else { 10.0 }));
    // A drag's frames (a preview) and a reorder in it: what DS-9's
    // structural keys add to an observed large layout (structure-in-the-graph.md §3.5).
    both("one container of 1,000 items, preview one", &big, 40, |s, names, i| s.preview(names["$r0"], "size.w".parse().unwrap(), Value::Number(if i % 2 == 0 { 30.0 } else { 10.0 })));
    both("one container of 1,000 items, reorder one", &big, 40, |s, names, i| {
        let r0 = names["$r0"];
        let kids = s.tree().children(s.tree().live(r0).unwrap().parent.unwrap()).to_vec();
        let order = if i % 2 == 0 { OrderKey::after(&s.tree().live(*kids.last().unwrap()).unwrap().order) } else { OrderKey::before(&s.tree().live(kids[0]).unwrap().order) };
        s.apply(tx(vec![Op::Reorder { id: r0, order }])).unwrap();
    });
    // Every container re-laid out by one edit (the RE-5.6 council's case):
    // each claim that moves clears layout's claim errors.
    let shared = r#"<rect id="$src" w="10" h="10"/>"#.to_owned()
        + &(0..1000).map(|i| format!(r#"<group x="0" y="{y}" size.w="10" size.h="10" layout.mode="stack_x" layout.gap="{{= $src.size.w / 10}}">{}</group>"#, r#"<rect w="10" h="10"/>"#.repeat(10), y = 20 + i * 20)).collect::<String>();
    both("1,000 containers, an edit all of them read", &shared, 10, |s, names, i| set_w(s, names["$src"], if i % 2 == 0 { 30.0 } else { 10.0 }));
    let repeats: String = (0..200).map(|i| format!(r#"<group y="{y}" repeat.count="3"><rect w="5" h="5"/></group>"#, y = i * 10)).collect::<String>() + r#"<rect id="$lone" w="10" h="10"/>"#;
    both("200 repeats, an edit none of them reads", &repeats, 40, |s, names, i| set_w(s, names["$lone"], if i % 2 == 0 { 30.0 } else { 10.0 }));
    // Load: 100 containers built from nothing.
    let mut best = f64::MAX;
    for _ in 0..5 {
        let mut s = StoreCore::new();
        let t = Instant::now();
        build(&mut s, &containers(100));
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("{:<44} {:>8.3} ms (best of 5)", "building 100 containers", best);
}
