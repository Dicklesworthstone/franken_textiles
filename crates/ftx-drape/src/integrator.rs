//! Chung–Hulbert Generalized-α structural time integrator (second-order accurate with
//! controllable high-frequency spectral radius `ρ_∞ ∈ [0, 1]`).
//!
//! Provenance: adapted from FrankenSim `crates/fs-time/src/galpha.rs`
//! (`frankensim-source-manifest.json` `source_root`
//! `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`; MIT + OpenAI/Anthropic
//! rider). Adaptations for FrankenTextiles:
//! - expressed in variational incremental-potential form for the cloth solver:
//!   `E_ga(x_{n+1}) = ½ c_m (x_{n+1} − x̃)ᵀ M (x_{n+1} − x̃) + (1 / (1 − α_f)) Ψ((1 − α_f) x_{n+1} + α_f x_n)`,
//!   whose gradient `c_m M (x_{n+1} − x̃) + ∇Ψ(x_{n+1−α_f})` and symmetric Hessian
//!   `c_m M + (1 − α_f) ∇²Ψ(x_{n+1−α_f})` are exact Chung–Hulbert equations and work natively
//!   with projected Newton, Armijo backtracking, and contact CCD;
//! - zero `unsafe`, zero `mul_add`, zero platform `libm`.

use ftx_core::{Refusal, RefusalCode, Vec3};
use serde::{Deserialize, Serialize};

/// Time-integration scheme for [`crate::cloth::Solver::step`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimeIntegrator {
    /// First-order backward Euler (the legacy default, bit-identical to `HEAD`).
    #[default]
    ImplicitEuler,
    /// Second-order Chung–Hulbert Generalized-α with high-frequency spectral radius
    /// `rho_inf ∈ [0.0, 1.0]`:
    /// - `1.0`: zero numerical dissipation (midpoint-like energy conservation on linear modes);
    /// - `0.8`: light high-frequency damping, preserves dynamic cloth swing;
    /// - `0.0`: annihilates `ω h → ∞` modes in one step while keeping 2nd-order accuracy.
    GeneralizedAlpha {
        /// Spectral radius at infinite frequency, in `[0, 1]`.
        rho_inf: f64,
    },
}

impl TimeIntegrator {
    /// Whether this is the legacy default [`TimeIntegrator::ImplicitEuler`].
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::ImplicitEuler)
    }

    /// Validate time-integrator parameters up front.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] if `rho_inf` is non-finite or outside `[0, 1]`.
    pub fn validate(&self) -> Result<(), Refusal> {
        match *self {
            Self::ImplicitEuler => Ok(()),
            Self::GeneralizedAlpha { rho_inf } => {
                GeneralizedAlphaParams::new(rho_inf)?;
                Ok(())
            }
        }
    }
}

/// Validated Chung–Hulbert Generalized-α parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeneralizedAlphaParams {
    /// High-frequency spectral radius `ρ_∞ ∈ [0, 1]`.
    pub rho_inf: f64,
    /// Inertia midpoint parameter `α_m = (2 ρ_∞ − 1) / (ρ_∞ + 1)`.
    pub alpha_m: f64,
    /// Internal-force midpoint parameter `α_f = ρ_∞ / (ρ_∞ + 1)`.
    pub alpha_f: f64,
    /// Newmark velocity weight `γ = ½ − α_m + α_f`.
    pub gamma: f64,
    /// Newmark displacement weight `β = ¼ (1 − α_m + α_f)²`.
    pub beta: f64,
}

impl GeneralizedAlphaParams {
    /// Validate `rho_inf ∈ [0, 1]` and compute exact Chung–Hulbert parameters.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] if `rho_inf` is non-finite or outside `[0, 1]`.
    pub fn new(rho_inf: f64) -> Result<Self, Refusal> {
        if !(rho_inf.is_finite() && (0.0..=1.0).contains(&rho_inf)) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("generalized-alpha rho_inf {rho_inf} must lie in [0, 1]"),
            ));
        }
        let alpha_m = (2.0 * rho_inf - 1.0) / (rho_inf + 1.0);
        let alpha_f = rho_inf / (rho_inf + 1.0);
        let gamma = 0.5 - alpha_m + alpha_f;
        let d = 1.0 - alpha_m + alpha_f;
        let beta = 0.25 * d * d;
        Ok(Self {
            rho_inf,
            alpha_m,
            alpha_f,
            gamma,
            beta,
        })
    }

    /// Effective mass stiffness coefficient `c_m = (1 − α_m) / (β h²)`.
    #[must_use]
    pub fn mass_coeff(&self, h: f64) -> f64 {
        (1.0 - self.alpha_m) / (self.beta * h * h)
    }

    /// Elastic-force evaluation weight `1 − α_f ∈ [0.5, 1.0]`.
    #[must_use]
    pub fn elastic_weight(&self) -> f64 {
        1.0 - self.alpha_f
    }

    /// Predictor position `x̃` such that `c_m M (x_{n+1} − x̃) = M a_{n+1−α_m} − M g`.
    #[must_use]
    pub fn predictor(&self, x0: Vec3, v0: Vec3, a0: Vec3, gravity: Vec3, h: f64) -> Vec3 {
        let a_weight = h * h * (0.5 - self.beta / (1.0 - self.alpha_m));
        let g_weight = self.beta * h * h / (1.0 - self.alpha_m);
        x0 + v0 * h + a0 * a_weight + gravity * g_weight
    }

    /// Generalized-midpoint position `x_{n+1−α_f} = (1 − α_f) x_{n+1} + α_f x_n`.
    #[must_use]
    pub fn eval_pos(&self, x_new: Vec3, x0: Vec3) -> Vec3 {
        x_new * (1.0 - self.alpha_f) + x0 * self.alpha_f
    }

    /// Newmark corrector `(v_{n+1}, a_{n+1})` from accepted `x_{n+1}`.
    #[must_use]
    pub fn corrector(&self, x0: Vec3, v0: Vec3, a0: Vec3, x_new: Vec3, h: f64) -> (Vec3, Vec3) {
        let dx = x_new - x0;
        let a_new = dx * (1.0 / (self.beta * h * h))
            - v0 * (1.0 / (self.beta * h))
            - a0 * (0.5 / self.beta - 1.0);
        let v_new = v0 + (a0 * (1.0 - self.gamma) + a_new * self.gamma) * h;
        (v_new, a_new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chung_hulbert_limiting_cases_and_refusals() {
        let p1 = GeneralizedAlphaParams::new(1.0).unwrap();
        assert!((p1.alpha_m - 0.5).abs() < 1e-15);
        assert!((p1.alpha_f - 0.5).abs() < 1e-15);
        assert!((p1.gamma - 0.5).abs() < 1e-15);
        assert!((p1.beta - 0.25).abs() < 1e-15);

        let p0 = GeneralizedAlphaParams::new(0.0).unwrap();
        assert!((p0.alpha_m + 1.0).abs() < 1e-15);
        assert!((p0.alpha_f).abs() < 1e-15);
        assert!((p0.gamma - 1.5).abs() < 1e-15);
        assert!((p0.beta - 1.0).abs() < 1e-15);

        assert!(GeneralizedAlphaParams::new(-0.01).is_err());
        assert!(GeneralizedAlphaParams::new(1.01).is_err());
        assert!(GeneralizedAlphaParams::new(f64::NAN).is_err());
    }

    #[test]
    fn predictor_matches_exact_generalized_alpha_inertia_identity() {
        // Verify c_m (x_new - x_tilde) == (1 - alpha_m) a_new + alpha_m a0 - g
        // for arbitrary (x0, v0, a0, x_new, g, h, rho_inf).
        for rho in [0.0, 0.3, 0.7, 1.0] {
            let p = GeneralizedAlphaParams::new(rho).unwrap();
            let h = 0.013;
            let x0 = Vec3::new(0.1, -0.2, 0.3);
            let v0 = Vec3::new(-0.4, 0.5, 0.6);
            let a0 = Vec3::new(1.2, -3.4, 2.1);
            let g = Vec3::new(0.0, 0.0, -9.81);
            let x_new = Vec3::new(0.11, -0.19, 0.295);
            let xt = p.predictor(x0, v0, a0, g, h);
            let (_, a_new) = p.corrector(x0, v0, a0, x_new, h);
            let lhs = (x_new - xt) * p.mass_coeff(h);
            let rhs = a_new * (1.0 - p.alpha_m) + a0 * p.alpha_m - g;
            assert!(
                (lhs - rhs).norm() < 1e-11,
                "rho={rho}: lhs={lhs:?} rhs={rhs:?}"
            );
        }
    }
}
