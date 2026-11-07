//! Bounded coordinate polling over explicit linked alterations and canonical raw fit readings.
//! Optional consumer guidance reuses the study comparator; no mode certifies the unvisited domain.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, curve::OUTLINE_DENSE};
use ftx_pattern::{Pattern, material_screen::Direction};
use serde::{Deserialize, Serialize};

use super::super::{Outcome, PreparedVariant, Request as StudyRequest, comparison};
use super::{
    MAX_DESIGNS, MAX_DOCUMENT_WORK_BYTES, MAX_OPERATIONS, Operation, Prepared, Variant, budget,
    fit, grid::Site, invalid,
};
use crate::cutting::geometry::Work;

/// Declared discovery objective; downstream material and production ranking remains independent.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    /// Minimize maximum absolute raw out-of-band residual, then their sum, both in millimetres.
    FitBandViolationMm,
    /// Use one admitted production comparison objective after complete flat qualification.
    ConsumerComparison,
    /// Explore the admitted tradeoffs of several ordinary objectives without a scalar score.
    ConsumerPareto,
    /// Explore tradeoffs across every named case's own objectives, without nominating one case.
    ConsumerMatrixPareto,
    /// Improve the worst admitted reading of one shared objective and basis across named cases.
    ConsumerWorstCase,
}

/// Explicit progress rule while complete production evidence still violates authored guards.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintRestoration {
    /// No guard side may worsen and at least one must improve, without combining physical units.
    Componentwise,
}

/// Optional expansion of the ordinary coordinate directions, using the same declared step sizes.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polling {
    /// Poll axes first, then every signed pair and triple of independently authored variables.
    AxisAndJoint,
}

/// One bounded scalar, applied to its authored linked alteration sites in order.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variable {
    /// Unique nonempty identity retained on every trial.
    pub key: String,
    /// Inclusive finite lower and upper bounds in millimetres; lower must be strictly smaller.
    pub bounds_mm: [f64; 2],
    /// Explicit starting coordinate inside the domain; no zero or body-derived start is inferred.
    pub initial_mm: f64,
    /// Positive first polling step, no larger than the declared domain span.
    pub initial_step_mm: f64,
    /// Positive final polling step; no larger than the initial step.
    pub minimum_step_mm: f64,
    /// Canonical linked sites, preserving the existing grid site's units and mark semantics.
    pub sites: Vec<Site>,
}

/// Adaptive discovery policy, exclusive with explicit designs and finite grids.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Raw fit-band discovery or an explicit production comparison; never an inferred fit score.
    pub objective: Objective,
    /// Opt-in approach to a feasible production region after flat fit passes. Unavailable sources
    /// never supply residuals, and failed constraints never enter the retained eligible designs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraint_restoration: Option<ConstraintRestoration>,
    /// Omission preserves axis-only discovery. Joint steps still require complete canonical
    /// qualification and consume the same unique-trial allowance; no intermediate is admitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polling: Option<Polling>,
    /// Exact material variant used to guide consumer polling. Required only for consumer guidance;
    /// selected geometries still cross every authored material alternative after discovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_key: Option<String>,
    /// Ordered material alternatives whose independent Pareto frontiers jointly guide discovery.
    /// Exclusive with material_key; each geometry is qualified once before these actual probes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_keys: Option<Vec<String>>,
    /// Named scenario whose comparison objectives guide matrix polling. Every case must still
    /// admit the trial. Forbidden for worst-case and all-case Pareto guidance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_key: Option<String>,
    /// One to three variables; axis polls retain authored order and negative then positive direction.
    pub variables: Vec<Variable>,
    /// Optional additional starts after local minimum-step stops. Each point names every variable.
    /// All starts share the same unique-trial, consumer, protocol and retention allowances.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restart_points_mm: Vec<BTreeMap<String, f64>>,
    /// Explicit canonical operations after each trial's linked alterations, such as seam truing.
    #[serde(default)]
    pub after: Vec<Operation>,
    /// One to 256 unique parameter trials, excluding the separately retained original baseline.
    pub max_evaluations: usize,
    /// One to 31 qualifying designs retained for ordinary production consumers, plus baseline.
    pub max_designs: usize,
}

/// Why candidate discovery stopped; none of these establishes a global or continuous optimum.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The declared number of fully flat-qualified designs was retained.
    QualifiedDesignLimit,
    /// The unique-trial allowance was exhausted; remaining feasibility is unknown.
    EvaluationBudget,
    /// Scalar polling found no improvement, or every current Pareto center was polled at the final
    /// steps. Other coordinates and consumers of unretained trials remain unknown.
    MinimumSteps,
}

/// One attempted local search within an explicitly requested multi-start discovery.
#[derive(Clone, Debug, Serialize)]
pub struct SearchStart {
    /// Zero is the variables' initial point; n > 0 is restart_points_mm[n - 1].
    pub start_index: usize,
    /// Original trial at this start, possibly reused from the shared coordinate cache.
    pub initial_trial_key: String,
    /// Last polling centre; it need not satisfy the production or flat-fit gates.
    pub final_trial_key: String,
    /// New unique trials spent here, including the start when it was not previously visited.
    pub evaluations: usize,
    /// Local termination reason. Only minimum_steps permits another declared start.
    pub stop_reason: StopReason,
    /// Actual polling steps at this local stop, keyed by the authored variables.
    pub final_steps_mm: BTreeMap<String, f64>,
}

/// One actual canonical trial, including geometry, body identity and refusal evidence.
#[derive(Clone, Debug, Serialize)]
pub struct Trial {
    /// Exact original/altered identity, operations, scalar coordinates and canonical validation.
    pub design: super::Receipt,
    /// Retained for the final material cross-product; removed consumer incumbents remain visible.
    pub selected: bool,
    /// Unmodified nominated-case comparator decision after flat admission. All-case Pareto mode
    /// omits this field: consumer_cases retains every original decision without a nominee.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer_comparison: Option<comparison::Candidate>,
    /// Original binding or additional-consumer refusal, kept even when comparison admission
    /// reports a more general missing metric. Only the explicit comparison controls selection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer_refusal: Option<Refusal>,
    /// Unmodified decisions for every named case, in authored order. A nominated-case improvement
    /// cannot select a trial that another case excludes; independent callback refusals stay visible.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub consumer_cases: Vec<CaseDecision>,
    /// Original decisions for every nominated material, in authored order. Present only for
    /// material_keys guidance; one unavailable material cannot erase another's admitted design.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub consumer_materials: Vec<MaterialDecision>,
    /// Case supplying the actual worst-case reading, absent when case admission failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer_case_key: Option<String>,
    /// Conservative worst-case range and its endpoint witnesses, only for reported-range guidance.
    /// The original nominal-case comparison and every per-case reading remain unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer_worst_case: Option<WorstCaseRange>,
    /// Unrounded shared point readings, or the concrete reason they could not be admitted.
    pub raw_fit: Outcome<Vec<fit::RawReading>>,
    /// Maximum absolute distance outside the declared bands; unknown for inadmissible geometry.
    pub max_fit_band_violation_mm: Option<f64>,
    /// Sum of absolute band distances, used only to break maximum-violation ties.
    pub total_fit_band_violation_mm: Option<f64>,
    /// Complete source-qualified guard vector across all nominated materials and named cases,
    /// only for opted-in restoration. Absence means this trial cannot guide that phase.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraint_violations: Option<Vec<ConstraintViolation>>,
    /// Canonical geometry/fit refusal or consumer-admission/protocol exclusion, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

/// One guard's two independent residuals and the exact consumer context that produced them.
#[derive(Clone, Debug, Serialize)]
pub struct ConstraintViolation {
    /// Original nominated material identity, without a generated design prefix.
    pub material_key: String,
    /// Original named case; absent for manufacturing and single-scenario discovery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub case_key: Option<String>,
    /// Metric, basis, unit and separate minimum/maximum distances from the ordinary comparator.
    #[serde(flatten)]
    pub violation: comparison::ConstraintViolation,
}

/// One named case's ordinary comparison decision during an adaptive production probe.
#[derive(Clone, Debug, Serialize)]
pub struct CaseDecision {
    /// Authored case identity, independent of its display or evaluation order.
    pub key: String,
    /// Exact ordinary source-qualified decision, including readings and all exclusions.
    pub comparison: comparison::Candidate,
    /// Original binding or scenario callback refusal, even when the policy does not require it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

/// One material's complete ordinary comparison admission at a shared geometry coordinate.
#[derive(Clone, Debug, Serialize)]
pub struct MaterialDecision {
    /// Exact authored material identity, independent of the canonical design/material pair key.
    pub material_key: String,
    /// True only after this material's ordinary policy and all requested cases admit the probe.
    pub eligible: bool,
    /// Original comparison for single/nominated-case Pareto guidance; absent in all-case mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<comparison::Candidate>,
    /// All original named-case decisions, without a synthetic aggregate material score.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<CaseDecision>,
    /// Original binding or callback refusal, including independent consumers not used for ranking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
    /// Ordinary policy or this material's protocol admission refusal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admission_refusal: Option<Refusal>,
}

/// Conditional interval enclosing the worst case of a single metric over the named case set.
/// Maxima/minima act separately on each endpoint; no independence or statistical claim is made.
#[derive(Clone, Debug, Serialize)]
pub struct WorstCaseRange {
    /// Worst nominal value with the envelope of all admitted case intervals, in the original unit.
    pub reading: comparison::Reading,
    /// Cases supplying the lower and upper endpoints, respectively. They may differ from each
    /// other and from the nominal limiting case. Authored order resolves equal endpoints.
    pub reported_bound_case_keys: [String; 2],
}

/// Exact finite discovery history within the authored continuous parameter box.
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    /// Always ftx-design-adaptive-search/1.
    pub schema: &'static str,
    /// Complete original policy, including all bounds, steps, sites and allowances.
    pub request: Request,
    /// Unique executed trials, excluding the original baseline and cached repeated coordinates.
    pub evaluations: usize,
    /// Trials satisfying every canonical geometry and explicit selected-body fit gate.
    pub qualified_designs: usize,
    /// Qualified trials retained for real consumers, excluding the original baseline.
    pub selected_designs: usize,
    /// Explicit stopping condition.
    pub stop_reason: StopReason,
    /// True only when the unique-trial allowance stopped discovery.
    pub budget_exhausted: bool,
    /// Always false: coordinate polling does not enumerate the continuous domain.
    pub complete_search: bool,
    /// Always false, including a zero fit-band violation or a step-limited stop.
    pub global_optimum_claim: bool,
    /// Best admitted scalar discovery trial, which can still be out of band; never an automatic
    /// winner. Pareto discovery has no single best trial and serializes this as null.
    pub best_trial_key: Option<String>,
    /// Best discovery maximum residual, not a manufacturing comparison metric.
    pub best_max_fit_band_violation_mm: Option<f64>,
    /// Best discovery secondary residual.
    pub best_total_fit_band_violation_mm: Option<f64>,
    /// Polling steps at termination, indexed by the original variable identities.
    pub final_steps_mm: BTreeMap<String, f64>,
    /// Attempted starts, including the initial point, only when restart points were requested.
    /// Remaining declared starts were not explored. Budgets, caches and protocols never reset.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub search_starts: Vec<SearchStart>,
    /// Actual production guidance and its bounded callback-slot reservation, when requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer_guidance: Option<Box<ConsumerGuidance>>,
    /// Every unique executed trial in deterministic evaluation order.
    pub rows: Vec<Trial>,
}

/// Explicit nominated materials and the existing comparison authority used for local polling.
#[derive(Clone, Debug, Serialize)]
pub struct ConsumerGuidance {
    /// Exact authored material key; other material alternatives do not silently steer discovery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_key: Option<String>,
    /// Ordered nominees for joint discovery. Absent for the original single-material policy.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub material_keys: Vec<String>,
    /// Explicit union of independently admitted material frontiers, without cross-material scores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_aggregation: Option<&'static str>,
    /// Each material's own frontier and source protocol; the common frontier contains their union.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub material_frontiers: Vec<MaterialFrontier>,
    /// Explicit nominated case for matrix guidance; absent for single-case/manufacturing studies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub case_key: Option<String>,
    /// Every case whose ordinary policy must admit a selected trial, in authored order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub case_order: Vec<String>,
    /// All-case Pareto policies in original units and bases; absent in nominated/scalar modes.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub case_policies: BTreeMap<String, comparison::Policy>,
    /// Explicit worst-case or all-case Pareto aggregation; omitted for nominated-case behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub case_aggregation: Option<&'static str>,
    /// The sole objective for scalar guidance; omitted for an explicit Pareto search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective: Option<comparison::Objective>,
    /// Complete ordinary objectives for Pareto guidance, without cross-unit normalization.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub objectives: Vec<comparison::Objective>,
    /// Nondominated evaluated trials and the separate bounded cache of actual consumer outputs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pareto_frontier: Option<ParetoFrontier>,
    /// Explicit range ordering when requested; absent preserves existing nominal receipts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<comparison::Basis>,
    /// Conservative upper bound on probe plus final consumer slots. Scenario work is still
    /// admitted per callback by the caller's shared production-session budgets.
    pub reserved_evaluations: usize,
    /// Actual design/material probes, after full canonical geometry and fit admission.
    pub evaluations: usize,
    /// Best admitted retained scalar discovery trial, excluding baseline; none for Pareto mode.
    pub best_trial_key: Option<String>,
    /// Scalar objective reading; none for Pareto mode. Worst-case range guidance encloses all case
    /// ranges without normalization.
    pub best_reading: Option<comparison::Reading>,
    /// Case supplying the best incumbent's worst-case reading, not a fixed guidance case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_case_key: Option<String>,
    /// Lower/upper range witnesses of the retained worst-case trial; omitted in nominal mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_reported_bound_case_keys: Option<[String; 2]>,
    /// First admitted input protocol; differing protocols cannot steer unless explicitly allowed.
    pub input_protocol_sha256: Option<String>,
}

/// Conditional frontier of admitted discovery trials, excluding the separately retained baseline.
/// An unretained frontier trial still guides polling; its full consumer output is not replayed.
#[derive(Clone, Debug, Serialize)]
pub struct ParetoFrontier {
    /// All nondominated admitted trials among the evaluated coordinates, in evaluation order.
    pub evaluated_trial_keys: Vec<String>,
    /// Frontier trials whose exact full outputs enter the final material cross-product.
    pub retained_trial_keys: Vec<String>,
    /// Evaluated frontier trials omitted when the output cache was full. Later freed slots do not
    /// reconstruct or rerun their earlier outputs; this list does not imply a consumer refusal.
    pub unretained_trial_keys: Vec<String>,
    /// Maximum cached discovery designs, excluding baseline; not a limit on frontier polling.
    pub retention_capacity: usize,
}

/// One nominee's lightweight decision frontier; retained geometries cache all nominated outputs.
#[derive(Clone, Debug, Serialize)]
pub struct MaterialFrontier {
    /// Exact nominee identity, in the original material_keys order.
    pub material_key: String,
    /// Actual probes for this material, excluding cached final-row reuse.
    pub evaluations: usize,
    /// Single-case protocol anchor; named cases retain their independent anchors in their decisions.
    pub input_protocol_sha256: Option<String>,
    /// This material's evaluated frontier and its subset with complete retained outputs.
    pub pareto_frontier: ParetoFrontier,
}

impl Request {
    pub(in crate::material_study) fn consumer_guided(&self) -> bool {
        matches!(
            self.objective,
            Objective::ConsumerComparison
                | Objective::ConsumerPareto
                | Objective::ConsumerMatrixPareto
                | Objective::ConsumerWorstCase
        )
    }

    pub(in crate::material_study) fn pareto(&self) -> bool {
        matches!(
            self.objective,
            Objective::ConsumerPareto | Objective::ConsumerMatrixPareto
        )
    }

    pub(in crate::material_study) fn matrix_pareto(&self) -> bool {
        matches!(self.objective, Objective::ConsumerMatrixPareto)
    }

    pub(in crate::material_study) fn worst_case(&self) -> bool {
        matches!(self.objective, Objective::ConsumerWorstCase)
    }

    pub(in crate::material_study) fn multiple_materials(&self) -> bool {
        self.material_keys.is_some()
    }

    pub(in crate::material_study) fn guidance_material_count(&self) -> usize {
        self.material_keys
            .as_ref()
            .map_or(usize::from(self.material_key.is_some()), Vec::len)
    }

    fn guides_material(&self, key: &str) -> bool {
        self.material_keys.as_ref().map_or_else(
            || self.material_key.as_deref() == Some(key),
            |keys| keys.iter().any(|candidate| candidate == key),
        )
    }

    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if self.constraint_restoration.is_some() && !self.consumer_guided() {
            return Err(invalid(
                "constraint_restoration requires explicit production-consumer guidance",
            ));
        }
        if self.consumer_guided() != (self.material_key.is_some() || self.material_keys.is_some())
            || (self.material_key.is_some() && self.material_keys.is_some())
            || self
                .material_key
                .as_ref()
                .is_some_and(|key| key.trim().is_empty())
        {
            return Err(invalid(if self.material_keys.is_some() {
                "consumer guidance needs one nonempty material_key or an exclusive material_keys list; fit_band_violation_mm accepts neither"
            } else {
                "consumer guidance needs one nonempty material_key; fit_band_violation_mm does not accept material_key"
            }));
        }
        if let Some(keys) = &self.material_keys {
            let mut unique = BTreeSet::new();
            if !self.pareto()
                || !(2..=16).contains(&keys.len())
                || keys
                    .iter()
                    .any(|key| key.trim().is_empty() || !unique.insert(key))
            {
                return Err(invalid(
                    "material_keys needs 2..16 unique nonempty nominees and consumer_pareto or consumer_matrix_pareto guidance",
                ));
            }
        }
        if self.case_key.as_ref().is_some_and(|key| {
            !matches!(
                self.objective,
                Objective::ConsumerComparison | Objective::ConsumerPareto
            ) || key.trim().is_empty()
                || key.len() > 128
        }) {
            return Err(invalid(
                "adaptive case_key needs consumer_comparison or consumer_pareto and a nonempty case name of at most 128 bytes",
            ));
        }
        if serde_json::to_vec(self)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > 256 * 1024
        {
            return Err(budget("adaptive design request exceeds 256 KiB"));
        }
        if !(1..=3).contains(&self.variables.len())
            || !(1..=256).contains(&self.max_evaluations)
            || !(1..=MAX_DESIGNS).contains(&self.max_designs)
        {
            return Err(invalid(
                "adaptive design needs 1..3 variables, max_evaluations in 1..=256 and max_designs in 1..=31",
            ));
        }
        let operations = self
            .variables
            .iter()
            .fold(self.after.len(), |n, v| n.saturating_add(v.sites.len()));
        if operations > MAX_OPERATIONS {
            return Err(budget("adaptive design exceeds 16 operations per trial"));
        }
        let mut keys = BTreeSet::new();
        for variable in &self.variables {
            let [lo, hi] = variable.bounds_mm;
            let span = hi - lo;
            if variable.key.trim().is_empty()
                || !keys.insert(&variable.key)
                || [
                    lo,
                    hi,
                    span,
                    variable.initial_mm,
                    variable.initial_step_mm,
                    variable.minimum_step_mm,
                ]
                .iter()
                .any(|v| !v.is_finite())
                || lo >= hi
                || !(lo..=hi).contains(&variable.initial_mm)
                || variable.minimum_step_mm <= 0.0
                || variable.initial_step_mm < variable.minimum_step_mm
                || variable.initial_step_mm > span
                || variable.sites.is_empty()
            {
                return Err(invalid(
                    "adaptive variables need unique keys, finite ordered bounds, an in-domain initial value and 0 < minimum_step_mm <= initial_step_mm <= domain span",
                ));
            }
            for site in &variable.sites {
                site.validate(&[lo, hi, variable.initial_mm])?;
            }
        }
        if self.restart_points_mm.len() > 31 {
            return Err(budget(
                "adaptive design accepts at most 31 additional start points",
            ));
        }
        let initial: Vec<_> = self.variables.iter().map(|v| v.initial_mm).collect();
        let mut starts = BTreeSet::from([coordinate_identity(&initial)]);
        for point in &self.restart_points_mm {
            if point.len() != self.variables.len() {
                return Err(invalid(
                    "each adaptive restart must name every variable exactly once",
                ));
            }
            let mut values = Vec::with_capacity(self.variables.len());
            for variable in &self.variables {
                let value = point.get(&variable.key).ok_or_else(|| {
                    invalid("each adaptive restart must name every variable exactly once")
                })?;
                if !value.is_finite()
                    || !(variable.bounds_mm[0]..=variable.bounds_mm[1]).contains(value)
                {
                    return Err(invalid(
                        "adaptive restart coordinates must be finite and inside their declared bounds",
                    ));
                }
                for site in &variable.sites {
                    site.validate(&[*value])?;
                }
                values.push(*value);
            }
            if !starts.insert(coordinate_identity(&values)) {
                return Err(invalid(
                    "adaptive starts must be distinct, including the initial point",
                ));
            }
        }
        Ok(())
    }

    fn design(
        &self,
        index: usize,
        coordinates: &[f64],
    ) -> Result<(Variant, BTreeMap<String, f64>), Refusal> {
        let mut operations = Vec::new();
        let mut values = BTreeMap::new();
        for (variable, &value) in self.variables.iter().zip(coordinates) {
            values.insert(variable.key.clone(), value);
            for site in &variable.sites {
                operations.push(site.operation(value)?);
            }
        }
        operations.extend(self.after.iter().cloned());
        Ok((
            Variant {
                key: format!("adaptive:{index:04}"),
                operations,
            },
            values,
        ))
    }

    fn neighbors(&self, center: &[f64], steps: &[f64]) -> Vec<Vec<f64>> {
        let endpoints: Vec<_> = self
            .variables
            .iter()
            .enumerate()
            .map(|(axis, variable)| {
                [false, true].map(|positive| {
                    let bound = variable.bounds_mm[usize::from(positive)];
                    let distance = (bound - center[axis]).abs();
                    // A reached boundary is an authored coordinate, not a repaired input.
                    let next = if steps[axis] >= distance {
                        bound
                    } else if positive {
                        center[axis] + steps[axis]
                    } else {
                        center[axis] - steps[axis]
                    };
                    (next != center[axis]).then_some(next)
                })
            })
            .collect();
        let dimensions = self.variables.len();
        let largest_group = if self.polling.is_some() { dimensions } else { 1 };
        let mut neighbors = Vec::new();
        // At most 3^3 - 1 = 26 points. Cardinality first preserves every old axis poll;
        // authored axis subsets and lexicographic signs make the added work reproducible.
        for count in 1..=largest_group {
            for mask in 1usize..(1 << dimensions) {
                if mask.count_ones() as usize != count {
                    continue;
                }
                let axes: Vec<_> = (0..dimensions)
                    .filter(|&axis| mask & (1 << axis) != 0)
                    .collect();
                for signs in 0..(1usize << count) {
                    let mut point = center.to_vec();
                    let mut moved = true;
                    for (position, &axis) in axes.iter().enumerate() {
                        let positive = (signs >> (count - 1 - position)) & 1;
                        if let Some(next) = endpoints[axis][positive] {
                            point[axis] = next;
                        } else {
                            // A joint direction must move every participating variable; otherwise
                            // it duplicates an axis or smaller group already polled this round.
                            moved = false;
                            break;
                        }
                    }
                    if moved {
                        neighbors.push(point);
                    }
                }
            }
        }
        neighbors
    }

    fn reserve(
        &self,
        study: &StudyRequest,
        pattern: &Pattern,
        materials: &[PreparedVariant],
        context: &fit::Context<'_>,
    ) -> Result<(), Refusal> {
        let rows = (self.max_designs + 1).saturating_mul(materials.len());
        if rows > 32 {
            return Err(budget(
                "adaptive design exceeds 32 reserved design/material alternatives, including baseline",
            ));
        }
        let trials = self.max_evaluations + 1;
        let operations = self
            .variables
            .iter()
            .fold(self.after.len(), |n, v| n.saturating_add(v.sites.len()));
        if operations.saturating_mul(self.max_evaluations) > 4096 {
            return Err(budget(
                "adaptive design exceeds 4096 reserved alteration operations",
            ));
        }
        let treatment = super::treatment::reserve(pattern, &self.after)?;
        let mut treatment_work = super::treatment::Reservation::default();
        treatment_work.add(treatment, self.max_evaluations)?;
        // Adaptive trials retain every completed operation report, including fit refusals. Chosen
        // trials also keep cached geometry and later clone their receipts across material rows.
        treatment_work.bytes = treatment_work.bytes.saturating_add(
            treatment
                .bytes
                .saturating_mul(self.max_designs)
                .saturating_mul(materials.len().saturating_add(2)),
        );
        let outline_work = pattern
            .instances
            .iter()
            .fold(0usize, |total, instance| {
                let points = instance.edges.iter().fold(0usize, |count, edge| {
                    count.saturating_add(edge.curve.as_ref().map_or(1, |curve| {
                        if curve.is_line() {
                            1
                        } else {
                            OUTLINE_DENSE.max(curve.controls().len().saturating_add(1))
                        }
                    }))
                });
                total.saturating_add(points.saturating_mul(points.saturating_add(1)) / 2)
            })
            .saturating_mul(trials);
        if outline_work > 40_000_000 {
            return Err(budget(
                "adaptive design exceeds 40000000 reserved outline-validation work units",
            ));
        }
        let dense = ftx_core::curve::MEASURE_DENSE.max(ftx_core::curve::EDGE_DENSE);
        let samples = pattern
            .instances
            .iter()
            .flat_map(|instance| &instance.edges)
            .fold(0usize, |count, edge| {
                count.saturating_add(edge.curve.as_ref().map_or(dense, |curve| {
                    dense.max(curve.controls().len().saturating_add(2))
                }))
            });
        let fit = study
            .design_fit
            .as_ref()
            .expect("validated adaptive fit request");
        // Canonical validation and the shared raw readings both sample actual geometry.
        if fit
            .measurement_work_units()
            .saturating_mul(samples.max(dense))
            .saturating_mul(trials)
            .saturating_mul(2)
            > 40_000_000
        {
            return Err(budget(
                "adaptive design exceeds 40000000 reserved measurement work units",
            ));
        }
        let passes = self
            .variables
            .iter()
            .map(|v| v.sites.len())
            .sum::<usize>()
            .saturating_add(
                self.after
                    .iter()
                    .map(Operation::document_passes)
                    .sum::<usize>(),
            )
            .saturating_mul(self.max_evaluations);
        let source_bytes = serde_json::to_vec(pattern.raw())
            .map_err(|e| invalid(e.to_string()))?
            .len();
        let request_bytes = serde_json::to_vec(&(self, fit, context.identity))
            .map_err(|e| invalid(e.to_string()))?
            .len();
        let material_bytes = materials.iter().try_fold(0usize, |count, material| {
            Ok::<_, Refusal>(
                count.saturating_add(
                    serde_json::to_vec(&material.variant)
                        .map_err(|e| invalid(e.to_string()))?
                        .len(),
                ),
            )
        })?;
        let probe_material_bytes = if self.consumer_guided() {
            materials
                .iter()
                .filter(|material| self.guides_material(&material.variant.key))
                .try_fold(0usize, |count, material| {
                    let bytes = serde_json::to_vec(&material.variant)
                        .map_err(|e| invalid(e.to_string()))?;
                    Ok::<_, Refusal>(count.saturating_add(bytes.len()))
                })?
                .saturating_mul(self.max_evaluations)
        } else {
            0
        };
        if source_bytes
            .saturating_add(request_bytes)
            .saturating_mul(
                passes
                    .saturating_add(trials.saturating_mul(4))
                    .saturating_add(rows.saturating_mul(3))
                    .saturating_add(if self.consumer_guided() {
                        self.max_evaluations
                            .saturating_mul(self.guidance_material_count())
                            .saturating_mul(3)
                    } else {
                        0
                    }),
            )
            .saturating_add(material_bytes.saturating_mul(self.max_designs + 1))
            .saturating_add(probe_material_bytes)
            .saturating_add(treatment_work.bytes)
            > MAX_DOCUMENT_WORK_BYTES
        {
            return Err(budget(
                "adaptive design exceeds 256 MiB reserved document-processing work",
            ));
        }
        Ok(())
    }

    pub(super) fn expand(
        &self,
        study: &StudyRequest,
        pattern: &Pattern,
        materials: Vec<PreparedVariant>,
        context: Option<&fit::Context<'_>>,
    ) -> Result<(Vec<PreparedVariant>, Receipt), Refusal> {
        if self.consumer_guided() {
            return Err(invalid(
                "consumer-guided adaptive discovery requires the production consumer entry point",
            ));
        }
        let (variants, receipt, _) =
            self.expand_with::<()>(study, pattern, materials, context, None, &[])?;
        Ok((variants, receipt))
    }

    pub(in crate::material_study) fn expand_with<T>(
        &self,
        study: &StudyRequest,
        pattern: &Pattern,
        materials: Vec<PreparedVariant>,
        context: Option<&fit::Context<'_>>,
        evaluate: Option<&mut ConsumerEvaluator<'_, T>>,
        cases: &[(String, comparison::Policy)],
    ) -> Result<Expansion<T>, Refusal> {
        self.validate()?;
        if self.constraint_restoration.is_some()
            && if cases.is_empty() {
                study
                    .comparison
                    .as_ref()
                    .is_none_or(|policy| policy.constraints.is_empty())
            } else {
                cases
                    .iter()
                    .all(|(_, policy)| policy.constraints.is_empty())
            }
        {
            return Err(invalid(
                "constraint_restoration needs at least one explicit constraint in the effective comparison policies",
            ));
        }
        if (self.worst_case() || self.matrix_pareto()) && cases.is_empty() {
            return Err(invalid(
                "all-case consumer guidance requires a named scenario matrix",
            ));
        }
        let context = context.ok_or_else(|| {
            invalid("design_adaptive needs the selected scenario's fitted-body measurement context")
        })?;
        context.validate()?;
        let fit = study
            .design_fit
            .as_ref()
            .expect("validated adaptive fit request");
        let mut keys = BTreeSet::new();
        if materials.iter().any(|material| {
            material.variant.key.trim().is_empty() || !keys.insert(&material.variant.key)
        }) {
            return Err(invalid("material variant keys must be unique and nonempty"));
        }
        self.reserve(study, pattern, &materials, context)?;
        let guidance = if self.consumer_guided() {
            let keys = self.material_keys.as_deref().unwrap_or_else(|| {
                std::slice::from_ref(self.material_key.as_ref().expect("validated material key"))
            });
            let nominees = keys
                .iter()
                .map(|key| {
                    let source = materials
                        .iter()
                        .find(|material| material.variant.key == *key)
                        .ok_or_else(|| {
                            invalid(if self.multiple_materials() {
                                "every adaptive material nominee must identify an admitted material alternative"
                            } else {
                                "adaptive material_key must identify an admitted material alternative"
                            })
                        })?;
                    Ok(GuidedMaterial {
                        source,
                        protocol: None,
                        evaluations: 0,
                    })
                })
                .collect::<Result<Vec<_>, Refusal>>()?;
            let policy = study
                .comparison
                .as_ref()
                .expect("validated consumer comparison");
            Some(Guidance {
                materials: nominees,
                policy,
                evaluate: evaluate.ok_or_else(|| {
                    invalid("consumer-guided adaptive discovery needs its production evaluator")
                })?,
                evaluations: 0,
                case_order: cases.iter().map(|(key, _)| key.clone()).collect(),
                case_policies: if self.matrix_pareto() {
                    cases.iter().map(|(_, policy)| policy.clone()).collect()
                } else {
                    Vec::new()
                },
                reserved_evaluations: self
                    .max_evaluations
                    .saturating_mul(self.guidance_material_count())
                    .saturating_add((self.max_designs + 1).saturating_mul(materials.len()))
                    .saturating_mul(cases.len().max(1)),
            })
        } else {
            None
        };
        let mut evaluator = Evaluator {
            source: pattern,
            fit,
            context,
            seam_tolerance_mm: study
                .sewing_options()
                .map_or(3.0, |options| options.seam_tolerance_mm),
            work: Work::new(40_000_000),
        };
        let baseline = Variant {
            key: super::BASELINE.into(),
            operations: Vec::new(),
        };
        let evaluated = evaluator.apply(&baseline, None)?;
        let mut discovery = Discovery {
            request: self,
            evaluator,
            guidance,
            rows: Vec::new(),
            chosen: vec![Chosen {
                variant: baseline,
                values: None,
                evaluated,
                index: None,
                consumers: BTreeMap::new(),
            }],
            seen: BTreeMap::new(),
            coordinates: Vec::new(),
            pareto_frontier: Vec::new(),
            material_frontiers: vec![Vec::new(); self.guidance_material_count()],
        };
        let mut current = discovery
            .inspect(
                self.variables
                    .iter()
                    .map(|variable| variable.initial_mm)
                    .collect(),
            )?
            .expect("positive trial allowance");
        let mut search_starts = Vec::new();
        let mut start_index = 0;
        let mut start_initial = Some(current);
        let mut start_evaluations = 0;
        let mut steps: Vec<_> = self
            .variables
            .iter()
            .map(|variable| variable.initial_step_mm)
            .collect();
        let stop_reason = loop {
            let local_stop = if self.pareto() {
                discovery.poll_pareto(&mut current, &mut steps, start_index > 0)?
            } else {
                'search: loop {
                    if !self.consumer_guided() && discovery.chosen.len() > self.max_designs {
                        break StopReason::QualifiedDesignLimit;
                    }
                    if discovery.rows.len() >= self.max_evaluations {
                        break StopReason::EvaluationBudget;
                    }
                    let center = discovery.coordinates[current].clone();
                    let mut best = current;
                    for point in self.neighbors(&center, &steps) {
                        let Some(index) = discovery.inspect(point)? else {
                            break 'search StopReason::EvaluationBudget;
                        };
                        if discovery.improves(&discovery.rows[index], &discovery.rows[best]) {
                            best = index;
                        }
                        if !self.consumer_guided() && discovery.chosen.len() > self.max_designs {
                            break 'search StopReason::QualifiedDesignLimit;
                        }
                    }
                    if best != current {
                        current = best;
                        continue;
                    }
                    let mut refined = false;
                    for (step, variable) in steps.iter_mut().zip(&self.variables) {
                        if *step > variable.minimum_step_mm {
                            *step = (*step * 0.5).max(variable.minimum_step_mm);
                            refined = true;
                        }
                    }
                    if !refined {
                        break StopReason::MinimumSteps;
                    }
                }
            };
            if !matches!(local_stop, StopReason::MinimumSteps) {
                break local_stop;
            }
            let Some(point) = self.restart_points_mm.get(start_index) else {
                break StopReason::MinimumSteps;
            };
            search_starts.push(discovery.record_start(
                start_index,
                start_initial.expect("active start"),
                current,
                start_evaluations,
                &steps,
                StopReason::MinimumSteps,
            ));
            let point = self.variables.iter().map(|v| point[&v.key]).collect();
            start_evaluations = discovery.rows.len();
            let Some(index) = discovery.inspect(point)? else {
                start_initial = None;
                break StopReason::EvaluationBudget;
            };
            current = index;
            start_initial = Some(index);
            start_index += 1;
            for (step, variable) in steps.iter_mut().zip(&self.variables) {
                *step = variable.initial_step_mm;
            }
        };
        if !self.restart_points_mm.is_empty()
            && let Some(initial) = start_initial
        {
            search_starts.push(discovery.record_start(
                start_index,
                initial,
                current,
                start_evaluations,
                &steps,
                stop_reason,
            ));
        }
        let mut best: Option<usize> = None;
        for (index, row) in discovery.rows.iter().enumerate() {
            let admitted = if self.consumer_guided() {
                row.consumer_admitted()
            } else {
                row.score().is_some()
            };
            if !self.pareto()
                && admitted
                && best.is_none_or(|previous| discovery.improves(row, &discovery.rows[previous]))
            {
                best = Some(index);
            }
        }
        let receipt = Receipt {
            schema: "ftx-design-adaptive-search/1",
            request: self.clone(),
            evaluations: discovery.rows.len(),
            qualified_designs: discovery
                .rows
                .iter()
                .filter(|row| row.flat_qualified())
                .count(),
            selected_designs: discovery.chosen.len() - 1,
            stop_reason,
            budget_exhausted: matches!(stop_reason, StopReason::EvaluationBudget),
            complete_search: false,
            global_optimum_claim: false,
            best_trial_key: best.map(|index| discovery.rows[index].design.key.clone()),
            best_max_fit_band_violation_mm: best
                .and_then(|index| discovery.rows[index].max_fit_band_violation_mm),
            best_total_fit_band_violation_mm: best
                .and_then(|index| discovery.rows[index].total_fit_band_violation_mm),
            final_steps_mm: self
                .variables
                .iter()
                .zip(steps)
                .map(|(variable, step)| (variable.key.clone(), step))
                .collect(),
            search_starts,
            consumer_guidance: discovery.guidance.as_ref().map(|guidance| {
                Box::new(ConsumerGuidance {
                    material_key: self.material_key.clone(),
                    material_keys: self.material_keys.clone().unwrap_or_default(),
                    material_aggregation: self
                        .multiple_materials()
                        .then_some("union_of_material_frontiers/1"),
                    material_frontiers: if self.multiple_materials() {
                        guidance
                            .materials
                            .iter()
                            .zip(&discovery.material_frontiers)
                            .map(|(material, indices)| MaterialFrontier {
                                material_key: material.source.variant.key.clone(),
                                evaluations: material.evaluations,
                                input_protocol_sha256: material.protocol.clone().flatten(),
                                pareto_frontier: discovery.frontier_receipt(indices),
                            })
                            .collect()
                    } else {
                        Vec::new()
                    },
                    case_key: self.case_key.clone(),
                    case_order: guidance.case_order.clone(),
                    case_policies: guidance
                        .case_order
                        .iter()
                        .cloned()
                        .zip(guidance.case_policies.iter().cloned())
                        .collect(),
                    case_aggregation: if self.matrix_pareto() {
                        Some("pareto")
                    } else {
                        self.worst_case().then_some("worst_case")
                    },
                    objective: (!self.pareto()).then(|| guidance.policy.objectives[0].clone()),
                    objectives: if self.pareto() && !self.matrix_pareto() {
                        guidance.policy.objectives.clone()
                    } else {
                        Vec::new()
                    },
                    pareto_frontier: self
                        .pareto()
                        .then(|| discovery.frontier_receipt(&discovery.pareto_frontier)),
                    basis: (!self.matrix_pareto()
                        && matches!(guidance.policy.basis, comparison::Basis::ReportedBounds))
                    .then_some(guidance.policy.basis),
                    reserved_evaluations: guidance.reserved_evaluations,
                    evaluations: guidance.evaluations,
                    best_trial_key: best.map(|index| discovery.rows[index].design.key.clone()),
                    best_reading: best
                        .and_then(|index| {
                            discovery.rows[index]
                                .consumer_reading(guidance.policy.objectives[0].metric)
                        })
                        .cloned(),
                    best_case_key: best
                        .and_then(|index| discovery.rows[index].consumer_case_key.clone()),
                    best_reported_bound_case_keys: best
                        .and_then(|index| discovery.rows[index].consumer_worst_case.as_ref())
                        .map(|range| range.reported_bound_case_keys.clone()),
                    input_protocol_sha256: if self.multiple_materials() {
                        None
                    } else {
                        guidance.materials[0].protocol.clone().flatten()
                    },
                })
            }),
            rows: discovery.rows,
        };
        let mut expanded = Vec::new();
        let mut retained = BTreeMap::new();
        for chosen in discovery.chosen {
            for (material_key, consumer) in chosen.consumers {
                let key = serde_json::to_string(&(&chosen.variant.key, material_key))
                    .map_err(|e| invalid(e.to_string()))?;
                retained.insert(key, consumer);
            }
            for material in &materials {
                let mut variant = material.variant.clone();
                variant.key = serde_json::to_string(&(&chosen.variant.key, &material.variant.key))
                    .map_err(|e| invalid(e.to_string()))?;
                expanded.push(PreparedVariant {
                    variant,
                    choices: material.choices.clone(),
                    design: Some(Prepared {
                        variant: chosen.variant.clone(),
                        material_key: Some(material.variant.key.clone()),
                        variables_mm: chosen.values.clone(),
                        evaluated: Some(chosen.evaluated.clone()),
                    }),
                });
            }
        }
        Ok((expanded, receipt, retained))
    }
}

/// Select an original case reading, not a synthetic score. All cases must already be admitted;
/// their protocols are compared across trials by the matrix, not against a different case.
pub(in crate::material_study) fn worst_case_index(
    cases: &[CaseDecision],
    objective: &comparison::Objective,
) -> Result<usize, Refusal> {
    let mut worst: Option<(usize, &comparison::Reading)> = None;
    for (index, case) in cases.iter().enumerate() {
        let reading = case
            .comparison
            .readings
            .get(&objective.metric)
            .filter(|reading| case.comparison.eligible && reading.value.is_finite())
            .ok_or_else(|| invalid("worst-case guidance needs every case's admitted reading"))?;
        if let Some((_, previous)) = worst {
            if reading.unit != previous.unit || reading.consumer != previous.consumer {
                return Err(invalid(
                    "worst-case readings must have identical units and consumers",
                ));
            }
            let worse = match objective.direction {
                Direction::Minimize => reading.value > previous.value,
                Direction::Maximize => reading.value < previous.value,
            };
            if !worse {
                continue;
            }
        }
        worst = Some((index, reading));
    }
    worst
        .map(|(index, _)| index)
        .ok_or_else(|| invalid("worst-case guidance needs at least one named case"))
}

fn worst_case_range(
    cases: &[CaseDecision],
    objective: &comparison::Objective,
) -> Result<WorstCaseRange, Refusal> {
    let nominal = worst_case_index(cases, objective)?;
    let mut reading = cases[nominal].comparison.readings[&objective.metric].clone();
    let mut envelope: Option<[f64; 2]> = None;
    let mut witnesses = [0; 2];
    for (index, case) in cases.iter().enumerate() {
        let source = &case.comparison.readings[&objective.metric];
        let bounds = source
            .reported_bounds
            .filter(|[lo, hi]| {
                lo.is_finite() && hi.is_finite() && *lo <= source.value && source.value <= *hi
            })
            .ok_or_else(|| {
                invalid("worst-case range needs every case's complete finite reported bounds")
            })?;
        if let Some(current) = &mut envelope {
            for endpoint in 0..2 {
                let worse = match objective.direction {
                    Direction::Minimize => bounds[endpoint] > current[endpoint],
                    Direction::Maximize => bounds[endpoint] < current[endpoint],
                };
                if worse {
                    current[endpoint] = bounds[endpoint];
                    witnesses[endpoint] = index;
                }
            }
        } else {
            envelope = Some(bounds);
        }
    }
    reading.reported_bounds = envelope;
    Ok(WorstCaseRange {
        reading,
        reported_bound_case_keys: witnesses.map(|index| cases[index].key.clone()),
    })
}

pub(in crate::material_study) struct ConsumerEvaluation<T> {
    pub comparison: comparison::Candidate,
    pub refusal: Option<Refusal>,
    pub cases: Vec<CaseDecision>,
    pub admission_refusal: Option<Refusal>,
    pub output: T,
}

type ConsumerEvaluator<'a, T> =
    dyn FnMut(PreparedVariant) -> Result<ConsumerEvaluation<T>, Refusal> + 'a;
type Expansion<T> = (Vec<PreparedVariant>, Receipt, BTreeMap<String, T>);

struct Guidance<'a, T> {
    materials: Vec<GuidedMaterial<'a>>,
    policy: &'a comparison::Policy,
    evaluate: &'a mut ConsumerEvaluator<'a, T>,
    evaluations: usize,
    case_order: Vec<String>,
    case_policies: Vec<comparison::Policy>,
    reserved_evaluations: usize,
}

struct GuidedMaterial<'a> {
    source: &'a PreparedVariant,
    protocol: Option<Option<String>>,
    evaluations: usize,
}

impl<T> Guidance<'_, T> {
    fn probe(
        &mut self,
        material_index: usize,
        request: &Request,
        mut design: Prepared,
    ) -> Result<(MaterialDecision, T), Refusal> {
        let material = &mut self.materials[material_index];
        let key = material.source.variant.key.clone();
        let mut variant = material.source.variant.clone();
        variant.key = serde_json::to_string(&(&design.variant.key, &key))
            .map_err(|e| invalid(e.to_string()))?;
        design.material_key = Some(key.clone());
        self.evaluations += 1;
        material.evaluations += 1;
        let evaluation = (self.evaluate)(PreparedVariant {
            variant,
            choices: material.source.choices.clone(),
            design: Some(design),
        })?;
        let mut candidate = evaluation.comparison;
        let mut cases = evaluation.cases;
        let mut refusal = evaluation.admission_refusal;
        let mut eligible = refusal.is_none() && candidate.eligible;
        let restoration_qualified = request.constraint_restoration.is_some()
            && if cases.is_empty() {
                refusal.is_none() && candidate.constraint_violations.is_some()
            } else {
                // Matrix admission removes these witnesses on any independent protocol failure;
                // an ordinary numeric constraint exclusion remains visible and ineligible.
                cases
                    .iter()
                    .all(|case| case.comparison.constraint_violations.is_some())
            };
        if (eligible || restoration_qualified) && !request.worst_case() && !request.matrix_pareto()
        {
            if material.protocol.as_ref().is_some_and(|protocol| {
                !self.policy.allow_mixed_protocols && *protocol != candidate.input_protocol_sha256
            }) {
                eligible = false;
                candidate.constraint_violations = None;
                for case in &mut cases {
                    case.comparison.constraint_violations = None;
                }
                refusal = Some(invalid(if request.multiple_materials() {
                    "adaptive consumer protocols differ from this material's first admitted incumbent; no ordering inferred"
                } else {
                    "adaptive consumer protocols differ from the first admitted incumbent; no ordering inferred"
                }));
            } else {
                material
                    .protocol
                    .get_or_insert_with(|| candidate.input_protocol_sha256.clone());
            }
        }
        if refusal.is_none() && !candidate.eligible {
            refusal = candidate.exclusions.first().cloned();
        }
        Ok((
            MaterialDecision {
                material_key: key,
                eligible,
                comparison: (!request.matrix_pareto()).then_some(candidate),
                cases,
                refusal: evaluation.refusal,
                admission_refusal: refusal,
            },
            evaluation.output,
        ))
    }
}

struct Chosen<T> {
    variant: Variant,
    values: Option<BTreeMap<String, f64>>,
    evaluated: (super::Receipt, Result<Pattern, Refusal>),
    index: Option<usize>,
    consumers: BTreeMap<String, T>,
}

struct Evaluator<'request, 'body> {
    source: &'request Pattern,
    fit: &'request fit::Request,
    context: &'request fit::Context<'body>,
    seam_tolerance_mm: f64,
    work: Work,
}

impl Evaluator<'_, '_> {
    fn apply(
        &mut self,
        variant: &Variant,
        values: Option<&BTreeMap<String, f64>>,
    ) -> Result<(super::Receipt, Result<Pattern, Refusal>), Refusal> {
        let evaluated = Prepared {
            variant: variant.clone(),
            material_key: None,
            variables_mm: values.cloned(),
            evaluated: None,
        }
        .apply(
            self.source,
            self.seam_tolerance_mm,
            &mut self.work,
            Some(self.fit),
            Some(self.context),
        )?;
        if self.work.exhausted {
            return Err(budget(
                "adaptive design exceeds 40000000 aggregate outline-validation work units",
            ));
        }
        Ok(evaluated)
    }

    fn trial(
        &mut self,
        variant: &Variant,
        values: &BTreeMap<String, f64>,
    ) -> Result<(Trial, Result<Pattern, Refusal>), Refusal> {
        let (design, mut result) = self.apply(variant, Some(values))?;
        let geometry_admitted = design.validation.as_ref().is_some_and(|validation| {
            validation.flat["status"] == "CONVERGED"
                && matches!(&validation.outlines, Outcome::Available { .. })
        });
        let fit_structure_admitted = matches!(&design.fit, Some(Outcome::Available { result })
            if result.validation["summary"]["seams_failing"] == 0
                && result.validation["summary"]["truing_failing"] == 0
                && result.validation["completeness"].as_array().is_some_and(Vec::is_empty)
                && result.validation["missing_measurements"].as_array().is_some_and(Vec::is_empty));
        let raw_fit = Outcome::from_result(if geometry_admitted {
            match &result {
                Ok(pattern) => self.fit.unrounded(pattern, self.context),
                Err(_) => Pattern::from_json(
                    design
                        .altered_pattern_json
                        .as_deref()
                        .expect("validated altered geometry"),
                )
                .map_err(invalid)
                .and_then(|pattern| self.fit.unrounded(&pattern, self.context)),
            }
        } else {
            Err(invalid(
                "adaptive raw fit requires admitted canonical structure, seams, completeness and sampled outlines",
            ))
        });
        let score = if fit_structure_admitted {
            raw_fit.value().and_then(|readings| {
                let maximum = readings
                    .iter()
                    .map(|reading| reading.measurement.residual_mm.abs())
                    .fold(0.0_f64, f64::max);
                let total: f64 = readings
                    .iter()
                    .map(|reading| reading.measurement.residual_mm.abs())
                    .sum();
                total.is_finite().then_some((maximum, total))
            })
        } else {
            None
        };
        if result.is_ok() && score != Some((0.0, 0.0)) {
            result = Err(invalid(
                "adaptive design requires complete finite unrounded readings inside every declared fit band",
            ));
        }
        Ok((
            Trial {
                design,
                selected: result.is_ok(),
                consumer_comparison: None,
                consumer_refusal: None,
                consumer_cases: Vec::new(),
                consumer_materials: Vec::new(),
                consumer_case_key: None,
                consumer_worst_case: None,
                raw_fit,
                max_fit_band_violation_mm: score.map(|score| score.0),
                total_fit_band_violation_mm: score.map(|score| score.1),
                constraint_violations: None,
                refusal: result.as_ref().err().cloned(),
            },
            result,
        ))
    }
}

struct Discovery<'request, 'body, 'consumer, T> {
    request: &'request Request,
    evaluator: Evaluator<'request, 'body>,
    rows: Vec<Trial>,
    guidance: Option<Guidance<'consumer, T>>,
    chosen: Vec<Chosen<T>>,
    seen: BTreeMap<Vec<u64>, usize>,
    coordinates: Vec<Vec<f64>>,
    // Lightweight decisions remain available even when their full outputs exceed retention.
    pareto_frontier: Vec<usize>,
    // Each nominee's own decision frontier. The common polling centers are their stable union.
    material_frontiers: Vec<Vec<usize>>,
}

impl<T> Discovery<'_, '_, '_, T> {
    fn frontier_receipt(&self, indices: &[usize]) -> ParetoFrontier {
        let mut frontier = ParetoFrontier {
            evaluated_trial_keys: Vec::new(),
            retained_trial_keys: Vec::new(),
            unretained_trial_keys: Vec::new(),
            retention_capacity: self.request.max_designs,
        };
        for &index in indices {
            let row = &self.rows[index];
            frontier.evaluated_trial_keys.push(row.design.key.clone());
            if row.selected {
                frontier.retained_trial_keys.push(row.design.key.clone());
            } else {
                frontier.unretained_trial_keys.push(row.design.key.clone());
            }
        }
        frontier
    }

    fn record_start(
        &self,
        start_index: usize,
        initial: usize,
        current: usize,
        before: usize,
        steps: &[f64],
        stop_reason: StopReason,
    ) -> SearchStart {
        SearchStart {
            start_index,
            initial_trial_key: self.rows[initial].design.key.clone(),
            final_trial_key: self.rows[current].design.key.clone(),
            evaluations: self.rows.len() - before,
            stop_reason,
            final_steps_mm: self
                .request
                .variables
                .iter()
                .zip(steps)
                .map(|(variable, &step)| (variable.key.clone(), step))
                .collect(),
        }
    }

    fn poll_pareto(
        &mut self,
        current: &mut usize,
        steps: &mut [f64],
        explore_seed: bool,
    ) -> Result<StopReason, Refusal> {
        let request = self.request;
        let mut fit_center = *current;
        let mut polled = BTreeSet::new();
        loop {
            if self.rows.len() >= request.max_evaluations {
                return Ok(StopReason::EvaluationBudget);
            }
            // Every current nondominated coordinate is a center, including those whose large
            // outputs did not fit the cache. A strict scalar improvement is not required to
            // explore an admitted tradeoff. Shared step rounds keep refinement deterministic.
            // An authored restart is explored even if its seed is dominated or inadmissible.
            // Otherwise a retained frontier in another region could silently erase that start.
            let center_index = if (explore_seed || self.pareto_frontier.is_empty())
                && !polled.contains(&fit_center)
            {
                Some(fit_center)
            } else {
                self.pareto_frontier
                    .iter()
                    .copied()
                    .find(|index| !polled.contains(index))
            };
            let Some(center_index) = center_index else {
                let mut refined = false;
                for (step, variable) in steps.iter_mut().zip(&request.variables) {
                    if *step > variable.minimum_step_mm {
                        *step = (*step * 0.5).max(variable.minimum_step_mm);
                        refined = true;
                    }
                }
                if !refined {
                    return Ok(StopReason::MinimumSteps);
                }
                polled.clear();
                continue;
            };
            polled.insert(center_index);
            *current = center_index;
            let center = self.coordinates[center_index].clone();
            let mut best_fit = fit_center;
            for point in request.neighbors(&center, steps) {
                let Some(index) = self.inspect(point)? else {
                    return Ok(StopReason::EvaluationBudget);
                };
                let candidate = &self.rows[index];
                let previous = &self.rows[best_fit];
                let improved = if request.constraint_restoration.is_some()
                    && !candidate.consumer_admitted()
                    && !previous.consumer_admitted()
                {
                    self.improves(candidate, previous)
                } else {
                    improves(candidate, previous)
                };
                if improved {
                    best_fit = index;
                }
            }
            // Each start can approach the admissible region through its own raw fit readings.
            // Polling another frontier center cannot redirect that local fit search. An admitted
            // center has zero residual; fit never ranks admitted tradeoffs or missing consumers.
            if center_index == fit_center {
                fit_center = best_fit;
            }
        }
    }

    fn retain_pareto(&mut self, index: usize, row: &mut Trial) {
        if self
            .pareto_frontier
            .iter()
            .any(|&previous| self.improves(&self.rows[previous], row))
        {
            row.selected = false;
            return;
        }
        let displaced: BTreeSet<_> = self
            .pareto_frontier
            .iter()
            .copied()
            .filter(|&previous| self.improves(row, &self.rows[previous]))
            .collect();
        for &previous in &displaced {
            self.rows[previous].selected = false;
        }
        self.chosen
            .retain(|chosen| chosen.index.is_none_or(|index| !displaced.contains(&index)));
        self.pareto_frontier
            .retain(|index| !displaced.contains(index));
        self.pareto_frontier.push(index);
        // Earlier incomparable results retain their exact outputs. The new decision still
        // joins the search frontier when full, and may reveal a later dominating geometry.
        row.selected = self.chosen.len() <= self.request.max_designs;
    }

    fn material_improves(&self, candidate: &MaterialDecision, current: &MaterialDecision) -> bool {
        if !candidate.eligible
            || !current.eligible
            || candidate.material_key != current.material_key
        {
            return false;
        }
        let guidance = self.guidance.as_ref().expect("material guidance");
        if self.request.matrix_pareto() {
            cases_prefer(
                &guidance.case_policies,
                &guidance.case_order,
                &candidate.cases,
                &current.cases,
            )
        } else {
            candidate
                .comparison
                .as_ref()
                .zip(current.comparison.as_ref())
                .is_some_and(|(candidate, current)| guidance.policy.prefers(candidate, current))
        }
    }

    fn retain_material_pareto(&mut self, index: usize, row: &mut Trial) {
        let mut nominated = false;
        for (material_index, decision) in row.consumer_materials.iter().enumerate() {
            if !decision.eligible
                || self.material_frontiers[material_index]
                    .iter()
                    .any(|&previous| {
                        self.material_improves(
                            &self.rows[previous].consumer_materials[material_index],
                            decision,
                        )
                    })
            {
                continue;
            }
            let displaced: BTreeSet<_> = self.material_frontiers[material_index]
                .iter()
                .copied()
                .filter(|&previous| {
                    self.material_improves(
                        decision,
                        &self.rows[previous].consumer_materials[material_index],
                    )
                })
                .collect();
            self.material_frontiers[material_index]
                .retain(|previous| !displaced.contains(previous));
            self.material_frontiers[material_index].push(index);
            nominated = true;
        }
        // Remove a retained design only after every material has displaced it. The ordered union
        // also keeps capacity-omitted coordinates available to guide subsequent polling.
        let union: BTreeSet<_> = self.material_frontiers.iter().flatten().copied().collect();
        for &previous in &self.pareto_frontier {
            if !union.contains(&previous) {
                self.rows[previous].selected = false;
            }
        }
        self.chosen
            .retain(|chosen| chosen.index.is_none_or(|index| union.contains(&index)));
        self.pareto_frontier = union.into_iter().collect();
        row.selected = nominated && self.chosen.len() <= self.request.max_designs;
    }

    fn improves(&self, candidate: &Trial, current: &Trial) -> bool {
        let Some(guidance) = &self.guidance else {
            return improves(candidate, current);
        };
        match (candidate.consumer_admitted(), current.consumer_admitted()) {
            (true, false) => true,
            (true, true) if self.request.matrix_pareto() => cases_prefer(
                &guidance.case_policies,
                &guidance.case_order,
                &candidate.consumer_cases,
                &current.consumer_cases,
            ),
            (true, true) if self.request.worst_case() => {
                // The matrix already checked each case against its own anchored protocol.
                // A change of limiting case must not compare unrelated case protocol hashes.
                let objective = &guidance.policy.objectives[0];
                let a = candidate
                    .consumer_reading(objective.metric)
                    .expect("admitted trial");
                let b = current
                    .consumer_reading(objective.metric)
                    .expect("admitted incumbent");
                let interval = |reading: &comparison::Reading| match guidance.policy.basis {
                    comparison::Basis::Nominal => [reading.value, reading.value],
                    comparison::Basis::ReportedBounds => {
                        reading.reported_bounds.expect("admitted envelope")
                    }
                };
                let [al, ah] = interval(a);
                let [bl, bh] = interval(b);
                a.unit == b.unit
                    && a.consumer == b.consumer
                    && match objective.direction {
                        Direction::Minimize => ah < bl,
                        Direction::Maximize => al > bh,
                    }
            }
            (true, true) => guidance.policy.prefers(
                candidate
                    .consumer_comparison
                    .as_ref()
                    .expect("admitted candidate"),
                current
                    .consumer_comparison
                    .as_ref()
                    .expect("admitted incumbent"),
            ),
            // Raw fit first approaches the flat-admissible region. Opted-in restoration then
            // follows only complete consumer guard vectors, never unavailable source evidence.
            (false, false)
                if self.request.constraint_restoration.is_some()
                    && candidate.flat_qualified()
                    && current.flat_qualified() =>
            {
                constraint_progress(candidate, current)
            }
            (false, false) => improves(candidate, current),
            (false, true) => false,
        }
    }

    fn inspect(&mut self, point: Vec<f64>) -> Result<Option<usize>, Refusal> {
        let identity = coordinate_identity(&point);
        if let Some(&index) = self.seen.get(&identity) {
            return Ok(Some(index));
        }
        if self.rows.len() >= self.request.max_evaluations {
            return Ok(None);
        }
        let index = self.rows.len();
        let (variant, values) = self.request.design(index, &point)?;
        let (mut row, result) = self.evaluator.trial(&variant, &values)?;
        let mut consumers = BTreeMap::new();
        if let Some(guidance) = &mut self.guidance {
            row.selected = false;
            if result.is_ok() {
                let mut constraint_violations =
                    self.request.constraint_restoration.map(|_| Vec::new());
                for material_index in 0..guidance.materials.len() {
                    let (decision, output) = guidance.probe(
                        material_index,
                        self.request,
                        Prepared {
                            variant: variant.clone(),
                            material_key: None,
                            variables_mm: Some(values.clone()),
                            evaluated: Some((row.design.clone(), result.clone())),
                        },
                    )?;
                    if let Some(violations) = &mut constraint_violations {
                        if let Some(next) = decision.constraint_violations() {
                            violations.extend(next);
                        } else {
                            constraint_violations = None;
                        }
                    }
                    if self.request.multiple_materials() {
                        row.selected |= decision.eligible;
                        // A different nominee may retain this geometry. Preserve even a refused
                        // output so final rows never rerun it with a differently consumed budget.
                        consumers.insert(decision.material_key.clone(), output);
                        row.consumer_materials.push(decision);
                    } else {
                        row.selected = decision.eligible;
                        row.refusal = decision.admission_refusal;
                        row.consumer_refusal = decision.refusal;
                        row.consumer_cases = decision.cases;
                        row.consumer_comparison = decision.comparison;
                        if row.selected {
                            consumers.insert(decision.material_key, output);
                        }
                    }
                }
                row.constraint_violations = constraint_violations;
                if self.request.multiple_materials() {
                    if !row.selected {
                        row.refusal = row
                            .consumer_materials
                            .iter()
                            .find_map(|material| material.admission_refusal.clone())
                            .or_else(|| {
                                Some(invalid("no nominated material admitted this design"))
                            });
                    }
                } else if row.selected && self.request.worst_case() {
                    let index =
                        worst_case_index(&row.consumer_cases, &guidance.policy.objectives[0])?;
                    row.consumer_case_key = Some(row.consumer_cases[index].key.clone());
                    if matches!(guidance.policy.basis, comparison::Basis::ReportedBounds) {
                        row.consumer_worst_case = Some(worst_case_range(
                            &row.consumer_cases,
                            &guidance.policy.objectives[0],
                        )?);
                    }
                }
            }
        }
        if self.request.pareto() && row.consumer_admitted() {
            if self.request.multiple_materials() {
                self.retain_material_pareto(index, &mut row);
            } else {
                self.retain_pareto(index, &mut row);
            }
        }
        if row.selected
            && self.guidance.is_some()
            && !self.request.pareto()
            && self.chosen.len() > self.request.max_designs
        {
            let replaced = if self.guidance.as_ref().is_some_and(|guidance| {
                matches!(guidance.policy.basis, comparison::Basis::ReportedBounds)
            }) {
                // Ranges are only partially ordered. Keep earlier incomparable incumbents and
                // replace the latest retained trial that this actual range strictly dominates.
                // A midpoint improvement or touching endpoints cannot evict a retained result.
                (1..self.chosen.len()).rev().find(|&position| {
                    self.improves(
                        &row,
                        &self.rows[self.chosen[position].index.expect("trial index")],
                    )
                })
            } else {
                let mut worst = 1;
                for position in 2..self.chosen.len() {
                    let previous = &self.rows[self.chosen[worst].index.expect("trial index")];
                    let next = &self.rows[self.chosen[position].index.expect("trial index")];
                    // Preserve the original single nominal objective's ordering and earlier ties.
                    if !self.improves(next, previous) {
                        worst = position;
                    }
                }
                self.improves(
                    &row,
                    &self.rows[self.chosen[worst].index.expect("trial index")],
                )
                .then_some(worst)
            };
            if let Some(position) = replaced {
                let previous = self.chosen[position].index.expect("trial index");
                self.rows[previous].selected = false;
                self.chosen.remove(position);
            } else {
                row.selected = false;
            }
        }
        if row.selected {
            self.chosen.push(Chosen {
                variant,
                values: Some(values),
                evaluated: (row.design.clone(), result),
                index: Some(index),
                consumers,
            });
        }
        self.seen.insert(identity, index);
        self.coordinates.push(point);
        self.rows.push(row);
        Ok(Some(index))
    }
}

impl MaterialDecision {
    fn constraint_violations(&self) -> Option<Vec<ConstraintViolation>> {
        let mut result = Vec::new();
        let mut append = |case_key: Option<&str>, candidate: &comparison::Candidate| {
            for violation in candidate.constraint_violations.as_ref()? {
                result.push(ConstraintViolation {
                    material_key: self.material_key.clone(),
                    case_key: case_key.map(str::to_owned),
                    violation: violation.clone(),
                });
            }
            Some(())
        };
        if self.cases.is_empty() {
            append(None, self.comparison.as_ref()?)?;
        } else {
            for case in &self.cases {
                append(Some(&case.key), &case.comparison)?;
            }
        }
        Some(result)
    }
}

fn constraint_progress(candidate: &Trial, current: &Trial) -> bool {
    let (Some(candidate), Some(current)) = (
        &candidate.constraint_violations,
        &current.constraint_violations,
    ) else {
        return false;
    };
    if candidate.is_empty() || candidate.len() != current.len() {
        return false;
    }
    let mut strictly = false;
    for (a, b) in candidate.iter().zip(current) {
        if a.material_key != b.material_key
            || a.case_key != b.case_key
            || a.violation.metric != b.violation.metric
            || a.violation.basis != b.violation.basis
            || a.violation.unit != b.violation.unit
        {
            return false;
        }
        for (a, b) in [
            (a.violation.minimum_shortfall, b.violation.minimum_shortfall),
            (a.violation.maximum_excess, b.violation.maximum_excess),
        ] {
            match (a, b) {
                (None, None) => {}
                (Some(a), Some(b)) if a <= b => strictly |= a < b,
                _ => return false,
            }
        }
    }
    strictly
}

impl Trial {
    fn consumer_reading(&self, metric: comparison::Metric) -> Option<&comparison::Reading> {
        self.consumer_worst_case
            .as_ref()
            .map(|range| &range.reading)
            .or_else(|| self.consumer_comparison.as_ref()?.readings.get(&metric))
    }

    fn flat_qualified(&self) -> bool {
        self.score() == Some((0.0, 0.0))
            && self
                .design
                .validation
                .as_ref()
                .is_some_and(|validation| validation.passed)
    }

    fn consumer_admitted(&self) -> bool {
        self.refusal.is_none()
            && (self
                .consumer_materials
                .iter()
                .any(|material| material.eligible)
                || self
                    .consumer_comparison
                    .as_ref()
                    .is_some_and(|candidate| candidate.eligible)
                || (!self.consumer_cases.is_empty()
                    && self
                        .consumer_cases
                        .iter()
                        .all(|case| case.comparison.eligible)))
    }

    fn score(&self) -> Option<(f64, f64)> {
        self.max_fit_band_violation_mm
            .zip(self.total_fit_band_violation_mm)
    }
}

fn improves(candidate: &Trial, current: &Trial) -> bool {
    match (candidate.score(), current.score()) {
        (Some(_), None) => true,
        (Some(a), Some(b)) => a.0 < b.0 || (a.0 == b.0 && a.1 < b.1),
        _ => false,
    }
}

fn cases_prefer(
    policies: &[comparison::Policy],
    order: &[String],
    candidate: &[CaseDecision],
    current: &[CaseDecision],
) -> bool {
    if policies.is_empty()
        || candidate.len() != policies.len()
        || current.len() != policies.len()
        || order.len() != policies.len()
        || candidate
            .iter()
            .zip(current)
            .zip(order)
            .any(|((a, b), key)| a.key != *key || b.key != *key)
    {
        return false;
    }
    comparison::Policy::prefers_cases(
        policies
            .iter()
            .zip(candidate)
            .zip(current)
            .map(|((policy, a), b)| (policy, &a.comparison, &b.comparison)),
    )
}

fn coordinate_identity(point: &[f64]) -> Vec<u64> {
    point
        .iter()
        .map(|value| if *value == 0.0 { 0 } else { value.to_bits() })
        .collect()
}

#[cfg(test)]
mod worst_case_range_tests {
    use super::*;

    fn case(key: &str, value: f64, bounds: [f64; 2]) -> CaseDecision {
        CaseDecision {
            key: key.into(),
            comparison: comparison::Candidate {
                key: "one-design".into(),
                readings: BTreeMap::from([(
                    comparison::Metric::AssemblyMassKg,
                    comparison::Reading {
                        value,
                        reported_bounds: Some(bounds),
                        unit: "kg".into(),
                        consumer: comparison::Consumer::AssemblyMass,
                    },
                )]),
                exclusions: Vec::new(),
                eligible: true,
                input_protocol_sha256: Some(key.into()),
                dominated_by: Vec::new(),
                constraint_violations: None,
            },
            refusal: None,
        }
    }

    #[test]
    fn worst_case_envelope_retains_independent_nominal_and_endpoint_witnesses() {
        for (direction, mut cases, expected) in [
            (
                Direction::Minimize,
                vec![
                    case("nominal", 8.0, [6.0, 9.0]),
                    case("lower", 7.0, [7.0, 8.0]),
                    case("upper", 5.0, [1.0, 12.0]),
                ],
                [7.0, 12.0],
            ),
            (
                Direction::Maximize,
                vec![
                    case("nominal", 6.0, [2.0, 10.0]),
                    case("lower", 8.0, [1.0, 12.0]),
                    case("upper", 7.0, [5.0, 8.0]),
                ],
                [1.0, 8.0],
            ),
        ] {
            let objective = comparison::Objective {
                metric: comparison::Metric::AssemblyMassKg,
                direction,
            };
            for reverse in [false, true] {
                if reverse {
                    cases.reverse();
                }
                let before = serde_json::to_value(&cases).unwrap();
                let nominal = worst_case_index(&cases, &objective).unwrap();
                let envelope = worst_case_range(&cases, &objective).unwrap();
                assert_eq!(
                    envelope.reading.value,
                    cases[nominal].comparison.readings[&objective.metric].value
                );
                assert_eq!(cases[nominal].key, "nominal");
                assert_eq!(envelope.reading.reported_bounds, Some(expected));
                assert_eq!(envelope.reported_bound_case_keys, ["lower", "upper"]);
                assert_eq!(serde_json::to_value(&cases).unwrap(), before);
                // All endpoint combinations lie inside the monotone envelope, without assuming
                // that these combinations are jointly attainable for a shared specimen.
                for mask in 0..8 {
                    let values = cases.iter().enumerate().map(|(i, case)| {
                        case.comparison.readings[&objective.metric]
                            .reported_bounds
                            .unwrap()[(mask >> i) & 1]
                    });
                    let worst = match direction {
                        Direction::Minimize => values.fold(f64::NEG_INFINITY, f64::max),
                        Direction::Maximize => values.fold(f64::INFINITY, f64::min),
                    };
                    assert!(expected[0] <= worst && worst <= expected[1]);
                }
            }
        }
    }

    #[test]
    fn worst_case_range_ties_and_single_case_keep_authored_witnesses() {
        for direction in [Direction::Minimize, Direction::Maximize] {
            let objective = comparison::Objective {
                metric: comparison::Metric::AssemblyMassKg,
                direction,
            };
            for bounds in [[2.0, 6.0], [4.0, 4.0]] {
                let mut cases = vec![case("first", 4.0, bounds), case("second", 4.0, bounds)];
                for _ in 0..2 {
                    let envelope = worst_case_range(&cases, &objective).unwrap();
                    assert_eq!(
                        envelope.reported_bound_case_keys,
                        [cases[0].key.clone(), cases[0].key.clone()]
                    );
                    assert_eq!(envelope.reading.reported_bounds, Some(bounds));
                    cases.reverse();
                }
                let envelope = worst_case_range(&cases[..1], &objective).unwrap();
                assert_eq!(envelope.reading.reported_bounds, Some(bounds));
            }
        }
    }

    #[test]
    fn worst_case_range_never_fills_or_repairs_invalid_case_readings() {
        let objective = comparison::Objective {
            metric: comparison::Metric::AssemblyMassKg,
            direction: Direction::Minimize,
        };
        assert!(worst_case_range(&[], &objective).is_err());
        for invalid in 0..8 {
            let mut cases = vec![case("first", 4.0, [2.0, 6.0]), case("bad", 5.0, [3.0, 7.0])];
            let candidate = &mut cases[1].comparison;
            let reading = candidate.readings.get_mut(&objective.metric).unwrap();
            match invalid {
                0 => reading.reported_bounds = None,
                1 => reading.reported_bounds = Some([3.0, f64::INFINITY]),
                2 => reading.reported_bounds = Some([6.0, 4.0]),
                3 => reading.reported_bounds = Some([6.0, 7.0]),
                4 => reading.unit = "g".into(),
                5 => reading.consumer = comparison::Consumer::Drape,
                6 => candidate.eligible = false,
                _ => candidate.readings.clear(),
            }
            assert!(worst_case_range(&cases, &objective).is_err());
        }
    }
}
