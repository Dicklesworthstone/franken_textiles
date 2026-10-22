//! Source-declared virtual extensometers evaluated on the production strip displacement field.
//!
//! A station follows the original side-edge material points, not a line in the deformed image.
//! Chord length and signed projection are different observations, particularly for bias cuts.

use ftx_core::{Refusal, Vec3};
use serde::{Deserialize, Serialize};

use super::experiment::{Fixture, State, invalid};

/// Geometric meaning of one measured transverse channel. No operator is inferred from a source.
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
    /// A station outside [0,1], nonfinite coordinate or non-unit projection axis.
    pub fn validate(&self) -> Result<(), Refusal> {
        let station = match self {
            Self::EdgeChord { station_fraction }
            | Self::EdgeProjection {
                station_fraction, ..
            } => *station_fraction,
        };
        if !station.is_finite() || !(0.0..=1.0).contains(&station) {
            return Err(invalid(
                "strip extensometer station must be finite and in [0,1] of the original free gauge",
            ));
        }
        if let Self::EdgeProjection { axis: [x, y], .. } = self
            && (!x.is_finite() || !y.is_finite() || (x * x + y * y - 1.0).abs() > 1e-12)
        {
            return Err(invalid(
                "strip extensometer projection needs an explicit finite unit axis in the strip plane",
            ));
        }
        Ok(())
    }
}

/// Evaluate the declared operator on a strip state, using the actual piecewise-linear side edges.
/// Exact mesh stations use their original vertex values; intermediate stations interpolate in
/// reference coordinates. This does not establish equilibrium or qualify a source's measurement.
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
    let station = match operator {
        Operator::EdgeChord { station_fraction }
        | Operator::EdgeProjection {
            station_fraction, ..
        } => *station_fraction,
    };
    let coordinate = station * ny as f64;
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
    let chord = point(nx) - point(0);
    let value = match operator {
        Operator::EdgeChord { .. } => chord.norm(),
        Operator::EdgeProjection { axis, .. } => chord.x * axis[0] + chord.y * axis[1],
    };
    if !value.is_finite() || (matches!(operator, Operator::EdgeChord { .. }) && value <= 0.0) {
        return Err(invalid(
            "strip extensometer produced an invalid chord or projected value",
        ));
    }
    Ok(value)
}
