//! File-free partial-seam consumers use the native planner and keep directed travel in source frames.
use ftx_export::sewing::{SewOptions, SewPlan, preview, program::SewProgram};
use ftx_pattern::Pattern;
use ftx_wasm::manufacturing::{sew_plan, sew_stitches, sew_svg};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const OPTIONS: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-span-options.json");

#[test]
fn portable_consumers_preserve_exact_span_reports_stitches_and_previews() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let options: SewOptions = serde_json::from_str(OPTIONS).unwrap();
    let plan = SewPlan::build(&pattern, &options).unwrap();
    let program = SewProgram::build(&pattern, &options).unwrap();
    assert_eq!(
        sew_plan(PATTERN, OPTIONS).unwrap(),
        serde_json::to_string(&plan).unwrap()
    );
    assert_eq!(
        sew_stitches(PATTERN, OPTIONS).unwrap(),
        serde_json::to_string(&program).unwrap()
    );
    assert_eq!(plan.schema, "ftx-sewing-plan/10");
    assert_eq!(plan.stitch_intervals, 40);
    assert_eq!(
        plan.seams[0].source_span.as_ref().unwrap().sewn_lengths_mm,
        [60.0, 60.0]
    );
    assert_eq!(
        plan.seams[1].source_span.as_ref().unwrap().source_ranges,
        [[0.6, 1.0], [0.6, 1.0]]
    );
    assert!(!plan.hardware_execution_authorized);
    assert!(!plan.physical_seam_quality_validated);
    for id in ["lower-side", "upper-side"] {
        assert_eq!(
            sew_svg(PATTERN, OPTIONS, id).unwrap(),
            preview::svg(&pattern, &options, id).unwrap()
        );
    }
    assert_eq!(program.stitches.len(), 40);
    assert!(
        program.stitches[..24]
            .iter()
            .all(|step| step.operation == 0 && step.advance_mm[0] > 0.0)
    );
    assert!(
        program.stitches[24..]
            .iter()
            .all(|step| step.operation == 1 && step.advance_mm[0] < 0.0)
    );
}

#[test]
fn portable_consumers_cannot_publish_an_uncovered_seam_or_discard_invalid_spans() {
    for (range, message) in [
        ([0.0, 0.55], "coverage gap"),
        ([0.7, 0.6], "source_range"),
        ([-0.1, 0.6], "source_range"),
    ] {
        let mut options: Value = serde_json::from_str(OPTIONS).unwrap();
        options["passes"][0]["source_range"] = json!(range);
        let options = options.to_string();
        for result in [
            sew_plan(PATTERN, &options),
            sew_stitches(PATTERN, &options),
            sew_svg(PATTERN, &options, "upper-side"),
        ] {
            assert!(result.unwrap_err().message.contains(message));
        }
    }
}
