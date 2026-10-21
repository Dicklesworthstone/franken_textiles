//! Portable adapters publish and replay the same jointly identified area law as the numerical core.
//! The example contains independent analytic software observations, not empirical textile evidence.
use ftx_core::receipt::sha256_hex;
use ftx_drape::strip::{self, Controls, Fixture, Request};
use ftx_pattern::{material_calibration::nonlinear, material_catalog::Selection, textile_import};
use ftx_wasm::materials;
use serde_json::{Value, json};

const SWATCH: &str = include_str!("../../../examples/area-identification/swatch.json");

#[test]
fn joint_swatch_api_preserves_the_qualified_selection_and_exact_forward_replay() {
    let input: nonlinear::Input = serde_json::from_str(SWATCH).unwrap();
    let core = nonlinear::calibrate(&input).unwrap();
    let result: Value =
        serde_json::from_str(&materials::calibrate_nonlinear(SWATCH, true).unwrap()).unwrap();
    assert_eq!(result["calibration"], serde_json::to_value(&core).unwrap());
    assert_eq!(
        result["calibration"]["input_sha256"],
        sha256_hex(&serde_json::to_vec(&input).unwrap())
    );
    assert_eq!(result["calibration"]["parameter_count"], 9);
    assert_eq!(result["fit_claim"], false);
    assert_eq!(
        result["material"]["capabilities"]["mechanical_coefficients"]["ready"],
        false
    );
    let frozen = result["selection_json"].as_str().unwrap();
    let selection: Selection = serde_json::from_str(frozen).unwrap();
    assert_eq!(
        serde_json::to_value(&selection).unwrap(),
        result["selection"]
    );
    assert_eq!(
        serde_json::from_str::<Value>(&materials::inspect(frozen).unwrap()).unwrap(),
        result["material"]
    );
    let resolved = textile_import::catalog()
        .unwrap()
        .resolve(&selection)
        .unwrap();
    assert_eq!(
        selection.expected_sha256.as_deref(),
        Some(resolved.sha256())
    );
    assert!(resolved.authored().properties.is_empty());
    assert_eq!(
        resolved
            .membrane_response()
            .unwrap()
            .unwrap()
            .area_ratio_range(),
        core.fitted_area_ratio_range
    );
    let area = core
        .material
        .membrane_response
        .as_ref()
        .unwrap()
        .area_response
        .as_ref()
        .unwrap();
    assert!((area.quadratic.value - 40.0).abs() < 1e-7);
    assert!((area.log_squared.value - 15.0).abs() < 1e-7);
    assert_eq!(area.conditions["specimen"], input.conditions["specimen_id"]);
    for coefficient in [&area.quadratic, &area.log_squared] {
        assert_eq!(
            serde_json::to_value(coefficient.evidence).unwrap(),
            "estimate"
        );
        assert!(core.material.sources.contains_key(&coefficient.source));
    }
    let request = Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selection,
        fixture: Fixture {
            gauge_length_m: 0.1,
            width_m: 0.06,
            warp_axis: [0.0, 1.0],
            cells: [2, 4],
        },
        extensions: vec![0.0, 0.02],
        controls: Controls::default(),
    };
    let expected = strip::evaluate(&request).unwrap();
    let forward = materials::strip_experiment(&serde_json::to_string(&request).unwrap()).unwrap();
    assert_eq!(forward, serde_json::to_string(&expected).unwrap());
    assert!(expected.states[1].force_n > 0.0);
    assert_eq!(
        materials::strip_experiment(&serde_json::to_string(&expected.input).unwrap()).unwrap(),
        forward
    );

    let mut changed: Value = serde_json::from_str(frozen).unwrap();
    changed["record"]["membrane_response"]["area_response"]["quadratic"]["value"] = json!(41.0);
    assert!(materials::inspect(&changed.to_string()).is_err());
    let denied: Value =
        serde_json::from_str(&materials::calibrate_nonlinear(SWATCH, false).unwrap()).unwrap();
    assert_eq!(denied["calibration"], result["calibration"]);
    let mut denied_request = request;
    denied_request.material = serde_json::from_value(denied["selection"].clone()).unwrap();
    assert!(materials::strip_experiment(&serde_json::to_string(&denied_request).unwrap()).is_err());
}

#[test]
fn area_identification_api_reports_the_shared_admission_refusals() {
    let original: Value = serde_json::from_str(SWATCH).unwrap();
    let mut mismatch = original.clone();
    mismatch["conditions"]["specimen"] = json!("another specimen");
    let mut denied = original.clone();
    denied["allow_estimated_observations"] = json!(false);
    let mut wrong_model = original;
    wrong_model["area_response"]["model"] = json!("unspecified-area-model");
    for input in [mismatch, denied, wrong_model] {
        let typed: nonlinear::Input = serde_json::from_value(input.clone()).unwrap();
        let expected = nonlinear::calibrate(&typed).unwrap_err();
        let actual = materials::calibrate_nonlinear(&input.to_string(), true).unwrap_err();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
}
