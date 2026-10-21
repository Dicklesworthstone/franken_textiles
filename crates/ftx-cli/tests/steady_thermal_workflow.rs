//! Production workbench over sourced HFM runs; area, reservoirs and films are illustrative inputs.
use ftx_drape::thermal::steady::request::Request;
use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog};
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/fsri-steady-conduction.json"
    ))
    .unwrap()
}
fn cli(input: &Value) -> Output {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "ftx-steady-{}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let bytes = serde_json::to_vec(input).unwrap();
    fs::write(&path, &bytes).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-steady")
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(fs::read(path).unwrap(), bytes);
    out
}
fn success(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn refusal(out: &Output) {
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let v: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert!(v["refusal"].is_object());
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8 * (1.0 + b.abs()), "{a} != {b}");
}

#[test]
fn real_binary_matches_library_and_independent_hfm_fourier_equations() {
    let input = fixture();
    let out = cli(&input);
    let result = success(&out);
    let request: Request = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(
        result,
        request.evaluate(&Catalog::builtin().unwrap()).unwrap()
    );
    assert_eq!(out.stdout, cli(&input).stdout);
    let a = result["state"]["layer_faces_c"][0][0].as_f64().unwrap();
    let b = result["state"]["layer_faces_c"][0][1].as_f64().unwrap();
    let q = result["state"]["flux_w_m2"].as_f64().unwrap();
    // Independently transcribed source readings, not values extracted from the candidate curve.
    let low = (0.02232 + 0.02276 + 0.02282) / 3.0;
    let high = (0.02681 + 0.02685 + 0.02688) / 3.0;
    let primitive = |t: f64| low * (t - 15.0) + (high - low) / 50.0 * (t - 15.0) * (t - 15.0) / 2.0;
    near(q, (primitive(a) - primitive(b)) / 0.0003);
    near(60.0 - a, q * 0.01);
    near(b - 20.0, q * 0.02);
    near(result["power_w"].as_f64().unwrap(), q * 0.01);
    assert_eq!(result["storage_required"], false);
    assert_eq!(result["comfort_claim"], false);
    assert_eq!(result["uncertainty_propagated"], false);
    assert_eq!(result["empirical_validation_claim"], false);
    assert!(
        result["request"]["layers"][0]["record"]["properties"]
            .get("areal_density")
            .is_none()
    );
}

#[test]
fn changing_only_selected_material_changes_the_result_and_locked_requests_replay_offline() {
    let mut input = fixture();
    let original = success(&cli(&input));
    input["layers"][0]["id"] = json!("fsri/hfm/cotton-sheet/wet/220318-r1");
    let changed = success(&cli(&input));
    assert_ne!(original["request_sha256"], changed["request_sha256"]);
    assert!(
        (original["power_w"].as_f64().unwrap() - changed["power_w"].as_f64().unwrap()).abs() > 0.1
    );
    let frozen: Request = serde_json::from_value(changed["request"].clone()).unwrap();
    let empty = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    assert_eq!(changed, frozen.evaluate(&empty).unwrap());
    assert_eq!(changed, success(&cli(&changed["request"])));
    let mut tampered = changed["request"].clone();
    tampered["layers"][0]["record"]["properties"]["thickness"]["value"] = json!(0.9);
    refusal(&cli(&tampered));
}

#[test]
fn capability_discovery_distinguishes_steady_transient_and_mechanical_readiness() {
    let id = "fsri/hfm/cotton-sheet/dry/220330-r1";
    let command = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
            .args(args)
            .output()
            .unwrap()
    };
    let denied = success(&command(&["show", id]));
    assert_eq!(denied["steady_conduction"]["ready"], false);
    let allowed = success(&command(&["show", id, "--allow-estimates"]));
    assert_eq!(allowed["steady_conduction"]["ready"], true);
    assert_eq!(
        allowed["steady_conduction"]["temperature_domain_c"],
        json!([15.0, 65.0])
    );
    assert_eq!(allowed["transient_thermal"]["ready"], false);
    assert_eq!(allowed["simulation_ready"], false);
    assert_eq!(allowed["selection"]["allow_estimates"], true);
    refusal(&command(&["show", id, "--allow-anything"]));
    // Previously withheld NIST Cp does not prevent its qualified conductivity being used.
    let nist = success(&command(&[
        "show",
        "nist/7282/cotton-duck",
        "--allow-estimates",
    ]));
    assert_eq!(nist["steady_conduction"]["ready"], true);
    assert_eq!(nist["transient_thermal"]["ready"], false);
}

#[test]
fn malformed_unapproved_out_of_domain_and_overflowing_requests_publish_no_partial_result() {
    for (key, value) in [
        ("schema", json!("wrong")),
        ("area_m2", json!(0.0)),
        ("area_m2", json!(1e308)),
        ("cells_per_layer", json!(1)),
        ("interface_resistance_m2_k_w", json!([0.0])),
    ] {
        let mut input = fixture();
        input[key] = value;
        refusal(&cli(&input));
    }
    let mut input = fixture();
    input["layers"][0]["allow_estimates"] = json!(false);
    refusal(&cli(&input));
    input = fixture();
    input["outside_c"] = json!(10.0);
    input["outside_resistance_m2_k_w"] = json!(0.0);
    refusal(&cli(&input));
    input = fixture();
    input["layers"][0]["id"] = json!("fsri/hfm/cotton-sheet/dry/unknown-run");
    refusal(&cli(&input));
}
