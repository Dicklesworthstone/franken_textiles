//! Finite unloaded buffers with explicit entry/exit boundaries and conservative mover reservations.
//! Open output tails have no guessed release; completed timelines are independently swept in time.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{
    Handoff, MAX_EXACT_MILLISECONDS, MAX_OPERATIONS, Policy as RoutePolicy, RouteStep,
    Timeline as RouteTimeline, source_valid,
};
use crate::sewing::bad;

/// Qualified finite cell buffers; copies enter at first loading and exit at final unloading.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `finite_unloaded_cell_buffers/1`; external input delivery and finished storage are outside
    /// the modeled boundary. Existing loading/unloading still cover every admitted operation.
    pub model: String,
    /// Evidence for uniform capacity, the entry/exit boundary and the handoff reservation protocol.
    pub source: Source,
    /// Qualification method; no unmeasured transport or blocked unloading duration is inferred.
    pub method: String,
    /// Source applicability conditions, without automatic extrapolation.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Evidence independent of cell, worker, fixture and transfer timing observations.
    pub evidence: Evidence,
    /// Separate consent for estimated buffer capacity or boundary applicability.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// One positive half-open reservation for one unloaded garment at one actual specialist cell.
#[derive(Clone, Debug, Serialize)]
pub struct SlotReservation {
    /// Physical garment copy.
    pub copy: usize,
    /// Actual selected cell, including eligible-cell assignments.
    pub cell: String,
    /// Copy-scoped operation that unloads to or loads from this reservation.
    pub operation: String,
    /// `after_unloading` or `before_loading`; mover reservations may cover both cells concurrently.
    pub kind: &'static str,
    /// Inclusive acquisition from the common job origin.
    pub start_milliseconds: u64,
    /// Exclusive release; simultaneous acquisitions may reuse the slot.
    pub finish_milliseconds: u64,
}

/// Complete reservation replay, independent of search construction and dispatch ordering.
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    /// Positive intervals only, deterministically ordered by cell, copy and operation.
    pub reservations: Vec<SlotReservation>,
    /// Maximum simultaneous reserved slots at every admitted cell, including unused buffers.
    pub peak_reserved_slots: BTreeMap<String, usize>,
    /// Exact replay ends at zero at every cell; finished output has left the modeled boundary.
    pub final_reserved_slots: BTreeMap<String, usize>,
}

/// Explicit capacity and independent baseline/selected reservation evidence.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `/1` one shared handoff boundary law; `/2` each selected carrier's qualified phase law.
    pub schema: &'static str,
    /// Exact admitted capacity/boundary qualification.
    pub policy: Policy,
    /// Uniform finite slot limit at each physical cell.
    pub capacity: usize,
    /// First loading enters the system; final unloading exits to external finished storage.
    pub entry_exit_protocol: &'static str,
    /// Exact owned pickup/deposit, or conservative full independent-mover reservations.
    pub handoff_reservations: &'static str,
    /// Complete feasible retained baseline, including a searched replacement when required.
    pub baseline: Timeline,
    /// Complete selected execution, independently swept against the same capacity.
    pub selected: Timeline,
    /// Declared qualifications and integer occupancy do not establish measured factory execution.
    pub physical_staging_validated: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    AfterUnloading,
    BeforeLoading,
}

#[derive(Clone)]
pub(super) struct Reservation {
    pub(super) copy: usize,
    pub(super) route: usize,
    pub(super) cell: usize,
    pub(super) kind: Kind,
    pub(super) start: u64,
    pub(super) finish: Option<u64>,
}

pub(super) enum Alignment {
    Ready(Vec<Reservation>),
    Delay {
        handoff_start: u64,
        loading_start: u64,
    },
    Blocked,
}

pub(super) struct Compiled {
    policy: Policy,
    capacity: usize,
    cells: Vec<String>,
    route: Vec<RouteStep>,
    handoffs: BTreeMap<Option<String>, (Vec<Handoff>, bool)>,
    owned: Vec<bool>,
    qualified_pool: bool,
}

impl Compiled {
    pub(super) fn compile(route: &RoutePolicy) -> Result<Option<Self>, Refusal> {
        let Some(policy) = &route.bounded_staging else {
            return Ok(None);
        };
        if policy.model != "finite_unloaded_cell_buffers/1"
            || !(1..=4096).contains(&route.staging_capacity)
            || !source_valid(&policy.source, &policy.method, &policy.conditions)
        {
            return Err(bad("finite staging needs a qualified positive capacity"));
        }
        if policy.evidence == Evidence::Estimate && !policy.allow_estimates {
            return Err(bad("estimated staging needs its own explicit consent"));
        }
        Ok(Some(Self {
            policy: policy.clone(),
            capacity: route.staging_capacity,
            cells: route.cells.iter().map(|cell| cell.id.clone()).collect(),
            route: route.route.clone(),
            handoffs: route
                .transfer_resources()
                .map(|carrier| {
                    (
                        route.transfer_pool.as_ref().map(|_| carrier.id.clone()),
                        (
                            carrier.transfers.clone(),
                            carrier.attending_worker.is_some(),
                        ),
                    )
                })
                .collect(),
            owned: route
                .transfer_resources()
                .map(|carrier| carrier.attending_worker.is_some())
                .collect(),
            qualified_pool: route.qualified_transfer_pool(),
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn align(
        &self,
        reservations: &[Reservation],
        copy: usize,
        route: usize,
        cell: usize,
        final_operation: bool,
        load_start: u64,
        finish: u64,
        handoff: Option<[u64; 2]>,
        carrier: usize,
    ) -> Result<Alignment, Refusal> {
        if route >= self.route.len()
            || cell >= self.cells.len()
            || final_operation != (route + 1 == self.route.len())
            || finish <= load_start
            || finish > MAX_EXACT_MILLISECONDS
            || reservations.len() > 2 * MAX_OPERATIONS
            || handoff.is_some_and(|[start, end]| start > end || end > load_start)
        {
            return Err(bad("finite staging has an invalid operation interval"));
        }
        let owned = *self
            .owned
            .get(carrier)
            .ok_or_else(|| bad("finite staging names an unknown carrier"))?;
        let mut next = reservations.to_vec();
        let mut open = next
            .iter_mut()
            .filter(|hold| hold.copy == copy && hold.finish.is_none());
        let previous = open.next();
        if open.next().is_some() || (route == 0) != previous.is_none() {
            return Err(bad("finite staging changed a copy's pending reservation"));
        }
        if let Some(previous) = previous {
            if previous.kind != Kind::AfterUnloading
                || previous.route.checked_add(1) != Some(route)
                || (previous.cell != cell) != handoff.is_some()
            {
                return Err(bad("finite staging changed a copy's route or handoff cell"));
            }
            let release =
                handoff.map_or(load_start, |[start, end]| if owned { start } else { end });
            if handoff.map_or(load_start, |times| times[0]) < previous.start {
                return Err(bad("finite staging releases before complete unloading"));
            }
            previous.finish = Some(release);
        } else if handoff.is_some() {
            return Err(bad("finite staging transfers an unentered garment"));
        }
        next.retain(|hold| hold.finish != Some(hold.start));
        let saturated = self.saturated(&next, cell)?;
        let mut loading_start = 0;
        let mut handoff_start = 0;
        if !final_operation {
            for &(_, end) in &saturated {
                match end {
                    None => return Ok(Alignment::Blocked),
                    Some(end) if end > finish => {
                        loading_start = loading_start.max(end.saturating_sub(finish - load_start));
                    }
                    _ => {}
                }
            }
        }
        if let Some([start, end]) = handoff {
            let arrival = if owned { end } else { start };
            if arrival < load_start {
                for &(blocked_start, blocked_end) in &saturated {
                    if blocked_start >= load_start || blocked_end.is_some_and(|end| end <= arrival)
                    {
                        continue;
                    }
                    // Exact pickup/deposit permits a zero-length stay at loading. A positive
                    // independent handoff reserves its destination for the entire mover interval.
                    let can_load_directly = owned || start == end;
                    let next_arrival = match blocked_end {
                        Some(end) if can_load_directly => end.min(load_start),
                        Some(end) => end,
                        None if can_load_directly => load_start,
                        None => return Ok(Alignment::Blocked),
                    };
                    handoff_start = handoff_start.max(next_arrival.saturating_sub(if owned {
                        end - start
                    } else {
                        0
                    }));
                }
            }
            if arrival < load_start {
                next.push(Reservation {
                    copy,
                    route,
                    cell,
                    kind: Kind::BeforeLoading,
                    start: arrival,
                    finish: Some(load_start),
                });
            }
        }
        if loading_start != 0 || handoff_start != 0 {
            return Ok(Alignment::Delay {
                handoff_start,
                loading_start,
            });
        }
        if !final_operation {
            next.push(Reservation {
                copy,
                route,
                cell,
                kind: Kind::AfterUnloading,
                start: finish,
                finish: None,
            });
        }
        if next.len() > 2 * MAX_OPERATIONS {
            return Err(bad("finite staging exceeds its reservation bound"));
        }
        Ok(Alignment::Ready(next))
    }

    /// Saturated intervals of the retained prefix, including closed holds later in physical time.
    fn saturated(
        &self,
        holds: &[Reservation],
        cell: usize,
    ) -> Result<Vec<(u64, Option<u64>)>, Refusal> {
        let mut events = Vec::with_capacity(2 * holds.len());
        for hold in holds {
            if hold.start > MAX_EXACT_MILLISECONDS
                || hold
                    .finish
                    .is_some_and(|end| end < hold.start || end > MAX_EXACT_MILLISECONDS)
                || hold.cell >= self.cells.len()
                || hold.route >= self.route.len()
                || hold.finish.is_none() && hold.kind != Kind::AfterUnloading
            {
                return Err(bad("finite staging has an invalid retained reservation"));
            }
            if hold.cell != cell || hold.finish == Some(hold.start) {
                continue;
            }
            events.push((hold.start, true));
            if let Some(end) = hold.finish {
                events.push((end, false));
            }
        }
        events.sort_unstable();
        let mut occupied = 0usize;
        let mut previous = 0;
        let mut saturated = Vec::new();
        for (time, acquire) in events {
            if previous < time && occupied == self.capacity {
                saturated.push((previous, Some(time)));
            }
            occupied = if acquire {
                occupied
                    .checked_add(1)
                    .filter(|&next| next <= self.capacity)
            } else {
                occupied.checked_sub(1)
            }
            .ok_or_else(|| bad("finite staging prefix violates its cell capacity"))?;
            previous = time;
        }
        if occupied == self.capacity {
            saturated.push((previous, None));
        }
        Ok(saturated)
    }

    fn timeline(&self, timeline: &RouteTimeline) -> Result<Timeline, Refusal> {
        if self.route.is_empty()
            || timeline.operations.is_empty()
            || timeline.operations.len() > MAX_OPERATIONS
            || !timeline.operations.len().is_multiple_of(self.route.len())
            || timeline.transfers.len() > timeline.operations.len()
        {
            return Err(bad("finite staging replay needs a complete garment route"));
        }
        let mut operations: Vec<_> = timeline.operations.iter().collect();
        operations.sort_by_key(|operation| (operation.copy, operation.route_index));
        let mut transfers = BTreeMap::new();
        for transfer in &timeline.transfers {
            if transfers
                .insert((transfer.copy, transfer.to_operation.as_str()), transfer)
                .is_some()
            {
                return Err(bad("finite staging replay duplicates a garment handoff"));
            }
        }
        let mut reservations = Vec::new();
        for (index, operation) in operations.iter().enumerate() {
            let route = index % self.route.len();
            let phases = [
                operation.loading_start_milliseconds,
                operation.setup_start_milliseconds,
                operation.feed_start_milliseconds,
                operation.feed_finish_milliseconds,
                operation.finish_milliseconds,
            ];
            if operation.copy != index / self.route.len()
                || operation.route_index != route
                || operation.operation_id != self.route[route].operation
                || operation.id != format!("{}/{}", operation.copy, operation.operation_id)
                || !self.route[route].cells().any(|cell| cell == operation.cell)
                || !self.cells.contains(&operation.cell)
                || phases.windows(2).any(|pair| pair[0] > pair[1])
                || phases[0] == phases[4]
                || phases[4] > MAX_EXACT_MILLISECONDS
            {
                return Err(bad("finite staging replay changed an operation or phases"));
            }
            if route == 0 {
                continue;
            }
            let previous = operations[index - 1];
            let transfer = transfers.remove(&(operation.copy, operation.operation_id.as_str()));
            let release = if previous.cell == operation.cell {
                if transfer.is_some() {
                    return Err(bad("finite staging replay added a same-cell handoff"));
                }
                operation.loading_start_milliseconds
            } else {
                let transfer =
                    transfer.ok_or_else(|| bad("finite staging replay omits a handoff"))?;
                let (edges, owned) = self
                    .handoffs
                    .get(&transfer.resource)
                    .ok_or_else(|| bad("finite staging replay changes its selected carrier"))?;
                if transfer.from != previous.cell
                    || transfer.to != operation.cell
                    || transfer.from_operation != previous.operation_id
                    || transfer.start_milliseconds < previous.finish_milliseconds
                    || transfer.start_milliseconds > transfer.finish_milliseconds
                    || transfer.finish_milliseconds > operation.loading_start_milliseconds
                    || !edges.iter().any(|edge| {
                        edge.from == transfer.from
                            && edge.to == transfer.to
                            && u64::from(edge.milliseconds)
                                == transfer.finish_milliseconds - transfer.start_milliseconds
                    })
                {
                    return Err(bad("finite staging replay changed its qualified handoff"));
                }
                push(
                    &mut reservations,
                    operation,
                    "before_loading",
                    if *owned {
                        transfer.finish_milliseconds
                    } else {
                        transfer.start_milliseconds
                    },
                    operation.loading_start_milliseconds,
                )?;
                if *owned {
                    transfer.start_milliseconds
                } else {
                    transfer.finish_milliseconds
                }
            };
            push(
                &mut reservations,
                previous,
                "after_unloading",
                previous.finish_milliseconds,
                release,
            )?;
        }
        if !transfers.is_empty()
            || operations.iter().map(|op| op.finish_milliseconds).max()
                != Some(timeline.makespan_milliseconds)
        {
            return Err(bad("finite staging replay has an unmatched handoff or end"));
        }
        reservations.sort_by(|a, b| {
            (&a.cell, a.copy, &a.operation, a.kind, a.start_milliseconds).cmp(&(
                &b.cell,
                b.copy,
                &b.operation,
                b.kind,
                b.start_milliseconds,
            ))
        });
        let mut occupied: BTreeMap<_, _> = self
            .cells
            .iter()
            .map(|cell| (cell.clone(), 0usize))
            .collect();
        let mut peaks = occupied.clone();
        let mut events = Vec::with_capacity(2 * reservations.len());
        for (index, reservation) in reservations.iter().enumerate() {
            events.push((reservation.start_milliseconds, true, index));
            events.push((reservation.finish_milliseconds, false, index));
        }
        events.sort_unstable();
        for (_, acquire, index) in events {
            let cell = &reservations[index].cell;
            let held = occupied.get_mut(cell).expect("admitted staging cell");
            *held = if acquire {
                held.checked_add(1).filter(|&next| next <= self.capacity)
            } else {
                held.checked_sub(1)
            }
            .ok_or_else(|| bad("finite staging replay violates exact cell capacity"))?;
            let peak = peaks.get_mut(cell).expect("admitted staging cell");
            *peak = (*peak).max(*held);
        }
        if occupied.values().any(|&held| held != 0) {
            return Err(bad("finite staging replay leaves a garment in a buffer"));
        }
        Ok(Timeline {
            reservations,
            peak_reserved_slots: peaks,
            final_reserved_slots: occupied,
        })
    }

    pub(super) fn report(
        &self,
        baseline: &RouteTimeline,
        selected: &RouteTimeline,
    ) -> Result<Report, Refusal> {
        Ok(Report {
            schema: if self.qualified_pool {
                "ftx-sewing-routing-staging/2"
            } else {
                "ftx-sewing-routing-staging/1"
            },
            policy: self.policy.clone(),
            capacity: self.capacity,
            entry_exit_protocol: "first_loading_entry_final_unloading_exit/1",
            handoff_reservations: if self.qualified_pool {
                "selected_carrier_source_boundaries/1"
            } else if self.owned[0] {
                "source_until_pickup_destination_from_deposit/1"
            } else {
                "source_until_handoff_finish_destination_from_handoff_start/1"
            },
            baseline: self.timeline(baseline)?,
            selected: self.timeline(selected)?,
            physical_staging_validated: false,
        })
    }
}

fn push(
    rows: &mut Vec<SlotReservation>,
    operation: &super::Operation,
    kind: &'static str,
    start: u64,
    finish: u64,
) -> Result<(), Refusal> {
    if finish < start || finish > MAX_EXACT_MILLISECONDS {
        return Err(bad("finite staging replay releases before acquisition"));
    }
    if start < finish {
        rows.push(SlotReservation {
            copy: operation.copy,
            cell: operation.cell.clone(),
            operation: operation.id.clone(),
            kind,
            start_milliseconds: start,
            finish_milliseconds: finish,
        });
    }
    Ok(())
}
