//! Native execution of the browser's file-free API; no compiled-WASM claim from this test alone.
use ftx_drape::thermal::compression::{Input, evaluate};
use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog};
use serde_json::{Value, json};

fn input() -> Value {
    let mut p: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    p["pressure_pa"] = json!(0.0);
    json!({"schema":"ftx-compressed-thermal-history/1","setup":{
        "stack":{"schema":"ftx-compression-stack/1","gap_m":0.040,"reference_area_m2":0.25,
            "direction":"loading","allow_estimates":true,
            "layers":[{"id":"a","preparation":p},{"id":"b","preparation":p}]},
        "cells_per_layer":2,"initial_temperature_c":[20.0,20.0,20.0,20.0],
        "thermal_evidence":"Synthetic constant-capacity fixture; explicit thermal boundaries; no mechanical heating"},
        "holds":[{"seconds":10.0,"gap_m":0.030,"interface_resistance_m2_k_w":[0.125],
            "inside_c":20.0,"outside_c":20.0,"inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2}]})
}

#[test]
fn shared_api_uses_the_same_history_and_retains_ordinary_selection_identity() {
    let value = input();
    let text = serde_json::to_string(&value).unwrap();
    let actual = ftx_wasm::thermal::compression_history(&text).unwrap();
    let typed: Input = serde_json::from_str(&text).unwrap();
    assert_eq!(
        actual,
        serde_json::to_string(&evaluate(&typed).unwrap()).unwrap()
    );
    let report: Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(report["steps"][0]["temperature_c"], json!(vec![20.0; 4]));
    assert_eq!(report["steps"][0]["stored_change_j"], 0.0);
    assert!((report["steps"][0]["external_work_j"].as_f64().unwrap() - 0.3125).abs() < 1e-12);
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    for (i, layer) in report["final_compression"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let selected = layer["conditioned"]["selection_json"].as_str().unwrap();
        let material = catalog
            .resolve(&serde_json::from_str(selected).unwrap())
            .unwrap();
        assert_eq!(
            report["steps"][0]["layer_material_sha256"][i],
            material.sha256()
        );
        assert!((material.thermal_resistance().unwrap() - 0.375).abs() < 1e-12);
    }
    assert_eq!(text, serde_json::to_string(&value).unwrap());
    assert_eq!(
        actual,
        ftx_wasm::thermal::compression_history(&text).unwrap()
    );
}

#[test]
fn schema_source_and_late_hold_refusals_cross_the_same_api_without_partial_output() {
    let mut value = input();
    let last = value["holds"][0].clone();
    value["holds"].as_array_mut().unwrap().push(last);
    value["holds"][1]["outside_c"] = json!(-300.0);
    assert!(ftx_wasm::thermal::compression_history(&value.to_string()).is_err());
    for mutate in [
        |v: &mut Value| v["schema"] = json!("unknown"),
        |v: &mut Value| {
            v["setup"]["stack"]["layers"][0]["preparation"]["allow_estimates"] = json!(false)
        },
        |v: &mut Value| v["holds"][0]["heat_from_work"] = json!(true),
        |v: &mut Value| v["holds"] = json!([]),
    ] {
        let mut v = input();
        mutate(&mut v);
        assert!(ftx_wasm::thermal::compression_history(&v.to_string()).is_err());
    }
}
