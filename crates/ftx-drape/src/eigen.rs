//! Small dense symmetric eigensolver and PSD projection of element Hessians.
//!
//! Provenance: [`jacobi_eigh`] is lifted from frankensim `crates/fs-la/src/eigen.rs` at commit
//! `93fc9b26f618bea6ebac6534f1d33c715d8a1c92` (MIT + rider) without its workspace-admission call; the
//! projection `V·max(Λ, 0)·Vᵀ` is new.

/// Eigendecomposition of a dense symmetric matrix (row-major n × n) by cyclic Jacobi: eigenvalues
/// ascending (lowest index first on ties) and eigenvectors as the columns of a row-major n × n matrix.
///
/// # Panics
/// If `a.len() != n * n`.
#[must_use]
pub fn jacobi_eigh(a: &[f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    let square = n * n;
    assert_eq!(a.len(), square, "a must be n*n = {square}");
    let mut m = a.to_vec();
    let mut v = vec![0.0f64; square];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    for _sweep in 0..60 {
        let mut rotated = false;
        for p in 0..n {
            for q in p + 1..n {
                let apq = m[p * n + q];
                let scale = (m[p * n + p].abs() + m[q * n + q].abs()).max(f64::MIN_POSITIVE);
                if apq.abs() <= 1e-16 * scale {
                    continue;
                }
                rotated = true;
                let theta = (m[q * n + q] - m[p * n + p]) / (2.0 * apq);
                let t = if theta >= 0.0 {
                    1.0 / (theta + (1.0 + theta * theta).sqrt())
                } else {
                    -1.0 / (-theta + (1.0 + theta * theta).sqrt())
                };
                let c = 1.0 / (1.0 + t * t).sqrt();
                let s = c * t;
                for k in 0..n {
                    let (mkp, mkq) = (m[k * n + p], m[k * n + q]);
                    m[k * n + p] = c.mul_add(mkp, -(s * mkq));
                    m[k * n + q] = s.mul_add(mkp, c * mkq);
                }
                for k in 0..n {
                    let (mpk, mqk) = (m[p * n + k], m[q * n + k]);
                    m[p * n + k] = c.mul_add(mpk, -(s * mqk));
                    m[q * n + k] = s.mul_add(mpk, c * mqk);
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c.mul_add(vkp, -(s * vkq));
                    v[k * n + q] = s.mul_add(vkp, c * vkq);
                }
            }
        }
        if !rotated {
            break;
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    let vals: Vec<f64> = (0..n).map(|i| m[i * n + i]).collect();
    order.sort_by(|&x, &y| vals[x].total_cmp(&vals[y]).then(x.cmp(&y)));
    let mut evals = vec![0.0f64; n];
    let mut evecs = vec![0.0f64; square];
    for (slot, &src) in order.iter().enumerate() {
        evals[slot] = vals[src];
        for i in 0..n {
            evecs[i * n + slot] = v[i * n + src];
        }
    }
    (evals, evecs)
}

/// Projects a symmetric matrix onto the positive semidefinite cone in place (`V·max(Λ, 0)·Vᵀ`), the
/// standard fix for indefinite element Hessians in projected Newton. Returns whether anything changed.
pub fn project_psd(h: &mut [f64], n: usize) -> bool {
    let (vals, vecs) = jacobi_eigh(h, n);
    if vals.first().is_none_or(|&l| l >= 0.0) {
        return false;
    }
    for i in 0..n {
        for j in 0..n {
            let mut acc = 0.0;
            for (k, &l) in vals.iter().enumerate() {
                if l > 0.0 {
                    acc += vecs[i * n + k] * l * vecs[j * n + k];
                }
            }
            h[i * n + j] = acc;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eigen_of_a_known_matrix() {
        // [[2, 1], [1, 2]]: eigenvalues 1 and 3.
        let (vals, vecs) = jacobi_eigh(&[2.0, 1.0, 1.0, 2.0], 2);
        assert!((vals[0] - 1.0).abs() < 1e-15 && (vals[1] - 3.0).abs() < 1e-15);
        assert!((vecs[0].abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15);
    }

    #[test]
    fn projection_clamps_negative_eigenvalues_only() {
        let mut h = [1.0, 2.0, 2.0, 1.0]; // eigenvalues -1 and 3
        assert!(project_psd(&mut h, 2));
        // Projected: 3 · v vᵀ with v = (1, 1)/√2 → all entries 1.5.
        for x in h {
            assert!((x - 1.5).abs() < 1e-14, "{h:?}");
        }
        let mut spd = [2.0, 0.0, 0.0, 1.0];
        assert!(!project_psd(&mut spd, 2));
        assert_eq!(spd, [2.0, 0.0, 0.0, 1.0]);
    }
}
