//! Shared deterministic box-yield return map for three work-conjugate material coordinates.
//! Callers own units, physical domains, source qualification and accepted-state transactions.
//! Arithmetic and active-set order are retained from the permanent-curvature evaluator.

use crate::material_response::Response;

pub(crate) struct Parameters {
    pub(crate) tensor: [[f64; 3]; 3],
    pub(crate) yield_value: [f64; 3],
    pub(crate) hardening: [f64; 3],
    pub(crate) accumulation_limit: [f64; 3],
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Failure {
    TrialOverflow,
    SingularActiveTensor,
    NoReturnMap,
    Accumulation,
    ResponseOverflow,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn product(a: [[f64; 3]; 3], x: [f64; 3]) -> [f64; 3] {
    a.map(|row| dot(row, x))
}

// Scaled Cholesky of an active principal block of D+H. Inactive coordinates use identity.
// No diagonal stabilization: ill-conditioned/unrepresentable input is refused, not retuned.
fn inverse_active(d: [[f64; 3]; 3], h: [f64; 3], active: [bool; 3]) -> Option<[[f64; 3]; 3]> {
    let scale = (0..3)
        .filter(|&i| active[i])
        .map(|i| d[i][i] + h[i])
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let mut l = [[0.; 3]; 3];
    for i in 0..3 {
        for j in 0..=i {
            let mut v = if active[i] && active[j] {
                (d[i][j] + if i == j { h[i] } else { 0. }) / scale
            } else if i == j {
                1.
            } else {
                0.
            };
            for k in 0..j {
                v -= l[i][k] * l[j][k];
            }
            if i == j {
                if !v.is_finite() || v <= 64. * f64::EPSILON {
                    return None;
                }
                l[i][j] = v.sqrt();
            } else {
                l[i][j] = v / l[j][j];
            }
        }
    }
    let mut inverse = [[0.; 3]; 3];
    for j in 0..3 {
        if !active[j] {
            continue;
        }
        let mut x = [0.; 3];
        for i in 0..3 {
            let rhs = if i == j { 1. } else { 0. };
            x[i] = (rhs - (0..i).map(|k| l[i][k] * x[k]).sum::<f64>()) / l[i][i];
        }
        for i in (0..3).rev() {
            x[i] = (x[i] - (i + 1..3).map(|k| l[k][i] * x[k]).sum::<f64>()) / l[i][i];
        }
        for i in 0..3 {
            inverse[i][j] = if active[i] { x[i] / scale } else { 0. };
        }
    }
    inverse
        .iter()
        .flatten()
        .all(|v| v.is_finite())
        .then_some(inverse)
}

// Trial-scaled cancellation bounds alone can accept zero residual stress at nonzero yield.
// This additional relative cap concerns representability, not a fitted material coefficient.
fn resolved_yield(stress: [f64; 3], limit: [f64; 3], delta: [f64; 3]) -> bool {
    (0..3).all(|i| {
        let tolerance = f64::EPSILON.sqrt() * stress[i].abs().max(limit[i]);
        stress[i].is_finite()
            && limit[i].is_finite()
            && limit[i] > 0.
            && tolerance.is_finite()
            && if delta[i] != 0. {
                (stress[i] - delta[i].signum() * limit[i]).abs() <= tolerance
            } else {
                stress[i].abs() <= limit[i] + tolerance
            }
    })
}

// Active stationarity requires K_A: = diag(H_A) * d(delta_A)/dE. Check this independent
// identity before accepting a subtractive Schur complement that may erase positive hardening.
fn resolved_tangent(
    d: [[f64; 3]; 3],
    h: [f64; 3],
    active: [bool; 3],
    inverse: [[f64; 3]; 3],
    tangent: [[f64; 3]; 3],
) -> bool {
    (0..3).all(|i| {
        let expected: [f64; 3] =
            std::array::from_fn(|j| h[i] * (0..3).map(|a| inverse[i][a] * d[a][j]).sum::<f64>());
        let scale = expected
            .iter()
            .chain(&tangent[i])
            .map(|v| v.abs())
            .fold(0., f64::max);
        !active[i]
            || (0..3).all(|j| {
                let tolerance = if h[i] > 0. {
                    f64::EPSILON.sqrt() * scale
                } else {
                    // Preserve the existing forward-roundoff allowance for a perfect-plastic row.
                    256. * f64::EPSILON * d[i][i].sqrt() * d[j][j].sqrt()
                };
                expected[j].is_finite()
                    && tolerance.is_finite()
                    && (tangent[i][j] - expected[j]).abs() <= tolerance
            })
    })
}

pub(crate) fn evaluate(
    parameters: Parameters,
    coordinate: [f64; 3],
    plastic: [f64; 3],
    accumulated: [f64; 3],
    incremental: bool,
) -> Result<(Response, [f64; 3]), Failure> {
    let d = parameters.tensor;
    let h = parameters.hardening;
    let e = std::array::from_fn(|i| coordinate[i] - plastic[i]);
    let trial = product(d, e);
    let limit: [f64; 3] =
        std::array::from_fn(|i| parameters.yield_value[i] + h[i] * accumulated[i]);
    if e.iter().chain(&trial).chain(&limit).any(|v| !v.is_finite()) {
        return Err(Failure::TrialOverflow);
    }
    let mut delta = [0.; 3];
    let mut tangent = d;
    if incremental && (0..3).any(|i| trial[i].abs() > limit[i]) {
        let mut found = false;
        let mut factorized = false;
        // Zero/inactive is ordered first: at an exact yield boundary prefer the elastic side.
        for code in 1..27 {
            let mut q = code;
            let sign: [f64; 3] = std::array::from_fn(|_| {
                let digit = q % 3;
                q /= 3;
                match digit {
                    0 => 0.,
                    1 => 1.,
                    _ => -1.,
                }
            });
            let active = sign.map(|s| s != 0.);
            // Failure belongs to this candidate; a later, smaller active set may still resolve.
            let Some(inv) = inverse_active(d, h, active) else {
                continue;
            };
            factorized = true;
            let rhs = std::array::from_fn(|i| {
                if active[i] {
                    trial[i] - sign[i] * limit[i]
                } else {
                    0.
                }
            });
            let candidate = product(inv, rhs);
            if (0..3).any(|i| active[i] && candidate[i] * sign[i] <= 0.) {
                continue;
            }
            let stress = product(d, std::array::from_fn(|i| e[i] - candidate[i]));
            let after = std::array::from_fn(|i| limit[i] + h[i] * candidate[i].abs());
            // Check the ORIGINAL KKT equations in work-conjugate stress/moment units.
            let tolerance: [f64; 3] = std::array::from_fn(|i| {
                256. * f64::EPSILON
                    * (trial[i].abs()
                        + limit[i]
                        + (0..3).map(|j| (d[i][j] * candidate[j]).abs()).sum::<f64>())
            });
            if !resolved_yield(stress, after, candidate)
                || (0..3).any(|i| {
                    !candidate[i].is_finite()
                        || !tolerance[i].is_finite()
                        || if active[i] {
                            (stress[i] - sign[i] * after[i]).abs() > tolerance[i]
                        } else {
                            stress[i].abs() > limit[i] + tolerance[i]
                        }
                })
            {
                continue;
            }
            // D - D_:A (D_AA+H)^-1 D_A:; copy the lower half for exact symmetry.
            for i in 0..3 {
                for j in 0..=i {
                    let mut correction = 0.;
                    for a in 0..3 {
                        for b in 0..3 {
                            correction += d[i][a] * inv[a][b] * d[b][j];
                        }
                    }
                    tangent[i][j] = d[i][j] - correction;
                    tangent[j][i] = tangent[i][j];
                }
            }
            if !resolved_tangent(d, h, active, inv, tangent) {
                continue;
            }
            delta = candidate;
            found = true;
            break;
        }
        if !found {
            return Err(if factorized {
                Failure::NoReturnMap
            } else {
                Failure::SingularActiveTensor
            });
        }
    }
    if (0..3).any(|i| accumulated[i] + delta[i].abs() > parameters.accumulation_limit[i]) {
        return Err(Failure::Accumulation);
    }
    if incremental {
        // Check the actual rounded state advance() will store, not only (coordinate-plastic)-delta.
        let stored_stress = product(
            d,
            std::array::from_fn(|i| coordinate[i] - (plastic[i] + delta[i])),
        );
        let stored_limit = std::array::from_fn(|i| {
            parameters.yield_value[i] + h[i] * (accumulated[i] + delta[i].abs())
        });
        if !resolved_yield(stored_stress, stored_limit, delta) {
            return Err(Failure::NoReturnMap);
        }
    }
    let elastic = std::array::from_fn(|i| e[i] - delta[i]);
    let stress = product(d, elastic);
    let energy = 0.5 * dot(elastic, stress)
        + if incremental {
            (0..3)
                .map(|i| limit[i] * delta[i].abs() + 0.5 * h[i] * delta[i] * delta[i])
                .sum::<f64>()
        } else {
            (0..3)
                .map(|i| 0.5 * h[i] * accumulated[i] * accumulated[i])
                .sum::<f64>()
        };
    if !energy.is_finite()
        || stress
            .iter()
            .chain(tangent.iter().flatten())
            .any(|v| !v.is_finite())
    {
        return Err(Failure::ResponseOverflow);
    }
    Ok((
        Response {
            energy,
            stress,
            tangent,
        },
        delta,
    ))
}
