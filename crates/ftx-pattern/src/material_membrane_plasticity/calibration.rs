//! Identify only yield and hardening of the existing permanent membrane law. Elasticity and
//! strain/accumulation limits are independently supplied. Every trial replays complete FIT paths
//! from their explicitly pristine reference; CHECK paths cannot choose or alter parameters.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, linalg::lstsq, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use super::{AXES, ELASTIC, MODEL, PlasticMembrane};
use crate::material_calibration::{Partition, Quality, fitted_domain};
use crate::material_catalog::normalize;
use crate::material_response::StrainDomain;
use crate::materials::{Evidence, Form, Material, Quantity, Source};

/// Portable acquisition and identification request.
pub const SCHEMA: &str = "ftx-membrane-plasticity-calibration/1";
const MAX_HISTORY_STEPS: usize = 2_000_000;
const STARTS: usize = 4;
const MAX_ITERATIONS: usize = 80;

/// Acquisition starts at the undeformed, unworked reference; no unobserved forming is inferred.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Initialization {
    /// Zero permanent and accumulated Green strain before the first recorded load.
    Pristine,
}

/// One recorded incremental state in the specimen's material frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Strictly increasing elapsed seconds, starting at zero. The fitted law is rate independent.
    pub time_seconds: f64,
    /// Raw Green strain [E11, E22, 2 E12], not engineering extension or a deformation gradient.
    pub green_strain: [f64; 3],
    /// Work-conjugate [S11, S22, S12] in N/m of reference sheet. Missing channels remain null.
    pub second_piola_n_m: [Option<f64>; 3],
    /// Positive residual weighting scale in N/m, not automatically a standard deviation.
    pub scale_n_m: f64,
}

/// A complete independently initialized acquisition, never a partition of one loading path.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    /// Unique acquisition identity.
    pub id: String,
    /// Source key covering this history, the declared specimen and conditioning.
    pub source: String,
    /// Synthetic or estimated acquisitions require separate explicit admission.
    pub evidence: Evidence,
    /// Entire history is FIT or CHECK.
    pub partition: Partition,
    /// Mandatory initial-state declaration.
    pub initialization: Initialization,
    /// Include every retained loading/unloading node, even when no load channel was observed.
    pub samples: Vec<Sample>,
}

/// Six identified coefficients in warp, weft and engineering-Green-shear order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    /// Positive directional yield resultants in N/m.
    pub yield_n_m: [f64; 3],
    /// Nonnegative directional isotropic hardening coefficients in N/m.
    pub hardening_n_m: [f64; 3],
}

/// Original source-bearing problem. The six domain quantities remain properties of `material`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Independently characterized four elastic and six strain/accumulation limit quantities.
    /// Other mass, contact, thermal or bending properties are retained, never inferred.
    pub material: Material,
    /// Admit estimated elastic/domain observations, separately from acquired load histories.
    #[serde(default)]
    pub allow_estimated_material: bool,
    /// Admit estimated or explicitly synthetic histories.
    #[serde(default)]
    pub allow_estimated_observations: bool,
    /// Required if a permanent-law selector or any previous yield/hardening quantity exists.
    #[serde(default)]
    pub replace_existing: bool,
    /// Acquisition/reduction protocol, including reference frame, loading and initialization.
    pub method: String,
    /// Must include specimen_id and conditioning, matching all elastic/domain observations.
    pub conditions: BTreeMap<String, String>,
    /// Explicit analysis date; no wall-clock value is inserted.
    pub fitted_on: String,
    /// Finite, strictly ordered positive bounds, not confidence intervals.
    pub yield_bounds_n_m: [[f64; 2]; 3],
    /// Finite, strictly ordered nonnegative bounds; explicit zero permits perfect plasticity.
    pub hardening_bounds_n_m: [[f64; 2]; 3],
    /// At most 32 complete histories and 2048 recorded nodes in total.
    pub experiments: Vec<Experiment>,
    /// All forward objective/probe/replay calls share this limit, in 1..=4096.
    pub max_evaluations: usize,
    /// Separate FIT and CHECK RMS scaled-residual acceptance threshold.
    pub max_rms_scaled: f64,
    /// Separate FIT and CHECK maximum absolute scaled-residual acceptance threshold.
    pub max_peak_scaled: f64,
}

/// Parameter identification and replay decision; numerical convergence is not empirical validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    /// Completed identifiable FIT with admitted residuals; CHECK, when observed, also passes.
    Accepted,
    /// Unexcited/indistinguishable coefficients, equivalent solutions, or incomplete optimization.
    Ambiguous,
    /// A shared work limit prevented complete identification and replay.
    BudgetExhausted,
    /// No admissible candidate or a selected model fails FIT/CHECK replay or residual thresholds.
    Rejected,
}

/// Source-ordered prediction of a complete recorded path, including unobserved load nodes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prediction {
    /// Original acquisition ID.
    pub experiment: String,
    /// Original whole-history partition.
    pub partition: Partition,
    /// Original elapsed time; no unseen intermediate path or rate law is synthesized.
    pub time_seconds: f64,
    /// Recorded material-frame strain.
    pub green_strain: [f64; 3],
    /// Predicted accepted second-Piola sheet resultants.
    pub second_piola_n_m: [f64; 3],
    /// Predicted minus observed; unobserved channels remain null.
    pub residual_n_m: [Option<f64>; 3],
    /// Accepted signed permanent Green strain after this node.
    pub plastic_strain: [f64; 3],
    /// Accepted absolute plastic-increment accumulation after this node.
    pub accumulated_strain: [f64; 3],
    /// Physical elastic plus hardening storage per reference area.
    pub stored_j_m2: f64,
    /// Cumulative irreversible work per reference area, not an automatic thermal source.
    pub plastic_dissipation_j_m2: f64,
}

/// One fixed-order optimization attempt, including failed starts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartReport {
    /// Index in the declared deterministic start order.
    pub index: usize,
    /// Physical starting coefficients after projection into the authored search bounds.
    pub initial: Parameters,
    /// Last admissible coefficients, if the start had any admissible forward replay.
    pub final_parameters: Option<Parameters>,
    /// Sum of squared scaled FIT residuals only.
    pub fit_cost: Option<f64>,
    /// CONVERGED, FLAT, REFUSED, ITERATION_LIMIT, or BUDGET_EXHAUSTED.
    pub termination: String,
}

/// Work and local excitation diagnostics, never parameter uncertainty or a global uniqueness proof.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostics {
    /// Forward objective, finite-difference probe and final partition replay calls, including refusals.
    pub evaluations: usize,
    /// Reserved recorded-node advances, charged before a replay including refused attempts.
    pub history_steps: usize,
    /// Caller-supplied whole-operation forward-call limit.
    pub evaluation_limit: usize,
    /// Fixed whole-operation recorded-node limit.
    pub history_step_limit: usize,
    /// Starts actually entered.
    pub starts_attempted: usize,
    /// Starts finished without an iteration or work limit; refused/flat starts remain visible.
    pub starts_completed: usize,
    /// Forward replays refused by the physical law or nonrepresentable arithmetic.
    pub trials_refused: usize,
    /// RMS scaled-force sensitivity per N/m, yields then hardening in axis order.
    pub parameter_sensitivity: [f64; 6],
    /// Minimum pivot of the column-normalized FIT sensitivity Gram matrix; zero if rank deficient.
    pub excitation_pivot: f64,
    /// Whether the FIT sensitivity calculation completed.
    pub sensitivity_evaluated: bool,
    /// A selected coefficient lies within 1e-7 of an authored normalized search endpoint.
    pub at_bounds: [bool; 6],
    /// All entered starts, including failed attempts; choice uses FIT only.
    pub starts: Vec<StartReport>,
}

/// Reviewable original evidence, selected FIT candidate, diagnostics and held-out replay.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// `ftx-membrane-plasticity-calibration-report/1`.
    pub schema: String,
    /// Complete original input, including any prior coefficients and all missing-channel markers.
    pub input: Input,
    /// Exact typed-input identity, not an experimental certificate.
    pub input_sha256: String,
    /// Whether the identified material is available for separate downstream estimate admission.
    pub status: Status,
    /// Explicit acceptance, ambiguity, rejection or incompleteness explanations.
    pub decision_reasons: Vec<String>,
    /// Best FIT candidate, also retained when CHECK subsequently rejects it.
    pub parameters: Option<Parameters>,
    /// Training residuals; absent when no admissible complete FIT replay was available.
    pub fit: Option<Quality>,
    /// Independent-history residuals; absent without observed CHECK channels or on refused replay.
    pub check: Option<Quality>,
    /// NOT_PROVIDED, NOT_EVALUATED, ACCEPTED, REJECTED, or REFUSED.
    pub check_status: String,
    /// Refused CHECK remains a failed independent replay, never promotion of another FIT candidate.
    pub check_refusal: Option<Refusal>,
    /// FIT and successfully replayed CHECK nodes in original acquisition order.
    pub predictions: Vec<Prediction>,
    /// Shared actual work and FIT-only excitation checks.
    pub diagnostics: Diagnostics,
    /// Only ACCEPTED supplies an ordinary material; fitted coefficients remain estimates.
    pub material: Option<Material>,
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn text_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 4096
}

struct Prepared {
    tensor: [[f64; 3]; 3],
    equilibrium_domain: Option<StrainDomain>,
    strain_limit: [f64; 3],
    accumulation_limit: [f64; 3],
    bounds: [[f64; 2]; 6],
}

impl Prepared {
    fn parameters(&self, x: [f64; 6]) -> Parameters {
        let p: [f64; 6] =
            std::array::from_fn(|i| (1.0 - x[i]) * self.bounds[i][0] + x[i] * self.bounds[i][1]);
        Parameters {
            yield_n_m: [p[0], p[1], p[2]],
            hardening_n_m: [p[3], p[4], p[5]],
        }
    }

    fn law(&self, x: [f64; 6]) -> Result<PlasticMembrane, Refusal> {
        let p = self.parameters(x);
        PlasticMembrane::new(
            self.tensor,
            p.yield_n_m,
            p.hardening_n_m,
            self.strain_limit,
            self.accumulation_limit,
        )
    }
}

fn prepare(input: &Input) -> Result<Prepared, Refusal> {
    input.material.validate()?;
    if input.schema != SCHEMA
        || !text_ok(&input.method)
        || !text_ok(&input.fitted_on)
        || input.conditions.len() > 64
        || input
            .conditions
            .iter()
            .any(|(k, v)| !text_ok(k) || !text_ok(v))
        || ["specimen_id", "conditioning"]
            .iter()
            .any(|k| !input.conditions.get(*k).is_some_and(|v| text_ok(v)))
        || !(1..=4096).contains(&input.max_evaluations)
        || [input.max_rms_scaled, input.max_peak_scaled]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        || matches!(input.material.form, Form::Fiber | Form::Yarn | Form::Fill)
        || input.material.membrane_response.is_some()
        || input.material.membrane_relaxation.is_some()
    {
        return Err(bad(
            "permanent membrane identification needs its schema, characterized sheet, scalar elasticity, matched conditions and finite positive residual/work policy",
        ));
    }
    let existing = input
        .material
        .structure
        .contains_key("membrane_plasticity_model")
        || ["membrane_yield", "membrane_hardening"]
            .iter()
            .any(|family| {
                AXES.iter().any(|axis| {
                    input
                        .material
                        .properties
                        .contains_key(&format!("{family}_{axis}"))
                })
            });
    if existing && !input.replace_existing {
        return Err(bad(
            "replacing existing permanent-membrane coefficients requires replace_existing; the original remains in the report",
        ));
    }
    let material = normalize(&input.material)?;
    let mut elasticity = [0.0; 4];
    let mut domains = [[0.0; 3]; 2];
    let mut required: Vec<_> = ELASTIC.iter().map(|(k, u)| (k.to_string(), *u)).collect();
    for family in ["membrane_strain_limit", "membrane_accumulation_limit"] {
        required.extend(AXES.iter().map(|axis| (format!("{family}_{axis}"), "1")));
    }
    for (i, (key, unit)) in required.iter().enumerate() {
        let value = material.require(key, unit, input.allow_estimated_material)?;
        let q = &material.properties[key];
        if ["specimen_id", "conditioning"]
            .iter()
            .any(|k| q.conditions.get(*k) != input.conditions.get(*k))
            || q.conditions
                .iter()
                .any(|(k, v)| input.conditions.get(k) != Some(v))
        {
            return Err(bad(format!(
                "{key}: elasticity and domains must match every declared acquisition condition, including specimen_id and conditioning"
            )));
        }
        if i < 4 {
            elasticity[i] = value;
        } else {
            domains[(i - 4) / 3][(i - 4) % 3] = value;
        }
    }
    let [warp, weft, poisson, shear] = elasticity;
    let d = 1.0 - poisson * (poisson * weft / warp);
    let bounds = std::array::from_fn(|i| {
        if i < 3 {
            input.yield_bounds_n_m[i]
        } else {
            input.hardening_bounds_n_m[i - 3]
        }
    });
    for (i, &[lo, hi]) in bounds.iter().enumerate() {
        if !lo.is_finite()
            || !hi.is_finite()
            || lo < 0.0
            || (i < 3 && lo == 0.0)
            || hi <= lo
            || !(hi - lo).is_finite()
            || lo + (hi - lo) * 1e-6 == lo
            || hi - (hi - lo) * 1e-6 == hi
        {
            return Err(bad(
                "yield/hardening search bounds must be finite, ordered, representable and physically positive/nonnegative; they are not uncertainty intervals",
            ));
        }
    }
    let prepared = Prepared {
        tensor: [
            [warp / d, poisson * weft / d, 0.0],
            [poisson * weft / d, weft / d, 0.0],
            [0.0, 0.0, shear],
        ],
        equilibrium_domain: fitted_domain(&material)?,
        strain_limit: domains[0],
        accumulation_limit: domains[1],
        bounds,
    };
    let law = prepared.law([0.5; 6])?;
    let mut ids = BTreeSet::new();
    let mut nodes = 0usize;
    let mut fit_observations = 0usize;
    if !(1..=32).contains(&input.experiments.len()) {
        return Err(bad(
            "permanent identification requires 1..32 complete acquisition histories",
        ));
    }
    for experiment in &input.experiments {
        nodes = nodes.saturating_add(experiment.samples.len());
        if !text_ok(&experiment.id)
            || !ids.insert(&experiment.id)
            || !input.material.sources.contains_key(&experiment.source)
            || (experiment.evidence == Evidence::Estimate && !input.allow_estimated_observations)
            || !(2..=2048).contains(&experiment.samples.len())
            || nodes > 2048
        {
            return Err(bad(
                "histories need unique IDs, resolved sources, admitted evidence and 2..2048 nodes within 2048 total nodes",
            ));
        }
        for (i, sample) in experiment.samples.iter().enumerate() {
            if experiment.partition == Partition::Fit
                && prepared
                    .equilibrium_domain
                    .is_some_and(|domain| !domain.contains(sample.green_strain))
            {
                return Err(bad(format!(
                    "{}: history node {i} outside fitted equilibrium strain domain",
                    experiment.id
                )));
            }
            if !sample.time_seconds.is_finite()
                || (i == 0 && (sample.time_seconds != 0.0 || sample.green_strain != [0.0; 3]))
                || (i > 0 && sample.time_seconds <= experiment.samples[i - 1].time_seconds)
                || !super::positive_metric(sample.green_strain)
                || (experiment.partition == Partition::Fit && !law.contains(sample.green_strain))
                || sample
                    .second_piola_n_m
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite())
                || !sample.scale_n_m.is_finite()
                || sample.scale_n_m <= 0.0
            {
                return Err(bad(format!(
                    "{}: recorded history needs pristine zero-time/zero-strain start, increasing times, physical in-domain Green strain and finite load/scales",
                    experiment.id
                )));
            }
            if experiment.partition == Partition::Fit {
                fit_observations += sample.second_piola_n_m.iter().flatten().count();
            }
        }
    }
    if fit_observations == 0 {
        return Err(bad("at least one observed FIT load channel is required"));
    }
    Ok(prepared)
}

struct Evaluation {
    residual: Vec<f64>,
    quality: Option<Quality>,
    cost: f64,
    predictions: Vec<Prediction>,
}

enum Failure {
    Budget,
    Physical(Refusal),
}

struct Work<'a> {
    input: &'a Input,
    prepared: Prepared,
    diagnostics: Diagnostics,
}

impl Work<'_> {
    fn evaluate(
        &mut self,
        x: [f64; 6],
        partition: Partition,
        retain: bool,
    ) -> Result<Evaluation, Failure> {
        let nodes = self
            .input
            .experiments
            .iter()
            .filter(|e| e.partition == partition)
            .map(|e| e.samples.len())
            .sum::<usize>();
        if self.diagnostics.evaluations == self.input.max_evaluations
            || self.diagnostics.history_steps + nodes > MAX_HISTORY_STEPS
        {
            return Err(Failure::Budget);
        }
        self.diagnostics.evaluations += 1;
        self.diagnostics.history_steps += nodes;
        let result = self.replay(x, partition, retain);
        if result.is_err() {
            self.diagnostics.trials_refused += 1;
        }
        result.map_err(Failure::Physical)
    }

    fn replay(
        &self,
        x: [f64; 6],
        partition: Partition,
        retain: bool,
    ) -> Result<Evaluation, Refusal> {
        let law = self.prepared.law(x)?;
        let mut residual = Vec::new();
        let mut predictions = Vec::new();
        for experiment in self
            .input
            .experiments
            .iter()
            .filter(|e| e.partition == partition)
        {
            let mut memory = law.pristine();
            let mut dissipated = 0.0;
            for (i, sample) in experiment.samples.iter().enumerate() {
                if self
                    .prepared
                    .equilibrium_domain
                    .is_some_and(|domain| !domain.contains(sample.green_strain))
                {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!(
                            "{}: history node {i} outside fitted equilibrium strain domain",
                            experiment.id
                        ),
                    ));
                }
                let update = memory.advance(sample.green_strain).map_err(|mut e| {
                    e.message = format!("{}: {}", experiment.id, e.message);
                    e
                })?;
                memory = update.memory;
                dissipated += update.dissipated_j_m2;
                let response = memory.response(sample.green_strain, false)?;
                if !dissipated.is_finite() {
                    return Err(bad("history dissipation overflows"));
                }
                let errors = std::array::from_fn(|i| {
                    sample.second_piola_n_m[i].map(|observed| response.stress[i] - observed)
                });
                residual.extend(errors.iter().flatten().map(|r| r / sample.scale_n_m));
                if retain {
                    predictions.push(Prediction {
                        experiment: experiment.id.clone(),
                        partition,
                        time_seconds: sample.time_seconds,
                        green_strain: sample.green_strain,
                        second_piola_n_m: response.stress,
                        residual_n_m: errors,
                        plastic_strain: memory.plastic_strain(),
                        accumulated_strain: memory.accumulated_strain(),
                        stored_j_m2: response.energy,
                        plastic_dissipation_j_m2: dissipated,
                    });
                }
            }
        }
        let cost = residual.iter().map(|r| r * r).sum::<f64>();
        if !cost.is_finite() {
            return Err(bad("scaled history residuals overflow"));
        }
        let quality = (!residual.is_empty()).then(|| Quality {
            components: residual.len(),
            rms_scaled: (cost / residual.len() as f64).sqrt(),
            peak_scaled: residual.iter().copied().map(f64::abs).fold(0.0, f64::max),
        });
        Ok(Evaluation {
            residual,
            quality,
            cost,
            predictions,
        })
    }

    fn jacobian(&mut self, x: [f64; 6], base: &Evaluation) -> Result<Vec<[f64; 6]>, Failure> {
        let mut rows = vec![[0.0; 6]; base.residual.len()];
        for j in 0..6 {
            let lo = (x[j] - 1e-4).max(0.0);
            let hi = (x[j] + 1e-4).min(1.0);
            let mut sides = Vec::new();
            for position in [lo, hi] {
                if position == x[j] {
                    continue;
                }
                let mut trial = x;
                trial[j] = position;
                match self.evaluate(trial, Partition::Fit, false) {
                    Ok(e) => sides.push((position, e.residual)),
                    Err(Failure::Budget) => return Err(Failure::Budget),
                    Err(Failure::Physical(_)) => {}
                }
            }
            if sides.is_empty() {
                return Err(Failure::Physical(bad(
                    "no admissible bounded sensitivity probe for a selected coefficient",
                )));
            }
            let (a, b, da, db): (&[f64], &[f64], f64, f64) = if sides.len() == 2 {
                (&sides[0].1, &sides[1].1, sides[0].0, sides[1].0)
            } else {
                (&base.residual, &sides[0].1, x[j], sides[0].0)
            };
            for (i, row) in rows.iter_mut().enumerate() {
                row[j] = (b[i] - a[i]) / (db - da);
            }
        }
        if rows.iter().flatten().any(|v| !v.is_finite()) {
            return Err(Failure::Physical(bad(
                "FIT coefficient sensitivity overflows",
            )));
        }
        Ok(rows)
    }
}

// With a complete observed prefix, z = e-Q^-1*S and a = sum(abs(delta z)). Active samples obey
// abs(S_i) = Y_i + H_i*a_i. This is only a projected FIT optimizer seed, never an extra force datum.
fn algebraic_seed(input: &Input, prepared: &Prepared) -> [f64; 6] {
    let mut rows: [Vec<[f64; 2]>; 3] = std::array::from_fn(|_| Vec::new());
    let mut forces: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::new());
    let q = prepared.tensor;
    for e in input
        .experiments
        .iter()
        .filter(|e| e.partition == Partition::Fit)
    {
        let mut old = [0.0; 3];
        let mut accumulated = [0.0; 3];
        for sample in &e.samples {
            let [Some(s0), Some(s1), Some(s2)] = sample.second_piola_n_m else {
                break;
            };
            let elastic1 =
                (s1 - q[0][1] / q[0][0] * s0) / (q[1][1] - q[0][1] * (q[0][1] / q[0][0]));
            let elastic = [
                s0 / q[0][0] - q[0][1] / q[0][0] * elastic1,
                elastic1,
                s2 / q[2][2],
            ];
            let plastic: [f64; 3] = std::array::from_fn(|i| sample.green_strain[i] - elastic[i]);
            if plastic.iter().any(|v| !v.is_finite()) {
                break;
            }
            for i in 0..3 {
                let delta = plastic[i] - old[i];
                accumulated[i] += delta.abs();
                let s = [s0, s1, s2][i];
                if delta.abs() > 1e-9 * (1.0 + plastic[i].abs()) && delta * s > 0.0 {
                    rows[i].push([1.0, accumulated[i]]);
                    forces[i].push(s.abs());
                }
            }
            old = plastic;
        }
    }
    let mut seed = [0.5; 6];
    for i in 0..3 {
        if rows[i].len() < 2 || rows[i].iter().all(|r| (r[1] - rows[i][0][1]).abs() < 1e-8) {
            continue;
        }
        let fitted = lstsq(&rows[i], &forces[i]);
        for (j, value) in [(i, fitted[0]), (i + 3, fitted[1])] {
            let [lo, hi] = prepared.bounds[j];
            if value.is_finite() {
                seed[j] = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
            }
        }
    }
    seed
}

fn optimize(
    work: &mut Work<'_>,
    mut x: [f64; 6],
) -> (Option<([f64; 6], Evaluation)>, &'static str) {
    let mut current = match work.evaluate(x, Partition::Fit, false) {
        Ok(e) => e,
        Err(Failure::Budget) => return (None, "BUDGET_EXHAUSTED"),
        Err(Failure::Physical(_)) => return (None, "REFUSED"),
    };
    let mut lambda: f64 = 1e-4;
    for _ in 0..MAX_ITERATIONS {
        if current.cost <= 1e-24 {
            return (Some((x, current)), "CONVERGED");
        }
        let jacobian = match work.jacobian(x, &current) {
            Ok(j) => j,
            Err(Failure::Budget) => return (Some((x, current)), "BUDGET_EXHAUSTED"),
            Err(Failure::Physical(_)) => return (Some((x, current)), "REFUSED"),
        };
        let scales: [f64; 6] =
            std::array::from_fn(|j| jacobian.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt());
        if scales.iter().any(|s| !s.is_finite()) {
            return (Some((x, current)), "REFUSED");
        }
        if scales.iter().all(|s| *s <= 1e-12) {
            return (Some((x, current)), "FLAT");
        }
        // A clipped Newton step can vanish at a corner with a feasible descent direction.
        // Only the scaled bound-constrained gradient can establish local stationarity here.
        let stationary = (0..6).all(|j| {
            let gradient = jacobian
                .iter()
                .zip(&current.residual)
                .map(|(row, r)| row[j] / scales[j].max(1e-12) * r)
                .sum::<f64>();
            (x[j] == 0.0 && gradient >= 0.0)
                || (x[j] == 1.0 && gradient <= 0.0)
                || gradient.abs() <= 1e-8 * (1.0 + current.cost.sqrt())
        });
        if stationary {
            return (Some((x, current)), "CONVERGED");
        }
        let mut accepted = None;
        for _ in 0..12 {
            let mut rows: Vec<[f64; 6]> = jacobian
                .iter()
                .map(|r| std::array::from_fn(|j| r[j] / scales[j].max(1e-12)))
                .collect();
            let mut rhs: Vec<_> = current.residual.iter().map(|r| -r).collect();
            for j in 0..6 {
                let mut row = [0.0; 6];
                row[j] = lambda.sqrt();
                rows.push(row);
                rhs.push(0.0);
            }
            let delta = lstsq(&rows, &rhs);
            let trial: [f64; 6] =
                std::array::from_fn(|j| (x[j] + delta[j] / scales[j].max(1e-12)).clamp(0.0, 1.0));
            let step = (0..6).map(|j| (trial[j] - x[j]).abs()).fold(0.0, f64::max);
            if step <= 1e-12 {
                lambda *= 10.0;
                continue;
            }
            match work.evaluate(trial, Partition::Fit, false) {
                Ok(next) if next.cost < current.cost => {
                    accepted = Some((trial, next));
                    break;
                }
                Err(Failure::Budget) => return (Some((x, current)), "BUDGET_EXHAUSTED"),
                _ => lambda *= 10.0,
            }
        }
        let Some((trial, next)) = accepted else {
            // A failed line search is not an optimizer convergence certificate.
            return (Some((x, current)), "ITERATION_LIMIT");
        };
        x = trial;
        current = next;
        lambda = (lambda * 0.2).max(1e-10);
    }
    (Some((x, current)), "ITERATION_LIMIT")
}

fn excitation(rows: &[[f64; 6]]) -> ([f64; 6], f64) {
    let scales: [f64; 6] =
        std::array::from_fn(|j| rows.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt());
    if scales.iter().any(|s| !s.is_finite() || *s <= 1e-10) {
        return (scales.map(|s| if s.is_finite() { s } else { 0.0 }), 0.0);
    }
    let mut l = [[0.0; 6]; 6];
    let mut pivot = 1.0_f64;
    for i in 0..6 {
        for j in 0..=i {
            let value = rows
                .iter()
                .map(|r| (r[i] / scales[i]) * (r[j] / scales[j]))
                .sum::<f64>()
                - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if i == j {
                if !value.is_finite() || value <= 0.0 {
                    return (scales, 0.0);
                }
                pivot = pivot.min(value);
                l[i][j] = value.sqrt();
            } else {
                l[i][j] = value / l[j][j];
            }
        }
    }
    (scales, pivot)
}

fn accepted_quality(input: &Input, quality: &Quality) -> bool {
    quality.rms_scaled <= input.max_rms_scaled && quality.peak_scaled <= input.max_peak_scaled
}

fn fitted_material(
    input: &Input,
    parameters: &Parameters,
    hash: &str,
) -> Result<Material, Refusal> {
    let mut material = input.material.clone();
    let source = format!("membrane-plasticity-calibration/{hash}");
    if material.sources.contains_key(&source) {
        return Err(bad("calibration receipt cannot replace an existing source"));
    }
    material.sources.insert(source.clone(), Source {
        reference: format!("local:ftx-membrane-plasticity-calibration/{hash}"),
        accessed_on: input.fitted_on.clone(),
        note: format!("{}; bounded FIT-only identification of yield/hardening through the existing incremental Green/second-Piola law. Original complete histories, search bounds, decisions and CHECK replay are retained. No empirical garment, damage, fatigue, thermal feedback or parameter-confidence claim.", input.method),
    });
    for (family, values) in [
        ("membrane_yield", parameters.yield_n_m),
        ("membrane_hardening", parameters.hardening_n_m),
    ] {
        for (axis, value) in AXES.iter().zip(values) {
            material.properties.insert(format!("{family}_{axis}"), Quantity { value, unit: "N/m".into(),
                source: source.clone(), evidence: Evidence::Estimate,
                method: "Bounded deterministic identification from complete FIT histories; independent elastic and domain observations retained; not a measured coefficient or confidence bound".into(),
                conditions: input.conditions.clone(), bounds: None });
        }
    }
    material
        .structure
        .insert("membrane_plasticity_model".into(), MODEL.into());
    normalize(&material)?.compiled_membrane_plasticity(true)?;
    Ok(material)
}

/// Identify six bounded coefficients and replay the chosen model without consulting CHECK during
/// optimization or excitation analysis. No optional material is returned after an unresolved result.
///
/// # Errors
/// Malformed/unqualified acquisition, independently supplied elasticity/domains, incompatible law,
/// denied evidence/replacement, or nonrepresentable input/output. Numerical fit failures are reports.
pub fn calibrate(input: &Input) -> Result<Report, Refusal> {
    let prepared = prepare(input)?;
    let seed = algebraic_seed(input, &prepared);
    let input_sha256 = sha256_hex(&serde_json::to_vec(input).map_err(|e| bad(e.to_string()))?);
    let has_check = input.experiments.iter().any(|e| {
        e.partition == Partition::Check
            && e.samples
                .iter()
                .any(|s| s.second_piola_n_m.iter().any(Option::is_some))
    });
    let mut work = Work {
        input,
        prepared,
        diagnostics: Diagnostics {
            evaluations: 0,
            history_steps: 0,
            evaluation_limit: input.max_evaluations,
            history_step_limit: MAX_HISTORY_STEPS,
            starts_attempted: 0,
            starts_completed: 0,
            trials_refused: 0,
            parameter_sensitivity: [0.0; 6],
            excitation_pivot: 0.0,
            sensitivity_evaluated: false,
            at_bounds: [false; 6],
            starts: Vec::new(),
        },
    };
    let mut best: Option<([f64; 6], Evaluation)> = None;
    let mut candidates = Vec::new();
    let mut exhausted = false;
    let mut incomplete = false;
    for (index, start) in [seed, [0.15; 6], [0.5; 6], [0.85; 6]]
        .into_iter()
        .enumerate()
    {
        work.diagnostics.starts_attempted += 1;
        let (result, termination) = optimize(&mut work, start);
        work.diagnostics.starts.push(StartReport {
            index,
            initial: work.prepared.parameters(start),
            final_parameters: result.as_ref().map(|(x, _)| work.prepared.parameters(*x)),
            fit_cost: result.as_ref().map(|(_, e)| e.cost),
            termination: termination.into(),
        });
        if termination == "BUDGET_EXHAUSTED" {
            exhausted = true;
        } else if termination == "ITERATION_LIMIT" {
            incomplete = true;
        } else {
            work.diagnostics.starts_completed += 1;
        }
        if let Some((x, e)) = result {
            candidates.push((x, e.cost));
            if best.as_ref().is_none_or(|(_, old)| e.cost < old.cost) {
                best = Some((x, e));
            }
        }
        if exhausted {
            break;
        }
    }
    let mut report = Report {
        schema: "ftx-membrane-plasticity-calibration-report/1".into(),
        input: input.clone(),
        input_sha256,
        status: Status::Rejected,
        decision_reasons: Vec::new(),
        parameters: None,
        fit: None,
        check: None,
        check_status: if has_check {
            "NOT_EVALUATED"
        } else {
            "NOT_PROVIDED"
        }
        .into(),
        check_refusal: None,
        predictions: Vec::new(),
        diagnostics: work.diagnostics.clone(),
        material: None,
    };
    if let Some((x, e)) = best {
        report.parameters = Some(work.prepared.parameters(x));
        report.fit = e.quality.clone();
        work.diagnostics.at_bounds = x.map(|v| v <= 1e-7 || v >= 1.0 - 1e-7);
        let equivalent = candidates.iter().any(|(other, cost)| {
            (*cost - e.cost).abs() <= 1e-12 * (1.0 + e.cost)
                && (0..6).any(|j| (other[j] - x[j]).abs() > 1e-3)
        });
        if !exhausted {
            match work.jacobian(x, &e) {
                Ok(rows) => {
                    let (scales, pivot) = excitation(&rows);
                    work.diagnostics.parameter_sensitivity = std::array::from_fn(|j| {
                        scales[j]
                            / (work.prepared.bounds[j][1] - work.prepared.bounds[j][0])
                            / (e.residual.len() as f64).sqrt()
                    });
                    if work
                        .diagnostics
                        .parameter_sensitivity
                        .iter()
                        .any(|v| !v.is_finite())
                    {
                        work.diagnostics.parameter_sensitivity = [0.0; 6];
                        incomplete = true;
                    } else {
                        work.diagnostics.excitation_pivot = pivot;
                        work.diagnostics.sensitivity_evaluated = true;
                    }
                }
                Err(Failure::Budget) => exhausted = true,
                Err(Failure::Physical(_)) => incomplete = true,
            }
        }
        if !exhausted {
            match work.evaluate(x, Partition::Fit, true) {
                Ok(replayed) => report.predictions = replayed.predictions,
                Err(Failure::Budget) => exhausted = true,
                Err(Failure::Physical(_)) => incomplete = true,
            }
        }
        // FIT parameters, competing starts and observability are fixed before any CHECK replay.
        if !exhausted
            && input
                .experiments
                .iter()
                .any(|e| e.partition == Partition::Check)
        {
            match work.evaluate(x, Partition::Check, true) {
                Ok(replayed) => {
                    report.check = replayed.quality;
                    report.predictions.extend(replayed.predictions);
                    if let Some(q) = &report.check {
                        report.check_status = if accepted_quality(input, q) {
                            "ACCEPTED"
                        } else {
                            "REJECTED"
                        }
                        .into();
                    }
                }
                Err(Failure::Budget) => exhausted = true,
                Err(Failure::Physical(refusal)) => {
                    report.check_refusal = Some(refusal);
                    report.check_status = "REFUSED".into();
                }
            }
        }
        report.predictions.sort_by_key(|p| {
            input
                .experiments
                .iter()
                .position(|e| e.id == p.experiment)
                .expect("retained acquisition")
        });
        if report
            .fit
            .as_ref()
            .is_some_and(|q| !accepted_quality(input, q))
        {
            report.decision_reasons.push(
                "selected FIT candidate exceeds the declared training residual threshold".into(),
            );
        }
        if equivalent {
            report.decision_reasons.push("materially different bounded coefficients give equivalent FIT residuals across starts".into());
        }
        if !work.diagnostics.sensitivity_evaluated || work.diagnostics.excitation_pivot <= 1e-6 {
            report.decision_reasons.push("FIT histories do not independently identify all six yield/hardening coefficients within the numerical excitation gate".into());
        }
        if incomplete {
            report.decision_reasons.push("at least one bounded optimization or selected sensitivity calculation did not complete".into());
        }
        let fit_pass = report
            .fit
            .as_ref()
            .is_some_and(|q| accepted_quality(input, q));
        let identifiable = !incomplete
            && !equivalent
            && work.diagnostics.sensitivity_evaluated
            && work.diagnostics.excitation_pivot > 1e-6;
        if report.check_status == "REJECTED" || report.check_status == "REFUSED" {
            report.decision_reasons.push("the fixed FIT candidate failed independent CHECK replay; no alternative is promoted".into());
        }
        report.status =
            if !fit_pass || report.check_status == "REJECTED" || report.check_status == "REFUSED" {
                Status::Rejected
            } else if !identifiable {
                Status::Ambiguous
            } else {
                Status::Accepted
            };
    } else {
        report.decision_reasons.push("no complete admissible FIT replay was available within the authored parameter and history domains".into());
    }
    if exhausted || work.diagnostics.starts_attempted != STARTS {
        report.status = Status::BudgetExhausted;
        report.decision_reasons.push("shared forward-call or recorded-node budget exhausted before complete identification and replay; partial results cannot bind".into());
    }
    if report.status == Status::Accepted {
        report.material = Some(fitted_material(
            input,
            report.parameters.as_ref().expect("accepted fit"),
            &report.input_sha256,
        )?);
        report.decision_reasons.push("completed FIT identification and declared residual gates passed; fitted coefficients remain estimates, with no global uniqueness or empirical validation claim".into());
    }
    report.diagnostics = work.diagnostics;
    Ok(report)
}
