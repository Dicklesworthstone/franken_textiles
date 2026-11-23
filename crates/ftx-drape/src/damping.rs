//! Physical aerodynamic drag and SE(3)-invariant Rayleigh internal damping for [`crate::cloth::Solver`].
//!
//! - **Aerodynamic drag** ([`AirDrag`]): anisotropic per-triangle crossflow (normal) and skin-friction
//!   (tangential) drag formulated as a strictly convex `C^∞` variational step potential with lagged
//!   step-start triangle area and unit normal. Its `9 × 9` element Hessian is symmetric positive
//!   semidefinite by construction (`psd = true`) and shares the exact sparsity pattern of
//!   [`crate::elements::MembraneElement`] (zero new matrix nonzeros). Supports both backward Euler
//!   (`v = (x − x₀)/h`) and Chung–Hulbert Generalized-α midpoint velocity (`v_{n+1−α_f}`).
//! - **Rayleigh internal damping** ([`DampingOptions::rayleigh_membrane_s`],
//!   [`DampingOptions::rayleigh_bending_s`]): stiffness-proportional Kelvin–Voigt strain-rate and
//!   dihedral curvature-rate potentials. Because Green strain `E(x)` and dihedral angle `θ(x)` are
//!   exact Euclidean invariants, both terms exert **exact zero force** on arbitrary 3D rigid
//!   translation and rotation (no parasitic molasses drag in free fall or swinging motion).
//!   Source-selected panel laws use their directional curvature tensor and full reference area,
//!   replacing their inactive scalar hinges. Vector-Hessian curvature increments are expressed in
//!   a source-triangle frame carried by the garment; normal-projected increments use raw covariant
//!   curvature, independent of preferred shape. Both keep rigid motion out of the rate response.
//!
//! Internal rates use backward differences of endpoint strain/curvature even when the structural
//! integrator is generalized-alpha. Its aerodynamic velocity is the generalized midpoint, but the
//! internal-rate extension is not a second-order Kelvin-Voigt time discretization.
//!
//! Honesty boundary: aerodynamic drag coefficients are uncalibrated reference defaults unless
//! measured in experiment. Physics KATs verify terminal velocity, anisotropy, and SE(3) invariance;
//! they do not certify real garment fit.

use std::f64::consts::PI;

use ftx_core::{Refusal, RefusalCode, Vec3};
use serde::{Deserialize, Serialize};

use crate::bending::PanelRateState;
use crate::cloth::ClothModel;
use crate::elements::Local;

#[allow(clippy::trivially_copy_pass_by_ref)] // serde skip_serializing_if requires &T
fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

/// Anisotropic per-triangle aerodynamic drag parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AirDrag {
    /// Ambient air density `ρ_air` (kg/m³; standard sea-level dry air is `1.225`).
    pub rho_air_kg_m3: f64,
    /// Normal (crossflow / bluff-face) drag coefficient `C_n >= 0` (flat-plate reference `1.2`).
    pub c_normal: f64,
    /// Tangential (skin-friction) drag coefficient `C_t >= 0` (reference `0.05`).
    pub c_tangent: f64,
    /// Smooth velocity regularization `ε_v > 0` (m/s; default `1e-3`). Transitions smoothly from
    /// quadratic drag (`|v| ≫ ε_v`) to linear viscous drag near zero speed so the potential is
    /// `C^∞` with strictly bounded PSD Hessian.
    pub eps_v_m_s: f64,
}

impl Default for AirDrag {
    fn default() -> Self {
        Self::standard()
    }
}

impl AirDrag {
    /// Sea-level dry air (`1.225 kg/m³`) with uncalibrated flat-plate reference drag coefficients
    /// (`C_n = 1.2`, `C_t = 0.05`, `ε_v = 1e-3 m/s`).
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            rho_air_kg_m3: 1.225,
            c_normal: 1.2,
            c_tangent: 0.05,
            eps_v_m_s: 1e-3,
        }
    }

    /// Validate aerodynamic parameters before stepping or meshing.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] or [`RefusalCode::NonFinite`] for negative/non-finite
    /// density/coefficients or non-positive velocity regularization.
    pub fn validate(&self) -> Result<(), Refusal> {
        Refusal::finite("air density", self.rho_air_kg_m3)?;
        Refusal::finite("normal air drag coefficient", self.c_normal)?;
        Refusal::finite("tangential air drag coefficient", self.c_tangent)?;
        Refusal::finite("air drag velocity regularization", self.eps_v_m_s)?;
        if self.rho_air_kg_m3 < 0.0 || self.c_normal < 0.0 || self.c_tangent < 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "air density and drag coefficients must be nonnegative",
            ));
        }
        if self.eps_v_m_s <= 0.0 || self.eps_v_m_s * self.eps_v_m_s <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "air drag velocity regularization eps_v_m_s must be positive without underflowing eps_v^2",
            ));
        }
        Refusal::finite(
            "air drag velocity regularization cubed",
            self.eps_v_m_s * self.eps_v_m_s * self.eps_v_m_s,
        )?;
        Ok(())
    }

    /// Instantaneous resistive force (`-F_drag(v)`, i.e. gradient contribution to `M a + ∇Ψ − F_drag = M g`)
    /// distributed equally across the three vertices of a triangle with area `area` and unit normal `normal`.
    #[must_use]
    pub fn triangle_resistance_force(
        &self,
        area: f64,
        normal: Vec3,
        centroid_velocity: Vec3,
    ) -> Vec3 {
        if area <= 0.0 || self.rho_air_kg_m3 == 0.0 {
            return Vec3::ZERO;
        }
        let s_n = centroid_velocity.dot(normal);
        let v_t = centroid_velocity - normal * s_n;
        let eps2 = self.eps_v_m_s * self.eps_v_m_s;
        let u_n = (s_n * s_n + eps2).sqrt();
        let u_t = (v_t.norm2() + eps2).sqrt();
        let pref = 0.5 * self.rho_air_kg_m3 * area;
        normal * (pref * self.c_normal * u_n * s_n) + v_t * (pref * self.c_tangent * u_t)
    }
}

/// Opt-in physical aerodynamic drag and SE(3)-invariant Rayleigh internal structural damping.
///
/// Default is completely disabled (`air_drag = None`, both Rayleigh constants `0.0`), preserving
/// bit-identical legacy vacuum trajectories and omitted JSON keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DampingOptions {
    /// Anisotropic per-triangle aerodynamic drag (`None` disables).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub air_drag: Option<AirDrag>,
    /// Stiffness-proportional membrane Green-strain-rate Kelvin–Voigt damping time constant `β_m`
    /// (seconds; `0.0` disables). Strictly SE(3)-invariant.
    #[serde(skip_serializing_if = "is_zero_f64")]
    pub rayleigh_membrane_s: f64,
    /// Stiffness-proportional curvature-rate damping time constant `β_b` (seconds; `0.0` disables).
    /// Uses source-panel tensors where selected, and scalar dihedral hinges elsewhere. SE(3)-invariant.
    #[serde(skip_serializing_if = "is_zero_f64")]
    pub rayleigh_bending_s: f64,
}

impl DampingOptions {
    /// Whether all physical damping terms are disabled (the legacy vacuum default).
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.air_drag.is_none() && self.rayleigh_membrane_s == 0.0 && self.rayleigh_bending_s == 0.0
    }

    /// Validate all configured damping parameters.
    ///
    /// # Errors
    /// Non-finite or negative parameters, or invalid [`AirDrag`] settings.
    pub fn validate(&self) -> Result<(), Refusal> {
        if let Some(drag) = self.air_drag {
            drag.validate()?;
        }
        Refusal::finite(
            "Rayleigh membrane damping time constant",
            self.rayleigh_membrane_s,
        )?;
        Refusal::finite(
            "Rayleigh bending damping time constant",
            self.rayleigh_bending_s,
        )?;
        if self.rayleigh_membrane_s < 0.0 || self.rayleigh_bending_s < 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "Rayleigh damping time constants must be nonnegative",
            ));
        }
        Ok(())
    }
}

/// Affine map from end-of-step vertex position `x[v]` to the integrator's evaluated velocity
/// `v_eval[v] = v_base[v] + c_v * (x[v] - x0[v])`.
#[derive(Clone, Debug)]
pub(crate) enum VelocityMap<'a> {
    /// Backward Euler: `v_eval(x) = (x - x0) / h`.
    ImplicitEuler { x0: &'a [Vec3], inv_h: f64 },
    /// Chung–Hulbert Generalized-α midpoint velocity `v_{n+1−α_f}(x) = v_base + c_v * (x - x0)`.
    GeneralizedAlpha {
        x0: &'a [Vec3],
        v_base: Vec<Vec3>,
        c_v: f64,
    },
}

impl<'a> VelocityMap<'a> {
    fn c_v(&self) -> f64 {
        match *self {
            Self::ImplicitEuler { inv_h, .. } => inv_h,
            Self::GeneralizedAlpha { c_v, .. } => c_v,
        }
    }

    fn centroid_velocity(&self, x: &[Vec3], v: [usize; 3]) -> Vec3 {
        match self {
            Self::ImplicitEuler { x0, inv_h } => {
                let d0 = x[v[0]] - x0[v[0]];
                let d1 = x[v[1]] - x0[v[1]];
                let d2 = x[v[2]] - x0[v[2]];
                (d0 + d1 + d2) * (*inv_h / 3.0)
            }
            Self::GeneralizedAlpha { x0, v_base, c_v } => {
                let b = (v_base[v[0]] + v_base[v[1]] + v_base[v[2]]) * (1.0 / 3.0);
                let d = (x[v[0]] - x0[v[0]]) + (x[v[1]] - x0[v[1]]) + (x[v[2]] - x0[v[2]]);
                b + d * (*c_v / 3.0)
            }
        }
    }
}

/// Per-step frozen geometry/strain/angle snapshot computed once at `x0` before Newton iterates.
#[derive(Clone, Debug)]
pub(crate) struct DampingStep<'a> {
    pub(crate) options: DampingOptions,
    pub(crate) h: f64,
    pub(crate) vel: VelocityMap<'a>,
    /// Current triangle area and unit normal at `x0` (empty when `air_drag` is `None`).
    pub(crate) tri_frames: Vec<(f64, Vec3)>,
    /// Reference Green strain `E(x0)` per membrane triangle (empty when `rayleigh_membrane_s == 0`).
    pub(crate) tri_e0: Vec<[f64; 3]>,
    /// Reference dihedral angle `θ(x0)` per hinge (empty when `rayleigh_bending_s == 0`).
    pub(crate) hinge_theta0: Vec<f64>,
    /// Source-panel curvature in objective coordinates, with each original directional tensor.
    panel_curvature0: Vec<PanelRateState>,
}

impl<'a> DampingStep<'a> {
    /// Builds the frozen step snapshot, returning `None` immediately when all damping is disabled.
    pub(crate) fn build(
        model: &ClothModel,
        options: DampingOptions,
        x0: &'a [Vec3],
        h: f64,
        vel: VelocityMap<'a>,
    ) -> Result<Option<Self>, Refusal> {
        if options.is_default() {
            return Ok(None);
        }
        options.validate()?;
        let c_v = vel.c_v();
        if !(h.is_finite()
            && h > 0.0
            && c_v.is_finite()
            && c_v > 0.0
            && (1.0 / c_v).is_finite()
            && (options.rayleigh_membrane_s / h).is_finite()
            && (options.rayleigh_bending_s / h).is_finite())
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("time step {h} s overflows damping velocity or strain-rate scaling"),
            ));
        }
        let tri_frames = if options.air_drag.is_some() {
            model
                .membranes
                .iter()
                .map(|e| {
                    let cross = (x0[e.v[1]] - x0[e.v[0]]).cross(x0[e.v[2]] - x0[e.v[0]]);
                    let norm = cross.norm();
                    if norm.is_finite() && norm > 0.0 {
                        (0.5 * norm, cross * (1.0 / norm))
                    } else {
                        (0.0, Vec3::ZERO)
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let tri_e0 = if options.rayleigh_membrane_s > 0.0 {
            model.membranes.iter().map(|e| e.strain(x0)).collect()
        } else {
            Vec::new()
        };
        let hinge_theta0 = if options.rayleigh_bending_s > 0.0 {
            let mut out = Vec::with_capacity(model.hinges.len());
            for (i, hinge) in model.hinges.iter().enumerate() {
                if model.hinge_rigidity(i) > 0.0 {
                    let th = hinge.dihedral_angle(x0);
                    if !th.is_finite() {
                        return Err(Refusal::new(
                            RefusalCode::DegenerateGeometry,
                            format!(
                                "hinge {i} is degenerate at step start; Rayleigh bending damping requires non-degenerate adjacent triangles"
                            ),
                        ));
                    }
                    out.push(th);
                } else {
                    out.push(0.0);
                }
            }
            out
        } else {
            Vec::new()
        };
        let panel_curvature0 = if options.rayleigh_bending_s > 0.0 {
            model
                .panel_bending_patches()
                .iter()
                .map(|patch| patch.rate_state(x0))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        Ok(Some(Self {
            options,
            h,
            vel,
            tri_frames,
            tri_e0,
            hinge_theta0,
            panel_curvature0,
        }))
    }

    /// Total damping potential energy across all triangles and hinges at candidate `x`.
    #[must_use]
    pub(crate) fn energy(&self, model: &ClothModel, x: &[Vec3]) -> f64 {
        let mut total = 0.0;
        if let Some(drag) = self.options.air_drag {
            let c_v = self.vel.c_v();
            for (i, e) in model.membranes.iter().enumerate() {
                let (area, normal) = self.tri_frames[i];
                if area <= 0.0 {
                    continue;
                }
                let v = self.vel.centroid_velocity(x, e.v);
                total += air_drag_energy(drag, area, normal, v, c_v);
            }
        }
        if self.options.rayleigh_membrane_s > 0.0 {
            let c = self.options.rayleigh_membrane_s / self.h;
            for (i, e) in model.membranes.iter().enumerate() {
                let q = model.material(i).q;
                total += e.strain_rate_energy(x, self.tri_e0[i], &q, c);
            }
        }
        if self.options.rayleigh_bending_s > 0.0 {
            let rate = self.options.rayleigh_bending_s / self.h;
            for (i, hinge) in model.hinges.iter().enumerate() {
                let b = model.hinge_rigidity(i);
                if b <= 0.0 {
                    continue;
                }
                let theta = hinge.dihedral_angle(x);
                let d = wrap_angle_diff(theta - self.hinge_theta0[i]);
                total += 0.5 * rate * b * hinge.dihedral_weight() * d * d;
            }
            for (patch, &previous) in model
                .panel_bending_patches()
                .iter()
                .zip(&self.panel_curvature0)
            {
                total += patch.rate_energy(x, previous, rate);
            }
        }
        total
    }

    /// Per-triangle aerodynamic drag local potential (`psd = true`).
    #[must_use]
    pub(crate) fn air_drag_local(
        &self,
        tri_index: usize,
        verts: [usize; 3],
        x: &[Vec3],
    ) -> Option<Local<9, 81>> {
        let drag = self.options.air_drag?;
        let (area, normal) = self.tri_frames[tri_index];
        if area <= 0.0 || drag.rho_air_kg_m3 == 0.0 {
            return None;
        }
        let v = self.vel.centroid_velocity(x, verts);
        Some(air_drag_local(drag, area, normal, v, self.vel.c_v()))
    }

    /// Per-triangle SE(3)-invariant Rayleigh membrane strain-rate local potential.
    #[must_use]
    pub(crate) fn rayleigh_membrane_local(
        &self,
        model: &ClothModel,
        tri_index: usize,
        x: &[Vec3],
    ) -> Option<Local<9, 81>> {
        if self.options.rayleigh_membrane_s <= 0.0 {
            return None;
        }
        let c = self.options.rayleigh_membrane_s / self.h;
        let q = model.material(tri_index).q;
        Some(model.membranes[tri_index].strain_rate_local(x, self.tri_e0[tri_index], &q, c))
    }

    /// Per-hinge SE(3)-invariant Rayleigh dihedral curvature-rate local potential.
    #[must_use]
    pub(crate) fn rayleigh_bending_local(
        &self,
        model: &ClothModel,
        hinge_index: usize,
        x: &[Vec3],
    ) -> Option<Local<12, 144>> {
        if self.options.rayleigh_bending_s <= 0.0 {
            return None;
        }
        let b = model.hinge_rigidity(hinge_index);
        if b <= 0.0 {
            return None;
        }
        let hinge = &model.hinges[hinge_index];
        let coeff = (self.options.rayleigh_bending_s / self.h) * b * hinge.dihedral_weight();
        let theta_cur = hinge.dihedral_angle(x);
        // Unwrap reference angle relative to current branch so `theta - target ∈ (−π, π]`.
        let d = wrap_angle_diff(theta_cur - self.hinge_theta0[hinge_index]);
        let target = theta_cur - d;
        Some(hinge.dihedral_quadratic_local(x, target, coeff))
    }

    /// Objective source-panel bending-rate response on the panel's existing six-vertex support.
    #[must_use]
    pub(crate) fn rayleigh_panel_local(
        &self,
        model: &ClothModel,
        panel_index: usize,
        x: &[Vec3],
    ) -> Option<Local<18, 324>> {
        if self.options.rayleigh_bending_s <= 0.0 {
            return None;
        }
        Some(model.panel_bending_patches()[panel_index].rate_local(
            x,
            self.panel_curvature0[panel_index],
            self.options.rayleigh_bending_s / self.h,
        ))
    }
}

fn wrap_angle_diff(mut d: f64) -> f64 {
    if !d.is_finite() {
        return d;
    }
    if d > PI {
        d -= 2.0 * PI;
    } else if d <= -PI {
        d += 2.0 * PI;
    }
    d
}

/// Smooth `C^∞` speed-cube potential `ψ(r; ε) = ⅓ ((r² + ε²)^{3/2} − ε³)` whose derivative is
/// `r √(r² + ε²)` (quadratic drag at high speed, smooth linear Stokes drag near zero speed).
fn smooth_drag_psi(r2: f64, eps: f64) -> (f64, f64) {
    let eps2 = eps * eps;
    if eps2 > 0.0 && r2 <= eps2 {
        // Rationalized form `r² ε (t² + 3t + 3) / (3 ((1 + t)√(1 + t) + 1))` for `t = r²/ε² ∈ [0, 1]`
        // preserves all 53 bits and exact `0.0` at `r = 0` without catastrophic cancellation when
        // `r ≪ ε` near Newton convergence.
        let t = r2 / eps2;
        let q = (1.0 + t).sqrt();
        let u = eps * q;
        let psi = r2 * eps * (t * t + 3.0 * t + 3.0) / (3.0 * ((1.0 + t) * q + 1.0));
        (psi, u)
    } else {
        let s = r2 + eps2;
        let u = s.sqrt();
        let psi = (u * s - eps * eps2) / 3.0;
        (psi, u)
    }
}

fn air_drag_energy(drag: AirDrag, area: f64, normal: Vec3, v: Vec3, c_v: f64) -> f64 {
    if drag.rho_air_kg_m3 == 0.0 {
        return 0.0;
    }
    let s_n = v.dot(normal);
    let v_t = v - normal * s_n;
    let (psi_n, _) = smooth_drag_psi(s_n * s_n, drag.eps_v_m_s);
    let (psi_t, _) = smooth_drag_psi(v_t.norm2(), drag.eps_v_m_s);
    (0.5 * drag.rho_air_kg_m3 * area / c_v) * (drag.c_normal * psi_n + drag.c_tangent * psi_t)
}

fn air_drag_local(drag: AirDrag, area: f64, normal: Vec3, v: Vec3, c_v: f64) -> Local<9, 81> {
    let s_n = v.dot(normal);
    let v_t = v - normal * s_n;
    let (psi_n, u_n) = smooth_drag_psi(s_n * s_n, drag.eps_v_m_s);
    let (psi_t, u_t) = smooth_drag_psi(v_t.norm2(), drag.eps_v_m_s);
    let pref = 0.5 * drag.rho_air_kg_m3 * area;
    let energy = (pref / c_v) * (drag.c_normal * psi_n + drag.c_tangent * psi_t);

    // Gradient wrt centroid velocity v: F_res = pref * (C_n * u_n * s_n * n + C_t * u_t * v_t).
    // Chain rule dv/dx_a = (c_v / 3) I_3 cancels the (1 / c_v) prefactor on grad.
    let f_vertex =
        (normal * (drag.c_normal * u_n * s_n) + v_t * (drag.c_tangent * u_t)) * (pref / 3.0);
    let mut grad = [0.0; 9];
    for a in 0..3 {
        grad[3 * a] = f_vertex.x;
        grad[3 * a + 1] = f_vertex.y;
        grad[3 * a + 2] = f_vertex.z;
    }

    // 3×3 velocity Jacobian H_v = ∂F_res / ∂v (strictly symmetric positive semidefinite):
    // H_v = pref * [ C_n * ((2 s_n² + ε²)/u_n) n nᵀ + C_t * (u_t (I − n nᵀ) + (1/u_t) v_t v_tᵀ) ].
    let eps2 = drag.eps_v_m_s * drag.eps_v_m_s;
    let kn = drag.c_normal * (2.0 * s_n * s_n + eps2) / u_n;
    let kt_iso = drag.c_tangent * u_t;
    let kt_dir = drag.c_tangent / u_t;
    let h_scale = pref * c_v / 9.0;
    let n = [normal.x, normal.y, normal.z];
    let vt = [v_t.x, v_t.y, v_t.z];
    let mut h3 = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let delta_ij = if i == j { 1.0 } else { 0.0 };
            let proj_t = delta_ij - n[i] * n[j];
            h3[i][j] = h_scale * (kn * n[i] * n[j] + kt_iso * proj_t + kt_dir * vt[i] * vt[j]);
        }
    }
    let mut hess = [0.0; 81];
    for a in 0..3 {
        for b in 0..3 {
            for i in 0..3 {
                for j in 0..3 {
                    hess[(3 * a + i) * 9 + (3 * b + j)] = h3[i][j];
                }
            }
        }
    }
    Local {
        energy,
        grad,
        hess,
        psd: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloth::ClothModel;
    use crate::elements::Membrane;
    use ftx_core::{Mat3, Vec2};

    fn check_fd<const N: usize, const NN: usize>(
        x: &[Vec3],
        verts: &[usize],
        eps: f64,
        energy: impl Fn(&[Vec3]) -> f64,
        local: impl Fn(&[Vec3]) -> Local<N, NN>,
    ) {
        let l = local(x);
        assert!((l.energy - energy(x)).abs() <= 1e-12 * l.energy.abs().max(1.0));
        for a in 0..verts.len() {
            for k in 0..3 {
                let mut xp = x.to_vec();
                let mut xm = x.to_vec();
                match k {
                    0 => {
                        xp[verts[a]].x += eps;
                        xm[verts[a]].x -= eps;
                    }
                    1 => {
                        xp[verts[a]].y += eps;
                        xm[verts[a]].y -= eps;
                    }
                    _ => {
                        xp[verts[a]].z += eps;
                        xm[verts[a]].z -= eps;
                    }
                }
                let fd_g = (energy(&xp) - energy(&xm)) / (2.0 * eps);
                let idx = 3 * a + k;
                assert!(
                    (l.grad[idx] - fd_g).abs() <= 1e-6 * fd_g.abs().max(1e-6),
                    "grad[{idx}]: got {} want {fd_g}",
                    l.grad[idx]
                );
                let gp = local(&xp).grad;
                let gm = local(&xm).grad;
                for j in 0..N {
                    let fd_h = (gp[j] - gm[j]) / (2.0 * eps);
                    assert!(
                        (l.hess[idx * N + j] - fd_h).abs() <= 1e-5 * fd_h.abs().max(1e-5),
                        "hess[{idx},{j}]: got {} want {fd_h}",
                        l.hess[idx * N + j]
                    );
                }
            }
        }
    }

    #[test]
    fn air_drag_and_rayleigh_derivatives_match_finite_differences() {
        let rest = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(0.02, 0.0),
            Vec2::new(0.01, 0.02),
            Vec2::new(0.01, -0.02),
        ];
        let tris = [[0, 1, 2], [1, 0, 3]];
        let mat = Membrane::orthotropic(500.0, 300.0, 0.2, 60.0, 1.0);
        let model = ClothModel::new(rest.clone(), &tris, mat, 2e-5, 0.15).unwrap();
        let x0: Vec<Vec3> = rest
            .iter()
            .enumerate()
            .map(|(i, p)| Vec3::new(p.x, p.y, if i == 3 { 0.005 } else { 0.0 }))
            .collect();
        let x1 = vec![
            x0[0] + Vec3::new(0.001, -0.002, -0.015),
            x0[1] + Vec3::new(0.002, 0.001, -0.012),
            x0[2] + Vec3::new(-0.001, 0.003, -0.018),
            x0[3] + Vec3::new(0.003, -0.001, -0.011),
        ];
        let h = 1.0 / 24.0;
        let opts = DampingOptions {
            air_drag: Some(AirDrag::standard()),
            rayleigh_membrane_s: 2e-3,
            rayleigh_bending_s: 5e-3,
        };

        // 1. Implicit Euler velocity map
        let step_ie = DampingStep::build(
            &model,
            opts,
            &x0,
            h,
            VelocityMap::ImplicitEuler {
                x0: &x0,
                inv_h: 1.0 / h,
            },
        )
        .unwrap()
        .unwrap();
        check_fd(
            &x1,
            &model.membranes[0].v,
            1e-7,
            |p| {
                let (a, n) = step_ie.tri_frames[0];
                air_drag_energy(
                    opts.air_drag.unwrap(),
                    a,
                    n,
                    step_ie.vel.centroid_velocity(p, model.membranes[0].v),
                    step_ie.vel.c_v(),
                )
            },
            |p| step_ie.air_drag_local(0, model.membranes[0].v, p).unwrap(),
        );
        check_fd(
            &x1,
            &model.membranes[0].v,
            1e-7,
            |p| {
                model.membranes[0].strain_rate_energy(
                    p,
                    step_ie.tri_e0[0],
                    &model.material(0).q,
                    opts.rayleigh_membrane_s / h,
                )
            },
            |p| step_ie.rayleigh_membrane_local(&model, 0, p).unwrap(),
        );
        check_fd(
            &x1,
            &model.hinges[0].v,
            1e-7,
            |p| {
                let th = model.hinges[0].dihedral_angle(p);
                let d = wrap_angle_diff(th - step_ie.hinge_theta0[0]);
                0.5 * (opts.rayleigh_bending_s / h)
                    * model.hinge_rigidity(0)
                    * model.hinges[0].dihedral_weight()
                    * d
                    * d
            },
            |p| step_ie.rayleigh_bending_local(&model, 0, p).unwrap(),
        );

        // 2. Generalized-α velocity map with non-zero base velocity
        let v_base = vec![
            Vec3::new(0.2, -0.1, -0.5),
            Vec3::new(-0.1, 0.3, -0.4),
            Vec3::new(0.05, 0.1, -0.6),
            Vec3::new(-0.2, -0.05, -0.3),
        ];
        let step_ga = DampingStep::build(
            &model,
            opts,
            &x0,
            h,
            VelocityMap::GeneralizedAlpha {
                x0: &x0,
                v_base,
                c_v: 1.25 / h,
            },
        )
        .unwrap()
        .unwrap();
        let drag_local = step_ga
            .air_drag_local(0, model.membranes[0].v, &x1)
            .unwrap();
        let (vals, _) = crate::eigen::jacobi_eigh(&drag_local.hess, 9);
        assert!(drag_local.psd && vals[0] >= -1e-12 * vals[8], "{vals:?}");
        check_fd(
            &x1,
            &model.membranes[0].v,
            1e-7,
            |p| {
                let (a, n) = step_ga.tri_frames[0];
                air_drag_energy(
                    opts.air_drag.unwrap(),
                    a,
                    n,
                    step_ga.vel.centroid_velocity(p, model.membranes[0].v),
                    step_ga.vel.c_v(),
                )
            },
            |p| step_ga.air_drag_local(0, model.membranes[0].v, p).unwrap(),
        );
    }

    #[test]
    fn rayleigh_damping_is_strictly_se3_invariant() {
        let rest = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(0.02, 0.0),
            Vec2::new(0.01, 0.02),
            Vec2::new(0.01, -0.02),
        ];
        let model = ClothModel::new(
            rest.clone(),
            &[[0, 1, 2], [1, 0, 3]],
            Membrane::orthotropic(500.0, 300.0, 0.2, 60.0, 1.0),
            2e-5,
            0.15,
        )
        .unwrap();
        // Start with a genuinely bent, stretched 3D shape so both strains and dihedral angle are nonzero.
        let x0 = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.021, 0.0, 0.0),
            Vec3::new(0.01, 0.019, 0.004),
            Vec3::new(0.01, -0.016, 0.011),
        ];
        let rot = Mat3::rotation(Vec3::new(0.4, -0.7, 0.5).normalized(), 1.37);
        let shift = Vec3::new(2.5, -1.7, 0.8);
        let x_rigid: Vec<Vec3> = x0.iter().map(|&p| rot.mul_vec(p) + shift).collect();

        let opts = DampingOptions {
            air_drag: None,
            rayleigh_membrane_s: 0.01,
            rayleigh_bending_s: 0.02,
        };
        let step = DampingStep::build(
            &model,
            opts,
            &x0,
            0.01,
            VelocityMap::ImplicitEuler {
                x0: &x0,
                inv_h: 100.0,
            },
        )
        .unwrap()
        .unwrap();
        let e = step.energy(&model, &x_rigid);
        assert!(e.abs() < 1e-26, "SE(3) energy must vanish: {e}");
        let lm = step.rayleigh_membrane_local(&model, 0, &x_rigid).unwrap();
        assert!(lm.grad.iter().all(|g| g.abs() < 1e-13), "{:?}", lm.grad);
        let lb = step.rayleigh_bending_local(&model, 0, &x_rigid).unwrap();
        assert!(lb.grad.iter().all(|g| g.abs() < 1e-14), "{:?}", lb.grad);
    }

    #[test]
    fn damping_validation_refuses_invalid_inputs() {
        assert!(DampingOptions::default().validate().is_ok());
        assert!(
            DampingOptions {
                air_drag: Some(AirDrag {
                    rho_air_kg_m3: -0.1,
                    ..AirDrag::standard()
                }),
                ..DampingOptions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            DampingOptions {
                air_drag: Some(AirDrag {
                    eps_v_m_s: 0.0,
                    ..AirDrag::standard()
                }),
                ..DampingOptions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            DampingOptions {
                air_drag: Some(AirDrag {
                    eps_v_m_s: 1e150,
                    ..AirDrag::standard()
                }),
                ..DampingOptions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            DampingOptions {
                air_drag: Some(AirDrag {
                    eps_v_m_s: 1e-200,
                    ..AirDrag::standard()
                }),
                ..DampingOptions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            DampingOptions {
                rayleigh_membrane_s: -1e-4,
                ..DampingOptions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            DampingOptions {
                rayleigh_bending_s: f64::NAN,
                ..DampingOptions::default()
            }
            .validate()
            .is_err()
        );

        // Tiny-velocity air drag potential must stay exact (zero cancellation at r << eps_v)
        // and vanish identically at rest.
        let (psi0, u0) = smooth_drag_psi(0.0, 1e-3);
        assert_eq!(psi0, 0.0);
        assert_eq!(u0, 1e-3);
        let r = 1e-11_f64;
        let (psi_tiny, u_tiny) = smooth_drag_psi(r * r, 1e-3);
        let want_tiny = 0.5 * 1e-3 * r * r;
        assert!(
            (psi_tiny - want_tiny).abs() <= 1e-14 * want_tiny,
            "psi_tiny={psi_tiny} want={want_tiny}"
        );
        assert!((u_tiny - 1e-3).abs() < 1e-15);

        // Subnormal time step overflowing strain-rate or c_v scaling must refuse fail-closed.
        let rest = vec![Vec2::ZERO, Vec2::new(0.02, 0.0), Vec2::new(0.0, 0.02)];
        let model = ClothModel::new(
            rest.clone(),
            &[[0, 1, 2]],
            Membrane::orthotropic(500.0, 300.0, 0.2, 60.0, 1.0),
            2e-5,
            0.15,
        )
        .unwrap();
        let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let err = DampingStep::build(
            &model,
            DampingOptions {
                air_drag: Some(AirDrag::standard()),
                rayleigh_membrane_s: 1e-2,
                rayleigh_bending_s: 0.0,
            },
            &x0,
            1e-320,
            VelocityMap::ImplicitEuler {
                x0: &x0,
                inv_h: 1.0 / 1e-320,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, RefusalCode::OutOfRange);
    }

    #[test]
    fn source_panel_rates_survive_replaced_scalar_hinges() {
        use ftx_pattern::material_bending::BendingTensor;

        let (rest, triangles) = crate::cloth::grid(0.12, 0.08, 5, 4, Vec2::ZERO);
        let x0: Vec<_> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let trial: Vec<_> = rest
            .iter()
            .map(|p| {
                Vec3::new(
                    p.x,
                    p.y,
                    0.7 * p.x * p.x + 0.4 * p.y * p.y - 0.2 * p.x * p.y,
                )
            })
            .collect();
        let options = DampingOptions {
            rayleigh_bending_s: 0.02,
            ..DampingOptions::default()
        };
        for normal in [false, true] {
            let mut model = ClothModel::new(
                rest.clone(),
                &triangles,
                Membrane::orthotropic(500.0, 300.0, 0.2, 60.0, 1.0),
                1e-5,
                0.15,
            )
            .unwrap();
            let mut law = BendingTensor::new(0.002, 0.005, 0.0004, 0.003).unwrap();
            if normal {
                law = law.normal_projected();
            }
            model
                .set_panel_bending(
                    &rest,
                    &triangles,
                    &(0..rest.len()).collect::<Vec<_>>(),
                    &vec![Some(law); triangles.len()],
                )
                .unwrap();
            assert!((0..model.hinges.len()).all(|i| model.hinge_rigidity(i) == 0.0));
            let h = 0.01;
            let step = DampingStep::build(
                &model,
                options,
                &x0,
                h,
                VelocityMap::ImplicitEuler {
                    x0: &x0,
                    inv_h: 1.0 / h,
                },
            )
            .unwrap()
            .unwrap();
            let expected: f64 = model
                .panel_bending_patches()
                .iter()
                .map(|patch| patch.energy(&trial) * options.rayleigh_bending_s / h)
                .sum();
            assert!(expected > 1e-6);
            let actual = step.energy(&model, &trial);
            assert!(
                (actual - expected).abs() < 1e-12 * expected,
                "normal={normal}: actual {actual}, expected {expected}"
            );
            assert_eq!(step.panel_curvature0.len(), triangles.len());
            assert!(
                step.rayleigh_panel_local(&model, 0, &trial)
                    .unwrap()
                    .grad
                    .iter()
                    .any(|g| g.abs() > 1e-8)
            );
            assert!(
                model
                    .panel_bending_patches()
                    .iter()
                    .all(|patch| patch.tensor() == law)
            );
            assert_eq!(step.energy(&model, &x0), 0.0);
        }
    }
}
