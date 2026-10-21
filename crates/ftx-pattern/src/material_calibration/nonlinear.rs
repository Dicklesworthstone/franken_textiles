//! Identify nonlinear sheet response from measured nominal loads and deformations.
//!
//! The unknowns are piecewise-linear second-Piola resultants at declared interior Green-strain
//! knots, one constant warp/weft coupling and optionally two physical-area coefficients. Endpoints
//! come from FIT observations; neither missing forces nor unmeasured compression branches are
//! invented. The resulting material uses the production tabulated membrane and its domain checks.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, linalg::lstsq, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use super::{Partition, Quality, Residual, Sample};
use crate::{
    material_response::{
        AREA_MODEL, AreaResponse, FITTED_AREA_RANGE_KEY, Interpolation, StressCurve,
        TabulatedMembrane, area_response_basis, physical_area_ratio,
    },
    materials::{Evidence, Form, Material, Quantity, Source},
};

/// Source-bearing nonlinear identification input schema.
pub const SCHEMA: &str = "ftx-nonlinear-swatch-calibration/1";
const MAX_PARAMETERS: usize = 49;
const SCALAR_KEYS: [&str; 4] = [
    "warp_stiffness",
    "weft_stiffness",
    "poisson_warp_weft",
    "shear_stiffness",
];

fn origin_knots() -> [Vec<f64>; 3] {
    std::array::from_fn(|_| vec![0.0])
}

/// Explicit area-model identification. Coefficients are fitted jointly with the base law;
/// the declared J interval only limits admissible observations, never supplies missing data.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaIdentification {
    /// Exactly [`AREA_MODEL`].
    pub model: String,
    /// Positive finite admissible J interval containing one. Publication uses FIT extrema.
    pub area_ratio_range: [f64; 2],
}

/// A bounded nonlinear identification problem in the same swatch frames as linear calibration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Exact specimen with independently characterized mass, thickness, bending and interfaces.
    pub material: Material,
    /// Acquisition and deformation/load reconstruction method.
    pub method: String,
    /// Shared conditions; specimen_id and conditioning are required and never inferred.
    pub conditions: BTreeMap<String, String>,
    /// Explicit analysis date retained with the source observations.
    pub fitted_on: String,
    /// Measured partial nominal forces and complete deformation gradients; maximum 4096 states.
    pub samples: Vec<Sample>,
    /// Interior Green-strain knots for warp, weft and engineering shear. Each ordered vector
    /// includes zero, contains 1..15 values and lies strictly inside the FIT strain envelope.
    /// The two endpoints are derived from observations, so the compiled law cannot extrapolate.
    #[serde(default = "origin_knots")]
    pub interior_knots: [Vec<f64>; 3],
    /// Jointly identify the existing production area response; absent preserves separable fitting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_response: Option<AreaIdentification>,
    /// Permit estimated INPUT observations; fitted coefficients are always estimates separately.
    #[serde(default)]
    pub allow_estimated_observations: bool,
    /// Permit replacing existing scalar/nonlinear equilibrium evidence, retained in this input.
    #[serde(default)]
    pub replace_existing: bool,
    /// Maximum scaled RMS residual independently for fit and held-out observations.
    pub max_rms_scaled: f64,
    /// Maximum absolute scaled component residual independently for each partition.
    pub max_peak_scaled: f64,
}

/// A normal material selection plus complete identification evidence and independent residuals.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// ftx-nonlinear-swatch-calibration-report/1.
    pub schema: String,
    /// Complete input including pre-existing material evidence.
    pub input: Input,
    /// SHA-256 of the serialized input, not a physical-validation claim.
    pub input_sha256: String,
    /// Source-bearing material with the identified production tabulated membrane.
    pub material: Material,
    /// Number of identified knot resultants, constant coupling and optional area coefficients.
    pub parameter_count: usize,
    /// Smallest normalized design Gram Cholesky pivot; deficient excitation refuses.
    pub excitation_pivot: f64,
    /// Actual FIT strain extrema, which are also the compiled law's exact domain endpoints.
    pub fitted_strain_range: [[f64; 2]; 3],
    /// Actual FIT physical J extrema including the undeformed reference, only in area mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fitted_area_ratio_range: Option<[f64; 2]>,
    /// Quality of observations used for identification.
    pub fit: Quality,
    /// Independent held-out quality; absent when none were provided.
    pub check: Option<Quality>,
    /// All nominal force predictions; residuals are present only for observed channels.
    pub residuals: Vec<Residual>,
    /// Other required mechanics remain absent; this calibration never invents them.
    pub missing_mechanics: Vec<String>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

struct Basis {
    knots: [Vec<f64>; 3],
    columns: [Vec<Option<usize>>; 3],
    coupling: usize,
    area_range: Option<[f64; 2]>,
}

impl Basis {
    fn new(
        input: &Input,
        ranges: [[f64; 2]; 3],
        area_range: Option<[f64; 2]>,
    ) -> Result<Self, Refusal> {
        let mut next = 0;
        let mut knots = std::array::from_fn(|_| Vec::new());
        let mut columns = std::array::from_fn(|_| Vec::new());
        for axis in 0..3 {
            let interior = &input.interior_knots[axis];
            let [lo, hi] = ranges[axis];
            if !(lo < 0.0 && hi > 0.0)
                || !(1..=15).contains(&interior.len())
                || !interior.contains(&0.0)
                || interior
                    .iter()
                    .any(|&v| !v.is_finite() || v <= lo || v >= hi)
                || interior.windows(2).any(|v| v[0] >= v[1])
            {
                return Err(invalid(format!(
                    "axis {axis}: characterize negative and positive strain and supply 1..15 increasing interior knots including zero, strictly inside fit extrema"
                )));
            }
            knots[axis].push(lo);
            knots[axis].extend(interior);
            knots[axis].push(hi);
            columns[axis] = knots[axis]
                .iter()
                .map(|&e| {
                    if e == 0.0 {
                        None
                    } else {
                        let column = next;
                        next += 1;
                        Some(column)
                    }
                })
                .collect();
        }
        if next + 1 + usize::from(area_range.is_some()) * 2 > MAX_PARAMETERS {
            return Err(invalid(
                "nonlinear swatch identification supports at most 49 total coefficients; reduce declared knots when identifying area response",
            ));
        }
        Ok(Self {
            knots,
            columns,
            coupling: next,
            area_range,
        })
    }

    fn design(&self, sample: &Sample) -> Result<([f64; 3], [[f64; MAX_PARAMETERS]; 4]), Refusal> {
        let strain = super::design(sample)?.0;
        let [[a, b], [c, d]] = sample.deformation;
        let factors = [[a, 0.0, c, 0.0], [0.0, b, 0.0, d], [b, a, d, c]];
        let mut rows = [[0.0; MAX_PARAMETERS]; 4];
        for axis in 0..3 {
            let knots = &self.knots[axis];
            let e = strain[axis];
            if e < knots[0] || e > knots[knots.len() - 1] {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "{}: axis {axis} outside FIT strain domain; held-out observations cannot extend the fitted law",
                        sample.id
                    ),
                ));
            }
            let j = knots
                .partition_point(|&k| k <= e)
                .saturating_sub(1)
                .min(knots.len() - 2);
            let t = (e - knots[j]) / (knots[j + 1] - knots[j]);
            for (node, weight) in [(j, 1.0 - t), (j + 1, t)] {
                if let Some(column) = self.columns[axis][node] {
                    for (row, factor) in rows.iter_mut().zip(factors[axis]) {
                        row[column] = factor * weight;
                    }
                }
            }
        }
        for (row, value) in
            rows.iter_mut()
                .zip([a * strain[1], b * strain[0], c * strain[1], d * strain[0]])
        {
            row[self.coupling] = value;
        }
        if let Some([lo, hi]) = self.area_range {
            let j = physical_area_ratio(strain)
                .ok_or_else(|| invalid("nonlinear swatch has an invalid physical area metric"))?;
            if j < lo || j > hi {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "{}: outside FIT area-ratio domain; held-out observations cannot extend it",
                        sample.id
                    ),
                ));
            }
            let basis = area_response_basis(strain)
                .ok_or_else(|| invalid("nonlinear area basis arithmetic overflow"))?;
            for (column, response) in basis.iter().enumerate() {
                let [x, y, s] = response.stress;
                for (row, value) in rows.iter_mut().zip([
                    a * x + b * s,
                    a * s + b * y,
                    c * x + d * s,
                    c * s + d * y,
                ]) {
                    row[self.coupling + 1 + column] = value;
                }
            }
        }
        Ok((strain, rows))
    }
}

fn solve(
    rows: &[[f64; MAX_PARAMETERS]],
    target: &[f64],
    count: usize,
) -> Result<([f64; MAX_PARAMETERS], f64), Refusal> {
    if rows.len() < count {
        return Err(invalid(
            "too few observed force components to identify every knot and coupling",
        ));
    }
    let scales: [f64; MAX_PARAMETERS] = std::array::from_fn(|j| {
        if j < count {
            rows.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt()
        } else {
            1.0
        }
    });
    if scales.iter().any(|&s| !s.is_finite() || s <= 0.0) {
        return Err(invalid(
            "swatches do not excite every nonlinear knot and warp/weft coupling",
        ));
    }
    let normalized: Vec<[f64; MAX_PARAMETERS]> = rows
        .iter()
        .map(|r| std::array::from_fn(|j| r[j] / scales[j]))
        .collect();
    let mut l = [[0.0; MAX_PARAMETERS]; MAX_PARAMETERS];
    let mut pivot = 1.0_f64;
    for i in 0..count {
        for j in 0..=i {
            let mut value: f64 = normalized.iter().map(|r| r[i] * r[j]).sum();
            for k in 0..j {
                value -= l[i][k] * l[j][k];
            }
            if i == j {
                if !value.is_finite() || value <= 1e-8 {
                    return Err(invalid(
                        "rank-deficient nonlinear swatches; add independent loading states or reduce declared knots",
                    ));
                }
                pivot = pivot.min(value);
                l[i][j] = value.sqrt();
            } else {
                l[i][j] = value / l[j][j];
            }
        }
    }
    // Unused columns are exactly zero. The existing deterministic SVD leaves their solutions zero.
    let values = lstsq(&normalized, target);
    let values = std::array::from_fn(|j| values[j] / scales[j]);
    if values.iter().any(|v| !v.is_finite()) {
        return Err(invalid("nonlinear identification arithmetic overflow"));
    }
    Ok((values, pivot))
}

fn quality(errors: &[f64], input: &Input, partition: &str) -> Result<Quality, Refusal> {
    let rms_scaled = (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt();
    let peak_scaled = errors.iter().copied().map(f64::abs).fold(0.0, f64::max);
    if errors.is_empty()
        || !rms_scaled.is_finite()
        || !peak_scaled.is_finite()
        || rms_scaled > input.max_rms_scaled
        || peak_scaled > input.max_peak_scaled
    {
        return Err(invalid(format!(
            "{partition} nonlinear swatch residuals exceed declared budget (RMS {rms_scaled}, peak {peak_scaled})"
        )));
    }
    Ok(Quality {
        components: errors.len(),
        rms_scaled,
        peak_scaled,
    })
}

/// Identify a production nonlinear law without converting nominal forces to small-strain moduli.
///
/// # Errors
/// Invalid evidence/geometry/knots, deficient excitation, unstable fitted response, extrapolation,
/// unapproved replacement/estimates or independently excessive fit/held-out residuals.
pub fn calibrate(input: &Input) -> Result<Report, Refusal> {
    input.material.validate()?;
    if input
        .material
        .membrane_response
        .as_ref()
        .is_some_and(|response| response.area_response.is_some())
        && input.area_response.is_none()
    {
        return Err(invalid(
            "separable nonlinear swatch calibration cannot identify or discard an existing area response; use an area-capable characterization model",
        ));
    }
    if input
        .material
        .structure
        .contains_key("membrane_plasticity_model")
    {
        return Err(invalid(
            "nonlinear elastic calibration does not identify permanent membrane history; recharacterize the cyclic law before replacing its elastic response",
        ));
    }
    if input.schema != SCHEMA
        || input.method.trim().is_empty()
        || input.method.len() > 1024
        || input.fitted_on.trim().is_empty()
        || input.samples.is_empty()
        || input.samples.len() > 4096
        || !input.max_rms_scaled.is_finite()
        || input.max_rms_scaled <= 0.0
        || !input.max_peak_scaled.is_finite()
        || input.max_peak_scaled <= 0.0
        || matches!(input.material.form, Form::Fiber | Form::Yarn | Form::Fill)
        || input.conditions.len() > 60
        || input.conditions.iter().any(|(k, v)| {
            k.trim().is_empty() || v.trim().is_empty() || k.len() > 4096 || v.len() > 4096
        })
        || ["specimen_id", "conditioning"]
            .iter()
            .any(|k| !input.conditions.contains_key(*k))
    {
        return Err(invalid(
            "invalid nonlinear swatch schema, specimen/conditioning, protocol, material form, sample count or residual budget",
        ));
    }
    if input
        .material
        .structure
        .get("specimen_id")
        .is_some_and(|s| input.conditions.get("specimen_id") != Some(s))
    {
        return Err(invalid(
            "nonlinear swatches and material name different specimens",
        ));
    }
    if let Some(area) = &input.area_response {
        let [lo, hi] = area.area_ratio_range;
        if area.model != AREA_MODEL
            || !lo.is_finite()
            || !hi.is_finite()
            || lo <= 0.0
            || lo > 1.0
            || hi < 1.0
            || lo >= hi
            || input.conditions.get("specimen") != input.conditions.get("specimen_id")
        {
            return Err(invalid(
                "area identification needs its model, positive J bounds containing one, and explicit matching specimen/specimen_id conditions",
            ));
        }
        if let Some(prior) = input
            .material
            .membrane_response
            .as_ref()
            .and_then(|response| response.area_response.as_ref())
            && prior.conditions.iter().any(|(key, value)| {
                ![
                    "fitted_green_strain_range",
                    FITTED_AREA_RANGE_KEY,
                    "held_out_checks",
                    "fixture_model",
                ]
                .contains(&key.as_str())
                    && input.conditions.get(key) != Some(value)
            })
        {
            return Err(invalid(
                "area identification must retain every existing area specimen and conditioning qualifier",
            ));
        }
    }
    if input.material.membrane_relaxation.is_some() {
        return Err(invalid(
            "recharacterize relaxation explicitly before replacing its equilibrium membrane; old relaxation is not silently reused",
        ));
    }
    if !input.replace_existing
        && (input.material.membrane_response.is_some()
            || SCALAR_KEYS
                .iter()
                .any(|k| input.material.properties.contains_key(*k)))
    {
        return Err(invalid(
            "nonlinear calibration would replace equilibrium evidence; explicit replace_existing is required",
        ));
    }
    let mut ranges = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    let mut area_range = input.area_response.as_ref().map(|_| [1.0_f64, 1.0_f64]);
    // New area identification may replace coefficients, but cannot erase an earlier law's
    // physical support. Keep the original separable fitting path and arithmetic unchanged.
    let prior_law = if input.area_response.is_some() {
        input.material.compiled_membrane(true)?
    } else {
        None
    };
    let prior_domain = if input.area_response.is_some() {
        super::fitted_domain(&input.material)?
    } else {
        None
    };
    let mut ids = BTreeSet::new();
    for sample in &input.samples {
        if sample.id.trim().is_empty()
            || !ids.insert(&sample.id)
            || !input.material.sources.contains_key(&sample.source)
            || (sample.evidence == Evidence::Estimate && !input.allow_estimated_observations)
            || !sample.scale_n_m.is_finite()
            || sample.scale_n_m <= 0.0
            || sample.nominal_n_m.iter().flatten().all(Option::is_none)
            || sample
                .nominal_n_m
                .iter()
                .flatten()
                .flatten()
                .any(|v| !v.is_finite())
        {
            return Err(invalid(format!(
                "{}: invalid sample identity, source, evidence, observed forces or scale",
                sample.id
            )));
        }
        let (strain, _) = super::design(sample)?;
        if prior_domain.is_some_and(|domain| !domain.contains(strain))
            || prior_law.as_ref().is_some_and(|law| !law.contains(strain))
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "{}: area identification cannot widen inherited membrane support",
                    sample.id
                ),
            ));
        }
        if let Some(area) = &input.area_response {
            let j = physical_area_ratio(strain)
                .ok_or_else(|| invalid("area identification needs a positive physical metric"))?;
            if j < area.area_ratio_range[0] || j > area.area_ratio_range[1] {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("{}: outside declared area-ratio domain", sample.id),
                ));
            }
            if sample.partition == Partition::Fit
                && let Some(range) = &mut area_range
            {
                range[0] = range[0].min(j);
                range[1] = range[1].max(j);
            }
        }
        if sample.partition == Partition::Fit {
            for (range, e) in ranges.iter_mut().zip(strain) {
                range[0] = range[0].min(e);
                range[1] = range[1].max(e);
            }
        }
    }
    if area_range.is_some_and(|[lo, hi]| lo >= hi) {
        return Err(invalid(
            "FIT swatches do not excite physical area change; both area coefficients are unidentifiable",
        ));
    }
    let basis = Basis::new(input, ranges, area_range)?;
    let mut rows = Vec::new();
    let mut target = Vec::new();
    for sample in &input.samples {
        let (_, design) = basis.design(sample)?;
        if sample.partition == Partition::Fit {
            for (row, observation) in design.iter().zip(sample.nominal_n_m.iter().flatten()) {
                if let Some(value) = observation {
                    let row = row.map(|v| v / sample.scale_n_m);
                    let value = value / sample.scale_n_m;
                    if !value.is_finite() || row.iter().any(|v| !v.is_finite()) {
                        return Err(invalid("nonlinear swatch weighting arithmetic overflow"));
                    }
                    rows.push(row);
                    target.push(value);
                }
            }
        }
    }
    let count = basis.coupling + 1 + usize::from(area_range.is_some()) * 2;
    let (values, pivot) = solve(&rows, &target, count)?;
    let digest = sha256_hex(&serde_json::to_vec(input).map_err(|e| invalid(e.to_string()))?);
    let source_id = format!("nonlinear-swatch-{digest}");
    let mut material = input.material.clone();
    if material.sources.contains_key(&source_id) {
        return Err(invalid("nonlinear calibration source ID already exists"));
    }
    let source_kind = if area_range.is_some() {
        "local:ftx-nonlinear-swatch-area-calibration"
    } else {
        "local:ftx-nonlinear-swatch-calibration"
    };
    let model_name = if area_range.is_some() {
        "Joint directional and physical-area model"
    } else {
        "Piecewise-linear model"
    };
    material.sources.insert(source_id.clone(), Source {
        reference: format!("{source_kind}/{digest}"),
        accessed_on: input.fitted_on.clone(),
        note: format!("{model_name} identified from nominal force/deformation swatches; complete observations and prior evidence in ftx-nonlinear-swatch-calibration-report/1 input SHA-256 {digest}. Protocol: {}", input.method),
    });
    let mut conditions = input.conditions.clone();
    conditions.insert(
        "fitted_green_strain_range".into(),
        serde_json::to_string(&ranges).map_err(|e| invalid(e.to_string()))?,
    );
    conditions.insert(
        "held_out_checks".into(),
        input
            .samples
            .iter()
            .any(|s| s.partition == Partition::Check)
            .to_string(),
    );
    if let Some(range) = area_range {
        conditions.insert(
            FITTED_AREA_RANGE_KEY.into(),
            serde_json::to_string(&range).map_err(|e| invalid(e.to_string()))?,
        );
    }
    let curve = |axis: usize| {
        StressCurve {
        points: basis.knots[axis].iter().zip(&basis.columns[axis]).map(|(&e, &column)| [e, column.map_or(0.0, |j| values[j])]).collect(),
        source: source_id.clone(), evidence: Evidence::Estimate,
        method: if area_range.is_some() {
            "Weighted joint nominal P=F*S identification of the directional contribution, constant base coupling and physical-area response; origin fixed stress-free, no extrapolation or hysteresis fit."
        } else {
            "Weighted identification from observed components of nominal P=F*S, with separable piecewise-linear S(Green strain); origin fixed stress-free, no extrapolation or hysteresis fit."
        }.into(),
        conditions: conditions.clone(),
    }
    };
    let area_response = area_range.map(|range| {
        let quantity = |value, term: &str| Quantity {
            value,
            unit: "N/m".into(),
            evidence: Evidence::Estimate,
            source: source_id.clone(),
            method: format!("Joint nominal-force identification of the {term} area-energy coefficient using the production constitutive basis; no empirical qualification or confidence interval inferred."),
            conditions: conditions.clone(),
            bounds: None,
        };
        AreaResponse {
            model: AREA_MODEL.into(),
            quadratic: quantity(values[basis.coupling + 1], "quadratic"),
            log_squared: quantity(values[basis.coupling + 2], "log-squared"),
            area_ratio_range: range,
            conditions: conditions.clone(),
        }
    });
    let response = TabulatedMembrane {
        model: crate::material_response::MODEL.into(), interpolation: Interpolation::PiecewiseLinear,
        area_response,
        warp: curve(0), weft: curve(1), shear: curve(2),
        coupling: Quantity {
            value: values[basis.coupling], unit: "N/m".into(), evidence: Evidence::Estimate,
            source: source_id, method: if area_range.is_some() {
                "Jointly identified constant base normal coupling, separate from the physical-area stress derivatives, in the nonlinear nominal-force fit."
            } else {
                "Jointly identified constant dS11/dE22=dS22/dE11 in the nonlinear nominal-force fit."
            }.into(),
            conditions, bounds: None,
        },
    };
    // The production compiler rejects nonmonotone stresses and unstable coupled tangents over the
    // ENTIRE strain box. No clamping, positivity penalty or data-dependent smoothing is inserted.
    let compiled = response.compile(&material.sources, true)?;
    let mut fit_errors = Vec::new();
    let mut check_errors = Vec::new();
    let mut residuals = Vec::new();
    for sample in &input.samples {
        let strain = super::design(sample)?.0;
        let [s11, s22, s12] = compiled.try_evaluate(strain)?.stress;
        let [[a, b], [c, d]] = sample.deformation;
        let prediction = [
            [a * s11 + b * s12, a * s12 + b * s22],
            [c * s11 + d * s12, c * s12 + d * s22],
        ];
        if prediction.iter().flatten().any(|v| !v.is_finite()) {
            return Err(invalid("nonlinear nominal-force prediction overflow"));
        }
        let mut residual = [[None; 2]; 2];
        for i in 0..2 {
            for j in 0..2 {
                if let Some(observed) = sample.nominal_n_m[i][j] {
                    let delta = prediction[i][j] - observed;
                    residual[i][j] = Some(delta);
                    let errors = if sample.partition == Partition::Fit {
                        &mut fit_errors
                    } else {
                        &mut check_errors
                    };
                    errors.push(delta / sample.scale_n_m);
                }
            }
        }
        residuals.push(Residual {
            id: sample.id.clone(),
            partition: sample.partition,
            green_strain: strain,
            predicted_n_m: prediction,
            residual_n_m: residual,
        });
    }
    let fit = quality(&fit_errors, input, "fit")?;
    let check = if check_errors.is_empty() {
        None
    } else {
        Some(quality(&check_errors, input, "held-out")?)
    };
    for key in SCALAR_KEYS {
        material.properties.remove(key);
    }
    material.membrane_response = Some(response);
    material.validate()?;
    let missing_mechanics = material
        .missing_mechanics()
        .into_iter()
        .map(str::to_owned)
        .collect();
    Ok(Report {
        schema: "ftx-nonlinear-swatch-calibration-report/1".into(),
        input: input.clone(),
        input_sha256: digest,
        material,
        parameter_count: count,
        excitation_pivot: pivot,
        fitted_strain_range: ranges,
        fitted_area_ratio_range: area_range,
        fit,
        check,
        residuals,
        missing_mechanics,
    })
}
