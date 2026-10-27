//! Identify yield/hardening, optionally together with one proportional Maxwell ratio and time.
//! Elasticity, domains and the parallel-network interpretation are independently supplied. Every
//! trial replays complete FIT paths; CHECK paths cannot choose or alter parameters.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, linalg::lstsq, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use super::{AXES, ELASTIC, MODEL, PlasticMembrane};
use crate::material_calibration::{Partition, Quality, fitted_domain};
use crate::material_catalog::normalize;
use crate::material_relaxation::{
    CompiledRelaxation, Initialization as RelaxationInitialization, MaxwellBranch, Relaxation,
    parallel,
};
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
    /// Strictly increasing elapsed seconds, starting at zero. A fixed parallel Maxwell spectrum
    /// uses every recorded interval; without it the fitted permanent law is rate independent.
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
    /// Present only when the single proportional Maxwell arm was jointly identified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relaxation: Option<RelaxationParameters>,
}

/// Jointly identified proportional branch; these are estimates, not fixed source observations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelaxationParameters {
    /// Positive multiplier of the independently characterized equilibrium tensor.
    pub stiffness_ratio: f64,
    /// Positive canonical backward-Euler recovery time, seconds.
    pub relaxation_time_seconds: f64,
}

/// An explicit single-arm model to identify together with the six permanent coefficients.
/// No provisional Maxwell coefficient is needed in the input material.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JointRelaxation {
    /// Identity of the one proportional arm retained in the fitted ordinary material.
    pub branch_id: String,
    /// Finite strictly ordered positive search bounds, never confidence intervals.
    pub stiffness_ratio_bounds: [f64; 2],
    /// Finite strictly ordered positive time search bounds, seconds.
    pub relaxation_time_bounds_seconds: [f64; 2],
    /// Independently qualified physical Green-strain domain; not enlarged by fitting.
    pub strain_range: [[f64; 2]; 3],
    /// Source-declared new-simulation initialization; acquisitions always start at zero strain.
    pub initialization: RelaxationInitialization,
    /// Source/method/specimen/conditioning for this exact network, domain and initialization.
    pub network: parallel::ParallelPlasticity,
}

/// Original source-bearing problem. The six domain quantities remain properties of `material`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Independently characterized four elastic and six strain/accumulation limit quantities,
    /// and optionally an explicitly qualified parallel Maxwell spectrum that remains fixed.
    /// Other mass, contact, thermal or bending properties are retained, never inferred.
    pub material: Material,
    /// Admit estimated elastic/domain/spectrum/network observations, separately from load histories.
    #[serde(default)]
    pub allow_estimated_material: bool,
    /// Admit estimated or explicitly synthetic histories.
    #[serde(default)]
    pub allow_estimated_observations: bool,
    /// Required if a permanent-law selector or any previous yield/hardening quantity exists.
    #[serde(default)]
    pub replace_existing: bool,
    /// Identify one proportional Maxwell arm at the same time as yield/hardening. Otherwise any
    /// existing spectrum stays fixed. Replacing a supplied arm requires replace_existing; other
    /// arms are never silently discarded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joint_relaxation: Option<JointRelaxation>,
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
    /// Predicted accepted TOTAL second-Piola resultants, including any Maxwell branches.
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
    /// Present only for an explicitly qualified parallel Maxwell spectrum. The existing
    /// stored_j_m2 remains equilibrium elastic/hardening storage; measured stress is the TOTAL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_history: Option<ParallelHistory>,
}

/// Additional fixed-spectrum response, separate from the identified equilibrium plastic branch.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParallelHistory {
    /// Sum of the accepted Maxwell branch stresses, N/m, in [S11,S22,S12] order.
    pub maxwell_second_piola_n_m: [f64; 3],
    /// Actual accepted Maxwell spring storage, J/m²; excludes numerical/physical dissipation.
    pub maxwell_stored_j_m2: f64,
    /// Cumulative physical dashpot work under the canonical backward-Euler quadrature, J/m².
    /// No thermal conversion or numerical integration loss is inferred.
    pub maxwell_dissipation_j_m2: f64,
    /// Accepted viscous [E11,E22,2E12] per branch, in the original material's authored order.
    pub viscous_green_strains: Vec<[f64; 3]>,
    /// Equilibrium elastic/hardening plus Maxwell spring storage, each counted once, J/m².
    pub total_stored_j_m2: f64,
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
    /// Each fixed-spectrum FIT/CHECK partition precomputation also costs one call.
    pub evaluations: usize,
    /// Reserved constitutive work: plastic recorded nodes plus fixed Maxwell branch-node states.
    /// Every partition is charged before replay, including refused attempts.
    pub history_steps: usize,
    /// Fixed-spectrum branch-node states charged within history_steps. Each whole FIT/CHECK
    /// partition is precomputed once; absent for ordinary rate-independent identification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_maxwell_branch_steps: Option<usize>,
    /// Joint full-parameter excitation and per-trial Maxwell work; absent for the legacy fits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joint_relaxation: Option<JointRelaxationDiagnostics>,
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

/// Additional diagnostics for the two recovery parameters in the full eight-column FIT problem.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JointRelaxationDiagnostics {
    /// RMS scaled-force sensitivity per ratio and per second; not parameter uncertainty.
    pub parameter_sensitivity: [f64; 2],
    /// Proximity to authored normalized ratio/time endpoints using the same 1e-7 rule.
    pub at_bounds: [bool; 2],
    /// Reserved Maxwell branch-node states, including refused trials and final replays.
    pub branch_steps: usize,
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

struct Prepared<const N: usize> {
    tensor: [[f64; 3]; 3],
    equilibrium_domain: Option<StrainDomain>,
    strain_limit: [f64; 3],
    accumulation_limit: [f64; 3],
    bounds: [[f64; 2]; N],
    relaxation: Option<CompiledRelaxation>,
    branch_count: usize,
    fixed_histories: Vec<Option<Vec<FixedState>>>,
    /// A qualified model template with explicitly synthetic, unpublished trial coefficients.
    joint_template: Option<Material>,
}

struct FixedState {
    stress: [f64; 3],
    stored: f64,
    dissipated: f64,
    viscous: Vec<[f64; 3]>,
}

fn fixed_history(
    law: &CompiledRelaxation,
    experiment: &Experiment,
) -> Result<Vec<FixedState>, Refusal> {
    let mut memory = law.initialize([0.0; 3])?;
    let mut dissipated = 0.0;
    let mut result = Vec::with_capacity(experiment.samples.len());
    for (index, sample) in experiment.samples.iter().enumerate() {
        if index > 0 {
            let update = memory
                .advance(
                    sample.green_strain,
                    sample.time_seconds - experiment.samples[index - 1].time_seconds,
                )
                .map_err(|mut e| {
                    e.message = format!("{}: Maxwell node {index}: {}", experiment.id, e.message);
                    e
                })?;
            memory = update.memory;
            dissipated += update.dissipated_j_m2;
        }
        let response = memory.response(sample.green_strain, None)?;
        if !dissipated.is_finite() {
            return Err(bad("fixed Maxwell history dissipation overflows"));
        }
        result.push(FixedState {
            stress: response.stress,
            stored: response.energy,
            dissipated,
            viscous: memory.viscous_strains().to_vec(),
        });
    }
    Ok(result)
}

impl<const N: usize> Prepared<N> {
    fn parameters(&self, x: [f64; N]) -> Parameters {
        let p: [f64; N] =
            std::array::from_fn(|i| (1.0 - x[i]) * self.bounds[i][0] + x[i] * self.bounds[i][1]);
        Parameters {
            yield_n_m: [p[0], p[1], p[2]],
            hardening_n_m: [p[3], p[4], p[5]],
            relaxation: self.joint_template.as_ref().map(|_| RelaxationParameters {
                stiffness_ratio: p[6],
                relaxation_time_seconds: p[7],
            }),
        }
    }

    fn law(&self, x: [f64; N]) -> Result<PlasticMembrane, Refusal> {
        let p = self.parameters(x);
        PlasticMembrane::new(
            self.tensor,
            p.yield_n_m,
            p.hardening_n_m,
            self.strain_limit,
            self.accumulation_limit,
        )
    }

    fn joint_law(
        &self,
        x: [f64; N],
        plastic: &PlasticMembrane,
    ) -> Result<Option<CompiledRelaxation>, Refusal> {
        let Some(template) = &self.joint_template else {
            return Ok(None);
        };
        let mut trial = template.clone();
        let parameters = self.parameters(x).relaxation.expect("joint parameters");
        let branch = &mut trial
            .membrane_relaxation
            .as_mut()
            .expect("joint template")
            .branches[0];
        branch.stiffness_ratio.value = parameters.stiffness_ratio;
        branch.relaxation_time.value = parameters.relaxation_time_seconds;
        // All independent evidence was admitted before the template was created. Only these
        // unpublished trial estimates use estimate admission; publication recompiles everything.
        parallel::compile_for_calibration(&trial, true, plastic).map(Some)
    }
}

fn joint_template(input: &Input, material: &Material) -> Result<Option<Material>, Refusal> {
    let Some(joint) = &input.joint_relaxation else {
        return Ok(None);
    };
    joint
        .network
        .validate(&material.sources, input.allow_estimated_material)?;
    if !text_ok(&joint.branch_id)
        || joint
            .network
            .conditions
            .iter()
            .any(|(key, value)| input.conditions.get(key) != Some(value))
    {
        return Err(bad(
            "joint recovery identification requires a branch ID and independently qualified matching network/domain/initialization conditions",
        ));
    }
    if let Some(previous) = &material.membrane_relaxation
        && (!input.replace_existing
            || previous.branches.len() != 1
            || !previous.orthotropic_branches.is_empty()
            || previous.branches[0].id != joint.branch_id)
    {
        return Err(bad(
            "joint recovery can replace only one matching proportional arm with replace_existing; additional or directional arms cannot be discarded",
        ));
    }
    let quantity = |value, unit: &str| Quantity {
        value,
        unit: unit.into(),
        source: joint.network.source.clone(),
        evidence: Evidence::Estimate,
        method: "Unpublished bounded joint-identification trial, not a measured coefficient".into(),
        conditions: joint.network.conditions.clone(),
        bounds: None,
    };
    let midpoint = |bounds: [f64; 2]| 0.5 * bounds[0] + 0.5 * bounds[1];
    let mut template = material.clone();
    template.membrane_relaxation = Some(Relaxation {
        model: "green_maxwell/1".into(),
        branches: vec![MaxwellBranch {
            id: joint.branch_id.clone(),
            stiffness_ratio: quantity(midpoint(joint.stiffness_ratio_bounds), "1"),
            relaxation_time: quantity(midpoint(joint.relaxation_time_bounds_seconds), "s"),
        }],
        orthotropic_branches: Vec::new(),
        initialization: joint.initialization,
        strain_range: joint.strain_range,
        source: joint.network.source.clone(),
        method: joint.network.method.clone(),
        conditions: joint.network.conditions.clone(),
        parallel_plasticity: Some(joint.network.clone()),
    });
    Ok(Some(template))
}

fn prepare<const N: usize>(input: &Input) -> Result<Prepared<N>, Refusal> {
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
    let bounds: [[f64; 2]; N] = std::array::from_fn(|i| {
        if i < 3 {
            input.yield_bounds_n_m[i]
        } else if i < 6 {
            input.hardening_bounds_n_m[i - 3]
        } else if i == 6 {
            input
                .joint_relaxation
                .as_ref()
                .expect("joint dimension")
                .stiffness_ratio_bounds
        } else {
            input
                .joint_relaxation
                .as_ref()
                .expect("joint dimension")
                .relaxation_time_bounds_seconds
        }
    });
    for (i, &[lo, hi]) in bounds.iter().enumerate() {
        if !lo.is_finite()
            || !hi.is_finite()
            || lo < 0.0
            || (!(3..6).contains(&i) && lo == 0.0)
            || hi <= lo
            || !(hi - lo).is_finite()
            || lo + (hi - lo) * 1e-6 == lo
            || hi - (hi - lo) * 1e-6 == hi
        {
            return Err(bad(if input.joint_relaxation.is_some() {
                "coefficient search bounds must be finite, ordered, representable and physically positive (hardening may be zero); they are not uncertainty intervals"
            } else {
                "yield/hardening search bounds must be finite, ordered, representable and physically positive/nonnegative; they are not uncertainty intervals"
            }));
        }
    }
    let mut prepared = Prepared {
        tensor: [
            [warp / d, poisson * weft / d, 0.0],
            [poisson * weft / d, weft / d, 0.0],
            [0.0, 0.0, shear],
        ],
        equilibrium_domain: fitted_domain(&material)?,
        strain_limit: domains[0],
        accumulation_limit: domains[1],
        bounds,
        relaxation: None,
        branch_count: 0,
        fixed_histories: Vec::new(),
        joint_template: joint_template(input, &material)?,
    };
    let law = prepared.law([0.5; N])?;
    if prepared.joint_template.is_some() {
        prepared.relaxation = prepared.joint_law([0.5; N], &law)?;
        prepared.branch_count = 1;
    } else if let Some(relaxation) = &material.membrane_relaxation {
        if relaxation
            .conditions
            .iter()
            .any(|(key, value)| input.conditions.get(key) != Some(value))
        {
            return Err(bad(
                "fixed Maxwell spectrum must match the complete acquisition conditions",
            ));
        }
        prepared.relaxation = Some(parallel::compile_for_calibration(
            &material,
            input.allow_estimated_material,
            &law,
        )?);
        prepared.branch_count = relaxation.branches.len() + relaxation.orthotropic_branches.len();
    }
    let mut ids = BTreeSet::new();
    let mut nodes = 0usize;
    let mut fit_observations = 0usize;
    if !(1..=32).contains(&input.experiments.len()) {
        return Err(bad(
            "permanent identification requires 1..32 complete acquisition histories",
        ));
    }
    prepared.fixed_histories = (0..input.experiments.len()).map(|_| None).collect();
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
            if experiment.partition == Partition::Fit
                && prepared
                    .relaxation
                    .as_ref()
                    .is_some_and(|law| !law.contains(sample.green_strain))
            {
                return Err(bad(format!(
                    "{}: history node {i} outside qualified parallel strain domain",
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

struct Work<'a, const N: usize> {
    input: &'a Input,
    prepared: Prepared<N>,
    diagnostics: Diagnostics,
}

impl<const N: usize> Work<'_, N> {
    // The fixed spectrum depends only on the original total strain/time history. Reserve its
    // entire partition before evaluation, reuse FIT for all coefficient probes, and leave CHECK
    // untouched until after parameter selection and excitation. No failed work is refunded.
    fn prepare_fixed_partition(&mut self, partition: Partition) -> Result<(), Failure> {
        if self.prepared.joint_template.is_some() {
            return Ok(());
        }
        let Some(law) = &self.prepared.relaxation else {
            return Ok(());
        };
        let pending: Vec<_> = self
            .input
            .experiments
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                e.partition == partition && self.prepared.fixed_histories[*i].is_none()
            })
            .map(|(i, e)| (i, e.samples.len()))
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        let work =
            pending.iter().map(|(_, nodes)| nodes).sum::<usize>() * self.prepared.branch_count;
        if self.diagnostics.evaluations == self.input.max_evaluations
            || self.diagnostics.history_steps + work > MAX_HISTORY_STEPS
        {
            return Err(Failure::Budget);
        }
        self.diagnostics.evaluations += 1;
        self.diagnostics.history_steps += work;
        *self
            .diagnostics
            .fixed_maxwell_branch_steps
            .as_mut()
            .expect("qualified spectrum work") += work;
        for (i, _) in pending {
            match fixed_history(law, &self.input.experiments[i]) {
                Ok(history) => self.prepared.fixed_histories[i] = Some(history),
                Err(refusal) => {
                    self.diagnostics.trials_refused += 1;
                    return Err(Failure::Physical(refusal));
                }
            }
        }
        Ok(())
    }

    fn evaluate(
        &mut self,
        x: [f64; N],
        partition: Partition,
        retain: bool,
    ) -> Result<Evaluation, Failure> {
        self.prepare_fixed_partition(partition)?;
        let nodes = self
            .input
            .experiments
            .iter()
            .filter(|e| e.partition == partition)
            .map(|e| e.samples.len())
            .sum::<usize>();
        let branch_nodes = if self.prepared.joint_template.is_some() {
            nodes * self.prepared.branch_count
        } else {
            0
        };
        if self.diagnostics.evaluations == self.input.max_evaluations
            || self.diagnostics.history_steps + nodes + branch_nodes > MAX_HISTORY_STEPS
        {
            return Err(Failure::Budget);
        }
        self.diagnostics.evaluations += 1;
        self.diagnostics.history_steps += nodes + branch_nodes;
        if let Some(joint) = &mut self.diagnostics.joint_relaxation {
            joint.branch_steps += branch_nodes;
        }
        let result = self.replay(x, partition, retain);
        if result.is_err() {
            self.diagnostics.trials_refused += 1;
        }
        result.map_err(Failure::Physical)
    }

    fn replay(
        &self,
        x: [f64; N],
        partition: Partition,
        retain: bool,
    ) -> Result<Evaluation, Refusal> {
        let law = self.prepared.law(x)?;
        let joint_law = self.prepared.joint_law(x, &law)?;
        let mut residual = Vec::new();
        let mut predictions = Vec::new();
        for (experiment_index, experiment) in self
            .input
            .experiments
            .iter()
            .enumerate()
            .filter(|(_, e)| e.partition == partition)
        {
            let joint_history = joint_law
                .as_ref()
                .map(|recovery| fixed_history(recovery, experiment))
                .transpose()?;
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
                let fixed = joint_history
                    .as_ref()
                    .or(self.prepared.fixed_histories[experiment_index].as_ref())
                    .map(|states| &states[i]);
                let stress = fixed.map_or(response.stress, |state| {
                    std::array::from_fn(|j| response.stress[j] + state.stress[j])
                });
                let total_storage = response.energy + fixed.map_or(0.0, |state| state.stored);
                if stress.iter().any(|s| !s.is_finite()) || !total_storage.is_finite() {
                    return Err(bad("combined material response overflows"));
                }
                let errors = std::array::from_fn(|i| {
                    sample.second_piola_n_m[i].map(|observed| stress[i] - observed)
                });
                residual.extend(errors.iter().flatten().map(|r| r / sample.scale_n_m));
                if retain {
                    predictions.push(Prediction {
                        experiment: experiment.id.clone(),
                        partition,
                        time_seconds: sample.time_seconds,
                        green_strain: sample.green_strain,
                        second_piola_n_m: stress,
                        residual_n_m: errors,
                        plastic_strain: memory.plastic_strain(),
                        accumulated_strain: memory.accumulated_strain(),
                        stored_j_m2: response.energy,
                        plastic_dissipation_j_m2: dissipated,
                        parallel_history: fixed.map(|state| ParallelHistory {
                            maxwell_second_piola_n_m: state.stress,
                            maxwell_stored_j_m2: state.stored,
                            maxwell_dissipation_j_m2: state.dissipated,
                            viscous_green_strains: state.viscous.clone(),
                            total_stored_j_m2: total_storage,
                        }),
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

    fn jacobian(&mut self, x: [f64; N], base: &Evaluation) -> Result<Vec<[f64; N]>, Failure> {
        let mut rows = vec![[0.0; N]; base.residual.len()];
        for j in 0..N {
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
fn algebraic_seed<const N: usize>(input: &Input, prepared: &Prepared<N>) -> [f64; N] {
    // The algebraic plastic seed assumes independently known transient stress. With an unknown
    // arm use a declared deterministic interior start, never interpret total load as plastic load.
    if prepared.joint_template.is_some() {
        return [0.35; N];
    }
    let mut rows: [Vec<[f64; 2]>; 3] = std::array::from_fn(|_| Vec::new());
    let mut forces: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::new());
    let q = prepared.tensor;
    for (experiment_index, e) in input
        .experiments
        .iter()
        .enumerate()
        .filter(|(_, e)| e.partition == Partition::Fit)
    {
        let mut old = [0.0; 3];
        let mut accumulated = [0.0; 3];
        for (sample_index, sample) in e.samples.iter().enumerate() {
            let [Some(mut s0), Some(mut s1), Some(mut s2)] = sample.second_piola_n_m else {
                break;
            };
            if let Some(history) = &prepared.fixed_histories[experiment_index] {
                let fixed = history[sample_index].stress;
                s0 -= fixed[0];
                s1 -= fixed[1];
                s2 -= fixed[2];
            }
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
    let mut seed = [0.5; N];
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

// Solve the damped local least-squares problem inside the actual parameter box. Merely clipping
// its unconstrained solution leaves the free coordinates compensating for a step that never
// occurred. The active set fixes reached endpoints, then releases any endpoint whose quadratic
// gradient points into the feasible box. Every face solve uses the existing deterministic SVD.
fn bounded_step<const N: usize>(
    rows: &[[f64; N]],
    rhs: &[f64],
    x: [f64; N],
    scales: [f64; N],
) -> Option<[f64; N]> {
    let scales = scales.map(|s| s.max(1e-12));
    let lower: [f64; N] = std::array::from_fn(|j| -x[j] * scales[j]);
    let upper: [f64; N] = std::array::from_fn(|j| (1.0 - x[j]) * scales[j]);
    let mut current = [0.0; N];
    // -1 is a lower endpoint, +1 an upper endpoint, 0 a free coordinate.
    let mut active = [0_i8; N];
    for _ in 0..4 * N * N {
        let mut reduced: Vec<[f64; N]> = rows
            .iter()
            .map(|row| std::array::from_fn(|j| if active[j] == 0 { row[j] } else { 0.0 }))
            .collect();
        let mut target_rhs: Vec<f64> = rows
            .iter()
            .zip(rhs)
            .map(|(row, target)| {
                target
                    - (0..N)
                        .filter(|&j| active[j] != 0)
                        .map(|j| row[j] * current[j])
                        .sum::<f64>()
            })
            .collect();
        for j in 0..N {
            if active[j] != 0 {
                let mut row = [0.0; N];
                row[j] = 1.0;
                reduced.push(row);
                target_rhs.push(current[j]);
            }
        }
        let mut target = lstsq(&reduced, &target_rhs);
        if target.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let mut fraction = 1.0;
        let mut blocker = None;
        for j in 0..N {
            if active[j] != 0 {
                target[j] = current[j];
                continue;
            }
            let endpoint = if target[j] < lower[j] {
                Some((lower[j], -1))
            } else if target[j] > upper[j] {
                Some((upper[j], 1))
            } else {
                None
            };
            if let Some((bound, side)) = endpoint {
                let reachable = (bound - current[j]) / (target[j] - current[j]);
                if reachable < fraction || (reachable == fraction && blocker.is_none()) {
                    fraction = reachable;
                    blocker = Some((j, bound, side));
                }
            }
        }
        if !fraction.is_finite() || fraction < 0.0 {
            return None;
        }
        if let Some((j, bound, side)) = blocker {
            current = std::array::from_fn(|k| {
                (current[k] + fraction * (target[k] - current[k])).clamp(lower[k], upper[k])
            });
            current[j] = bound;
            active[j] = side;
            continue;
        }
        current = target;
        let residual: Vec<f64> = rows
            .iter()
            .zip(rhs)
            .map(|(row, target)| (0..N).map(|j| row[j] * current[j]).sum::<f64>() - target)
            .collect();
        let tolerance = 128.0
            * f64::EPSILON
            * (1.0
                + rhs.iter().map(|v| v.abs()).sum::<f64>()
                + residual.iter().map(|v| v.abs()).sum::<f64>());
        if !tolerance.is_finite() {
            return None;
        }
        let mut release = None;
        let mut violation = tolerance;
        for j in 0..N {
            if active[j] != 0 {
                let gradient = rows
                    .iter()
                    .zip(&residual)
                    .map(|(row, residual)| row[j] * residual)
                    .sum::<f64>();
                let outward = f64::from(active[j]) * gradient;
                if !outward.is_finite() {
                    return None;
                }
                if outward > violation {
                    violation = outward;
                    release = Some(j);
                }
            }
        }
        if let Some(j) = release {
            active[j] = 0;
        } else {
            return Some(std::array::from_fn(|j| {
                (x[j] + current[j] / scales[j]).clamp(0.0, 1.0)
            }));
        }
    }
    None
}

fn optimize<const N: usize>(
    work: &mut Work<'_, N>,
    mut x: [f64; N],
) -> (Option<([f64; N], Evaluation)>, &'static str) {
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
        let scales: [f64; N] =
            std::array::from_fn(|j| jacobian.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt());
        if scales.iter().any(|s| !s.is_finite()) {
            return (Some((x, current)), "REFUSED");
        }
        if scales.iter().all(|s| *s <= 1e-12) {
            return (Some((x, current)), "FLAT");
        }
        // A clipped Newton step can vanish at a corner with a feasible descent direction.
        // Only the scaled bound-constrained gradient can establish local stationarity here.
        let stationary = (0..N).all(|j| {
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
            let mut rows: Vec<[f64; N]> = jacobian
                .iter()
                .map(|r| std::array::from_fn(|j| r[j] / scales[j].max(1e-12)))
                .collect();
            let mut rhs: Vec<_> = current.residual.iter().map(|r| -r).collect();
            for j in 0..N {
                let mut row = [0.0; N];
                row[j] = lambda.sqrt();
                rows.push(row);
                rhs.push(0.0);
            }
            let trial: [f64; N] = if N == 8 {
                let Some(trial) = bounded_step(&rows, &rhs, x, scales) else {
                    lambda *= 10.0;
                    continue;
                };
                trial
            } else {
                // Preserve the existing six-parameter arithmetic when joint mode is absent.
                let delta = lstsq(&rows, &rhs);
                std::array::from_fn(|j| (x[j] + delta[j] / scales[j].max(1e-12)).clamp(0.0, 1.0))
            };
            let step = (0..N).map(|j| (trial[j] - x[j]).abs()).fold(0.0, f64::max);
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

fn excitation<const N: usize>(rows: &[[f64; N]]) -> ([f64; N], f64) {
    let scales: [f64; N] =
        std::array::from_fn(|j| rows.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt());
    if scales.iter().any(|s| !s.is_finite() || *s <= 1e-10) {
        return (scales.map(|s| if s.is_finite() { s } else { 0.0 }), 0.0);
    }
    let mut l = [[0.0; N]; N];
    let mut pivot = 1.0_f64;
    for i in 0..N {
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
    if let Some(recovery) = &parameters.relaxation {
        material = joint_template(input, &material)?.expect("joint publication template");
        let branch = &mut material
            .membrane_relaxation
            .as_mut()
            .expect("joint publication")
            .branches[0];
        for (quantity, value) in [
            (&mut branch.stiffness_ratio, recovery.stiffness_ratio),
            (
                &mut branch.relaxation_time,
                recovery.relaxation_time_seconds,
            ),
        ] {
            quantity.value = value;
            quantity.source = source.clone();
            quantity.method = "Bounded joint FIT-only identification with the six permanent coefficients through recorded-node return mapping and canonical backward-Euler recovery; not a measured coefficient or confidence bound".into();
            quantity.conditions = input.conditions.clone();
        }
        material.sources.get_mut(&source).expect("fit source").note = format!(
            "{}; bounded joint FIT-only identification of six directional yield/hardening coefficients and one proportional Maxwell ratio/time. All eight coefficients share complete-history residuals and the full local excitation gate. Original evidence, declared bounds and independent CHECK replay are retained; no global uniqueness, empirical garment or parameter-confidence claim.",
            input.method
        );
    }
    normalize(&material)?.compiled_membrane_plasticity(true)?;
    Ok(material)
}

/// Identify six permanent coefficients, optionally together with a proportional recovery ratio/time,
/// without consulting CHECK during optimization or excitation analysis. Unresolved results cannot bind.
///
/// # Errors
/// Malformed/unqualified acquisition, independently supplied elasticity/domains, incompatible law,
/// denied evidence/replacement, or nonrepresentable input/output. Numerical fit failures are reports.
pub fn calibrate(input: &Input) -> Result<Report, Refusal> {
    if input.joint_relaxation.is_some() {
        calibrate_inner::<8>(input)
    } else {
        calibrate_inner::<6>(input)
    }
}

fn calibrate_inner<const N: usize>(input: &Input) -> Result<Report, Refusal> {
    let prepared = prepare::<N>(input)?;
    let fixed_steps = prepared
        .relaxation
        .as_ref()
        .filter(|_| prepared.joint_template.is_none())
        .map(|_| 0);
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
            fixed_maxwell_branch_steps: fixed_steps,
            joint_relaxation: input
                .joint_relaxation
                .as_ref()
                .map(|_| JointRelaxationDiagnostics {
                    parameter_sensitivity: [0.0; 2],
                    at_bounds: [false; 2],
                    branch_steps: 0,
                }),
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
    if let Err(failure) = work.prepare_fixed_partition(Partition::Fit) {
        match failure {
            Failure::Budget => {
                report.status = Status::BudgetExhausted;
                report.decision_reasons.push(
                    "shared work budget cannot complete the fixed FIT Maxwell histories".into(),
                );
            }
            Failure::Physical(refusal) => report.decision_reasons.push(format!(
                "fixed FIT Maxwell history refused: {}",
                refusal.message
            )),
        }
        report.diagnostics = work.diagnostics;
        return Ok(report);
    }
    let seed = algebraic_seed(input, &work.prepared);
    let mut best: Option<([f64; N], Evaluation)> = None;
    let mut candidates = Vec::new();
    let mut exhausted = false;
    let mut incomplete = false;
    for (index, start) in [seed, [0.15; N], [0.5; N], [0.85; N]]
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
    if let Some((x, e)) = best {
        report.parameters = Some(work.prepared.parameters(x));
        report.fit = e.quality.clone();
        work.diagnostics.at_bounds = std::array::from_fn(|j| x[j] <= 1e-7 || x[j] >= 1.0 - 1e-7);
        if let Some(joint) = &mut work.diagnostics.joint_relaxation {
            joint.at_bounds = std::array::from_fn(|j| x[j + 6] <= 1e-7 || x[j + 6] >= 1.0 - 1e-7);
        }
        let equivalent = candidates.iter().any(|(other, cost)| {
            (*cost - e.cost).abs() <= 1e-12 * (1.0 + e.cost)
                && (0..N).any(|j| (other[j] - x[j]).abs() > 1e-3)
        });
        if !exhausted {
            match work.jacobian(x, &e) {
                Ok(rows) => {
                    let (scales, pivot) = excitation(&rows);
                    let sensitivity: [f64; N] = std::array::from_fn(|j| {
                        scales[j]
                            / (work.prepared.bounds[j][1] - work.prepared.bounds[j][0])
                            / (e.residual.len() as f64).sqrt()
                    });
                    if sensitivity.iter().any(|v| !v.is_finite()) {
                        work.diagnostics.parameter_sensitivity = [0.0; 6];
                        incomplete = true;
                    } else {
                        work.diagnostics.parameter_sensitivity =
                            std::array::from_fn(|j| sensitivity[j]);
                        if let Some(joint) = &mut work.diagnostics.joint_relaxation {
                            joint.parameter_sensitivity = [sensitivity[6], sensitivity[7]];
                        }
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
            report.decision_reasons.push(if input.joint_relaxation.is_some() {
                "FIT histories do not independently identify all eight permanent/recovery coefficients within the joint numerical excitation gate".into()
            } else {
                "FIT histories do not independently identify all six yield/hardening coefficients within the numerical excitation gate".into()
            });
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

#[cfg(test)]
mod tests {
    use super::bounded_step;

    #[test]
    fn bounded_quadratic_reoptimizes_the_free_coordinate_after_reaching_a_bound() {
        // min (u+v-2)^2 + (u-1)^2, 0<=u<=0.2, 0<=v<=2.
        // Unconstrained (1,1) clipped to (0.2,1) is not stationary; the exact constrained
        // solution is (0.2,1.8). Returned coordinates divide by the supplied physical spans.
        let point =
            bounded_step(&[[1.0, 1.0], [1.0, 0.0]], &[2.0, 1.0], [0.0; 2], [0.2, 2.0]).unwrap();
        assert!((point[0] - 1.0).abs() < 1e-12, "{point:?}");
        assert!((point[1] - 0.9).abs() < 1e-12, "{point:?}");
    }

    #[test]
    fn bounded_quadratic_releases_an_endpoint_with_an_inward_gradient() {
        // H=[[1,.9],[.9,1]], b=[1,1.5], 0<=u,v<=1. The unconstrained direction first
        // reaches u=0; after v reaches 1, that lower bound must release. Independent KKT
        // equations give u=.1, v=1, gradient=[0,-.41]. Keeping u=0 is nonstationary.
        let root = 0.19_f64.sqrt();
        let point = bounded_step(
            &[[1.0, 0.9], [0.0, root]],
            &[1.0, 0.6 / root],
            [0.0; 2],
            [1.0; 2],
        )
        .unwrap();
        assert!((point[0] - 0.1).abs() < 1e-12, "{point:?}");
        assert!((point[1] - 1.0).abs() < 1e-12, "{point:?}");
    }
}
