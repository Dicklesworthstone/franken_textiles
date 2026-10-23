//! Shape-reuse CLI against a tiny closed synthetic Anny-format model.
//! Includes the real image-fit producer; no production-body or real-wearer accuracy claim.

#[path = "accepted_body_workflow/adaptive.rs"]
mod adaptive;
#[path = "accepted_body_workflow/outfit.rs"]
mod outfit;

use ftx_core::receipt::sha256_hex;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

fn model_bytes() -> Vec<u8> {
    model_bytes_with_rig(false)
}

#[test]
fn fit_driven_design_grid_uses_one_selected_reference_body_in_native_and_portable_scenarios() {
    use ftx_drape::study::fitted::{BodyRequest, FittedBody};
    use ftx_wasm::materials::scenario::{on_body, on_model};

    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    // The accepted synthetic shape is 1300 mm tall. Conflicting new tape stature is retained
    // as a residual; it must never replace the already selected model's flat-fit target.
    let body_input = json!({"schema":"ftx-scenario-body/1",
        "accepted_body_fit":receipt(&digest),"arm_abduction_deg":50.0,
        "measurements":{"wearer":{"stature_mm":1111.0}}});
    let fitted =
        FittedBody::prepare(&model, &digest, &BodyRequest::read(&body_input).unwrap()).unwrap();
    let reference = fitted.flat_fit_body(&model, &digest).unwrap();
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let mut request: Value =
        serde_json::from_str(include_str!("../../../examples/design-study.json")).unwrap();
    request.as_object_mut().unwrap().remove("designs");
    request.as_object_mut().unwrap().remove("sewing");
    request["variants"].as_array_mut().unwrap().truncate(1);
    request["design_grid"] = json!({"variables":[{"key":"length","values_mm":[0.0,30.0,40.0],
        "sites":[{"piece":"front","axis":"y","at_mm":50.0,"multiplier":1.0},
            {"piece":"back","axis":"y","at_mm":50.0,"multiplier":1.0}]}],"max_designs":3});
    request["design_fit"] = json!({"fit_spec":{"schema":"fabric-fit/1",
        "seam_tol_mm":3.0,"truing_tol_deg":8.0,"points":[{
            "id":"synthetic_scaled_stature","kind":"vertical","instance":"front",
            "from":"a","to":"d","body":"stature","body_scale":0.1,
            "target_mm":[-0.1,0.0,0.1]}]}});
    request["comparison"] = json!({"basis":"nominal",
        "objectives":[{"metric":"stock_mass_kg","direction":"minimize"}]});
    let scenario = json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":100.0,"sew_steps":3,"drape_steps":3,"step":0.001},
        "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":1000000});
    let portable = |scenario: &Value, body_input: &Value| -> Value {
        serde_json::from_str(
            &on_model(
                &model,
                &digest,
                pattern,
                &request.to_string(),
                &scenario.to_string(),
                &body_input.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap()
    };
    let single = portable(&scenario, &body_input);
    let winner = json!(serde_json::to_string(&("grid:0001", "light-sheet")).unwrap());
    assert_eq!(
        single["comparison"]["frontier"],
        json!([winner]),
        "{single}"
    );
    assert_eq!(single["scenario"]["design_fit_body"], *reference.identity());
    assert_eq!(
        single["scenario"]["design_fit_body"]["measurement_pose"],
        "a_pose_35"
    );
    assert_eq!(
        single["scenario"]["body_fit"]["pose"]["arm_abduction_deg"],
        50.0
    );
    for row in single["manufacturing"]["alternatives"].as_array().unwrap() {
        let fit = &row["design"]["fit"]["result"];
        let passed = row["design"]["key"] == "grid:0001";
        assert_eq!(fit["body_identity"], *reference.identity());
        assert_eq!(fit["validation"]["points"][0]["body_mm"], 130.0);
        assert_eq!(fit["passed"], passed);
        assert_eq!(
            row["binding"]["status"],
            if passed { "available" } else { "refused" }
        );
        assert_eq!(
            row["cutting"]["status"],
            if passed { "available" } else { "refused" }
        );
    }
    let matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":8,
        "cases":[{"key":"first","scenario":scenario},{"key":"second","scenario":scenario}]});
    let combined = portable(&matrix, &body_input);
    assert_eq!(combined["planned_evaluations"], 8);
    assert_eq!(combined["evaluations"], 2);
    assert_eq!(combined["comparison"]["frontier"], json!([winner]));
    assert_eq!(
        combined["manufacturing"]["alternatives"],
        single["manufacturing"]["alternatives"]
    );
    for case in combined["cases"].as_array().unwrap() {
        assert_eq!(case["scenario"]["design_fit_body"], *reference.identity());
        assert_eq!(case["comparison"]["frontier"], json!([winner]));
    }

    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    std::fs::write(dir.join("pattern.json"), pattern).unwrap();
    save(&dir.join("study.json"), &request);
    save(&dir.join("body-input.json"), &body_input);
    for (scenario, expected) in [(&scenario, &single), (&matrix, &combined)] {
        save(&dir.join("scenario.json"), scenario);
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
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result.get("result").unwrap_or(&result), expected);
        }
    }

    // A raw collision/placement body does not carry a selected shape measurement authority.
    let raw: Value = serde_json::from_str(
        &on_body(
            &fitted.body(),
            pattern,
            &request.to_string(),
            &matrix.to_string(),
            |_| {},
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(raw["evaluations"], 0);
    assert_eq!(raw["comparison"]["frontier"], json!([]));
    assert!(raw["cases"][0]["scenario"]["design_fit_setup_refusal"]["message"].is_string());
    assert!(
        raw["manufacturing"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["design"]["fit"]["status"] == "refused"
                && row["binding"]["status"] == "refused")
    );
    let mut invalid_body = body_input;
    invalid_body["accepted_body_fit"]["status"] = json!("REJECTED");
    let refused = portable(&scenario, &invalid_body);
    assert!(refused["scenario"]["design_fit_setup_refusal"]["message"].is_string());
    assert!(refused["scenario"]["design_fit_body"].is_null());
    assert_eq!(refused["comparison"]["frontier"], json!([]));
    assert!(
        refused["simulations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["evaluation"]["status"] == "refused")
    );
}

#[test]
fn fit_screening_reaches_a_late_feasible_design_with_one_downstream_slot() {
    use ftx_drape::study::fitted::{BodyRequest, FittedBody};
    use ftx_wasm::materials::scenario::on_model;

    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body_input = json!({"schema":"ftx-scenario-body/1",
        "accepted_body_fit":receipt(&digest),"arm_abduction_deg":50.0,
        "measurements":{"wearer":{"stature_mm":1111.0}}});
    let fitted =
        FittedBody::prepare(&model, &digest, &BodyRequest::read(&body_input).unwrap()).unwrap();
    let reference = fitted.flat_fit_body(&model, &digest).unwrap();
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let mut request: Value =
        serde_json::from_str(include_str!("../../../examples/design-study.json")).unwrap();
    request.as_object_mut().unwrap().remove("designs");
    request.as_object_mut().unwrap().remove("sewing");
    request["variants"].as_array_mut().unwrap().truncate(1);
    // This test must reach the actual drape, so use the existing complete synthetic mechanics
    // selection rather than the manufacturing-only material supplied by the design example.
    let mechanical_pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    request["variants"][0]["changes"]["default"] = mechanical_pattern["fabric"]["material"].clone();
    let mut placed_pattern: Value = serde_json::from_str(pattern).unwrap();
    // Place the two synthetic strips on adjoining arcs outside the tiny model's torso.
    // Manufacturing examples deliberately carry no simulation placement or fit claim.
    for (index, piece) in placed_pattern["pieces"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        piece["placement"] = json!({"part":"torso","radius_mm":180.0,
            "x0":if index == 0 {10.0} else {-10.0}});
    }
    let placed_json = serde_json::to_string(&placed_pattern).unwrap();
    let pattern = placed_json.as_str();
    request["design_grid"] = json!({"variables":[{"key":"length",
        "values_mm":[0.0,5.0,10.0,15.0,20.0,25.0,30.0,35.0,40.0],
        "sites":[{"piece":"front","axis":"y","at_mm":50.0,"multiplier":1.0},
            {"piece":"back","axis":"y","at_mm":50.0,"multiplier":1.0}]}],
        "max_designs":1,"fit_screening":{"max_candidates":9}});
    request["design_fit"] = json!({"fit_spec":{"schema":"fabric-fit/1",
        "seam_tol_mm":3.0,"truing_tol_deg":8.0,"points":[{
            "id":"synthetic_scaled_stature","kind":"vertical","instance":"front",
            "from":"a","to":"d","body":"stature","body_scale":0.1,
            "target_mm":[-0.1,0.0,0.1]}]}});
    request["comparison"] = json!({"basis":"nominal",
        "objectives":[{"metric":"stock_mass_kg","direction":"minimize"}]});
    let scenario = json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":100.0,"sew_steps":3,"drape_steps":3,"step":0.001},
        "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":1000000});
    let matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":4,
        "cases":[{"key":"first","scenario":scenario},{"key":"second","scenario":scenario}]});
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    std::fs::write(dir.join("pattern.json"), pattern).unwrap();
    save(&dir.join("study.json"), &request);
    save(&dir.join("body-input.json"), &body_input);
    let export = std::env::var_os("FTX_DESIGN_SCREENING_REPLAY").map(PathBuf::from);
    if let Some(export) = &export {
        std::fs::create_dir_all(export).unwrap();
        std::fs::write(export.join("model.ftxbody"), &bytes).unwrap();
        std::fs::write(export.join("pattern.json"), pattern).unwrap();
        save(&export.join("study.json"), &request);
        save(&export.join("body-input.json"), &body_input);
    }
    let winner = json!(serde_json::to_string(&("grid:0006", "light-sheet")).unwrap());
    for scenario in [&scenario, &matrix] {
        let actual: Value = serde_json::from_str(
            &on_model(
                &model,
                &digest,
                pattern,
                &request.to_string(),
                &scenario.to_string(),
                &body_input.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(actual["comparison"]["frontier"], json!([winner]));
        let manufacturing = &actual["manufacturing"];
        assert_eq!(manufacturing["alternatives"].as_array().unwrap().len(), 2);
        let screening = &manufacturing["design_search"]["fit_screening"];
        assert_eq!(screening["screened_designs"], 9);
        assert_eq!(screening["qualified_designs"], 1);
        assert_eq!(screening["selected_designs"], 1);
        assert_eq!(screening["rejected_designs"], 8);
        assert_eq!(screening["unvisited_designs"], 0);
        for row in screening["rows"].as_array().unwrap() {
            assert_eq!(
                row["design"]["fit"]["result"]["body_identity"],
                *reference.identity()
            );
            assert_eq!(row["selected"], row["design"]["key"] == "grid:0006");
        }
        let selected = manufacturing["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == winner)
            .unwrap();
        assert_eq!(selected["cutting"]["status"], "available");
        assert_eq!(selected["design"]["variables_mm"]["length"], 30.0);
        if actual["cases"].is_array() {
            assert_eq!(actual["planned_evaluations"], 4);
            assert_eq!(actual["evaluations"], 2);
        } else {
            let simulated = actual["simulations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == winner)
                .unwrap();
            assert_eq!(
                simulated["evaluation"]["status"], "available",
                "{simulated}"
            );
        }
        save(&dir.join("scenario.json"), scenario);
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
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let native: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(native.get("result").unwrap_or(&native), &actual);
        }
        if let Some(export) = &export {
            let name = if actual["cases"].is_array() {
                "matrix"
            } else {
                "single"
            };
            save(&export.join(format!("{name}-scenario.json")), scenario);
            save(&export.join(format!("{name}-report.json")), &actual);
        }
    }
}

#[test]
fn finite_body_water_objectives_use_actual_motion_in_native_and_portable_scenarios() {
    use ftx_drape::study::fitted::{BodyRequest, FittedBody};
    use ftx_wasm::materials::scenario::on_model;

    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body_request = json!({"schema":"ftx-scenario-body/1",
        "accepted_body_fit":receipt(&digest),"arm_abduction_deg":35.0});
    let fitted =
        FittedBody::prepare(&model, &digest, &BodyRequest::read(&body_request).unwrap()).unwrap();
    let body = fitted.body();
    // Select a genuine vertical face of this synthetic prism and cover its entire height.
    // The small original band covers only part of a tall body triangle, which cannot qualify
    // complete-coverage objectives. The domain has no anatomical interpretation.
    let (body_triangle, triangle) = body
        .collision
        .1
        .iter()
        .enumerate()
        .find(|(_, t)| {
            let [a, b, c] = t.map(|i| body.collision.0[i]);
            let normal = (b - a).cross(c - a);
            normal.z.abs() < 0.1 * normal.norm()
        })
        .unwrap();
    let frame = body.frames.frame("torso").unwrap();
    let heights = triangle.map(|i| (body.collision.0[i] * 1000.0 - frame.origin).dot(frame.up));
    let low = heights.into_iter().fold(f64::INFINITY, f64::min) - 50.0;
    let high = heights.into_iter().fold(f64::NEG_INFINITY, f64::max) + 50.0;
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let width = 180.0 * std::f64::consts::TAU;
    let piece = &mut pattern["pieces"][0];
    piece["vertices"]["b"][0] = json!(width);
    piece["vertices"]["c"] = json!([width, high - low]);
    piece["vertices"]["d"][1] = json!(high - low);
    piece["placement"]["radius_mm"] = json!(180.0);
    piece["placement"]["x0"] = json!(width / 2.0);
    piece["placement"]["y0"] = json!(body.frames.landmark_h("torso", "waist").unwrap() - low);
    let mut fast = pattern["fabric"]["material"].clone();
    fast["record"]["properties"]["vapor_resistance"] = json!({
        "value":7.0,"unit":"m2*Pa/W","source":"test","evidence":"estimate",
        "method":"Synthetic constant Ret for finite-body-water comparison",
        "conditions":{"conditioning":"synthetic dry coupon"}});
    let mut slow = fast.clone();
    slow["record"]["properties"]["vapor_resistance"]["value"] = json!(17.0);
    let mut study = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"a-fast","changes":{"schema":"ftx-material-bindings/1","default":fast}},
        {"key":"b-slow","changes":{"schema":"ftx-material-bindings/1","default":slow}}],
        "cutting":{"copies":1,"gap_mm":1.0,"edge_margin_mm":0.0,
            "end_margin_mm":0.0,"seam_allowance_mm":0.0}});
    let mut scenario: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-scenario.json"
    ))
    .unwrap();
    scenario.as_object_mut().unwrap().remove("coverage");
    scenario["options"] = json!({"spacing":100.0,"sew_steps":120,"drape_steps":0,"step":0.001});
    scenario["vapor_coverage"] = json!({"body_triangle_ids":[body_triangle],
        "instance_layers":{"band":"shell"},
        "settings":{"max_distance_m":0.2,"min_normal_cosine":0.1,
            "separation_tolerance_m":0.00001,"interfaces":[]},
        "conditions":{"boundaries":[{"inside_vapor_pressure_pa":2000.0,
            "outside_vapor_pressure_pa":1000.0,"inside_film_resistance_m2_pa_w":1.0,
            "outside_film_resistance_m2_pa_w":2.0,"latent_heat_j_kg":2_500_000.0}]}});
    scenario["motion"] = json!({"schema":"ftx-drape-motion/1",
        "keyframes":[{"time_s":0.002,"arm_abduction_deg":35.1},
            {"time_s":0.004,"arm_abduction_deg":35.2}],"max_step_s":0.001,
        "max_trials":16,"max_pairs":100000,"max_sample_vertices":10000,
        "vapor_coverage_history":{"schema":"ftx-motion-vapor-coverage/1",
            "integration":"right_endpoint","max_geometry_operations":1_000_000}});
    let portable = |request: &Value, scenarios: &Value| -> Value {
        serde_json::from_str(
            &on_model(
                &model,
                &digest,
                &pattern.to_string(),
                &request.to_string(),
                &scenarios.to_string(),
                &body_request.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap()
    };
    let initial = portable(&study, &scenario);
    let initial_simulation = &initial["simulations"][0]["evaluation"]["result"];
    assert_eq!(
        initial_simulation["metrics"]["simulation_gating_clear"], true,
        "{initial_simulation}"
    );
    assert_eq!(initial_simulation["motion"]["status"], "available");
    let initial_coverage = &initial_simulation["vapor_coverage"]["result"];
    assert_eq!(
        initial_coverage["coverage"]["uncovered_area_m2"], 0.0,
        "{initial_coverage}"
    );
    let film = json!({"schema":"ftx-body-wet-film/1",
        "body_mesh_sha256":initial_coverage["body_mesh_sha256"],
        "source":"test:explicit-body-water",
        "basis":"Synthetic initial film and constant body supply; no inferred wearer physiology",
        "initial_liquid_mass_kg":[0.0],"supply_rate_kg_s":[0.0]});
    scenario["motion"]["vapor_coverage_history"]["wet_film"] = film;
    let policy = |maximum| {
        json!({"basis":"nominal",
        "required_consumers":["motion_wet_film"],
        "objectives":[{"metric":"motion_wet_film_evaporated_mass_kg","direction":"maximize"}],
        "constraints":[{"metric":"motion_wet_film_evaporated_mass_kg","minimum":0.0,"maximum":maximum},
            {"metric":"motion_wet_film_remaining_liquid_mass_kg","minimum":0.0,"maximum":0.0},
            {"metric":"motion_wet_film_unmet_demand_mass_kg","minimum":0.0},
            {"metric":"motion_wet_film_supplied_mass_kg","minimum":0.0,"maximum":5e-13},
            {"metric":"motion_wet_film_body_reservoir_latent_energy_j","minimum":0.0,"maximum":5e-6}]})
    };
    study["comparison"] = policy(2e-12);
    let mut supplied = scenario.clone();
    supplied["motion"]["vapor_coverage_history"]["wet_film"]["initial_liquid_mass_kg"] =
        json!([1e-12]);
    supplied["motion"]["vapor_coverage_history"]["wet_film"]["supply_rate_kg_s"] = json!([1e-10]);
    let matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":4,
        "cases":[{"key":"dry-body","scenario":scenario,"comparison":policy(0.0)},
            {"key":"supply-limited","scenario":supplied}]});
    let result = portable(&study, &matrix);
    assert_eq!(
        result["comparison"]["frontier"],
        json!(["a-fast", "b-slow"]),
        "{}",
        result["cases"]
    );
    for (index, case) in result["cases"].as_array().unwrap().iter().enumerate() {
        assert_eq!(case["comparison"]["frontier"], json!(["a-fast", "b-slow"]));
        let mut potentials = Vec::new();
        let mut evaporation = Vec::new();
        for (candidate, simulation) in case["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["simulations"].as_array().unwrap())
        {
            assert_eq!(candidate["eligible"], true, "{candidate}");
            let current = &simulation["evaluation"]["result"];
            let motion = &current["motion"]["result"];
            assert_eq!(current["metrics"]["simulation_gating_clear"], true);
            assert_eq!(
                motion["samples"],
                initial_simulation["motion"]["result"]["samples"]
            );
            let history = &motion["vapor_coverage_history"]["result"];
            let water = &history["wet_film"]["totals"];
            for (metric, field, unit) in [
                (
                    "motion_wet_film_evaporated_mass_kg",
                    "evaporated_mass_kg",
                    "kg",
                ),
                (
                    "motion_wet_film_remaining_liquid_mass_kg",
                    "remaining_liquid_mass_kg",
                    "kg",
                ),
                (
                    "motion_wet_film_unmet_demand_mass_kg",
                    "unmet_demand_mass_kg",
                    "kg",
                ),
                ("motion_wet_film_supplied_mass_kg", "supplied_mass_kg", "kg"),
                (
                    "motion_wet_film_body_reservoir_latent_energy_j",
                    "body_reservoir_latent_energy_j",
                    "J",
                ),
            ] {
                let reading = &candidate["readings"][metric];
                assert_eq!(reading["value"], water[field]);
                assert_eq!(reading["unit"], unit);
                assert_eq!(reading["consumer"], "motion_wet_film");
            }
            let actual = water["evaporated_mass_kg"].as_f64().unwrap();
            let expected = if index == 0 {
                0.0
            } else {
                1e-12 + 1e-10 * 0.004
            };
            assert!((actual - expected).abs() < 1e-25);
            assert_eq!(water["remaining_liquid_mass_kg"], 0.0);
            assert!(
                (water["body_reservoir_latent_energy_j"].as_f64().unwrap() - actual * 2_500_000.0)
                    .abs()
                    < 1e-18
            );
            assert!(water["mass_balance_error_kg"].as_f64().unwrap().abs() < 1e-25);
            potentials.push(history["totals"]["potential_mass_kg"].as_f64().unwrap());
            evaporation.push(actual);
            let replay: ftx_drape::vapor::coverage::history::Request =
                serde_json::from_str(history["replay_request_json"].as_str().unwrap()).unwrap();
            assert_eq!(replay.evaluate().unwrap(), *history);
        }
        assert!(potentials[0] > potentials[1] && potentials[1] > 2e-12);
        assert_eq!(evaporation[0], evaporation[1]);
    }

    // Both executable boundaries and the file-free browser boundary consume the same real
    // model, source materials, sewn geometry and continuing motion; no callback report is forged.
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    for (name, value) in [
        ("pattern.json", &pattern),
        ("study.json", &study),
        ("scenario.json", &matrix),
        ("body-input.json", &body_request),
    ] {
        save(&dir.join(name), value);
    }
    for (binary, prefix) in [
        (env!("CARGO_BIN_EXE_ftx-materials"), vec!["scenario-study"]),
        (
            env!("CARGO_BIN_EXE_ftx"),
            vec!["--json", "materials", "scenario-study"],
        ),
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
        let document: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(document.get("result").unwrap_or(&document), &result);
    }
    let mut frozen = study.clone();
    for (variant, row) in frozen["variants"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(result["manufacturing"]["alternatives"].as_array().unwrap())
    {
        variant["changes"] = row["frozen_changes"].clone();
    }
    let replay = portable(&frozen, &matrix);
    for index in 0..2 {
        assert_eq!(
            replay["cases"][index]["simulations"],
            result["cases"][index]["simulations"]
        );
        assert_eq!(
            replay["cases"][index]["comparison"]["frontier"],
            result["cases"][index]["comparison"]["frontier"]
        );
    }
    for invalid in [Value::Null, json!("stale")] {
        let mut bad = matrix.clone();
        let wet = &mut bad["cases"][1]["scenario"]["motion"]["vapor_coverage_history"]["wet_film"];
        if invalid.is_null() {
            *wet = Value::Null;
        } else {
            wet["body_mesh_sha256"] = json!("0".repeat(64));
        }
        let refused = portable(&study, &bad);
        assert_eq!(refused["comparison"]["frontier"], json!([]));
        assert_eq!(
            refused["cases"][0]["comparison"]["frontier"],
            json!(["a-fast", "b-slow"])
        );
        assert_eq!(refused["cases"][1]["comparison"]["frontier"], json!([]));
        for simulation in refused["cases"][1]["simulations"].as_array().unwrap() {
            let motion = &simulation["evaluation"]["result"]["motion"];
            assert_eq!(motion["status"], "available");
            for endpoint in motion["result"]["keyframes"].as_array().unwrap() {
                assert_eq!(endpoint["vapor_coverage"]["status"], "available");
                assert_eq!(endpoint["surface"]["status"], "available");
            }
        }
        for row in refused["manufacturing"]["alternatives"].as_array().unwrap() {
            assert_eq!(row["cutting"]["status"], "available");
        }
    }
}

fn model_bytes_with_rig(articulated: bool) -> Vec<u8> {
    // The placement case is an articulated closed prism; the original image-fit case is a box.
    // Both are synthetic analytic geometry, never a stand-in for anatomical acceptance.
    let mut bones = vec!["root"];
    let mut parents = vec![-1];
    let mut heads = vec![0.0; 3];
    let mut tails = vec![0.0, 0.0, 0.1];
    if articulated {
        bones.extend([
            "upperarm01.R",
            "upperarm02.R",
            "lowerarm01.R",
            "wrist.R",
            "upperarm01.L",
            "upperarm02.L",
            "lowerarm01.L",
            "wrist.L",
            "upperleg01.R",
            "lowerleg01.R",
            "foot.R",
            "upperleg01.L",
            "lowerleg01.L",
            "foot.L",
            "neck01",
            "head",
        ]);
        parents.extend([0, 1, 2, 3, 0, 5, 6, 7, 0, 9, 10, 0, 12, 13, 0, 15]);
        for sign in [1.0, -1.0] {
            for j in 0..4 {
                let x = sign * (0.12 + 0.05 * f64::from(j));
                let z = 0.9 - 0.06 * f64::from(j);
                heads.extend([x, 0.0, z]);
                tails.extend([x + sign * 0.05, 0.0, z - 0.06]);
            }
        }
        for sign in [1.0, -1.0] {
            for z in [0.6, 0.3, 0.05] {
                heads.extend([sign * 0.06, 0.0, z]);
                tails.extend([sign * 0.06, 0.0, z - 0.05]);
            }
        }
        heads.extend([0.0, 0.0, 0.9, 0.0, 0.0, 1.0]);
        tails.extend([0.0, 0.0, 1.0, 0.0, 0.0, 1.1]);
    }
    let bone_count = bones.len();
    let mut arrays = BTreeMap::new();
    let mut data = Vec::new();
    let mut add = |name: &str, dtype: &str, shape: Vec<usize>, bytes: Vec<u8>| {
        arrays.insert(name.to_string(),json!({"dtype":dtype,"shape":shape,"offset":data.len(),"nbytes":bytes.len(),"sha256":sha256_hex(&bytes)}));
        data.extend(bytes);
    };
    let f64s = |a: Vec<f64>| a.into_iter().flat_map(f64::to_le_bytes).collect::<Vec<_>>();
    let u32s = |a: Vec<u32>| a.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
    let mut vertices = Vec::new();
    let mut influences = Vec::new();
    let mut faces = Vec::new();
    if articulated {
        for (r, z) in [0.0, 0.4, 0.8, 1.0].into_iter().enumerate() {
            for i in 0..16 {
                let (s, c) = ftx_core::det::sin_cos(f64::from(i) * std::f64::consts::TAU / 16.0);
                vertices.extend([0.1 * c, 0.1 * s, z]);
                influences.push(if r == 3 && [15, 0, 1].contains(&i) {
                    1
                } else if r == 3 && [7, 8, 9].contains(&i) {
                    5
                } else if r == 0 && c > 0.5 {
                    9
                } else if r == 0 && c < -0.5 {
                    12
                } else {
                    0
                });
            }
        }
        vertices.extend([0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        influences.extend([0, 0]);
        for i in 0..16 {
            let j = (i + 1) % 16;
            faces.extend([64, j, i, 65, 48 + i, 48 + j]);
            for r in 0..3 {
                let a = 16 * r + i;
                let b = 16 * r + j;
                faces.extend([a, b, b + 16, a, b + 16, a + 16]);
            }
        }
    } else {
        for z in [0.0, 1.0] {
            for y in [-0.1, 0.1] {
                for x in [-0.1, 0.1] {
                    vertices.extend([x, y, z]);
                }
            }
        }
        influences = vec![0; 8];
        faces = vec![
            0, 2, 3, 0, 3, 1, 4, 5, 7, 4, 7, 6, 0, 1, 5, 0, 5, 4, 2, 6, 7, 2, 7, 3, 0, 4, 6, 0, 6,
            2, 1, 3, 7, 1, 7, 5,
        ];
    }
    let n = vertices.len() / 3;
    let nfaces = faces.len() / 3;
    let delta = vertices
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|v| [0.0, 0.0, 0.5 * v[2]])
        .collect();
    add("template_vertices", "f64", vec![n, 3], f64s(vertices));
    add("faces", "u32", vec![nfaces, 3], u32s(faces));
    add(
        "blendshape_row_ptr",
        "u32",
        vec![2],
        u32s(vec![0, n as u32]),
    );
    add(
        "blendshape_vertex",
        "u32",
        vec![n],
        u32s((0..n as u32).collect()),
    );
    add("blendshape_delta", "f64", vec![n, 3], f64s(delta));
    let mut mask = vec![0; 26];
    mask[1] = 1;
    add("phenotype_mask", "u8", vec![1, 26], mask);
    add(
        "template_bone_heads",
        "f64",
        vec![bone_count, 3],
        f64s(heads),
    );
    add(
        "template_bone_tails",
        "f64",
        vec![bone_count, 3],
        f64s(tails),
    );
    add(
        "bone_heads_blendshapes",
        "f64",
        vec![1, bone_count, 3],
        f64s(vec![0.0; 3 * bone_count]),
    );
    add(
        "bone_tails_blendshapes",
        "f64",
        vec![1, bone_count, 3],
        f64s(vec![0.0; 3 * bone_count]),
    );
    add(
        "bone_rolls",
        "f64",
        vec![bone_count, 3, 3],
        f64s([1., 0., 0., 0., 1., 0., 0., 0., 1.].repeat(bone_count)),
    );
    add("vertex_bone_indices", "u32", vec![n, 1], u32s(influences));
    add("vertex_bone_weights", "f64", vec![n, 1], f64s(vec![1.0; n]));
    add(
        "base_mesh_vertex_indices",
        "u32",
        vec![n],
        u32s((0..n as u32).collect()),
    );
    let mut components = vec![json!(["height", "low"]), json!(["height", "high"])];
    components.extend((0..24).map(|i| json!(["race", format!("r{i}")])));
    let waist_loop: Vec<usize> = if articulated {
        (16..32).collect()
    } else {
        vec![]
    };
    let header = json!({"schema":"ftxbody/1","model":"anny","anny_version":"0.6.1","configuration":"synthetic interchange test",
        "licence":"synthetic","package_sha256":{},"counts":{"vertices":n,"faces":nfaces,"bones":bone_count,"blendshapes":1,"phenotype_blendshapes":1,"local_changes":0,"influences":1},
        "phenotype":{"features":["height","race"],"components":components,"anchors":{"height":[0,1]},"exposed":["height"],"fixed_value":0.5},
        "y_axis":[0,1,0],"degenerate_rotation":[[1,0,0],[0,1,0],[0,0,1]],"labels":{"blendshapes":["height-high"],"bones":bones,"bone_parents":parents,"local_changes":[]},"waist_loop_final":waist_loop,"arrays":arrays});
    let mut h = serde_json::to_vec(&header).unwrap();
    while !h.len().is_multiple_of(8) {
        h.push(b' ');
    }
    let mut out = b"FTXBODY\0".to_vec();
    out.extend(1u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend((h.len() as u64).to_le_bytes());
    out.extend(h);
    out.extend(data);
    out
}

fn receipt(hash: &str) -> Value {
    let parameter = json!([{"name":"height","lower":0.0,"upper":1.0}]);
    let attempt = json!({"status":"CONVERGED","flags":[],"checks":[{"name":"p0","unit":"px","residual":0.25,"limit":3.0,"passed":true}]});
    json!({"schema":"ftx-body-fit/1","status":"ACCEPTED","input_sha256":"b".repeat(64),
        "input":{"starts":[{},{}],"frames":[{},{}]},"attempts":[attempt,attempt],
        "options":{"max_shape_spread_fraction":0.01},"shape_spread_fraction":[0.0],
        "model":{"model_id":format!("anny/0.6.1/sha256:{hash}"),"shape":parameter},
        "anny_configuration":{"model_sha256":hash,"age_years":2.5,"gender":1.0,"fixed_shape":{},"shape_parameters":parameter},
        "solution":{"shape":[0.6],"poses":[{"values":[5000,1000,1000]}],"anny_parameters":{"provenance":"MODEL_FIT",
            "phenotype":{"gender":1.0,"age":ftx_body::age_parameter_for_years(2.5),"height":0.6,"weight":0.5,"muscle":0.5,"proportions":0.5},"local_changes":[]}},
        "advisories":["Synthetic interchange receipt; not a physical fit"],"no_claim":"Synthetic test"})
}

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-shape-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let bytes = model_bytes();
    save(&dir.join("fit.json"), &receipt(&sha256_hex(&bytes)));
    std::fs::write(dir.join("model.ftxbody"), bytes).unwrap();
    dir
}
fn invoke(dir: &Path, args: &[&str], expected: i32) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{args:?}: {} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn body(dir: &Path, extra: &[&str], expected: i32) -> Value {
    let mut args = vec![
        "body",
        "--body-fit",
        "fit.json",
        "--model",
        "model.ftxbody",
        "--pose",
        "rest",
    ];
    args.extend(extra);
    serde_json::from_slice(&invoke(dir, &args, expected).stdout).unwrap()
}

#[test]
fn body_reuses_exact_shape_not_image_pose_or_new_stature() {
    let dir = setup();
    save(
        &dir.join("measurements.json"),
        &json!({"wearer":{"age_months":30,"sex":"F","stature_mm":1111}}),
    );
    let before = std::fs::read(dir.join("fit.json")).unwrap();
    let value = body(
        &dir,
        &["--out", "first", "--measurements", "measurements.json"],
        0,
    );
    let doc = &value["result"]["body"];
    assert_eq!(doc["schema"], "ftx-body/1");
    assert_eq!(doc["readings_mm"]["stature"], 1300.0);
    assert_eq!(doc["shape_source"]["refitted"], false);
    assert_eq!(doc["shape_source"]["image_poses_reused"], false);
    assert_eq!(doc["phenotype"]["height"], 0.6);
    assert_eq!(doc["model_fit"], json!([]));
    assert_eq!(doc["tape_evaluation"]["status"], "UNAVAILABLE");
    assert_eq!(doc["source_fit"], read(&dir.join("fit.json")));
    assert_eq!(doc["measurement_comparison"][0]["residual_mm"], 189.0);
    assert_eq!(
        read(&dir.join("first/body_fit.json")),
        read(&dir.join("fit.json"))
    );
    assert_eq!(
        std::fs::read(dir.join("first/body_fit.json")).unwrap(),
        before
    );
    let mesh =
        ftx_drape::render::read_obj(&std::fs::read_to_string(dir.join("first/body.obj")).unwrap())
            .unwrap();
    assert_eq!(mesh.vertices.len(), 8);
    assert_eq!(mesh.triangles.len(), 12);
    assert_eq!(mesh.vertices.iter().map(|p| p.z).fold(0.0, f64::max), 1.3);
    body(&dir, &["--out", "second"], 0);
    assert_eq!(
        std::fs::read(dir.join("first/body.obj")).unwrap(),
        std::fs::read(dir.join("second/body.obj")).unwrap()
    );
    assert_eq!(std::fs::read(dir.join("fit.json")).unwrap(), before);
}

#[test]
fn actual_image_fit_report_round_trips_to_the_same_closed_body() {
    let dir = setup();
    let frame = |i| json!({"stream":"synthetic","timestamp_us":i * 100000});
    let mut frames = Vec::new();
    for (i, offset) in [0.04, 0.11].into_iter().enumerate() {
        // Independent analytic pinhole observations of the known 1.3 m cuboid, translated
        // separately in each image. No candidate fitter output supplies these pixels.
        let mut points = Vec::new();
        for z in [0.0, 1.3] {
            for y in [-0.1, 0.1] {
                for x in [-0.1, 0.1] {
                    points.push(json!({"name":format!("p{}",points.len()),
                        "pixel_px":[640.0+1000.0*x/(y+4.0),480.0-1000.0*(z+offset-0.7)/(y+4.0)],
                        "sigma_px":0.5,"partition":if points.len()==7 {"held_out"}else{"fit"}}));
                }
            }
        }
        frames.push(
            json!({"frame":frame(i),"image_size_px":[1280,960],"rotation_cw_deg":0,
            "camera":{"intrinsics":{"focal_px":[1000,1000],"principal_px":[640,480]},
            "world_to_camera":{"rows":[[1,0,0],[0,0,-1],[0,1,0]]},"center_m":[0,-4,0.7]},
            "keypoints":points}),
        );
    }
    let targets: BTreeMap<_, _> = (0..8)
        .map(|i| (format!("p{i}"), json!({"kind":"vertex","index":i})))
        .collect();
    let starts: Vec<_> = [0.4, 0.8]
        .into_iter()
        .map(|height| {
            json!({"shape":[height],"poses":[{"frame":frame(0),"values":[0.0]},
            {"frame":frame(1),"values":[0.2]}]})
        })
        .collect();
    let request = json!({"schema":"ftx-anny-body-fit-input/1",
        "model":{"model_sha256":sha256_hex(&model_bytes()),"age_years":2.5,"gender":1.0,
            "shape_parameters":[{"name":"height","lower":0.0,"upper":1.0}],
            "pose_parameters":[{"name":"root_z_m","lower":-1.0,"upper":1.0}],
            "keypoints":targets,"measurements":["stature"]},
        "fit":{"schema":"ftx-body-fit-input/1","metric_scale_source":"analytic metre camera",
            "observation_evidence":"synthetic cuboid corners","frames":frames,"measurements":[],"starts":starts}});
    save(&dir.join("observations.json"), &request);
    invoke(
        &dir,
        &[
            "measure",
            "fit-body",
            "observations.json",
            "--model",
            "model.ftxbody",
            "--out",
            "producer.json",
        ],
        0,
    );
    let source = read(&dir.join("producer.json"));
    assert_eq!(source["status"], "ACCEPTED");
    assert!((source["solution"]["shape"][0].as_f64().unwrap() - 0.6).abs() < 1e-7);
    let output = invoke(
        &dir,
        &[
            "body",
            "--body-fit",
            "producer.json",
            "--model",
            "model.ftxbody",
            "--pose",
            "rest",
            "--out",
            "reused",
        ],
        0,
    );
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    let doc = &output["result"]["body"];
    assert_eq!(doc["source_fit"], source);
    assert_eq!(doc["phenotype"]["height"], source["solution"]["shape"][0]);
    assert!((doc["readings_mm"]["stature"].as_f64().unwrap() - 1300.0).abs() < 1e-4);
    let mesh =
        ftx_drape::render::read_obj(&std::fs::read_to_string(dir.join("reused/body.obj")).unwrap())
            .unwrap();
    assert!(
        mesh.vertices
            .iter()
            .map(|p| p.z)
            .fold(f64::INFINITY, f64::min)
            .abs()
            < 1e-12
    );
    assert_eq!(
        std::fs::read(dir.join("reused/body_fit.json")).unwrap(),
        std::fs::read(dir.join("producer.json")).unwrap()
    );
}

#[test]
fn malformed_rejected_and_wrong_model_reports_publish_nothing() {
    for mutation in [
        "status",
        "parameters",
        "model",
        "age",
        "expanded",
        "damaged_model",
    ] {
        let dir = setup();
        let mut v = read(&dir.join("fit.json"));
        match mutation {
            "status" => v["status"] = json!("AMBIGUOUS"),
            "parameters" => v["solution"]["shape"][0] = json!(0.7),
            "model" => v["anny_configuration"]["model_sha256"] = json!("c".repeat(64)),
            "age" => v["anny_configuration"]["age_years"] = json!(5.0),
            "expanded" => v["solution"]["anny_parameters"]["local_changes"] = json!([0.0]),
            _ => std::fs::write(dir.join("model.ftxbody"), b"broken").unwrap(),
        }
        save(&dir.join("fit.json"), &v);
        body(&dir, &["--out", "nested/body"], 3);
        assert!(!dir.join("nested").exists(), "{mutation}");
    }
}

#[test]
fn demographic_conflicts_and_unknown_options_refuse_before_publication() {
    for wearer in [
        json!({"age_months":31,"sex":"F"}),
        json!({"age_months":30,"sex":"M"}),
    ] {
        let dir = setup();
        save(&dir.join("m.json"), &json!({"wearer":wearer}));
        body(&dir, &["--out", "body", "--measurements", "m.json"], 3);
        assert!(!dir.join("body").exists());
    }
    let dir = setup();
    body(&dir, &["--out", "body", "--typo", "x"], 3);
    assert!(!dir.join("body").exists());
}

#[test]
fn reused_shapes_never_overwrite_evidence_or_completed_outputs() {
    let dir = setup();
    let before = std::fs::read(dir.join("fit.json")).unwrap();
    body(&dir, &["--out", "fit.json"], 3);
    assert_eq!(std::fs::read(dir.join("fit.json")).unwrap(), before);
    body(&dir, &["--out", "body"], 0);
    let output = std::fs::read(dir.join("body/body.json")).unwrap();
    body(&dir, &["--out", "body"], 3);
    assert_eq!(std::fs::read(dir.join("body/body.json")).unwrap(), output);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        symlink(dir.join("body"), dir.join("alias")).unwrap();
        body(&dir, &["--out", "alias"], 3);
        for f in ["body.obj", "body.json", "body_fit.json"] {
            assert_eq!(
                std::fs::metadata(dir.join("body").join(f))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn drape_and_round_reach_shape_reuse_without_falling_back_to_tape() {
    let dir = setup();
    save(&dir.join("pattern.json"), &json!({}));
    save(&dir.join("m.json"), &json!({}));
    let mut report = read(&dir.join("fit.json"));
    report["status"] = json!("REJECTED");
    save(&dir.join("fit.json"), &report);
    let out = invoke(
        &dir,
        &[
            "drape",
            "pattern.json",
            "--measurements",
            "m.json",
            "--body-fit",
            "fit.json",
            "--model",
            "model.ftxbody",
            "--out",
            "sim",
        ],
        3,
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        result["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("ACCEPTED")
    );
    assert!(!dir.join("sim/sim_report.json").exists());
    invoke(&dir, &["round", "ws", "--body-fit", "fit.json"], 3);
    assert!(!dir.join("ws").exists());
}

#[test]
fn report_budget_is_enforced_before_model_or_output_access() {
    let dir = setup();
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("fit.json"))
        .unwrap();
    f.set_len(16 * 1024 * 1024 + 1).unwrap();
    body(&dir, &["--out", "body"], 4);
    assert!(!dir.join("body").exists());
}

#[test]
fn simulated_round_preserves_the_import_refusal_in_its_own_verdict() {
    let dir = setup();
    let fixtures: Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/oracle/draft/programs.json"
    ))
    .unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap();
    save(&dir.join("pattern.json"), &case["output"]["pattern"]);
    save(&dir.join("m.json"), &case["input"]["measurements"]);
    save(&dir.join("fit_spec.json"), &case["output"]["fit_spec"]);
    let mut fit = read(&dir.join("fit.json"));
    fit["status"] = json!("AMBIGUOUS");
    save(&dir.join("fit.json"), &fit);
    let original = std::fs::read(dir.join("fit.json")).unwrap();
    let output = invoke(
        &dir,
        &[
            "round",
            "ws",
            "--pattern",
            "pattern.json",
            "--measurements",
            "m.json",
            "--fit",
            "fit_spec.json",
            "--body-fit",
            "fit.json",
            "--model",
            "model.ftxbody",
            "--sim",
            "--as-of",
            "2026-10-13",
        ],
        2,
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let result = &value["result"];
    assert_eq!(result["status"], "SIM_GATING_FLAGS");
    assert_eq!(result["sim"]["settled"], false);
    assert!(
        result["sim"]["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("ACCEPTED")
    );
    assert_eq!(
        read(&dir.join("ws/rounds/000/sim_failure.json")),
        result["sim"]
    );
    assert!(result["viewer"].is_null());
    assert!(!dir.join("ws/rounds/000/sim/sim_report.json").exists());
    assert_eq!(std::fs::read(dir.join("fit.json")).unwrap(), original);
}

#[test]
fn scenario_binaries_replay_accepted_shape_and_pose_with_real_drape_and_heat() {
    let dir = setup();
    let bytes = model_bytes_with_rig(true);
    let accepted = receipt(&sha256_hex(&bytes));
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    save(&dir.join("accepted.json"), &accepted);
    let mut body_request = json!({"schema":"ftx-scenario-body/1","accepted_body_fit":accepted,
        "arm_abduction_deg":35.0,"measurements":{"wearer":{"stature_mm":1111.0}}});
    save(&dir.join("body-input.json"), &body_request);
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let width = std::f64::consts::TAU * 180.0;
    pattern["pieces"][0]["vertices"]["b"][0] = json!(width);
    pattern["pieces"][0]["vertices"]["c"][0] = json!(width);
    pattern["pieces"][0]["placement"]["radius_mm"] = json!(180.0);
    pattern["pieces"][0]["placement"]["x0"] = json!(width / 2.0);
    pattern["fabric"]["material"]["record"]["properties"]["specific_heat"] = json!({
        "value":1000.0,"unit":"J/(kg*K)","source":"test","evidence":"estimate",
        "method":"Synthetic transient-heat integration test; not measured textile data"});
    save(&dir.join("pattern.json"), &pattern);
    let material = pattern["fabric"]["material"].clone();
    let mut transport = material.clone();
    transport["record"]["properties"]["thermal_conductivity"]["value"] = json!(0.1);
    let study = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"baseline","changes":{"schema":"ftx-material-bindings/1","default":material}},
        {"key":"transport","changes":{"schema":"ftx-material-bindings/1","default":transport}}],
        "cutting":{"copies":1,"gap_mm":1.0,"edge_margin_mm":0.0,"end_margin_mm":0.0}});
    save(&dir.join("study.json"), &study);
    let mut scenario: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-scenario.json"
    ))
    .unwrap();
    scenario["options"]["spacing"] = json!(100.0);
    // No unsupported anatomical interpretation of the cylinder fixture's body patch index.
    scenario.as_object_mut().unwrap().remove("coverage");
    save(&dir.join("scenario.json"), &scenario);
    let run_in = |directory: &Path, binary: &str, prefix: &[&str]| {
        let out = Command::new(binary)
            .current_dir(directory)
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
            out.status.success(),
            "{} / {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let raw: Value = serde_json::from_slice(&out.stdout).unwrap();
        raw.get("result").unwrap_or(&raw).clone()
    };
    let run = |binary: &str, prefix: &[&str]| run_in(&dir, binary, prefix);
    let a = run(
        env!("CARGO_BIN_EXE_ftx"),
        &["--json", "materials", "scenario-study"],
    );
    let b = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
    assert_eq!(a, b);
    let fit = &a["scenario"]["body_fit"];
    assert_eq!(fit["shape_source"]["refitted"], false, "{}", a["scenario"]);
    assert_eq!(fit["shape_source"]["image_poses_reused"], false);
    assert_eq!(fit["source_fit"], accepted);
    assert_eq!(fit["residuals"][0]["residual_mm"], 189.0);
    assert_eq!(a["scenario"]["body_input"], body_request);
    let first = &a["simulations"][0]["evaluation"];
    let second = &a["simulations"][1]["evaluation"];
    assert_eq!(first["status"], "available", "{first}");
    assert_eq!(second["status"], "available", "{second}");
    assert_eq!(first["result"]["surface"]["status"], "available");
    assert_eq!(
        first["result"]["positions_sha256"],
        second["result"]["positions_sha256"]
    );
    assert_ne!(
        first["result"]["metrics"]["independent_surface_power_w"],
        second["result"]["metrics"]["independent_surface_power_w"]
    );
    assert_eq!(
        fit["shape_parameters"],
        accepted["solution"]["anny_parameters"]
    );
    body_request["arm_abduction_deg"] = json!(50.0);
    save(&dir.join("body-input.json"), &body_request);
    let posed = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
    assert_eq!(
        posed["scenario"]["body_fit"]["shape_sha256"],
        fit["shape_sha256"]
    );
    assert_ne!(posed["request_sha256"], a["request_sha256"]);
    assert_ne!(
        posed["scenario"]["body_geometry_sha256"],
        a["scenario"]["body_geometry_sha256"]
    );
    assert_eq!(posed["manufacturing"], a["manufacturing"]);

    // Actual model-backed continuation, not independent re-draping of these endpoint poses.
    body_request["arm_abduction_deg"] = json!(35.0);
    save(&dir.join("body-input.json"), &body_request);
    scenario["motion"] = json!({"schema":"ftx-drape-motion/1",
        "keyframes":[{"time_s":0.002,"arm_abduction_deg":35.1},
            {"time_s":0.004,"arm_abduction_deg":35.2}],
        "max_step_s":0.001,"max_trials":16,"max_pairs":100000,"max_sample_vertices":10000});
    let films = json!([
        {"inside_c":40.,"outside_c":5.,"inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2},
        {"inside_c":5.,"outside_c":40.,"inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2}]);
    scenario["motion"]["transient_surface"] = json!({"schema":"ftx-motion-surface-heat/1",
        "initial_temperature_c":15.,"cells_per_layer":2,"boundaries":films,
        "max_step_s":0.0005,"max_cell_steps":100000});
    // An explicit domain on this synthetic prism only; not an anatomical chest-patch convention.
    scenario["motion"]["transient_coverage"] = json!({"schema":"ftx-motion-coverage-heat/1",
        "body_triangle_ids":[4],"instance_layers":{"band":"shell"},
        "settings":{"max_distance_m":0.12,"min_normal_cosine":0.1,
            "separation_tolerance_m":0.00001,"cells_per_layer":2,"interfaces":[]},
        "initial_temperature_c":15.,"boundaries":films,"exposed_boundaries":films,
        "max_step_s":0.0005,"max_cell_steps":100000,"max_geometry_operations":2000000});
    save(&dir.join("scenario.json"), &scenario);
    let moving = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
    assert_eq!(
        moving,
        run(
            env!("CARGO_BIN_EXE_ftx"),
            &["--json", "materials", "scenario-study"]
        )
    );
    assert_eq!(
        moving,
        run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"])
    );
    assert_eq!(moving["manufacturing"], a["manufacturing"]);
    assert_eq!(moving["scenario"]["body_fit"], a["scenario"]["body_fit"]);
    assert_ne!(moving["request_sha256"], a["request_sha256"]);
    let motion = &moving["simulations"][0]["evaluation"]["result"]["motion"];
    let other = &moving["simulations"][1]["evaluation"]["result"]["motion"];
    assert_eq!(motion["status"], "available", "{motion}");
    assert_eq!(other["status"], "available", "{other}");
    let motion = &motion["result"];
    let other = &other["result"];
    assert_eq!(
        motion["path_sha256"],
        moving["scenario"]["motion_path"]["sha256"]
    );
    assert_eq!(
        motion["initial_positions_sha256"],
        first["result"]["positions_sha256"]
    );
    assert_eq!(motion["samples"], other["samples"]);
    assert_ne!(
        motion["keyframes"][1]["surface"]["result"]["steady"]["power_w"],
        other["keyframes"][1]["surface"]["result"]["steady"]["power_w"]
    );
    let samples = motion["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 4);
    assert_eq!(samples.last().unwrap()["time_s"], 0.004);
    assert_ne!(
        samples[0]["body_positions_m"],
        samples[3]["body_positions_m"]
    );
    assert_ne!(samples[3]["positions_m"], first["result"]["positions_m"]);
    assert_eq!(motion["physical_validation_claim"], false);
    for name in ["transient_surface", "transient_coverage"] {
        assert_eq!(motion[name]["status"], "available", "{}", motion[name]);
        assert_eq!(other[name]["status"], "available", "{}", other[name]);
        let h = &motion[name]["result"];
        assert_eq!(h["duration_s"], motion["duration_s"]);
        assert_eq!(h["path_sha256"], motion["path_sha256"]);
        assert_eq!(
            h["initial_positions_sha256"],
            motion["initial_positions_sha256"]
        );
        assert_eq!(h["trace"].as_array().unwrap().len(), samples.len());
        assert!(h["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
        assert_eq!(h["physical_validation_claim"], false);
    }
    assert_ne!(
        motion["transient_surface"]["result"]["final"]["temperature_c"],
        other["transient_surface"]["result"]["final"]["temperature_c"]
    );
    assert_ne!(
        motion["transient_coverage"]["result"]["checkpoint"]["exposed_temperature_c"],
        other["transient_coverage"]["result"]["checkpoint"]["exposed_temperature_c"]
    );
    // Verify final endpoint vertices against the direct source-model pose, not a guessed rig.
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, false).unwrap())
            .unwrap();
    let mut final_request = ftx_drape::study::fitted::BodyRequest::read(&body_request).unwrap();
    final_request.arm_abduction_deg = 35.2;
    let final_body =
        ftx_drape::study::fitted::FittedBody::prepare(&model, &sha256_hex(&bytes), &final_request)
            .unwrap();
    assert_eq!(
        samples[3]["body_positions_m"],
        json!(final_body.body().collision.0)
    );
    // Changed timing is a different scenario, despite having the same endpoint poses and shape.
    scenario["motion"]["keyframes"][0]["time_s"] = json!(0.004);
    scenario["motion"]["keyframes"][1]["time_s"] = json!(0.008);
    save(&dir.join("scenario.json"), &scenario);
    let slower = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
    assert_ne!(slower["request_sha256"], moving["request_sha256"]);
    assert_eq!(
        slower["scenario"]["body_fit"],
        moving["scenario"]["body_fit"]
    );
    let slow_motion = &slower["simulations"][0]["evaluation"]["result"]["motion"];
    assert_eq!(slow_motion["status"], "available", "{slow_motion}");
    assert_ne!(
        slow_motion["result"]["final_velocities_m_s"],
        motion["final_velocities_m_s"]
    );
    assert_ne!(
        slow_motion["result"]["transient_surface"]["result"]["final"]["temperature_c"],
        motion["transient_surface"]["result"]["final"]["temperature_c"]
    );
    // Failed continuation keeps the complete, unmodified initial static result and manufacturing.
    scenario["motion"]["max_sample_vertices"] = json!(1);
    save(&dir.join("scenario.json"), &scenario);
    let exhausted = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
    let failed_motion = &exhausted["simulations"][0]["evaluation"]["result"];
    assert_eq!(failed_motion["motion"]["status"], "refused");
    assert!(failed_motion["motion"]["result"].is_null());
    for key in ["drape", "surface", "positions_m"] {
        assert_eq!(failed_motion[key], first["result"][key]);
    }
    assert_eq!(exhausted["manufacturing"], a["manufacturing"]);
    // Keep the successful motion fixture available for independent native/WASM replay checks.
    scenario["motion"]["max_sample_vertices"] = json!(10000);
    save(&dir.join("scenario.json"), &scenario);
    save(&dir.join("motion-report.json"), &slower);
    for bad in [json!("REJECTED"), Value::Null] {
        body_request["accepted_body_fit"]["status"] = bad;
        save(&dir.join("body-input.json"), &body_request);
        let failed = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
        assert_eq!(failed["manufacturing"], a["manufacturing"]);
        let message = failed["scenario"]["body_setup_refusal"]["message"]
            .as_str()
            .unwrap();
        assert!(message.contains("not an accepted Anny shape"), "{message}");
        assert!(!message.contains("age_months") && !message.contains("local change"));
        assert!(
            failed["simulations"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["evaluation"]["status"] == "refused")
        );
    }
    // Leave a valid replay request alongside the synthetic files for independent decoder tests.
    body_request["accepted_body_fit"] = accepted.clone();
    save(&dir.join("body-input.json"), &body_request);
    assert_eq!(read(&dir.join("accepted.json")), accepted);
    assert_eq!(std::fs::read(dir.join("model.ftxbody")).unwrap(), bytes);

    // The short, loose-band run has not physically settled, even though its window means barely
    // drift. Retain it as a negative comparison before constructing a supported positive case.
    let mut compared_study = study.clone();
    compared_study["comparison"] = json!({"basis":"nominal", "objectives":[
        {"metric":"motion_max_absolute_edge_strain_pct","direction":"minimize"},
        {"metric":"transient_surface_inside_energy_j","direction":"minimize"},
        {"metric":"transient_surface_net_boundary_energy_j","direction":"minimize"},
        {"metric":"transient_coverage_net_boundary_energy_j","direction":"minimize"},
        {"metric":"transient_coverage_final_min_temperature_c","direction":"maximize"}],
        "required_consumers":["transient_surface","transient_coverage"]});
    save(&dir.join("study.json"), &compared_study);
    let unsettled = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
    assert!(
        unsettled["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for sim in unsettled["simulations"].as_array().unwrap() {
        assert_eq!(
            sim["evaluation"]["result"]["drape"]["numerical"]["settled"],
            false
        );
        assert_eq!(
            sim["evaluation"]["result"]["metrics"]["simulation_gating_clear"],
            false
        );
    }

    // A slightly stretched band grips the 100 mm radius synthetic cylinder. Give contact and
    // friction explicit physical time to settle; do not relax any numerical acceptance limit.
    let width = std::f64::consts::TAU * 98.0;
    pattern["pieces"][0]["vertices"]["b"][0] = json!(width);
    pattern["pieces"][0]["vertices"]["c"][0] = json!(width);
    pattern["pieces"][0]["placement"]["radius_mm"] = json!(110.0);
    pattern["pieces"][0]["placement"]["x0"] = json!(width / 2.0);
    scenario["options"] = json!({"spacing":30.0,"sew_steps":12,"drape_steps":96,"step":0.005});
    save(&dir.join("pattern.json"), &pattern);
    save(&dir.join("scenario.json"), &scenario);
    save(&dir.join("study.json"), &study);
    let settled = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
    for sim in settled["simulations"].as_array().unwrap() {
        assert_eq!(
            sim["evaluation"]["result"]["drape"]["numerical"]["settled"],
            true,
            "status {}, refusal {}, gates {}, final speed {}",
            sim["evaluation"]["status"],
            sim["evaluation"]["refusal"],
            sim["evaluation"]["result"]["drape"]["gating"],
            sim["evaluation"]["result"]["drape"]["numerical"]["drape"]["max_speed"]
        );
    }
    save(&dir.join("study.json"), &compared_study);
    let compared = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
    assert_eq!(
        compared,
        run(
            env!("CARGO_BIN_EXE_ftx"),
            &["--json", "materials", "scenario-study"]
        )
    );
    assert_eq!(compared["simulations"], settled["simulations"]);
    // The policy changes the canonical study identity, while every manufacturing outcome stays
    // unchanged as parsed JSON. Scenario-only comparison remains top-level.
    let compared_request: ftx_export::material_study::Request =
        serde_json::from_value(compared_study).unwrap();
    let mut expected_manufacturing = settled["manufacturing"].clone();
    expected_manufacturing["request_sha256"] =
        json!(sha256_hex(&serde_json::to_vec(&compared_request).unwrap()));
    assert_ne!(
        expected_manufacturing["request_sha256"],
        settled["manufacturing"]["request_sha256"]
    );
    assert_eq!(compared["manufacturing"], expected_manufacturing);
    assert_ne!(compared["request_sha256"], settled["request_sha256"]);
    assert_eq!(
        compared["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        !compared["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        compared["comparison"]["context_sha256"],
        compared["request_sha256"]
    );
    assert_eq!(compared["comparison"]["motion_required"], true);
    for (candidate, evaluated) in compared["comparison"]["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .zip(compared["simulations"].as_array().unwrap())
    {
        let result = &evaluated["evaluation"]["result"];
        assert_eq!(candidate["key"], evaluated["key"]);
        assert_eq!(result["drape"]["settled"], true);
        assert_eq!(result["metrics"]["simulation_gating_clear"], true);
        assert_eq!(candidate["eligible"], true, "{candidate}");
        assert!(candidate["exclusions"].as_array().unwrap().is_empty());
        let motion = &result["motion"]["result"];
        assert_eq!(
            candidate["readings"]["motion_max_absolute_edge_strain_pct"]["value"],
            motion["metrics"]["max_absolute_edge_strain_pct"]
        );
        assert_eq!(
            candidate["readings"]["transient_surface_inside_energy_j"]["value"],
            motion["transient_surface"]["result"]["inside_energy_j"]
        );
        for name in ["transient_surface", "transient_coverage"] {
            assert_eq!(
                candidate["readings"][format!("{name}_net_boundary_energy_j")]["value"],
                motion[name]["result"]["net_boundary_energy_j"]
            );
        }
        assert_eq!(
            candidate["readings"]["transient_coverage_final_min_temperature_c"]["value"],
            motion["transient_coverage"]["result"]["trace"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()["temperature_range_c"][0]
        );
    }
    save(&dir.join("motion-report.json"), &compared);

    // Opt-in export for the existing compiled-WASM parity check, using this exact accepted
    // synthetic model and positive production result. These are replay inputs, not an oracle.
    if let Some(destination) = std::env::var_os("FTX_MOTION_REPLAY") {
        let destination = PathBuf::from(destination);
        std::fs::create_dir_all(&destination).unwrap();
        for name in [
            "model.ftxbody",
            "pattern.json",
            "study.json",
            "scenario.json",
            "body-input.json",
            "motion-report.json",
        ] {
            std::fs::copy(dir.join(name), destination.join(name)).unwrap();
        }
    }

    // A named decision keeps each physical case intact and shares only manufacturing. The
    // first case is the already-qualified motion above; the other is an independent static
    // opposite-temperature boundary. Both binaries must reproduce each standalone case.
    let mut opposite = scenario.clone();
    opposite.as_object_mut().unwrap().remove("motion");
    opposite["surface"]["boundaries"][0]["inside_c"] = json!(10.0);
    opposite["surface"]["boundaries"][0]["outside_c"] = json!(30.0);
    let opposite_policy = json!({"basis":"nominal","objectives":[
        {"metric":"independent_surface_power_w","direction":"minimize"}]});
    let mut opposite_study = serde_json::to_value(&compared_request).unwrap();
    opposite_study["comparison"] = opposite_policy.clone();
    save(&dir.join("scenario.json"), &opposite);
    save(&dir.join("study.json"), &opposite_study);
    let independent_opposite = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
    let matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":4,"cases":[
        {"key":"declared-motion","scenario":scenario},
        {"key":"opposite-static-boundary","scenario":opposite,"comparison":opposite_policy}]});
    save(
        &dir.join("study.json"),
        &serde_json::to_value(&compared_request).unwrap(),
    );
    save(&dir.join("scenario.json"), &matrix);
    let combined = run(env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"]);
    assert_eq!(
        combined,
        run(
            env!("CARGO_BIN_EXE_ftx"),
            &["--json", "materials", "scenario-study"]
        )
    );
    assert_eq!(combined["schema"], "ftx-material-scenario-matrix-study/1");
    assert_eq!(combined["manufacturing"], compared["manufacturing"]);
    assert_eq!(combined["planned_evaluations"], 4);
    assert_eq!(combined["evaluations"], 4);
    for (index, independent) in [&compared, &independent_opposite].into_iter().enumerate() {
        for field in ["scenario", "request_sha256", "simulations", "comparison"] {
            assert_eq!(
                combined["cases"][index][field], independent[field],
                "case {index}: {field}"
            );
        }
    }
    assert!(
        combined["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["eligible"] == true)
    );
    assert!(
        !combined["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    save(&dir.join("scenario.json"), &scenario);

    // Retain a separate model-backed replay: changing only conversion of accepted plastic work
    // changes independent-sheet and covered/exposed heat, while every accepted trajectory stays identical.
    // The generated native output is an adapter parity input, not an empirical physics oracle.
    let plastic_dir = dir.join("plastic-heat");
    std::fs::create_dir(&plastic_dir).unwrap();
    std::fs::write(plastic_dir.join("model.ftxbody"), &bytes).unwrap();
    save(&plastic_dir.join("body-input.json"), &body_request);
    let mut unconverted: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-membrane-plasticity.json"
    ))
    .unwrap();
    unconverted["record"]["properties"]["specific_heat"]["method"] =
        json!("Synthetic constant storage with separately requested irreversible-yield heating");
    unconverted["record"]["properties"]["membrane_plastic_heat_fraction"] = json!({
        "value":0.,"unit":"1","source":"test","evidence":"estimate",
        "method":"Synthetic irreversible-yield heat conversion; not measured textile data",
        "conditions":{"specimen_id":"artificial-stretch-A","conditioning":"synthetic constant dry state",
            "work_basis":"irreversible_membrane_plastic_dissipation",
            "temperature_min_c":"-20","temperature_max_c":"80"}});
    // Low yield retains accepted irreversible work, while strong hardening keeps the sewn
    // band's permanent circumference below the supporting cylinder. Softening both instead
    // leaves a loose band in free fall; the ordinary gravity/quiet-step gate must still pass.
    // These synthetic coefficients do not describe a measured textile or real garment fit.
    for (kind, scale) in [("yield", 0.01), ("hardening", 1000.)] {
        for axis in ["warp", "weft", "shear"] {
            let value = &mut unconverted["record"]["properties"][format!("membrane_{kind}_{axis}")]
                ["value"];
            *value = json!(value.as_f64().unwrap() * scale);
        }
    }
    let mut converted = unconverted.clone();
    converted["record"]["properties"]["membrane_plastic_heat_fraction"]["value"] = json!(0.8);
    let mut plastic_pattern = pattern.clone();
    plastic_pattern["fabric"]["material"] = unconverted.clone();
    // Placement subtracts y0 from waist height: -600 mm seats the collar at z = 1.12 m,
    // in the articulated upper ring's contact region. The rigid waist cannot load it in motion.
    plastic_pattern["pieces"][0]["placement"]["y0"] = json!(-600.0);
    save(&plastic_dir.join("pattern.json"), &plastic_pattern);
    save(
        &plastic_dir.join("study.json"),
        &json!({
        "schema":"ftx-material-study/1","variants":[
            {"key":"unconverted","changes":{"schema":"ftx-material-bindings/1","default":unconverted}},
            {"key":"converted","changes":{"schema":"ftx-material-bindings/1","default":converted}}],
        "comparison":{"basis":"nominal","objectives":[
            {"metric":"transient_surface_final_max_temperature_c","direction":"minimize"},
            {"metric":"transient_coverage_final_max_temperature_c","direction":"minimize"}]}}),
    );
    let mut plastic_scenario = scenario.clone();
    plastic_scenario["motion"]["keyframes"] = json!([
        {"time_s":0.02,"arm_abduction_deg":35.5},
        {"time_s":0.04,"arm_abduction_deg":36.0}]);
    // The longer explicit path needs forty 1 ms steps; this is a work cap, not a solve tolerance.
    plastic_scenario["motion"]["max_trials"] = json!(64);
    let equilibrium = json!({"inside_c":20.,"outside_c":20.,
        "inside_resistance_m2_k_w":1.,"outside_resistance_m2_k_w":1.});
    plastic_scenario["motion"]["transient_surface"] = json!({
        "schema":"ftx-motion-surface-heat/1","initial_temperature_c":20.,"cells_per_layer":2,
        "boundaries":[equilibrium,equilibrium],"max_step_s":0.0005,"max_cell_steps":5000000,
        "plastic_dissipation_heat":"uniform_reference_thickness"});
    let covered = &mut plastic_scenario["motion"]["transient_coverage"];
    covered["initial_temperature_c"] = json!(20.);
    covered["boundaries"] = json!([equilibrium, equilibrium]);
    covered["exposed_boundaries"] = json!([null, null]);
    covered["max_cell_steps"] = json!(5000000);
    covered["plastic_dissipation_heat"] = json!("uniform_reference_thickness");
    save(&plastic_dir.join("scenario.json"), &plastic_scenario);
    let heated = run_in(
        &plastic_dir,
        env!("CARGO_BIN_EXE_ftx-materials"),
        &["scenario-study"],
    );
    assert_eq!(
        heated,
        run_in(
            &plastic_dir,
            env!("CARGO_BIN_EXE_ftx"),
            &["--json", "materials", "scenario-study"]
        )
    );
    assert_eq!(
        heated["comparison"]["frontier"],
        json!(["unconverted"]),
        "{}",
        heated["comparison"]["alternatives"]
    );
    assert_eq!(heated["comparison"]["incomparable"], json!([]));
    for candidate in heated["comparison"]["alternatives"].as_array().unwrap() {
        assert_eq!(candidate["eligible"], true, "{candidate}");
    }
    let simulations = heated["simulations"].as_array().unwrap();
    assert_eq!(simulations.len(), 2);
    for row in simulations {
        assert_eq!(row["evaluation"]["status"], "available", "{row}");
        let result = &row["evaluation"]["result"];
        assert_eq!(result["drape"]["settled"], true);
        assert_eq!(result["drape"]["numerical"]["settled"], true);
        assert_eq!(result["drape"]["gating"], json!([]));
        assert_eq!(result["metrics"]["simulation_gating_clear"], true);
        let motion = &result["motion"];
        assert_eq!(motion["status"], "available", "{motion}");
        let motion = &motion["result"];
        let heat = &motion["transient_surface"];
        assert_eq!(heat["status"], "available", "{heat}");
        let heat = &heat["result"];
        let work = &heat["plastic_dissipation_heat"];
        let coverage = &motion["transient_coverage"];
        assert_eq!(coverage["status"], "available", "{coverage}");
        let coverage = &coverage["result"];
        assert_eq!(coverage["plastic_dissipation_heat"], *work);
        assert_eq!(work["distribution"], "uniform_reference_thickness");
        assert_eq!(
            work["temporal_distribution"],
            "uniform_over_accepted_interval"
        );
        let initial = work["initial_plastic_dissipation_j"].as_f64().unwrap();
        let dissipated = work["plastic_dissipation_j"].as_f64().unwrap();
        let supplied = work["supplied_heat_j"].as_f64().unwrap();
        assert!(initial > 0. && dissipated > 0., "{work}");
        assert!(
            (initial
                - result["drape"]["membrane_plasticity_history"]["plastic_dissipation_j"]
                    .as_f64()
                    .unwrap())
            .abs()
                < 1e-12
        );
        assert!(
            (initial + dissipated
                - motion["membrane_plasticity_history"]["plastic_dissipation_j"]
                    .as_f64()
                    .unwrap())
            .abs()
                < 1e-12
        );
        let fraction = match row["key"].as_str() {
            Some("unconverted") => 0.,
            Some("converted") => 0.8,
            other => panic!("unexpected plastic heat alternative {other:?}"),
        };
        assert!((supplied - fraction * dissipated).abs() < 1e-12);
        assert_eq!(work["material_fractions"][0]["fraction"], fraction);
        assert!(
            (supplied + work["unconverted_dissipation_j"].as_f64().unwrap() - dissipated).abs()
                < 1e-12
        );
        let balance = heat["stored_change_j"].as_f64().unwrap()
            - heat["net_boundary_energy_j"].as_f64().unwrap()
            - supplied;
        assert!(balance.abs() < 1e-9, "{heat}");
        assert!((balance - heat["balance_error_j"].as_f64().unwrap()).abs() < 1e-12);
        let coverage_balance = coverage["physical_stored_change_j"].as_f64().unwrap()
            - coverage["net_boundary_energy_j"].as_f64().unwrap()
            - supplied;
        assert!(coverage_balance.abs() < 1e-9, "{coverage}");
        assert!((coverage_balance - coverage["balance_error_j"].as_f64().unwrap()).abs() < 1e-12);
        for sample in coverage["trace"].as_array().unwrap() {
            let supplied = sample["plastic_dissipation_heat"]["supplied_heat_j"]
                .as_f64()
                .unwrap();
            let boundary = sample["inside_energy_j"].as_f64().unwrap()
                - sample["outside_energy_j"].as_f64().unwrap();
            assert!((boundary - sample["net_boundary_energy_j"].as_f64().unwrap()).abs() < 1e-12);
            assert!(
                (sample["physical_stored_change_j"].as_f64().unwrap() - boundary - supplied).abs()
                    < 1e-9
            );
        }
        if fraction == 0. {
            assert_eq!(supplied, 0.);
            assert!(
                heat["final"]["temperature_c"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|row| row
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|t| (t.as_f64().unwrap() - 20.).abs() < 1e-10))
            );
        } else {
            assert!(supplied > 0.);
        }
    }
    let by_key: BTreeMap<_, _> = simulations
        .iter()
        .map(|row| (row["key"].as_str().unwrap(), &row["evaluation"]["result"]))
        .collect();
    let cold = by_key["unconverted"];
    let warm = by_key["converted"];
    assert_eq!(cold["positions_m"], warm["positions_m"]);
    assert_eq!(
        cold["motion"]["result"]["samples"],
        warm["motion"]["result"]["samples"]
    );
    assert_ne!(
        cold["motion"]["result"]["transient_surface"]["result"]["final"]["temperature_c"],
        warm["motion"]["result"]["transient_surface"]["result"]["final"]["temperature_c"]
    );
    assert_ne!(
        cold["motion"]["result"]["transient_coverage"]["result"]["checkpoint"]["exposed_temperature_c"],
        warm["motion"]["result"]["transient_coverage"]["result"]["checkpoint"]["exposed_temperature_c"]
    );
    save(&plastic_dir.join("motion-report.json"), &heated);
    eprintln!("plastic heat replay fixture: {}", plastic_dir.display());

    // Coverage alone must request the accepted mechanical work; it cannot depend on the
    // independent-sheet consumer having enabled capture as a side effect.
    let only_dir = plastic_dir.join("coverage-only");
    std::fs::create_dir(&only_dir).unwrap();
    std::fs::write(only_dir.join("model.ftxbody"), &bytes).unwrap();
    save(&only_dir.join("body-input.json"), &body_request);
    save(&only_dir.join("pattern.json"), &plastic_pattern);
    let mut only_scenario = plastic_scenario.clone();
    only_scenario["motion"]
        .as_object_mut()
        .unwrap()
        .remove("transient_surface");
    save(&only_dir.join("scenario.json"), &only_scenario);
    let mut only_study: Value =
        serde_json::from_slice(&std::fs::read(plastic_dir.join("study.json")).unwrap()).unwrap();
    only_study["comparison"]["objectives"] = json!([
        {"metric":"transient_coverage_final_max_temperature_c","direction":"minimize"}]);
    let mut unknown = plastic_pattern["fabric"]["material"].clone();
    unknown["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("membrane_plastic_heat_fraction");
    only_study["variants"]
        .as_array_mut()
        .unwrap()
        .push(json!({"key":"unqualified",
        "changes":{"schema":"ftx-material-bindings/1","default":unknown}}));
    save(&only_dir.join("study.json"), &only_study);
    let only = run_in(
        &only_dir,
        env!("CARGO_BIN_EXE_ftx-materials"),
        &["scenario-study"],
    );
    assert_eq!(only["comparison"]["frontier"], json!(["unconverted"]));
    for row in only["simulations"].as_array().unwrap() {
        let result = &row["evaluation"]["result"];
        assert_eq!(row["evaluation"]["status"], "available", "{row}");
        assert_eq!(result["motion"]["status"], "available", "{result}");
        let motion = &result["motion"]["result"];
        if row["key"] == "unqualified" {
            assert_eq!(
                motion["transient_coverage"]["status"], "refused",
                "{motion}"
            );
            assert!(
                motion["samples"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|s| s["solve"]["converged"] == true)
            );
        } else {
            let reference = &by_key[row["key"].as_str().unwrap()]["motion"]["result"];
            assert_eq!(motion["samples"], reference["samples"]);
            let heat = &motion["transient_coverage"];
            assert_eq!(heat["status"], "available", "{heat}");
            for key in [
                "plastic_dissipation_heat",
                "physical_stored_change_j",
                "net_boundary_energy_j",
                "balance_error_j",
                "checkpoint",
            ] {
                assert_eq!(
                    heat["result"][key],
                    reference["transient_coverage"]["result"][key]
                );
            }
        }
    }
}
