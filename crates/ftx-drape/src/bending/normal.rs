//! Flat-reference covariant curvature: b_ab = n dot x_,ab, with the normal of the same
//! quadratic reconstruction at the integrated triangle's centroid. Unlike vector-Hessian bending,
//! this excludes tangential second derivatives of a nonuniformly stretched planar sheet.
//! Independent implementation; continuum convention: doi:10.1016/j.cad.2021.103075, equation (5).

use super::{BendingPatch, bad, quadratic};
use crate::ad::Jet;
use crate::elements::Local;
use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};

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
    fn normal_frame(&self, x: &[Vec3]) -> Option<Vec3> {
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
        Some(cross / area2.sqrt())
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
    /// not current principal curvatures. The reference sheet is flat, so its rest curvature is zero.
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

    pub(super) fn normal_energy(&self, x: &[Vec3]) -> f64 {
        let Ok(c) = self.normal_curvature(x) else {
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
        let curv: [J; 3] = std::array::from_fn(|a| {
            let derivative = std::array::from_fn(|k| {
                (1..6).fold(z, |sum, i| sum + p[i][k] * self.weights[a][i])
            });
            dot(normal, derivative)
        });
        let d = self.tensor.matrix();
        let mut e = z;
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
}
