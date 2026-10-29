//! Explicit workers attend complete routed operations. Their travel, waiting, loading, setup,
//! feed and unloading never overlap on the same person; unloaded garment handoffs stay independent.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{
    Policy as RoutePolicy, Task, Timeline as RouteTimeline, add, source_valid, window_valid,
};
use crate::sewing::{bad, production::parallel::operator::Transfer, qualified_text};

/// A source-qualified assignment of complete attended operations to explicit physical workers.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `exclusive_full_operation_attendance/1`; feed occupies the worker through full unloading.
    pub model: String,
    /// Full effective cell/recipe/timing locks, including every eligible but unused alternative.
    pub cells: BTreeMap<String, String>,
    /// One to eight physical workers, each owning a nonempty disjoint set of configured cells.
    pub workers: Vec<Worker>,
    /// Evidence for the complete worker assignment, availability and travel protocol.
    pub source: Source,
    /// Complete phase definition; garment handoffs have their own independently qualified resource.
    pub method: String,
    /// Source applicability conditions, without inferred live worker availability.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Evidence class independent of machine, process, handling and fixture evidence.
    pub evidence: Evidence,
    /// Separate explicit consent for estimated labor and travel observations.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// One person whose complete attended operations and travel are mutually exclusive.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worker {
    /// Stable physical identity, distinct from cells, other workers and the garment handoff resource.
    pub id: String,
    /// Exactly the configured cells this person may attend; each cell has one worker in this model.
    pub cells: Vec<String>,
    /// Availability from the common job origin, at most one day.
    pub available_at_milliseconds: u32,
    /// Optional inclusive end, covering travel, waiting, feed and final unloading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_until_milliseconds: Option<u64>,
    /// Explicit travel from the qualified initial location to every owned cell, including zero.
    pub initial_arrival_milliseconds: BTreeMap<String, u32>,
    /// Exactly every directed pair of distinct owned cells; remaining at a cell requires no travel.
    pub transfers: Vec<Transfer>,
}

/// One exact exclusive worker interval, tied to the canonical routed operation phase boundaries.
#[derive(Clone, Debug, Serialize)]
pub struct Step {
    /// Physical attending worker.
    pub worker: String,
    /// Actual eligible cell selected for this operation.
    pub cell: String,
    /// Previous attended cell, absent for travel from the qualified initial location.
    pub from_cell: Option<String>,
    /// Exact global copy/operation identity from the routed timeline.
    pub operation: String,
    /// Physical garment copy.
    pub copy: usize,
    /// Departure after the previous complete unloading or this worker's availability start.
    pub departure_milliseconds: u64,
    /// Arrival after explicitly supplied initial or directed travel.
    pub arrival_milliseconds: u64,
    /// Loading starts after worker, garment, machine and fixture availability; waiting holds labor.
    pub loading_start_milliseconds: u64,
    /// Loading completes and setup begins.
    pub setup_start_milliseconds: u64,
    /// Setup completes and fully attended feed begins.
    pub feed_start_milliseconds: u64,
    /// Feed completes and fully attended unloading begins.
    pub feed_finish_milliseconds: u64,
    /// Full unloading completes and this worker may travel to the next cell.
    pub finish_milliseconds: u64,
}

/// Complete exclusive labor use, in the existing routed dispatch order.
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    /// Every actual operation once; different workers may overlap in physical time.
    pub steps: Vec<Step>,
    /// Sum of source-qualified initial and directed worker travel, distinct from garment handoffs.
    pub travel_milliseconds: u64,
    /// Sum of waiting after arrival and before loading, during which the person remains occupied.
    pub waiting_milliseconds: u64,
    /// Sum of complete loading, setup, feed and unloading occupancy, excluding travel and waiting.
    pub attended_milliseconds: u64,
}

/// Exact source policy and complete retained baseline/selected worker replay.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-attending-labor/1`.
    pub schema: &'static str,
    /// Complete admitted worker identities, cell locks, travel evidence and availability windows.
    pub policy: Policy,
    /// Labor occupancy for the complete baseline, including a searched replacement when necessary.
    pub baseline: Timeline,
    /// Labor occupancy for the selected complete routed execution.
    pub selected: Timeline,
    /// Declared software timing does not establish measured factory labor performance.
    pub physical_labor_validated: bool,
}

#[derive(Clone)]
pub(super) struct State {
    ready: Vec<u64>,
    last: Vec<Option<usize>>,
}

#[derive(Clone, Copy)]
pub(super) struct Arrival {
    worker: usize,
    from: Option<usize>,
    departure: u64,
    pub(super) time: u64,
}

pub(super) struct Compiled {
    policy: Policy,
    cell_ids: Vec<String>,
    cell_worker: Vec<usize>,
    initial: Vec<u64>,
    travel: Vec<Vec<u64>>,
}

impl Compiled {
    pub(super) fn compile(route: &RoutePolicy) -> Result<Option<Self>, Refusal> {
        let Some(policy) = &route.attending_labor else {
            return Ok(None);
        };
        if policy.model != "exclusive_full_operation_attendance/1"
            || policy.workers.is_empty()
            || policy.workers.len() > route.cells.len()
            || policy.cells != route.cell_locks
            || !source_valid(&policy.source, &policy.method, &policy.conditions)
        {
            return Err(bad(
                "routed attending labor needs a qualified full-operation protocol, exact cell locks and one to eight workers",
            ));
        }
        if policy.evidence == Evidence::Estimate && !policy.allow_estimates {
            return Err(bad(
                "estimated routed attending labor needs its own explicit consent",
            ));
        }
        let cells: BTreeMap<_, _> = route
            .cells
            .iter()
            .enumerate()
            .map(|(index, cell)| (cell.id.as_str(), index))
            .collect();
        let mut assigned = BTreeSet::new();
        let mut workers = BTreeSet::new();
        let mut compiled = Self {
            policy: policy.clone(),
            cell_ids: route.cells.iter().map(|cell| cell.id.clone()).collect(),
            cell_worker: vec![0; cells.len()],
            initial: vec![0; cells.len()],
            travel: vec![vec![0; cells.len()]; cells.len()],
        };
        for (index, worker) in policy.workers.iter().enumerate() {
            let owned: BTreeSet<_> = worker.cells.iter().map(String::as_str).collect();
            if !qualified_text(&worker.id)
                || !workers.insert(worker.id.as_str())
                || cells.contains_key(worker.id.as_str())
                || worker.id == route.transfer.id
                || !window_valid(
                    worker.available_at_milliseconds,
                    worker.available_until_milliseconds,
                )
                || owned.is_empty()
                || owned.len() != worker.cells.len()
                || owned.len() > cells.len()
                || worker.initial_arrival_milliseconds.len() != owned.len()
                || worker.transfers.len() != owned.len() * (owned.len() - 1)
            {
                return Err(bad(
                    "invalid routed worker identity, ownership, availability or complete travel domain",
                ));
            }
            for &id in &owned {
                let Some(&cell) = cells.get(id) else {
                    return Err(bad("routed worker names an unknown physical cell"));
                };
                let Some(&arrival) = worker.initial_arrival_milliseconds.get(id) else {
                    return Err(bad("routed worker omits an explicit initial cell arrival"));
                };
                if !assigned.insert(id) || arrival > 86_400_000 {
                    return Err(bad(
                        "routed cells need exactly one worker and bounded initial arrival times",
                    ));
                }
                compiled.cell_worker[cell] = index;
                compiled.initial[cell] = u64::from(arrival);
            }
            let mut seen = BTreeSet::new();
            for transfer in &worker.transfers {
                if transfer.from == transfer.to
                    || !owned.contains(transfer.from.as_str())
                    || !owned.contains(transfer.to.as_str())
                    || transfer.milliseconds > 86_400_000
                    || !seen.insert((&transfer.from, &transfer.to))
                {
                    return Err(bad(
                        "routed labor needs exactly every bounded directed travel between its worker's cells",
                    ));
                }
                compiled.travel[cells[transfer.from.as_str()]][cells[transfer.to.as_str()]] =
                    u64::from(transfer.milliseconds);
            }
        }
        if assigned.len() != cells.len() {
            return Err(bad(
                "routed attending labor must cover every configured cell",
            ));
        }
        Ok(Some(compiled))
    }

    pub(super) fn initial(&self) -> State {
        State {
            ready: self
                .policy
                .workers
                .iter()
                .map(|worker| u64::from(worker.available_at_milliseconds))
                .collect(),
            last: vec![None; self.policy.workers.len()],
        }
    }

    pub(super) fn arrival(&self, state: &State, cell: usize) -> Result<Arrival, Refusal> {
        let worker = self.cell_worker[cell];
        let from = state.last[worker];
        let departure = state.ready[worker];
        let duration = from.map_or(self.initial[cell], |from| self.travel[from][cell]);
        Ok(Arrival {
            worker,
            from,
            departure,
            time: add(departure, duration)?,
        })
    }

    pub(super) fn available(&self, arrival: Arrival, finish: u64) -> bool {
        self.policy.workers[arrival.worker]
            .available_until_milliseconds
            .is_none_or(|end| finish <= end)
    }

    pub(super) fn finish(&self, state: &mut State, arrival: Arrival, cell: usize, finish: u64) {
        state.ready[arrival.worker] = finish;
        state.last[arrival.worker] = Some(cell);
    }

    pub(super) fn lower_bound(
        &self,
        tasks: &[Task],
        choices: &[Vec<usize>],
        copies: usize,
    ) -> Result<u64, Refusal> {
        let mut forced = vec![0; self.policy.workers.len()];
        for domain in choices {
            let worker = self.cell_worker[tasks[domain[0]].cell];
            if domain
                .iter()
                .all(|&task| self.cell_worker[tasks[task].cell] == worker)
            {
                let minimum = domain
                    .iter()
                    .map(|&task| {
                        let task = &tasks[task];
                        add(add(task.load, task.feed)?, task.unload)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .min()
                    .expect("nonempty admitted eligible domain");
                forced[worker] = add(forced[worker], minimum * copies as u64)?;
            }
        }
        let mut lower = 0;
        for (index, work) in forced.into_iter().enumerate().filter(|&(_, work)| work > 0) {
            // Ignore optional work, all setup and between-cell travel. The first cell may be any
            // owned cell, even one not selected; that relaxation cannot overstate required labor.
            let first = self
                .initial
                .iter()
                .enumerate()
                .filter(|&(cell, _)| self.cell_worker[cell] == index)
                .map(|(_, &arrival)| arrival)
                .min()
                .expect("nonempty worker ownership");
            lower = lower.max(add(
                add(
                    u64::from(self.policy.workers[index].available_at_milliseconds),
                    first,
                )?,
                work,
            )?);
        }
        Ok(lower)
    }

    fn timeline(&self, route: &RouteTimeline) -> Result<Timeline, Refusal> {
        let mut state = self.initial();
        let mut out = Timeline {
            steps: Vec::with_capacity(route.operations.len()),
            travel_milliseconds: 0,
            waiting_milliseconds: 0,
            attended_milliseconds: 0,
        };
        for operation in &route.operations {
            let cell = self
                .cell_ids
                .iter()
                .position(|id| *id == operation.cell)
                .ok_or_else(|| bad("routed labor replay names an unknown cell"))?;
            let arrival = self.arrival(&state, cell)?;
            let phases = [
                arrival.time,
                operation.loading_start_milliseconds,
                operation.setup_start_milliseconds,
                operation.feed_start_milliseconds,
                operation.feed_finish_milliseconds,
                operation.finish_milliseconds,
            ];
            if phases.windows(2).any(|pair| pair[0] > pair[1])
                || !self.available(arrival, operation.finish_milliseconds)
            {
                return Err(bad(
                    "routed operation violates exclusive worker occupancy or availability",
                ));
            }
            out.travel_milliseconds =
                add(out.travel_milliseconds, arrival.time - arrival.departure)?;
            out.waiting_milliseconds = add(
                out.waiting_milliseconds,
                operation.loading_start_milliseconds - arrival.time,
            )?;
            out.attended_milliseconds = add(
                out.attended_milliseconds,
                operation.finish_milliseconds - operation.loading_start_milliseconds,
            )?;
            out.steps.push(Step {
                worker: self.policy.workers[arrival.worker].id.clone(),
                cell: operation.cell.clone(),
                from_cell: arrival.from.map(|cell| self.cell_ids[cell].clone()),
                operation: operation.id.clone(),
                copy: operation.copy,
                departure_milliseconds: arrival.departure,
                arrival_milliseconds: arrival.time,
                loading_start_milliseconds: operation.loading_start_milliseconds,
                setup_start_milliseconds: operation.setup_start_milliseconds,
                feed_start_milliseconds: operation.feed_start_milliseconds,
                feed_finish_milliseconds: operation.feed_finish_milliseconds,
                finish_milliseconds: operation.finish_milliseconds,
            });
            self.finish(&mut state, arrival, cell, operation.finish_milliseconds);
        }
        Ok(out)
    }

    pub(super) fn report(
        &self,
        baseline: &RouteTimeline,
        selected: &RouteTimeline,
    ) -> Result<Report, Refusal> {
        Ok(Report {
            schema: "ftx-sewing-attending-labor/1",
            policy: self.policy.clone(),
            baseline: self.timeline(baseline)?,
            selected: self.timeline(selected)?,
            physical_labor_validated: false,
        })
    }
}
