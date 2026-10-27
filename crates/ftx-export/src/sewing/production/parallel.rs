//! Whole garments stay inside explicitly independent qualified cells. Each cell reuses the serial
//! production solver, including its own setup, handling and finite fixture inventory. Integer
//! makespan optimization uses the caller's explicit once-per-cell feed-ceiling time convention.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use ftx_core::{Refusal, RefusalCode, receipt::Sha256};
use ftx_pattern::Pattern;
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{handling, workholding};
use crate::sewing::{
    MAX_STATIONS, SewOptions, SewPlan, SewingMachine, SewingQualification, access, bad, ordering,
    qualified_text, sha256_text,
};
use crate::sewing_selection::Recipe;

const MAX_CELLS: usize = 8;
const MAX_WORK: usize = 200_000;
const MAX_EXACT_MILLISECONDS: u64 = 9_007_199_254_740_991;

/// Independent physical cells with an explicitly qualified shared (v1) or per-cell (v2) configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `independent_whole_garment_cells/1` or `/2`; no garment transfers are represented.
    pub model: String,
    /// `minimize_makespan`, over complete cell assignments and their serial schedules.
    pub objective: String,
    /// `ceil_cell_feed_milliseconds/1`: round total continuous feed upward once per used cell.
    pub time_basis: String,
    /// `independent_machines_fixtures_and_handling_labor/1`, covered by the retained source.
    /// A shared operator or shared fixture pool cannot satisfy this declaration.
    pub independence: String,
    /// Shared upper bound for count schedules, baseline/selected replay and allocation transitions.
    pub max_search_nodes: usize,
    /// Complete outer serial-reference configuration; v1 shares it and v2 locks each cell separately.
    pub machine_sha256: String,
    /// Actual outer serial-reference qualifications; v2 additionally locks each cell's own sources.
    pub qualifications: BTreeMap<String, Vec<workholding::QualificationRef>>,
    /// One to eight separately identified physical cells, including their independent inventories.
    pub cells: Vec<Cell>,
    /// Source qualifying these instances, their declared configurations and concurrent independence.
    pub source: Source,
    /// Qualification protocol; must distinguish machine, fixtures and handling labor.
    pub method: String,
    /// Exact source applicability conditions, without inferred live-factory state.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Evidence class independent of the underlying sewing and timing qualifications.
    pub evidence: Evidence,
    /// Separate consent for an estimated cell independence/applicability declaration.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// One physical instance of a fully qualified configuration. No per-operation transfers occur.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    /// Stable physical identity, unique across this concurrent job.
    pub id: String,
    /// Explicit time at which this cell is available, from the common job origin, at most one day.
    pub available_at_milliseconds: u32,
    /// Its explicit initial setup and complete directed time table; objective must include handling.
    pub ordering: ordering::Policy,
    /// Independent observed loading, transitions and final unloading for this cell.
    pub handling: handling::Policy,
    /// Independent fixture inventory, never multiplied from the single-machine reference policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workholding: Option<workholding::Policy>,
    /// Version-2 independent cell configuration and source-qualified process recipes. Version 1
    /// retains the parent geometry/configuration; version 2 requires this field for every cell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Configuration>,
}

/// A source-qualified cell configuration. Pattern topology, assemblies and garment identity stay
/// fixed; explicit recipe overrides change the actual needle, yarn, stitch pitch and feed rate.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    /// Complete machine envelope for this cell, admitted by the ordinary sewing planner.
    pub machine: SewingMachine,
    /// Full typed machine lock; a reused configuration name cannot preserve changed applicability.
    pub machine_sha256: String,
    /// Source process qualifications for this cell's exact materials, machine and recipe choices.
    pub qualifications: Vec<SewingQualification>,
    /// Full qualification locks covering each actually admitted operation on this physical cell.
    pub qualification_refs: BTreeMap<String, Vec<workholding::QualificationRef>>,
    /// Explicit fixed overrides by original operation ID. Omitted operations inherit the current
    /// parent template/candidate recipe and must still pass this cell's own qualifications.
    #[serde(default)]
    pub recipes: BTreeMap<String, Recipe>,
    /// Independently qualified tool/fixture geometry for existing operations. Omission retains
    /// parent access; changing machine never silently drops an inherited clearance requirement.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub access: BTreeMap<String, access::Policy>,
}

impl Cell {
    /// Prepare one ordinary source job for this cell without a recursive production, ordering or
    /// purchasing request. Explicit recipe overrides stay fixed; every other recipe is inherited.
    /// The returned options still need ordinary [`SewPlan::build`] admission on the actual pattern.
    ///
    /// # Errors
    /// Configuration locks or referenced operation/recipe identities are invalid.
    pub fn configured_options(&self, parent: &SewOptions) -> Result<Option<SewOptions>, Refusal> {
        let Some(configuration) = &self.configuration else {
            return Ok(None);
        };
        if !sha256_text(&configuration.machine_sha256)
            || configuration.machine_sha256 != workholding::machine_sha256(&configuration.machine)?
        {
            return Err(bad(
                "parallel cell machine configuration content lock mismatch",
            ));
        }
        let ids: BTreeSet<_> = parent
            .passes
            .iter()
            .map(|pass| pass.operation_id())
            .collect();
        if configuration
            .recipes
            .keys()
            .chain(configuration.access.keys())
            .any(|id| !ids.contains(id.as_str()))
            || configuration
                .recipes
                .values()
                .any(|recipe| !qualified_text(&recipe.id) || recipe.id == "template")
        {
            return Err(bad(
                "parallel cell overrides need existing operation IDs and nonreserved recipe identities",
            ));
        }
        let mut options = SewOptions {
            machine: configuration.machine.clone(),
            passes: parent.passes.clone(),
            ordering: None,
            production: None,
            qualifications: configuration.qualifications.clone(),
            require_qualification: true,
            consumables: None,
            seam_tolerance_mm: parent.seam_tolerance_mm,
            seam_allowance_mm: parent.seam_allowance_mm,
        };
        for pass in &mut options.passes {
            if let Some(recipe) = configuration.recipes.get(pass.operation_id()) {
                recipe.apply(pass);
            }
            if let Some(access) = configuration.access.get(pass.operation_id()) {
                pass.access = Some(access.clone());
            }
        }
        Ok(Some(options))
    }
}

/// One admitted cell-specific source plan, retained once rather than per copy/count schedule.
#[derive(Clone, Debug, Serialize)]
pub struct CellPlan {
    /// Physical cell identity used by assignments and emitted stitch streams.
    pub cell: String,
    /// Full configuration digest qualified for this cell.
    pub machine_sha256: String,
    /// Actual admitted full source qualification locks, distinct from the outer serial reference.
    pub qualifications: BTreeMap<String, workholding::QualificationRef>,
    /// Fresh ordinary source geometry, materials, recipes and machine admission. Contains no
    /// production or consumables request; cell schedules reference its own seam indices.
    pub plan: Box<SewPlan>,
}

/// Complete serial schedule at one cell and the exact mapping to physical garment copies.
#[derive(Clone, Debug, Serialize)]
pub struct CellSchedule {
    /// Physical cell identity from the qualification.
    pub cell: String,
    /// Local schedule copy index -> global physical garment index. Every garment appears once.
    pub copies: Vec<usize>,
    /// Declared common-origin availability; unused cells perform no work and have no finish time.
    pub available_at_milliseconds: u32,
    /// Exact inner search cap used, retained alongside the unchanged source policy for replay.
    pub schedule_node_limit: usize,
    /// Once-per-used-cell ceiling of generated feed under the explicitly chosen millisecond model.
    pub scheduled_feed_milliseconds: u64,
    /// Numerical feed-ceiling padding, in [0, 0.001) seconds; not additional physical work.
    pub feed_padding_seconds: f64,
    /// Common-origin completion of a used cell including its actual final unloading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_milliseconds: Option<u64>,
    /// Complete ordinary schedule with cell-local IDs and independently qualified resource history.
    /// Version 2 seam indices resolve in this cell's retained `CellPlan`, not the outer plan.
    /// The `copies` map applies to every operation, dependency, hold, handling transition and stitch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<super::Report>,
}

/// A complete feasible whole-garment allocation, with elapsed time separate from aggregate work.
#[derive(Clone, Debug, Serialize)]
pub struct Assignment {
    /// Cells in lexicographic physical-ID order, including explicitly idle cells.
    pub cells: Vec<CellSchedule>,
    /// Maximum used-cell finish under the stated planning convention, not a sum of durations.
    pub makespan_milliseconds: u64,
    /// Aggregate generated continuous feed across the assigned physical copies, without rounding.
    pub feed_seconds: f64,
    /// Sum of once-per-cell feed ceilings; distinct from the elapsed makespan.
    pub scheduled_feed_milliseconds: u64,
    /// Aggregate actual selected-cell setup work, including each used cell's initial setup.
    pub setup_milliseconds: u64,
    /// Aggregate actual selected-cell handling work, including each used cell's final unload.
    pub handling_milliseconds: u64,
}

/// Bounded assignment with exact replay and proof conditional on the declared integer time model.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `/1` shared source geometry; `/2` separately configured cell source plans.
    pub schema: &'static str,
    /// Same explicit time convention as the qualified input.
    pub time_basis: &'static str,
    /// Complete outer reference configuration digest; cell/concurrency evidence stays in options.
    pub machine_sha256: String,
    /// Exact outer reference qualifications; v2 cell plans retain their distinct actual sources.
    pub qualifications: BTreeMap<String, workholding::QualificationRef>,
    /// Version-2 cell-specific source geometry/configuration, including finally idle cells.
    /// Version 1 omits this field and references the parent plan's single source station table.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cell_plans: Vec<CellPlan>,
    /// All copies on the first lexicographic cell, with its complete selected serial schedule.
    pub baseline: Assignment,
    /// Complete best feasible assignment, retaining every used cell's full schedule.
    pub selected: Assignment,
    /// Allocation optimum over the valid lower bounds of every cell/count serial subproblem.
    pub lower_bound_makespan_milliseconds: u64,
    /// Actual nested examinations, including reconstruction, plus both allocation DP transitions.
    pub search_nodes: usize,
    /// All nested schedules and allocation subproblems completed their declared search.
    pub search_complete: bool,
    /// The incumbent meets the global lower bound, or all inner searches and allocation completed.
    pub optimality_proven: bool,
    /// At least one nested serial search stopped at its assigned work limit without a proof.
    pub budget_exhausted: bool,
    /// Whole garments remain in one cell; this report never schedules or qualifies transfers.
    pub intercell_transfers: usize,
    /// Source qualification and software scheduling do not grant device execution permission.
    pub hardware_execution_authorized: bool,
}

struct Budget {
    per_trial: usize,
    allocation: usize,
    bound: usize,
}

fn budget(message: &str) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}

impl Policy {
    fn budget(&self, copies: usize, serial_nodes: usize) -> Result<Budget, Refusal> {
        if !(1..=MAX_CELLS).contains(&self.cells.len())
            || !(1..=63).contains(&copies)
            || !(1..=MAX_WORK).contains(&self.max_search_nodes)
        {
            return Err(budget(
                "parallel production supports 1..8 cells, 1..63 copies and at most 200000 total scheduling work units",
            ));
        }
        // Two DPs (incumbent and rigorous subproblem bounds), including idle-cell choices.
        let allocation = self.cells.len() * (copies + 1) * (copies + 2);
        let trials = self.cells.len() * copies;
        let replays = 1 + self.cells.len().min(copies);
        let remaining = self
            .max_search_nodes
            .checked_sub(allocation)
            .ok_or_else(|| {
                budget("parallel work limit cannot cover the complete allocation dynamic programs")
            })?;
        let per_trial = remaining / (trials + replays);
        if per_trial == 0 {
            return Err(budget(
                "parallel work limit must reserve at least one search node for every cell/copy-count schedule and baseline/selected replay",
            ));
        }
        let bound = serial_nodes
            .checked_add(self.max_search_nodes)
            .ok_or_else(|| budget("parallel scheduling work reservation overflows"))?;
        if bound > MAX_WORK {
            return Err(budget(
                "serial reference, all parallel cell schedules and allocation work together exceed 200000; reduce the declared node limits",
            ));
        }
        Ok(Budget {
            per_trial,
            allocation,
            bound,
        })
    }

    /// Complete configured work reservation, including the outer serial reference and all nested
    /// count/cell solves. Consumers must multiply this once across their own declared alternatives.
    ///
    /// # Errors
    /// Bounds, checked allocation arithmetic or total work exceed the supported policy.
    pub fn work_bound(&self, copies: usize, serial_nodes: usize) -> Result<usize, Refusal> {
        Ok(self.budget(copies, serial_nodes)?.bound)
    }

    fn validate(
        &self,
        plan: &SewPlan,
    ) -> Result<BTreeMap<String, workholding::QualificationRef>, Refusal> {
        let configured = self.model == "independent_whole_garment_cells/2";
        if (!configured && self.model != "independent_whole_garment_cells/1")
            || self.objective != "minimize_makespan"
            || self.time_basis != "ceil_cell_feed_milliseconds/1"
            || self.independence != "independent_machines_fixtures_and_handling_labor/1"
            || !sha256_text(&self.machine_sha256)
            || self.machine_sha256 != workholding::machine_sha256(&plan.options.machine)?
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
                "invalid parallel model, explicit timing/independence declaration, source or full machine lock",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated parallel-cell independence needs its own explicit consent",
            ));
        }
        if self.qualifications.values().map(Vec::len).sum::<usize>() > 256 {
            return Err(budget(
                "parallel cells support at most 256 qualification references",
            ));
        }
        let mut cells = BTreeSet::new();
        for cell in &self.cells {
            if !qualified_text(&cell.id)
                || !cells.insert(&cell.id)
                || cell.available_at_milliseconds > 86_400_000
                || cell.ordering.objective != ordering::Objective::MinimizeProductionTime
                || cell.configuration.is_some() != configured
            {
                return Err(bad(
                    "parallel cells need unique physical IDs, explicit availability within one day and complete production-time ordering",
                ));
            }
            // Validate the caller's complete policy before assigning a smaller shared-work cap.
            // Budget partitioning must not conceal malformed original limits or timing evidence.
            cell.ordering.validate_production(
                plan.seams.len(),
                cell.configuration
                    .as_ref()
                    .map_or(&plan.options.machine.id, |configuration| {
                        &configuration.machine.id
                    }),
            )?;
        }
        qualification_refs(plan, &self.qualifications)
    }
}

fn qualification_refs(
    plan: &SewPlan,
    references: &BTreeMap<String, Vec<workholding::QualificationRef>>,
) -> Result<BTreeMap<String, workholding::QualificationRef>, Refusal> {
    if references.values().map(Vec::len).sum::<usize>() > 256 {
        return Err(budget(
            "parallel cells support at most 256 qualification references per configuration",
        ));
    }
    let operations: BTreeSet<_> = plan
        .seams
        .iter()
        .map(|seam| seam.pass.operation_id())
        .collect();
    if references
        .keys()
        .any(|id| !operations.contains(id.as_str()))
    {
        return Err(bad(
            "parallel applicability names an unknown original operation",
        ));
    }
    let declared: BTreeMap<_, _> = plan
        .options
        .qualifications
        .iter()
        .map(|q| (q.id.as_str(), q))
        .collect();
    let mut admitted = BTreeMap::new();
    for seam in &plan.seams {
        let operation = seam.pass.operation_id();
        let refs = references.get(operation).ok_or_else(|| {
            Refusal::new(
                RefusalCode::NotConverged,
                format!("{operation}: missing parallel-cell qualification evidence"),
            )
        })?;
        let mut seen = BTreeSet::new();
        for reference in refs {
            let qualification = declared
                .get(reference.id.as_str())
                .ok_or_else(|| bad("parallel policy references an absent sewing qualification"))?;
            if !sha256_text(&reference.sha256)
                || !seen.insert(reference.id.as_str())
                || reference.sha256 != workholding::qualification_sha256(qualification)?
            {
                return Err(bad(
                    "parallel sewing qualification content lock mismatch or duplicate reference",
                ));
            }
        }
        let actual = seam.qualification_id.as_ref().and_then(|id| refs.iter().find(|r| &r.id == id))
                .ok_or_else(|| Refusal::new(RefusalCode::NotConverged, format!("{operation}: actual admitted qualification is not qualified for all parallel cells")))?;
        admitted.insert(operation.to_string(), actual.clone());
    }
    Ok(admitted)
}

// Fixed-size receipts keep source operation and fixture histories out of the count cache.
#[derive(Clone, Debug, PartialEq)]
struct Trial {
    node_limit: usize,
    feed_seconds: f64,
    setup_milliseconds: u64,
    handling_milliseconds: u64,
    feed_milliseconds: u64,
    padding_seconds: f64,
    finish: u64,
    lower: u64,
    search_nodes: usize,
    search_complete: bool,
    budget_exhausted: bool,
    schedule_sha256: [u8; 32],
}

struct ScheduleDigest(Sha256);

impl Write for ScheduleDigest {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn schedule_sha256(schedule: &super::Report) -> Result<[u8; 32], Refusal> {
    let mut digest = ScheduleDigest(Sha256::new());
    serde_json::to_writer(&mut digest, schedule)
        .map_err(|error| bad(format!("parallel schedule digest: {error}")))?;
    Ok(digest.0.finalize())
}

fn sum_time(a: u64, b: u64) -> Result<u64, Refusal> {
    a.checked_add(b)
        .filter(|&value| value <= MAX_EXACT_MILLISECONDS)
        .ok_or_else(|| {
            budget("parallel time exceeds exactly representable JSON millisecond bounds")
        })
}

fn trial(
    plan: &SewPlan,
    cell: &Cell,
    copies: usize,
    node_limit: usize,
    pattern: &Pattern,
) -> Result<(Trial, super::Report), Refusal> {
    let mut ordering = cell.ordering.clone();
    ordering.max_search_nodes = node_limit;
    let policy = super::Policy {
        production_copies: Some(copies),
        ordering,
        workholding: cell.workholding.clone(),
        handling: Some(cell.handling.clone()),
        parallel: None,
    };
    let schedule = super::evaluate(plan, &policy, pattern)?;
    let feed = schedule.feed_seconds * 1000.0;
    let rounded = feed.ceil();
    if !rounded.is_finite() || !(0.0..=MAX_EXACT_MILLISECONDS as f64).contains(&rounded) {
        return Err(budget(
            "parallel feed duration exceeds exactly representable millisecond bounds",
        ));
    }
    let feed_milliseconds = rounded as u64;
    let setup = schedule
        .ordering
        .timing
        .as_ref()
        .expect("qualified timed schedule");
    let handling = schedule
        .ordering
        .handling_timing
        .as_ref()
        .expect("qualified handling schedule");
    let start = sum_time(u64::from(cell.available_at_milliseconds), feed_milliseconds)?;
    let finish = sum_time(
        start,
        sum_time(
            setup.selected_setup_milliseconds,
            handling.selected_handling_milliseconds,
        )?,
    )?;
    let lower = sum_time(start, handling.lower_bound_setup_and_handling_milliseconds)?;
    let receipt = Trial {
        node_limit,
        feed_seconds: schedule.feed_seconds,
        setup_milliseconds: setup.selected_setup_milliseconds,
        handling_milliseconds: handling.selected_handling_milliseconds,
        feed_milliseconds,
        padding_seconds: (rounded - feed) / 1000.0,
        finish,
        lower,
        search_nodes: schedule.ordering.search_nodes,
        search_complete: schedule.ordering.search_complete,
        budget_exhausted: schedule.ordering.budget_exhausted,
        schedule_sha256: schedule_sha256(&schedule)?,
    };
    Ok((receipt, schedule))
}

// Dynamic programming over complete count assignments. A cell's zero-count cost is exactly zero:
// no unused cell's availability, first setup/load or final unload enters the objective.
fn allocate(trials: &[Vec<Trial>], copies: usize, lower: bool) -> (u64, Vec<usize>, usize) {
    let mut costs = vec![u64::MAX; copies + 1];
    costs[0] = 0;
    let mut parents = vec![vec![0; copies + 1]; trials.len()];
    let mut work = 0;
    for (cell, row) in trials.iter().enumerate() {
        let mut next = vec![u64::MAX; copies + 1];
        for total in 0..=copies {
            for count in 0..=total {
                work += 1;
                if costs[total - count] == u64::MAX {
                    continue;
                }
                let finish = if count == 0 {
                    0
                } else if lower {
                    row[count - 1].lower
                } else {
                    row[count - 1].finish
                };
                let candidate = costs[total - count].max(finish);
                if candidate < next[total] {
                    next[total] = candidate;
                    parents[cell][total] = count;
                }
            }
        }
        costs = next;
    }
    let mut allocation = vec![0; trials.len()];
    let mut total = copies;
    for cell in (0..trials.len()).rev() {
        allocation[cell] = parents[cell][total];
        total -= allocation[cell];
    }
    (costs[copies], allocation, work)
}

fn assignment(
    plan: &SewPlan,
    cell_plans: &[CellPlan],
    cells: &[&Cell],
    trials: &[Vec<Trial>],
    counts: &[usize],
    search_nodes: &mut usize,
    pattern: &Pattern,
) -> Result<Assignment, Refusal> {
    let mut out = Assignment {
        cells: Vec::with_capacity(cells.len()),
        makespan_milliseconds: 0,
        feed_seconds: 0.0,
        scheduled_feed_milliseconds: 0,
        setup_milliseconds: 0,
        handling_milliseconds: 0,
    };
    let mut first_copy = 0;
    for ((cell, row), &count) in cells.iter().zip(trials).zip(counts) {
        let active = count.checked_sub(1).map(|index| &row[index]);
        let copies = (first_copy..first_copy + count).collect();
        first_copy += count;
        let schedule = if let Some(expected) = active {
            let source = cell_plans
                .iter()
                .find(|source| source.cell == cell.id)
                .map_or(plan, |source| source.plan.as_ref());
            let (replayed, schedule) = trial(source, cell, count, expected.node_limit, pattern)?;
            if &replayed != expected {
                return Err(Refusal::new(
                    RefusalCode::NotConverged,
                    "parallel baseline/selected schedule did not exactly replay its cached receipt",
                ));
            }
            *search_nodes += replayed.search_nodes;
            out.makespan_milliseconds = out.makespan_milliseconds.max(replayed.finish);
            out.feed_seconds += replayed.feed_seconds;
            out.scheduled_feed_milliseconds =
                sum_time(out.scheduled_feed_milliseconds, replayed.feed_milliseconds)?;
            out.setup_milliseconds = sum_time(out.setup_milliseconds, replayed.setup_milliseconds)?;
            out.handling_milliseconds =
                sum_time(out.handling_milliseconds, replayed.handling_milliseconds)?;
            Some(schedule)
        } else {
            None
        };
        out.cells.push(CellSchedule {
            cell: cell.id.clone(),
            copies,
            available_at_milliseconds: cell.available_at_milliseconds,
            schedule_node_limit: active.map_or(0, |trial| trial.node_limit),
            scheduled_feed_milliseconds: active.map_or(0, |trial| trial.feed_milliseconds),
            feed_padding_seconds: active.map_or(0.0, |trial| trial.padding_seconds),
            finish_milliseconds: active.map(|trial| trial.finish),
            schedule,
        });
    }
    Ok(out)
}

pub(super) fn evaluate(
    plan: &SewPlan,
    policy: &Policy,
    copies: usize,
    pattern: &Pattern,
) -> Result<Report, Refusal> {
    let serial_nodes = plan
        .options
        .production
        .as_ref()
        .expect("production caller")
        .ordering
        .max_search_nodes;
    let reservation = policy.budget(copies, serial_nodes)?;
    let qualifications = policy.validate(plan)?;
    // The outer production report is not attached to plan yet: its source interval count must be
    // expanded explicitly. Reuse the ordinary stitch-program bound before any count/cell search.
    if plan
        .stitch_intervals
        .checked_mul(copies)
        .is_none_or(|intervals| intervals > MAX_STATIONS)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "parallel production exceeds the ordinary expanded stitch-program interval budget",
        ));
    }
    let mut cells: Vec<_> = policy.cells.iter().collect();
    cells.sort_by_key(|cell| &cell.id);
    let mut cell_plans = Vec::new();
    let mut station_count: usize = plan.seams.iter().map(|seam| seam.stations.fa.len()).sum();
    for cell in &cells {
        if let Some(options) = cell.configured_options(&plan.options)? {
            let source = SewPlan::build(pattern, &options)?;
            // Every count, including all copies on this cell, participates in the bounded search.
            // Checking its actual geometry also bounds the interval count of every mixed allocation.
            if source
                .stitch_intervals
                .checked_mul(copies)
                .is_none_or(|intervals| intervals > MAX_STATIONS)
            {
                return Err(budget(
                    "configured parallel cell exceeds the ordinary expanded stitch-program interval budget",
                ));
            }
            station_count = station_count
                .checked_add(
                    source
                        .seams
                        .iter()
                        .map(|seam| seam.stations.fa.len())
                        .sum::<usize>(),
                )
                .ok_or_else(|| budget("parallel source station count overflows"))?;
            if station_count > MAX_STATIONS {
                return Err(budget(
                    "serial reference and configured cell source plans together exceed 250000 station slots",
                ));
            }
            let configuration = cell.configuration.as_ref().expect("configured cell");
            cell_plans.push(CellPlan {
                cell: cell.id.clone(),
                machine_sha256: configuration.machine_sha256.clone(),
                qualifications: qualification_refs(&source, &configuration.qualification_refs)?,
                plan: Box::new(source),
            });
        }
    }
    let mut trials = Vec::with_capacity(cells.len());
    let mut search_nodes = 0;
    let mut search_complete = true;
    let mut budget_exhausted = false;
    for cell in &cells {
        let source = cell_plans
            .iter()
            .find(|source| source.cell == cell.id)
            .map_or(plan, |source| source.plan.as_ref());
        let node_limit = cell.ordering.max_search_nodes.min(reservation.per_trial);
        let mut row = Vec::with_capacity(copies);
        for count in 1..=copies {
            let (receipt, _) = trial(source, cell, count, node_limit, pattern)?;
            search_nodes += receipt.search_nodes;
            search_complete &= receipt.search_complete;
            budget_exhausted |= receipt.budget_exhausted;
            row.push(receipt);
        }
        trials.push(row);
    }
    let mut baseline_counts = vec![0; cells.len()];
    baseline_counts[0] = copies;
    let baseline = assignment(
        plan,
        &cell_plans,
        &cells,
        &trials,
        &baseline_counts,
        &mut search_nodes,
        pattern,
    )?;
    let (cost, counts, incumbent_work) = allocate(&trials, copies, false);
    let selected = if cost < baseline.makespan_milliseconds {
        assignment(
            plan,
            &cell_plans,
            &cells,
            &trials,
            &counts,
            &mut search_nodes,
            pattern,
        )?
    } else {
        baseline.clone()
    };
    let (lower_bound_makespan_milliseconds, _, bound_work) = allocate(&trials, copies, true);
    search_nodes += incumbent_work + bound_work;
    debug_assert_eq!(incumbent_work + bound_work, reservation.allocation);
    debug_assert!(search_nodes <= policy.max_search_nodes);
    let optimality_proven = selected.makespan_milliseconds == lower_bound_makespan_milliseconds;
    Ok(Report {
        schema: if cell_plans.is_empty() {
            "ftx-sewing-parallel/1"
        } else {
            "ftx-sewing-parallel/2"
        },
        time_basis: "ceil_cell_feed_milliseconds/1",
        machine_sha256: policy.machine_sha256.clone(),
        qualifications,
        cell_plans,
        baseline,
        selected,
        lower_bound_makespan_milliseconds,
        search_nodes,
        search_complete,
        optimality_proven,
        budget_exhausted,
        intercell_transfers: 0,
        hardware_execution_authorized: false,
    })
}
