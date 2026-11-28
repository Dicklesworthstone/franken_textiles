//! 6-DOF `SO(3)` [`JunctionNode`] coupling for degree != 2 branching vertices
//! (mixed Cosserat and DER arms) and `fs-mbd` rigid payload clamps, conserving
//! linear force and angular momentum across junctions.

use super::cosserat::CosseratRod;
use super::der::DerCurve;
use super::{
    CodimError, DetMath, all_finite3, all_finite4, cross3, dot3, eye3, mat3_mul, mat3_t_vec,
    mat3_transpose, mat3_vec, norm2_3, norm3, quat_conj, quat_log, quat_mul, quat_normalize,
    quat_to_mat3, scale3, skew3, sub3,
};

/// Which end of a [`DerCurve`] attaches to a [`JunctionNode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveEnd {
    /// Start of the curve (edge `0`, node `0`..`1`).
    Start,
    /// End of an open curve (last edge `E - 1`, tip node `N - 1`).
    End,
}

/// Target rod arm endpoint coupled to a [`JunctionNode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RodArmKind {
    /// Attach to a node of a 6-DOF Cosserat rod.
    Cosserat {
        /// Index into `RodNetwork::cosserat_rods`.
        rod_idx: usize,
        /// Node index on that rod.
        node_idx: usize,
    },
    /// Attach to an end of an open 4-DOF DER curve.
    Der {
        /// Index into `RodNetwork::der_curves`.
        curve_idx: usize,
        /// Start or end of that curve.
        end: CurveEnd,
    },
}

/// One incident arm attached to a [`JunctionNode`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JunctionArm {
    /// Attached rod endpoint.
    pub kind: RodArmKind,
    /// Rigid offset of the arm attachment point in the junction's body frame (`m`).
    pub local_offset_m: [f64; 3],
    /// Fixed unit quaternion `[w, x, y, z]` rotating from junction frame to arm frame.
    pub q_offset_wxyz: [f64; 4],
    /// Translational coupling stiffness (`N/m`, `>= 0.0`).
    pub k_trans_n_m: f64,
    /// Rotational coupling stiffness (`N*m/rad`, `>= 0.0`).
    pub k_rot_nm: f64,
}

/// 6-DOF rigid `SO(3)` junction node for degree != 2 branching and rigid clamps.
#[derive(Clone, Debug, PartialEq)]
pub struct JunctionNode {
    /// Junction center position `x_J` in metres.
    pub position: [f64; 3],
    /// Junction body-to-world unit quaternion `[w, x, y, z]`.
    pub q_wxyz: [f64; 4],
    /// Incident coupled rod arms.
    pub arms: Vec<JunctionArm>,
}

/// Global DOF layout mapping a [`JunctionNode`] and its attached arms into the
/// unified network vector.
#[derive(Clone, Debug)]
pub struct NetworkDofLayout {
    /// Starting global DOF index for each Cosserat rod (`6 * node_count` DOFs).
    pub cosserat_offsets: Vec<usize>,
    /// Starting global DOF index for each DER curve (`3 * N + E` DOFs).
    pub der_offsets: Vec<usize>,
    /// Starting global DOF index for each junction (`6` DOFs: `[x(3), phi(3)]`).
    pub junction_offsets: Vec<usize>,
    /// Total global DOFs.
    pub total_dofs: usize,
}

impl JunctionNode {
    /// Validate junction coordinates, unit quaternion, and arm parameters.
    pub fn validate<M: DetMath>(&self) -> Result<(), CodimError> {
        if !all_finite3(self.position) || !all_finite4(self.q_wxyz) {
            return Err(CodimError::NonFiniteInput {
                context: "JunctionNode",
            });
        }
        let _ = quat_normalize::<M>(self.q_wxyz)?;
        if self.arms.is_empty() {
            return Err(CodimError::InvalidTopology {
                detail: "JunctionNode must attach at least one arm",
            });
        }
        for arm in &self.arms {
            if !all_finite3(arm.local_offset_m)
                || !all_finite4(arm.q_offset_wxyz)
                || !arm.k_trans_n_m.is_finite()
                || !arm.k_rot_nm.is_finite()
            {
                return Err(CodimError::NonFiniteInput {
                    context: "JunctionArm",
                });
            }
            let _ = quat_normalize::<M>(arm.q_offset_wxyz)?;
            if arm.k_trans_n_m < 0.0 || arm.k_rot_nm < 0.0 {
                return Err(CodimError::InvalidParameter {
                    detail: "JunctionArm stiffnesses must be non-negative",
                });
            }
        }
        Ok(())
    }

    /// Apply `[dx, dy, dz, dphi_x, dphi_y, dphi_z]` scaled by `alpha`.
    pub fn apply_increment<M: DetMath>(
        &mut self,
        delta: &[f64; 6],
        alpha: f64,
    ) -> Result<(), CodimError> {
        self.position[0] += alpha * delta[0];
        self.position[1] += alpha * delta[1];
        self.position[2] += alpha * delta[2];
        let dq = super::quat_exp::<M>([alpha * delta[3], alpha * delta[4], alpha * delta[5]]);
        self.q_wxyz = quat_normalize::<M>(quat_mul(self.q_wxyz, dq))?;
        Ok(())
    }
}

/// Result of evaluating all [`JunctionNode`]s in a network: energy, additive
/// global gradient contributions, and symmetric Hessian triplets.
#[derive(Clone, Debug, Default)]
pub struct JunctionEval {
    /// Total junction coupling strain energy (`J`).
    pub energy_j: f64,
    /// Global gradient contributions (length `total_dofs`).
    pub grad: Vec<f64>,
    /// Symmetric Hessian triplets `(row, col, val)`.
    pub hess_triplets: Vec<(usize, usize, f64)>,
}

/// Evaluate all [`JunctionNode`] couplings against the provided Cosserat rods
/// and DER curves.
#[allow(clippy::too_many_lines)]
pub fn evaluate_junctions<M: DetMath>(
    junctions: &[JunctionNode],
    cosserat_rods: &[CosseratRod],
    der_curves: &[DerCurve],
    layout: &NetworkDofLayout,
) -> Result<JunctionEval, CodimError> {
    let mut energy_j = 0.0;
    let mut grad = vec![0.0_f64; layout.total_dofs];
    let mut hess_triplets: Vec<(usize, usize, f64)> = Vec::new();

    for (j_idx, junc) in junctions.iter().enumerate() {
        junc.validate::<M>()?;
        let j_dof = layout.junction_offsets[j_idx];
        let q_j = quat_normalize::<M>(junc.q_wxyz)?;
        let r_j = quat_to_mat3(q_j);

        for arm in &junc.arms {
            let q_off = quat_normalize::<M>(arm.q_offset_wxyz)?;
            let r_off = quat_to_mat3(q_off);

            let (arm_pos, arm_pos_dof) = match arm.kind {
                RodArmKind::Cosserat { rod_idx, node_idx } => {
                    let rod = cosserat_rods
                        .get(rod_idx)
                        .ok_or(CodimError::InvalidTopology {
                            detail: "JunctionArm Cosserat rod_idx out of range",
                        })?;
                    if node_idx >= rod.positions.len() {
                        return Err(CodimError::InvalidTopology {
                            detail: "JunctionArm Cosserat node_idx out of range",
                        });
                    }
                    let dof = layout.cosserat_offsets[rod_idx] + 6 * node_idx;
                    (rod.positions[node_idx], dof)
                }
                RodArmKind::Der { curve_idx, end } => {
                    let curve = der_curves
                        .get(curve_idx)
                        .ok_or(CodimError::InvalidTopology {
                            detail: "JunctionArm Der curve_idx out of range",
                        })?;
                    if curve.closed {
                        return Err(CodimError::InvalidTopology {
                            detail: "JunctionArm cannot attach end of a closed DER curve",
                        });
                    }
                    let node_idx = match end {
                        CurveEnd::Start => 0,
                        CurveEnd::End => curve.positions.len() - 1,
                    };
                    let dof = layout.der_offsets[curve_idx] + 3 * node_idx;
                    (curve.positions[node_idx], dof)
                }
            };

            // 1. Translational coupling: E = 0.5 * k_t * ||x_A - (x_J + R_J * r_loc)||^2.
            if arm.k_trans_n_m > 0.0 {
                let r_loc = arm.local_offset_m;
                let r_w = mat3_vec(r_j, r_loc);
                let e_t = sub3(
                    arm_pos,
                    [
                        junc.position[0] + r_w[0],
                        junc.position[1] + r_w[1],
                        junc.position[2] + r_w[2],
                    ],
                );
                energy_j += 0.5 * arm.k_trans_n_m * norm2_3(e_t);
                let f = scale3(e_t, arm.k_trans_n_m);
                let f_loc = mat3_t_vec(r_j, f);
                let g_phi = cross3(r_loc, scale3(f_loc, -1.0));
                for c in 0..3 {
                    grad[arm_pos_dof + c] += f[c];
                    grad[j_dof + c] -= f[c];
                    grad[j_dof + 3 + c] += g_phi[c];
                }
                // Jacobian B_t = [I_3 (arm), -I_3 (j_pos), R_J [r_loc]_x (j_phi)].
                let j_phi = mat3_mul(r_j, skew3(r_loc));
                let kt = arm.k_trans_n_m;
                let dofs9 = [
                    arm_pos_dof,
                    arm_pos_dof + 1,
                    arm_pos_dof + 2,
                    j_dof,
                    j_dof + 1,
                    j_dof + 2,
                    j_dof + 3,
                    j_dof + 4,
                    j_dof + 5,
                ];
                let mut bt = [[0.0_f64; 9]; 3];
                for r in 0..3 {
                    bt[r][r] = 1.0;
                    bt[r][3 + r] = -1.0;
                    for c in 0..3 {
                        bt[r][6 + c] = j_phi[r][c];
                    }
                }
                for i in 0..9 {
                    for j_col in i..9 {
                        let mut acc = 0.0;
                        for r in 0..3 {
                            acc += kt * bt[r][i] * bt[r][j_col];
                        }
                        if acc != 0.0 {
                            hess_triplets.push((dofs9[i], dofs9[j_col], acc));
                            if i != j_col {
                                hess_triplets.push((dofs9[j_col], dofs9[i], acc));
                            }
                        }
                    }
                }
                // Geometric term on j_phi x j_phi:
                let fr = dot3(f_loc, r_loc);
                for i in 0..3 {
                    for j_col in i..3 {
                        let diag = if i == j_col { fr } else { 0.0 };
                        let gval = diag - 0.5 * (f_loc[i] * r_loc[j_col] + r_loc[i] * f_loc[j_col]);
                        if gval != 0.0 {
                            hess_triplets.push((j_dof + 3 + i, j_dof + 3 + j_col, gval));
                            if i != j_col {
                                hess_triplets.push((j_dof + 3 + j_col, j_dof + 3 + i, gval));
                            }
                        }
                    }
                }
            }

            // 2. Rotational coupling.
            if arm.k_rot_nm > 0.0 {
                match arm.kind {
                    RodArmKind::Cosserat { rod_idx, node_idx } => {
                        let rod = &cosserat_rods[rod_idx];
                        let q_arm = quat_normalize::<M>(rod.quats[node_idx])?;
                        let q_tgt = quat_mul(q_j, q_off);
                        let dot_ta = q_tgt[0] * q_arm[0]
                            + q_tgt[1] * q_arm[1]
                            + q_tgt[2] * q_arm[2]
                            + q_tgt[3] * q_arm[3];
                        let hemi = if dot_ta >= 0.0 { 1.0 } else { -1.0 };
                        let q_arm_t = [
                            hemi * q_arm[0],
                            hemi * q_arm[1],
                            hemi * q_arm[2],
                            hemi * q_arm[3],
                        ];
                        let q_rel = quat_mul(quat_conj(q_tgt), q_arm_t);
                        let psi = quat_log::<M>(q_rel);
                        let kr = arm.k_rot_nm;
                        energy_j += 0.5 * kr * norm2_3(psi);

                        let (g6, h6) = so3_log_energy_derivs::<M>(psi, kr);
                        // Transform target frame variation delta_phi_tgt = R_off^T * delta_phi_J.
                        let rt_off = mat3_transpose(r_off);
                        let arm_rot_dof = layout.cosserat_offsets[rod_idx] + 6 * node_idx + 3;
                        let g_jphi = mat3_vec(r_off, [g6[0], g6[1], g6[2]]);
                        for c in 0..3 {
                            grad[j_dof + 3 + c] += g_jphi[c];
                            grad[arm_rot_dof + c] += g6[3 + c];
                        }

                        // Map 6x6 Hessian under diag(R_off^T, I_3).
                        let mut xform = [[0.0_f64; 6]; 6];
                        for i in 0..3 {
                            for j_col in 0..3 {
                                xform[i][j_col] = rt_off[i][j_col];
                            }
                            xform[3 + i][3 + i] = 1.0;
                        }
                        let mut hx = [[0.0_f64; 6]; 6];
                        for r in 0..6 {
                            for c in 0..6 {
                                let mut acc = 0.0;
                                for k in 0..6 {
                                    acc += h6[r][k] * xform[k][c];
                                }
                                hx[r][c] = acc;
                            }
                        }
                        let dofs6 = [
                            j_dof + 3,
                            j_dof + 4,
                            j_dof + 5,
                            arm_rot_dof,
                            arm_rot_dof + 1,
                            arm_rot_dof + 2,
                        ];
                        for i in 0..6 {
                            for j_col in i..6 {
                                let mut acc = 0.0;
                                for r in 0..6 {
                                    acc += xform[r][i] * hx[r][j_col];
                                }
                                if acc != 0.0 {
                                    hess_triplets.push((dofs6[i], dofs6[j_col], acc));
                                    if i != j_col {
                                        hess_triplets.push((dofs6[j_col], dofs6[i], acc));
                                    }
                                }
                            }
                        }
                    }
                    RodArmKind::Der { curve_idx, end } => {
                        let curve = &der_curves[curve_idx];
                        let e_count = curve.edge_count();
                        let (e_idx, n_a, n_b) = match end {
                            CurveEnd::Start => (0usize, 0usize, 1usize),
                            CurveEnd::End => (
                                e_count - 1,
                                curve.positions.len() - 2,
                                curve.positions.len() - 1,
                            ),
                        };
                        let d_vec = sub3(curve.positions[n_b], curve.positions[n_a]);
                        let l = norm3::<M>(d_vec);
                        if !l.is_finite() || l <= 1e-15 {
                            return Err(CodimError::DegenerateSegment { segment_idx: e_idx });
                        }
                        let t = scale3(d_vec, 1.0 / l);
                        if 1.0 + dot3(curve.ref_tangents[e_idx], t) <= 1e-12 {
                            return Err(CodimError::AntiparallelTangents { joint_idx: j_idx });
                        }

                        let q_tgt = quat_mul(q_j, q_off);
                        let r_arm = quat_to_mat3(q_tgt);
                        let (e_rot, g10, h10) = eval_der_junction_frame::<M>(
                            t,
                            curve.ref_tangents[e_idx],
                            curve.ref_normals_u[e_idx],
                            curve.edge_twists_rad[e_idx],
                            l,
                            r_arm,
                            r_off,
                            arm.k_rot_nm,
                        );
                        energy_j += e_rot;
                        let base = layout.der_offsets[curve_idx];
                        let th_dof = base + 3 * curve.positions.len() + e_idx;
                        let dofs10 = [
                            base + 3 * n_a,
                            base + 3 * n_a + 1,
                            base + 3 * n_a + 2,
                            base + 3 * n_b,
                            base + 3 * n_b + 1,
                            base + 3 * n_b + 2,
                            th_dof,
                            j_dof + 3,
                            j_dof + 4,
                            j_dof + 5,
                        ];
                        for i in 0..10 {
                            grad[dofs10[i]] += g10[i];
                            for col in 0..10 {
                                let val = h10[i][col];
                                if val != 0.0 {
                                    hess_triplets.push((dofs10[i], dofs10[col], val));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(JunctionEval {
        energy_j,
        grad,
        hess_triplets,
    })
}

fn d_exp_inv_coeffs<M: DetMath>(theta: f64) -> (f64, f64) {
    let t2 = theta * theta;
    if theta < 1e-3 {
        let t4 = t2 * t2;
        (
            1.0 / 12.0 + t2 / 720.0 + t4 / 30240.0,
            1.0 / 360.0 + t2 / 7560.0 + t4 / 201600.0,
        )
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

/// Exact gradient and symmetric chart Hessian of `0.5 * k_rot * ||psi||^2`
/// wrt `[dphi_a, dphi_b]` where `psi = log(q_a^{-1} * q_b)`.
fn so3_log_energy_derivs<M: DetMath>(psi: [f64; 3], k_rot: f64) -> ([f64; 6], [[f64; 6]; 6]) {
    let jl_inv = j_inv_so3::<M>(psi, -1.0);
    let jr_inv = j_inv_so3::<M>(psi, 1.0);
    let mv = scale3(psi, k_rot);

    // Since J_l^{-1}(psi)^T psi = psi and J_r^{-1}(psi)^T psi = psi identically!
    // Still compute via explicit J matrices for generality.
    let ga = mat3_t_vec(jl_inv, scale3(mv, -1.0));
    let gb = mat3_t_vec(jr_inv, mv);
    let grad = [ga[0], ga[1], ga[2], gb[0], gb[1], gb[2]];

    // Material Hessian k_rot * B^T B, B = [-J_l^{-1}, J_r^{-1}].
    let mut b6 = [[0.0_f64; 6]; 3];
    for i in 0..3 {
        for j in 0..3 {
            b6[i][j] = -jl_inv[i][j];
            b6[i][3 + j] = jr_inv[i][j];
        }
    }
    let mut hess = [[0.0_f64; 6]; 6];
    for i in 0..6 {
        for j in i..6 {
            let mut acc = 0.0;
            for r in 0..3 {
                acc += k_rot * b6[r][i] * b6[r][j];
            }
            hess[i][j] = acc;
            hess[j][i] = acc;
        }
    }

    // Geometric Hessian for mv . psi (same exact formula as Cosserat).
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
            sym_part[i][j] = alpha_k * (psi[i] * mv[j] + diag - 2.0 * mv[i] * psi[j])
                + beta_k * w_psi[i] * psi[j];
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
    let mut k_geom = [[0.0_f64; 6]; 6];
    for i in 0..3 {
        for j in 0..3 {
            k_geom[i][j] = -k_aa[i][j];
            k_geom[i][3 + j] = k_ab[i][j];
            k_geom[3 + i][j] = -k_ba[i][j];
            k_geom[3 + i][3 + j] = k_bb[i][j];
        }
    }
    for i in 0..6 {
        for j in i..6 {
            let s = 0.5 * (k_geom[i][j] + k_geom[j][i]);
            hess[i][j] += s;
            if i != j {
                hess[j][i] += s;
            }
        }
    }

    (grad, hess)
}

/// Evaluate `E = k_rot * (3 - t . d0 - m1 . d1 - m2 . d2)` and its exact
/// gradient and symmetric chart Hessian wrt `[x_a(3), x_b(3), theta_e(1), dphi_J(3)]`.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn eval_der_junction_frame<M: DetMath>(
    t: [f64; 3],
    t_ref: [f64; 3],
    u_ref: [f64; 3],
    theta_e: f64,
    l: f64,
    r_arm: [[f64; 3]; 3],
    r_off: [[f64; 3]; 3],
    k_rot: f64,
) -> (f64, [f64; 10], [[f64; 10]; 10]) {
    // 7 intermediate variables: z = [t_0, t_1, t_2, theta_e, psi_arm_0, psi_arm_1, psi_arm_2]
    // where d_k(psi_arm) = R_arm * (I + [psi] + 0.5 [psi]^2) e_k.
    // Then psi_arm = R_off^T * dphi_J.
    #[derive(Clone, Copy)]
    struct J7 {
        v: f64,
        g: [f64; 7],
        h: [[f64; 7]; 7],
    }
    impl J7 {
        fn c(v: f64) -> Self {
            Self {
                v,
                g: [0.0; 7],
                h: [[0.0; 7]; 7],
            }
        }
        fn var(v: f64, idx: usize) -> Self {
            let mut g = [0.0; 7];
            g[idx] = 1.0;
            Self {
                v,
                g,
                h: [[0.0; 7]; 7],
            }
        }
        fn add(self, o: Self) -> Self {
            let mut g = [0.0; 7];
            let mut h = [[0.0; 7]; 7];
            for i in 0..7 {
                g[i] = self.g[i] + o.g[i];
                for j in i..7 {
                    let s = self.h[i][j] + o.h[i][j];
                    h[i][j] = s;
                    h[j][i] = s;
                }
            }
            Self {
                v: self.v + o.v,
                g,
                h,
            }
        }
        fn sub(self, o: Self) -> Self {
            let mut g = [0.0; 7];
            let mut h = [[0.0; 7]; 7];
            for i in 0..7 {
                g[i] = self.g[i] - o.g[i];
                for j in i..7 {
                    let s = self.h[i][j] - o.h[i][j];
                    h[i][j] = s;
                    h[j][i] = s;
                }
            }
            Self {
                v: self.v - o.v,
                g,
                h,
            }
        }
        fn scale(self, s: f64) -> Self {
            let mut g = [0.0; 7];
            let mut h = [[0.0; 7]; 7];
            for i in 0..7 {
                g[i] = s * self.g[i];
                for j in i..7 {
                    let v = s * self.h[i][j];
                    h[i][j] = v;
                    h[j][i] = v;
                }
            }
            Self {
                v: s * self.v,
                g,
                h,
            }
        }
        fn mul(self, o: Self) -> Self {
            let mut g = [0.0; 7];
            let mut h = [[0.0; 7]; 7];
            for i in 0..7 {
                g[i] = self.g[i] * o.v + self.v * o.g[i];
                for j in i..7 {
                    let v = self.h[i][j] * o.v
                        + self.g[i] * o.g[j]
                        + self.g[j] * o.g[i]
                        + self.v * o.h[i][j];
                    h[i][j] = v;
                    h[j][i] = v;
                }
            }
            Self {
                v: self.v * o.v,
                g,
                h,
            }
        }
        fn unary(self, f: f64, df: f64, d2f: f64) -> Self {
            let mut g = [0.0; 7];
            let mut h = [[0.0; 7]; 7];
            for i in 0..7 {
                g[i] = df * self.g[i];
                for j in i..7 {
                    let v = df * self.h[i][j] + d2f * self.g[i] * self.g[j];
                    h[i][j] = v;
                    h[j][i] = v;
                }
            }
            Self { v: f, g, h }
        }
        fn div(self, o: Self) -> Self {
            let inv = 1.0 / o.v;
            let inv2 = inv * inv;
            self.mul(o.unary(inv, -inv2, 2.0 * inv2 * inv))
        }
    }

    let jt = [J7::var(t[0], 0), J7::var(t[1], 1), J7::var(t[2], 2)];
    let jth = J7::var(theta_e, 3);
    let jp = [J7::var(0.0, 4), J7::var(0.0, 5), J7::var(0.0, 6)];

    let tref_j = [J7::c(t_ref[0]), J7::c(t_ref[1]), J7::c(t_ref[2])];
    let uref_j = [J7::c(u_ref[0]), J7::c(u_ref[1]), J7::c(u_ref[2])];
    let denom = J7::c(1.0).add(
        tref_j[0]
            .mul(jt[0])
            .add(tref_j[1].mul(jt[1]))
            .add(tref_j[2].mul(jt[2])),
    );
    let num = uref_j[0]
        .mul(jt[0])
        .add(uref_j[1].mul(jt[1]))
        .add(uref_j[2].mul(jt[2]));
    let coeff = num.div(denom);
    let u = [
        uref_j[0].sub(coeff.mul(tref_j[0].add(jt[0]))),
        uref_j[1].sub(coeff.mul(tref_j[1].add(jt[1]))),
        uref_j[2].sub(coeff.mul(tref_j[2].add(jt[2]))),
    ];
    let v = [
        jt[1].mul(u[2]).sub(jt[2].mul(u[1])),
        jt[2].mul(u[0]).sub(jt[0].mul(u[2])),
        jt[0].mul(u[1]).sub(jt[1].mul(u[0])),
    ];
    let cv = M::cos(jth.v);
    let sv = M::sin(jth.v);
    let ct = jth.unary(cv, -sv, -cv);
    let st = jth.unary(sv, cv, -sv);
    let mut m1 = [J7::c(0.0); 3];
    let mut m2 = [J7::c(0.0); 3];
    for c in 0..3 {
        m1[c] = ct.mul(u[c]).add(st.mul(v[c]));
        m2[c] = ct.mul(v[c]).sub(st.mul(u[c]));
    }

    // R_exp(jp) = I + [jp] + 0.5 [jp]^2.
    let p_skew = [
        [J7::c(0.0), jp[2].scale(-1.0), jp[1]],
        [jp[2], J7::c(0.0), jp[0].scale(-1.0)],
        [jp[1].scale(-1.0), jp[0], J7::c(0.0)],
    ];
    let mut r_exp = [[J7::c(0.0); 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let eye = if i == j { J7::c(1.0) } else { J7::c(0.0) };
            let p2 = p_skew[i][0]
                .mul(p_skew[0][j])
                .add(p_skew[i][1].mul(p_skew[1][j]))
                .add(p_skew[i][2].mul(p_skew[2][j]));
            r_exp[i][j] = eye.add(p_skew[i][j]).add(p2.scale(0.5));
        }
    }
    let mut d_cols = [[J7::c(0.0); 3]; 3];
    for k in 0..3 {
        for r in 0..3 {
            d_cols[k][r] = J7::c(r_arm[r][0])
                .mul(r_exp[0][k])
                .add(J7::c(r_arm[r][1]).mul(r_exp[1][k]))
                .add(J7::c(r_arm[r][2]).mul(r_exp[2][k]));
        }
    }
    let dot_j =
        |a: [J7; 3], b: [J7; 3]| -> J7 { a[0].mul(b[0]).add(a[1].mul(b[1])).add(a[2].mul(b[2])) };
    let tr = dot_j(jt, d_cols[0])
        .add(dot_j(m1, d_cols[1]))
        .add(dot_j(m2, d_cols[2]));
    let e_jet = J7::c(3.0).sub(tr).scale(k_rot);

    // Chain rule from z7 = [t(x_a, x_b), theta_e, R_off^T dphi_J] to [x_a(3), x_b(3), theta_e(1), dphi_J(3)].
    let inv_l = 1.0 / l;
    let mut jt_mat = [[0.0_f64; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            let diag = if r == c { 1.0 } else { 0.0 };
            jt_mat[r][c] = inv_l * (diag - t[r] * t[c]);
        }
    }
    let rt_off = mat3_transpose(r_off);
    let mut bz = [[0.0_f64; 10]; 7];
    for r in 0..3 {
        for c in 0..3 {
            bz[r][c] = -jt_mat[r][c];
            bz[r][3 + c] = jt_mat[r][c];
            bz[4 + r][7 + c] = rt_off[r][c];
        }
    }
    bz[3][6] = 1.0;

    let mut g10 = [0.0_f64; 10];
    for c in 0..10 {
        let mut acc = 0.0;
        for r in 0..7 {
            acc += bz[r][c] * e_jet.g[r];
        }
        g10[c] = acc;
    }

    let mut h7_bz = [[0.0_f64; 10]; 7];
    for r in 0..7 {
        for c in 0..10 {
            let mut acc = 0.0;
            for k in 0..7 {
                acc += e_jet.h[r][k] * bz[k][c];
            }
            h7_bz[r][c] = acc;
        }
    }
    let mut h10 = [[0.0_f64; 10]; 10];
    for i in 0..10 {
        for j_col in i..10 {
            let mut acc = 0.0;
            for r in 0..7 {
                acc += bz[r][i] * h7_bz[r][j_col];
            }
            h10[i][j_col] = acc;
            h10[j_col][i] = acc;
        }
    }

    // Add d^2(t)/d(e)^2 normalization curvature on (x_a, x_b).
    let g_t = [e_jet.g[0], e_jet.g[1], e_jet.g[2]];
    let gt = dot3(g_t, t);
    let inv_l2 = inv_l * inv_l;
    for p in 0..3 {
        for q in 0..3 {
            let diag = if p == q { gt } else { 0.0 };
            let val = (-g_t[p] * t[q] - g_t[q] * t[p] - diag + 3.0 * gt * t[p] * t[q]) * inv_l2;
            h10[p][q] += val;
            h10[3 + p][3 + q] += val;
            h10[p][3 + q] -= val;
            h10[3 + p][q] -= val;
        }
    }

    (e_jet.v, g10, h10)
}
