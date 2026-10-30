//! Multi-start discovery through the actual sewn-body API and both native commands.
//! Artificial body/material fixtures qualify software behavior, not physical garment fit.
use super::*;

pub(super) fn replay(
    model: &ftx_body::AnnyModel,
    digest: &str,
    [pattern, study, scenario, body]: [&Value; 4],
    dir: &Path,
    capture: Option<&Path>,
) {
    for (restart, limited, label) in [
        (false, false, "restart-single"),
        (true, false, "restart-complete"),
        (true, true, "restart-budget"),
    ] {
        let mut study = study.clone();
        let adaptive = &mut study["design_adaptive"];
        adaptive["objective"] = json!("consumer_worst_case");
        adaptive.as_object_mut().unwrap().remove("case_key");
        adaptive["variables"][0]["initial_mm"] = json!(-30.0);
        adaptive["variables"][0]["initial_step_mm"] = json!(5.0);
        adaptive["variables"][0]["minimum_step_mm"] = json!(5.0);
        if restart {
            adaptive["restart_points_mm"] = json!([{"length":0.0},{"length":-5.0}]);
        }
        let mut matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":24,
            "cases":[{"key":"weight","scenario":scenario},
                {"key":"guard","scenario":scenario,"comparison":{"basis":"nominal",
                    "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
                    "constraints":[{"metric":"simulated_cloth_mass_kg","minimum":0.020}]}}]});
        if limited {
            matrix["cases"][1]["scenario"]["max_vertex_frames"] = json!(25000);
        }
        let report: Value = serde_json::from_str(
            &ftx_wasm::materials::scenario::on_model(
                model,
                digest,
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
        assert_eq!(guidance["case_aggregation"], "worst_case");
        assert_eq!(guidance["reserved_evaluations"], 24);
        assert_eq!(report["planned_evaluations"], 24);
        assert_eq!(search["complete_search"], false);
        assert_eq!(search["global_optimum_claim"], false);
        let trials = search["rows"].as_array().unwrap();
        // The original local search cannot cross these two simulation-evaluated but excluded
        // trials: both pass flat fit, and neither satisfies the declared production guard.
        for (index, coordinate) in [-30.0, -25.0].into_iter().enumerate() {
            assert_eq!(
                trials[index]["design"]["variables_mm"]["length"],
                coordinate
            );
            assert_eq!(trials[index]["max_fit_band_violation_mm"], 0.0);
            assert_eq!(trials[index]["selected"], false);
            assert_eq!(
                trials[index]["consumer_cases"][1]["comparison"]["eligible"],
                false
            );
        }
        if restart {
            let starts = search["search_starts"].as_array().unwrap();
            assert_eq!(starts.len(), 3);
            assert_eq!(starts[0]["evaluations"], 2);
            assert_eq!(starts[0]["stop_reason"], "minimum_steps");
            assert_eq!(starts[1]["start_index"], 1);
            assert_eq!(starts[1]["initial_trial_key"], trials[2]["design"]["key"]);
            assert_eq!(trials[2]["design"]["variables_mm"]["length"], 0.0);
            assert_eq!(
                starts
                    .iter()
                    .map(|s| s["evaluations"].as_u64().unwrap())
                    .sum::<u64>(),
                search["evaluations"].as_u64().unwrap()
            );
        } else {
            assert_eq!(search["evaluations"], 2);
            assert!(search.get("search_starts").is_none());
        }
        if restart && !limited {
            let selected = trials.iter().find(|row| row["selected"] == true).unwrap();
            assert_eq!(selected["design"]["variables_mm"]["length"], -5.0);
            assert_eq!(search["selected_designs"], 1);
            assert_eq!(search["evaluations"], 5);
            assert_eq!(search["search_starts"][2]["evaluations"], 0);
            assert_eq!(
                search["search_starts"][2]["initial_trial_key"],
                selected["design"]["key"]
            );
            // The 90 mm candidate meets the mass minimum but its actual drape is not admitted.
            // Preserve that negative instead of weakening settling or taking nominal mass alone.
            assert_eq!(trials[4]["design"]["variables_mm"]["length"], -10.0);
            assert_eq!(
                trials[4]["consumer_cases"][0]["comparison"]["eligible"],
                false
            );
            assert!(
                trials[4]["refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("geometrically settled")
            );
            let expected = std::f64::consts::TAU * 180.0 * 95.0 * 1e-6 * 0.2;
            assert!((guidance["best_reading"]["value"].as_f64().unwrap() - expected).abs() < 1e-6);
            let selected_key =
                serde_json::to_string(&json!([selected["design"]["key"], "light"])).unwrap();
            let manufacture = report["manufacturing"]["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == selected_key)
                .unwrap();
            let altered = ftx_pattern::Pattern::from_json(
                manufacture["design"]["altered_pattern_json"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            let changes: ftx_pattern::material_binding::Changes =
                serde_json::from_value(manufacture["frozen_changes"].clone()).unwrap();
            assert_eq!(
                changes.apply(&altered).unwrap()["pattern"],
                manufacture["binding"]["result"]["pattern"]
            );
            for (index, case) in report["cases"].as_array().unwrap().iter().enumerate() {
                let simulation = case["simulations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["key"] == selected_key)
                    .unwrap();
                assert_eq!(simulation["evaluation"]["status"], "available");
                assert_eq!(simulation["evaluation"]["result"]["drape"]["settled"], true);
                assert_eq!(
                    simulation["evaluation"]["result"]["pattern_sha256"],
                    manufacture["binding"]["result"]["pattern_sha256"]
                );
                assert_eq!(
                    selected["consumer_cases"][index]["comparison"]["readings"]["simulated_cloth_mass_kg"]
                        ["value"],
                    simulation["evaluation"]["result"]["drape"]["mass_kg"]
                );
            }
        } else {
            assert_eq!(search["selected_designs"], 0);
            assert!(guidance["best_reading"].is_null());
            if limited {
                assert_eq!(
                    trials[2]["consumer_cases"][1]["refusal"]["code"],
                    "budget_exceeded"
                );
                assert!(trials.iter().skip(2).all(|row| row["selected"] == false));
            }
        }
        let final_rows = report["manufacturing"]["alternatives"]
            .as_array()
            .unwrap()
            .len();
        let probes = guidance["evaluations"].as_u64().unwrap() as usize;
        let cached = usize::from(restart && !limited);
        assert_eq!(report["evaluations"], 2 * (probes + final_rows - cached));
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
                .current_dir(dir)
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
        if let Some(capture) = capture {
            save(&capture.join(study_file), &study);
            save(&capture.join(scenario_file), &matrix);
            save(&capture.join(format!("{label}-report.json")), &report);
        }
    }
}
