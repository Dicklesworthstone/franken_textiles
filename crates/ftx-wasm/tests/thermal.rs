//! Native execution of the file-free entry point exported by wasm-bindgen, not browser parity.
use ftx_drape::thermal::steady::request::Request;
use ftx_pattern::materials::Catalog;
use ftx_wasm::thermal;
use serde_json::{Value, json};

const INPUT: &str = include_str!("../../../tests/fixtures/materials/fsri-steady-conduction.json");

#[test]
fn browser_entry_point_matches_shared_workbench_and_replays_frozen_request() {
    let request: Request = serde_json::from_str(INPUT).unwrap();
    let expected = request.evaluate(&Catalog::builtin().unwrap()).unwrap();
    let output = thermal::steady(INPUT).unwrap();
    let actual: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        output,
        thermal::steady(&actual["request"].to_string()).unwrap()
    );
    assert_eq!(actual["storage_required"], false);
    assert_eq!(actual["comfort_claim"], false);
}

#[test]
fn browser_entry_point_preserves_evidence_and_domain_refusals() {
    assert!(thermal::steady("not JSON").is_err());
    let mut input: Value = serde_json::from_str(INPUT).unwrap();
    input["layers"][0]["allow_estimates"] = json!(false);
    assert!(thermal::steady(&input.to_string()).is_err());
    input["layers"][0]["allow_estimates"] = json!(true);
    input["outside_c"] = json!(10.0);
    input["outside_resistance_m2_k_w"] = json!(0.0);
    assert!(thermal::steady(&input.to_string()).is_err());
}

const SURFACE_REPORT: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-steady-surface-report.json");
const SURFACE_CONDITIONS: &str =
    include_str!("../../../tests/fixtures/materials/steady-surface-conditions.json");

#[test]
fn surface_entry_matches_shared_library_and_replays_without_catalog_lookup() {
    use ftx_drape::thermal::steady::request::SurfaceConditions;
    let c: SurfaceConditions = serde_json::from_str(SURFACE_CONDITIONS).unwrap();
    let expected = c
        .evaluate(&serde_json::from_str(SURFACE_REPORT).unwrap())
        .unwrap();
    let actual = thermal::steady_surface(SURFACE_REPORT, SURFACE_CONDITIONS).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&actual).unwrap(), expected);
    assert_eq!(
        actual,
        thermal::steady_surface(
            &expected["request"]["report"].to_string(),
            &expected["request"]["conditions"].to_string()
        )
        .unwrap()
    );
    assert_eq!(expected["storage_required"], false);
}

#[test]
fn surface_entry_rejects_unsupported_changed_and_oversized_inputs() {
    assert!(thermal::steady_surface("{}", SURFACE_CONDITIONS).is_err());
    assert!(thermal::steady_surface(SURFACE_REPORT, "{}").is_err());
    let mut wrong: Value = serde_json::from_str(SURFACE_REPORT).unwrap();
    wrong["steady_thermal_surface"]["positions_m"][0][0] = json!(1e100);
    assert!(thermal::steady_surface(&wrong.to_string(), SURFACE_CONDITIONS).is_err());
    let mut wrong: Value = serde_json::from_str(SURFACE_CONDITIONS).unwrap();
    wrong["transient"] = json!({"steps":10});
    assert!(thermal::steady_surface(SURFACE_REPORT, &wrong.to_string()).is_err());
    let oversized = " ".repeat(64 * 1024 * 1024);
    assert_eq!(
        thermal::steady_surface(&oversized, SURFACE_CONDITIONS)
            .unwrap_err()
            .code,
        ftx_core::RefusalCode::BudgetExceeded
    );
}

#[test]
fn exterior_entry_uses_same_material_policy_replay_and_finish_sensitive_equations() {
    let text = include_str!("../../../tests/fixtures/materials/synthetic-exterior-exchange.json");
    let mut input: Value = serde_json::from_str(text).unwrap();
    let first: Value = serde_json::from_str(&thermal::steady(text).unwrap()).unwrap();
    let request: Request = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(
        first,
        request.evaluate(&Catalog::builtin().unwrap()).unwrap()
    );
    assert_eq!(
        first,
        serde_json::from_str::<Value>(&thermal::steady(&first["request"].to_string()).unwrap())
            .unwrap()
    );
    input["layers"][0]["record"]["properties"]["emissivity"]["value"] = json!(0.05);
    let second: Value =
        serde_json::from_str(&thermal::steady(&input.to_string()).unwrap()).unwrap();
    assert!(first["power_w"].as_f64().unwrap() > second["power_w"].as_f64().unwrap());
    assert_ne!(first["request_sha256"], second["request_sha256"]);
    let mut tampered = first["request"].clone();
    tampered["layers"][0]["record"]["properties"]["emissivity"]["value"] = json!(0.05);
    assert!(thermal::steady(&tampered.to_string()).is_err());
    input["exterior_exchange"]["radiation"]["exposed_face"] = json!("reverse");
    assert!(thermal::steady(&input.to_string()).is_err());
    let mut conditions: Value = serde_json::from_str(SURFACE_CONDITIONS).unwrap();
    conditions["boundaries"][0]["outside_c"] = json!(5.);
    conditions["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.);
    conditions["exterior_exchange"] = json!([input["exterior_exchange"]]);
    conditions["exterior_exchange"][0]
        .as_object_mut()
        .unwrap()
        .remove("radiation");
    let actual: Value = serde_json::from_str(
        &thermal::steady_surface(SURFACE_REPORT, &conditions.to_string()).unwrap(),
    )
    .unwrap();
    let c: ftx_drape::thermal::steady::request::SurfaceConditions =
        serde_json::from_value(conditions).unwrap();
    assert_eq!(
        actual,
        c.evaluate(&serde_json::from_str(SURFACE_REPORT).unwrap())
            .unwrap()
    );
}
