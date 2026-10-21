//! Actual partial-seam CLI plans, stitch travel and previews. All machine/material data are synthetic.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

use ftx_export::sewing::{SewOptions, SewPlan, preview, program::SewProgram};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const OPTIONS: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-span-options.json");

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ftx-sewing-span-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("pattern.json"), PATTERN).unwrap();
        fs::write(path.join("options.json"), OPTIONS).unwrap();
        Self(path)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

#[test]
fn real_commands_preserve_complete_source_coverage_and_distinct_directed_operations() {
    let workspace = Workspace::new();
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let options: SewOptions = serde_json::from_str(OPTIONS).unwrap();
    let plan = workspace.json(&["sew-plan", "pattern.json", "options.json"]);
    let program = workspace.json(&["sew-stitches", "pattern.json", "options.json"]);
    assert_eq!(
        plan,
        serde_json::to_value(SewPlan::build(&pattern, &options).unwrap()).unwrap()
    );
    assert_eq!(
        program,
        serde_json::to_value(SewProgram::build(&pattern, &options).unwrap()).unwrap()
    );
    assert_eq!(program["plan"], plan);
    assert_eq!(plan["schema"], "ftx-sewing-plan/10");
    assert_eq!(plan["stitch_intervals"], 40);
    assert_eq!(plan["feed_seconds"], 4.0);
    assert_eq!(plan["hardware_execution_authorized"], false);
    assert_eq!(plan["physical_seam_quality_validated"], false);
    for (index, id, range, length, count) in [
        (0, "lower-side", [0.0, 0.6], 60.0, 24),
        (1, "upper-side", [0.6, 1.0], 40.0, 16),
    ] {
        let operation = &plan["seams"][index];
        assert_eq!(operation["pass"]["id"], id);
        assert_eq!(
            operation["source_span"]["source_ranges"],
            json!([range, range])
        );
        assert_eq!(
            operation["source_span"]["source_lengths_mm"],
            json!([100.0, 100.0])
        );
        assert_eq!(
            operation["source_span"]["sewn_lengths_mm"],
            json!([length, length])
        );
        assert_eq!(operation["stitch_intervals"], count);
        let file = format!("{id}.svg");
        workspace.json(&["sew-svg", "pattern.json", "options.json", id, &file]);
        assert_eq!(
            fs::read_to_string(workspace.0.join(file)).unwrap(),
            preview::svg(&pattern, &options, id).unwrap()
        );
    }
    let stitches = program["stitches"].as_array().unwrap();
    assert_eq!(stitches.len(), 40);
    assert_eq!(
        stitches
            .iter()
            .filter(|step| step["begins_run"] == true)
            .count(),
        2
    );
    assert_eq!(stitches[23]["operation"], 0);
    assert_eq!(stitches[24]["operation"], 1);
    assert_eq!(stitches[24]["from_station"], 16);
    assert_eq!(stitches[39]["to_station"], 0);
    for (index, step) in stitches.iter().enumerate() {
        for side in 0..2 {
            let advance = step["advance_mm"][side].as_f64().unwrap();
            assert_eq!(advance.is_sign_positive(), index < 24);
            assert!((advance.abs() - 2.5).abs() < 1e-10);
        }
    }
    assert_eq!(
        fs::read_to_string(workspace.0.join("pattern.json")).unwrap(),
        PATTERN
    );
    assert_eq!(
        fs::read_to_string(workspace.0.join("options.json")).unwrap(),
        OPTIONS
    );
}

#[test]
fn every_cli_consumer_refuses_incomplete_or_invalid_spans_before_writing_outputs() {
    for (range, message) in [
        ([0.0, 0.55], "coverage gap"),
        ([0.7, 0.6], "source_range"),
        ([-0.1, 0.6], "source_range"),
    ] {
        let workspace = Workspace::new();
        let mut options: Value = serde_json::from_str(OPTIONS).unwrap();
        options["passes"][0]["source_range"] = json!(range);
        fs::write(workspace.0.join("invalid.json"), options.to_string()).unwrap();
        for args in [
            vec!["sew-plan", "pattern.json", "invalid.json"],
            vec!["sew-stitches", "pattern.json", "invalid.json"],
            vec![
                "sew-svg",
                "pattern.json",
                "invalid.json",
                "upper-side",
                "refused.svg",
            ],
        ] {
            let output = workspace.run(&args);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            let refusal: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert!(
                refusal["refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(message)
            );
            assert!(!workspace.0.join("refused.svg").exists());
        }
    }
}
