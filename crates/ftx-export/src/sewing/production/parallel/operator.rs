//! One qualified operator attends setup and handling while independently qualified machines feed.
//! Whole garments and each retained serial operation order stay in their original physical cell.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{Assignment, Cell, MAX_EXACT_MILLISECONDS, MAX_WORK, Trial, budget, sum_time};
use crate::sewing::{SewOptions, SewPlan, bad, ordering::Setup, qualified_text, sha256_text};

/// A separately qualified one-person operating protocol. Every operation is handled, set up and
/// then fed; handling and setup form one nonpreemptive attended interval. Feed releases the person.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `exclusive_setup_handling_autonomous_feed/1`; no other autonomous phase is inferred.
    pub model: String,
    /// Stable identity of the one operator shared by all declared cells.
    pub operator: String,
    /// Source-qualified availability from the common job origin, at most one day.
    pub available_at_milliseconds: u32,
    /// Optional inclusive operator-shift end, covering all travel, waiting and final unloading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_until_milliseconds: Option<u64>,
    /// Full cell and effective recipe/timing locks, including cells that ultimately remain idle.
    pub cells: BTreeMap<String, String>,
    /// Explicit travel from the declared initial operator location to every possible first cell.
    /// Missing rows are unknown; zero travel must be supplied as an observation or admitted estimate.
    pub initial_arrival_milliseconds: BTreeMap<String, u32>,
    /// Every directed pair of distinct cells. Remaining at a cell adds no between-cell travel.
    pub transfers: Vec<Transfer>,
    /// Reserved part of the enclosing parallel cap, including seeds, allocations and dispatches.
    pub max_search_nodes: usize,
    /// Evidence for this exact person/cell protocol, unattended feed and arrival/transfer times.
    pub source: Source,
    /// Acquisition and phase protocol, including safe waiting and separation of travel from handling.
    pub method: String,
    /// Explicit applicability; no live worker state, robotics permission or timing bound is inferred.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Evidence class independent of process, setup, handling and fixture qualifications.
    pub evidence: Evidence,
    /// Separate explicit consent for estimated operator applicability and travel observations.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// A complete directed operator transfer, excluding the destination's existing handling and setup.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transfer {
    /// Previous attended cell.
    pub from: String,
    /// Next attended cell; must differ from `from`.
    pub to: String,
    /// Nonpreemptive travel duration, at most one day. Zero is an explicit input.
    pub milliseconds: u32,
}

/// Exact resource interval for one operation, or for a cell's final unloading. Milliseconds use the
/// qualified per-operation feed ceiling convention; local operation identities map through the cell.
#[derive(Clone, Debug, Serialize)]
pub struct Step {
    /// Physical machine cell attended by this one operator.
    pub cell: String,
    /// Previous cell, absent for travel from the declared initial operator location.
    pub from_cell: Option<String>,
    /// Scoped cell-local operation, absent for final unloading.
    pub operation: Option<String>,
    /// Global physical garment copy, absent for final batch unloading.
    pub copy: Option<usize>,
    /// Previous and next local operation identities of the exact qualified handling transition.
    pub handling_from: Option<String>,
    /// Successor local operation, absent for final unloading.
    pub handling_to: Option<String>,
    /// Operator departure, immediately after the previous attended interval or shift start.
    pub departure_milliseconds: u64,
    /// Arrival after the exact initial or directed cell-transfer duration.
    pub arrival_milliseconds: u64,
    /// Start after arrival and previous autonomous feed/cell availability. Waiting holds the person.
    pub handling_start_milliseconds: u64,
    /// Handling ends and the source-qualified setup interval starts at this time.
    pub setup_start_milliseconds: u64,
    /// Setup ends, the operator is free, and this operation's autonomous feed starts.
    pub operator_release_milliseconds: u64,
    /// End of scheduled autonomous feed; equals release for final unloading.
    pub feed_finish_milliseconds: u64,
}

/// Complete replay of one shared person's exclusive attendance and all autonomous cell feed.
#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    /// Dispatch order, with no overlapping travel, waiting, setup or handling for this operator.
    pub steps: Vec<Step>,
    /// Sum of explicit initial arrival and directed transfer work.
    pub transfer_milliseconds: u64,
    /// Time spent at a chosen cell waiting for its availability or previous autonomous feed.
    pub waiting_milliseconds: u64,
    /// Travel plus setup and handling, excluding waiting and unattended feed.
    pub busy_milliseconds: u64,
}

/// Bounded operator search, separate from the unrestricted global makespan proof in its parent.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-shared-operator/1`.
    pub schema: &'static str,
    /// The one source-qualified operator, never multiplied by the number of cells.
    pub operator: String,
    /// Complete feasible first-cell seed, or the selected feasible replacement when that seed fails.
    pub baseline: Timeline,
    /// All attended intervals in the selected complete whole-garment allocation.
    pub selected: Timeline,
    /// Whether the first-cell seed was unavailable under operator/cell timing and shift constraints.
    pub baseline_replaced: bool,
    /// `retained_serial_incumbents/1`: allocation and interleaving use each cell/count's admitted
    /// serial incumbent. Another serial order can improve the unrestricted joint scheduling problem.
    pub serial_order_scope: &'static str,
    /// Actual seed, allocation-prefix and dispatch extensions charged to the enclosing shared cap.
    pub search_nodes: usize,
    /// This restricted allocation/interleaving search completed or met the independent global bound.
    /// This field alone never proves an unrestricted optimum or physical impossibility.
    pub search_complete: bool,
    /// The declared operator cap stopped a search that had not met the independent global bound.
    pub budget_exhausted: bool,
    /// Declared source observations and numerical scheduling do not validate physical factory timing.
    pub physical_timing_validated: bool,
}

/// Lock a physical cell and its actual inherited/overridden recipe, access, qualification and timing
/// inputs. Production quantity, global allocation and purchasing are excluded from the source job.
/// Hashing does not validate the cell or qualify autonomous operation.
///
/// # Errors
/// The configured source options or canonical typed serialization are invalid.
pub fn cell_sha256(cell: &Cell, parent: &SewOptions) -> Result<String, Refusal> {
    let mut source = cell
        .configured_options(parent)?
        .unwrap_or_else(|| parent.clone());
    source.ordering = None;
    source.production = None;
    source.consumables = None;
    let value = serde_json::to_value((cell, source)).map_err(|error| bad(error.to_string()))?;
    Ok(sha256_hex(
        &serde_json::to_vec(&value).map_err(|error| bad(error.to_string()))?,
    ))
}

impl Policy {
    pub(super) fn validate(
        &self,
        plan: &SewPlan,
        cells: &[&Cell],
        copies: usize,
    ) -> Result<(), Refusal> {
        if self.max_search_nodes < plan.seams.len() * copies + 1 {
            return Err(budget(
                "shared operator cap must reserve every operation and final unloading of one complete first-cell seed",
            ));
        }
        if self.model != "exclusive_setup_handling_autonomous_feed/1"
            || !qualified_text(&self.operator)
            || self.available_at_milliseconds > 86_400_000
            || self.available_until_milliseconds.is_some_and(|end| {
                end < u64::from(self.available_at_milliseconds) || end > MAX_EXACT_MILLISECONDS
            })
            || !(1..=MAX_WORK).contains(&self.max_search_nodes)
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
                "invalid qualified shared-operator model, source, availability or work cap",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated shared-operator applicability and travel need separate explicit consent",
            ));
        }
        if self.cells.len() != cells.len()
            || self.initial_arrival_milliseconds.len() != cells.len()
            || self.transfers.len() != cells.len() * (cells.len() - 1)
        {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                "shared operator needs exact cell locks, every initial arrival and every directed inter-cell transfer",
            ));
        }
        let ids: BTreeSet<_> = cells.iter().map(|cell| cell.id.as_str()).collect();
        for cell in cells {
            let lock = self
                .cells
                .get(&cell.id)
                .ok_or_else(|| bad("shared operator omits a physical cell lock"))?;
            if !sha256_text(lock) || *lock != cell_sha256(cell, &plan.options)? {
                return Err(bad(
                    "shared operator cell/recipe/timing content lock mismatch",
                ));
            }
            if self
                .initial_arrival_milliseconds
                .get(&cell.id)
                .is_none_or(|&value| value > 86_400_000)
            {
                return Err(bad(
                    "shared operator needs a bounded explicit initial arrival for every cell",
                ));
            }
        }
        let mut seen = BTreeSet::new();
        for transfer in &self.transfers {
            if transfer.from == transfer.to
                || !ids.contains(transfer.from.as_str())
                || !ids.contains(transfer.to.as_str())
                || transfer.milliseconds > 86_400_000
                || !seen.insert((&transfer.from, &transfer.to))
            {
                return Err(bad(
                    "invalid, repeated or unknown directed shared-operator transfer",
                ));
            }
        }
        Ok(())
    }
}

// Count candidates retain only integer phase durations and source operation indices. Full handling,
// fixture, operation and geometry histories are still reconstructed only for returned assignments.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Phase {
    operation: Option<usize>,
    handling: u64,
    setup: u64,
    feed: u64,
}

impl Phase {
    fn attended(&self) -> Result<u64, Refusal> {
        sum_time(self.handling, self.setup)
    }
    fn duration(&self) -> Result<u64, Refusal> {
        sum_time(self.attended()?, self.feed)
    }
}

pub(super) fn phases(
    plan: &SewPlan,
    cell: &Cell,
    schedule: &super::super::Report,
    total_feed: u64,
) -> Result<Vec<Phase>, Refusal> {
    let times = cell
        .ordering
        .setup_times
        .as_ref()
        .expect("admitted timed order");
    let transitions: BTreeMap<_, _> = times
        .transitions
        .iter()
        .map(|row| ((&row.from, &row.to), u64::from(row.milliseconds)))
        .collect();
    let handling = &schedule
        .handling
        .as_ref()
        .expect("admitted cell handling")
        .selected;
    let mut previous = cell
        .ordering
        .initial_setup
        .clone()
        .expect("admitted initial setup");
    let mut scheduled_feed = 0;
    let mut result = Vec::with_capacity(schedule.operations.len() + 1);
    for (position, id) in schedule.ordering.selected_order.iter().enumerate() {
        let index = schedule
            .operations
            .iter()
            .position(|operation| &operation.id == id)
            .expect("complete serial operation order");
        let operation = &schedule.operations[index];
        let pass = &plan.seams[operation.seam_index].pass;
        let setup = Setup {
            needle: pass.needle.clone(),
            thread: pass.thread.clone(),
        };
        let duration = if previous == setup {
            0
        } else {
            *transitions
                .get(&(&previous, &setup))
                .expect("ordinary scheduler admitted directed setup")
        };
        // Each feed holds its machine for its own full ceiling. Padding before an intervening
        // attended interval cannot be borrowed to release a later positive feed early.
        let feed = feed_milliseconds(operation.feed_seconds)?;
        result.push(Phase {
            operation: Some(index),
            handling: handling.steps[position].milliseconds,
            setup: duration,
            feed,
        });
        scheduled_feed = sum_time(scheduled_feed, feed)?;
        previous = setup;
    }
    if scheduled_feed != total_feed {
        return Err(bad(
            "shared operator phase feed does not replay its complete cell duration",
        ));
    }
    result.push(Phase {
        operation: None,
        handling: handling.steps.last().expect("final unloading").milliseconds,
        setup: 0,
        feed: 0,
    });
    Ok(result)
}

pub(super) fn feed_milliseconds(seconds: f64) -> Result<u64, Refusal> {
    let rounded = (seconds * 1000.0).ceil();
    if !rounded.is_finite() || !(0.0..=MAX_EXACT_MILLISECONDS as f64).contains(&rounded) {
        return Err(budget(
            "shared operator feed exceeds exact millisecond bounds",
        ));
    }
    Ok(rounded as u64)
}

#[derive(Clone, Debug, PartialEq)]
struct Dispatch {
    cell: usize,
    phase: usize,
    previous: Option<usize>,
    departure: u64,
    arrival: u64,
    start: u64,
    release: u64,
    finish: u64,
}

#[derive(Clone)]
pub(super) struct Candidate {
    pub counts: Vec<usize>,
    pub makespan: u64,
    steps: Vec<Dispatch>,
    finishes: Vec<Option<u64>>,
}

pub(super) struct Outcome {
    pub baseline: Candidate,
    pub selected: Candidate,
    pub baseline_replaced: bool,
    pub lower: u64,
    pub nodes: usize,
    pub bound_work: usize,
    pub complete: bool,
    pub exhausted: bool,
}

// A separate complete DP relaxes all operator travel, waiting and autonomous-feed timing. Each
// serial subproblem's valid setup+handling bound remains valid for every admitted operation order.
fn attended_bound(cells: &[&Cell], trials: &[Vec<Trial>], copies: usize) -> (u64, usize) {
    let mut costs = vec![u64::MAX; copies + 1];
    costs[0] = 0;
    let mut work = 0;
    for (cell, row) in cells.iter().zip(trials) {
        let mut next = vec![u64::MAX; copies + 1];
        for total in 0..=copies {
            for count in 0..=total {
                work += 1;
                if costs[total - count] == u64::MAX
                    || count > 0
                        && cell
                            .available_until_milliseconds
                            .is_some_and(|end| row[count - 1].lower > end)
                {
                    continue;
                }
                // Every constituent is at most 63 one-day setup/handling durations. This addition
                // cannot reach u64 or the exact JSON bound under the ordinary production limits.
                let candidate = costs[total - count]
                    + if count == 0 {
                        0
                    } else {
                        row[count - 1].attended_lower
                    };
                next[total] = next[total].min(candidate);
            }
        }
        costs = next;
    }
    (costs[copies], work)
}

struct Search<'a> {
    policy: &'a Policy,
    cells: &'a [&'a Cell],
    trials: &'a [Vec<Trial>],
    arrivals: Vec<u64>,
    transfers: Vec<Vec<u64>>,
    nodes: usize,
    exhausted: bool,
    best: Option<Candidate>,
    lower: u64,
}

impl Search<'_> {
    fn tick(&mut self) -> bool {
        if self.nodes == self.policy.max_search_nodes {
            self.exhausted = true;
            false
        } else {
            self.nodes += 1;
            true
        }
    }

    fn proved(&self) -> bool {
        self.best
            .as_ref()
            .is_some_and(|best| best.makespan == self.lower)
    }

    fn phase(&self, counts: &[usize], cell: usize, phase: usize) -> &Phase {
        &self.trials[cell][counts[cell] - 1].phases[phase]
    }

    fn dispatch(
        &self,
        state: &State,
        counts: &[usize],
        cell: usize,
    ) -> Result<Option<Dispatch>, Refusal> {
        let phase = self.phase(counts, cell, state.positions[cell]);
        let travel = state
            .previous
            .map_or(self.arrivals[cell], |from| self.transfers[from][cell]);
        let arrival = sum_time(state.free, travel)?;
        let start = arrival.max(state.ready[cell]);
        let release = sum_time(start, phase.attended()?)?;
        let finish = sum_time(release, phase.feed)?;
        if self
            .policy
            .available_until_milliseconds
            .is_some_and(|end| release > end)
            || self.cells[cell]
                .available_until_milliseconds
                .is_some_and(|end| finish > end)
        {
            return Ok(None);
        }
        Ok(Some(Dispatch {
            cell,
            phase: state.positions[cell],
            previous: state.previous,
            departure: state.free,
            arrival,
            start,
            release,
            finish,
        }))
    }

    fn initial(&self) -> State {
        State {
            positions: vec![0; self.cells.len()],
            ready: self
                .cells
                .iter()
                .map(|cell| u64::from(cell.available_at_milliseconds))
                .collect(),
            previous: None,
            free: u64::from(self.policy.available_at_milliseconds),
            steps: Vec::new(),
        }
    }

    fn next_cells(&self, state: &State, counts: &[usize]) -> Vec<usize> {
        let mut choices: Vec<_> = (0..counts.len())
            .filter(|&cell| {
                counts[cell] > 0
                    && state.positions[cell] < self.trials[cell][counts[cell] - 1].phases.len()
            })
            .collect();
        choices.sort_by_key(|&cell| {
            (
                self.cells[cell]
                    .available_until_milliseconds
                    .unwrap_or(MAX_EXACT_MILLISECONDS),
                state.ready[cell].max(state.free),
                cell,
            )
        });
        choices
    }

    fn candidate(&self, state: &State, counts: &[usize]) -> Candidate {
        let finishes: Vec<_> = counts
            .iter()
            .enumerate()
            .map(|(cell, &count)| (count > 0).then_some(state.ready[cell]))
            .collect();
        Candidate {
            counts: counts.to_vec(),
            makespan: finishes.iter().flatten().copied().max().unwrap_or(0),
            finishes,
            steps: state.steps.clone(),
        }
    }

    fn retain(&mut self, candidate: Candidate) {
        if self
            .best
            .as_ref()
            .is_none_or(|best| candidate.makespan < best.makespan)
        {
            self.best = Some(candidate);
        }
    }

    fn seed(&mut self, counts: &[usize]) -> Result<Option<Candidate>, Refusal> {
        let mut state = self.initial();
        loop {
            let Some(cell) = self.next_cells(&state, counts).first().copied() else {
                let candidate = self.candidate(&state, counts);
                self.retain(candidate.clone());
                return Ok(Some(candidate));
            };
            if !self.tick() {
                return Ok(None);
            }
            let Some(dispatch) = self.dispatch(&state, counts, cell)? else {
                return Ok(None);
            };
            state.push(dispatch);
        }
    }

    fn optimistic(&self, state: &State, counts: &[usize]) -> Result<Option<u64>, Refusal> {
        let mut finish = 0;
        let mut attended = 0;
        for (cell, &count) in counts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let mut remaining = 0;
            for phase in &self.trials[cell][count - 1].phases[state.positions[cell]..] {
                remaining = sum_time(remaining, phase.duration()?)?;
                attended = sum_time(attended, phase.attended()?)?;
            }
            let end = sum_time(state.ready[cell], remaining)?;
            if self.cells[cell]
                .available_until_milliseconds
                .is_some_and(|limit| end > limit)
            {
                return Ok(None);
            }
            finish = finish.max(end);
        }
        let operator_end = sum_time(state.free, attended)?;
        if self
            .policy
            .available_until_milliseconds
            .is_some_and(|limit| operator_end > limit)
        {
            return Ok(None);
        }
        Ok(Some(finish.max(operator_end)))
    }

    fn interleave(&mut self, state: &mut State, counts: &[usize]) -> Result<(), Refusal> {
        if self.exhausted || self.proved() {
            return Ok(());
        }
        let Some(bound) = self.optimistic(state, counts)? else {
            return Ok(());
        };
        if self
            .best
            .as_ref()
            .is_some_and(|best| bound >= best.makespan)
        {
            return Ok(());
        }
        let choices = self.next_cells(state, counts);
        if choices.is_empty() {
            self.retain(self.candidate(state, counts));
            return Ok(());
        }
        for cell in choices {
            if !self.tick() {
                break;
            }
            let Some(dispatch) = self.dispatch(state, counts, cell)? else {
                continue;
            };
            let old_ready = state.ready[cell];
            let old_free = state.free;
            let old_previous = state.previous;
            state.push(dispatch);
            self.interleave(state, counts)?;
            state.steps.pop();
            state.positions[cell] -= 1;
            state.ready[cell] = old_ready;
            state.free = old_free;
            state.previous = old_previous;
            if self.exhausted || self.proved() {
                break;
            }
        }
        Ok(())
    }

    fn allocate(
        &mut self,
        counts: &mut [usize],
        cell: usize,
        remaining: usize,
    ) -> Result<(), Refusal> {
        if self.exhausted || self.proved() {
            return Ok(());
        }
        if cell == counts.len() {
            if remaining == 0 {
                self.interleave(&mut self.initial(), counts)?;
            }
            return Ok(());
        }
        let first = if cell + 1 == counts.len() {
            remaining
        } else {
            0
        };
        for count in first..=remaining {
            if !self.tick() {
                break;
            }
            if count > 0 {
                let trial = &self.trials[cell][count - 1];
                if self.cells[cell]
                    .available_until_milliseconds
                    .is_some_and(|end| trial.finish > end)
                    || self
                        .best
                        .as_ref()
                        .is_some_and(|best| trial.finish >= best.makespan)
                {
                    continue;
                }
            }
            counts[cell] = count;
            self.allocate(counts, cell + 1, remaining - count)?;
            if self.exhausted || self.proved() {
                break;
            }
        }
        counts[cell] = 0;
        Ok(())
    }
}

struct State {
    positions: Vec<usize>,
    ready: Vec<u64>,
    previous: Option<usize>,
    free: u64,
    steps: Vec<Dispatch>,
}

impl State {
    fn push(&mut self, dispatch: Dispatch) {
        self.positions[dispatch.cell] += 1;
        self.ready[dispatch.cell] = dispatch.finish;
        self.free = dispatch.release;
        self.previous = Some(dispatch.cell);
        self.steps.push(dispatch);
    }
}

pub(super) fn evaluate(
    policy: &Policy,
    cells: &[&Cell],
    trials: &[Vec<Trial>],
    copies: usize,
    independent_counts: &[usize],
    independent_lower: u64,
) -> Result<Outcome, Refusal> {
    let (attended, bound_work) = attended_bound(cells, trials, copies);
    if attended == u64::MAX {
        return Err(bad(
            "qualified cell windows exclude every shared-operator lower-bound allocation",
        ));
    }
    let arrivals: Vec<_> = cells
        .iter()
        .map(|cell| u64::from(policy.initial_arrival_milliseconds[&cell.id]))
        .collect();
    let work_lower = sum_time(
        sum_time(
            u64::from(policy.available_at_milliseconds),
            *arrivals.iter().min().expect("nonempty cells"),
        )?,
        attended,
    )?;
    let lower = independent_lower.max(work_lower);
    if policy
        .available_until_milliseconds
        .is_some_and(|end| lower > end)
    {
        return Err(bad(
            "qualified shared-operator shift cannot accommodate the required work: a complete allocation over independent cell-finish and attended-work lower bounds exceeds its end",
        ));
    }
    let mut transfers = vec![vec![0; cells.len()]; cells.len()];
    for transfer in &policy.transfers {
        let from = cells
            .iter()
            .position(|cell| cell.id == transfer.from)
            .expect("qualified origin");
        let to = cells
            .iter()
            .position(|cell| cell.id == transfer.to)
            .expect("qualified destination");
        transfers[from][to] = u64::from(transfer.milliseconds);
    }
    let mut search = Search {
        policy,
        cells,
        trials,
        arrivals,
        transfers,
        nodes: 0,
        exhausted: false,
        best: None,
        lower,
    };
    let mut baseline_counts = vec![0; cells.len()];
    baseline_counts[0] = copies;
    let baseline = search.seed(&baseline_counts)?;
    if !search.proved() && independent_counts != baseline_counts {
        search.seed(independent_counts)?;
    }
    if !search.proved() {
        search.allocate(&mut vec![0; cells.len()], 0, copies)?;
    }
    let proved = search.proved();
    let selected = search.best.ok_or_else(|| Refusal::new(RefusalCode::NotConverged,
        "no complete shared-operator assignment was found inside the qualified cell/operator windows; retained serial incumbents do not exclude other feasible operation orders, so infeasibility is not proved")
        .repair("increase the bounded serial and shared-operator search allowances")
        .repair("supply qualified longer availability or a different operator/cell protocol"))?;
    let baseline_replaced = baseline.is_none();
    Ok(Outcome {
        baseline: baseline.unwrap_or_else(|| selected.clone()),
        selected,
        baseline_replaced,
        lower,
        nodes: search.nodes,
        bound_work,
        complete: !search.exhausted || proved,
        exhausted: search.exhausted && !proved,
    })
}

impl Candidate {
    fn apply(
        &self,
        assignment: &mut Assignment,
        trials: &[Vec<Trial>],
    ) -> Result<Timeline, Refusal> {
        let mut timeline = Timeline {
            steps: Vec::with_capacity(self.steps.len()),
            transfer_milliseconds: 0,
            waiting_milliseconds: 0,
            busy_milliseconds: 0,
        };
        for dispatch in &self.steps {
            let cell = &assignment.cells[dispatch.cell];
            let phase =
                &trials[dispatch.cell][self.counts[dispatch.cell] - 1].phases[dispatch.phase];
            let schedule = cell
                .schedule
                .as_ref()
                .expect("replayed active operator cell");
            let operation = phase.operation.map(|index| &schedule.operations[index]);
            let handling = &schedule
                .handling
                .as_ref()
                .expect("replayed handling")
                .selected
                .steps[dispatch.phase];
            timeline.transfer_milliseconds = sum_time(
                timeline.transfer_milliseconds,
                dispatch.arrival - dispatch.departure,
            )?;
            timeline.waiting_milliseconds = sum_time(
                timeline.waiting_milliseconds,
                dispatch.start - dispatch.arrival,
            )?;
            timeline.steps.push(Step {
                cell: cell.cell.clone(),
                from_cell: dispatch
                    .previous
                    .map(|index| assignment.cells[index].cell.clone()),
                operation: operation.map(|operation| operation.id.clone()),
                copy: operation.map(|operation| cell.copies[operation.copy]),
                handling_from: handling.from.clone(),
                handling_to: handling.to.clone(),
                departure_milliseconds: dispatch.departure,
                arrival_milliseconds: dispatch.arrival,
                handling_start_milliseconds: dispatch.start,
                setup_start_milliseconds: sum_time(dispatch.start, phase.handling)?,
                operator_release_milliseconds: dispatch.release,
                feed_finish_milliseconds: dispatch.finish,
            });
        }
        timeline.busy_milliseconds = sum_time(
            timeline.transfer_milliseconds,
            sum_time(
                assignment.setup_milliseconds,
                assignment.handling_milliseconds,
            )?,
        )?;
        for (cell, finish) in assignment.cells.iter_mut().zip(&self.finishes) {
            cell.finish_milliseconds = *finish;
        }
        assignment.makespan_milliseconds = self.makespan;
        Ok(timeline)
    }
}

impl Outcome {
    pub(super) fn report(
        &self,
        policy: &Policy,
        baseline: &mut Assignment,
        selected: &mut Assignment,
        trials: &[Vec<Trial>],
    ) -> Result<Report, Refusal> {
        Ok(Report {
            schema: "ftx-sewing-shared-operator/1",
            operator: policy.operator.clone(),
            baseline: self.baseline.apply(baseline, trials)?,
            selected: self.selected.apply(selected, trials)?,
            baseline_replaced: self.baseline_replaced,
            serial_order_scope: "retained_serial_incumbents/1",
            search_nodes: self.nodes,
            search_complete: self.complete,
            budget_exhausted: self.exhausted,
            physical_timing_validated: false,
        })
    }
}
