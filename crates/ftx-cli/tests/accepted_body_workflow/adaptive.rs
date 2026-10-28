//! Synthetic selected-body synthesis through both commands and the file-free browser authority.
use super::*;

#[test]
fn adaptive_linked_vertex_moves_reach_native_and_portable_selected_body_workflows() {
    linked_vertex_workflow(true);
}

pub(super) fn linked_vertex_workflow(adaptive: bool) {
    use ftx_wasm::materials::scenario::{manufacturing_on_model, on_model};

    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body_input = json!({"schema":"ftx-scenario-body/1",
        "accepted_body_fit":receipt(&digest),"arm_abduction_deg":50.0,
        "measurements":{"wearer":{"stature_mm":1111.0}}});
    let mut source: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    for (index, piece) in source["pieces"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        piece["placement"] = json!({"part":"torso","radius_mm":180.0,
            "x0":if index == 0 {10.0} else {-10.0}});
    }
    let pattern = source.to_string();
    let mechanical: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let mut study: Value =
        serde_json::from_str(include_str!("../../../../examples/design-study.json")).unwrap();
    study.as_object_mut().unwrap().remove("designs");
    study.as_object_mut().unwrap().remove("sewing");
    study["variants"].as_array_mut().unwrap().truncate(1);
    study["variants"][0]["changes"]["default"] = mechanical["fabric"]["material"].clone();
    // A single coordinate moves both front boundary vertices and drives the existing back
    // slash site. Both rectangles widen equally while their sewn sides remain 100 mm long.
    let sites = json!([
        {"op":"move","piece":"front","vertex":"b","dx_multiplier":1.0,"dy_multiplier":0.0},
        {"op":"move","piece":"front","vertex":"c","dx_multiplier":1.0,"dy_multiplier":0.0},
        {"piece":"back","axis":"x","at_mm":10.0,"multiplier":1.0}
    ]);
    let search_key = if adaptive {
        study["design_adaptive"] = json!({"objective":"fit_band_violation_mm",
            "variables":[{"key":"width","bounds_mm":[0.0,16.0],"initial_mm":0.0,
                "initial_step_mm":8.0,"minimum_step_mm":0.00001,"sites":sites}],
            "max_evaluations":96,"max_designs":1});
        "design_adaptive"
    } else {
        study["design_grid"] = json!({"variables":[{"key":"width",
            "values_mm":[0.0,3.0,6.0,9.0],"sites":sites}],
            "max_designs":1,"fit_screening":{"max_candidates":4}});
        "design_grid"
    };
    let ease = if adaptive { 3.7 } else { 0.0 };
    // The accepted synthetic shape supplies 1300 * 0.02 = 26 mm. The conflicting tape
    // value would imply 22.22 mm and cannot satisfy either independently chosen target.
    study["design_fit"] = json!({"fit_spec":{"schema":"fabric-fit/1",
        "seam_tol_mm":3.0,"truing_tol_deg":8.0,"points":[{
            "id":"synthetic_scaled_stature_width","kind":"width","instance":"front",
            "at_vertex":"a","dy":50.0,"body":"stature","body_scale":0.02,
            "target_mm":[ease-0.0001,ease,ease+0.0001]}]}});
    study["comparison"] = json!({"basis":"nominal",
        "objectives":[{"metric":"stock_mass_kg","direction":"minimize"}]});
    let scenario = json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":100.0,"sew_steps":3,"drape_steps":3,"step":0.001},
        "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":1000000});
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    std::fs::write(dir.join("pattern.json"), &pattern).unwrap();
    save(&dir.join("study.json"), &study);
    save(&dir.join("body-input.json"), &body_input);
    save(&dir.join("scenario.json"), &scenario);
    let check_native = |command: &str, expected: &Value| {
        let files = [
            "pattern.json",
            "study.json",
            "body-input.json",
            "model.ftxbody",
        ];
        let before: Vec<_> = files
            .iter()
            .map(|name| std::fs::read(dir.join(name)).unwrap())
            .collect();
        for (binary, prefix) in [
            (env!("CARGO_BIN_EXE_ftx"), &["--json", "materials"][..]),
            (env!("CARGO_BIN_EXE_ftx-materials"), &[][..]),
        ] {
            let mut args = vec![command, "pattern.json", "study.json"];
            if command == "scenario-study" {
                args.push("scenario.json");
            }
            args.extend(["body-input.json", "model.ftxbody"]);
            let output = Command::new(binary)
                .current_dir(&dir)
                .args(prefix)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(actual.get("result").unwrap_or(&actual), expected);
            for (name, original) in files.iter().zip(&before) {
                assert_eq!(&std::fs::read(dir.join(name)).unwrap(), original);
            }
        }
    };
    let manufacture = |request: &Value| -> Value {
        serde_json::from_str(
            &manufacturing_on_model(
                &model,
                &digest,
                &pattern,
                &request.to_string(),
                &body_input.to_string(),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let manufacturing = manufacture(&study);
    let search = &manufacturing["design_search"];
    let rows = if adaptive {
        assert_eq!(search["stop_reason"], "qualified_design_limit");
        assert_eq!(search["selected_designs"], 1);
        assert!(search["evaluations"].as_u64().unwrap() <= 96);
        &search["rows"]
    } else {
        assert_eq!(search["fit_screening"]["screened_designs"], 4);
        assert_eq!(search["fit_screening"]["rejected_designs"], 3);
        assert_eq!(search["fit_screening"]["selected_designs"], 1);
        &search["fit_screening"]["rows"]
    };
    let winner = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["selected"] == true)
        .unwrap();
    let selected = manufacturing["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["design"]["key"] == winner["design"]["key"])
        .unwrap();
    let design = &selected["design"];
    let coordinate = design["variables_mm"]["width"].as_f64().unwrap();
    assert!((coordinate - (6.0 + ease)).abs() <= 0.001);
    let operations = json!([
        {"op":"move","piece":"front","vertex":"b","dx_mm":coordinate,"dy_mm":0.0},
        {"op":"move","piece":"front","vertex":"c","dx_mm":coordinate,"dy_mm":0.0},
        {"op":"slash_spread","piece":"back","axis":"x","at_mm":10.0,"add_mm":coordinate}
    ]);
    assert_eq!(design["operations"], operations);
    assert_eq!(
        design["source_pattern_sha256"],
        sha256_hex(pattern.as_bytes())
    );
    let moved = ftx_pattern::alter::move_vertex(&source, "front", "b", coordinate, 0.0).unwrap();
    let moved = ftx_pattern::alter::move_vertex(&moved, "front", "c", coordinate, 0.0).unwrap();
    let replay = ftx_pattern::alter::slash_spread(&moved, "back", "x", 10.0, coordinate).unwrap();
    let frozen = design["altered_pattern_json"].as_str().unwrap();
    assert_eq!(frozen, replay.to_string());
    assert_eq!(
        design["altered_pattern_sha256"],
        sha256_hex(frozen.as_bytes())
    );
    // These coordinates are an independent geometric answer, not another fit-validator call.
    for piece in replay["pieces"].as_array().unwrap() {
        let vertices = &piece["vertices"];
        for (vertex, y) in [("a", 0.0), ("d", 100.0)] {
            assert_eq!(vertices[vertex][0].as_f64(), Some(0.0));
            assert_eq!(vertices[vertex][1].as_f64(), Some(y));
        }
        for (vertex, y) in [("b", 0.0), ("c", 100.0)] {
            assert!((vertices[vertex][0].as_f64().unwrap() - (26.0 + ease)).abs() < 1e-9);
            assert_eq!(vertices[vertex][1], y);
        }
    }
    let point = &design["fit"]["result"]["validation"]["points"][0];
    assert_eq!(point["body_mm"], 26.0);
    assert_eq!(point["status"], "IN_BAND");
    assert_eq!(selected["cutting"]["status"], "available");
    assert_eq!(
        manufacturing["comparison"]["frontier"],
        json!([selected["key"]])
    );
    let bound = selected["binding"]["result"]["pattern_json"]
        .as_str()
        .unwrap();
    let direct = ftx_export::cutting::CutPlan::build(
        &ftx_pattern::Pattern::from_json(bound).unwrap(),
        &serde_json::from_value(study["cutting"].clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(direct).unwrap(),
        selected["cutting"]["result"]
    );
    assert_eq!(manufacture(&study), manufacturing);
    check_native("study-on-body", &manufacturing);

    let simulated: Value = serde_json::from_str(
        &on_model(
            &model,
            &digest,
            &pattern,
            &study.to_string(),
            &scenario.to_string(),
            &body_input.to_string(),
            |_| {},
        )
        .unwrap(),
    )
    .unwrap();
    let mut expected_manufacturing = manufacturing.clone();
    expected_manufacturing
        .as_object_mut()
        .unwrap()
        .remove("comparison");
    assert_eq!(simulated["manufacturing"], expected_manufacturing);
    let simulation = simulated["simulations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == selected["key"])
        .unwrap();
    assert_eq!(
        simulation["evaluation"]["status"], "available",
        "{simulation}"
    );
    check_native("scenario-study", &simulated);

    // Refuse after the first Move has succeeded: no partially altered candidate may become
    // available, and the original pattern and accepted body must remain byte-for-byte intact.
    let mut missing = study.clone();
    missing[search_key]["variables"][0]["sites"][1]["vertex"] = json!("missing");
    let refused = manufacture(&missing);
    assert_eq!(refused["comparison"]["frontier"], json!([]));
    let refused_search = &refused["design_search"];
    let refused_rows = if adaptive {
        &refused_search["rows"]
    } else {
        &refused_search["fit_screening"]["rows"]
    };
    assert!(!refused_rows.as_array().unwrap().is_empty());
    for row in refused_rows.as_array().unwrap() {
        assert_eq!(row["selected"], false);
        assert!(row["design"]["altered_pattern_json"].is_null());
        assert!(row["design"]["altered_pattern_sha256"].is_null());
        assert!(
            row["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("no vertex 'missing'")
        );
    }
    save(&dir.join("study.json"), &missing);
    check_native("study-on-body", &refused);
    assert_eq!(
        std::fs::read_to_string(dir.join("pattern.json")).unwrap(),
        pattern
    );
    assert_eq!(source.to_string(), pattern);
}

#[test]
fn adaptive_fit_finds_an_intermediate_design_in_native_and_portable_scenarios() {
    use ftx_wasm::materials::scenario::on_model;

    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body_input = json!({"schema":"ftx-scenario-body/1",
        "accepted_body_fit":receipt(&digest),"arm_abduction_deg":50.0,
        "measurements":{"wearer":{"stature_mm":1111.0}}});
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    for (index, piece) in pattern["pieces"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        piece["placement"] = json!({"part":"torso","radius_mm":180.0,
            "x0":if index == 0 {10.0} else {-10.0}});
    }
    let pattern = pattern.to_string();
    let mechanical: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let mut study: Value =
        serde_json::from_str(include_str!("../../../../examples/design-study.json")).unwrap();
    study.as_object_mut().unwrap().remove("designs");
    study.as_object_mut().unwrap().remove("sewing");
    study["variants"].as_array_mut().unwrap().truncate(1);
    study["variants"][0]["changes"]["default"] = mechanical["fabric"]["material"].clone();
    study["design_adaptive"] = json!({"objective":"fit_band_violation_mm",
        "variables":[{"key":"length","bounds_mm":[0.0,60.0],"initial_mm":0.0,
            "initial_step_mm":20.0,"minimum_step_mm":0.00001,
            "sites":[{"piece":"front","axis":"y","at_mm":50.0,"multiplier":1.0},
                {"piece":"back","axis":"y","at_mm":50.0,"multiplier":1.0}]}],
        "max_evaluations":96,"max_designs":1});
    // Accepted shape, not the conflicting tape value, supplies the 130 mm scaled body length.
    // Coarse grid values 20, 30 and 40 cannot meet this declared numerical band.
    study["design_fit"] = json!({"fit_spec":{"schema":"fabric-fit/1",
        "seam_tol_mm":3.0,"truing_tol_deg":8.0,"points":[{
            "id":"synthetic_scaled_stature","kind":"vertical","instance":"front",
            "from":"a","to":"d","body":"stature","body_scale":0.1,
            "target_mm":[3.6999,3.7,3.7001]}]}});
    study["comparison"] = json!({"basis":"nominal",
        "objectives":[{"metric":"stock_mass_kg","direction":"minimize"}]});
    let single = json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":100.0,"sew_steps":3,"drape_steps":3,"step":0.001},
        "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":1000000});
    let matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":4,
        "cases":[{"key":"first","scenario":single},{"key":"second","scenario":single}]});
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    std::fs::write(dir.join("pattern.json"), &pattern).unwrap();
    save(&dir.join("study.json"), &study);
    save(&dir.join("body-input.json"), &body_input);
    let export = std::env::var_os("FTX_DESIGN_ADAPTIVE_REPLAY").map(PathBuf::from);
    if let Some(export) = &export {
        std::fs::create_dir_all(export).unwrap();
        std::fs::write(export.join("model.ftxbody"), &bytes).unwrap();
        std::fs::write(export.join("pattern.json"), &pattern).unwrap();
        save(&export.join("study.json"), &study);
        save(&export.join("body-input.json"), &body_input);
    }
    let mut selected_manufacturing = None;
    for (name, scenario) in [("single", single), ("matrix", matrix)] {
        let report: Value = serde_json::from_str(
            &on_model(
                &model,
                &digest,
                &pattern,
                &study.to_string(),
                &scenario.to_string(),
                &body_input.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap();
        let manufacturing = &report["manufacturing"];
        let search = &manufacturing["design_search"];
        assert_eq!(search["schema"], "ftx-design-adaptive-search/1");
        assert_eq!(search["stop_reason"], "qualified_design_limit", "{search}");
        assert_eq!(search["complete_search"], false);
        assert_eq!(search["global_optimum_claim"], false);
        assert!(search["evaluations"].as_u64().unwrap() <= 96);
        let winner = search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["selected"] == true)
            .unwrap();
        assert!(
            (winner["design"]["variables_mm"]["length"].as_f64().unwrap() - 33.7).abs() <= 0.001
        );
        assert_eq!(winner["max_fit_band_violation_mm"], 0.0);
        assert_eq!(winner["raw_fit"]["status"], "available");
        assert_eq!(
            winner["design"]["fit"]["result"]["validation"]["points"][0]["body_mm"],
            130.0
        );
        let selected = manufacturing["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["design"]["key"] == winner["design"]["key"])
            .unwrap();
        assert_eq!(selected["cutting"]["status"], "available");
        assert_eq!(report["comparison"]["frontier"], json!([selected["key"]]));
        let bound = selected["binding"]["result"]["pattern_json"]
            .as_str()
            .unwrap();
        let direct = ftx_export::cutting::CutPlan::build(
            &ftx_pattern::Pattern::from_json(bound).unwrap(),
            &serde_json::from_value(study["cutting"].clone()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(direct).unwrap(),
            selected["cutting"]["result"]
        );
        if let Some(previous) = &selected_manufacturing {
            assert_eq!(previous, manufacturing);
            assert_eq!(report["evaluations"], 2);
        } else {
            let simulation = report["simulations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|simulation| simulation["key"] == selected["key"])
                .unwrap();
            assert_eq!(
                simulation["evaluation"]["status"], "available",
                "{simulation}"
            );
            selected_manufacturing = Some(manufacturing.clone());
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
        if let Some(export) = &export {
            save(&export.join(format!("{name}-scenario.json")), &scenario);
            save(&export.join(format!("{name}-report.json")), &report);
        }
    }
}
