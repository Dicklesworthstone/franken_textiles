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
fn portable_coverage_matches_the_canonical_geometry_network_and_exact_frozen_replay() {
    let input = include_str!("../../../examples/vapor-coverage/layered.json");
    let request: ftx_drape::vapor::coverage::Request = serde_json::from_str(input).unwrap();
    let expected = serde_json::to_string(&request.evaluate().unwrap()).unwrap();
    let output = thermal::vapor_coverage(input).unwrap();
    assert_eq!(output, expected);
    let report: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        thermal::vapor_coverage(report["replay_request_json"].as_str().unwrap()).unwrap(),
        output
    );
    let mut changed: Value =
        serde_json::from_str(report["replay_request_json"].as_str().unwrap()).unwrap();
    changed["source"]["materials"][0]["record"]["name"] = json!("Unmatched source revision");
    assert!(thermal::vapor_coverage(&changed.to_string()).is_err());
    assert_eq!(
        thermal::vapor_coverage("{oops").unwrap_err().code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn portable_moving_coverage_preserves_the_shared_report_and_rejects_stale_sources() {
    let initial: Value = serde_json::from_str(include_str!(
        "../../../examples/vapor-coverage/layered.json"
    ))
    .unwrap();
    let input = json!({"schema":"ftx-vapor-coverage-history/1",
        "policy":{"schema":"ftx-motion-vapor-coverage/1","integration":"right_endpoint","max_geometry_operations":10000},
        "samples":[{"time_s":2,"positions_m":initial["source"]["positions_m"],
            "body_positions_m":initial["map"]["body_positions_m"]}],"initial":initial}).to_string();
    let request: ftx_drape::vapor::coverage::history::Request =
        serde_json::from_str(&input).unwrap();
    let output = thermal::vapor_coverage_history(&input).unwrap();
    assert_eq!(
        output,
        serde_json::to_string(&request.evaluate().unwrap()).unwrap()
    );
    let report: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        thermal::vapor_coverage_history(report["replay_request_json"].as_str().unwrap()).unwrap(),
        output
    );
    assert_eq!(report["latent_heat_coupled"], false);
    let energy = 1000.0 * 0.002 / (2.0 * 11.27 + 5.0) * 2.0;
    assert!((report["totals"]["potential_energy_j"].as_f64().unwrap() - energy).abs() < 1e-12);
    let mut changed: Value =
        serde_json::from_str(report["replay_request_json"].as_str().unwrap()).unwrap();
    changed["initial"]["source"]["materials"][0]["record"]["name"] = json!("Changed specimen");
    assert!(thermal::vapor_coverage_history(&changed.to_string()).is_err());
    assert_eq!(
        thermal::vapor_coverage_history("{oops").unwrap_err().code,
        RefusalCode::InvalidDocument
    );
    assert_eq!(
        thermal::vapor_coverage_history(&" ".repeat(64 * 1024 * 1024 + 1))
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn portable_moving_coverage_honors_the_declared_quadrature_and_exact_replay() {
    let initial: Value = serde_json::from_str(include_str!(
        "../../../examples/vapor-coverage/layered.json"
    ))
    .unwrap();
    let mut cloth = initial["source"]["positions_m"].clone();
    let mut body = initial["map"]["body_positions_m"].clone();
    for positions in [&mut cloth, &mut body] {
        for vertex in positions.as_array_mut().unwrap() {
            vertex[0] = json!(2.0 * vertex[0].as_f64().unwrap());
        }
    }
    for (rule, area_time_factor) in [("right_endpoint", 4.0), ("composite_trapezoid", 3.0)] {
        let input = json!({"schema":"ftx-vapor-coverage-history/1",
            "policy":{"schema":"ftx-motion-vapor-coverage/1","integration":rule,"max_geometry_operations":10000},
            "initial":initial,"samples":[{"time_s":2,"positions_m":cloth,"body_positions_m":body}]}).to_string();
        let output = thermal::vapor_coverage_history(&input).unwrap();
        let report: Value = serde_json::from_str(&output).unwrap();
        // The illustrative boundary and measured Ret give an independent series-network power.
        // Doubling projected area over two seconds yields distinct declared quadrature values.
        let energy = 1000.0 * 0.002 / (2.0 * 11.27 + 5.0) * area_time_factor;
        assert_eq!(report["integration"], rule);
        assert!((report["totals"]["potential_energy_j"].as_f64().unwrap() - energy).abs() < 1e-12);
        assert!(
            (report["totals"]["potential_mass_kg"].as_f64().unwrap() - energy / 2_500_000.0).abs()
                < 1e-18
        );
        assert_eq!(
            thermal::vapor_coverage_history(report["replay_request_json"].as_str().unwrap())
                .unwrap(),
            output
        );
    }
}

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
