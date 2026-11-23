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
/// The constraints' problems: what grouping could not use or solve, and
/// what doesn't hold at a cluster's pose (DS-7).
fn constraint_problems(store: &Store) -> Vec<(NodeId, String)> {
    store.problems().into_iter().filter(|p| p.kind == "constraint").map(|p| (p.node, p.message)).collect()
}

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
    let ids = store.authored().next_ids(14);
    let (group, platform) = (ids[0], ids[1]);
    let joints = [ids[2], ids[3], ids[4]];
    let ball = ids[5];
    let arms = [ids[6], ids[7], ids[8]];
    let bar = ids[9];
    let fix = ids[10];
    let hinge = ids[11];
    let links = [ids[12], ids[13]];
    let rels = store.authored().next_relation_ids(9);
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
    let third = store.authored().next_ids(15)[14];
    let link_ids = [links[0], links[1], third];
    for i in 0..3 {
        let id = link_ids[i];
        ops.push(Op::Create { id, ty: ty("constraint"), parent: Some(group), order: order(), facets: constraint("distance", LINK, &format!("link{}", i + 1)) });
        ops.push(link(rels[3 + 2 * i], id, chain[i], "a", [0.0, 0.0]));
        ops.push(link(rels[4 + 2 * i], id, chain[i + 1], "b", [0.0, 0.0]));
    }
    store.apply(tx("build pendulum", ops)).unwrap();
    store.authored().check_invariants().unwrap();
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
    p.store.authored().check_invariants().unwrap();
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

/// A contradiction is applied, not refused (DS-7): the cluster keeps the
/// pose it had, and its problem names the constraints at fault.
#[test]
fn a_contradiction_applies_keeps_its_pose_and_names_the_culprits() {
    let mut p = pendulum();
    let version = p.store.authored().version();
    let joints: Vec<_> = p.joints.iter().map(|&j| pos(&p.store, j)).collect();
    // j1 coincident with j2 contradicts their distance of 100.
    let id = p.store.authored().next_id();
    let rels = p.store.authored().next_relation_ids(2);
    let mut c = Constraint::default();
    c.kind = Slot::Const(Value::Enum("coincident".into()));
    let link = |rel: RelationId, to: NodeId, role: &str| {
        let mut data = BTreeMap::new();
        data.insert("role".to_owned(), Value::Str(role.into()));
        Op::Link { rel: Relation { id: rel, from: id, to, rel: RelKind::Constrains, data } }
    };
    let applied = p
        .store
        .apply(tx(
            "weld",
            vec![
                Op::Create { id, ty: ty("constraint"), parent: Some(p.group), order: OrderKey::first(), facets: vec![c.wrap()] },
                link(rels[0], p.joints[1], "a"),
                link(rels[1], p.joints[2], "b"),
            ],
        ))
        .unwrap_or_else(|e| panic!("applied, not refused: {e:?}"));
    assert!(p.store.authored().version() > version, "applied");
    assert_eq!(p.joints.iter().map(|&j| pos(&p.store, j)).collect::<Vec<_>>(), joints, "the cluster keeps its pose");
    // The reply names the constraints at fault, from the failed solve.
    let told: Vec<String> = applied.solver.as_ref().map(|s| s.problems.iter().map(|q| q.message.clone()).collect()).unwrap_or_default();
    assert!(told.iter().any(|m| m.contains(&id.to_string()) && m.contains(&p.links[1].to_string())), "both sides of the contradiction are named: {told:?}");
    assert!(told.iter().all(|m| !m.contains(&p.links[0].to_string())), "an innocent constraint is not: {told:?}");
    // The document's problems say what doesn't hold where it is.
    let problems = constraint_problems(&p.store);
    assert!(problems.iter().any(|(n, _)| *n == id), "{problems:?}");
    // Undo takes it back, and its problem with it: the problem is read from
    // the pose, not remembered from the solve.
    p.store.undo(0).unwrap().unwrap();
    assert!(!p.store.tree().is_live(id));
    assert!(constraint_problems(&p.store).is_empty(), "{:?}", constraint_problems(&p.store));
    p.store.redo(0).unwrap().unwrap();
    assert!(!constraint_problems(&p.store).is_empty(), "redo shows it again");
    p.store.undo(0).unwrap().unwrap();
    p.store.authored().check_invariants().unwrap();

    // A malformed constraint (a missing operand, an unknown kind, a
    // duplicate) is applied and reported; the rest still solve.
    let id = p.store.authored().next_id();
    p.store.apply(tx("half", vec![Op::Create { id, ty: ty("constraint"), parent: Some(p.group), order: OrderKey::first(), facets: vec![Constraint::default().wrap()] }])).unwrap();
    let problems = constraint_problems(&p.store);
    assert!(problems.iter().any(|(n, m)| *n == id && m.contains("missing operand")), "{problems:?}");
    assert_eq!(p.store.clusters().list.len(), 1, "the good constraints still form their cluster");
    p.store.undo(0).unwrap().unwrap();
    let mut dup = Constraint::default();
    dup.kind = Slot::Const(Value::Enum("distance".into()));
    dup.value = num(50.0);
    let id = p.store.authored().next_id();
    let rels = p.store.authored().next_relation_ids(2);
    let link = |rel: RelationId, to: NodeId, role: &str| {
        let mut data = BTreeMap::new();
        data.insert("role".to_owned(), Value::Str(role.into()));
        Op::Link { rel: Relation { id: rel, from: id, to, rel: RelKind::Constrains, data } }
    };
    p.store
        .apply(tx(
            "dup",
            vec![
                Op::Create { id, ty: ty("constraint"), parent: Some(p.group), order: OrderKey::first(), facets: vec![dup.wrap()] },
                link(rels[0], p.joints[0], "a"),
                link(rels[1], p.joints[1], "b"),
            ],
        ))
        .unwrap();
    let problems = constraint_problems(&p.store);
    assert!(problems.iter().any(|(n, m)| *n == id && m.contains("already constrained by") && m.contains(&p.links[0].to_string())), "the later of the pair is left out: {problems:?}");
    assert!((dist(pos(&p.store, p.joints[0]), pos(&p.store, p.joints[1])) - 100.0).abs() < 1e-6, "the first of the pair still holds");
}

#[test]
fn a_binding_that_reads_and_feeds_a_cluster_is_a_cycle() {
    let mut p = pendulum();
    // Applied and reported, not refused (DS-7): the cluster is not solved.
    p.store.apply(tx("loop", vec![Op::Set { id: p.links[0], path: path("constraint.value"), slot: bind(&format!("{}.transform2d.x / 3", p.joints[1])) }])).unwrap();
    let problems = constraint_problems(&p.store);
    assert!(problems.iter().any(|(n, m)| *n == p.links[0] && m.contains("feeds the same cluster") && m.contains(&p.group.to_string())), "{problems:?}");
    p.store.undo(0).unwrap().unwrap();
    assert!(constraint_problems(&p.store).is_empty());
    // A value bound to something outside the cluster is a driving dimension.
    let slider = p.store.authored().next_id();
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
    p.store.take_changes();
    p.store.preview(slider, path("slider.value"), Value::Number(0.25));
    assert!((dist(pos(&p.store, p.joints[2]), pos(&p.store, p.ball)) - 50.0).abs() < 1e-6, "a scrub re-solves live");
    p.store.cancel_gesture();
    assert!((dist(pos(&p.store, p.joints[2]), pos(&p.store, p.ball)) - 100.0).abs() < 1e-6);
}

#[test]
fn the_pendulum_view_matches_the_golden_file() {
    let p = pendulum();
    let v = p.store.view(&ViewQuery { omit_defaults: false, expand: Vec::new(), relation: Default::default(), scope: None, fidelity: Fidelity::Full, depth: None }).unwrap();
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
    let version = p.store.authored().version();
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
    assert_eq!(p.store.authored().version(), Version(version.0 + 2));

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
    let authored_x = p.store.tree().leaf(p.ball, path("transform2d.x")).unwrap().constant().cloned();
    p.store.take_changes();
    let h = p.store.begin_drag(p.ball, [0.0, 0.0]);
    assert_eq!(h.mode, DragMode::Cluster(p.group));
    let report = p.store.drag_to(&h, [420.0, 300.0]);
    assert!(report.converged && report.clusters == 1 && report.vars == 12, "{report:?}");
    let ball = pos(&p.store, p.ball);
    assert!(dist(ball, [420.0, 300.0]) < 1e-3, "reachable: {ball:?}");
    assert_chain_holds(&p);
    // Everything moved is previewed, nothing is written, the arms follow.
    assert_eq!(p.store.tree().leaf(p.ball, path("transform2d.x")).unwrap().constant().cloned(), authored_x);
    let changed: Vec<NodeId> = p.store.take_changes().values.into_iter().map(|l| l.node).collect();
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
    let rel = p.store.authored().next_relation_id();
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
    let doc = p.store.tree();
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
    let ball = kids[p.store.tree().children(p.group).iter().position(|&k| k == p.ball).unwrap()];
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
        let comps: Vec<String> = v["facets"].as_object().unwrap().iter().flat_map(|(kind, t)| t.as_object().unwrap().iter().filter(|(id, _)| live.contains(id)).map(move |(id, c)| format!("{kind} {id} {c}"))).collect();
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
    assert!(p.store.tree().relations_to(ball).next().is_none());
    // Free: it drags anywhere without a solve.
    let h = p.store.begin_drag(ball, [0.0, 0.0]);
    assert!(matches!(h.mode, DragMode::Axes { .. }), "{:?}", h.mode);
}

#[test]
fn a_live_solve_leaves_nothing_behind_once_a_transaction_applies() {
    // A producer publishing every frame re-solves the clusters live, and the
    // solve previews its pose. Outside a gesture nothing owned those
    // previews, so once a transaction (or an undo) removed the cluster, a
    // stale preview kept shadowing what the document now says: a meter
    // unplugged by undo stayed at its mated y (smoke, 2026-10-20).
    let mut p = pendulum();
    let ball = p.ball;
    let before = pos(&p.store, ball);
    // A producer driving a link's length moves the chain live, by preview.
    let stats = p.store.register_producer("stats");
    p.store.publish(stats, BTreeMap::from([(Leaf { node: p.links[0], path: path("constraint.value") }, Value::Number(150.0))]));
    let live = pos(&p.store, ball);
    assert!(dist(before, live) > 1e-6, "the live solve previews a pose of its own: {before:?} → {live:?}");

    // Delete every constraint and place the ball: no cluster is left, and
    // the ball reads its new constant, not the live solve's pose.
    let constraints: Vec<NodeId> = p.store.tree().children(p.group).iter().copied().filter(|&c| p.store.tree().live(c).is_some_and(|n| n.ty.name() == "constraint")).collect();
    assert!(!constraints.is_empty());
    let mut ops: Vec<Op> = constraints.iter().map(|&id| Op::Delete { id }).collect();
    ops.push(Op::Set { id: ball, path: path("transform2d.x"), slot: num(500.0) });
    ops.push(Op::Set { id: ball, path: path("transform2d.y"), slot: num(420.0) });
    p.store.apply(tx("free the ball", ops)).unwrap();
    p.store.publish(stats, BTreeMap::new());
    assert_eq!(pos(&p.store, ball), [500.0, 420.0]);
    // Undo, which bypasses `apply`, brings the chain back; then a ball
    // placed by a later transaction reads what it says once more.
    p.store.undo(0).unwrap().unwrap();
    p.store.publish(stats, BTreeMap::new());
    assert!(dist(pos(&p.store, ball), before) < 1e-3, "{:?} vs {before:?}", pos(&p.store, ball));
    p.store.undo(0);
}

/// Phase 5's first step (reactive-engine.md §9.4, A6): a cluster nothing of
/// which changed is not solved again, so a drag in one cluster solves no
/// other. (Under the owners' check, each cluster left is solved anyway and
/// must come out where it is.)
#[test]
fn a_drag_in_one_cluster_solves_no_other() {
    let mut p = pendulum();
    let parent = p.store.tree().live(p.group).unwrap().parent;
    let built = markup::ops(r#"<ellipse id="$a" x="0" y="400" rx="5" ry="5"/><ellipse id="$b" x="60" y="400" rx="5" ry="5"/><constraint id="$f" kind="fix" value="0"/><constraint id="$d" kind="distance" value="40"/>"#, parent, OrderKey::after(&OrderKey::first()), p.store.authored().next_id()).unwrap();
    let n = built.named;
    let rels = p.store.authored().next_relation_ids(3);
    let link = |id: RelationId, from: NodeId, to: NodeId, role: &str| {
        let mut data = std::collections::BTreeMap::new();
        data.insert("role".to_owned(), Value::Str(role.into()));
        data.insert("anchor".to_owned(), Value::Vec2([0.0, 0.0]));
        Op::Link { rel: Relation { id, from, to, rel: RelKind::Constrains, data } }
    };
    p.store.apply(tx("second cluster", built.ops.into_iter().chain([link(rels[0], n["f"], n["a"], "a"), link(rels[1], n["d"], n["a"], "a"), link(rels[2], n["d"], n["b"], "b")]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(p.store.clusters().list.len(), 2);
    let h = p.store.begin_drag(p.ball, [0.0, 0.0]);
    let report = p.store.drag_to(&h, [420.0, 300.0]);
    assert!(report.converged && report.clusters == 1, "{report:?}");
    assert_chain_holds(&p);
}

fn constrains(id: RelationId, from: NodeId, to: NodeId, role: &str) -> Op {
    let mut data = std::collections::BTreeMap::new();
    data.insert("role".to_owned(), Value::Str(role.into()));
    data.insert("anchor".to_owned(), Value::Vec2([0.0, 0.0]));
    Op::Link { rel: Relation { id, from, to, rel: RelKind::Constrains, data } }
}

/// A sheet with two groups, at x 0 and x 20, and a rect `a` in the first
/// at 20 and `b` in the second at 0, coincident in the world, both fixed.
fn two_parents() -> (Store, BTreeMap<String, NodeId>) {
    let mut store = Store::new();
    let comp = store.authored().next_id();
    let mut c = facet::Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![named("sheet").wrap(), c.wrap()] }])).unwrap();
    let built = markup::ops(r#"<group id="$g0" x="0" y="0"><rect id="$a" x="20" y="0" w="5" h="5"/></group><group id="$g1" x="20" y="0"><rect id="$b" x="0" y="0" w="5" h="5"/></group><constraint id="$c" kind="coincident" value="0"/><constraint id="$fa" kind="fix" value="0"/><constraint id="$fb" kind="fix" value="0"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let n = built.named;
    let r = store.authored().next_relation_ids(4);
    store.apply(tx("tie", built.ops.into_iter().chain([constrains(r[0], n["c"], n["a"], "a"), constrains(r[1], n["c"], n["b"], "b"), constrains(r[2], n["fa"], n["a"], "a"), constrains(r[3], n["fb"], n["b"], "a")]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    (store, n)
}

/// Swapping two coincident operands between their parents keeps the same
/// ancestor leaves but breaks the constraint: the cluster is solved again,
/// and with both operands fixed it fails, which is reported (DS-7).
#[test]
fn swapping_constrained_operands_between_parents_is_solved_again() {
    let (mut store, n) = two_parents();
    store.apply(tx("swap", vec![Op::Reparent { id: n["a"], parent: Some(n["g1"]), order: OrderKey::first() }, Op::Reparent { id: n["b"], parent: Some(n["g0"]), order: OrderKey::first() }])).unwrap();
    let problems = constraint_problems(&store);
    assert!(problems.iter().any(|(id, _)| *id == n["c"]), "coincident operands 40 apart, both fixed: {problems:?}");
}

/// A live solve's answer is a preview: the next transaction, even an
/// unrelated one, still solves the cluster and writes it.
#[test]
fn a_live_solve_is_written_by_the_next_transaction() {
    let mut store = Store::new();
    let comp = store.authored().next_id();
    let mut c = facet::Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![named("sheet").wrap(), c.wrap()] }])).unwrap();
    let built = markup::ops(r#"<group id="$g" name="g" x="0" y="0"><rect id="$a" x="0" y="0" w="5" h="5"/></group><constraint id="$p" kind="pin" value="0"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let n = built.named;
    let mut link = constrains(store.authored().next_relation_ids(1)[0], n["p"], n["a"], "a");
    if let Op::Link { rel } = &mut link {
        rel.data.insert("at".to_owned(), Value::Vec2([15.0, 0.0]));
    }
    store.apply(tx("pin", built.ops.into_iter().chain([link]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    let x = path("transform2d.x");
    assert_eq!(store.tree().leaf(n["a"], x).and_then(Slot::constant), Some(&Value::Number(15.0)));
    let live = store.register_producer("live");
    store.publish(live, BTreeMap::from([(Leaf { node: n["g"], path: x }, Value::Number(10.0))]));
    assert_eq!(store.resolved(n["a"], x), Some(Value::Number(5.0)));
    store.apply(tx("rename", vec![Op::Set { id: n["g"], path: path("name.text"), slot: Slot::Const(Value::Str("renamed".into())) }])).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.tree().leaf(n["a"], x).and_then(Slot::constant), Some(&Value::Number(5.0)));
}

/// As above, with each operand in a wrapper group and the wrappers swapped
/// between the parents: the operands are not touched, but their ancestry
/// is, and the cluster is solved again (RE-5.5's watchers look again on a
/// reparent).
#[test]
fn swapping_wrapped_operands_between_parents_is_solved_again() {
    let mut store = Store::new();
    let comp = store.authored().next_id();
    let mut c = facet::Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![named("sheet").wrap(), c.wrap()] }])).unwrap();
    let built = markup::ops(r#"<group id="$g0" x="0" y="0"><group id="$w0"><rect id="$a" x="20" y="0" w="5" h="5"/></group></group><group id="$g1" x="20" y="0"><group id="$w1"><rect id="$b" x="0" y="0" w="5" h="5"/></group></group><constraint id="$c" kind="coincident" value="0"/><constraint id="$fa" kind="fix" value="0"/><constraint id="$fb" kind="fix" value="0"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let n = built.named;
    let r = store.authored().next_relation_ids(4);
    store.apply(tx("tie", built.ops.into_iter().chain([constrains(r[0], n["c"], n["a"], "a"), constrains(r[1], n["c"], n["b"], "b"), constrains(r[2], n["fa"], n["a"], "a"), constrains(r[3], n["fb"], n["b"], "a")]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    store.apply(tx("swap", vec![Op::Reparent { id: n["w0"], parent: Some(n["g1"]), order: OrderKey::first() }, Op::Reparent { id: n["w1"], parent: Some(n["g0"]), order: OrderKey::first() }])).unwrap();
    let problems = constraint_problems(&store);
    assert!(problems.iter().any(|(id, _)| *id == n["c"]), "coincident operands 40 apart, both fixed: {problems:?}");
}

/// A constraint's value read through `deref`, retargeted onto a variable of
/// its own cluster at the same value, is a cycle: reported, and the cluster
/// left unsolved, as a binding reading and feeding one cluster is (DS-7).
#[test]
fn a_deref_retargeted_onto_its_own_clusters_variable_is_a_cycle() {
    let mut store = Store::new();
    let comp = store.authored().next_id();
    let mut c = facet::Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    store.apply(tx("sheet", vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![named("sheet").wrap(), c.wrap()] }])).unwrap();
    let built = markup::ops(r#"<rect id="$a" x="0" y="0" w="5" h="5"/><rect id="$b" x="10" y="0" w="5" h="5"/><rect id="$o" x="10" y="50" w="5" h="5"/><let id="$ref" value="0"/><constraint id="$d" kind="distance" value="{= deref($ref.let.value, 'transform2d.x', 10)}"/><constraint id="$fa" kind="fix" value="0"/>"#, Some(comp), OrderKey::after(&OrderKey::first()), store.authored().next_id()).unwrap();
    let n = built.named;
    let r = store.authored().next_relation_ids(3);
    store.apply(tx("tie", built.ops.into_iter().chain([constrains(r[0], n["d"], n["a"], "a"), constrains(r[1], n["d"], n["b"], "b"), constrains(r[2], n["fa"], n["a"], "a")]).collect())).unwrap_or_else(|e| panic!("{e:?}"));
    store.apply(tx("outside", vec![Op::Set { id: n["ref"], path: path("let.value"), slot: Slot::Const(Value::Ref(n["o"])) }])).unwrap_or_else(|e| panic!("{e:?}"));
    // b's x is 10, as o's is: the value does not move, but what it reads does.
    store.apply(tx("inside", vec![Op::Set { id: n["ref"], path: path("let.value"), slot: Slot::Const(Value::Ref(n["b"])) }])).unwrap();
    let problems = constraint_problems(&store);
    assert!(problems.iter().any(|(id, m)| *id == n["d"] && m.contains("feeds the same cluster")), "the distance reads b's x, a variable of its own cluster: {problems:?}");
}

/// RE-5.5's natives do no work where nothing of theirs changed: an edit
/// nothing constrained reads runs neither the grouping, nor validation, nor
/// any cluster's watcher.
#[test]
fn an_unrelated_edit_runs_no_constraint_native() {
    let (mut store, n) = two_parents();
    let comp = store.tree().live(n["g0"]).and_then(|x| x.parent).unwrap();
    let lone = store.authored().next_id();
    let mut s = facet::Size::default();
    s.w = Slot::Const(Value::Number(5.0));
    s.h = Slot::Const(Value::Number(5.0));
    store.apply(tx("lone", vec![Op::Create { id: lone, ty: ty("rect"), parent: Some(comp), order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), s.wrap()] }])).unwrap();
    let runs = |s: &Store| {
        let r = s.reactive();
        [graph::NativeKey::Clusters].map(|k| r.runs_of(k))
    };
    let cluster = graph::NativeKey::Cluster(store.clusters().list[0].constraints.iter().map(|c| c.id).min().unwrap());
    let (before, cluster_before) = (runs(&store), store.reactive().runs_of(cluster));
    store.apply(tx("move", vec![Op::Set { id: lone, path: path("transform2d.x"), slot: Slot::Const(Value::Number(30.0)) }])).unwrap();
    assert_eq!(runs(&store), before, "grouping (which validates) did not run");
    assert_eq!(store.reactive().runs_of(cluster), cluster_before, "nor the cluster's watcher");
    assert!(before.iter().all(|r| r.is_some()), "the natives exist: {before:?}");
}

/// A formula on a field that decides structure (`constraint.enabled`) is
/// reported on its leaf, not silently read as its default (DS-10a, decided
/// by the user: such fields become bindable in DS-13 and DS-16).
#[test]
fn a_formula_on_a_structural_field_is_reported() {
    let mut p = pendulum();
    p.store.apply(tx("bind", vec![Op::Set { id: p.links[0], path: path("constraint.enabled"), slot: bind("1 > 0") }])).unwrap();
    let problems = p.store.problems();
    assert!(problems.iter().any(|q| q.node == p.links[0] && q.kind == "binding" && q.message.contains("decides structure")), "{problems:?}");
    assert_eq!(p.store.clusters().list.len(), 1, "the constraint still groups, enabled as set");
    // A bound kind is reported once, on its leaf, not again as unknown.
    p.store.apply(tx("kind", vec![Op::Set { id: p.links[1], path: path("constraint.kind"), slot: bind("'distance'") }])).unwrap();
    let problems = p.store.problems();
    assert_eq!(problems.iter().filter(|q| q.node == p.links[1]).count(), 1, "{problems:?}");
}

/// A constraint whose value can't be read (its formula fails, with no value
/// before it) is left out with a problem, not solved as 0 (review of DS-7,
/// Sol).
#[test]
fn an_unreadable_constraint_value_is_a_problem_not_zero() {
    let mut p = pendulum();
    let before = dist(pos(&p.store, p.joints[0]), pos(&p.store, p.joints[1]));
    let applied = p.store.apply(tx("bad", vec![Op::Set { id: p.links[0], path: path("constraint.value"), slot: bind("sin('bad')") }])).unwrap();
    assert!((dist(pos(&p.store, p.joints[0]), pos(&p.store, p.joints[1])) - before).abs() < 1e-6, "the joints did not collapse");
    let problems = constraint_problems(&p.store);
    assert!(problems.iter().any(|(n, m)| *n == p.links[0] && m.contains("can't be read")), "{problems:?}");
    let told = applied.solver.map(|s| s.problems).unwrap_or_default();
    assert!(told.iter().any(|q| q.node == p.links[0]), "the reply says so: {told:?}");
}

/// A constraint value that isn't a finite number (NaN) is a problem, not a
/// solve that "holds" (review of DS-7, Opus: NaN residuals compared false).
#[test]
fn a_nan_constraint_value_is_a_problem() {
    let mut p = pendulum();
    p.store.apply(tx("nan", vec![Op::Set { id: p.links[0], path: path("constraint.value"), slot: bind("sqrt(-1)") }])).unwrap();
    let problems = constraint_problems(&p.store);
    assert!(problems.iter().any(|(n, _)| *n == p.links[0]), "{problems:?}");
}

/// When motion glides a layout child's placement, `g.targets` jumps to the new
/// position while the eased value at `t0` still equals the old drawn position.
/// A cluster reading that child through `Cx::settled` must wake and solve to
/// the new settled target immediately in that transaction, not stay stale at
/// the old position (EN-0 audit).
#[test]
fn a_glided_layout_target_wakes_a_constraint_cluster_at_t0() {
    let mut store = Store::new();
    store.set_motion(true);
    let src = r#"<edit><composition name="scene" dims="2d">
      <group id="$g" name="g" x="0" y="0" size.w="100" size.h="100" layout.mode="stack_x" layout.gap="10">
        <rect id="$a" name="a" w="20" h="20"/>
        <rect id="$b" name="b" w="20" h="20"/>
      </group>
      <rect id="$follower" name="follower" x="30" y="50" w="10" h="10"/>
      <constraint id="$c" kind="distance" value="50"/>
    </composition></edit>"#;
    let plan = iron_document::edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let (g, b, follower, c) = (plan.aliases["$g"], plan.aliases["$b"], plan.aliases["$follower"], plan.aliases["$c"]);
    let rels = store.authored().next_relation_ids(2);
    let link = |id: RelationId, from: NodeId, to: NodeId, role: &str| {
        let mut data = BTreeMap::new();
        data.insert("role".to_owned(), Value::Str(role.into()));
        data.insert("anchor".to_owned(), Value::Vec2([0.0, 0.0]));
        Op::Link { rel: Relation { id, from, to, rel: RelKind::Constrains, data } }
    };
    let mut ops = plan.ops;
    ops.push(link(rels[0], c, b, "a"));
    ops.push(link(rels[1], c, follower, "b"));
    store.apply(tx("setup", ops)).unwrap();
    store.advance_motion(0.0);
    // `$b` is at x=30 (20 + gap 10); `$follower` is at (30, 50), distance 50.
    assert!((pos(&store, b)[0] - 30.0).abs() < 1e-6);
    assert!((dist(pos(&store, b), pos(&store, follower)) - 50.0).abs() < 1e-4);

    // Widen gap to 100: `$b`'s settled target moves to x=120 while its eased
    // drawn position at t0 is still x=30. The cluster must wake and solve
    // `$follower` to distance 50 from settled (120, 0) in this transaction.
    store.apply(tx("widen gap", vec![Op::Set { id: g, path: path("layout.gap"), slot: num(100.0) }])).unwrap();
    let settled_b = [120.0, 0.0];
    let f = pos(&store, follower);
    assert!(
        (dist(settled_b, f) - 50.0).abs() < 1e-3,
        "follower {f:?} must solve to distance 50 from settled {settled_b:?}, not stay at old drawn b={:?}",
        pos(&store, b),
    );
    assert!(constraint_problems(&store).is_empty(), "no constraint problem: {:?}", constraint_problems(&store));
}
