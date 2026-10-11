//! Degrees of freedom and conflicts (constraint-system.md §5.6). Run on
//! structural change, not per frame: it computes the required Jacobian's
//! rank once per required term.

use crate::{Problem, Tier, linalg};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Analysis {
    /// Rank of the required rows' Jacobian at the given pose.
    pub rank: usize,
    /// Variables minus rank: how many ways the pose can still move.
    pub dof: usize,
    /// Required terms whose rows depend on the others and that hold at the
    /// pose: harmless duplicates. SolveSpace's "drop one and re-test rank".
    pub redundant: Vec<usize>,
    /// Required terms whose rows depend on the others and that do *not*
    /// hold at the pose: the culprits of an unsatisfiable system. Every
    /// member of a mutually conflicting set is listed, since removing any
    /// one of them would resolve it.
    pub conflicting: Vec<usize>,
}

/// Analyse `problem` at pose `x` (usually a solution). `tol` is the residual
/// under which a dependent term counts as holding.
pub fn analyze(problem: &Problem, x: &[f64], tol: f64) -> Analysis {
    let n = x.len();
    let req: Vec<usize> = (0..problem.terms.len()).filter(|&i| problem.terms[i].tier == Tier::Required).collect();
    let rows: usize = req.iter().map(|&i| problem.terms[i].len).sum();
    if rows == 0 || n == 0 {
        return Analysis { rank: 0, dof: n, redundant: vec![], conflicting: vec![] };
    }
    // Unweighted required Jacobian in scaled columns, central differences.
    let mut jac = vec![0.0; rows * n];
    let mut row_of = vec![0usize; req.len()];
    let mut row = 0;
    let mut xp = x.to_vec();
    for (k, &i) in req.iter().enumerate() {
        let t = &problem.terms[i];
        row_of[k] = row;
        let mut plus = vec![0.0; t.len];
        let mut minus = vec![0.0; t.len];
        for &j in &t.vars {
            let h = 1e-6 * (x[j] / problem.scale[j]).abs().max(1.0);
            let dx = h * problem.scale[j];
            xp[j] = x[j] + dx;
            t.eval(&xp, &mut plus);
            xp[j] = x[j] - dx;
            t.eval(&xp, &mut minus);
            xp[j] = x[j];
            for l in 0..t.len {
                jac[(row + l) * n + j] += (plus[l] - minus[l]) / (2.0 * h);
            }
        }
        row += t.len;
    }
    const RANK_TOL: f64 = 1e-9;
    let rank = linalg::rank(&jac, rows, n, RANK_TOL);
    let mut redundant = Vec::new();
    let mut conflicting = Vec::new();
    if rank < rows {
        for (k, &i) in req.iter().enumerate() {
            let t = &problem.terms[i];
            let without: Vec<f64> = jac
                .chunks(n)
                .enumerate()
                .filter(|(r, _)| *r < row_of[k] || *r >= row_of[k] + t.len)
                .flat_map(|(_, r)| r.iter().copied())
                .collect();
            if linalg::rank(&without, rows - t.len, n, RANK_TOL) == rank {
                let mut r = vec![0.0; t.len];
                t.eval(x, &mut r);
                if r.iter().all(|v| v.abs() <= tol) { redundant.push(i) } else { conflicting.push(i) }
            }
        }
    }
    Analysis { rank, dof: n - rank, redundant, conflicting }
}
