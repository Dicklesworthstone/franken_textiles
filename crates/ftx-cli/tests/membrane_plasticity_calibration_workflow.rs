//! Acquired permanent-stretch histories through both real commands and the existing material binding.
//! The shared fixture is synthetic and establishes no empirical textile or garment claim.
use std::{fs, path::PathBuf, process::Command};

use ftx_pattern::material_membrane_plasticity::calibration;
use serde_json::{Value, json};

const INPUT: &str = include_str!(
    "../../../tests/fixtures/materials/synthetic-membrane-plasticity-calibration.json"
);
const PARALLEL_INPUT: &str = include_str!("../../../examples/material-parallel-calibration.json");
const JOINT_INPUT: &str = include_str!("../../../examples/material-joint-calibration.json");

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ftx-permanent-calibration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, input: &Value) {
        fs::write(self.0.join(name), serde_json::to_vec(input).unwrap()).unwrap();
    }

    fn identify(&self, main: bool, admit: bool) -> Value {
        let mut command = Command::new(if main {
            env!("CARGO_BIN_EXE_ftx")
        } else {
            env!("CARGO_BIN_EXE_ftx-materials")
        });
        command.current_dir(&self.0);
        if main {
            command.args(["--json", "materials"]);
        }
        command.args(["calibrate-membrane-plasticity", "histories.json"]);
        if admit {
            command.arg("--allow-estimates");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        result.get("result").unwrap_or(&result).clone()
    }

    fn workbench(&self, arguments: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
            .current_dir(&self.0)
            .args(arguments)
            .output()
            .unwrap()
    }
}

#[test]
fn jointly_unknown_branches_reach_both_commands_binding_and_actual_strip_replay() {
    let workspace = Workspace::new();
    fs::write(workspace.0.join("histories.json"), JOINT_INPUT).unwrap();
    let input: Value = serde_json::from_str(JOINT_INPUT).unwrap();
    assert!(input["material"]["membrane_relaxation"].is_null());
    assert!(input["material"]["properties"]["membrane_yield_warp"].is_null());
    let identified = workspace.identify(false, true);
    assert_eq!(identified, workspace.identify(true, true));
    let calibration = &identified["calibration"];
    assert_eq!(calibration["status"], "ACCEPTED", "{calibration}");
    assert_eq!(calibration["check_status"], "ACCEPTED");
    let parameters = &calibration["parameters"];
    for (key, expected) in [
        ("yield_n_m", [10.0, 8.0, 5.0]),
        ("hardening_n_m", [100.0, 50.0, 25.0]),
    ] {
        for (axis, target) in expected.into_iter().enumerate() {
            assert!((parameters[key][axis].as_f64().unwrap() - target).abs() < 1e-6);
        }
    }
    for (key, target) in [("stiffness_ratio", 1.4), ("relaxation_time_seconds", 0.18)] {
        assert!((parameters["relaxation"][key].as_f64().unwrap() - target).abs() < 1e-6);
    }
    assert_eq!(calibration["predictions"].as_array().unwrap().len(), 32);
    assert!(
        calibration["diagnostics"]["joint_relaxation"]["branch_steps"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(identified["simulation_ready"], true);
    let portable_text =
        ftx_wasm::materials::calibrate_membrane_plasticity(JOINT_INPUT, true).unwrap();
    let portable: Value = serde_json::from_str(&portable_text).unwrap();
    for key in ["calibration", "selection", "selection_json"] {
        assert_eq!(portable[key], identified[key]);
    }
    assert_eq!(workspace.identify(false, false)["simulation_ready"], false);

    let pattern = json!({"schema":"fabric-pattern/1","units":"mm","seams":[],
        "pieces":[{"id":"swatch","cut":{"qty":1},
            "vertices":{"a":[0,0],"b":[100,0],"c":[0,100]},
            "edges":[{"id":"ab","from":"a","to":"b"},
                {"id":"bc","from":"b","to":"c"},{"id":"ca","from":"c","to":"a"}],
            "grain":[[0,0],[0,100]]}]});
    workspace.write("pattern.json", &pattern);
    fs::write(
        workspace.0.join("selection.json"),
        identified["selection_json"].as_str().unwrap(),
    )
    .unwrap();
    let bound = workspace.workbench(&["bind", "pattern.json", "selection.json", "bound.json"]);
    assert!(
        bound.status.success(),
        "{}",
        String::from_utf8_lossy(&bound.stderr)
    );
    let saved: Value =
        serde_json::from_slice(&fs::read(workspace.0.join("bound.json")).unwrap()).unwrap();
    assert_eq!(saved["pieces"], pattern["pieces"]);
    assert_eq!(saved["seams"], pattern["seams"]);
    assert_eq!(saved["fabric"]["material"], identified["selection"]);

    let history = json!({"schema":"ftx-strip-history/1","material":identified["selection"],
        "fixture":{"gauge_length_m":0.1,"width_m":0.06,"warp_axis":[0.0,1.0],"cells":[2,2]},
        "loading":[{"time_s":0.0,"extension_fraction":0.0},
            {"time_s":0.05,"extension_fraction":0.08},{"time_s":0.15,"extension_fraction":0.08},
            {"time_s":0.2,"extension_fraction":0.0},{"time_s":0.25,"extension_fraction":0.0}],
        "controls":{"max_step_s":0.01,"max_steps":32,"max_newton":100,
            "velocity_tolerance_m_s":1e-9,"force_tolerance_n":1e-6}});
    let direct =
        ftx_drape::strip::history::evaluate(&serde_json::from_value(history.clone()).unwrap())
            .unwrap();
    let last = direct.states.last().unwrap();
    assert!(last.plastic_dissipation_j > 0.0 && last.maxwell_dissipation_j > 0.0);
    assert!(
        direct.states[1..]
            .iter()
            .all(|state| state.solve.unwrap().converged)
    );
    let report = serde_json::to_value(&direct).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(
            &ftx_wasm::materials::strip_history(&history.to_string()).unwrap()
        )
        .unwrap(),
        report
    );
    assert_eq!(
        serde_json::from_str::<Value>(
            &ftx_wasm::materials::strip_history(&direct.replay_request_json).unwrap()
        )
        .unwrap(),
        report
    );
    fs::write(workspace.0.join("portable.json"), portable_text).unwrap();
    workspace.write("forward-request.json", &history);
    workspace.write("forward-report.json", &report);
    eprintln!("simultaneous calibration replay: {}", workspace.0.display());

    let mut failed = input;
    let check = failed["experiments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|experiment| experiment["partition"] == "check")
        .unwrap();
    check["samples"].as_array_mut().unwrap().last_mut().unwrap()["second_piola_n_m"][0] =
        json!(1e6);
    workspace.write("histories.json", &failed);
    let rejected = workspace.identify(true, true);
    assert_eq!(rejected["calibration"]["status"], "REJECTED");
    assert_eq!(rejected["calibration"]["parameters"], *parameters);
    assert_eq!(rejected["calibration"]["fit"], calibration["fit"]);
    assert!(rejected["selection_json"].is_null());
}

#[test]
fn joint_calibration_publishes_one_frozen_material_for_actual_timed_cloth_history() {
    let workspace = Workspace::new();
    fs::write(workspace.0.join("histories.json"), PARALLEL_INPUT).unwrap();
    let input: Value = serde_json::from_str(PARALLEL_INPUT).unwrap();
    let identified = workspace.identify(false, true);
    assert_eq!(identified, workspace.identify(true, true));
    assert_eq!(identified["calibration"]["status"], "ACCEPTED");
    assert_eq!(identified["simulation_ready"], true);
    assert_eq!(
        identified["selection"]["record"]["membrane_relaxation"],
        serde_json::to_value(
            serde_json::from_str::<calibration::Input>(PARALLEL_INPUT)
                .unwrap()
                .material
                .membrane_relaxation
        )
        .unwrap()
    );
    let portable_text =
        ftx_wasm::materials::calibrate_membrane_plasticity(PARALLEL_INPUT, true).unwrap();
    let portable: Value = serde_json::from_str(&portable_text).unwrap();
    for key in ["calibration", "selection", "selection_json"] {
        assert_eq!(portable[key], identified[key]);
    }
    assert_eq!(
        portable["material"]["capabilities"]["mechanical_coefficients"]["ready"],
        true
    );
    let denied = workspace.identify(false, false);
    assert_eq!(denied["calibration"], identified["calibration"]);
    assert_eq!(denied["simulation_ready"], false);

    // The independently supplied fixed spectrum has a narrower characterized strain domain.
    // Use an admitted clamped path; global extension alone does not bound every local triangle.
    let history = json!({"schema":"ftx-strip-history/1","material":identified["selection"],
        "fixture":{"gauge_length_m":0.1,"width_m":0.06,"warp_axis":[0.0,1.0],"cells":[2,2]},
        "loading":[{"time_s":0.0,"extension_fraction":0.0},
            {"time_s":0.05,"extension_fraction":0.04},{"time_s":0.15,"extension_fraction":0.04},
            {"time_s":0.2,"extension_fraction":0.0},{"time_s":0.25,"extension_fraction":0.0}],
        "controls":{"max_step_s":0.01,"max_steps":32,"max_newton":100,
            "velocity_tolerance_m_s":1e-9,"force_tolerance_n":1e-6}});
    let request = serde_json::from_value(history.clone()).unwrap();
    let direct = ftx_drape::strip::history::evaluate(&request).unwrap();
    let last = direct.states.last().unwrap();
    assert!(last.plastic_dissipation_j > 0.0 && last.maxwell_dissipation_j > 0.0);
    assert!(last.relaxation.is_some() && last.plasticity.is_some());
    for state in &direct.states[1..] {
        assert!(state.solve.unwrap().converged);
        assert!(state.free_dynamic_residual_n <= 1e-6);
    }
    let report = serde_json::to_value(&direct).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(
            &ftx_wasm::materials::strip_history(&history.to_string()).unwrap()
        )
        .unwrap(),
        report
    );
    let replay = serde_json::from_str(&direct.replay_request_json).unwrap();
    assert_eq!(
        serde_json::to_value(ftx_drape::strip::history::evaluate(&replay).unwrap()).unwrap(),
        report
    );
    fs::write(workspace.0.join("portable.json"), portable_text).unwrap();
    workspace.write("forward-request.json", &history);
    workspace.write("forward-report.json", &report);
    eprintln!("joint calibration replay: {}", workspace.0.display());

    let mut failed = input;
    let check = failed["experiments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|experiment| experiment["partition"] == "check")
        .unwrap();
    check["samples"].as_array_mut().unwrap().last_mut().unwrap()["second_piola_n_m"][0] =
        json!(1e6);
    let rejected: Value = serde_json::from_str(
        &ftx_wasm::materials::calibrate_membrane_plasticity(&failed.to_string(), true).unwrap(),
    )
    .unwrap();
    assert_eq!(rejected["calibration"]["status"], "REJECTED");
    assert_eq!(
        rejected["calibration"]["parameters"],
        identified["calibration"]["parameters"]
    );
    assert_eq!(
        rejected["calibration"]["fit"],
        identified["calibration"]["fit"]
    );
    assert!(rejected["selection_json"].is_null());
}

#[test]
fn both_commands_publish_the_native_calibration_and_bind_the_exact_source_selection() {
    let workspace = Workspace::new();
    fs::write(workspace.0.join("histories.json"), INPUT).unwrap();
    let native = calibration::calibrate(&serde_json::from_str(INPUT).unwrap()).unwrap();
    let denied = workspace.identify(false, false);
    let identified = workspace.identify(false, true);
    assert_eq!(identified, workspace.identify(true, true));
    assert_eq!(
        identified["calibration"],
        serde_json::to_value(native).unwrap()
    );
    assert_eq!(denied["calibration"], identified["calibration"]);
    assert_eq!(identified["calibration"]["status"], "ACCEPTED");
    assert_eq!(denied["simulation_ready"], false);
    assert_eq!(denied["selection"]["allow_estimates"], false);
    assert_eq!(identified["simulation_ready"], true);
    assert_eq!(identified["selection"]["allow_estimates"], true);
    assert_eq!(identified["empirical_validation_claim"], false);
    assert_eq!(identified["fit_claim"], false);
    let original = json!({"schema":"fabric-pattern/1","units":"mm","seams":[],
        "fabric":{"note":"Keep authored metadata"},
        "pieces":[{"id":"swatch","material":"shell","cut":{"qty":1},
            "vertices":{"a":[0,0],"b":[100,0],"c":[0,100]},
            "edges":[{"id":"ab","from":"a","to":"b"},
                {"id":"bc","from":"b","to":"c"},{"id":"ca","from":"c","to":"a"}],
            "grain":[[0,0],[0,100]]}]});
    workspace.write("pattern.json", &original);
    fs::write(
        workspace.0.join("selection.json"),
        identified["selection_json"].as_str().unwrap(),
    )
    .unwrap();
    let bound = workspace.workbench(&["bind", "pattern.json", "selection.json", "bound.json"]);
    assert!(
        bound.status.success(),
        "{}",
        String::from_utf8_lossy(&bound.stderr)
    );
    let bound: Value = serde_json::from_slice(&bound.stdout).unwrap();
    assert_eq!(bound["geometry_changed"], false);
    let bound_bytes = fs::read(workspace.0.join("bound.json")).unwrap();
    let saved: Value = serde_json::from_slice(&bound_bytes).unwrap();
    assert_eq!(saved["pieces"], original["pieces"]);
    assert_eq!(saved["seams"], original["seams"]);
    assert_eq!(saved["fabric"]["note"], original["fabric"]["note"]);
    let resolved = workspace.workbench(&["resolve", "bound.json"]);
    assert!(
        resolved.status.success(),
        "{}",
        String::from_utf8_lossy(&resolved.stderr)
    );
    let resolved: Value = serde_json::from_slice(&resolved.stdout).unwrap();
    assert_eq!(
        resolved["record_sha256"],
        identified["selection"]["expected_sha256"]
    );
    assert!(resolved["membrane_plasticity_law_sha256"].is_string());
    let mut changed = saved;
    changed["fabric"]["material"]["record"]["properties"]["membrane_yield_warp"]["value"] =
        json!(123.0);
    workspace.write("changed.json", &changed);
    assert!(
        !workspace
            .workbench(&["resolve", "changed.json"])
            .status
            .success()
    );
    assert!(
        !workspace
            .workbench(&["bind", "pattern.json", "selection.json", "bound.json"])
            .status
            .success()
    );
    assert_eq!(
        fs::read(workspace.0.join("bound.json")).unwrap(),
        bound_bytes
    );
}

#[test]
fn failed_checks_and_exhausted_work_retain_the_report_without_an_assignable_result() {
    let workspace = Workspace::new();
    let original: Value = serde_json::from_str(INPUT).unwrap();
    workspace.write("histories.json", &original);
    let accepted = workspace.identify(false, true);
    let mut failed = original.clone();
    let check = failed["experiments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|experiment| experiment["partition"] == "check")
        .unwrap();
    check["samples"].as_array_mut().unwrap().last_mut().unwrap()["second_piola_n_m"][0] =
        json!(1_000_000.0);
    workspace.write("histories.json", &failed);
    let refused = workspace.identify(false, true);
    assert_eq!(refused["calibration"]["status"], "REJECTED");
    assert_eq!(
        refused["calibration"]["parameters"],
        accepted["calibration"]["parameters"]
    );
    assert_eq!(
        refused["calibration"]["fit"],
        accepted["calibration"]["fit"]
    );
    for key in ["selection", "selection_json"] {
        assert!(refused[key].is_null());
    }
    assert_eq!(refused["simulation_ready"], false);
    assert!(
        refused["calibration"]["check"]["peak_scaled"]
            .as_f64()
            .unwrap()
            > 1.0
    );
    let mut limited = original;
    limited["max_evaluations"] = json!(1);
    workspace.write("histories.json", &limited);
    let stopped = workspace.identify(true, true);
    assert_eq!(stopped["calibration"]["status"], "BUDGET_EXHAUSTED");
    assert!(stopped["selection"].is_null());
    assert!(stopped["selection_json"].is_null());
    assert_eq!(stopped["simulation_ready"], false);
}

#[test]
fn consuming_estimates_cannot_admit_unapproved_acquisition_data() {
    let workspace = Workspace::new();
    let mut denied: Value = serde_json::from_str(INPUT).unwrap();
    denied["allow_estimated_observations"] = json!(false);
    workspace.write("histories.json", &denied);
    let result = workspace.workbench(&[
        "calibrate-membrane-plasticity",
        "histories.json",
        "--allow-estimates",
    ]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let unknown = workspace.workbench(&[
        "calibrate-membrane-plasticity",
        "histories.json",
        "--allow-anything",
    ]);
    assert!(!unknown.status.success());
    assert!(unknown.stdout.is_empty());
}
