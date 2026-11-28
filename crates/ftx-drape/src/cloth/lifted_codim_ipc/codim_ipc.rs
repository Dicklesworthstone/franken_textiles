//! Codimensional IPC barriers, Li et al. (2021) rest-scaled `C^2` cubic
//! mollifier, exact analytical `12x12` gradients/Hessians, deterministic
//! `12x12` Jacobi SPD projection, and additive physical [`NormalContactLaw`]
//! implementations (strictly distinct from `ConstraintBarrierReceipt`).

use super::{CodimContactError, DetMath, all_finite3, cross3, dot3, norm2_3, scale3, sub3};

/// 1D IPC logarithmic gap barrier:
/// `b(g; dhat) = -kappa * (g - dhat)^2 * ln(g / dhat)` for `0 < g < dhat`,
/// `0.0` for `g >= dhat`, and divergent as `g -> 0+`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScalarBarrier {
    /// Barrier stiffness `kappa > 0` (SI energy-per-area units).
    pub kappa: f64,
    /// Activation gap `dhat > 0` in metres.
    pub dhat: f64,
}

impl ScalarBarrier {
    /// Validate that `kappa` and `dhat` are finite and strictly positive.
    ///
    /// # Errors
    /// Returns [`CodimContactError::NonFiniteInput`] or
    /// [`CodimContactError::InvalidParameter`] if either parameter is invalid.
    pub fn validate(&self) -> Result<(), CodimContactError> {
        if !self.kappa.is_finite() {
            return Err(CodimContactError::NonFiniteInput {
                context: "ScalarBarrier::kappa",
            });
        }
        if self.kappa <= 0.0 {
            return Err(CodimContactError::InvalidParameter {
                detail: "ScalarBarrier kappa must be strictly positive",
            });
        }
        if !self.dhat.is_finite() {
            return Err(CodimContactError::NonFiniteInput {
                context: "ScalarBarrier::dhat",
            });
        }
        if self.dhat <= 0.0 {
            return Err(CodimContactError::InvalidParameter {
                detail: "ScalarBarrier dhat must be strictly positive",
            });
        }
        Ok(())
    }

    /// Evaluate `b(g; dhat)` using deterministic [`DetMath::ln`].
    #[must_use]
    pub fn value<M: DetMath>(&self, g: f64) -> f64 {
        if g >= self.dhat {
            return 0.0;
        }
        if !g.is_finite() || g <= 0.0 {
            return f64::INFINITY;
        }
        let r = g - self.dhat;
        -self.kappa * r * r * M::ln(g / self.dhat)
    }

    /// Evaluate first derivative `b'(g; dhat)` using deterministic [`DetMath::ln`].
    #[must_use]
    pub fn d1<M: DetMath>(&self, g: f64) -> f64 {
        if g >= self.dhat {
            return 0.0;
        }
        if !g.is_finite() || g <= 0.0 {
            return f64::NAN;
        }
        let r = g - self.dhat;
        let l = M::ln(g / self.dhat);
        -self.kappa * (2.0 * r * l + r * r / g)
    }

    /// Evaluate second derivative `b''(g; dhat)` using deterministic [`DetMath::ln`].
    #[must_use]
    pub fn d2<M: DetMath>(&self, g: f64) -> f64 {
        if g >= self.dhat {
            return 0.0;
        }
        if !g.is_finite() || g <= 0.0 {
            return f64::NAN;
        }
        let r = g - self.dhat;
        let l = M::ln(g / self.dhat);
        -self.kappa * ((2.0 * l + 4.0 * r / g) - r * r / (g * g))
    }
}

/// Active closest-feature classification for a 4-vertex stencil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveKind {
    /// Edge-edge pair `([p0, p1], [p2, p3])`.
    EdgeEdge,
    /// Point-triangle pair `(p0, [p1, p2, p3])`.
    PointTriangle,
    /// Point-edge pair `(p0, [p1, p2])` (`p3` unused).
    PointEdge,
    /// Point-point pair `(p0, p1)` (`p2, p3` unused).
    PointPoint,
}

/// Output of evaluating a codimensional C-IPC pair potential and its exact
/// `12x12` analytical derivatives + Jacobi SPD-projected Hessian.
#[derive(Clone, Debug, PartialEq)]
pub struct CipcEval {
    /// Primitive stencil classification.
    pub kind: PrimitiveKind,
    /// Unsigned centerline Euclidean distance `d` in metres.
    pub distance_m: f64,
    /// Effective gap `g = d - d_offset_pair` in metres.
    pub gap_m: f64,
    /// Li et al. (2021) cubic mollifier value (`1.0` for non-EE primitives).
    pub mollifier: f64,
    /// Potential energy `V = m * b(g)` in Joules.
    pub energy_j: f64,
    /// Exact analytical `12`-DOF gradient `dV/dq`.
    pub grad: [f64; 12],
    /// Exact analytical `12x12` symmetric Hessian `d^2V/dq^2`.
    pub hess: [[f64; 12]; 12],
    /// Deterministic Jacobi SPD-projected `12x12` Hessian (`H_spd >= 0`).
    pub hess_spd: [[f64; 12]; 12],
}

/// Compute Li et al. (2021) rest-scaled mollifier threshold:
/// `m_bar = 1e-3 * ||e_a0||^2 * ||e_b0||^2`.
///
/// # Errors
/// Returns [`CodimContactError::NonFiniteInput`] or
/// [`CodimContactError::DegenerateRestEdge`] if either rest edge has zero or
/// non-finite length.
pub fn rest_mollifier_threshold(
    a0: [f64; 3],
    a1: [f64; 3],
    b0: [f64; 3],
    b1: [f64; 3],
) -> Result<f64, CodimContactError> {
    if !all_finite3(a0) || !all_finite3(a1) || !all_finite3(b0) || !all_finite3(b1) {
        return Err(CodimContactError::NonFiniteInput {
            context: "rest_mollifier_threshold",
        });
    }
    let ea0 = sub3(a1, a0);
    let eb0 = sub3(b1, b0);
    let la2 = norm2_3(ea0);
    let lb2 = norm2_3(eb0);
    let m_bar = 1e-3 * la2 * lb2;
    if !m_bar.is_finite() || m_bar <= 0.0 {
        return Err(CodimContactError::DegenerateRestEdge {
            detail: "rest edge squared-length product must be strictly positive",
        });
    }
    Ok(m_bar)
}

/// Scalar value, `12`-DOF gradient, and `12x12` Hessian tuple for a 4-vertex
/// stencil.
pub type StencilDerivatives12 = (f64, [f64; 12], [[f64; 12]; 12]);

/// Evaluate the `C^2` cubic mollifier `m(u) = min(1, 3u - 3u^2 + u^3)` and its
/// exact `12`-DOF gradient and `12x12` Hessian for edge-edge stencil
/// `q = [p0, p1, p2, p3]`.
///
/// # Errors
/// Returns [`CodimContactError::DegenerateRestEdge`] if `m_bar <= 0` or
/// non-finite.
pub fn ee_mollifier_12(
    q: [[f64; 3]; 4],
    m_bar: f64,
) -> Result<StencilDerivatives12, CodimContactError> {
    if !m_bar.is_finite() || m_bar <= 0.0 {
        return Err(CodimContactError::DegenerateRestEdge {
            detail: "m_bar must be finite and strictly positive",
        });
    }
    let ea = sub3(q[1], q[0]);
    let eb = sub3(q[3], q[2]);
    let a = norm2_3(ea);
    let b = norm2_3(eb);
    let c_dot = dot3(ea, eb);
    let c_val = norm2_3(cross3(ea, eb));
    let u = c_val / m_bar;

    if u >= 1.0 {
        return Ok((1.0, [0.0; 12], [[0.0; 12]; 12]));
    }

    let one_minus_u = 1.0 - u;
    let m = 3.0 * u - 3.0 * u * u + u * u * u;
    let dm_du = 3.0 * one_minus_u * one_minus_u;
    let d2m_du2 = -6.0 * one_minus_u;

    // Lagrange identity: c = ||ea||^2 ||eb||^2 - (ea . eb)^2
    // dc/dea = 2*B*ea - 2*C*eb
    // dc/deb = 2*A*eb - 2*C*ea
    let g_ea = sub3(scale3(ea, 2.0 * b), scale3(eb, 2.0 * c_dot));
    let g_eb = sub3(scale3(eb, 2.0 * a), scale3(ea, 2.0 * c_dot));

    // q = [p0, p1, p2, p3], ea = p1 - p0, eb = p3 - p2
    // signs s = [-1, +1, -1, +1]
    let dc = pack_ee_vecs(g_ea, g_eb);

    // 6x6 Hessian blocks of c wrt (ea, eb):
    // H_aa = 2*B*I - 2*eb*eb^T
    // H_bb = 2*A*I - 2*ea*ea^T
    // H_ab = 4*ea*eb^T - 2*eb*ea^T - 2*C*I
    let mut h_aa = [[0.0; 3]; 3];
    let mut h_bb = [[0.0; 3]; 3];
    let mut h_ab = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let eye = if i == j { 1.0 } else { 0.0 };
            h_aa[i][j] = 2.0 * b * eye - 2.0 * eb[i] * eb[j];
            h_bb[i][j] = 2.0 * a * eye - 2.0 * ea[i] * ea[j];
            h_ab[i][j] = 4.0 * ea[i] * eb[j] - 2.0 * eb[i] * ea[j] - 2.0 * c_dot * eye;
        }
    }

    let mut d2c = [[0.0; 12]; 12];
    let signs = [-1.0_f64, 1.0_f64, -1.0_f64, 1.0_f64];
    for bi in 0..4 {
        for bj in 0..4 {
            let s = signs[bi] * signs[bj];
            let block = match (bi < 2, bj < 2) {
                (true, true) => h_aa,
                (false, false) => h_bb,
                (true, false) => h_ab,
                (false, true) => transpose3(h_ab),
            };
            for i in 0..3 {
                for j in 0..3 {
                    d2c[3 * bi + i][3 * bj + j] = s * block[i][j];
                }
            }
        }
    }

    let alpha = dm_du / m_bar;
    let beta = d2m_du2 / (m_bar * m_bar);

    let mut grad_m = [0.0; 12];
    for i in 0..12 {
        grad_m[i] = alpha * dc[i];
    }

    let mut hess_m = [[0.0; 12]; 12];
    for i in 0..12 {
        for j in 0..12 {
            hess_m[i][j] = beta * dc[i] * dc[j] + alpha * d2c[i][j];
        }
    }

    Ok((m, grad_m, hess_m))
}

/// Deterministic `12x12` symmetric Jacobi eigendecomposition projecting all
/// negative eigenvalues to `0.0` (PSD/SPD projection, zero `mul_add`, zero
/// external BLAS/LAPACK).
#[must_use]
pub fn project_spd_12x12<M: DetMath>(mat: &[[f64; 12]; 12]) -> [[f64; 12]; 12] {
    let mut a = [[0.0; 12]; 12];
    let mut max_entry = 0.0_f64;
    for i in 0..12 {
        for j in 0..12 {
            let sym = 0.5 * (mat[i][j] + mat[j][i]);
            a[i][j] = sym;
            let abs_val = sym.abs();
            if abs_val > max_entry {
                max_entry = abs_val;
            }
        }
    }
    if max_entry == 0.0 || !max_entry.is_finite() {
        return a;
    }

    let mut v = [[0.0; 12]; 12];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }

    let tol = 1e-15 * max_entry;
    for _sweep in 0..64 {
        let mut max_off = 0.0_f64;
        for p in 0..11 {
            for q in (p + 1)..12 {
                let apq = a[p][q];
                let abs_apq = apq.abs();
                if abs_apq > max_off {
                    max_off = abs_apq;
                }
                if abs_apq <= tol {
                    continue;
                }
                let app = a[p][p];
                let aqq = a[q][q];
                let tau = (aqq - app) / (2.0 * apq);
                let t = if tau >= 0.0 {
                    1.0 / (tau + M::sqrt(1.0 + tau * tau))
                } else {
                    -1.0 / (-tau + M::sqrt(1.0 + tau * tau))
                };
                let c = 1.0 / M::sqrt(1.0 + t * t);
                let s = t * c;

                a[p][p] = app - t * apq;
                a[q][q] = aqq + t * apq;
                a[p][q] = 0.0;
                a[q][p] = 0.0;

                for r in 0..12 {
                    if r == p || r == q {
                        continue;
                    }
                    let arp = a[r][p];
                    let arq = a[r][q];
                    let new_rp = c * arp - s * arq;
                    let new_rq = s * arp + c * arq;
                    a[r][p] = new_rp;
                    a[p][r] = new_rp;
                    a[r][q] = new_rq;
                    a[q][r] = new_rq;
                }

                for row in &mut v {
                    let vrp = row[p];
                    let vrq = row[q];
                    row[p] = c * vrp - s * vrq;
                    row[q] = s * vrp + c * vrq;
                }
            }
        }
        if max_off <= tol {
            break;
        }
    }

    let mut lam = [0.0; 12];
    for i in 0..12 {
        lam[i] = if a[i][i] > 0.0 { a[i][i] } else { 0.0 };
    }

    let mut out = [[0.0; 12]; 12];
    for i in 0..12 {
        for j in i..12 {
            let mut sum = 0.0;
            for k in 0..12 {
                sum += lam[k] * v[i][k] * v[j][k];
            }
            out[i][j] = sum;
            out[j][i] = sum;
        }
    }
    out
}

/// Evaluate a Point-Point pair `[p0, p1]` against `ScalarBarrier`.
///
/// # Errors
/// Fails closed if inputs are non-finite, `barrier` is invalid, `d_offset < 0`,
/// or effective gap `g <= 0`.
pub fn evaluate_pp_barrier<M: DetMath>(
    p0: [f64; 3],
    p1: [f64; 3],
    d_offset: f64,
    barrier: &ScalarBarrier,
) -> Result<CipcEval, CodimContactError> {
    validate_common_pair(&[p0, p1], d_offset, barrier)?;
    let (d2_val, grad_d2, hess_d2) = pp_d2_in_12(p0, p1, 0, 1);
    assemble_unmollified::<M>(
        PrimitiveKind::PointPoint,
        d2_val,
        &grad_d2,
        &hess_d2,
        d_offset,
        barrier,
    )
}

/// Evaluate a Point-Edge pair `(p0, [p1, p2])` against `ScalarBarrier`.
///
/// # Errors
/// Fails closed if inputs are non-finite, `barrier` is invalid, `d_offset < 0`,
/// or effective gap `g <= 0`.
pub fn evaluate_pe_barrier<M: DetMath>(
    p0: [f64; 3],
    p1: [f64; 3],
    p2: [f64; 3],
    d_offset: f64,
    barrier: &ScalarBarrier,
) -> Result<CipcEval, CodimContactError> {
    validate_common_pair(&[p0, p1, p2], d_offset, barrier)?;
    let (d2_val, grad_d2, hess_d2) = pe_d2_in_12(p0, p1, p2, 0, 1, 2);
    assemble_unmollified::<M>(
        PrimitiveKind::PointEdge,
        d2_val,
        &grad_d2,
        &hess_d2,
        d_offset,
        barrier,
    )
}

/// Evaluate a Point-Triangle pair `(p0, [p1, p2, p3])` against `ScalarBarrier`.
///
/// # Errors
/// Fails closed if inputs are non-finite, `barrier` is invalid, `d_offset < 0`,
/// or effective gap `g <= 0`.
pub fn evaluate_pt_barrier<M: DetMath>(
    p0: [f64; 3],
    p1: [f64; 3],
    p2: [f64; 3],
    p3: [f64; 3],
    d_offset: f64,
    barrier: &ScalarBarrier,
) -> Result<CipcEval, CodimContactError> {
    validate_common_pair(&[p0, p1, p2, p3], d_offset, barrier)?;
    let (d2_val, grad_d2, hess_d2) = pt_d2_in_12(p0, p1, p2, p3);
    assemble_unmollified::<M>(
        PrimitiveKind::PointTriangle,
        d2_val,
        &grad_d2,
        &hess_d2,
        d_offset,
        barrier,
    )
}

/// Evaluate a mollified Edge-Edge pair `([p0, p1], [p2, p3])` against
/// `ScalarBarrier` using rest-scaled `m_bar`.
///
/// # Errors
/// Fails closed if inputs are non-finite, `barrier` or `m_bar` is invalid,
/// `d_offset < 0`, or effective gap `g <= 0`.
pub fn evaluate_ee_barrier<M: DetMath>(
    q: [[f64; 3]; 4],
    m_bar: f64,
    d_offset: f64,
    barrier: &ScalarBarrier,
) -> Result<CipcEval, CodimContactError> {
    validate_common_pair(&q, d_offset, barrier)?;
    let (m, grad_m, hess_m) = ee_mollifier_12(q, m_bar)?;
    let (d2_val, grad_d2, hess_d2) = ee_d2_in_12(q);
    let d = M::sqrt(d2_val);
    let g = d - d_offset;
    if !g.is_finite() || g <= 0.0 {
        return Err(CodimContactError::InitialPenetration { min_clearance_m: g });
    }
    if g >= barrier.dhat {
        return Ok(CipcEval {
            kind: PrimitiveKind::EdgeEdge,
            distance_m: d,
            gap_m: g,
            mollifier: m,
            energy_j: 0.0,
            grad: [0.0; 12],
            hess: [[0.0; 12]; 12],
            hess_spd: [[0.0; 12]; 12],
        });
    }

    let (b_val, grad_b, hess_b) = barrier_chain::<M>(d, g, &grad_d2, &hess_d2, barrier)?;
    let energy_j = m * b_val;

    let mut grad = [0.0; 12];
    for i in 0..12 {
        grad[i] = m * grad_b[i] + b_val * grad_m[i];
    }

    let mut hess = [[0.0; 12]; 12];
    for i in 0..12 {
        for j in 0..12 {
            hess[i][j] = m * hess_b[i][j]
                + grad_m[i] * grad_b[j]
                + grad_b[i] * grad_m[j]
                + b_val * hess_m[i][j];
        }
    }
    let hess_spd = project_spd_12x12::<M>(&hess);

    Ok(CipcEval {
        kind: PrimitiveKind::EdgeEdge,
        distance_m: d,
        gap_m: g,
        mollifier: m,
        energy_j,
        grad,
        hess,
        hess_spd,
    })
}

// ---------------------------------------------------------------------------
// Internal distance + squared-distance exact derivative machinery.
// ---------------------------------------------------------------------------

fn validate_common_pair(
    pts: &[[f64; 3]],
    d_offset: f64,
    barrier: &ScalarBarrier,
) -> Result<(), CodimContactError> {
    barrier.validate()?;
    if !d_offset.is_finite() {
        return Err(CodimContactError::NonFiniteInput {
            context: "d_offset",
        });
    }
    if d_offset < 0.0 {
        return Err(CodimContactError::InvalidParameter {
            detail: "d_offset must be non-negative",
        });
    }
    for &p in pts {
        if !all_finite3(p) {
            return Err(CodimContactError::NonFiniteInput {
                context: "primitive vertex position",
            });
        }
    }
    Ok(())
}

fn barrier_chain<M: DetMath>(
    d: f64,
    g: f64,
    grad_d2: &[f64; 12],
    hess_d2: &[[f64; 12]; 12],
    barrier: &ScalarBarrier,
) -> Result<StencilDerivatives12, CodimContactError> {
    if !d.is_finite() || d <= 0.0 {
        return Err(CodimContactError::InitialPenetration { min_clearance_m: g });
    }
    let inv_2d = 0.5 / d;
    let inv_4d3 = 0.25 / (d * d * d);

    let mut grad_g = [0.0; 12];
    for i in 0..12 {
        grad_g[i] = inv_2d * grad_d2[i];
    }

    let b_val = barrier.value::<M>(g);
    let b1 = barrier.d1::<M>(g);
    let b2 = barrier.d2::<M>(g);

    let mut grad_b = [0.0; 12];
    for i in 0..12 {
        grad_b[i] = b1 * grad_g[i];
    }

    let mut hess_b = [[0.0; 12]; 12];
    for i in 0..12 {
        for j in 0..12 {
            let hess_g_ij = inv_2d * hess_d2[i][j] - inv_4d3 * grad_d2[i] * grad_d2[j];
            hess_b[i][j] = b2 * grad_g[i] * grad_g[j] + b1 * hess_g_ij;
        }
    }
    Ok((b_val, grad_b, hess_b))
}

fn assemble_unmollified<M: DetMath>(
    kind: PrimitiveKind,
    d2_val: f64,
    grad_d2: &[f64; 12],
    hess_d2: &[[f64; 12]; 12],
    d_offset: f64,
    barrier: &ScalarBarrier,
) -> Result<CipcEval, CodimContactError> {
    let d = M::sqrt(d2_val);
    let g = d - d_offset;
    if !g.is_finite() || g <= 0.0 {
        return Err(CodimContactError::InitialPenetration { min_clearance_m: g });
    }
    if g >= barrier.dhat {
        return Ok(CipcEval {
            kind,
            distance_m: d,
            gap_m: g,
            mollifier: 1.0,
            energy_j: 0.0,
            grad: [0.0; 12],
            hess: [[0.0; 12]; 12],
            hess_spd: [[0.0; 12]; 12],
        });
    }
    let (energy_j, grad, hess) = barrier_chain::<M>(d, g, grad_d2, hess_d2, barrier)?;
    let hess_spd = project_spd_12x12::<M>(&hess);
    Ok(CipcEval {
        kind,
        distance_m: d,
        gap_m: g,
        mollifier: 1.0,
        energy_j,
        grad,
        hess,
        hess_spd,
    })
}

fn pp_d2_in_12(
    p0: [f64; 3],
    p1: [f64; 3],
    slot0: usize,
    slot1: usize,
) -> (f64, [f64; 12], [[f64; 12]; 12]) {
    let w = sub3(p0, p1);
    let d2_val = norm2_3(w);
    let mut grad = [0.0; 12];
    let mut hess = [[0.0; 12]; 12];
    for c in 0..3 {
        grad[3 * slot0 + c] = 2.0 * w[c];
        grad[3 * slot1 + c] = -2.0 * w[c];
        hess[3 * slot0 + c][3 * slot0 + c] = 2.0;
        hess[3 * slot1 + c][3 * slot1 + c] = 2.0;
        hess[3 * slot0 + c][3 * slot1 + c] = -2.0;
        hess[3 * slot1 + c][3 * slot0 + c] = -2.0;
    }
    (d2_val, grad, hess)
}

fn pe_d2_in_12(
    p: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    slot_p: usize,
    slot_a: usize,
    slot_b: usize,
) -> (f64, [f64; 12], [[f64; 12]; 12]) {
    let e = sub3(b, a);
    let e2 = norm2_3(e);
    if e2 <= 1e-30 {
        return pp_d2_in_12(p, a, slot_p, slot_a);
    }
    let pa = sub3(p, a);
    let t = dot3(pa, e) / e2;
    if t <= 0.0 {
        return pp_d2_in_12(p, a, slot_p, slot_a);
    }
    if t >= 1.0 {
        return pp_d2_in_12(p, b, slot_p, slot_b);
    }

    // Interior PE: r = p - (1-t)*a - t*b
    let one_m_t = 1.0 - t;
    let r = sub3(pa, scale3(e, t));
    let d2_val = norm2_3(r);

    let slots = [slot_p, slot_a, slot_b];
    let weights = [1.0, -one_m_t, -t];
    let mut grad = [0.0; 12];
    for k in 0..3 {
        let sk = 3 * slots[k];
        for c in 0..3 {
            grad[sk + c] = 2.0 * weights[k] * r[c];
        }
    }

    // Schur complement: H = 2 A^T A - 2 e2 * g_t * g_t^T
    // g_t = [e, -(1-t)e - r, -t*e + r] / e2
    let gt_blocks = [
        scale3(e, 1.0 / e2),
        scale3(sub3(scale3(e, -one_m_t), r), 1.0 / e2),
        scale3(sub3(r, scale3(e, t)), 1.0 / e2),
    ];

    let mut hess = [[0.0; 12]; 12];
    for bi in 0..3 {
        let si = 3 * slots[bi];
        for bj in 0..3 {
            let sj = 3 * slots[bj];
            let w_ij = 2.0 * weights[bi] * weights[bj];
            for ci in 0..3 {
                for cj in 0..3 {
                    let eye = if ci == cj { w_ij } else { 0.0 };
                    hess[si + ci][sj + cj] = eye - 2.0 * e2 * gt_blocks[bi][ci] * gt_blocks[bj][cj];
                }
            }
        }
    }
    (d2_val, grad, hess)
}

fn pt_d2_in_12(
    p: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
) -> (f64, [f64; 12], [[f64; 12]; 12]) {
    // Ericson §5.1.5 exact triangle closest-point Voronoi classification.
    let ab = sub3(b, a);
    let ac = sub3(c, a);
    let ap = sub3(p, a);

    let d1 = dot3(ab, ap);
    let d2 = dot3(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return pp_d2_in_12(p, a, 0, 1);
    }

    let bp = sub3(p, b);
    let d3 = dot3(ab, bp);
    let d4 = dot3(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return pp_d2_in_12(p, b, 0, 2);
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return pe_d2_in_12(p, a, b, 0, 1, 2);
    }

    let cp = sub3(p, c);
    let d5 = dot3(ab, cp);
    let d6 = dot3(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return pp_d2_in_12(p, c, 0, 3);
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return pe_d2_in_12(p, a, c, 0, 1, 3);
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return pe_d2_in_12(p, b, c, 0, 2, 3);
    }

    let denom_bary = va + vb + vc;
    if denom_bary <= 1e-30 {
        return pe_d2_in_12(p, a, b, 0, 1, 2);
    }
    let u = vb / denom_bary;
    let v = vc / denom_bary;
    let w = 1.0 - u - v;

    let e1 = ab;
    let e2 = ac;
    let r = sub3(
        ap,
        [
            u * e1[0] + v * e2[0],
            u * e1[1] + v * e2[1],
            u * e1[2] + v * e2[2],
        ],
    );
    let d2_val = norm2_3(r);

    let weights = [1.0, -w, -u, -v];
    let mut grad = [0.0; 12];
    for bk in 0..4 {
        for ci in 0..3 {
            grad[3 * bk + ci] = 2.0 * weights[bk] * r[ci];
        }
    }

    let h00 = 2.0 * dot3(e1, e1);
    let h01 = 2.0 * dot3(e1, e2);
    let h11 = 2.0 * dot3(e2, e2);
    let det_uv = h00 * h11 - h01 * h01;
    if det_uv <= 1e-24 * (dot3(e1, e1) * dot3(e2, e2) + 1e-30) {
        return pe_d2_in_12(p, a, b, 0, 1, 2);
    }
    let inv_det = 1.0 / det_uv;
    let inv00 = h11 * inv_det;
    let inv01 = -h01 * inv_det;
    let inv11 = h00 * inv_det;

    // Mixed partials H_{q,u} and H_{q,v}
    let hqu = [
        scale3(e1, -2.0),
        scale3(sub3(scale3(e1, w), scale3(r, -1.0)), 2.0),
        scale3(sub3(scale3(e1, u), r), 2.0),
        scale3(e1, 2.0 * v),
    ];
    let hqv = [
        scale3(e2, -2.0),
        scale3(sub3(scale3(e2, w), scale3(r, -1.0)), 2.0),
        scale3(e2, 2.0 * u),
        scale3(sub3(scale3(e2, v), r), 2.0),
    ];
    let hqu_flat = flatten_blocks4(hqu);
    let hqv_flat = flatten_blocks4(hqv);

    let mut hess = [[0.0; 12]; 12];
    for bi in 0..4 {
        for bj in 0..4 {
            let wij = 2.0 * weights[bi] * weights[bj];
            for ci in 0..3 {
                let row = 3 * bi + ci;
                for cj in 0..3 {
                    let col = 3 * bj + cj;
                    let eye = if ci == cj { wij } else { 0.0 };
                    let schur = (hqu_flat[row] * inv00 + hqv_flat[row] * inv01) * hqu_flat[col]
                        + (hqu_flat[row] * inv01 + hqv_flat[row] * inv11) * hqv_flat[col];
                    hess[row][col] = eye - schur;
                }
            }
        }
    }

    (d2_val, grad, hess)
}

fn ee_d2_in_12(q: [[f64; 3]; 4]) -> (f64, [f64; 12], [[f64; 12]; 12]) {
    let ea = sub3(q[1], q[0]);
    let eb = sub3(q[3], q[2]);
    let r0 = sub3(q[0], q[2]);
    let a = dot3(ea, ea);
    let e = dot3(eb, eb);
    let f = dot3(eb, r0);

    if a <= 1e-30 && e <= 1e-30 {
        return pp_d2_in_12(q[0], q[2], 0, 2);
    }
    if a <= 1e-30 {
        return pe_d2_in_12(q[0], q[2], q[3], 0, 2, 3);
    }
    let c = dot3(ea, r0);
    if e <= 1e-30 {
        return pe_d2_in_12(q[2], q[0], q[1], 2, 0, 1);
    }

    let b = dot3(ea, eb);
    let denom = a * e - b * b;
    let mut s = if denom > 1e-24 * a * e {
        ((b * f - c * e) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut t = (b * s + f) / e;
    if t < 0.0 {
        t = 0.0;
        s = (-c / a).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((b - c) / a).clamp(0.0, 1.0);
    }

    // Check boundary clamping to PE / PP
    if s <= 0.0 {
        return pe_d2_in_12(q[0], q[2], q[3], 0, 2, 3);
    }
    if s >= 1.0 {
        return pe_d2_in_12(q[1], q[2], q[3], 1, 2, 3);
    }
    if t <= 0.0 {
        return pe_d2_in_12(q[2], q[0], q[1], 2, 0, 1);
    }
    if t >= 1.0 {
        return pe_d2_in_12(q[3], q[0], q[1], 3, 0, 1);
    }

    // Interior EE: r = (1-s)p0 + s*p1 - (1-t)p2 - t*p3
    let one_m_s = 1.0 - s;
    let one_m_t = 1.0 - t;
    let r = sub3(
        sub3(scale3(q[0], one_m_s), scale3(q[2], one_m_t)),
        sub3(scale3(q[3], t), scale3(q[1], s)),
    );
    let d2_val = norm2_3(r);
    let weights = [one_m_s, s, -one_m_t, -t];

    let mut grad = [0.0; 12];
    for bk in 0..4 {
        for ci in 0..3 {
            grad[3 * bk + ci] = 2.0 * weights[bk] * r[ci];
        }
    }

    let h_ss = 2.0 * a;
    let h_st = -2.0 * b;
    let h_tt = 2.0 * e;
    let det_st = h_ss * h_tt - h_st * h_st;
    if det_st <= 1e-24 * (a * e + 1e-30) {
        return pe_d2_in_12(q[0], q[2], q[3], 0, 2, 3);
    }
    let inv_det = 1.0 / det_st;
    let inv00 = h_tt * inv_det;
    let inv01 = -h_st * inv_det;
    let inv11 = h_ss * inv_det;

    let hqs = [
        scale3(sub3(scale3(ea, one_m_s), r), 2.0),
        scale3(sub3(scale3(ea, s), scale3(r, -1.0)), 2.0),
        scale3(ea, -2.0 * one_m_t),
        scale3(ea, -2.0 * t),
    ];
    let hqt = [
        scale3(eb, -2.0 * one_m_s),
        scale3(eb, -2.0 * s),
        scale3(sub3(scale3(eb, one_m_t), scale3(r, -1.0)), 2.0),
        scale3(sub3(scale3(eb, t), r), 2.0),
    ];
    let hqs_flat = flatten_blocks4(hqs);
    let hqt_flat = flatten_blocks4(hqt);

    let mut hess = [[0.0; 12]; 12];
    for bi in 0..4 {
        for bj in 0..4 {
            let wij = 2.0 * weights[bi] * weights[bj];
            for ci in 0..3 {
                let row = 3 * bi + ci;
                for cj in 0..3 {
                    let col = 3 * bj + cj;
                    let eye = if ci == cj { wij } else { 0.0 };
                    let schur = (hqs_flat[row] * inv00 + hqt_flat[row] * inv01) * hqs_flat[col]
                        + (hqs_flat[row] * inv01 + hqt_flat[row] * inv11) * hqt_flat[col];
                    hess[row][col] = eye - schur;
                }
            }
        }
    }

    (d2_val, grad, hess)
}

#[inline]
fn pack_ee_vecs(ea_part: [f64; 3], eb_part: [f64; 3]) -> [f64; 12] {
    [
        -ea_part[0],
        -ea_part[1],
        -ea_part[2],
        ea_part[0],
        ea_part[1],
        ea_part[2],
        -eb_part[0],
        -eb_part[1],
        -eb_part[2],
        eb_part[0],
        eb_part[1],
        eb_part[2],
    ]
}

#[inline]
fn flatten_blocks4(b: [[f64; 3]; 4]) -> [f64; 12] {
    [
        b[0][0], b[0][1], b[0][2], b[1][0], b[1][1], b[1][2], b[2][0], b[2][1], b[2][2], b[3][0],
        b[3][1], b[3][2],
    ]
}

#[inline]
fn transpose3(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
}

// ---------------------------------------------------------------------------
// Additive Physical Normal Contact Laws (strictly distinct from
// ConstraintBarrierReceipt).
// ---------------------------------------------------------------------------

/// Evaluation result of an additive physical [`NormalContactLaw`] at a given
/// soft-core normal overlap `delta >= 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalLawEval {
    /// Stored reversible normal elastic energy in Joules (`>= 0`).
    pub energy_j: f64,
    /// Compressive normal contact force in Newtons (`>= 0`).
    pub force_n: f64,
    /// Tangent normal stiffness `d(force_n)/d(overlap)` in `N/m` (`>= 0`).
    pub stiffness_n_m: f64,
}

/// Pluggable physical soft-core normal compliance law (additive alongside the
/// hard-core non-penetration C-IPC barrier).
pub trait NormalContactLaw {
    /// Evaluate reversible normal compliance at `overlap_m` (metres) and active
    /// contact length `contact_len_m` (metres).
    ///
    /// # Errors
    /// Returns [`CodimContactError`] when parameters, overlap, or contact length
    /// are non-finite, negative, or outside the declared physical envelope.
    fn evaluate_normal<M: DetMath>(
        &self,
        overlap_m: f64,
        contact_len_m: f64,
    ) -> Result<NormalLawEval, CodimContactError>;
}

/// Transverse Hertzian compliance for stiff metallic wires / nitinol filaments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HertzianWire {
    /// Combined contact modulus `E*` in Pascals (`> 0`).
    pub reduced_modulus_pa: f64,
    /// Effective local curvature radius `R_eff` in metres (`> 0`).
    pub effective_radius_m: f64,
}

impl NormalContactLaw for HertzianWire {
    fn evaluate_normal<M: DetMath>(
        &self,
        overlap_m: f64,
        contact_len_m: f64,
    ) -> Result<NormalLawEval, CodimContactError> {
        if !self.reduced_modulus_pa.is_finite()
            || !self.effective_radius_m.is_finite()
            || !overlap_m.is_finite()
            || !contact_len_m.is_finite()
        {
            return Err(CodimContactError::NonFiniteInput {
                context: "HertzianWire::evaluate_normal",
            });
        }
        if self.reduced_modulus_pa <= 0.0 || self.effective_radius_m <= 0.0 || contact_len_m < 0.0 {
            return Err(CodimContactError::InvalidParameter {
                detail: "HertzianWire requires positive modulus/radius and non-negative contact_len_m",
            });
        }
        if overlap_m <= 0.0 {
            return Ok(NormalLawEval {
                energy_j: 0.0,
                force_n: 0.0,
                stiffness_n_m: 0.0,
            });
        }
        if overlap_m > self.effective_radius_m {
            return Err(CodimContactError::ApplicabilityExceeded {
                detail: "HertzianWire small-strain limit exceeded (overlap > effective_radius_m)",
            });
        }
        let sqrt_r = M::sqrt(self.effective_radius_m);
        let sqrt_d = M::sqrt(overlap_m);
        let d_32 = overlap_m * sqrt_d;
        let d_52 = overlap_m * d_32;
        let coeff = (4.0 / 3.0) * self.reduced_modulus_pa * sqrt_r;
        Ok(NormalLawEval {
            energy_j: 0.4 * coeff * d_52,
            force_n: coeff * d_32,
            stiffness_n_m: 1.5 * coeff * sqrt_d,
        })
    }
}

/// Exponential transverse compression law for porous multi-fiber bundles
/// capturing stiffening toward fiber packing jamming.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExponentialYarn {
    /// Base transverse stiffness modulus `E_t0` in Pascals (`> 0`).
    pub stiffness_pa: f64,
    /// Characteristic compression decay length in metres (`> 0`).
    pub decay_length_m: f64,
    /// Reference uncompressed cross-section radius in metres (`> 0`).
    pub reference_radius_m: f64,
}

impl NormalContactLaw for ExponentialYarn {
    fn evaluate_normal<M: DetMath>(
        &self,
        overlap_m: f64,
        contact_len_m: f64,
    ) -> Result<NormalLawEval, CodimContactError> {
        if !self.stiffness_pa.is_finite()
            || !self.decay_length_m.is_finite()
            || !self.reference_radius_m.is_finite()
            || !overlap_m.is_finite()
            || !contact_len_m.is_finite()
        {
            return Err(CodimContactError::NonFiniteInput {
                context: "ExponentialYarn::evaluate_normal",
            });
        }
        if self.stiffness_pa <= 0.0
            || self.decay_length_m <= 0.0
            || self.reference_radius_m <= 0.0
            || contact_len_m < 0.0
        {
            return Err(CodimContactError::InvalidParameter {
                detail: "ExponentialYarn requires positive stiffness, decay length, and radius",
            });
        }
        if overlap_m <= 0.0 {
            return Ok(NormalLawEval {
                energy_j: 0.0,
                force_n: 0.0,
                stiffness_n_m: 0.0,
            });
        }
        let x = overlap_m / self.decay_length_m;
        if x > 40.0 || overlap_m >= 1.9 * self.reference_radius_m {
            return Err(CodimContactError::ApplicabilityExceeded {
                detail: "ExponentialYarn compression exceeded jamming applicability envelope",
            });
        }
        let eff_len = if contact_len_m > 0.0 {
            contact_len_m
        } else {
            self.reference_radius_m
        };
        let k_line = self.stiffness_pa * eff_len * self.decay_length_m;
        let exp_x = M::exp(x);
        Ok(NormalLawEval {
            energy_j: k_line * self.decay_length_m * (exp_x - 1.0 - x),
            force_n: k_line * (exp_x - 1.0),
            stiffness_n_m: (k_line / self.decay_length_m) * exp_x,
        })
    }
}

/// Transverse flattening compliance for moderate-aspect dry composite tows
/// and flat ribbons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RibbonFlattening {
    /// Linear transverse stiffness per overlap in `N/m` (`> 0`).
    pub stiffness_n_per_m: f64,
    /// Ribbon width in metres (`> 0`).
    pub width_m: f64,
    /// Nominal ribbon thickness in metres (`> 0`).
    pub thickness_m: f64,
    /// Maximum admitted aspect ratio `width_m / thickness_m` (`>= 1.0`).
    pub aspect_limit: f64,
}

impl NormalContactLaw for RibbonFlattening {
    fn evaluate_normal<M: DetMath>(
        &self,
        overlap_m: f64,
        contact_len_m: f64,
    ) -> Result<NormalLawEval, CodimContactError> {
        let _ = M::sqrt(1.0);
        if !self.stiffness_n_per_m.is_finite()
            || !self.width_m.is_finite()
            || !self.thickness_m.is_finite()
            || !self.aspect_limit.is_finite()
            || !overlap_m.is_finite()
            || !contact_len_m.is_finite()
        {
            return Err(CodimContactError::NonFiniteInput {
                context: "RibbonFlattening::evaluate_normal",
            });
        }
        if self.stiffness_n_per_m <= 0.0
            || self.width_m <= 0.0
            || self.thickness_m <= 0.0
            || self.aspect_limit < 1.0
            || contact_len_m < 0.0
        {
            return Err(CodimContactError::InvalidParameter {
                detail: "RibbonFlattening parameters must be strictly positive (aspect_limit >= 1)",
            });
        }
        let aspect = self.width_m / self.thickness_m;
        if aspect > self.aspect_limit {
            return Err(CodimContactError::ApplicabilityExceeded {
                detail: "RibbonFlattening width/thickness aspect ratio exceeds aspect_limit",
            });
        }
        if overlap_m <= 0.0 {
            return Ok(NormalLawEval {
                energy_j: 0.0,
                force_n: 0.0,
                stiffness_n_m: 0.0,
            });
        }
        if overlap_m > self.thickness_m {
            return Err(CodimContactError::ApplicabilityExceeded {
                detail: "RibbonFlattening overlap exceeds ribbon thickness",
            });
        }
        let len_scale = if contact_len_m > 0.0 {
            (contact_len_m / self.width_m).max(1.0)
        } else {
            1.0
        };
        let k_eff = self.stiffness_n_per_m * len_scale;
        let t = self.thickness_m;
        let d = overlap_m;
        let d2 = d * d;
        let d3 = d2 * d;
        Ok(NormalLawEval {
            energy_j: k_eff * (0.5 * d2 + d3 / (6.0 * t)),
            force_n: k_eff * (d + 0.5 * d2 / t),
            stiffness_n_m: k_eff * (1.0 + d / t),
        })
    }
}

/// Tagged union of built-in additive normal compliance models.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NormalContactModel {
    /// Hard-core barrier only (zero additive compliance).
    None,
    /// Transverse Hertzian wire contact.
    HertzianWire(HertzianWire),
    /// Soft-core exponential bundle compression.
    ExponentialYarn(ExponentialYarn),
    /// Moderate-aspect ribbon flattening compliance.
    RibbonFlattening(RibbonFlattening),
}

impl NormalContactLaw for NormalContactModel {
    fn evaluate_normal<M: DetMath>(
        &self,
        overlap_m: f64,
        contact_len_m: f64,
    ) -> Result<NormalLawEval, CodimContactError> {
        match self {
            Self::None => {
                if !overlap_m.is_finite() || !contact_len_m.is_finite() {
                    return Err(CodimContactError::NonFiniteInput {
                        context: "NormalContactModel::None",
                    });
                }
                if contact_len_m < 0.0 {
                    return Err(CodimContactError::InvalidParameter {
                        detail: "contact_len_m must be non-negative",
                    });
                }
                Ok(NormalLawEval {
                    energy_j: 0.0,
                    force_n: 0.0,
                    stiffness_n_m: 0.0,
                })
            }
            Self::HertzianWire(law) => law.evaluate_normal::<M>(overlap_m, contact_len_m),
            Self::ExponentialYarn(law) => law.evaluate_normal::<M>(overlap_m, contact_len_m),
            Self::RibbonFlattening(law) => law.evaluate_normal::<M>(overlap_m, contact_len_m),
        }
    }
}
