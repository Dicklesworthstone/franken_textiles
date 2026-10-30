//! Completion constraints consume the shared production planner's complete timing and bounds.
//! A feasible incumbent needs no time-optimality proof; an unproved missed deadline stays unknown.

use serde::Serialize;

use crate::sewing::SewPlan;

/// Largest exactly representable millisecond deadline on native and browser JSON boundaries.
pub(super) const MAX_MILLISECONDS: u64 = 9_007_199_254_740_991;

/// Status of one complete recipe/route trial against its inclusive completion limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The retained complete schedule finishes on or before the deadline.
    Feasible,
    /// A valid lower bound or complete timing proof places every admitted schedule after it.
    Excluded,
    /// No complete timing exists, or an incumbent misses without a proof ruling out improvement.
    Unresolved,
}

/// Canonical complete timing authority; continuous feed alone never represents full completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Basis {
    /// Shared serial feed, setup and handling accounting, including both batch endpoints.
    #[serde(rename = "serial_feed_setup_handling/1")]
    SerialFeedSetupHandling,
    /// Whole-garment parallel allocation's exact integer completion clock.
    #[serde(rename = "parallel_makespan/1")]
    ParallelMakespan,
    /// Complete routed specialist schedule's exact integer completion clock.
    #[serde(rename = "routed_makespan/1")]
    RoutedMakespan,
}

/// Source-preserving deadline decision, separate from the original recipe selection objective.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-completion-deadline/1`.
    pub schema: &'static str,
    /// Authored inclusive elapsed completion limit, without a hidden rounding or tolerance policy.
    pub limit_milliseconds: u64,
    /// Feasible incumbent, proved exclusion, or unresolved feasibility.
    pub status: Status,
    /// Actual shared timing authority, absent when complete production timing is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<Basis>,
    /// Canonical complete incumbent time, not the search's original cost or feed objective.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_completion_seconds: Option<f64>,
    /// Shared valid completion lower bound, including fixed feed in serial mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lower_bound_completion_seconds: Option<f64>,
    /// Exact routed/parallel makespan used for the integer deadline comparison.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_makespan_milliseconds: Option<u64>,
    /// Exact routed/parallel lower bound used for the integer deadline comparison.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lower_bound_makespan_milliseconds: Option<u64>,
    /// Whether the timing authority proved its selected complete schedule minimal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing_optimality_proven: Option<bool>,
    /// Concrete reason for admission, exclusion or unresolved feasibility; no empirical guarantee.
    pub reason: &'static str,
}

impl Report {
    pub(super) fn unavailable(limit_milliseconds: u64) -> Self {
        Self {
            schema: "ftx-sewing-completion-deadline/1",
            limit_milliseconds,
            status: Status::Unresolved,
            basis: None,
            selected_completion_seconds: None,
            lower_bound_completion_seconds: None,
            selected_makespan_milliseconds: None,
            lower_bound_makespan_milliseconds: None,
            timing_optimality_proven: None,
            reason: "no admitted complete production timing; inspect the trial refusal and retained source timing policy",
        }
    }

    fn decide(&mut self, meets: bool, bound_excludes: bool, optimal: bool) {
        self.timing_optimality_proven = Some(optimal);
        (self.status, self.reason) = if meets {
            (
                Status::Feasible,
                "the complete retained schedule finishes on or before the inclusive deadline",
            )
        } else if bound_excludes {
            (
                Status::Excluded,
                "the canonical completion lower bound exceeds the inclusive deadline",
            )
        } else if optimal {
            (
                Status::Excluded,
                "the proved minimum complete schedule finishes after the inclusive deadline",
            )
        } else {
            (
                Status::Unresolved,
                "the complete incumbent misses the deadline, but its lower bound and timing proof do not rule out a feasible schedule",
            )
        };
    }

    fn makespan(&mut self, basis: Basis, selected: u64, lower: u64, optimal: bool) {
        self.basis = Some(basis);
        self.selected_completion_seconds = Some(selected as f64 / 1000.0);
        self.lower_bound_completion_seconds = Some(lower as f64 / 1000.0);
        self.selected_makespan_milliseconds = Some(selected);
        self.lower_bound_makespan_milliseconds = Some(lower);
        self.decide(
            selected <= self.limit_milliseconds,
            lower > self.limit_milliseconds,
            optimal,
        );
    }

    pub(super) fn evaluate(plan: &SewPlan, limit_milliseconds: u64) -> Self {
        let mut report = Self::unavailable(limit_milliseconds);
        if let Some(route) = plan
            .production
            .as_ref()
            .and_then(|production| production.routing.as_ref())
        {
            report.makespan(
                Basis::RoutedMakespan,
                route.selected.makespan_milliseconds,
                route.lower_bound_makespan_milliseconds,
                route.optimality_proven,
            );
        } else if let Some(parallel) = plan
            .production
            .as_ref()
            .and_then(|production| production.parallel.as_ref())
        {
            report.makespan(
                Basis::ParallelMakespan,
                parallel.selected.makespan_milliseconds,
                parallel.lower_bound_makespan_milliseconds,
                parallel.optimality_proven,
            );
        } else if let Some((ordering, timing)) = plan.ordering_report().and_then(|ordering| {
            ordering
                .handling_timing
                .as_ref()
                .map(|timing| (ordering, timing))
        }) {
            // Preserve the existing serial seconds authority. Do not apply the distinct routed/
            // parallel feed-ceiling clock or round a fractional serial completion into feasibility.
            let selected = timing.selected_modeled_production_seconds;
            let lower = plan.total_feed_seconds()
                + timing.lower_bound_setup_and_handling_milliseconds as f64 / 1000.0;
            if !selected.is_finite() || !lower.is_finite() {
                report.reason = "canonical complete serial timing is not finitely representable";
                return report;
            }
            report.basis = Some(Basis::SerialFeedSetupHandling);
            report.selected_completion_seconds = Some(selected);
            report.lower_bound_completion_seconds = Some(lower);
            let deadline_seconds = limit_milliseconds as f64 / 1000.0;
            report.decide(
                selected <= deadline_seconds,
                lower > deadline_seconds,
                ordering.optimality_proven,
            );
        } else {
            report.reason = "complete source-qualified handling and setup timing is absent; feed or feed-plus-setup seconds alone cannot establish production completion";
        }
        report
    }
}
