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
            let inv = inverse_active(d, h, active).ok_or(Failure::SingularActiveTensor)?;
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
            // Check the ORIGINAL KKT equations in work-conjugate stress/moment units.
            let tolerance: [f64; 3] = std::array::from_fn(|i| {
                256. * f64::EPSILON
                    * (trial[i].abs()
                        + limit[i]
                        + (0..3).map(|j| (d[i][j] * candidate[j]).abs()).sum::<f64>())
            });
            if (0..3).any(|i| {
                !candidate[i].is_finite()
                    || !stress[i].is_finite()
                    || !tolerance[i].is_finite()
                    || if active[i] {
                        (stress[i] - sign[i] * (limit[i] + h[i] * candidate[i].abs())).abs()
                            > tolerance[i]
                    } else {
                        stress[i].abs() > limit[i] + tolerance[i]
                    }
            }) {
                continue;
            }
            delta = candidate;
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
            found = true;
            break;
        }
        if !found {
            return Err(Failure::NoReturnMap);
        }
    }
    if (0..3).any(|i| accumulated[i] + delta[i].abs() > parameters.accumulation_limit[i]) {
        return Err(Failure::Accumulation);
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
