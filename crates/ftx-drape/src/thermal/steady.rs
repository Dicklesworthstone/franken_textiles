//! Bounded, source-qualified steady Fourier conduction without fabricated thermal storage.
//!
//! One-dimensional, equal-area, fixed-geometry layers with declared contact and film resistances.
//! Integrating k(T) across each layer avoids the mean-temperature approximation and spatial mesh
//! error. This is not a moisture, compression, wearer comfort or measured-garment validation model.

use super::{Boundary, invalid, nonnegative, positive, temperature};
use ftx_core::Refusal;
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, Selection},
    material_thermal::ConductionLaw,
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub mod coverage;
pub mod request;
pub mod surface;

const ITERATIONS: usize = 128;
const MAX_LAYERS: usize = 256;

/// Face temperatures and through-flux, always in the original inside-to-outside layer order.
#[derive(Clone, Debug, Serialize)]
pub struct SteadyState {
    /// [Inside face, outside face] in degrees Celsius for each layer; contact jumps stay explicit.
    pub layer_faces_c: Vec<[f64; 2]>,
    /// Through-flux in W/m², positive from the inside reservoir toward the outside reservoir.
    pub flux_w_m2: f64,
    /// Largest absolute residual of the original integrated layer Fourier equations, W/m².
    pub max_flux_residual_w_m2: f64,
    /// Largest absolute contact/film temperature-drop residual, in degrees Celsius.
    pub max_temperature_residual_c: f64,
}

/// Immutable shared material selections; no cells, time steps, mass or heat capacity are invented.
#[derive(Clone, Debug)]
pub struct SteadyConductionModel {
    layers: Vec<ResolvedMaterial>,
    laws: Vec<ConductionLaw>,
    interfaces: Vec<f64>,
    // Temporary coverage solve factors A_body/A_cloth. These never alter source observations or
    // public snapshots; only the coverage adapter creates the short-lived scaled solve object.
    area_factors: Vec<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema: String,
    layers: Vec<Selection>,
    interface_resistance_m2_k_w: Vec<f64>,
}

// An out-of-domain trial is an ordering bound, NEVER a clamped material evaluation. If low/high
// bounds meet without a valid, residual-qualified shot, solve() refuses the whole calculation.
enum Shot {
    Low,
    High,
    Valid { faces: Vec<[f64; 2]>, residual: f64 },
}

impl SteadyConductionModel {
    /// Resolve transport from the same material authority as drape and transient heat transport.
    /// Interfaces are explicitly supplied equal-area contact resistances, not guessed air gaps.
    ///
    /// # Errors
    /// Missing transport/thickness, denied evidence, invalid interfaces or more than 256 layers.
    pub fn new(layers: Vec<ResolvedMaterial>, interfaces: Vec<f64>) -> Result<Self, Refusal> {
        if layers.is_empty() || layers.len() > MAX_LAYERS || interfaces.len() != layers.len() - 1 {
            return Err(invalid(
                "steady conduction needs 1..256 layers and exactly one resistance per interface",
            ));
        }
        for &r in &interfaces {
            nonnegative("interface resistance", r)?;
        }
        let laws = layers
            .iter()
            .map(|m| ConductionLaw::from_material(m.material(), m.allows_estimates()))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            area_factors: vec![1.0; layers.len()],
            layers,
            laws,
            interfaces,
        })
    }

    pub(super) fn with_area_factors(&self, factors: Vec<f64>) -> Result<Self, Refusal> {
        if factors.len() != self.layers.len() {
            return Err(invalid("steady coverage needs one area factor per layer"));
        }
        for &factor in &factors {
            positive("steady coverage area factor", factor)?;
        }
        let mut scaled = self.clone();
        scaled.area_factors = factors;
        Ok(scaled)
    }

    /// Portable source-complete model inputs; replay does not consult a live catalog.
    #[must_use]
    pub fn snapshot(&self) -> Value {
        json!({"schema":"ftx-steady-conduction-model/1",
            "layers":self.layers.iter().map(ResolvedMaterial::frozen_selection).collect::<Vec<_>>(),
            "interface_resistance_m2_k_w":self.interfaces})
    }

    /// Replay source-locked model inputs, refusing unknown fields and unfrozen selections.
    ///
    /// # Errors
    /// Unsupported schema, missing records/digests, tampered evidence or invalid transport data.
    pub fn from_snapshot(value: &Value) -> Result<Self, Refusal> {
        let input: Snapshot =
            serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
        if input.schema != "ftx-steady-conduction-model/1"
            || input.layers.is_empty()
            || input.layers.len() > MAX_LAYERS
            || input
                .layers
                .iter()
                .any(|s| s.record.is_none() || s.expected_sha256.is_none())
        {
            return Err(invalid(
                "steady replay requires a supported, fully frozen model",
            ));
        }
        let empty = Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: Vec::new(),
        };
        let layers = input
            .layers
            .iter()
            .map(|s| empty.resolve(s))
            .collect::<Result<_, _>>()?;
        Self::new(layers, input.interface_resistance_m2_k_w)
    }

    /// Solve integrated Fourier equations, contacts and films in a bounded monotone flux search.
    /// Material faces must lie in their OWN characterized intervals; reservoir temperatures behind
    /// films need not. Nonoverlapping layer intervals can be connected by declared contact jumps.
    ///
    /// # Errors
    /// Invalid boundary conditions, unresolvable domains, overflow, exhausted iteration budget or
    /// failed original-equation residuals. No partial or extrapolated solution is returned.
    pub fn solve(&self, boundary: Boundary) -> Result<SteadyState, Refusal> {
        temperature(boundary.inside_c)?;
        temperature(boundary.outside_c)?;
        nonnegative("inside film resistance", boundary.inside_resistance)?;
        nonnegative("outside film resistance", boundary.outside_resistance)?;
        let reverse = boundary.inside_c < boundary.outside_c;
        let (cold, hot, cold_r, hot_r) = if reverse {
            (
                boundary.inside_c,
                boundary.outside_c,
                boundary.inside_resistance,
                boundary.outside_resistance,
            )
        } else {
            (
                boundary.outside_c,
                boundary.inside_c,
                boundary.outside_resistance,
                boundary.inside_resistance,
            )
        };
        let span = hot - cold;
        if !span.is_finite() {
            return Err(invalid("reservoir temperature difference overflows"));
        }
        if span == 0.0 {
            for law in &self.laws {
                law.check_temperature(cold)?;
            }
            return self.finish(vec![[cold, cold]; self.layers.len()], 0.0, boundary);
        }
        // A seed only: these midpoints do not become solved material temperatures or observations.
        // Assemble in cold-to-hot order too: physically reversing the stack and reservoirs then
        // keeps the identical floating-point path, rather than perturbing the root-search seed.
        let mut resistance = cold_r + hot_r;
        for step in 0..self.layers.len() {
            let i = if reverse {
                step
            } else {
                self.layers.len() - 1 - step
            };
            let law = &self.laws[i];
            let [a, b] = law.domain().unwrap_or([cold, hot]);
            resistance += positive(
                "area-scaled steady resistance",
                law.resistance_at(a / 2.0 + b / 2.0)? * self.area_factors[i],
            )?;
            if step + 1 < self.layers.len() {
                resistance += self.interfaces[if reverse { i } else { i - 1 }];
            }
        }
        positive("steady seed resistance", resistance)?;
        let mut high = positive("steady seed flux", span / resistance)?;
        let mut low = 0.0;
        let tolerance = temperature_tolerance(boundary);
        // Expand a bracket, then bisect. Each shot inverts strictly positive conductivity integrals.
        let mut bracketed = false;
        for _ in 0..ITERATIONS {
            match self.shoot(high, cold, hot, cold_r, hot_r, reverse)? {
                Shot::Valid { faces, residual } if residual.abs() <= tolerance => {
                    return self.finish(faces, if reverse { -high } else { high }, boundary);
                }
                Shot::High => {
                    bracketed = true;
                    break;
                }
                Shot::Valid { residual, .. } if residual > 0.0 => {
                    bracketed = true;
                    break;
                }
                _ => {
                    low = high;
                    high = positive("steady flux bracket", high * 2.0)?;
                }
            }
        }
        if bracketed {
            // Include zero in the bracket when the very first shot was already too hot.
            for _ in 0..ITERATIONS {
                let q = low + (high - low) / 2.0;
                if q == low || q == high {
                    break;
                }
                match self.shoot(q, cold, hot, cold_r, hot_r, reverse)? {
                    Shot::Valid { faces, residual } if residual.abs() <= tolerance => {
                        return self.finish(faces, if reverse { -q } else { q }, boundary);
                    }
                    Shot::High => high = q,
                    Shot::Valid { residual, .. } if residual > 0.0 => high = q,
                    _ => low = q,
                }
            }
        }
        Err(invalid(
            "no residual-qualified steady solution within characterized layer domains and iteration budget",
        ))
    }

    fn shoot(
        &self,
        q: f64,
        cold: f64,
        hot: f64,
        cold_r: f64,
        hot_r: f64,
        reverse: bool,
    ) -> Result<Shot, Refusal> {
        let mut t = cold + q * cold_r;
        let mut faces = vec![[0.0; 2]; self.layers.len()];
        for step in 0..self.layers.len() {
            let i = if reverse {
                step
            } else {
                self.layers.len() - 1 - step
            };
            let law = &self.laws[i];
            let [lo, hi] = law.domain().unwrap_or([cold, hot]);
            if t < lo {
                return Ok(Shot::Low);
            }
            let ceiling = hi.min(hot);
            if !t.is_finite() || t > ceiling {
                return Ok(Shot::High);
            }
            let maximum = scaled_flux(law, ceiling, t, self.area_factors[i])?;
            if q > maximum {
                return Ok(Shot::High);
            }
            let warm = if q == maximum {
                ceiling
            } else {
                inverse(law, t, ceiling, q, self.area_factors[i])?
            };
            faces[i] = if reverse { [t, warm] } else { [warm, t] };
            t = warm;
            if step + 1 < self.layers.len() {
                t += q * self.interfaces[if reverse { i } else { i - 1 }];
            }
        }
        let residual = (t - hot) + q * hot_r;
        if !residual.is_finite() {
            return Ok(Shot::High);
        }
        Ok(Shot::Valid { faces, residual })
    }

    fn finish(&self, faces: Vec<[f64; 2]>, q: f64, b: Boundary) -> Result<SteadyState, Refusal> {
        let mut flux_residual: f64 = 0.0;
        let mut temperature_residual = (b.inside_c - faces[0][0] - q * b.inside_resistance).abs();
        let last = faces[faces.len() - 1][1];
        temperature_residual =
            temperature_residual.max((last - b.outside_c - q * b.outside_resistance).abs());
        for (i, (law, &[inside, outside])) in self.laws.iter().zip(&faces).enumerate() {
            if [inside, outside].iter().any(|&t| {
                !t.is_finite() || t < b.inside_c.min(b.outside_c) || t > b.inside_c.max(b.outside_c)
            }) {
                return Err(invalid("steady maximum principle failed"));
            }
            flux_residual = flux_residual
                .max((scaled_flux(law, inside, outside, self.area_factors[i])? - q).abs());
            if i < self.interfaces.len() {
                temperature_residual = temperature_residual
                    .max((outside - faces[i + 1][0] - q * self.interfaces[i]).abs());
            }
        }
        if !q.is_finite()
            || !flux_residual.is_finite()
            || !temperature_residual.is_finite()
            || flux_residual > 1e-9 * q.abs()
            || temperature_residual > temperature_tolerance(b)
        {
            return Err(invalid("steady original-equation residual check failed"));
        }
        Ok(SteadyState {
            layer_faces_c: faces,
            flux_w_m2: q,
            max_flux_residual_w_m2: flux_residual,
            max_temperature_residual_c: temperature_residual,
        })
    }
}

fn temperature_tolerance(b: Boundary) -> f64 {
    256.0 * f64::EPSILON * (1.0 + b.inside_c.abs().max(b.outside_c.abs()))
        + 1e-11 * (b.inside_c - b.outside_c).abs()
}

fn scaled_flux(law: &ConductionLaw, a: f64, b: f64, factor: f64) -> Result<f64, Refusal> {
    let flux = law.flux_between(a, b)?;
    let scaled = flux / factor;
    if !scaled.is_finite() || (flux != 0.0 && scaled == 0.0) {
        return Err(invalid("area-scaled steady flux is not representable"));
    }
    Ok(scaled)
}

fn inverse(
    law: &ConductionLaw,
    cold: f64,
    mut high: f64,
    q: f64,
    factor: f64,
) -> Result<f64, Refusal> {
    let mut low = cold;
    for _ in 0..ITERATIONS {
        let mid = low + (high - low) / 2.0;
        if mid == low || mid == high {
            break;
        }
        let flux = scaled_flux(law, mid, cold, factor)?;
        if flux == q {
            return Ok(mid);
        }
        if flux > q {
            high = mid;
        } else {
            low = mid;
        }
    }
    let lo_error = (scaled_flux(law, low, cold, factor)? - q).abs();
    let hi_error = (scaled_flux(law, high, cold, factor)? - q).abs();
    Ok(if lo_error < hi_error { low } else { high })
}
