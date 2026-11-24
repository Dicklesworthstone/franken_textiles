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
    let mut v = vec![0.0f64; square];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    if a.iter().any(|val| !val.is_finite()) {
        return (vec![f64::NAN; n], v);
    }
    let max_abs = a.iter().map(|val| val.abs()).fold(0.0f64, f64::max);
    let scale_all = if max_abs > 0.0 && !(1e-150..=1e150).contains(&max_abs) {
        max_abs
    } else {
        1.0
    };
    let mut m: Vec<f64> = if scale_all == 1.0 {
        a.to_vec()
    } else {
        a.iter().map(|&val| val / scale_all).collect()
    };
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
                let hypot_theta = if theta.abs() > 1e150 {
                    ftx_core::det::hypot(1.0, theta)
                } else {
                    (1.0 + theta * theta).sqrt()
                };
                let t = if theta >= 0.0 {
                    1.0 / (theta + hypot_theta)
                } else {
                    -1.0 / (-theta + hypot_theta)
                };
                let c = 1.0 / (1.0 + t * t).sqrt();
                let s = c * t;
                for k in 0..n {
                    let (mkp, mkq) = (m[k * n + p], m[k * n + q]);
                    m[k * n + p] = c * mkp - s * mkq;
                    m[k * n + q] = s * mkp + c * mkq;
                }
                for k in 0..n {
                    let (mpk, mqk) = (m[p * n + k], m[q * n + k]);
                    m[p * n + k] = c * mpk - s * mqk;
                    m[q * n + k] = s * mpk + c * mqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
        if !rotated {
            break;
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    let vals: Vec<f64> = if scale_all == 1.0 {
        (0..n).map(|i| m[i * n + i]).collect()
    } else {
        (0..n).map(|i| m[i * n + i] * scale_all).collect()
    };
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
/// Non-finite matrices are left untouched (never zeroed) so upstream callers fail closed.
///
/// The element sizes (3 to 15) run in fixed-size stack buffers ([`project_psd_fixed`]), which perform
/// the same operations in the same order as the general path, so the result is the same bit for bit.
pub fn project_psd(h: &mut [f64], n: usize) -> bool {
    assert_eq!(h.len(), n * n, "h must be n*n = {}", n * n);
    match n {
        3 => project_psd_fixed::<3>(h),
        6 => project_psd_fixed::<6>(h),
        9 => project_psd_fixed::<9>(h),
        12 => project_psd_fixed::<12>(h),
        15 => project_psd_fixed::<15>(h),
        _ => project_psd_general(h, n),
    }
}

/// [`jacobi_eigh`] of a finite N × N matrix in fixed-size arrays: the same rotations, in the same
/// order, on the same values, without heap buffers or a run-time size.
fn jacobi_eigh_fixed<const N: usize>(a: &[f64]) -> ([f64; N], [[f64; N]; N]) {
    let mut v = [[0.0f64; N]; N];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    let max_abs = a.iter().map(|val| val.abs()).fold(0.0f64, f64::max);
    let scale_all = if max_abs > 0.0 && !(1e-150..=1e150).contains(&max_abs) {
        max_abs
    } else {
        1.0
    };
    let mut m = [[0.0f64; N]; N];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, entry) in row.iter_mut().enumerate() {
            *entry = if scale_all == 1.0 {
                a[i * N + j]
            } else {
                a[i * N + j] / scale_all
            };
        }
    }
    for _sweep in 0..60 {
        let mut rotated = false;
        for p in 0..N {
            for q in p + 1..N {
                let apq = m[p][q];
                let scale = (m[p][p].abs() + m[q][q].abs()).max(f64::MIN_POSITIVE);
                if apq.abs() <= 1e-16 * scale {
                    continue;
                }
                rotated = true;
                let theta = (m[q][q] - m[p][p]) / (2.0 * apq);
                let hypot_theta = if theta.abs() > 1e150 {
                    ftx_core::det::hypot(1.0, theta)
                } else {
                    (1.0 + theta * theta).sqrt()
                };
                let t = if theta >= 0.0 {
                    1.0 / (theta + hypot_theta)
                } else {
                    -1.0 / (-theta + hypot_theta)
                };
                let c = 1.0 / (1.0 + t * t).sqrt();
                let s = c * t;
                for row in &mut m {
                    let (mkp, mkq) = (row[p], row[q]);
                    row[p] = c * mkp - s * mkq;
                    row[q] = s * mkp + c * mkq;
                }
                for k in 0..N {
                    let (mpk, mqk) = (m[p][k], m[q][k]);
                    m[p][k] = c * mpk - s * mqk;
                    m[q][k] = s * mpk + c * mqk;
                }
                for row in &mut v {
                    let (vkp, vkq) = (row[p], row[q]);
                    row[p] = c * vkp - s * vkq;
                    row[q] = s * vkp + c * vkq;
                }
            }
        }
        if !rotated {
            break;
        }
    }
    let vals: [f64; N] = std::array::from_fn(|i| {
        if scale_all == 1.0 {
            m[i][i]
        } else {
            m[i][i] * scale_all
        }
    });
    let mut order: [usize; N] = std::array::from_fn(|i| i);
    order.sort_by(|&x, &y| vals[x].total_cmp(&vals[y]).then(x.cmp(&y)));
    let mut evals = [0.0f64; N];
    let mut evecs = [[0.0f64; N]; N];
    for (slot, &src) in order.iter().enumerate() {
        evals[slot] = vals[src];
        for (row, from) in evecs.iter_mut().zip(&v) {
            row[slot] = from[src];
        }
    }
    (evals, evecs)
}

/// [`project_psd_general`] for an N × N matrix (`h.len() == N * N`), bit for bit.
fn project_psd_fixed<const N: usize>(h: &mut [f64]) -> bool {
    if h.iter().any(|v| !v.is_finite()) {
        return false;
    }
    let (vals, vecs) = jacobi_eigh_fixed::<N>(h);
    if vals.iter().any(|l| !l.is_finite()) || vecs.iter().flatten().any(|v| !v.is_finite()) {
        h.fill(f64::NAN);
        return false;
    }
    if vals[0] >= 0.0 {
        return false;
    }
    for i in 0..N {
        for j in 0..N {
            let mut acc = 0.0;
            for (k, &l) in vals.iter().enumerate() {
                if l > 0.0 {
                    acc += vecs[i][k] * l * vecs[j][k];
                }
            }
            h[i * N + j] = acc;
        }
    }
    true
}

/// [`project_psd`] for any size, with heap buffers.
fn project_psd_general(h: &mut [f64], n: usize) -> bool {
    if h.iter().any(|v| !v.is_finite()) {
        return false;
    }
    let (vals, vecs) = jacobi_eigh(h, n);
    if vals.iter().any(|l| !l.is_finite()) || vecs.iter().any(|v| !v.is_finite()) {
        h.fill(f64::NAN);
        return false;
    }
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
    assert_eq!(h.len(), n * n, "h must be (3v)^2 = {}", n * n);
    if v == 0 || h.iter().any(|x| !x.is_finite()) {
        return false;
    }
    if v == 1 {
        return project_psd(h, 3);
    }
    // The element sizes in fixed-size buffers, bit for bit the general path below.
    match v {
        2 => return translation_free_fixed::<2, 6, 3>(h),
        3 => return translation_free_fixed::<3, 9, 6>(h),
        4 => return translation_free_fixed::<4, 12, 9>(h),
        6 => return translation_free_fixed::<6, 18, 15>(h),
        _ => {}
    }
    translation_free_general(h, v)
}

/// The Helmert basis vector `q_j` (j ≥ 1) of {1}⊥ in Rᵛ at component `i`:
/// `(1, …, 1, −j, 0, …) / √(j(j + 1))`.
fn helmert(j: usize, i: usize) -> f64 {
    let s = ((j * (j + 1)) as f64).sqrt();
    match i.cmp(&j) {
        std::cmp::Ordering::Less => 1.0 / s,
        std::cmp::Ordering::Equal => -(j as f64) / s,
        std::cmp::Ordering::Greater => 0.0,
    }
}

/// [`translation_free_general`] for `V` vertices (`N = 3V`, `R = 3(V − 1)`), in fixed-size arrays:
/// the same sums in the same order, so the same bits.
fn translation_free_fixed<const V: usize, const N: usize, const R: usize>(h: &mut [f64]) -> bool {
    debug_assert!(N == 3 * V && R + 3 == N && h.len() == N * N);
    let mut qm = [[0.0f64; V]; V]; // rows 0..V−1: qm[j][i] = q_{j+1}[i]
    for (j, row) in qm.iter_mut().take(V - 1).enumerate() {
        for (i, entry) in row.iter_mut().enumerate() {
            *entry = helmert(j + 1, i);
        }
    }
    let mut half = [[0.0f64; R]; N]; // H Q
    for (a, row) in half.iter_mut().enumerate() {
        for l in 0..V - 1 {
            for d in 0..3 {
                let mut acc = 0.0;
                for i2 in 0..V {
                    acc += h[a * N + 3 * i2 + d] * qm[l][i2];
                }
                row[3 * l + d] = acc;
            }
        }
    }
    let mut red_rows = [[0.0f64; R]; R]; // Qᵀ H Q
    for j in 0..V - 1 {
        for c in 0..3 {
            for b in 0..R {
                let mut acc = 0.0;
                for i in 0..V {
                    acc += qm[j][i] * half[3 * i + c][b];
                }
                red_rows[3 * j + c][b] = acc;
            }
        }
    }
    for a in 0..R {
        for b in a + 1..R {
            let s = 0.5 * (red_rows[a][b] + red_rows[b][a]);
            red_rows[a][b] = s;
            red_rows[b][a] = s;
        }
    }
    let mut red = [0.0f64; 15 * 15];
    let red = &mut red[..R * R];
    for (a, row) in red_rows.iter().enumerate() {
        red[a * R..(a + 1) * R].copy_from_slice(row);
    }
    if !project_psd_fixed::<R>(red) {
        if red.iter().any(|x| !x.is_finite()) {
            h.fill(f64::NAN);
        }
        return false;
    }
    let mut back = [[0.0f64; R]; N]; // Q R⁺
    for i in 0..V {
        for c in 0..3 {
            for b in 0..R {
                let mut acc = 0.0;
                for j in 0..V - 1 {
                    acc += qm[j][i] * red[(3 * j + c) * R + b];
                }
                back[3 * i + c][b] = acc;
            }
        }
    }
    for (a, row) in back.iter().enumerate() {
        for i2 in 0..V {
            for d in 0..3 {
                let mut acc = 0.0;
                for l in 0..V - 1 {
                    acc += row[3 * l + d] * qm[l][i2];
                }
                h[a * N + 3 * i2 + d] = acc;
            }
        }
    }
    true
}

/// [`project_psd_translation_free`] for any vertex count ≥ 2, with heap buffers.
fn translation_free_general(h: &mut [f64], v: usize) -> bool {
    let n = 3 * v;
    let r = 3 * (v - 1);
    // Helmert basis of {1}⊥ in Rᵛ, j = 1..v−1.
    let qm: Vec<f64> = (0..v - 1)
        .flat_map(|j| (0..v).map(move |i| helmert(j + 1, i)))
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
        if red.iter().any(|x| !x.is_finite()) {
            h.fill(f64::NAN);
        }
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

    /// The fixed-size paths give the general paths' results bit for bit, across element sizes,
    /// definite and indefinite matrices, and the rescaled extremes.
    #[test]
    fn fixed_size_projections_match_the_general_ones_bit_for_bit() {
        let bits = |h: &[f64]| h.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        for n in [3, 6, 9, 12, 15] {
            for seed in 0..12u64 {
                for (scale, shift) in [(1.0, -0.4), (1.0, 0.3), (1e-170, -0.2), (1e170, -0.2)] {
                    let mut s = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ n as u64;
                    let mut rnd = || {
                        s = s
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        ((s >> 11) as f64) / (1u64 << 53) as f64 - 0.5
                    };
                    let b: Vec<f64> = (0..n * n).map(|_| rnd()).collect();
                    let mut h = vec![0.0; n * n];
                    for i in 0..n {
                        for j in 0..n {
                            let dot: f64 = (0..n).map(|k| b[i * n + k] * b[j * n + k]).sum();
                            h[i * n + j] = (dot + if i == j { shift } else { 0.0 }) * scale;
                        }
                    }
                    let (mut fixed, mut general) = (h.clone(), h.clone());
                    assert_eq!(
                        project_psd(&mut fixed, n),
                        project_psd_general(&mut general, n)
                    );
                    assert_eq!(bits(&fixed), bits(&general), "n {n}, seed {seed}");
                }
            }
        }
        for v in [2, 3, 4, 6] {
            for seed in 0..12u64 {
                for shift in [-0.4, 0.1] {
                    let h = translation_free(v, seed + 100, shift);
                    let (mut fixed, mut general) = (h.clone(), h.clone());
                    assert_eq!(
                        project_psd_translation_free(&mut fixed, v),
                        translation_free_general(&mut general, v)
                    );
                    assert_eq!(bits(&fixed), bits(&general), "v {v}, seed {seed}");
                }
            }
        }
    }

    #[test]
    fn non_finite_hessian_is_preserved_and_extreme_scales_project_cleanly() {
        // Non-finite inputs must NEVER be silently zeroed out by PSD projection.
        let mut all_nan = [f64::NAN; 4];
        assert!(!project_psd(&mut all_nan, 2));
        assert!(all_nan.iter().all(|v| v.is_nan()));

        let mut mixed_nan = [-1.0, 0.0, 0.0, f64::NAN];
        assert!(!project_psd(&mut mixed_nan, 2));
        assert!(mixed_nan[3].is_nan());

        let mut inf_h = [f64::INFINITY, 0.0, 0.0, -1.0];
        assert!(!project_psd(&mut inf_h, 2));
        assert!(inf_h[0].is_infinite());

        let mut tf_nan = vec![f64::NAN; 81];
        assert!(!project_psd_translation_free(&mut tf_nan, 3));
        assert!(tf_nan.iter().all(|v| v.is_nan()));

        // Extreme-scale symmetric indefinite matrices project accurately without overflow/underflow.
        for scale in [1e-200_f64, 1e200_f64] {
            let mut h = [1.0 * scale, 2.0 * scale, 2.0 * scale, 1.0 * scale];
            assert!(project_psd(&mut h, 2));
            for x in h {
                assert!(
                    ((x - 1.5 * scale).abs() / (1.5 * scale)) < 1e-13,
                    "scale={scale:e}: {h:?}"
                );
            }
        }
    }
}
