//! Preconditioned conjugate gradients, for inexact Newton steps preconditioned by a recent LDLᵀ factor.
//!
//! Provenance: [`pcg`] is lifted from frankensim `crates/fs-sparse/src/precond.rs` at commit
//! `6eb72a0c075fa9ea7dff083e1aafec6c61c5be21` (same author, MIT + OpenAI/Anthropic rider). Changes: the
//! preconditioner is any `Fn(&[f64]) -> Vec<f64>` (here an [`super::direct::LdltFactor`]'s solve); the
//! iteration starts at zero and refuses (returns `None`) on non-positive curvature, a non-finite value or
//! the iteration cap, since the caller then factorizes. An iterate is accepted only when the recomputed
//! residual `b − a x` meets the tolerance too (frankensim `crates/fs-solver/src/checked_cg.rs`'s gate).

use super::Csr;

/// What one PCG solve did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PcgReport {
    /// Iterations performed.
    pub iters: usize,
    /// Final relative residual ‖b − Ax‖₂ / ‖b‖₂.
    pub rel_residual: f64,
}

/// Why [`pcg`] stopped short, after how many iterations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcgFailure {
    /// A search direction with non-positive curvature: `a` is not positive definite.
    NegativeCurvature(usize),
    /// The iteration cap, or a non-finite value.
    NotConverged(usize),
}

/// Solves `a x = b` from `x = 0` by conjugate gradients preconditioned with `m`, to `‖r‖₂ ≤ tol ‖b‖₂` within
/// `max_iters`.
///
/// # Errors
/// [`PcgFailure`] when a search direction has non-positive curvature, or the tolerance is not reached
/// within `max_iters`, or anything becomes non-finite.
pub fn pcg(
    a: &Csr,
    b: &[f64],
    m: impl Fn(&[f64]) -> Vec<f64>,
    tol: f64,
    max_iters: usize,
) -> Result<(Vec<f64>, PcgReport), PcgFailure> {
    let n = b.len();
    if a.nrows() != n || a.ncols() != n || !(tol.is_finite() && tol > 0.0) {
        return Err(PcgFailure::NotConverged(0));
    }
    let norm = |v: &[f64]| -> f64 {
        let mut max_abs = 0.0f64;
        for &x in v {
            if x.is_nan() {
                return f64::NAN;
            }
            max_abs = max_abs.max(x.abs());
        }
        if !max_abs.is_finite() || max_abs == 0.0 {
            return max_abs;
        }
        if (1e-100..=1e100).contains(&max_abs) {
            return v.iter().map(|x| x * x).sum::<f64>().sqrt();
        }
        max_abs
            * v.iter()
                .map(|x| {
                    let s = x / max_abs;
                    s * s
                })
                .sum::<f64>()
                .sqrt()
    };
    let bnorm_raw = norm(b);
    if !bnorm_raw.is_finite() {
        return Err(PcgFailure::NotConverged(0));
    }
    let bnorm = bnorm_raw.max(f64::MIN_POSITIVE);
    let mut x = vec![0.0f64; n];
    let mut r = b.to_vec();
    let mut z = m(&r);
    if z.len() != n {
        return Err(PcgFailure::NotConverged(0));
    }
    let mut p = z.clone();
    let mut rz: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
    let mut ap = vec![0.0f64; n];
    for it in 0..max_iters {
        let rel = norm(&r) / bnorm;
        if !rel.is_finite() || !rz.is_finite() {
            return Err(PcgFailure::NotConverged(it));
        }
        if rel <= tol {
            return accept(a, b, x, it, tol, bnorm, &norm);
        }
        if rz <= 0.0 {
            return Err(PcgFailure::NotConverged(it));
        }
        a.spmv(&p, &mut ap);
        let pap: f64 = p.iter().zip(&ap).map(|(a, b)| a * b).sum();
        if !pap.is_finite() {
            return Err(PcgFailure::NotConverged(it));
        }
        if pap <= 0.0 {
            return Err(PcgFailure::NegativeCurvature(it + 1));
        }
        let alpha = rz / pap;
        if !alpha.is_finite() {
            return Err(PcgFailure::NotConverged(it));
        }
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        z = m(&r);
        let rz_new: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
        let beta = rz_new / rz;
        rz = rz_new;
        for i in 0..n {
            p[i] = beta * p[i] + z[i];
        }
    }
    let rel = norm(&r) / bnorm;
    if rel.is_finite() && rel <= tol {
        accept(a, b, x, max_iters, tol, bnorm, &norm)
    } else {
        Err(PcgFailure::NotConverged(max_iters))
    }
}

/// The recomputed-residual gate (frankensim `fs-solver` `checked_cg`): the recursive residual drifts from
/// `b − a x` in floating point, so an iterate is accepted only when the true residual meets the tolerance
/// too; otherwise the solve reports no convergence and the caller factorizes.
fn accept(
    a: &Csr,
    b: &[f64],
    x: Vec<f64>,
    iters: usize,
    tol: f64,
    bnorm: f64,
    norm: &impl Fn(&[f64]) -> f64,
) -> Result<(Vec<f64>, PcgReport), PcgFailure> {
    let mut ax = vec![0.0f64; b.len()];
    a.spmv(&x, &mut ax);
    let diff: Vec<f64> = b.iter().zip(&ax).map(|(bi, axi)| bi - axi).collect();
    let true_rel = norm(&diff) / bnorm;
    if true_rel.is_finite() && true_rel <= tol {
        Ok((
            x,
            PcgReport {
                iters,
                rel_residual: true_rel,
            },
        ))
    } else {
        Err(PcgFailure::NotConverged(iters))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse::Coo;

    #[test]
    fn solves_a_spd_system_and_refuses_an_indefinite_one() {
        // 1D Laplacian + diagonal shift (SPD), Jacobi preconditioner
        let n = 50;
        let mut c = Coo::new(n, n);
        for i in 0..n {
            c.push(i, i, 2.5);
            if i + 1 < n {
                c.push(i, i + 1, -1.0);
                c.push(i + 1, i, -1.0);
            }
        }
        let a = c.assemble();
        let b: Vec<f64> = (0..n).map(|i| f64::from(i as u32 % 7) - 3.0).collect();
        let (x, rep) = pcg(
            &a,
            &b,
            |r: &[f64]| r.iter().map(|v| v / 2.5).collect(),
            1e-10,
            200,
        )
        .unwrap();
        let mut ax = vec![0.0; n];
        a.spmv(&x, &mut ax);
        assert!(
            ax.iter().zip(&b).all(|(p, q)| (p - q).abs() < 1e-8),
            "{rep:?}"
        );
        // the iteration cap
        let jacobi = |r: &[f64]| r.iter().map(|v| v / 2.5).collect();
        assert_eq!(
            pcg(&a, &b, jacobi, 1e-10, 3).unwrap_err(),
            PcgFailure::NotConverged(3)
        );
        // an indefinite matrix: negative curvature is refused
        let mut c = Coo::new(2, 2);
        c.push(0, 0, 1.0);
        c.push(1, 1, -1.0);
        let fail = pcg(
            &c.assemble(),
            &[1.0, 1.0],
            |r: &[f64]| r.to_vec(),
            1e-12,
            10,
        )
        .unwrap_err();
        assert_eq!(fail, PcgFailure::NegativeCurvature(1));

        // Non-SPD preconditioner (M = -I) must refuse immediately instead of stepping uphill.
        let mut spd = Coo::new(2, 2);
        spd.push(0, 0, 2.0);
        spd.push(1, 1, 3.0);
        let spd = spd.assemble();
        assert_eq!(
            pcg(
                &spd,
                &[1.0, 1.0],
                |r: &[f64]| r.iter().map(|v| -v).collect(),
                1e-12,
                10
            )
            .unwrap_err(),
            PcgFailure::NotConverged(0)
        );
        // Zero RHS accepts immediately without dividing by rz == 0; NaN RHS refuses cleanly.
        let (xz, rz) = pcg(&spd, &[0.0, 0.0], |r: &[f64]| r.to_vec(), 1e-12, 10).unwrap();
        assert_eq!(xz, vec![0.0, 0.0]);
        assert_eq!(rz.iters, 0);
        assert_eq!(
            pcg(&spd, &[f64::NAN, 1.0], |r: &[f64]| r.to_vec(), 1e-12, 10).unwrap_err(),
            PcgFailure::NotConverged(0)
        );
    }
}
