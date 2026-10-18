//! Constraints in the document (constraint-system.md phase 2 exits) and the
//! drag API over the solver (§8), on a pendulum: a fixed platform, three
//! joints and a ball chained by distance constraints, arms drawn by bindings
//! that read the solved joints, and a bar whose height is bound to the ball.

use iron_document::facet::{Constraint, Fill, Name, Radius, Size, Transform2d};
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

fn bind(text: &str) -> Slot {
    Slot::Bound(Expr::parse(text).unwrap())
}

fn named(text: &str) -> Name {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(text.into()));
    n
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn pos(store: &Store, id: NodeId) -> [f64; 2] {
    let f = |p: &str| store.resolved(id, path(p)).and_then(|v| v.as_f64()).unwrap();
    [f("transform2d.x"), f("transform2d.y")]
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Canonical JSON without the version, for before/after comparisons.
fn content(store: &Store) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&store.save()).unwrap();
    v.as_object_mut().unwrap().remove("version");
    serde_json::to_string_pretty(&v).unwrap()
}

struct Pendulum {
    store: Store,
    group: NodeId,
    platform: NodeId,
    joints: [NodeId; 3],
    ball: NodeId,
    arms: [NodeId; 3],
    bar: NodeId,
    fix: NodeId,
    hinge: NodeId,
    links: [NodeId; 3],
    fix_rel: RelationId,
}

const LINK: f64 = 100.0;

/// The platform hinge is at world (320, 60); the chain hangs to (320, 360).
/// Joints start deliberately off their solution so apply has to solve.
fn pendulum() -> Pendulum {
    let mut store = Store::new();
    let ids = store.document().next_ids(14);
    let (group, platform) = (ids[0], ids[1]);
    let joints = [ids[2], ids[3], ids[4]];
    let ball = ids[5];
    let arms = [ids[6], ids[7], ids[8]];
    let bar = ids[9];
    let fix = ids[10];
    let hinge = ids[11];
    let links = [ids[12], ids[13]];
    let rels = store.document().next_relation_ids(9);
    let mut key = OrderKey::first();
    let mut order = || {
        let k = key.clone();
        key = OrderKey::after(&key);
        k
    };
    let mut ops = Vec::new();
    ops.push(Op::Create { id: group, ty: ty("group"), parent: None, order: order(), facets: vec![named("pendulum").wrap(), Transform2d::default().wrap()] });
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
    ops.push(Op::Create { id: platform, ty: ty("rect"), parent: Some(group), order: order(), facets: rect(240.0, 40.0, 160.0, 20.0, "platform") });
    for (i, (x, y)) in [(300.0, 100.0), (300.0, 150.0), (310.0, 230.0)].into_iter().enumerate() {
        ops.push(Op::Create { id: joints[i], ty: ty("ellipse"), parent: Some(group), order: order(), facets: circle(x, y, 6.0, &format!("j{i}")) });
    }
    ops.push(Op::Create { id: ball, ty: ty("ellipse"), parent: Some(group), order: order(), facets: circle(320.0, 300.0, 14.0, "ball") });
    // Arms: drawn from joint a to joint b by bindings over the solved joints.
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
        ops.push(Op::Create { id: arms[i], ty: ty("rect"), parent: Some(group), order: order(), facets: vec![t.wrap(), s.wrap(), Fill::default().wrap(), named(&format!("arm{}", i + 1)).wrap()] });
    }
    let mut t = Transform2d::default();
    t.x = num(720.0);
    t.y = num(60.0);
    let mut s = Size::default();
    s.w = num(30.0);
    s.h = bind(&format!("{ball}.transform2d.y - 60"));
    ops.push(Op::Create { id: bar, ty: ty("rect"), parent: Some(group), order: order(), facets: vec![t.wrap(), s.wrap(), Fill::default().wrap(), named("height").wrap()] });

    // Constraints, as children of the group.
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
    ops.push(Op::Create { id: fix, ty: ty("constraint"), parent: Some(group), order: order(), facets: constraint("fix", 0.0, "fixed platform") });
    ops.push(link(rels[0], fix, platform, "a", [0.0, 0.0]));
    ops.push(Op::Create { id: hinge, ty: ty("constraint"), parent: Some(group), order: order(), facets: constraint("coincident", 0.0, "hinge") });
    ops.push(link(rels[1], hinge, platform, "a", [80.0, 20.0]));
    ops.push(link(rels[2], hinge, joints[0], "b", [0.0, 0.0]));
    let third = store.document().next_ids(15)[14];
    let link_ids = [links[0], links[1], third];
    for i in 0..3 {
        let id = link_ids[i];
        ops.push(Op::Create { id, ty: ty("constraint"), parent: Some(group), order: order(), facets: constraint("distance", LINK, &format!("link{}", i + 1)) });
        ops.push(link(rels[3 + 2 * i], id, chain[i], "a", [0.0, 0.0]));
        ops.push(link(rels[4 + 2 * i], id, chain[i + 1], "b", [0.0, 0.0]));
    }
    store.apply(tx("build pendulum", ops)).unwrap();
    store.document().check_invariants().unwrap();
    Pendulum { store, group, platform, joints, ball, arms, bar, fix, hinge, links: link_ids, fix_rel: rels[0] }
}

fn assert_chain_holds(p: &Pendulum) {
    let s = &p.store;
    let j0 = pos(s, p.joints[0]);
    assert!(dist(j0, [320.0, 60.0]) < 1e-6, "j0 on the hinge: {j0:?}");
    let chain = [p.joints[0], p.joints[1], p.joints[2], p.ball];
    for w in chain.windows(2) {
        let d = dist(pos(s, w[0]), pos(s, w[1]));
        assert!((d - LINK).abs() < 1e-6, "{} -> {}: {d}", w[0], w[1]);
    }
    assert_eq!(pos(s, p.platform), [240.0, 40.0], "the fixed platform never moves");
}

#[test]
fn a_pendulum_built_from_ops_is_solved_on_apply() {
    let p = pendulum();
    assert_chain_holds(&p);
    let tx = p.store.history().past().last().unwrap();
    assert!(!tx.derived_ops.is_empty(), "the solver's writes are recorded");
    assert!(tx.derived_ops.iter().all(|op| matches!(op, Op::Set { id, .. } if p.joints.contains(id) || *id == p.ball)));
    let s = tx.solver.as_ref().expect("a summary");
    assert_eq!(s.clusters, vec![p.group], "the cluster is named after the group");
    assert!(s.moved.contains(&p.joints[0]) && s.moved.contains(&p.ball));
    assert!(s.unmet.is_empty());
    // One cluster, twelve variables (x, y, rot of four free nodes), the
    // platform's leaves known.
    let c = &p.store.clusters().list;
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].vars.len(), 12, "{:?}", c[0].vars);
    assert!(c[0].vars.iter().all(|v| v.node != p.platform));
    // The arms followed the solve through their bindings.
    let h = p.store.resolved(p.arms[0], path("size.h")).unwrap().as_f64().unwrap();
    assert!((h - LINK).abs() < 1e-6, "arm length {h}");
    // Least change keeps the chain near where it started, so it hangs bent;
    // the bar reads the ball's height through its binding either way.
    let bar_h = p.store.resolved(p.bar, path("size.h")).unwrap().as_f64().unwrap();
    assert!((bar_h - (pos(&p.store, p.ball)[1] - 60.0)).abs() < 1e-9, "bar height {bar_h}");
}

#[test]
fn setting_a_value_resolves_with_derived_ops_and_undo_restores_exactly() {
    let mut p = pendulum();
    let before = content(&p.store);
    let ops = p.links.iter().map(|&l| Op::Set { id: l, path: path("constraint.value"), slot: num(80.0) }).collect();
    let applied = p.store.apply(tx("shorten", ops)).unwrap();
    assert!(applied.solver.is_some());
    for w in [p.joints[0], p.joints[1], p.joints[2], p.ball].windows(2) {
        assert!((dist(pos(&p.store, w[0]), pos(&p.store, w[1])) - 80.0).abs() < 1e-6);
    }
    let tx = p.store.history().past().last().unwrap().clone();
    assert_eq!(tx.ops.len(), 3);
    assert!(tx.derived_ops.len() >= 6, "{:?}", tx.derived_ops);
    let after = content(&p.store);
    p.store.undo(1).unwrap().unwrap();
    assert_eq!(content(&p.store), before, "undo restores requested and inferred values");
    p.store.redo(2).unwrap().unwrap();
    assert_eq!(content(&p.store), after, "redo applies the recorded derived ops");
    // A redo transaction carries the derived ops as its ops and solves nothing new.
    let redo = p.store.history().past().last().unwrap();
    assert_eq!(redo.ops.len(), 3 + tx.derived_ops.len());
    assert!(redo.derived_ops.is_empty());
    p.store.document().check_invariants().unwrap();
}

#[test]
fn save_load_and_replay_from_empty_agree() {
    let mut p = pendulum();
    p.store.apply(tx("shorten", vec![Op::Set { id: p.links[1], path: path("constraint.value"), slot: num(60.0) }])).unwrap();
    let text = p.store.save();
    let loaded = Store::load(&text).unwrap();
    assert_eq!(loaded.save(), text);
    assert_eq!(loaded.clusters().list.len(), 1, "clusters rebuild on load");
    let mut replayed = Store::new();
    for t in p.store.history().log().cloned().collect::<Vec<_>>() {
        replayed.replay(&t).unwrap();
    }
    assert_eq!(replayed.save(), text, "replay applies recorded derived ops; the solver is never consulted");
}

#[test]
fn a_contradiction_is_unsatisfiable_and_names_the_culprits() {
    let mut p = pendulum();
    let version = p.store.document().version();
    let before = content(&p.store);
    // j1 coincident with j2 contradicts their distance of 100.
    let id = p.store.document().next_id();
    let rels = p.store.document().next_relation_ids(2);
    let mut c = Constraint::default();
    c.kind = Slot::Const(Value::Enum("coincident".into()));
    let link = |rel: RelationId, to: NodeId, role: &str| {
        let mut data = BTreeMap::new();
        data.insert("role".to_owned(), Value::Str(role.into()));
        Op::Link { rel: Relation { id: rel, from: id, to, rel: RelKind::Constrains, data } }
    };
    let errs = p
        .store
        .apply(tx(
            "weld",
            vec![
                Op::Create { id, ty: ty("constraint"), parent: Some(p.group), order: OrderKey::first(), facets: vec![c.wrap()] },
                link(rels[0], p.joints[1], "a"),
                link(rels[1], p.joints[2], "b"),
            ],
        ))
        .unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::Unsatisfiable, "{}", errs[0].message);
    let m = &errs[0].message;
    assert!(m.contains(&id.to_string()) && m.contains(&p.links[1].to_string()), "both sides of the contradiction are named: {m}");
    assert!(!m.contains(&p.links[0].to_string()), "an innocent constraint is not: {m}");
    assert_eq!(p.store.document().version(), version, "nothing applied");
    assert_eq!(content(&p.store), before);
    assert_eq!(p.store.clusters().list.len(), 1);
    p.store.document().check_invariants().unwrap();

    // Malformed constraints are BadConstraint: a missing operand, an unknown
    // kind, a duplicate.
    let errs = p
        .store
        .apply(tx("half", vec![Op::Create { id, ty: ty("constraint"), parent: Some(p.group), order: OrderKey::first(), facets: vec![Constraint::default().wrap()] }]))
        .unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::BadConstraint);
    assert!(errs[0].message.contains("missing operand"), "{}", errs[0].message);
    let mut dup = Constraint::default();
    dup.kind = Slot::Const(Value::Enum("distance".into()));
    dup.value = num(50.0);
    let errs = p
        .store
        .apply(tx(
            "dup",
            vec![
                Op::Create { id, ty: ty("constraint"), parent: Some(p.group), order: OrderKey::first(), facets: vec![dup.wrap()] },
                link(rels[0], p.joints[0], "a"),
                link(rels[1], p.joints[1], "b"),
            ],
        ))
        .unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::BadConstraint);
    assert!(errs[0].message.contains("already constrained by") && errs[0].message.contains(&p.links[0].to_string()), "{}", errs[0].message);
}

#[test]
fn a_binding_that_reads_and_feeds_a_cluster_is_a_cycle() {
    let mut p = pendulum();
    let errs = p
        .store
        .apply(tx("loop", vec![Op::Set { id: p.links[0], path: path("constraint.value"), slot: bind(&format!("{}.transform2d.x / 3", p.joints[1])) }]))
        .unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::Cycle, "{}", errs[0].message);
    assert!(errs[0].message.contains(&p.group.to_string()), "{}", errs[0].message);
    // A value bound to something outside the cluster is a driving dimension.
    let slider = p.store.document().next_id();
    let mut sl = iron_document::facet::Slider::default();
    sl.value = num(0.5);
    p.store
        .apply(tx(
            "slider",
            vec![
                Op::Create { id: slider, ty: ty("slider"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![Transform2d::default().wrap(), sl.wrap()] },
                Op::Set { id: p.links[2], path: path("constraint.value"), slot: bind(&format!("{slider}.slider.value * 200")) },
            ],
        ))
        .unwrap();
    assert!((dist(pos(&p.store, p.joints[2]), pos(&p.store, p.ball)) - 100.0).abs() < 1e-6);
    p.store.drain_changes();
    p.store.preview(slider, path("slider.value"), Value::Number(0.25));
    assert!((dist(pos(&p.store, p.joints[2]), pos(&p.store, p.ball)) - 50.0).abs() < 1e-6, "a scrub re-solves live");
    p.store.cancel_gesture();
    assert!((dist(pos(&p.store, p.joints[2]), pos(&p.store, p.ball)) - 100.0).abs() < 1e-6);
}

#[test]
fn the_pendulum_view_matches_the_golden_file() {
    let p = pendulum();
    let v = p.store.view(&ViewQuery { relation: Default::default(), scope: None, fidelity: Fidelity::Full, depth: None }).unwrap();
    // Solver variables are marked with the cluster that maintains them.
    assert!(v.contains(&format!(r#"x.by="{}""#, p.group)), "{v}");
    assert!(v.contains(r##"<constraint id="#"##) && v.contains(r#"kind="distance""#), "{v}");
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/pendulum.xml");
    if std::env::var_os("UPDATE_GOLDEN").is_some() || !golden.exists() {
        std::fs::write(&golden, &v).unwrap();
    }
    let want = std::fs::read_to_string(&golden).unwrap();
    assert_eq!(v, want, "the view the model sees changed; run with UPDATE_GOLDEN=1 if intended");
}

#[test]
fn a_set_out_of_reach_is_adjusted_and_reported_or_rejected_with_exact() {
    let mut p = pendulum();
    let version = p.store.document().version();
    let applied = p.store.apply(tx("push ball", vec![Op::Set { id: p.ball, path: path("transform2d.x"), slot: num(900.0) }])).unwrap();
    let s = applied.solver.expect("summary");
    assert_eq!(s.unmet.len(), 1, "{s:?}");
    assert_eq!(s.unmet[0].leaf, format!("{}.transform2d.x", p.ball));
    assert_eq!(s.unmet[0].requested, 900.0);
    assert!(s.unmet[0].solved < 900.0 && s.unmet[0].reason.contains(&p.group.to_string()));
    assert_chain_holds(&p);
    // Full extension toward the request: 300 from the hinge.
    assert!((dist(pos(&p.store, p.ball), [320.0, 60.0]) - 300.0).abs() < 1e-5);
    assert!(pos(&p.store, p.ball)[0] > 600.0);
    let last = p.store.history().past().last().unwrap();
    assert!(last.derived_ops.iter().any(|op| matches!(op, Op::Set { id, path: pth, .. } if *id == p.ball && pth.field == "x")), "the ball's x is itself adjusted");
    p.store.undo(1).unwrap().unwrap();
    assert_eq!(p.store.document().version(), Version(version.0 + 2));

    let mut strict = tx("push ball exactly", vec![Op::Set { id: p.ball, path: path("transform2d.x"), slot: num(900.0) }]);
    strict.exact = true;
    let errs = p.store.apply(strict).unwrap_err();
    assert_eq!(errs[0].kind, ErrorKind::Unsatisfiable);
    assert!(errs[0].message.contains("900"), "{}", errs[0].message);
    assert_chain_holds(&p);
}

#[test]
fn dragging_the_ball_moves_the_chain_and_commits_one_transaction() {
    let mut p = pendulum();
    let before = content(&p.store);
    let authored_x = p.store.document().leaf(p.ball, path("transform2d.x")).unwrap().constant().cloned();
    p.store.drain_changes();
    let h = p.store.begin_drag(p.ball, [0.0, 0.0]);
    assert_eq!(h.mode, DragMode::Cluster(p.group));
    let report = p.store.drag_to(&h, [420.0, 300.0]);
    assert!(report.converged && report.clusters == 1 && report.vars == 12, "{report:?}");
    let ball = pos(&p.store, p.ball);
    assert!(dist(ball, [420.0, 300.0]) < 1e-3, "reachable: {ball:?}");
    assert_chain_holds(&p);
    // Everything moved is previewed, nothing is written, the arms follow.
    assert_eq!(p.store.document().leaf(p.ball, path("transform2d.x")).unwrap().constant().cloned(), authored_x);
    let changed: Vec<NodeId> = p.store.drain_changes().into_iter().map(|c| c.leaf.node).collect();
    assert!(changed.contains(&p.joints[1]) && changed.contains(&p.arms[2]) && changed.contains(&p.bar), "{changed:?}");
    let rot = p.store.resolved(p.arms[2], path("transform2d.rot")).unwrap().as_f64().unwrap();
    assert!(rot.abs() > 0.1, "the last arm turned: {rot}");

    // Beyond reach: the chain straightens toward the pointer.
    p.store.drag_to(&h, [320.0, 500.0]);
    assert_chain_holds(&p);
    let ball = pos(&p.store, p.ball);
    assert!(dist(ball, [320.0, 360.0]) < 1e-4, "full extension: {ball:?}");

    // Release: one transaction, the ball requested, the joints derived.
    p.store.drag_to(&h, [400.0, 320.0]);
    p.store.end_drag(&h);
    let applied = p.store.commit_gesture("move circle", Actor::Human, 1).unwrap().unwrap();
    assert!(dist(pos(&p.store, p.ball), [400.0, 320.0]) < 1e-3);
    let tx = p.store.history().past().last().unwrap().clone();
    assert_eq!(tx.label, "move circle");
    assert!(tx.ops.iter().all(|op| matches!(op, Op::Set { id, .. } if *id == p.ball)), "{:?}", tx.ops);
    let derived: Vec<NodeId> = tx.derived_ops.iter().filter_map(|op| match op { Op::Set { id, .. } => Some(*id), _ => None }).collect();
    assert!(derived.contains(&p.joints[1]) && derived.contains(&p.joints[2]), "{derived:?}");
    assert!(!derived.contains(&p.ball), "the driven node is in ops, not derived ops: {derived:?}");
    assert!(applied.solver.unwrap().unmet.is_empty());
    assert!(!p.store.gesture_in_progress());
    assert_chain_holds(&p);
    p.store.undo(2).unwrap().unwrap();
    assert_eq!(content(&p.store), before, "one undo returns every part");
}

#[test]
fn moving_the_fix_to_the_ball_lets_the_platform_drive() {
    let mut p = pendulum();
    // Give the chain slack first: a fully extended chain cannot follow a
    // sideways platform drag at all.
    p.store.apply(tx("lift", vec![Op::Set { id: p.ball, path: path("transform2d.y"), slot: num(300.0) }])).unwrap();
    assert_chain_holds(&p);
    let rel = p.store.document().next_relation_id();
    let mut data = BTreeMap::new();
    data.insert("role".to_owned(), Value::Str("a".into()));
    p.store
        .apply(tx(
            "fix the ball",
            vec![Op::Unlink { id: p.fix_rel }, Op::Link { rel: Relation { id: rel, from: p.fix, to: p.ball, rel: RelKind::Constrains, data } }],
        ))
        .unwrap();
    let ball = pos(&p.store, p.ball);
    let h = p.store.begin_drag(p.platform, [80.0, 10.0]);
    assert_eq!(h.mode, DragMode::Cluster(p.group));
    p.store.drag_to(&h, [370.0, 50.0]); // 50 px to the right
    assert_eq!(pos(&p.store, p.ball), ball, "the fixed ball stays put");
    // The grab point lands under the pointer. The platform may also turn a
    // little (its rotation is free and the chain pulls on a different
    // point), so the hinge is wherever the platform now puts it.
    let grab = p.store.world_affine(p.platform).apply([80.0, 10.0]);
    assert!(dist(grab, [370.0, 50.0]) < 1e-3, "{grab:?}");
    assert!(pos(&p.store, p.platform)[0] > 285.0);
    let hinge = p.store.world_affine(p.platform).apply([80.0, 20.0]);
    let j0 = pos(&p.store, p.joints[0]);
    assert!(dist(j0, hinge) < 1e-6, "j0 follows the hinge: {j0:?} vs {hinge:?}");
    for w in [p.joints[0], p.joints[1], p.joints[2], p.ball].windows(2) {
        assert!((dist(pos(&p.store, w[0]), pos(&p.store, w[1])) - LINK).abs() < 1e-6);
    }
    p.store.end_drag(&h);
    p.store.commit_gesture("move rect", Actor::Human, 1).unwrap().unwrap();
    assert_eq!(pos(&p.store, p.ball), ball);
    let _ = p.hinge;
}

#[test]
fn a_bound_node_has_no_drag_and_says_why() {
    let mut p = pendulum();
    let h = p.store.begin_drag(p.arms[0], [6.0, 50.0]);
    let DragMode::Locked(reasons) = &h.mode else { panic!("{h:?}") };
    assert_eq!(reasons.len(), 2);
    assert!(reasons[0].contains("bound"), "{reasons:?}");
    assert_eq!(p.store.drag_to(&h, [0.0, 0.0]), SolveReport { converged: true, ..Default::default() });
    assert!(!p.store.gesture_in_progress());
    // The fixed platform in a cluster is locked too, with the cluster named.
    let h = p.store.begin_drag(p.platform, [0.0, 0.0]);
    let DragMode::Locked(reasons) = &h.mode else { panic!("{h:?}") };
    assert!(reasons[0].contains("fixed") && reasons[0].contains(&p.group.to_string()), "{reasons:?}");
}

#[test]
fn duplicating_the_pendulum_copies_its_mechanism_and_undo_removes_it() {
    let mut p = pendulum();
    let before = content(&p.store);
    let pasted = copy::duplicate(&p.store, &[p.group]).unwrap();
    assert!(pasted.notes.is_empty(), "{:?}", pasted.notes);
    p.store.apply(tx("duplicate group", pasted.ops)).unwrap();
    let doc = p.store.document();
    let copy = pasted.roots[0];
    // Right above the original, as a sibling.
    let roots: Vec<NodeId> = doc.roots().to_vec();
    assert_eq!(roots, vec![p.group, copy]);
    // Its constraints are its own: they name the copy's joints.
    let kids = doc.children(copy).to_vec();
    assert_eq!(kids.len(), doc.children(p.group).len());
    let is_constraint = |k: &NodeId| doc.live(*k).unwrap().ty.name() == "constraint";
    let constraints: Vec<NodeId> = kids.iter().copied().filter(is_constraint).collect();
    assert_eq!(constraints.len(), doc.children(p.group).iter().filter(|k| is_constraint(k)).count());
    assert!(!constraints.is_empty());
    for c in &constraints {
        for r in doc.relations_from(*c) {
            assert!(kids.contains(&r.to), "{c} constrains {} outside the copy", r.to);
        }
    }
    // The copy's ball moves; the original's stays.
    let ball = kids[p.store.document().children(p.group).iter().position(|&k| k == p.ball).unwrap()];
    let still = pos(&p.store, p.ball);
    let h = p.store.begin_drag(ball, [0.0, 0.0]);
    assert!(matches!(h.mode, DragMode::Cluster(_)), "{:?}", h.mode);
    p.store.drag_to(&h, [420.0, 300.0]);
    p.store.end_drag(&h);
    p.store.commit_gesture("move ball", Actor::Human, 0).unwrap().unwrap();
    assert!(dist(pos(&p.store, ball), still) > 50.0);
    assert!(dist(pos(&p.store, p.ball), still) < 1e-6);
    // Two undos: the drag, then the duplicate. What is live is the original
    // again; the copies stay behind only as tombstones (ids are never reused).
    p.store.undo(0).unwrap().unwrap();
    p.store.undo(0).unwrap().unwrap();
    let live = |text: &str| {
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        let nodes = v["nodes"].as_object().unwrap();
        let live: Vec<&String> = nodes.iter().filter(|(_, n)| n.get("tombstoned").is_none()).map(|(k, _)| k).collect();
        let comps: Vec<String> = v["components"].as_object().unwrap().iter().flat_map(|(kind, t)| t.as_object().unwrap().iter().filter(|(id, _)| live.contains(id)).map(move |(id, c)| format!("{kind} {id} {c}"))).collect();
        let rels: Vec<String> = v["relations"].as_array().unwrap().iter().map(|r| r.to_string()).collect();
        (live.into_iter().cloned().collect::<Vec<_>>(), comps, rels)
    };
    assert_eq!(live(&content(&p.store)), live(&before));
}

#[test]
fn a_constraint_reaching_outside_the_copy_stays_behind() {
    let mut p = pendulum();
    let pasted = copy::duplicate(&p.store, &[p.ball]).unwrap();
    // The ball's distance link to the last joint is not copied, and says so.
    assert_eq!(pasted.notes.len(), 1, "{:?}", pasted.notes);
    assert!(pasted.notes[0].contains("constraint") && pasted.notes[0].contains("outside the copy"), "{:?}", pasted.notes);
    p.store.apply(tx("duplicate ellipse", pasted.ops)).unwrap();
    let ball = pasted.roots[0];
    assert!(p.store.document().relations_to(ball).next().is_none());
    // Free: it drags anywhere without a solve.
    let h = p.store.begin_drag(ball, [0.0, 0.0]);
    assert!(matches!(h.mode, DragMode::Axes { .. }), "{:?}", h.mode);
}
