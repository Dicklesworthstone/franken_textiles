//! File-free browser adapters retain the native source-qualified pressure result. Synthetic only.
use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
use ftx_pattern::Pattern;
use ftx_wasm::manufacturing::{sew_plan, sew_stitches, sew_svg};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const OPTIONS: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pressure-options.json");

#[test]
fn browser_adapters_share_loaded_stack_receipts_and_preserve_qualification() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let options: SewOptions = serde_json::from_str(OPTIONS).unwrap();
    let plan: Value = serde_json::from_str(&sew_plan(PATTERN, OPTIONS).unwrap()).unwrap();
    assert_eq!(
        plan,
        serde_json::to_value(SewPlan::build(&pattern, &options).unwrap()).unwrap()
    );
    let program: Value = serde_json::from_str(&sew_stitches(PATTERN, OPTIONS).unwrap()).unwrap();
    assert_eq!(
        program,
        serde_json::to_value(SewProgram::build(&pattern, &options).unwrap()).unwrap()
    );
    assert_eq!(program["plan"], plan);
    assert_eq!(plan["schema"], "ftx-sewing-plan/7");
    assert_eq!(plan["seams"][0]["presser_foot"]["stack_mm"], 0.44);
    assert_eq!(
        plan["seams"][0]["qualification_id"],
        "synthetic-pressure-process"
    );
    let preview = sew_svg(PATTERN, OPTIONS, "side").unwrap();
    assert_eq!(
        preview,
        ftx_export::sewing::preview::svg(&pattern, &options, "side").unwrap()
    );
    assert!(preview.contains("Foot pressure 500 Pa"));
}

#[test]
fn browser_adapters_refuse_loaded_limits_without_falling_back_to_uncompressed_review() {
    let mut options: Value = serde_json::from_str(OPTIONS).unwrap();
    options["machine"]["presser_foot"]["max_loaded_stack_mm"] = json!(0.42);
    let options = options.to_string();
    for refusal in [
        sew_plan(PATTERN, &options),
        sew_stitches(PATTERN, &options),
        sew_svg(PATTERN, &options, "side"),
    ] {
        assert!(
            refusal
                .unwrap_err()
                .message
                .contains("loaded machine limit")
        );
    }
}
