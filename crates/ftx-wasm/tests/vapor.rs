//! Native tests of the file-free WASM boundary; compiled-worker coverage lives in the web checks.
use ftx_core::RefusalCode;
use ftx_pattern::{
    material_vapor::Request,
    materials::{CATALOG_SCHEMA, Catalog},
};
use ftx_wasm::thermal;
use serde_json::{Value, json};

const INPUT: &str = include_str!("../../../tests/fixtures/materials/leather-vapor-stack.json");

#[test]
fn portable_entry_returns_the_exact_shared_report_and_frozen_offline_replay() {
    let request: Request = serde_json::from_str(INPUT).unwrap();
    let report = request
        .evaluate(&ftx_pattern::textile_import::catalog().unwrap())
        .unwrap();
    let output = thermal::vapor(INPUT).unwrap();
    assert_eq!(output, serde_json::to_string(&report).unwrap());
    assert_eq!(output, thermal::vapor(INPUT).unwrap());
    let replay: Request = serde_json::from_str(&report.replay_request_json).unwrap();
    let empty = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    assert_eq!(
        thermal::vapor(&report.replay_request_json).unwrap(),
        serde_json::to_string(&replay.evaluate(&empty).unwrap()).unwrap()
    );
    assert_eq!(
        report.layers[0].observation.conditions["relative_humidity_percent"],
        "40"
    );
    assert!(!report.physical_validation_claim);
}

#[test]
fn portable_entry_preserves_source_tamper_protocol_and_pressure_refusals() {
    assert_eq!(
        thermal::vapor("{oops").unwrap_err().code,
        RefusalCode::InvalidDocument
    );
    let output: Value = serde_json::from_str(&thermal::vapor(INPUT).unwrap()).unwrap();
    let mut changed = output["replay_request"].clone();
    changed["layers"][0]["selection"]["record"]["properties"]["vapor_resistance"]["value"] =
        json!(99.0);
    assert!(thermal::vapor(&changed.to_string()).is_err());
    for (field, value) in [
        ("schema", json!("ftx-vapor-stack/2")),
        ("area_m2", json!(0.0)),
        ("outside_vapor_pressure_pa", json!(4000.0)),
        ("latent_heat_j_kg", json!(0.0)),
        ("infer_condensation", json!(true)),
    ] {
        let mut input: Value = serde_json::from_str(INPUT).unwrap();
        input[field] = value;
        assert!(thermal::vapor(&input.to_string()).is_err());
    }
    let mut mixed: Value = serde_json::from_str(INPUT).unwrap();
    mixed["layers"].as_array_mut().unwrap().push(json!({
        "key":"second-layer", "selection":{"id":"zagreb/leather-2022/syn-d-h-neck"}
    }));
    assert!(thermal::vapor(&mixed.to_string()).is_err());
    mixed["allow_mixed_protocols"] = json!(true);
    let admitted: Value =
        serde_json::from_str(&thermal::vapor(&mixed.to_string()).unwrap()).unwrap();
    assert_eq!(admitted["mixed_protocols"], true);
    assert_eq!(admitted["layers"].as_array().unwrap().len(), 2);
}

#[test]
fn portable_entry_bounds_input_before_parsing() {
    let oversized = " ".repeat(64 * 1024 * 1024 + 1);
    assert_eq!(
        thermal::vapor(&oversized).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}
