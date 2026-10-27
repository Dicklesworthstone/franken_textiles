//! Actual native commands and portable adapters consume one complete production plan and quantity.
//! All source fixtures are synthetic; these checks establish software integration only.

use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const OPTIONS: &str = include_str!("../../../examples/sewing-production-options.json");
const TIMED_SELECTION: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-timed-selection.json");
static NEXT: AtomicU64 = AtomicU64::new(0);

fn workspace() -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "ftx-sewing-production-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    directory
}

fn run(main: bool, directory: &Path, action: &str, pattern: &str, request: &str) -> Value {
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(directory);
    if main {
        command.args(["--json", "materials"]);
    }
    let output = command.args([action, pattern, request]).output().unwrap();
    assert!(
        output.status.success(),
        "{main}/{action}: {} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    report.get("result").unwrap_or(&report).clone()
}

fn portable(text: String) -> Value {
    serde_json::from_str(&text).unwrap()
}

#[test]
fn both_native_plan_and_stitch_commands_execute_the_documented_batch_and_match_portable() {
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    fs::write(directory.join("options.json"), OPTIONS).unwrap();
    let plan = portable(ftx_wasm::manufacturing::sew_plan(PATTERN, OPTIONS).unwrap());
    let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, OPTIONS).unwrap());
    assert_eq!(plan["schema"], "ftx-sewing-plan/12");
    assert_eq!(plan["production"]["production_copies"], 3);
    assert_eq!(plan["production"]["stitch_intervals"], 240);
    assert_eq!(plan["production"]["feed_seconds"], 24.0);
    assert_eq!(
        plan["production"]["ordering"]["timing"]["feed_and_setup_seconds"],
        25.5
    );
    assert_eq!(program["schema"], "ftx-sewing-program/4");
    assert_eq!(program["stitches"].as_array().unwrap().len(), 240);
    assert_eq!(program["plan"], plan);
    for main in [false, true] {
        assert_eq!(
            run(main, &directory, "sew-plan", "pattern.json", "options.json"),
            plan
        );
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "options.json"
            ),
            program
        );
    }
    assert_eq!(
        fs::read_to_string(directory.join("pattern.json")).unwrap(),
        PATTERN
    );
    assert_eq!(
        fs::read_to_string(directory.join("options.json")).unwrap(),
        OPTIONS
    );
}

#[test]
fn cutting_quantity_reaches_batch_studies_and_exact_standalone_native_replay() {
    let directory = workspace();
    let mut pattern: Value = serde_json::from_str(PATTERN).unwrap();
    pattern["fabric"]["material"]["record"]["properties"]["width"] = json!({
        "value":0.2,"unit":"m","evidence":"estimate","source":"test",
        "method":"Synthetic stock width for complete production integration"
    });
    let pattern_text = pattern.to_string();
    fs::write(directory.join("pattern.json"), &pattern_text).unwrap();
    let mut options: Value = serde_json::from_str(OPTIONS).unwrap();
    options["production"]
        .as_object_mut()
        .unwrap()
        .remove("production_copies");
    let mut study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"original","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern["fabric"]["material"]}}],
        "cutting":{"copies":3,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0},
        "sewing":options,
        "comparison":{"basis":"nominal","required_consumers":["cutting","sewing"],
            "objectives":[{"metric":"sewing_feed_and_setup_seconds","direction":"minimize"}]}});
    let expected = portable(ftx_wasm::materials::study(&pattern_text, &study.to_string()).unwrap());
    let row = &expected["alternatives"][0];
    assert_eq!(row["cutting"]["status"], "available");
    assert_eq!(row["sewing"]["status"], "available");
    let plan = &row["sewing"]["result"];
    assert_eq!(plan["options"]["production"]["production_copies"], 3);
    assert_eq!(row["metrics"]["stitch_intervals"], 240);
    assert_eq!(row["metrics"]["feed_seconds"], 24.0);
    assert_eq!(row["metrics"]["sewing_setup_seconds"], 1.5);
    assert_eq!(row["metrics"]["sewing_feed_and_setup_seconds"], 25.5);
    assert_eq!(expected["comparison"]["frontier"], json!(["original"]));
    fs::write(directory.join("study.json"), study.to_string()).unwrap();
    fs::write(
        directory.join("bound.json"),
        row["binding"]["result"]["pattern_json"].as_str().unwrap(),
    )
    .unwrap();
    fs::write(directory.join("resolved.json"), plan["options"].to_string()).unwrap();
    for main in [false, true] {
        assert_eq!(
            run(main, &directory, "study", "pattern.json", "study.json"),
            expected
        );
        assert_eq!(
            run(main, &directory, "sew-plan", "bound.json", "resolved.json"),
            *plan
        );
    }

    // A contradictory authored schedule withdraws sewing while preserving independent stock planning.
    study["sewing"]["production"]["production_copies"] = json!(2);
    fs::write(directory.join("mismatch.json"), study.to_string()).unwrap();
    let refused = portable(ftx_wasm::materials::study(&pattern_text, &study.to_string()).unwrap());
    assert_eq!(refused["alternatives"][0]["cutting"], row["cutting"]);
    assert_eq!(refused["alternatives"][0]["sewing"]["status"], "refused");
    assert!(refused["alternatives"][0]["metrics"]["feed_seconds"].is_null());
    assert_eq!(refused["comparison"]["frontier"], json!([]));
    for main in [false, true] {
        assert_eq!(
            run(main, &directory, "study", "pattern.json", "mismatch.json"),
            refused
        );
    }
    assert_eq!(
        fs::read_to_string(directory.join("pattern.json")).unwrap(),
        pattern_text
    );
}

#[test]
fn complete_batch_timing_changes_recipe_choice_and_both_commands_replay_the_selected_plan() {
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    let single = portable(ftx_wasm::sew_select(PATTERN, TIMED_SELECTION).unwrap());
    assert_eq!(single["report"]["selected_objective_seconds"], 8.0);
    assert_eq!(
        single["report"]["selected_choices"],
        json!({"join":"template","finish":"template"})
    );
    let mut request: Value = serde_json::from_str(TIMED_SELECTION).unwrap();
    let ordering = request["options"]
        .as_object_mut()
        .unwrap()
        .remove("ordering")
        .unwrap();
    request["options"]["production"] = json!({"production_copies":3,"ordering":ordering});
    let text = request.to_string();
    fs::write(directory.join("selection.json"), &text).unwrap();
    let selected = portable(ftx_wasm::sew_select(PATTERN, &text).unwrap());
    assert_eq!(selected["report"]["selected_objective_seconds"], 22.0);
    assert_eq!(
        selected["report"]["selected_choices"],
        json!({"join":"fast","finish":"fast"})
    );
    assert_eq!(selected["report"]["optimality_proven"], true);
    assert_eq!(
        selected["report"]["trials"]
            .as_array()
            .unwrap()
            .iter()
            .map(|trial| trial["objective_seconds"].as_f64().unwrap())
            .collect::<Vec<_>>(),
        [24.0, 35.0, 28.0, 22.0]
    );
    fs::write(
        directory.join("selected.json"),
        selected["selected_options_json"].as_str().unwrap(),
    )
    .unwrap();
    let program = portable(
        ftx_wasm::manufacturing::sew_stitches(
            PATTERN,
            selected["selected_options_json"].as_str().unwrap(),
        )
        .unwrap(),
    );
    assert_eq!(program["plan"], selected["report"]["selected_plan"]);
    assert_eq!(program["plan"]["feed_seconds"], 4.0);
    assert_eq!(program["plan"]["production"]["feed_seconds"], 12.0);
    assert_eq!(program["stitches"].as_array().unwrap().len(), 150);
    for main in [false, true] {
        assert_eq!(
            run(
                main,
                &directory,
                "sew-select",
                "pattern.json",
                "selection.json"
            ),
            selected
        );
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "selected.json"
            ),
            program
        );
    }
    assert_eq!(
        fs::read_to_string(directory.join("selection.json")).unwrap(),
        text
    );
}
