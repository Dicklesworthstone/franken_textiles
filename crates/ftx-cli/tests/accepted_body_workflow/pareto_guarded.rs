//! Independent named-case guards on real mass and signed heat tradeoffs, with shared quotas.
//! The body and material properties are synthetic fixtures, not empirical garment evidence.
use super::*;

#[test]
fn independent_case_gates_preserve_signed_heat_tradeoffs_and_exact_native_outputs() {
    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body = json!({"schema":"ftx-scenario-body/1","accepted_body_fit":receipt(&digest),
        "arm_abduction_deg":35.0,"measurements":{"wearer":{"stature_mm":1111.0}}});
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let width = std::f64::consts::TAU * 180.0;
    pattern["pieces"][0]["vertices"]["b"] = json!([width, 0.]);
    pattern["pieces"][0]["vertices"]["c"] = json!([width, 100.]);
    pattern["pieces"][0]["vertices"]["d"] = json!([0., 100.]);
    pattern["pieces"][0]["placement"]["radius_mm"] = json!(180.);
    pattern["pieces"][0]["placement"]["x0"] = json!(width / 2.);
    let light = pattern["fabric"]["material"].clone();
    let mut heavy = light.clone();
    heavy["record"]["properties"]["areal_density"]["value"] = json!(0.4);
    let mut study = json!({"schema":"ftx-design-study/1",
        "design_adaptive":{"objective":"consumer_pareto","material_key":"light","case_key":"guide",
            "max_evaluations":8,"max_designs":3,"variables":[{
                "key":"length","bounds_mm":[-30.,0.],"initial_mm":0.,"initial_step_mm":20.,"minimum_step_mm":20.,
                "sites":[{"piece":"band","axis":"y","at_mm":50.,"multiplier":1.}]}]},
        "design_fit":{"fit_spec":{"schema":"fabric-fit/1","points":[{
            "id":"declared-height-band","kind":"vertical","instance":"band","from":"a","to":"d",
            "body":"stature","body_scale":0.05,"target_mm":[0.,20.,100.]}]}},
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,"seam_allowance_mm":0,"extras":[]},
        "variants":[{"key":"light","changes":{"schema":"ftx-material-bindings/1","default":light}},
            {"key":"heavy","changes":{"schema":"ftx-material-bindings/1","default":heavy}}],
        "comparison":{"basis":"nominal","objectives":[
            {"metric":"assembly_mass_kg","direction":"minimize"},
            {"metric":"independent_surface_power_w","direction":"minimize"}]}});
    for (index, value) in [0.1, 0.4].into_iter().enumerate() {
        study["variants"][index]["changes"]["default"]["record"]["properties"]["thermal_resistance"] = json!({"value":value,"unit":"m2*K/W","source":"test","evidence":"estimate",
                "method":"Synthetic guarded Pareto heat integration"});
    }
    // Reversed reservoir temperatures make larger area more negative in signed power while
    // increasing mass. Both declared objectives minimize: neither design strictly dominates.
    let scenario = json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":100.,"sew_steps":120,"drape_steps":0,"step":0.001},
        "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":1000000,
        "surface":{"schema":"ftx-steady-surface-conditions/1","boundaries":[{
            "inside_c":10.,"outside_c":30.,"inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2}]}});
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    save(&dir.join("pattern.json"), &pattern);
    save(&dir.join("body-input.json"), &body);
    let capture = std::env::var_os("FTX_CONSUMER_ADAPTIVE_CAPTURE_DIR")
        .map(|path| PathBuf::from(path).join("guarded"));
    if let Some(capture) = &capture {
        std::fs::create_dir_all(capture).unwrap();
        std::fs::write(capture.join("model.ftxbody"), &bytes).unwrap();
        save(&capture.join("pattern.json"), &pattern);
        save(&capture.join("body-input.json"), &body);
    }
    for limited in [false, true] {
        let label = if limited {
            "pareto-guarded-budget"
        } else {
            "pareto-guarded"
        };
        let mut matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":32,
            "cases":[{"key":"guide","scenario":scenario},{"key":"guard","scenario":scenario,
                "comparison":study["comparison"]}]});
        matrix["cases"][1]["comparison"]["constraints"] = json!([
            {"metric":"assembly_mass_kg","minimum":0.017}]);
        if limited {
            matrix["cases"][1]["scenario"]["max_vertex_frames"] = json!(25000);
        }
        let report: Value = serde_json::from_str(
            &ftx_wasm::materials::scenario::on_model(
                &model,
                &digest,
                &pattern.to_string(),
                &study.to_string(),
                &matrix.to_string(),
                &body.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap();
        let search = &report["manufacturing"]["design_search"];
        let guidance = &search["consumer_guidance"];
        let frontier = &guidance["pareto_frontier"];
        let rows = search["rows"].as_array().unwrap();
        let selected: Vec<_> = rows.iter().filter(|row| row["selected"] == true).collect();
        assert_eq!(selected.len(), if limited { 1 } else { 2 }, "{search}");
        assert!(guidance.get("objective").is_none());
        assert!(guidance["best_reading"].is_null());
        assert!(guidance["best_trial_key"].is_null());
        assert_eq!(guidance["objectives"], study["comparison"]["objectives"]);
        assert_eq!(frontier["retention_capacity"], 3);
        assert_eq!(frontier["unretained_trial_keys"], json!([]));
        assert_eq!(
            frontier["retained_trial_keys"],
            frontier["evaluated_trial_keys"]
        );
        assert_eq!(guidance["reserved_evaluations"], 32);
        assert_eq!(search["global_optimum_claim"], false);
        assert_eq!(
            frontier["retained_trial_keys"],
            json!(
                selected
                    .iter()
                    .map(|row| &row["design"]["key"])
                    .collect::<Vec<_>>()
            )
        );
        for selected in &selected {
            let length = selected["design"]["variables_mm"]["length"]
                .as_f64()
                .unwrap();
            let expected_mass = width * (100. + length) * 1e-6 * 0.2;
            let readings = &selected["consumer_comparison"]["readings"];
            let mass = readings["assembly_mass_kg"]["value"].as_f64().unwrap();
            let power = readings["independent_surface_power_w"]["value"]
                .as_f64()
                .unwrap();
            assert!((mass - expected_mass).abs() < 1e-6);
            assert!(power < 0.);
            let key = serde_json::to_string(&json!([selected["design"]["key"], "light"])).unwrap();
            let manufacture = report["manufacturing"]["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == key)
                .unwrap();
            assert_eq!(
                manufacture["assembly_mass"]["result"]["assembly_mass"]["kg"],
                mass
            );
            for (index, case) in report["cases"].as_array().unwrap().iter().enumerate() {
                let simulation = case["simulations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["key"] == key)
                    .unwrap();
                let actual = &simulation["evaluation"]["result"];
                assert_eq!(simulation["evaluation"]["status"], "available");
                assert_eq!(actual["drape"]["settled"], true);
                assert_eq!(actual["metrics"]["simulation_gating_clear"], true);
                assert_eq!(actual["surface"]["result"]["steady"]["power_w"], power);
                assert_eq!(
                    actual["pattern_sha256"],
                    manufacture["binding"]["result"]["pattern_sha256"]
                );
                let candidate = case["comparison"]["alternatives"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["key"] == key)
                    .unwrap();
                for field in [
                    "readings",
                    "eligible",
                    "exclusions",
                    "input_protocol_sha256",
                ] {
                    assert_eq!(
                        selected["consumer_cases"][index]["comparison"][field],
                        candidate[field]
                    );
                }
            }
        }
        if !limited {
            let initial = &selected[0]["consumer_comparison"]["readings"];
            let shorter = &selected[1]["consumer_comparison"]["readings"];
            assert!(
                initial["assembly_mass_kg"]["value"].as_f64().unwrap()
                    > shorter["assembly_mass_kg"]["value"].as_f64().unwrap()
            );
            assert!(
                initial["independent_surface_power_w"]["value"]
                    .as_f64()
                    .unwrap()
                    < shorter["independent_surface_power_w"]["value"]
                        .as_f64()
                        .unwrap()
            );
        }
        let rejected = rows
            .iter()
            .find(|row| {
                row["design"]["variables_mm"]["length"] == if limited { -20. } else { -30. }
            })
            .unwrap();
        assert_eq!(rejected["selected"], false);
        assert_eq!(
            rejected["consumer_cases"][0]["comparison"]["eligible"],
            true
        );
        assert_eq!(
            rejected["consumer_cases"][1]["comparison"]["eligible"],
            false
        );
        if limited {
            assert_eq!(
                rejected["consumer_cases"][1]["refusal"]["code"],
                "budget_exceeded"
            );
        }
        let final_rows = report["manufacturing"]["alternatives"]
            .as_array()
            .unwrap()
            .len();
        let probes = guidance["evaluations"].as_u64().unwrap() as usize;
        assert_eq!(
            report["evaluations"],
            2 * (probes + final_rows - selected.len())
        );
        let study_file = format!("{label}-study.json");
        let scenario_file = format!("{label}-scenario.json");
        save(&dir.join(&study_file), &study);
        save(&dir.join(&scenario_file), &matrix);
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
                    &study_file,
                    &scenario_file,
                    "body-input.json",
                    "model.ftxbody",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(actual.get("result").unwrap_or(&actual), &report);
        }
        if let Some(capture) = &capture {
            save(&capture.join(study_file), &study);
            save(&capture.join(scenario_file), &matrix);
            save(&capture.join(format!("{label}-report.json")), &report);
        }
    }
}
