//! Bounded coordinate polling over explicit linked alterations and canonical raw fit readings.
//! Optional consumer guidance reuses the study comparator; no mode certifies the unvisited domain.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, curve::OUTLINE_DENSE};
use ftx_pattern::Pattern;
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
    /// Use one admitted nominal production comparison objective after complete flat qualification.
    ConsumerComparison,
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
    /// Raw fit-band discovery or one existing production comparison; never an inferred fit score.
    pub objective: Objective,
    /// Exact material variant used to guide consumer polling. Required only for consumer guidance;
    /// selected geometries still cross every authored material alternative after discovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_key: Option<String>,
    /// Named scenario whose sole nominal objective guides matrix polling. Every case must still
    /// admit the trial. Required for consumer-guided matrices and forbidden in single-case work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_key: Option<String>,
    /// One to three variables, polled in authored order and negative then positive direction.
    pub variables: Vec<Variable>,
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
    /// No strictly better poll remained at the declared final steps; other points remain unknown.
    MinimumSteps,
}

/// One actual canonical trial, including geometry, body identity and refusal evidence.
#[derive(Clone, Debug, Serialize)]
pub struct Trial {
    /// Exact original/altered identity, operations, scalar coordinates and canonical validation.
    pub design: super::Receipt,
    /// Retained for the final material cross-product; removed consumer incumbents remain visible.
    pub selected: bool,
    /// Unmodified production comparator decision for the nominated material, after flat admission.
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
    /// Best admitted discovery trial, which can still be out of band; never an automatic winner.
    pub best_trial_key: Option<String>,
    /// Best discovery maximum residual, not a manufacturing comparison metric.
    pub best_max_fit_band_violation_mm: Option<f64>,
    /// Best discovery secondary residual.
    pub best_total_fit_band_violation_mm: Option<f64>,
    /// Polling steps at termination, indexed by the original variable identities.
    pub final_steps_mm: BTreeMap<String, f64>,
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
    /// The sole objective from the ordinary nominal comparison policy.
    pub objective: comparison::Objective,
    /// Conservative upper bound on probe plus final consumer slots. Scenario work is still
    /// admitted per callback by the caller's shared production-session budgets.
    pub reserved_evaluations: usize,
    /// Actual nominated-material probes, after full canonical geometry and fit admission.
    pub evaluations: usize,
    /// Best admitted retained discovery trial, excluding the separately retained baseline.
    pub best_trial_key: Option<String>,
    /// Existing comparator reading, never a separately computed or normalized score.
    pub best_reading: Option<comparison::Reading>,
    /// First admitted input protocol; differing protocols cannot steer unless explicitly allowed.
    pub input_protocol_sha256: Option<String>,
}

impl Request {
    pub(in crate::material_study) fn consumer_guided(&self) -> bool {
        matches!(self.objective, Objective::ConsumerComparison)
    }

    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if self.consumer_guided() != self.material_key.is_some()
            || self
                .material_key
                .as_ref()
                .is_some_and(|key| key.trim().is_empty())
        {
            return Err(invalid(
                "consumer_comparison needs one nonempty material_key; fit_band_violation_mm does not accept material_key",
            ));
        }
        if self
            .case_key
            .as_ref()
            .is_some_and(|key| !self.consumer_guided() || key.trim().is_empty() || key.len() > 128)
        {
            return Err(invalid(
                "adaptive case_key needs consumer_comparison and a nonempty case name of at most 128 bytes",
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
        case_order: &[String],
    ) -> Result<Expansion<T>, Refusal> {
        self.validate()?;
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
                case_order: case_order.to_vec(),
                reserved_evaluations: self
                    .max_evaluations
                    .saturating_add((self.max_designs + 1).saturating_mul(materials.len()))
                    .saturating_mul(case_order.len().max(1)),
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
        };
        let mut current = discovery
            .inspect(
                self.variables
                    .iter()
                    .map(|variable| variable.initial_mm)
                    .collect(),
            )?
            .expect("positive trial allowance");
        let mut steps: Vec<_> = self
            .variables
            .iter()
            .map(|variable| variable.initial_step_mm)
            .collect();
        let stop_reason = 'search: loop {
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
                    if !self.consumer_guided() && discovery.chosen.len() > self.max_designs {
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
        };
        let mut best: Option<usize> = None;
        for (index, row) in discovery.rows.iter().enumerate() {
            let admitted = if self.consumer_guided() {
                row.consumer_admitted()
            } else {
                row.score().is_some()
            };
            if admitted
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
            consumer_guidance: discovery.guidance.as_ref().map(|guidance| {
                Box::new(ConsumerGuidance {
                    material_key: guidance.material.variant.key.clone(),
                    case_key: self.case_key.clone(),
                    case_order: guidance.case_order.clone(),
                    objective: guidance.policy.objectives[0].clone(),
                    reserved_evaluations: guidance.reserved_evaluations,
                    evaluations: guidance.evaluations,
                    best_trial_key: best.map(|index| discovery.rows[index].design.key.clone()),
                    best_reading: best
                        .and_then(|index| discovery.rows[index].consumer_comparison.as_ref())
                        .and_then(|candidate| {
                            candidate
                                .readings
                                .get(&guidance.policy.objectives[0].metric)
                        })
                        .cloned(),
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
}

impl<T> Discovery<'_, '_, '_, T> {
    fn improves(&self, candidate: &Trial, current: &Trial) -> bool {
        let Some(guidance) = &self.guidance else {
            return improves(candidate, current);
        };
        match (candidate.consumer_admitted(), current.consumer_admitted()) {
            (true, false) => true,
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
        let identity: Vec<_> = point
            .iter()
            .map(|value| if *value == 0.0 { 0 } else { value.to_bits() })
            .collect();
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
                    if guidance.protocol.as_ref().is_some_and(|protocol| {
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
                row.consumer_comparison = Some(candidate);
            }
        }
        if row.selected && self.guidance.is_some() && self.chosen.len() > self.request.max_designs {
            let mut worst = 1;
            for position in 2..self.chosen.len() {
                let previous = &self.rows[self.chosen[worst].index.expect("trial index")];
                let next = &self.rows[self.chosen[position].index.expect("trial index")];
                // Evict the latest of equally bad retained trials, preserving earlier ties.
                if !self.improves(next, previous) {
                    worst = position;
                }
            }
            let previous = self.chosen[worst].index.expect("trial index");
            if self.improves(&row, &self.rows[previous]) {
                self.rows[previous].selected = false;
                self.chosen.remove(worst);
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
            && self
                .consumer_comparison
                .as_ref()
                .is_some_and(|candidate| candidate.eligible)
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
