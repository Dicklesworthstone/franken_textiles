//! The public area-response examples use the same production authority at both native boundaries.
//! Their declared coefficients are synthetic numerical inputs, not measured textile properties.
use ftx_core::refusal::{Refusal, RefusalCode};
use ftx_drape::strip::{self, Request};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const BASELINE: &str = include_str!("../../../examples/area-response/baseline.json");
const ENABLED: &str = include_str!("../../../examples/area-response/enabled.json");

fn workspace() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ftx-area-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn write(dir: &Path, name: &str, value: &Value) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(name))
        .unwrap()
        .write_all(&serde_json::to_vec(value).unwrap())
        .unwrap();
}

fn run(dir: &Path, unified: bool, name: &str) -> Output {
    let mut command = Command::new(if unified {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(dir);
    if unified {
        command.arg("materials");
    }
    command.args(["strip-experiment", name]);
    if unified {
        command.arg("--json");
    }
    command.output().unwrap()
}

fn success(output: Output, unified: bool) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    if unified {
        assert_eq!(value["ok"], true);
        assert_eq!(value["exit"], 0);
        value["result"].clone()
    } else {
        value
    }
}

fn refused(output: Output, unified: bool, expected: &Refusal) {
    assert_eq!(output.status.code(), Some(if unified { 3 } else { 2 }));
    let value: Value = if unified {
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        value
    } else {
        assert!(output.stdout.is_empty());
        serde_json::from_slice(&output.stderr).unwrap()
    };
    assert_eq!(value["refusal"], serde_json::to_value(expected).unwrap());
    assert!(value.get("result").is_none());
}

#[test]
fn material_area_change_reaches_both_commands_and_frozen_replay_exactly() {
    let dir = workspace();
    let mut last_states = Vec::new();
    for (name, raw) in [("baseline", BASELINE), ("enabled", ENABLED)] {
        let input: Request = serde_json::from_str(raw).unwrap();
        let core = strip::evaluate(&input).unwrap();
        let expected = serde_json::to_value(&core).unwrap();
        let original = format!("{name}.json");
        let replay = format!("{name}-frozen.json");
        write(&dir, &original, &serde_json::from_str(raw).unwrap());
        write(&dir, &replay, &expected["input"]);
        for unified in [false, true] {
            assert_eq!(success(run(&dir, unified, &original), unified), expected);
            assert_eq!(success(run(&dir, unified, &replay), unified), expected);
        }
        assert!(core.input.material.expected_sha256.is_some());
        assert!(
            core.input
                .material
                .record
                .as_ref()
                .unwrap()
                .properties
                .is_empty()
        );
        last_states.push(core.states.last().unwrap().clone());
    }
    assert!((last_states[0].force_n - last_states[1].force_n).abs() > 1e-6);
    assert!((last_states[0].mid_width_m - last_states[1].mid_width_m).abs() > 1e-8);
}

#[test]
fn both_commands_preserve_estimate_source_matching_and_area_domain_refusals() {
    let dir = workspace();
    let original: Value = serde_json::from_str(ENABLED).unwrap();
    let mut denied = original.clone();
    denied["material"]["allow_estimates"] = json!(false);
    let mut mismatched = original.clone();
    mismatched["material"]["record"]["membrane_response"]["area_response"]["quadratic"]["conditions"]
        ["specimen"] = json!("a different synthetic specimen");
    let mut outside = original;
    outside["material"]["record"]["membrane_response"]["area_response"]["area_ratio_range"] =
        json!([1.0, 1.00001]);
    for (name, value, code) in [
        ("denied.json", denied, RefusalCode::InvalidDocument),
        ("mismatched.json", mismatched, RefusalCode::InvalidDocument),
        ("outside.json", outside, RefusalCode::OutOfRange),
    ] {
        let typed: Request = serde_json::from_value(value.clone()).unwrap();
        let expected = strip::evaluate(&typed).unwrap_err();
        assert_eq!(expected.code, code);
        write(&dir, name, &value);
        for unified in [false, true] {
            refused(run(&dir, unified, name), unified, &expected);
        }
    }
}
