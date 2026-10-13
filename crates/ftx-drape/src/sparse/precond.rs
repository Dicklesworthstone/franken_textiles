//! Preconditioned conjugate gradients, for inexact Newton steps preconditioned by a recent LDLᵀ factor.
//!
//! Provenance: [`pcg`] is lifted from frankensim `crates/fs-sparse/src/precond.rs` at commit
//! `6eb72a0c075fa9ea7dff083e1aafec6c61c5be21` (same author, MIT + OpenAI/Anthropic rider). Changes: the
//! preconditioner is any `Fn(&[f64]) -> Vec<f64>` (here an [`super::direct::LdltFactor`]'s solve); the
//! iteration starts at zero and refuses (returns `None`) on non-positive curvature, a non-finite value or
//! the iteration cap, since the caller then factorizes.

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
    let norm = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
    let bnorm = norm(b).max(f64::MIN_POSITIVE);
    let mut x = vec![0.0f64; n];
    let mut r = b.to_vec();
    let mut z = m(&r);
    let mut p = z.clone();
    let mut rz: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
    let mut ap = vec![0.0f64; n];
    for it in 0..max_iters {
        let rel = norm(&r) / bnorm;
        if !rel.is_finite() || !rz.is_finite() {
            return Err(PcgFailure::NotConverged(it));
        }
        if rel <= tol {
            return Ok((
                x,
                PcgReport {
                    iters: it,
                    rel_residual: rel,
                },
            ));
        }
        a.spmv(&p, &mut ap);
        let pap: f64 = p.iter().zip(&ap).map(|(a, b)| a * b).sum();
        if pap.is_nan() {
            return Err(PcgFailure::NotConverged(it));
        }
        if pap <= 0.0 {
            return Err(PcgFailure::NegativeCurvature(it + 1));
        }
        let alpha = rz / pap;
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
    if rel <= tol {
        Ok((
            x,
            PcgReport {
                iters: max_iters,
                rel_residual: rel,
            },
        ))
    } else {
        Err(PcgFailure::NotConverged(max_iters))
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
    }
}
