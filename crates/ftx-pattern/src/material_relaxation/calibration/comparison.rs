//! Bounded comparison of directional recovery models on one acquired specimen.
//!
//! Every numerical fit and model choice precedes CHECK replay. A held-out failure rejects the chosen
//! model; it never promotes a different spectrum. Equivalent FIT residuals expose competing models,
//! without treating a force weighting scale as noise variance or inventing a confidence interval.

use std::collections::BTreeSet;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use super::{Input, OrthotropicInput, Partition, Quality, Report, bad, orthotropic};

/// Portable input for the existing calibration command and browser entry point.
pub const SCHEMA: &str = "ftx-relaxation-model-comparison/1";

fn evaluations() -> usize {
    1024
}

fn projected_iterations() -> usize {
    262_144
}

/// One declared model order and candidate-time basis or bounded time search.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    /// Stable unique name, used to resolve an ambiguity explicitly on a subsequent request.
    pub id: String,
    /// The same physical tensor constraints, threshold and time policy as single-model calibration.
    pub orthotropic: OrthotropicInput,
}

/// One common evidence set and a small, explicit collection of competing directional models.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonInput {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Complete original acquisition. Leave both time policies empty: models supply those choices.
    pub calibration: Input,
    /// Two to four alternatives, evaluated in ID order with one shared work budget.
    pub candidates: Vec<Model>,
    /// Largest RMS excess above the best FIT RMS still considered equivalent, in the declared
    /// scaled-force units. This explicit numerical comparison tolerance is not a significance test.
    pub fit_rms_equivalence_scaled: f64,
    /// Optional explicit choice among FIT-admissible models. CHECK must still accept that choice;
    /// failure never selects another candidate. Omission publishes only a unique FIT alternative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select_candidate_id: Option<String>,
    /// Total fixed-time and time-search objective evaluations, including failed trials; 1..=1024.
    #[serde(default = "evaluations")]
    pub max_objective_evaluations: usize,
    /// Total projected tensor iterations across every model/start/trial; 1..=262144.
    #[serde(default = "projected_iterations")]
    pub max_projected_iterations: usize,
}

/// Whether a model was numerically identified and met the common FIT force-error budgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FitStatus {
    /// Identified and within both FIT residual limits.
    Accepted,
    /// Identified, with retained residuals exceeding a FIT limit.
    ResidualsExceeded,
    /// Source, excitation or numerical failure; this is not evidence against its physical model.
    Refused,
    /// Not attempted after the shared budget stopped the comparison.
    NotRun,
}

/// Independent checks are not part of model ranking, equivalence or candidate choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckStatus {
    /// No replay because the model was unavailable or the total work budget stopped the request.
    NotRun,
    /// The request supplied no independent acquisition; this is not a passed physical check.
    NotProvided,
    /// Every supplied CHECK residual met its declared numerical budgets.
    Accepted,
    /// A CHECK force error exceeded its declared budget or replay refused.
    Rejected,
}

/// A complete candidate cell, including failures and the actual consuming material when fitted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelResult {
    /// Corresponding declared model ID.
    pub id: String,
    /// Four tensor coefficients per fixed time; five parameters per optimized time. This is the
    /// declared parameter count, not effective degrees of freedom of a constrained estimator.
    pub declared_parameters: usize,
    /// FIT-only numerical/force acceptance.
    pub fit_status: FitStatus,
    /// Full calibration, retaining all acquired observations. Predictions contain only FIT rows if
    /// CHECK replay has not completed; `check_status` distinguishes that from absent evidence.
    pub calibration: Option<Report>,
    /// Source, excitation, convergence or budget failure for this candidate.
    pub refusal: Option<Refusal>,
    /// Independent check result, computed after `fit_decision` is fixed.
    pub check_status: CheckStatus,
    /// Replay arithmetic/source refusal, distinct from finite excessive residuals in the report.
    pub check_refusal: Option<Refusal>,
    /// Objective evaluations used by this candidate, including unsuccessful work.
    pub objective_evaluations: usize,
    /// Projected tensor iterations used by this candidate, including unsuccessful work.
    pub projected_iterations: usize,
}

/// Difference in model predictions on observed FIT force channels only.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PredictionDifference {
    /// First candidate ID.
    pub first: String,
    /// Second candidate ID.
    pub second: String,
    /// RMS/peak of (first prediction - second prediction) / acquisition scale. Unobserved axes,
    /// CHECK histories and behavior outside the acquired strain/time paths are not compared.
    pub fit_prediction_difference: Quality,
}

/// All model-choice fields are fixed before any CHECK forces are replayed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FitDecision {
    /// Every requested numerical fit produced a report; a refusal prevents automatic selection.
    pub comparison_complete: bool,
    /// Smallest RMS among models meeting both FIT limits, or absent when none meet them.
    pub best_rms_scaled: Option<f64>,
    /// All FIT-admissible models within the explicit RMS equivalence tolerance, in ID order.
    pub equivalent_candidate_ids: Vec<String>,
    /// More than one declared spectrum/model order remains FIT-equivalent. This is an ambiguity
    /// within the supplied alternatives and tolerance, not a global identifiability proof.
    pub ambiguous: bool,
    /// Fewest declared parameters among the equivalent models, then FIT RMS and ID. This explicit
    /// parsimony preference never silently resolves an ambiguity into a material selection.
    pub parsimonious_candidate_id: Option<String>,
    /// Explicitly requested candidate or the only FIT-equivalent model in a complete comparison.
    /// A later held-out rejection leaves this choice unchanged and prevents its publication.
    pub chosen_candidate_id: Option<String>,
    /// `explicit_candidate`, `unique_fit_alternative`, or `unresolved`.
    pub basis: String,
}

/// Publication outcome; numerical completion alone is not empirical validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    /// A unique or explicitly chosen model meets all supplied force-error gates.
    Accepted,
    /// Several alternatives remain equivalent; a reviewed explicit choice is required to bind one.
    Ambiguous,
    /// No identified model satisfies the FIT budgets.
    NoAdmissibleModel,
    /// A numerical refusal leaves an alternative unresolved, preventing automatic selection.
    Incomplete,
    /// The unchanged chosen candidate failed FIT or independent CHECK acceptance.
    ChosenCandidateRejected,
    /// The global allowance stopped work. No partial fitted selection is published.
    BudgetStop,
}

/// Reviewable comparison with original observations, alternative spectra and independent checks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComparisonReport {
    /// `ftx-relaxation-model-comparison-report/1`.
    pub schema: String,
    /// Original acquisition, model set, comparison policy and any explicit choice.
    pub input: ComparisonInput,
    /// Exact serialized input identity; no claim about the truth of acquisition records.
    pub input_sha256: String,
    /// Outcome controlling whether an ordinary frozen material can be published.
    pub status: Status,
    /// Every requested alternative, in canonical ID order.
    pub candidates: Vec<ModelResult>,
    /// FIT-only model choice and ambiguity, unchanged by CHECK outcomes.
    pub fit_decision: FitDecision,
    /// Pairwise FIT prediction differences for every pair of numerically fitted alternatives.
    pub prediction_differences: Vec<PredictionDifference>,
    /// Total objective evaluations across all alternatives, starts and unsuccessful trials.
    pub objective_evaluations: usize,
    /// Total projected tensor iterations across all alternatives, starts and unsuccessful trials.
    pub projected_iterations: usize,
    /// Physical and statistical boundaries of this numerical comparison.
    pub no_claim: String,
}

impl ComparisonReport {
    /// The only calibration eligible for normal material binding. Ambiguity, failed checks,
    /// incomplete automatic comparison and budget stops never fall back to another candidate.
    #[must_use]
    pub fn selected_calibration(&self) -> Option<&Report> {
        if self.status != Status::Accepted {
            return None;
        }
        let id = self.fit_decision.chosen_candidate_id.as_ref()?;
        self.candidates
            .iter()
            .find(|candidate| &candidate.id == id)?
            .calibration
            .as_ref()
    }
}

fn passes(quality: &Quality, input: &Input) -> bool {
    quality.rms_scaled <= input.max_rms_scaled && quality.peak_scaled <= input.max_peak_scaled
}

fn decision(input: &ComparisonInput, candidates: &[ModelResult], stopped: bool) -> FitDecision {
    let admissible: Vec<_> = candidates
        .iter()
        .filter(|c| c.fit_status == FitStatus::Accepted)
        .collect();
    let rms = |candidate: &&ModelResult| {
        candidate
            .calibration
            .as_ref()
            .expect("fitted candidate")
            .fit
            .rms_scaled
    };
    let best = admissible.iter().map(rms).min_by(f64::total_cmp);
    let equivalent: Vec<_> = admissible
        .into_iter()
        .filter(|candidate| {
            rms(candidate) - best.expect("admissible") <= input.fit_rms_equivalence_scaled
        })
        .collect();
    let complete = !stopped && candidates.iter().all(|c| c.calibration.is_some());
    let parsimonious = equivalent.iter().min_by(|a, b| {
        a.declared_parameters
            .cmp(&b.declared_parameters)
            .then_with(|| rms(a).total_cmp(&rms(b)))
            .then_with(|| a.id.cmp(&b.id))
    });
    let (chosen, basis) = if stopped {
        (None, "unresolved")
    } else if let Some(id) = &input.select_candidate_id {
        (Some(id.clone()), "explicit_candidate")
    } else if complete && equivalent.len() == 1 {
        (Some(equivalent[0].id.clone()), "unique_fit_alternative")
    } else {
        (None, "unresolved")
    };
    FitDecision {
        comparison_complete: complete,
        best_rms_scaled: best,
        equivalent_candidate_ids: equivalent.iter().map(|c| c.id.clone()).collect(),
        ambiguous: equivalent.len() > 1,
        parsimonious_candidate_id: parsimonious.map(|c| c.id.clone()),
        chosen_candidate_id: chosen,
        basis: basis.into(),
    }
}

fn difference(first: &Report, second: &Report) -> Result<Quality, Refusal> {
    let mut a = first
        .predictions
        .iter()
        .filter(|p| p.partition == Partition::Fit);
    let mut b = second
        .predictions
        .iter()
        .filter(|p| p.partition == Partition::Fit);
    let mut residuals = Vec::new();
    for experiment in first
        .input
        .experiments
        .iter()
        .filter(|e| e.partition == Partition::Fit)
    {
        for sample in &experiment.samples {
            let (pa, pb) = (
                a.next().expect("FIT prediction"),
                b.next().expect("FIT prediction"),
            );
            for i in 0..2 {
                for j in 0..2 {
                    if sample.nominal_n_m[i][j].is_some() {
                        residuals
                            .push((pa.nominal_n_m[i][j] - pb.nominal_n_m[i][j]) / sample.scale_n_m);
                    }
                }
            }
        }
    }
    super::quality(&residuals, &first.input, "FIT prediction difference", false)
}

/// Compare fixed/bounded directional spectra using one common acquired specimen and work allowance.
/// All choices use FIT forces only. CHECK changes can reject the retained choice but cannot select
/// a replacement. A bare comparison does not certify unique parameters or measured textile behavior.
///
/// # Errors
/// Malformed schema/model identities, conflicting base time choices, invalid comparison policy or
/// unrepresentable prediction comparisons. Individual solver refusals remain visible report cells.
pub fn compare(input: &ComparisonInput) -> Result<ComparisonReport, Refusal> {
    if input.schema != SCHEMA
        || !(2..=4).contains(&input.candidates.len())
        || input.calibration.orthotropic.is_some()
        || !input.calibration.time_bounds_seconds.is_empty()
        || !input.fit_rms_equivalence_scaled.is_finite()
        || input.fit_rms_equivalence_scaled < 0.
    {
        return Err(bad(
            "comparison needs two to four directional models, no base time policy and a finite nonnegative FIT RMS equivalence tolerance",
        ));
    }
    let mut ids = BTreeSet::new();
    for candidate in &input.candidates {
        if candidate.id.trim().is_empty() || candidate.id.len() > 128 || !ids.insert(&candidate.id)
        {
            return Err(bad(
                "comparison model IDs must be nonempty, unique and at most 128 bytes",
            ));
        }
    }
    if input
        .select_candidate_id
        .as_ref()
        .is_some_and(|id| !ids.contains(id))
    {
        return Err(bad(
            "explicit comparison choice does not name a requested candidate",
        ));
    }
    let mut budget = orthotropic::ComparisonBudget::new(
        input.max_objective_evaluations,
        input.max_projected_iterations,
    )?;
    let mut models: Vec<_> = input.candidates.iter().collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    let mut candidates = Vec::new();
    let mut stopped = false;
    for model in models {
        let options = &model.orthotropic;
        let mut result = ModelResult {
            id: model.id.clone(),
            declared_parameters: if options.time_bounds_seconds.is_empty() {
                4 * options.times_seconds.len()
            } else {
                5 * options.time_bounds_seconds.len()
            },
            fit_status: FitStatus::NotRun,
            calibration: None,
            refusal: None,
            check_status: CheckStatus::NotRun,
            check_refusal: None,
            objective_evaluations: 0,
            projected_iterations: 0,
        };
        if !stopped {
            let before = budget.used();
            let mut candidate_input = input.calibration.clone();
            candidate_input.orthotropic = Some(options.clone());
            match orthotropic::calibrate_candidate(&candidate_input, options, &mut budget) {
                Ok(report) => {
                    result.fit_status = if passes(&report.fit, &input.calibration) {
                        FitStatus::Accepted
                    } else {
                        FitStatus::ResidualsExceeded
                    };
                    result.calibration = Some(report);
                }
                Err(error) => {
                    stopped = error.code == RefusalCode::BudgetExceeded;
                    result.fit_status = FitStatus::Refused;
                    result.refusal = Some(error);
                }
            }
            result.objective_evaluations = budget.used()[0] - before[0];
            result.projected_iterations = budget.used()[1] - before[1];
        }
        candidates.push(result);
    }
    // Freeze the complete FIT decision before even calculating one held-out force prediction.
    let fit_decision = decision(input, &candidates, stopped);
    let mut prediction_differences = Vec::new();
    for (i, first) in candidates.iter().enumerate() {
        for second in &candidates[i + 1..] {
            if let (Some(a), Some(b)) = (&first.calibration, &second.calibration) {
                prediction_differences.push(PredictionDifference {
                    first: first.id.clone(),
                    second: second.id.clone(),
                    fit_prediction_difference: difference(a, b)?,
                });
            }
        }
    }
    if !stopped {
        for candidate in &mut candidates {
            if let Some(report) = &mut candidate.calibration {
                match orthotropic::replay_checks(report) {
                    Ok(()) => {
                        candidate.check_status = match &report.check {
                            None => CheckStatus::NotProvided,
                            Some(q) if passes(q, &input.calibration) => CheckStatus::Accepted,
                            Some(_) => CheckStatus::Rejected,
                        };
                    }
                    Err(error) => {
                        candidate.check_status = CheckStatus::Rejected;
                        candidate.check_refusal = Some(error);
                    }
                }
            }
        }
    }
    let status = if stopped {
        Status::BudgetStop
    } else if let Some(id) = &fit_decision.chosen_candidate_id {
        let chosen = candidates
            .iter()
            .find(|c| &c.id == id)
            .expect("validated candidate ID");
        if chosen.fit_status == FitStatus::Accepted
            && matches!(
                chosen.check_status,
                CheckStatus::Accepted | CheckStatus::NotProvided
            )
        {
            Status::Accepted
        } else {
            Status::ChosenCandidateRejected
        }
    } else if !fit_decision.comparison_complete {
        Status::Incomplete
    } else if fit_decision.ambiguous {
        Status::Ambiguous
    } else {
        Status::NoAdmissibleModel
    };
    Ok(ComparisonReport {
        schema: "ftx-relaxation-model-comparison-report/1".into(),
        input: input.clone(),
        input_sha256: sha256_hex(&serde_json::to_vec(input).map_err(|e| bad(e.to_string()))?),
        status,
        candidates,
        fit_decision,
        prediction_differences,
        objective_evaluations: budget.used()[0],
        projected_iterations: budget.used()[1],
        no_claim: "FIT equivalence and declared model size describe only the requested alternatives and acquisition. Force scales are not noise variances; no information criterion, parameter confidence, global uniqueness, fixed-equilibrium accuracy or empirical garment validation is established. Missing CHECK data are not passed checks.".into(),
    })
}
