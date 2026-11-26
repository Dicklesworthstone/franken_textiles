//! Individual garment arrivals and authored completion requirements on the existing route clock.
//! Qualified pre-loading waiting stays outside the finite in-process staging boundary.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{MAX_EXACT_MILLISECONDS, Prepared, State, Task, Timeline, add, source_valid};
use crate::sewing::bad;

/// One physical copy's unloaded availability and optional authored completion requirement.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// Existing zero-based physical garment identity, independent of this row's order.
    pub copy: usize,
    /// Inclusive earliest first loading at the selected first cell, in exact milliseconds.
    pub available_at_milliseconds: u64,
    /// Inclusive latest complete final unloading, not a source-measured process duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete_by_milliseconds: Option<u64>,
}

/// Complete declared copy windows with an independently qualified arrival/waiting boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `explicit_copy_release_and_completion/1`.
    pub model: String,
    /// Exactly one window for every resolved physical garment copy, in any authored order.
    pub copies: Vec<Window>,
    /// Evidence for unloaded availability and waiting at every eligible first-cell choice.
    pub source: Source,
    /// The waiting area precedes route entry; inbound transport and its capacity are not inferred.
    pub method: String,
    /// Source applicability, separate from the authored deadlines and machine/process evidence.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Independent classification of arrival/waiting applicability, not a physical deadline claim.
    pub evidence: Evidence,
    /// Separate consent for estimated arrivals or waiting applicability.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Actual first loading and complete final unloading for one exact physical garment copy.
#[derive(Clone, Debug, Serialize)]
pub struct CopyTiming {
    /// Existing zero-based physical garment identity.
    pub copy: usize,
    /// Actual selected first cell, including eligible-cell assignments.
    pub first_cell: String,
    /// Exact authored release at the selected first cell.
    pub available_at_milliseconds: u64,
    /// Actual first loading after garment, resource and qualified phase admission.
    pub loading_start_milliseconds: u64,
    /// Unloaded wait before route entry; does not occupy the in-process staging buffer.
    pub waiting_milliseconds: u64,
    /// Complete final unloading, not feed completion or the batch's latest finish.
    pub finish_milliseconds: u64,
    /// Original inclusive deadline, when authored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complete_by_milliseconds: Option<u64>,
    /// Exact nonnegative time left at final unloading; absent without an authored deadline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_slack_milliseconds: Option<u64>,
}

/// Exact policy and independently checked per-copy boundaries of both complete schedules.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-copy-windows/1`.
    pub schema: &'static str,
    /// Original policy and qualification; authored row order is preserved.
    pub policy: Policy,
    /// Source-qualified waiting precedes first loading and the existing staging entry boundary.
    pub waiting_scope: &'static str,
    /// Retained baseline, in physical copy order.
    pub baseline: Vec<CopyTiming>,
    /// Selected complete schedule, in physical copy order.
    pub selected: Vec<CopyTiming>,
}

impl Policy {
    pub(super) fn validate(&self, copies: usize) -> Result<(), Refusal> {
        if self.model != "explicit_copy_release_and_completion/1"
            || self.copies.len() != copies
            || !source_valid(&self.source, &self.method, &self.conditions)
        {
            return Err(bad(
                "copy windows need a complete copy list and a qualified unloaded arrival/waiting model",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated copy arrivals and waiting need their own explicit consent",
            ));
        }
        let mut seen = BTreeSet::new();
        for window in &self.copies {
            if window.copy >= copies
                || !seen.insert(window.copy)
                || window.available_at_milliseconds > MAX_EXACT_MILLISECONDS
                || window.complete_by_milliseconds.is_some_and(|finish| {
                    finish < window.available_at_milliseconds || finish > MAX_EXACT_MILLISECONDS
                })
            {
                return Err(bad(
                    "copy windows must name every copy exactly once with exact nonnegative millisecond bounds and no deadline before release",
                ));
            }
        }
        Ok(())
    }
}

pub(super) struct Compiled {
    policy: Policy,
    windows: Vec<Window>,
    /// Dense symmetry-class index (`0..copies <= MAX_OPERATIONS`) grouping copies with identical
    /// `(available_at_milliseconds, complete_by_milliseconds)`.
    symmetry_class: Vec<usize>,
    /// Minimum load/feed/unload work still needed from each route position, ignoring all setup,
    /// handoffs, cell availability and contention. Independent stage choices only relax the route.
    minimum_remaining: Vec<u64>,
    /// Mandatory work from each exact next task through final unloading, allowing every eligible
    /// continuation and relaxing all future contention, setup and availability.
    task_remaining: Vec<u64>,
    pub(super) has_deadlines: bool,
}

impl Compiled {
    pub(super) fn new(
        policy: &Policy,
        tasks: &[Task],
        choices: &[Vec<usize>],
        handoffs: &[Vec<Option<u64>>],
    ) -> Result<Self, Refusal> {
        let mut windows = policy.copies.clone();
        windows.sort_by_key(|window| window.copy);
        let mut class_ids = BTreeMap::new();
        let symmetry_class = windows
            .iter()
            .map(|window| {
                let next_id = class_ids.len();
                *class_ids
                    .entry((
                        window.available_at_milliseconds,
                        window.complete_by_milliseconds,
                    ))
                    .or_insert(next_id)
            })
            .collect();
        let mut minimum_remaining = vec![0; choices.len() + 1];
        for route in (0..choices.len()).rev() {
            let minimum = choices[route]
                .iter()
                .map(|&index| {
                    let task = &tasks[index];
                    add(add(task.load, task.feed)?, task.unload)
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .min()
                .expect("admitted nonempty route domain");
            minimum_remaining[route] = add(minimum, minimum_remaining[route + 1])?;
        }
        let mut task_remaining = vec![0; tasks.len()];
        for (route, alternatives) in choices.iter().enumerate().rev() {
            for &index in alternatives {
                let task = &tasks[index];
                let continuation = choices.get(route + 1).map_or(0, |next| {
                    next.iter()
                        .map(|&to| {
                            handoffs[task.cell][tasks[to].cell].unwrap_or(0) + task_remaining[to]
                        })
                        .min()
                        .expect("admitted nonempty route domain")
                });
                // Even a full 256-stage route with MAX_EXACT_MILLISECONDS in each feed fits u64.
                // An individually too-long alternative must not invalidate a shorter eligible one.
                task_remaining[index] = task.load + task.feed + task.unload + continuation;
            }
        }
        let has_deadlines = windows
            .iter()
            .any(|window| window.complete_by_milliseconds.is_some());
        Ok(Self {
            policy: policy.clone(),
            windows,
            symmetry_class,
            minimum_remaining,
            task_remaining,
            has_deadlines,
        })
    }

    pub(super) fn symmetry_class(&self, copy: usize) -> usize {
        self.symmetry_class[copy]
    }

    /// Check *every* unfinished garment against the current append-only cell clocks. A newly
    /// dispatched operation can strand a different copy on the same bottleneck. The smallest
    /// nonnegative slack goes first in search; equal slacks retain the physical copy ID order.
    pub(super) fn by_urgency(
        &self,
        prepared: &Prepared<'_>,
        state: &State,
    ) -> Option<Vec<(u64, usize)>> {
        let mut order = Vec::with_capacity(self.windows.len());
        for window in &self.windows {
            let copy = window.copy;
            let Some(choices) = prepared.choices.get(state.next[copy]) else {
                continue;
            };
            let slack = if let Some(deadline) = window.complete_by_milliseconds {
                let earliest_finish = choices
                    .iter()
                    .map(|&index| {
                        let cell = prepared.tasks[index].cell;
                        let handoff = state.copy_cell[copy]
                            .and_then(|from| prepared.handoffs[from][cell])
                            .unwrap_or(0);
                        (state.copy_ready[copy] + handoff).max(state.cell_ready[cell])
                            + self.task_remaining[index]
                    })
                    .min()
                    .expect("admitted nonempty route domain");
                deadline.checked_sub(earliest_finish)?
            } else {
                u64::MAX
            };
            order.push((slack, copy));
        }
        order.sort_unstable();
        Some(order)
    }

    pub(super) fn initial_ready(&self) -> Vec<u64> {
        self.windows
            .iter()
            .map(|window| window.available_at_milliseconds)
            .collect()
    }

    pub(super) fn allows(&self, copy: usize, next_route: usize, ready: u64) -> bool {
        self.windows[copy]
            .complete_by_milliseconds
            .is_none_or(|deadline| {
                deadline
                    .checked_sub(ready)
                    .is_some_and(|left| self.minimum_remaining[next_route] <= left)
            })
    }

    pub(super) fn lower_bound(&self) -> Result<u64, Refusal> {
        let mut lower = 0;
        for window in &self.windows {
            if !self.allows(window.copy, 0, window.available_at_milliseconds) {
                return Err(Refusal::new(
                    RefusalCode::NotConverged,
                    format!(
                        "copy {} cannot complete its mandatory loading, feed and unloading between the declared release and deadline",
                        window.copy
                    ),
                ));
            }
            lower = lower.max(add(
                window.available_at_milliseconds,
                self.minimum_remaining[0],
            )?);
        }
        Ok(lower)
    }

    fn timings(&self, timeline: &Timeline) -> Result<Vec<CopyTiming>, Refusal> {
        self.windows
            .iter()
            .map(|window| {
                let first = timeline
                    .operations
                    .iter()
                    .find(|operation| operation.copy == window.copy && operation.route_index == 0)
                    .ok_or_else(|| bad("copy-window replay omits first loading"))?;
                let last = timeline
                    .operations
                    .iter()
                    .find(|operation| {
                        operation.copy == window.copy
                            && operation.route_index + 2 == self.minimum_remaining.len()
                    })
                    .ok_or_else(|| bad("copy-window replay omits final unloading"))?;
                if first.loading_start_milliseconds < window.available_at_milliseconds
                    || last.finish_milliseconds < first.loading_start_milliseconds
                    || window
                        .complete_by_milliseconds
                        .is_some_and(|deadline| last.finish_milliseconds > deadline)
                {
                    return Err(bad(
                        "copy-window replay violates a garment release or completion deadline",
                    ));
                }
                Ok(CopyTiming {
                    copy: window.copy,
                    first_cell: first.cell.clone(),
                    available_at_milliseconds: window.available_at_milliseconds,
                    loading_start_milliseconds: first.loading_start_milliseconds,
                    waiting_milliseconds: first.loading_start_milliseconds
                        - window.available_at_milliseconds,
                    finish_milliseconds: last.finish_milliseconds,
                    complete_by_milliseconds: window.complete_by_milliseconds,
                    deadline_slack_milliseconds: window
                        .complete_by_milliseconds
                        .map(|deadline| deadline - last.finish_milliseconds),
                })
            })
            .collect()
    }

    pub(super) fn report(
        &self,
        baseline: &Timeline,
        selected: &Timeline,
    ) -> Result<Report, Refusal> {
        Ok(Report {
            schema: "ftx-sewing-copy-windows/1",
            policy: self.policy.clone(),
            waiting_scope: "qualified_unloaded_waiting_at_selected_first_cell_before_loading/1",
            baseline: self.timings(baseline)?,
            selected: self.timings(selected)?,
        })
    }
}
