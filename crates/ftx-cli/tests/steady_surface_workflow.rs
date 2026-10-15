//! Both real CLI entry points over the same declared synthetic source/geometry exchange.
use ftx_drape::thermal::steady::request::SurfaceConditions;
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

const REPORT: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-steady-surface-report.json");
const CONDITIONS: &str =
    include_str!("../../../tests/fixtures/materials/steady-surface-conditions.json");

fn invoke(main: bool, report: &Value, conditions: &Value, extra: &[&str]) -> Output {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "ftx-steady-surface-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    let report_bytes = serde_json::to_vec(report).unwrap();
    let conditions_bytes = serde_json::to_vec(conditions).unwrap();
    fs::write(root.join("report.json"), &report_bytes).unwrap();
    fs::write(root.join("conditions.json"), &conditions_bytes).unwrap();
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    if main {
        command.args(["materials", "--json"]);
    }
    let out = command
        .arg("thermal-steady-surface")
        .arg(root.join("report.json"))
        .arg(root.join("conditions.json"))
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(fs::read(root.join("report.json")).unwrap(), report_bytes);
    assert_eq!(
        fs::read(root.join("conditions.json")).unwrap(),
        conditions_bytes
    );
    out
}
fn success(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    if value["schema"] == "ftx-cli/1" {
        assert_eq!(value["command"], "materials");
        assert_eq!(value["ok"], true);
        assert_eq!(value["exit"], 0);
        value["result"].clone()
    } else {
        value
    }
}

#[test]
fn both_binaries_match_the_library_and_frozen_request_replays() {
    let report: Value = serde_json::from_str(REPORT).unwrap();
    let conditions: Value = serde_json::from_str(CONDITIONS).unwrap();
    let request: SurfaceConditions = serde_json::from_value(conditions.clone()).unwrap();
    let expected = request.evaluate(&report).unwrap();
    for main in [false, true] {
        let out = invoke(main, &report, &conditions, &[]);
        let value = success(&out);
        assert_eq!(value, expected);
        assert_eq!(out.stdout, invoke(main, &report, &conditions, &[]).stdout);
        let request = &expected["request"];
        assert_eq!(
            value,
            success(&invoke(
                main,
                &request["report"],
                &request["conditions"],
                &[]
            ))
        );
    }
    let power = expected["steady"]["power_w"].as_f64().unwrap();
    assert!((power - (15.0 / 0.7 + 15.0 / 1.1)).abs() < 1e-8);
    assert_eq!(expected["steady"]["area_m2"], 1.0);
    assert_eq!(expected["storage_required"], false);
    assert_eq!(expected["comfort_claim"], false);
}

#[test]
fn boundary_assignments_are_model_bound_and_changed_or_unsupported_inputs_refuse() {
    let report: Value = serde_json::from_str(REPORT).unwrap();
    let conditions: Value = serde_json::from_str(CONDITIONS).unwrap();
    let mut invalids = Vec::new();
    let mut wrong = report.clone();
    wrong["steady_thermal_surface"]["positions_m"][0][0] = json!(2.0);
    invalids.push((wrong, conditions.clone()));
    for (key, value) in [
        ("schema", json!("wrong")),
        ("boundaries", json!([])),
        ("transient", json!({"steps":10})),
        ("model_sha256", json!("stale")),
        (
            "boundaries",
            json!([conditions["boundaries"][0], conditions["boundaries"][0]]),
        ),
    ] {
        let mut wrong = conditions.clone();
        wrong[key] = value;
        invalids.push((report.clone(), wrong));
    }
    for main in [false, true] {
        for (report, conditions) in &invalids {
            let out = invoke(main, report, conditions, &[]);
            assert!(!out.status.success());
            assert!(!String::from_utf8_lossy(&out.stdout).contains("ftx-steady-surface-result/1"));
        }
        assert!(
            !invoke(main, &report, &conditions, &["--allow-estimates"])
                .status
                .success()
        );
    }
    let mut bound = conditions;
    bound["boundaries"] = json!([bound["boundaries"][0], bound["boundaries"][0]]);
    bound["model_sha256"] = report["steady_thermal_surface"]["model"]["sha256"].clone();
    assert!(invoke(false, &report, &bound, &[]).status.success());
}

#[test]
fn main_stack_command_matches_existing_companion_and_rejects_unknown_arguments() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/materials/fsri-steady-conduction.json"
    );
    let a = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .args(["materials", "thermal-steady", path, "--json"])
        .output()
        .unwrap();
    let b = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .args(["thermal-steady", path])
        .output()
        .unwrap();
    assert_eq!(success(&a), success(&b));
    for args in [
        vec!["materials", "thermal-steady"],
        vec!["materials", "thermal-steady", path, "extra"],
        vec!["materials", "thermal-steady", path, "--allow-estimates"],
    ] {
        assert!(
            !Command::new(env!("CARGO_BIN_EXE_ftx"))
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}

#[test]
fn primary_discovery_agrees_with_workbench_on_condition_qualified_readiness() {
    for id in [
        "fsri/hfm/cotton-sheet/dry/220330-r1",
        "nist/7282/cotton-duck",
    ] {
        for allowed in [false, true] {
            let mut main = Command::new(env!("CARGO_BIN_EXE_ftx"));
            main.args(["materials", "show", id, "--json"]);
            let mut companion = Command::new(env!("CARGO_BIN_EXE_ftx-materials"));
            companion.args(["show", id]);
            if allowed {
                main.arg("--allow-estimates");
                companion.arg("--allow-estimates");
            }
            let a = success(&main.output().unwrap());
            let b = success(&companion.output().unwrap());
            assert_eq!(a["steady_conduction"], b["steady_conduction"]);
            assert_eq!(a["transient_thermal"], b["transient_thermal"]);
            assert_eq!(a["steady_conduction"]["ready"], allowed);
            assert_eq!(a["transient_thermal"]["ready"], false);
        }
    }
}
