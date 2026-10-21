//! Measured branch paths, not an inferred hysteresis or recovery law. A reversal must be linked by
//! an independently supplied source receipt and meet at the current pressure and EVERY layer's
//! thickness. All branch changes and thermal holds use the original compression/heat authorities.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::{material_compression::stack, materials::Evidence};
use serde::{Deserialize, Serialize};

use crate::thermal::adaptive;

use super::{
    CompressedStack, Hold, MAX_STEPS, Setup, StepReport, adaptive_budget, check_storage, invalid,
    model,
};

/// Receipt for traversing from one acquired branch to another on the same specimen/history.
/// A numerical intersection alone cannot establish that two independently acquired curves connect.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionEvidence {
    /// Source key in the unchanged original material record.
    pub source: String,
    /// Estimated links need both the preceding and replacement layer's explicit estimate consent.
    pub evidence: Evidence,
    /// Source protocol establishing this reversal, including peak/history and dwell applicability.
    pub method: String,
    /// Must match specimen, conditioning, from_loading_branch, to_loading_branch and pressure_pa.
    /// Other source conditions are retained, not interpreted as an automatic history model.
    pub conditions: BTreeMap<String, String>,
}

/// An explicit new branch at the CURRENT mechanical state, not a replacement initial state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchChange {
    /// Original acquisitions for the opposite travel direction. The authored entry pressure and
    /// clearance must reproduce the current state without travel/work. Area and ordered layer IDs
    /// stay fixed; every original material, specimen, conditioning and consent policy is unchanged.
    pub entry: stack::Input,
    /// One source-qualified transition for each physical layer, keyed by its unchanged layer ID.
    pub transitions: BTreeMap<String, TransitionEvidence>,
}

/// A successful source transition and the first thermal hold after it; published atomically.
#[derive(Clone, Debug, Serialize)]
pub struct BranchStep {
    /// Identity of the preceding actual compression receipt, before changing source branches.
    pub previous_compression_sha256: String,
    /// Full original source link; no inferred branch, offset or peak correction.
    pub change: BranchChange,
    /// Entry on the new branch, with zero external work and no temperature reset.
    pub entry_compression: stack::Report,
    /// Ordinary committed step; cumulative heat/work span all preceding branches.
    pub step: StepReport,
}

fn text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096
}

// Numerical representation agreement only, NOT an observational or fitted join tolerance.
fn same(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 128.0 * f64::EPSILON * a.abs().max(b.abs())
}

impl CompressedStack {
    /// Reverse onto a separately acquired branch and advance one thermal hold, as one transaction.
    /// Only the active branch changes. Mass cells, temperatures, initial heat reference, work and
    /// heat ledgers, and the global committed-step budget carry across the reversal unchanged.
    ///
    /// # Errors
    /// Missing source links, changed physical identity/consent, nonzero entry travel, discontinuous
    /// layer thickness, unqualified storage, or any ordinary compression/thermal failure. A failed
    /// first hold does not leave the replacement branch active; retries see the original state.
    pub fn advance_on(
        &mut self,
        hold: &Hold,
        change: &BranchChange,
    ) -> Result<BranchStep, Refusal> {
        self.advance_on_inner(hold, change, None)
    }

    /// Commit a qualified reversal and adaptive hold without resetting the caller's work budget.
    /// Failed attempts leave the active branch and physical state unchanged; work remains charged.
    ///
    /// # Errors
    /// The same source/branch refusals as [`Self::advance_on`], or adaptive tolerance/work failure.
    pub fn advance_on_adaptive(
        &mut self,
        hold: &Hold,
        change: &BranchChange,
        budget: &mut adaptive::Budget,
    ) -> Result<BranchStep, Refusal> {
        self.advance_on_inner(hold, change, Some(budget))
    }

    fn advance_on_inner(
        &mut self,
        hold: &Hold,
        change: &BranchChange,
        budget: Option<&mut adaptive::Budget>,
    ) -> Result<BranchStep, Refusal> {
        let old = &self.setup.stack;
        let new = &change.entry;
        if self.committed_steps == MAX_STEPS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "compressed thermal step budget exhausted",
            ));
        }
        if old.direction == new.direction
            || old.reference_area_m2 != new.reference_area_m2
            || old.allow_estimates != new.allow_estimates
            || old.layers.len() != new.layers.len()
            || change.transitions.len() != new.layers.len()
            || !same(new.gap_m, self.compression.thickness_m)
        {
            return Err(invalid(
                "branch reversal needs opposite direction, unchanged area/layers/consent, current gap and one source link per layer",
            ));
        }
        for (prior, next) in old.layers.iter().zip(&new.layers) {
            let (a, b) = (&prior.preparation, &next.preparation);
            if prior.id != next.id
                || a.specimen != b.specimen
                || a.conditioning != b.conditioning
                || a.allow_estimates != b.allow_estimates
                || a.loading_branch == b.loading_branch
                || !same(b.pressure_pa, self.compression.pressure_pa)
                || serde_json::to_vec(&a.material).map_err(|e| invalid(e.to_string()))?
                    != serde_json::to_vec(&b.material).map_err(|e| invalid(e.to_string()))?
            {
                return Err(invalid(
                    "branch reversal cannot change original material, physical layer order, specimen, conditioning, consent or current pressure",
                ));
            }
            let link = change
                .transitions
                .get(&prior.id)
                .ok_or_else(|| invalid("missing physical layer's branch transition receipt"))?;
            if !a.material.sources.contains_key(&link.source)
                || !text(&link.method)
                || link.conditions.len() > 64
                || link.conditions.iter().any(|(k, v)| !text(k) || !text(v))
                || (link.evidence == Evidence::Estimate && !a.allow_estimates)
            {
                return Err(invalid(
                    "branch transition needs a resolved source, method, bounded conditions and admitted evidence",
                ));
            }
            for (key, expected) in [
                ("specimen", &a.specimen),
                ("conditioning", &a.conditioning),
                ("from_loading_branch", &a.loading_branch),
                ("to_loading_branch", &b.loading_branch),
            ] {
                if link.conditions.get(key) != Some(expected) {
                    return Err(invalid(format!("branch transition must match {key}")));
                }
            }
            if !link
                .conditions
                .get("pressure_pa")
                .and_then(|p| p.parse::<f64>().ok())
                .is_some_and(|p| same(p, self.compression.pressure_pa))
            {
                return Err(invalid(
                    "branch transition must qualify the actual reversal pressure",
                ));
            }
        }
        check_storage(new)?;
        let entry = stack::close(new)?;
        if entry.pressure_pa != entry.initial_pressure_pa
            || entry.external_work_j != 0.0
            || entry.layers.iter().any(|l| l.external_work_j_m2 != 0.0)
            || entry
                .layers
                .iter()
                .zip(&self.compression.layers)
                .any(|(a, b)| {
                    !same(a.thickness_m, b.thickness_m)
                        || a.reference_mass_kg != b.reference_mass_kg
                })
        {
            return Err(invalid(
                "new branch must meet every layer at current pressure with zero entry travel/work; summed thickness alone is insufficient",
            ));
        }
        let thermal = model(
            &entry,
            &vec![0.0; entry.layers.len() - 1],
            self.setup.cells_per_layer,
        )?;
        if thermal.variable.is_some() || thermal.capacity != self.capacity {
            return Err(invalid(
                "branch reversal cannot change material-cell heat storage",
            ));
        }
        // The existing step remains the only heat/work update and budget authority. A late error
        // cannot leak a new branch, pressure, source lock, temperature or cumulative ledger.
        let mut trial = self.clone();
        trial.setup.stack = new.clone();
        trial.compression = entry.clone();
        let step = match budget {
            Some(budget) => trial.advance_adaptive(hold, budget)?,
            None => trial.advance(hold)?,
        };
        let report = BranchStep {
            previous_compression_sha256: self.compression.input_sha256.clone(),
            change: change.clone(),
            entry_compression: entry,
            step,
        };
        *self = trial;
        Ok(report)
    }
}

/// Consecutive holds on one branch. Omit `branch_change` to continue the active acquisition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leg {
    /// Source-qualified reversal before the first hold, committed only if that hold succeeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_change: Option<BranchChange>,
    /// Positive-duration thermal holds; the same declared branch is used throughout this leg.
    pub holds: Vec<Hold>,
}

/// Version 2 extends the original one-branch request without reinterpreting version 1 inputs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// `ftx-compressed-thermal-history/2`.
    pub schema: String,
    /// Original source/state and thermal reference; never reset between legs.
    pub setup: Setup,
    /// One to 32 explicit legs, at most 512 holds and 262144 cell-holds in total.
    pub legs: Vec<Leg>,
    /// Optional local time-error control; the same budget spans all legs and branch changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<adaptive::Options>,
}

/// One branch transition at its unambiguous global hold index. Retains the zero-work entry receipt.
#[derive(Clone, Debug, Serialize)]
pub struct TransitionReport {
    /// Zero-based global hold index, including holds in preceding legs.
    pub before_hold: usize,
    /// Complete change and its first accepted hold. No heat or mechanical work occurs at the join.
    pub result: BranchStep,
}

/// Complete path result; any failed leg returns a refusal instead of partial success.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-compressed-thermal-history-report/2`.
    pub schema: &'static str,
    /// All authored branches, transition evidence and holds in acquisition order.
    pub input: Input,
    /// Digest of the complete typed input.
    pub input_sha256: String,
    /// Initial preparation, excluded from subsequent work/heat ledgers.
    pub initial_compression: stack::Report,
    /// Global chronological steps; all cumulative quantities span branch reversals.
    pub steps: Vec<StepReport>,
    /// Source transitions, never silently inferred from a changed gap or a branch label.
    pub transitions: Vec<TransitionReport>,
    /// Last branch's ordinary frozen selections, not a new hysteretic material type.
    pub final_compression: stack::Report,
    /// The boundary between numerical branch traversal and physical qualification.
    pub no_claim: &'static str,
}

/// Traverse explicit qualified loading/unloading legs without reinitializing material cells.
///
/// # Errors
/// Invalid schema, excessive total work, missing links, discontinuous branches or any refused hold.
/// No partial path or usable final selection is returned, including on failures in the last leg.
pub fn evaluate(input: &Input) -> Result<Report, Refusal> {
    if input.schema != "ftx-compressed-thermal-history/2" {
        return Err(invalid("expected ftx-compressed-thermal-history/2"));
    }
    let holds = input
        .legs
        .iter()
        .try_fold(0usize, |n, leg| n.checked_add(leg.holds.len()));
    let work = input
        .setup
        .stack
        .layers
        .len()
        .checked_mul(input.setup.cells_per_layer)
        .and_then(|cells| holds.and_then(|h| cells.checked_mul(h)));
    if !(1..=32).contains(&input.legs.len())
        || input.legs.iter().any(|l| l.holds.is_empty())
        || holds.is_none_or(|h| h > MAX_STEPS)
        || work.is_none_or(|w| w > 262_144)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "compressed thermal path needs 1..32 nonempty legs, at most 512 total holds and 262144 cell-holds",
        ));
    }
    let mut budget = adaptive_budget(input.adaptive.as_ref())?;
    let mut state = CompressedStack::new(&input.setup)?;
    let initial_compression = state.compression().clone();
    let mut steps = Vec::with_capacity(holds.expect("checked hold budget"));
    let mut transitions = Vec::new();
    for leg in &input.legs {
        for (i, hold) in leg.holds.iter().enumerate() {
            if let Some(change) = leg.branch_change.as_ref().filter(|_| i == 0) {
                let result = match &mut budget {
                    Some(budget) => state.advance_on_adaptive(hold, change, budget)?,
                    None => state.advance_on(hold, change)?,
                };
                steps.push(result.step.clone());
                transitions.push(TransitionReport {
                    before_hold: steps.len() - 1,
                    result,
                });
            } else {
                steps.push(match &mut budget {
                    Some(budget) => state.advance_adaptive(hold, budget)?,
                    None => state.advance(hold)?,
                });
            }
        }
    }
    Ok(Report {
        schema: "ftx-compressed-thermal-history-report/2",
        input: input.clone(),
        input_sha256: sha256_hex(&serde_json::to_vec(input).map_err(|e| invalid(e.to_string()))?),
        initial_compression,
        steps,
        transitions,
        final_compression: state.compression().clone(),
        no_claim: "Explicit source-qualified monotone branches joined at the same pressure and each layer's thickness. Fixed area/reference mass/heat capacity, no temperature resets, quasi-static changes followed by prescribed dry thermal holds. No inferred minor loop, recovery time, damage law, continuous squeeze, thermomechanical feedback, mechanical heat conversion, garment pressure field or empirical qualification. Net cycle work remains external work, not a measured dissipation or heat claim.",
    })
}
