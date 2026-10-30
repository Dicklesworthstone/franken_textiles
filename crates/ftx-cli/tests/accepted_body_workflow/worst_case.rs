//! Actual sewn-body worst-case execution; no synthetic substitute for a simulation result.
use super::*;

pub(super) fn replay(
    model: &ftx_body::AnnyModel,
    digest: &str,
    [pattern, study, scenario, body]: [&Value; 4],
    dir: &Path,
    capture: Option<&Path>,
) {
    let mut study = study.clone();
    study["design_adaptive"]["objective"] = json!("consumer_worst_case");
    study["design_adaptive"]
        .as_object_mut()
        .unwrap()
        .remove("case_key");
    for limited in [false, true] {
        let label = if limited { "worst-budget" } else { "worst" };
        let mut matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":24,
            "cases":[{"key":"weight","scenario":scenario},
                {"key":"guard","scenario":scenario,"comparison":{"basis":"nominal",
                    "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
                    "constraints":[{"metric":"simulated_cloth_mass_kg","minimum":0.017}]}}]});
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
        let selected = search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["selected"] == true)
            .unwrap();
        let length = if limited { 0.0 } else { -20.0 };
        assert_eq!(selected["design"]["variables_mm"]["length"], length);
        assert_eq!(guidance["case_aggregation"], "worst_case");
        assert!(guidance.get("case_key").is_none());
        assert_eq!(guidance["best_case_key"], "weight");
        assert_eq!(selected["consumer_case_key"], "weight");
        assert_eq!(guidance["case_order"], json!(["weight", "guard"]));
        assert_eq!(guidance["reserved_evaluations"], 24);
        assert_eq!(search["selected_designs"], 1);
        assert_eq!(search["global_optimum_claim"], false);
        let expected = std::f64::consts::TAU * 180.0 * (100.0 + length) * 1e-6 * 0.2;
        let mass = guidance["best_reading"]["value"].as_f64().unwrap();
        assert!((mass - expected).abs() < 1e-6);
        let selected_key =
            serde_json::to_string(&json!([selected["design"]["key"], "light"])).unwrap();
        for (index, case) in report["cases"].as_array().unwrap().iter().enumerate() {
            let simulation = case["simulations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == selected_key)
                .unwrap();
            assert_eq!(simulation["evaluation"]["status"], "available");
            assert_eq!(simulation["evaluation"]["result"]["drape"]["settled"], true);
            assert_eq!(simulation["evaluation"]["result"]["drape"]["mass_kg"], mass);
            let candidate = case["comparison"]["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == selected_key)
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
        let failed = search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["design"]["variables_mm"]["length"] == if limited { -20.0 } else { -30.0 }
            })
            .unwrap();
        assert_eq!(failed["selected"], false);
        assert_eq!(failed["consumer_cases"][0]["comparison"]["eligible"], true);
        assert_eq!(failed["consumer_cases"][1]["comparison"]["eligible"], false);
        assert!(failed.get("consumer_case_key").is_none());
        if limited {
            assert_eq!(
                failed["consumer_cases"][1]["refusal"]["code"],
                "budget_exceeded"
            );
        }
        let summary = report["notes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .find(|note| note.starts_with("Worst-case retained trial"))
            .unwrap();
        assert!(summary.contains("limiting case 'weight'"));
        assert!(summary.contains(selected["design"]["key"].as_str().unwrap()));
        assert!(summary.contains("not an average"));
        let rows = report["manufacturing"]["alternatives"]
            .as_array()
            .unwrap()
            .len();
        let probes = guidance["evaluations"].as_u64().unwrap() as usize;
        assert_eq!(report["evaluations"], 2 * (probes + rows - 1));
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
