//! Flat-reference covariant curvature: b_ab = n dot x_,ab, with the normal of the same
//! quadratic reconstruction at the integrated triangle's centroid. Unlike vector-Hessian bending,
//! this excludes tangential second derivatives of a nonuniformly stretched planar sheet.
//! Independent implementation; continuum convention: doi:10.1016/j.cad.2021.103075, equation (5).

use super::{BendingPatch, bad, quadratic};
use crate::ad::Jet;
use crate::elements::Local;
use crate::knit::{AsymmetricKnitBending, cayley_hamilton_inv_sqrt_2x2};
use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};
use ftx_pattern::material_plasticity::Response;

pub(super) fn tangent_weights(
    base: [Vec2; 3],
    scale: f64,
    curvature: &[[f64; 6]; 3],
) -> Result<[[f64; 6]; 2], Refusal> {
    let a = base[1] - base[0];
    let b = base[2] - base[0];
    let det = a.cross(b);
    let g1 = Vec2::new(b.y, -b.x) / det;
    let g2 = Vec2::new(-a.y, a.x) / det;
    let gradients = [(g1 + g2) * -1.0, g1, g2];
    let centre = (base[0] + base[1] + base[2]) / 3.0;
    let dq = [[centre.x, 0.0], [0.0, centre.y], [centre.y, centre.x]];
    let mut out = [[0.0; 6]; 2];
    for axis in 0..2 {
        for i in 1..3 {
            out[axis][i] = if axis == 0 {
                gradients[i].x
            } else {
                gradients[i].y
            } / scale;
        }
        // Differentiate the quadratic residual after subtracting its affine base interpolation.
        for k in 0..3 {
            let affine: f64 = (0..3)
                .map(|i| {
                    let g = if axis == 0 {
                        gradients[i].x
                    } else {
                        gradients[i].y
                    };
                    g * quadratic(base[i])[k]
                })
                .sum();
            let correction = (dq[k][axis] - affine) * scale / if k == 2 { 2.0 } else { 1.0 };
            for i in 1..6 {
                out[axis][i] += correction * curvature[k][i];
            }
        }
        out[axis][0] = -out[axis][1..].iter().sum::<f64>();
    }
    if out.iter().flatten().any(|v| !v.is_finite()) {
        return Err(bad("quadratic tangent reconstruction overflows"));
    }
    Ok(out)
}

impl BendingPatch {
    fn tangent_and_normal(&self, x: &[Vec3]) -> Option<([Vec3; 2], Vec3)> {
        if self.v.iter().any(|&v| v >= x.len() || !x[v].is_finite()) {
            return None;
        }
        let t: [Vec3; 2] = std::array::from_fn(|k| {
            (1..6).fold(Vec3::ZERO, |sum, i| {
                sum + (x[self.v[i]] - x[self.v[0]]) * self.tangents[k][i]
            })
        });
        let cross = t[0].cross(t[1]);
        let area2 = cross.norm2();
        let norm_product = t[0].norm2() * t[1].norm2();
        if !area2.is_finite()
            || !norm_product.is_finite()
            || norm_product <= 0.0
            || area2 <= 1e-16 * norm_product
        {
            return None;
        }
        Some((t, cross / area2.sqrt()))
    }

    fn normal_frame(&self, x: &[Vec3]) -> Option<Vec3> {
        self.tangent_and_normal(x).map(|(_, n)| n)
    }

    /// Objective physical curvature `[kappa_11, kappa_22, 2*kappa_12]` in inverse deformed metres,
    /// obtained by pulling back the covariant second fundamental form `b` through the closed-form
    /// singularity-free Cayley–Hamilton metric inverse square root `C^{-1/2}` (`FTX-K09`).
    ///
    /// Unlike [`Self::normal_curvature`], this removes pure membrane stretch pollution so a
    /// pre-stretched cylinder retains its exact physical bending curvature (`KAT-SHELL-1`).
    ///
    /// # Errors
    /// Vector-Hessian patch, singular tangent frame, or non-SPD metric tensor `C`.
    pub fn physical_curvature(&self, x: &[Vec3]) -> Result<[f64; 3], Refusal> {
        if !self.tensor.uses_normal() {
            return Err(bad(
                "physical curvature pullback requires the explicit normal-projected model",
            ));
        }
        let (t, n) = self.tangent_and_normal(x).ok_or_else(|| {
            Refusal::new(
                RefusalCode::OutOfRange,
                "normal-projected bending has a singular/non-finite reconstructed surface frame",
            )
            .repair("refine or repair the panel deformation; no substitute normal is used")
        })?;
        let c00 = t[0].dot(t[0]);
        let c01 = t[0].dot(t[1]);
        let c11 = t[1].dot(t[1]);
        let [m00, m01, m11] = cayley_hamilton_inv_sqrt_2x2(c00, c01, c11)?;
        let raw = self.curvature(x);
        // `self.weights[2]` already includes the engineering twist factor 2, so `b_uv = 0.5 * n·(2 x_uv)`.
        let buu = n.dot(raw[0]);
        let bvv = n.dot(raw[1]);
        let buv = 0.5 * n.dot(raw[2]);
        let mb00 = m00 * buu + m01 * buv;
        let mb01 = m00 * buv + m01 * bvv;
        let mb10 = m01 * buu + m11 * buv;
        let mb11 = m01 * buv + m11 * bvv;
        let k11 = mb00 * m00 + mb01 * m01;
        let k22 = mb10 * m01 + mb11 * m11;
        let k12_tensor = mb00 * m01 + mb01 * m11;
        let out = [k11, k22, 2.0 * k12_tensor];
        if out.iter().any(|v| !v.is_finite()) {
            return Err(bad("physical curvature pullback overflows"));
        }
        Ok(out)
    }

    /// Reference-area asymmetric knit bending energy (J) evaluated on objective physical curvature
    /// `kappa_phys = C^{-1/2} b C^{-1/2}` (`FTX-K09`).
    ///
    /// # Errors
    /// Invalid normal/metric domain or non-finite energy.
    pub fn knit_bending_energy(
        &self,
        x: &[Vec3],
        law: &AsymmetricKnitBending,
    ) -> Result<f64, Refusal> {
        let kp = self.physical_curvature(x)?;
        let energy = self.area * law.evaluate_from_kappa(kp).energy_density;
        if !energy.is_finite() {
            return Err(bad("asymmetric knit bending energy overflows"));
        }
        Ok(energy)
    }

    /// Exact 18-DOF gradient and symmetric Hessian of [`Self::knit_bending_energy`] using
    /// singularity-free Cayley–Hamilton `C^{-1/2}` pullback inside `Jet<18>` (`FTX-K09`).
    ///
    /// # Errors
    /// Invalid normal/metric domain or non-finite composed derivatives.
    pub fn knit_bending_local(
        &self,
        x: &[Vec3],
        law: &AsymmetricKnitBending,
    ) -> Result<Local<18, 324>, Refusal> {
        let k = self.physical_curvature_jets(x)?;
        let eval = law.evaluate_from_kappa(k.map(|c| c.v));
        let grad = std::array::from_fn(|i| {
            self.area * (0..3).map(|a| eval.moment[a] * k[a].g[i]).sum::<f64>()
        });
        let mut hess = [0.0; 324];
        for i in 0..18 {
            for j in 0..=i {
                let mut h = 0.0;
                for a in 0..3 {
                    h += eval.moment[a] * k[a].h[i][j];
                    for b in 0..3 {
                        h += eval.tangent[a][b] * k[a].g[i] * k[b].g[j];
                    }
                }
                let val = self.area * h;
                hess[i * 18 + j] = val;
                hess[j * 18 + i] = val;
            }
        }
        let energy = self.area * eval.energy_density;
        if !energy.is_finite() || grad.iter().chain(&hess).any(|v| !v.is_finite()) {
            return Err(bad("composed asymmetric knit bending derivatives overflow"));
        }
        Ok(Local {
            energy,
            grad,
            hess,
            psd: false,
        })
    }

    fn physical_curvature_jets(&self, x: &[Vec3]) -> Result<[Jet<18>; 3], Refusal> {
        if !self.tensor.uses_normal() {
            return Err(bad(
                "physical curvature pullback requires the explicit normal-projected model",
            ));
        }
        self.check_domain(x)?;
        type J = Jet<18>;
        let z = J::constant(0.0);
        let p: [[J; 3]; 6] = std::array::from_fn(|i| {
            std::array::from_fn(|k| {
                if i == 0 {
                    z
                } else {
                    J::var(x[self.v[i]][k] - x[self.v[0]][k], 3 * i + k) - J::var(0.0, k)
                }
            })
        });
        let dot = |a: [J; 3], b: [J; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let t: [[J; 3]; 2] = std::array::from_fn(|a| {
            std::array::from_fn(|k| (1..6).fold(z, |sum, i| sum + p[i][k] * self.tangents[a][i]))
        });
        let c00 = dot(t[0], t[0]);
        let c01 = dot(t[0], t[1]);
        let c11 = dot(t[1], t[1]);
        let tr = c00 + c11;
        let det = c00 * c11 - c01 * c01;
        if !tr.v.is_finite() || !det.v.is_finite() || tr.v <= 0.0 || det.v <= 1e-18 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "non-SPD metric tensor in knit bending patch (tr={}, det={})",
                    tr.v, det.v
                ),
            ));
        }
        // Singularity-free Cayley–Hamilton C^{-1/2} in Jet<18> (no eigendecomposition atan2(0,0)!).
        let s = det.sqrt();
        let q = (tr + s * 2.0).sqrt();
        let inv_sq = J::constant(1.0) / (s * q);
        let m00 = (c11 + s) * inv_sq;
        let m01 = (-c01) * inv_sq;
        let m11 = (c00 + s) * inv_sq;

        let cross = [
            t[0][1] * t[1][2] - t[0][2] * t[1][1],
            t[0][2] * t[1][0] - t[0][0] * t[1][2],
            t[0][0] * t[1][1] - t[0][1] * t[1][0],
        ];
        let length = dot(cross, cross).sqrt();
        let normal = cross.map(|v| v / length);
        let b_voigt: [J; 3] = std::array::from_fn(|a| {
            let derivative = std::array::from_fn(|k| {
                (1..6).fold(z, |sum, i| sum + p[i][k] * self.weights[a][i])
            });
            dot(normal, derivative)
        });
        let buu = b_voigt[0];
        let bvv = b_voigt[1];
        let buv = b_voigt[2] * 0.5;
        let mb00 = m00 * buu + m01 * buv;
        let mb01 = m00 * buv + m01 * bvv;
        let mb10 = m01 * buu + m11 * buv;
        let mb11 = m01 * buv + m11 * bvv;
        let k11 = mb00 * m00 + mb01 * m01;
        let k22 = mb10 * m01 + mb11 * m11;
        let k12_tensor = mb00 * m01 + mb01 * m11;
        Ok([k11, k22, k12_tensor * 2.0])
    }

    /// Check the deformed quadratic tangent frame. A numerical fold or collapse cannot silently
    /// choose a default normal. Legacy vector-Hessian patches retain their original domain.
    ///
    /// # Errors
    /// A normal-projected patch has invalid positions or a singular/near-parallel tangent frame.
    pub fn check_domain(&self, x: &[Vec3]) -> Result<(), Refusal> {
        if self.tensor.uses_normal() && self.normal_frame(x).is_none() {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "normal-projected bending has a singular/non-finite reconstructed surface frame",
            )
            .repair("refine or repair the panel deformation; no substitute normal is used"));
        }
        Ok(())
    }

    /// Covariant [b_uu,b_vv,2b_uv] in inverse metres of the reference material coordinates,
    /// not current principal curvatures. This returns the geometric curvature, NOT the departure
    /// from any preferred material curvature.
    ///
    /// # Errors
    /// This is a vector-Hessian patch, or its deformed reconstruction has no reliable normal.
    pub fn normal_curvature(&self, x: &[Vec3]) -> Result<[f64; 3], Refusal> {
        if !self.tensor.uses_normal() {
            return Err(bad(
                "normal curvature requires the explicit normal-projected model",
            ));
        }
        self.check_domain(x)?;
        let n = self.normal_frame(x).expect("checked frame");
        Ok(self.curvature(x).map(|c| n.dot(c)))
    }

    /// Signed preferred covariant curvature [b_uu,b_vv,2b_uv], inverse reference metres.
    #[must_use]
    pub fn preferred_curvature(&self) -> [f64; 3] {
        self.tensor.preferred_curvature()
    }

    /// Difference between the actual and preferred covariant curvature in material coordinates.
    ///
    /// # Errors
    /// As [`Self::normal_curvature`], or a non-finite curvature difference.
    pub fn bending_strain(&self, x: &[Vec3]) -> Result<[f64; 3], Refusal> {
        let actual = self.normal_curvature(x)?;
        let preferred = self.preferred_curvature();
        let strain = std::array::from_fn(|i| actual[i] - preferred[i]);
        if strain.iter().any(|v| !v.is_finite()) {
            return Err(bad("preferred-curvature subtraction overflows"));
        }
        Ok(strain)
    }

    pub(super) fn normal_energy(&self, x: &[Vec3]) -> f64 {
        let Ok(c) = self.bending_strain(x) else {
            return f64::INFINITY;
        };
        let d = self.tensor.matrix();
        let mut e = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                e += d[i][j] * c[i] * c[j];
            }
        }
        0.5 * self.area * e
    }

    pub(super) fn normal_local(&self, x: &[Vec3]) -> Local<18, 324> {
        if self.check_domain(x).is_err() {
            // Solver::potential gates this before assembling; a direct invalid call is not zero energy.
            return Local {
                energy: f64::INFINITY,
                grad: [f64::NAN; 18],
                hess: [f64::NAN; 324],
                psd: false,
            };
        }
        let curv = self.normal_strain_jets(x);
        let d = self.tensor.matrix();
        let mut e = Jet::constant(0.0);
        for i in 0..3 {
            for j in 0..3 {
                e = e + curv[i] * curv[j] * d[i][j];
            }
        }
        let e = e * (0.5 * self.area);
        Local {
            energy: e.v,
            grad: e.g,
            hess: std::array::from_fn(|i| e.h[i / 18][i % 18]),
            psd: false,
        }
    }

    /// Compose a material potential/moment/tangent with the exact current-normal curvature jets.
    /// This replaces the elastic panel law, never adds it twice. The callback sees departure from
    /// the authored preferred shape; all geometric-stiffness terms and engineering twist remain.
    ///
    /// # Errors
    /// Invalid normal kinematics/frame, a material refusal or non-finite composed derivatives.
    pub fn normal_local_with_response(
        &self,
        x: &[Vec3],
        response: impl FnOnce([f64; 3]) -> Result<Response, Refusal>,
    ) -> Result<Local<18, 324>, Refusal> {
        if !self.tensor.uses_normal() {
            return Err(bad("curvature response requires normal-projected bending"));
        }
        self.check_domain(x)?;
        let k = self.normal_strain_jets(x);
        let r = response(k.map(|c| c.v))?;
        let grad = std::array::from_fn(|i| {
            self.area * (0..3).map(|a| r.moment_n[a] * k[a].g[i]).sum::<f64>()
        });
        let mut hess = [0.; 324];
        for i in 0..18 {
            for j in 0..=i {
                let mut h = 0.;
                for a in 0..3 {
                    h += r.moment_n[a] * k[a].h[i][j];
                    for b in 0..3 {
                        h += r.tangent_nm[a][b] * k[a].g[i] * k[b].g[j];
                    }
                }
                hess[i * 18 + j] = self.area * h;
                hess[j * 18 + i] = hess[i * 18 + j];
            }
        }
        let energy = self.area * r.energy;
        if !energy.is_finite() || grad.iter().chain(&hess).any(|v| !v.is_finite()) {
            return Err(bad("composed curvature response overflows"));
        }
        Ok(Local {
            energy,
            grad,
            hess,
            psd: false,
        })
    }

    fn normal_strain_jets(&self, x: &[Vec3]) -> [Jet<18>; 3] {
        let actual = self.normal_curvature_jets(x);
        std::array::from_fn(|a| actual[a] - Jet::constant(self.preferred_curvature()[a]))
    }

    // Raw curvature is also the objective rate coordinate. Subtracting a large preferred shape
    // before taking its change would discard small, physically real curvature increments.
    pub(super) fn normal_curvature_jets(&self, x: &[Vec3]) -> [Jet<18>; 3] {
        type J = Jet<18>;
        let z = J::constant(0.0);
        let p: [[J; 3]; 6] = std::array::from_fn(|i| {
            std::array::from_fn(|k| {
                if i == 0 {
                    z
                } else {
                    J::var(x[self.v[i]][k] - x[self.v[0]][k], 3 * i + k) - J::var(0.0, k)
                }
            })
        });
        let dot = |a: [J; 3], b: [J; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let t: [[J; 3]; 2] = std::array::from_fn(|a| {
            std::array::from_fn(|k| (1..6).fold(z, |sum, i| sum + p[i][k] * self.tangents[a][i]))
        });
        let cross = [
            t[0][1] * t[1][2] - t[0][2] * t[1][1],
            t[0][2] * t[1][0] - t[0][0] * t[1][2],
            t[0][0] * t[1][1] - t[0][1] * t[1][0],
        ];
        let length = dot(cross, cross).sqrt();
        let normal = cross.map(|v| v / length);
        std::array::from_fn(|a| {
            let derivative = std::array::from_fn(|k| {
                (1..6).fold(z, |sum, i| sum + p[i][k] * self.weights[a][i])
            });
            dot(normal, derivative)
        })
    }
}
