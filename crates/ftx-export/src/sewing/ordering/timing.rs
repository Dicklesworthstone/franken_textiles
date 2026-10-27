//! Exact, directed setup durations for the existing bounded scheduling search. Integer milliseconds
//! keep pruning and tie comparisons exact; no measured time is inferred from setup-change counts.
use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::Setup;
use crate::sewing::{bad, qualified_text};

const MAX_TRANSITIONS: usize = 4096;
const MAX_MILLISECONDS: u32 = 86_400_000;
type Durations<'a> = BTreeMap<(&'a Setup, &'a Setup), u64>;

fn duration(rows: &Durations<'_>, from: &Setup, to: &Setup) -> Result<u64, Refusal> {
    if from == to {
        Ok(0)
    } else {
        rows.get(&(from, to)).copied().ok_or_else(|| {
            bad(format!(
                "missing directed setup duration: {}/{} -> {}/{}",
                from.needle, from.thread, to.needle, to.thread
            ))
        })
    }
}

/// One directed, complete needle/thread change. Combined changes are not added independently.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    /// Exact setup before the change.
    pub from: Setup,
    /// Exact setup after the change; must differ from `from`.
    pub to: Setup,
    /// Declared duration in integer milliseconds, at most one day. Zero must be explicit.
    pub milliseconds: u32,
}

/// Source-bearing time model for one exact machine configuration and stated operating conditions.
/// All rows share this acquisition/protocol and evidence classification; heterogeneous evidence
/// must be conservatively classified or reviewed into a common source-qualified table.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupTimes {
    /// Exact SewingMachine.id; no fuzzy matching to a model family.
    pub machine: String,
    /// Acquisition or retained local evidence covering the whole table.
    pub source: Source,
    /// Timing/reduction protocol, including what work is and is not counted.
    pub method: String,
    /// Applicability conditions; preserved, not inferred or matched to physical machine sensors.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Origin of the declared durations. No confidence interval or worst-case guarantee is inferred.
    pub evidence: Evidence,
    /// Separate explicit admission for estimated timings, not material/process permission.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Directed changes. No-change transitions cost zero; other missing rows are never zero.
    pub transitions: Vec<Transition>,
}

impl SetupTimes {
    fn rows(&self) -> Durations<'_> {
        self.transitions
            .iter()
            .map(|row| ((&row.from, &row.to), u64::from(row.milliseconds)))
            .collect()
    }

    pub(super) fn validate(&self, machine: &str) -> Result<(), Refusal> {
        if self.machine != machine
            || !qualified_text(&self.machine)
            || !qualified_text(&self.source.reference)
            || !qualified_text(&self.source.accessed_on)
            || !qualified_text(&self.source.note)
            || !qualified_text(&self.method)
            || self.conditions.len() > 64
            || self
                .conditions
                .iter()
                .any(|(k, v)| !qualified_text(k) || !qualified_text(v))
            || self.transitions.len() > MAX_TRANSITIONS
        {
            return Err(bad(
                "invalid source-qualified setup timing table or machine mismatch",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated setup durations need their own explicit consent",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for row in &self.transitions {
            if !row.from.valid()
                || !row.to.valid()
                || row.from == row.to
                || row.milliseconds > MAX_MILLISECONDS
                || !seen.insert((&row.from, &row.to))
            {
                return Err(bad("invalid, duplicate or no-change setup timing row"));
            }
        }
        Ok(())
    }

    /// Preflight a recipe search's entire timing domain. Missing evidence must not be mistaken for
    /// an infeasible recipe and thereby produce a false optimum among only the known timings.
    pub(crate) fn check_choices(
        &self,
        machine: &str,
        choices: &BTreeSet<Setup>,
        initial: &Setup,
    ) -> Result<(), Refusal> {
        if choices.is_empty() || choices.len() > 64 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "timed recipe selection supports 1..64 distinct candidate setups",
            ));
        }
        self.validate(machine)?;
        if !initial.valid() || choices.iter().any(|s| !s.valid()) {
            return Err(bad("invalid recipe or initial setup in timing domain"));
        }
        let rows = self.rows();
        for from in choices.iter().chain(std::iter::once(initial)) {
            for to in choices {
                duration(&rows, from, to)?;
            }
        }
        Ok(())
    }

    /// Costs between the active jobs and from the known initial setup (last row). Unused timing
    /// rows remain legal so a recipe search can reuse one evidence table across all its candidates.
    pub(super) fn matrix(
        &self,
        setups: &[Setup],
        initial: &Setup,
    ) -> Result<Vec<Vec<u64>>, Refusal> {
        let rows = self.rows();
        setups
            .iter()
            .chain(std::iter::once(initial))
            .map(|from| setups.iter().map(|to| duration(&rows, from, to)).collect())
            .collect()
    }
}

/// Timed objective diagnostics alongside, never substituted for, the existing setup-change counts.
#[derive(Clone, Debug, Serialize)]
pub struct TimingReport {
    /// Duration of the admitted baseline under the same source table. Integer milliseconds use u64
    /// so complete production batches remain exact on wasm32 as well as native targets.
    pub baseline_setup_milliseconds: u64,
    /// Duration of the selected complete schedule, including initial setup and excluding final reset.
    pub selected_setup_milliseconds: u64,
    /// Proved lower bound for the setup objective; equals the selected value only when proved.
    pub lower_bound_setup_milliseconds: u64,
    /// Generated feed duration plus selected setup duration. Not total manufacturing time.
    pub feed_and_setup_seconds: f64,
}

/// Source-qualified serial handling added to actual feed and setup, not a factory throughput claim.
#[derive(Clone, Debug, Serialize)]
pub struct HandlingTimingReport {
    /// Initial, between-operation and final handling in the complete baseline.
    pub baseline_handling_milliseconds: u64,
    /// The same handling scope in the selected complete schedule.
    pub selected_handling_milliseconds: u64,
    /// Baseline feed plus setup plus handling, without overlap or undeclared phases.
    pub baseline_modeled_production_seconds: f64,
    /// Selected feed plus setup plus handling, without overlap or undeclared phases.
    pub selected_modeled_production_seconds: f64,
    /// Valid bound on the combined integer objective; feed is a fixed independent addition.
    pub lower_bound_setup_and_handling_milliseconds: u64,
}
