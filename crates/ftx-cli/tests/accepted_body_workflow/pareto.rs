//! Real mass/steady-heat tradeoffs retain multiple designs without a fabricated scalar winner.
use super::*;

pub(super) fn replay(
    model: &ftx_body::AnnyModel,
    digest: &str,
    [pattern, study, scenario, body]: [&Value; 4],
    dir: &Path,
    capture: Option<&Path>,
) {
    let mut study = study.clone();
    study["design_adaptive"]["objective"] = json!("consumer_pareto");
    study["design_adaptive"]["case_key"] = json!("cooling");
    study["design_adaptive"]["max_designs"] = json!(2);
    study["variants"].as_array_mut().unwrap().truncate(1);
    study["assembly_mass"] = json!({"schema":"ftx-assembly-mass/1",
        "inventory_complete":true,"seam_allowance_mm":0,"extras":[]});
    study["comparison"] = json!({"basis":"nominal","required_consumers":["drape"],
        "objectives":[{"metric":"assembly_mass_kg","direction":"minimize"},
            {"metric":"independent_surface_power_w","direction":"maximize"}]});
    let mut scenario = scenario.clone();
    scenario["surface"] = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/steady-surface-conditions.json"
    ))
    .unwrap();
    for (label, limited) in [("pareto", false), ("pareto-budget", true)] {
        let mut scenario = scenario.clone();
        if limited {
            scenario["max_vertex_frames"] = json!(25000);
        }
        let matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":11,
            "cases":[{"key":"cooling","scenario":scenario}]});
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
        let frontier = &guidance["pareto_frontier"];
        let retained = frontier["retained_trial_keys"].as_array().unwrap();
        let evaluated = frontier["evaluated_trial_keys"].as_array().unwrap();
        let unretained = frontier["unretained_trial_keys"].as_array().unwrap();
        let trials = search["rows"].as_array().unwrap();
        assert_eq!(guidance["objectives"], study["comparison"]["objectives"]);
        assert!(guidance.get("objective").is_none());
        assert!(guidance["best_trial_key"].is_null());
        assert!(guidance["best_reading"].is_null());
        assert_eq!(guidance["case_key"], "cooling");
        assert_eq!(guidance["reserved_evaluations"], 11);
        assert_eq!(frontier["retention_capacity"], 2);
        assert_eq!(retained.len(), if limited { 1 } else { 2 });
        assert_eq!(search["selected_designs"], retained.len());
        assert_eq!(search["global_optimum_claim"], false);
        assert_eq!(search["complete_search"], false);
        assert_eq!(evaluated.len(), retained.len() + unretained.len());
        for key in evaluated {
            let trial = trials
                .iter()
                .find(|row| row["design"]["key"] == *key)
                .unwrap();
            assert_eq!(trial["consumer_comparison"]["eligible"], true);
            assert_eq!(trial["selected"], retained.contains(key));
            assert_eq!(
                trial["consumer_comparison"],
                trial["consumer_cases"][0]["comparison"]
            );
            let length = trial["design"]["variables_mm"]["length"].as_f64().unwrap();
            let expected_mass = std::f64::consts::TAU * 180.0 * (100.0 + length) * 1e-6 * 0.2;
            assert!(
                (trial["consumer_comparison"]["readings"]["assembly_mass_kg"]["value"]
                    .as_f64()
                    .unwrap()
                    - expected_mass)
                    .abs()
                    < 1e-6
            );
        }
        if limited {
            assert!(unretained.is_empty());
            assert_eq!(evaluated.len(), 1);
            assert!(
                trials
                    .iter()
                    .any(|row| row["consumer_refusal"]["code"] == "budget_exceeded"
                        && row["consumer_comparison"]["eligible"] == false)
            );
        } else {
            // Polling continues from incomparable centers after both output slots fill. The
            // shorter candidate has lower real mass and lower real heat flow, so neither wins.
            assert!(!unretained.is_empty());
            let shortest = trials
                .iter()
                .find(|row| row["design"]["variables_mm"]["length"] == -30.0)
                .unwrap();
            assert!(unretained.contains(&shortest["design"]["key"]));
            let first = &trials[0]["consumer_comparison"]["readings"];
            let last = &shortest["consumer_comparison"]["readings"];
            for metric in ["assembly_mass_kg", "independent_surface_power_w"] {
                assert!(
                    last[metric]["value"].as_f64().unwrap()
                        < first[metric]["value"].as_f64().unwrap()
                );
            }
        }
        let final_rows = report["manufacturing"]["alternatives"].as_array().unwrap();
        for key in retained {
            let trial = trials
                .iter()
                .find(|row| row["design"]["key"] == *key)
                .unwrap();
            let combined = serde_json::to_string(&json!([key, "light"])).unwrap();
            let row = final_rows
                .iter()
                .find(|row| row["key"] == combined)
                .unwrap();
            let case = &report["cases"][0];
            let simulation = case["simulations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == combined)
                .unwrap();
            let result = &simulation["evaluation"]["result"];
            assert_eq!(simulation["evaluation"]["status"], "available");
            assert_eq!(result["drape"]["settled"], true);
            assert_eq!(result["surface"]["status"], "available");
            assert_eq!(
                result["pattern_sha256"],
                row["binding"]["result"]["pattern_sha256"]
            );
            let candidate = case["comparison"]["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == combined)
                .unwrap();
            for field in [
                "readings",
                "eligible",
                "exclusions",
                "input_protocol_sha256",
            ] {
                assert_eq!(candidate[field], trial["consumer_comparison"][field]);
            }
            let heat = &result["surface"]["result"]["steady"];
            let watts = candidate["readings"]["independent_surface_power_w"]["value"]
                .as_f64()
                .unwrap();
            let expected_watts =
                heat["area_m2"].as_f64().unwrap() * 30.0 / (0.1 + 0.2 + 0.0004 / 0.05);
            assert!((watts - expected_watts).abs() < 1e-10);
            assert_eq!(heat["power_w"], watts);
        }
        // Only baseline needs a new callback; every retained discovery reuses its exact result.
        assert_eq!(
            report["evaluations"],
            guidance["evaluations"].as_u64().unwrap() as usize + final_rows.len() - retained.len()
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
                "{} / {}",
                String::from_utf8_lossy(&output.stdout),
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
