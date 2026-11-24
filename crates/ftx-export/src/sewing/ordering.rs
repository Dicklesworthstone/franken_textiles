//! Bounded setup minimization over an already admitted sewing job: either identity-change counts
//! or explicitly source-qualified directed durations. Never infers handling time or accessibility.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::production::routing::calendars;
use super::{PlannedSeam, bad, qualified_text};

const MAX_PASSES: usize = 20;
pub(super) const MAX_PRODUCTION_PASSES: usize = 256;
const MAX_SEARCH_NODES: usize = 200_000;

mod timing;
pub use timing::{HandlingTimingReport, SetupTimes, TimingReport, Transition};

/// Explicitly chosen manufacturing objective; neither component has a hidden time/cost weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    /// Minimize needle changes plus thread changes, counting a change of both as two.
    MinimizeSetupChanges,
    /// Minimize declared directed setup durations; requires known initial setup and complete times.
    MinimizeSetupTime,
    /// Minimize serial feed, setup and explicitly qualified garment handling in production.
    MinimizeProductionTime,
}

/// Caller-declared setup immediately before the first operation; no final reset is assumed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    /// Exact needle identifier, using the same identity convention as a sewing pass.
    pub needle: String,
    /// Exact thread identifier, using the same identity convention as a sewing pass.
    pub thread: String,
}

impl Setup {
    fn valid(&self) -> bool {
        qualified_text(&self.needle) && qualified_text(&self.thread)
    }
}

/// Bounded scheduling policy. One-garment ordering admits 20 operations; explicit production admits 256.
/// Larger legacy jobs remain supported without ordering.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Explicit count or duration objective, not sewing quality or total production time.
    pub objective: Objective,
    /// Maximum examined feasible prefix extensions, in 1..=200000.
    pub max_search_nodes: usize,
    /// Omission excludes first installation from the count; it never assumes a particular setup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_setup: Option<Setup>,
    /// Directed, source-qualified times. Only supported with the explicit time objective.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_times: Option<SetupTimes>,
}

impl Policy {
    pub(super) fn validate(&self, passes: usize, machine: &str) -> Result<(), Refusal> {
        if self.objective == Objective::MinimizeProductionTime {
            return Err(bad(
                "production-time ordering requires a production handling policy",
            ));
        }
        self.validate_count(passes, MAX_PASSES, machine)
    }

    pub(super) fn validate_production(&self, passes: usize, machine: &str) -> Result<(), Refusal> {
        self.validate_count(passes, MAX_PRODUCTION_PASSES, machine)
    }

    fn validate_count(&self, passes: usize, maximum: usize, machine: &str) -> Result<(), Refusal> {
        if passes > maximum || !(1..=MAX_SEARCH_NODES).contains(&self.max_search_nodes) {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                format!(
                    "sewing ordering supports at most {maximum} passes and 1..200000 search nodes"
                ),
            ));
        }
        if self
            .initial_setup
            .as_ref()
            .is_some_and(|setup| !setup.valid())
        {
            return Err(bad(
                "initial sewing setup needs nonempty bounded needle/thread IDs",
            ));
        }
        match (self.objective, &self.setup_times) {
            (Objective::MinimizeSetupChanges, None) => {}
            (Objective::MinimizeSetupTime | Objective::MinimizeProductionTime, Some(times))
                if self.initial_setup.is_some() =>
            {
                times.validate(machine)?;
            }
            _ => {
                return Err(bad(
                    "timed sewing order requires minimize_setup_time, explicit initial_setup and setup_times; count mode cannot consume timings",
                ));
            }
        }
        Ok(())
    }
}

/// Actual identity transitions, without assigning durations or charging the unknown first setup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Changes {
    /// Transitions whose exact needle IDs differ.
    pub needle_changes: usize,
    /// Transitions whose exact thread IDs differ.
    pub thread_changes: usize,
    /// Explicit equal-count objective: needle_changes + thread_changes.
    pub total_changes: usize,
}

/// Complete feasible incumbent, with proof status conditional on the supplied prerequisites/setup IDs.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Same declared objective as the retained input policy.
    pub objective: Objective,
    /// Admitted baseline: normal topology/copy order when feasible, otherwise the first complete
    /// resource-feasible schedule found within the same bounded search.
    pub baseline_order: Vec<String>,
    /// The original topology/copy-order seed violated qualified resource or lifecycle constraints.
    /// Omitted for existing feasible seeds. Replacement never means relaxed source constraints.
    #[serde(skip_serializing_if = "super::is_false")]
    pub baseline_replaced: bool,
    /// Every admitted operation exactly once, in the returned plan's order.
    pub selected_order: Vec<String>,
    /// Setup changes in the original admitted order, under the same initial-setup policy.
    pub baseline_changes: Changes,
    /// Setup changes in the selected complete feasible order.
    pub selected_changes: Changes,
    /// Examined feasible prefix extensions, including pruned trials; never exceeds the policy.
    pub search_nodes: usize,
    /// All better schedules were explored or eliminated by a valid objective lower bound.
    pub search_complete: bool,
    /// True only when the selected objective is proved minimal for these declared prerequisites.
    pub optimality_proven: bool,
    /// Work stopped with unexamined possibilities; the complete incumbent is still usable.
    pub budget_exhausted: bool,
    /// Proved count lower bound. In time mode this is NOT the selected count or a count-optimal claim.
    pub lower_bound_total_changes: usize,
    /// Exact time accounting, absent in legacy count mode; source table stays in plan.options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<TimingReport>,
    /// Complete serial handling objective, including the first load and final unload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handling_timing: Option<HandlingTimingReport>,
    /// Separate complete-calendar objective and proof for independently staffed parallel cells.
    /// Serial timing above still reports productive work, excluding calendar waiting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_timing: Option<CalendarTimingReport>,
    /// Deterministic incumbent and traversal tie policy; no hidden secondary objective.
    pub tie_break: &'static str,
}

/// Bounded operation-order search through qualified openings, including final unloading.
#[derive(Clone, Debug, Serialize)]
pub struct CalendarTimingReport {
    /// `all_dependency_and_resource_feasible_orders/1`; every extension uses the admitted graph.
    pub search_scope: &'static str,
    /// Calendar replay of the ordinary serial-work incumbent before calendar-specific search.
    pub serial_incumbent_finish_milliseconds: Option<u64>,
    /// Complete selected finishing instant; absent when no calendar-feasible order was found.
    pub selected_finish_milliseconds: Option<u64>,
    /// Valid finish lower bound; an exhausted search never promotes its incumbent to a bound.
    pub lower_bound_finish_milliseconds: u64,
    /// Calendar prefix extensions, charged to the remainder of the original serial node cap.
    pub search_nodes: usize,
    /// All better orders were exhausted or excluded by a valid finish lower bound.
    pub search_complete: bool,
    /// The complete selected order attains its valid bound; separate from serial-work optimality.
    pub optimality_proven: bool,
    /// Exhaustive calendar search found no complete dependency/resource-feasible order.
    pub infeasibility_proven: bool,
    /// Unvisited calendar prefixes remain after the shared serial/calendar allowance is spent.
    pub budget_exhausted: bool,
}

/// Already source-qualified parallel-cell calendar and exact per-operation feed clocks.
pub(super) struct Calendar<'a> {
    pub policy: &'a calendars::Policy,
    pub cell: &'a str,
    pub available_at_milliseconds: u64,
    pub available_until_milliseconds: Option<u64>,
    pub feed_milliseconds: BTreeMap<&'a str, u64>,
}

impl Calendar<'_> {
    fn finish(&self, ready: u64, duration: u64) -> Result<Option<u64>, Refusal> {
        let Some(start) = self.policy.earliest_start(self.cell, ready, duration)? else {
            return Ok(None);
        };
        let end = self
            .available_until_milliseconds
            .unwrap_or(9_007_199_254_740_991);
        Ok(if start <= end && duration <= end - start {
            Some(start + duration)
        } else {
            None
        })
    }
}

/// Borrowed identity/dependency view of an already admitted operation. Geometry stays with its plan.
#[derive(Clone, Copy)]
pub(super) struct Operation<'a> {
    pub id: &'a str,
    pub after: &'a [String],
    pub needle: &'a str,
    pub thread: &'a str,
}

/// Shared integer occupancy authority for the resource-qualified production caller and search.
/// Operation indices use the same sorted IDs as the optimizer; counts are bounded during admission.
pub(super) struct ResourceLimits {
    pub(super) ids: Vec<String>,
    pub(super) capacity: Vec<u32>,
    pub(super) acquire: Vec<Vec<u32>>,
    pub(super) holds: Vec<ResourceHold>,
}

pub(super) struct ResourceHold {
    pub(super) resource: usize,
    pub(super) units: u32,
    pub(super) acquire: usize,
    pub(super) release: usize,
}

// Exact, allocation-free state for the admitted production graph. The fixed four words bound
// every search frame and avoid native/wasm word-size differences at indices 63, 127, 191 and 255.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct OperationSet([u64; MAX_PRODUCTION_PASSES.div_ceil(64)]);

impl OperationSet {
    pub(super) fn all(count: usize) -> Self {
        debug_assert!(count <= MAX_PRODUCTION_PASSES);
        let mut result = Self::default();
        for index in 0..count {
            result.insert(index);
        }
        result
    }

    pub(super) fn contains(self, index: usize) -> bool {
        self.0
            .get(index / 64)
            .is_some_and(|word| word & (1_u64 << (index % 64)) != 0)
    }

    pub(super) fn insert(&mut self, index: usize) {
        self.0[index / 64] |= 1_u64 << (index % 64);
    }

    fn with(mut self, index: usize) -> Self {
        self.insert(index);
        self
    }

    fn contains_all(self, other: Self) -> bool {
        self.0
            .iter()
            .zip(other.0)
            .all(|(&have, need)| have & need == need)
    }

    fn count(self) -> usize {
        self.0.iter().map(|word| word.count_ones() as usize).sum()
    }
}

/// Qualified operation/copy-aware handling costs in the same sorted operation order as search.
pub(super) struct HandlingCosts {
    pub(super) ids: Vec<String>,
    /// The last row contains initial loading; diagonal entries are unused.
    pub(super) transitions: Vec<Vec<u64>>,
    pub(super) final_milliseconds: Vec<u64>,
}

impl ResourceLimits {
    pub(super) fn occupied(&self, mask: OperationSet) -> Vec<u32> {
        let mut occupied = vec![0; self.capacity.len()];
        for hold in &self.holds {
            if mask.contains(hold.acquire) && !mask.contains(hold.release) {
                occupied[hold.resource] += hold.units;
            }
        }
        occupied
    }

    pub(super) fn admits(&self, occupied: &[u32], next: usize) -> bool {
        occupied
            .iter()
            .zip(&self.acquire[next])
            .zip(&self.capacity)
            .all(|((&held, &acquire), &capacity)| held + acquire <= capacity)
    }
}

struct Task<'a> {
    id: &'a str,
    predecessors: OperationSet,
    needle: usize,
    thread: usize,
}

struct Search<'a> {
    tasks: Vec<Task<'a>>,
    initial: Option<(usize, usize)>,
    all: OperationSet,
    times: Option<Vec<Vec<u64>>>,
    resources: Option<&'a ResourceLimits>,
    final_milliseconds: Option<&'a [u64]>,
}

struct Frame {
    mask: OperationSet,
    last: Option<usize>,
    cost: u64,
    candidates: Vec<usize>,
    next: usize,
}

impl Search<'_> {
    fn feasible(&self, order: &[usize]) -> bool {
        let mut mask = OperationSet::default();
        for &next in order {
            let task = &self.tasks[next];
            if mask.contains(next)
                || !mask.contains_all(task.predecessors)
                || self
                    .resources
                    .is_some_and(|resources| !resources.admits(&resources.occupied(mask), next))
            {
                return false;
            }
            mask.insert(next);
        }
        mask == self.all
    }

    fn setup(&self, last: Option<usize>) -> Option<(usize, usize)> {
        last.map(|i| (self.tasks[i].needle, self.tasks[i].thread))
            .or(self.initial)
    }

    fn changes(&self, last: Option<usize>, next: usize) -> Changes {
        let Some((needle, thread)) = self.setup(last) else {
            return Changes::default();
        };
        let needle_changes = usize::from(needle != self.tasks[next].needle);
        let thread_changes = usize::from(thread != self.tasks[next].thread);
        Changes {
            needle_changes,
            thread_changes,
            total_changes: needle_changes + thread_changes,
        }
    }

    fn count(&self, order: &[usize]) -> Changes {
        let mut result = Changes::default();
        let mut last = None;
        for &next in order {
            let change = self.changes(last, next);
            result.needle_changes += change.needle_changes;
            result.thread_changes += change.thread_changes;
            result.total_changes += change.total_changes;
            last = Some(next);
        }
        result
    }

    // Each still-needed identity requires at least one transition, except the currently installed
    // identity. With unknown initial setup the first identity in each component is uncharged.
    // Ignoring prerequisites only weakens this bound; summing the component bounds stays valid.
    fn count_lower_bound(&self, mask: OperationSet, last: Option<usize>) -> usize {
        let (mut needles, mut threads) = (OperationSet::default(), OperationSet::default());
        for (i, task) in self.tasks.iter().enumerate() {
            if !mask.contains(i) {
                needles.insert(task.needle);
                threads.insert(task.thread);
            }
        }
        let component = |remaining: OperationSet, current: Option<usize>| {
            let count = remaining.count();
            count - usize::from(count != 0 && current.is_none_or(|v| remaining.contains(v)))
        };
        let setup = self.setup(last);
        component(needles, setup.map(|v| v.0)) + component(threads, setup.map(|v| v.1))
    }

    fn cost(&self, last: Option<usize>, next: usize) -> u64 {
        self.times.as_ref().map_or_else(
            || self.changes(last, next).total_changes as u64,
            |times| times[last.unwrap_or(self.tasks.len())][next],
        )
    }

    fn total_cost(&self, order: &[usize]) -> u64 {
        let mut last = None;
        order
            .iter()
            .map(|&next| {
                let cost = self.cost(last, next);
                last = Some(next);
                cost
            })
            .sum::<u64>()
            + self.final_cost(last)
    }

    fn final_cost(&self, last: Option<usize>) -> u64 {
        last.and_then(|i| self.final_milliseconds.map(|times| times[i]))
            .unwrap_or(0)
    }

    fn lower_bound(&self, mask: OperationSet, last: Option<usize>) -> u64 {
        if mask == self.all {
            return self.final_cost(last);
        }
        if self.times.is_none() {
            return self.count_lower_bound(mask, last) as u64;
        }
        // Every remaining operation needs one incoming transition, from the current setup or
        // another remaining operation. Independent minima relax order/degree/dependency constraints,
        // so their sum is a lower bound even for asymmetric, nonmetric durations and repeated setups.
        let incoming: u64 = (0..self.tasks.len())
            .filter(|&j| !mask.contains(j))
            .map(|j| {
                (0..self.tasks.len())
                    .filter(|&i| i != j && !mask.contains(i))
                    .map(|i| self.cost(Some(i), j))
                    .fold(self.cost(last, j), u64::min)
            })
            .sum();
        // Exactly one still-unvisited operation must be last. Relaxing which one yields a bound.
        incoming
            + self.final_milliseconds.map_or(0, |times| {
                (0..self.tasks.len())
                    .filter(|&i| !mask.contains(i))
                    .map(|i| times[i])
                    .min()
                    .unwrap_or(0)
            })
    }

    fn frame(&self, mask: OperationSet, last: Option<usize>, cost: u64) -> Frame {
        let occupied = self.resources.map(|resources| resources.occupied(mask));
        let mut candidates: Vec<_> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(i, task)| {
                !mask.contains(*i)
                    && mask.contains_all(task.predecessors)
                    && self.resources.is_none_or(|resources| {
                        resources.admits(occupied.as_ref().expect("resource state"), *i)
                    })
            })
            .map(|(i, _)| i)
            .collect();
        // Tasks are already in operation-ID order; index is a portable, deterministic tie key.
        candidates.sort_by_key(|&i| (self.cost(last, i), i));
        Frame {
            mask,
            last,
            cost,
            candidates,
            next: 0,
        }
    }

    fn calendar_order(
        &self,
        calendar: &Calendar<'_>,
        serial_order: &[usize],
        serial_lower_bound: u64,
        node_limit: usize,
    ) -> Result<(Vec<usize>, CalendarTimingReport), Refusal> {
        let feed: Vec<_> = self
            .tasks
            .iter()
            .map(|task| {
                calendar
                    .feed_milliseconds
                    .get(task.id)
                    .copied()
                    .ok_or_else(|| {
                        bad("calendar feed clocks do not cover the admitted operation graph")
                    })
            })
            .collect::<Result<_, _>>()?;
        let total_feed = feed.iter().try_fold(0_u64, |sum, &value| {
            sum.checked_add(value)
                .filter(|&sum| sum <= 9_007_199_254_740_991)
                .ok_or_else(|| bad("calendar feed exceeds the exact millisecond clock"))
        })?;
        let root_bound = calendar.available_at_milliseconds + total_feed + serial_lower_bound;
        if root_bound > 9_007_199_254_740_991 {
            return Err(bad(
                "calendar completion lower bound exceeds the exact millisecond clock",
            ));
        }
        let mut serial_finish = Some(calendar.available_at_milliseconds);
        let mut last = None;
        for &next in serial_order {
            serial_finish = match serial_finish {
                Some(ready) => calendar.finish(ready, self.cost(last, next) + feed[next])?,
                None => None,
            };
            last = Some(next);
        }
        serial_finish = match serial_finish {
            Some(ready) => calendar.finish(ready, self.final_cost(last))?,
            None => None,
        };
        let mut best_finish = serial_finish;
        let mut best = serial_finish.map(|_| serial_order.to_vec());
        let mut nodes = 0;
        let mut exhausted = false;
        let mut stack = vec![self.frame(
            OperationSet::default(),
            None,
            calendar.available_at_milliseconds,
        )];
        let mut path = Vec::with_capacity(self.tasks.len());
        // A fixed operation order starts each nonpreemptive block as early as possible. Earlier
        // completion can never hurt a later block, so no intentional-idling branch is required.
        while best_finish.is_none_or(|finish| finish > root_bound) && !stack.is_empty() {
            let frame = stack.last_mut().expect("nonempty calendar search");
            if frame.next == frame.candidates.len() {
                stack.pop();
                path.pop();
                continue;
            }
            if nodes == node_limit {
                exhausted = true;
                break;
            }
            let next = frame.candidates[frame.next];
            frame.next += 1;
            nodes += 1;
            let duration = self.cost(frame.last, next) + feed[next];
            let Some(ready) = calendar.finish(frame.cost, duration)? else {
                continue;
            };
            let mask = frame.mask.with(next);
            let remaining_feed: u64 = feed
                .iter()
                .enumerate()
                .filter(|(i, _)| !mask.contains(*i))
                .map(|(_, value)| value)
                .sum();
            let bound = ready + remaining_feed + self.lower_bound(mask, Some(next));
            if best_finish.is_some_and(|finish| bound >= finish) {
                continue;
            }
            if mask == self.all {
                if let Some(finish) = calendar.finish(ready, self.final_cost(Some(next)))?
                    && best_finish.is_none_or(|old| finish < old)
                {
                    let mut complete = path.clone();
                    complete.push(next);
                    best = Some(complete);
                    best_finish = Some(finish);
                }
            } else {
                path.push(next);
                stack.push(self.frame(mask, Some(next), ready));
            }
        }
        let report = CalendarTimingReport {
            search_scope: "all_dependency_and_resource_feasible_orders/1",
            serial_incumbent_finish_milliseconds: serial_finish,
            selected_finish_milliseconds: best_finish,
            lower_bound_finish_milliseconds: if exhausted {
                root_bound
            } else {
                best_finish.unwrap_or(root_bound)
            },
            search_nodes: nodes,
            search_complete: !exhausted,
            optimality_proven: best_finish.is_some() && !exhausted,
            infeasibility_proven: best_finish.is_none() && !exhausted,
            budget_exhausted: exhausted,
        };
        // Preserve the admitted resource-feasible serial schedule for unavailable count receipts.
        // Its null calendar finish excludes it from every feasible parallel assignment.
        Ok((best.unwrap_or_else(|| serial_order.to_vec()), report))
    }
}

pub(super) fn optimize(policy: &Policy, seams: &[PlannedSeam]) -> Result<Report, Refusal> {
    optimize_operations(
        policy,
        &seams
            .iter()
            .map(|seam| Operation {
                id: seam.pass.operation_id(),
                after: &seam.pass.after,
                needle: &seam.pass.needle,
                thread: &seam.pass.thread,
            })
            .collect::<Vec<_>>(),
        seams.iter().map(|s| s.feed_seconds).sum(),
    )
}

// The caller has admitted the complete operation graph and bounded its size. Production copies
// change operation identities/prerequisites only; station geometry is not duplicated for search.
pub(super) fn optimize_operations(
    policy: &Policy,
    baseline_passes: &[Operation<'_>],
    feed_seconds: f64,
) -> Result<Report, Refusal> {
    optimize_operations_with_resources(policy, baseline_passes, feed_seconds, None, None, None)
}

// Resource-qualified callers admit the graph and source evidence. A source-order seed can violate
// finite resources or added lifecycle edges: find a complete feasible replacement before pruning.
// Relaxing resource capacity only weakens existing lower bounds; pruning/proof remain sound.
pub(super) fn optimize_operations_with_resources(
    policy: &Policy,
    baseline_passes: &[Operation<'_>],
    feed_seconds: f64,
    resources: Option<&ResourceLimits>,
    handling: Option<&HandlingCosts>,
    calendar: Option<&Calendar<'_>>,
) -> Result<Report, Refusal> {
    let mut passes = baseline_passes.to_vec();
    passes.sort_by_key(|p| p.id);
    if resources.is_some_and(|resources| {
        resources
            .ids
            .iter()
            .map(String::as_str)
            .ne(passes.iter().map(|pass| pass.id))
    }) {
        return Err(bad(
            "workholding resource indices do not match admitted operations",
        ));
    }
    if (policy.objective == Objective::MinimizeProductionTime) != handling.is_some()
        || handling.is_some_and(|handling| {
            handling
                .ids
                .iter()
                .map(String::as_str)
                .ne(passes.iter().map(|pass| pass.id))
                || handling.transitions.len() != passes.len() + 1
                || handling
                    .transitions
                    .iter()
                    .any(|row| row.len() != passes.len())
                || handling.final_milliseconds.len() != passes.len()
        })
    {
        return Err(bad(
            "production-time ordering needs matching qualified handling costs",
        ));
    }
    if calendar.is_some() && handling.is_none() {
        return Err(bad("calendar ordering needs qualified production handling"));
    }
    let setup_times = policy
        .setup_times
        .as_ref()
        .map(|times| {
            times.matrix(
                &passes
                    .iter()
                    .map(|pass| Setup {
                        needle: pass.needle.to_string(),
                        thread: pass.thread.to_string(),
                    })
                    .collect::<Vec<_>>(),
                policy
                    .initial_setup
                    .as_ref()
                    .expect("validated timed policy"),
            )
        })
        .transpose()?;
    let mut times = setup_times.clone();
    if let Some(handling) = handling {
        let times = times
            .as_mut()
            .ok_or_else(|| bad("handling needs complete setup times"))?;
        for (row, additions) in times.iter_mut().zip(&handling.transitions) {
            for (value, addition) in row.iter_mut().zip(additions) {
                *value += addition;
            }
        }
    }
    let ids: BTreeMap<_, _> = passes
        .iter()
        .enumerate()
        .map(|(i, pass)| (pass.id, i))
        .collect();
    let mut needles = BTreeMap::new();
    let mut threads = BTreeMap::new();
    let mut tasks = Vec::with_capacity(passes.len());
    for pass in passes {
        let next_needle = needles.len();
        let needle = *needles.entry(pass.needle).or_insert(next_needle);
        let next_thread = threads.len();
        let thread = *threads.entry(pass.thread).or_insert(next_thread);
        let predecessors = pass.after.iter().fold(OperationSet::default(), |mask, id| {
            mask.with(ids[id.as_str()])
        });
        tasks.push(Task {
            id: pass.id,
            predecessors,
            needle,
            thread,
        });
    }
    let search = Search {
        all: OperationSet::all(tasks.len()),
        tasks,
        times,
        resources,
        final_milliseconds: handling.map(|handling| handling.final_milliseconds.as_slice()),
        // An explicitly installed identity absent from the job is a known different setup,
        // not the omitted/unknown setup represented by None.
        initial: policy.initial_setup.as_ref().map(|s| {
            (
                needles
                    .get(s.needle.as_str())
                    .copied()
                    .unwrap_or(usize::MAX),
                threads
                    .get(s.thread.as_str())
                    .copied()
                    .unwrap_or(usize::MAX),
            )
        }),
    };
    let original: Vec<_> = baseline_passes.iter().map(|pass| ids[pass.id]).collect();
    let baseline_replaced = !search.feasible(&original);
    let mut baseline = (!baseline_replaced).then_some(original);
    let mut best = baseline.clone();
    let mut best_cost = best.as_deref().map(|order| search.total_cost(order));
    let root_bound = search.lower_bound(OperationSet::default(), None);
    let mut search_nodes = 0;
    let mut budget_exhausted = false;
    let mut stack = vec![search.frame(OperationSet::default(), None, 0)];
    let mut path = Vec::with_capacity(baseline_passes.len());
    // Iterative depth-first branch-and-bound: at most 256 frames, each with at most 256 candidates.
    // Feasibility discovery and objective improvement share one prefix budget. An invalid original
    // seed supplies no cost upper bound, even when all operations have identical zero-change setups.
    // A budget stop cannot replace the incumbent with an unfinished prefix.
    while best_cost.is_none_or(|cost| cost > root_bound) && !stack.is_empty() {
        let frame = stack.last_mut().expect("nonempty search stack");
        if frame.next == frame.candidates.len() {
            stack.pop();
            path.pop();
            continue;
        }
        if search_nodes == policy.max_search_nodes {
            budget_exhausted = true;
            break;
        }
        let next = frame.candidates[frame.next];
        frame.next += 1;
        search_nodes += 1;
        let mask = frame.mask.with(next);
        let cost = frame.cost + search.cost(frame.last, next);
        if best_cost.is_some_and(|best| cost + search.lower_bound(mask, Some(next)) >= best) {
            continue;
        }
        if mask == search.all {
            let mut complete = path.clone();
            complete.push(next);
            if baseline.is_none() {
                baseline = Some(complete.clone());
            }
            best = Some(complete);
            best_cost = Some(cost + search.final_cost(Some(next)));
        } else {
            path.push(next);
            stack.push(search.frame(mask, Some(next), cost));
        }
    }
    let Some(mut best) = best else {
        return Err(if budget_exhausted {
            Refusal::new(
                RefusalCode::NotConverged,
                format!(
                    "no complete resource-feasible sewing schedule found within {search_nodes} search nodes; unvisited schedules remain unresolved"
                ),
            )
        } else {
            bad(format!(
                "no complete resource-feasible sewing schedule exists under the declared prerequisites and acquire-before-start/release-after-completion capacities; exhaustive search visited {search_nodes} nodes"
            ))
        });
    };
    let baseline = baseline.expect("a complete incumbent supplies a feasible baseline");
    let baseline_changes = search.count(&baseline);
    let baseline_cost = search.total_cost(&baseline);
    let serial_lower_bound = if budget_exhausted {
        root_bound
    } else {
        best_cost.expect("a complete incumbent supplies its objective")
    };
    let calendar_timing = calendar
        .map(|calendar| {
            let (selected, report) = search.calendar_order(
                calendar,
                &best,
                serial_lower_bound,
                policy.max_search_nodes - search_nodes,
            )?;
            best = selected;
            search_nodes += report.search_nodes;
            budget_exhausted |= report.budget_exhausted;
            Ok::<_, Refusal>(report)
        })
        .transpose()?;
    let best_cost = search.total_cost(&best);
    let serial_optimality_proven = best_cost == serial_lower_bound;
    let names = |order: &[usize]| {
        order
            .iter()
            .map(|&i| search.tasks[i].id.to_string())
            .collect()
    };
    let sum_transitions = |matrix: &[Vec<u64>], order: &[usize]| {
        let mut last = search.tasks.len();
        order
            .iter()
            .map(|&next| {
                let cost = matrix[last][next];
                last = next;
                cost
            })
            .sum::<u64>()
    };
    let baseline_setup = setup_times
        .as_ref()
        .map(|times| sum_transitions(times, &baseline));
    let selected_setup = setup_times
        .as_ref()
        .map(|times| sum_transitions(times, &best));
    Ok(Report {
        objective: policy.objective,
        baseline_order: names(&baseline),
        baseline_replaced,
        selected_order: names(&best),
        baseline_changes,
        selected_changes: search.count(&best),
        search_nodes,
        search_complete: !budget_exhausted && serial_optimality_proven,
        optimality_proven: serial_optimality_proven,
        budget_exhausted,
        lower_bound_total_changes: if search.times.is_some() {
            search.count_lower_bound(OperationSet::default(), None)
        } else if budget_exhausted {
            root_bound as usize
        } else {
            best_cost as usize
        },
        timing: search.times.as_ref().map(|_| TimingReport {
            baseline_setup_milliseconds: baseline_setup.expect("timed baseline"),
            selected_setup_milliseconds: selected_setup.expect("timed incumbent"),
            lower_bound_setup_milliseconds: if handling.is_some() {
                // Combined optimality does not establish setup-only optimality.
                0
            } else if budget_exhausted {
                root_bound
            } else {
                best_cost
            },
            feed_and_setup_seconds: feed_seconds
                + selected_setup.expect("timed incumbent") as f64 / 1000.0,
        }),
        handling_timing: handling.map(|handling| HandlingTimingReport {
            baseline_handling_milliseconds: sum_transitions(&handling.transitions, &baseline)
                + search.final_cost(baseline.last().copied()),
            selected_handling_milliseconds: sum_transitions(&handling.transitions, &best)
                + search.final_cost(best.last().copied()),
            baseline_modeled_production_seconds: feed_seconds + baseline_cost as f64 / 1000.0,
            selected_modeled_production_seconds: feed_seconds + best_cost as f64 / 1000.0,
            lower_bound_setup_and_handling_milliseconds: serial_lower_bound,
        }),
        calendar_timing,
        tie_break: if handling.is_some() {
            "Retain the incumbent on equal integer setup-plus-handling milliseconds; explore by immediate duration and operation ID."
        } else if search.times.is_some() {
            "Retain the incumbent on equal integer setup milliseconds; start with the lexicographic topological schedule, then explore by immediate duration and operation ID."
        } else {
            "Retain the incumbent on equal total changes; start with the lexicographic topological schedule, then explore by immediate total changes and operation ID."
        },
    })
}
