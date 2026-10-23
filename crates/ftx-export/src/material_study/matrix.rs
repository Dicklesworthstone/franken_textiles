//! Named production scenarios over one frozen material study and one manufacturing evaluation.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_pattern::Pattern;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Outcome, Request, ScenarioAlternative, budget, comparison, hash, invalid};

/// One exact production scenario and its optional override of the study's comparison policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Unique nonempty name, retained in caller order rather than sorted by a computed outcome.
    pub key: String,
    /// Exact descriptor supplied by the production consumer, including its body and scenario locks.
    pub scenario: Value,
    /// Omission uses `Request::comparison`; if both are absent this case has no comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<comparison::Policy>,
}

/// A case's ordinary scenario outputs, without another copy of shared manufacturing results.
#[derive(Debug, Serialize)]
pub struct CaseReport<T> {
    /// Authored case name.
    pub key: String,
    /// The same identity as an independent `evaluate_with` using this case's effective policy.
    pub request_sha256: String,
    /// Unmodified body, pose, environment and numerical descriptor.
    pub scenario: Value,
    /// Every manufacturing alternative, in the same key order, including binding failures.
    pub simulations: Vec<ScenarioAlternative<T>>,
    /// Existing production comparison under this case's explicitly resolved policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<comparison::Report>,
}

/// A material alternative's complete set of per-case decisions.
#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    /// Shared material alternative key.
    pub key: String,
    /// True only if every case's declared comparison admits this alternative.
    pub eligible: bool,
    /// Exact case-keyed readings, exclusions and protocol identities from the existing comparator.
    pub cases: BTreeMap<String, comparison::Candidate>,
    /// Alternatives no worse in every case/objective and strictly better in at least one.
    pub dominated_by: Vec<String>,
}

/// A pair whose case-specific protocols or objective readings do not establish an ordering.
#[derive(Clone, Debug, Serialize)]
pub struct Incomparable {
    /// Stable material alternative keys.
    pub alternatives: [String; 2],
    /// Cases whose protocols differ, or all cases defining an unresolved joint tradeoff.
    pub cases: Vec<String>,
    /// The limitation, without assigning a probability or substituting a weighted score.
    pub reason: &'static str,
}

/// Conditional Pareto decision across all explicitly declared case/objective coordinates.
#[derive(Clone, Debug, Serialize)]
pub struct Comparison {
    /// `ftx-material-scenario-matrix-comparison/1`.
    pub schema: &'static str,
    /// Shared original pattern identity.
    pub source_pattern_sha256: String,
    /// Exact matrix request, including case order, descriptors, policies and evaluation allowance.
    pub context_sha256: String,
    /// Authored case order for presentation; map order never changes the meaning of a coordinate.
    pub case_order: Vec<String>,
    /// Effective explicit policy for every case; none is synthesized for the aggregate.
    pub policies: BTreeMap<String, comparison::Policy>,
    /// Key-ordered material alternatives and their unchanged per-case decisions.
    pub alternatives: Vec<Candidate>,
    /// All-cases eligible alternatives without a dominating comparable alternative.
    pub frontier: Vec<String>,
    /// Protocol-incomparable and unresolved case tradeoffs remain visible.
    pub incomparable: Vec<Incomparable>,
    /// Always false: conditional source ranges are not a general uncertainty model.
    pub uncertainty_propagated: bool,
    /// Always false: multiple numerical cases do not establish measured garment agreement.
    pub physical_validation_claim: bool,
    /// Scope of the finite case set and the shared dominance rule.
    pub notes: Vec<&'static str>,
}

/// One manufacturing study composed with a bounded set of fresh production scenarios.
#[derive(Debug, Serialize)]
pub struct Report<T> {
    /// `ftx-material-scenario-matrix-study/1`, or `ftx-design-scenario-matrix-study/1`.
    pub schema: &'static str,
    /// Exact source pattern, authored material study, ordered cases and evaluation allowance.
    pub request_sha256: String,
    /// Caller-declared maximum callback slots, between one and 64 inclusive.
    pub max_evaluations: usize,
    /// All alternatives times all cases, reserved before any manufacturing or callback work.
    pub planned_evaluations: usize,
    /// Actual callback attempts; binding refusals retain their rows without invoking a consumer.
    pub evaluations: usize,
    /// Binding, screening and manufacturing run once for all cases.
    pub manufacturing: super::Report,
    /// Cases in caller order, each with alternatives in stable key order.
    pub cases: Vec<CaseReport<T>>,
    /// Present only when every case has an explicit effective comparison policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<Comparison>,
    /// Missing comparisons and physical-scope limitations, without hiding available case results.
    pub notes: Vec<&'static str>,
}

impl Request {
    /// Run one material/manufacturing study through one to eight named production scenarios.
    /// The callback starts fresh for each case and alternative. Case-specific physics budgets are
    /// the production adapter's responsibility; this layer reserves every requested callback slot.
    ///
    /// # Errors
    /// Invalid cases or effective comparison policies, excessive total evaluations, or the existing
    /// structural/manufacturing aggregate refusals. Individual binding and callback failures remain
    /// independent retained outcomes, as in `evaluate_with`.
    pub fn evaluate_matrix_with<T: Serialize>(
        &self,
        pattern: &Pattern,
        cases: &[Case],
        max_evaluations: usize,
        evaluate: impl FnMut(usize, &str, &Pattern) -> Result<T, Refusal>,
    ) -> Result<Report<T>, Refusal> {
        self.evaluate_matrix_with_fit(pattern, cases, max_evaluations, None, evaluate)
    }

    /// Evaluate named cases after one shared design-fit gate on the caller's fixed body.
    /// Body selection and measurement are performed once by the production adapter.
    ///
    /// # Errors
    /// The same case, work and structural refusals as [`Self::evaluate_matrix_with`].
    pub fn evaluate_matrix_with_fit<T: Serialize>(
        &self,
        pattern: &Pattern,
        cases: &[Case],
        max_evaluations: usize,
        fit: Option<&super::design::fit::Context<'_>>,
        mut evaluate: impl FnMut(usize, &str, &Pattern) -> Result<T, Refusal>,
    ) -> Result<Report<T>, Refusal> {
        if cases.is_empty() || cases.len() > 8 || !(1..=64).contains(&max_evaluations) {
            return Err(invalid(
                "scenario matrix needs 1..8 cases and 1..64 maximum evaluations",
            ));
        }
        let mut names = BTreeSet::new();
        for case in cases {
            if case.key.trim().is_empty() || !names.insert(&case.key) {
                return Err(invalid(
                    "scenario matrix case names must be unique and nonempty",
                ));
            }
            if let Some(policy) = case.comparison.as_ref().or(self.comparison.as_ref()) {
                policy.validate(self, true)?;
            }
        }
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(invalid(errors.join("; ")));
        }
        // Case policies have already been validated. An unused fallback must not reject valid
        // overrides, and shared manufacturing must not invent another comparison context.
        let mut manufacturing_request = self.clone();
        manufacturing_request.comparison = None;
        let (variants, screening) = manufacturing_request.variants(pattern, true)?;
        let planned_evaluations = variants.len().checked_mul(cases.len()).ok_or_else(|| {
            budget("scenario matrix alternative/case evaluation reservation overflows")
        })?;
        if planned_evaluations > max_evaluations {
            return Err(budget(
                "scenario matrix exceeds its aggregate alternative/case evaluation allowance",
            ));
        }
        let request_sha256 = if self.design_fit.is_some() {
            hash(&(
                pattern.raw(),
                self,
                cases,
                max_evaluations,
                fit.map(|context| context.identity),
            ))?
        } else {
            hash(&(pattern.raw(), self, cases, max_evaluations))?
        };
        let mut manufacturing =
            manufacturing_request.evaluate_prepared(pattern, true, variants, screening, fit)?;
        // The shared request is still the exact authored study, including its optional fallback.
        manufacturing.request_sha256 = self.manufacturing_identity(fit)?;
        let mut reports = Vec::with_capacity(cases.len());
        let mut evaluations = 0;
        for (case_index, case) in cases.iter().enumerate() {
            let mut effective_request = self.clone();
            effective_request.comparison = case.comparison.clone().or(self.comparison.clone());
            let context = effective_request.scenario_identity(pattern, &case.scenario, fit)?;
            let mut simulations = Vec::with_capacity(manufacturing.alternatives.len());
            for row in &manufacturing.alternatives {
                let evaluation = match &row.binding {
                    Outcome::Available { result } => {
                        let bound = Pattern::new(result["pattern"].clone());
                        evaluations += 1;
                        Outcome::from_result(evaluate(case_index, &row.key, &bound))
                    }
                    Outcome::Refused { refusal } => Outcome::Refused {
                        refusal: refusal.clone(),
                    },
                    Outcome::NotRequested => Outcome::Refused {
                        refusal: invalid("additional evaluation requires a successful binding"),
                    },
                };
                simulations.push(ScenarioAlternative {
                    key: row.key.clone(),
                    evaluation,
                });
            }
            let comparison = effective_request
                .comparison
                .as_ref()
                .map(|policy| {
                    policy.evaluate_scenarios(
                        &manufacturing.alternatives,
                        &simulations,
                        &case.scenario,
                        &manufacturing.source_pattern_sha256,
                        &context,
                        self.purchasing_currency(),
                    )
                })
                .transpose()?;
            reports.push(CaseReport {
                key: case.key.clone(),
                request_sha256: context,
                scenario: case.scenario.clone(),
                simulations,
                comparison,
            });
        }
        let comparison = comparison::Policy::evaluate_matrix(
            &manufacturing.source_pattern_sha256,
            &request_sha256,
            &reports,
        );
        let mut notes = vec![
            "One source-locked material and manufacturing study is shared by every named case. Each additional production evaluation starts fresh; a preceding case or material does not supply a cached physical state.",
            "The finite case set is an explicit sensitivity study, not a probability distribution, exhaustive operating envelope, comfort prediction or measured garment validation.",
        ];
        if comparison.is_none() {
            notes.push("At least one case has no explicit effective comparison policy. Requested per-case comparisons remain available; no all-cases policy or frontier is synthesized.");
        }
        Ok(Report {
            schema: if self.is_design() {
                "ftx-design-scenario-matrix-study/1"
            } else {
                "ftx-material-scenario-matrix-study/1"
            },
            request_sha256,
            max_evaluations,
            planned_evaluations,
            evaluations,
            manufacturing,
            cases: reports,
            comparison,
            notes,
        })
    }
}
