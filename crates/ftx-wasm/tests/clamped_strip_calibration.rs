//! Browser adapter integration against the real finite-strip inverse; no browser or solver doubles.
//! Synthetic acquisitions establish numerical plumbing, never measured textile or garment fit.

use ftx_core::{RefusalCode, receipt::sha256_hex};
use ftx_drape::strip::{
    Request,
    fit::{Input, Report},
};
use ftx_pattern::{material_calibration::Partition, material_catalog::Selection, textile_import};
use ftx_wasm::materials;
use serde_json::{Value, json};

const INPUT: &str = include_str!("../../../tests/fixtures/materials/synthetic-clamped-strip.json");
const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");

#[test]
fn portable_joint_area_identification_keeps_six_coordinates_and_the_frozen_area_law() {
    let raw = include_str!("../../../examples/area-response/identify.json");
    let result: Value =
        serde_json::from_str(&materials::calibrate_strip(raw, true).unwrap()).unwrap();
    let report = &result["calibration"];
    assert_eq!(report["accepted"], true, "{report}");
    assert_eq!(report["domain_replay_verified"], true);
    let parameters = report["parameters"].as_array().unwrap();
    assert_eq!(parameters.len(), 6);
    for (actual, known) in parameters.iter().zip([1.1, 0.9, 1.2, -80.0, 31.0, 19.0]) {
        assert!((actual.as_f64().unwrap() - known).abs() < 1e-3);
    }
    let selected: Selection =
        serde_json::from_str(result["selection_json"].as_str().unwrap()).unwrap();
    let fitted: [f64; 2] =
        serde_json::from_value(report["fitted_area_ratio_range"].clone()).unwrap();
    assert!(fitted[0] > 0.65 && fitted[1] < 1.45);
    assert_eq!(
        serde_json::to_value(&selected).unwrap(),
        report["selection"]
    );
    let resolved = textile_import::catalog()
        .unwrap()
        .resolve(&selected)
        .unwrap();
    assert_eq!(
        resolved
            .membrane_response()
            .unwrap()
            .unwrap()
            .area_ratio_range(),
        Some(fitted)
    );
    // The portable consumer must replay the published CHECK state through the normal solver,
    // including the FIT-only area domain, without refitting or changing the selection.
    let typed: Report = serde_json::from_value(report.clone()).unwrap();
    let experiment = typed
        .input
        .experiments
        .iter()
        .find(|experiment| experiment.partition == Partition::Check)
        .unwrap();
    let mut fixture = experiment.fixture.clone();
    fixture.cells.iter_mut().for_each(|cells| *cells *= 2);
    let request = Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selected.clone(),
        fixture,
        extensions: experiment
            .observations
            .iter()
            .map(|state| state.extension_fraction)
            .collect(),
        controls: typed.input.controls.clone(),
    };
    let raw_forward =
        materials::strip_experiment(&serde_json::to_string(&request).unwrap()).unwrap();
    let forward: Value = serde_json::from_str(&raw_forward).unwrap();
    let expected: Vec<_> = typed
        .predictions
        .iter()
        .filter(|prediction| prediction.experiment == experiment.id)
        .map(|prediction| &prediction.state)
        .collect();
    assert_eq!(forward["states"], serde_json::to_value(expected).unwrap());
    assert_eq!(
        materials::strip_experiment(&forward["input"].to_string()).unwrap(),
        raw_forward
    );
    // Removing the content lock cannot turn the identified law into a broader area law.
    let mut widened = selected.clone();
    widened.expected_sha256 = None;
    widened
        .record
        .as_mut()
        .unwrap()
        .membrane_response
        .as_mut()
        .unwrap()
        .area_response
        .as_mut()
        .unwrap()
        .area_ratio_range = [0.65, 1.45];
    assert!(materials::inspect(&serde_json::to_string(&widened).unwrap()).is_err());
    assert!(resolved.mechanics().is_err());
    assert_eq!(result["fit_claim"], false);
    assert_eq!(result["empirical_validation_claim"], false);
}

#[test]
fn accepted_strip_api_freezes_the_source_estimate_without_fabricating_missing_physics() {
    // Exactly one accepted inverse run; all following checks reuse its frozen selection.
    // The API argument, rather than the serialized output-consumption policy, controls consent.
    let mut request: Value = serde_json::from_str(INPUT).unwrap();
    request["allow_estimates"] = json!(false);
    let identified: Value =
        serde_json::from_str(&materials::calibrate_strip(&request.to_string(), true).unwrap())
            .unwrap();
    let calibration = &identified["calibration"];
    assert_eq!(calibration["accepted"], true, "{identified}");
    assert_eq!(calibration["reasons"], json!([]));
    assert_eq!(calibration["domain_replay_verified"], true);
    assert!(calibration["check"].is_object());
    assert_eq!(identified["fit_claim"], false);
    assert_eq!(identified["empirical_validation_claim"], false);
    assert_eq!(identified["simulation_ready"], false);
    let mut input: Input = serde_json::from_value(request).unwrap();
    input.allow_estimates = true;
    let hash = sha256_hex(&serde_json::to_vec(&input).unwrap());
    assert_eq!(calibration["input_sha256"], hash);
    assert_eq!(calibration["input"], serde_json::to_value(&input).unwrap());
    let selection_json = identified["selection_json"].as_str().unwrap();
    let selection: Selection = serde_json::from_str(selection_json).unwrap();
    assert_eq!(
        serde_json::to_value(&selection).unwrap(),
        identified["selection"]
    );
    assert_eq!(identified["selection"], calibration["selection"]);
    assert_eq!(identified["selection_json"], calibration["selection_json"]);
    assert!(selection.allow_estimates);
    let source = format!("strip-{hash}");
    let record = selection.record.as_ref().unwrap();
    assert_eq!(
        record.sources[&source].reference,
        format!("local:ftx-strip-calibration/{hash}")
    );
    assert_eq!(
        serde_json::to_value(record).unwrap(),
        calibration["material"]
    );
    for (key, original) in &input.material.sources {
        assert_eq!(
            serde_json::to_value(&record.sources[key]).unwrap(),
            serde_json::to_value(original).unwrap()
        );
    }
    assert_eq!(
        serde_json::to_value(&record.properties["areal_density"]).unwrap(),
        serde_json::to_value(&input.material.properties["areal_density"]).unwrap()
    );
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
    ] {
        let coefficient = &record.properties[key];
        assert_eq!(
            serde_json::to_value(coefficient.evidence).unwrap(),
            "estimate"
        );
        assert_eq!(coefficient.source, source);
        let domain: Value =
            serde_json::from_str(&coefficient.conditions["fitted_green_strain_range"]).unwrap();
        assert_eq!(domain, calibration["fitted_green_strain_range"]);
    }
    let catalog = textile_import::catalog().unwrap();
    let resolved = catalog.resolve(&selection).unwrap();
    assert_eq!(
        identified["mechanics_refusal"],
        serde_json::to_value(resolved.mechanics().unwrap_err()).unwrap()
    );
    assert_eq!(
        selection.expected_sha256.as_deref(),
        Some(resolved.sha256())
    );
    assert!(
        resolved
            .material()
            .require("warp_stiffness", "N/m", resolved.allows_estimates())
            .is_ok()
    );
    assert!(resolved.mechanics().is_err());
    let detail: Value = serde_json::from_str(&materials::inspect(selection_json).unwrap()).unwrap();
    assert_eq!(detail, identified["material"]);
    assert_eq!(
        detail["capabilities"]["mechanical_coefficients"]["ready"],
        false
    );
    for missing in [
        "bending_rigidity",
        "thickness",
        "friction_skin",
        "friction_self",
    ] {
        assert!(
            detail["missing_mechanics"]
                .as_array()
                .unwrap()
                .contains(&json!(missing))
        );
    }
    let changes = json!({"schema":"ftx-material-bindings/1","default":identified["selection"]});
    let bound: Value =
        serde_json::from_str(&materials::bind(PATTERN, &changes.to_string()).unwrap()).unwrap();
    assert_eq!(bound["geometry_changed"], false);
    assert_eq!(
        bound["drape_materials_without_self_contact"]["ready"],
        false
    );
    let original: Value = serde_json::from_str(PATTERN).unwrap();
    assert_eq!(bound["pattern"]["pieces"], original["pieces"]);
    assert_eq!(bound["pattern"]["seams"], original["seams"]);
    assert_eq!(
        bound["pattern"]["fabric"]["material"],
        identified["selection"]
    );
    let mut changed: Value = serde_json::from_str(selection_json).unwrap();
    changed["record"]["properties"]["warp_stiffness"]["value"] =
        json!(record.properties["warp_stiffness"].value + 1.0);
    assert!(materials::inspect(&changed.to_string()).is_err());
    let mut changed: Value = serde_json::from_str(selection_json).unwrap();
    changed["allow_estimates"] = json!(false);
    assert!(materials::inspect(&changed.to_string()).is_err());
}

#[test]
fn unresolved_strip_api_keeps_its_report_without_an_assignable_selection() {
    let mut request: Value = serde_json::from_str(INPUT).unwrap();
    request["policy"]["max_iterations"] = json!(1);
    let response: Value =
        serde_json::from_str(&materials::calibrate_strip(&request.to_string(), true).unwrap())
            .unwrap();
    assert_eq!(response["calibration"]["accepted"], false, "{response}");
    assert!(
        response["calibration"]["reasons"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
    );
    assert!(
        response["calibration"]["starts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|start| start["converged"] == false)
    );
    for field in ["material", "selection", "selection_json"] {
        assert!(response["calibration"][field].is_null());
    }
    for field in ["selection", "selection_json", "material"] {
        assert!(response[field].is_null(), "{field}: {response}");
    }
    assert_eq!(response["simulation_ready"], false);
    assert_eq!(response["fit_claim"], false);
    assert_eq!(response["empirical_validation_claim"], false);
    let mut input: Input = serde_json::from_value(request).unwrap();
    input.allow_estimates = true;
    assert_eq!(
        response["calibration"]["input_sha256"],
        sha256_hex(&serde_json::to_vec(&input).unwrap())
    );
}

#[test]
fn exhausted_strip_api_refuses_without_publishing_a_candidate() {
    let mut request: Value = serde_json::from_str(INPUT).unwrap();
    request["policy"]["max_forward_solves"] = json!(1);
    let refusal = materials::calibrate_strip(&request.to_string(), true).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::BudgetExceeded);
}

#[test]
fn strip_api_requires_acquisition_consent_and_valid_observation_units() {
    assert!(materials::calibrate_strip("{}", true).is_err());
    let original: Value = serde_json::from_str(INPUT).unwrap();
    // These guards run before an inverse evaluation; result-consumption permission cannot
    // substitute for independent admission of the model or estimated acquisition observations.
    for policy in ["admit_planar_elastic_model", "allow_estimated_observations"] {
        let mut request = original.clone();
        request[policy] = json!(false);
        assert!(
            materials::calibrate_strip(&request.to_string(), true).is_err(),
            "{policy}"
        );
    }
    let mut request = original.clone();
    request["experiments"][0]["observations"][0]["mid_width"]["value_m"] = json!(0);
    assert!(materials::calibrate_strip(&request.to_string(), true).is_err());
    let mut request = original.clone();
    // An unqualified compression fraction cannot silently become a physical midpoint width.
    request["experiments"][0]["observations"][0]["compression_fraction"] = json!(0.1);
    assert!(materials::calibrate_strip(&request.to_string(), true).is_err());
    let mut request = original;
    request["policy"]["max_forward_solves"] = json!(0);
    assert!(materials::calibrate_strip(&request.to_string(), true).is_err());
}
