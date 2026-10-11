//! Dense `f64` helpers, row-major. Clusters are tiny (tens of variables),
//! so hand-rolled O(n³) routines beat a dependency (constraint-system.md
//! §5.4, open question I4 for the day they do not).

/// `a = JᵀJ` (n×n) and `g = Jᵀr` (n) for a row-major `jac` of `m` rows.
pub fn normal_equations(jac: &[f64], r: &[f64], m: usize, n: usize, a: &mut [f64], g: &mut [f64]) {
    a.iter_mut().for_each(|v| *v = 0.0);
    g.iter_mut().for_each(|v| *v = 0.0);
    for i in 0..m {
        let row = &jac[i * n..(i + 1) * n];
        for j in 0..n {
            let rj = row[j];
            if rj == 0.0 {
                continue;
            }
            g[j] += rj * r[i];
            for k in j..n {
                a[j * n + k] += rj * row[k];
            }
        }
    }
    for j in 0..n {
        for k in 0..j {
            a[j * n + k] = a[k * n + j];
        }
    }
}

/// Solve `a x = b` for symmetric positive-definite `a` by Cholesky. `None`
/// when `a` is not positive definite (a zero or negative pivot).
pub fn cholesky_solve(a: &[f64], b: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut l = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..=i {
            let mut s = a[i * n + j];
            for k in 0..j {
                s -= l[i * n + k] * l[j * n + k];
            }
            if i == j {
                if s <= 0.0 || !s.is_finite() {
                    return None;
                }
                l[i * n + i] = s.sqrt();
            } else {
                l[i * n + j] = s / l[j * n + j];
            }
        }
    }
    // Forward: L y = b.
    let mut y = vec![0.0; n];
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= l[i * n + k] * y[k];
        }
        y[i] = s / l[i * n + i];
    }
    // Back: Lᵀ x = y.
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = y[i];
        for k in i + 1..n {
            s -= l[k * n + i] * x[k];
        }
        x[i] = s / l[i * n + i];
    }
    Some(x)
}

/// Numerical rank of a row-major `m`×`n` matrix by Gaussian elimination with
/// full pivoting; a pivot under `tol` times the largest entry counts as zero.
pub fn rank(mat: &[f64], m: usize, n: usize, tol: f64) -> usize {
    let mut a = mat.to_vec();
    let largest = a.iter().fold(0.0f64, |mx, v| mx.max(v.abs()));
    if largest == 0.0 {
        return 0;
    }
    let threshold = tol * largest;
    let mut rank = 0;
    let mut used_rows = vec![false; m];
    let mut used_cols = vec![false; n];
    loop {
        // Largest remaining entry.
        let mut best = (0.0f64, usize::MAX, usize::MAX);
        for i in (0..m).filter(|&i| !used_rows[i]) {
            for j in (0..n).filter(|&j| !used_cols[j]) {
                let v = a[i * n + j].abs();
                if v > best.0 {
                    best = (v, i, j);
                }
            }
        }
        let (v, pi, pj) = best;
        if v <= threshold {
            return rank;
        }
        rank += 1;
        used_rows[pi] = true;
        used_cols[pj] = true;
        let pivot = a[pi * n + pj];
        for i in (0..m).filter(|&i| !used_rows[i]) {
            let f = a[i * n + pj] / pivot;
            if f == 0.0 {
                continue;
            }
            for j in 0..n {
                a[i * n + j] -= f * a[pi * n + j];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cholesky_and_rank() {
        // [4 2; 2 3] x = [2; 1] -> x = [0.5, 0].
        let a = [4.0, 2.0, 2.0, 3.0];
        let x = cholesky_solve(&a, &[2.0, 1.0], 2).unwrap();
        assert!((x[0] - 0.5).abs() < 1e-12 && x[1].abs() < 1e-12, "{x:?}");
        assert!(cholesky_solve(&[1.0, 2.0, 2.0, 1.0], &[1.0, 1.0], 2).is_none(), "indefinite");
        assert_eq!(rank(&[1.0, 2.0, 2.0, 4.0], 2, 2, 1e-12), 1);
        assert_eq!(rank(&[1.0, 0.0, 0.0, 1.0, 1.0, 1.0], 3, 2, 1e-12), 2);
        assert_eq!(rank(&[0.0; 4], 2, 2, 1e-12), 0);
        let mut a = vec![0.0; 4];
        let mut g = vec![0.0; 2];
        normal_equations(&[1.0, 2.0, 3.0, 4.0], &[1.0, 1.0], 2, 2, &mut a, &mut g);
        assert_eq!(a, vec![10.0, 14.0, 14.0, 20.0]);
        assert_eq!(g, vec![4.0, 6.0]);
    }
}
