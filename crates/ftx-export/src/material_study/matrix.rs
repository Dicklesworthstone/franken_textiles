//! Named production scenarios over one frozen material study and one manufacturing evaluation.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_pattern::Pattern;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    Alternative, ManufacturingWork, Outcome, Request, ScenarioAlternative, budget, comparison,
    design, hash, invalid, scenario_alternative,
};

struct GuidedProbe<T> {
    manufacturing: Alternative,
    simulations: Vec<ScenarioAlternative<T>>,
}

struct GuidedEvaluation<T> {
    manufacturing: super::Report,
    simulations: Vec<Vec<ScenarioAlternative<T>>>,
    planned_evaluations: usize,
    evaluations: usize,
}

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
    /// Consumer-guided discovery additionally reserves every possible probe for every case.
    pub planned_evaluations: usize,
    /// Actual callback attempts, including adaptive probes; cached final outputs add no attempts.
    /// Binding refusals retain their rows without invoking a consumer.
    pub evaluations: usize,
    /// Binding, screening and manufacturing run once for all cases.
    pub manufacturing: super::Report,
    /// Cases in caller order, each with alternatives in stable key order.
    pub cases: Vec<CaseReport<T>>,
    /// Present only when every case has an explicit effective comparison policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<Comparison>,
    /// Missing comparisons and physical-scope limitations, without hiding available case results.
    pub notes: Vec<String>,
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
        let (mut manufacturing, mut cached, planned_evaluations, mut evaluations) = if self
            .consumer_guided()
        {
            let result =
                self.evaluate_consumer_matrix(pattern, cases, max_evaluations, fit, &mut evaluate)?;
            (
                result.manufacturing,
                Some(result.simulations.into_iter()),
                result.planned_evaluations,
                result.evaluations,
            )
        } else {
            // An unused fallback must not reject valid case overrides, and shared
            // manufacturing must not invent another comparison context.
            let mut manufacturing_request = self.clone();
            manufacturing_request.comparison = None;
            let prepared = manufacturing_request.variants(pattern, true, fit)?;
            let planned_evaluations = prepared
                .variants
                .len()
                .checked_mul(cases.len())
                .ok_or_else(|| {
                    budget("scenario matrix alternative/case evaluation reservation overflows")
                })?;
            if planned_evaluations > max_evaluations {
                return Err(budget(
                    "scenario matrix exceeds its aggregate alternative/case evaluation allowance",
                ));
            }
            (
                manufacturing_request.evaluate_prepared(pattern, true, prepared, fit)?,
                None,
                planned_evaluations,
                0,
            )
        };
        // The shared request is still the exact authored study, including its optional fallback.
        manufacturing.request_sha256 = self.manufacturing_identity(fit)?;
        let mut reports = Vec::with_capacity(cases.len());
        for (case_index, case) in cases.iter().enumerate() {
            let mut effective_request = self.clone();
            effective_request.comparison = case.comparison.clone().or(self.comparison.clone());
            let context = effective_request.scenario_identity(pattern, &case.scenario, fit)?;
            let simulations = if let Some(cached) = &mut cached {
                cached
                    .next()
                    .expect("one cached output vector for every case")
            } else {
                manufacturing
                    .alternatives
                    .iter()
                    .map(|row| {
                        scenario_alternative(row, &mut |key, bound| {
                            evaluations += 1;
                            evaluate(case_index, key, bound)
                        })
                    })
                    .collect()
            };
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
            "One source-locked material and manufacturing study is shared by every named case. Each additional production evaluation starts fresh; a preceding case or material does not supply a cached physical state.".into(),
            "The finite case set is an explicit sensitivity study, not a probability distribution, exhaustive operating envelope, comfort prediction or measured garment validation.".into(),
        ];
        if comparison.is_none() {
            notes.push("At least one case has no explicit effective comparison policy. Requested per-case comparisons remain available; no all-cases policy or frontier is synthesized.".into());
        }
        if self
            .design_adaptive
            .as_ref()
            .is_some_and(|policy| policy.worst_case())
        {
            notes.push("Adaptive polling improves the worst admitted reading of one shared objective across all named cases under the declared nominal or reported-range basis. Every case retains its own constraints and source-protocol admission. No case weights, cross-unit score, unrestricted optimum or physical fit is inferred.".into());
            // The browser already renders these authoritative notes as text. Keep the same
            // explanation in CLI/portable reports instead of deriving another browser score.
            let guidance = match &manufacturing.design_search {
                Some(design::SearchReceipt::Adaptive(search)) => search.consumer_guidance.as_ref(),
                _ => None,
            };
            let incumbent = guidance.and_then(|guidance| {
                Some((
                    guidance,
                    guidance.best_trial_key.as_ref()?,
                    guidance.best_case_key.as_ref()?,
                    guidance.best_reading.as_ref()?,
                    guidance.objective.as_ref()?,
                ))
            });
            notes.push(if let Some((guidance, trial, case, reading, objective)) = incumbent {
                if let (Some([lo, hi]), Some([lower_case, upper_case])) = (
                    reading.reported_bounds,
                    guidance.best_reported_bound_case_keys.as_ref(),
                ) {
                    format!(
                        "Worst-case retained trial '{trial}' on material '{}': nominal limiting case '{case}', {} = {} {}; reported worst-case range [{lo}, {hi}] {}, with lower endpoint from '{lower_case}' and upper endpoint from '{upper_case}'. Endpoints aggregate the same metric across all admitted cases, not a single case's interval or an average. Only strictly separated ranges establish improvement; overlaps and touching endpoints remain unresolved. Source ranges are conditional enclosures, not statistical confidence or uncertain geometry. Inspect consumer_cases for all original readings and refusals.",
                        guidance.material_key,
                        serde_json::to_string(&objective.metric)
                            .map_err(|error| invalid(error.to_string()))?,
                        reading.value,
                        reading.unit,
                        reading.unit
                    )
                } else {
                    format!(
                    "Worst-case retained trial '{trial}' on material '{}': limiting case '{case}', {} = {} {}. This is the original admitted case reading, not an average. Every named case admitted this trial; the limiting case can change between trials. Inspect consumer_cases for all readings and refusals.",
                    guidance.material_key,
                    serde_json::to_string(&objective.metric)
                        .map_err(|error| invalid(error.to_string()))?,
                    reading.value,
                    reading.unit
                    )
                }
            } else {
                "No worst-case trial was admitted in every named case. No limiting case or objective value is available; inspect the retained per-case refusals. The separately retained original baseline is not a discovery incumbent.".into()
            });
        } else if self
            .design_adaptive
            .as_ref()
            .is_some_and(|policy| policy.pareto())
        {
            notes.push("Adaptive Pareto polling follows the explicitly nominated case's complete comparison objectives under their declared nominal or reported-range basis. Every named case independently admits each trial under its own constraints and anchored source protocol. The evaluated nondominated frontier and bounded retained output subset are separate; capacity-omitted frontier points still guide polling. No case weights, scalar winner, exhaustive frontier or physical fit is inferred.".into());
        } else if self.consumer_guided() {
            notes.push("Adaptive polling follows only the explicitly nominated case's sole comparison objective and declared nominal or reported-range basis. Every named case must independently admit each selected trial under its own policy and source protocol. Complete successful probe outputs are reused exactly for final rows; no cross-case score, unrestricted optimum or physical fit is inferred.".into());
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

    fn evaluate_consumer_matrix<T: Serialize>(
        &self,
        pattern: &Pattern,
        cases: &[Case],
        max_evaluations: usize,
        fit: Option<&design::fit::Context<'_>>,
        evaluate: &mut impl FnMut(usize, &str, &Pattern) -> Result<T, Refusal>,
    ) -> Result<GuidedEvaluation<T>, Refusal> {
        let adaptive = self
            .design_adaptive
            .as_ref()
            .expect("consumer-guided matrix");
        let nominated = if adaptive.worst_case() {
            None
        } else {
            let case_key = adaptive.case_key.as_ref().ok_or_else(|| {
                invalid("consumer-guided named matrices require an explicit adaptive case_key")
            })?;
            Some(
                cases
                    .iter()
                    .position(|case| case.key == *case_key)
                    .ok_or_else(|| {
                        invalid("adaptive case_key must identify an authored named case")
                    })?,
            )
        };
        let policies = cases
            .iter()
            .map(|case| {
                case.comparison.as_ref().or(self.comparison.as_ref()).ok_or_else(|| {
                    invalid(format!(
                        "adaptive matrix case '{}' requires an explicit effective comparison policy",
                        case.key
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if adaptive.worst_case() {
            let first = policies[0].objectives.first();
            if policies.iter().any(|policy| {
                std::mem::discriminant(&policy.basis) != std::mem::discriminant(&policies[0].basis)
                    || policy.objectives.len() != 1
                    || first.is_none_or(|first| {
                        policy.objectives[0].metric != first.metric
                            || std::mem::discriminant(&policy.objectives[0].direction)
                                != std::mem::discriminant(&first.direction)
                    })
            }) {
                return Err(invalid(
                    "worst-case guidance requires one identical metric, direction and comparison basis in every case",
                ));
            }
        }
        // Resolve the guidance policy before the ordinary design/material admission. An
        // unused fallback is not the nominated case's objective and cannot silently steer search.
        let mut guided = self.clone();
        guided.comparison = Some(policies[nominated.unwrap_or(0)].clone());
        let materials = guided.material_variants(pattern, true)?;
        let slots = adaptive
            .max_evaluations
            .saturating_add((adaptive.max_designs + 1).saturating_mul(materials.variants.len()));
        let planned_evaluations = slots.saturating_mul(cases.len());
        if planned_evaluations > max_evaluations {
            return Err(budget(
                "adaptive matrix exceeds its aggregate probe and final case evaluation allowance",
            ));
        }
        self.reserve_consumers(pattern, slots)?;
        let source = hash(pattern.raw())?;
        let contexts = cases
            .iter()
            .zip(&policies)
            .map(|(case, policy)| {
                let mut effective = self.clone();
                effective.comparison = Some((*policy).clone());
                effective.scenario_identity(pattern, &case.scenario, fit)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let case_order: Vec<_> = cases.iter().map(|case| case.key.clone()).collect();
        let mut work = ManufacturingWork::new();
        let mut evaluations = 0;
        let mut protocols: Option<Vec<Option<String>>> = None;
        let (mut variants, receipt, mut retained) = adaptive.expand_with(
            &guided,
            pattern,
            materials.variants,
            fit,
            Some(&mut |variant| {
                let row = self.evaluate_charged(pattern, variant, fit, &mut work)?;
                let simulations = evaluate_cases(&row, cases.len(), &mut evaluations, evaluate);
                let decisions = cases
                    .iter()
                    .enumerate()
                    .map(|(index, case)| {
                        let mut comparison = policies[index].evaluate_scenarios(
                            std::slice::from_ref(&row),
                            std::slice::from_ref(&simulations[index]),
                            &case.scenario,
                            &source,
                            &contexts[index],
                            self.purchasing_currency(),
                        )?;
                        let refusal = match &simulations[index].evaluation {
                            Outcome::Refused { refusal } => Some(refusal.clone()),
                            _ => None,
                        };
                        Ok(design::adaptive::CaseDecision {
                            key: case.key.clone(),
                            comparison: comparison.alternatives.remove(0),
                            refusal,
                        })
                    })
                    .collect::<Result<Vec<_>, Refusal>>()?;
                let mut admission_refusal = decisions
                    .iter()
                    .find(|decision| !decision.comparison.eligible)
                    .map(|decision| {
                        invalid(format!(
                            "adaptive matrix case '{}' did not admit this trial: {}",
                            decision.key,
                            decision.comparison.exclusions.first().map_or(
                                "comparison unavailable",
                                |refusal| refusal.message.as_str()
                            )
                        ))
                    });
                let mut guiding = nominated.unwrap_or(0);
                if adaptive.worst_case() && admission_refusal.is_none() {
                    match design::adaptive::worst_case_index(&decisions, &policies[0].objectives[0]) {
                        Ok(index) => guiding = index,
                        Err(refusal) => admission_refusal = Some(refusal),
                    }
                }
                if admission_refusal.is_none() {
                    let current: Vec<_> = decisions
                        .iter()
                        .map(|decision| decision.comparison.input_protocol_sha256.clone())
                        .collect();
                    if let Some(first) = &protocols {
                        if let Some(index) = (0..cases.len()).find(|&index| {
                            !policies[index].allow_mixed_protocols && current[index] != first[index]
                        }) {
                            admission_refusal = Some(invalid(format!(
                                "adaptive matrix case '{}' source protocols differ from the first admitted incumbent; no ordering inferred",
                                cases[index].key
                            )));
                        }
                    } else {
                        protocols = Some(current);
                    }
                }
                Ok(design::adaptive::ConsumerEvaluation {
                    comparison: decisions[guiding].comparison.clone(),
                    refusal: decisions[guiding].refusal.clone(),
                    cases: decisions,
                    admission_refusal,
                    output: GuidedProbe {
                        manufacturing: row,
                        simulations,
                    },
                })
            }),
            &case_order,
        )?;
        variants.sort_by(|a, b| a.variant.key.cmp(&b.variant.key));
        let mut alternatives = Vec::with_capacity(variants.len());
        let mut simulations: Vec<Vec<ScenarioAlternative<T>>> = (0..cases.len())
            .map(|_| Vec::with_capacity(variants.len()))
            .collect();
        for variant in variants {
            let probe = if let Some(probe) = retained.remove(&variant.variant.key) {
                probe
            } else {
                let row = self.evaluate_charged(pattern, variant, fit, &mut work)?;
                let simulations = evaluate_cases(&row, cases.len(), &mut evaluations, evaluate);
                GuidedProbe {
                    manufacturing: row,
                    simulations,
                }
            };
            alternatives.push(probe.manufacturing);
            for (case, output) in simulations.iter_mut().zip(probe.simulations) {
                case.push(output);
            }
        }
        let manufacturing = self.finish_manufacturing(
            pattern,
            true,
            materials.screening,
            Some(design::SearchReceipt::Adaptive(receipt)),
            alternatives,
            fit,
        )?;
        Ok(GuidedEvaluation {
            manufacturing,
            simulations,
            planned_evaluations,
            evaluations,
        })
    }
}

fn evaluate_cases<T>(
    row: &Alternative,
    count: usize,
    evaluations: &mut usize,
    evaluate: &mut impl FnMut(usize, &str, &Pattern) -> Result<T, Refusal>,
) -> Vec<ScenarioAlternative<T>> {
    (0..count)
        .map(|index| {
            scenario_alternative(row, &mut |key, bound| {
                *evaluations += 1;
                evaluate(index, key, bound)
            })
        })
        .collect()
}
