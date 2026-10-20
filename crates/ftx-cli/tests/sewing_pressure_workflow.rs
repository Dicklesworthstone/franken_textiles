//! Actual command-line sewing admission; every input is synthetic, never a machine recommendation.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
use ftx_pattern::{Pattern, material_catalog::Selection, textile_import::catalog};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const OPTIONS: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pressure-options.json");
static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ftx-pressure-sewing-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("pattern.json"), PATTERN).unwrap();
        fs::write(path.join("options.json"), OPTIONS).unwrap();
        Self(path)
    }
    fn write(&self, file: &str, value: &Value) {
        fs::write(self.0.join(file), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
    fn json(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
const PLAN: &[&str] = &["sew-plan", "pattern.json", "options.json"];

#[test]
fn real_commands_retain_loaded_and_unloaded_stacks_and_the_same_source_identity() {
    let w = Workspace::new();
    let plan = w.json(PLAN);
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let options: SewOptions = serde_json::from_str(OPTIONS).unwrap();
    assert_eq!(
        plan,
        serde_json::to_value(SewPlan::build(&pattern, &options).unwrap()).unwrap()
    );
    assert_eq!(plan, w.json(PLAN));
    let seam = &plan["seams"][0];
    assert_eq!(plan["schema"], "ftx-sewing-plan/7");
    assert_eq!(seam["stack_mm"], 0.8);
    assert_eq!(seam["presser_foot"]["nominal_stack_mm"], 0.4);
    assert_eq!(seam["presser_foot"]["stack_mm"], 0.44);
    assert_eq!(seam["qualification_id"], "synthetic-pressure-process");
    assert_eq!(plan["hardware_execution_authorized"], false);
    assert_eq!(plan["physical_seam_quality_validated"], false);
    let program = w.json(&["sew-stitches", "pattern.json", "options.json"]);
    assert_eq!(program["plan"], plan);
    assert_eq!(
        program,
        serde_json::to_value(SewProgram::build(&pattern, &options).unwrap()).unwrap()
    );
    assert_eq!(program["stitches"].as_array().unwrap().len(), 40);
    assert_eq!(
        fs::read_to_string(w.0.join("pattern.json")).unwrap(),
        PATTERN
    );
    assert_eq!(
        fs::read_to_string(w.0.join("options.json")).unwrap(),
        OPTIONS
    );
}

#[test]
fn material_only_compression_swap_changes_feasibility_without_changing_stitch_geometry() {
    let w = Workspace::new();
    let original = w.json(PLAN);
    let mut p: Value = serde_json::from_str(PATTERN).unwrap();
    let mut o: Value = serde_json::from_str(OPTIONS).unwrap();
    p["fabric"]["material"]["id"] = json!("synthetic-less-compressible-sheet");
    p["fabric"]["material"]["record"]["id"] = p["fabric"]["material"]["id"].clone();
    p["fabric"]["material"]["record"]["name"] = json!("Synthetic less compressible construction");
    let selection: Selection = serde_json::from_value(p["fabric"]["material"].clone()).unwrap();
    let selected = catalog().unwrap().resolve(&selection).unwrap();
    let mut input = o["passes"][0]["presser_foot"]["materials"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    input["material"] = serde_json::to_value(selected.authored()).unwrap();
    input["specimen"] = json!(selected.authored().id);
    for sample in input["samples"].as_array_mut().unwrap() {
        sample["properties"]["thickness"]["conditions"]["specimen"] = json!(selected.authored().id);
    }
    // Same 0.4-mm unloaded thickness; changed material has 0.35 mm under the same 500 Pa.
    input["samples"][1]["properties"]["thickness"]["value"] = json!(0.35);
    input["samples"][1]["properties"]["thickness"]["bounds"] = json!([0.32, 0.38]);
    o["passes"][0]["presser_foot"]["materials"] = json!({selected.sha256():input});
    // New specimen needs its own process evidence; this comparison is explicitly unqualified.
    o["require_qualification"] = json!(false);
    o["qualifications"] = json!([]);
    w.write("pattern.json", &p);
    w.write("options.json", &o);
    let refused = w.run(PLAN);
    assert_eq!(refused.status.code(), Some(2));
    assert!(refused.stdout.is_empty());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("loaded machine limit"));
    o["machine"]["presser_foot"]["max_loaded_stack_mm"] = json!(0.8);
    w.write("options.json", &o);
    let admitted = w.json(PLAN);
    assert_eq!(
        admitted["seams"][0]["stations"],
        original["seams"][0]["stations"]
    );
    assert_eq!(
        admitted["seams"][0]["stack_mm"],
        original["seams"][0]["stack_mm"]
    );
    assert_eq!(
        admitted["seams"][0]["presser_foot"]["nominal_stack_mm"],
        0.7
    );
    assert_eq!(admitted["seams"][0]["presser_foot"]["stack_mm"], 0.76);
    assert_ne!(admitted["pattern_sha256"], original["pattern_sha256"]);
}

#[test]
fn pressure_refusals_have_no_partial_plan_or_preview_and_old_outputs_are_preserved() {
    let w = Workspace::new();
    let baseline: Value = serde_json::from_str(OPTIONS).unwrap();
    for kind in 0..4 {
        let mut o = baseline.clone();
        match kind {
            0 => o["machine"]["max_stack_mm"] = json!(0.7),
            1 => o["qualifications"][0]
                .as_object_mut()
                .unwrap()
                .remove("presser_foot")
                .map(|_| ())
                .unwrap(),
            2 => {
                o["passes"][0]["presser_foot"]["pressure_pa"] = json!(501);
                for profile in o["passes"][0]["presser_foot"]["materials"]
                    .as_object_mut()
                    .unwrap()
                    .values_mut()
                {
                    profile["pressure_pa"] = json!(501);
                }
            }
            _ => o["passes"][0]["presser_foot"]["materials"] = json!({}),
        }
        w.write("options.json", &o);
        for args in [
            PLAN,
            &["sew-stitches", "pattern.json", "options.json"],
            &[
                "sew-svg",
                "pattern.json",
                "options.json",
                "side",
                "refused.svg",
            ],
        ] {
            let refused = w.run(args);
            assert_eq!(refused.status.code(), Some(2), "case {kind}");
            assert!(refused.stdout.is_empty());
        }
        assert!(!w.0.join("refused.svg").exists());
    }
    w.write("options.json", &baseline);
    let svg_args = &[
        "sew-svg",
        "pattern.json",
        "options.json",
        "side",
        "review.svg",
    ];
    w.json(svg_args);
    let before = fs::read(w.0.join("review.svg")).unwrap();
    let text = std::str::from_utf8(&before).unwrap();
    assert!(text.contains("Foot pressure 500 Pa | loaded nominal 0.4 mm | checked stack 0.44 mm"));
    assert!(text.contains("class=\"loaded-stack\""));
    assert!(
        text.contains("pressure_conditioned_scalar/1")
            || text.contains("independent_ply_equal_pressure/1")
    );
    assert_eq!(w.run(svg_args).status.code(), Some(2));
    assert_eq!(fs::read(w.0.join("review.svg")).unwrap(), before);
}
