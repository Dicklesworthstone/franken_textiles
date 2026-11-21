//! Source-declared virtual extensometers evaluated on the production strip displacement field.
//!
//! Stations and markers follow original material points, not a line in the deformed image.
//! Chord length and signed projection are different observations, particularly for bias cuts.

use ftx_core::{Refusal, Vec3};
use serde::{Deserialize, Serialize};

use super::experiment::{Fixture, State, invalid};

/// Geometric meaning of one measured channel. No operator is inferred from a source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operator {
    /// Euclidean distance from the left to the right side material point at this reference station.
    EdgeChord {
        /// Original axial coordinate divided by the original free gauge, in [0,1].
        station_fraction: f64,
    },
    /// Signed projection of the left-to-right material chord on a fixed strip-plane unit axis.
    EdgeProjection {
        /// Original axial coordinate divided by the original free gauge, in [0,1].
        station_fraction: f64,
        /// Explicit unit [x,y] axis in the strip frame; it is never normalized or rotated implicitly.
        axis: [f64; 2],
    },
    /// Euclidean distance between two declared reference material points, including interior markers.
    MaterialChord {
        /// Original [across-width, along-gauge] coordinates divided by the respective dimensions.
        start_fraction: [f64; 2],
        /// Distinct original material point in the same normalized reference coordinates.
        end_fraction: [f64; 2],
    },
    /// Signed projection of the end-minus-start material chord on a fixed strip-plane unit axis.
    MaterialProjection {
        /// Original [across-width, along-gauge] coordinates divided by the respective dimensions.
        start_fraction: [f64; 2],
        /// Distinct original material point in the same normalized reference coordinates.
        end_fraction: [f64; 2],
        /// Explicit unit [x,y] axis in the strip frame; it is never normalized or rotated implicitly.
        axis: [f64; 2],
    },
}

/// Source-bearing through its parent acquisition; residual scale is not an uncertainty interval.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extensometer {
    /// Unique channel identity within one observed state, retained across predictions.
    pub id: String,
    /// Explicit material-following spatial observation.
    pub operator: Operator,
    /// Observed length or signed projected displacement, metres.
    pub value_m: f64,
    /// Finite positive residual scale, metres.
    pub scale_m: f64,
}

/// Prediction from the accepted production equilibrium and the exact declared spatial operator.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtensometerPrediction {
    /// Original channel identity.
    pub id: String,
    /// Unchanged observation definition, independent of mesh refinement.
    pub operator: Operator,
    /// Actual predicted length or signed projected displacement, metres.
    pub predicted_m: f64,
    /// Predicted minus observed value, metres.
    pub residual_m: f64,
}

impl Operator {
    /// Check finite reference coordinates and an explicitly supplied unit projection axis.
    ///
    /// # Errors
    /// A reference coordinate outside [0,1], coincident markers or a non-unit projection axis.
    pub fn validate(&self) -> Result<(), Refusal> {
        match self {
            Self::EdgeChord { station_fraction }
            | Self::EdgeProjection {
                station_fraction, ..
            } => {
                if !station_fraction.is_finite() || !(0.0..=1.0).contains(station_fraction) {
                    return Err(invalid(
                        "strip extensometer station must be finite and in [0,1] of the original free gauge",
                    ));
                }
            }
            Self::MaterialChord {
                start_fraction,
                end_fraction,
            }
            | Self::MaterialProjection {
                start_fraction,
                end_fraction,
                ..
            } => {
                if start_fraction == end_fraction
                    || start_fraction
                        .iter()
                        .chain(end_fraction)
                        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                {
                    return Err(invalid(
                        "strip extensometer markers need distinct finite [across-width, along-gauge] reference fractions in [0,1]",
                    ));
                }
            }
        }
        if let Self::EdgeProjection { axis: [x, y], .. }
        | Self::MaterialProjection { axis: [x, y], .. } = self
            && (!x.is_finite() || !y.is_finite() || (x * x + y * y - 1.0).abs() > 1e-12)
        {
            return Err(invalid(
                "strip extensometer projection needs an explicit finite unit axis in the strip plane",
            ));
        }
        Ok(())
    }

    pub(super) fn is_chord(&self) -> bool {
        matches!(self, Self::EdgeChord { .. } | Self::MaterialChord { .. })
    }
}

/// Evaluate the declared operator on the actual piecewise-linear production displacement field.
/// Side stations interpolate on material edges; interior markers use the grid's alternating
/// triangles in reference coordinates. This does not establish equilibrium or qualify a source.
///
/// # Errors
/// Invalid fixture/operator, a mismatched mesh, nonfinite positions or an invalid resulting value.
pub fn evaluate(operator: &Operator, fixture: &Fixture, state: &State) -> Result<f64, Refusal> {
    fixture.validate()?;
    operator.validate()?;
    let [nx, ny] = fixture.cells;
    if state.positions_m.len() != (nx + 1) * (ny + 1)
        || state.positions_m.iter().flatten().any(|v| !v.is_finite())
        || state.positions_m.iter().any(|p| p[2] != 0.0)
    {
        return Err(invalid(
            "strip extensometer requires the matching finite planar production mesh",
        ));
    }
    let chord = match operator {
        Operator::EdgeChord { station_fraction }
        | Operator::EdgeProjection {
            station_fraction, ..
        } => {
            // Retain the original edge arithmetic for existing observations and frozen replays.
            let coordinate = station_fraction * ny as f64;
            let row = coordinate.floor() as usize;
            let fraction = coordinate - row as f64;
            let point = |column: usize| {
                let p = state.positions_m[row * (nx + 1) + column];
                let a = Vec3::new(p[0], p[1], p[2]);
                if fraction == 0.0 {
                    a
                } else {
                    let p = state.positions_m[(row + 1) * (nx + 1) + column];
                    let b = Vec3::new(p[0], p[1], p[2]);
                    a + (b - a) * fraction
                }
            };
            point(nx) - point(0)
        }
        Operator::MaterialChord {
            start_fraction,
            end_fraction,
        }
        | Operator::MaterialProjection {
            start_fraction,
            end_fraction,
            ..
        } => {
            material_point(*end_fraction, fixture, state)
                - material_point(*start_fraction, fixture, state)
        }
    };
    let value = match operator {
        Operator::EdgeChord { .. } | Operator::MaterialChord { .. } => chord.norm(),
        Operator::EdgeProjection { axis, .. } | Operator::MaterialProjection { axis, .. } => {
            chord.x * axis[0] + chord.y * axis[1]
        }
    };
    if !value.is_finite() || (operator.is_chord() && value <= 0.0) {
        return Err(invalid(
            "strip extensometer produced an invalid chord or projected value",
        ));
    }
    Ok(value)
}

/// Exact vertex-position gradient `∂(operator)/∂x` on the production displacement field, matching
/// [`evaluate`] on every edge station and interior alternating-diagonal triangle.
///
/// # Errors
/// Invalid fixture/operator, mismatched mesh, nonfinite positions, degenerate chord or nonfinite
/// gradient.
pub fn gradient_dx(
    operator: &Operator,
    fixture: &Fixture,
    state: &State,
) -> Result<Vec<Vec3>, Refusal> {
    fixture.validate()?;
    operator.validate()?;
    let [nx, ny] = fixture.cells;
    let n_verts = (nx + 1) * (ny + 1);
    if state.positions_m.len() != n_verts
        || state.positions_m.iter().flatten().any(|v| !v.is_finite())
        || state.positions_m.iter().any(|p| p[2] != 0.0)
    {
        return Err(invalid(
            "strip extensometer requires the matching finite planar production mesh",
        ));
    }
    let (start_weights, end_weights) = match operator {
        Operator::EdgeChord { station_fraction }
        | Operator::EdgeProjection {
            station_fraction, ..
        } => {
            let coordinate = station_fraction * ny as f64;
            let row = coordinate.floor() as usize;
            let fraction = coordinate - row as f64;
            let edge = |column: usize| {
                let mut out = Vec::with_capacity(2);
                let a = row * (nx + 1) + column;
                if fraction == 0.0 {
                    out.push((a, 1.0));
                } else {
                    out.push((a, 1.0 - fraction));
                    out.push(((row + 1) * (nx + 1) + column, fraction));
                }
                out
            };
            (edge(0), edge(nx))
        }
        Operator::MaterialChord {
            start_fraction,
            end_fraction,
        }
        | Operator::MaterialProjection {
            start_fraction,
            end_fraction,
            ..
        } => (
            material_point_weights(*start_fraction, fixture),
            material_point_weights(*end_fraction, fixture),
        ),
    };
    let point_from = |weights: &[(usize, f64)]| {
        weights.iter().fold(Vec3::ZERO, |acc, &(idx, w)| {
            let p = state.positions_m[idx];
            acc + Vec3::new(p[0], p[1], p[2]) * w
        })
    };
    let chord = point_from(&end_weights) - point_from(&start_weights);
    let direction = match operator {
        Operator::EdgeChord { .. } | Operator::MaterialChord { .. } => {
            let len = chord.norm();
            if !len.is_finite() || len <= 0.0 {
                return Err(invalid(
                    "strip extensometer produced an invalid chord or projected value",
                ));
            }
            chord / len
        }
        Operator::EdgeProjection { axis, .. } | Operator::MaterialProjection { axis, .. } => {
            let value = chord.x * axis[0] + chord.y * axis[1];
            if !value.is_finite() {
                return Err(invalid(
                    "strip extensometer produced an invalid chord or projected value",
                ));
            }
            Vec3::new(axis[0], axis[1], 0.0)
        }
    };
    let mut grad = vec![Vec3::ZERO; n_verts];
    for &(idx, w) in &end_weights {
        grad[idx] += direction * w;
    }
    for &(idx, w) in &start_weights {
        grad[idx] -= direction * w;
    }
    if grad.iter().any(|g| !g.is_finite()) {
        return Err(invalid("strip extensometer gradient overflow"));
    }
    Ok(grad)
}

/// Exact barycentric weights of [`material_point`] on the alternating-diagonal `cloth::grid`.
fn material_point_weights(reference: [f64; 2], fixture: &Fixture) -> Vec<(usize, f64)> {
    let [nx, ny] = fixture.cells;
    let x = reference[0] * nx as f64;
    let y = reference[1] * ny as f64;
    let column = x.floor() as usize;
    let row = y.floor() as usize;
    let u = x - column as f64;
    let v = y - row as f64;
    let idx = |c: usize, r: usize| r * (nx + 1) + c;
    let a = idx(column, row);
    if u == 0.0 {
        return if v == 0.0 {
            vec![(a, 1.0)]
        } else {
            vec![(a, 1.0 - v), (idx(column, row + 1), v)]
        };
    }
    let b = idx(column + 1, row);
    if v == 0.0 {
        return vec![(a, 1.0 - u), (b, u)];
    }
    let c = idx(column + 1, row + 1);
    let d = idx(column, row + 1);
    if (column + row).is_multiple_of(2) {
        if v <= u {
            vec![(a, 1.0 - u), (b, u - v), (c, v)]
        } else {
            vec![(a, 1.0 - v), (c, u), (d, v - u)]
        }
    } else if u + v <= 1.0 {
        vec![(a, 1.0 - u - v), (b, u), (d, v)]
    } else {
        vec![(c, u + v - 1.0), (b, 1.0 - v), (d, 1.0 - u)]
    }
}

/// Barycentric interpolation on the exact alternating diagonals emitted by `cloth::grid`.
/// Validation has established the finite matching mesh and bounded reference coordinates.
fn material_point(reference: [f64; 2], fixture: &Fixture, state: &State) -> Vec3 {
    let [nx, ny] = fixture.cells;
    let x = reference[0] * nx as f64;
    let y = reference[1] * ny as f64;
    let column = x.floor() as usize;
    let row = y.floor() as usize;
    let u = x - column as f64;
    let v = y - row as f64;
    let point = |column: usize, row: usize| {
        let p = state.positions_m[row * (nx + 1) + column];
        Vec3::new(p[0], p[1], p[2])
    };
    let a = point(column, row);
    // Grid lines also cover the upper/right boundary without indexing a nonexistent cell.
    if u == 0.0 {
        return if v == 0.0 {
            a
        } else {
            a + (point(column, row + 1) - a) * v
        };
    }
    let b = point(column + 1, row);
    if v == 0.0 {
        return a + (b - a) * u;
    }
    let c = point(column + 1, row + 1);
    let d = point(column, row + 1);
    if (column + row).is_multiple_of(2) {
        if v <= u {
            a + (b - a) * (u - v) + (c - a) * v
        } else {
            a + (c - a) * u + (d - a) * (v - u)
        }
    } else if u + v <= 1.0 {
        a + (b - a) * u + (d - a) * v
    } else {
        c + (b - c) * (1.0 - v) + (d - c) * (1.0 - u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strip::experiment::{Controls, solve};

    #[test]
    fn gradient_dx_matches_central_fd_across_all_operators_and_diagonals() {
        let fixture = Fixture {
            gauge_length_m: 0.10,
            width_m: 0.06,
            warp_axis: [
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ],
            cells: [4, 4],
        };
        let state = solve(
            &fixture,
            [300.0, 180.0, 0.2, 70.0],
            0.08,
            &Controls::default(),
        )
        .unwrap();
        let operators = [
            Operator::EdgeChord {
                station_fraction: 0.37,
            },
            Operator::EdgeChord {
                station_fraction: 0.5,
            },
            Operator::EdgeProjection {
                station_fraction: 0.63,
                axis: [0.8, 0.6],
            },
            Operator::MaterialChord {
                start_fraction: [0.12, 0.08], // even cell, v <= u
                end_fraction: [0.82, 0.91],   // even cell, v > u
            },
            Operator::MaterialChord {
                start_fraction: [0.31, 0.14], // odd cell, u + v <= 1
                end_fraction: [0.46, 0.69],   // odd cell, u + v > 1
            },
            Operator::MaterialProjection {
                start_fraction: [0.0, 0.25], // left edge u == 0
                end_fraction: [0.75, 1.0],   // top edge v == 0 at row == ny
                axis: [-0.6, 0.8],
            },
        ];
        let eps = 1e-7;
        for op in &operators {
            let grad = gradient_dx(op, &fixture, &state).unwrap();
            assert_eq!(grad.len(), state.positions_m.len());
            for v in 0..state.positions_m.len() {
                for axis in 0..2 {
                    let mut plus = state.clone();
                    plus.positions_m[v][axis] += eps;
                    let mut minus = state.clone();
                    minus.positions_m[v][axis] -= eps;
                    let fd = (evaluate(op, &fixture, &plus).unwrap()
                        - evaluate(op, &fixture, &minus).unwrap())
                        / (2.0 * eps);
                    let analytic = grad[v][axis];
                    assert!(
                        (analytic - fd).abs() <= 1e-9 * (1.0 + fd.abs()),
                        "op={op:?} v={v} axis={axis}: analytic={analytic} fd={fd}"
                    );
                }
                assert_eq!(grad[v].z, 0.0);
            }
        }
    }
}
