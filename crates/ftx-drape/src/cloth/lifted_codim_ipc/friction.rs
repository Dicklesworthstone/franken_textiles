//! Pluggable tangential friction laws (`SmoothCoulombStribeck`,
//! `DirectionalAnisotropicScale`, `WireFrettingMicroSlip`) and the explicit
//! outer lagged fixed-point contract freezing `(lambda_n, T, mu_eff)` per
//! symmetric inner Newton-LDLT solve.

use super::{
    CodimContactError, DetMath, all_finite3, dot3, mat3_vec, norm2_3, norm3, scale3, sub3,
};

/// Frozen contact-pair state held constant throughout one symmetric inner
/// Newton-LDLT solve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenFrictionLag {
    /// Frozen compressive normal force magnitude `lambda_n >= 0` in Newtons.
    pub lambda_n: f64,
    /// Unit contact normal vector pointing from primitive B toward A.
    pub normal: [f64; 3],
    /// Symmetric `3x3` tangent projection matrix `T = I - n n^T`.
    pub tangent_proj: [[f64; 3]; 3],
    /// Frozen effective scalar friction coefficient `mu_eff >= 0`.
    pub mu_eff: f64,
    /// Smoothing slip threshold `eps_s > 0` in metres (`eps_v * dt`).
    pub eps_s_m: f64,
    /// Optional tangential micro-slip elastic stiffness (`N/m`) active for
    /// fretting micro-slip; `0.0` for pure IPC Coulomb/Stribeck/anisotropic.
    pub k_micro_n_per_m: f64,
    /// Previously converged tangential friction force vector `lambda_f` in N.
    pub prev_lambda_f: [f64; 3],
}

impl FrozenFrictionLag {
    /// Construct a validated [`FrozenFrictionLag`] from normal vector `normal`,
    /// frozen normal force `lambda_n`, effective friction `mu_eff`, and
    /// regularization threshold `eps_s_m`.
    ///
    /// # Errors
    /// Fails closed if inputs are non-finite, `lambda_n < 0`, `mu_eff < 0`,
    /// `eps_s_m <= 0`, or `normal` has near-zero norm.
    pub fn from_normal<M: DetMath>(
        lambda_n: f64,
        normal: [f64; 3],
        mu_eff: f64,
        eps_s_m: f64,
        k_micro_n_per_m: f64,
        prev_lambda_f: [f64; 3],
    ) -> Result<Self, CodimContactError> {
        if !lambda_n.is_finite()
            || !mu_eff.is_finite()
            || !eps_s_m.is_finite()
            || !k_micro_n_per_m.is_finite()
            || !all_finite3(normal)
            || !all_finite3(prev_lambda_f)
        {
            return Err(CodimContactError::NonFiniteInput {
                context: "FrozenFrictionLag::from_normal",
            });
        }
        if lambda_n < 0.0 || mu_eff < 0.0 || eps_s_m <= 0.0 || k_micro_n_per_m < 0.0 {
            return Err(CodimContactError::InvalidParameter {
                detail: "FrozenFrictionLag requires lambda_n >= 0, mu_eff >= 0, eps_s_m > 0, k_micro >= 0",
            });
        }
        let n_len = norm3::<M>(normal);
        if n_len <= 1e-15 {
            return Err(CodimContactError::DegenerateGeometry {
                detail: "contact normal has zero norm",
            });
        }
        let n = scale3(normal, 1.0 / n_len);
        let tangent_proj = tangent_projection(n);
        Ok(Self {
            lambda_n,
            normal: n,
            tangent_proj,
            mu_eff,
            eps_s_m,
            k_micro_n_per_m,
            prev_lambda_f,
        })
    }
}

/// Build the `3x3` tangent projection `T = I - n n^T` for unit normal `n`.
#[must_use]
pub fn tangent_projection(n: [f64; 3]) -> [[f64; 3]; 3] {
    let mut t = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let eye = if i == j { 1.0 } else { 0.0 };
            t[i][j] = eye - n[i] * n[j];
        }
    }
    t
}

/// Output of evaluating the symmetric frozen-lag tangential friction potential
/// over a 4-vertex stencil (`12` DOFs).
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenFrictionEval {
    /// Smoothed pseudo-potential energy `E_f` in Joules.
    pub energy_j: f64,
    /// Exact `12`-DOF gradient `dE_f/d(delta_q)`.
    pub grad: [f64; 12],
    /// Symmetric PSD `12x12` Hessian `d^2E_f/d(delta_q)^2`.
    pub hess: [[f64; 12]; 12],
    /// Evaluated 3D tangential friction force vector `lambda_f` in Newtons.
    pub lambda_f: [f64; 3],
    /// Tangential slip magnitude `||u_t||_2` in metres.
    pub slip_m: f64,
}

/// Evaluate the conservative symmetric frozen-lag tangential potential and
/// derivatives for incremental displacement `delta_q = [dq0, dq1, dq2, dq3]`
/// using barycentric weights `weights_a = [1 - s, s]` on `(dq0, dq1)` and
/// `weights_b = [1 - t, t]` on `(dq2, dq3)`.
///
/// Relative tangential slip is `u_t = T * (u_a - u_b)`.
///
/// # Errors
/// Fails closed if `delta_q`, weights, or `lag` contain non-finite or invalid
/// parameters.
pub fn evaluate_frozen_friction_12<M: DetMath>(
    lag: &FrozenFrictionLag,
    delta_q: &[[f64; 3]; 4],
    weights_a: [f64; 2],
    weights_b: [f64; 2],
) -> Result<FrozenFrictionEval, CodimContactError> {
    if !lag.lambda_n.is_finite()
        || !lag.mu_eff.is_finite()
        || !lag.eps_s_m.is_finite()
        || !lag.k_micro_n_per_m.is_finite()
        || !weights_a[0].is_finite()
        || !weights_a[1].is_finite()
        || !weights_b[0].is_finite()
        || !weights_b[1].is_finite()
    {
        return Err(CodimContactError::NonFiniteInput {
            context: "evaluate_frozen_friction_12",
        });
    }
    if lag.eps_s_m <= 0.0 || lag.lambda_n < 0.0 || lag.mu_eff < 0.0 {
        return Err(CodimContactError::InvalidParameter {
            detail: "frozen friction lag requires eps_s_m > 0, lambda_n >= 0, mu_eff >= 0",
        });
    }
    for &dq in delta_q {
        if !all_finite3(dq) {
            return Err(CodimContactError::NonFiniteInput {
                context: "evaluate_frozen_friction_12::delta_q",
            });
        }
    }

    let j_weights = [weights_a[0], weights_a[1], -weights_b[0], -weights_b[1]];
    let mut u_rel = [0.0; 3];
    for k in 0..4 {
        for c in 0..3 {
            u_rel[c] += j_weights[k] * delta_q[k][c];
        }
    }
    let u_t = mat3_vec(lag.tangent_proj, u_rel);
    let s2 = norm2_3(u_t);
    let s = M::sqrt(s2);
    let coeff = lag.mu_eff * lag.lambda_n;

    if coeff == 0.0 && lag.k_micro_n_per_m == 0.0 {
        return Ok(FrozenFrictionEval {
            energy_j: 0.0,
            grad: [0.0; 12],
            hess: [[0.0; 12]; 12],
            lambda_f: [0.0; 3],
            slip_m: s,
        });
    }

    let (energy_j, grad_rel, hess_rel) = ipc_friction_tangent_eval(lag, u_t, s, s2, coeff);

    let mut grad = [0.0; 12];
    for bi in 0..4 {
        for c in 0..3 {
            grad[3 * bi + c] = j_weights[bi] * grad_rel[c];
        }
    }

    let mut hess = [[0.0; 12]; 12];
    for bi in 0..4 {
        for bj in 0..4 {
            let wij = j_weights[bi] * j_weights[bj];
            for ci in 0..3 {
                for cj in 0..3 {
                    hess[3 * bi + ci][3 * bj + cj] = wij * hess_rel[ci][cj];
                }
            }
        }
    }

    Ok(FrozenFrictionEval {
        energy_j,
        grad,
        hess,
        lambda_f: grad_rel,
        slip_m: s,
    })
}

fn ipc_friction_tangent_eval(
    lag: &FrozenFrictionLag,
    u_t: [f64; 3],
    s: f64,
    s2: f64,
    coeff: f64,
) -> (f64, [f64; 3], [[f64; 3]; 3]) {
    let eps = lag.eps_s_m;
    let (f0, df_over_s, d2f) = if s < eps {
        let eps2 = eps * eps;
        let val = -(s * s * s) / (3.0 * eps2) + (s * s) / eps;
        let ratio = 2.0 / eps - s / eps2;
        let second = 2.0 / eps - (2.0 * s) / eps2;
        (val, ratio, second)
    } else {
        (s - eps / 3.0, 1.0 / s, 0.0)
    };

    let energy_j = coeff * f0 + 0.5 * lag.k_micro_n_per_m * s2;
    let grad_rel = [
        (coeff * df_over_s + lag.k_micro_n_per_m) * u_t[0],
        (coeff * df_over_s + lag.k_micro_n_per_m) * u_t[1],
        (coeff * df_over_s + lag.k_micro_n_per_m) * u_t[2],
    ];

    let mut hess_rel = [[0.0; 3]; 3];
    if s > 1e-18 {
        let u_hat = scale3(u_t, 1.0 / s);
        for i in 0..3 {
            for j in 0..3 {
                let p_perp = lag.tangent_proj[i][j] - u_hat[i] * u_hat[j];
                let p_par = u_hat[i] * u_hat[j];
                hess_rel[i][j] = coeff * (df_over_s * p_perp + d2f * p_par)
                    + lag.k_micro_n_per_m * lag.tangent_proj[i][j];
            }
        }
    } else {
        let diag_coeff = coeff * (2.0 / eps) + lag.k_micro_n_per_m;
        for i in 0..3 {
            for j in 0..3 {
                hess_rel[i][j] = diag_coeff * lag.tangent_proj[i][j];
            }
        }
    }

    (energy_j, grad_rel, hess_rel)
}

/// Pluggable tangential friction constitutive law computing effective friction
/// coefficient `mu_eff` and optional micro-slip stiffness from current slip
/// kinematics.
pub trait TangentialFrictionLaw {
    /// Validate law parameters prior to stepping.
    ///
    /// # Errors
    /// Returns [`CodimContactError`] if parameters are non-finite or outside
    /// valid bounds.
    fn validate(&self) -> Result<(), CodimContactError>;

    /// Evaluate `(mu_eff, eps_s_m, k_micro_n_per_m)` given slip vector `u_t`,
    /// oriented strand tangent `strand_tangent`, step size `dt_s > 0`, and
    /// normal load `lambda_n >= 0`.
    ///
    /// # Errors
    /// Fails closed on non-finite inputs or `dt_s <= 0`.
    fn effective_parameters<M: DetMath>(
        &self,
        u_t: [f64; 3],
        strand_tangent: [f64; 3],
        dt_s: f64,
        lambda_n: f64,
    ) -> Result<(f64, f64, f64), CodimContactError>;
}

/// Smooth IPC Coulomb + Stribeck velocity-weakening friction:
/// `mu_eff(v) = mu_kinetic + (mu_static - mu_kinetic) * exp(-(v / v_stribeck)^2)`.
/// Reduces to isotropic Coulomb friction when `mu_static == mu_kinetic`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothCoulombStribeck {
    /// Static friction coefficient (`>= mu_kinetic >= 0`).
    pub mu_static: f64,
    /// Kinetic friction coefficient (`>= 0`).
    pub mu_kinetic: f64,
    /// Characteristic Stribeck transition speed in `m/s` (`> 0`).
    pub v_stribeck_m_s: f64,
    /// Velocity smoothing half-width `eps_v` in `m/s` (`> 0`).
    pub eps_v_m_s: f64,
}

impl TangentialFrictionLaw for SmoothCoulombStribeck {
    fn validate(&self) -> Result<(), CodimContactError> {
        if !self.mu_static.is_finite()
            || !self.mu_kinetic.is_finite()
            || !self.v_stribeck_m_s.is_finite()
            || !self.eps_v_m_s.is_finite()
        {
            return Err(CodimContactError::NonFiniteInput {
                context: "SmoothCoulombStribeck",
            });
        }
        if self.mu_static < 0.0
            || self.mu_kinetic < 0.0
            || self.mu_static < self.mu_kinetic
            || self.v_stribeck_m_s <= 0.0
            || self.eps_v_m_s <= 0.0
        {
            return Err(CodimContactError::InvalidParameter {
                detail: "SmoothCoulombStribeck requires 0 <= mu_kinetic <= mu_static, v_stribeck > 0, eps_v > 0",
            });
        }
        Ok(())
    }

    fn effective_parameters<M: DetMath>(
        &self,
        u_t: [f64; 3],
        _strand_tangent: [f64; 3],
        dt_s: f64,
        _lambda_n: f64,
    ) -> Result<(f64, f64, f64), CodimContactError> {
        self.validate()?;
        if !all_finite3(u_t) || !dt_s.is_finite() {
            return Err(CodimContactError::NonFiniteInput {
                context: "SmoothCoulombStribeck::effective_parameters",
            });
        }
        if dt_s <= 0.0 {
            return Err(CodimContactError::InvalidParameter {
                detail: "dt_s must be strictly positive",
            });
        }
        let v = norm3::<M>(u_t) / dt_s;
        let ratio = v / self.v_stribeck_m_s;
        let exp_term = M::exp(-(ratio * ratio).min(80.0));
        let mu_eff = self.mu_kinetic + (self.mu_static - self.mu_kinetic) * exp_term;
        Ok((mu_eff, self.eps_v_m_s * dt_s, 0.0))
    }
}

/// Directional anisotropic surface-scale friction (`mu_forward != mu_reverse`)
/// along oriented filament tangent `t_a`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalAnisotropicScale {
    /// Friction coefficient along `+t_a` (`>= 0`).
    pub mu_forward: f64,
    /// Friction coefficient along `-t_a` (`>= 0`).
    pub mu_reverse: f64,
    /// Velocity smoothing half-width `eps_v` in `m/s` (`> 0`).
    pub eps_v_m_s: f64,
}

impl TangentialFrictionLaw for DirectionalAnisotropicScale {
    fn validate(&self) -> Result<(), CodimContactError> {
        if !self.mu_forward.is_finite()
            || !self.mu_reverse.is_finite()
            || !self.eps_v_m_s.is_finite()
        {
            return Err(CodimContactError::NonFiniteInput {
                context: "DirectionalAnisotropicScale",
            });
        }
        if self.mu_forward < 0.0 || self.mu_reverse < 0.0 || self.eps_v_m_s <= 0.0 {
            return Err(CodimContactError::InvalidParameter {
                detail: "DirectionalAnisotropicScale requires non-negative mu and eps_v > 0",
            });
        }
        Ok(())
    }

    fn effective_parameters<M: DetMath>(
        &self,
        u_t: [f64; 3],
        strand_tangent: [f64; 3],
        dt_s: f64,
        _lambda_n: f64,
    ) -> Result<(f64, f64, f64), CodimContactError> {
        let _ = M::sqrt(1.0);
        self.validate()?;
        if !all_finite3(u_t) || !all_finite3(strand_tangent) || !dt_s.is_finite() {
            return Err(CodimContactError::NonFiniteInput {
                context: "DirectionalAnisotropicScale::effective_parameters",
            });
        }
        if dt_s <= 0.0 {
            return Err(CodimContactError::InvalidParameter {
                detail: "dt_s must be strictly positive",
            });
        }
        let t_len = norm3::<M>(strand_tangent);
        let proj = if t_len > 1e-15 {
            dot3(u_t, strand_tangent) / t_len
        } else {
            0.0
        };
        let mu_eff = if proj >= 0.0 {
            self.mu_forward
        } else {
            self.mu_reverse
        };
        Ok((mu_eff, self.eps_v_m_s * dt_s, 0.0))
    }
}

/// Wire fretting micro-slip compliance prior to gross Coulomb sliding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WireFrettingMicroSlip {
    /// Gross sliding friction coefficient (`>= 0`).
    pub mu_gross: f64,
    /// Tangential elastic micro-slip stiffness in `N/m` (`>= 0`).
    pub k_tangential_n_per_m: f64,
    /// Critical micro-slip threshold in metres (`> 0`).
    pub slip_limit_m: f64,
    /// Velocity smoothing half-width `eps_v` in `m/s` (`> 0`).
    pub eps_v_m_s: f64,
}

impl TangentialFrictionLaw for WireFrettingMicroSlip {
    fn validate(&self) -> Result<(), CodimContactError> {
        if !self.mu_gross.is_finite()
            || !self.k_tangential_n_per_m.is_finite()
            || !self.slip_limit_m.is_finite()
            || !self.eps_v_m_s.is_finite()
        {
            return Err(CodimContactError::NonFiniteInput {
                context: "WireFrettingMicroSlip",
            });
        }
        if self.mu_gross < 0.0
            || self.k_tangential_n_per_m < 0.0
            || self.slip_limit_m <= 0.0
            || self.eps_v_m_s <= 0.0
        {
            return Err(CodimContactError::InvalidParameter {
                detail: "WireFrettingMicroSlip requires non-negative mu/k and positive slip_limit/eps_v",
            });
        }
        Ok(())
    }

    fn effective_parameters<M: DetMath>(
        &self,
        u_t: [f64; 3],
        _strand_tangent: [f64; 3],
        dt_s: f64,
        lambda_n: f64,
    ) -> Result<(f64, f64, f64), CodimContactError> {
        self.validate()?;
        if !all_finite3(u_t) || !dt_s.is_finite() || !lambda_n.is_finite() {
            return Err(CodimContactError::NonFiniteInput {
                context: "WireFrettingMicroSlip::effective_parameters",
            });
        }
        if dt_s <= 0.0 || lambda_n < 0.0 {
            return Err(CodimContactError::InvalidParameter {
                detail: "WireFrettingMicroSlip requires dt_s > 0 and lambda_n >= 0",
            });
        }
        let s = norm3::<M>(u_t);
        if s < self.slip_limit_m {
            let frac = (s / self.slip_limit_m).clamp(0.0, 1.0);
            Ok((
                self.mu_gross * frac,
                self.eps_v_m_s * dt_s,
                self.k_tangential_n_per_m,
            ))
        } else {
            Ok((self.mu_gross, self.eps_v_m_s * dt_s, 0.0))
        }
    }
}

/// Tagged union of built-in tangential friction laws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TangentialFrictionModel {
    /// Smooth Coulomb / Stribeck velocity-weakening law.
    SmoothCoulombStribeck(SmoothCoulombStribeck),
    /// Directional anisotropic surface-scale friction (`mu_+ != mu_-`).
    DirectionalAnisotropicScale(DirectionalAnisotropicScale),
    /// Wire fretting elastic micro-slip transitioning to gross Coulomb sliding.
    WireFrettingMicroSlip(WireFrettingMicroSlip),
}

impl TangentialFrictionLaw for TangentialFrictionModel {
    fn validate(&self) -> Result<(), CodimContactError> {
        match self {
            Self::SmoothCoulombStribeck(l) => l.validate(),
            Self::DirectionalAnisotropicScale(l) => l.validate(),
            Self::WireFrettingMicroSlip(l) => l.validate(),
        }
    }

    fn effective_parameters<M: DetMath>(
        &self,
        u_t: [f64; 3],
        strand_tangent: [f64; 3],
        dt_s: f64,
        lambda_n: f64,
    ) -> Result<(f64, f64, f64), CodimContactError> {
        match self {
            Self::SmoothCoulombStribeck(l) => {
                l.effective_parameters::<M>(u_t, strand_tangent, dt_s, lambda_n)
            }
            Self::DirectionalAnisotropicScale(l) => {
                l.effective_parameters::<M>(u_t, strand_tangent, dt_s, lambda_n)
            }
            Self::WireFrettingMicroSlip(l) => {
                l.effective_parameters::<M>(u_t, strand_tangent, dt_s, lambda_n)
            }
        }
    }
}

/// Outer lagged fixed-point stopping configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OuterFrictionConfig {
    /// Convergence tolerance on `||lambda_f^(k+1) - lambda_f^(k)||_inf` in N.
    pub tol_f: f64,
    /// Hard upper bound on outer lagged rounds.
    pub max_outer_iters: u32,
}

impl Default for OuterFrictionConfig {
    fn default() -> Self {
        Self {
            tol_f: 1e-6,
            max_outer_iters: 16,
        }
    }
}

/// Explicit convergence/residual telemetry from the outer lagged fixed-point
/// friction loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OuterFrictionReport {
    /// Number of outer lagged rounds executed.
    pub outer_iters: u32,
    /// Final infinity-norm update `||Delta lambda_f||_inf` in Newtons.
    pub final_delta_lambda_inf: f64,
    /// `true` iff `final_delta_lambda_inf < tol_f`.
    pub converged: bool,
}

/// Per-pair snapshot returned by a symmetric inner Newton-LDLT solve at the end
/// of an outer lagged round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InnerPairOutcome {
    /// Updated normal reaction force magnitude `lambda_n >= 0` in Newtons.
    pub lambda_n: f64,
    /// Updated contact normal vector.
    pub normal: [f64; 3],
    /// Converged tangential relative slip `u_t` in metres.
    pub u_t: [f64; 3],
    /// Oriented strand tangent `t_a` for anisotropic laws.
    pub strand_tangent: [f64; 3],
    /// Evaluated 3D tangential friction force `lambda_f` in Newtons.
    pub lambda_f: [f64; 3],
}

/// Execute the outer lagged fixed-point friction loop:
/// 1. Freeze `(lambda_n^(k), T^(k), mu_eff^(k))`.
/// 2. Invoke `inner_solve(&lags)` (caller runs symmetric SPD-projected Newton).
/// 3. Update `(lambda_n, T, mu_eff, lambda_f)` and check
///    `||Delta lambda_f||_inf < tol_f`.
/// 4. Enforce `max_outer_iters` and return explicit [`OuterFrictionReport`].
///
/// # Errors
/// Fails closed if config or law parameters are invalid, inner solve fails, or
/// pair counts mismatch.
pub fn run_outer_lagged_fixed_point<M, L, F>(
    law: &L,
    dt_s: f64,
    config: OuterFrictionConfig,
    initial_lags: Vec<FrozenFrictionLag>,
    mut inner_solve: F,
) -> Result<(Vec<FrozenFrictionLag>, OuterFrictionReport), CodimContactError>
where
    M: DetMath,
    L: TangentialFrictionLaw,
    F: FnMut(&[FrozenFrictionLag]) -> Result<Vec<InnerPairOutcome>, CodimContactError>,
{
    law.validate()?;
    if !dt_s.is_finite() || !config.tol_f.is_finite() {
        return Err(CodimContactError::NonFiniteInput {
            context: "run_outer_lagged_fixed_point",
        });
    }
    if dt_s <= 0.0 || config.tol_f <= 0.0 || config.max_outer_iters == 0 {
        return Err(CodimContactError::InvalidParameter {
            detail: "outer friction loop requires dt_s > 0, tol_f > 0, and max_outer_iters >= 1",
        });
    }

    let mut lags = initial_lags;
    let mut final_delta = f64::INFINITY;
    let mut rounds_done = 0_u32;

    for round in 1..=config.max_outer_iters {
        rounds_done = round;
        let outcomes = inner_solve(&lags)?;
        if outcomes.len() != lags.len() {
            return Err(CodimContactError::InvalidParameter {
                detail: "inner_solve returned mismatched pair count",
            });
        }

        let mut max_delta = 0.0_f64;
        for (lag, out) in lags.iter_mut().zip(outcomes.iter()) {
            let diff = sub3(out.lambda_f, lag.prev_lambda_f);
            let inf_d = diff[0].abs().max(diff[1].abs()).max(diff[2].abs());
            if inf_d > max_delta {
                max_delta = inf_d;
            }
            let (mu_eff, eps_s_m, k_micro) =
                law.effective_parameters::<M>(out.u_t, out.strand_tangent, dt_s, out.lambda_n)?;
            *lag = FrozenFrictionLag::from_normal::<M>(
                out.lambda_n,
                out.normal,
                mu_eff,
                eps_s_m,
                k_micro,
                out.lambda_f,
            )?;
        }

        final_delta = max_delta;
        if final_delta < config.tol_f {
            return Ok((
                lags,
                OuterFrictionReport {
                    outer_iters: rounds_done,
                    final_delta_lambda_inf: final_delta,
                    converged: true,
                },
            ));
        }
    }

    Ok((
        lags,
        OuterFrictionReport {
            outer_iters: rounds_done,
            final_delta_lambda_inf: final_delta,
            converged: false,
        },
    ))
}

/// Fail closed with [`CodimContactError::OuterLagDidNotConverge`] when
/// `report.converged == false`.
///
/// # Errors
/// Returns [`CodimContactError::OuterLagDidNotConverge`] if unconverged.
pub fn require_outer_converged(
    report: OuterFrictionReport,
) -> Result<OuterFrictionReport, CodimContactError> {
    if report.converged {
        Ok(report)
    } else {
        Err(CodimContactError::OuterLagDidNotConverge {
            residual: report.final_delta_lambda_inf,
            rounds: report.outer_iters,
        })
    }
}
