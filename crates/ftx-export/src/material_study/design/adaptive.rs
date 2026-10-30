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
    /// Exact material variant used to guide consumer polling. Required only for consumer guidance;
    /// selected geometries still cross every authored material alternative after discovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_key: Option<String>,
    /// Named scenario whose comparison objectives guide matrix polling. Every case must still
    /// admit the trial. Forbidden for worst-case and all-case Pareto guidance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_key: Option<String>,
    /// One to three variables, polled in authored order and negative then positive direction.
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
    /// Canonical geometry/fit refusal or consumer-admission/protocol exclusion, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
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

/// One nominated material and the existing comparison authority used for local polling.
#[derive(Clone, Debug, Serialize)]
pub struct ConsumerGuidance {
    /// Exact authored material key; other material alternatives do not silently steer discovery.
    pub material_key: String,
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
    /// Actual nominated-material probes, after full canonical geometry and fit admission.
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

    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if self.consumer_guided() != self.material_key.is_some()
            || self
                .material_key
                .as_ref()
                .is_some_and(|key| key.trim().is_empty())
        {
            return Err(invalid(
                "consumer guidance needs one nonempty material_key; fit_band_violation_mm does not accept material_key",
            ));
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
                .find(|material| Some(&material.variant.key) == self.material_key.as_ref())
                .map(|material| serde_json::to_vec(&material.variant))
                .transpose()
                .map_err(|e| invalid(e.to_string()))?
                .map_or(0, |bytes| bytes.len().saturating_mul(self.max_evaluations))
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
                        self.max_evaluations.saturating_mul(3)
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
            let material_key = self.material_key.as_ref().expect("validated material key");
            let material = materials
                .iter()
                .find(|material| material.variant.key == *material_key)
                .ok_or_else(|| {
                    invalid("adaptive material_key must identify an admitted material alternative")
                })?;
            let policy = study
                .comparison
                .as_ref()
                .expect("validated consumer comparison");
            Some(Guidance {
                material,
                policy,
                evaluate: evaluate.ok_or_else(|| {
                    invalid("consumer-guided adaptive discovery needs its production evaluator")
                })?,
                protocol: None,
                evaluations: 0,
                case_order: cases.iter().map(|(key, _)| key.clone()).collect(),
                case_policies: if self.matrix_pareto() {
                    cases.iter().map(|(_, policy)| policy.clone()).collect()
                } else {
                    Vec::new()
                },
                reserved_evaluations: self
                    .max_evaluations
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
                consumer: None,
            }],
            seen: BTreeMap::new(),
            coordinates: Vec::new(),
            pareto_frontier: Vec::new(),
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
                    for (axis, variable) in self.variables.iter().enumerate() {
                        for positive in [false, true] {
                            let bound = variable.bounds_mm[usize::from(positive)];
                            let distance = (bound - center[axis]).abs();
                            // Boundary points are actual declared coordinates, not repaired user inputs.
                            let next = if steps[axis] >= distance {
                                bound
                            } else if positive {
                                center[axis] + steps[axis]
                            } else {
                                center[axis] - steps[axis]
                            };
                            if next == center[axis] {
                                continue;
                            }
                            let mut point = center.clone();
                            point[axis] = next;
                            let Some(index) = discovery.inspect(point)? else {
                                break 'search StopReason::EvaluationBudget;
                            };
                            if discovery.improves(&discovery.rows[index], &discovery.rows[best]) {
                                best = index;
                            }
                            if !self.consumer_guided() && discovery.chosen.len() > self.max_designs
                            {
                                break 'search StopReason::QualifiedDesignLimit;
                            }
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
                    material_key: guidance.material.variant.key.clone(),
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
                    pareto_frontier: self.pareto().then(|| {
                        let mut frontier = ParetoFrontier {
                            evaluated_trial_keys: Vec::new(),
                            retained_trial_keys: Vec::new(),
                            unretained_trial_keys: Vec::new(),
                            retention_capacity: self.max_designs,
                        };
                        for &index in &discovery.pareto_frontier {
                            let row = &discovery.rows[index];
                            frontier.evaluated_trial_keys.push(row.design.key.clone());
                            if row.selected {
                                frontier.retained_trial_keys.push(row.design.key.clone());
                            } else {
                                frontier.unretained_trial_keys.push(row.design.key.clone());
                            }
                        }
                        frontier
                    }),
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
                    input_protocol_sha256: guidance.protocol.clone().flatten(),
                })
            }),
            rows: discovery.rows,
        };
        let mut expanded = Vec::new();
        let mut retained = BTreeMap::new();
        for chosen in discovery.chosen {
            if let Some(consumer) = chosen.consumer {
                let key = serde_json::to_string(&(
                    &chosen.variant.key,
                    self.material_key.as_ref().expect("consumer material"),
                ))
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
    material: &'a PreparedVariant,
    policy: &'a comparison::Policy,
    evaluate: &'a mut ConsumerEvaluator<'a, T>,
    protocol: Option<Option<String>>,
    evaluations: usize,
    case_order: Vec<String>,
    case_policies: Vec<comparison::Policy>,
    reserved_evaluations: usize,
}

struct Chosen<T> {
    variant: Variant,
    values: Option<BTreeMap<String, f64>>,
    evaluated: (super::Receipt, Result<Pattern, Refusal>),
    index: Option<usize>,
    consumer: Option<T>,
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
                consumer_case_key: None,
                consumer_worst_case: None,
                raw_fit,
                max_fit_band_violation_mm: score.map(|score| score.0),
                total_fit_band_violation_mm: score.map(|score| score.1),
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
}

impl<T> Discovery<'_, '_, '_, T> {
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
            for (axis, variable) in request.variables.iter().enumerate() {
                for positive in [false, true] {
                    let bound = variable.bounds_mm[usize::from(positive)];
                    let distance = (bound - center[axis]).abs();
                    let next = if steps[axis] >= distance {
                        bound
                    } else if positive {
                        center[axis] + steps[axis]
                    } else {
                        center[axis] - steps[axis]
                    };
                    if next == center[axis] {
                        continue;
                    }
                    let mut point = center.clone();
                    point[axis] = next;
                    let Some(index) = self.inspect(point)? else {
                        return Ok(StopReason::EvaluationBudget);
                    };
                    if improves(&self.rows[index], &self.rows[best_fit]) {
                        best_fit = index;
                    }
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

    fn improves(&self, candidate: &Trial, current: &Trial) -> bool {
        let Some(guidance) = &self.guidance else {
            return improves(candidate, current);
        };
        match (candidate.consumer_admitted(), current.consumer_admitted()) {
            (true, false) => true,
            (true, true) if self.request.matrix_pareto() => {
                let cases = &guidance.case_policies;
                if candidate.consumer_cases.len() != cases.len()
                    || current.consumer_cases.len() != cases.len()
                    || cases.is_empty()
                    || candidate
                        .consumer_cases
                        .iter()
                        .zip(&current.consumer_cases)
                        .zip(&guidance.case_order)
                        .any(|((a, b), key)| a.key != *key || b.key != *key)
                {
                    return false;
                }
                comparison::Policy::prefers_cases(
                    cases
                        .iter()
                        .zip(&candidate.consumer_cases)
                        .zip(&current.consumer_cases)
                        .map(|((policy, a), b)| (policy, &a.comparison, &b.comparison)),
                )
            }
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
            // Raw fit only approaches the declared admissible region; it never replaces an
            // admitted production incumbent or substitutes a failed consumer reading.
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
        let mut consumer = None;
        if let Some(guidance) = &mut self.guidance {
            row.selected = false;
            if result.is_ok() {
                let mut material = guidance.material.variant.clone();
                material.key = serde_json::to_string(&(&variant.key, &material.key))
                    .map_err(|e| invalid(e.to_string()))?;
                guidance.evaluations += 1;
                let evaluation = (guidance.evaluate)(PreparedVariant {
                    variant: material,
                    choices: guidance.material.choices.clone(),
                    design: Some(Prepared {
                        variant: variant.clone(),
                        material_key: Some(guidance.material.variant.key.clone()),
                        variables_mm: Some(values.clone()),
                        evaluated: Some((row.design.clone(), result.clone())),
                    }),
                })?;
                let candidate = evaluation.comparison;
                row.consumer_refusal = evaluation.refusal;
                row.consumer_cases = evaluation.cases;
                if let Some(refusal) = evaluation.admission_refusal {
                    row.refusal = Some(refusal);
                } else if candidate.eligible {
                    if self.request.worst_case() {
                        let index =
                            worst_case_index(&row.consumer_cases, &guidance.policy.objectives[0])?;
                        row.consumer_case_key = Some(row.consumer_cases[index].key.clone());
                        if matches!(guidance.policy.basis, comparison::Basis::ReportedBounds) {
                            row.consumer_worst_case = Some(worst_case_range(
                                &row.consumer_cases,
                                &guidance.policy.objectives[0],
                            )?);
                        }
                        row.selected = true;
                        consumer = Some(evaluation.output);
                    } else if self.request.matrix_pareto() {
                        // Every case was independently admitted and protocol-anchored by the
                        // matrix. No first-case or synthetic aggregate protocol replaces that.
                        row.selected = true;
                        consumer = Some(evaluation.output);
                    } else if guidance.protocol.as_ref().is_some_and(|protocol| {
                        !guidance.policy.allow_mixed_protocols
                            && *protocol != candidate.input_protocol_sha256
                    }) {
                        row.refusal = Some(invalid(
                            "adaptive consumer protocols differ from the first admitted incumbent; no ordering inferred",
                        ));
                    } else {
                        guidance
                            .protocol
                            .get_or_insert_with(|| candidate.input_protocol_sha256.clone());
                        row.selected = true;
                        consumer = Some(evaluation.output);
                    }
                } else {
                    row.refusal = candidate.exclusions.first().cloned();
                }
                // All-case mode has no nominated decision. Original decisions remain in cases.
                row.consumer_comparison = (!self.request.matrix_pareto()).then_some(candidate);
            }
        }
        if self.request.pareto() && row.consumer_admitted() {
            self.retain_pareto(index, &mut row);
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
                consumer,
            });
        }
        self.seen.insert(identity, index);
        self.coordinates.push(point);
        self.rows.push(row);
        Ok(Some(index))
    }
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
