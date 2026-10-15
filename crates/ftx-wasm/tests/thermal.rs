//! Native execution of the file-free entry point exported by wasm-bindgen, not browser parity.
use ftx_drape::thermal::steady::request::Request;
use ftx_pattern::materials::Catalog;
use ftx_wasm::thermal;
use serde_json::{Value, json};

const INPUT: &str = include_str!("../../../tests/fixtures/materials/fsri-steady-conduction.json");

#[test]
fn browser_entry_point_matches_shared_workbench_and_replays_frozen_request() {
    let request: Request = serde_json::from_str(INPUT).unwrap();
    let expected = request.evaluate(&Catalog::builtin().unwrap()).unwrap();
    let output = thermal::steady(INPUT).unwrap();
    let actual: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        output,
        thermal::steady(&actual["request"].to_string()).unwrap()
    );
    assert_eq!(actual["storage_required"], false);
    assert_eq!(actual["comfort_claim"], false);
}

#[test]
fn browser_entry_point_preserves_evidence_and_domain_refusals() {
    assert!(thermal::steady("not JSON").is_err());
    let mut input: Value = serde_json::from_str(INPUT).unwrap();
    input["layers"][0]["allow_estimates"] = json!(false);
    assert!(thermal::steady(&input.to_string()).is_err());
    input["layers"][0]["allow_estimates"] = json!(true);
    input["outside_c"] = json!(10.0);
    input["outside_resistance_m2_k_w"] = json!(0.0);
    assert!(thermal::steady(&input.to_string()).is_err());
}
