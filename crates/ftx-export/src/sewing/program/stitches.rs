//! Expand a freshly built source-locked plan, never an unvalidated edited report. Operation/run
//! boundaries are retained; no joining movement, needle action or robot-coordinate transform is guessed.

use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_pattern::Pattern;
use serde::Serialize;

use super::super::{MAX_STATIONS, SewOptions, SewPlan, bad};
use super::compile;

/// One traversed stitch interval, in paired original pattern-side frames (mm), not machine space.
#[derive(Clone, Debug, Serialize)]
pub struct StitchStep {
    /// Index into the retained plan's operation-ordered `seams` array.
    pub operation: usize,
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
    /// `/1` centerline, `/2` offset geometry, `/3` shared flat-fold needle/material correspondence.
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
        if plan.stitch_intervals > MAX_STATIONS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "expanded sewing program exceeds stitch budget",
            ));
        }
        // Legacy jobs get an explicit forward run here, but `SewPlan::build` keeps their v1 output.
        let offset = plan.schema == "ftx-sewing-plan/3";
        let assembly = plan.schema == "ftx-sewing-plan/4";
        if !offset && !assembly {
            plan.schema = "ftx-sewing-plan/2";
        }
        let mut stitches = Vec::with_capacity(plan.stitch_intervals);
        for (operation, seam) in plan.seams.iter_mut().enumerate() {
            if seam.runs.is_empty() {
                seam.runs = compile(
                    &seam.stations,
                    [seam.stations.len_a, seam.stations.len_b],
                    &seam.pass,
                    &options.machine,
                )?;
            }
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
        if stitches.len() != plan.stitch_intervals {
            return Err(bad("inconsistent sewing program interval accounting"));
        }
        Ok(Self {
            schema: if assembly {
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
