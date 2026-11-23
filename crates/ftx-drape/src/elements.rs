//! Cloth element energies with exact gradients and Hessians (plan §11.2).
//!
//! - **Membrane**: constant-strain triangle, Green strain from the deformation gradient against the
//!   piece's flat rest coordinates (rotation-invariant, so large rotations store no energy), orthotropic
//!   plane-stress resultants `Q` (N/m) in the material frame (axis 1 = warp/grain, axis 2 = weft,
//!   engineering shear = bias), StVK energy `A·½ εᵀQε`. Each normal strain is softened under compression,
//!   `φ(e) = e` for e ≥ 0 and `r·e + (1 − r)·δ·tanh(e/δ)` below (r = √s), so cloth buckles and gathers
//!   instead of resisting compression like a plate. The blend makes the energy C² at e = 0: with a plain
//!   `r·e` the Hessian jumps by 1/s at zero strain and Newton cycles on a sagging sheet. Well into
//!   compression the stiffness tends to s·Q. Rest coordinates are expected in the material frame (rotate
//!   the pattern so the grainline is +u).
//! - **Bending**: Bergou et al.'s quadratic model for inextensible surfaces: per interior edge
//!   `½·B·w·|Σᵢ Kᵢ xᵢ|²` with cotangent weights K from the flat rest shape. Its Hessian is constant and
//!   positive semidefinite, which suits Newton.

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};
use ftx_pattern::material_response::{CompiledMembrane, Response};
use serde::{Deserialize, Serialize};

use crate::ad::{JVec, Jet};

/// Hinge bending formulation for [`crate::cloth::ClothModel`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BendingModel {
    /// Legacy Bergou quadratic isometric hinge `½·B·w·|Σᵢ Kᵢ xᵢ|²` (bit-identical default).
    #[default]
    Quadratic,
    /// Finite-angle Discrete Shells dihedral hinge `½·B·w_d·(θ − θ₀)²` with smooth `atan2`
    /// curvature, zero in-plane stretch pollution, non-vanishing hairpin fold torque up to `π`,
    /// and per-hinge rest-angle crease/pleat support.
    DiscreteShells,
}

impl BendingModel {
    /// Whether this is the legacy default [`BendingModel::Quadratic`].
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Quadratic)
    }
}

/// How membrane compression softening acts on the 2D Green strain tensor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionModel {
    /// Legacy axis-aligned softening on warp `E₁₁` and weft `E₂₂` only (shear `γ₁₂` unsoftened;
    /// bit-identical default).
    #[default]
    AxisAligned,
    /// Principal-strain tension-field softening: applies the `C²` `tanh` compression blend to
    /// the eigenvalues of the 2×2 Green strain tensor `E` via the singularity-free spectral
    /// formula `Φ(E) = a I + b (E − m I)`, eliminating 45° bias and pure-shear membrane locking.
    Principal,
}

impl CompressionModel {
    /// Whether this is the legacy default [`CompressionModel::AxisAligned`].
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::AxisAligned)
    }
}

/// Orthotropic membrane material (resultants in N/m, so thickness is folded in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Membrane {
    /// Plane-stress stiffness in the material frame, Voigt order (ε₁₁, ε₂₂, γ₁₂).
    pub q: [[f64; 3]; 3],
    /// Compression stiffness factor per normal direction (1 = as stiff as in tension).
    pub compression: [f64; 2],
    /// Strain width δ of the tension–compression blend (default 1e-3).
    pub transition: f64,
}

impl Membrane {
    /// Check the constitutive assumptions used by the element's PSD shortcut before assembly.
    ///
    /// # Errors
    /// Non-finite, asymmetric or non-positive-definite stiffness, invalid compression or transition.
    pub fn validate(&self) -> Result<(), Refusal> {
        let bad = || {
            Refusal::new(
                RefusalCode::OutOfRange,
                "membrane needs finite symmetric positive-definite stiffness, compression in (0,1] and positive transition",
            )
        };
        if self.q.iter().flatten().any(|v| !v.is_finite())
            || self
                .compression
                .iter()
                .any(|c| !c.is_finite() || *c <= 0.0 || *c > 1.0)
            || !self.transition.is_finite()
            || self.transition <= 0.0
        {
            return Err(bad());
        }
        // Scale before Cholesky so validation itself does not overflow for a finite matrix.
        let scale = self.q.iter().flatten().map(|x| x.abs()).fold(0.0, f64::max);
        if scale == 0.0 {
            return Err(bad());
        }
        let mut lower = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..=i {
                if self.q[i][j] != self.q[j][i] {
                    return Err(bad());
                }
                let r =
                    self.q[i][j] / scale - (0..j).map(|k| lower[i][k] * lower[j][k]).sum::<f64>();
                if i == j {
                    if !r.is_finite() || r <= 0.0 {
                        return Err(bad());
                    }
                    lower[i][j] = r.sqrt();
                } else {
                    lower[i][j] = r / lower[j][j];
                }
            }
        }
        Ok(())
    }

    /// From warp and weft Young's moduli E1, E2 (N/m), the major Poisson ratio ν12, the shear (bias)
    /// modulus G12 (N/m) and a compression factor applied to both directions.
    #[must_use]
    pub fn orthotropic(e1: f64, e2: f64, nu12: f64, g12: f64, compression: f64) -> Self {
        let nu21 = nu12 * e2 / e1;
        let d = 1.0 - nu12 * nu21;
        Self {
            q: [
                [e1 / d, nu12 * e2 / d, 0.0],
                [nu12 * e2 / d, e2 / d, 0.0],
                [0.0, 0.0, g12],
            ],
            compression: [compression, compression],
            transition: 1e-3,
        }
    }
}

/// A constant-strain membrane triangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MembraneElement {
    /// Vertex indices.
    pub v: [usize; 3],
    /// Rest area (m²).
    pub area: f64,
    /// Shape-function gradients: `g[a] = (∂Nₐ/∂u, ∂Nₐ/∂v)` in the material frame.
    pub g: [[f64; 2]; 3],
}

/// Energy, gradient (9) and Hessian (9 × 9, row-major) of one element.
#[derive(Clone, Copy, Debug)]
pub struct Local<const N: usize, const NN: usize> {
    /// Energy (J).
    pub energy: f64,
    /// Gradient.
    pub grad: [f64; N],
    /// Hessian, row-major.
    pub hess: [f64; NN],
    /// The Hessian is positive semidefinite by construction (exactly, before rounding), so projecting
    /// it would change nothing but rounding: projected Newton skips it. For a membrane this assumes a
    /// physical material (`Membrane::q` positive semidefinite, as `Membrane::orthotropic` gives for
    /// positive moduli and ν₁₂ν₂₁ < 1).
    pub psd: bool,
}

impl MembraneElement {
    /// Builds the element from its rest coordinates; `None` for a degenerate triangle.
    #[must_use]
    pub fn new(v: [usize; 3], rest: [Vec2; 3]) -> Option<Self> {
        let (d1, d2) = (rest[1] - rest[0], rest[2] - rest[0]);
        let det = d1.x * d2.y - d2.x * d1.y;
        if !det.is_finite() || det == 0.0 {
            return None;
        }
        // Dm = [d1 d2] (columns); Dm⁻¹ rows give ∂N₁ and ∂N₂.
        let inv = [[d2.y / det, -d2.x / det], [-d1.y / det, d1.x / det]];
        let g1 = inv[0];
        let g2 = inv[1];
        let g0 = [-g1[0] - g2[0], -g1[1] - g2[1]];
        Some(Self {
            v,
            area: 0.5 * det.abs(),
            g: [g0, g1, g2],
        })
    }

    /// Rest lengths of the edges (v0v1, v1v2, v2v0), recovered from the shape gradients (exact also
    /// across welds, where per-vertex rest coordinates are not).
    #[must_use]
    pub fn rest_edge_lengths(&self) -> [f64; 3] {
        let (g1, g2) = (self.g[1], self.g[2]);
        let det = g1[0] * g2[1] - g1[1] * g2[0];
        let d1 = Vec2::new(g2[1] / det, -g2[0] / det);
        let d2 = Vec2::new(-g1[1] / det, g1[0] / det);
        [d1.norm(), (d2 - d1).norm(), d2.norm()]
    }

    fn frame(&self, x: &[Vec3]) -> (Vec3, Vec3) {
        let mut f1 = Vec3::ZERO;
        let mut f2 = Vec3::ZERO;
        for (a, &vi) in self.v.iter().enumerate() {
            f1 += x[vi] * self.g[a][0];
            f2 += x[vi] * self.g[a][1];
        }
        (f1, f2)
    }

    /// Green strain `(E₁₁, E₂₂, γ₁₂)` in the material frame.
    #[must_use]
    pub fn strain(&self, x: &[Vec3]) -> [f64; 3] {
        let (f1, f2) = self.frame(x);
        [
            0.5 * (f1.dot(f1) - 1.0),
            0.5 * (f2.dot(f2) - 1.0),
            f1.dot(f2),
        ]
    }

    /// Energy only.
    #[must_use]
    pub fn energy(&self, x: &[Vec3], m: &Membrane) -> f64 {
        let e = self.strain(x);
        let (eff, _, _) = soften(e, m);
        let s = mat3(&m.q, eff);
        0.5 * self.area * (eff[0] * s[0] + eff[1] * s[1] + eff[2] * s[2])
    }

    /// Energy under the chosen [`CompressionModel`]. [`CompressionModel::AxisAligned`] is
    /// bit-identical to [`Self::energy`].
    #[must_use]
    pub fn energy_with_model(&self, x: &[Vec3], m: &Membrane, model: CompressionModel) -> f64 {
        match model {
            CompressionModel::AxisAligned => self.energy(x, m),
            CompressionModel::Principal => {
                let r = principal_ratio(m);
                if r >= 1.0 {
                    return self.energy(x, m);
                }
                let e = self.strain(x);
                let eff = soften_principal_value(e, r, m.transition);
                let s = mat3(&m.q, eff);
                0.5 * self.area * (eff[0] * s[0] + eff[1] * s[1] + eff[2] * s[2])
            }
        }
    }

    /// Energy of a characterized nonlinear law, with the existing gather/compression transform.
    /// The caller must first check the raw physical strain against the law's characterized domain.
    #[must_use]
    pub fn response_energy(&self, x: &[Vec3], law: &CompiledMembrane, m: &Membrane) -> f64 {
        let e = self.strain(x);
        let (eff, _, _) = soften(e, m);
        if law.area_ratio_range().is_some() {
            if law.check_domain(e).is_err() {
                return f64::NAN;
            }
            return law
                .evaluate_separable(eff)
                .zip(law.evaluate_area(e))
                .map_or(f64::NAN, |(base, area)| {
                    self.area * (base.energy + area.energy)
                });
        }
        law.evaluate(eff).map_or(f64::NAN, |r| self.area * r.energy)
    }

    /// Exact chain-rule gradient/Hessian for a characterized nonlinear membrane response.
    /// The material tangent supplied by the response replaces Q; geometric and gather-softening
    /// terms remain identical to the linear element. No extrapolation is performed.
    #[must_use]
    pub fn response_local(&self, x: &[Vec3], law: &CompiledMembrane, m: &Membrane) -> Local<9, 81> {
        if law.area_ratio_range().is_some() {
            if law.check_domain(self.strain(x)).is_err() {
                return self.local_with_response(x, m, |_| None);
            }
            let mut base = self.local_with_response(x, m, |e| law.evaluate_separable(e));
            let raw = Membrane {
                compression: [1.0; 2],
                ..*m
            };
            let area = self.local_with_response(x, &raw, |e| law.evaluate_area(e));
            base.energy += area.energy;
            for (base, area) in base.grad.iter_mut().zip(area.grad) {
                *base += area;
            }
            for (base, area) in base.hess.iter_mut().zip(area.hess) {
                *base += area;
            }
            // Nonnegative area energy does not imply a PSD Green-strain tangent. The existing
            // projected Newton path must see the full exact Hessian, including area geometry.
            base.psd = false;
            return base;
        }
        self.local_with_response(x, m, |strain| law.evaluate(strain))
    }

    // Shared exact geometric/compression chain rule for equilibrium and Maxwell responses. The
    // PSD shortcut assumes a PSD closure tangent. The physical-area caller disables that shortcut
    // explicitly; raw physical domains are checked before a Newton evaluation.
    pub(crate) fn local_with_response(
        &self,
        x: &[Vec3],
        m: &Membrane,
        evaluate: impl FnOnce([f64; 3]) -> Option<Response>,
    ) -> Local<9, 81> {
        let (f1, f2) = self.frame(x);
        let e = [
            0.5 * (f1.dot(f1) - 1.0),
            0.5 * (f2.dot(f2) - 1.0),
            f1.dot(f2),
        ];
        let (eff, dphi, ddphi) = soften(e, m);
        let Some(response) = evaluate(eff) else {
            return Local {
                energy: f64::NAN,
                grad: [f64::NAN; 9],
                hess: [f64::NAN; 81],
                psd: false,
            };
        };
        let sigma = response.stress;
        let q = response.tangent;
        let mut b = [[0.0; 9]; 3];
        for a in 0..3 {
            let (ga0, ga1) = (self.g[a][0], self.g[a][1]);
            for k in 0..3 {
                b[0][3 * a + k] = dphi[0] * ga0 * f1[k];
                b[1][3 * a + k] = dphi[1] * ga1 * f2[k];
                b[2][3 * a + k] = ga0 * f2[k] + ga1 * f1[k];
            }
        }
        let mut grad = [0.0; 9];
        for (i, gi) in grad.iter_mut().enumerate() {
            *gi = self.area * (sigma[0] * b[0][i] + sigma[1] * b[1][i] + sigma[2] * b[2][i]);
        }
        let mut hess = [0.0; 81];
        let mut qb = [[0.0; 9]; 3];
        for r in 0..3 {
            for j in 0..9 {
                qb[r][j] = q[r][0] * b[0][j] + q[r][1] * b[1][j] + q[r][2] * b[2][j];
            }
        }
        for i in 0..9 {
            for j in 0..9 {
                hess[i * 9 + j] =
                    self.area * (b[0][i] * qb[0][j] + b[1][i] * qb[1][j] + b[2][i] * qb[2][j]);
            }
        }
        for a in 0..3 {
            for c in 0..3 {
                let (ga, gc) = (self.g[a], self.g[c]);
                let s = sigma[0] * dphi[0] * ga[0] * gc[0]
                    + sigma[1] * dphi[1] * ga[1] * gc[1]
                    + sigma[2] * (ga[0] * gc[1] + ga[1] * gc[0]);
                for k in 0..3 {
                    hess[(3 * a + k) * 9 + 3 * c + k] += self.area * s;
                }
            }
        }
        for (i, (&dd, f)) in ddphi.iter().zip([f1, f2]).enumerate() {
            if dd == 0.0 {
                continue;
            }
            let de: [f64; 9] = std::array::from_fn(|j| self.g[j / 3][i] * f[j % 3]);
            let c = self.area * sigma[i] * dd;
            for r in 0..9 {
                for s in 0..9 {
                    hess[r * 9 + s] += c * de[r] * de[s];
                }
            }
        }
        let (m11, m22) = (sigma[0] * dphi[0], sigma[1] * dphi[1]);
        let psd = m11 >= 0.0
            && m22 >= 0.0
            && m11 * m22 >= sigma[2] * sigma[2]
            && sigma[0] * ddphi[0] >= 0.0
            && sigma[1] * ddphi[1] >= 0.0;
        Local {
            energy: self.area * response.energy,
            grad,
            hess,
            psd,
        }
    }

    /// Energy, gradient and exact Hessian (material plus geometric part).
    #[must_use]
    pub fn local(&self, x: &[Vec3], m: &Membrane) -> Local<9, 81> {
        let (f1, f2) = self.frame(x);
        let e = [
            0.5 * (f1.dot(f1) - 1.0),
            0.5 * (f2.dot(f2) - 1.0),
            f1.dot(f2),
        ];
        let (eff, dphi, ddphi) = soften(e, m);
        let sigma = mat3(&m.q, eff);
        // B (3 × 9): rows d(eff_i)/dx.
        let mut b = [[0.0; 9]; 3];
        for a in 0..3 {
            let (ga0, ga1) = (self.g[a][0], self.g[a][1]);
            for k in 0..3 {
                b[0][3 * a + k] = dphi[0] * ga0 * f1[k];
                b[1][3 * a + k] = dphi[1] * ga1 * f2[k];
                b[2][3 * a + k] = ga0 * f2[k] + ga1 * f1[k];
            }
        }
        let mut grad = [0.0; 9];
        for (i, gi) in grad.iter_mut().enumerate() {
            *gi = self.area * (sigma[0] * b[0][i] + sigma[1] * b[1][i] + sigma[2] * b[2][i]);
        }
        let mut hess = [0.0; 81];
        // Material part A·Bᵀ Q B.
        let mut qb = [[0.0; 9]; 3];
        for r in 0..3 {
            for j in 0..9 {
                qb[r][j] = m.q[r][0] * b[0][j] + m.q[r][1] * b[1][j] + m.q[r][2] * b[2][j];
            }
        }
        for i in 0..9 {
            for j in 0..9 {
                hess[i * 9 + j] =
                    self.area * (b[0][i] * qb[0][j] + b[1][i] * qb[1][j] + b[2][i] * qb[2][j]);
            }
        }
        // Geometric part: Σ σᵢ ∂²effᵢ, a scalar per vertex pair times I₃.
        for a in 0..3 {
            for c in 0..3 {
                let (ga, gc) = (self.g[a], self.g[c]);
                let s = sigma[0] * dphi[0] * ga[0] * gc[0]
                    + sigma[1] * dphi[1] * ga[1] * gc[1]
                    + sigma[2] * (ga[0] * gc[1] + ga[1] * gc[0]);
                for k in 0..3 {
                    hess[(3 * a + k) * 9 + 3 * c + k] += self.area * s;
                }
            }
        }
        // Curvature of the softening blend: σᵢ·φ''(Eᵢᵢ)·(∂Eᵢᵢ/∂x)(∂Eᵢᵢ/∂x)ᵀ.
        for (i, (&dd, f)) in ddphi.iter().zip([f1, f2]).enumerate() {
            if dd == 0.0 {
                continue;
            }
            let de: [f64; 9] = std::array::from_fn(|j| self.g[j / 3][i] * f[j % 3]);
            let c = self.area * sigma[i] * dd;
            for r in 0..9 {
                for s in 0..9 {
                    hess[r * 9 + s] += c * de[r] * de[s];
                }
            }
        }
        // PSD by construction when the geometric part's 2 × 2 stress matrix M = [[σ₁φ₁', σ₁₂], [σ₁₂,
        // σ₂φ₂']] is (it enters as G·M·Gᵀ ⊗ I₃) and no softening curvature term σᵢφᵢ'' is negative; the
        // material part A·BᵀQB is PSD for PSD Q.
        let (m11, m22) = (sigma[0] * dphi[0], sigma[1] * dphi[1]);
        let psd = m11 >= 0.0
            && m22 >= 0.0
            && m11 * m22 >= sigma[2] * sigma[2]
            && sigma[0] * ddphi[0] >= 0.0
            && sigma[1] * ddphi[1] >= 0.0;
        Local {
            energy: 0.5 * self.area * (eff[0] * sigma[0] + eff[1] * sigma[1] + eff[2] * sigma[2]),
            grad,
            hess,
            psd,
        }
    }

    /// Energy, gradient and exact Hessian under the chosen [`CompressionModel`].
    /// [`CompressionModel::AxisAligned`] is bit-identical to [`Self::local`].
    #[must_use]
    pub fn local_with_model(
        &self,
        x: &[Vec3],
        m: &Membrane,
        model: CompressionModel,
    ) -> Local<9, 81> {
        match model {
            CompressionModel::AxisAligned => self.local(x, m),
            CompressionModel::Principal => {
                let r = principal_ratio(m);
                if r >= 1.0 {
                    return self.local(x, m);
                }
                let (f1, f2) = self.frame(x);
                let e = [
                    0.5 * (f1.dot(f1) - 1.0),
                    0.5 * (f2.dot(f2) - 1.0),
                    f1.dot(f2),
                ];
                let phi = soften_principal_jets(e, r, m.transition);
                let eff = [phi[0].v, phi[1].v, phi[2].v];
                let sigma = mat3(&m.q, eff);
                // Raw Green-strain Jacobian B_raw (3 × 9): rows ∂e_p / ∂x.
                let mut b_raw = [[0.0; 9]; 3];
                for a in 0..3 {
                    let (ga0, ga1) = (self.g[a][0], self.g[a][1]);
                    for k in 0..3 {
                        b_raw[0][3 * a + k] = ga0 * f1[k];
                        b_raw[1][3 * a + k] = ga1 * f2[k];
                        b_raw[2][3 * a + k] = ga0 * f2[k] + ga1 * f1[k];
                    }
                }
                // Effective-strain Jacobian B = (∂Φ/∂e) B_raw.
                let mut b = [[0.0; 9]; 3];
                for row in 0..3 {
                    let j = phi[row].g;
                    for col in 0..9 {
                        b[row][col] =
                            j[0] * b_raw[0][col] + j[1] * b_raw[1][col] + j[2] * b_raw[2][col];
                    }
                }
                let mut grad = [0.0; 9];
                for (i, gi) in grad.iter_mut().enumerate() {
                    *gi =
                        self.area * (sigma[0] * b[0][i] + sigma[1] * b[1][i] + sigma[2] * b[2][i]);
                }
                // Material part: A · Bᵀ Q B.
                let mut qb = [[0.0; 9]; 3];
                for row in 0..3 {
                    for col in 0..9 {
                        qb[row][col] = m.q[row][0] * b[0][col]
                            + m.q[row][1] * b[1][col]
                            + m.q[row][2] * b[2][col];
                    }
                }
                let mut hess = [0.0; 81];
                for i in 0..9 {
                    for j in 0..9 {
                        hess[i * 9 + j] = self.area
                            * (b[0][i] * qb[0][j] + b[1][i] * qb[1][j] + b[2][i] * qb[2][j]);
                    }
                }
                // Geometric part: A · Σ_p (Σ_r σ_r ∂Φ_r/∂e_p) ∂²e_p/∂x².
                let mut s_raw = [0.0; 3];
                for p in 0..3 {
                    s_raw[p] =
                        sigma[0] * phi[0].g[p] + sigma[1] * phi[1].g[p] + sigma[2] * phi[2].g[p];
                }
                for a in 0..3 {
                    for c in 0..3 {
                        let (ga, gc) = (self.g[a], self.g[c]);
                        let s = s_raw[0] * ga[0] * gc[0]
                            + s_raw[1] * ga[1] * gc[1]
                            + s_raw[2] * (ga[0] * gc[1] + ga[1] * gc[0]);
                        for k in 0..3 {
                            hess[(3 * a + k) * 9 + 3 * c + k] += self.area * s;
                        }
                    }
                }
                // Softening-curvature part: A · B_rawᵀ (Σ_r σ_r ∂²Φ_r/∂e²) B_raw.
                let mut c_mat = [[0.0; 3]; 3];
                for p in 0..3 {
                    for q in 0..3 {
                        c_mat[p][q] = sigma[0] * phi[0].h[p][q]
                            + sigma[1] * phi[1].h[p][q]
                            + sigma[2] * phi[2].h[p][q];
                    }
                }
                let mut cb = [[0.0; 9]; 3];
                for p in 0..3 {
                    for j in 0..9 {
                        cb[p][j] = c_mat[p][0] * b_raw[0][j]
                            + c_mat[p][1] * b_raw[1][j]
                            + c_mat[p][2] * b_raw[2][j];
                    }
                }
                for i in 0..9 {
                    for j in 0..9 {
                        hess[i * 9 + j] += self.area
                            * (b_raw[0][i] * cb[0][j]
                                + b_raw[1][i] * cb[1][j]
                                + b_raw[2][i] * cb[2][j]);
                    }
                }
                Local {
                    energy: 0.5
                        * self.area
                        * (eff[0] * sigma[0] + eff[1] * sigma[1] + eff[2] * sigma[2]),
                    grad,
                    hess,
                    psd: false,
                }
            }
        }
    }

    /// Reference Green strain-rate Kelvin–Voigt viscous contribution
    /// `½ · c · A · (E(x) − e0)ᵀ Q (E(x) − e0)`, strictly SE(3)-invariant.
    pub(crate) fn strain_rate_energy(
        &self,
        x: &[Vec3],
        e0: [f64; 3],
        q: &[[f64; 3]; 3],
        c: f64,
    ) -> f64 {
        let e = self.strain(x);
        let de = [e[0] - e0[0], e[1] - e0[1], e[2] - e0[2]];
        let s = mat3(q, de);
        0.5 * c * self.area * (de[0] * s[0] + de[1] * s[1] + de[2] * s[2])
    }

    /// Energy, gradient and exact Hessian of the SE(3)-invariant membrane Kelvin–Voigt
    /// strain-rate potential `½ · c · A · (E(x) − e0)ᵀ Q (E(x) − e0)`.
    pub(crate) fn strain_rate_local(
        &self,
        x: &[Vec3],
        e0: [f64; 3],
        q: &[[f64; 3]; 3],
        c: f64,
    ) -> Local<9, 81> {
        let (f1, f2) = self.frame(x);
        let e = [
            0.5 * (f1.dot(f1) - 1.0),
            0.5 * (f2.dot(f2) - 1.0),
            f1.dot(f2),
        ];
        let de = [e[0] - e0[0], e[1] - e0[1], e[2] - e0[2]];
        let sigma = mat3(q, de);
        let scale = c * self.area;
        let mut b = [[0.0; 9]; 3];
        for a in 0..3 {
            let (ga0, ga1) = (self.g[a][0], self.g[a][1]);
            for k in 0..3 {
                b[0][3 * a + k] = ga0 * f1[k];
                b[1][3 * a + k] = ga1 * f2[k];
                b[2][3 * a + k] = ga0 * f2[k] + ga1 * f1[k];
            }
        }
        let mut grad = [0.0; 9];
        for (i, gi) in grad.iter_mut().enumerate() {
            *gi = scale * (sigma[0] * b[0][i] + sigma[1] * b[1][i] + sigma[2] * b[2][i]);
        }
        let mut qb = [[0.0; 9]; 3];
        for row in 0..3 {
            for col in 0..9 {
                qb[row][col] =
                    q[row][0] * b[0][col] + q[row][1] * b[1][col] + q[row][2] * b[2][col];
            }
        }
        let mut hess = [0.0; 81];
        for i in 0..9 {
            for j in 0..9 {
                hess[i * 9 + j] =
                    scale * (b[0][i] * qb[0][j] + b[1][i] * qb[1][j] + b[2][i] * qb[2][j]);
            }
        }
        for a in 0..3 {
            for col in 0..3 {
                let (ga, gc) = (self.g[a], self.g[col]);
                let s = sigma[0] * ga[0] * gc[0]
                    + sigma[1] * ga[1] * gc[1]
                    + sigma[2] * (ga[0] * gc[1] + ga[1] * gc[0]);
                for k in 0..3 {
                    hess[(3 * a + k) * 9 + 3 * col + k] += scale * s;
                }
            }
        }
        let psd = c >= 0.0
            && sigma[0] >= 0.0
            && sigma[1] >= 0.0
            && sigma[0] * sigma[1] >= sigma[2] * sigma[2];
        Local {
            energy: 0.5 * scale * (de[0] * sigma[0] + de[1] * sigma[1] + de[2] * sigma[2]),
            grad,
            hess,
            psd,
        }
    }
}

/// Scalar isotropic principal-strain softening ratio `r = (c0 * c1)^(1/4) ∈ (0, 1]` (equals
/// `c.sqrt()` whenever warp and weft compression factors coincide).
fn principal_ratio(m: &Membrane) -> f64 {
    (m.compression[0] * m.compression[1]).sqrt().sqrt()
}

/// Scalar `C²` softening `φ(λ)` and its derivatives `(φ, φ', φ'', φ''', φ⁴, φ⁵)`.
fn phi_derivs(lambda: f64, r: f64, delta: f64) -> [f64; 6] {
    if lambda >= 0.0 || r >= 1.0 {
        return [lambda, 1.0, 0.0, 0.0, 0.0, 0.0];
    }
    let t = ftx_core::det::tanh(lambda / delta);
    let t2 = t * t;
    let s2 = 1.0 - t2;
    let omr = 1.0 - r;
    let inv_d = 1.0 / delta;
    let inv_d2 = inv_d * inv_d;
    let inv_d3 = inv_d2 * inv_d;
    let inv_d4 = inv_d2 * inv_d2;
    [
        r * lambda + omr * delta * t,
        r + omr * s2,
        -omr * 2.0 * s2 * t * inv_d,
        -omr * 2.0 * s2 * (1.0 - 3.0 * t2) * inv_d2,
        omr * 8.0 * t * s2 * (2.0 - 3.0 * t2) * inv_d3,
        omr * 8.0 * s2 * (2.0 - 15.0 * t2 + 15.0 * t2 * t2) * inv_d4,
    ]
}

fn soften_principal_value(e: [f64; 3], r: f64, delta: f64) -> [f64; 3] {
    let m = 0.5 * (e[0] + e[1]);
    let d = 0.5 * (e[0] - e[1]);
    let g = 0.5 * e[2];
    let s2 = d * d + g * g;
    let s = s2.sqrt();
    let l1 = m + s;
    let l2 = m - s;
    if l2 >= 0.0 {
        return e;
    }
    let (a, b) = if s > 1e-4 * delta {
        let p1 = phi_derivs(l1, r, delta)[0];
        let p2 = phi_derivs(l2, r, delta)[0];
        (0.5 * (p1 + p2), (p1 - p2) / (2.0 * s))
    } else if l1 < 0.0 {
        let p = phi_derivs(m, r, delta);
        (
            p[0] + 0.5 * p[2] * s2 + (1.0 / 24.0) * p[4] * s2 * s2,
            p[1] + (1.0 / 6.0) * p[3] * s2 + (1.0 / 120.0) * p[5] * s2 * s2,
        )
    } else if s > 1e-12 * delta {
        let p1 = phi_derivs(l1, r, delta)[0];
        let p2 = phi_derivs(l2, r, delta)[0];
        (0.5 * (p1 + p2), (p1 - p2) / (2.0 * s))
    } else {
        (m, 1.0)
    };
    [a + b * d, a - b * d, b * e[2]]
}

fn jet_bivariate<const N: usize>(
    m: Jet<N>,
    u: Jet<N>,
    f: f64,
    grad: [f64; 2],
    hess: [f64; 3],
) -> Jet<N> {
    let [fm, fu] = grad;
    let [fmm, fmu, fuu] = hess;
    let mut out = Jet::constant(f);
    for i in 0..N {
        out.g[i] = fm * m.g[i] + fu * u.g[i];
        for j in 0..N {
            out.h[i][j] = fm * m.h[i][j]
                + fu * u.h[i][j]
                + fmm * m.g[i] * m.g[j]
                + fuu * u.g[i] * u.g[j]
                + fmu * (m.g[i] * u.g[j] + u.g[i] * m.g[j]);
        }
    }
    out
}

/// Exact singularity-free spectral transform `Φ(E) = A(m, s²) I + B(m, s²)(E − m I)` and its
/// first/second derivatives with respect to Voigt strain `(E₁₁, E₂₂, γ₁₂)`.
fn soften_principal_jets(e: [f64; 3], r: f64, delta: f64) -> [Jet<3>; 3] {
    let e0 = Jet::<3>::var(e[0], 0);
    let e1 = Jet::<3>::var(e[1], 1);
    let e2 = Jet::<3>::var(e[2], 2);
    let m = (e0 + e1) * 0.5;
    let d = (e0 - e1) * 0.5;
    let g = e2 * 0.5;
    let u = d * d + g * g;
    let s2 = u.v;
    let s = s2.sqrt();
    let l1 = m.v + s;
    let l2 = m.v - s;
    if l2 >= 0.0 {
        return [e0, e1, e2];
    }
    let (a_jet, b_jet) = if s > 5e-3 * delta || (l1 >= 0.0 && s > 1e-6 * delta) {
        let p1 = phi_derivs(l1, r, delta);
        let p2 = phi_derivs(l2, r, delta);
        let inv_2s = 0.5 / s;
        let inv_2u = 0.5 / s2;
        let a_v = 0.5 * (p1[0] + p2[0]);
        let a_m = 0.5 * (p1[1] + p2[1]);
        let b_v = (p1[0] - p2[0]) * inv_2s;
        let b_m = (p1[1] - p2[1]) * inv_2s;
        let a_u = 0.5 * b_m;
        let a_mm = 0.5 * (p1[2] + p2[2]);
        let b_mm = (p1[2] - p2[2]) * inv_2s;
        let a_mu = 0.5 * b_mm;
        let a_uu = (a_mm - b_m) * (0.25 / s2);
        let b_u = (a_m - b_v) * inv_2u;
        let b_mu = (a_mm - b_m) * inv_2u;
        let b_uu = (a_mu - 3.0 * b_u) * inv_2u;
        (
            jet_bivariate(m, u, a_v, [a_m, a_u], [a_mm, a_mu, a_uu]),
            jet_bivariate(m, u, b_v, [b_m, b_u], [b_mm, b_mu, b_uu]),
        )
    } else if l1 < 0.0 {
        // Both eigenvalues strictly negative and close (`s <= 5e-3 * delta`): exact analytic
        // Taylor expansion in `u = s²` around `m` avoids `0/0` division completely.
        let p = phi_derivs(m.v, r, delta);
        let a_v = p[0] + 0.5 * p[2] * s2 + (1.0 / 24.0) * p[4] * s2 * s2;
        let a_m = p[1] + 0.5 * p[3] * s2;
        let a_u = 0.5 * p[2] + (1.0 / 12.0) * p[4] * s2;
        let a_mm = p[2] + 0.5 * p[4] * s2;
        let a_mu = 0.5 * p[3] + (1.0 / 12.0) * p[5] * s2;
        let a_uu = (1.0 / 12.0) * p[4];

        let b_v = p[1] + (1.0 / 6.0) * p[3] * s2 + (1.0 / 120.0) * p[5] * s2 * s2;
        let b_m = p[2] + (1.0 / 6.0) * p[4] * s2;
        let b_u = (1.0 / 6.0) * p[3] + (1.0 / 60.0) * p[5] * s2;
        let b_mm = p[3] + (1.0 / 6.0) * p[5] * s2;
        let b_mu = (1.0 / 6.0) * p[4];
        let b_uu = (1.0 / 60.0) * p[5];
        (
            jet_bivariate(m, u, a_v, [a_m, a_u], [a_mm, a_mu, a_uu]),
            jet_bivariate(m, u, b_v, [b_m, b_u], [b_mm, b_mu, b_uu]),
        )
    } else {
        // Straddling zero with `|m| <= s <= 1e-6 * delta`: `φ(λ) - λ = O(1e-18 * delta)`.
        return [e0, e1, e2];
    };
    [a_jet + b_jet * d, a_jet - b_jet * d, b_jet * e2]
}

fn mat3(q: &[[f64; 3]; 3], e: [f64; 3]) -> [f64; 3] {
    [
        q[0][0] * e[0] + q[0][1] * e[1] + q[0][2] * e[2],
        q[1][0] * e[0] + q[1][1] * e[1] + q[1][2] * e[2],
        q[2][0] * e[0] + q[2][1] * e[1] + q[2][2] * e[2],
    ]
}

/// Compression softening of the two normal strains: effective strains φ(e), φ'(e) and φ''(e).
pub(crate) fn soften(e: [f64; 3], m: &Membrane) -> ([f64; 3], [f64; 2], [f64; 2]) {
    let mut eff = e;
    let mut d = [1.0; 2];
    let mut dd = [0.0; 2];
    for i in 0..2 {
        let r = m.compression[i].sqrt();
        if e[i] < 0.0 && r < 1.0 {
            let delta = m.transition;
            let t = ftx_core::det::tanh(e[i] / delta);
            let sech2 = 1.0 - t * t;
            eff[i] = r * e[i] + (1.0 - r) * delta * t;
            d[i] = r + (1.0 - r) * sech2;
            dd[i] = -(1.0 - r) * 2.0 * sech2 * t / delta;
        }
    }
    (eff, d, dd)
}

/// A bending hinge: interior edge (v0, v1) with opposite vertices v2 and v3.
///
/// Energy `½·B·w·|Σᵢ Kᵢ xᵢ|²`. On a fine mesh `|Σ Kᵢxᵢ|² → κ²(A₀+A₁)²/3` for normal curvature κ across
/// the edge, so `w = 3a/(A₀+A₁)²` makes the hinge carry `½·B·κ²·a`, the plate energy of the area `a` it
/// stands for. Interior hinges stand for a third of each triangle (`w = 1/(A₀+A₁)`); [`Hinge::set_area`]
/// lets a mesh hand boundary triangles' whole area to their few hinges (else free edges are too soft by
/// O(h)). Checked on a cylinder: total energy → ½·B·A/R².
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hinge {
    /// `[edge start, edge end, opposite in the first triangle, opposite in the second]`.
    pub v: [usize; 4],
    /// Cotangent weights (they sum to zero, so translations store no energy).
    pub k: [f64; 4],
    /// Rest areas of the two triangles (m²).
    pub areas: [f64; 2],
    /// `3a / (A₀ + A₁)²` (1/m²).
    pub weight: f64,
    /// Common rest length of the shared hinge edge (m); equals `|e|` on an isometric edge and
    /// `2(A₀+A₁)/(altitude₀+altitude₁)` on an eased seam.
    pub edge_length: f64,
    /// Rest dihedral angle `θ₀` (rad, default `0.0` flat) used by [`BendingModel::DiscreteShells`]
    /// for pressed pleats, ironed creases, or folded lapels.
    pub rest_angle: f64,
    // Rest-chart normal orientation sign (`+1` or `-1`, shared by both triangles) so
    // `dihedral_angle == 0` whenever the two triangles unfold flat onto opposite sides of the
    // shared edge — matching `BendingModel::Quadratic` even across cross-chart seams where both
    // 2D rest charts have the same winding sign — and positive angles always fold toward `first`'s
    // public (`+z` rest) side regardless of vertex swap or edge reversal.
    pub(crate) orient: f64,
}

fn cot(a: Vec2, b: Vec2) -> f64 {
    a.dot(b) / a.cross(b).abs()
}

impl Hinge {
    /// From the flat rest coordinates of the stencil; `None` if a triangle is degenerate.
    #[must_use]
    pub fn new(v: [usize; 4], rest: [Vec2; 4]) -> Option<Self> {
        let [x0, x1, x2, x3] = rest;
        Self::from_rest_triangles(v, [x0, x1, x2], [x0, x1, x3])
    }

    /// Both rest triangles give the edge endpoints in `v[0], v[1]` order. An eased seam can have a
    /// different cut length on each side, so each triangle supplies its own angles and area.
    pub(crate) fn from_rest_triangles(
        v: [usize; 4],
        first: [Vec2; 3],
        second: [Vec2; 3],
    ) -> Option<Self> {
        let [a, b, _] = first;
        let [c, d, opposite] = second;
        let (edge0, edge1) = (b - a, d - c);
        let (edge0_sq, edge1_sq) = (edge0.norm2(), edge1.norm2());
        let cross0 = edge0.cross(first[2] - a);
        // An isometric weld admits the original rigid map into the first chart. Keep its
        // arithmetic on that domain, including the identity for neighbors in the same chart.
        let second = if edge0_sq == edge1_sq && (a, b) != (c, d) {
            let (u, w) = (edge0.normalized(), edge1.normalized());
            let (cos, sin) = (u.dot(w), w.cross(u));
            let r = opposite - c;
            let mapped = a + Vec2::new(cos * r.x - sin * r.y, sin * r.x + cos * r.y);
            [a, b, mapped]
        } else {
            second
        };
        let geometry = |[a, b, opposite]: [Vec2; 3]| {
            let edge = b - a;
            let arm = opposite - a;
            (
                cot(edge, arm),
                cot(-edge, opposite - b),
                0.5 * edge.cross(arm).abs(),
            )
        };
        let (mut start0, mut end0, a0) = geometry(first);
        let (mut start1, mut end1, a1) = geometry(second);
        let edge_length = if edge0_sq != edge1_sq {
            // Each cotangent stencil equals its cut edge length times the cross-edge derivative.
            // Compare the derivatives at a common length, or a flat, tangentially eased seam bends.
            // Keeping the authored area gives L = 2(A₀+A₁)/(altitude₀+altitude₁).
            let (l0, l1) = (edge0_sq.sqrt(), edge1_sq.sqrt());
            let common = (a0 + a1) / (a0 / l0 + a1 / l1);
            start0 *= common / l0;
            end0 *= common / l0;
            start1 *= common / l1;
            end1 *= common / l1;
            common
        } else {
            edge0_sq.sqrt()
        };
        let k = [end0 + end1, start0 + start1, -start0 - end0, -start1 - end1];
        let orient = if cross0 >= 0.0 { 1.0 } else { -1.0 };
        (a0 > 0.0
            && a1 > 0.0
            && edge_length.is_finite()
            && edge_length > 0.0
            && k.iter().all(|c| c.is_finite()))
        .then(|| Self {
            v,
            k,
            areas: [a0, a1],
            weight: 1.0 / (a0 + a1),
            edge_length,
            rest_angle: 0.0,
            orient,
        })
    }

    /// Sets the area this hinge stands for (m²); the default is a third of each triangle.
    pub fn set_area(&mut self, area: f64) {
        let s = self.areas[0] + self.areas[1];
        self.weight = 3.0 * area / (s * s);
    }

    /// Returns a copy of this hinge with rest dihedral angle `theta0` (rad) for pressed pleats or
    /// ironed creases under [`BendingModel::DiscreteShells`].
    #[must_use]
    pub const fn with_rest_angle(mut self, theta0: f64) -> Self {
        self.rest_angle = theta0;
        self
    }

    /// Dimensionless Discrete Shells geometric weight `w_d = w · |e_common|² = 3 a |e|² / (A₀ + A₁)²`.
    /// On both equilateral and right-triangle meshes rolled to radius `R`, `Σ ½ B w_d θ² → ½ B A / R²`.
    #[must_use]
    pub fn dihedral_weight(&self) -> f64 {
        self.weight * self.edge_length * self.edge_length
    }

    /// Signed dihedral angle `θ ∈ (−π, π]` (rad) between the two adjacent triangles: `0` when
    /// flat, positive for a valley fold toward the public (`+z` rest) side, negative for a
    /// mountain fold. Returns `NaN` if the shared edge or either triangle is degenerate.
    #[must_use]
    pub fn dihedral_angle(&self, x: &[Vec3]) -> f64 {
        let p0 = x[self.v[0]];
        let p1 = x[self.v[1]];
        let p2 = x[self.v[2]];
        let p3 = x[self.v[3]];
        let e = p1 - p0;
        let e_len = e.norm();
        if !(e_len.is_finite() && e_len > 0.0) {
            return f64::NAN;
        }
        let e_hat = e * (self.orient / e_len);
        let n0 = e.cross(p2 - p0) * self.orient;
        let n1 = (p3 - p0).cross(e) * self.orient;
        let cos_raw = n0.dot(n1);
        let sin_raw = n1.cross(n0).dot(e_hat);
        let scale = cos_raw.abs().max(sin_raw.abs());
        if !(scale.is_finite() && scale > 0.0) {
            return f64::NAN;
        }
        ftx_core::det::atan2(sin_raw / scale, cos_raw / scale)
    }

    /// Exact value, gradient (12) and Hessian (12 × 12) of the signed dihedral angle `θ(x)` via
    /// deterministic `atan2` forward-mode autodiff (`Jet<12>`).
    #[must_use]
    pub fn dihedral_angle_jet(&self, x: &[Vec3]) -> Jet<12> {
        let p0 = JVec::<Jet<12>>::point(x[self.v[0]], 0, false);
        let p1 = JVec::<Jet<12>>::point(x[self.v[1]], 1, false);
        let p2 = JVec::<Jet<12>>::point(x[self.v[2]], 2, false);
        let p3 = JVec::<Jet<12>>::point(x[self.v[3]], 3, false);
        let e = p1.sub(&p0);
        let e_len2 = e.dot(&e);
        if !(e_len2.v.is_finite() && e_len2.v > 0.0) {
            return Jet {
                v: f64::NAN,
                g: [f64::NAN; 12],
                h: [[f64::NAN; 12]; 12],
            };
        }
        let inv_e_len = Jet::constant(self.orient) / e_len2.sqrt();
        let e_hat = JVec([e.0[0] * inv_e_len, e.0[1] * inv_e_len, e.0[2] * inv_e_len]);
        let d2 = p2.sub(&p0);
        let d3 = p3.sub(&p0);
        let n0_raw = e.cross(&d2);
        let n1_raw = d3.cross(&e);
        let s = self.orient;
        let n0 = JVec([n0_raw.0[0] * s, n0_raw.0[1] * s, n0_raw.0[2] * s]);
        let n1 = JVec([n1_raw.0[0] * s, n1_raw.0[1] * s, n1_raw.0[2] * s]);
        let cos_raw = n0.dot(&n1);
        let sin_raw = n1.cross(&n0).dot(&e_hat);
        Jet::atan2(sin_raw, cos_raw)
    }

    fn curvature_vector(&self, x: &[Vec3]) -> Vec3 {
        self.v
            .iter()
            .zip(self.k)
            .fold(Vec3::ZERO, |acc, (&vi, ki)| acc + x[vi] * ki)
    }

    /// Energy `½·B·w·|Σ Kᵢxᵢ|²` for bending stiffness `b` (N·m).
    #[must_use]
    pub fn energy(&self, x: &[Vec3], b: f64) -> f64 {
        0.5 * b * self.weight * self.curvature_vector(x).norm2()
    }

    /// Finite-angle Discrete Shells bending energy `½·B·w_d·(θ − θ₀)²` for bending stiffness `b` (N·m).
    #[must_use]
    pub fn dihedral_energy(&self, x: &[Vec3], b: f64) -> f64 {
        if b == 0.0 {
            return 0.0;
        }
        let d = self.dihedral_angle(x) - self.rest_angle;
        0.5 * b * self.dihedral_weight() * d * d
    }

    /// Energy under the chosen [`BendingModel`]. [`BendingModel::Quadratic`] is bit-identical to
    /// [`Self::energy`].
    #[must_use]
    pub fn energy_with_model(&self, x: &[Vec3], b: f64, model: BendingModel) -> f64 {
        match model {
            BendingModel::Quadratic => self.energy(x, b),
            BendingModel::DiscreteShells => self.dihedral_energy(x, b),
        }
    }

    /// Energy, gradient (12) and Hessian (12 × 12, constant).
    #[must_use]
    pub fn local(&self, x: &[Vec3], b: f64) -> Local<12, 144> {
        let s = self.curvature_vector(x);
        let c = b * self.weight;
        let mut grad = [0.0; 12];
        let mut hess = [0.0; 144];
        for i in 0..4 {
            for k in 0..3 {
                grad[3 * i + k] = c * self.k[i] * s[k];
            }
            for j in 0..4 {
                for k in 0..3 {
                    hess[(3 * i + k) * 12 + 3 * j + k] = c * self.k[i] * self.k[j];
                }
            }
        }
        Local {
            energy: 0.5 * c * s.norm2(),
            grad,
            hess,
            // B·w·(k kᵀ ⊗ I₃): a Gram matrix
            psd: c >= 0.0,
        }
    }

    /// Finite-angle Discrete Shells energy, gradient (12) and exact Hessian (12 × 12) for
    /// `½·B·w_d·(θ − θ₀)²`.
    #[must_use]
    pub fn dihedral_local(&self, x: &[Vec3], b: f64) -> Local<12, 144> {
        if b == 0.0 {
            return Local {
                energy: 0.0,
                grad: [0.0; 12],
                hess: [0.0; 144],
                psd: true,
            };
        }
        self.dihedral_quadratic_local(x, self.rest_angle, b * self.dihedral_weight())
    }

    /// Shared helper for `½ · coeff · (θ(x) − target)²` (used by both [`Self::dihedral_local`]
    /// and SE(3)-invariant Rayleigh bending damping).
    pub(crate) fn dihedral_quadratic_local(
        &self,
        x: &[Vec3],
        target: f64,
        coeff: f64,
    ) -> Local<12, 144> {
        let theta = self.dihedral_angle_jet(x);
        let d = theta - Jet::constant(target);
        let e = d * d * (0.5 * coeff);
        let mut hess = [0.0; 144];
        for i in 0..12 {
            for j in 0..12 {
                hess[i * 12 + j] = e.h[i][j];
            }
        }
        Local {
            energy: e.v,
            grad: e.g,
            hess,
            psd: false,
        }
    }

    /// Energy, gradient and Hessian under the chosen [`BendingModel`].
    #[must_use]
    pub fn local_with_model(&self, x: &[Vec3], b: f64, model: BendingModel) -> Local<12, 144> {
        match model {
            BendingModel::Quadratic => self.local(x, b),
            BendingModel::DiscreteShells => self.dihedral_local(x, b),
        }
    }
}

/// Width of the slack-to-taut blend of an elastic (m).
pub const ELASTIC_ONSET: f64 = 1e-4;

/// A tension-only elastic segment (a stretch of the elastic tape in a casing): `(EA/r)·g(l − r)` with
/// `g` zero below `r − δ`, `(e + δ)³/(6δ)` up to `r` and `e²/2 + δe/2 + δ²/6` beyond (δ =
/// [`ELASTIC_ONSET`]). A slack elastic stores nothing, a stretched one pulls like a spring of stiffness
/// EA/r, and the blend keeps the energy C² (a kink at `l = r` would make Newton cycle, as compression's
/// did). Its Hessian `(EA/r)·[g''·n nᵀ + (g'/l)(I − n nᵀ)]` is PSD: `g'`, `g''` ≥ 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElasticSegment {
    /// The two vertices.
    pub v: [usize; 2],
    /// Relaxed length (m).
    pub rest: f64,
    /// Axial stiffness EA (N).
    pub ea: f64,
}

/// `g(e)`, `g'(e)`, `g''(e)` of [`ElasticSegment`].
fn tension_only(e: f64) -> (f64, f64, f64) {
    let d = ELASTIC_ONSET;
    if e <= -d {
        (0.0, 0.0, 0.0)
    } else if e < 0.0 {
        let s = e + d;
        (s * s * s / (6.0 * d), s * s / (2.0 * d), s / d)
    } else {
        (0.5 * e * e + 0.5 * d * e + d * d / 6.0, e + 0.5 * d, 1.0)
    }
}

impl ElasticSegment {
    /// Energy (J).
    #[must_use]
    pub fn energy(&self, x: &[Vec3]) -> f64 {
        let l = (x[self.v[1]] - x[self.v[0]]).norm();
        self.ea / self.rest * tension_only(l - self.rest).0
    }

    /// Tension (N): the force along the segment.
    #[must_use]
    pub fn tension(&self, x: &[Vec3]) -> f64 {
        let l = (x[self.v[1]] - x[self.v[0]]).norm();
        self.ea / self.rest * tension_only(l - self.rest).1
    }

    /// Energy, gradient (6) and Hessian (6 × 6).
    #[must_use]
    pub fn local(&self, x: &[Vec3]) -> Local<6, 36> {
        let k = self.ea / self.rest;
        let d = x[self.v[1]] - x[self.v[0]];
        let l = d.norm();
        let (g, g1, g2) = tension_only(l - self.rest);
        let mut grad = [0.0; 6];
        let mut hess = [0.0; 36];
        if l > 0.0 && g1 > 0.0 {
            let n = d * (1.0 / l);
            // the 3 × 3 block H₁₁ = k·[g''·n nᵀ + (g'/l)(I − n nᵀ)]; H₀₀ = H₁₁, H₀₁ = −H₁₁
            let mut b = [[0.0; 3]; 3];
            for (i, row) in b.iter_mut().enumerate() {
                for (j, h) in row.iter_mut().enumerate() {
                    let id = if i == j { 1.0 } else { 0.0 };
                    *h = k * (g2 * n[i] * n[j] + g1 / l * (id - n[i] * n[j]));
                }
            }
            for i in 0..3 {
                grad[i] = -k * g1 * n[i];
                grad[3 + i] = k * g1 * n[i];
                for j in 0..3 {
                    hess[i * 6 + j] = b[i][j];
                    hess[(3 + i) * 6 + 3 + j] = b[i][j];
                    hess[i * 6 + 3 + j] = -b[i][j];
                    hess[(3 + i) * 6 + j] = -b[i][j];
                }
            }
        }
        Local {
            energy: k * g,
            grad,
            hess,
            psd: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((*seed >> 11) as f64) / ((1u64 << 53) as f64) - 0.5
    }

    /// Central differences (step `h`, m) of the energy and of the gradient against the analytic
    /// gradient and Hessian.
    fn check<const N: usize, const NN: usize>(
        x: &[Vec3],
        dofs: &[usize],
        h: f64,
        energy: impl Fn(&[Vec3]) -> f64,
        local: impl Fn(&[Vec3]) -> Local<N, NN>,
    ) {
        let l = local(x);
        assert!((l.energy - energy(x)).abs() <= 1e-14 * l.energy.abs().max(1e-12));
        let scale_g = l.grad.iter().fold(0.0f64, |m, g| m.max(g.abs())).max(1e-12);
        let scale_h = l.hess.iter().fold(0.0f64, |m, g| m.max(g.abs())).max(1e-12);
        for i in 0..N {
            let (vi, k) = (dofs[i / 3], i % 3);
            let mut xp = x.to_vec();
            let mut xm = x.to_vec();
            let bump = |p: &mut Vec3, d: f64| match k {
                0 => p.x += d,
                1 => p.y += d,
                _ => p.z += d,
            };
            bump(&mut xp[vi], h);
            bump(&mut xm[vi], -h);
            let fd = (energy(&xp) - energy(&xm)) / (2.0 * h);
            assert!(
                (fd - l.grad[i]).abs() < 1e-6 * scale_g,
                "grad[{i}]: fd {fd} vs {}",
                l.grad[i]
            );
            let (gp, gm) = (local(&xp).grad, local(&xm).grad);
            for j in 0..N {
                let fdh = (gp[j] - gm[j]) / (2.0 * h);
                assert!(
                    (fdh - l.hess[j * N + i]).abs() < 1e-5 * scale_h,
                    "hess[{j}][{i}]: fd {fdh} vs {}",
                    l.hess[j * N + i]
                );
            }
        }
    }

    /// Whenever an element says its Hessian is PSD by construction, its eigenvalues are ≥ −rounding;
    /// and both answers occur over the sample (stretched and compressed states).
    #[test]
    fn psd_by_construction_holds() {
        let mat = Membrane::orthotropic(800.0, 400.0, 0.2, 60.0, 0.05);
        let rest = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.012, 0.001),
            Vec2::new(0.003, 0.010),
        ];
        let e = MembraneElement::new([0, 1, 2], rest).unwrap();
        let mut seed = 7u64;
        let (mut yes, mut no) = (0, 0);
        for _ in 0..400 {
            let s = 1.0 + 0.2 * lcg(&mut seed);
            let x: Vec<Vec3> = rest
                .iter()
                .map(|r| {
                    Vec3::new(
                        s * r.x + 0.002 * lcg(&mut seed),
                        s * r.y + 0.002 * lcg(&mut seed),
                        0.002 * lcg(&mut seed),
                    )
                })
                .collect();
            let l = e.local(&x, &mat);
            let (vals, _) = crate::eigen::jacobi_eigh(&l.hess, 9);
            let scale = vals.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            if l.psd {
                yes += 1;
                assert!(vals[0] >= -1e-10 * scale, "{vals:?}");
            } else {
                no += 1;
            }
        }
        assert!(yes > 20 && no > 20, "{yes} PSD, {no} not");
        let hinge = Hinge::new(
            [0, 1, 2, 3],
            [
                Vec2::new(0.0, 0.0),
                Vec2::new(0.01, 0.0),
                Vec2::new(0.004, 0.008),
                Vec2::new(0.006, -0.009),
            ],
        )
        .unwrap();
        let x = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.01, 0.0, 0.0),
            Vec3::new(0.004, 0.008, 0.003),
            Vec3::new(0.006, -0.009, 0.002),
        ];
        let l = hinge.local(&x, 1e-5);
        let (vals, _) = crate::eigen::jacobi_eigh(&l.hess, 12);
        assert!(l.psd && vals[0] >= -1e-12 * vals[11], "{vals:?}");
    }

    #[test]
    fn membrane_derivatives_match_finite_differences() {
        let mat = Membrane::orthotropic(800.0, 400.0, 0.2, 60.0, 0.05);
        let rest = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.012, 0.001),
            Vec2::new(0.003, 0.010),
        ];
        let e = MembraneElement::new([0, 1, 2], rest).unwrap();
        let mut seed = 5;
        for case in 0..20 {
            // Random deformations around a stretched or compressed state, and (every third case) small
            // ones that keep the strains inside the softening blend around zero.
            let s = [1.1, 0.9, 0.9995][case % 3];
            let noise = if case % 3 == 2 { 2e-6 } else { 0.002 };
            let x: Vec<Vec3> = rest
                .iter()
                .map(|p| {
                    Vec3::new(p.x * s, p.y * (2.0 - s), 0.0)
                        + Vec3::new(lcg(&mut seed), lcg(&mut seed), lcg(&mut seed)) * noise
                })
                .collect();
            // Inside the blend the strain moves by ~1e-4 per micrometre: step finer there.
            let h = if case % 3 == 2 { 1e-8 } else { 1e-6 };
            check(
                &x,
                &[0, 1, 2],
                h,
                |x| e.energy(x, &mat),
                |x| e.local(x, &mat),
            );
        }
    }

    #[test]
    fn nonlinear_membrane_derivatives_match_finite_differences() {
        let raw: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-nonlinear.json"
        ))
        .unwrap();
        let response: ftx_pattern::material_response::TabulatedMembrane =
            serde_json::from_value(raw["membrane_response"].clone()).unwrap();
        let sources = serde_json::from_value(raw["sources"].clone()).unwrap();
        let law = response.compile(&sources, true).unwrap();
        let [q11, q22, q12, g] = law.origin_stiffness();
        let nu = q12 / q22;
        let d = 1.0 - nu * q12 / q11;
        let mat = Membrane::orthotropic(q11 * d, q22 * d, nu, g, 0.2);
        let rest = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.012, 0.001),
            Vec2::new(0.003, 0.010),
        ];
        let e = MembraneElement::new([0, 1, 2], rest).unwrap();
        let x: Vec<Vec3> = rest
            .iter()
            .map(|p| Vec3::new(1.07 * p.x + 0.01 * p.y, 0.96 * p.y, 0.02 * p.x))
            .collect();
        assert!(law.contains(e.strain(&x)));
        check(
            &x,
            &[0, 1, 2],
            1e-7,
            |x| e.response_energy(x, &law, &mat),
            |x| e.response_local(x, &law, &mat),
        );
        assert!(e.response_local(&x, &law, &mat).energy.is_finite());
    }

    #[test]
    fn rigid_motion_stores_no_membrane_energy() {
        let mat = Membrane::orthotropic(800.0, 400.0, 0.2, 60.0, 0.05);
        let rest = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.012, 0.001),
            Vec2::new(0.003, 0.010),
        ];
        let e = MembraneElement::new([0, 1, 2], rest).unwrap();
        let r = ftx_core::Mat3::rotation(Vec3::new(0.3, -0.5, 0.8).normalized(), 1.1);
        let x: Vec<Vec3> = rest
            .iter()
            .map(|p| r.mul_vec(Vec3::new(p.x, p.y, 0.0)) + Vec3::new(1.0, 2.0, 3.0))
            .collect();
        let l = e.local(&x, &mat);
        assert!(l.energy.abs() < 1e-20, "{}", l.energy);
        assert!(l.grad.iter().all(|g| g.abs() < 1e-12), "{:?}", l.grad);
    }

    #[test]
    fn bending_derivatives_and_flat_zero() {
        let rest = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.01, 0.0),
            Vec2::new(0.004, 0.008),
            Vec2::new(0.006, -0.009),
        ];
        let hinge = Hinge::new([0, 1, 2, 3], rest).unwrap();
        let flat: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        assert!(hinge.energy(&flat, 1e-5) < 1e-30);
        let mut seed = 9;
        let x: Vec<Vec3> = flat
            .iter()
            .map(|p| *p + Vec3::new(lcg(&mut seed), lcg(&mut seed), lcg(&mut seed)) * 0.003)
            .collect();
        check(
            &x,
            &[0, 1, 2, 3],
            1e-6,
            |x| hinge.energy(x, 1e-5),
            |x| hinge.local(x, 1e-5),
        );
    }

    #[test]
    fn elastic_derivatives_in_every_regime() {
        // 20 mm relaxed; stretched to 22 mm, inside the onset blend, and slack
        let seg = ElasticSegment {
            v: [0, 1],
            rest: 0.02,
            ea: 15.0,
        };
        for (len, tilt) in [
            (0.022, 0.3_f64),
            (0.02 - 0.5 * ELASTIC_ONSET, 0.7),
            (0.015, 0.2),
        ] {
            let x = [
                Vec3::new(0.001, -0.002, 0.003),
                Vec3::new(0.001, -0.002, 0.003)
                    + Vec3::new(tilt.cos(), tilt.sin(), 0.4).normalized() * len,
            ];
            check(&x, &[0, 1], 1e-8, |x| seg.energy(x), |x| seg.local(x));
        }
        // slack: nothing; stretched by 10 %: tension EA·10 % (plus the blend's δ/2)
        let at = |len: f64| [Vec3::ZERO, Vec3::new(len, 0.0, 0.0)];
        assert_eq!(seg.energy(&at(0.015)), 0.0);
        assert_eq!(seg.local(&at(0.015)).grad, [0.0; 6]);
        let t = seg.tension(&at(0.022));
        let want = 15.0 / 0.02 * (0.002 + 0.5 * ELASTIC_ONSET);
        assert!((t - want).abs() < 1e-12, "{t} vs {want}");
        assert!(seg.local(&at(0.022)).psd);
    }

    #[test]
    fn discrete_shells_dihedral_derivatives_inplane_invariance_and_hairpin_torque() {
        let rest = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.01, 0.0),
            Vec2::new(0.004, 0.008),
            Vec2::new(0.006, -0.009),
        ];
        let hinge = Hinge::new([0, 1, 2, 3], rest).unwrap();
        let b = 2.5e-5;

        // Non-uniform in-plane stretch stores ZERO Discrete Shells energy & gradient.
        let stretched = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.013, 0.001, 0.0),
            Vec3::new(0.005, 0.011, 0.0),
            Vec3::new(0.007, -0.007, 0.0),
        ];
        let l_flat = hinge.dihedral_local(&stretched, b);
        assert!(l_flat.energy.abs() < 1e-30, "{}", l_flat.energy);
        assert!(
            l_flat.grad.iter().all(|g| g.abs() < 1e-20),
            "{:?}",
            l_flat.grad
        );

        // Central FD check across small, 90°, sharp (150° & 175°), negative, and creased rest angles.
        let sym_rest = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.01, 0.0),
            Vec2::new(0.005, 0.01),
            Vec2::new(0.005, -0.01),
        ];
        for theta0 in [0.0, 0.7] {
            let h_sym = Hinge::new([0, 1, 2, 3], sym_rest)
                .unwrap()
                .with_rest_angle(theta0);
            for deg in [0.0_f64, 35.0, 90.0, -60.0, 150.0, 175.0] {
                let rad = deg.to_radians();
                let (s, c) = ftx_core::det::sin_cos(rad);
                let x = [
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(0.01, 0.0, 0.0),
                    Vec3::new(0.005, 0.01, 0.0),
                    Vec3::new(0.005, -0.01 * c, 0.01 * s),
                ];
                let measured = h_sym.dihedral_angle(&x);
                assert!(
                    (measured - rad).abs() < 1e-12,
                    "deg={deg} measured={measured} want={rad}"
                );
                check(
                    &x,
                    &[0, 1, 2, 3],
                    1e-7,
                    |p| h_sym.dihedral_energy(p, b),
                    |p| h_sym.dihedral_local(p, b),
                );
            }
        }

        // Swapping opposite vertices preserves signed angle convention (valley toward +z > 0).
        let swapped = Hinge::new([0, 1, 3, 2], [rest[0], rest[1], rest[3], rest[2]]).unwrap();
        let (s, c) = ftx_core::det::sin_cos(40.0_f64.to_radians());
        let fold_up = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.01, 0.0, 0.0),
            Vec3::new(0.004, 0.008, 0.0),
            Vec3::new(0.006, -0.009 * c, 0.009 * s),
        ];
        assert!((hinge.dihedral_angle(&fold_up) - swapped.dihedral_angle(&fold_up)).abs() < 1e-12);

        // Cross-chart seam where both 2D rest triangles have the same winding sign (e.g. a
        // `reverse: false` seam) must still report theta == 0 when unfolded flat in 3D, matching
        // `BendingModel::Quadratic`, and positive valley angle + exact FD derivatives when folded.
        let same_sign_seam = Hinge::from_rest_triangles(
            [0, 1, 2, 3],
            [rest[0], rest[1], rest[2]],
            [rest[0], rest[1], Vec2::new(0.006, 0.009)],
        )
        .unwrap();
        let flat_3d = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.01, 0.0, 0.0),
            Vec3::new(0.004, 0.008, 0.0),
            Vec3::new(0.006, -0.009, 0.0),
        ];
        assert!(same_sign_seam.energy(&flat_3d, b).abs() < 1e-24);
        assert!(
            same_sign_seam.dihedral_angle(&flat_3d).abs() < 1e-14,
            "same-sign cross-chart seam must have zero dihedral angle when flat, got {}",
            same_sign_seam.dihedral_angle(&flat_3d)
        );
        assert!(
            (same_sign_seam.dihedral_angle(&fold_up) - 40.0_f64.to_radians()).abs() < 1e-12,
            "got {}",
            same_sign_seam.dihedral_angle(&fold_up)
        );
        check(
            &fold_up,
            &[0, 1, 2, 3],
            1e-7,
            |p| same_sign_seam.dihedral_energy(p, b),
            |p| same_sign_seam.dihedral_local(p, b),
        );

        // Hairpin fold torque check: Discrete Shells torque grows monotonically from 90° to 170°,
        // whereas legacy quadratic Hinge torque collapses toward zero as sin(θ) -> 0.
        let h_sym = Hinge::new([0, 1, 2, 3], sym_rest).unwrap();
        let torque_at = |deg: f64, model: BendingModel| -> f64 {
            let rad = deg.to_radians();
            let (s, c) = ftx_core::det::sin_cos(rad);
            let x = [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.01, 0.0, 0.0),
                Vec3::new(0.005, 0.01, 0.0),
                Vec3::new(0.005, -0.01 * c, 0.01 * s),
            ];
            let l = h_sym.local_with_model(&x, b, model);
            // Tangent velocity of vertex 3 per unit dθ is (0, 0.01 * sin θ, 0.01 * cos θ).
            l.grad[10] * (0.01 * s) + l.grad[11] * (0.01 * c)
        };
        let t_ds_90 = torque_at(90.0, BendingModel::DiscreteShells);
        let t_ds_170 = torque_at(170.0, BendingModel::DiscreteShells);
        assert!(t_ds_170 > 1.8 * t_ds_90, "ds 170°={t_ds_170} 90°={t_ds_90}");
        let t_q_90 = torque_at(90.0, BendingModel::Quadratic);
        let t_q_170 = torque_at(170.0, BendingModel::Quadratic);
        assert!(t_q_170 < 0.2 * t_q_90, "quad 170°={t_q_170} 90°={t_q_90}");
    }

    #[test]
    fn principal_strain_softening_derivatives_and_45deg_bias_isotropy() {
        let mat = Membrane::orthotropic(800.0, 400.0, 0.2, 60.0, 0.04);
        let rest = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.012, 0.001),
            Vec2::new(0.003, 0.010),
        ];
        let e = MembraneElement::new([0, 1, 2], rest).unwrap();
        let cases: &[([f64; 3], bool, f64)] = &[
            // Uniaxial compression along warp (λ₁ near 0 blend => fine FD step)
            ([0.92, 1.0, 0.0], true, 1e-7),
            // 45° bias compression + shear
            ([0.95, 0.95, -0.06], true, 1e-6),
            // Pure shear (tension + compression on 45° diagonals)
            ([1.0, 1.0, 0.08], true, 1e-6),
            // Deep isotropic compression
            ([0.94, 0.94, 0.0], false, 1e-7),
            // Exact repeated eigenvalue inside the nonlinear tanh blend (`s == 0`, `m ≈ -8e-4`)
            ([0.9992, 0.9992, 0.0], false, 1e-8),
            // Near-isotropic inside Taylor patch (`s < 5e-3 * delta`) inside the nonlinear blend
            ([0.9992, 0.9992 + 1e-6, 1e-6], false, 1e-8),
            // Near-isotropic just outside Taylor patch (`s > 5e-3 * delta`) inside the nonlinear blend
            ([0.9992, 0.9992 + 2e-5, 0.0], false, 1e-8),
            // Tension everywhere
            ([1.08, 1.04, 0.03], true, 1e-6),
        ];
        for &(def, tilt_z, h) in cases {
            let [sx, sy, sh] = def;
            let x: Vec<Vec3> = rest
                .iter()
                .map(|p| {
                    let z = if tilt_z { 0.05 * p.x - 0.03 * p.y } else { 0.0 };
                    Vec3::new(sx * p.x + sh * p.y, sy * p.y, z)
                })
                .collect();
            check(
                &x,
                &[0, 1, 2],
                h,
                |p| e.energy_with_model(p, &mat, CompressionModel::Principal),
                |p| e.local_with_model(p, &mat, CompressionModel::Principal),
            );
        }

        // Exact rotational isotropy check: for an isotropic membrane (E, nu, G = E/(2(1+nu))),
        // 10% uniaxial compression along 0° and along 45° must have identical energy under Principal,
        // whereas AxisAligned locks along 45° bias.
        let nu = 0.25;
        let ym = 600.0;
        let iso = Membrane::orthotropic(ym, ym, nu, ym / (2.0 * (1.0 + nu)), 0.02);
        let sq = MembraneElement::new(
            [0, 1, 2],
            [Vec2::ZERO, Vec2::new(0.01, 0.0), Vec2::new(0.0, 0.01)],
        )
        .unwrap();
        let lam = 0.9;
        let x_0: Vec<Vec3> = [Vec2::ZERO, Vec2::new(0.01, 0.0), Vec2::new(0.0, 0.01)]
            .iter()
            .map(|p| Vec3::new(lam * p.x, p.y, 0.0))
            .collect();
        // Compress along 45° unit vector u = (1,1)/√2 by `lam`, unchanged along v = (-1,1)/√2.
        let c45 = 0.5 * (lam + 1.0);
        let s45 = 0.5 * (lam - 1.0);
        let x_45: Vec<Vec3> = [Vec2::ZERO, Vec2::new(0.01, 0.0), Vec2::new(0.0, 0.01)]
            .iter()
            .map(|p| Vec3::new(c45 * p.x + s45 * p.y, s45 * p.x + c45 * p.y, 0.0))
            .collect();
        let ep_0 = sq.energy_with_model(&x_0, &iso, CompressionModel::Principal);
        let ep_45 = sq.energy_with_model(&x_45, &iso, CompressionModel::Principal);
        assert!(
            (ep_0 - ep_45).abs() < 1e-13 * ep_0.max(1e-12),
            "principal bias isotropy failed: 0°={ep_0} 45°={ep_45}"
        );
        let ea_0 = sq.energy_with_model(&x_0, &iso, CompressionModel::AxisAligned);
        let ea_45 = sq.energy_with_model(&x_45, &iso, CompressionModel::AxisAligned);
        assert!(
            ea_45 > 5.0 * ea_0,
            "expected axis-aligned bias locking contrast: 0°={ea_0} 45°={ea_45}"
        );
    }
}
