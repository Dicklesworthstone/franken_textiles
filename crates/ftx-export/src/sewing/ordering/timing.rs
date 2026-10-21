//! Exact, directed setup durations for the existing bounded scheduling search. Integer milliseconds
//! keep pruning and tie comparisons exact; no measured time is inferred from setup-change counts.
use std::collections::BTreeMap;

use ftx_core::Refusal;
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::Setup;
use crate::sewing::{SewingPass, bad, qualified_text};

const MAX_TRANSITIONS: usize = 4096;
const MAX_MILLISECONDS: u32 = 86_400_000;

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

    /// Costs between the active jobs and from the known initial setup (last row). Unused timing
    /// rows remain legal so a recipe search can reuse one evidence table across all its candidates.
    pub(super) fn matrix(
        &self,
        passes: &[&SewingPass],
        initial: &Setup,
    ) -> Result<Vec<Vec<usize>>, Refusal> {
        let rows: BTreeMap<_, _> = self
            .transitions
            .iter()
            .map(|row| ((&row.from, &row.to), row.milliseconds as usize))
            .collect();
        let mut setups: Vec<_> = passes
            .iter()
            .map(|p| Setup {
                needle: p.needle.clone(),
                thread: p.thread.clone(),
            })
            .collect();
        setups.push(initial.clone());
        setups
            .iter()
            .map(|from| {
                setups[..passes.len()]
                    .iter()
                    .map(|to| {
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
                    })
                    .collect()
            })
            .collect()
    }
}

/// Timed objective diagnostics alongside, never substituted for, the existing setup-change counts.
#[derive(Clone, Debug, Serialize)]
pub struct TimingReport {
    /// Duration of the ordinary lexicographic topological schedule under the same source table.
    pub baseline_setup_milliseconds: usize,
    /// Duration of the selected complete schedule, including initial setup and excluding final reset.
    pub selected_setup_milliseconds: usize,
    /// Proved lower bound for the setup objective; equals the selected value only when proved.
    pub lower_bound_setup_milliseconds: usize,
    /// Generated feed duration plus selected setup duration. Not total manufacturing time.
    pub feed_and_setup_seconds: f64,
}
