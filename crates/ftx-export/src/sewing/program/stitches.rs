//! Expand a freshly built source-locked plan, never an unvalidated edited report. Operation/run
//! boundaries are retained; no joining movement, needle action or robot-coordinate transform is guessed.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_pattern::Pattern;
use serde::Serialize;

use super::super::{MAX_STATIONS, SewOptions, SewPlan, bad};
use super::compile;

/// One traversed stitch interval, in paired original pattern-side frames (mm), not machine space.
#[derive(Clone, Debug, Serialize)]
pub struct StitchStep {
    /// Index into the source plan's operation-ordered `seams` array. With configured parallel cells,
    /// this is the `plan.parallel_cell_plan(cell)` array, not the outer serial-reference geometry.
    pub operation: usize,
    /// Zero-based physical garment copy when a production schedule was explicitly requested.
    /// Omission retains the existing single-garment program representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy: Option<usize>,
    /// Physical cell for explicitly independent concurrent production. Stitches are serialized by
    /// cell then that cell's operation order; this is not one chronological cross-cell needle stream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    /// Index into that operation's directed `runs` array; its phase identifies lock travel.
    pub run: usize,
    /// Canonical station before this interval, on both material sides.
    pub from_station: usize,
    /// Canonical station after this interval, on both material sides.
    pub to_station: usize,
    /// Destination on side a, in the source instance's local millimetres.
    pub a_mm: Vec2,
    /// Destination on side b, in its own source instance's local millimetres.
    pub b_mm: Vec2,
    /// Shared assembly destination when explicitly registered/folded; not machine coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assembly_mm: Option<Vec2>,
    /// Signed material arclength advances [a, b], in mm; reverse feed makes both negative.
    pub advance_mm: [f64; 2],
    /// Absolute side-b / side-a advance, never inverted by reversal.
    pub feed_b_over_a: f64,
    /// Continuous-feed interval duration, excluding acceleration/turning/handling.
    pub feed_seconds: f64,
    /// First interval of a run; a new operation or direction change needs external handling.
    pub begins_run: bool,
}

/// Fully expanded geometric stitch travel for software/CAM review. No device commands or permission.
#[derive(Clone, Debug, Serialize)]
pub struct SewProgram {
    /// `/1` centerline, `/2` offset, `/3` assembly, `/4` copies, `/5` shared parallel cells,
    /// `/6` separately admitted cell geometry, recipes and rates; `/7` routed garment operations.
    pub schema: &'static str,
    /// Original inputs, frozen source materials, paired station tables and directed operation order.
    pub plan: SewPlan,
    /// Every actual interval, including backtacks/reinforcement, with no inter-operation travel.
    pub stitches: Vec<StitchStep>,
}

impl SewProgram {
    /// Resolve materials and validate the complete job before expanding any output stitch travel.
    /// This calls the same planner as `sew-plan`; it neither trusts a saved report nor copies a solver.
    ///
    /// # Errors
    /// Every [`SewPlan::build`] refusal, or expansion beyond the shared 250000 interval budget.
    pub fn build(pattern: &Pattern, options: &SewOptions) -> Result<Self, Refusal> {
        let mut plan = SewPlan::build(pattern, options)?;
        let total_intervals = plan.total_stitch_intervals();
        if total_intervals > MAX_STATIONS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "expanded sewing program exceeds stitch budget",
            ));
        }
        // Legacy jobs get an explicit forward run here, but `SewPlan::build` keeps their v1 output.
        // Geometry is a capability, not an exclusive version number. Qualification/ordering/foot
        // pressure may coexist with offsets and folded stacks; never erase their plan schema.
        let offset = plan.seams.iter().any(|seam| seam.path.is_some());
        let assembly = plan.seams.iter().any(|seam| seam.assembly.is_some());
        if plan.schema == "ftx-sewing-plan/1" {
            plan.schema = "ftx-sewing-plan/2";
        }
        for seam in &mut plan.seams {
            if seam.runs.is_empty() {
                seam.runs = compile(
                    &seam.stations,
                    [seam.stations.len_a, seam.stations.len_b],
                    &seam.pass,
                    &options.machine,
                )?;
            }
        }
        let sequence: Vec<_> = if let Some(routing) = plan
            .production
            .as_ref()
            .and_then(|production| production.routing.as_ref())
        {
            routing
                .selected
                .operations
                .iter()
                .map(|operation| {
                    (
                        Some(operation.cell.clone()),
                        Some(operation.copy),
                        operation.seam_index,
                    )
                })
                .collect()
        } else if let Some(parallel) = plan
            .production
            .as_ref()
            .and_then(|production| production.parallel.as_ref())
        {
            let mut sequence = Vec::new();
            for cell in &parallel.selected.cells {
                if let Some(schedule) = &cell.schedule {
                    let by_id: BTreeMap<_, _> = schedule
                        .operations
                        .iter()
                        .map(|operation| (operation.id.as_str(), operation))
                        .collect();
                    for id in &schedule.ordering.selected_order {
                        let operation = by_id[id.as_str()];
                        sequence.push((
                            Some(cell.cell.clone()),
                            Some(cell.copies[operation.copy]),
                            operation.seam_index,
                        ));
                    }
                }
            }
            sequence
        } else if let Some(production) = &plan.production {
            let by_id: BTreeMap<_, _> = production
                .operations
                .iter()
                .map(|operation| (operation.id.as_str(), operation))
                .collect();
            production
                .ordering
                .selected_order
                .iter()
                .map(|id| {
                    let operation = by_id[id.as_str()];
                    (None, Some(operation.copy), operation.seam_index)
                })
                .collect()
        } else {
            (0..plan.seams.len()).map(|i| (None, None, i)).collect()
        };
        let mut stitches = Vec::with_capacity(total_intervals);
        for (cell, copy, operation) in sequence {
            let source = cell.as_deref().map_or(&plan, |cell| {
                plan.parallel_cell_plan(cell)
                    .expect("admitted parallel cell")
            });
            let seam = &source.seams[operation];
            for (run, travel) in seam.runs.iter().enumerate() {
                let mut from = travel.from_station;
                while from != travel.to_station {
                    let to = if from < travel.to_station {
                        from + 1
                    } else {
                        from - 1
                    };
                    let advance_mm = [
                        (seam.stations.fa[to] - seam.stations.fa[from]) * seam.stations.len_a,
                        (seam.stations.fb[to] - seam.stations.fb[from]) * seam.stations.len_b,
                    ];
                    stitches.push(StitchStep {
                        operation,
                        copy,
                        cell: cell.clone(),
                        run,
                        from_station: from,
                        to_station: to,
                        a_mm: seam.stations.a[to],
                        b_mm: seam.stations.b[to],
                        assembly_mm: seam.assembly.as_ref().map(|a| a.needle_mm[to]),
                        advance_mm,
                        feed_b_over_a: advance_mm[1] / advance_mm[0],
                        feed_seconds: 60.0 / seam.pass.stitches_per_minute,
                        begins_run: from == travel.from_station,
                    });
                    from = to;
                }
            }
        }
        if stitches.len() != total_intervals {
            return Err(bad("inconsistent sewing program interval accounting"));
        }
        Ok(Self {
            schema: if plan
                .production
                .as_ref()
                .is_some_and(|production| production.routing.is_some())
            {
                "ftx-sewing-program/7"
            } else if plan
                .production
                .as_ref()
                .and_then(|production| production.parallel.as_ref())
                .is_some_and(|parallel| !parallel.cell_plans.is_empty())
            {
                "ftx-sewing-program/6"
            } else if plan
                .production
                .as_ref()
                .is_some_and(|production| production.parallel.is_some())
            {
                "ftx-sewing-program/5"
            } else if plan.production.is_some() {
                "ftx-sewing-program/4"
            } else if assembly {
                "ftx-sewing-program/3"
            } else if offset {
                "ftx-sewing-program/2"
            } else {
                "ftx-sewing-program/1"
            },
            plan,
            stitches,
        })
    }
}
