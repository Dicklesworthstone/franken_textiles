//! Quasi-static normal closure of coextensive layers between rigid, parallel faces.
//!
//! Every layer carries its own qualified pressure/thickness branch. Fixed reference area and
//! common normal pressure are explicit assumptions, not inferred garment contact topology.
//! External work integrates that branch; it is NOT identified as stored energy or generated heat.

use std::collections::BTreeSet;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use super::{Input as Preparation, Report as Conditioned, condition, invalid, rows, text_ok};
use crate::material_catalog::normalize;
use crate::materials::Source;

/// Input schema for an inverse, fixed-reference-area compression calculation.
pub const SCHEMA: &str = "ftx-compression-stack/1";

/// A monotone traverse of the caller's explicitly chosen source branches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Increase pressure from the supplied initial state; stack thickness cannot increase.
    Loading,
    /// Decrease pressure from the supplied initial state; stack thickness cannot decrease.
    Unloading,
}

/// One ordered layer; identical materials may occur more than once under different layer IDs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    /// Unique assembly identity, separate from the material/specimen identity.
    pub id: String,
    /// Existing pressure-conditioning input, including all acquisition and invariance evidence.
    /// Its pressure is the KNOWN INITIAL pressure, identical across every layer, not a guess.
    /// Supply the branch appropriate to the requested direction; no branch is inferred from a name.
    pub preparation: Preparation,
}

/// Prescribed geometry and the same qualified inputs accepted by pressure conditioning.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Final distance between the parallel faces, metres. No extra air gap is invented.
    pub gap_m: f64,
    /// Fixed common reference/contact area, square metres; no lateral expansion is modeled.
    pub reference_area_m2: f64,
    /// Declared loading/unloading traverse of the supplied source branches.
    pub direction: Direction,
    /// One to 32 coextensive layers, in physical stack order, each with at most 128 pressure knots.
    pub layers: Vec<Layer>,
    /// Admit the piecewise-linear inverse/path model. Separate layer policies still govern
    /// estimated observations, retained properties and interpolated final material quantities.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Per-layer result, with an ordinary immutable selection for all existing material consumers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayerResult {
    /// Input layer identity; output order remains physical stack order.
    pub id: String,
    /// Full layer thickness at the known initial pressure, metres.
    pub initial_thickness_m: f64,
    /// Full thickness at the inferred final pressure, metres.
    pub thickness_m: f64,
    /// Initial minus final thickness, negative for unloading.
    pub closure_m: f64,
    /// Signed external normal work per reference area along the declared branch, J/m².
    /// Not a claim of reversible storage, dissipated work, hysteresis or thermal conversion.
    pub external_work_j_m2: f64,
    /// Retained reference mass for this layer, if independently qualified, kg.
    pub reference_mass_kg: Option<f64>,
    /// Scalar through-thickness resistance at this pressure, under its retained source conditions.
    /// Excludes interlayer/contact/air and boundary-film resistances.
    pub material_resistance_m2_k_w: Option<f64>,
    /// Missing optional mass/transport capabilities do not invalidate mechanical closure.
    pub unavailable: Vec<String>,
    /// Complete existing conditioning result, including discarded properties and exact
    /// `selection_json`. Neither a new material type nor a calibration-only selection.
    pub conditioned: Conditioned,
}

/// Atomic stack result. Invalid or nonunique inverse problems return a refusal, not partial states.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// `ftx-compression-stack-report/1`.
    pub schema: String,
    /// Complete authored inputs and ordered layer preimages, unchanged.
    pub input: Input,
    /// Digest of the complete typed request, also referenced by every resulting material.
    pub input_sha256: String,
    /// Intersection of all acquired pressure domains, before the direction restriction, Pa.
    pub common_pressure_range_pa: [f64; 2],
    /// Known initial common pressure, Pa.
    pub initial_pressure_pa: f64,
    /// Inferred final common pressure, Pa. This is a model estimate, not a pressure observation.
    pub pressure_pa: f64,
    /// The union-knot interval used by the inverse, or [p,p] at an exact knot.
    pub bracket_pa: [f64; 2],
    /// Sum of initial full thicknesses, metres.
    pub initial_thickness_m: f64,
    /// Sum of final full thicknesses, metres.
    pub thickness_m: f64,
    /// Final thickness minus requested clearance, metres; numerical, not experimental error.
    pub closure_residual_m: f64,
    /// Normal force at the final state, pressure times the declared reference area, newtons.
    pub normal_force_n: f64,
    /// Signed external work on the complete stack, joules, never used automatically as heating.
    pub external_work_j: f64,
    /// Total reference mass only if every layer supplies admitted reference areal mass, kg.
    pub reference_mass_kg: Option<f64>,
    /// Series material resistance only if every layer supplies admitted scalar transport.
    /// No film/interface/air resistance or comfort rating is inferred.
    pub material_resistance_m2_k_w: Option<f64>,
    /// Layer states in physical stack order; missing optional capabilities remain visible.
    pub layers: Vec<LayerResult>,
    /// Scope of the equilibrium/path model; not a physical acceptance certificate.
    pub no_claim: String,
}

struct Curve {
    points: Vec<[f64; 2]>, // pressure (Pa), normalized full thickness (m)
}

impl Curve {
    fn thickness(&self, pressure: f64) -> f64 {
        let hi = self.points.partition_point(|p| p[0] < pressure);
        if self.points[hi][0] == pressure {
            self.points[hi][1]
        } else {
            let [pa, a] = self.points[hi - 1];
            let [pb, b] = self.points[hi];
            super::lerp(a, b, (pressure - pa) / (pb - pa))
        }
    }

    fn work(&self, initial: f64, final_pressure: f64) -> Result<f64, Refusal> {
        let lo = initial.min(final_pressure);
        let hi = initial.max(final_pressure);
        let mut knots = vec![lo];
        knots.extend(
            self.points
                .iter()
                .map(|p| p[0])
                .filter(|&p| p > lo && p < hi),
        );
        knots.push(hi);
        let mut work = 0.0;
        for pair in knots.windows(2) {
            // Exact integral of p*(-dt/dp) on each linear segment. Half first avoids overflow
            // from pa+pb when the individually finite pressures are very large.
            let closure = self.thickness(pair[0]) - self.thickness(pair[1]);
            let increment = (0.5 * pair[0] + 0.5 * pair[1]) * closure;
            if closure < 0.0 || (closure > 0.0 && pair[1] > 0.0 && increment == 0.0) {
                return Err(invalid("compression-stack work is not representable"));
            }
            work += increment;
        }
        if !work.is_finite() || work < 0.0 {
            return Err(invalid("compression-stack work is not representable"));
        }
        Ok(if final_pressure < initial {
            -work
        } else {
            work
        })
    }
}

fn sum_finite(values: impl Iterator<Item = f64>, label: &str) -> Result<f64, Refusal> {
    let total: f64 = values.sum();
    if total.is_finite() {
        Ok(total)
    } else {
        Err(invalid(format!("compression-stack {label} overflow")))
    }
}

fn optional_sum(
    values: impl Iterator<Item = Option<f64>>,
    label: &str,
) -> Result<Option<f64>, Refusal> {
    // Missing quantities are never replaced with zero to manufacture a complete assembly total.
    let values: Option<Vec<_>> = values.collect();
    values.map(|v| sum_finite(v.into_iter(), label)).transpose()
}

/// Solve sum_i thickness_i(p) = gap over the common characterized domain and requested direction.
/// Each source branch is piecewise linear in pressure, so the inverse and path work are computed
/// on the union of actual knots, without a convergence heuristic or pressure-grid approximation.
///
/// # Errors
/// Invalid/denied evidence, mismatched initial pressures, no common domain, wrong travel direction,
/// extrapolation, flat/ill-conditioned inverse intervals, nonfinite arithmetic or a refused final
/// conditioning. The entire operation fails before publishing any selections.
pub fn close(input: &Input) -> Result<Report, Refusal> {
    if input.schema != SCHEMA
        || !input.allow_estimates
        || !(1..=32).contains(&input.layers.len())
        || !input.gap_m.is_finite()
        || input.gap_m <= 0.0
        || !input.reference_area_m2.is_finite()
        || input.reference_area_m2 <= 0.0
    {
        return Err(invalid(
            "compression stack needs schema /1, 1..32 layers, positive finite gap/area and explicit allow_estimates for the inverse/path model",
        ));
    }
    let initial = input.layers[0].preparation.pressure_pa;
    let mut ids = BTreeSet::new();
    let mut curves = Vec::with_capacity(input.layers.len());
    let mut domain = [0.0_f64, f64::INFINITY];
    for layer in &input.layers {
        if !text_ok(&layer.id) || !ids.insert(&layer.id) || layer.preparation.pressure_pa != initial
        {
            return Err(invalid(
                "stack layers need unique IDs and the same known initial pressure",
            ));
        }
        // Use the existing full qualification boundary; checking just the thickness numbers would
        // bypass source, conditioning, invariance, original-record and estimate policies.
        let checked = condition(&layer.preparation)?;
        domain[0] = domain[0].max(checked.pressure_range_pa[0]);
        domain[1] = domain[1].min(checked.pressure_range_pa[1]);
        curves.push(Curve {
            points: rows(&layer.preparation)?
                .iter()
                .zip(&layer.preparation.samples)
                .map(|(m, s)| [s.pressure_pa, m.properties["thickness"].value])
                .collect(),
        });
    }
    if domain[0] >= domain[1] {
        return Err(invalid(
            "compression layers have no shared pressure interval",
        ));
    }
    let travel = match input.direction {
        Direction::Loading => [initial, domain[1]],
        Direction::Unloading => [domain[0], initial],
    };
    let mut knots = vec![travel[0], travel[1]];
    for curve in &curves {
        knots.extend(
            curve
                .points
                .iter()
                .map(|p| p[0])
                .filter(|&p| p > travel[0] && p < travel[1]),
        );
    }
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    let totals: Vec<f64> = knots
        .iter()
        .map(|&p| sum_finite(curves.iter().map(|c| c.thickness(p)), "thickness"))
        .collect::<Result<_, _>>()?;
    if totals.windows(2).any(|t| t[1] > t[0]) {
        return Err(invalid(
            "compression-stack thickness lost monotonicity numerically",
        ));
    }
    if input.gap_m > totals[0] || input.gap_m < *totals.last().unwrap() {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "requested gap is outside the common characterized branch in the declared direction; no extrapolation or uncharacterized air gap",
        ));
    }
    // A whole interval at the target thickness determines no unique pressure. Check it BEFORE
    // accepting a matching knot; otherwise the first knot would conceal this nonuniqueness.
    for (p, t) in knots.windows(2).zip(totals.windows(2)) {
        if t[0] >= input.gap_m
            && input.gap_m >= t[1]
            && t[0] - t[1] <= 64.0 * f64::EPSILON * t[0].max(t[1])
        {
            return Err(invalid(format!(
                "stack pressure is nonunique or numerically unobservable on {}..{} Pa",
                p[0], p[1]
            )));
        }
    }
    let (pressure, bracket) = if let Some(i) = totals.iter().position(|&t| t == input.gap_m) {
        (knots[i], [knots[i]; 2])
    } else {
        let hi = totals.partition_point(|&t| t > input.gap_m);
        let lo = hi - 1;
        let fraction = (totals[lo] - input.gap_m) / (totals[lo] - totals[hi]);
        let p = super::lerp(knots[lo], knots[hi], fraction);
        if !p.is_finite() || p < knots[lo] || p > knots[hi] {
            return Err(invalid("compression-stack inverse arithmetic failed"));
        }
        (p, [knots[lo], knots[hi]])
    };
    let input_sha256 = sha256_hex(&serde_json::to_vec(input).map_err(|e| invalid(e.to_string()))?);
    let source_id = format!("compression-stack/{input_sha256}");
    let mut results = Vec::with_capacity(curves.len());
    for (layer, curve) in input.layers.iter().zip(&curves) {
        let mut preparation = layer.preparation.clone();
        if preparation.material.sources.contains_key(&source_id) {
            return Err(invalid(
                "compression-stack receipt cannot replace an existing source",
            ));
        }
        preparation.material.sources.insert(source_id.clone(), Source {
            reference: format!("local:ftx-compression-stack/{input_sha256}"),
            accessed_on: preparation.conditioned_on.clone(),
            note: format!("Model-inferred common pressure for layer {} in a coextensive, fixed-area, rigid-face stack. Retain the complete stack receipt for clearance, area, branch direction and all layer acquisitions. This is not an observed pressure or a garment contact solution.", layer.id),
        });
        preparation.pressure_pa = pressure;
        let conditioned = condition(&preparation)?;
        let normalized = normalize(&conditioned.material)?;
        let mut unavailable = Vec::new();
        let mass = match normalized.require("areal_density", "kg/m2", preparation.allow_estimates) {
            Ok(m) => Some(m * input.reference_area_m2),
            Err(e) => {
                unavailable.push(format!("reference mass: {e}"));
                None
            }
        };
        if mass.is_some_and(|m| !m.is_finite() || m <= 0.0) {
            return Err(invalid(
                "compression-stack reference mass is not representable",
            ));
        }
        let resistance = match normalized.thermal_resistance(preparation.allow_estimates) {
            Ok(r) => Some(r),
            Err(e) => {
                unavailable.push(format!("scalar transport: {e}"));
                None
            }
        };
        let initial_thickness = curve.thickness(initial);
        let thickness = normalized.properties["thickness"].value;
        results.push(LayerResult {
            id: layer.id.clone(),
            initial_thickness_m: initial_thickness,
            thickness_m: thickness,
            closure_m: initial_thickness - thickness,
            external_work_j_m2: curve.work(initial, pressure)?,
            reference_mass_kg: mass,
            material_resistance_m2_k_w: resistance,
            unavailable,
            conditioned,
        });
    }
    let initial_thickness_m = sum_finite(
        results.iter().map(|r| r.initial_thickness_m),
        "initial thickness",
    )?;
    let thickness_m = sum_finite(results.iter().map(|r| r.thickness_m), "final thickness")?;
    let closure_residual_m = thickness_m - input.gap_m;
    if closure_residual_m.abs() > 128.0 * f64::EPSILON * thickness_m.max(input.gap_m) {
        return Err(invalid(
            "conditioned layer thicknesses do not close the requested gap numerically",
        ));
    }
    let normal_force_n = pressure * input.reference_area_m2;
    let work_per_area = sum_finite(results.iter().map(|r| r.external_work_j_m2), "work")?;
    let external_work_j = work_per_area * input.reference_area_m2;
    if !normal_force_n.is_finite()
        || !external_work_j.is_finite()
        || (pressure > 0.0 && normal_force_n == 0.0)
        || (work_per_area != 0.0 && external_work_j == 0.0)
    {
        return Err(invalid("compression-stack force/work is not representable"));
    }
    Ok(Report {
        schema: "ftx-compression-stack-report/1".into(),
        input: input.clone(),
        input_sha256,
        common_pressure_range_pa: domain,
        initial_pressure_pa: initial,
        pressure_pa: pressure,
        bracket_pa: bracket,
        initial_thickness_m,
        thickness_m,
        closure_residual_m,
        normal_force_n,
        external_work_j,
        reference_mass_kg: optional_sum(results.iter().map(|r| r.reference_mass_kg), "total mass")?,
        material_resistance_m2_k_w: optional_sum(results.iter().map(|r| r.material_resistance_m2_k_w), "series material resistance")?,
        layers: results,
        no_claim: "Quasi-static uniform normal pressure between rigid parallel faces over one fixed common reference area, on caller-qualified monotone branches. Central thickness values determine the inverse; observation bounds are retained but not propagated to pressure/work uncertainty. No lateral expansion, buckling, garment contact topology, recovery-time law, branch switching, recoverable/dissipated energy split, heat generation, physical specimen validation or wearer-comfort claim.".into(),
    })
}
