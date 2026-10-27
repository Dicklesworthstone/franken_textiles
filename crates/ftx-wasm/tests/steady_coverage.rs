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
fn qualified_bridge_power_reaches_the_portable_coverage_result_and_frozen_replay() {
    let mut input = request();
    let baseline = evaluate(&input).unwrap();
    input["thermal_bridges"] = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-steady-thermal-bridges.json"
    ))
    .unwrap();
    let result = evaluate(&input).unwrap();
    assert_eq!(
        result,
        serde_json::from_value::<Request>(input.clone())
            .unwrap()
            .evaluate()
            .unwrap()
    );
    assert_eq!(
        result["steady"]["baseline_power_w"],
        baseline["steady"]["power_w"]
    );
    assert_eq!(result["steady"]["bridge_power_w"], 6.);
    assert!(
        (result["steady"]["power_w"].as_f64().unwrap()
            - baseline["steady"]["power_w"].as_f64().unwrap()
            - 6.)
            .abs()
            < 1e-12
    );
    assert_eq!(result, evaluate(&result["request"]).unwrap());
    let mut stale = result["request"].clone();
    stale["thermal_bridges"]["paths"][0]["qualifications"][0]["material_sha256"][0] =
        json!("0".repeat(64));
    assert!(evaluate(&stale).is_err());
    stale = input.clone();
    stale["thermal_bridges"]["paths"][0]["qualifications"][0]["allow_estimates"] = json!(false);
    assert!(evaluate(&stale).is_err());
    input.as_object_mut().unwrap().remove("thermal_bridges");
    assert_eq!(baseline, evaluate(&input).unwrap());
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

#[test]
fn exterior_coverage_preserves_spatial_binding_and_distinguishes_bare_body_exchange() {
    let mut input = request();
    let example: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
    ))
    .unwrap();
    let mut e = example["exterior_exchange"].clone();
    e["air_c"] = json!(0.);
    e["convection_w_m2_k"] = json!(5.);
    e.as_object_mut().unwrap().remove("radiation");
    input["conditions"]["exterior_exchange"] = json!([e]);
    input["conditions"]["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.);
    let full = evaluate(&input).unwrap();
    let expected = 15. / 0.7 + 15. / 1.1;
    assert!((full["steady"]["power_w"].as_f64().unwrap() - expected).abs() < 1e-7);
    assert_eq!(full, evaluate(&full["request"]).unwrap());
    // Extend the body boundary; local geometric projection now leaves one square metre bare.
    input["map"]["body_positions_m"][1][0] = json!(2.);
    input["map"]["body_positions_m"][3][0] = json!(2.);
    assert!(evaluate(&input).is_err());
    input["conditions"]["bare_boundaries"] = json!([{
        "inside_c":30.,"outside_c":0.,"inside_resistance_m2_k_w":0.2,"outside_resistance_m2_k_w":0.4
    }]);
    let partial = evaluate(&input).unwrap();
    let native: Request = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(partial, native.evaluate().unwrap());
    assert!((partial["steady"]["covered_area_m2"].as_f64().unwrap() - 1.).abs() < 1e-10);
    assert!((partial["steady"]["bare_power_w"].as_f64().unwrap() - 50.).abs() < 1e-7);
    assert!((partial["steady"]["power_w"].as_f64().unwrap() - expected - 50.).abs() < 1e-7);
    assert!(
        (partial["exterior_exchange"]["convective_power_w"]
            .as_f64()
            .unwrap()
            - expected)
            .abs()
            < 1e-7
    );
    assert_eq!(partial, evaluate(&partial["request"]).unwrap());
    for field in ["exterior_exchange", "bare_boundaries"] {
        let mut bad = input.clone();
        bad["conditions"][field] =
            json!([input["conditions"][field][0], input["conditions"][field][0]]);
        assert!(evaluate(&bad).is_err());
        bad["conditions"]["body_mesh_sha256"] = partial["body_mesh_sha256"].clone();
        assert!(evaluate(&bad).is_ok());
        bad["map"]["body_positions_m"][0][2] = json!(-0.04);
        assert!(evaluate(&bad).is_err());
    }
    input["conditions"]
        .as_object_mut()
        .unwrap()
        .remove("exterior_exchange");
    assert!(evaluate(&input).is_err());
}

#[test]
fn closed_seam_loop_includes_the_closing_edge_and_replays_without_backtracking() {
    let mut input = request();
    // One covered body face avoids treating the adjacent, differently characterized back sheet
    // as a second physical qualification for the front triangle's shared diagonal edge.
    input["map"]["body_triangles"] = json!([[0, 1, 2]]);
    let baseline = evaluate(&input).unwrap();
    input["thermal_bridges"] = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-steady-thermal-bridges.json"
    ))
    .unwrap();
    input["thermal_bridges"]["paths"][0]["vertices"] = json!([0, 1, 2]);
    let open = evaluate(&input).unwrap();
    input["thermal_bridges"]["paths"][0]["vertices"] = json!([0, 1, 2, 0]);
    let closed = evaluate(&input).unwrap();
    let perimeter = 2. + 2_f64.sqrt();
    let expected_excess = 0.2 * perimeter * 30.;
    assert!(
        (closed["thermal_bridges"]["paths"][0]["length_m"]
            .as_f64()
            .unwrap()
            - perimeter)
            .abs()
            < 1e-12
    );
    assert!((closed["steady"]["bridge_power_w"].as_f64().unwrap() - expected_excess).abs() < 1e-10);
    assert_eq!(
        closed["steady"]["baseline_power_w"],
        baseline["steady"]["power_w"]
    );
    assert!(
        (closed["steady"]["power_w"].as_f64().unwrap()
            - baseline["steady"]["power_w"].as_f64().unwrap()
            - expected_excess)
            .abs()
            < 1e-10
    );
    assert!(
        (closed["steady"]["bridge_power_w"].as_f64().unwrap()
            - open["steady"]["bridge_power_w"].as_f64().unwrap()
            - 6.)
            .abs()
            < 1e-10
    );
    let mut edges: Vec<[usize; 2]> = closed["thermal_bridges"]["paths"][0]["contributions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| serde_json::from_value(row["edge"].clone()).unwrap())
        .collect();
    edges.sort_unstable();
    edges.dedup();
    assert_eq!(edges, vec![[0, 1], [0, 2], [1, 2]]);
    assert_eq!(
        closed,
        serde_json::from_value::<Request>(input.clone())
            .unwrap()
            .evaluate()
            .unwrap()
    );
    assert_eq!(closed, evaluate(&closed["request"]).unwrap());
    let mut reversed = input.clone();
    reversed["thermal_bridges"]["paths"][0]["vertices"] = json!([0, 2, 1, 0]);
    assert!(
        (evaluate(&reversed).unwrap()["steady"]["bridge_power_w"]
            .as_f64()
            .unwrap()
            - expected_excess)
            .abs()
            < 1e-10
    );
    for invalid in [
        json!([0, 0]),
        json!([0, 1, 0]),
        json!([0, 1, 2, 1]),
        json!([0, 1, 2, 0, 1]),
        json!([0, 1, 2, 0, 1, 2, 0]),
    ] {
        let mut bad = input.clone();
        bad["thermal_bridges"]["paths"][0]["vertices"] = invalid.clone();
        assert!(evaluate(&bad).is_err(), "accepted repeated path {invalid}");
    }
    let mut too_long = input;
    too_long["thermal_bridges"]["paths"][0]["qualifications"][0]["length_range_m"] =
        json!([0.1, 3.0]);
    assert!(evaluate(&too_long).is_err());
}
