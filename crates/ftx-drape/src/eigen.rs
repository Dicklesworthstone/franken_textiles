//! Small dense symmetric eigensolver and PSD projection of element Hessians.
//!
//! Provenance: [`jacobi_eigh`] is lifted from frankensim `crates/fs-la/src/eigen.rs` at commit
//! `93fc9b26f618bea6ebac6534f1d33c715d8a1c92` (MIT + rider) without its workspace-admission call; the
//! projection `V·max(Λ, 0)·Vᵀ` and its reduction to the complement of the translations are new.

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

/// [`project_psd`] for the Hessian of an energy of `v` vertices that depends on their positions only
/// through differences (membranes, hinges, elastic segments), so every rigid translation is in its null
/// space: the eigendecomposition runs on the complement of the translations (Helmert basis ⊗ I₃), a
/// 3(v − 1)-square matrix instead of 3v. The same projection in exact arithmetic, at about 30 % of the cost
/// for a triangle. Returns whether anything changed.
pub fn project_psd_translation_free(h: &mut [f64], v: usize) -> bool {
    let n = 3 * v;
    let r = 3 * (v - 1);
    // Helmert basis of {1}⊥ in Rᵛ: q_j = (1, …, 1, −j, 0, …) / √(j(j + 1)), j = 1..v−1.
    let q = |j: usize, i: usize| -> f64 {
        let s = ((j * (j + 1)) as f64).sqrt();
        match i.cmp(&j) {
            std::cmp::Ordering::Less => 1.0 / s,
            std::cmp::Ordering::Equal => -(j as f64) / s,
            std::cmp::Ordering::Greater => 0.0,
        }
    };
    let qm: Vec<f64> = (0..v - 1)
        .flat_map(|j| (0..v).map(move |i| q(j + 1, i)))
        .collect(); // qm[j * v + i]
    // Reduced matrix: R[(3j+c),(3l+d)] = Σ_{i,i'} q_j[i] q_l[i'] H[(3i+c),(3i'+d)].
    let mut half = vec![0.0f64; n * r]; // H Q: half[a * r + (3l+d)]
    for a in 0..n {
        for l in 0..v - 1 {
            for d in 0..3 {
                let mut acc = 0.0;
                for i2 in 0..v {
                    acc += h[a * n + 3 * i2 + d] * qm[l * v + i2];
                }
                half[a * r + 3 * l + d] = acc;
            }
        }
    }
    let mut red = vec![0.0f64; r * r];
    for j in 0..v - 1 {
        for c in 0..3 {
            for b in 0..r {
                let mut acc = 0.0;
                for i in 0..v {
                    acc += qm[j * v + i] * half[(3 * i + c) * r + b];
                }
                red[(3 * j + c) * r + b] = acc;
            }
        }
    }
    // symmetric up to rounding; Jacobi reads both triangles, so make it exactly symmetric
    for a in 0..r {
        for b in a + 1..r {
            let s = 0.5 * (red[a * r + b] + red[b * r + a]);
            red[a * r + b] = s;
            red[b * r + a] = s;
        }
    }
    if !project_psd(&mut red, r) {
        return false;
    }
    // H⁺ = Q R⁺ Qᵀ
    let mut back = vec![0.0f64; n * r]; // Q R⁺: back[(3i+c) * r + b]
    for i in 0..v {
        for c in 0..3 {
            for b in 0..r {
                let mut acc = 0.0;
                for j in 0..v - 1 {
                    acc += qm[j * v + i] * red[(3 * j + c) * r + b];
                }
                back[(3 * i + c) * r + b] = acc;
            }
        }
    }
    for a in 0..n {
        for i2 in 0..v {
            for d in 0..3 {
                let mut acc = 0.0;
                for l in 0..v - 1 {
                    acc += back[a * r + 3 * l + d] * qm[l * v + i2];
                }
                h[a * n + 3 * i2 + d] = acc;
            }
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

    /// `Dᵀ S D` for the differences of `v` vertices to the first (translation-free by construction) and a
    /// symmetric `S` from a seed, shifted by `shift` (negative: indefinite).
    fn translation_free(v: usize, seed: u64, shift: f64) -> Vec<f64> {
        let (n, r) = (3 * v, 3 * (v - 1));
        let mut s = seed;
        let mut rnd = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 11) as f64) / (1u64 << 53) as f64 - 0.5
        };
        let b: Vec<f64> = (0..r * r).map(|_| rnd()).collect();
        let mut sm = vec![0.0; r * r];
        for i in 0..r {
            for j in 0..r {
                sm[i * r + j] = (0..r).map(|k| b[i * r + k] * b[j * r + k]).sum::<f64>()
                    + if i == j { shift } else { 0.0 };
            }
        }
        // D: row (3(e−1)+c) has +1 at vertex e, −1 at vertex 0 (coordinate c)
        let d = |row: usize, col: usize| -> f64 {
            let (e, c) = (row / 3 + 1, row % 3);
            if col % 3 != c {
                0.0
            } else if col / 3 == e {
                1.0
            } else if col / 3 == 0 {
                -1.0
            } else {
                0.0
            }
        };
        let mut h = vec![0.0; n * n];
        for a in 0..n {
            for c in 0..n {
                let mut acc = 0.0;
                for i in 0..r {
                    for j in 0..r {
                        acc += d(i, a) * sm[i * r + j] * d(j, c);
                    }
                }
                h[a * n + c] = acc;
            }
        }
        h
    }

    #[test]
    fn translation_free_projection_equals_the_full_one() {
        for (v, seed) in [(2, 1), (3, 2), (3, 3), (4, 4)] {
            let n = 3 * v;
            let h = translation_free(v, seed, -0.4);
            let (mut full, mut reduced) = (h.clone(), h.clone());
            assert!(project_psd(&mut full, n));
            assert!(project_psd_translation_free(&mut reduced, v));
            let scale = h.iter().fold(0.0f64, |m, x| m.max(x.abs()));
            for (a, b) in full.iter().zip(&reduced) {
                assert!((a - b).abs() < 1e-12 * scale, "v {v}: {a} vs {b}");
            }
            // already PSD: unchanged
            let mut psd = translation_free(v, seed, 0.1);
            let before = psd.clone();
            assert!(!project_psd_translation_free(&mut psd, v));
            assert_eq!(psd, before);
        }
    }
}
