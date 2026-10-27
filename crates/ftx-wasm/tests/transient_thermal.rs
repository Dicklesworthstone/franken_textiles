//! Native execution of the file-free exports; generated-WASM parity is checked separately.
use ftx_core::RefusalCode;
use ftx_drape::thermal::request::{self, Request};
use ftx_pattern::{material_catalog::Selection, materials::Catalog, textile_import::catalog};
use ftx_wasm::thermal;
use serde_json::{Value, json};

const COTTON: &str = "nist/7282/cotton-duck-linear-cp";

fn fixture() -> Value {
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/nist-7282-transient.json"
    ))
    .unwrap();
    input["layers"][0]["id"] = json!(COTTON);
    input
}

fn typed(input: &Value) -> Request {
    serde_json::from_value(input.clone()).unwrap()
}

fn portable(input: &Value) -> Value {
    serde_json::from_str(&thermal::transient(&input.to_string()).unwrap()).unwrap()
}

fn same_refusal(input: &Value) {
    let expected = typed(input).evaluate(&catalog().unwrap()).unwrap_err();
    let actual = thermal::transient(&input.to_string()).unwrap_err();
    assert_eq!(actual.code, expected.code);
    assert_eq!(actual.message, expected.message);
}

#[test]
fn point_matches_shared_authority_and_preserves_frozen_source_policy() {
    let db = catalog().unwrap();
    let selected = Selection {
        id: COTTON.into(),
        allow_estimates: true,
        ..Selection::default()
    };
    let output = thermal::point(&serde_json::to_string(&selected).unwrap(), 50.0).unwrap();
    let actual: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(actual, request::point(&selected, 50.0, &db).unwrap());
    assert_eq!(actual["temperature_domain_c"], json!([20.0, 70.0]));
    assert!(
        (actual["areal_heat_capacity_j_m2_k"].as_f64().unwrap() - 0.684948 * 1617.9).abs() < 1e-9
    );
    let frozen: Selection = serde_json::from_value(actual["selection"].clone()).unwrap();
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    assert_eq!(request::point(&frozen, 50.0, &empty).unwrap(), actual);
    assert_eq!(
        thermal::point(&actual["selection"].to_string(), 50.0).unwrap(),
        output
    );
    for t in [19.0, 71.0, f64::NAN, f64::INFINITY] {
        let expected = request::point(&frozen, t, &empty).unwrap_err();
        let refused = thermal::point(&actual["selection"].to_string(), t).unwrap_err();
        assert_eq!(refused.code, expected.code);
        assert_eq!(refused.message, expected.message);
    }
    let mut changed = actual["selection"].clone();
    changed["record"]["sources"]["nist7282"]["reference"] = json!("test:unrelated-source");
    assert!(thermal::point(&changed.to_string(), 50.0).is_err());
    changed = actual["selection"].clone();
    changed["allow_estimates"] = json!(false);
    assert!(thermal::point(&changed.to_string(), 50.0).is_err());
    assert!(
        thermal::point(
            r#"{"id":"nist/7282/cotton-duck","allow_estimates":true}"#,
            50.0
        )
        .is_err()
    );
}

#[test]
fn transients_match_complete_shared_reports_and_replay_without_catalog() {
    let db = catalog().unwrap();
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    for cooling in [false, true] {
        let mut input = fixture();
        if cooling {
            input["initial_temperature_c"] = json!(50.0);
            input["inside_c"] = json!(20.0);
            input["outside_c"] = json!(35.0);
        }
        let expected = typed(&input).evaluate(&db).unwrap();
        assert_eq!(portable(&input), expected);
        assert_eq!(expected["model"]["schema"], "ftx-dry-thermal-model/2");
        let sign = if cooling { -1.0 } else { 1.0 };
        let stored = expected["stored_energy_change_j"].as_f64().unwrap();
        assert!(sign * stored > 0.0);
        assert!(sign * expected["steady_heat_w"].as_f64().unwrap() > 0.0);
        let incoming = expected["incoming_energy_j"].as_f64().unwrap();
        let outgoing = expected["outgoing_energy_j"].as_f64().unwrap();
        let steps = expected["steps"].as_f64().unwrap();
        let max_step_error = expected["largest_step_energy_balance_error_j"]
            .as_f64()
            .unwrap();
        assert!(max_step_error < 1e-7);
        // The signed global residual accumulates accepted per-step residuals. Keep the
        // original per-step accuracy check, then allow their sum plus floating-point summation.
        let rounding =
            32.0 * f64::EPSILON * steps * (1.0 + stored.abs() + incoming.abs() + outgoing.abs());
        let budget = steps * max_step_error + rounding;
        let residual = (stored - incoming + outgoing).abs();
        assert!(
            residual <= budget,
            "cooling={cooling}: accumulated residual {residual} exceeds {budget}; \
             {steps} steps, maximum step residual {max_step_error}, rounding {rounding}"
        );
        // Independent source primitives, as in the native NIST workflow regression.
        let primitive = |t: f64, cotton: bool| {
            if cotton {
                1422.9 * t + 1.95 * t * t
            } else {
                1832.2 * t - 24.5 * t * t + 0.4 * t * t * t - 0.00175 * t * t * t * t
            }
        };
        let initial = input["initial_temperature_c"].as_f64().unwrap();
        let final_t = expected["trace"].as_array().unwrap().last().unwrap()["temperature_c"]
            .as_array()
            .unwrap();
        assert_eq!(final_t.len(), 8);
        let analytic: f64 = final_t
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let cotton = i < 4;
                let mass = if cotton { 0.684948 } else { 0.259858 };
                mass / 4.0 * (primitive(t.as_f64().unwrap(), cotton) - primitive(initial, cotton))
            })
            .sum();
        assert!(
            (stored - analytic).abs() < 1e-7 * (1.0 + analytic.abs()),
            "cooling={cooling}: stored energy {stored} differs from source integral {analytic}"
        );
        assert_eq!(expected["comfort_claim"], false);
        assert_eq!(expected["empirical_validation_claim"], false);
        let replay_text = expected["replay_request_json"].as_str().unwrap();
        let replay: Request = serde_json::from_str(replay_text).unwrap();
        let mut original_settings = serde_json::to_value(typed(&input)).unwrap();
        let mut frozen_settings = serde_json::to_value(&replay).unwrap();
        original_settings.as_object_mut().unwrap().remove("layers");
        frozen_settings.as_object_mut().unwrap().remove("layers");
        assert_eq!(frozen_settings, original_settings);
        for (original, frozen) in typed(&input).layers.iter().zip(&replay.layers) {
            assert_eq!(frozen.allow_estimates, original.allow_estimates);
            assert!(frozen.record.is_some());
            assert!(frozen.expected_sha256.is_some());
        }
        assert_eq!(replay.evaluate(&empty).unwrap(), expected);
        assert_eq!(
            thermal::transient(replay_text).unwrap(),
            thermal::transient(&input.to_string()).unwrap()
        );
        let mut changed: Value = serde_json::from_str(replay_text).unwrap();
        changed["layers"][0]["record"]["sources"]["nist7282"]["reference"] = json!("test:changed");
        same_refusal(&changed);
    }
}

fn exterior_fixture(emissivity: f64) -> Value {
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
    ))
    .unwrap();
    input["schema"] = json!("ftx-thermal-transient/1");
    input["cells_per_layer"] = json!(2);
    input["initial_temperature_c"] = json!(10.0);
    input["step_seconds"] = json!(0.5);
    input["steps"] = json!(4);
    input["record_every"] = json!(1);
    input["layers"][0]["record"]["properties"]["emissivity"]["value"] = json!(emissivity);
    for (key, value, unit) in [
        ("areal_density", 0.2, "kg/m2"),
        ("specific_heat", 1000.0, "J/(kg*K)"),
    ] {
        input["layers"][0]["record"]["properties"][key] = json!({"value":value,
            "unit":unit,"evidence":"estimate","source":"synthetic","method":"Synthetic storage",
            "conditions":{"specimen":"synthetic-coupon","conditioning":"synthetic-dry"}});
    }
    input
}

#[test]
fn exterior_transients_preserve_material_sensitive_radiation_and_refusals() {
    let input = exterior_fixture(0.8);
    let first = portable(&input);
    assert_eq!(first, typed(&input).evaluate(&catalog().unwrap()).unwrap());
    assert_eq!(
        first["exterior_exchange"]["conditions"],
        input["exterior_exchange"]
    );
    let replay_text = first["replay_request_json"].as_str().unwrap();
    let replay: Request = serde_json::from_str(replay_text).unwrap();
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    assert_eq!(replay.evaluate(&empty).unwrap(), first);
    assert_eq!(
        thermal::transient(replay_text).unwrap(),
        thermal::transient(&input.to_string()).unwrap()
    );
    let exterior = &first["exterior_exchange"];
    assert!(
        (first["outgoing_energy_j"].as_f64().unwrap()
            - exterior["convective_energy_j"].as_f64().unwrap()
            - exterior["radiative_energy_j"].as_f64().unwrap())
        .abs()
            < 1e-7
    );
    let reflective = exterior_fixture(0.05);
    let second = portable(&reflective);
    assert_eq!(
        second,
        typed(&reflective).evaluate(&catalog().unwrap()).unwrap()
    );
    assert_ne!(first["trace"], second["trace"]);
    for field in ["outside_c", "outside_resistance_m2_k_w"] {
        let mut bad = input.clone();
        bad[field] = json!(bad[field].as_f64().unwrap() + 1.0);
        same_refusal(&bad);
    }
    let mut bad = input;
    bad["exterior_exchange"]["allow_estimates"] = json!(false);
    same_refusal(&bad);
}

#[test]
fn frozen_replay_preserves_mixed_layer_policies_exterior_controls_and_negative_zero() {
    let mut input = exterior_fixture(0.8);
    let mut strict = input["layers"][0].clone();
    strict["id"] = json!("synthetic-strict-policy-sheet");
    strict["record"]["id"] = strict["id"].clone();
    strict["allow_estimates"] = json!(false);
    strict["record"]["sources"]["synthetic"]["note"] =
        json!("Synthetic evidence-classification fixture; no measured textile data");
    for quantity in strict["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        quantity["evidence"] = json!("published");
    }
    let estimated = input["layers"][0].clone();
    input["layers"] = json!([strict, estimated]);
    input["interface_resistance_m2_k_w"] = json!([0.03]);
    input["initial_temperature_c"] = json!(-0.0);
    input["step_seconds"] = json!(0.125);
    input["record_every"] = json!(3);
    let output_text = thermal::transient(&input.to_string()).unwrap();
    let output: Value = serde_json::from_str(&output_text).unwrap();
    let replay_text = output["replay_request_json"].as_str().unwrap();
    let replay: Request = serde_json::from_str(replay_text).unwrap();
    assert_eq!(replay.initial_temperature_c.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(replay.step_seconds, 0.125);
    assert_eq!(replay.steps, 4);
    assert_eq!(replay.record_every, 3);
    assert_eq!(replay.interface_resistance_m2_k_w, vec![0.03]);
    assert!(!replay.layers[0].allow_estimates);
    assert!(replay.layers[1].allow_estimates);
    let original: Request = serde_json::from_value(input).unwrap();
    assert_eq!(
        serde_json::to_value(&replay.exterior_exchange).unwrap(),
        serde_json::to_value(&original.exterior_exchange).unwrap()
    );
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    assert_eq!(replay.evaluate(&empty).unwrap(), output);
    assert_eq!(thermal::transient(replay_text).unwrap(), output_text);
    assert_eq!(output["trace"].as_array().unwrap().len(), 3);
    assert_eq!(output["trace"][1]["time_seconds"], json!(0.375));
    assert_eq!(output["trace"][2]["time_seconds"], json!(0.5));
}

#[test]
fn portable_input_domain_evidence_and_work_limits_refuse_without_partial_output() {
    for (field, value) in [
        ("schema", json!("unknown")),
        ("steps", json!(0)),
        ("steps", json!(100_001)),
        ("record_every", json!(0)),
        ("step_seconds", json!(0.0)),
        ("area_m2", json!(0.0)),
        ("cells_per_layer", json!(5000)),
        ("outside_c", json!(19.0)),
        ("initial_temperature_c", json!(71.0)),
    ] {
        let mut bad = fixture();
        bad[field] = value;
        same_refusal(&bad);
    }
    for cells in [512, 2000] {
        let mut bad = fixture();
        bad["cells_per_layer"] = json!(cells);
        bad["steps"] = json!(5000);
        bad["record_every"] = json!(1);
        same_refusal(&bad);
    }
    let mut bad = fixture();
    bad["layers"][0]["allow_estimates"] = json!(false);
    same_refusal(&bad);
    bad = fixture();
    bad["extra"] = json!(true);
    assert!(thermal::transient(&bad.to_string()).is_err());
    assert!(thermal::transient("not JSON").is_err());
    assert!(thermal::point("not JSON", 50.0).is_err());
    let oversized = " ".repeat(64 * 1024 * 1024 + 1);
    assert_eq!(
        thermal::transient(&oversized).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(
        thermal::point(&oversized, 50.0).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn impossible_stack_shape_refuses_before_expanding_any_material_selection() {
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    for (layers, cells_per_layer, interfaces) in [(1, 0, 0), (4097, 1, 4096), (2, 1, 0)] {
        let mut input = fixture();
        input["layers"] = json!(vec![json!({"id":"must-not-be-resolved"}); layers]);
        input["cells_per_layer"] = json!(cells_per_layer);
        input["interface_resistance_m2_k_w"] = json!(vec![0.0; interfaces]);
        input["steps"] = json!(1);
        input["record_every"] = json!(1);
        let refused = typed(&input).evaluate(&empty).unwrap_err();
        assert_eq!(
            refused.message,
            "thermal stack needs layers, 1..4096 cells and exactly one resistance per interface"
        );
        same_refusal(&input);
    }
}
