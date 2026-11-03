//! Routed operations reserve finite fixtures from their source-qualified owning cell inventory.
//! Explicit retained lifecycles span operations without transporting an occupied fixture. Local
//! and shared reservations are checked in physical time, independently of dispatch construction.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use serde::Serialize;

use super::super::{parallel, workholding};
use super::{MAX_EXACT_MILLISECONDS, Policy, Timeline as RouteTimeline};
use crate::sewing::{SewOptions, bad};

const MAX_SOURCE_HOLDS: usize = 64;
const MAX_EXPANDED_HOLDS: usize = 1024;
const MAX_QUALIFICATION_REFERENCES: usize = 256;

/// Exact admitted local inventory and source qualifications for an actual specialist cell.
#[derive(Clone, Debug, Serialize)]
pub struct Cell {
    /// Physical cell ID; equal resource names on different cells never imply a common pool.
    pub cell: String,
    /// Complete machine content lock from the existing workholding qualification.
    pub machine_sha256: String,
    /// Actual admitted qualification/content lock for each operation owned by this cell.
    pub qualifications: BTreeMap<String, workholding::QualificationRef>,
    /// Independently qualified cell-local capacities, still enforced when bound to shared pools.
    pub resources: Vec<workholding::Resource>,
    /// Source lifecycles; retained mode reserves inventory through the declared release operation.
    pub holds: Vec<workholding::Hold>,
}

/// One cell-owned fixture reservation, from acquisition loading through release unloading.
#[derive(Clone, Debug, Serialize)]
pub struct Hold {
    /// Physical specialist cell.
    pub cell: String,
    /// Global physical garment copy, identical to the emitted stitch identity.
    pub copy: usize,
    /// Exact copy-scoped source hold ID within the named cell.
    pub hold: String,
    /// Original local resource identity.
    pub resource: String,
    /// Explicit common pool, if bound; omission retains independent local inventory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    /// Integer units continuously reserved over the full half-open interval.
    pub units: u32,
    /// Exact copy-scoped acquisition operation identity.
    pub operation: String,
    /// Distinct copy-scoped release operation for a retained lifecycle; omitted for ordinary holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_operation: Option<String>,
    /// Reservation begins before loading, setup and feed.
    pub acquired_at_milliseconds: u64,
    /// Complete unloading releases the reservation; a simultaneous acquisition may reuse it.
    pub released_at_milliseconds: u64,
}

/// Independently swept full-duration reservations, including intervals inserted before others.
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    /// Every requested source hold on every physical garment, sorted by cell/copy/hold.
    pub holds: Vec<Hold>,
    /// Maximum simultaneous occupancy of each declared cell-local resource.
    pub local_peak_occupancy: BTreeMap<String, BTreeMap<String, u32>>,
    /// Maximum simultaneous occupancy of each explicitly declared common pool.
    pub shared_peak_occupancy: BTreeMap<String, u32>,
    /// Complete local inventory replay must end at zero for every resource.
    pub local_final_occupancy: BTreeMap<String, BTreeMap<String, u32>>,
    /// Complete common-pool replay must end at zero for every pool.
    pub shared_final_occupancy: BTreeMap<String, u32>,
}

/// Source-qualified fixture enforcement on the actual routed baseline and selected execution.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `/1` complete-operation holds; `/2` explicitly retained cell-owned lifecycles.
    pub schema: &'static str,
    /// Exact release/access convention, independent of any garment handoff protocol.
    pub release_protocol: &'static str,
    /// Admitted local capacities, lifecycles and machine/material/process locks.
    pub cells: Vec<Cell>,
    /// Independent common inventory, access and return qualification, when requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_workholding: Option<parallel::shared_workholding::Policy>,
    /// Complete retained baseline, including an explicitly replaced copy-order seed.
    pub baseline: Timeline,
    /// Complete selected execution, independently replayed against both kinds of capacity.
    pub selected: Timeline,
    /// Integer inventory enforcement does not establish grip adequacy or geometric access.
    pub fixture_geometry_and_access_validated: bool,
    /// Declared source evidence does not establish measured physical pool access or return.
    pub physical_pool_access_validated: bool,
}

/// One exact physical copy's source lifecycle. An undispatched release has no invented end time.
#[derive(Clone)]
pub(super) struct Reservation {
    lifecycle: usize,
    copy: usize,
    start: u64,
    finish: Option<u64>,
}

struct Lifecycle {
    cell: usize,
    acquire: usize,
    release: usize,
    local: usize,
    pool: Option<usize>,
    units: u32,
}

pub(super) struct Compiled {
    cells: Vec<Cell>,
    shared: Option<parallel::shared_workholding::Policy>,
    demands: Vec<Vec<Vec<u32>>>,
    retained_demands: Vec<Vec<Vec<u32>>>,
    capacities: Vec<u32>,
    lifecycles: Vec<Lifecycle>,
    maximum_holds: usize,
    retained: bool,
}

impl Compiled {
    pub(super) fn compile(
        policy: &Policy,
        options: &SewOptions,
        plans: &[parallel::CellPlan],
        copies: usize,
    ) -> Result<Option<Self>, Refusal> {
        if policy.shared_workholding.is_none()
            && policy.cells.iter().all(|cell| cell.workholding.is_none())
        {
            return Ok(None);
        }
        if let Some(shared) = &policy.shared_workholding {
            shared.validate_routed(
                options,
                &policy.cells.iter().collect::<Vec<_>>(),
                policy.retained_fixtures(),
            )?;
        }
        let mut capacities: Vec<u32> = policy
            .shared_workholding
            .as_ref()
            .map(|shared| shared.resources.iter().map(|pool| pool.capacity).collect())
            .unwrap_or_default();
        let mut cells = Vec::new();
        let mut local_offsets = vec![None; policy.cells.len()];
        let mut source_holds = 0usize;
        let mut qualification_references = 0usize;
        for (index, cell) in policy.cells.iter().enumerate() {
            let Some(local) = &cell.workholding else {
                continue;
            };
            let qualifications = local.validate(&plans[index].plan)?;
            source_holds += local.holds.len();
            qualification_references += local.qualifications.values().map(Vec::len).sum::<usize>();
            if source_holds > MAX_SOURCE_HOLDS
                || qualification_references > MAX_QUALIFICATION_REFERENCES
            {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "routed workholding supports at most 64 total source holds, 1024 expanded holds and 256 qualification references",
                ));
            }
            local_offsets[index] = Some(capacities.len());
            capacities.extend(local.resources.iter().map(|resource| resource.capacity));
            cells.push(Cell {
                cell: cell.id.clone(),
                machine_sha256: local.machine_sha256.clone(),
                qualifications,
                resources: local.resources.clone(),
                holds: local.holds.clone(),
            });
        }
        let demands = vec![vec![vec![0; capacities.len()]; policy.cells.len()]; policy.route.len()];
        let mut compiled = Self {
            cells,
            shared: policy.shared_workholding.clone(),
            retained_demands: demands.clone(),
            demands,
            capacities,
            lifecycles: Vec::with_capacity(source_holds),
            maximum_holds: 0,
            retained: policy.retained_fixtures(),
        };
        let mut hold_counts = vec![vec![0usize; policy.cells.len()]; policy.route.len()];
        for (index, cell) in policy.cells.iter().enumerate() {
            let Some(local) = &cell.workholding else {
                continue;
            };
            for hold in &local.holds {
                if hold.acquire != hold.release && !compiled.retained {
                    return Err(bad(
                        "retained or transported occupied fixtures require a qualified protocol; cell-retained reservations need the explicit retained protocol and never authorize occupied fixture transport",
                    ));
                }
                let anchor = |operation: &str| {
                    policy
                        .route
                        .iter()
                        .position(|step| {
                            step.cells().any(|id| id == cell.id) && step.operation == operation
                        })
                        .ok_or_else(|| bad("routed hold omits its exact owning cell and operation"))
                };
                let acquire = anchor(&hold.acquire)?;
                let release = anchor(&hold.release)?;
                if release < acquire {
                    return Err(bad("routed fixture release precedes its acquisition"));
                }
                hold_counts[acquire][index] += 1;
                let local_index = local_offsets[index].expect("admitted local inventory")
                    + local
                        .resources
                        .iter()
                        .position(|resource| resource.id == hold.resource)
                        .expect("admitted local resource");
                let pool = compiled.pool(&cell.id, &hold.resource);
                for resource in std::iter::once(local_index).chain(pool) {
                    compiled.demands[acquire][index][resource] = compiled.demands[acquire][index]
                        [resource]
                        .checked_add(hold.units)
                        .ok_or_else(|| bad("routed fixture acquisition demand overflows"))?;
                    if compiled.demands[acquire][index][resource] > compiled.capacities[resource] {
                        let scope = if Some(resource) == pool {
                            "shared pool"
                        } else {
                            "local resource"
                        };
                        return Err(bad(format!(
                            "{}: fixture acquisition exceeds {scope} capacity",
                            hold.acquire
                        )));
                    }
                    if acquire != release {
                        compiled.retained_demands[acquire][index][resource] += hold.units;
                    }
                }
                compiled.lifecycles.push(Lifecycle {
                    cell: index,
                    acquire,
                    release,
                    local: local_index,
                    pool,
                    units: hold.units,
                });
            }
        }
        compiled.maximum_holds = hold_counts
            .iter()
            .map(|choices| choices.iter().max().copied().unwrap_or(0))
            .sum::<usize>()
            * copies;
        if compiled.maximum_holds > MAX_EXPANDED_HOLDS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "routed workholding supports at most 1024 expanded holds across every eligible assignment",
            ));
        }
        Ok(Some(compiled))
    }

    fn pool(&self, cell: &str, resource: &str) -> Option<usize> {
        let shared = self.shared.as_ref()?;
        let binding = shared
            .bindings
            .iter()
            .find(|binding| binding.cell == cell && binding.resource == resource)?;
        shared
            .resources
            .iter()
            .position(|pool| pool.id == binding.pool)
    }

    pub(super) fn earliest_start(
        &self,
        reservations: &[Reservation],
        route: usize,
        cell: usize,
        earliest: u64,
        duration: u64,
    ) -> Result<Option<u64>, Refusal> {
        if duration > MAX_EXACT_MILLISECONDS - earliest {
            return Ok(None);
        }
        let required = &self.demands[route][cell];
        if required.iter().all(|&units| units == 0) {
            return Ok(Some(earliest));
        }
        let retained = &self.retained_demands[route][cell];
        let mut events = Vec::with_capacity(2 * reservations.len());
        for (index, reservation) in reservations.iter().enumerate() {
            let lifecycle = &self.lifecycles[reservation.lifecycle];
            let finish = reservation.finish.unwrap_or(MAX_EXACT_MILLISECONDS);
            if finish <= earliest
                || std::iter::once(lifecycle.local)
                    .chain(lifecycle.pool)
                    .all(|resource| required[resource] == 0)
            {
                continue;
            }
            events.push((reservation.start, true, index));
            events.push((finish, false, index));
        }
        events.sort_unstable();
        let mut occupied = vec![0u32; self.capacities.len()];
        let mut previous = 0;
        let mut start = earliest;
        for (time, acquire, index) in events {
            let blocked_short = self
                .capacities
                .iter()
                .enumerate()
                .any(|(resource, &capacity)| occupied[resource] > capacity - required[resource]);
            let blocked_retained = self
                .capacities
                .iter()
                .enumerate()
                .any(|(resource, &capacity)| occupied[resource] > capacity - retained[resource]);
            if previous < time
                && start < time
                && (blocked_retained
                    || blocked_short && (start > previous || duration > previous - start))
            {
                // A new retained hold has no known release yet. Keep its entire open tail within
                // capacity, including reservations already placed later in physical time. Its
                // declared release can only shorten this conservative interval. An existing open
                // hold blocks through the exact-clock bound; no future release time is guessed.
                start = time;
            }
            let lifecycle = &self.lifecycles[reservations[index].lifecycle];
            for resource in std::iter::once(lifecycle.local).chain(lifecycle.pool) {
                let held = &mut occupied[resource];
                *held = if acquire {
                    held.checked_add(lifecycle.units)
                } else {
                    held.checked_sub(lifecycle.units)
                }
                .ok_or_else(|| bad("routed fixture reservation sweep is inconsistent"))?;
            }
            previous = time;
        }
        if occupied.iter().any(|&units| units != 0) {
            return Err(bad(
                "routed fixture reservation sweep leaves inventory held",
            ));
        }
        Ok((duration <= MAX_EXACT_MILLISECONDS - start).then_some(start))
    }

    pub(super) fn can_dispatch(
        &self,
        reservations: &[Reservation],
        copy: usize,
        route: usize,
        cell: usize,
    ) -> bool {
        // Release must occur at the actual acquisition cell, and selecting a release cell cannot
        // silently borrow a source lifecycle whose acquisition was routed to another alternative.
        reservations.iter().all(|reservation| {
            let lifecycle = &self.lifecycles[reservation.lifecycle];
            reservation.copy != copy
                || reservation.finish.is_some()
                || lifecycle.release != route
                || lifecycle.cell == cell
        }) && self
            .lifecycles
            .iter()
            .enumerate()
            .all(|(index, lifecycle)| {
                lifecycle.cell != cell
                    || lifecycle.release != route
                    || lifecycle.acquire == lifecycle.release
                    || reservations.iter().any(|reservation| {
                        reservation.copy == copy
                            && reservation.lifecycle == index
                            && reservation.finish.is_none()
                    })
            })
    }

    pub(super) fn reserve(
        &self,
        reservations: &mut Vec<Reservation>,
        copy: usize,
        route: usize,
        cell: usize,
        start: u64,
        finish: u64,
    ) -> Result<(), Refusal> {
        if !self.can_dispatch(reservations, copy, route, cell) {
            return Err(bad(
                "routed fixture release changed its owning copy or cell",
            ));
        }
        for (index, lifecycle) in self
            .lifecycles
            .iter()
            .enumerate()
            .filter(|(_, lifecycle)| lifecycle.acquire == route && lifecycle.cell == cell)
        {
            reservations.push(Reservation {
                lifecycle: index,
                copy,
                start,
                finish: (lifecycle.acquire == lifecycle.release).then_some(finish),
            });
        }
        for reservation in reservations.iter_mut().filter(|reservation| {
            let lifecycle = &self.lifecycles[reservation.lifecycle];
            reservation.copy == copy
                && reservation.finish.is_none()
                && lifecycle.release == route
                && lifecycle.cell == cell
        }) {
            if finish <= reservation.start {
                return Err(bad(
                    "routed fixture release precedes its actual acquisition",
                ));
            }
            reservation.finish = Some(finish);
        }
        if reservations.len() > self.maximum_holds {
            return Err(bad(
                "routed fixture reservations exceed their admitted lifecycle bound",
            ));
        }
        Ok(())
    }

    fn timeline(&self, route: &RouteTimeline) -> Result<Timeline, Refusal> {
        let mut holds = Vec::with_capacity(self.maximum_holds);
        for operation in &route.operations {
            let Some(cell) = self.cells.iter().find(|cell| cell.cell == operation.cell) else {
                continue;
            };
            if operation.finish_milliseconds <= operation.loading_start_milliseconds {
                return Err(bad(
                    "routed fixture lifetime must include a complete positive operation",
                ));
            }
            for hold in cell.holds.iter().filter(|hold| {
                hold.acquire == operation.operation_id || hold.release == operation.operation_id
            }) {
                let acquire = route
                    .operations
                    .iter()
                    .find(|candidate| {
                        candidate.copy == operation.copy && candidate.operation_id == hold.acquire
                    })
                    .ok_or_else(|| bad("routed fixture replay omits its acquisition operation"))?;
                let release = route
                    .operations
                    .iter()
                    .find(|candidate| {
                        candidate.copy == operation.copy && candidate.operation_id == hold.release
                    })
                    .ok_or_else(|| bad("routed fixture replay omits its release operation"))?;
                if acquire.cell != cell.cell
                    || release.cell != cell.cell
                    || release.finish_milliseconds < acquire.finish_milliseconds
                {
                    return Err(bad(
                        "routed fixture replay changed its cell ownership or lifecycle order",
                    ));
                }
                if hold.acquire != operation.operation_id {
                    continue;
                }
                holds.push(Hold {
                    cell: cell.cell.clone(),
                    copy: operation.copy,
                    hold: format!("{}/{}", operation.copy, hold.id),
                    resource: hold.resource.clone(),
                    pool: self.pool(&cell.cell, &hold.resource).map(|index| {
                        self.shared.as_ref().expect("bound pool").resources[index]
                            .id
                            .clone()
                    }),
                    units: hold.units,
                    operation: operation.id.clone(),
                    release_operation: (hold.release != hold.acquire).then(|| release.id.clone()),
                    acquired_at_milliseconds: operation.loading_start_milliseconds,
                    released_at_milliseconds: release.finish_milliseconds,
                });
            }
        }
        if holds.len() > self.maximum_holds {
            return Err(bad(
                "routed fixture replay exceeds the admitted complete lifecycle bound",
            ));
        }
        holds.sort_by(|a, b| (&a.cell, a.copy, &a.hold).cmp(&(&b.cell, b.copy, &b.hold)));
        let mut local: BTreeMap<String, BTreeMap<String, u32>> = self
            .cells
            .iter()
            .map(|cell| {
                (
                    cell.cell.clone(),
                    cell.resources.iter().map(|r| (r.id.clone(), 0)).collect(),
                )
            })
            .collect();
        let mut shared: BTreeMap<String, u32> = self
            .shared
            .iter()
            .flat_map(|policy| policy.resources.iter().map(|r| (r.id.clone(), 0)))
            .collect();
        let mut local_peaks = local.clone();
        let mut shared_peaks = shared.clone();
        let mut events = Vec::with_capacity(2 * holds.len());
        for (index, hold) in holds.iter().enumerate() {
            events.push((hold.acquired_at_milliseconds, true, index));
            events.push((hold.released_at_milliseconds, false, index));
        }
        events.sort_unstable();
        for (_, acquire, index) in events {
            let hold = &holds[index];
            let cell = self
                .cells
                .iter()
                .find(|cell| cell.cell == hold.cell)
                .expect("source cell");
            let capacity = cell
                .resources
                .iter()
                .find(|r| r.id == hold.resource)
                .expect("source resource")
                .capacity;
            change(
                local
                    .get_mut(&hold.cell)
                    .expect("source cell")
                    .get_mut(&hold.resource)
                    .expect("source resource"),
                local_peaks
                    .get_mut(&hold.cell)
                    .expect("source cell")
                    .get_mut(&hold.resource)
                    .expect("source resource"),
                hold.units,
                capacity,
                acquire,
            )?;
            if let Some(pool) = &hold.pool {
                let capacity = self
                    .shared
                    .as_ref()
                    .expect("source pool")
                    .resources
                    .iter()
                    .find(|r| r.id == *pool)
                    .expect("source pool")
                    .capacity;
                change(
                    shared.get_mut(pool).expect("source pool"),
                    shared_peaks.get_mut(pool).expect("source pool"),
                    hold.units,
                    capacity,
                    acquire,
                )?;
            }
        }
        if local
            .values()
            .flat_map(BTreeMap::values)
            .chain(shared.values())
            .any(|&units| units != 0)
        {
            return Err(bad("routed fixture replay leaves inventory unreleased"));
        }
        Ok(Timeline {
            holds,
            local_peak_occupancy: local_peaks,
            shared_peak_occupancy: shared_peaks,
            local_final_occupancy: local,
            shared_final_occupancy: shared,
        })
    }

    pub(super) fn report(
        &self,
        baseline: &RouteTimeline,
        selected: &RouteTimeline,
    ) -> Result<Report, Refusal> {
        Ok(Report {
            schema: if self.retained {
                "ftx-sewing-routed-workholding/2"
            } else {
                "ftx-sewing-routed-workholding/1"
            },
            release_protocol: if self.retained {
                parallel::shared_workholding::RETAINED_ROUTED_RELEASE_PROTOCOL
            } else {
                parallel::shared_workholding::ROUTED_RELEASE_PROTOCOL
            },
            cells: self.cells.clone(),
            shared_workholding: self.shared.clone(),
            baseline: self.timeline(baseline)?,
            selected: self.timeline(selected)?,
            fixture_geometry_and_access_validated: false,
            physical_pool_access_validated: false,
        })
    }
}

fn change(
    held: &mut u32,
    peak: &mut u32,
    units: u32,
    capacity: u32,
    acquire: bool,
) -> Result<(), Refusal> {
    *held = if acquire {
        held.checked_add(units).filter(|&next| next <= capacity)
    } else {
        held.checked_sub(units)
    }
    .ok_or_else(|| bad("routed fixture replay violates exact finite capacity"))?;
    *peak = (*peak).max(*held);
    Ok(())
}
