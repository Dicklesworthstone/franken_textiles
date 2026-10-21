//! Native execution of the actual file-free WASM consumer and production strip authority.
//! Synthetic coefficients establish software/known-answer behavior, not real fabric qualification.
use ftx_core::receipt::sha256_hex;
use ftx_drape::strip::{Request, evaluate};
use ftx_wasm::materials;
use serde_json::{Value, json};

fn request() -> Value {
    let mut properties = serde_json::Map::new();
    for (key, value, unit) in [
        ("warp_stiffness", 0.8, "kN/m"),
        ("weft_stiffness", 0.4, "kN/m"),
        ("poisson_warp_weft", 0.0, "1"),
        ("shear_stiffness", 0.12, "kN/m"),
    ] {
        properties.insert(
            key.into(),
            json!({"value":value,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic independent force check",
            "conditions":{"conditioning":"synthetic dry reference"}}),
        );
    }
    json!({"schema":"ftx-strip-experiment/1",
        "material":{"id":"synthetic-browser-strip","allow_estimates":true,"record":{
            "id":"synthetic-browser-strip","name":"Synthetic membrane only","form":"knit",
            "properties":properties,"sources":{"test":{"reference":"test:strip-consumers",
                "accessed_on":"2026-10-21","note":"Synthetic numerical fixture, not measured fabric"}}}},
        "fixture":{"width_m":0.06,"gauge_length_m":0.1,"warp_axis":[0.0,1.0],"cells":[2,4]},
        "extensions":[0.0,0.1],"controls":{"tolerance_m":1e-10,"force_tolerance_n":1e-7,"max_newton":100}})
}

#[test]
fn browser_boundary_uses_actual_membrane_only_physics_and_replays_frozen_sources_exactly() {
    let input = request();
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let direct = evaluate(&typed).unwrap();
    let text = materials::strip_experiment(&input.to_string()).unwrap();
    let actual: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(actual, serde_json::to_value(&direct).unwrap());
    assert_eq!(
        actual["input"]["material"]["record"],
        serde_json::to_value(&typed.material.record).unwrap()
    );
    assert_eq!(
        direct
            .input
            .material
            .record
            .as_ref()
            .unwrap()
            .properties
            .len(),
        4
    );
    assert_eq!(
        direct.input_sha256,
        sha256_hex(&serde_json::to_vec(&direct.input).unwrap())
    );
    assert!(direct.input.material.expected_sha256.is_some());
    assert!((direct.states[1].force_n - 0.06 * 800.0 * 1.1 * 0.105).abs() < 1e-10);
    assert_eq!(
        text,
        materials::strip_experiment(&actual["input"].to_string()).unwrap()
    );

    let mut stale = actual["input"].clone();
    stale["material"]["record"]["properties"]["warp_stiffness"]["conditions"]["conditioning"] =
        json!("Different synthetic conditioning; unchanged numerical modulus");
    let refusal = materials::strip_experiment(&stale.to_string()).unwrap_err();
    assert!(refusal.message.contains("expected_sha256"));
}

#[test]
fn browser_boundary_forwards_domain_estimate_and_solver_budget_refusals_without_partial_states() {
    let original = request();
    let mut denied = original.clone();
    denied["material"]["allow_estimates"] = json!(false);
    let mut outside = original.clone();
    outside["material"]["record"]["sources"]["test"]["reference"] =
        json!("local:ftx-strip-calibration/synthetic-consumer-domain");
    for quantity in outside["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        quantity["conditions"]["fitted_green_strain_range"] =
            json!("[[-0.01,0.03],[-0.01,0.03],[-0.01,0.01]]");
    }
    let mut exhausted = original.clone();
    exhausted["material"]["record"]["properties"]["poisson_warp_weft"]["value"] = json!(0.25);
    exhausted["controls"]["max_newton"] = json!(1);
    let mut excessive = original;
    excessive["extensions"] = json!(vec![0.0; 65]);
    for input in [denied, outside, exhausted, excessive] {
        let typed: Request = serde_json::from_value(input.clone()).unwrap();
        let expected = evaluate(&typed).unwrap_err();
        let actual = materials::strip_experiment(&input.to_string()).unwrap_err();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
}
