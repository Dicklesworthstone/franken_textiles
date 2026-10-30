//! All-cases Pareto decisions reuse the exact individual comparison relation.

use std::collections::BTreeMap;

use super::Policy;
use crate::material_study::matrix::{Candidate, CaseReport, Comparison, Incomparable};

impl Policy {
    /// The same admitted all-case relation for discovery and the final matrix frontier.
    /// Each coordinate keeps its own units, basis and source-protocol permission.
    pub(in crate::material_study) fn prefers_cases<'a>(
        cases: impl IntoIterator<Item = (&'a Self, &'a super::Candidate, &'a super::Candidate)>,
    ) -> bool {
        let mut strict = false;
        for (policy, a, b) in cases {
            if !a.eligible
                || !b.eligible
                || (!policy.allow_mixed_protocols
                    && a.input_protocol_sha256 != b.input_protocol_sha256)
            {
                return false;
            }
            let Some(better) = policy.relation(a, b) else {
                return false;
            };
            strict |= better;
        }
        strict
    }

    pub(in crate::material_study) fn evaluate_matrix<T>(
        source: &str,
        context: &str,
        cases: &[CaseReport<T>],
    ) -> Option<Comparison> {
        let reports = cases
            .iter()
            .map(|case| case.comparison.as_ref())
            .collect::<Option<Vec<_>>>()?;
        let first = reports.first()?;
        let case_order: Vec<_> = cases.iter().map(|case| case.key.clone()).collect();
        let mut alternatives: Vec<_> = first
            .alternatives
            .iter()
            .enumerate()
            .map(|(index, alternative)| {
                // Every report comes from the same retained manufacturing alternatives in key
                // order. Only their admitted readings vary; no imported aggregate is accepted here.
                let decisions: BTreeMap<_, _> = cases
                    .iter()
                    .zip(&reports)
                    .map(|(case, report)| (case.key.clone(), report.alternatives[index].clone()))
                    .collect();
                Candidate {
                    key: alternative.key.clone(),
                    eligible: decisions.values().all(|candidate| candidate.eligible),
                    cases: decisions,
                    dominated_by: Vec::new(),
                }
            })
            .collect();
        let relation =
            |a: &Candidate, b: &Candidate| {
                Self::prefers_cases(cases.iter().zip(&reports).map(|(case, report)| {
                    (&report.policy, &a.cases[&case.key], &b.cases[&case.key])
                }))
            };
        let mut incomparable = Vec::new();
        for i in 0..alternatives.len() {
            for j in i + 1..alternatives.len() {
                if !alternatives[i].eligible || !alternatives[j].eligible {
                    continue;
                }
                let incompatible: Vec<_> = cases
                    .iter()
                    .zip(&reports)
                    .filter(|(case, report)| {
                        !report.policy.allow_mixed_protocols
                            && alternatives[i].cases[&case.key].input_protocol_sha256
                                != alternatives[j].cases[&case.key].input_protocol_sha256
                    })
                    .map(|(case, _)| case.key.clone())
                    .collect();
                if !incompatible.is_empty() {
                    incomparable.push(Incomparable {
                        alternatives: [alternatives[i].key.clone(), alternatives[j].key.clone()],
                        cases: incompatible,
                        reason: "Relevant source protocols differ within the listed cases; no all-cases dominance inferred.",
                    });
                } else if relation(&alternatives[i], &alternatives[j]) {
                    let key = alternatives[i].key.clone();
                    alternatives[j].dominated_by.push(key);
                } else if relation(&alternatives[j], &alternatives[i]) {
                    let key = alternatives[j].key.clone();
                    alternatives[i].dominated_by.push(key);
                } else {
                    incomparable.push(Incomparable {
                        alternatives: [alternatives[i].key.clone(), alternatives[j].key.clone()],
                        cases: case_order.clone(),
                        reason: "The declared case objectives trade off or tie, or reported ranges do not establish strict dominance.",
                    });
                }
            }
        }
        let frontier = alternatives
            .iter()
            .filter(|candidate| candidate.eligible && candidate.dominated_by.is_empty())
            .map(|candidate| candidate.key.clone())
            .collect();
        Some(Comparison {
            schema: "ftx-material-scenario-matrix-comparison/1",
            source_pattern_sha256: source.into(),
            context_sha256: context.into(),
            case_order,
            policies: cases
                .iter()
                .zip(reports)
                .map(|(case, report)| (case.key.clone(), report.policy.clone()))
                .collect(),
            alternatives,
            frontier,
            incomparable,
            uncertainty_propagated: false,
            physical_validation_claim: false,
            notes: vec![
                "Eligibility requires every named case's declared objectives, constraints and required consumers. Each case retains its ordinary readings, exclusions and protocol identity; unrelated consumer failures are not hidden or substituted as zero.",
                "Dominance uses the existing nominal or reported-range relation separately for every case and objective: no worse everywhere and strictly better somewhere. There is no averaging, cross-case unit conversion, probability, weighted score or inferred global optimum.",
                "Source protocols are compared only between alternatives within the same named case. Different case environments and policies remain explicit; cross-protocol permission in one case does not relax another case.",
            ],
        })
    }
}
