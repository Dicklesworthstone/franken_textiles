//! Actual sewn-cloth objectives guide bounded design through both CLIs and portable authority.
//! The body and source properties are synthetic software fixtures, not measured garment evidence.
use super::*;

#[test]
fn consumer_adaptive_uses_real_scenario_mass_and_preserves_cached_incumbent_on_budget_refusal() {
    use ftx_wasm::materials::scenario::on_model;

    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body = json!({"schema":"ftx-scenario-body/1",
        "accepted_body_fit":receipt(&digest),"arm_abduction_deg":35.0,
        "measurements":{"wearer":{"stature_mm":1111.0}}});
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let width = std::f64::consts::TAU * 180.0;
    pattern["pieces"][0]["vertices"]["b"] = json!([width, 0.0]);
    pattern["pieces"][0]["vertices"]["c"] = json!([width, 100.0]);
    pattern["pieces"][0]["vertices"]["d"] = json!([0.0, 100.0]);
    pattern["pieces"][0]["placement"]["radius_mm"] = json!(180.0);
    pattern["pieces"][0]["placement"]["x0"] = json!(width / 2.0);
    let light = pattern["fabric"]["material"].clone();
    let mut heavy = light.clone();
    heavy["record"]["properties"]["areal_density"]["value"] = json!(0.4);
    let study = json!({"schema":"ftx-design-study/1",
        "design_adaptive":{"objective":"consumer_comparison","material_key":"light",
            "variables":[{"key":"length","bounds_mm":[-30.0,0.0],"initial_mm":0.0,
                "initial_step_mm":20.0,"minimum_step_mm":20.0,
                "sites":[{"piece":"band","axis":"y","at_mm":50.0,"multiplier":1.0}]}],
            "max_evaluations":8,"max_designs":1},
        "design_fit":{"fit_spec":{"schema":"fabric-fit/1","points":[{
            "id":"declared-height-band","kind":"vertical","instance":"band",
            "from":"a","to":"d","body":"stature","body_scale":0.05,
            "target_mm":[0.0,20.0,100.0]}]}},
        "variants":[
            {"key":"light","changes":{"schema":"ftx-material-bindings/1","default":light}},
            {"key":"heavy","changes":{"schema":"ftx-material-bindings/1","default":heavy}}],
        "comparison":{"basis":"nominal",
            "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}]}});
    // The existing production sewing-only fixture is settled and gating-clear. This does not
    // claim gravity-settled fit; the comparator still admits the complete real drape descriptor.
    let scenario = json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":100.0,"sew_steps":120,"drape_steps":0,"step":0.001},
        "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":1000000});
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    save(&dir.join("pattern.json"), &pattern);
    save(&dir.join("study.json"), &study);
    save(&dir.join("body-input.json"), &body);
    let capture = std::env::var_os("FTX_CONSUMER_ADAPTIVE_CAPTURE_DIR").map(PathBuf::from);
    if let Some(capture) = &capture {
        std::fs::create_dir_all(capture).unwrap();
        std::fs::write(capture.join("model.ftxbody"), &bytes).unwrap();
        save(&capture.join("pattern.json"), &pattern);
        save(&capture.join("study.json"), &study);
        save(&capture.join("body-input.json"), &body);
    }
    let mut limited = scenario.clone();
    // One initial 98-point conservative reservation at 212 frames fits. Subsequent probes
    // exceed the SAME Session's quota; they cannot reset it or replace the cached incumbent.
    limited["max_vertex_frames"] = json!(25000);
    for (name, scenario) in [("complete", scenario), ("budget", limited)] {
        let report: Value = serde_json::from_str(
            &on_model(
                &model,
                &digest,
                &pattern.to_string(),
                &study.to_string(),
                &scenario.to_string(),
                &body.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap();
        let search = &report["manufacturing"]["design_search"];
        let guidance = &search["consumer_guidance"];
        assert_eq!(search["selected_designs"], 1, "{search}");
        assert_eq!(search["complete_search"], false);
        assert_eq!(search["global_optimum_claim"], false);
        assert_eq!(guidance["objective"]["metric"], "simulated_cloth_mass_kg");
        assert_eq!(guidance["reserved_evaluations"], 12);
        let selected = search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|trial| trial["selected"] == true)
            .unwrap();
        let coordinate = selected["design"]["variables_mm"]["length"]
            .as_f64()
            .unwrap();
        assert_eq!(coordinate, if name == "complete" { -30.0 } else { 0.0 });
        let selected_key =
            serde_json::to_string(&json!([selected["design"]["key"], "light"])).unwrap();
        let row = report["manufacturing"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == selected_key)
            .unwrap();
        let simulation = report["simulations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == selected_key)
            .unwrap();
        assert_eq!(
            simulation["evaluation"]["status"], "available",
            "{simulation}"
        );
        assert_eq!(simulation["evaluation"]["result"]["drape"]["settled"], true);
        assert_eq!(
            simulation["evaluation"]["result"]["pattern_sha256"],
            row["binding"]["result"]["pattern_sha256"]
        );
        assert_eq!(report["comparison"]["frontier"], json!([selected_key]));
        let candidate = &selected["consumer_comparison"];
        assert_eq!(candidate["eligible"], true);
        assert_eq!(
            guidance["best_reading"],
            candidate["readings"]["simulated_cloth_mass_kg"]
        );
        let value = guidance["best_reading"]["value"].as_f64().unwrap();
        let expected = width * (100.0 + coordinate) * 1e-6 * 0.2;
        assert!((value - expected).abs() < 1e-6, "{value} / {expected}");
        let altered = ftx_pattern::Pattern::from_json(
            row["design"]["altered_pattern_json"].as_str().unwrap(),
        )
        .unwrap();
        let frozen: ftx_pattern::material_binding::Changes =
            serde_json::from_value(row["frozen_changes"].clone()).unwrap();
        assert_eq!(
            frozen.apply(&altered).unwrap()["pattern"],
            row["binding"]["result"]["pattern"]
        );
        if name == "complete" {
            assert_eq!(search["rows"][0]["selected"], false);
            assert_eq!(search["rows"][0]["max_fit_band_violation_mm"], 0.0);
            assert!(value < search["rows"][0]["consumer_comparison"]["readings"]["simulated_cloth_mass_kg"]["value"].as_f64().unwrap());
        } else {
            assert!(
                search["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .skip(1)
                    .any(|trial| {
                        trial["consumer_refusal"]["code"] == "budget_exceeded"
                            && trial["consumer_comparison"]["eligible"] == false
                    })
            );
        }
        save(&dir.join("scenario.json"), &scenario);
        for (binary, prefix) in [
            (
                env!("CARGO_BIN_EXE_ftx"),
                &["--json", "materials", "scenario-study"][..],
            ),
            (env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"][..]),
        ] {
            let output = Command::new(binary)
                .current_dir(&dir)
                .args(prefix)
                .args([
                    "pattern.json",
                    "study.json",
                    "scenario.json",
                    "body-input.json",
                    "model.ftxbody",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(actual.get("result").unwrap_or(&actual), &report);
        }
        if let Some(capture) = &capture {
            save(&capture.join(format!("{name}-scenario.json")), &scenario);
            save(&capture.join(format!("{name}-report.json")), &report);
        }
    }
}
