//! Complete routed operations borrow finite fixtures from their source-qualified cell inventory.
//! Shared pools remain occupied through unloading; reservations are checked in physical time,
//! independently of the order in which the append-dispatch search constructs cell timelines.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use serde::Serialize;

use super::super::{parallel, workholding};
use super::{Policy, Timeline as RouteTimeline, add};
use crate::sewing::{SewOptions, bad};

const MAX_POOLS: usize = 16;
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
    /// Source lifecycles, each confined to one complete load/setup/feed/unload operation.
    pub holds: Vec<workholding::Hold>,
}

/// One physical fixture reservation, from the beginning of loading through complete unloading.
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
    /// Exact copy-scoped operation identity.
    pub operation: String,
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
    /// `ftx-sewing-routed-workholding/1`.
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

/// One reserved complete operation. Pool demand follows its actual eligible cell.
#[derive(Clone)]
pub(super) struct Reservation {
    route: usize,
    cell: usize,
    start: u64,
    finish: u64,
}

pub(super) struct Compiled {
    cells: Vec<Cell>,
    shared: Option<parallel::shared_workholding::Policy>,
    demands: Vec<Vec<[u32; MAX_POOLS]>>,
    capacities: Vec<u32>,
    maximum_holds: usize,
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
            shared.validate_routed(options, &policy.cells.iter().collect::<Vec<_>>())?;
        }
        let capacities = policy
            .shared_workholding
            .as_ref()
            .map(|shared| shared.resources.iter().map(|pool| pool.capacity).collect())
            .unwrap_or_default();
        let mut compiled = Self {
            cells: Vec::new(),
            shared: policy.shared_workholding.clone(),
            demands: vec![vec![[0; MAX_POOLS]; policy.cells.len()]; policy.route.len()],
            capacities,
            maximum_holds: 0,
        };
        let mut hold_counts = vec![vec![0usize; policy.cells.len()]; policy.route.len()];
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
            let mut local_demands: BTreeMap<(&str, &str), u32> = BTreeMap::new();
            for hold in &local.holds {
                if hold.acquire != hold.release {
                    return Err(bad(
                        "unloaded routing requires every fixture hold to start and end on one operation; retained or transported occupied fixtures need a different qualified protocol",
                    ));
                }
                let route = policy
                    .route
                    .iter()
                    .position(|step| {
                        step.cells().any(|id| id == cell.id) && step.operation == hold.acquire
                    })
                    .ok_or_else(|| bad("routed hold omits its exact owning cell and operation"))?;
                hold_counts[route][index] += 1;
                let local_units = local_demands
                    .entry((hold.acquire.as_str(), hold.resource.as_str()))
                    .or_default();
                *local_units = local_units
                    .checked_add(hold.units)
                    .ok_or_else(|| bad("routed local fixture demand overflows"))?;
                let capacity = local
                    .resources
                    .iter()
                    .find(|resource| resource.id == hold.resource)
                    .expect("admitted local resource")
                    .capacity;
                if *local_units > capacity {
                    return Err(bad(format!(
                        "{}: complete operation fixture demand exceeds local resource {} capacity",
                        hold.acquire, hold.resource
                    )));
                }
                if let Some(pool) = compiled.pool(&cell.id, &hold.resource) {
                    compiled.demands[route][index][pool] = compiled.demands[route][index][pool]
                        .checked_add(hold.units)
                        .ok_or_else(|| bad("routed shared fixture demand overflows"))?;
                    if compiled.demands[route][index][pool] > compiled.capacities[pool] {
                        return Err(bad(format!(
                            "{}: complete operation fixture demand exceeds shared pool {} capacity",
                            hold.acquire,
                            compiled.shared.as_ref().expect("bound pool").resources[pool].id
                        )));
                    }
                }
            }
            compiled.cells.push(Cell {
                cell: cell.id.clone(),
                machine_sha256: local.machine_sha256.clone(),
                qualifications,
                resources: local.resources.clone(),
                holds: local.holds.clone(),
            });
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
    ) -> Result<u64, Refusal> {
        let required = &self.demands[route][cell];
        if required.iter().all(|&units| units == 0) {
            return Ok(earliest);
        }
        let mut events = Vec::with_capacity(2 * reservations.len());
        for (index, reservation) in reservations.iter().enumerate() {
            if reservation.finish <= earliest
                || !required
                    .iter()
                    .zip(&self.demands[reservation.route][reservation.cell])
                    .any(|(&need, &held)| need != 0 && held != 0)
            {
                continue;
            }
            events.push((reservation.start, true, index));
            events.push((reservation.finish, false, index));
        }
        events.sort_unstable();
        let mut occupied = [0u32; MAX_POOLS];
        let mut previous = 0;
        let mut start = earliest;
        for (time, acquire, index) in events {
            let blocked = self
                .capacities
                .iter()
                .enumerate()
                .any(|(pool, &capacity)| occupied[pool] + required[pool] > capacity);
            if previous < time && blocked && start < time && add(start, duration)? > previous {
                // Every admissible start within this forbidden interval must wait until it ends.
                // Repeating over all sorted intervals also handles future reservations and short
                // gaps; inspecting occupancy only at the requested start would miss these cases.
                start = time;
            }
            for (held, &units) in occupied
                .iter_mut()
                .zip(&self.demands[reservations[index].route][reservations[index].cell])
            {
                *held = if acquire {
                    held.checked_add(units)
                } else {
                    held.checked_sub(units)
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
        add(start, duration)?;
        Ok(start)
    }

    pub(super) fn reserve(
        &self,
        reservations: &mut Vec<Reservation>,
        route: usize,
        cell: usize,
        start: u64,
        finish: u64,
    ) {
        if self.demands[route][cell].iter().any(|&units| units != 0) {
            reservations.push(Reservation {
                route,
                cell,
                start,
                finish,
            });
        }
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
            for hold in cell
                .holds
                .iter()
                .filter(|hold| hold.acquire == operation.operation_id)
            {
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
                    acquired_at_milliseconds: operation.loading_start_milliseconds,
                    released_at_milliseconds: operation.finish_milliseconds,
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
            schema: "ftx-sewing-routed-workholding/1",
            release_protocol: parallel::shared_workholding::ROUTED_RELEASE_PROTOCOL,
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
