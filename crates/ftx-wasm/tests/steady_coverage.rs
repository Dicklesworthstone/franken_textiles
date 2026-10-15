//! File-free browser path is the native geometry/transport implementation, not a second solver.
use ftx_drape::thermal::steady::coverage::request::Request;
use serde_json::{Value, json};
fn request() -> Value {
    json!({"schema":"ftx-steady-coverage-request/1",
    "report":serde_json::from_str::<Value>(include_str!("../../../tests/fixtures/materials/synthetic-steady-surface-report.json")).unwrap(),
    "map":serde_json::from_str::<Value>(include_str!("../../../tests/fixtures/materials/steady-coverage-map.json")).unwrap(),
    "conditions":serde_json::from_str::<Value>(include_str!("../../../tests/fixtures/materials/steady-coverage-conditions.json")).unwrap()})
}
fn evaluate(r: &Value) -> Result<Value, ftx_core::Refusal> {
    let output = ftx_wasm::thermal::steady_coverage(&r.to_string())?;
    Ok(serde_json::from_str(&output).unwrap())
}
#[test]
fn browser_native_exact_equality_frozen_replay_and_independent_parallel_answer() {
    let mut input = request();
    input["report"]["unrelated_private_metadata"] = json!("not needed for thermal replay");
    let native = serde_json::from_value::<Request>(input.clone())
        .unwrap()
        .evaluate()
        .unwrap();
    let browser = evaluate(&input).unwrap();
    assert_eq!(native, browser);
    let q = browser["steady"]["power_w"].as_f64().unwrap();
    assert!((q - 15.0 / 0.7 - 15.0 / 1.1).abs() < 1e-7);
    assert_eq!(browser["storage_required"], false);
    assert_eq!(browser["request"]["report"].as_object().unwrap().len(), 2);
    assert_eq!(browser, evaluate(&browser["request"]).unwrap());
    assert!(
        browser["projection"]["footprints"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
}
#[test]
fn stale_spatial_boundaries_unknown_layers_tampered_receipts_and_budget_refuse() {
    let input = request();
    let frozen = evaluate(&input).unwrap()["request"].clone();
    let mut bad = frozen.clone();
    bad["map"]["body_positions_m"][0][2] = json!(-0.04);
    assert!(evaluate(&bad).is_err());
    let mut bad = input.clone();
    bad["conditions"]["boundaries"]
        .as_array_mut()
        .unwrap()
        .push(input["conditions"]["boundaries"][0].clone());
    assert!(evaluate(&bad).is_err());
    bad["conditions"]["body_mesh_sha256"] = frozen["conditions"]["body_mesh_sha256"].clone();
    assert!(evaluate(&bad).is_ok());
    let mut bad = input.clone();
    bad["map"]["instance_layers"]["extra"] = json!("shell");
    assert!(evaluate(&bad).is_err());
    let mut bad = input.clone();
    bad["map"]["instance_layers"]
        .as_object_mut()
        .unwrap()
        .remove("front");
    assert!(evaluate(&bad).is_err());
    let mut bad = input.clone();
    bad["report"]["steady_thermal_surface"]["positions_m"][0][0] = json!(3.0);
    assert!(evaluate(&bad).is_err());
    let mut bad = input;
    bad["conditions"]["transient"] = json!({"steps":1});
    assert!(evaluate(&bad).is_err());
    assert!(ftx_wasm::thermal::steady_coverage(&" ".repeat(64 * 1024 * 1024 + 1)).is_err());
}
