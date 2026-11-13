//! Measured branch paths, not an inferred hysteresis or recovery law. A reversal must be linked by
//! an independently supplied source receipt and meet at the current pressure and EVERY layer's
//! thickness. All branch changes and thermal holds use the original compression/heat authorities.

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::material_compression::stack;
use serde::{Deserialize, Serialize};

use crate::thermal::adaptive;

use super::{
    CompressedStack, ControlledHold, Hold, MAX_STEPS, Setup, StepReport, adaptive_budget,
    check_storage, invalid, model,
};

pub use stack::{BranchChange, TransitionEvidence};

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
        self.advance_on_inner(hold, None, false, change, None)
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
        self.advance_on_inner(hold, None, false, change, Some(budget))
    }

    /// Qualify a branch reversal and its first explicitly pressure- or gap-controlled hold in
    /// one transaction. The source join remains at the actual current pressure and layer states.
    ///
    /// # Errors
    /// The same source continuity and thermal failures as [`Self::advance_on`], or an inadmissible
    /// prescribed pressure. A failed first hold does not activate the replacement branch.
    pub fn advance_controlled_on(
        &mut self,
        hold: &ControlledHold,
        change: &BranchChange,
    ) -> Result<BranchStep, Refusal> {
        let (hold, pressure) = hold.parameters(self.compression.thickness_m);
        self.advance_on_inner(&hold, pressure, true, change, None)
    }

    /// Advance a qualified reversal and explicit boundary while retaining charged adaptive work.
    ///
    /// # Errors
    /// As [`Self::advance_controlled_on`], or adaptive tolerance/work exhaustion. All physical
    /// state and cumulative ledgers roll back together on failure.
    pub fn advance_controlled_on_adaptive(
        &mut self,
        hold: &ControlledHold,
        change: &BranchChange,
        budget: &mut adaptive::Budget,
    ) -> Result<BranchStep, Refusal> {
        let (hold, pressure) = hold.parameters(self.compression.thickness_m);
        self.advance_on_inner(&hold, pressure, true, change, Some(budget))
    }

    fn advance_on_inner(
        &mut self,
        hold: &Hold,
        prescribed_pressure: Option<f64>,
        known_pressure_entry: bool,
        change: &BranchChange,
        budget: Option<&mut adaptive::Budget>,
    ) -> Result<BranchStep, Refusal> {
        let new = &change.entry;
        if self.committed_steps == MAX_STEPS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "compressed thermal step budget exhausted",
            ));
        }
        check_storage(new)?;
        let entry = stack::qualify_branch_change(
            &self.setup.stack,
            &self.compression,
            change,
            known_pressure_entry,
        )?;
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
        let step = trial.advance_inner(hold, prescribed_pressure, budget)?;
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

/// Consecutive holds with explicit pressure/clearance control, on one acquired source branch.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlledLeg {
    /// Independently source-qualified reversal before this leg's first hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_change: Option<BranchChange>,
    /// One to 512 holds across the entire request; each declares exactly one mechanical boundary.
    pub holds: Vec<ControlledHold>,
}

/// Version 3 applies known pressures as well as inverse clearances without guessing source branches.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlledInput {
    /// `ftx-compressed-thermal-history/3`.
    pub schema: String,
    /// Original gap-qualified preparation and independent post-preparation temperature state.
    pub setup: Setup,
    /// Explicit source legs; switching controls alone does not switch the active source branch.
    pub legs: Vec<ControlledLeg>,
    /// One shared adaptive thermal work policy across all controls, holds and reversals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<adaptive::Options>,
}

/// Complete pressure/clearance-controlled path using the original compression and heat authorities.
#[derive(Clone, Debug, Serialize)]
pub struct ControlledReport {
    /// `ftx-compressed-thermal-history-report/3`.
    pub schema: &'static str,
    /// Exact typed source inputs and control schedule.
    pub input: ControlledInput,
    /// Identity of the complete typed request.
    pub input_sha256: String,
    /// Initial preparation; its work remains outside the heat-history ledger.
    pub initial_compression: stack::Report,
    /// Every accepted hold and its explicit pressure-control marker when applicable.
    pub steps: Vec<StepReport>,
    /// Independently qualified source branch joins, indexed in the complete history.
    pub transitions: Vec<TransitionReport>,
    /// Final source-qualified selections for ordinary downstream material consumers.
    pub final_compression: stack::Report,
    /// Numerical and physical scope of the supported boundary conditions.
    pub no_claim: &'static str,
}

/// Apply explicit pressures/clearances to source-qualified loading and unloading paths.
/// All histories, controls and transitions use one state and one aggregate work admission.
///
/// # Errors
/// Invalid schema/control, excessive work, unsupported source domains, missing branch links or
/// any refused thermal hold. No partial path or replacement material is returned on failure.
pub fn evaluate_controlled(input: &ControlledInput) -> Result<ControlledReport, Refusal> {
    if input.schema != "ftx-compressed-thermal-history/3" {
        return Err(invalid("expected ftx-compressed-thermal-history/3"));
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
            "controlled compressed thermal path needs 1..32 nonempty legs, at most 512 total holds and 262144 cell-holds",
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
                    Some(budget) => state.advance_controlled_on_adaptive(hold, change, budget)?,
                    None => state.advance_controlled_on(hold, change)?,
                };
                steps.push(result.step.clone());
                transitions.push(TransitionReport {
                    before_hold: steps.len() - 1,
                    result,
                });
            } else {
                steps.push(match &mut budget {
                    Some(budget) => state.advance_controlled_adaptive(hold, budget)?,
                    None => state.advance_controlled(hold)?,
                });
            }
        }
    }
    Ok(ControlledReport {
        schema: "ftx-compressed-thermal-history-report/3",
        input: input.clone(),
        input_sha256: sha256_hex(&serde_json::to_vec(input).map_err(|e| invalid(e.to_string()))?),
        initial_compression,
        steps,
        transitions,
        final_compression: state.compression().clone(),
        no_claim: "Explicit quasi-static uniform pressure or rigid-face clearance drives source-qualified layer thickness and dry transport at fixed reference area and conserved mass/heat capacity. A change of control does not qualify a branch reversal. Source joins preserve every layer's pressure/thickness and all temperatures/ledgers; pressure-controlled plateaus require no inverse. No inferred garment pressure field, lateral deformation, continuous squeeze, transient recovery, mechanical heat conversion, thermomechanical feedback, moisture, empirical garment warmth or comfort claim.",
    })
}
