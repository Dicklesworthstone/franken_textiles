//! Bounded, deterministic inverse strip experiments. Every objective evaluation solves the
//! actual gripped membrane geometry. CHECK observations never choose coefficients or minima.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, linalg::lstsq, receipt::sha256_hex};
use ftx_pattern::{
    material_calibration::Partition,
    material_catalog::Selection,
    material_response::StrainDomain,
    materials::{Evidence, Form, Material, Quantity, Source},
    textile_import,
};
use serde::{Deserialize, Serialize};

use super::experiment::{Controls, Fixture, SCOPE, State, invalid, solve, solve_law};
use crate::Membrane;

const KEYS: [&str; 4] = [
    "warp_stiffness",
    "weft_stiffness",
    "poisson_warp_weft",
    "shear_stiffness",
];

/// An optional independently observed mid-gauge transverse width.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Width {
    /// Distance between opposite side material points at the reference mid-gauge, m.
    pub value_m: f64,
    /// Positive residual weighting scale in metres, not a confidence interval.
    pub scale_m: f64,
}

/// Total force observation. Grip extension is a boundary displacement, never a local strain.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Globally unique acquisition-row ID, retained verbatim.
    pub id: String,
    /// Change of grip separation divided by the undeformed gauge length.
    pub extension_fraction: f64,
    /// Total measured tensile force in N. Signed instrument noise is retained.
    pub force_n: f64,
    /// Positive force residual weighting scale, N, not inferred uncertainty.
    pub scale_n: f64,
    /// Optional transverse observation; missing width never means no contraction.
    #[serde(default)]
    pub mid_width: Option<Width>,
}

/// A matched specimen/cut/fixture acquisition under one method and conditioning state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    /// Unique strip/cut acquisition identifier.
    pub id: String,
    /// The actual fixture and material-axis orientation.
    pub fixture: Fixture,
    /// Source key in the input material, naming these acquisition observations.
    pub source: String,
    /// Measured, published or explicitly admitted estimated observations.
    pub evidence: Evidence,
    /// Entire strip acquisition participates in FIT or is independently held out as CHECK.
    pub partition: Partition,
    /// Independent quasistatic elastic loading states; no unloading-history inference.
    pub observations: Vec<Observation>,
}

/// Numerical and model-acceptance policy, separate from source evidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Maximum scaled RMS residual, independently for FIT and CHECK.
    pub max_rms_scaled: f64,
    /// Maximum absolute scaled residual, independently for FIT and CHECK.
    pub max_peak_scaled: f64,
    /// Maximum coarse-to-doubled-mesh change divided by each observation's scale.
    pub max_mesh_scaled: f64,
    /// Minimum column-normalized sensitivity Gram pivot for all four parameters.
    pub min_excitation_pivot: f64,
    /// Per-start Levenberg-Marquardt iteration cap, 1..80.
    pub max_iterations: usize,
    /// Aggregate independent forward solves, including rejected trials, sensitivities and checks.
    pub max_forward_solves: usize,
}

/// Reproducible limited-model identification. Published coefficients remain model estimates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// `ftx-strip-calibration/1`.
    pub schema: String,
    /// Matched specimen and all acquisition sources; unrelated properties are preserved.
    pub material: Material,
    /// Actual loading, gauge, grip and conditioning protocol; disclose absent source conditions.
    pub method: String,
    /// Explicit same-specimen test conditions; no guessed rate or humidity.
    pub conditions: BTreeMap<String, String>,
    /// Caller-supplied acquisition/analysis date for deterministic receipts.
    pub fitted_on: String,
    /// Complete strip acquisitions (1..16, at most 64 observed states in total).
    pub experiments: Vec<Experiment>,
    /// Bounds for [warp N/m, weft N/m, major Poisson ratio, shear N/m].
    /// The entire box must be positive-definite; no bound is silently repaired.
    pub parameter_bounds: [[f64; 2]; 4],
    /// Acknowledge the planar ideal-grip elastic model; this does not assert it fits a real knit.
    pub admit_planar_elastic_model: bool,
    /// Explicit permission for estimated input observations, independently of fitted outputs.
    #[serde(default)]
    pub allow_estimated_observations: bool,
    /// Explicit permission to replace existing scalar membrane coefficients.
    #[serde(default)]
    pub replace_existing: bool,
    /// Explicit admission of the resulting estimated coefficients in the returned frozen selection.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Production solver tolerances.
    #[serde(default)]
    pub controls: Controls,
    /// Declared model, numerical-refinement and work limits.
    pub policy: Policy,
}

/// Residual summary with no assumed probability distribution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quality {
    /// Measured scalar channels, not number of physical specimens.
    pub components: usize,
    /// Root-mean-square residual divided by the explicit measurement scales.
    pub rms_scaled: f64,
    /// Largest absolute scaled residual.
    pub peak_scaled: f64,
}

/// Every attempt is retained; an exhausted search is never promoted to a characterized material.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Start {
    /// Uniform normalized starting coordinate in the declared four-dimensional box.
    pub initial_fraction: f64,
    /// Final physical coefficients in the documented order.
    pub parameters: [f64; 4],
    /// Squared scaled FIT residual at this start's final point.
    pub fit_sse: f64,
    /// Actual first-order objective stationarity reached before the iteration limit.
    pub converged: bool,
    /// Objective iterations attempted.
    pub iterations: usize,
}

/// One full predicted experiment state, preserving its observation and partition identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prediction {
    /// Acquisition ID.
    pub experiment: String,
    /// Exact acquisition-row ID.
    pub observation: String,
    /// FIT or independently held-out CHECK.
    pub partition: Partition,
    /// Production equilibrium on the doubled mesh.
    pub state: State,
    /// Predicted minus measured total force, N.
    pub force_residual_n: f64,
    /// Predicted minus measured width, m, only when measured.
    pub width_residual_m: Option<f64>,
}

/// Reviewable candidate, diagnostics and conditional material publication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// `ftx-strip-calibration-report/1`.
    pub schema: String,
    /// Complete source observations, bounds and acceptance policy for exact replay.
    pub input: Input,
    /// SHA-256 identity of the serialized input; not evidence of measurement quality.
    pub input_sha256: String,
    /// Best FIT candidate, whether or not accepted.
    pub parameters: [f64; 4],
    /// Whether every publication gate passed.
    pub accepted: bool,
    /// Specific remaining numerical, identifiability or model-adequacy gates.
    pub reasons: Vec<String>,
    /// Model estimate only when accepted; unknown bending/contact/thermal data remain absent.
    pub material: Option<Material>,
    /// Normal source-locked selection, only when accepted, with the caller's estimate policy.
    pub selection: Option<Selection>,
    /// Exact Rust serialization of the frozen selection; source hashes must survive browser replay.
    pub selection_json: Option<String>,
    /// Three independently initialized deterministic bounded local searches.
    pub starts: Vec<Start>,
    /// Column-normalized local FIT sensitivity pivot; not global uniqueness or confidence.
    pub excitation_pivot: f64,
    /// All attempted equilibrium solves across the complete operation.
    pub forward_solves: usize,
    /// FIT quality re-evaluated with the chosen coefficients on the doubled mesh.
    pub fit: Quality,
    /// CHECK quality, evaluated only after coefficients are fixed, on the doubled mesh.
    pub check: Option<Quality>,
    /// Largest observed doubled-mesh prediction change in measurement-scale units.
    pub mesh_change_peak_scaled: f64,
    /// Every declared coarse/refined experiment re-solved with the final FIT domain enforced.
    /// False when an earlier gate prevented replay or a domain-limited replay failed.
    pub domain_replay_verified: bool,
    /// Local FIT element strain envelope including the explicitly simulated undeformed state.
    pub fitted_green_strain_range: [[f64; 2]; 3],
    /// Prediction for every input observation, in stable input order.
    pub predictions: Vec<Prediction>,
    /// Explicit modeling limits; no empirical garment or global-optimum claim.
    pub model_scope: String,
}

struct Budget {
    used: usize,
    limit: usize,
}

impl Budget {
    fn admit(&mut self) -> Result<(), Refusal> {
        if self.used >= self.limit {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "strip calibration exhausted its complete forward-solve budget; no material is published",
            ));
        }
        self.used += 1;
        Ok(())
    }

    fn solve(
        &mut self,
        fixture: &Fixture,
        parameters: [f64; 4],
        extension: f64,
        controls: &Controls,
    ) -> Result<State, Refusal> {
        self.admit()?;
        solve(fixture, parameters, extension, controls)
    }

    fn replay(
        &mut self,
        fixture: &Fixture,
        parameters: [f64; 4],
        domain: StrainDomain,
        extension: f64,
        controls: &Controls,
    ) -> Result<State, Refusal> {
        self.admit()?;
        let [warp, weft, nu, shear] = parameters;
        solve_law(
            fixture,
            Membrane::orthotropic(warp, weft, nu, shear, 1.0),
            None,
            Some(domain),
            extension,
            controls,
        )
    }
}

fn physical(input: &Input, p: [f64; 4]) -> [f64; 4] {
    std::array::from_fn(|i| {
        input.parameter_bounds[i][0]
            + p[i] * (input.parameter_bounds[i][1] - input.parameter_bounds[i][0])
    })
}

fn validate(input: &Input) -> Result<(), Refusal> {
    input.material.validate()?;
    input.controls.validate()?;
    let policy = &input.policy;
    if input.schema != "ftx-strip-calibration/1"
        || !input.admit_planar_elastic_model
        || input.method.trim().is_empty()
        || input.fitted_on.trim().is_empty()
        || matches!(input.material.form, Form::Fiber | Form::Yarn | Form::Fill)
        || input.experiments.is_empty()
        || input.experiments.len() > 16
        || input.material.membrane_response.is_some()
        || input.material.membrane_relaxation.is_some()
        || input
            .material
            .structure
            .contains_key("membrane_plasticity_model")
    {
        return Err(invalid(
            "strip calibration needs its schema, explicit planar elastic model consent, sheet specimen, protocol/date and 1..16 experiments; existing nonlinear/history laws cannot be erased",
        ));
    }
    if !input.replace_existing
        && KEYS
            .iter()
            .any(|key| input.material.properties.contains_key(*key))
    {
        return Err(invalid(
            "strip calibration would replace existing stiffness observations; explicit replace_existing is required",
        ));
    }
    if input
        .experiments
        .iter()
        .map(|experiment| experiment.observations.len())
        .sum::<usize>()
        > 64
    {
        return Err(invalid(
            "strip calibration admits at most 64 observed states",
        ));
    }
    if [
        policy.max_rms_scaled,
        policy.max_peak_scaled,
        policy.max_mesh_scaled,
        policy.min_excitation_pivot,
    ]
    .iter()
    .any(|v| !v.is_finite() || *v <= 0.0)
        || policy.min_excitation_pivot >= 1.0
        || !(1..=80).contains(&policy.max_iterations)
        || !(1..=32_768).contains(&policy.max_forward_solves)
    {
        return Err(invalid(
            "invalid strip residual, refinement, sensitivity or bounded-work policy",
        ));
    }
    for (i, &[lo, hi]) in input.parameter_bounds.iter().enumerate() {
        if !lo.is_finite()
            || !hi.is_finite()
            || hi <= lo
            || !((hi - lo).is_finite())
            || (i != 2 && lo <= 0.0)
        {
            return Err(invalid(
                "strip parameter bounds must be finite, nonempty and positive for all stiffnesses",
            ));
        }
    }
    let nu = input.parameter_bounds[2][0]
        .abs()
        .max(input.parameter_bounds[2][1].abs());
    if nu * nu * input.parameter_bounds[1][1] / input.parameter_bounds[0][0] >= 1.0 {
        return Err(invalid(
            "the entire declared orthotropic parameter box must be positive-definite",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut acquisition_ids = BTreeSet::new();
    let mut fit_channels = 0;
    for experiment in &input.experiments {
        experiment.fixture.validate()?;
        if experiment.fixture.cells.iter().any(|n| *n > 16)
            || experiment.fixture.cells.iter().product::<usize>() > 128
        {
            return Err(invalid(
                "inverse strip meshes must permit doubling within the 512-cell forward limit",
            ));
        }
        if experiment.id.trim().is_empty()
            || !acquisition_ids.insert(&experiment.id)
            || !input.material.sources.contains_key(&experiment.source)
            || experiment.observations.is_empty()
            || (experiment.evidence == Evidence::Estimate && !input.allow_estimated_observations)
        {
            return Err(invalid(
                "strip acquisition needs a unique ID, retained source, observations and admitted evidence",
            ));
        }
        let mut extensions = BTreeSet::new();
        for point in &experiment.observations {
            let extension_bits = if point.extension_fraction == 0.0 {
                0.0_f64.to_bits()
            } else {
                point.extension_fraction.to_bits()
            };
            if point.id.trim().is_empty()
                || !ids.insert(&point.id)
                || !point.extension_fraction.is_finite()
                || !(0.0..=2.0).contains(&point.extension_fraction)
                || !extensions.insert(extension_bits)
                || !point.force_n.is_finite()
                || !point.scale_n.is_finite()
                || point.scale_n <= 0.0
                || point.mid_width.as_ref().is_some_and(|w| {
                    !w.value_m.is_finite()
                        || w.value_m <= 0.0
                        || !w.scale_m.is_finite()
                        || w.scale_m <= 0.0
                })
            {
                return Err(invalid(
                    "strip observation needs a unique ID/extension, finite force and positive residual scales/observed width",
                ));
            }
            if experiment.partition == Partition::Fit {
                fit_channels += 1 + usize::from(point.mid_width.is_some());
            }
        }
    }
    if fit_channels < 4 {
        return Err(invalid(
            "strip calibration requires at least four measured FIT channels",
        ));
    }
    for fit in input
        .experiments
        .iter()
        .filter(|e| e.partition == Partition::Fit)
    {
        for check in input
            .experiments
            .iter()
            .filter(|e| e.partition == Partition::Check)
        {
            let fit_source = &input.material.sources[&fit.source].reference;
            let check_source = &input.material.sources[&check.source].reference;
            // Reversing both material axes leaves an orthotropic sheet and this acquisition
            // unchanged; a sign flip cannot create independent validation evidence.
            let same_direction = fit.fixture.warp_axis == check.fixture.warp_axis
                || fit.fixture.warp_axis == check.fixture.warp_axis.map(|v| -v);
            if fit_source == check_source
                && fit.fixture.gauge_length_m == check.fixture.gauge_length_m
                && fit.fixture.width_m == check.fixture.width_m
                && same_direction
            {
                return Err(invalid(
                    "FIT and CHECK reuse the same source acquisition, fixture and direction; renamed rows or changed meshes do not establish independent held-out evidence",
                ));
            }
        }
    }
    Ok(())
}

struct Evaluation {
    residuals: Vec<f64>,
    sse: f64,
}

fn evaluate(input: &Input, p: [f64; 4], budget: &mut Budget) -> Result<Evaluation, Refusal> {
    let mut residuals = Vec::new();
    let parameters = physical(input, p);
    for experiment in input
        .experiments
        .iter()
        .filter(|e| e.partition == Partition::Fit)
    {
        for point in &experiment.observations {
            let state = budget.solve(
                &experiment.fixture,
                parameters,
                point.extension_fraction,
                &input.controls,
            )?;
            residuals.push((state.force_n - point.force_n) / point.scale_n);
            if let Some(width) = &point.mid_width {
                residuals.push((state.mid_width_m - width.value_m) / width.scale_m);
            }
        }
    }
    let sse = residuals.iter().map(|v| v * v).sum::<f64>();
    if !sse.is_finite() {
        return Err(invalid("strip objective overflow"));
    }
    Ok(Evaluation { residuals, sse })
}

fn jacobian(input: &Input, p: [f64; 4], budget: &mut Budget) -> Result<Vec<[f64; 4]>, Refusal> {
    let mut rows = Vec::new();
    for j in 0..4 {
        let mut lo = p;
        let mut hi = p;
        lo[j] = (p[j] - 1e-4).max(0.0);
        hi[j] = (p[j] + 1e-4).min(1.0);
        let left = evaluate(input, lo, budget)?;
        let right = evaluate(input, hi, budget)?;
        if rows.is_empty() {
            rows = vec![[0.0; 4]; left.residuals.len()];
        }
        for ((row, a), b) in rows.iter_mut().zip(left.residuals).zip(right.residuals) {
            row[j] = (b - a) / (hi[j] - lo[j]);
        }
    }
    if rows.iter().flatten().any(|v| !v.is_finite()) {
        return Err(invalid("strip force sensitivity overflow"));
    }
    Ok(rows)
}

fn norms(j: &[[f64; 4]]) -> [f64; 4] {
    std::array::from_fn(|k| j.iter().map(|row| row[k] * row[k]).sum::<f64>().sqrt())
}

fn gradient(j: &[[f64; 4]], e: &Evaluation) -> f64 {
    let scales = norms(j);
    (0..4)
        .map(|k| {
            let value = j
                .iter()
                .zip(&e.residuals)
                .map(|(row, r)| row[k] * r)
                .sum::<f64>();
            if scales[k] == 0.0 {
                0.0
            } else {
                value.abs() / (scales[k] * (1.0 + e.sse.sqrt()))
            }
        })
        .fold(0.0, f64::max)
}

fn excitation(j: &[[f64; 4]]) -> f64 {
    let scales = norms(j);
    if scales.iter().any(|s| !s.is_finite() || *s <= 1e-10) {
        return 0.0;
    }
    let mut lower = [[0.0; 4]; 4];
    let mut pivot = 1.0_f64;
    for i in 0..4 {
        for k in 0..=i {
            let dot = j
                .iter()
                .map(|row| (row[i] / scales[i]) * (row[k] / scales[k]))
                .sum::<f64>();
            let value = dot - (0..k).map(|n| lower[i][n] * lower[k][n]).sum::<f64>();
            if i == k {
                if !value.is_finite() || value <= 0.0 {
                    return 0.0;
                }
                lower[i][k] = value.sqrt();
                pivot = pivot.min(value);
            } else {
                lower[i][k] = value / lower[k][k];
            }
        }
    }
    pivot
}

fn search(input: &Input, initial: f64, budget: &mut Budget) -> Result<(Start, [f64; 4]), Refusal> {
    let mut p = [initial; 4];
    let mut e = evaluate(input, p, budget)?;
    let mut lambda = 1e-3_f64;
    let mut converged = false;
    let mut iterations = 0;
    for _ in 0..input.policy.max_iterations {
        iterations += 1;
        let j = jacobian(input, p, budget)?;
        if gradient(&j, &e) < 1e-6 {
            converged = true;
            break;
        }
        let scales = norms(&j);
        if scales.iter().any(|s| *s <= 1e-10) {
            break;
        }
        let mut accepted = false;
        for _ in 0..10 {
            let mut rows: Vec<[f64; 4]> = j
                .iter()
                .map(|row| std::array::from_fn(|k| row[k] / scales[k]))
                .collect();
            let mut target: Vec<f64> = e.residuals.iter().map(|r| -r).collect();
            for k in 0..4 {
                let mut row = [0.0; 4];
                row[k] = lambda.sqrt();
                rows.push(row);
                target.push(0.0);
            }
            let scaled = lstsq(&rows, &target);
            let mut step: [f64; 4] = std::array::from_fn(|k| scaled[k] / scales[k]);
            let max = step.iter().map(|v| v.abs()).fold(0.0, f64::max);
            if !max.is_finite() {
                return Err(invalid("strip inverse step overflow"));
            }
            if max > 0.25 {
                step.iter_mut().for_each(|v| *v *= 0.25 / max);
            }
            // Projection is an explicit bounded-search operation, never an input/property repair.
            let candidate = std::array::from_fn(|k| (p[k] + step[k]).clamp(0.0, 1.0));
            let trial = evaluate(input, candidate, budget)?;
            if trial.sse < e.sse {
                p = candidate;
                e = trial;
                lambda = (lambda * 0.2).max(1e-10);
                accepted = true;
                break;
            }
            lambda *= 10.0;
        }
        if !accepted {
            break;
        }
    }
    Ok((
        Start {
            initial_fraction: initial,
            parameters: physical(input, p),
            fit_sse: e.sse,
            converged,
            iterations,
        },
        p,
    ))
}

fn quality(values: &[f64]) -> Quality {
    Quality {
        components: values.len(),
        rms_scaled: (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt(),
        peak_scaled: values.iter().map(|v| v.abs()).fold(0.0, f64::max),
    }
}

/// Identify the same scalar orthotropic law used by garment assembly, including actual grips.
/// Three bounded starts, local excitation, held-out experiments and doubled-mesh checks are
/// publication gates. An inadequate candidate returns diagnostics with no material or selection.
///
/// # Errors
/// Invalid acquisition/source/model input, exhausted aggregate work, or any failed forward solve.
pub fn calibrate(input: &Input) -> Result<Report, Refusal> {
    validate(input)?;
    let mut budget = Budget {
        used: 0,
        limit: input.policy.max_forward_solves,
    };
    let mut starts = Vec::new();
    let mut coordinates = Vec::new();
    for initial in [0.25, 0.5, 0.75] {
        let (start, p) = search(input, initial, &mut budget)?;
        starts.push(start);
        coordinates.push(p);
    }
    let best = (0..starts.len())
        .min_by(|&a, &b| starts[a].fit_sse.total_cmp(&starts[b].fit_sse))
        .unwrap_or(0);
    let p = coordinates[best];
    let parameters = starts[best].parameters;
    let j = jacobian(input, p, &mut budget)?;
    let pivot = excitation(&j);
    let mut reasons = Vec::new();
    if starts.iter().any(|s| !s.converged) {
        reasons.push("not all three bounded starts reached objective stationarity".into());
    }
    if p.iter().any(|v| *v <= 2e-4 || *v >= 1.0 - 2e-4) {
        reasons
            .push("best fit lies at a parameter bound; expand justified bounds and repeat".into());
    }
    if pivot < input.policy.min_excitation_pivot {
        reasons
            .push("FIT forces/widths do not independently identify all four coefficients".into());
    }
    for (i, start) in starts.iter().enumerate() {
        if start.fit_sse <= starts[best].fit_sse + 1e-8 * (1.0 + starts[best].fit_sse)
            && coordinates[i]
                .iter()
                .zip(p)
                .any(|(a, b)| (a - b).abs() > 0.02)
        {
            reasons.push("equivalent FIT minima have materially different coefficients".into());
            break;
        }
    }
    let mut predictions = Vec::new();
    let mut fit_errors = Vec::new();
    let mut check_errors = Vec::new();
    let mut mesh_peak = 0.0_f64;
    // The zero-strain reference is explicitly included by a solved undeformed state per FIT cut.
    let mut ranges = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for experiment in &input.experiments {
        let mut refined = experiment.fixture.clone();
        refined.cells.iter_mut().for_each(|n| *n *= 2);
        if experiment.partition == Partition::Fit {
            let zero = budget.solve(&refined, parameters, 0.0, &input.controls)?;
            for (range, actual) in ranges.iter_mut().zip(zero.green_strain_range) {
                range[0] = range[0].min(actual[0]);
                range[1] = range[1].max(actual[1]);
            }
        }
        for observation in &experiment.observations {
            let coarse = budget.solve(
                &experiment.fixture,
                parameters,
                observation.extension_fraction,
                &input.controls,
            )?;
            let state = budget.solve(
                &refined,
                parameters,
                observation.extension_fraction,
                &input.controls,
            )?;
            mesh_peak = mesh_peak.max((state.force_n - coarse.force_n).abs() / observation.scale_n);
            let error = state.force_n - observation.force_n;
            let width_error = observation
                .mid_width
                .as_ref()
                .map(|w| state.mid_width_m - w.value_m);
            let errors = if experiment.partition == Partition::Fit {
                &mut fit_errors
            } else {
                &mut check_errors
            };
            errors.push(error / observation.scale_n);
            if let Some(width) = &observation.mid_width {
                errors.push((state.mid_width_m - width.value_m) / width.scale_m);
                mesh_peak =
                    mesh_peak.max((state.mid_width_m - coarse.mid_width_m).abs() / width.scale_m);
            }
            if experiment.partition == Partition::Fit {
                for (range, actual) in ranges.iter_mut().zip(state.green_strain_range) {
                    range[0] = range[0].min(actual[0]);
                    range[1] = range[1].max(actual[1]);
                }
            }
            predictions.push(Prediction {
                experiment: experiment.id.clone(),
                observation: observation.id.clone(),
                partition: experiment.partition,
                state,
                force_residual_n: error,
                width_residual_m: width_error,
            });
        }
    }
    let fit = quality(&fit_errors);
    let check = (!check_errors.is_empty()).then(|| quality(&check_errors));
    if predictions
        .iter()
        .filter(|p| p.partition == Partition::Check)
        .any(|prediction| {
            prediction
                .state
                .green_strain_range
                .iter()
                .zip(ranges)
                .any(|(actual, fitted)| actual[0] < fitted[0] || actual[1] > fitted[1])
        })
    {
        reasons.push(
            "held-out CHECK states escape the FIT-only local strain domain; CHECK data cannot enlarge the fitted law's domain".into(),
        );
    }
    if check.is_none() {
        reasons.push(
            "an independent held-out CHECK acquisition is required before publishing a material"
                .into(),
        );
    }
    for (name, q) in [("FIT", Some(&fit)), ("CHECK", check.as_ref())] {
        if let Some(q) = q
            && (!q.rms_scaled.is_finite()
                || !q.peak_scaled.is_finite()
                || q.rms_scaled > input.policy.max_rms_scaled
                || q.peak_scaled > input.policy.max_peak_scaled)
        {
            reasons.push(format!(
                "{name} force/width residuals exceed the declared elastic-model budget"
            ));
        }
    }
    if !mesh_peak.is_finite() || mesh_peak > input.policy.max_mesh_scaled {
        reasons.push(
            "doubling the strip mesh changes predictions beyond the declared numerical budget"
                .into(),
        );
    }
    // A converged unconstrained state alone does not prove that ordinary initialization and
    // Newton trials can reproduce it inside the fitted domain. Exercise the real domain gate;
    // never enlarge the authority with CHECK states, trial states or a numerical tolerance.
    let mut domain_replay_verified = false;
    if reasons.is_empty() {
        let domain = StrainDomain::new(ranges)?;
        'replay: for experiment in &input.experiments {
            for factor in [1, 2] {
                let mut fixture = experiment.fixture.clone();
                fixture.cells.iter_mut().for_each(|n| *n *= factor);
                for observation in &experiment.observations {
                    if let Err(refusal) = budget.replay(
                        &fixture,
                        parameters,
                        domain,
                        observation.extension_fraction,
                        &input.controls,
                    ) {
                        if refusal.code == RefusalCode::BudgetExceeded {
                            return Err(refusal);
                        }
                        reasons.push(format!(
                            "fitted-domain replay failed for {} / {} at mesh factor {factor}: {}; no domain was widened",
                            experiment.id, observation.id, refusal.message,
                        ));
                        break 'replay;
                    }
                }
            }
        }
        domain_replay_verified = reasons.is_empty();
    }
    let digest = sha256_hex(&serde_json::to_vec(input).map_err(invalid)?);
    let material = if reasons.is_empty() {
        Some(publish(input, &digest, parameters, ranges)?)
    } else {
        None
    };
    let selection = material
        .as_ref()
        .map(|material| {
            textile_import::catalog()?
                .resolve(&Selection {
                    id: material.id.clone(),
                    record: Some(material.clone()),
                    allow_estimates: input.allow_estimates,
                    ..Selection::default()
                })
                .map(|m| m.frozen_selection())
        })
        .transpose()?;
    let selection_json = selection
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(invalid)?;
    Ok(Report {
        schema: "ftx-strip-calibration-report/1".into(),
        input: input.clone(),
        input_sha256: digest,
        parameters,
        accepted: material.is_some(),
        reasons,
        material,
        selection,
        selection_json,
        starts,
        excitation_pivot: pivot,
        forward_solves: budget.used,
        fit,
        check,
        mesh_change_peak_scaled: mesh_peak,
        domain_replay_verified,
        fitted_green_strain_range: ranges,
        predictions,
        model_scope: SCOPE.into(),
    })
}

fn publish(
    input: &Input,
    digest: &str,
    parameters: [f64; 4],
    ranges: [[f64; 2]; 3],
) -> Result<Material, Refusal> {
    let mut material = input.material.clone();
    let source = format!("strip-{digest}");
    if material.sources.contains_key(&source) {
        return Err(invalid("strip receipt source identity already exists"));
    }
    material.sources.insert(source.clone(), Source { reference: format!("local:ftx-strip-calibration/{digest}"), accessed_on: input.fitted_on.clone(), note: format!("Inverse production-membrane clamped-strip model; complete acquisitions, assumptions, sensitivity and mesh checks retained in ftx-strip-calibration-report/1 input {digest}. {}", input.method) });
    let mut conditions = input.conditions.clone();
    conditions.insert(
        "fitted_green_strain_range".into(),
        serde_json::to_string(&ranges).map_err(invalid)?,
    );
    conditions.insert("held_out_checks".into(), "true".into());
    conditions.insert("fixture_model".into(), SCOPE.into());
    for (key, value) in KEYS.into_iter().zip(parameters) {
        material.properties.insert(key.into(), Quantity { value, unit: if key == "poisson_warp_weft" { "1" } else { "N/m" }.into(), evidence: Evidence::Estimate, source: source.clone(), method: "Bounded inverse orthotropic Green-strain membrane using clamped strip equilibrium, total grip force and optional transverse width; local sensitivity and held-out doubled-mesh checks. No independent bending or contact identification.".into(), conditions: conditions.clone(), bounds: None });
    }
    material.validate()?;
    Ok(material)
}
