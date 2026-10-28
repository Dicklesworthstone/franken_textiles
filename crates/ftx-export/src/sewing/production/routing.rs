//! Source-qualified specialist cells execute a complete operation route. Every operation is
//! loaded, attended and unloaded at its cell; a separate serialized resource moves unloaded garments.
//! Geometry and process admission remain with the ordinary sewing planner on the original pattern.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::Pattern;
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{parallel, workholding};
use crate::sewing::{
    MAX_STATIONS, SewOptions, SewPlan, bad, consumables, ordering, qualified_text, sha256_text,
    span,
};

pub mod fixtures;

const MAX_CELLS: usize = 8;
const MAX_OPERATIONS: usize = 63;
const MAX_WORK: usize = 200_000;
const MAX_EXACT_MILLISECONDS: u64 = 9_007_199_254_740_991;

/// A complete route for each physical garment through independently attended qualified cells.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `fixed_operation_route/1` or explicit `eligible_operation_route/1` machine alternatives.
    pub model: String,
    /// `minimize_makespan` over bounded copy dispatch and any declared eligible-cell choices.
    pub objective: String,
    /// `ceil_operation_feed_milliseconds/1`; each actual feed is ceiled separately.
    pub time_basis: String,
    /// `unloaded_between_operations/1`; every operation consumes its own load and unload evidence.
    pub protocol: String,
    /// `independent_attended_cells_and_transfer_resource/1`; each cell has dedicated attending labor.
    pub independence: String,
    /// Maximum examined dispatch extensions, including the complete copy-major seed attempt.
    pub max_search_nodes: usize,
    /// Qualified unloaded staging slots at each cell, at least the complete garment quantity.
    pub staging_capacity: usize,
    /// One to eight configured specialist cells. Every cell must own at least one route operation.
    pub cells: Vec<parallel::Cell>,
    /// Full cell/effective-template locks, using [`parallel::operator::cell_sha256`].
    pub cell_locks: BTreeMap<String, String>,
    /// Optional explicitly shared finite fixture inventory. Access/return must be qualified inside
    /// cell loading/unloading; occupied fixtures never travel with a routed garment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_workholding: Option<parallel::shared_workholding::Policy>,
    /// Every original operation exactly once, in an order respecting every original predecessor.
    pub route: Vec<RouteStep>,
    /// One independent resource for all unloaded inter-cell handoffs.
    pub transfer: TransferPolicy,
    /// Evidence for this complete route, dedicated cell labor, staging and handoff independence.
    pub source: Source,
    /// Applicability and phase protocol; no live factory state is inferred.
    pub method: String,
    /// Exact source conditions, retained without automatic extrapolation.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Evidence class, separate from each machine/process/timing observation.
    pub evidence: Evidence,
    /// Separate explicit consent for an estimated route/resource protocol.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// One original operation with an explicit seed cell and optional qualified alternatives.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteStep {
    /// Original pass ID, not a seam name or copy-scoped alias.
    pub operation: String,
    /// Physical cell ID used by the copy-major seed and the outer one-garment reference geometry.
    pub cell: String,
    /// Required only for `eligible_operation_route/1`: unique declared cells, including `cell`.
    /// The seed is searched first, followed by the other cells in this authored order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub eligible_cells: Vec<String>,
}

impl RouteStep {
    fn cells(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.cell.as_str()).chain(
            self.eligible_cells
                .iter()
                .map(String::as_str)
                .filter(|&cell| cell != self.cell),
        )
    }
}

/// An explicitly independent, serialized unloaded-garment handoff protocol.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferPolicy {
    /// Stable physical transfer-resource identity, distinct from all attending cell identities.
    pub id: String,
    /// `serialized_unloaded_handoffs/1`: each complete interval includes positioning, pickup,
    /// movement and deposit, returning to a source-qualified ready state before another handoff.
    pub model: String,
    /// Availability from the common job origin, at most one day.
    pub available_at_milliseconds: u32,
    /// Optional inclusive end; every complete handoff must finish inside this window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_until_milliseconds: Option<u64>,
    /// Exactly every directed inter-cell edge between adjacent eligible domains; same-cell steps
    /// need no handoff. Fixed routing has one cell per domain.
    pub transfers: Vec<Handoff>,
    /// Evidence for complete handoffs and their independence from attending cell workers.
    pub source: Source,
    /// Full phase definition, including any positioning/return work in each declared duration.
    pub method: String,
    /// Source applicability conditions.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Independent evidence classification for the transfer protocol.
    pub evidence: Evidence,
    /// Explicit consent for estimated handoff observations.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// One complete directed handoff duration, outside both cells' load/unload phases.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handoff {
    /// Cell at which the garment has finished unloading.
    pub from: String,
    /// Receiving cell with independently sufficient unloaded staging space.
    pub to: String,
    /// Full resource occupancy, at most one day. Explicit zero is admitted as supplied evidence.
    pub milliseconds: u32,
}

/// One complete operation on its actual cell-specific source geometry.
#[derive(Clone, Debug, Serialize)]
pub struct Operation {
    /// Exact global `copy/original_operation` identity.
    pub id: String,
    /// Zero-based physical garment copy.
    pub copy: usize,
    /// Original operation ID.
    pub operation_id: String,
    /// Physical specialist cell.
    pub cell: String,
    /// Index in this cell's retained source plan, not the outer merged seam table.
    pub seam_index: usize,
    /// Index in the fixed complete route.
    pub route_index: usize,
    /// Start of exclusive loading, after garment arrival and previous cell unloading.
    pub loading_start_milliseconds: u64,
    /// Loading completes and the admitted needle/thread setup begins.
    pub setup_start_milliseconds: u64,
    /// Setup completes and fully attended feed begins.
    pub feed_start_milliseconds: u64,
    /// End of the separately ceiled feed; exclusive unloading starts here.
    pub feed_finish_milliseconds: u64,
    /// Unloading completes; cell and its attending worker are free and the garment may transfer.
    pub finish_milliseconds: u64,
}

/// One complete exclusive movement of an already unloaded physical garment.
#[derive(Clone, Debug, Serialize)]
pub struct Transfer {
    /// Zero-based physical garment copy.
    pub copy: usize,
    /// Completed original operation at the source cell.
    pub from_operation: String,
    /// Next original operation at the destination cell.
    pub to_operation: String,
    /// Source physical cell.
    pub from: String,
    /// Destination physical cell.
    pub to: String,
    /// Begins after source unloading and the previous complete handoff.
    pub start_milliseconds: u64,
    /// The garment is staged at the destination and the transfer resource is ready again.
    pub finish_milliseconds: u64,
}

/// A complete feasible batch execution, with independent cells and one exclusive transfer resource.
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    /// Dispatch order; independent cell phases may overlap and need not start in this list's order.
    pub operations: Vec<Operation>,
    /// Serialized handoffs in transfer-resource order.
    pub transfers: Vec<Transfer>,
    /// Final unloaded garment completion from the common job origin.
    pub makespan_milliseconds: u64,
    /// Actual continuous feed, before conservative millisecond ceiling.
    pub feed_seconds: f64,
    /// Sum of the independently ceiled actual operation feeds.
    pub scheduled_feed_milliseconds: u64,
    /// Sum of qualified setup work across independent cells, not elapsed time.
    pub setup_milliseconds: u64,
    /// Sum of every operation's source-qualified loading and unloading.
    pub handling_milliseconds: u64,
    /// Complete occupancy of the one independently qualified handoff resource.
    pub transfer_milliseconds: u64,
}

/// Source-qualified fixed routing with an exact complete incumbent and bounded dispatch search.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `/1` fixed route; `/2` eligible cells; `/3` qualified local/shared fixture reservations.
    pub schema: &'static str,
    /// Source-qualified fixed operation route.
    pub route: Vec<RouteStep>,
    /// With eligible cells, the outer plan's seams/count/feed retain one seed-route garment only.
    /// Actual execution lives in selected operations, cell source plans and production batch totals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_scope: Option<&'static str>,
    /// Actual specialist source plans on the unchanged original pattern; each owns only its passes.
    pub cell_plans: Vec<parallel::CellPlan>,
    /// Optional complete local/shared fixture capacities and independently swept physical holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workholding: Option<fixtures::Report>,
    /// Complete copy-major seed, or selected replacement if resources/windows excluded that seed.
    pub baseline: Timeline,
    /// Best complete feasible dispatch found within the admitted budget.
    pub selected: Timeline,
    /// Whether the baseline had to be replaced by a complete feasible searched dispatch.
    pub baseline_replaced: bool,
    /// Examined dispatch extensions, including failed timing attempts and the initial seed.
    pub search_nodes: usize,
    /// Every improving dispatch in the stated search scope was explored or validly pruned.
    pub search_complete: bool,
    /// The incumbent reaches an independent mandatory-work lower bound for the fixed route.
    /// Exhaustion of the restricted dispatch search alone does not establish this flag.
    pub optimality_proven: bool,
    /// The complete incumbent is retained after an exhausted dispatch budget.
    pub budget_exhausted: bool,
    /// Independent lower bound from mandatory per-cell work, garment path and handoff-resource work.
    pub lower_bound_makespan_milliseconds: u64,
    /// `coupled_handoff_cell_append_dispatch/1`; does not enumerate arbitrary prestaging/reordering.
    pub search_scope: &'static str,
    /// Deterministic event evaluations reserved for exact baseline/selected reconstruction.
    pub replay_operations: usize,
    /// Search extensions plus the two complete replay evaluations; within the reserved work bound.
    pub work_used: usize,
    /// Declared process timing does not establish measured physical factory execution.
    pub physical_routing_validated: bool,
}

fn budget(message: &str) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}

fn source_valid(source: &Source, method: &str, conditions: &BTreeMap<String, String>) -> bool {
    qualified_text(&source.reference)
        && qualified_text(&source.accessed_on)
        && qualified_text(&source.note)
        && qualified_text(method)
        && conditions.len() <= 64
        && conditions
            .iter()
            .all(|(key, value)| qualified_text(key) && qualified_text(value))
}

fn window_valid(start: u32, end: Option<u64>) -> bool {
    start <= 86_400_000
        && end.is_none_or(|end| end >= u64::from(start) && end <= MAX_EXACT_MILLISECONDS)
}

fn add(a: u64, b: u64) -> Result<u64, Refusal> {
    a.checked_add(b)
        .filter(|&sum| sum <= MAX_EXACT_MILLISECONDS)
        .ok_or_else(|| budget("routed time exceeds exact native/browser integer milliseconds"))
}

impl Policy {
    pub(crate) fn eligible(&self) -> bool {
        self.model == "eligible_operation_route/1"
    }

    /// Reserve bounded search and two complete exact replays, independent of whether a seed is valid.
    ///
    /// # Errors
    /// Invalid quantity/cell/route bounds, an unreservable seed, or more than 200000 total work units.
    pub fn work_bound(&self, copies: usize, operations: usize) -> Result<usize, Refusal> {
        let expanded = copies
            .checked_mul(operations)
            .filter(|&count| count > 0 && count <= MAX_OPERATIONS)
            .ok_or_else(|| budget("routing supports 1..63 expanded garment operations"))?;
        if !(1..=MAX_CELLS).contains(&self.cells.len())
            || self.route.len() != operations
            || self
                .route
                .iter()
                .any(|step| step.eligible_cells.len() > MAX_CELLS)
            || !(expanded..=MAX_WORK).contains(&self.max_search_nodes)
        {
            return Err(budget(
                "routing needs 1..8 used cells, a complete route and work for one complete seed",
            ));
        }
        self.max_search_nodes
            .checked_add(2 * expanded)
            .filter(|&work| work <= MAX_WORK)
            .ok_or_else(|| budget("routing search and complete replays exceed 200000 work units"))
    }

    fn validate(&self, options: &SewOptions, copies: usize) -> Result<BTreeSet<String>, Refusal> {
        self.work_bound(copies, options.passes.len())?;
        let independence = if self.shared_workholding.is_some() {
            "independent_attended_cells_shared_workholding_and_transfer_resource/1"
        } else {
            "independent_attended_cells_and_transfer_resource/1"
        };
        if !matches!(
            self.model.as_str(),
            "fixed_operation_route/1" | "eligible_operation_route/1"
        ) || self.objective != "minimize_makespan"
            || self.time_basis != "ceil_operation_feed_milliseconds/1"
            || self.protocol != "unloaded_between_operations/1"
            || self.independence != independence
            || self.staging_capacity < copies
            || self.staging_capacity > 4096
            || !source_valid(&self.source, &self.method, &self.conditions)
        {
            return Err(bad(
                "invalid routing model, independent resources, source or complete staging capacity",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated routing applicability needs its own explicit consent",
            ));
        }
        let mut cells = BTreeSet::new();
        for cell in &self.cells {
            if !qualified_text(&cell.id)
                || !cells.insert(cell.id.as_str())
                || !window_valid(
                    cell.available_at_milliseconds,
                    cell.available_until_milliseconds,
                )
                || cell.configuration.is_none()
                || cell.ordering.objective != ordering::Objective::MinimizeProductionTime
                || !cell.handling.transitions.is_empty()
            {
                return Err(bad(
                    "routing needs unique configured attended cells, valid windows, and per-operation loading/unloading without serial handling transitions",
                ));
            }
            let lock = self
                .cell_locks
                .get(&cell.id)
                .ok_or_else(|| bad("routing omits a full specialist cell lock"))?;
            if !sha256_text(lock) || *lock != parallel::operator::cell_sha256(cell, options)? {
                return Err(bad(
                    "routing cell configuration, template recipe or phase timing content lock mismatch",
                ));
            }
        }
        if self.cell_locks.len() != cells.len() {
            return Err(bad(
                "routing cell locks must name exactly the declared physical cells",
            ));
        }
        let mut operations = BTreeMap::new();
        for pass in &options.passes {
            if !qualified_text(pass.operation_id())
                || operations.insert(pass.operation_id(), pass).is_some()
            {
                return Err(bad("routing needs unique nonempty original operation IDs"));
            }
        }
        let mut done = BTreeSet::new();
        let mut used = BTreeSet::new();
        for step in &self.route {
            let eligible: BTreeSet<_> = step.eligible_cells.iter().map(String::as_str).collect();
            if if self.eligible() {
                eligible.is_empty()
                    || eligible.len() != step.eligible_cells.len()
                    || !eligible.contains(step.cell.as_str())
                    || !eligible.is_subset(&cells)
            } else {
                !eligible.is_empty()
            } {
                return Err(bad(
                    "eligible routing needs unique declared cells including each seed; fixed routing forbids eligible lists",
                ));
            }
            let pass = operations
                .get(step.operation.as_str())
                .ok_or_else(|| bad("route names an unknown original operation"))?;
            let predecessors: BTreeSet<_> = pass.after.iter().collect();
            if !cells.contains(step.cell.as_str())
                || done.contains(&step.operation)
                || predecessors.len() != pass.after.len()
                || pass.after.iter().any(|id| !done.contains(id))
            {
                return Err(bad(
                    "fixed route must name each operation once in predecessor order on a declared cell",
                ));
            }
            done.insert(step.operation.clone());
            used.extend(step.cells());
        }
        if done.len() != operations.len() || used != cells {
            return Err(bad(
                "fixed route must cover every operation and every configured specialist cell",
            ));
        }
        let transfer = &self.transfer;
        if !qualified_text(&transfer.id)
            || cells.contains(transfer.id.as_str())
            || transfer.model != "serialized_unloaded_handoffs/1"
            || !window_valid(
                transfer.available_at_milliseconds,
                transfer.available_until_milliseconds,
            )
            || !source_valid(&transfer.source, &transfer.method, &transfer.conditions)
            || transfer.transfers.len() > MAX_OPERATIONS
        {
            return Err(bad(
                "invalid independent serialized handoff resource, source or availability",
            ));
        }
        if transfer.evidence == Evidence::Estimate && !transfer.allow_estimates {
            return Err(bad(
                "estimated unloaded handoffs need their own explicit consent",
            ));
        }
        let required: BTreeSet<_> = self
            .route
            .windows(2)
            .flat_map(|pair| {
                pair[0].cells().flat_map(move |from| {
                    pair[1]
                        .cells()
                        .filter(move |&to| from != to)
                        .map(move |to| (from, to))
                })
            })
            .collect();
        let mut supplied = BTreeSet::new();
        for row in &transfer.transfers {
            if row.from == row.to
                || row.milliseconds > 86_400_000
                || !supplied.insert((row.from.as_str(), row.to.as_str()))
            {
                return Err(bad("invalid or duplicate directed unloaded handoff"));
            }
        }
        if supplied != required {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                "source-qualified handoffs must cover exactly the inter-cell edges of every adjacent route domain",
            ));
        }
        Ok(done)
    }
}

/// Prepare only the operations assigned to this specialist, preserving original dependency IDs.
/// The caller still performs ordinary source geometry/process admission and global route validation.
pub(crate) fn source_options(
    cell: &parallel::Cell,
    parent: &SewOptions,
    route: &[RouteStep],
) -> Result<SewOptions, Refusal> {
    let assigned: BTreeSet<_> = route
        .iter()
        .filter(|step| step.cells().any(|id| id == cell.id))
        .map(|step| step.operation.as_str())
        .collect();
    let configuration = cell
        .configuration
        .as_ref()
        .ok_or_else(|| bad("a routed specialist needs an explicit configured machine"))?;
    if assigned.is_empty()
        || configuration
            .recipes
            .keys()
            .chain(configuration.access.keys())
            .chain(configuration.qualification_refs.keys())
            .any(|id| !assigned.contains(id.as_str()))
    {
        return Err(bad(
            "routed specialist overrides and qualification locks must name only its assigned operations",
        ));
    }
    let mut source = cell
        .configured_options(parent)?
        .ok_or_else(|| bad("missing routed cell configuration"))?;
    source
        .passes
        .retain(|pass| assigned.contains(pass.operation_id()));
    Ok(source)
}

#[derive(Clone)]
struct Task {
    route: usize,
    cell: usize,
    seam_index: usize,
    setup: ordering::Setup,
    load: u64,
    unload: u64,
    feed: u64,
    feed_seconds: f64,
}

struct Prepared<'a> {
    policy: &'a Policy,
    tasks: Vec<Task>,
    /// Task indices by route position, seed first then remaining authored eligible cells.
    choices: Vec<Vec<usize>>,
    /// Task-index transition costs; the last row represents each cell's own initial setup.
    setup: Vec<Vec<u64>>,
    handoffs: Vec<Vec<Option<u64>>>,
    copies: usize,
    fixtures: Option<fixtures::Compiled>,
}

#[derive(Clone)]
struct State {
    next: Vec<usize>,
    copy_ready: Vec<u64>,
    copy_cell: Vec<Option<usize>>,
    cell_ready: Vec<u64>,
    last: Vec<Option<usize>>,
    transfer_ready: u64,
    makespan: u64,
    reservations: Vec<fixtures::Reservation>,
}

#[derive(Clone, Copy)]
struct Event {
    task: usize,
    load_start: u64,
    setup_start: u64,
    feed_start: u64,
    feed_finish: u64,
    finish: u64,
    handoff: Option<[u64; 2]>,
}

#[derive(Clone, Copy)]
struct Dispatch {
    copy: usize,
    task: usize,
}

impl Prepared<'_> {
    fn initial(&self) -> State {
        State {
            next: vec![0; self.copies],
            copy_ready: vec![0; self.copies],
            copy_cell: vec![None; self.copies],
            cell_ready: self
                .policy
                .cells
                .iter()
                .map(|cell| u64::from(cell.available_at_milliseconds))
                .collect(),
            last: vec![None; self.policy.cells.len()],
            transfer_ready: u64::from(self.policy.transfer.available_at_milliseconds),
            makespan: 0,
            reservations: Vec::new(),
        }
    }

    fn advance(
        &self,
        state: &State,
        dispatch: Dispatch,
    ) -> Result<Option<(State, Event)>, Refusal> {
        let Dispatch { copy, task: index } = dispatch;
        let task = &self.tasks[index];
        if state.next[copy] != task.route {
            return Err(bad("routed dispatch changed its original operation order"));
        }
        let mut ready = state.copy_ready[copy];
        let duration = state.copy_cell[copy].and_then(|from| self.handoffs[from][task.cell]);
        let handoff = if let Some(duration) = duration {
            let start = ready.max(state.transfer_ready);
            let finish = add(start, duration)?;
            if self
                .policy
                .transfer
                .available_until_milliseconds
                .is_some_and(|end| finish > end)
            {
                return Ok(None);
            }
            ready = finish;
            Some([start, finish])
        } else {
            None
        };
        let from = state.last[task.cell].unwrap_or(self.tasks.len());
        let duration = add(
            add(add(task.load, self.setup[from][index])?, task.feed)?,
            task.unload,
        )?;
        let earliest = ready.max(state.cell_ready[task.cell]);
        let load_start = if let Some(fixtures) = &self.fixtures {
            fixtures.earliest_start(
                &state.reservations,
                task.route,
                task.cell,
                earliest,
                duration,
            )?
        } else {
            earliest
        };
        let setup_start = add(load_start, task.load)?;
        let feed_start = add(setup_start, self.setup[from][index])?;
        let feed_finish = add(feed_start, task.feed)?;
        let finish = add(feed_finish, task.unload)?;
        if self.policy.cells[task.cell]
            .available_until_milliseconds
            .is_some_and(|end| finish > end)
        {
            return Ok(None);
        }
        let mut next = state.clone();
        next.next[copy] += 1;
        next.copy_ready[copy] = finish;
        next.copy_cell[copy] = Some(task.cell);
        next.cell_ready[task.cell] = finish;
        next.last[task.cell] = Some(index);
        if let Some([_, finish]) = handoff {
            next.transfer_ready = finish;
        }
        next.makespan = next.makespan.max(finish);
        if let Some(fixtures) = &self.fixtures {
            fixtures.reserve(
                &mut next.reservations,
                task.route,
                task.cell,
                load_start,
                finish,
            );
        }
        Ok(Some((
            next,
            Event {
                task: index,
                load_start,
                setup_start,
                feed_start,
                feed_finish,
                finish,
                handoff,
            },
        )))
    }

    fn replay(&self, dispatch: &[Dispatch]) -> Result<Timeline, Refusal> {
        let mut state = self.initial();
        let mut timeline = Timeline {
            operations: Vec::with_capacity(dispatch.len()),
            transfers: Vec::new(),
            makespan_milliseconds: 0,
            feed_seconds: 0.0,
            scheduled_feed_milliseconds: 0,
            setup_milliseconds: 0,
            handling_milliseconds: 0,
            transfer_milliseconds: 0,
        };
        for &dispatch in dispatch {
            let copy = dispatch.copy;
            let (next, event) = self
                .advance(&state, dispatch)?
                .ok_or_else(|| bad("a retained routed dispatch failed exact reconstruction"))?;
            let task = &self.tasks[event.task];
            let step = &self.policy.route[task.route];
            let cell = &self.policy.cells[task.cell].id;
            timeline.operations.push(Operation {
                id: format!("{copy}/{}", step.operation),
                copy,
                operation_id: step.operation.clone(),
                cell: cell.clone(),
                seam_index: task.seam_index,
                route_index: task.route,
                loading_start_milliseconds: event.load_start,
                setup_start_milliseconds: event.setup_start,
                feed_start_milliseconds: event.feed_start,
                feed_finish_milliseconds: event.feed_finish,
                finish_milliseconds: event.finish,
            });
            if let Some([start, finish]) = event.handoff {
                let previous = &self.policy.route[task.route - 1];
                timeline.transfers.push(Transfer {
                    copy,
                    from_operation: previous.operation.clone(),
                    to_operation: step.operation.clone(),
                    from: self.policy.cells
                        [state.copy_cell[copy].expect("previous operation cell")]
                    .id
                    .clone(),
                    to: cell.clone(),
                    start_milliseconds: start,
                    finish_milliseconds: finish,
                });
                timeline.transfer_milliseconds =
                    add(timeline.transfer_milliseconds, finish - start)?;
            }
            timeline.feed_seconds += task.feed_seconds;
            timeline.scheduled_feed_milliseconds =
                add(timeline.scheduled_feed_milliseconds, task.feed)?;
            timeline.setup_milliseconds = add(
                timeline.setup_milliseconds,
                event.feed_start - event.setup_start,
            )?;
            timeline.handling_milliseconds =
                add(timeline.handling_milliseconds, add(task.load, task.unload)?)?;
            state = next;
        }
        if state.next.iter().any(|&next| next != self.choices.len())
            || !timeline.feed_seconds.is_finite()
        {
            return Err(bad(
                "routed reconstruction is incomplete or has nonfinite feed",
            ));
        }
        timeline.makespan_milliseconds = state.makespan;
        Ok(timeline)
    }

    fn lower_bound(&self) -> Result<u64, Refusal> {
        if self.policy.eligible() {
            return self.eligible_lower_bound();
        }
        let mut path = 0;
        let mut transfer = 0;
        let mut cell_work = vec![0u64; self.policy.cells.len()];
        for task in &self.tasks {
            let occupied = add(add(task.load, task.feed)?, task.unload)?;
            let handoff = task
                .route
                .checked_sub(1)
                .and_then(|previous| self.handoffs[self.tasks[previous].cell][task.cell]);
            path = add(path, add(occupied, handoff.unwrap_or(0))?)?;
            cell_work[task.cell] = add(cell_work[task.cell], occupied * self.copies as u64)?;
            transfer = add(transfer, handoff.unwrap_or(0) * self.copies as u64)?;
        }
        let mut lower = add(
            path,
            u64::from(self.policy.cells[self.tasks[0].cell].available_at_milliseconds),
        )?;
        if self
            .tasks
            .windows(2)
            .any(|pair| pair[0].cell != pair[1].cell)
        {
            lower = lower.max(add(
                transfer,
                u64::from(self.policy.transfer.available_at_milliseconds),
            )?);
        }
        for (cell, work) in cell_work.into_iter().enumerate() {
            let mut unique = BTreeMap::new();
            for (route, task) in self
                .tasks
                .iter()
                .enumerate()
                .filter(|(_, task)| task.cell == cell)
            {
                unique.entry(&task.setup).or_insert(route);
            }
            let setups: Vec<_> = unique.into_values().collect();
            // Every distinct setup has a first entry. Choose a hypothetical first one, then relax
            // each other's incoming edge independently; disconnected cycles only lower this bound.
            let mut setup_bound = u64::MAX;
            for &first in &setups {
                let mut bound = self.setup[self.tasks.len()][first];
                for &to in setups.iter().filter(|&&to| to != first) {
                    let incoming = setups
                        .iter()
                        .filter(|&&from| from != to)
                        .map(|&from| self.setup[from][to])
                        .min()
                        .expect("another distinct setup");
                    bound = add(bound, incoming)?;
                }
                setup_bound = setup_bound.min(bound);
            }
            lower = lower.max(add(
                add(work, setup_bound)?,
                u64::from(self.policy.cells[cell].available_at_milliseconds),
            )?);
        }
        Ok(lower)
    }

    fn eligible_lower_bound(&self) -> Result<u64, Refusal> {
        let mut path = 0;
        let mut transfer = 0;
        let mut total_work = 0;
        let mut forced_work = vec![0u64; self.policy.cells.len()];
        for (route, choices) in self.choices.iter().enumerate() {
            let occupied = choices
                .iter()
                .map(|&index| {
                    let task = &self.tasks[index];
                    add(add(task.load, task.feed)?, task.unload)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let minimum = *occupied.iter().min().expect("nonempty admitted domain");
            path = add(path, minimum)?;
            total_work = add(total_work, minimum * self.copies as u64)?;
            if choices.len() == 1 {
                let cell = self.tasks[choices[0]].cell;
                forced_work[cell] = add(forced_work[cell], minimum * self.copies as u64)?;
            }
            if route > 0 {
                // Independent minima may choose incompatible cells at consecutive stages. That
                // relaxation only lowers mandatory work; it is not a proposed feasible path.
                let duration = self.choices[route - 1]
                    .iter()
                    .flat_map(|&from| {
                        choices.iter().map(move |&to| {
                            self.handoffs[self.tasks[from].cell][self.tasks[to].cell].unwrap_or(0)
                        })
                    })
                    .min()
                    .expect("nonempty adjacent domains");
                path = add(path, duration)?;
                transfer = add(transfer, duration * self.copies as u64)?;
            }
        }
        let first = self.choices[0]
            .iter()
            .map(|&index| {
                u64::from(self.policy.cells[self.tasks[index].cell].available_at_milliseconds)
            })
            .min()
            .expect("nonempty initial domain");
        let mut lower = add(first, path)?.max(total_work.div_ceil(self.policy.cells.len() as u64));
        if transfer > 0 {
            lower = lower.max(add(
                transfer,
                u64::from(self.policy.transfer.available_at_milliseconds),
            )?);
        }
        for (cell, work) in forced_work
            .into_iter()
            .enumerate()
            .filter(|&(_, work)| work > 0)
        {
            lower = lower.max(add(
                work,
                u64::from(self.policy.cells[cell].available_at_milliseconds),
            )?);
        }
        // No setup is mandatory on an optional cell. Even forced setups can be entered through
        // optional intermediate recipes, so fixed-route setup bounds are deliberately not reused.
        Ok(lower)
    }
}

struct Search<'a, 'b> {
    prepared: &'a Prepared<'b>,
    used: usize,
    complete: bool,
    lower: u64,
    best: Option<(u64, Vec<Dispatch>)>,
}

impl Search<'_, '_> {
    fn visit(&mut self, state: &State, path: &mut Vec<Dispatch>) -> Result<(), Refusal> {
        if path.len() == self.prepared.copies * self.prepared.choices.len() {
            if self
                .best
                .as_ref()
                .is_none_or(|(finish, _)| state.makespan < *finish)
            {
                self.best = Some((state.makespan, path.clone()));
            }
            return Ok(());
        }
        if self
            .best
            .as_ref()
            .is_some_and(|(finish, _)| state.makespan >= *finish || *finish == self.lower)
        {
            return Ok(());
        }
        for copy in 0..self.prepared.copies {
            if state.next[copy] == self.prepared.choices.len() {
                continue;
            }
            for &task in &self.prepared.choices[state.next[copy]] {
                if self.used == self.prepared.policy.max_search_nodes {
                    self.complete = false;
                    return Ok(());
                }
                self.used += 1;
                let dispatch = Dispatch { copy, task };
                if let Some((next, _)) = self.prepared.advance(state, dispatch)? {
                    path.push(dispatch);
                    self.visit(&next, path)?;
                    path.pop();
                }
                if self
                    .best
                    .as_ref()
                    .is_some_and(|(finish, _)| *finish == self.lower)
                {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

fn setup_duration(
    times: &ordering::SetupTimes,
    from: &ordering::Setup,
    to: &ordering::Setup,
) -> u64 {
    if from == to {
        0
    } else {
        u64::from(
            times
                .transitions
                .iter()
                .find(|row| &row.from == from && &row.to == to)
                .expect("complete admitted setup domain")
                .milliseconds,
        )
    }
}

fn changes(policy: &Policy, tasks: &[Task], timeline: &Timeline) -> ordering::Changes {
    let mut last: Vec<_> = policy
        .cells
        .iter()
        .map(|cell| {
            cell.ordering
                .initial_setup
                .as_ref()
                .expect("qualified initial setup")
                .clone()
        })
        .collect();
    let mut counts = ordering::Changes::default();
    for operation in &timeline.operations {
        let task = tasks
            .iter()
            .find(|task| {
                task.route == operation.route_index && policy.cells[task.cell].id == operation.cell
            })
            .expect("retained assigned task");
        counts.needle_changes += usize::from(last[task.cell].needle != task.setup.needle);
        counts.thread_changes += usize::from(last[task.cell].thread != task.setup.thread);
        last[task.cell] = task.setup.clone();
    }
    counts.total_changes = counts.needle_changes + counts.thread_changes;
    counts
}

/// Build a complete routed job without requiring a fictitious machine that qualifies every pass.
pub(in crate::sewing) fn build(
    pattern: &Pattern,
    options: &SewOptions,
) -> Result<SewPlan, Refusal> {
    consumables::purchasing_work_bound(options)?;
    let production = options.production.as_ref().expect("routing production");
    let policy = production.routing.as_ref().expect("routing policy");
    let copies = production
        .production_copies
        .expect("resolved production count");
    let global = policy.validate(options, copies)?;
    let source_seams: BTreeSet<_> = pattern
        .seams()
        .iter()
        .map(|seam| {
            seam.id()
                .ok_or_else(|| bad("every routed manufacturing seam needs an ID"))
        })
        .collect::<Result<_, _>>()?;
    let covered: BTreeSet<_> = options
        .passes
        .iter()
        .map(|pass| pass.seam.as_str())
        .collect();
    if covered != source_seams {
        return Err(bad(
            "routed operations must cover every original pattern seam and no unknown seams",
        ));
    }
    for id in &source_seams {
        span::require_coverage(
            id,
            "a",
            options
                .passes
                .iter()
                .filter(|pass| pass.seam == *id)
                .map(|pass| pass.source_range.unwrap_or([0.0, 1.0]))
                .collect(),
        )?;
    }
    let mut cell_plans = Vec::with_capacity(policy.cells.len());
    let mut stations = 0usize;
    for cell in &policy.cells {
        let source = source_options(cell, options, &policy.route)?;
        let plan = SewPlan::build_routing_subset(pattern, &source, &global)?;
        let configuration = cell.configuration.as_ref().expect("configured specialist");
        cell.ordering
            .validate_production(plan.seams.len() * copies, &source.machine.id)?;
        let choices = plan
            .seams
            .iter()
            .map(|seam| ordering::Setup {
                needle: seam.pass.needle.clone(),
                thread: seam.pass.thread.clone(),
            })
            .collect();
        cell.ordering
            .setup_times
            .as_ref()
            .expect("timed ordering")
            .check_choices(
                &source.machine.id,
                &choices,
                cell.ordering
                    .initial_setup
                    .as_ref()
                    .expect("timed initial setup"),
            )?;
        let qualifications =
            parallel::qualification_refs(&plan, &configuration.qualification_refs)?;
        cell.handling.validate(&plan)?;
        stations = stations.saturating_add(
            plan.seams
                .iter()
                .map(|seam| seam.stations.a.len())
                .sum::<usize>(),
        );
        if stations.saturating_mul(2) > MAX_STATIONS {
            return Err(budget(
                "routed specialist and merged source station tables exceed 250000 slots",
            ));
        }
        cell_plans.push(parallel::CellPlan {
            cell: cell.id.clone(),
            machine_sha256: workholding::machine_sha256(&source.machine)?,
            qualifications,
            plan: Box::new(plan),
        });
    }
    let fixtures = fixtures::Compiled::compile(policy, options, &cell_plans, copies)?;
    let uses_fixtures = fixtures.is_some();
    let mut out = SewPlan {
        schema: if uses_fixtures {
            "ftx-sewing-plan/20"
        } else if policy.eligible() {
            "ftx-sewing-plan/19"
        } else {
            "ftx-sewing-plan/18"
        },
        pattern_sha256: cell_plans[0].plan.pattern_sha256.clone(),
        options: options.clone(),
        materials: BTreeMap::new(),
        seams: Vec::with_capacity(policy.route.len()),
        ordering: None,
        production: None,
        consumables: None,
        stitch_intervals: 0,
        feed_seconds: 0.0,
        hardware_execution_authorized: false,
        physical_seam_quality_validated: false,
    };
    let mut tasks = Vec::with_capacity(policy.route.len());
    let mut choices = Vec::with_capacity(policy.route.len());
    let mut maximum_intervals = 0usize;
    for (route_index, step) in policy.route.iter().enumerate() {
        let mut domain = Vec::new();
        let mut maximum = 0;
        for cell_id in step.cells() {
            let cell_index = policy
                .cells
                .iter()
                .position(|cell| cell.id == cell_id)
                .expect("admitted cell");
            let cell = &policy.cells[cell_index];
            let source = &cell_plans[cell_index].plan;
            let seam_index = source
                .seams
                .iter()
                .position(|seam| seam.pass.operation_id() == step.operation)
                .ok_or_else(|| bad("routed specialist source omits its assigned operation"))?;
            let seam = &source.seams[seam_index];
            let ceiled_feed = (seam.feed_seconds * 1000.0).ceil();
            if !ceiled_feed.is_finite()
                || ceiled_feed <= 0.0
                || ceiled_feed > MAX_EXACT_MILLISECONDS as f64
            {
                return Err(budget(
                    "routed operation feed cannot be represented in exact milliseconds",
                ));
            }
            domain.push(tasks.len());
            tasks.push(Task {
                route: route_index,
                cell: cell_index,
                seam_index,
                setup: ordering::Setup {
                    needle: seam.pass.needle.clone(),
                    thread: seam.pass.thread.clone(),
                },
                load: u64::from(cell.handling.initial_milliseconds[&step.operation]),
                unload: u64::from(cell.handling.final_milliseconds[&step.operation]),
                feed: ceiled_feed as u64,
                feed_seconds: seam.feed_seconds,
            });
            maximum = maximum.max(seam.stitch_intervals);
            out.materials.extend(source.materials.clone());
            if cell_id == step.cell {
                out.stitch_intervals = out
                    .stitch_intervals
                    .checked_add(seam.stitch_intervals)
                    .ok_or_else(|| budget("routed stitch count overflows"))?;
                out.feed_seconds += seam.feed_seconds;
                out.seams.push(seam.clone());
            }
        }
        maximum_intervals = maximum_intervals
            .checked_add(maximum)
            .ok_or_else(|| budget("eligible routed stitch count overflows"))?;
        choices.push(domain);
    }
    if maximum_intervals
        .checked_mul(copies)
        .is_none_or(|count| count > MAX_STATIONS)
        || !out.feed_seconds.is_finite()
    {
        return Err(budget(
            "complete routed garment batch, including every eligible choice, exceeds the 250000 stitch-interval bound",
        ));
    }
    for id in source_seams {
        span::require_coverage(
            id,
            "b",
            out.seams
                .iter()
                .filter(|seam| seam.pass.seam == id)
                .map(|seam| {
                    seam.source_span
                        .as_ref()
                        .map_or([0.0, 1.0], |span| span.source_ranges[1])
                })
                .collect(),
        )?;
    }
    let count = policy.route.len();
    let task_count = tasks.len();
    let mut setup = vec![vec![0; task_count]; task_count + 1];
    for (to, task) in tasks.iter().enumerate() {
        let cell = &policy.cells[task.cell];
        let times = cell
            .ordering
            .setup_times
            .as_ref()
            .expect("qualified setup timing");
        setup[task_count][to] = setup_duration(
            times,
            cell.ordering.initial_setup.as_ref().expect("initial setup"),
            &task.setup,
        );
        for (from, previous) in tasks
            .iter()
            .enumerate()
            .filter(|(_, previous)| previous.cell == task.cell)
        {
            setup[from][to] = setup_duration(times, &previous.setup, &task.setup);
        }
    }
    let mut handoffs = vec![vec![None; policy.cells.len()]; policy.cells.len()];
    for row in &policy.transfer.transfers {
        let from = policy
            .cells
            .iter()
            .position(|cell| cell.id == row.from)
            .expect("admitted source cell");
        let to = policy
            .cells
            .iter()
            .position(|cell| cell.id == row.to)
            .expect("admitted receiving cell");
        handoffs[from][to] = Some(u64::from(row.milliseconds));
    }
    let prepared = Prepared {
        policy,
        tasks,
        choices,
        setup,
        handoffs,
        copies,
        fixtures,
    };
    let lower = prepared.lower_bound()?;
    let mut seed = prepared.initial();
    let mut seed_path = Vec::with_capacity(copies * count);
    let mut seed_used = 0;
    'seed: for copy in 0..copies {
        for _ in 0..count {
            seed_used += 1;
            let dispatch = Dispatch {
                copy,
                task: prepared.choices[seed.next[copy]][0],
            };
            let Some((next, _)) = prepared.advance(&seed, dispatch)? else {
                break 'seed;
            };
            seed_path.push(dispatch);
            seed = next;
        }
    }
    let seed_complete = seed_path.len() == copies * count;
    let mut search = Search {
        prepared: &prepared,
        used: seed_used,
        complete: true,
        lower,
        best: seed_complete.then(|| (seed.makespan, seed_path.clone())),
    };
    search.visit(&prepared.initial(), &mut Vec::with_capacity(copies * count))?;
    let Some((best_finish, selected_path)) = search.best.as_ref() else {
        return Err(Refusal::new(
            RefusalCode::NotConverged,
            "no complete routed dispatch was found inside the qualified windows and bounded search; this does not establish unrestricted production infeasibility",
        ));
    };
    let selected = prepared.replay(selected_path)?;
    let baseline = prepared.replay(if seed_complete {
        &seed_path
    } else {
        selected_path
    })?;
    if selected.makespan_milliseconds != *best_finish {
        return Err(bad(
            "routed incumbent changed during exact timeline reconstruction",
        ));
    }
    let identity_order = ordering::Report {
        objective: ordering::Objective::MinimizeSetupChanges,
        baseline_replaced: false,
        baseline_order: baseline
            .operations
            .iter()
            .map(|operation| operation.id.clone())
            .collect(),
        selected_order: selected
            .operations
            .iter()
            .map(|operation| operation.id.clone())
            .collect(),
        baseline_changes: changes(policy, &prepared.tasks, &baseline),
        selected_changes: changes(policy, &prepared.tasks, &selected),
        search_nodes: 0,
        search_complete: false,
        optimality_proven: false,
        budget_exhausted: false,
        lower_bound_total_changes: 0,
        timing: None,
        handling_timing: None,
        tie_break: "Routed dispatch identities and summed per-cell setup changes only. No serial ordering was optimized; execution, timing, search and proof are exclusively in production.routing.",
    };
    let mut operations = Vec::with_capacity(copies * count);
    let mut selected_intervals = 0usize;
    for copy in 0..copies {
        for (route_index, reference) in out.seams.iter().enumerate() {
            let assigned = selected
                .operations
                .iter()
                .find(|operation| operation.copy == copy && operation.route_index == route_index)
                .expect("complete selected execution");
            let source = cell_plans
                .iter()
                .find(|source| source.cell == assigned.cell)
                .expect("admitted selected source");
            let seam = &source.plan.seams[assigned.seam_index];
            selected_intervals = selected_intervals
                .checked_add(seam.stitch_intervals)
                .ok_or_else(|| budget("selected routed stitch count overflows"))?;
            operations.push(super::Operation {
                id: format!("{copy}/{}", seam.pass.operation_id()),
                copy,
                operation_id: seam.pass.operation_id().to_string(),
                cell: policy.eligible().then(|| assigned.cell.clone()),
                seam_index: if policy.eligible() {
                    assigned.seam_index
                } else {
                    route_index
                },
                after: seam
                    .pass
                    .after
                    .iter()
                    .map(|id| format!("{copy}/{id}"))
                    .collect(),
                qualification_id: seam.qualification_id.clone(),
                stitch_intervals: seam.stitch_intervals,
                feed_seconds: seam.feed_seconds,
            });
            debug_assert_eq!(reference.pass.operation_id(), seam.pass.operation_id());
        }
    }
    let proven = selected.makespan_milliseconds == lower;
    let search_complete = search.complete || proven;
    let workholding = prepared
        .fixtures
        .as_ref()
        .map(|fixtures| fixtures.report(&baseline, &selected))
        .transpose()?;
    let report = Report {
        schema: if uses_fixtures {
            "ftx-sewing-routing/3"
        } else if policy.eligible() {
            "ftx-sewing-routing/2"
        } else {
            "ftx-sewing-routing/1"
        },
        route: policy.route.clone(),
        reference_scope: policy.eligible().then_some("seed_route_one_garment/1"),
        cell_plans,
        workholding,
        baseline,
        selected,
        baseline_replaced: !seed_complete,
        search_nodes: search.used,
        search_complete,
        optimality_proven: proven,
        budget_exhausted: !search_complete,
        lower_bound_makespan_milliseconds: lower,
        search_scope: if policy.eligible() {
            "coupled_eligible_handoff_cell_append_dispatch/1"
        } else {
            "coupled_handoff_cell_append_dispatch/1"
        },
        replay_operations: 2 * copies * count,
        work_used: search.used + 2 * copies * count,
        physical_routing_validated: false,
    };
    out.production = Some(super::Report {
        schema: if uses_fixtures {
            "ftx-sewing-production/8"
        } else if policy.eligible() {
            "ftx-sewing-production/7"
        } else {
            "ftx-sewing-production/6"
        },
        production_copies: copies,
        machine: options.machine.id.clone(),
        operations,
        ordering: identity_order,
        stitch_intervals: selected_intervals,
        feed_seconds: report.selected.feed_seconds,
        workholding: None,
        handling: None,
        parallel: None,
        routing: Some(report),
    });
    if let Some(request) = &options.consumables {
        out.consumables = Some(consumables::evaluate(&out, request)?);
    }
    Ok(out)
}
