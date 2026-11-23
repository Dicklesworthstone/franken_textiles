//! Small dense linear algebra.
//!
//! [`lstsq`] is the minimum-norm least-squares solve the reference gets from numpy's
//! `linalg.lstsq(A, b, rcond=None)`, done with a one-sided Jacobi SVD (Hestenes): rotations
//! orthogonalise the columns of A in a fixed sweep order, so the result is deterministic and as
//! accurate as LAPACK's SVD for the tall, thin systems used here (a quadratic fit has 3 columns).

/// Maximum number of Jacobi sweeps; small systems converge in fewer than ten.
const MAX_SWEEPS: usize = 64;

/// Minimum-norm least-squares solution `x` of `A x ≈ b`, with `A` given as rows.
///
/// Singular values at or below `ε · max(m, N) · σ_max` count as zero (numpy's `rcond=None`), so a
/// rank-deficient system gets the minimum-norm solution, like numpy. An empty system returns zeros.
/// `b` must have one entry per row.
///
/// # Panics
/// If `b.len() != a.len()`.
#[must_use]
pub fn lstsq<const N: usize>(a: &[[f64; N]], b: &[f64]) -> [f64; N] {
    assert_eq!(a.len(), b.len(), "lstsq: one right-hand side per row");
    let m = a.len();
    if m == 0 || N == 0 {
        return [0.0; N];
    }
    if a.iter().flatten().any(|v| !v.is_finite()) || b.iter().any(|v| !v.is_finite()) {
        return [f64::NAN; N];
    }
    // Scale extreme-magnitude systems so squared column norms and `alpha * beta` neither overflow
    // to infinity (silently skipping Jacobi rotations) nor underflow to zero. Normal-magnitude
    // systems stay at scale 1.0 to preserve bit-identical trajectories.
    let a_max = a
        .iter()
        .flatten()
        .copied()
        .map(f64::abs)
        .fold(0.0_f64, f64::max);
    let a_scale = if a_max > 0.0 && !(1e-60..=1e60).contains(&a_max) {
        a_max
    } else {
        1.0
    };
    let b_max = b.iter().copied().map(f64::abs).fold(0.0_f64, f64::max);
    let b_scale = if b_max > 0.0 && !(1e-60..=1e60).contains(&b_max) {
        b_max
    } else {
        1.0
    };
    // u holds the columns of A (scaled by 1/a_scale), rotated in place until mutually orthogonal (u = UΣ).
    let mut u: [Vec<f64>; N] = std::array::from_fn(|j| {
        if a_scale == 1.0 {
            a.iter().map(|row| row[j]).collect()
        } else {
            a.iter().map(|row| row[j] / a_scale).collect()
        }
    });
    let mut v = [[0.0; N]; N];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _ in 0..MAX_SWEEPS {
        let mut rotated = false;
        for p in 0..N {
            for q in p + 1..N {
                let (mut alpha, mut beta, mut gamma) = (0.0, 0.0, 0.0);
                for i in 0..m {
                    alpha += u[p][i] * u[p][i];
                    beta += u[q][i] * u[q][i];
                    gamma += u[p][i] * u[q][i];
                }
                if gamma == 0.0 || gamma.abs() <= f64::EPSILON * (alpha.sqrt() * beta.sqrt()) {
                    continue;
                }
                rotated = true;
                let zeta = (beta - alpha) / (2.0 * gamma);
                let t = zeta.signum() / (zeta.abs() + crate::det::hypot(1.0, zeta));
                let c = 1.0 / crate::det::hypot(1.0, t);
                let s = c * t;
                for i in 0..m {
                    let (up, uq) = (u[p][i], u[q][i]);
                    u[p][i] = c * up - s * uq;
                    u[q][i] = s * up + c * uq;
                }
                for row in &mut v {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
        if !rotated {
            break;
        }
    }
    let sigma: [f64; N] = std::array::from_fn(|j| u[j].iter().map(|x| x * x).sum::<f64>().sqrt());
    let sigma_max = sigma.iter().copied().fold(0.0, f64::max);
    let cutoff = f64::EPSILON * (m.max(N) as f64) * sigma_max;
    let mut x = [0.0; N];
    for j in 0..N {
        if sigma[j] <= cutoff {
            continue;
        }
        // x += V[:, j] · (u_j · b_scaled) / σ_j²  (u_j = σ_j · U[:, j]).
        let dot: f64 = if b_scale == 1.0 {
            u[j].iter().zip(b).map(|(ui, bi)| ui * bi).sum()
        } else {
            u[j].iter().zip(b).map(|(ui, bi)| ui * (bi / b_scale)).sum()
        };
        let proj = dot / (sigma[j] * sigma[j]);
        for (k, xk) in x.iter_mut().enumerate() {
            *xk += v[k][j] * proj;
        }
    }
    if a_scale != 1.0 || b_scale != 1.0 {
        let ratio = b_scale / a_scale;
        for xk in &mut x {
            if ratio.is_finite() && ratio > 0.0 {
                *xk *= ratio;
            } else {
                *xk = (*xk * b_scale) / a_scale;
            }
        }
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_quadratic_is_recovered() {
        let s = [0.0, 1.0, 2.5, 4.0, 7.0];
        let a: Vec<[f64; 3]> = s.iter().map(|&t| [1.0, t, t * t]).collect();
        let b: Vec<f64> = s.iter().map(|&t| 2.0 - 3.0 * t + 0.5 * t * t).collect();
        let x = lstsq(&a, &b);
        assert!(
            (x[0] - 2.0).abs() < 1e-13 && (x[1] + 3.0).abs() < 1e-13 && (x[2] - 0.5).abs() < 1e-13,
            "{x:?}"
        );
    }

    #[test]
    fn overdetermined_line_fit() {
        // Points (0,0), (1,1), (2,1): least-squares line y = 1/6 + x/2.
        let a = [[1.0, 0.0], [1.0, 1.0], [1.0, 2.0]];
        let x = lstsq(&a, &[0.0, 1.0, 1.0]);
        assert!(
            (x[0] - 1.0 / 6.0).abs() < 1e-15 && (x[1] - 0.5).abs() < 1e-15,
            "{x:?}"
        );
    }

    #[test]
    fn rank_deficient_gives_minimum_norm() {
        // Two identical columns: every x0 + x1 = 1 fits; the minimum-norm one is (0.5, 0.5).
        let a = [[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]];
        let x = lstsq(&a, &[1.0, 2.0, 3.0]);
        assert!(
            (x[0] - 0.5).abs() < 1e-15 && (x[1] - 0.5).abs() < 1e-15,
            "{x:?}"
        );
        // numpy.linalg.lstsq([[1,0,0],[1,0,0],[1,1,1],[1,1,1]], [0,0,1,1]) = [0, 0.5, 0.5].
        let a = [
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ];
        let x = lstsq(&a, &[0.0, 0.0, 1.0, 1.0]);
        assert!(
            x[0].abs() < 1e-15 && (x[1] - 0.5).abs() < 1e-15 && (x[2] - 0.5).abs() < 1e-15,
            "{x:?}"
        );
    }

    #[test]
    fn zero_matrix_gives_zero() {
        assert_eq!(lstsq(&[[0.0, 0.0], [0.0, 0.0]], &[1.0, 2.0]), [0.0, 0.0]);
        assert_eq!(lstsq::<3>(&[], &[]), [0.0; 3]);
    }

    #[test]
    fn extreme_scales_do_not_skip_jacobi_rotations_or_underflow() {
        for scale in [1e-200, 1e-80, 1e80, 1e200] {
            let a = [
                [1.0 * scale, 1.0 * scale],
                [1.0 * scale, 2.0 * scale],
                [2.0 * scale, 3.0 * scale],
            ];
            let b = [3.0 * scale, 5.0 * scale, 8.0 * scale];
            let x = lstsq(&a, &b);
            assert!(
                (x[0] - 1.0).abs() < 1e-13 && (x[1] - 2.0).abs() < 1e-13,
                "scale={scale:e}: got {x:?}"
            );
        }
    }

    #[test]
    fn non_finite_inputs_return_nan_instead_of_zeros() {
        for (a, b) in [
            ([[f64::INFINITY, 0.0], [0.0, 1.0]], [1.0, 2.0]),
            ([[f64::NAN, 1e100], [1e100, 1e100]], [1.0, 2.0]),
            ([[1.0, 0.0], [0.0, 1.0]], [f64::NAN, 2.0]),
            ([[1.0, 0.0], [0.0, 1.0]], [1.0, f64::NEG_INFINITY]),
        ] {
            let x = lstsq(&a, &b);
            assert!(x.iter().all(|v| v.is_nan()), "expected NaNs, got {x:?}");
        }
    }
}
