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
