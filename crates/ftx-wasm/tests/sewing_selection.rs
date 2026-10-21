//! Native-callable browser-boundary parity; actual JS/WASM execution is a separate gate.
use serde_json::{Value, json};
const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const REQUEST: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-selection.json");

#[test]
fn file_free_selection_matches_core_and_keeps_exact_replay_options() {
    let text = ftx_wasm::sew_select(PATTERN, REQUEST).unwrap();
    assert_eq!(
        text,
        ftx_export::sewing_selection::select_json(PATTERN, REQUEST).unwrap()
    );
    let job: Value = serde_json::from_str(&text).unwrap();
    let plan: Value = serde_json::from_str(
        &ftx_wasm::manufacturing::sew_plan(PATTERN, job["selected_options_json"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(plan, job["report"]["selected_plan"]);
    assert_eq!(plan["feed_seconds"], 2.0);
}

#[test]
fn absent_incumbents_have_no_options_and_malformed_inputs_refuse() {
    let mut request: Value = serde_json::from_str(REQUEST).unwrap();
    request["options"]["qualifications"][0]["allow_estimates"] = json!(false);
    let text = ftx_wasm::sew_select(PATTERN, &request.to_string()).unwrap();
    let job: Value = serde_json::from_str(&text).unwrap();
    assert!(job["selected_options_json"].is_null());
    assert_eq!(job["report"]["no_feasible_listed_recipe"], true);
    assert!(ftx_wasm::sew_select("{", REQUEST).is_err());
    request["unknown_field"] = json!(true);
    assert!(ftx_wasm::sew_select(PATTERN, &request.to_string()).is_err());
}

#[test]
fn file_free_timed_selection_preserves_complete_plan_and_withdraws_unknown_time_results() {
    let timed =
        include_str!("../../../tests/fixtures/materials/synthetic-sewing-timed-selection.json");
    let text = ftx_wasm::sew_select(PATTERN, timed).unwrap();
    assert_eq!(
        text,
        ftx_export::sewing_selection::select_json(PATTERN, timed).unwrap()
    );
    let job: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(job["report"]["selected_objective_seconds"], 8.0);
    let plan: Value = serde_json::from_str(
        &ftx_wasm::manufacturing::sew_plan(PATTERN, job["selected_options_json"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(plan, job["report"]["selected_plan"]);
    assert_eq!(plan["ordering"]["timing"]["feed_and_setup_seconds"], 8.0);
    assert_eq!(plan["hardware_execution_authorized"], false);
    let mut request: Value = serde_json::from_str(timed).unwrap();
    request["options"]["ordering"]["setup_times"]["allow_estimates"] = json!(false);
    assert!(ftx_wasm::sew_select(PATTERN, &request.to_string()).is_err());
}
