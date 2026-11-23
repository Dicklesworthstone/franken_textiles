//! Objective curvature increments for the source-selected panel law. The source tensor and
//! reference integration area are unchanged; the optional Rayleigh time is a separate numerical
//! constitutive assumption. Vector-Hessian rates use a frame carried by the source triangle,
//! never a difference between world-space curvature vectors on a rotating garment.

use ftx_core::{Refusal, RefusalCode, Vec3};

use super::BendingPatch;
use crate::ad::Jet;
use crate::elements::Local;

/// Accepted geometric curvature, independent of preferred shape or permanent-curvature memory.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PanelRateState {
    Normal([f64; 3]),
    Corotated([[f64; 3]; 3]),
}

fn invalid() -> Local<18, 324> {
    Local {
        energy: f64::NAN,
        grad: [f64::NAN; 18],
        hess: [f64::NAN; 324],
        psd: false,
    }
}

impl BendingPatch {
    fn rate_frame(&self, x: &[Vec3]) -> Result<[Vec3; 3], Refusal> {
        let bad = || {
            Refusal::new(
                RefusalCode::DegenerateGeometry,
                "panel damping requires a finite nondegenerate source-triangle frame",
            )
        };
        if self.v.iter().any(|&v| v >= x.len() || !x[v].is_finite()) {
            return Err(bad());
        }
        let edge = x[self.v[1]] - x[self.v[0]];
        let arm = x[self.v[2]] - x[self.v[0]];
        let cross = edge.cross(arm);
        let (edge2, cross2) = (edge.norm2(), cross.norm2());
        let product = edge2 * arm.norm2();
        if !product.is_finite()
            || product <= 0.0
            || !cross2.is_finite()
            || cross2 <= 1e-16 * product
        {
            return Err(bad());
        }
        let tangent = edge / edge2.sqrt();
        let normal = cross / cross2.sqrt();
        Ok([tangent, normal.cross(tangent), normal])
    }

    /// Freeze only accepted curvature, using the same kinematics and tensor as the source panel.
    pub(crate) fn rate_state(&self, x: &[Vec3]) -> Result<PanelRateState, Refusal> {
        if self.tensor.uses_normal() {
            let value = self.normal_curvature(x)?;
            if value.iter().all(|v| v.is_finite()) {
                return Ok(PanelRateState::Normal(value));
            }
        } else {
            let frame = self.rate_frame(x)?;
            let curvature = self.curvature(x);
            let value =
                std::array::from_fn(|k| std::array::from_fn(|axis| curvature[k].dot(frame[axis])));
            if value.iter().flatten().all(|v| v.is_finite()) {
                return Ok(PanelRateState::Corotated(value));
            }
        }
        Err(Refusal::new(
            RefusalCode::NonFinite,
            "panel damping curvature is not finite",
        ))
    }

    /// Backward-difference curvature potential `A beta/(2h) Δkᵀ D Δk`; vector patches sum
    /// the three corotating spatial components. Invalid trial geometry produces a failed potential.
    pub(crate) fn rate_energy(&self, x: &[Vec3], previous: PanelRateState, rate: f64) -> f64 {
        if !rate.is_finite() || rate < 0.0 {
            return f64::NAN;
        }
        let delta: [[f64; 3]; 3] = match (self.rate_state(x), previous) {
            (Ok(PanelRateState::Normal(now)), PanelRateState::Normal(old)) => {
                std::array::from_fn(|k| [now[k] - old[k], 0.0, 0.0])
            }
            (Ok(PanelRateState::Corotated(now)), PanelRateState::Corotated(old)) => {
                std::array::from_fn(|k| std::array::from_fn(|axis| now[k][axis] - old[k][axis]))
            }
            _ => return f64::NAN,
        };
        let d = self.tensor.matrix();
        let mut energy = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                for axis in 0..3 {
                    energy += d[i][j] * delta[i][axis] * delta[j][axis];
                }
            }
        }
        0.5 * self.area * rate * energy
    }

    /// Exact endpoint-position gradient and Hessian, including the rotation of the local frame.
    pub(crate) fn rate_local(
        &self,
        x: &[Vec3],
        previous: PanelRateState,
        rate: f64,
    ) -> Local<18, 324> {
        if !rate.is_finite() || rate < 0.0 {
            return invalid();
        }
        let z = Jet::constant(0.0);
        let delta: [[Jet<18>; 3]; 3] = match previous {
            PanelRateState::Normal(old) if self.tensor.uses_normal() => {
                if self.check_domain(x).is_err() {
                    return invalid();
                }
                let current = self.normal_curvature_jets(x);
                std::array::from_fn(|k| [current[k] - Jet::constant(old[k]), z, z])
            }
            PanelRateState::Corotated(old) if !self.tensor.uses_normal() => {
                if self.rate_frame(x).is_err() {
                    return invalid();
                }
                let current = self.corotated_curvature_jets(x);
                std::array::from_fn(|k| {
                    std::array::from_fn(|axis| current[k][axis] - Jet::constant(old[k][axis]))
                })
            }
            _ => return invalid(),
        };
        let d = self.tensor.matrix();
        let mut energy = z;
        for i in 0..3 {
            for j in 0..3 {
                for axis in 0..3 {
                    energy = energy + delta[i][axis] * delta[j][axis] * d[i][j];
                }
            }
        }
        let energy = energy * (0.5 * self.area * rate);
        Local {
            energy: energy.v,
            grad: energy.g,
            hess: std::array::from_fn(|ij| energy.h[ij / 18][ij % 18]),
            // A PSD material tensor does not remove the exact frame/geometric stiffness.
            psd: false,
        }
    }

    fn corotated_curvature_jets(&self, x: &[Vec3]) -> [[Jet<18>; 3]; 3] {
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
        let cross = |a: [J; 3], b: [J; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let edge_length = dot(p[1], p[1]).sqrt();
        let tangent = p[1].map(|v| v / edge_length);
        let area_vector = cross(p[1], p[2]);
        let area = dot(area_vector, area_vector).sqrt();
        let normal = area_vector.map(|v| v / area);
        let frame = [tangent, cross(normal, tangent), normal];
        std::array::from_fn(|k| {
            let curvature = std::array::from_fn(|axis| {
                (1..6).fold(z, |sum, i| sum + p[i][axis] * self.weights[k][i])
            });
            std::array::from_fn(|axis| dot(curvature, frame[axis]))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::{Mat3, Vec2};
    use ftx_pattern::material_bending::BendingTensor;

    fn rest() -> [Vec2; 6] {
        [(0., 0.), (1., 0.), (0., 1.), (1., 1.), (2., 0.), (0., 2.)].map(|(x, y)| Vec2::new(x, y))
    }

    fn patch(normal: bool, preferred: [f64; 3]) -> BendingPatch {
        let mut tensor = BendingTensor::new(2.0, 5.0, 0.4, 3.0).unwrap();
        if normal {
            tensor = tensor
                .normal_projected()
                .with_preferred_curvature(preferred)
                .unwrap();
        }
        BendingPatch::new([0, 1, 2, 3, 4, 5], rest(), tensor).unwrap()
    }

    fn surface(amplitude: f64) -> [Vec3; 6] {
        rest().map(|p| {
            Vec3::new(
                p.x + 0.02 * p.x * p.y,
                p.y + 0.01 * p.x * p.x,
                amplitude * (0.03 * p.x * p.x + 0.02 * p.y * p.y - 0.015 * p.x * p.y),
            )
        })
    }

    fn check_derivatives(patch: &BendingPatch, previous: PanelRateState, x: [Vec3; 6]) {
        let rate = 0.7;
        let local = patch.rate_local(&x, previous, rate);
        let energy = patch.rate_energy(&x, previous, rate);
        assert!((local.energy - energy).abs() < 1e-14);
        let gs = local.grad.iter().map(|g| g.abs()).fold(1e-8, f64::max);
        let hs = local.hess.iter().map(|h| h.abs()).fold(1e-8, f64::max);
        let epsilon = 1e-6;
        for i in 0..18 {
            let (mut plus, mut minus) = (x, x);
            let offset = match i % 3 {
                0 => Vec3::new(epsilon, 0.0, 0.0),
                1 => Vec3::new(0.0, epsilon, 0.0),
                _ => Vec3::new(0.0, 0.0, epsilon),
            };
            plus[i / 3] = plus[i / 3] + offset;
            minus[i / 3] = minus[i / 3] - offset;
            let numerical = (patch.rate_energy(&plus, previous, rate)
                - patch.rate_energy(&minus, previous, rate))
                / (2.0 * epsilon);
            assert!(
                (numerical - local.grad[i]).abs() < 3e-6 * gs,
                "gradient {i}: exact {} numerical {numerical}",
                local.grad[i]
            );
            let (plus, minus) = (
                patch.rate_local(&plus, previous, rate),
                patch.rate_local(&minus, previous, rate),
            );
            for j in 0..18 {
                let numerical = (plus.grad[j] - minus.grad[j]) / (2.0 * epsilon);
                assert!(
                    (numerical - local.hess[j * 18 + i]).abs() < 3e-6 * hs,
                    "Hessian {j},{i}: exact {} numerical {numerical}",
                    local.hess[j * 18 + i]
                );
                assert!((local.hess[i * 18 + j] - local.hess[j * 18 + i]).abs() < 1e-13 * hs);
            }
        }
    }

    #[test]
    fn source_panel_rate_force_and_hessian_match_energy_for_both_models() {
        for normal in [false, true] {
            let p = patch(normal, [0.2, -0.1, 0.05]);
            let prior = p.rate_state(&surface(0.8)).unwrap();
            check_derivatives(&p, prior, surface(1.7));
        }
    }

    #[test]
    fn bent_source_panel_has_no_dissipation_under_finite_rigid_motion() {
        let rotation = Mat3::rotation(Vec3::new(0.4, -0.7, 0.5).normalized(), 1.37);
        let shift = Vec3::new(2.5, -1.7, 0.8);
        let transform = |x: [Vec3; 6]| x.map(|p| rotation.mul_vec(p) + shift);
        let old = surface(1.2);
        let current = surface(1.9);
        for normal in [false, true] {
            let p = patch(normal, [0.2, -0.1, 0.05]);
            let prior = p.rate_state(&old).unwrap();
            let local = p.rate_local(&transform(old), prior, 0.7);
            assert!(
                local.energy.abs() < 1e-25,
                "normal={normal}: {}",
                local.energy
            );
            assert!(local.grad.iter().all(|g| g.abs() < 1e-12));
            let moved_prior = p.rate_state(&transform(old)).unwrap();
            let a = p.rate_energy(&current, prior, 0.7);
            let b = p.rate_energy(&transform(current), moved_prior, 0.7);
            assert!((a - b).abs() < 1e-14);
        }
    }

    #[test]
    fn flat_reference_rate_uses_the_source_tensor_and_full_triangle_area() {
        let flat = rest().map(|p| Vec3::new(p.x, p.y, 0.0));
        let current = surface(1.6);
        for normal in [false, true] {
            let p = patch(normal, [0.0; 3]);
            let prior = p.rate_state(&flat).unwrap();
            // At zero initial curvature the rate potential is beta/h times the existing panel
            // energy, including vector components, mixed curvature and the authored D12 coupling.
            assert!((p.rate_energy(&current, prior, 0.7) - 0.7 * p.energy(&current)).abs() < 1e-14);
        }
    }

    #[test]
    fn preferred_curvature_does_not_pollute_geometric_rate_coordinates() {
        let plain = patch(true, [0.0; 3]);
        let preferred = patch(true, [1e12, -2e12, 3e12]);
        let old = surface(0.8);
        let current = surface(0.80001);
        let a = plain.rate_local(&current, plain.rate_state(&old).unwrap(), 0.7);
        let b = preferred.rate_local(&current, preferred.rate_state(&old).unwrap(), 0.7);
        assert_eq!(a.energy.to_bits(), b.energy.to_bits());
        assert_eq!(a.grad.map(f64::to_bits), b.grad.map(f64::to_bits));
        assert_eq!(a.hess.map(f64::to_bits), b.hess.map(f64::to_bits));
    }

    #[test]
    fn invalid_rate_frames_refuse_instead_of_silently_disabling_damping() {
        for normal in [false, true] {
            let p = patch(normal, [0.0; 3]);
            let old = surface(0.8);
            let prior = p.rate_state(&old).unwrap();
            let mut invalid = [Vec3::ZERO; 6];
            assert!(p.rate_state(&invalid).is_err());
            assert!(p.rate_energy(&invalid, prior, 0.7).is_nan());
            assert!(p.rate_local(&invalid, prior, 0.7).energy.is_nan());
            invalid = old;
            invalid[4].x = f64::INFINITY;
            assert!(p.rate_state(&invalid).is_err());
            assert!(p.rate_energy(&old, prior, -0.1).is_nan());
            assert!(p.rate_local(&old, prior, f64::NAN).energy.is_nan());
        }
    }
}
