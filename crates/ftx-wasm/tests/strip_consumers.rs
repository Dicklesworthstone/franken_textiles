//! Native-callable browser adapters share the production engines and retain explicit source policy.
//! Synthetic forward fixtures are software checks, not empirical material identification.
use ftx_core::RefusalCode;
use ftx_drape::strip::{self, Controls, Fixture, Request};
use ftx_pattern::{
    material_catalog::{Selection, knit_strips},
    materials::Material,
};
use ftx_wasm::materials;
use serde_json::{Value, json};

const ACQUISITION: &str =
    include_str!("../../ftx-pattern/src/material_catalog/knit-strip-measurements.json");
const INVERSE: &str =
    include_str!("../../../examples/strip-identification/synthetic-calibration.json");
const NONLINEAR: &str =
    include_str!("../../../examples/strip-identification/synthetic-nonlinear-calibration.json");
const INDEPENDENT_INTERVALS: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-nonlinear-clamped-strip.json");
const SCALAR_AREA: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-scalar-area-clamped-strip.json");

fn forward() -> Request {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    let mut material: Material = serde_json::from_value(fixture["material"].clone()).unwrap();
    material.properties.retain(|key, _| {
        [
            "warp_stiffness",
            "weft_stiffness",
            "poisson_warp_weft",
            "shear_stiffness",
        ]
        .contains(&key.as_str())
    });
    Request {
        schema: "ftx-strip-experiment/1".into(),
        material: Selection {
            id: material.id.clone(),
            record: Some(material),
            allow_estimates: true,
            ..Selection::default()
        },
        fixture: Fixture {
            gauge_length_m: 0.1,
            width_m: 0.06,
            warp_axis: [0.0, 1.0],
            cells: [2, 4],
        },
        extensions: vec![0.0, 0.04],
        controls: Controls::default(),
    }
}

#[test]
fn forward_api_is_exact_core_report_and_retains_frozen_offline_replay_and_admission() {
    let input = forward();
    let raw = serde_json::to_string_pretty(&input).unwrap();
    let core = strip::evaluate(&input).unwrap();
    let report = materials::strip_experiment(&raw).unwrap();
    assert_eq!(report, serde_json::to_string(&core).unwrap());
    assert_eq!(
        materials::strip_experiment(&serde_json::to_string(&core.input).unwrap()).unwrap(),
        report
    );
    let selection = serde_json::to_string(&core.input.material).unwrap();
    let inspected: Value = serde_json::from_str(&materials::inspect(&selection).unwrap()).unwrap();
    assert_eq!(inspected["selection_json"], selection);
    assert_eq!(inspected["selection"]["allow_estimates"], true);
    assert_eq!(
        inspected["capabilities"]["mechanical_coefficients"]["ready"],
        false
    );
    // Missing mass/bending/contact do not block the membrane-only experiment or become invented.
    assert!(
        !core
            .input
            .material
            .record
            .as_ref()
            .unwrap()
            .properties
            .contains_key("areal_density")
    );
    assert!(core.states[1].force_n > 0.0);
    assert!(core.states[1].free_force_residual_n <= input.controls.force_tolerance_n);
    let mut denied = input;
    denied.material.allow_estimates = false;
    assert_eq!(
        materials::strip_experiment(&serde_json::to_string(&denied).unwrap())
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn acquisition_api_preserves_original_source_hashes_curves_and_absent_measurement_channels() {
    let raw = format!("\n{ACQUISITION}\n");
    let core = knit_strips::parse_acquisition(&raw).unwrap();
    let report = materials::strip_acquisition(&raw).unwrap();
    assert_eq!(report, serde_json::to_string(&core).unwrap());
    assert_eq!(
        core.acquisition_sha256,
        ftx_core::receipt::sha256_hex(raw.as_bytes())
    );
    let parsed: Value = serde_json::from_str(&report).unwrap();
    assert_eq!(
        parsed["curves"][0]["observations"]
            .as_array()
            .unwrap()
            .len(),
        916
    );
    assert_eq!(
        parsed["curves"][0]["observations"][915]["branch"],
        "unloading"
    );
    assert!(
        parsed["curves"][0]["observations"][0]
            .get("local_strain")
            .is_none()
    );
    assert!(
        parsed["curves"][0]["observations"][0]
            .get("width_m")
            .is_none()
    );
    assert!(parsed.get("selection").is_none());
    let mut changed: Value = serde_json::from_str(ACQUISITION).unwrap();
    changed["metadata_json"] = json!("{}");
    assert!(materials::strip_acquisition(&changed.to_string()).is_err());
}

#[test]
fn inverse_api_replays_accepted_selections_and_preserves_rejected_candidates_without_publication() {
    let mut input: strip::fit::Input = serde_json::from_str(INVERSE).unwrap();
    let core = strip::fit::calibrate(&input).unwrap();
    assert!(core.accepted, "{:?}", core.reasons);
    let report: Value =
        serde_json::from_str(&materials::calibrate_strip(INVERSE, true).unwrap()).unwrap();
    assert_eq!(report["calibration"], serde_json::to_value(&core).unwrap());
    let selected = core.selection_json.as_deref().unwrap();
    let inspected: Value = serde_json::from_str(&materials::inspect(selected).unwrap()).unwrap();
    assert_eq!(inspected["selection_json"], selected);
    assert_eq!(inspected["selection"]["allow_estimates"], true);
    assert_eq!(
        inspected["capabilities"]["mechanical_coefficients"]["ready"],
        false
    );
    let acquired = &input.experiments[0];
    let forward = Request {
        schema: "ftx-strip-experiment/1".into(),
        material: core.selection.clone().unwrap(),
        fixture: acquired.fixture.clone(),
        controls: input.controls.clone(),
        extensions: acquired
            .observations
            .iter()
            .map(|p| p.extension_fraction)
            .collect(),
    };
    let actual = materials::strip_experiment(&serde_json::to_string(&forward).unwrap()).unwrap();
    assert_eq!(
        actual,
        serde_json::to_string(&strip::evaluate(&forward).unwrap()).unwrap()
    );
    input.allow_estimates = false;
    let denied: Value = serde_json::from_str(
        &materials::calibrate_strip(&serde_json::to_string(&input).unwrap(), false).unwrap(),
    )
    .unwrap();
    let denied: strip::fit::Report = serde_json::from_value(denied["calibration"].clone()).unwrap();
    assert!(denied.accepted);
    assert_eq!(denied.parameters, core.parameters);
    assert!(!denied.selection.as_ref().unwrap().allow_estimates);
    let refused_forward = Request {
        material: denied.selection.unwrap(),
        ..forward
    };
    assert_eq!(
        materials::strip_experiment(&serde_json::to_string(&refused_forward).unwrap())
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
    input
        .experiments
        .iter_mut()
        .find(|e| e.partition == ftx_pattern::material_calibration::Partition::Check)
        .unwrap()
        .observations[0]
        .force_n += 10.0;
    let rejected: Value = serde_json::from_str(
        &materials::calibrate_strip(&serde_json::to_string(&input).unwrap(), false).unwrap(),
    )
    .unwrap();
    for field in ["material", "selection", "selection_json"] {
        assert!(rejected[field].is_null());
    }
    let rejected: strip::fit::Report =
        serde_json::from_value(rejected["calibration"].clone()).unwrap();
    assert!(!rejected.accepted);
    assert_eq!(rejected.parameters, core.parameters);
    assert!(
        rejected.material.is_none()
            && rejected.selection.is_none()
            && rejected.selection_json.is_none()
    );
    assert!(!rejected.reasons.is_empty());
    assert!(rejected.check.unwrap().peak_scaled > 90.0);
    input.allow_estimated_observations = false;
    assert_eq!(
        materials::calibrate_strip(&serde_json::to_string(&input).unwrap(), false)
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn nonlinear_strip_api_preserves_declared_shapes_and_replays_only_admitted_frozen_material() {
    let input: strip::fit::Input = serde_json::from_str(NONLINEAR).unwrap();
    let result: Value =
        serde_json::from_str(&materials::calibrate_strip(NONLINEAR, true).unwrap()).unwrap();
    let report: strip::fit::Report = serde_json::from_value(result["calibration"].clone()).unwrap();
    assert!(report.accepted, "{:?}", report.reasons);
    assert!(report.domain_replay_verified);
    assert_eq!(
        serde_json::to_value(&report.input.response_family).unwrap(),
        serde_json::to_value(&input.response_family).unwrap()
    );
    for (actual, known) in report.parameters.iter().zip([1.1, 0.9, 1.2, 40.0]) {
        assert!((actual - known).abs() < 1e-4);
    }
    assert_eq!(
        result["calibration"]["parameter_descriptors"][3]["id"],
        "normal_coupling"
    );
    assert_eq!(
        result["calibration"]["parameter_descriptors"][3]["unit"],
        "N/m"
    );
    assert_eq!(
        result["selection_json"],
        result["calibration"]["selection_json"]
    );
    let raw = result["selection_json"].as_str().unwrap();
    let selection: Selection = serde_json::from_str(raw).unwrap();
    let inspected: Value = serde_json::from_str(&materials::inspect(raw).unwrap()).unwrap();
    assert_eq!(inspected, result["material"]);
    assert_eq!(inspected["selection_json"], raw);
    assert_eq!(
        inspected["capabilities"]["mechanical_coefficients"]["ready"],
        false
    );
    let material = selection.record.as_ref().unwrap();
    assert!(material.properties.is_empty());
    assert!(material.membrane_response.is_some());
    let resolved = ftx_pattern::textile_import::catalog()
        .unwrap()
        .resolve(&selection)
        .unwrap();
    assert!(resolved.membrane_response().unwrap().is_some());
    let request = Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selection,
        fixture: input.experiments[0].fixture.clone(),
        extensions: input.experiments[0]
            .observations
            .iter()
            .map(|o| o.extension_fraction)
            .collect(),
        controls: input.controls.clone(),
    };
    let forward: Value = serde_json::from_str(
        &materials::strip_experiment(&serde_json::to_string(&request).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        forward["input"]["material"]["expected_sha256"],
        result["selection"]["expected_sha256"]
    );
    assert_eq!(
        forward["states"].as_array().unwrap().len(),
        request.extensions.len()
    );
    assert!(forward["states"][1]["force_n"].as_f64().unwrap() > 0.0);
    let denied: Value =
        serde_json::from_str(&materials::calibrate_strip(NONLINEAR, false).unwrap()).unwrap();
    assert_eq!(denied["calibration"]["accepted"], true);
    assert_eq!(
        denied["calibration"]["parameters"],
        result["calibration"]["parameters"]
    );
    assert_eq!(denied["selection"]["allow_estimates"], false);
    assert_eq!(
        denied["calibration"]["input"]["response_family"],
        result["calibration"]["input"]["response_family"]
    );
    let denied_forward = Request {
        material: serde_json::from_str(denied["selection_json"].as_str().unwrap()).unwrap(),
        ..request
    };
    assert_eq!(
        materials::strip_experiment(&serde_json::to_string(&denied_forward).unwrap())
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );

    let mut changed = input;
    changed
        .experiments
        .iter_mut()
        .find(|e| e.partition == ftx_pattern::material_calibration::Partition::Check)
        .unwrap()
        .observations[0]
        .force_n += 10.0;
    let rejected: Value = serde_json::from_str(
        &materials::calibrate_strip(&serde_json::to_string(&changed).unwrap(), true).unwrap(),
    )
    .unwrap();
    assert_eq!(rejected["calibration"]["accepted"], false);
    assert_eq!(
        rejected["calibration"]["parameters"],
        result["calibration"]["parameters"]
    );
    assert_eq!(
        rejected["calibration"]["parameter_descriptors"],
        result["calibration"]["parameter_descriptors"]
    );
    for field in ["material", "selection", "selection_json"] {
        assert!(rejected[field].is_null() && rejected["calibration"][field].is_null());
    }
}

#[test]
fn independent_interval_strip_api_matches_core_and_replays_exact_selection() {
    let input: strip::fit::Input = serde_json::from_str(INDEPENDENT_INTERVALS).unwrap();
    let core = strip::fit::calibrate(&input).unwrap();
    assert!(core.accepted, "{:?}", core.reasons);
    assert!(core.domain_replay_verified);
    assert_eq!(core.parameters.len(), 9);
    let result: Value =
        serde_json::from_str(&materials::calibrate_strip(INDEPENDENT_INTERVALS, true).unwrap())
            .unwrap();
    assert_eq!(result["calibration"], serde_json::to_value(&core).unwrap());
    assert!(
        result["calibration"]["input"]
            .get("response_family")
            .is_none()
    );
    assert!(
        result["calibration"]["input"]
            .get("parameter_bounds")
            .is_none()
    );
    let model = input.nonlinear.as_ref().unwrap();
    let bounds: Vec<_> = model
        .tangent_bounds
        .iter()
        .flatten()
        .copied()
        .chain([model.coupling_bounds])
        .collect();
    assert_eq!(core.parameter_bounds, bounds);
    let descriptors = core.parameter_descriptors.as_ref().unwrap();
    assert_eq!(descriptors.len(), 9);
    assert_eq!(descriptors[0].id, "warp_tangent_0");
    assert_eq!(descriptors[8].id, "normal_coupling");
    assert!(descriptors.iter().all(|parameter| parameter.unit == "N/m"));

    let raw_selection = core.selection_json.as_deref().unwrap();
    assert_eq!(result["selection_json"], raw_selection);
    let inspected: Value =
        serde_json::from_str(&materials::inspect(raw_selection).unwrap()).unwrap();
    assert_eq!(inspected, result["material"]);
    assert_eq!(inspected["selection_json"], raw_selection);
    assert_eq!(
        inspected["capabilities"]["mechanical_coefficients"]["ready"],
        false
    );
    assert_eq!(result["fit_claim"], false);
    assert_eq!(result["empirical_validation_claim"], false);
    let selection: Selection = serde_json::from_str(raw_selection).unwrap();
    let resolved = ftx_pattern::textile_import::catalog()
        .unwrap()
        .resolve(&selection)
        .unwrap();
    assert_eq!(
        serde_json::to_string(&resolved.frozen_selection()).unwrap(),
        raw_selection
    );
    assert!(resolved.membrane_response().unwrap().is_some());
    assert!(resolved.material().properties.is_empty());
    assert!(resolved.mechanics().is_err());

    let acquired = input
        .experiments
        .iter()
        .find(|experiment| {
            experiment.partition == ftx_pattern::material_calibration::Partition::Check
        })
        .unwrap();
    let mut fixture = acquired.fixture.clone();
    fixture.cells.iter_mut().for_each(|cells| *cells *= 2);
    let request = Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selection,
        fixture,
        extensions: acquired
            .observations
            .iter()
            .map(|observation| observation.extension_fraction)
            .collect(),
        controls: input.controls.clone(),
    };
    let forward = strip::evaluate(&request).unwrap();
    assert_eq!(
        materials::strip_experiment(&serde_json::to_string(&request).unwrap()).unwrap(),
        serde_json::to_string(&forward).unwrap()
    );
    for (observation, state) in acquired.observations.iter().zip(&forward.states) {
        let prediction = core
            .predictions
            .iter()
            .find(|prediction| {
                prediction.experiment == acquired.id && prediction.observation == observation.id
            })
            .unwrap();
        assert!((state.force_n - prediction.state.force_n).abs() < 1e-7);
        assert!((state.mid_width_m - prediction.state.mid_width_m).abs() < 1e-9);
    }
}

#[test]
fn fresh_scalar_area_api_publishes_the_complete_frozen_law_and_replays_it() {
    let input: strip::fit::Input = serde_json::from_str(SCALAR_AREA).unwrap();
    assert!(input.material.properties.is_empty());
    assert!(input.material.membrane_response.is_none());
    assert!(input.response_family.is_none());
    let result: Value =
        serde_json::from_str(&materials::calibrate_strip(SCALAR_AREA, true).unwrap()).unwrap();
    let calibration = &result["calibration"];
    assert_eq!(calibration["accepted"], true, "{calibration}");
    assert_eq!(calibration["domain_replay_verified"], true);
    let parameters = calibration["parameters"].as_array().unwrap();
    assert_eq!(parameters.len(), 6);
    for (actual, expected) in parameters.iter().zip([
        294.44444444444446,
        160.6060606060606,
        -0.4444444444444444,
        96.0,
        31.0,
        19.0,
    ]) {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-4);
    }
    assert_eq!(calibration["parameter_descriptors"][2]["unit"], "1");
    assert_eq!(
        calibration["parameter_descriptors"][4]["id"],
        "area_quadratic"
    );
    assert_eq!(result["simulation_ready"], false);
    assert_eq!(result["fit_claim"], false);
    let frozen = result["selection_json"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&materials::inspect(frozen).unwrap()).unwrap(),
        result["material"]
    );
    let selection: Selection = serde_json::from_str(frozen).unwrap();
    assert_eq!(
        serde_json::to_value(&selection).unwrap(),
        result["selection"]
    );
    let record = selection.record.as_ref().unwrap();
    assert!(record.properties.is_empty());
    let area = record
        .membrane_response
        .as_ref()
        .unwrap()
        .area_response
        .as_ref()
        .unwrap();
    assert_eq!(
        json!(area.area_ratio_range),
        calibration["fitted_area_ratio_range"]
    );
    assert!(area.area_ratio_range[0] > 0.65 && area.area_ratio_range[1] < 1.45);
    assert_eq!(
        area.quadratic.evidence,
        ftx_pattern::materials::Evidence::Estimate
    );

    let acquired = input
        .experiments
        .iter()
        .find(|e| e.partition == ftx_pattern::material_calibration::Partition::Check)
        .unwrap();
    let mut fixture = acquired.fixture.clone();
    fixture.cells.iter_mut().for_each(|cells| *cells *= 2);
    let request = Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selection,
        fixture,
        extensions: acquired
            .observations
            .iter()
            .map(|o| o.extension_fraction)
            .collect(),
        controls: input.controls.clone(),
    };
    let raw_forward =
        materials::strip_experiment(&serde_json::to_string(&request).unwrap()).unwrap();
    let forward: Value = serde_json::from_str(&raw_forward).unwrap();
    let prediction = calibration["predictions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["experiment"] == acquired.id)
        .unwrap();
    assert!(
        (forward["states"][0]["force_n"].as_f64().unwrap()
            - prediction["state"]["force_n"].as_f64().unwrap())
        .abs()
            < 1e-7
    );
    assert_eq!(
        materials::strip_experiment(&forward["input"].to_string()).unwrap(),
        raw_forward
    );
    let mut denied = request;
    denied.material.allow_estimates = false;
    assert_eq!(
        materials::strip_experiment(&serde_json::to_string(&denied).unwrap())
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
    let mut missing: Value = serde_json::from_str(SCALAR_AREA).unwrap();
    missing["area_response"]
        .as_object_mut()
        .unwrap()
        .remove("area_ratio_range");
    assert_eq!(
        materials::calibrate_strip(&missing.to_string(), true)
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn nonlinear_shape_evidence_requires_its_own_permission_before_any_candidate_is_published() {
    let original: Value = serde_json::from_str(NONLINEAR).unwrap();
    for omitted in [false, true] {
        let mut denied = original.clone();
        if omitted {
            denied["response_family"]
                .as_object_mut()
                .unwrap()
                .remove("allow_estimated_shapes");
        } else {
            denied["response_family"]["allow_estimated_shapes"] = json!(false);
        }
        let refusal = materials::calibrate_strip(&denied.to_string(), true).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::InvalidDocument);
        assert!(refusal.message.contains("allow_estimated_shapes"));
    }
    let mut denied = original;
    denied["allow_estimated_observations"] = json!(false);
    assert_eq!(
        materials::calibrate_strip(&denied.to_string(), true)
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn strip_apis_refuse_malformed_unknown_and_oversized_inputs_before_any_result() {
    let mut request = serde_json::to_value(forward()).unwrap();
    request["assume_homogeneous_strain"] = json!(true);
    assert_eq!(
        materials::strip_experiment(&request.to_string())
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
    let huge = " ".repeat(64 * 1024 * 1024 + 1);
    type JsonApi = fn(&str) -> Result<String, ftx_core::Refusal>;
    let apis: [JsonApi; 3] = [
        materials::strip_acquisition,
        materials::strip_experiment,
        |input| materials::calibrate_strip(input, false),
    ];
    for api in apis {
        for invalid in ["{", "{}", "null"] {
            assert_eq!(api(invalid).unwrap_err().code, RefusalCode::InvalidDocument);
        }
        assert_eq!(api(&huge).unwrap_err().code, RefusalCode::BudgetExceeded);
    }
}
