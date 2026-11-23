//! One spectral compression transform for energy, derivatives and accepted material memory.
//! Material stiffness remains in its authored grain frame. This regularization is not a measured
//! wrinkle law or a fully relaxed anisotropic tension-field constitutive model.

use super::{Local, Membrane, MembraneElement, Response, principal_ratio};
use crate::ad::Jet;
use ftx_core::{Vec3, det};

type Matrix = [[f64; 2]; 2];
const BASIS: [Matrix; 3] = [
    [[1.0, 0.0], [0.0, 0.0]],
    [[0.0, 0.0], [0.0, 1.0]],
    [[0.0, 0.5], [0.5, 0.0]],
];

fn product(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j]))
}

fn voigt(a: Matrix) -> [f64; 3] {
    [a[0][0], a[1][1], a[0][1] + a[1][0]]
}

fn scalar(e: f64, r: f64, delta: f64) -> [f64; 3] {
    if e >= 0.0 || r == 1.0 {
        return [e, 1.0, 0.0];
    }
    let t = det::tanh(e / delta);
    let sech2 = 1.0 - t * t;
    [
        r * e + (1.0 - r) * delta * t,
        r + (1.0 - r) * sech2,
        -(1.0 - r) * 2.0 * sech2 * t / delta,
    ]
}

// First divided difference and the two mixed second divided differences. Close eigenvalues must
// not subtract almost equal scalar values/derivatives. Their exact integral representations are
// b=∫φ', c_lo=∫(1-t)φ'', c_hi=∫tφ''. Eight-point Gauss integration is used only over a negative
// interval shorter than 0.05 δ, where its truncation is below floating-point resolution. Splitting
// at zero respects the C² law's distinct tensile branch. Repeated roots use their exact limits.
fn divided(mean: f64, radius: f64, r: f64, delta: f64) -> [f64; 3] {
    let lo = mean - radius;
    let hi = mean + radius;
    let gap = 2.0 * radius;
    if gap <= 0.05 * delta || (hi >= 0.0 && -lo <= 0.05 * delta) {
        const NODES: [f64; 4] = [
            0.183_434_642_495_649_8,
            0.525_532_409_916_329,
            0.796_666_477_413_626_7,
            0.960_289_856_497_536_3,
        ];
        const WEIGHTS: [f64; 4] = [
            0.362_683_783_378_362,
            0.313_706_645_877_887_3,
            0.222_381_034_453_374_5,
            0.101_228_536_290_376_3,
        ];
        let end = if hi > 0.0 { -lo / gap } else { 1.0 };
        let half = 0.5 * end;
        let mut result = [0.0; 3];
        for (node, weight) in NODES.into_iter().zip(WEIGHTS) {
            for sign in [-1.0, 1.0] {
                let t = half * (1.0 + sign * node);
                let strain = lo + t * gap;
                let tanh = det::tanh(strain / delta);
                let sech2 = 1.0 - tanh * tanh;
                let second = -(1.0 - r) * 2.0 * sech2 * tanh / delta;
                let w = half * weight;
                result[0] += w * (1.0 - r) * sech2;
                result[1] += w * (1.0 - t) * second;
                result[2] += w * t * second;
            }
        }
        // Keep a tiny admitted r: 1+integral(φ'-1) would lose it in saturated compression.
        result[0] += (1.0 - end) + end * r;
        return result;
    }
    let low_tanh = det::tanh(lo / delta);
    if hi < 0.0 {
        // Remove the common linear part before forming second differences.
        let high_tanh = det::tanh(hi / delta);
        let slope = delta * (high_tanh - low_tanh) / gap;
        [
            r + (1.0 - r) * slope,
            (1.0 - r) * (slope - (1.0 - low_tanh * low_tanh)) / gap,
            (1.0 - r) * ((1.0 - high_tanh * high_tanh) - slope) / gap,
        ]
    } else {
        // Keep the positive pieces of the secant slope separate: 1-correction can round to
        // zero for a tiny r and a saturated negative eigenvalue meeting the tensile branch.
        let tensile = hi / gap - (delta / gap) * low_tanh;
        let correction = (1.0 - r) * ((delta / gap) * low_tanh - lo / gap);
        [
            r + (1.0 - r) * tensile,
            (1.0 - r) * (tensile - (1.0 - low_tanh * low_tanh)) / gap,
            correction / gap,
        ]
    }
}

struct Spectral {
    value: [f64; 3],
    projectors: [Matrix; 2],
    first: [[f64; 2]; 2],
    // A second divided difference depends only on the count of upper eigenvalues (0..=3).
    second: [f64; 4],
    identity: bool,
}

fn spectral(e: [f64; 3], r: f64, delta: f64) -> Option<Spectral> {
    if e.iter().any(|v| !v.is_finite())
        || !(r > 0.0 && r <= 1.0)
        || !delta.is_finite()
        || delta <= 0.0
    {
        return None;
    }
    let mean = 0.5 * e[0] + 0.5 * e[1];
    let difference = 0.5 * e[0] - 0.5 * e[1];
    let shear = 0.5 * e[2];
    let radius = det::hypot(difference, shear);
    let lo = mean - radius;
    let hi = mean + radius;
    if !lo.is_finite() || !hi.is_finite() || !(2.0 * radius).is_finite() {
        return None;
    }
    let lower = scalar(lo, r, delta);
    let upper = scalar(hi, r, delta);
    let identity = r == 1.0 || lo >= 0.0;
    if radius == 0.0 || identity {
        return Some(Spectral {
            value: if identity {
                e
            } else {
                [lower[0], lower[0], 0.0]
            },
            projectors: [BASIS[0], BASIS[1]],
            first: [[lower[1]; 2]; 2],
            second: [0.5 * lower[2]; 4],
            identity,
        });
    }
    let [slope, mixed_low, mixed_high] = divided(mean, radius, r, delta);
    let a = 0.5 * lower[0] + 0.5 * upper[0];
    let u = difference / radius;
    let v = shear / radius;
    let p = [[0.5 * (1.0 + u), 0.5 * v], [0.5 * v, 0.5 * (1.0 - u)]];
    Some(Spectral {
        value: [a + slope * difference, a - slope * difference, slope * e[2]],
        projectors: [[[1.0 - p[0][0], -p[0][1]], [-p[1][0], 1.0 - p[1][1]]], p],
        first: [[lower[1], slope], [slope, upper[1]]],
        second: [0.5 * lower[2], mixed_low, mixed_high, 0.5 * upper[2]],
        identity: false,
    })
}

pub(super) fn value(e: [f64; 3], r: f64, delta: f64) -> [f64; 3] {
    spectral(e, r, delta).map_or([f64::NAN; 3], |s| s.value)
}

// Fréchet derivatives of a spectral matrix function use divided differences and projectors.
// At a repeated eigenvalue Dφ=φ'H and D²φ=φ''(HK+KH)/2, with no eigenvector differentiation.
// Engineering shear is converted at both boundaries.
pub(super) fn principal(e: [f64; 3], r: f64, delta: f64) -> [Jet<3>; 3] {
    let Some(s) = spectral(e, r, delta) else {
        return [Jet {
            v: f64::NAN,
            g: [f64::NAN; 3],
            h: [[f64::NAN; 3]; 3],
        }; 3];
    };
    if s.identity {
        return std::array::from_fn(|i| Jet::var(e[i], i));
    }
    let mut out = s.value.map(Jet::constant);
    let blocks: [[[Matrix; 2]; 2]; 3] = std::array::from_fn(|j| {
        std::array::from_fn(|a| {
            std::array::from_fn(|b| product(product(s.projectors[a], BASIS[j]), s.projectors[b]))
        })
    });
    for j in 0..3 {
        for a in 0..2 {
            for b in 0..2 {
                let derivative = voigt(blocks[j][a][b]);
                for i in 0..3 {
                    out[i].g[j] += s.first[a][b] * derivative[i];
                }
            }
        }
        for k in 0..=j {
            for a in 0..2 {
                for b in 0..2 {
                    for c in 0..2 {
                        let left = product(product(blocks[j][a][b], BASIS[k]), s.projectors[c]);
                        let right = product(product(blocks[k][a][b], BASIS[j]), s.projectors[c]);
                        let derivative = voigt(std::array::from_fn(|row| {
                            std::array::from_fn(|col| left[row][col] + right[row][col])
                        }));
                        for i in 0..3 {
                            out[i].h[j][k] += s.second[a + b + c] * derivative[i];
                        }
                    }
                }
            }
            for item in &mut out {
                item.h[k][j] = item.h[j][k];
            }
        }
    }
    out
}

pub(super) fn local(
    element: &MembraneElement,
    x: &[Vec3],
    m: &Membrane,
    evaluate: impl FnOnce([f64; 3]) -> Option<Response>,
) -> Local<9, 81> {
    let transform = principal(element.strain(x), principal_ratio(m), m.transition);
    let Some(response) = evaluate(transform.map(|v| v.v)) else {
        return Local {
            energy: f64::NAN,
            grad: [f64::NAN; 9],
            hess: [f64::NAN; 81],
            psd: false,
        };
    };
    // Pull back stress/tangent to raw physical strain, including every mixed transform curvature.
    let stress: [f64; 3] =
        std::array::from_fn(|a| (0..3).map(|i| response.stress[i] * transform[i].g[a]).sum());
    let tangent: [[f64; 3]; 3] = std::array::from_fn(|a| {
        std::array::from_fn(|b| {
            let mut value = 0.0;
            for i in 0..3 {
                value += response.stress[i] * transform[i].h[a][b];
                for j in 0..3 {
                    value += transform[i].g[a] * response.tangent[i][j] * transform[j].g[b];
                }
            }
            value
        })
    });
    let (f1, f2) = element.frame(x);
    let mut b = [[0.0; 9]; 3];
    for a in 0..3 {
        for k in 0..3 {
            b[0][3 * a + k] = element.g[a][0] * f1[k];
            b[1][3 * a + k] = element.g[a][1] * f2[k];
            b[2][3 * a + k] = element.g[a][0] * f2[k] + element.g[a][1] * f1[k];
        }
    }
    let grad = std::array::from_fn(|i| {
        element.area * (stress[0] * b[0][i] + stress[1] * b[1][i] + stress[2] * b[2][i])
    });
    let tb: [[f64; 9]; 3] = std::array::from_fn(|a| {
        std::array::from_fn(|i| {
            tangent[a][0] * b[0][i] + tangent[a][1] * b[1][i] + tangent[a][2] * b[2][i]
        })
    });
    let mut hess = std::array::from_fn(|ij| {
        let (i, j) = (ij / 9, ij % 9);
        element.area * (b[0][i] * tb[0][j] + b[1][i] * tb[1][j] + b[2][i] * tb[2][j])
    });
    for a in 0..3 {
        for c in 0..3 {
            let (ga, gc) = (element.g[a], element.g[c]);
            let geometric = element.area
                * (stress[0] * ga[0] * gc[0]
                    + stress[1] * ga[1] * gc[1]
                    + stress[2] * (ga[0] * gc[1] + ga[1] * gc[0]));
            for k in 0..3 {
                hess[(3 * a + k) * 9 + 3 * c + k] += geometric;
            }
        }
    }
    Local {
        energy: element.area * response.energy,
        grad,
        hess,
        psd: false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::CompressionModel;
    use super::*;
    use ftx_core::Vec2;

    fn near(a: f64, b: f64, tolerance: f64) {
        assert!(
            (a - b).abs() <= tolerance,
            "{a:e} != {b:e} (tolerance {tolerance:e})"
        );
    }

    #[test]
    fn repeated_and_nearly_repeated_eigenvalues_have_finite_continuous_derivatives() {
        let (r, delta) = (0.2, 1e-3);
        assert_eq!(
            principal([0.0; 3], r, delta),
            std::array::from_fn(|i| Jet::var(0.0, i))
        );
        for value in [-0.0002, 0.0002] {
            let actual = principal([value, value, 0.0], r, delta);
            let [phi, first, second] = scalar(value, r, delta);
            assert_eq!(actual.map(|v| v.v), [phi, phi, 0.0]);
            near(actual[0].g[0], first, 1e-15);
            near(actual[1].g[1], first, 1e-15);
            near(actual[2].g[2], first, 1e-15);
            near(actual[0].h[0][0], second, 1e-12);
            near(actual[0].h[2][2], 0.25 * second, 1e-12);
            near(actual[2].h[0][2], 0.5 * second, 1e-12);
            for radius in [1e-8, 1e-12, 1e-16, 1e-24] {
                let nearby = principal([value + radius, value - radius, radius], r, delta);
                for i in 0..3 {
                    assert!(
                        nearby[i]
                            .g
                            .iter()
                            .chain(nearby[i].h.iter().flatten())
                            .all(|v| v.is_finite())
                    );
                    for j in 0..3 {
                        near(nearby[i].g[j], actual[i].g[j], 2e3 * radius + 2e-15);
                        for k in 0..3 {
                            near(nearby[i].h[j][k], actual[i].h[j][k], 2e7 * radius + 2e-12);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tiny_compression_factors_do_not_underflow_or_cancel_in_repeated_roots() {
        let mut m = Membrane::orthotropic(800.0, 400.0, 0.2, 60.0, 1e-200);
        assert_eq!(principal_ratio(&m), 1e-100);
        for strain in [[-0.03, -0.03, 0.0], [-0.03, -0.03 + 1e-8, 2e-8]] {
            let mapped = principal(strain, principal_ratio(&m), m.transition);
            for (i, component) in mapped.iter().enumerate() {
                for j in 0..3 {
                    near(
                        component.g[j] / 1e-100,
                        if i == j { 1.0 } else { 0.0 },
                        2e-15,
                    );
                    assert_eq!(component.h[j], [0.0; 3]);
                }
            }
        }
        m.compression = [1e-200, 1e-240];
        near(principal_ratio(&m) / 1e-110, 1.0, 2e-15);
        assert!(principal_ratio(&m) > 0.0);
        let [slope, low, high] = divided(-0.015, 0.015, 1e-100, 1e-200);
        near(slope / 1e-100, 1.0, 2e-15);
        near(low / (1e-200 / (0.03 * 0.03)), 1.0, 2e-15);
        near(high * 0.03, 1.0, 2e-15);
    }

    #[test]
    fn principal_nodal_derivatives_match_fd_at_repeated_and_mixed_roots() {
        let m = Membrane::orthotropic(800.0, 400.0, 0.2, 60.0, 0.04);
        let element = MembraneElement::new(
            [0, 1, 2],
            [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
        )
        .unwrap();
        for e in [
            [-0.03, -0.03, 0.0],
            [-0.0002, -0.0002, 0.0],
            [-0.0002, -0.0002 + 1e-14, -2e-14],
            [-0.0002, -0.0002 + 1e-8, 3e-8],
            [-0.003, 0.002, 0.0004],
            [-0.00002, 0.001, 0.00001],
            [0.0, 0.0, 0.04],
        ] {
            // Cholesky factor of C=I+2E independently realizes the requested Green strain.
            let a = (1.0_f64 + 2.0 * e[0]).sqrt();
            let b = e[2] / a;
            let c = (1.0 + 2.0 * e[1] - b * b).sqrt();
            let x = [Vec3::ZERO, Vec3::new(a, 0.0, 0.0), Vec3::new(b, c, 0.0)];
            super::super::tests::check(
                &x,
                &[0, 1, 2],
                2e-8,
                |x| element.energy_with_model(x, &m, CompressionModel::Principal),
                |x| element.local_with_model(x, &m, CompressionModel::Principal),
            );
        }
    }
}
