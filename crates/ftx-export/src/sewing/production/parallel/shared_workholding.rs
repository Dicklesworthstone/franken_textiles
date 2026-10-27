//! A qualified common inventory supplies existing cell-local fixture lifecycles. Acquisitions
//! precede handling; only completion of the declared release operation makes units available again.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{Assignment, Cell, operator};
use crate::sewing::{SewPlan, bad, production::workholding, qualified_text, sha256_text};

const MAX_POOLS: usize = 16;
const MAX_BINDINGS: usize = 128;
const RELEASE_PROTOCOL: &str = "available_at_feed_completion_with_access_in_cell_handling/1";

/// Separately qualified common inventory. This declaration replaces independence only for the
/// explicitly bound cell resources; every other cell-local resource remains independent.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `shared_workholding_pools/1`; garments themselves remain in their assigned sewing cell.
    pub model: String,
    /// Exact protocol qualifying availability at feed completion and access in existing handling.
    /// No unobserved physical fixture transfer or return duration is inferred.
    pub release_protocol: String,
    /// Complete effective cell locks, including local fixture lifecycles and finally idle cells.
    pub cells: BTreeMap<String, String>,
    /// One to sixteen common pools, each with an independently qualified integer capacity.
    pub resources: Vec<workholding::Resource>,
    /// Each cell-local resource may draw from at most one pool. Unbound resources stay independent.
    pub bindings: Vec<Binding>,
    /// Evidence for the common inventory, interchangeability, access and release protocol.
    pub source: Source,
    /// Acquisition/qualification method, including what physical access and return were assessed.
    pub method: String,
    /// Explicit source applicability, without inferred live factory state.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Evidence class independent of local workholding and shared-operator qualification.
    pub evidence: Evidence,
    /// Separate explicit consent for estimated shared inventory or access applicability.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Exact existing cell-local resource identity supplied by a named common pool.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// Physical cell identity in the enclosing parallel policy.
    pub cell: String,
    /// Existing resource identity in that cell's qualified workholding policy.
    pub resource: String,
    /// Common pool identity in this policy's `resources`.
    pub pool: String,
}

/// One complete physical hold. Operation and hold identities stay local to the named cell; `copy`
/// maps the retained lifecycle to the actual global garment identity used by emitted stitches.
#[derive(Clone, Debug, Serialize)]
pub struct Hold {
    /// Owning physical sewing cell.
    pub cell: String,
    /// Global physical garment copy used by the actual stitch program.
    pub copy: usize,
    /// Local copy index in this cell's reconstructed ordinary production report.
    pub local_copy: usize,
    /// Exact local expanded hold identity, including its copy prefix.
    pub hold: String,
    /// Local resource identity qualified by the original workholding policy.
    pub resource: String,
    /// Common pool supplying these units.
    pub pool: String,
    /// Integer units held continuously between these source lifecycle boundaries.
    pub units: u32,
    /// Exact local scoped acquisition operation.
    pub acquire: String,
    /// Exact local scoped release operation, possibly equal to `acquire`.
    pub release: String,
    /// Common-origin start of the acquisition operation's qualified handling.
    pub acquired_at_milliseconds: u64,
    /// Common-origin completion of the release operation's complete scheduled feed.
    pub released_at_milliseconds: u64,
}

/// Complete pool lifecycles, independently replayed with releases before equal-time acquisitions.
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    /// Every bound source hold, sorted by cell and local expanded hold identity.
    pub holds: Vec<Hold>,
    /// Maximum simultaneous held units in each declared common pool.
    pub peak_occupancy: BTreeMap<String, u32>,
    /// Replayed completion state; every declared pool must return to zero.
    pub final_occupancy: BTreeMap<String, u32>,
}

/// Shared finite-resource enforcement on the exact baseline and selected operator timelines.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-shared-workholding/1`.
    pub schema: &'static str,
    /// Exact qualified release/access convention, without an inferred transfer delay.
    pub release_protocol: &'static str,
    /// Full admitted physical-cell source locks, including idle cells.
    pub cells: BTreeMap<String, String>,
    /// Exact qualified finite pool capacities.
    pub resources: Vec<workholding::Resource>,
    /// Existing cell-local resources supplied by these pools.
    pub bindings: Vec<Binding>,
    /// Complete retained baseline holds, including an explicitly replaced initial assignment.
    pub baseline: Timeline,
    /// Complete selected assignment's holds and integer capacity replay.
    pub selected: Timeline,
    /// Numerical inventory enforcement does not establish measured physical pool access.
    pub physical_pool_access_validated: bool,
}

impl Policy {
    pub(super) fn validate(&self, plan: &SewPlan, cells: &[&Cell]) -> Result<(), Refusal> {
        if !(1..=MAX_POOLS).contains(&self.resources.len())
            || !(1..=MAX_BINDINGS).contains(&self.bindings.len())
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "shared workholding supports 1..16 pools and 1..128 cell-resource bindings",
            ));
        }
        if self.model != "shared_workholding_pools/1"
            || self.release_protocol != RELEASE_PROTOCOL
            || !qualified_text(&self.source.reference)
            || !qualified_text(&self.source.accessed_on)
            || !qualified_text(&self.source.note)
            || !qualified_text(&self.method)
            || self.conditions.len() > 64
            || self
                .conditions
                .iter()
                .any(|(key, value)| !qualified_text(key) || !qualified_text(value))
        {
            return Err(bad(
                "invalid shared-workholding model, release protocol or source",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated shared workholding needs separate explicit consent",
            ));
        }
        if self.cells.len() != cells.len() {
            return Err(bad(
                "shared workholding needs every exact physical cell lock",
            ));
        }
        for cell in cells {
            let lock = self
                .cells
                .get(&cell.id)
                .ok_or_else(|| bad("shared workholding omits a physical cell lock"))?;
            if !sha256_text(lock) || *lock != operator::cell_sha256(cell, &plan.options)? {
                return Err(bad(
                    "shared workholding cell/recipe/timing content lock mismatch",
                ));
            }
        }
        let mut pools = BTreeSet::new();
        for resource in &self.resources {
            if !qualified_text(&resource.id)
                || !(1..=4096).contains(&resource.capacity)
                || !pools.insert(resource.id.as_str())
            {
                return Err(bad("invalid or duplicate shared-workholding pool"));
            }
        }
        let mut bindings = BTreeSet::new();
        let mut used = BTreeSet::new();
        for binding in &self.bindings {
            let cell = cells.iter().find(|cell| cell.id == binding.cell);
            if !pools.contains(binding.pool.as_str())
                || !bindings.insert((binding.cell.as_str(), binding.resource.as_str()))
                || cell
                    .and_then(|cell| cell.workholding.as_ref())
                    .is_none_or(|policy| {
                        !policy
                            .resources
                            .iter()
                            .any(|resource| resource.id == binding.resource)
                    })
            {
                return Err(bad(
                    "shared-workholding binding needs one existing cell resource and one known pool",
                ));
            }
            used.insert(binding.pool.as_str());
        }
        if used != pools {
            return Err(bad(
                "every shared-workholding pool needs an explicit cell-resource binding",
            ));
        }
        Ok(())
    }

    fn pool(&self, cell: &str, resource: &str) -> Option<usize> {
        self.bindings
            .iter()
            .find(|binding| binding.cell == cell && binding.resource == resource)
            .and_then(|binding| {
                self.resources
                    .iter()
                    .position(|pool| pool.id == binding.pool)
            })
    }

    pub(super) fn phases(
        &self,
        cell: &Cell,
        schedule: &super::super::Report,
    ) -> Result<Vec<Phase>, Refusal> {
        let mut result = vec![Phase::default(); schedule.operations.len() + 1];
        let Some(workholding) = &schedule.workholding else {
            return Ok(result);
        };
        let positions: BTreeMap<_, _> = schedule
            .ordering
            .selected_order
            .iter()
            .enumerate()
            .map(|(index, operation)| (operation.as_str(), index))
            .collect();
        for hold in &workholding.holds {
            let Some(pool) = self.pool(&cell.id, &hold.resource) else {
                continue;
            };
            let acquire = positions[hold.acquire.as_str()];
            let release = positions[hold.release.as_str()];
            result[acquire].acquire[pool] =
                result[acquire].acquire[pool]
                    .checked_add(hold.units)
                    .ok_or_else(|| bad("shared-workholding acquisition count overflows"))?;
            result[release].release[pool] =
                result[release].release[pool]
                    .checked_add(hold.units)
                    .ok_or_else(|| bad("shared-workholding release count overflows"))?;
        }
        Ok(result)
    }

    fn admits(&self, state: &State, phase: &Phase) -> bool {
        self.resources.iter().enumerate().all(|(pool, resource)| {
            state.occupied[pool]
                .checked_add(phase.acquire[pool])
                .is_some_and(|units| units <= resource.capacity)
        })
    }

    pub(super) fn earliest_start(
        &self,
        state: &State,
        phase: &Phase,
        earliest: u64,
    ) -> Result<Option<u64>, Refusal> {
        let mut trial = state.clone();
        let mut time = earliest;
        loop {
            trial.advance(time)?;
            if self.admits(&trial, phase) {
                return Ok(Some(time));
            }
            let Some(next) = trial.pending.iter().map(|pending| pending.at).min() else {
                // Persistent holds can be released only by another not-yet-dispatched operation.
                // This branch must attend that cell first; waiting alone cannot create inventory.
                return Ok(None);
            };
            time = next;
        }
    }

    pub(super) fn acquire(
        &self,
        state: &mut State,
        phase: &Phase,
        start: u64,
        finish: u64,
    ) -> Result<(), Refusal> {
        state.advance(start)?;
        if !self.admits(state, phase) {
            return Err(bad(
                "shared-workholding dispatch exceeds qualified capacity",
            ));
        }
        for (held, acquire) in state.occupied.iter_mut().zip(&phase.acquire) {
            *held += acquire;
        }
        if phase.release.iter().any(|&units| units != 0) {
            // Each cell has at most one feed in flight. Its next phase starts no earlier than its
            // previous finish, so every dispatch examines at most eight pending release events.
            if state.pending.len() >= super::MAX_CELLS {
                return Err(bad(
                    "shared-workholding pending releases exceed the cell bound",
                ));
            }
            state.pending.push(Pending {
                at: finish,
                release: phase.release,
            });
        }
        Ok(())
    }

    fn timeline(
        &self,
        assignment: &Assignment,
        operator: &operator::Timeline,
    ) -> Result<Timeline, Refusal> {
        let steps: BTreeMap<_, _> = operator
            .steps
            .iter()
            .filter_map(|step| {
                step.operation
                    .as_deref()
                    .map(|operation| ((step.cell.as_str(), operation), step))
            })
            .collect();
        let mut holds = Vec::new();
        for cell in &assignment.cells {
            let Some(workholding) = cell
                .schedule
                .as_ref()
                .and_then(|schedule| schedule.workholding.as_ref())
            else {
                continue;
            };
            for hold in &workholding.holds {
                let Some(pool) = self.pool(&cell.cell, &hold.resource) else {
                    continue;
                };
                let acquire = steps
                    .get(&(cell.cell.as_str(), hold.acquire.as_str()))
                    .ok_or_else(|| bad("shared hold acquisition omits its exact operator phase"))?;
                let release = steps
                    .get(&(cell.cell.as_str(), hold.release.as_str()))
                    .ok_or_else(|| bad("shared hold release omits its exact operator phase"))?;
                if release.feed_finish_milliseconds <= acquire.handling_start_milliseconds {
                    return Err(bad("shared hold must end after its acquisition starts"));
                }
                holds.push(Hold {
                    cell: cell.cell.clone(),
                    copy: *cell
                        .copies
                        .get(hold.copy)
                        .ok_or_else(|| bad("shared hold lacks a global garment copy"))?,
                    local_copy: hold.copy,
                    hold: hold.id.clone(),
                    resource: hold.resource.clone(),
                    pool: self.resources[pool].id.clone(),
                    units: hold.units,
                    acquire: hold.acquire.clone(),
                    release: hold.release.clone(),
                    acquired_at_milliseconds: acquire.handling_start_milliseconds,
                    released_at_milliseconds: release.feed_finish_milliseconds,
                });
            }
        }
        holds.sort_by(|left, right| (&left.cell, &left.hold).cmp(&(&right.cell, &right.hold)));
        let mut events = Vec::with_capacity(holds.len() * 2);
        for (index, hold) in holds.iter().enumerate() {
            events.push((hold.acquired_at_milliseconds, true, index));
            events.push((hold.released_at_milliseconds, false, index));
        }
        events.sort_unstable();
        let pools: BTreeMap<_, _> = self
            .resources
            .iter()
            .enumerate()
            .map(|(index, pool)| (pool.id.as_str(), index))
            .collect();
        let mut occupied = vec![0_u32; self.resources.len()];
        let mut peaks = occupied.clone();
        for (_, acquire, index) in events {
            let hold = &holds[index];
            let pool = pools[hold.pool.as_str()];
            occupied[pool] = if acquire {
                occupied[pool]
                    .checked_add(hold.units)
                    .filter(|&units| units <= self.resources[pool].capacity)
            } else {
                occupied[pool].checked_sub(hold.units)
            }
            .ok_or_else(|| bad("shared-workholding timeline fails exact finite-capacity replay"))?;
            peaks[pool] = peaks[pool].max(occupied[pool]);
        }
        if occupied.iter().any(|&units| units != 0) {
            return Err(bad(
                "shared-workholding timeline leaves unreleased inventory",
            ));
        }
        let named = |values: Vec<u32>| {
            self.resources
                .iter()
                .zip(values)
                .map(|(pool, units)| (pool.id.clone(), units))
                .collect()
        };
        Ok(Timeline {
            holds,
            peak_occupancy: named(peaks),
            final_occupancy: named(occupied),
        })
    }

    pub(super) fn report(
        &self,
        baseline: &Assignment,
        selected: &Assignment,
        operator: &operator::Report,
    ) -> Result<Report, Refusal> {
        Ok(Report {
            schema: "ftx-sewing-shared-workholding/1",
            release_protocol: RELEASE_PROTOCOL,
            cells: self.cells.clone(),
            resources: self.resources.clone(),
            bindings: self.bindings.clone(),
            baseline: self.timeline(baseline, &operator.baseline)?,
            selected: self.timeline(selected, &operator.selected)?,
            physical_pool_access_validated: false,
        })
    }
}

/// Only bounded integer changes are cached per phase; original holds stay in reconstructed reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Phase {
    acquire: [u32; MAX_POOLS],
    release: [u32; MAX_POOLS],
}

#[derive(Clone)]
struct Pending {
    at: u64,
    release: [u32; MAX_POOLS],
}

#[derive(Clone, Default)]
pub(super) struct State {
    occupied: [u32; MAX_POOLS],
    pending: Vec<Pending>,
}

impl State {
    fn advance(&mut self, time: u64) -> Result<(), Refusal> {
        for pending in &self.pending {
            if pending.at <= time {
                for (held, release) in self.occupied.iter_mut().zip(&pending.release) {
                    *held = held.checked_sub(*release).ok_or_else(|| {
                        bad("shared-workholding release exceeds retained inventory")
                    })?;
                }
            }
        }
        self.pending.retain(|pending| pending.at > time);
        Ok(())
    }

    pub(super) fn complete(&self, finish: u64) -> Result<(), Refusal> {
        let mut final_state = self.clone();
        final_state.advance(finish)?;
        if !final_state.pending.is_empty() || final_state.occupied.iter().any(|&units| units != 0) {
            return Err(bad(
                "complete shared-workholding dispatch leaves a hold unreleased",
            ));
        }
        Ok(())
    }
}
