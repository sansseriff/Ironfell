//! Phase 1 exit tests (plans/specs/constraint-system.md §14): a chain reaching a
//! target, an unreachable target, a four-bar linkage through a full crank
//! turn, redundant and conflicting systems, and determinism.

use iron_solve::analysis::analyze;
use iron_solve::cluster::clusters;
use iron_solve::{Options, Problem, Status, Term, Tier, solve};

/// `n` free points chained from a fixed root at `root`, each `link` apart.
/// Variables are `[x0, y0, x1, y1, …]`.
fn chain(n: usize, link: f64, root: [f64; 2], start: &[[f64; 2]]) -> Problem<'static> {
    let vars: Vec<f64> = start.iter().flat_map(|p| [p[0], p[1]]).collect();
    assert_eq!(vars.len(), 2 * n);
    let mut p = Problem::uniform(vars, link);
    for i in 0..n {
        let (bx, by) = (2 * i, 2 * i + 1);
        if i == 0 {
            p.term(Term::new(Tier::Required, vec![bx, by], 1, move |v, r| {
                r[0] = (v[0] - root[0]).hypot(v[1] - root[1]) - link
            }));
        } else {
            let (ax, ay) = (2 * i - 2, 2 * i - 1);
            p.term(Term::new(Tier::Required, vec![ax, ay, bx, by], 1, move |v, r| {
                r[0] = (v[2] - v[0]).hypot(v[3] - v[1]) - link
            }));
        }
    }
    p
}

fn finger(p: &mut Problem<'static>, point: usize, target: [f64; 2]) {
    p.term(Term::new(Tier::Strong, vec![2 * point, 2 * point + 1], 2, move |v, r| {
        r[0] = v[0] - target[0];
        r[1] = v[1] - target[1];
    }));
}

#[test]
fn three_link_chain_reaches_a_target() {
    let mut p = chain(3, 100.0, [0.0, 0.0], &[[100.0, 0.0], [200.0, 0.0], [300.0, 0.0]]);
    finger(&mut p, 2, [150.0, 150.0]);
    let s = solve(&p, &Options::default());
    assert_eq!(s.status, Status::Converged, "{s:?}");
    assert!(s.max_required_residual < 1e-6, "required {}", s.max_required_residual);
    // The strong terms hold to the weak rows' pull (weights 1 : 1e-2), far
    // inside a pixel; the required terms hold to tolerance exactly.
    assert!(s.max_strong_residual < 1e-4, "the finger is reached: {}", s.max_strong_residual);
    assert!((s.vars[4] - 150.0).abs() < 1e-4 && (s.vars[5] - 150.0).abs() < 1e-4, "{:?}", s.vars);
    assert!(s.iterations < 50, "{} iterations", s.iterations);
}

#[test]
fn unreachable_target_stops_at_full_extension() {
    let mut p = chain(3, 100.0, [0.0, 0.0], &[[100.0, 0.0], [200.0, 0.0], [300.0, 0.0]]);
    // 400 px away along the diagonal; the chain reaches 300.
    let target = [400.0 / 2f64.sqrt(), 400.0 / 2f64.sqrt()];
    finger(&mut p, 2, target);
    let s = solve(&p, &Options::default());
    assert_eq!(s.status, Status::Converged, "{s:?}");
    assert!(s.max_required_residual < 1e-6, "required {}", s.max_required_residual);
    // The chain is straight along the diagonal; the gap is 100 px, split
    // evenly over x and y.
    let gap = 100.0 / 2f64.sqrt();
    assert!((s.max_strong_residual - gap).abs() < 1e-4, "strong {} vs gap {gap}", s.max_strong_residual);
    let end = (s.vars[4], s.vars[5]);
    assert!((end.0.hypot(end.1) - 300.0).abs() < 1e-6, "full extension: {end:?}");
    assert!((end.0 - end.1).abs() < 1e-3, "pointing at the target: {end:?}");
}

/// Grashof crank-rocker: crank r=1 at A=(0,0), coupler 4, rocker 3 at
/// D=(4,0). The crank turns fully; the rocker swings. The coupler joint C is
/// the one free point, solved each step from the previous pose.
#[test]
fn four_bar_linkage_turns_a_full_crank_without_flipping() {
    let (r, coupler, rocker, d) = (1.0, 4.0, 3.0, 4.0);
    let mut c = [3.0, 2.5]; // a plausible starting pose for φ = 0 (B = (1, 0))
    let mut prev_angle: Option<f64> = None;
    let mut max_jump = 0.0f64;
    let steps = 360;
    for k in 0..=steps {
        let phi = k as f64 * std::f64::consts::TAU / steps as f64;
        let b = [r * phi.cos(), r * phi.sin()];
        let mut p = Problem::uniform(vec![c[0], c[1]], 1.0);
        p.term(Term::new(Tier::Required, vec![0, 1], 1, move |v, out| out[0] = (v[0] - b[0]).hypot(v[1] - b[1]) - coupler));
        p.term(Term::new(Tier::Required, vec![0, 1], 1, move |v, out| out[0] = (v[0] - d).hypot(v[1]) - rocker));
        let s = solve(&p, &Options::default());
        assert_eq!(s.status, Status::Converged, "step {k}: {s:?}");
        assert!(s.max_required_residual < 1e-6, "step {k}: {}", s.max_required_residual);
        c = [s.vars[0], s.vars[1]];
        let angle = (c[1]).atan2(c[0] - d);
        if let Some(prev) = prev_angle {
            let mut jump = (angle - prev).abs();
            if jump > std::f64::consts::PI {
                jump = std::f64::consts::TAU - jump;
            }
            max_jump = max_jump.max(jump);
            assert!(jump < std::f64::consts::FRAC_PI_2, "step {k}: rocker jumped {jump} rad");
        }
        prev_angle = Some(angle);
        assert!(c[1] > 0.0, "step {k}: the rocker stayed on its branch: {c:?}");
    }
    assert!(max_jump < 0.1, "largest per-step rocker motion {max_jump} rad");
}

#[test]
fn redundant_and_conflicting_systems_name_the_culprit() {
    // One free point at distance 1 from the origin, said twice: redundant.
    let mut p = Problem::uniform(vec![0.8, 0.5], 1.0);
    p.term(Term::new(Tier::Required, vec![0, 1], 1, |v, r| r[0] = v[0].hypot(v[1]) - 1.0));
    p.term(Term::new(Tier::Required, vec![0, 1], 1, |v, r| r[0] = v[0].hypot(v[1]) - 1.0));
    let s = solve(&p, &Options::default());
    assert_eq!(s.status, Status::Converged);
    let a = analyze(&p, &s.vars, 1e-9);
    assert_eq!((a.rank, a.dof), (1, 1), "{a:?}");
    assert_eq!(a.redundant, vec![0, 1], "{a:?}");
    assert!(a.conflicting.is_empty(), "{a:?}");

    // Distance 1 and distance 2 from the same origin: conflicting, and the
    // solve cannot satisfy both.
    let mut p = Problem::uniform(vec![0.8, 0.5], 1.0);
    p.term(Term::new(Tier::Required, vec![0, 1], 1, |v, r| r[0] = v[0].hypot(v[1]) - 1.0));
    p.term(Term::new(Tier::Required, vec![0, 1], 1, |v, r| r[0] = v[0].hypot(v[1]) - 2.0));
    let s = solve(&p, &Options::default());
    assert_eq!(s.status, Status::NotConverged, "{s:?}");
    let a = analyze(&p, &s.vars, 1e-6);
    assert_eq!(a.conflicting, vec![0, 1], "{a:?}");
    assert!(a.redundant.is_empty(), "{a:?}");

    // A well-posed chain with one duplicated link constraint: only the
    // duplicate pair is flagged, as redundant; the finger still lands.
    let mut p = chain(2, 10.0, [0.0, 0.0], &[[10.0, 0.0], [20.0, 0.0]]);
    p.term(Term::new(Tier::Required, vec![0, 1], 1, |v, r| r[0] = v[0].hypot(v[1]) - 10.0));
    finger(&mut p, 1, [5.0, 15.0]);
    let s = solve(&p, &Options::default());
    assert_eq!(s.status, Status::Converged, "{s:?}");
    assert!(s.max_strong_residual < 1e-4, "{s:?}");
    let a = analyze(&p, &s.vars, 1e-9);
    assert_eq!((a.rank, a.dof), (2, 2), "{a:?}");
    assert_eq!(a.redundant, vec![0, 2], "{a:?}");
    assert!(a.conflicting.is_empty());

    // Two pins on one point at different places: conflicting, culprits named.
    let mut p = Problem::uniform(vec![0.0, 0.0], 1.0);
    p.term(Term::new(Tier::Required, vec![0, 1], 2, |v, r| {
        r[0] = v[0];
        r[1] = v[1];
    }));
    p.term(Term::new(Tier::Required, vec![0, 1], 2, |v, r| {
        r[0] = v[0] - 1.0;
        r[1] = v[1];
    }));
    let s = solve(&p, &Options::default());
    let a = analyze(&p, &s.vars, 1e-6);
    assert_eq!(a.conflicting, vec![0, 1], "{a:?}");
    assert_eq!(a.dof, 0);
}

#[test]
fn clusters_split_independent_mechanisms() {
    let mut p = chain(2, 10.0, [0.0, 0.0], &[[10.0, 0.0], [20.0, 0.0]]);
    // A second, unrelated chain further along the variable vector.
    p.vars.extend([5.0, 5.0, 8.0, 8.0]);
    p.scale.extend([10.0; 4]);
    p.term(Term::new(Tier::Required, vec![4, 5], 1, |v, r| r[0] = v[0].hypot(v[1]) - 10.0));
    p.term(Term::new(Tier::Required, vec![4, 5, 6, 7], 1, |v, r| r[0] = (v[2] - v[0]).hypot(v[3] - v[1]) - 10.0));
    assert_eq!(clusters(&p.terms), vec![vec![0, 1], vec![2, 3]]);
}

#[test]
fn results_are_bit_identical_across_runs() {
    let build = || {
        let mut p = chain(4, 80.0, [10.0, 20.0], &[[90.0, 20.0], [170.0, 20.0], [250.0, 20.0], [330.0, 20.0]]);
        finger(&mut p, 3, [120.0, 260.0]);
        p
    };
    let a = solve(&build(), &Options::default());
    let b = solve(&build(), &Options::default());
    assert_eq!(a.status, Status::Converged);
    assert_eq!(a.iterations, b.iterations);
    assert_eq!(a.evaluations, b.evaluations);
    let bits = |s: &iron_solve::Solution| s.vars.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&a), bits(&b));
    // And the benchmark problem too.
    let x = iron_solve::bench::run(20, 1);
    let y = iron_solve::bench::run(20, 3);
    assert_eq!(bits(&x), bits(&y));
    assert!(x.max_required_residual < 1e-6, "{x:?}");
}

#[test]
fn step_limit_and_least_change_are_honoured() {
    // Without least change and without the finger, a single required term
    // leaves the solution where it starts if it already holds.
    let mut p = Problem::uniform(vec![1.0, 0.0], 1.0);
    p.term(Term::new(Tier::Required, vec![0, 1], 1, |v, r| r[0] = v[0].hypot(v[1]) - 1.0));
    let s = solve(&p, &Options::default());
    assert_eq!(s.vars, vec![1.0, 0.0]);
    // A finger far away with a tight step limit: the first iteration moves
    // at most the limit (in scaled units, scale 1 here).
    finger(&mut p, 0, [-1.0, 0.0]);
    let s = solve(&p, &Options { max_iterations: 1, step_limit: Some(0.1), project: true, ..Options::default() });
    let moved = (s.vars[0] - 1.0).hypot(s.vars[1]);
    assert!(moved <= 0.11, "moved {moved}: the step is bounded, plus the projection back onto the circle");
}
