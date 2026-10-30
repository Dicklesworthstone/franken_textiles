//! Explicit workers attend complete routed operations. Their travel, waiting, loading, setup,
//! feed and unloading never overlap on the same person, including explicitly owned garment handoffs.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{
    Policy as RoutePolicy, Task, Timeline as RouteTimeline, TransferPolicy, add, source_valid,
    window_valid,
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
    /// Complete phase definition, including any explicit ownership of the garment handoff resource.
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
    /// Optional inclusive end, covering travel, waiting, owned handoffs and complete machine attendance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_until_milliseconds: Option<u64>,
    /// Explicit travel to every owned cell, or every configured cell for the transfer owner.
    pub initial_arrival_milliseconds: BTreeMap<String, u32>,
    /// Every directed pair in that travel domain; remaining at a cell requires no travel.
    pub transfers: Vec<Transfer>,
}

/// One exact exclusive worker interval, tied to the canonical routed operation phase boundaries.
#[derive(Clone, Debug, Serialize)]
pub struct Step {
    /// Physical attending worker.
    pub worker: String,
    /// Actual eligible cell selected for this operation.
    pub cell: String,
    /// Previous worker location after an attended operation or an explicitly owned handoff.
    /// Absent for travel from the qualified initial location.
    pub from_cell: Option<String>,
    /// Exact global copy/operation identity from the routed timeline.
    pub operation: String,
    /// Physical garment copy.
    pub copy: usize,
    /// Departure after the previous complete operation/handoff or worker availability start.
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

/// One worker's empty approach, waiting and complete source-to-destination garment handoff.
#[derive(Clone, Debug, Serialize)]
pub struct TransferStep {
    /// Explicitly assigned attending worker and separately identified handoff resource.
    pub worker: String,
    /// Exact qualified resource identity from the retained transfer policy.
    pub resource: String,
    /// Physical garment copy and its exact global operation identities.
    pub copy: usize,
    /// Completed global copy/operation at the source.
    pub from_operation: String,
    /// Next global copy/operation at the destination.
    pub to_operation: String,
    /// Previous worker location, absent for travel from the qualified initial location.
    pub from_cell: Option<String>,
    /// Cell where the unloaded garment is collected.
    pub source_cell: String,
    /// Worker and garment both finish at this destination after deposit.
    pub destination_cell: String,
    /// Beginning of exclusive empty approach after the worker's prior activity or shift start.
    pub departure_milliseconds: u64,
    /// Arrival at the source after separately qualified empty travel.
    pub arrival_milliseconds: u64,
    /// Pickup starts after arrival, source unloading and handoff-resource availability.
    pub start_milliseconds: u64,
    /// Complete deposit; no return or second source-to-destination travel is added.
    pub finish_milliseconds: u64,
}

/// Complete exclusive labor use, in the existing routed dispatch order.
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    /// Every actual operation once; different workers may overlap in physical time.
    pub steps: Vec<Step>,
    /// Actual worker-owned handoffs in their dispatch order; absent for an independent resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfers: Option<Vec<TransferStep>>,
    /// Sum of source-qualified initial and directed worker travel, distinct from garment handoffs.
    pub travel_milliseconds: u64,
    /// Sum of waiting after arrival and before loading or pickup; the person remains occupied.
    pub waiting_milliseconds: u64,
    /// Sum of complete loading, setup, feed and unloading occupancy, excluding travel and waiting.
    pub attended_milliseconds: u64,
    /// Complete pickup/carry/deposit occupancy, excluding the separately counted empty approach.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_milliseconds: Option<u64>,
}

/// Exact source policy and complete retained baseline/selected worker replay.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `/1` independent handoffs; `/2` explicit worker-owned handoffs and their exact replay.
    pub schema: &'static str,
    /// Complete admitted worker identities, cell locks, travel evidence and availability windows.
    pub policy: Policy,
    /// Exact source-qualified ownership, phase law, directed durations and resource windows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer: Option<TransferPolicy>,
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
    transfer: Option<TransferPolicy>,
    transfer_worker: Option<usize>,
    initial: Vec<Vec<u64>>,
    travel: Vec<Vec<Vec<u64>>>,
}

impl Compiled {
    pub(super) fn compile(route: &RoutePolicy) -> Result<Option<Self>, Refusal> {
        let Some(policy) = &route.attending_labor else {
            if route.transfer.attending_worker.is_some() {
                return Err(bad("handoff ownership requires explicit attending labor"));
            }
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
        let transfer_worker = route
            .transfer
            .attending_worker
            .as_ref()
            .map(|id| {
                policy
                    .workers
                    .iter()
                    .position(|worker| worker.id == *id)
                    .ok_or_else(|| bad("handoff ownership names an unknown attending worker"))
            })
            .transpose()?;
        let mut compiled = Self {
            policy: policy.clone(),
            cell_ids: route.cells.iter().map(|cell| cell.id.clone()).collect(),
            cell_worker: vec![0; cells.len()],
            transfer: transfer_worker.map(|_| route.transfer.clone()),
            transfer_worker,
            initial: vec![vec![0; cells.len()]; policy.workers.len()],
            travel: vec![vec![vec![0; cells.len()]; cells.len()]; policy.workers.len()],
        };
        for (index, worker) in policy.workers.iter().enumerate() {
            let owned: BTreeSet<_> = worker.cells.iter().map(String::as_str).collect();
            let visited = if transfer_worker == Some(index) {
                cells.keys().copied().collect::<BTreeSet<_>>()
            } else {
                owned.clone()
            };
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
                || worker.initial_arrival_milliseconds.len() != visited.len()
                || worker.transfers.len() != visited.len() * visited.len().saturating_sub(1)
            {
                return Err(bad(
                    "invalid routed worker identity, ownership, availability or complete travel domain",
                ));
            }
            for &id in &owned {
                let Some(&cell) = cells.get(id) else {
                    return Err(bad("routed worker names an unknown physical cell"));
                };
                if !assigned.insert(id) {
                    return Err(bad("routed cells need exactly one attending worker"));
                }
                compiled.cell_worker[cell] = index;
            }
            for &id in &visited {
                let Some(&arrival) = worker.initial_arrival_milliseconds.get(id) else {
                    return Err(bad("routed worker omits an explicit initial cell arrival"));
                };
                if arrival > 86_400_000 {
                    return Err(bad("routed initial worker travel exceeds one day"));
                }
                compiled.initial[index][cells[id]] = u64::from(arrival);
            }
            let mut seen = BTreeSet::new();
            for transfer in &worker.transfers {
                if transfer.from == transfer.to
                    || !visited.contains(transfer.from.as_str())
                    || !visited.contains(transfer.to.as_str())
                    || transfer.milliseconds > 86_400_000
                    || !seen.insert((&transfer.from, &transfer.to))
                {
                    return Err(bad(
                        "routed labor needs every bounded directed travel in its worker's complete visit domain",
                    ));
                }
                compiled.travel[index][cells[transfer.from.as_str()]]
                    [cells[transfer.to.as_str()]] = u64::from(transfer.milliseconds);
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
        self.worker_arrival(state, self.cell_worker[cell], cell)
    }

    pub(super) fn transfer_arrival(
        &self,
        state: &State,
        source: usize,
    ) -> Result<Option<Arrival>, Refusal> {
        self.transfer_worker
            .map(|worker| self.worker_arrival(state, worker, source))
            .transpose()
    }

    fn worker_arrival(
        &self,
        state: &State,
        worker: usize,
        cell: usize,
    ) -> Result<Arrival, Refusal> {
        let from = state.last[worker];
        let departure = state.ready[worker];
        let duration = from.map_or(self.initial[worker][cell], |from| {
            self.travel[worker][from][cell]
        });
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
        handoffs: &[Vec<Option<u64>>],
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
        if let Some(worker) = self.transfer_worker {
            for pair in choices.windows(2) {
                // Independent adjacent minima may not form a feasible route. They only relax
                // mandatory carrying work; same-cell alternatives contribute zero.
                let minimum = pair[0]
                    .iter()
                    .flat_map(|&from| {
                        pair[1]
                            .iter()
                            .map(move |&to| handoffs[tasks[from].cell][tasks[to].cell].unwrap_or(0))
                    })
                    .min()
                    .expect("nonempty adjacent route domains");
                forced[worker] = add(forced[worker], minimum * copies as u64)?;
            }
        }
        let mut lower = 0;
        for (index, work) in forced.into_iter().enumerate().filter(|&(_, work)| work > 0) {
            // Ignore optional work, all setup and between-cell travel. The first cell may be any
            // visited cell, even one not selected. A transfer owner can begin at an unowned
            // source; considering only their machine cells could overstate required travel.
            let first = self.initial[index]
                .iter()
                .enumerate()
                .filter(|&(cell, _)| {
                    self.transfer_worker == Some(index) || self.cell_worker[cell] == index
                })
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
            transfers: self.transfer.as_ref().map(|_| Vec::new()),
            travel_milliseconds: 0,
            waiting_milliseconds: 0,
            attended_milliseconds: 0,
            transfer_milliseconds: self.transfer.as_ref().map(|_| 0),
        };
        let mut transfers = route.transfers.iter().peekable();
        let mut transfer_ready = self
            .transfer
            .as_ref()
            .map_or(0, |policy| u64::from(policy.available_at_milliseconds));
        for operation in &route.operations {
            let cell = self
                .cell_ids
                .iter()
                .position(|id| *id == operation.cell)
                .ok_or_else(|| bad("routed labor replay names an unknown cell"))?;
            let transfer = if transfers.peek().is_some_and(|transfer| {
                transfer.copy == operation.copy && transfer.to_operation == operation.operation_id
            }) {
                transfers.next()
            } else {
                None
            };
            if let (Some(policy), Some(transfer)) = (&self.transfer, transfer) {
                let source = self
                    .cell_ids
                    .iter()
                    .position(|id| *id == transfer.from)
                    .ok_or_else(|| bad("worker handoff names an unknown source cell"))?;
                let previous = route
                    .operations
                    .iter()
                    .find(|previous| {
                        previous.copy == operation.copy
                            && previous.operation_id == transfer.from_operation
                    })
                    .ok_or_else(|| bad("worker handoff omits its completed source operation"))?;
                let duration = policy
                    .transfers
                    .iter()
                    .find(|row| row.from == transfer.from && row.to == transfer.to)
                    .ok_or_else(|| bad("worker handoff omits its qualified carrying duration"))?;
                let arrival = self
                    .transfer_arrival(&state, source)?
                    .expect("explicit handoff owner");
                if transfer.from != previous.cell
                    || transfer.to != operation.cell
                    || transfer.start_milliseconds
                        < arrival
                            .time
                            .max(previous.finish_milliseconds)
                            .max(transfer_ready)
                    || transfer.finish_milliseconds
                        != add(
                            transfer.start_milliseconds,
                            u64::from(duration.milliseconds),
                        )?
                    || operation.loading_start_milliseconds < transfer.finish_milliseconds
                    || !self.available(arrival, transfer.finish_milliseconds)
                    || policy
                        .available_until_milliseconds
                        .is_some_and(|end| transfer.finish_milliseconds > end)
                {
                    return Err(bad(
                        "worker handoff violates source, exclusive occupancy or availability",
                    ));
                }
                out.travel_milliseconds =
                    add(out.travel_milliseconds, arrival.time - arrival.departure)?;
                out.waiting_milliseconds = add(
                    out.waiting_milliseconds,
                    transfer.start_milliseconds - arrival.time,
                )?;
                let total = out
                    .transfer_milliseconds
                    .as_mut()
                    .expect("owned transfer total");
                *total = add(
                    *total,
                    transfer.finish_milliseconds - transfer.start_milliseconds,
                )?;
                out.transfers
                    .as_mut()
                    .expect("owned transfer replay")
                    .push(TransferStep {
                        worker: self.policy.workers[arrival.worker].id.clone(),
                        resource: policy.id.clone(),
                        copy: operation.copy,
                        from_operation: previous.id.clone(),
                        to_operation: operation.id.clone(),
                        from_cell: arrival.from.map(|cell| self.cell_ids[cell].clone()),
                        source_cell: transfer.from.clone(),
                        destination_cell: transfer.to.clone(),
                        departure_milliseconds: arrival.departure,
                        arrival_milliseconds: arrival.time,
                        start_milliseconds: transfer.start_milliseconds,
                        finish_milliseconds: transfer.finish_milliseconds,
                    });
                self.finish(&mut state, arrival, cell, transfer.finish_milliseconds);
                transfer_ready = transfer.finish_milliseconds;
            }
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
        if transfers.next().is_some() {
            return Err(bad("routed labor replay left an unmatched garment handoff"));
        }
        Ok(out)
    }

    pub(super) fn report(
        &self,
        baseline: &RouteTimeline,
        selected: &RouteTimeline,
    ) -> Result<Report, Refusal> {
        Ok(Report {
            schema: if self.transfer.is_some() {
                "ftx-sewing-attending-labor/2"
            } else {
                "ftx-sewing-attending-labor/1"
            },
            policy: self.policy.clone(),
            transfer: self.transfer.clone(),
            baseline: self.timeline(baseline)?,
            selected: self.timeline(selected)?,
            physical_labor_validated: false,
        })
    }
}
