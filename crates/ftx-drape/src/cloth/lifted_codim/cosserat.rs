//! 6-DOF geometrically exact Antman / Simo-Reissner Cosserat rod kinematics,
//! `[w, x, y, z]` unit quaternions with `SO(3)` exponential updates, exact
//! analytical 12-DOF segment gradients and Hessians, and left-trivialized
//! follower-moment tangent support.

use super::section_laws::{RodSectionLaw, RodSectionModel, SectionHistory};
use super::{
    CodimError, DetMath, all_finite3, all_finite4, cross3, dot3, eye3, mat3_mul, mat3_t_vec,
    mat3_transpose, norm2_3, outer3, quat_conj, quat_exp, quat_log, quat_mid, quat_mul,
    quat_normalize, quat_rotate, quat_to_mat3, scale3, skew3, sub3,
};

/// 6-DOF Cosserat rod state and constitutive specification.
#[derive(Clone, Debug, PartialEq)]
pub struct CosseratRod {
    /// Nodal positions `x_i` in metres (`N >= 2`).
    pub positions: Vec<[f64; 3]>,
    /// Nodal body-to-world unit quaternions `[w, x, y, z]` (`N`).
    pub quats: Vec<[f64; 4]>,
    /// Reference rest length `l0_e > 0` for each segment (`N - 1`).
    pub rest_lengths_m: Vec<f64>,
    /// Reference translational strain offset `Gamma_0` per segment (`N - 1`).
    pub rest_gamma: Vec<[f64; 3]>,
    /// Reference curvature/twist vector `kappa_0` per segment (`N - 1`).
    pub rest_kappa: Vec<[f64; 3]>,
    /// Constitutive section model per segment (`N - 1`).
    pub sections: Vec<RodSectionModel>,
}

/// Evaluation of a single 6-DOF Cosserat segment: strain energy, 12-DOF
/// left-trivialized gradient `[dp_a, dphi_a, dp_b, dphi_b]`, symmetric 12x12
/// chart Hessian, and candidate next section history.
#[derive(Clone, Debug, PartialEq)]
pub struct CosseratSegmentEval {
    /// Translational strain `Gamma = R(qm)^T * (p_b - p_a)/l0 - e1 - rest_gamma`.
    pub gamma: [f64; 3],
    /// Rotational strain `kappa = log(q_a^{-1} * q_b) / l0 - rest_kappa`.
    pub kappa: [f64; 3],
    /// Stored strain energy `l0 * energy_density` in Joules.
    pub energy_j: f64,
    /// Analytical 12-DOF gradient `[dp_a, dphi_a, dp_b, dphi_b]`.
    pub grad_12: [f64; 12],
    /// Exact analytical symmetric 12x12 chart Hessian.
    pub hess_12: [[f64; 12]; 12],
    /// Candidate updated [`SectionHistory`] (committed only upon step acceptance).
    pub next_history: SectionHistory,
}

impl CosseratRod {
    /// Construct a straight Cosserat rod along `+x` with uniform segment length
    /// and a single shared section model.
    pub fn straight(
        length_m: f64,
        segments: usize,
        section: RodSectionModel,
    ) -> Result<Self, CodimError> {
        if !length_m.is_finite() || length_m <= 0.0 {
            return Err(CodimError::InvalidParameter {
                detail: "CosseratRod::straight requires finite length_m > 0",
            });
        }
        if segments == 0 {
            return Err(CodimError::InvalidTopology {
                detail: "CosseratRod requires at least 1 segment",
            });
        }
        section.validate()?;
        let l0 = length_m / segments as f64;
        let positions = (0..=segments).map(|i| [i as f64 * l0, 0.0, 0.0]).collect();
        let quats = vec![[1.0, 0.0, 0.0, 0.0]; segments + 1];
        Ok(Self {
            positions,
            quats,
            rest_lengths_m: vec![l0; segments],
            rest_gamma: vec![[0.0; 3]; segments],
            rest_kappa: vec![[0.0; 3]; segments],
            sections: vec![section; segments],
        })
    }

    /// Number of segments.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.rest_lengths_m.len()
    }

    /// Validate array lengths, finiteness, positive rest lengths, and unit
    /// quaternions.
    pub fn validate<M: DetMath>(&self) -> Result<(), CodimError> {
        let n = self.positions.len();
        if n < 2 || self.quats.len() != n {
            return Err(CodimError::InvalidTopology {
                detail: "CosseratRod requires positions.len() == quats.len() >= 2",
            });
        }
        let s = n - 1;
        if self.rest_lengths_m.len() != s
            || self.rest_gamma.len() != s
            || self.rest_kappa.len() != s
            || self.sections.len() != s
        {
            return Err(CodimError::InvalidTopology {
                detail: "CosseratRod segment arrays must have length positions.len() - 1",
            });
        }
        for &p in &self.positions {
            if !all_finite3(p) {
                return Err(CodimError::NonFiniteInput {
                    context: "CosseratRod::positions",
                });
            }
        }
        for &q in &self.quats {
            let _ = quat_normalize::<M>(q)?;
        }
        for (idx, &l0) in self.rest_lengths_m.iter().enumerate() {
            if !l0.is_finite() {
                return Err(CodimError::NonFiniteInput {
                    context: "CosseratRod::rest_lengths_m",
                });
            }
            if l0 <= 1e-15 {
                return Err(CodimError::DegenerateSegment { segment_idx: idx });
            }
        }
        for (&rg, &rk) in self.rest_gamma.iter().zip(self.rest_kappa.iter()) {
            if !all_finite3(rg) || !all_finite3(rk) {
                return Err(CodimError::NonFiniteInput {
                    context: "CosseratRod rest strains",
                });
            }
        }
        for sec in &self.sections {
            sec.validate()?;
        }
        Ok(())
    }

    /// Normalize all nodal quaternions in place.
    pub fn normalize_quats<M: DetMath>(&mut self) -> Result<(), CodimError> {
        for q in &mut self.quats {
            *q = quat_normalize::<M>(*q)?;
        }
        Ok(())
    }

    /// Apply left-trivialized nodal increment `[dp_0, dphi_0, ...]` scaled by `alpha`.
    pub fn apply_increment<M: DetMath>(
        &mut self,
        delta: &[f64],
        alpha: f64,
    ) -> Result<(), CodimError> {
        let expected = 6 * self.positions.len();
        if delta.len() != expected {
            return Err(CodimError::InvalidTopology {
                detail: "CosseratRod increment length mismatch",
            });
        }
        for node in 0..self.positions.len() {
            let k = 6 * node;
            self.positions[node][0] += alpha * delta[k];
            self.positions[node][1] += alpha * delta[k + 1];
            self.positions[node][2] += alpha * delta[k + 2];
            let phi = [
                alpha * delta[k + 3],
                alpha * delta[k + 4],
                alpha * delta[k + 5],
            ];
            let dq = quat_exp::<M>(phi);
            self.quats[node] = quat_normalize::<M>(quat_mul(self.quats[node], dq))?;
        }
        Ok(())
    }

    /// Compute segment strains `(Gamma, kappa)` for segment `seg`.
    pub fn segment_strains<M: DetMath>(
        &self,
        seg: usize,
    ) -> Result<([f64; 3], [f64; 3]), CodimError> {
        if seg >= self.segment_count() {
            return Err(CodimError::InvalidTopology {
                detail: "Cosserat segment index out of range",
            });
        }
        segment_strains::<M>(
            self.positions[seg],
            self.quats[seg],
            self.positions[seg + 1],
            self.quats[seg + 1],
            self.rest_lengths_m[seg],
            self.rest_gamma[seg],
            self.rest_kappa[seg],
        )
    }
}

/// Evaluate Cosserat segment strains `(Gamma, kappa)` matching `fs-solid/src/rod.rs`
/// (`[w, x, y, z]` quaternions, hemisphere-aligned mid-frame, `DetMath::atan2`
/// logarithm).
pub fn segment_strains<M: DetMath>(
    pa: [f64; 3],
    qa: [f64; 4],
    pb: [f64; 3],
    qb: [f64; 4],
    l0: f64,
    rest_gamma: [f64; 3],
    rest_kappa: [f64; 3],
) -> Result<([f64; 3], [f64; 3]), CodimError> {
    if !all_finite3(pa)
        || !all_finite3(pb)
        || !all_finite4(qa)
        || !all_finite4(qb)
        || !l0.is_finite()
        || !all_finite3(rest_gamma)
        || !all_finite3(rest_kappa)
    {
        return Err(CodimError::NonFiniteInput {
            context: "segment_strains",
        });
    }
    if l0 <= 1e-15 {
        return Err(CodimError::DegenerateSegment { segment_idx: 0 });
    }
    let qa = quat_normalize::<M>(qa)?;
    let qb = quat_normalize::<M>(qb)?;
    let qm = quat_mid::<M>(qa, qb);
    let dr = scale3(sub3(pb, pa), 1.0 / l0);
    let local = quat_rotate(quat_conj(qm), dr);
    let gamma = [
        local[0] - 1.0 - rest_gamma[0],
        local[1] - rest_gamma[1],
        local[2] - rest_gamma[2],
    ];
    let rel = quat_mul(quat_conj(qa), qb);
    let log_q = quat_log::<M>(rel);
    let kappa = [
        log_q[0] / l0 - rest_kappa[0],
        log_q[1] / l0 - rest_kappa[1],
        log_q[2] / l0 - rest_kappa[2],
    ];
    Ok((gamma, kappa))
}

/// Compute `alpha(theta)` and `beta(theta) = alpha'(theta) / theta` for the
/// inverse right/left Jacobian of `SO(3)`.
fn d_exp_inv_coeffs<M: DetMath>(theta: f64) -> (f64, f64) {
    let t2 = theta * theta;
    if theta < 1e-3 {
        let t4 = t2 * t2;
        let alpha = 1.0 / 12.0 + t2 / 720.0 + t4 / 30240.0;
        let beta = 1.0 / 360.0 + t2 / 7560.0 + t4 / 201600.0;
        (alpha, beta)
    } else {
        let half = 0.5 * theta;
        let ch = M::cos(half);
        let sh = M::sin(half);
        let cot_half = ch / sh;
        let alpha = 1.0 / t2 - cot_half / (2.0 * theta);
        let beta = if theta < 1e-2 {
            1.0 / 360.0 + t2 / 7560.0 + t2 * t2 / 201600.0
        } else {
            (-2.0 / (t2 * theta) + cot_half / (2.0 * t2) + 1.0 / (4.0 * theta * sh * sh)) / theta
        };
        (alpha, beta)
    }
}

/// Inverse right (sign `+1.0`) or left (sign `-1.0`) Jacobian of `SO(3)` at `psi`.
fn j_inv_so3<M: DetMath>(psi: [f64; 3], right_sign: f64) -> [[f64; 3]; 3] {
    let theta = M::sqrt(norm2_3(psi));
    let (alpha, _) = d_exp_inv_coeffs::<M>(theta);
    let sk = skew3(psi);
    let sk2 = mat3_mul(sk, sk);
    let eye = eye3();
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = eye[i][j] + 0.5 * right_sign * sk[i][j] + alpha * sk2[i][j];
        }
    }
    out
}

/// Right-multiplication matrix `q * [0, e_k]` columns `[4 x 3]`: for `q = [w, x, y, z]`,
/// column 0 is `[-x, w, z, -y]`, column 1 is `[-y, -z, w, x]`, column 2 is `[-z, y, -x, w]`.
fn quat_basis_cols(q: [f64; 4]) -> [[f64; 3]; 4] {
    let [w, x, y, z] = q;
    [[-x, -y, -z], [w, -z, y], [z, w, -x], [-y, x, w]]
}

/// Exact analytical energy, 12-DOF left-trivialized gradient, and symmetric
/// 12x12 chart Hessian for a single 6-DOF Cosserat segment.
#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
pub fn evaluate_cosserat_segment<M: DetMath, L: RodSectionLaw>(
    pa: [f64; 3],
    qa: [f64; 4],
    pb: [f64; 3],
    qb: [f64; 4],
    l0: f64,
    rest_gamma: [f64; 3],
    rest_kappa: [f64; 3],
    law: &L,
    committed: &SectionHistory,
) -> Result<CosseratSegmentEval, CodimError> {
    if !all_finite3(pa)
        || !all_finite3(pb)
        || !all_finite4(qa)
        || !all_finite4(qb)
        || !l0.is_finite()
        || !all_finite3(rest_gamma)
        || !all_finite3(rest_kappa)
    {
        return Err(CodimError::NonFiniteInput {
            context: "evaluate_cosserat_segment",
        });
    }
    if l0 <= 1e-15 {
        return Err(CodimError::DegenerateSegment { segment_idx: 0 });
    }
    let qa = quat_normalize::<M>(qa)?;
    let qb = quat_normalize::<M>(qb)?;

    // Hemisphere-aligned qb_tilde so qa . qb_tilde >= 0.
    let dot_ab = qa[0] * qb[0] + qa[1] * qb[1] + qa[2] * qb[2] + qa[3] * qb[3];
    let hemi = if dot_ab >= 0.0 { 1.0 } else { -1.0 };
    let qb_t = [hemi * qb[0], hemi * qb[1], hemi * qb[2], hemi * qb[3]];

    let m_vec = [
        qa[0] + qb_t[0],
        qa[1] + qb_t[1],
        qa[2] + qb_t[2],
        qa[3] + qb_t[3],
    ];
    let s_m = m_vec[0] * m_vec[0] + m_vec[1] * m_vec[1] + m_vec[2] * m_vec[2] + m_vec[3] * m_vec[3];
    let n_m = M::sqrt(s_m);
    if n_m <= 1e-15 {
        return Err(CodimError::AntiparallelTangents { joint_idx: 0 });
    }
    let qm = [
        m_vec[0] / n_m,
        m_vec[1] / n_m,
        m_vec[2] / n_m,
        m_vec[3] / n_m,
    ];
    let r_m = quat_to_mat3(qm);

    let d = scale3(sub3(pb, pa), 1.0 / l0);
    let local = mat3_t_vec(r_m, d);
    let gamma = [
        local[0] - 1.0 - rest_gamma[0],
        local[1] - rest_gamma[1],
        local[2] - rest_gamma[2],
    ];

    let q_rel = quat_mul(quat_conj(qa), qb_t);
    let psi = quat_log::<M>(q_rel);
    let kappa = [
        psi[0] / l0 - rest_kappa[0],
        psi[1] / l0 - rest_kappa[1],
        psi[2] / l0 - rest_kappa[2],
    ];

    let (sec, next_history) = law.evaluate_section::<M>(
        gamma[0],
        [gamma[1], gamma[2]],
        [kappa[1], kappa[2]],
        kappa[0],
        committed,
    )?;

    let energy_j = l0 * sec.energy_density_j_m;
    let f_g = [
        l0 * sec.stress_6[0],
        l0 * sec.stress_6[1],
        l0 * sec.stress_6[2],
    ];
    let f_k = [
        l0 * sec.stress_6[3],
        l0 * sec.stress_6[4],
        l0 * sec.stress_6[5],
    ];

    // Mid-frame angular Jacobians: dphi_m = M_a * dphi_a + M_b * dphi_b.
    let qm_conj = quat_conj(qm);
    let ha = quat_mul(qm_conj, qa);
    let hb = quat_mul(qm_conj, qb_t);
    let inv_n = 1.0 / n_m;
    let ma_skew = skew3([ha[1], ha[2], ha[3]]);
    let mb_skew = skew3([hb[1], hb[2], hb[3]]);
    let mut m_a = [[0.0; 3]; 3];
    let mut m_b = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let diag = if i == j { 1.0 } else { 0.0 };
            m_a[i][j] = inv_n * (ha[0] * diag + ma_skew[i][j]);
            m_b[i][j] = inv_n * (hb[0] * diag + mb_skew[i][j]);
        }
    }

    // Strain Jacobian B (6 x 12) in strain ordering [Gamma_0, Gamma_1, Gamma_2, kappa_0, kappa_1, kappa_2].
    let mut bmat = [[0.0_f64; 12]; 6];
    let inv_l0 = 1.0 / l0;
    let rt_inv_l0 = {
        let rt = mat3_transpose(r_m);
        let mut out = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                out[i][j] = inv_l0 * rt[i][j];
            }
        }
        out
    };
    let sk_local = skew3(local);
    let d_g_d_phia = mat3_mul(sk_local, m_a);
    let d_g_d_phib = mat3_mul(sk_local, m_b);

    let jl_inv = j_inv_so3::<M>(psi, -1.0);
    let jr_inv = j_inv_so3::<M>(psi, 1.0);

    for i in 0..3 {
        for j in 0..3 {
            bmat[i][j] = -rt_inv_l0[i][j];
            bmat[i][3 + j] = d_g_d_phia[i][j];
            bmat[i][6 + j] = rt_inv_l0[i][j];
            bmat[i][9 + j] = d_g_d_phib[i][j];

            bmat[3 + i][3 + j] = -inv_l0 * jl_inv[i][j];
            bmat[3 + i][9 + j] = inv_l0 * jr_inv[i][j];
        }
    }

    let f6 = [f_g[0], f_g[1], f_g[2], f_k[0], f_k[1], f_k[2]];
    let mut grad_12 = [0.0_f64; 12];
    for c in 0..12 {
        let mut acc = 0.0;
        for r in 0..6 {
            acc += bmat[r][c] * f6[r];
        }
        grad_12[c] = acc;
    }

    // Material tangent stiffness K_mat = l0 * B^T * C_6x6 * B.
    // Note SectionStressTangent::tangent_6x6 has ordering [G0, G1, G2, k0, k1, k2].
    let mut cb = [[0.0_f64; 12]; 6];
    for r in 0..6 {
        for c in 0..12 {
            let mut acc = 0.0;
            for k in 0..6 {
                acc += l0 * sec.tangent_6x6[r][k] * bmat[k][c];
            }
            cb[r][c] = acc;
        }
    }
    let mut hess_12 = [[0.0_f64; 12]; 12];
    for i in 0..12 {
        for j in i..12 {
            let mut acc = 0.0;
            for r in 0..6 {
                acc += bmat[r][i] * cb[r][j];
            }
            hess_12[i][j] = acc;
            hess_12[j][i] = acc;
        }
    }

    // Geometric stiffness for Gamma: cross terms (pos <-> rot) and Rayleigh quotient (rot <-> rot).
    let sk_fg = skew3(f_g);
    let r_sk_fg = mat3_mul(r_m, sk_fg);
    let pos_phia = mat3_mul(r_sk_fg, m_a);
    let pos_phib = mat3_mul(r_sk_fg, m_b);
    for i in 0..3 {
        for j in 0..3 {
            let ga_ij = inv_l0 * pos_phia[i][j];
            let gb_ij = inv_l0 * pos_phib[i][j];
            // p_a <-> phi_a (+), p_b <-> phi_a (-)
            hess_12[i][3 + j] += ga_ij;
            hess_12[3 + j][i] += ga_ij;
            hess_12[6 + i][3 + j] -= ga_ij;
            hess_12[3 + j][6 + i] -= ga_ij;
            // p_a <-> phi_b (+), p_b <-> phi_b (-)
            hess_12[i][9 + j] += gb_ij;
            hess_12[9 + j][i] += gb_ij;
            hess_12[6 + i][9 + j] -= gb_ij;
            hess_12[9 + j][6 + i] -= gb_ij;
        }
    }

    // Rayleigh quotient W(m) = (m^T Q m) / (m . m) for f_g . (R_m^T d).
    let alpha0 = dot3(f_g, d);
    let c_vec = cross3(f_g, d);
    let wd = outer3(f_g, d);
    let dw = outer3(d, f_g);
    let mut q_mat = [[0.0_f64; 4]; 4];
    q_mat[0][0] = alpha0;
    for i in 0..3 {
        q_mat[0][1 + i] = c_vec[i];
        q_mat[1 + i][0] = c_vec[i];
        for j in 0..3 {
            let diag = if i == j { -alpha0 } else { 0.0 };
            q_mat[1 + i][1 + j] = diag + wd[i][j] + dw[i][j];
        }
    }
    let mut qm_vec = [0.0_f64; 4];
    for i in 0..4 {
        let mut acc = 0.0;
        for j in 0..4 {
            acc += q_mat[i][j] * m_vec[j];
        }
        qm_vec[i] = acc;
    }
    let m_qm =
        m_vec[0] * qm_vec[0] + m_vec[1] * qm_vec[1] + m_vec[2] * qm_vec[2] + m_vec[3] * qm_vec[3];
    let lam = m_qm / s_m;
    let inv_s2 = 2.0 / s_m;
    let mut grad_m = [0.0_f64; 4];
    for i in 0..4 {
        grad_m[i] = inv_s2 * (qm_vec[i] - lam * m_vec[i]);
    }
    let mut h_mm = [[0.0_f64; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            let diag = if i == j { lam } else { 0.0 };
            h_mm[i][j] =
                inv_s2 * (q_mat[i][j] - diag - m_vec[i] * grad_m[j] - grad_m[i] * m_vec[j]);
        }
    }
    // J_m = [0.5 * cols(qa), 0.5 * cols(qb_t)] (4 x 6).
    let cols_a = quat_basis_cols(qa);
    let cols_b = quat_basis_cols(qb_t);
    let mut j_m = [[0.0_f64; 6]; 4];
    for r in 0..4 {
        for c in 0..3 {
            j_m[r][c] = 0.5 * cols_a[r][c];
            j_m[r][3 + c] = 0.5 * cols_b[r][c];
        }
    }
    let mut h_mm_j = [[0.0_f64; 6]; 4];
    for r in 0..4 {
        for c in 0..6 {
            let mut acc = 0.0;
            for k in 0..4 {
                acc += h_mm[r][k] * j_m[k][c];
            }
            h_mm_j[r][c] = acc;
        }
    }
    let rot_idx = [3usize, 4, 5, 9, 10, 11];
    for i in 0..6 {
        for j in i..6 {
            let mut acc = 0.0;
            for r in 0..4 {
                acc += j_m[r][i] * h_mm_j[r][j];
            }
            hess_12[rot_idx[i]][rot_idx[j]] += acc;
            if i != j {
                hess_12[rot_idx[j]][rot_idx[i]] += acc;
            }
        }
    }
    let gm_dot_qa = grad_m[0] * qa[0] + grad_m[1] * qa[1] + grad_m[2] * qa[2] + grad_m[3] * qa[3];
    let gm_dot_qb =
        grad_m[0] * qb_t[0] + grad_m[1] * qb_t[1] + grad_m[2] * qb_t[2] + grad_m[3] * qb_t[3];
    for c in 0..3 {
        hess_12[3 + c][3 + c] -= 0.25 * gm_dot_qa;
        hess_12[9 + c][9 + c] -= 0.25 * gm_dot_qb;
    }

    // Geometric stiffness for kappa: m_vec = f_k / l0.
    let mv = scale3(f_k, inv_l0);
    let theta = M::sqrt(norm2_3(psi));
    let (alpha_k, beta_k) = d_exp_inv_coeffs::<M>(theta);
    let psi_dot_m = dot3(psi, mv);
    let theta2 = theta * theta;
    let w_psi = sub3(scale3(psi, psi_dot_m), scale3(mv, theta2));
    let sk_m_half = {
        let sk = skew3(mv);
        let mut out = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                out[i][j] = 0.5 * sk[i][j];
            }
        }
        out
    };
    let mut sym_part = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let diag = if i == j { psi_dot_m } else { 0.0 };
            let t1 = alpha_k * (psi[i] * mv[j] + diag - 2.0 * mv[i] * psi[j]);
            let t2 = beta_k * w_psi[i] * psi[j];
            sym_part[i][j] = t1 + t2;
        }
    }
    let mut dga_dpsi = [[0.0_f64; 3]; 3];
    let mut dgb_dpsi = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            dga_dpsi[i][j] = sk_m_half[i][j] - sym_part[i][j];
            dgb_dpsi[i][j] = sk_m_half[i][j] + sym_part[i][j];
        }
    }
    let k_aa = mat3_mul(dga_dpsi, jl_inv);
    let k_ab = mat3_mul(dga_dpsi, jr_inv);
    let k_ba = mat3_mul(dgb_dpsi, jl_inv);
    let k_bb = mat3_mul(dgb_dpsi, jr_inv);
    // Directional derivative blocks:
    // K_k = [[-k_aa, +k_ab], [-k_ba, +k_bb]]. Symmetrize: 0.5 * (K_k + K_k^T).
    let mut k_kappa = [[0.0_f64; 6]; 6];
    for i in 0..3 {
        for j in 0..3 {
            k_kappa[i][j] = -k_aa[i][j];
            k_kappa[i][3 + j] = k_ab[i][j];
            k_kappa[3 + i][j] = -k_ba[i][j];
            k_kappa[3 + i][3 + j] = k_bb[i][j];
        }
    }
    for i in 0..6 {
        for j in i..6 {
            let sym_ij = 0.5 * (k_kappa[i][j] + k_kappa[j][i]);
            hess_12[rot_idx[i]][rot_idx[j]] += sym_ij;
            if i != j {
                hess_12[rot_idx[j]][rot_idx[i]] += sym_ij;
            }
        }
    }

    Ok(CosseratSegmentEval {
        gamma,
        kappa,
        energy_j,
        grad_12,
        hess_12,
        next_history,
    })
}

/// Evaluate total internal strain energy of a [`CosseratRod`] at given committed
/// histories without mutating them.
pub fn rod_energy<M: DetMath>(
    rod: &CosseratRod,
    histories: &[SectionHistory],
) -> Result<f64, CodimError> {
    rod.validate::<M>()?;
    if histories.len() != rod.segment_count() {
        return Err(CodimError::InvalidTopology {
            detail: "histories length must equal CosseratRod segment count",
        });
    }
    let mut total = 0.0;
    for seg in 0..rod.segment_count() {
        let (g, k) = rod.segment_strains::<M>(seg)?;
        let (sec, _) = rod.sections[seg].evaluate_section::<M>(
            g[0],
            [g[1], g[2]],
            [k[1], k[2]],
            k[0],
            &histories[seg],
        )?;
        total += rod.rest_lengths_m[seg] * sec.energy_density_j_m;
    }
    Ok(total)
}
