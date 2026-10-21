//! Identify Maxwell spectra from complete planar swatch loading histories.
//!
//! The equilibrium response is supplied independently, never fitted from an instantaneous modulus.
//! Internal lag is integrated exactly for piecewise-linear Green strain, including finite ramps and
//! unloading. Only fit experiments enter the optimizer; held-out histories are replayed afterwards.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use super::{Initialization, MaxwellBranch, Relaxation};
use crate::material_calibration::{Partition, Quality, fitted_domain};
use crate::material_catalog::normalize;
use crate::materials::{Evidence, Form, Material, Quantity, Source};

pub mod comparison;
mod fit;
mod orthotropic;

pub use orthotropic::{Candidate, OrthotropicFitReport, OrthotropicInput, TimeSearchReport};

/// Portable measurement-to-material input schema.
pub const SCHEMA: &str = "ftx-relaxation-calibration/1";

/// One observed endpoint. Missing loads may still define a necessary part of the strain history.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Seconds from the experiment's explicitly initialized state; starts at zero.
    pub time_seconds: f64,
    /// Row-major F, columns in warp/weft or wale/course reference axes.
    pub deformation: [[f64; 2]; 2],
    /// Nominal P, N/m of undeformed width, not Cauchy stress; null means unobserved, never zero.
    pub nominal_n_m: [[Option<f64>; 2]; 2],
    /// Positive scale for the observed residuals in N/m; not automatically a standard deviation.
    pub scale_n_m: f64,
}

/// One independently initialized history at the shared specimen condition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    /// Unique acquisition ID. Use a separate experiment for a different specimen preparation.
    pub id: String,
    /// Source in the input material's map, covering the full acquisition and its conditioning.
    pub source: String,
    /// Origin of the history; synthetic/estimated inputs require explicit admission.
    pub evidence: Evidence,
    /// Entire histories are held out, not isolated force samples from the training acquisition.
    pub partition: Partition,
    /// Known initial internal state, independent of the initialization used for future garments.
    pub initialization: Initialization,
    /// Strictly time-ordered states including ramp/unloading nodes, with optional force observations.
    pub samples: Vec<Sample>,
}

/// Source-bearing calibration problem. No files, network, new material type or solver dependency.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Specimen and independently characterized equilibrium response; other properties may be absent.
    pub material: Material,
    /// Explicit declaration that the supplied scalar/curve response is long-time equilibrium.
    /// A declaration is not experimental proof; preserve the supporting source in the material.
    pub equilibrium_is_long_time: bool,
    /// Permit estimated equilibrium data. This does not admit estimated acquisition histories.
    #[serde(default)]
    pub allow_estimated_equilibrium: bool,
    /// Separately admit estimated/synthetic acquisition data.
    #[serde(default)]
    pub allow_estimated_observations: bool,
    /// Replace a previous relaxation law only with explicit consent; original remains in the report.
    #[serde(default)]
    pub replace_existing: bool,
    /// Protocol, including how nominal loads, F, equilibrium and initial states were obtained.
    pub method: String,
    /// Shared condition record; different moisture/temperature/finish states need separate fits.
    pub conditions: BTreeMap<String, String>,
    /// Source for the supplied applicability domain, in the material source map.
    pub domain_source: String,
    /// Explicit strain applicability, not inferred from the smallest observed error.
    pub strain_range: [[f64; 2]; 3],
    /// Initialization declaration for future simulations, NOT a reset of acquisition histories.
    pub initialization: Initialization,
    /// Analysis date; caller supplied to keep replay deterministic.
    pub fitted_on: String,
    /// One to three ordered, non-overlapping search intervals in seconds. They are NOT error bars.
    /// Omit or leave empty when fitting orthotropic tensors at explicit candidate times instead.
    #[serde(default)]
    pub time_bounds_seconds: Vec<[f64; 2]>,
    /// Optional independent directional tensors at fixed candidate times. Mutually exclusive with
    /// time_bounds_seconds; omission preserves the original proportional identification path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orthotropic: Option<OrthotropicInput>,
    /// Fit and held-out acquisitions, at most 32 histories and 2048 states in total.
    pub experiments: Vec<Experiment>,
    /// Maximum RMS of scaled residuals in EACH partition, not a confidence threshold.
    pub max_rms_scaled: f64,
    /// Maximum absolute scaled residual in EACH partition.
    pub max_peak_scaled: f64,
}

/// Every predicted endpoint is retained, including nodes with no observed force.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prediction {
    /// Acquisition identity.
    pub experiment: String,
    /// Fit or held-out acquisition.
    pub partition: Partition,
    /// Elapsed acquisition time in seconds.
    pub time_seconds: f64,
    /// Material-frame Green strain [E11,E22,2E12].
    pub strain: [f64; 3],
    /// Predicted first Piola resultants, N/m of undeformed width.
    pub nominal_n_m: [[f64; 2]; 2],
    /// Predicted minus observed, preserving unobserved components as null.
    pub residual_n_m: [[Option<f64>; 2]; 2],
}

/// Ready for the existing material resolver; fitted quantities always remain estimates.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// `ftx-relaxation-calibration-report/1`.
    pub schema: String,
    /// Original evidence, protocol, old properties, search bounds and partition choices.
    pub input: Input,
    /// Identity of serialized input, not proof of truthful observations.
    pub input_sha256: String,
    /// Ordinary material with fitted membrane_relaxation and a source pointing to this receipt.
    pub material: Material,
    /// Training residuals only.
    pub fit: Quality,
    /// Independent-history residuals, or null when no check acquisition was provided.
    pub check: Option<Quality>,
    /// Source-ordered force predictions and residuals for all acquisitions.
    pub predictions: Vec<Prediction>,
    /// Smallest pivot of the column-normalized force Jacobian Gram matrix, not uncertainty.
    pub excitation_pivot: f64,
    /// Proportional mode: scaled-force sensitivity to each log ratio then each log time. Orthotropic
    /// mode: sensitivity to each N/m coefficient [warp,weft,coupling,shear] in candidate order.
    /// These are numerical excitation diagnostics, not standard errors.
    pub parameter_sensitivity: Vec<f64>,
    /// Deterministic starts tried; multiple starts do not establish a unique global optimum.
    pub starts_attempted: usize,
    /// Candidate tensors, explicit physical constraints and threshold decisions for directional
    /// identification. Omitted for the original proportional estimator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orthotropic_fit: Option<OrthotropicFitReport>,
    /// Physical properties still absent; no bending, mass or contact values are invented.
    pub missing_mechanics: Vec<String>,
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

struct Point {
    time: f64,
    f: [[f64; 2]; 2],
    strain: [f64; 3],
    equilibrium: [f64; 4],
    observed: [Option<f64>; 4],
    scale: f64,
}

struct History {
    initialization: Initialization,
    points: Vec<Point>,
    partition: Partition,
}

fn piola(f: [[f64; 2]; 2], s: [f64; 3]) -> [f64; 4] {
    [
        f[0][0] * s[0] + f[0][1] * s[2],
        f[0][0] * s[2] + f[0][1] * s[1],
        f[1][0] * s[0] + f[1][1] * s[2],
        f[1][0] * s[2] + f[1][1] * s[1],
    ]
}

fn green(f: [[f64; 2]; 2]) -> Result<[f64; 3], Refusal> {
    let [[a, b], [c, d]] = f;
    let determinant = a * d - b * c;
    let norm = a * a + b * b + c * c + d * d;
    if !norm.is_finite() || !determinant.is_finite() || determinant <= 1e-12 * norm {
        return Err(bad("singular, inverted or non-finite swatch deformation"));
    }
    Ok([
        0.5 * (a * a + c * c - 1.),
        0.5 * (b * b + d * d - 1.),
        a * b + c * d,
    ])
}

fn datum(value: f64, unit: &str, source: &str) -> Quantity {
    Quantity {
        value, unit: unit.into(), source: source.into(), evidence: Evidence::Estimate,
        method: "Model identification from complete timed nominal-force histories; not an empirical constant or confidence bound".into(),
        conditions: BTreeMap::new(), bounds: None,
    }
}

fn law(input: &Input, ratios: &[f64], times: &[f64], source: &str) -> Relaxation {
    Relaxation {
        model: "green_maxwell/1".into(),
        initialization: input.initialization,
        strain_range: input.strain_range,
        source: input.domain_source.clone(),
        method: format!(
            "{}; proportional Maxwell fit with fixed equilibrium and exact piecewise-linear Green-strain integration",
            input.method
        ),
        conditions: input.conditions.clone(),
        orthotropic_branches: Vec::new(),
        branches: ratios
            .iter()
            .zip(times)
            .enumerate()
            .map(|(i, (&r, &t))| MaxwellBranch {
                id: format!("fitted-{}", i + 1),
                stiffness_ratio: datum(r, "1", source),
                relaxation_time: datum(t, "s", source),
            })
            .collect(),
    }
}

fn prepare(input: &Input, include_check: bool) -> Result<(Vec<History>, [[f64; 3]; 3]), Refusal> {
    input.material.validate()?;
    if input
        .material
        .structure
        .contains_key("membrane_plasticity_model")
    {
        return Err(bad(
            "Maxwell calibration does not identify permanent membrane history; a recoverable spectrum cannot silently retain the irreversible law",
        ));
    }
    if input.schema != SCHEMA
        || !input.equilibrium_is_long_time
        || matches!(input.material.form, Form::Fiber | Form::Yarn | Form::Fill)
        || input.method.trim().is_empty()
        || input.fitted_on.trim().is_empty()
        || (input.orthotropic.is_none() && !(1..=3).contains(&input.time_bounds_seconds.len()))
        || (input.orthotropic.is_some() && !input.time_bounds_seconds.is_empty())
        || !(1..=32).contains(&input.experiments.len())
        || input
            .experiments
            .iter()
            .map(|e| e.samples.len())
            .sum::<usize>()
            > 2048
        || !input.max_rms_scaled.is_finite()
        || input.max_rms_scaled <= 0.
        || !input.max_peak_scaled.is_finite()
        || input.max_peak_scaled <= 0.
    {
        return Err(bad(
            "invalid relaxation calibration schema, fixed-equilibrium declaration, protocol or budget",
        ));
    }
    if input.material.membrane_relaxation.is_some() && !input.replace_existing {
        return Err(bad(
            "existing relaxation evidence requires explicit replace_existing",
        ));
    }
    let mut upper = 0.;
    for &[lo, hi] in &input.time_bounds_seconds {
        if !lo.is_finite() || !hi.is_finite() || lo <= upper || hi <= lo {
            return Err(bad(
                "time search intervals must be positive, ordered and non-overlapping",
            ));
        }
        upper = hi;
    }
    // Resolve only the equilibrium consumer: absent mass, bending and friction do not block fitting.
    let material = normalize(&input.material)?;
    let equilibrium_domain = fitted_domain(&material)?;
    let nonlinear = material.compiled_membrane(input.allow_estimated_equilibrium)?;
    let q = if let Some(curve) = &nonlinear {
        let [a, b, c, g] = curve.origin_stiffness();
        [[a, c, 0.], [c, b, 0.], [0., 0., g]]
    } else {
        let mut v = [0.; 4];
        for (i, (key, unit)) in [
            ("warp_stiffness", "N/m"),
            ("weft_stiffness", "N/m"),
            ("poisson_warp_weft", "1"),
            ("shear_stiffness", "N/m"),
        ]
        .iter()
        .enumerate()
        {
            v[i] = material.require(key, unit, input.allow_estimated_equilibrium)?;
        }
        if v[0] <= 0. || v[1] <= 0. || v[3] <= 0. {
            return Err(bad("equilibrium sheet stiffness must be positive"));
        }
        let d = 1. - v[2] * (v[2] * v[1] / v[0]);
        [
            [v[0] / d, v[2] * v[1] / d, 0.],
            [v[2] * v[1] / d, v[1] / d, 0.],
            [0., 0., v[3]],
        ]
    };
    // Reuse the consuming law's source, domain and positive-definiteness validation.
    let domain =
        law(input, &[1.], &[1.], &input.domain_source).compile(&material.sources, true, q)?;
    let mut ids = BTreeSet::new();
    let mut histories = Vec::new();
    let mut fit_components = 0;
    for e in &input.experiments {
        if e.id.trim().is_empty()
            || !ids.insert(&e.id)
            || !material.sources.contains_key(&e.source)
            || (e.evidence == Evidence::Estimate && !input.allow_estimated_observations)
            || e.samples.len() < 2
        {
            return Err(bad(
                "invalid experiment identity, source, admitted evidence or history length",
            ));
        }
        // Model comparison fixes every FIT decision before evaluating held-out constitutive
        // states. A CHECK domain refusal must reject that candidate, not alter the FIT frontier.
        if !include_check && e.partition == Partition::Check {
            continue;
        }
        let mut points = Vec::new();
        let mut last = -1.;
        let mut components = 0;
        for (i, s) in e.samples.iter().enumerate() {
            if !s.time_seconds.is_finite()
                || (i == 0 && s.time_seconds != 0.)
                || s.time_seconds <= last
                || !s.scale_n_m.is_finite()
                || s.scale_n_m <= 0.
                || s.nominal_n_m
                    .iter()
                    .flatten()
                    .flatten()
                    .any(|v| !v.is_finite())
            {
                return Err(bad(format!(
                    "{}: invalid time order, force or residual scale",
                    e.id
                )));
            }
            last = s.time_seconds;
            let strain = green(s.deformation)?;
            if equilibrium_domain.is_some_and(|domain| !domain.contains(strain)) {
                return Err(bad(format!(
                    "{}: history node {i} outside fitted equilibrium strain domain",
                    e.id
                )));
            }
            if !domain.contains(strain) {
                return Err(bad("observed strain outside declared domain"));
            }
            let stress = match &nonlinear {
                Some(curve) => curve.try_evaluate(strain)?.stress,
                None => super::mul(&q, strain),
            };
            let equilibrium = piola(s.deformation, stress);
            if equilibrium.iter().any(|v| !v.is_finite()) {
                return Err(bad("equilibrium force overflow"));
            }
            let observed = std::array::from_fn(|j| s.nominal_n_m[j / 2][j % 2]);
            components += observed.iter().flatten().count();
            points.push(Point {
                time: last,
                strain,
                f: s.deformation,
                equilibrium,
                observed,
                scale: s.scale_n_m,
            });
        }
        if components == 0 {
            return Err(bad("each acquisition needs at least one force observation"));
        }
        if e.partition == Partition::Fit {
            fit_components += components;
        }
        histories.push(History {
            initialization: e.initialization,
            points,
            partition: e.partition,
        });
    }
    if fit_components <= 2 * input.time_bounds_seconds.len() {
        return Err(bad(
            "insufficient independent fit observations for the requested spectrum",
        ));
    }
    Ok((histories, q))
}

fn quality(
    residuals: &[f64],
    input: &Input,
    partition: &str,
    enforce_budget: bool,
) -> Result<Quality, Refusal> {
    let rms = (residuals.iter().map(|r| r * r).sum::<f64>() / residuals.len() as f64).sqrt();
    let peak = residuals.iter().map(|r| r.abs()).fold(0., f64::max);
    if !rms.is_finite()
        || !peak.is_finite()
        || (enforce_budget && (rms > input.max_rms_scaled || peak > input.max_peak_scaled))
    {
        return Err(bad(format!(
            "{partition} relaxation residual budget exceeded: RMS {rms}, peak {peak}"
        )));
    }
    Ok(Quality {
        components: residuals.len(),
        rms_scaled: rms,
        peak_scaled: peak,
    })
}

fn residuals(
    input: &Input,
    histories: &[History],
    predict: impl FnMut(&History) -> Result<Vec<[f64; 4]>, Refusal>,
) -> Result<(Vec<Prediction>, Quality, Option<Quality>), Refusal> {
    evaluate_residuals(input, histories, true, true, predict)
}

// Comparison separates FIT decisions from CHECK replay. Unenforced budgets retain finite residuals
// for an explicit rejected candidate; they never qualify that candidate for a material selection.
fn evaluate_residuals(
    input: &Input,
    histories: &[History],
    include_check: bool,
    enforce_budget: bool,
    mut predict: impl FnMut(&History) -> Result<Vec<[f64; 4]>, Refusal>,
) -> Result<(Vec<Prediction>, Quality, Option<Quality>), Refusal> {
    let mut predictions = Vec::new();
    let mut fit_errors = Vec::new();
    let mut check_errors = Vec::new();
    for (e, history) in input
        .experiments
        .iter()
        .filter(|e| include_check || e.partition == Partition::Fit)
        .zip(
            histories
                .iter()
                .filter(|h| include_check || h.partition == Partition::Fit),
        )
    {
        let predicted = predict(history)?;
        for (point, values) in history.points.iter().zip(predicted) {
            let mut residual = [[None; 2]; 2];
            for j in 0..4 {
                if let Some(observed) = point.observed[j] {
                    let delta = values[j] - observed;
                    residual[j / 2][j % 2] = Some(delta);
                    let errors = if e.partition == Partition::Fit {
                        &mut fit_errors
                    } else {
                        &mut check_errors
                    };
                    errors.push(delta / point.scale);
                }
            }
            predictions.push(Prediction {
                experiment: e.id.clone(),
                partition: e.partition,
                time_seconds: point.time,
                strain: point.strain,
                nominal_n_m: [[values[0], values[1]], [values[2], values[3]]],
                residual_n_m: residual,
            });
        }
    }
    let fit_quality = quality(&fit_errors, input, "fit", enforce_budget)?;
    let check = if check_errors.is_empty() {
        None
    } else {
        Some(quality(&check_errors, input, "held-out", enforce_budget)?)
    };
    Ok((predictions, fit_quality, check))
}

fn receipt(input: &Input) -> Result<(Material, String, String), Refusal> {
    let bytes = serde_json::to_vec(input).map_err(|e| bad(e.to_string()))?;
    let digest = sha256_hex(&bytes);
    let source = format!("relaxation-fit-{digest}");
    let mut material = input.material.clone();
    if material.sources.contains_key(&source) {
        return Err(bad("calibration source identity already exists"));
    }
    material.sources.insert(source.clone(), Source {
        reference: format!("local:ftx-relaxation-calibration/{digest}"), accessed_on: input.fitted_on.clone(),
        note: "Fitted model estimates. Complete acquisitions and previous material evidence are retained in the calibration report; preserve that report with the selection.".into(),
    });
    Ok((material, source, digest))
}

/// Identify positive proportional ratios/times or independent orthotropic tensors at fixed times,
/// producing the existing material law. The supplied equilibrium is fixed, and all histories must
/// use the same specimen conditions. This does not identify yarn structure, plastic set or strain
/// uncertainty; directional candidate times are supplied, not optimized automatically.
///
/// # Errors
/// Invalid evidence/history/domain, missing equilibrium response, unexcited/ill-conditioned spectrum,
/// boundary-limited or unconverged optimization, or excessive training/held-out force residuals.
pub fn calibrate(input: &Input) -> Result<Report, Refusal> {
    if let Some(options) = &input.orthotropic {
        return orthotropic::calibrate(input, options);
    }
    let (histories, q) = prepare(input, true)?;
    let fit = fit::solve(&histories, q, &input.time_bounds_seconds)?;
    let n = input.time_bounds_seconds.len();
    let ratios: Vec<_> = fit.parameters[..n]
        .iter()
        .map(|v| ftx_core::det::exp(*v))
        .collect();
    let times: Vec<_> = fit.parameters[3..3 + n]
        .iter()
        .map(|v| ftx_core::det::exp(*v))
        .collect();
    let (predictions, fit_quality, check) = residuals(input, &histories, |history| {
        Ok(fit::predict(history, q, &ratios, &times)?
            .into_iter()
            .map(|p| p.force)
            .collect())
    })?;
    let (mut material, source, digest) = receipt(input)?;
    let fitted = law(input, &ratios, &times, &source);
    fitted.compile(&material.sources, true, q)?;
    material.membrane_relaxation = Some(fitted);
    material.validate()?;
    let missing_mechanics = material
        .missing_mechanics()
        .into_iter()
        .map(str::to_owned)
        .collect();
    Ok(Report {
        schema: "ftx-relaxation-calibration-report/1".into(),
        input: input.clone(),
        input_sha256: digest,
        material,
        fit: fit_quality,
        check,
        predictions,
        excitation_pivot: fit.pivot,
        parameter_sensitivity: fit.sensitivity,
        starts_attempted: fit.starts,
        orthotropic_fit: None,
        missing_mechanics,
    })
}
