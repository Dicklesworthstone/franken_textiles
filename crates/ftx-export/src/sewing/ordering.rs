//! Bounded setup minimization over an already admitted sewing job: either identity-change counts
//! or explicitly source-qualified directed durations. Never infers handling time or accessibility.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::{PlannedSeam, bad, qualified_text};

const MAX_PASSES: usize = 20;
const MAX_PRODUCTION_PASSES: usize = 63;
const MAX_SEARCH_NODES: usize = 200_000;

mod timing;
pub use timing::{SetupTimes, TimingReport, Transition};

/// Explicitly chosen manufacturing objective; neither component has a hidden time/cost weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    /// Minimize needle changes plus thread changes, counting a change of both as two.
    MinimizeSetupChanges,
    /// Minimize declared directed setup durations; requires known initial setup and complete times.
    MinimizeSetupTime,
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

/// Bounded scheduling policy. One-garment ordering admits 20 operations; explicit production admits 63.
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
            (Objective::MinimizeSetupTime, Some(times)) if self.initial_setup.is_some() => {
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
    /// Admitted baseline: normal lexicographic topology, or complete garments in copy order.
    pub baseline_order: Vec<String>,
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
    /// Deterministic incumbent and traversal tie policy; no hidden secondary objective.
    pub tie_break: &'static str,
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
    pub(super) acquire: u64,
    pub(super) release: u64,
}

impl ResourceLimits {
    pub(super) fn occupied(&self, mask: u64) -> Vec<u32> {
        let mut occupied = vec![0; self.capacity.len()];
        for hold in &self.holds {
            if mask & hold.acquire != 0 && mask & hold.release == 0 {
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
    predecessors: u64,
    needle: u64,
    thread: u64,
}

struct Search<'a> {
    tasks: Vec<Task<'a>>,
    initial: Option<(u64, u64)>,
    all: u64,
    times: Option<Vec<Vec<u64>>>,
    resources: Option<&'a ResourceLimits>,
}

struct Frame {
    mask: u64,
    last: Option<usize>,
    cost: u64,
    candidates: Vec<usize>,
    next: usize,
}

impl Search<'_> {
    fn setup(&self, last: Option<usize>) -> Option<(u64, u64)> {
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
    fn count_lower_bound(&self, mask: u64, last: Option<usize>) -> usize {
        let (mut needles, mut threads) = (0_u64, 0_u64);
        for (i, task) in self.tasks.iter().enumerate() {
            if mask & (1 << i) == 0 {
                needles |= task.needle;
                threads |= task.thread;
            }
        }
        let component = |remaining: u64, current: Option<u64>| {
            remaining.count_ones() as usize
                - usize::from(remaining != 0 && current.is_none_or(|v| remaining & v != 0))
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
            .sum()
    }

    fn lower_bound(&self, mask: u64, last: Option<usize>) -> u64 {
        if self.times.is_none() {
            return self.count_lower_bound(mask, last) as u64;
        }
        // Every remaining operation needs one incoming transition, from the current setup or
        // another remaining operation. Independent minima relax order/degree/dependency constraints,
        // so their sum is a lower bound even for asymmetric, nonmetric durations and repeated setups.
        (0..self.tasks.len())
            .filter(|&j| mask & (1 << j) == 0)
            .map(|j| {
                (0..self.tasks.len())
                    .filter(|&i| i != j && mask & (1 << i) == 0)
                    .map(|i| self.cost(Some(i), j))
                    .fold(self.cost(last, j), u64::min)
            })
            .sum()
    }

    fn frame(&self, mask: u64, last: Option<usize>, cost: u64) -> Frame {
        let occupied = self.resources.map(|resources| resources.occupied(mask));
        let mut candidates: Vec<_> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(i, task)| {
                mask & (1 << i) == 0
                    && task.predecessors & mask == task.predecessors
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
    optimize_operations_with_resources(policy, baseline_passes, feed_seconds, None)
}

// Resource-qualified callers first admit the complete baseline, including added lifecycle edges.
// Relaxing resource capacity only weakens existing lower bounds; pruning/proof remain sound.
pub(super) fn optimize_operations_with_resources(
    policy: &Policy,
    baseline_passes: &[Operation<'_>],
    feed_seconds: f64,
    resources: Option<&ResourceLimits>,
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
    let times = policy
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
    let ids: BTreeMap<_, _> = passes
        .iter()
        .enumerate()
        .map(|(i, pass)| (pass.id, i))
        .collect();
    let mut needles = BTreeMap::new();
    let mut threads = BTreeMap::new();
    let mut tasks = Vec::with_capacity(passes.len());
    for pass in passes {
        let next_needle = 1_u64 << needles.len();
        let needle = *needles.entry(pass.needle).or_insert(next_needle);
        let next_thread = 1_u64 << threads.len();
        let thread = *threads.entry(pass.thread).or_insert(next_thread);
        let predecessors = pass
            .after
            .iter()
            .fold(0, |mask, id| mask | (1 << ids[id.as_str()]));
        tasks.push(Task {
            id: pass.id,
            predecessors,
            needle,
            thread,
        });
    }
    let search = Search {
        all: (1_u64 << tasks.len()) - 1,
        tasks,
        times,
        resources,
        // An explicitly installed identity absent from the job is a known different setup (zero),
        // not the omitted/unknown setup represented by None.
        initial: policy.initial_setup.as_ref().map(|s| {
            (
                needles.get(s.needle.as_str()).copied().unwrap_or(0),
                threads.get(s.thread.as_str()).copied().unwrap_or(0),
            )
        }),
    };
    let baseline: Vec<_> = baseline_passes.iter().map(|pass| ids[pass.id]).collect();
    let baseline_changes = search.count(&baseline);
    let mut best = baseline.clone();
    let baseline_cost = search.total_cost(&baseline);
    let mut best_cost = baseline_cost;
    let root_bound = search.lower_bound(0, None);
    let mut search_nodes = 0;
    let mut budget_exhausted = false;
    let mut stack = vec![search.frame(0, None, 0)];
    let mut path = Vec::with_capacity(baseline_passes.len());
    // Iterative depth-first branch-and-bound: at most 63 frames, each with at most 63 candidates.
    // A budget stop cannot replace the incumbent with an unfinished prefix.
    while best_cost > root_bound && !stack.is_empty() {
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
        let mask = frame.mask | (1 << next);
        let cost = frame.cost + search.cost(frame.last, next);
        if cost + search.lower_bound(mask, Some(next)) >= best_cost {
            continue;
        }
        if mask == search.all {
            best.clone_from(&path);
            best.push(next);
            best_cost = cost;
        } else {
            path.push(next);
            stack.push(search.frame(mask, Some(next), cost));
        }
    }
    let names = |order: &[usize]| {
        order
            .iter()
            .map(|&i| search.tasks[i].id.to_string())
            .collect()
    };
    Ok(Report {
        objective: policy.objective,
        baseline_order: names(&baseline),
        selected_order: names(&best),
        baseline_changes,
        selected_changes: search.count(&best),
        search_nodes,
        search_complete: !budget_exhausted,
        optimality_proven: !budget_exhausted,
        budget_exhausted,
        lower_bound_total_changes: if search.times.is_some() {
            search.count_lower_bound(0, None)
        } else if budget_exhausted {
            root_bound as usize
        } else {
            best_cost as usize
        },
        timing: search.times.as_ref().map(|_| TimingReport {
            baseline_setup_milliseconds: baseline_cost,
            selected_setup_milliseconds: best_cost,
            lower_bound_setup_milliseconds: if budget_exhausted {
                root_bound
            } else {
                best_cost
            },
            feed_and_setup_seconds: feed_seconds + best_cost as f64 / 1000.0,
        }),
        tie_break: if search.times.is_some() {
            "Retain the incumbent on equal integer setup milliseconds; start with the lexicographic topological schedule, then explore by immediate duration and operation ID."
        } else {
            "Retain the incumbent on equal total changes; start with the lexicographic topological schedule, then explore by immediate total changes and operation ID."
        },
    })
}
