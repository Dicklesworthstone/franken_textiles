//! Source-qualified finite operating windows for routed cells, workers and unloaded handoffs.
//! A window contains a whole occupied interval; inactive gaps never imply preemption or travel.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{MAX_EXACT_MILLISECONDS, Policy as RoutePolicy, add, source_valid};
use crate::sewing::bad;

pub(super) const MAX_WINDOWS: usize = 16;

/// One explicit finite availability interval measured from the common production-job origin.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// Inclusive earliest occupied instant, in exact integer milliseconds.
    pub start_milliseconds: u64,
    /// Inclusive latest finish, at most the largest exactly represented browser integer.
    pub end_milliseconds: u64,
}

/// Independently qualified nonpreemptive availability inside the existing outer resource windows.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `nonpreemptive_resource_windows/1`.
    pub model: String,
    /// Exact physical cell, attending-worker or transfer IDs. Each named resource has 1..16 sorted,
    /// strictly separated windows. Unnamed resources retain their existing contiguous availability.
    pub resources: BTreeMap<String, Vec<Window>>,
    /// Evidence for these windows and their resource-specific applicability.
    pub source: Source,
    /// Qualification method; no live factory calendar or resource reservation is inferred.
    pub method: String,
    /// Source conditions retained without extrapolation.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Evidence class independent of machine, process, handling and labor qualifications.
    pub evidence: Evidence,
    /// Separate consent for an estimated calendar.
    #[serde(default)]
    pub allow_estimates: bool,
}

impl Policy {
    pub(super) fn validate(&self, route: &RoutePolicy) -> Result<(), Refusal> {
        if self.model != "nonpreemptive_resource_windows/1"
            || self.resources.is_empty()
            || self.resources.len() > 17
            || !source_valid(&self.source, &self.method, &self.conditions)
        {
            return Err(bad(
                "routed calendars need a qualified nonpreemptive model and named resources",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated routed calendars need their own explicit consent",
            ));
        }
        let mut bounds: BTreeMap<_, _> = route
            .cells
            .iter()
            .map(|cell| {
                (
                    cell.id.as_str(),
                    (
                        u64::from(cell.available_at_milliseconds),
                        cell.available_until_milliseconds,
                    ),
                )
            })
            .collect();
        bounds.insert(
            route.transfer.id.as_str(),
            (
                u64::from(route.transfer.available_at_milliseconds),
                route.transfer.available_until_milliseconds,
            ),
        );
        for worker in route
            .attending_labor
            .iter()
            .flat_map(|labor| &labor.workers)
        {
            if bounds
                .insert(
                    worker.id.as_str(),
                    (
                        u64::from(worker.available_at_milliseconds),
                        worker.available_until_milliseconds,
                    ),
                )
                .is_some()
            {
                return Err(bad(
                    "routed calendar resources need distinct physical identities",
                ));
            }
        }
        for (id, windows) in &self.resources {
            let Some(&(start, end)) = bounds.get(id.as_str()) else {
                return Err(bad("routed calendar names an unknown physical resource"));
            };
            if windows.is_empty()
                || windows.len() > MAX_WINDOWS
                || windows.iter().any(|window| {
                    window.start_milliseconds < start
                        || window.start_milliseconds > window.end_milliseconds
                        || window.end_milliseconds > end.unwrap_or(MAX_EXACT_MILLISECONDS)
                        || window.end_milliseconds > MAX_EXACT_MILLISECONDS
                })
                || windows
                    .windows(2)
                    .any(|pair| pair[0].end_milliseconds >= pair[1].start_milliseconds)
            {
                return Err(bad(
                    "routed calendars need 1..16 sorted, disjoint finite windows inside each resource's outer availability",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn windows(&self, id: &str) -> Option<&[Window]> {
        self.resources.get(id).map(Vec::as_slice)
    }

    pub(super) fn earliest_start(
        &self,
        id: &str,
        earliest: u64,
        duration: u64,
    ) -> Result<Option<u64>, Refusal> {
        let Some(windows) = self.windows(id) else {
            return Ok(Some(earliest));
        };
        for window in windows {
            let start = earliest.max(window.start_milliseconds);
            // Subtraction avoids overflowing exact milliseconds merely while rejecting a window.
            if start <= window.end_milliseconds && duration <= window.end_milliseconds - start {
                add(start, duration)?;
                return Ok(Some(start));
            }
        }
        Ok(None)
    }
}
