//! The phase 1 benchmark (constraint-system.md §14): a chain of joints with
//! fixed link lengths and a finger on the last one, solved at a fixed
//! iteration count. Timing is the caller's: natively from `examples/bench.rs`,
//! in wasm from the app's `solve_bench` FFI, so one problem is measured on
//! both targets.

use crate::{Options, Problem, Solution, Term, Tier, solve};

/// A planar chain: `joints` free points (2 variables each) hanging off a
/// fixed root at the origin, consecutive points `link` apart, with a strong
/// finger pulling the last point to `target`. `joints = 6` is the 12-variable
/// cluster the plan names.
pub fn chain(joints: usize, link: f64, target: [f64; 2]) -> Problem<'static> {
    let mut vars = Vec::with_capacity(joints * 2);
    for i in 0..joints {
        // Start laid out along +x, slightly bent so the Jacobian is not
        // degenerate at the first step.
        vars.push(link * (i + 1) as f64 * 0.95);
        vars.push(0.1 * link * (i + 1) as f64);
    }
    let mut p = Problem::uniform(vars, link);
    for i in 0..joints {
        let (bx, by) = (2 * i, 2 * i + 1);
        if i == 0 {
            p.term(Term::new(Tier::Required, vec![bx, by], 1, move |v, r| r[0] = v[0].hypot(v[1]) - link));
        } else {
            let (ax, ay) = (2 * i - 2, 2 * i - 1);
            p.term(Term::new(Tier::Required, vec![ax, ay, bx, by], 1, move |v, r| {
                r[0] = (v[2] - v[0]).hypot(v[3] - v[1]) - link
            }));
        }
    }
    let (ex, ey) = (2 * joints - 2, 2 * joints - 1);
    p.term(Term::new(Tier::Strong, vec![ex, ey], 2, move |v, r| {
        r[0] = v[0] - target[0];
        r[1] = v[1] - target[1];
    }));
    p
}

/// Solve the 12-variable chain `repeats` times at exactly `iterations`
/// iterations at most (it converges in fewer and the solution says how many),
/// returning the last solution. Divide the caller's elapsed time by
/// `repeats` for the per-solve cost.
pub fn run(iterations: u32, repeats: u32) -> Solution {
    let p = chain(6, 50.0, [120.0, 180.0]);
    let opts = Options { max_iterations: iterations, ..Options::default() };
    let mut last = None;
    for _ in 0..repeats {
        last = Some(solve(&p, &opts));
    }
    last.expect("at least one repeat")
}
