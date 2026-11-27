//! Implicit-function-theorem (IFT) adjoint sensitivities and fail-closed gradient verification
//! for cloth equilibria.
//!
//! Provenance: adapted from FrankenSim `crates/fs-adjoint/src/ift.rs` and
//! `crates/fs-adjoint/src/verify.rs` (`frankensim-source-manifest.json` `source_root`
//! `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`; MIT + OpenAI/Anthropic
//! rider). Adaptations for FrankenTextiles:
//! - reuses the cloth solver's exact symmetric tangent `H(x*)` and cached [`LdltFactor`] when its
//!   recomputed residual on `H(x*) λ = ∂J/∂x` passes the tight tolerance (`1e-11`), otherwise
//!   factors `H(x*)` once;
//! - provides exact elementwise parameter pullbacks `(∂R/∂Q)ᵀ λ` for homogeneous orthotropic
//!   membrane stiffness `[Q11, Q22, Q12, Q33]` (plus analytical chain rule to `[E1, E2, ν12, G12]`),
//!   homogeneous hinge bending rigidity `B`, and general nonlinear/area membrane parameterizations
//!   via [`Solver::membrane_parameter_gradient_with_reactions`] and
//!   [`area_response_stress_derivatives`];
//! - refuses fail-closed when the model has heterogeneous overrides, panel bending, active friction,
//!   or history-dependent constitutive fields;
//! - zero `unsafe`, zero `mul_add`, zero platform `libm`.

use ftx_core::{Refusal, RefusalCode, Vec3};

use crate::cloth::Solver;
use crate::elements::{CompressionModel, Membrane, principal_strain_jets, soften};
use crate::sparse::advanced::{
    BACKWARD_ERROR_ULPS, rigal_gaches_backward_error, true_relative_residual,
};

/// Outcome of [`verify_gradient`]: checks an analytic gradient against central finite differences
/// along deterministic probe directions, refusing vacuous zero-signal probes fail-closed.
#[derive(Debug, Clone, PartialEq)]
pub struct GradientVerdict {
    /// Worst relative error across all probed directions (`+∞` when refused fail-closed).
    pub max_rel_err: f64,
    /// `(analytic, finite_difference)` directional-derivative pairs.
    pub pairs: Vec<(f64, f64)>,
    /// Number of directions where `max(|analytic|, |fd|) > 1e-12 * ‖d‖∞`.
    pub informative_directions: usize,
    /// `true` only when every direction is finite, at least one is informative, and
    /// `max_rel_err < tol`.
    pub pass: bool,
}

fn fail_closed(pairs: Vec<(f64, f64)>) -> GradientVerdict {
    GradientVerdict {
        max_rel_err: f64::INFINITY,
        pairs,
        informative_directions: 0,
        pass: false,
    }
}

/// Verify a claimed gradient of scalar objective `j` at parameter vector `p` against central
/// finite differences `p ± eps * d` along `directions`.
///
/// Refuses fail-closed (`pass = false`, `max_rel_err = +∞`) if any perturbation fails to move
/// the floating-point bit pattern of `p`, any output is non-finite, or ZERO directions clear the
/// `1e-12 * ‖d‖∞` signal floor (preventing silent-zero gradients from scoring `0.0`).
///
/// # Panics
/// If `gradient.len() != p.len()` or any direction length differs from `p.len()`.
pub fn verify_gradient(
    j: &dyn Fn(&[f64]) -> f64,
    p: &[f64],
    gradient: &[f64],
    directions: &[Vec<f64>],
    eps: f64,
    tol: f64,
) -> GradientVerdict {
    assert_eq!(p.len(), gradient.len(), "gradient length mismatch");
    for d in directions {
        assert_eq!(d.len(), p.len(), "direction length mismatch");
    }
    if !eps.is_finite()
        || eps <= 0.0
        || !tol.is_finite()
        || tol <= 0.0
        || !p.iter().all(|v| v.is_finite())
        || !gradient.iter().all(|v| v.is_finite())
        || directions.is_empty()
    {
        return fail_closed(Vec::new());
    }

    let mut pairs = Vec::with_capacity(directions.len());
    let mut worst = 0.0f64;
    let mut informative_directions = 0usize;
    for d in directions {
        if !d.iter().all(|v| v.is_finite()) || d.iter().all(|&v| v == 0.0) {
            return fail_closed(pairs);
        }
        let dir_mag = d.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
        if !dir_mag.is_finite() || dir_mag == 0.0 {
            return fail_closed(pairs);
        }
        let mut analytic = 0.0;
        for (&gv, &dv) in gradient.iter().zip(d) {
            let term = gv * dv;
            if !term.is_finite() {
                return fail_closed(pairs);
            }
            analytic += term;
            if !analytic.is_finite() {
                return fail_closed(pairs);
            }
        }
        let mut plus = p.to_vec();
        let mut minus = p.to_vec();
        for i in 0..p.len() {
            let step = eps * d[i];
            if !step.is_finite() {
                return fail_closed(pairs);
            }
            let pv = p[i] + step;
            let mv = p[i] - step;
            if !pv.is_finite() || !mv.is_finite() {
                return fail_closed(pairs);
            }
            if d[i] != 0.0 && (pv.to_bits() == p[i].to_bits() || mv.to_bits() == p[i].to_bits()) {
                return fail_closed(pairs);
            }
            plus[i] = pv;
            minus[i] = mv;
        }
        let jp = j(&plus);
        let jm = j(&minus);
        if !jp.is_finite() || !jm.is_finite() {
            return fail_closed(pairs);
        }
        let fd = (jp - jm) / (2.0 * eps);
        if !fd.is_finite() {
            return fail_closed(pairs);
        }
        let scale_floor = 1e-12 * dir_mag;
        let signal = analytic.abs().max(fd.abs());
        if signal > scale_floor {
            informative_directions += 1;
        }
        let rel = (analytic - fd).abs() / signal.max(scale_floor);
        if !rel.is_finite() {
            return fail_closed(pairs);
        }
        worst = worst.max(rel);
        pairs.push((analytic, fd));
    }
    if informative_directions == 0 {
        return fail_closed(pairs);
    }
    let pass = pairs.len() == directions.len() && worst.is_finite() && worst < tol;
    GradientVerdict {
        max_rel_err: worst,
        pairs,
        informative_directions,
        pass,
    }
}

/// Solution of the static-equilibrium adjoint system `H(x*) λ = ∂J/∂x` on free DOFs.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjointSolution {
    /// Adjoint state per vertex (zero on fixed DOFs).
    pub lambda: Vec<Vec3>,
    /// Recomputed Euclidean relative residual `‖H λ − ∂J/∂x‖₂ / ‖∂J/∂x‖₂`.
    pub relative_residual: f64,
    /// Recomputed Rigal–Gaches normwise backward error.
    pub backward_error: f64,
    /// Whether an already-cached `LdltFactor` solved `H(x*)` without re-factorizing.
    pub reused_factor: bool,
}

/// Total IFT gradient `dJ/dp = ∂J/∂p − (∂R/∂p)ᵀ λ` of a scalar objective `J(x*(p), p)` at a
/// converged static cloth equilibrium with respect to homogeneous membrane stiffness
/// `Q = [Q11, Q22, Q12, Q33]` and hinge bending stiffness `B`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstitutiveGradient {
    /// `dJ / d[Q11, Q22, Q12, Q33]` (per unit N/m).
    pub dq: [f64; 4],
    /// `dJ / dB` (per unit N·m).
    pub d_bending: f64,
    /// Underlying adjoint solve evidence.
    pub adjoint: AdjointSolution,
}

/// Per-element Green-strain and adjoint virtual-strain kinematics at a converged static cloth
/// equilibrium, separated into softened separable kinematics (`eff_strain`, `w_sep`) and
/// unsoftened raw kinematics (`raw_strain`, `w_raw`) used by physical-area energies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MembranePullbackKinematics {
    /// Membrane element index in `solver.model.membranes`.
    pub element: usize,
    /// Reference triangle area `A_e` (m²).
    pub area: f64,
    /// Softened/effective Green strain `[E11, E22, 2 E12]` passed to separable membrane laws.
    pub eff_strain: [f64; 3],
    /// Unsoftened physical Green strain `[E11, E22, 2 E12]` passed to physical-area laws.
    pub raw_strain: [f64; 3],
    /// Adjoint virtual strain `B_sep λ` conjugate to separable stress `S_sep`.
    pub w_sep: [f64; 3],
    /// Adjoint virtual strain `B_raw λ` conjugate to physical-area stress `S_area`.
    pub w_raw: [f64; 3],
}

/// Total IFT gradient `dΦ/dθ = ∂Φ/∂θ − (∂R/∂θ)ᵀ λ` of a combined geometry/reaction functional
/// with respect to an arbitrary dynamic parameter vector `θ`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterGradient {
    /// Total derivative `dΦ / dθ_k` in caller parameter order.
    pub d_theta: Vec<f64>,
    /// Underlying adjoint solve evidence.
    pub adjoint: AdjointSolution,
}

/// Exact second-Piola stress derivatives `[∂S_area/∂k₁, ∂S_area/∂k₂]` of the physical-area
/// energy `W_area(J) = k₁ (J − 1)² + k₂ (ln J)²` with respect to its `[quadratic, log_squared]`
/// coefficients at raw physical Green strain `[E11, E22, 2 E12]`.
///
/// Uses the same cancellation-resistant `J − 1` metric and deterministic `ln(J)` evaluation as
/// `ftx_pattern::material_response::AreaResponse` (zero `mul_add`, zero platform `libm`).
///
/// # Errors
/// [`RefusalCode::OutOfRange`] if the strain is non-finite or fails to define a positive-definite
/// right Cauchy–Green metric `C = I + 2 E`; [`RefusalCode::NonFinite`] on arithmetic overflow.
pub fn area_response_stress_derivatives(raw_strain: [f64; 3]) -> Result<[[f64; 3]; 2], Refusal> {
    if raw_strain.iter().any(|v| !v.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "area-response stress derivative requires finite Green strain",
        ));
    }
    let [x, y, z] = raw_strain;
    let a = 1.0 + 2.0 * x;
    let b = 1.0 + 2.0 * y;
    if !a.is_finite() || !b.is_finite() || a <= 0.0 || b <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "area-response stress derivative requires a positive-definite metric",
        ));
    }
    let delta = 2.0 * x + 2.0 * y + (2.0 * x) * (2.0 * y) - z * z;
    let determinant = if delta.abs() < 0.5 {
        1.0 + delta
    } else {
        a * b - z * z
    };
    if !determinant.is_finite() || determinant <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "area-response stress derivative requires positive metric determinant",
        ));
    }
    let j = ftx_core::det::sqrt(determinant);
    let minus_one = if delta.abs() < 0.5 {
        delta / (j + 1.0)
    } else {
        j - 1.0
    };
    let log_j = if minus_one.abs() >= 0.25 {
        ftx_core::det::ln(j)
    } else {
        let t = minus_one / (2.0 + minus_one);
        let sq = t * t;
        let mut power = 1.0;
        let mut sum = 1.0;
        for n in 1..16 {
            power *= sq;
            sum += power / f64::from(2 * n + 1);
        }
        2.0 * t * sum
    };
    let inv_j = 1.0 / j;
    let grad_j = [(1.0 + 2.0 * y) * inv_j, (1.0 + 2.0 * x) * inv_j, -z * inv_j];
    let c1 = 2.0 * minus_one;
    let c2 = 2.0 * log_j * inv_j;
    let out = [grad_j.map(|g| c1 * g), grad_j.map(|g| c2 * g)];
    if out.iter().flatten().any(|v| !v.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "area-response stress derivative overflowed",
        ));
    }
    Ok(out)
}

/// Analytical chain rule converting `dJ / d[Q11, Q22, Q12, Q33]` into
/// `dJ / d[E1, E2, nu12, G12]` for [`Membrane::orthotropic`].
///
/// # Errors
/// [`RefusalCode::OutOfRange`] on non-positive moduli or thermodynamically inadmissible
/// `ν12² E2 / E1 ≥ 1`.
pub fn orthotropic_chain_rule(
    e1: f64,
    e2: f64,
    nu12: f64,
    g12: f64,
    dq: [f64; 4],
) -> Result<[f64; 4], Refusal> {
    if ![e1, e2, nu12, g12].iter().all(|v| v.is_finite())
        || !dq.iter().all(|v| v.is_finite())
        || e1 <= 0.0
        || e2 <= 0.0
        || g12 <= 0.0
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "orthotropic chain rule requires finite positive E1, E2, G12 and finite inputs",
        ));
    }
    let nu2 = nu12 * nu12;
    let denom = e1 - nu2 * e2;
    if !(denom.is_finite() && denom > 0.0) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "orthotropic Poisson coupling violates 1 - nu12 * nu21 > 0",
        ));
    }
    let inv_d2 = 1.0 / (denom * denom);
    let [g11, g22, g12q, g33] = dq;

    let dq11_de1 = e1 * (e1 - 2.0 * nu2 * e2) * inv_d2;
    let dq22_de1 = -nu2 * e2 * e2 * inv_d2;
    let dq12_de1 = -nu2 * nu12 * e2 * e2 * inv_d2;
    let de1 = g11 * dq11_de1 + g22 * dq22_de1 + g12q * dq12_de1;

    let dq11_de2 = e1 * e1 * nu2 * inv_d2;
    let dq22_de2 = e1 * e1 * inv_d2;
    let dq12_de2 = nu12 * e1 * e1 * inv_d2;
    let de2 = g11 * dq11_de2 + g22 * dq22_de2 + g12q * dq12_de2;

    let dq11_dnu = 2.0 * nu12 * e1 * e1 * e2 * inv_d2;
    let dq22_dnu = 2.0 * nu12 * e1 * e2 * e2 * inv_d2;
    let dq12_dnu = e1 * e2 * (e1 + nu2 * e2) * inv_d2;
    let dnu12 = g11 * dq11_dnu + g22 * dq22_dnu + g12q * dq12_dnu;

    let dg12 = g33;
    let out = [de1, de2, dnu12, dg12];
    if out.iter().any(|v| !v.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "orthotropic chain rule produced a non-finite derivative",
        ));
    }
    Ok(out)
}

impl Solver {
    /// Solve the static-equilibrium adjoint system `H(x*) λ = ∂J/∂x` on free DOFs.
    ///
    /// Reuses the cached `LdltFactor` if it already matches the exact tangent `H(x*)` to
    /// `≤ 1e-11` relative residual; otherwise factors the exact unprojected `H(x*)` once.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] if `dj_dx.len() != self.x.len()`;
    /// [`RefusalCode::NonFinite`] for non-finite inputs/outputs;
    /// [`RefusalCode::NotConverged`] if `H(x*)` is singular or the linear residual exceeds
    /// tolerance.
    pub fn equilibrium_adjoint(&mut self, dj_dx: &[Vec3]) -> Result<AdjointSolution, Refusal> {
        if self.x.len() != self.model.rest.len()
            || self.model.compression.len() != self.model.membranes.len()
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "adjoint solve requires cloth positions and compression arrays matching the model topology",
            ));
        }
        if dj_dx.len() != self.x.len() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "adjoint rhs length {} must match vertex count {}",
                    dj_dx.len(),
                    self.x.len()
                ),
            ));
        }
        if dj_dx.iter().any(|v| !v.is_finite()) || self.x.iter().any(|v| !v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "adjoint solve requires finite positions and objective gradient",
            ));
        }
        self.check_liquid_mass()?;
        self.damping.validate()?;
        self.check_static_air_load()?;
        self.check_constitutive_models()?;
        self.model.check_membrane_domains(&self.x)?;
        self.model.check_panel_bending_domains(&self.x)?;
        self.model.check_dihedral_domains(&self.x)?;
        let rhs = self.pack_free(dj_dx);
        let nfree = rhs.len();
        if nfree == 0 || rhs.iter().all(|&v| v == 0.0) {
            return Ok(AdjointSolution {
                lambda: vec![Vec3::ZERO; self.x.len()],
                relative_residual: 0.0,
                backward_error: 0.0,
                reused_factor: true,
            });
        }
        let (_, h_mat) = self.assemble_static_exact();
        let tol = 1e-10;
        let (lam_free, rel, eta, reused) = if let Some(cached) = self.cached_factor_solve(&rhs) {
            let rel = true_relative_residual(&h_mat, &rhs, &cached);
            let eta = rigal_gaches_backward_error(&h_mat, &rhs, &cached);
            if rel.is_finite() && (rel <= 1e-11 || eta <= BACKWARD_ERROR_ULPS * f64::EPSILON) {
                (cached, rel, eta, true)
            } else {
                let solved = self.factor_and_solve_exact(&h_mat, &rhs)?;
                let rel = true_relative_residual(&h_mat, &rhs, &solved);
                let eta = rigal_gaches_backward_error(&h_mat, &rhs, &solved);
                (solved, rel, eta, false)
            }
        } else {
            let solved = self.factor_and_solve_exact(&h_mat, &rhs)?;
            let rel = true_relative_residual(&h_mat, &rhs, &solved);
            let eta = rigal_gaches_backward_error(&h_mat, &rhs, &solved);
            (solved, rel, eta, false)
        };
        if lam_free.iter().any(|v| !v.is_finite())
            || !(rel.is_finite() && eta.is_finite())
            || (rel > tol && eta > BACKWARD_ERROR_ULPS * f64::EPSILON)
        {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                format!(
                    "adjoint solve did not meet residual gate (rel={rel:.3e}, backward_error={eta:.3e})"
                ),
            ));
        }
        Ok(AdjointSolution {
            lambda: self.unpack_free(&lam_free),
            relative_residual: rel,
            backward_error: eta,
            reused_factor: reused,
        })
    }

    /// Compute the exact IFT sensitivity of a scalar objective `J(x*(Q, B), Q, B)` at a converged
    /// static cloth equilibrium with respect to homogeneous membrane stiffness `[Q11, Q22, Q12, Q33]`
    /// and homogeneous hinge bending rigidity `B`.
    ///
    /// Returns `dJ/dp = ∂J/∂p − (∂R/∂p)ᵀ λ` where `H(x*) λ = ∂J/∂x`.
    ///
    /// # Errors
    /// Refuses fail-closed if the model uses heterogeneous materials, nonlinear membrane laws,
    /// panel-bending patches, or viscoelastic/plastic history fields (which homogeneous `[Q, B]`
    /// does not parameterize), or if [`Solver::equilibrium_adjoint`] fails.
    pub fn constitutive_gradient(
        &mut self,
        dj_dx: &[Vec3],
        dj_dq: [f64; 4],
        dj_db: f64,
    ) -> Result<ConstitutiveGradient, Refusal> {
        let zero_rxn = vec![Vec3::ZERO; self.x.len()];
        self.constitutive_gradient_with_reactions(dj_dx, &zero_rxn, dj_dq, dj_db)
    }

    /// Compute the exact IFT sensitivity of a linear Dirichlet reaction-force / torque functional
    /// `F(p) = Σ_{(i,k) fixed} reaction_weights[i][k] * R_i[k](x*(p), p)` at a converged static
    /// cloth equilibrium (for example axial holder force `F_z` or torque `τ_z = Σ (x_i × R_i)_z`
    /// in clamped strip or cylindrical-holder identification).
    ///
    /// Uses a single adjoint solve `H_{FF} λ_F = H_{FD} c_D` (reusing the cached `LdltFactor` when
    /// valid) with `λ_D = -c_D`, yielding `dF/dp = -(∂R/∂p)ᵀ λ` without finite differences.
    ///
    /// # Errors
    /// Same admission and linear-residual refusals as [`Solver::constitutive_gradient_with_reactions`].
    pub fn reaction_gradient(
        &mut self,
        reaction_weights: &[Vec3],
    ) -> Result<ConstitutiveGradient, Refusal> {
        let zero_x = vec![Vec3::ZERO; self.x.len()];
        self.constitutive_gradient_with_reactions(&zero_x, reaction_weights, [0.0; 4], 0.0)
    }

    /// Solve the unified static-equilibrium adjoint system for a combined geometry objective
    /// (`dj_dx`) and fixed-DOF Dirichlet reaction functional (`reaction_weights`):
    /// `H_{FF} λ_F = ∂J/∂x_F + H_{FD} c_D` with `λ_D = -c_D` on fixed DOFs.
    ///
    /// Admits memoryless conservative cloth models including characterized nonlinear and
    /// physical-area membrane laws (`element_responses`), reusing the cached exact `LdltFactor`
    /// whenever valid.
    ///
    /// # Errors
    /// Refuses fail-closed on length mismatch, non-finite inputs, heterogeneous overrides,
    /// panel bending, active friction, material history fields, or adjoint linear-residual failure.
    pub fn equilibrium_adjoint_with_reactions(
        &mut self,
        dj_dx: &[Vec3],
        reaction_weights: &[Vec3],
    ) -> Result<AdjointSolution, Refusal> {
        if dj_dx.len() != self.x.len() || reaction_weights.len() != self.x.len() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "adjoint input lengths ({}, {}) must match vertex count {}",
                    dj_dx.len(),
                    reaction_weights.len(),
                    self.x.len()
                ),
            ));
        }
        if dj_dx.iter().any(|v| !v.is_finite()) || reaction_weights.iter().any(|v| !v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "explicit objective and reaction derivatives must be finite",
            ));
        }
        self.check_constitutive_models()?;
        if !self.is_memoryless_elastic_for_adjoint() {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "equilibrium_adjoint_with_reactions requires a memoryless conservative cloth model \
                 (no per-element material overrides, panel bending, material history, or active friction)",
            ));
        }
        let has_fixed_weights = reaction_weights
            .iter()
            .enumerate()
            .any(|(i, w)| (0..3).any(|k| self.is_fixed_dof(i, k) && w[k] != 0.0));
        let eff_dj_dx: Vec<Vec3> = if has_fixed_weights {
            let h_fd_c = self.fixed_to_free_hessian_mul(reaction_weights);
            let h_fd_nodal = self.unpack_free(&h_fd_c);
            dj_dx
                .iter()
                .zip(&h_fd_nodal)
                .map(|(&a, &b)| a + b)
                .collect()
        } else {
            dj_dx.to_vec()
        };
        let mut adjoint = self.equilibrium_adjoint(&eff_dj_dx)?;
        // Set λ_D = -c_D on fixed (Dirichlet) DOFs so -(∂R/∂θ)ᵀ λ = c_Dᵀ (∂R_D/∂θ) - λ_Fᵀ (∂R_F/∂θ).
        if has_fixed_weights {
            for (i, w) in reaction_weights.iter().enumerate() {
                if self.is_fixed_dof(i, 0) {
                    adjoint.lambda[i].x = -w.x;
                }
                if self.is_fixed_dof(i, 1) {
                    adjoint.lambda[i].y = -w.y;
                }
                if self.is_fixed_dof(i, 2) {
                    adjoint.lambda[i].z = -w.z;
                }
            }
        }
        Ok(adjoint)
    }

    /// Compute exact IFT sensitivities `dΦ/dθ = ∂Φ/∂θ − (∂R/∂θ)ᵀ λ` of a combined geometry and
    /// Dirichlet-reaction functional with respect to an arbitrary dynamic membrane parameter vector
    /// `θ` (supporting scalar orthotropic, `TabulatedMembrane`, `AreaResponse`, and future
    /// memoryless nonlinear membrane parameterizations).
    ///
    /// Reuses the converged static exact `LdltFactor` across channels at the same equilibrium. The
    /// callback receives each membrane element's exact softened and raw virtual-strain kinematics
    /// and accumulates `A_e (∂S_sep/∂θ · w_sep + ∂S_area/∂θ · w_raw)` into `pull`.
    ///
    /// # Errors
    /// Same admission and linear-residual refusals as
    /// [`Solver::equilibrium_adjoint_with_reactions`], plus any callback or non-finite refusal.
    pub fn membrane_parameter_gradient_with_reactions<F>(
        &mut self,
        dj_dx: &[Vec3],
        reaction_weights: &[Vec3],
        dj_dtheta: &[f64],
        mut accumulate_element_pullback: F,
    ) -> Result<ParameterGradient, Refusal>
    where
        F: FnMut(&MembranePullbackKinematics, &mut [f64]) -> Result<(), Refusal>,
    {
        if dj_dtheta.iter().any(|v| !v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "explicit parameter derivatives must be finite",
            ));
        }
        let adjoint = self.equilibrium_adjoint_with_reactions(dj_dx, reaction_weights)?;
        let lam = &adjoint.lambda;
        let mut pull = vec![0.0f64; dj_dtheta.len()];
        for (i, e) in self.model.membranes.iter().enumerate() {
            let mat: Membrane = self.model.material(i);
            let mut f1 = Vec3::ZERO;
            let mut f2 = Vec3::ZERO;
            for (a, &vi) in e.v.iter().enumerate() {
                f1 += self.x[vi] * e.g[a][0];
                f2 += self.x[vi] * e.g[a][1];
            }
            let raw_strain = [
                0.5 * (f1.dot(f1) - 1.0),
                0.5 * (f2.dot(f2) - 1.0),
                f1.dot(f2),
            ];
            let mut w_raw = [0.0; 3];
            for (a, &vi) in e.v.iter().enumerate() {
                let (ga0, ga1) = (e.g[a][0], e.g[a][1]);
                let lv = lam[vi];
                w_raw[0] += ga0 * f1.dot(lv);
                w_raw[1] += ga1 * f2.dot(lv);
                w_raw[2] += ga0 * f2.dot(lv) + ga1 * f1.dot(lv);
            }
            let (eff_strain, w_sep) = if self.model.compression_model == CompressionModel::Principal
                && mat.compression != [1.0; 2]
            {
                let mapped = principal_strain_jets(raw_strain, &mat);
                (
                    mapped.map(|v| v.v),
                    mapped.map(|v| v.g[0] * w_raw[0] + v.g[1] * w_raw[1] + v.g[2] * w_raw[2]),
                )
            } else {
                let (eff, dphi, _) = soften(raw_strain, &mat);
                (eff, [dphi[0] * w_raw[0], dphi[1] * w_raw[1], w_raw[2]])
            };
            let kin = MembranePullbackKinematics {
                element: i,
                area: e.area,
                eff_strain,
                raw_strain,
                w_sep,
                w_raw,
            };
            accumulate_element_pullback(&kin, &mut pull)?;
        }
        let d_theta: Vec<f64> = dj_dtheta
            .iter()
            .zip(&pull)
            .map(|(&direct, &p)| direct - p)
            .collect();
        if d_theta.iter().any(|v| !v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "membrane parameter gradient pullback overflowed",
            ));
        }
        Ok(ParameterGradient { d_theta, adjoint })
    }

    /// Unified exact IFT sensitivity of `Φ(p) = J(x_F(p), Q, B) + c_Dᵀ R_D(x_F(p), x_D, Q, B)`,
    /// combining a free-DOF geometry objective (`dj_dx`) and a fixed-DOF Dirichlet reaction
    /// functional (`reaction_weights` restricted to fixed DOFs).
    ///
    /// # Errors
    /// Refuses fail-closed on length mismatch, non-finite inputs/outputs, heterogeneous or
    /// history-bearing cloth models, or adjoint linear-residual failure.
    pub fn constitutive_gradient_with_reactions(
        &mut self,
        dj_dx: &[Vec3],
        reaction_weights: &[Vec3],
        dj_dq: [f64; 4],
        dj_db: f64,
    ) -> Result<ConstitutiveGradient, Refusal> {
        if dj_dx.len() != self.x.len() || reaction_weights.len() != self.x.len() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "adjoint input lengths ({}, {}) must match vertex count {}",
                    dj_dx.len(),
                    reaction_weights.len(),
                    self.x.len()
                ),
            ));
        }
        if dj_dx.iter().any(|v| !v.is_finite())
            || reaction_weights.iter().any(|v| !v.is_finite())
            || !dj_dq.iter().all(|v| v.is_finite())
            || !dj_db.is_finite()
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "explicit objective and reaction derivatives must be finite",
            ));
        }
        self.check_constitutive_models()?;
        if !self.is_homogeneous_elastic_for_adjoint() {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "constitutive_gradient requires a homogeneous memoryless elastic cloth model \
                 (no per-element overrides, nonlinear membrane laws, panel bending, material history, or active friction)",
            ));
        }

        let membrane_grad = self.membrane_parameter_gradient_with_reactions(
            dj_dx,
            reaction_weights,
            &dj_dq,
            |kin, pull| {
                let a = kin.area;
                let e = kin.eff_strain;
                let w = kin.w_sep;
                pull[0] += a * e[0] * w[0];
                pull[1] += a * e[1] * w[1];
                pull[2] += a * (e[1] * w[0] + e[0] * w[1]);
                pull[3] += a * e[2] * w[2];
                Ok(())
            },
        )?;
        let lam = &membrane_grad.adjoint.lambda;

        // Hinge bending pullback: (∂R/∂B)ᵀ λ = Σ_h w_h (Σ_a k_a x_a) · (Σ_a k_a λ_a).
        let mut pull_b = 0.0f64;
        for h in &self.model.hinges {
            let mut s = Vec3::ZERO;
            let mut sl = Vec3::ZERO;
            for (&vi, ki) in h.v.iter().zip(h.k) {
                s += self.x[vi] * ki;
                sl += lam[vi] * ki;
            }
            pull_b += h.weight * s.dot(sl);
        }

        let dq = [
            membrane_grad.d_theta[0],
            membrane_grad.d_theta[1],
            membrane_grad.d_theta[2],
            membrane_grad.d_theta[3],
        ];
        let d_bending = dj_db - pull_b;
        if dq.iter().any(|v| !v.is_finite()) || !d_bending.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "constitutive gradient pullback overflowed",
            ));
        }
        Ok(ConstitutiveGradient {
            dq,
            d_bending,
            adjoint: membrane_grad.adjoint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloth::{ClothModel, grid};
    use ftx_core::Vec2;

    fn sagging_sheet(e1: f64, e2: f64, nu12: f64, g12: f64, bending: f64) -> Solver {
        let (rest, tris) = grid(0.12, 0.10, 4, 3, Vec2::ZERO);
        let x: Vec<Vec3> = rest
            .iter()
            .map(|p| Vec3::new(p.x, p.y, 0.008 * (p.x / 0.12) * (1.0 - p.x / 0.12)))
            .collect();
        let fixed: Vec<[bool; 3]> = rest
            .iter()
            .map(|p| {
                // Clamp the left edge (x = 0) and pin y along bottom-right to remove rigid modes
                // while allowing warp, weft, shear, AND bending to all carry active load.
                if p.x == 0.0 {
                    [true, true, true]
                } else {
                    [false, false, false]
                }
            })
            .collect();
        let mat = Membrane::orthotropic(e1, e2, nu12, g12, 0.15);
        let model = ClothModel::new(rest, &tris, mat, bending, 0.25).unwrap();
        let mut solver = Solver::new(model, x, &fixed, Vec3::new(1.5, 0.8, -9.81));
        solver.tol = 1e-14;
        solver
    }

    #[test]
    fn ift_constitutive_gradient_matches_central_fd_on_converged_3d_cloth() {
        let (e1, e2, nu12, g12, bending) = (320.0, 210.0, 0.24, 55.0, 4e-4);
        let mut base = sagging_sheet(e1, e2, nu12, g12, bending);
        base.pcg_iters = 1; // populate cached factor on final exact Newton step
        let rep = base.static_solve().unwrap();
        assert!(rep.converged);

        // Objective: J(x) = Σ_i (0.5 * ‖x_i‖² + c_i · x_i) -> smooth, sensitive to all modes.
        let obj = |x: &[Vec3]| -> f64 {
            x.iter()
                .enumerate()
                .map(|(i, p)| {
                    let w = 1.0 + 0.1 * (i as f64);
                    0.5 * p.norm2() + w * (p.z - 0.3 * p.x + 0.2 * p.y)
                })
                .sum()
        };
        let dj_dx: Vec<Vec3> = base
            .x
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let w = 1.0 + 0.1 * (i as f64);
                *p + Vec3::new(-0.3 * w, 0.2 * w, w)
            })
            .collect();

        let grad = base.constitutive_gradient(&dj_dx, [0.0; 4], 0.0).unwrap();
        assert!(grad.adjoint.relative_residual < 1e-11);
        // Second call at the exact same converged equilibrium must reuse the cached LDLT factor.
        let again = base.equilibrium_adjoint(&dj_dx).unwrap();
        assert!(
            again.reused_factor,
            "second adjoint solve at unchanged x* should reuse cached LDLT factor"
        );
        let d_ortho = orthotropic_chain_rule(e1, e2, nu12, g12, grad.dq).unwrap();
        let analytic = [
            d_ortho[0],
            d_ortho[1],
            d_ortho[2],
            d_ortho[3],
            grad.d_bending,
        ];
        let p0 = [e1, e2, nu12, g12, bending];

        // Probe every parameter axis plus combined oblique directions via fail-closed verify_gradient.
        let directions = vec![
            vec![10.0, 0.0, 0.0, 0.0, 0.0],
            vec![0.0, 10.0, 0.0, 0.0, 0.0],
            vec![0.0, 0.0, 1e-2, 0.0, 0.0],
            vec![0.0, 0.0, 0.0, 5.0, 0.0],
            vec![0.0, 0.0, 0.0, 0.0, 1e-4],
            vec![5.0, -4.0, 5e-3, 3.0, -2e-5],
        ];
        let eval_j = |p: &[f64]| -> f64 {
            let mut s = sagging_sheet(p[0], p[1], p[2], p[3], p[4]);
            s.x = base.x.clone();
            s.static_solve().unwrap();
            obj(&s.x)
        };
        let verdict = verify_gradient(&eval_j, &p0, &analytic, &directions, 1e-3, 1e-5);
        assert!(
            verdict.pass,
            "IFT adjoint gradient failed FD verification: {verdict:?}"
        );
        assert_eq!(verdict.informative_directions, directions.len());
        assert!(
            verdict.max_rel_err < 1e-5,
            "worst rel err = {:.3e}",
            verdict.max_rel_err
        );
    }

    #[test]
    fn verify_gradient_refuses_silent_zero_probes_fail_closed() {
        let constant = |_p: &[f64]| 42.0;
        let v = verify_gradient(
            &constant,
            &[1.0, 2.0],
            &[0.0, 0.0],
            &[vec![1.0, 0.0]],
            1e-5,
            1e-4,
        );
        assert!(!v.pass);
        assert_eq!(v.informative_directions, 0);
        assert!(v.max_rel_err.is_infinite());
    }

    #[test]
    fn ift_constitutive_gradient_matches_central_fd_under_active_compression_softening() {
        for compression_model in [CompressionModel::AxisAligned, CompressionModel::Principal] {
            // Compress the sheet along x while pulling softly along y and z so compression softening
            // (`raw[0] < 0`, `dphi[0] < 1`) is active at equilibrium while staying strictly stable.
            let make = |e1: f64, e2: f64, nu12: f64, g12: f64, bending: f64| {
                let (rest, tris) = grid(0.10, 0.10, 2, 2, Vec2::ZERO);
                let x: Vec<Vec3> = rest
                    .iter()
                    .map(|p| Vec3::new(p.x * 0.985, p.y * 1.01, 0.006 * (p.x / 0.10)))
                    .collect();
                let fixed: Vec<[bool; 3]> = rest
                    .iter()
                    .map(|p| {
                        if p.x == 0.0 || (p.x - 0.10).abs() < 1e-12 {
                            [true, true, true]
                        } else {
                            [false, false, false]
                        }
                    })
                    .collect();
                let mat = Membrane::orthotropic(e1, e2, nu12, g12, 0.12);
                let mut model = ClothModel::new(rest, &tris, mat, bending, 0.2).unwrap();
                model.compression_model = compression_model;
                let mut s = Solver::new(model, x, &fixed, Vec3::new(0.0, 0.5, -12.0));
                s.tol = 1e-12;
                s
            };
            let (e1, e2, nu12, g12, bending) = (250.0, 180.0, 0.2, 45.0, 2e-3);
            let mut base = make(e1, e2, nu12, g12, bending);
            base.static_solve().unwrap();
            let dj_dx: Vec<Vec3> = base
                .x
                .iter()
                .map(|&p| p + Vec3::new(0.1, -0.2, 1.0))
                .collect();
            let grad = base.constitutive_gradient(&dj_dx, [0.0; 4], 0.0).unwrap();
            let d_ortho = orthotropic_chain_rule(e1, e2, nu12, g12, grad.dq).unwrap();
            let analytic = [
                d_ortho[0],
                d_ortho[1],
                d_ortho[2],
                d_ortho[3],
                grad.d_bending,
            ];
            let p0 = [e1, e2, nu12, g12, bending];
            let dirs = vec![
                vec![1.0, 0.0, 0.0, 0.0, 0.0],
                vec![0.0, 1.0, 0.0, 0.0, 0.0],
                vec![0.0, 0.0, 0.0, 1.0, 0.0],
                vec![0.0, 0.0, 0.0, 0.0, 1e-4],
            ];
            let eval_j = |p: &[f64]| -> f64 {
                let mut s = make(p[0], p[1], p[2], p[3], p[4]);
                s.x = base.x.clone();
                s.static_solve().unwrap();
                s.x.iter()
                    .map(|p| 0.5 * p.norm2() + Vec3::new(0.1, -0.2, 1.0).dot(*p))
                    .sum()
            };
            let verdict = verify_gradient(&eval_j, &p0, &analytic, &dirs, 1e-4, 1e-5);
            assert!(
                verdict.pass,
                "compression-softened IFT adjoint failed FD gate: {verdict:?}"
            );
        }
    }

    #[test]
    fn ift_reaction_gradient_matches_central_fd_on_clamped_tensile_and_bent_strip() {
        // Clamped strip stretched along x, sheared along y, and deflected out-of-plane along z
        // so right-clamp axial force F_x and torque τ_z = Σ (x_i × R_i)_z depend nontrivially on
        // all 5 constitutive parameters [E1, E2, nu12, G12, B] through both direct ∂R_D/∂p and
        // indirect interior equilibrium deformation dx_F/dp.
        let make = |e1: f64, e2: f64, nu12: f64, g12: f64, bending: f64| {
            let (rest, tris) = grid(0.12, 0.08, 4, 3, Vec2::ZERO);
            let x: Vec<Vec3> = rest
                .iter()
                .map(|p| {
                    let s = p.x / 0.12;
                    Vec3::new(p.x * 1.06, p.y + 0.012 * s, 0.015 * s * (1.0 - 0.3 * s))
                })
                .collect();
            let fixed: Vec<[bool; 3]> = rest
                .iter()
                .map(|p| {
                    if p.x == 0.0 || (p.x - 0.12).abs() < 1e-12 {
                        [true, true, true]
                    } else {
                        [false, false, false]
                    }
                })
                .collect();
            let mat = Membrane::orthotropic(e1, e2, nu12, g12, 0.15);
            let model = ClothModel::new(rest, &tris, mat, bending, 0.2).unwrap();
            let mut s = Solver::new(model, x, &fixed, Vec3::new(0.0, 0.0, -9.81));
            s.tol = 1e-14;
            s
        };
        let (e1, e2, nu12, g12, bending) = (320.0, 210.0, 0.24, 55.0, 8e-4);
        let mut base = make(e1, e2, nu12, g12, bending);
        assert_eq!(base.pcg_iters, 0);
        base.static_solve().unwrap();

        // Linear reaction functional: axial force + torque about z at the right clamp (x == 0.12).
        let weights: Vec<Vec3> = base
            .model
            .rest
            .iter()
            .zip(&base.x)
            .map(|(r, p)| {
                if (r.x - 0.12).abs() < 1e-12 {
                    // w · R = R.x + 2.0 * (p.x * R.y - p.y * R.x)
                    Vec3::new(1.0 - 2.0 * p.y, 2.0 * p.x, 0.5)
                } else {
                    Vec3::ZERO
                }
            })
            .collect();
        let grad = base.reaction_gradient(&weights).unwrap();
        assert!(
            !grad.adjoint.reused_factor,
            "first adjoint solve on default pcg_iters == 0 solver populates the exact SPD factor"
        );
        // Subsequent channel solves at the same equilibrium must reuse the cached LDLT factor
        // even when `pcg_iters == 0` (the default for strip and cylinder calibration solvers).
        let second = base.reaction_gradient(&weights).unwrap();
        assert!(
            second.adjoint.reused_factor,
            "second adjoint solve with default pcg_iters == 0 must reuse cached SPD LDLT factor"
        );
        assert_eq!(second.dq, grad.dq);
        assert_eq!(second.d_bending, grad.d_bending);
        let d_ortho = orthotropic_chain_rule(e1, e2, nu12, g12, grad.dq).unwrap();
        let analytic = [
            d_ortho[0],
            d_ortho[1],
            d_ortho[2],
            d_ortho[3],
            grad.d_bending,
        ];
        let p0 = [e1, e2, nu12, g12, bending];
        let dirs = vec![
            vec![10.0, 0.0, 0.0, 0.0, 0.0],
            vec![0.0, 10.0, 0.0, 0.0, 0.0],
            vec![0.0, 0.0, 1e-2, 0.0, 0.0],
            vec![0.0, 0.0, 0.0, 5.0, 0.0],
            vec![0.0, 0.0, 0.0, 0.0, 1e-4],
            vec![5.0, -4.0, 5e-3, 3.0, -5e-5],
        ];
        let eval_rxn = |p: &[f64]| -> f64 {
            let mut s = make(p[0], p[1], p[2], p[3], p[4]);
            s.x = base.x.clone();
            s.static_solve().unwrap();
            let rxn = s.nodal_reactions();
            weights.iter().zip(&rxn).map(|(w, r)| w.dot(*r)).sum()
        };
        let verdict = verify_gradient(&eval_rxn, &p0, &analytic, &dirs, 1e-3, 1e-5);
        assert!(
            verdict.pass,
            "IFT reaction-force/torque gradient failed FD gate: {verdict:?}"
        );
        assert_eq!(verdict.informative_directions, dirs.len());
    }

    #[test]
    fn static_sensitivities_admit_only_wind_loads_that_vanish_at_rest() {
        let mut base = sagging_sheet(320.0, 210.0, 0.24, 55.0, 4e-4);
        base.static_solve().unwrap();
        let weights = vec![Vec3::new(0.1, -0.3, 1.0); base.x.len()];
        let gradient = |s: &mut Solver| {
            s.constitutive_gradient_with_reactions(&weights, &weights, [0.0; 4], 0.0)
        };
        let expected = gradient(&mut base.clone()).unwrap();
        let air = crate::AirDrag::standard();
        let moving = crate::AirDrag {
            air_velocity_m_s: [0.0, 3.0, 0.0],
            ..air
        };
        for drag in [
            air,
            crate::AirDrag {
                rho_air_kg_m3: 0.0,
                ..moving
            },
            crate::AirDrag {
                c_normal: 0.0,
                c_tangent: 0.0,
                ..moving
            },
        ] {
            let mut solver = base.clone();
            solver.damping = crate::DampingOptions {
                air_drag: Some(drag),
                rayleigh_membrane_s: 0.01,
                rayleigh_bending_s: 0.02,
            };
            let grad = gradient(&mut solver).unwrap();
            assert_eq!(grad.dq, expected.dq);
            assert_eq!(grad.d_bending, expected.d_bending);
            assert!(solver.static_solve().unwrap().converged);
        }

        base.damping.air_drag = Some(moving);
        let zeros = vec![Vec3::ZERO; base.x.len()];
        for error in [
            base.equilibrium_adjoint(&zeros).unwrap_err(),
            base.equilibrium_adjoint(&weights).unwrap_err(),
            gradient(&mut base).unwrap_err(),
            base.reaction_gradient(&weights).unwrap_err(),
            base.static_solve().unwrap_err(),
        ] {
            assert_eq!(error.code, RefusalCode::OutOfRange);
            assert!(error.message.contains("wind"));
        }
    }

    #[test]
    fn adjoint_refuses_active_friction_and_mismatched_compression_lengths() {
        let (rest, tris) = grid(0.06, 0.06, 2, 2, Vec2::ZERO);
        let x: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let fixed: Vec<[bool; 3]> = rest
            .iter()
            .map(|p| {
                if p.x == 0.0 {
                    [true, true, true]
                } else {
                    [false, false, false]
                }
            })
            .collect();
        let make_contact = |mu: f64| {
            let body = crate::contact::Body::new(
                vec![
                    Vec3::new(-0.05, -0.05, -0.02),
                    Vec3::new(0.12, -0.05, -0.02),
                    Vec3::new(-0.05, 0.12, -0.02),
                ],
                vec![[0, 1, 2]],
                0.01,
            )
            .unwrap();
            let params = crate::contact::ContactParams {
                dhat: 1e-3,
                thickness: 2e-4,
                kappa: 50.0,
                mu,
                eps_v: 1e-3,
            };
            let mat = Membrane::orthotropic(200.0, 200.0, 0.2, 40.0, 0.15);
            let model = ClothModel::new(rest.clone(), &tris, mat, 1e-4, 0.2).unwrap();
            let mut s = Solver::new(model, x.clone(), &fixed, Vec3::new(0.0, 0.0, -9.81))
                .with_contact(body, params)
                .unwrap();
            s.tol = 1e-12;
            s
        };
        let dl_dx = vec![Vec3::new(0.0, 0.0, 1.0); rest.len()];

        // Active Coulomb friction (mu > 0) violates homogeneous-elastic conservative adjoint scope.
        let mut with_friction = make_contact(0.25);
        with_friction.static_solve().unwrap();
        let err = with_friction
            .constitutive_gradient(&dl_dx, [0.0; 4], 0.0)
            .unwrap_err();
        assert_eq!(err.code, ftx_core::RefusalCode::OutOfRange);
        assert!(err.message.contains("friction"), "{err}");

        // Frictionless barrier contact (mu == 0) remains conservative and admitted.
        let mut frictionless = make_contact(0.0);
        frictionless.static_solve().unwrap();
        assert!(
            frictionless
                .constitutive_gradient(&dl_dx, [0.0; 4], 0.0)
                .is_ok()
        );

        // Authored crease rest angles under default Quadratic bending must refuse fail-closed on
        // both equilibrium_adjoint and constitutive_gradient before assembling a mismatched tangent.
        let mut creased = frictionless.clone();
        let mut angles = vec![0.0; creased.model.hinges.len()];
        angles[0] = 0.4;
        creased.model.set_hinge_rest_angles(&angles).unwrap();
        assert!(!creased.is_homogeneous_elastic_for_adjoint());
        assert_eq!(
            creased.equilibrium_adjoint(&dl_dx).unwrap_err().code,
            ftx_core::RefusalCode::InvalidDocument
        );
        assert_eq!(
            creased
                .constitutive_gradient(&dl_dx, [0.0; 4], 0.0)
                .unwrap_err()
                .code,
            ftx_core::RefusalCode::InvalidDocument
        );

        // DiscreteShells equilibrium_adjoint must refuse a degenerate / exact-±π dihedral hinge
        // cleanly with OutOfRange rather than feeding NaN into exact tangent factorization.
        let mut ds = frictionless.clone();
        ds.model.bending_model = crate::elements::BendingModel::DiscreteShells;
        let h0 = ds.model.hinges[0];
        ds.x[h0.v[3]] = (ds.x[h0.v[0]] + ds.x[h0.v[1]]) * 0.5;
        assert_eq!(
            ds.equilibrium_adjoint(&dl_dx).unwrap_err().code,
            ftx_core::RefusalCode::OutOfRange
        );

        // Mismatched compression table length must refuse cleanly rather than panic.
        frictionless.model.compression.push([1.0, 1.0]);
        let err = frictionless.equilibrium_adjoint(&dl_dx).unwrap_err();
        assert_eq!(err.code, ftx_core::RefusalCode::OutOfRange);
    }

    #[test]
    fn area_response_stress_derivatives_match_compiled_area_law_across_branches() {
        use ftx_pattern::material_response::{
            AREA_MODEL, AreaResponse, Interpolation, StressCurve, TabulatedMembrane,
        };
        use ftx_pattern::materials::{Evidence, Quantity, Source};
        use std::collections::BTreeMap;

        let cond = BTreeMap::from([
            ("specimen".into(), "test-area".into()),
            ("conditioning".into(), "synthetic".into()),
        ]);
        let sources = BTreeMap::from([(
            "s".into(),
            Source {
                reference: "test:area-deriv".into(),
                accessed_on: "2026-11-27".into(),
                note: "synthetic area derivative parity check".into(),
            },
        )]);
        let make_compiled = |k1: f64, k2: f64| {
            let curve = |slope: f64| StressCurve {
                points: vec![[-0.4, -0.4 * slope], [0.0, 0.0], [0.8, 0.8 * slope]],
                source: "s".into(),
                evidence: Evidence::Estimate,
                method: "test".into(),
                conditions: cond.clone(),
            };
            let q = |v: f64| Quantity {
                value: v,
                unit: "N/m".into(),
                evidence: Evidence::Estimate,
                source: "s".into(),
                method: "test".into(),
                conditions: cond.clone(),
                bounds: None,
            };
            TabulatedMembrane {
                model: ftx_pattern::material_response::MODEL.into(),
                interpolation: Interpolation::PiecewiseLinear,
                warp: curve(200.0),
                weft: curve(150.0),
                shear: curve(60.0),
                coupling: q(20.0),
                area_response: Some(AreaResponse {
                    model: AREA_MODEL.into(),
                    quadratic: q(k1),
                    log_squared: q(k2),
                    area_ratio_range: [0.3, 2.5],
                    conditions: cond.clone(),
                }),
            }
            .compile(&sources, true)
            .unwrap()
        };
        let unit_k1 = make_compiled(1.0, 0.0);
        let unit_k2 = make_compiled(0.0, 1.0);
        // Test zero strain, small near-1 J (atanh series branch), negative/sheared contraction,
        // and large dilation (|J - 1| >= 0.25 det::ln branch).
        for strain in [
            [0.0, 0.0, 0.0],
            [0.03, -0.015, 0.02],
            [-0.12, -0.08, -0.14],
            [0.35, 0.25, 0.18],
        ] {
            let [d1, d2] = area_response_stress_derivatives(strain).unwrap();
            let s1 = unit_k1.evaluate_area(strain).unwrap().stress;
            let s2 = unit_k2.evaluate_area(strain).unwrap().stress;
            for axis in 0..3 {
                assert!(
                    (d1[axis] - s1[axis]).abs() <= 1e-13 * (1.0 + s1[axis].abs()),
                    "strain={strain:?} axis={axis}: d1={} != s1={}",
                    d1[axis],
                    s1[axis]
                );
                assert!(
                    (d2[axis] - s2[axis]).abs() <= 1e-13 * (1.0 + s2[axis].abs()),
                    "strain={strain:?} axis={axis}: d2={} != s2={}",
                    d2[axis],
                    s2[axis]
                );
            }
        }
        assert!(area_response_stress_derivatives([f64::NAN, 0.0, 0.0]).is_err());
        assert!(area_response_stress_derivatives([-0.55, 0.0, 0.0]).is_err());
        assert!(area_response_stress_derivatives([0.0, 0.0, 1.2]).is_err());
    }

    #[test]
    fn ift_nonlinear_and_area_membrane_gradient_matches_central_fd_under_both_compression_models() {
        use ftx_pattern::material_response::{
            AREA_MODEL, AreaResponse, Interpolation, StressCurve, TabulatedMembrane,
        };
        use ftx_pattern::materials::{Evidence, Quantity, Source};
        use std::collections::BTreeMap;

        let cond = BTreeMap::from([
            ("specimen".into(), "test-nl-area".into()),
            ("conditioning".into(), "synthetic".into()),
        ]);
        let sources = BTreeMap::from([(
            "s".into(),
            Source {
                reference: "test:nl-area-adjoint".into(),
                accessed_on: "2026-11-27".into(),
                note: "synthetic nonlinear+area 3D adjoint check".into(),
            },
        )]);
        let knots: [&[f64]; 3] = [
            &[-0.25, 0.0, 0.03, 0.35],
            &[-0.25, 0.0, 0.03, 0.35],
            &[-0.35, 0.0, 0.35],
        ];
        let build_compiled = |p: &[f64]| {
            // p = [w_neg, w_p1, w_p2, f_neg, f_p1, f_p2, sh_neg, sh_pos, kc, k1, k2]
            let make_curve = |k: &[f64], slopes: &[f64]| {
                let origin = k.iter().position(|&v| v == 0.0).unwrap();
                let mut pts: Vec<[f64; 2]> = k.iter().map(|&e| [e, 0.0]).collect();
                for i in origin..k.len() - 1 {
                    pts[i + 1][1] = pts[i][1] + slopes[i] * (k[i + 1] - k[i]);
                }
                for i in (0..origin).rev() {
                    pts[i][1] = pts[i + 1][1] - slopes[i] * (k[i + 1] - k[i]);
                }
                StressCurve {
                    points: pts,
                    source: "s".into(),
                    evidence: Evidence::Estimate,
                    method: "test".into(),
                    conditions: cond.clone(),
                }
            };
            let q = |v: f64| Quantity {
                value: v,
                unit: "N/m".into(),
                evidence: Evidence::Estimate,
                source: "s".into(),
                method: "test".into(),
                conditions: cond.clone(),
                bounds: None,
            };
            TabulatedMembrane {
                model: ftx_pattern::material_response::MODEL.into(),
                interpolation: Interpolation::PiecewiseLinear,
                warp: make_curve(knots[0], &p[0..3]),
                weft: make_curve(knots[1], &p[3..6]),
                shear: make_curve(knots[2], &p[6..8]),
                coupling: q(p[8]),
                area_response: Some(AreaResponse {
                    model: AREA_MODEL.into(),
                    quadratic: q(p[9]),
                    log_squared: q(p[10]),
                    area_ratio_range: [0.5, 1.8],
                    conditions: cond.clone(),
                }),
            }
            .compile(&sources, true)
            .unwrap()
        };

        let p0 = [
            220.0, 280.0, 460.0, 140.0, 180.0, 310.0, 55.0, 75.0, 22.0, 28.0, 16.0,
        ];
        for compression_model in [CompressionModel::AxisAligned, CompressionModel::Principal] {
            let make = |p: &[f64]| {
                let compiled = build_compiled(p);
                let [e1, e2, nu, g12] = compiled.reference_moduli().unwrap();
                let (rest, tris) = grid(0.12, 0.08, 3, 2, Vec2::ZERO);
                // Stretch along x, compress slightly along y, shear and deflect out-of-plane so
                // positive/negative branches, both positive knots, compression softening, and area
                // response all contribute nontrivially.
                let x: Vec<Vec3> = rest
                    .iter()
                    .map(|pt| {
                        let s = pt.x / 0.12;
                        Vec3::new(
                            pt.x * 1.07,
                            pt.y * 0.985 + 0.012 * s,
                            0.01 * s * (1.0 - 0.25 * s),
                        )
                    })
                    .collect();
                let fixed: Vec<[bool; 3]> = rest
                    .iter()
                    .map(|pt| {
                        if pt.x == 0.0 || (pt.x - 0.12).abs() < 1e-12 {
                            [true, true, true]
                        } else {
                            [false, false, false]
                        }
                    })
                    .collect();
                let mat = Membrane::orthotropic(e1, e2, nu, g12, 0.2);
                let mut model = ClothModel::new(rest, &tris, mat, 4e-4, 0.2).unwrap();
                model.compression_model = compression_model;
                model
                    .set_membrane_responses(&vec![Some(compiled); tris.len()])
                    .unwrap();
                let mut s = Solver::new(model, x, &fixed, Vec3::new(0.0, 0.4, -9.81));
                s.tol = 1e-14;
                s
            };

            let mut base = make(&p0);
            base.static_solve().unwrap();
            assert!(!base.is_homogeneous_elastic_for_adjoint());
            assert!(base.is_memoryless_elastic_for_adjoint());

            let dj_dx: Vec<Vec3> = base
                .x
                .iter()
                .map(|&pt| pt + Vec3::new(0.15, -0.25, 0.8))
                .collect();
            let rxn_weights: Vec<Vec3> = base
                .model
                .rest
                .iter()
                .zip(&base.x)
                .map(|(r, pt)| {
                    if (r.x - 0.12).abs() < 1e-12 {
                        Vec3::new(1.0 - 1.5 * pt.y, 1.5 * pt.x, 0.4)
                    } else {
                        Vec3::ZERO
                    }
                })
                .collect();
            // Homogeneous constitutive_gradient must still refuse nonlinear element_responses.
            assert_eq!(
                base.constitutive_gradient(&dj_dx, [0.0; 4], 0.0)
                    .unwrap_err()
                    .code,
                RefusalCode::OutOfRange
            );

            let pullback = |kin: &MembranePullbackKinematics, pull: &mut [f64]| {
                let mut offset = 0usize;
                for (axis, k) in knots.iter().enumerate() {
                    let origin = k.iter().position(|&v| v == 0.0).unwrap();
                    let eps = kin.eff_strain[axis];
                    let wa = kin.area * kin.w_sep[axis];
                    if eps > 0.0 {
                        for i in origin..k.len() - 1 {
                            if eps <= k[i] {
                                break;
                            }
                            pull[offset + i] += wa * (eps.min(k[i + 1]) - k[i]);
                        }
                    } else if eps < 0.0 {
                        for i in (0..origin).rev() {
                            if eps >= k[i + 1] {
                                break;
                            }
                            pull[offset + i] -= wa * (k[i + 1] - eps.max(k[i]));
                        }
                    }
                    offset += k.len() - 1;
                }
                pull[8] += kin.area
                    * (kin.eff_strain[1] * kin.w_sep[0] + kin.eff_strain[0] * kin.w_sep[1]);
                let [ds1, ds2] = area_response_stress_derivatives(kin.raw_strain)?;
                pull[9] += kin.area
                    * (ds1[0] * kin.w_raw[0] + ds1[1] * kin.w_raw[1] + ds1[2] * kin.w_raw[2]);
                pull[10] += kin.area
                    * (ds2[0] * kin.w_raw[0] + ds2[1] * kin.w_raw[1] + ds2[2] * kin.w_raw[2]);
                Ok(())
            };
            let grad = base
                .membrane_parameter_gradient_with_reactions(
                    &dj_dx,
                    &rxn_weights,
                    &[0.0; 11],
                    pullback,
                )
                .unwrap();
            assert!(!grad.adjoint.reused_factor);
            // Second channel at the same converged equilibrium must reuse the cached SPD LDLT factor.
            let second = base
                .membrane_parameter_gradient_with_reactions(
                    &dj_dx,
                    &rxn_weights,
                    &[0.0; 11],
                    pullback,
                )
                .unwrap();
            assert!(second.adjoint.reused_factor);
            assert_eq!(second.d_theta, grad.d_theta);

            let eval_phi = |p: &[f64]| -> f64 {
                let mut s = make(p);
                s.x = base.x.clone();
                s.static_solve().unwrap();
                let geom: f64 =
                    s.x.iter()
                        .map(|pt| 0.5 * pt.norm2() + Vec3::new(0.15, -0.25, 0.8).dot(*pt))
                        .sum();
                let rxn = s.nodal_reactions();
                let force: f64 = rxn_weights.iter().zip(&rxn).map(|(w, r)| w.dot(*r)).sum();
                geom + force
            };
            let dirs: Vec<Vec<f64>> = (0..p0.len())
                .map(|k| {
                    let mut d = vec![0.0; p0.len()];
                    d[k] = 1.0;
                    d
                })
                .collect();
            let verdict = verify_gradient(&eval_phi, &p0, &grad.d_theta, &dirs, 1e-4, 1e-5);
            assert!(
                verdict.pass,
                "nonlinear+area adjoint failed FD gate under {compression_model:?}: {verdict:?}"
            );
            assert!(verdict.informative_directions >= 7, "{verdict:?}");
        }
    }
}
