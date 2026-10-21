//! Real catalog observations through the executable steady solver; boundaries are synthetic.
use ftx_drape::thermal::steady::request::Request;
use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog};
use serde_json::{Value, json};
use std::{path::Path, process::Command};

#[test]
fn neoprene_fixture_runs_with_measured_resistance_and_replays_without_a_catalog() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/materials/measured-neoprene-steady.json");
    let output = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-steady")
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let request: Request = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/measured-neoprene-steady.json"
    ))
    .unwrap();
    let catalog = Catalog::builtin().unwrap();
    assert_eq!(result, request.evaluate(&catalog).unwrap());
    // Source Table 1 M3NOb5 Rct = 0.1720 m2*K/W. All other numbers are declared scenario inputs.
    let expected_flux = 15.0 / (0.03 + 0.1720 + 0.05);
    assert!((result["state"]["flux_w_m2"].as_f64().unwrap() - expected_flux).abs() < 1e-8);
    assert!((result["power_w"].as_f64().unwrap() - 0.6 * expected_flux).abs() < 1e-8);
    let faces = &result["state"]["layer_faces_c"][0];
    assert!((faces[0].as_f64().unwrap() - (35.0 - 0.03 * expected_flux)).abs() < 1e-8);
    assert!((faces[1].as_f64().unwrap() - (20.0 + 0.05 * expected_flux)).abs() < 1e-8);
    assert_eq!(result["storage_required"], false);
    assert_eq!(result["empirical_validation_claim"], false);
    let properties = &result["request"]["layers"][0]["record"]["properties"];
    assert_eq!(
        properties["compression_displacement_200kpa"]["value"],
        3.7701
    );
    assert_eq!(properties["compression_displacement_200kpa"]["unit"], "mm");
    assert_eq!(properties["thermal_resistance"]["evidence"], "measured");
    let frozen: Request = serde_json::from_value(result["request"].clone()).unwrap();
    let offline = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    assert_eq!(result, frozen.evaluate(&offline).unwrap());
    let mut changed = serde_json::to_value(&request).unwrap();
    changed["layers"][0]["id"] = json!("zagreb/neoprene-2025/m1no5");
    let changed: Request = serde_json::from_value(changed).unwrap();
    let changed = changed.evaluate(&catalog).unwrap();
    assert!((changed["power_w"].as_f64().unwrap() - 9.0 / (0.08 + 0.0915)).abs() < 1e-8);
    assert_ne!(result["request_sha256"], changed["request_sha256"]);
    assert!(changed["power_w"].as_f64().unwrap() > result["power_w"].as_f64().unwrap());
}
