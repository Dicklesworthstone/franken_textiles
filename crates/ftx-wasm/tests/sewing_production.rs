//! Native execution of the file-free browser boundary, not a claim of actual JavaScript/WASM execution.

use ftx_export::{
    material_study::Request,
    sewing::{SewOptions, SewPlan, program::SewProgram},
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const OPTIONS: &str = include_str!("../../../examples/sewing-production-options.json");
const TIMED_SELECTION: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-timed-selection.json");

#[test]
fn documented_batch_uses_exact_portable_plan_and_stitch_authorities_with_complete_replay() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let options: SewOptions = serde_json::from_str(OPTIONS).unwrap();
    let plan = SewPlan::build(&pattern, &options).unwrap();
    let program = SewProgram::build(&pattern, &options).unwrap();
    let text = ftx_wasm::manufacturing::sew_plan(PATTERN, OPTIONS).unwrap();
    assert_eq!(text, serde_json::to_string(&plan).unwrap());
    assert_eq!(
        ftx_wasm::manufacturing::sew_stitches(PATTERN, OPTIONS).unwrap(),
        serde_json::to_string(&program).unwrap()
    );
    assert_eq!(
        ftx_wasm::manufacturing::sew_plan(PATTERN, &serde_json::to_string(&plan.options).unwrap())
            .unwrap(),
        text
    );
    assert_eq!(plan.total_stitch_intervals(), 240);
    assert_eq!(plan.total_feed_seconds(), 24.0);
    assert_eq!(
        plan.ordering_report()
            .unwrap()
            .timing
            .as_ref()
            .unwrap()
            .feed_and_setup_seconds,
        25.5
    );
    assert_eq!(program.stitches.len(), 240);
    assert!(program.stitches.iter().all(|step| step.copy.is_some()));

    let mut malformed: Value = serde_json::from_str(OPTIONS).unwrap();
    malformed["consumables"] = json!({"production_copies":2});
    assert!(ftx_wasm::manufacturing::sew_plan(PATTERN, &malformed.to_string()).is_err());
    assert!(ftx_wasm::manufacturing::sew_stitches(PATTERN, &malformed.to_string()).is_err());
}

#[test]
fn portable_recipe_selection_uses_batch_feed_and_shared_setup_not_a_scaled_single_garment_score() {
    let mut request: Value = serde_json::from_str(TIMED_SELECTION).unwrap();
    let ordering = request["options"]
        .as_object_mut()
        .unwrap()
        .remove("ordering")
        .unwrap();
    request["options"]["production"] = json!({"production_copies":3,"ordering":ordering});
    let request = request.to_string();
    let text = ftx_wasm::sew_select(PATTERN, &request).unwrap();
    assert_eq!(
        text,
        ftx_export::sewing_selection::select_json(PATTERN, &request).unwrap()
    );
    let result: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(result["report"]["selected_objective_seconds"], 22.0);
    assert_eq!(
        result["report"]["selected_choices"],
        json!({"join":"fast","finish":"fast"})
    );
    let plan: Value = serde_json::from_str(
        &ftx_wasm::manufacturing::sew_plan(
            PATTERN,
            result["selected_options_json"].as_str().unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(plan, result["report"]["selected_plan"]);
    assert_eq!(plan["production"]["production_copies"], 3);
    assert_eq!(
        plan["production"]["ordering"]["timing"]["selected_setup_milliseconds"],
        10000
    );
    assert_eq!(
        plan["production"]["ordering"]["timing"]["feed_and_setup_seconds"],
        22.0
    );
}

#[test]
fn portable_study_inherits_cutting_quantity_and_preserves_independent_cutting_on_conflict() {
    let mut pattern: Value = serde_json::from_str(PATTERN).unwrap();
    pattern["fabric"]["material"]["record"]["properties"]["width"] = json!({
        "value":0.2,"unit":"m","evidence":"estimate","source":"test",
        "method":"Synthetic stock width for portable production integration"
    });
    let mut options: Value = serde_json::from_str(OPTIONS).unwrap();
    options["production"]
        .as_object_mut()
        .unwrap()
        .remove("production_copies");
    let mut study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"original","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern["fabric"]["material"]}}],
        "cutting":{"copies":3,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0},
        "sewing":options,
        "comparison":{"basis":"nominal","required_consumers":["cutting","sewing"],
            "objectives":[{"metric":"sewing_feed_and_setup_seconds","direction":"minimize"}]}});
    let pattern_text = pattern.to_string();
    let text = ftx_wasm::materials::study(&pattern_text, &study.to_string()).unwrap();
    let core: Request = serde_json::from_value(study.clone()).unwrap();
    assert_eq!(
        text,
        serde_json::to_string(&core.evaluate(&Pattern::new(pattern.clone())).unwrap()).unwrap()
    );
    let report: Value = serde_json::from_str(&text).unwrap();
    let row = &report["alternatives"][0];
    assert_eq!(
        row["sewing"]["result"]["options"]["production"]["production_copies"],
        3
    );
    assert_eq!(row["metrics"]["feed_seconds"], 24.0);
    assert_eq!(row["metrics"]["sewing_feed_and_setup_seconds"], 25.5);
    assert_eq!(report["comparison"]["frontier"], json!(["original"]));
    study["sewing"]["production"]["production_copies"] = json!(2);
    let conflict: Value = serde_json::from_str(
        &ftx_wasm::materials::study(&pattern_text, &study.to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(conflict["alternatives"][0]["cutting"], row["cutting"]);
    assert_eq!(conflict["alternatives"][0]["sewing"]["status"], "refused");
    assert_eq!(conflict["comparison"]["frontier"], json!([]));
}
