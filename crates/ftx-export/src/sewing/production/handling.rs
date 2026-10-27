//! Source-qualified serial garment loading, transitions and unloading. Explicit operation/copy
//! identity prevents a repeated needle/thread setup from implying zero garment handling.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::super::{SewPlan, bad, ordering, qualified_text, sha256_text};
use super::{Operation, workholding};

const MAX_MILLISECONDS: u32 = 86_400_000;
const MAX_ROWS: usize = 8192;

/// Whether the consecutive operations act on one physical garment or require another garment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// Continue working on the same physical copy.
    SameCopy,
    /// Transfer to another physical copy, even when the operation/setup IDs are identical.
    DifferentCopy,
}

/// One directed, source-qualified interval between complete operation executions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    /// Original completed operation ID, before copy expansion.
    pub from: String,
    /// Original next operation ID, before copy expansion.
    pub to: String,
    /// Exact physical copy relationship, not an assumption derived from setup identity.
    pub relation: Relation,
    /// Serial handling duration, excluding already counted feed and needle/thread setup.
    pub milliseconds: u32,
}

/// Complete bounded handling model for the declared machine and qualified material/process domain.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `serial_operation_transitions/1`: handling, setup and feed are additive, nonoverlapping phases.
    pub model: String,
    /// Complete typed machine configuration lock.
    pub machine_sha256: String,
    /// Supported full source qualification locks for every original operation.
    pub qualifications: BTreeMap<String, Vec<workholding::QualificationRef>>,
    /// Explicit loading duration for every possible first original operation.
    pub initial_milliseconds: BTreeMap<String, u32>,
    /// Explicit unloading duration for every possible last original operation.
    pub final_milliseconds: BTreeMap<String, u32>,
    /// Complete directed operation/copy domain. Missing rows never mean zero.
    pub transitions: Vec<Transition>,
    /// Retained observations or estimates covering the complete table and its applicability.
    pub source: Source,
    /// Timing protocol and phase boundaries, including separation from setup and feed.
    pub method: String,
    /// Exact source conditions; not a live sensor observation or inferred workload match.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Source evidence classification; no statistical or worst-case bound is inferred.
    pub evidence: Evidence,
    /// Independent explicit consent for estimated handling times.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// One complete serial handling interval. The first has no predecessor; the last no successor.
#[derive(Clone, Debug, Serialize)]
pub struct Step {
    /// Previous physical scoped operation, absent for the initial load.
    pub from: Option<String>,
    /// Next physical scoped operation, absent for final unloading.
    pub to: Option<String>,
    /// Exact declared integer duration, including explicit zero observations.
    pub milliseconds: u64,
}

/// Full handling replay around every admitted operation, including both batch endpoints.
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    /// Ordered first load, between-operation transitions and last unload.
    pub steps: Vec<Step>,
    /// Exact sum, using u64 on native and wasm32.
    pub total_milliseconds: u64,
}

/// Qualified handling evidence and complete baseline/selected execution order.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-handling/1`.
    pub schema: &'static str,
    /// Exact complete machine identity admitted by the source policy.
    pub machine_sha256: String,
    /// Actual admitted material/process qualification for each original operation.
    pub qualifications: BTreeMap<String, workholding::QualificationRef>,
    /// Complete serial-copy baseline handling, before optimization.
    pub baseline: Timeline,
    /// Handling around the actual selected operation/stitch order.
    pub selected: Timeline,
    /// Declared source timing is not an independent measured factory validation.
    pub physical_timing_validated: bool,
}

fn budget() -> Refusal {
    Refusal::new(
        RefusalCode::BudgetExceeded,
        "handling supports at most 63 source operations, 256 qualification locks and 8192 transition rows",
    )
}

impl Policy {
    fn validate(
        &self,
        plan: &SewPlan,
    ) -> Result<BTreeMap<String, workholding::QualificationRef>, Refusal> {
        if self.qualifications.len() > 63
            || self.qualifications.values().map(Vec::len).sum::<usize>() > 256
            || self.initial_milliseconds.len() > 63
            || self.final_milliseconds.len() > 63
            || self.transitions.len() > MAX_ROWS
        {
            return Err(budget());
        }
        if self.model != "serial_operation_transitions/1"
            || !sha256_text(&self.machine_sha256)
            || self.machine_sha256 != workholding::machine_sha256(&plan.options.machine)?
            || !qualified_text(&self.source.reference)
            || !qualified_text(&self.source.accessed_on)
            || !qualified_text(&self.source.note)
            || !qualified_text(&self.method)
            || self.conditions.len() > 64
            || self
                .conditions
                .iter()
                .any(|(key, value)| !qualified_text(key) || !qualified_text(value))
        {
            return Err(bad(
                "invalid serial handling model, source metadata or full machine lock",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad("estimated handling needs its own explicit consent"));
        }
        let operations: BTreeSet<_> = plan
            .seams
            .iter()
            .map(|seam| seam.pass.operation_id())
            .collect();
        if self
            .qualifications
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != operations
            || self
                .initial_milliseconds
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                != operations
            || self
                .final_milliseconds
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                != operations
            || self
                .initial_milliseconds
                .values()
                .chain(self.final_milliseconds.values())
                .any(|&value| value > MAX_MILLISECONDS)
        {
            return Err(bad(
                "handling needs exact qualification, initial and final coverage of every original operation, with durations at most one day",
            ));
        }
        let qualified: BTreeMap<_, _> = plan
            .options
            .qualifications
            .iter()
            .map(|q| (q.id.as_str(), q))
            .collect();
        let mut admitted = BTreeMap::new();
        for seam in &plan.seams {
            let operation = seam.pass.operation_id();
            let references = &self.qualifications[operation];
            let mut seen = BTreeSet::new();
            for reference in references {
                let Some(qualification) = qualified.get(reference.id.as_str()) else {
                    return Err(bad("handling names an absent sewing qualification"));
                };
                if !sha256_text(&reference.sha256)
                    || !seen.insert(reference.id.as_str())
                    || reference.sha256 != workholding::qualification_sha256(qualification)?
                {
                    return Err(bad(
                        "handling sewing qualification content lock mismatch or duplicate reference",
                    ));
                }
            }
            let reference = seam
                .qualification_id
                .as_ref()
                .and_then(|id| references.iter().find(|r| &r.id == id));
            let Some(reference) = reference else {
                // Missing applicability is unresolved timing evidence, not an infeasible recipe.
                return Err(Refusal::new(
                    RefusalCode::NotConverged,
                    format!(
                        "{operation}: actual admitted qualification lacks handling timing evidence"
                    ),
                ));
            };
            admitted.insert(operation.to_string(), reference.clone());
        }
        let mut seen = BTreeSet::new();
        for row in &self.transitions {
            if !operations.contains(row.from.as_str())
                || !operations.contains(row.to.as_str())
                || (row.from == row.to && row.relation == Relation::SameCopy)
                || row.milliseconds > MAX_MILLISECONDS
                || !seen.insert((row.from.as_str(), row.to.as_str(), row.relation))
            {
                return Err(bad(
                    "invalid or duplicate operation/copy handling transition",
                ));
            }
        }
        Ok(admitted)
    }
}

pub(super) struct Compiled {
    pub(super) costs: ordering::HandlingCosts,
    machine_sha256: String,
    qualifications: BTreeMap<String, workholding::QualificationRef>,
}

impl Compiled {
    fn timeline(&self, order: &[String]) -> Result<Timeline, Refusal> {
        let ids: BTreeMap<_, _> = self
            .costs
            .ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();
        let mut last = self.costs.ids.len();
        let mut previous = None;
        let mut seen = BTreeSet::new();
        let mut steps = Vec::with_capacity(order.len() + 1);
        for id in order {
            let next = *ids
                .get(id.as_str())
                .ok_or_else(|| bad("unknown handling replay operation"))?;
            if !seen.insert(next) {
                return Err(bad("repeated handling replay operation"));
            }
            steps.push(Step {
                from: previous.clone(),
                to: Some(id.clone()),
                milliseconds: self.costs.transitions[last][next],
            });
            previous = Some(id.clone());
            last = next;
        }
        if seen.len() != self.costs.ids.len() || previous.is_none() {
            return Err(bad("incomplete handling replay operation order"));
        }
        steps.push(Step {
            from: previous,
            to: None,
            milliseconds: self.costs.final_milliseconds[last],
        });
        Ok(Timeline {
            total_milliseconds: steps.iter().map(|step| step.milliseconds).sum(),
            steps,
        })
    }

    pub(super) fn report(
        self,
        baseline: &[String],
        selected: &[String],
    ) -> Result<Report, Refusal> {
        let baseline = self.timeline(baseline)?;
        let selected = self.timeline(selected)?;
        Ok(Report {
            schema: "ftx-sewing-handling/1",
            machine_sha256: self.machine_sha256,
            qualifications: self.qualifications,
            baseline,
            selected,
            physical_timing_validated: false,
        })
    }
}

pub(super) fn compile(
    plan: &SewPlan,
    policy: &Policy,
    operations: &[Operation],
) -> Result<Compiled, Refusal> {
    let qualifications = policy.validate(plan)?;
    let mut sorted: Vec<_> = operations.iter().collect();
    sorted.sort_by_key(|operation| &operation.id);
    let rows: BTreeMap<_, _> = policy
        .transitions
        .iter()
        .map(|row| {
            (
                (row.from.as_str(), row.to.as_str(), row.relation),
                u64::from(row.milliseconds),
            )
        })
        .collect();
    let mut transitions = Vec::with_capacity(sorted.len() + 1);
    for from in &sorted {
        transitions.push(
            sorted
                .iter()
                .map(|to| {
                    if from.id == to.id {
                        return Ok(0);
                    }
                    let relation = if from.copy == to.copy {
                        Relation::SameCopy
                    } else {
                        Relation::DifferentCopy
                    };
                    rows.get(&(
                        from.operation_id.as_str(),
                        to.operation_id.as_str(),
                        relation,
                    ))
                    .copied()
                    .ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::NotConverged,
                            format!(
                                "missing handling timing: {} -> {} ({relation:?})",
                                from.operation_id, to.operation_id
                            ),
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    transitions.push(
        sorted
            .iter()
            .map(|operation| u64::from(policy.initial_milliseconds[&operation.operation_id]))
            .collect(),
    );
    let final_milliseconds = sorted
        .iter()
        .map(|operation| u64::from(policy.final_milliseconds[&operation.operation_id]))
        .collect();
    Ok(Compiled {
        costs: ordering::HandlingCosts {
            ids: sorted
                .iter()
                .map(|operation| operation.id.clone())
                .collect(),
            transitions,
            final_milliseconds,
        },
        machine_sha256: policy.machine_sha256.clone(),
        qualifications,
    })
}
