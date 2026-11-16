//! A timing probe (reactive-engine handoff §4.1, task 1): what the solver
//! costs with many constraint clusters at rest. 1, 10 and 100 pendulums
//! (tests/constraints.rs's, offset so they do not touch) in one store; the
//! time of one gesture step dragging one ball, and of a transaction setting
//! an unrelated constant. Today every cluster is solved at each, whether or
//! not anything of it changed (spec §9.4). Ignored; run with
//!   cargo test -p iron_document --release --target aarch64-apple-darwin --test solve_cost -- --ignored --nocapture

use iron_document::facet::{Constraint, Fill, Name, Radius, Size, Transform2d};
use iron_document::*;
use std::collections::BTreeMap;
use std::time::Instant;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
}

fn bind(text: &str) -> Slot {
    Slot::Bound(Expr::parse(text).unwrap())
}

fn named(text: &str) -> Name {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(text.into()));
    n
}

const LINK: f64 = 100.0;

/// tests/constraints.rs's pendulum, moved right by `dx`, applied to `store`.
/// Returns its ball.
fn pendulum(store: &mut Store, order: &mut OrderKey, dx: f64) -> NodeId {
    let ids = store.document().next_ids(15);
    let (group, platform) = (ids[0], ids[1]);
    let joints = [ids[2], ids[3], ids[4]];
    let ball = ids[5];
    let arms = [ids[6], ids[7], ids[8]];
    let bar = ids[9];
    let (fix, hinge) = (ids[10], ids[11]);
    let links = [ids[12], ids[13], ids[14]];
    let rels = store.document().next_relation_ids(9);
    let mut key = OrderKey::first();
    let mut next = || {
        let k = key.clone();
        key = OrderKey::after(&key);
        k
    };
    let mut ops = Vec::new();
    *order = OrderKey::after(order);
    let mut gt = Transform2d::default();
    gt.x = num(dx);
    ops.push(Op::Create { id: group, ty: ty("group"), parent: None, order: order.clone(), facets: vec![named("pendulum").wrap(), gt.wrap()] });
    let rect = |x: f64, y: f64, w: f64, h: f64, name: &str| {
        let mut t = Transform2d::default();
        t.x = num(x);
        t.y = num(y);
        let mut s = Size::default();
        s.w = num(w);
        s.h = num(h);
        vec![t.wrap(), s.wrap(), Fill::default().wrap(), named(name).wrap()]
    };
    let circle = |x: f64, y: f64, r: f64, name: &str| {
        let mut t = Transform2d::default();
        t.x = num(x);
        t.y = num(y);
        let mut rad = Radius::default();
        rad.rx = num(r);
        rad.ry = num(r);
        vec![t.wrap(), rad.wrap(), Fill::default().wrap(), named(name).wrap()]
    };
    ops.push(Op::Create { id: platform, ty: ty("rect"), parent: Some(group), order: next(), facets: rect(240.0, 40.0, 160.0, 20.0, "platform") });
    for (i, (x, y)) in [(300.0, 100.0), (300.0, 150.0), (310.0, 230.0)].into_iter().enumerate() {
        ops.push(Op::Create { id: joints[i], ty: ty("ellipse"), parent: Some(group), order: next(), facets: circle(x, y, 6.0, &format!("j{i}")) });
    }
    ops.push(Op::Create { id: ball, ty: ty("ellipse"), parent: Some(group), order: next(), facets: circle(320.0, 300.0, 14.0, "ball") });
    let chain = [joints[0], joints[1], joints[2], ball];
    for i in 0..3 {
        let (a, b) = (chain[i], chain[i + 1]);
        let mut t = Transform2d::default();
        t.rot = bind(&format!("atan2({b}.transform2d.y - {a}.transform2d.y, {b}.transform2d.x - {a}.transform2d.x) - pi / 2"));
        t.x = bind(&format!("{a}.transform2d.x - 6 * cos({}.transform2d.rot)", arms[i]));
        t.y = bind(&format!("{a}.transform2d.y - 6 * sin({}.transform2d.rot)", arms[i]));
        let mut s = Size::default();
        s.w = num(12.0);
        s.h = bind(&format!("hypot({b}.transform2d.x - {a}.transform2d.x, {b}.transform2d.y - {a}.transform2d.y)"));
        ops.push(Op::Create { id: arms[i], ty: ty("rect"), parent: Some(group), order: next(), facets: vec![t.wrap(), s.wrap(), Fill::default().wrap(), named(&format!("arm{}", i + 1)).wrap()] });
    }
    let mut t = Transform2d::default();
    t.x = num(720.0);
    t.y = num(60.0);
    let mut s = Size::default();
    s.w = num(30.0);
    s.h = bind(&format!("{ball}.transform2d.y - 60"));
    ops.push(Op::Create { id: bar, ty: ty("rect"), parent: Some(group), order: next(), facets: vec![t.wrap(), s.wrap(), Fill::default().wrap(), named("height").wrap()] });
    let constraint = |kind: &str, value: f64, name: &str| {
        let mut c = Constraint::default();
        c.kind = Slot::Const(Value::Enum(kind.into()));
        c.value = num(value);
        vec![c.wrap(), named(name).wrap()]
    };
    let link = |rel: RelationId, from: NodeId, to: NodeId, role: &str, anchor: [f64; 2]| {
        let mut data = BTreeMap::new();
        data.insert("role".to_owned(), Value::Str(role.into()));
        data.insert("anchor".to_owned(), Value::Vec2(anchor));
        Op::Link { rel: Relation { id: rel, from, to, rel: RelKind::Constrains, data } }
    };
    ops.push(Op::Create { id: fix, ty: ty("constraint"), parent: Some(group), order: next(), facets: constraint("fix", 0.0, "fixed platform") });
    ops.push(link(rels[0], fix, platform, "a", [0.0, 0.0]));
    ops.push(Op::Create { id: hinge, ty: ty("constraint"), parent: Some(group), order: next(), facets: constraint("coincident", 0.0, "hinge") });
    ops.push(link(rels[1], hinge, platform, "a", [80.0, 20.0]));
    ops.push(link(rels[2], hinge, joints[0], "b", [0.0, 0.0]));
    for i in 0..3 {
        ops.push(Op::Create { id: links[i], ty: ty("constraint"), parent: Some(group), order: next(), facets: constraint("distance", LINK, &format!("link{}", i + 1)) });
        ops.push(link(rels[3 + 2 * i], links[i], chain[i], "a", [0.0, 0.0]));
        ops.push(link(rels[4 + 2 * i], links[i], chain[i + 1], "b", [0.0, 0.0]));
    }
    store.apply(tx("build pendulum", ops)).unwrap();
    ball
}

fn p50(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

#[test]
#[ignore]
fn solver_cost_with_clusters_at_rest() {
    for count in [1, 10, 100] {
        let mut store = Store::new();
        let mut order = OrderKey::first();
        let balls: Vec<NodeId> = (0..count).map(|k| pendulum(&mut store, &mut order, 900.0 * k as f64)).collect();
        assert_eq!(store.clusters().list.len(), count);
        // An unrelated rect, for the constant edit.
        let rect = store.document().next_id();
        order = OrderKey::after(&order);
        let mut s = Size::default();
        s.w = num(10.0);
        s.h = num(10.0);
        store.apply(tx("rect", vec![Op::Create { id: rect, ty: ty("rect"), parent: None, order, facets: vec![Transform2d::default().wrap(), s.wrap()] }])).unwrap();
        let h = store.begin_drag(balls[0], [0.0, 0.0]);
        let mut steps = Vec::new();
        for i in 0..60 {
            let a = i as f64 * 0.1;
            let t = Instant::now();
            store.drag_to(&h, [320.0 + 80.0 * a.cos(), 300.0 + 40.0 * a.sin()]);
            steps.push(t.elapsed().as_secs_f64() * 1e3);
        }
        store.end_drag(&h);
        let mut sets = Vec::new();
        for i in 0..9 {
            let t = Instant::now();
            store.apply(tx("move", vec![Op::Set { id: rect, path: "transform2d.x".parse().unwrap(), slot: num(i as f64) }])).unwrap();
            sets.push(t.elapsed().as_secs_f64() * 1e3);
        }
        println!("{count} pendulums: drag step p50 {:.3} ms; unrelated constant edit p50 {:.3} ms", p50(steps), p50(sets));
    }
}
