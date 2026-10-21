//! Native execution of the file-free WASM boundary against the production membrane authority.
//! This verifies integration and replay; the shared example coefficients are explicitly synthetic.
use ftx_core::refusal::RefusalCode;
use ftx_drape::strip::{self, Request};
use ftx_wasm::materials;
use serde_json::{Value, json};

const BASELINE: &str = include_str!("../../../examples/area-response/baseline.json");
const ENABLED: &str = include_str!("../../../examples/area-response/enabled.json");

#[test]
fn area_examples_keep_core_numerics_source_identity_and_exact_frozen_replay() {
    let mut forces = Vec::new();
    for raw in [BASELINE, ENABLED] {
        let input: Request = serde_json::from_str(raw).unwrap();
        let core = strip::evaluate(&input).unwrap();
        let actual = materials::strip_experiment(raw).unwrap();
        assert_eq!(actual, serde_json::to_string(&core).unwrap());
        assert_eq!(
            actual,
            materials::strip_experiment(&serde_json::to_string(&core.input).unwrap()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&core.input.material.record).unwrap(),
            serde_json::to_value(&input.material.record).unwrap()
        );
        assert!(core.input.material.expected_sha256.is_some());
        forces.push(core.states.last().unwrap().force_n);
        if raw == ENABLED {
            let mut stale = serde_json::to_value(&core.input).unwrap();
            stale["material"]["record"]["membrane_response"]["area_response"]["quadratic"]["value"] =
                json!(101.0);
            let refusal = materials::strip_experiment(&stale.to_string()).unwrap_err();
            assert_eq!(refusal.code, RefusalCode::InvalidDocument);
            assert!(refusal.message.contains("expected_sha256"));
        }
    }
    assert!((forces[0] - forces[1]).abs() > 1e-6);
}

#[test]
fn area_boundary_preserves_source_consent_matching_and_physical_domain_refusals() {
    let original: Value = serde_json::from_str(ENABLED).unwrap();
    let mut denied = original.clone();
    denied["material"]["allow_estimates"] = json!(false);
    let mut mismatched = original.clone();
    mismatched["material"]["record"]["membrane_response"]["area_response"]["log_squared"]["conditions"]
        ["conditioning"] = json!("a different conditioning state");
    let mut outside = original;
    outside["material"]["record"]["membrane_response"]["area_response"]["area_ratio_range"] =
        json!([1.0, 1.00001]);
    for (value, code) in [
        (denied, RefusalCode::InvalidDocument),
        (mismatched, RefusalCode::InvalidDocument),
        (outside, RefusalCode::OutOfRange),
    ] {
        let input: Request = serde_json::from_value(value.clone()).unwrap();
        let expected = strip::evaluate(&input).unwrap_err();
        assert_eq!(expected.code, code);
        let actual = materials::strip_experiment(&value.to_string()).unwrap_err();
        assert_eq!(actual, expected);
    }
}
