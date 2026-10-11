//! `iron_solve`: a small dense constraint solver (plans/constraint-system.md §5).
//!
//! A [`Problem`] is a vector of variables, a characteristic scale per
//! variable, and residual [`Term`]s in three [`Tier`]s. The solver minimises
//! the weighted stacked residual with projected Levenberg–Marquardt steps,
//! warm-started from the given values:
//!
//! - **Required** terms are the constraints. They must hold after a
//!   successful solve. Every trial step is projected back onto them (Newton
//!   on the required rows, minimum-norm) before it is judged, so the
//!   iteration walks along the constraint manifold rather than fighting a
//!   penalty weight.
//! - **Strong** terms are drivers (a finger on a dragged point, a value a
//!   transaction asked for). Satisfied as far as the required terms allow:
//!   an out-of-reach finger leaves a strong residual equal to the gap.
//! - **Weak** terms express preference. The solver adds one implicit weak
//!   row per variable, "stay where you were", which is what keeps motion
//!   continuous and stops elbows flipping between solutions. Its weight is
//!   small enough that a finger lands within a thousandth of a pixel.
//!
//! Variables are column-scaled by their characteristic size so a radian and
//! a pixel are comparable. Jacobians are central finite differences per
//! term, which only touch that term's variables. Everything iterates in
//! index order with no hashing, so the same input gives bit-identical output.
//!
//! The crate knows nothing about documents, nodes, or constraint kinds; the
//! adapter in `iron_document` maps leaves to variables and kinds to residual
//! closures. That separation (Inkling's lesson: reduction is separable from
//! the numerical method) is what lets the solver be swapped.

pub mod analysis;
pub mod bench;
pub mod cluster;
mod linalg;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tier {
    Required,
    Strong,
    Weak,
}

impl Tier {
    fn index(self) -> usize {
        match self {
            Tier::Required => 0,
            Tier::Strong => 1,
            Tier::Weak => 2,
        }
    }
}

/// A block of residual equations over a few variables. `residual` receives
/// the current values of `vars`, in order, and writes `len` outputs that are
/// zero when the term is satisfied.
pub struct Term<'a> {
    pub tier: Tier,
    pub vars: Vec<usize>,
    pub len: usize,
    pub residual: Box<dyn Fn(&[f64], &mut [f64]) + 'a>,
}

impl<'a> Term<'a> {
    pub fn new(tier: Tier, vars: impl Into<Vec<usize>>, len: usize, residual: impl Fn(&[f64], &mut [f64]) + 'a) -> Term<'a> {
        Term { tier, vars: vars.into(), len, residual: Box::new(residual) }
    }

    /// Evaluate against the full variable vector.
    pub fn eval(&self, x: &[f64], out: &mut [f64]) {
        let vals: Vec<f64> = self.vars.iter().map(|&v| x[v]).collect();
        (self.residual)(&vals, out);
    }
}

pub struct Problem<'a> {
    /// Initial values: the warm start, and what the weak rows pull toward.
    pub vars: Vec<f64>,
    /// Characteristic size per variable (pixels for a position, the node's
    /// half-diagonal for a rotation). Steps and preferences are measured in
    /// these units.
    pub scale: Vec<f64>,
    pub terms: Vec<Term<'a>>,
}

impl<'a> Problem<'a> {
    pub fn new(vars: Vec<f64>, scale: Vec<f64>) -> Problem<'a> {
        assert_eq!(vars.len(), scale.len(), "one scale per variable");
        Problem { vars, scale, terms: Vec::new() }
    }

    /// Every variable at the same scale.
    pub fn uniform(vars: Vec<f64>, scale: f64) -> Problem<'a> {
        let n = vars.len();
        Problem::new(vars, vec![scale; n])
    }

    pub fn term(&mut self, term: Term<'a>) -> &mut Self {
        for &v in &term.vars {
            assert!(v < self.vars.len(), "term references variable {v} of {}", self.vars.len());
        }
        self.terms.push(term);
        self
    }

    /// Raw (unweighted) residuals of every term in `tier`, in term order.
    pub fn residuals(&self, x: &[f64], tier: Tier) -> Vec<f64> {
        let mut out = Vec::new();
        let mut buf = Vec::new();
        for t in self.terms.iter().filter(|t| t.tier == tier) {
            buf.clear();
            buf.resize(t.len, 0.0);
            t.eval(x, &mut buf);
            out.extend_from_slice(&buf);
        }
        out
    }

    /// Largest absolute raw residual in `tier`; zero when the tier is empty.
    pub fn max_residual(&self, x: &[f64], tier: Tier) -> f64 {
        self.residuals(x, tier).iter().fold(0.0, |m, r| m.max(r.abs()))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub max_iterations: u32,
    /// A required residual at or under this is "holds".
    pub tolerance: f64,
    /// Row weights for required, strong, and weak terms.
    pub weights: [f64; 3],
    /// Add one weak "stay put" row per variable, in scaled units.
    pub least_change: bool,
    /// Largest step per iteration, in scaled units (`None`: unbounded). A
    /// bound keeps a fast finger from jumping the solution to another
    /// branch; the caller sub-steps the driver instead.
    pub step_limit: Option<f64>,
    /// Project every trial step (and the warm start) onto the required rows
    /// before judging it. Off, the required terms are only weighted.
    pub project: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            max_iterations: 50,
            tolerance: 1e-9,
            weights: [1e2, 1.0, 1e-3],
            least_change: true,
            step_limit: None,
            project: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Every required residual is within tolerance.
    Converged,
    /// The iteration cap was reached, or the required rows cannot all hold
    /// (a conflict; see `analysis`). The variables are the best pose found;
    /// a caller animating a drag should keep the previous pose instead
    /// (§5.5).
    NotConverged,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub vars: Vec<f64>,
    pub status: Status,
    /// Outer iterations taken.
    pub iterations: u32,
    /// Residual evaluations, as a cost measure that does not depend on timing.
    pub evaluations: u64,
    pub max_required_residual: f64,
    pub max_strong_residual: f64,
}

/// Rows of the stacked residual: which term (or which variable, for the
/// implicit least-change rows) and its weight.
struct Layout {
    /// (term index, first row, weight) per term.
    terms: Vec<(usize, usize, f64)>,
    /// First row of the least-change block, if any.
    least_change: Option<(usize, f64)>,
    rows: usize,
    /// (term index, first row) of the required terms within their own block.
    required: Vec<(usize, usize)>,
    required_rows: usize,
}

fn layout(p: &Problem, opts: &Options) -> Layout {
    let mut terms = Vec::with_capacity(p.terms.len());
    let mut rows = 0;
    for (i, t) in p.terms.iter().enumerate() {
        terms.push((i, rows, opts.weights[t.tier.index()]));
        rows += t.len;
    }
    let least_change = opts.least_change.then(|| {
        let start = rows;
        rows += p.vars.len();
        (start, opts.weights[Tier::Weak.index()])
    });
    let mut required = Vec::new();
    let mut required_rows = 0;
    for (i, t) in p.terms.iter().enumerate() {
        if t.tier == Tier::Required {
            required.push((i, required_rows));
            required_rows += t.len;
        }
    }
    Layout { terms, least_change, rows, required, required_rows }
}

/// The solver's working state: scaled variables `z` (x = z · scale).
struct Work<'p, 'a> {
    p: &'p Problem<'a>,
    layout: Layout,
    z0: Vec<f64>,
    evaluations: u64,
    buf: Vec<f64>,
    plus: Vec<f64>,
    minus: Vec<f64>,
}

impl Work<'_, '_> {
    fn unscale(&self, z: &[f64]) -> Vec<f64> {
        z.iter().zip(&self.p.scale).map(|(z, s)| z * s).collect()
    }

    /// Weighted stacked residual at `z`.
    fn residual(&mut self, z: &[f64], r: &mut [f64]) {
        let x = self.unscale(z);
        for &(i, row, w) in &self.layout.terms {
            let t = &self.p.terms[i];
            self.buf.clear();
            self.buf.resize(t.len, 0.0);
            t.eval(&x, &mut self.buf);
            for (k, v) in self.buf.iter().enumerate() {
                r[row + k] = w * v;
            }
            self.evaluations += 1;
        }
        if let Some((row, w)) = self.layout.least_change {
            for (j, (zj, z0)) in z.iter().zip(&self.z0).enumerate() {
                r[row + j] = w * (zj - z0);
            }
        }
    }

    /// One term's rows of the Jacobian at `x` (scaled columns), by central
    /// differences over the term's own variables, scaled by `w`, written
    /// into `jac` (with `n` columns) from `row`.
    fn term_jacobian(&mut self, i: usize, x: &mut [f64], z: &[f64], w: f64, row: usize, n: usize, jac: &mut [f64]) {
        let t = &self.p.terms[i];
        self.plus.clear();
        self.plus.resize(t.len, 0.0);
        self.minus.clear();
        self.minus.resize(t.len, 0.0);
        for &j in &t.vars {
            let h = 1e-6 * z[j].abs().max(1.0);
            let dx = h * self.p.scale[j];
            let xj = x[j];
            x[j] = xj + dx;
            t.eval(x, &mut self.plus);
            x[j] = xj - dx;
            t.eval(x, &mut self.minus);
            x[j] = xj;
            self.evaluations += 2;
            for k in 0..t.len {
                jac[(row + k) * n + j] += w * (self.plus[k] - self.minus[k]) / (2.0 * h);
            }
        }
    }

    /// Weighted Jacobian at `z` (rows × n, row-major); the least-change
    /// block is diagonal.
    fn jacobian(&mut self, z: &[f64], jac: &mut [f64]) {
        let n = z.len();
        jac.iter_mut().for_each(|v| *v = 0.0);
        let mut x = self.unscale(z);
        let terms: Vec<(usize, usize, f64)> = self.layout.terms.clone();
        for (i, row, w) in terms {
            self.term_jacobian(i, &mut x, z, w, row, n, jac);
        }
        if let Some((row, w)) = self.layout.least_change {
            for j in 0..n {
                jac[(row + j) * n + j] = w;
            }
        }
    }

    /// Unweighted required residual and Jacobian at `z`.
    fn required_system(&mut self, z: &[f64], r: &mut [f64], jac: &mut [f64]) {
        let n = z.len();
        jac.iter_mut().for_each(|v| *v = 0.0);
        let mut x = self.unscale(z);
        let required: Vec<(usize, usize)> = self.layout.required.clone();
        for (i, row) in required {
            let t = &self.p.terms[i];
            self.buf.clear();
            self.buf.resize(t.len, 0.0);
            t.eval(&x, &mut self.buf);
            r[row..row + t.len].copy_from_slice(&self.buf);
            self.evaluations += 1;
            self.term_jacobian(i, &mut x, z, 1.0, row, n, jac);
        }
    }

    /// Newton on the required rows alone, minimum-norm steps, until they
    /// hold to a tenth of the tolerance or the attempts run out. Returns
    /// the largest remaining residual.
    fn project(&mut self, z: &mut [f64], tolerance: f64) -> f64 {
        let n = z.len();
        let m = self.layout.required_rows;
        if m == 0 || n == 0 {
            return 0.0;
        }
        let mut r = vec![0.0; m];
        let mut jac = vec![0.0; m * n];
        let mut a = vec![0.0; n * n];
        let mut g = vec![0.0; n];
        for _ in 0..10 {
            self.required_system(z, &mut r, &mut jac);
            let worst = r.iter().fold(0.0f64, |mx, v| mx.max(v.abs()));
            if worst <= tolerance * 0.1 {
                return worst;
            }
            linalg::normal_equations(&jac, &r, m, n, &mut a, &mut g);
            // Tiny Tikhonov term: with fewer rows than columns this picks the
            // minimum-norm step, and it keeps a redundant system solvable.
            let scale = a.iter().fold(0.0f64, |mx, v| mx.max(v.abs())).max(1e-300);
            for j in 0..n {
                a[j * n + j] += 1e-12 * scale;
            }
            let rhs: Vec<f64> = g.iter().map(|v| -v).collect();
            let Some(delta) = linalg::cholesky_solve(&a, &rhs, n) else { return worst };
            for (zj, d) in z.iter_mut().zip(&delta) {
                *zj += d;
            }
        }
        self.required_system(z, &mut r, &mut jac);
        r.iter().fold(0.0f64, |mx, v| mx.max(v.abs()))
    }
}

/// Solve `problem` with `opts`. Deterministic: the same problem and options
/// give bit-identical results.
pub fn solve(problem: &Problem, opts: &Options) -> Solution {
    let n = problem.vars.len();
    let z0: Vec<f64> = problem.vars.iter().zip(&problem.scale).map(|(x, s)| x / s).collect();
    let mut work = Work {
        p: problem,
        layout: layout(problem, opts),
        z0: z0.clone(),
        evaluations: 0,
        buf: Vec::new(),
        plus: Vec::new(),
        minus: Vec::new(),
    };
    let m = work.layout.rows;
    let mut z = z0;
    let mut iterations = 0u32;

    if n > 0 && m > 0 {
        if opts.project {
            work.project(&mut z, opts.tolerance);
        }
        let mut r = vec![0.0; m];
        let mut jac = vec![0.0; m * n];
        work.residual(&z, &mut r);
        let mut cost = 0.5 * r.iter().map(|v| v * v).sum::<f64>();
        // Damping is λ·I in the scaled space, not Marquardt's λ·diag(JᵀJ):
        // the diagonal is dominated by the required rows, and damping in
        // proportion to it would shrink every step along the constraint
        // manifold, which is where the strong terms get satisfied. Columns
        // are already comparable through `scale`, so λ is in units of
        // weight²; it starts near the weak weight's and grows on failure.
        let mut lambda = 1e-4;
        let mut a = vec![0.0; n * n];
        let mut g = vec![0.0; n];
        let mut r_try = vec![0.0; m];
        let mut done = false;
        while !done && iterations < opts.max_iterations {
            iterations += 1;
            work.jacobian(&z, &mut jac);
            linalg::normal_equations(&jac, &r, m, n, &mut a, &mut g);
            if g.iter().all(|v| v.abs() < 1e-15) {
                break;
            }
            let mut accepted = false;
            for _ in 0..16 {
                let mut damped = a.clone();
                for j in 0..n {
                    damped[j * n + j] += lambda;
                }
                let rhs: Vec<f64> = g.iter().map(|v| -v).collect();
                let Some(mut delta) = linalg::cholesky_solve(&damped, &rhs, n) else {
                    lambda *= 10.0;
                    continue;
                };
                if let Some(limit) = opts.step_limit {
                    let norm = delta.iter().map(|d| d * d).sum::<f64>().sqrt();
                    if norm > limit {
                        let k = limit / norm;
                        delta.iter_mut().for_each(|d| *d *= k);
                    }
                }
                let mut z_try: Vec<f64> = z.iter().zip(&delta).map(|(z, d)| z + d).collect();
                if opts.project {
                    work.project(&mut z_try, opts.tolerance);
                }
                work.residual(&z_try, &mut r_try);
                let cost_try = 0.5 * r_try.iter().map(|v| v * v).sum::<f64>();
                if cost_try < cost {
                    let step = z_try.iter().zip(&z).fold(0.0f64, |mx, (a, b)| mx.max((a - b).abs()));
                    let drop = cost - cost_try;
                    z = z_try;
                    r.copy_from_slice(&r_try);
                    cost = cost_try;
                    lambda = (lambda / 3.0).max(1e-12);
                    accepted = true;
                    // Converged: the pose stopped moving (a nanometre of a
                    // scale unit) or the cost stopped falling.
                    done = step < 1e-9 || drop <= 1e-12 * cost.max(1e-300);
                    break;
                }
                lambda *= 10.0;
            }
            if !accepted {
                break;
            }
        }
    }

    let vars = work.unscale(&z);
    let max_required_residual = problem.max_residual(&vars, Tier::Required);
    let max_strong_residual = problem.max_residual(&vars, Tier::Strong);
    let status = if max_required_residual <= opts.tolerance { Status::Converged } else { Status::NotConverged };
    Solution { vars, status, iterations, evaluations: work.evaluations, max_required_residual, max_strong_residual }
}
