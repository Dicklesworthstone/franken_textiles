//! Bounded setup minimization over an already admitted sewing job: either identity-change counts
//! or explicitly source-qualified directed durations. Never infers handling time or accessibility.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::{PlannedSeam, SewingPass, bad, qualified_text};

const MAX_PASSES: usize = 20;
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

/// Opt-in search over at most 20 admitted operations. Larger legacy jobs remain supported without it.
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
        if passes > MAX_PASSES || !(1..=MAX_SEARCH_NODES).contains(&self.max_search_nodes) {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "sewing ordering supports at most 20 passes and 1..200000 search nodes",
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
    /// The normal lexicographic topological schedule, before optimization.
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

struct Task<'a> {
    pass: &'a SewingPass,
    predecessors: u32,
    needle: u32,
    thread: u32,
}

struct Search<'a> {
    tasks: Vec<Task<'a>>,
    initial: Option<(u32, u32)>,
    all: u32,
    times: Option<Vec<Vec<usize>>>,
}

struct Frame {
    mask: u32,
    last: Option<usize>,
    cost: usize,
    candidates: Vec<usize>,
    next: usize,
}

impl Search<'_> {
    fn setup(&self, last: Option<usize>) -> Option<(u32, u32)> {
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
    fn count_lower_bound(&self, mask: u32, last: Option<usize>) -> usize {
        let (mut needles, mut threads) = (0_u32, 0_u32);
        for (i, task) in self.tasks.iter().enumerate() {
            if mask & (1 << i) == 0 {
                needles |= task.needle;
                threads |= task.thread;
            }
        }
        let component = |remaining: u32, current: Option<u32>| {
            remaining.count_ones() as usize
                - usize::from(remaining != 0 && current.is_none_or(|v| remaining & v != 0))
        };
        let setup = self.setup(last);
        component(needles, setup.map(|v| v.0)) + component(threads, setup.map(|v| v.1))
    }

    fn cost(&self, last: Option<usize>, next: usize) -> usize {
        self.times.as_ref().map_or_else(
            || self.changes(last, next).total_changes,
            |times| times[last.unwrap_or(self.tasks.len())][next],
        )
    }

    fn total_cost(&self, order: &[usize]) -> usize {
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

    fn lower_bound(&self, mask: u32, last: Option<usize>) -> usize {
        if self.times.is_none() {
            return self.count_lower_bound(mask, last);
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
                    .fold(self.cost(last, j), usize::min)
            })
            .sum()
    }

    fn frame(&self, mask: u32, last: Option<usize>, cost: usize) -> Frame {
        let mut candidates: Vec<_> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(i, task)| {
                mask & (1 << i) == 0 && task.predecessors & mask == task.predecessors
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
    let mut passes: Vec<_> = seams.iter().map(|s| &s.pass).collect();
    passes.sort_by_key(|p| p.operation_id());
    let times = policy
        .setup_times
        .as_ref()
        .map(|times| {
            times.matrix(
                &passes,
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
        .map(|(i, pass)| (pass.operation_id(), i))
        .collect();
    let mut needles = BTreeMap::new();
    let mut threads = BTreeMap::new();
    let mut tasks = Vec::with_capacity(passes.len());
    for pass in passes {
        let next_needle = 1_u32 << needles.len();
        let needle = *needles.entry(pass.needle.as_str()).or_insert(next_needle);
        let next_thread = 1_u32 << threads.len();
        let thread = *threads.entry(pass.thread.as_str()).or_insert(next_thread);
        let predecessors = pass
            .after
            .iter()
            .fold(0, |mask, id| mask | (1 << ids[id.as_str()]));
        tasks.push(Task {
            pass,
            predecessors,
            needle,
            thread,
        });
    }
    let search = Search {
        all: (1_u32 << tasks.len()) - 1,
        tasks,
        times,
        // An explicitly installed identity absent from the job is a known different setup (zero),
        // not the omitted/unknown setup represented by None.
        initial: policy.initial_setup.as_ref().map(|s| {
            (
                needles.get(s.needle.as_str()).copied().unwrap_or(0),
                threads.get(s.thread.as_str()).copied().unwrap_or(0),
            )
        }),
    };
    let baseline: Vec<_> = seams.iter().map(|s| ids[s.pass.operation_id()]).collect();
    let baseline_changes = search.count(&baseline);
    let mut best = baseline.clone();
    let baseline_cost = search.total_cost(&baseline);
    let mut best_cost = baseline_cost;
    let root_bound = search.lower_bound(0, None);
    let mut search_nodes = 0;
    let mut budget_exhausted = false;
    let mut stack = vec![search.frame(0, None, 0)];
    let mut path = Vec::with_capacity(seams.len());
    // Iterative depth-first branch-and-bound: at most 20 frames, each with at most 20 candidates.
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
            .map(|&i| search.tasks[i].pass.operation_id().to_string())
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
            root_bound
        } else {
            best_cost
        },
        timing: search.times.as_ref().map(|_| TimingReport {
            baseline_setup_milliseconds: baseline_cost,
            selected_setup_milliseconds: best_cost,
            lower_bound_setup_milliseconds: if budget_exhausted {
                root_bound
            } else {
                best_cost
            },
            feed_and_setup_seconds: seams.iter().map(|s| s.feed_seconds).sum::<f64>()
                + best_cost as f64 / 1000.0,
        }),
        tie_break: if search.times.is_some() {
            "Retain the incumbent on equal integer setup milliseconds; start with the lexicographic topological schedule, then explore by immediate duration and operation ID."
        } else {
            "Retain the incumbent on equal total changes; start with the lexicographic topological schedule, then explore by immediate total changes and operation ID."
        },
    })
}
