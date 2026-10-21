//! Both native binaries execute the actual source-qualified finite strip, without garment data.
//! Coefficients and conditions are synthetic software inputs, not textile measurements.
use ftx_core::receipt::sha256_hex;
use ftx_drape::strip::{Request, evaluate};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn request() -> Value {
    let mut properties = serde_json::Map::new();
    for (key, value, unit) in [
        ("warp_stiffness", 0.8, "kN/m"),
        ("weft_stiffness", 0.4, "kN/m"),
        ("poisson_warp_weft", 0.0, "1"),
        ("shear_stiffness", 0.12, "kN/m"),
    ] {
        properties.insert(
            key.into(),
            json!({"value":value,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic independent force check",
            "conditions":{"conditioning":"synthetic dry reference"}}),
        );
    }
    json!({"schema":"ftx-strip-experiment/1",
        "material":{"id":"synthetic-native-strip","allow_estimates":true,"record":{
            "id":"synthetic-native-strip","name":"Synthetic membrane only","form":"knit",
            "properties":properties,"sources":{"test":{"reference":"test:strip-consumers",
                "accessed_on":"2026-10-21","note":"Synthetic numerical fixture, not measured fabric"}}}},
        "fixture":{"width_m":0.06,"gauge_length_m":0.1,"warp_axis":[0.0,1.0],"cells":[2,4]},
        "extensions":[0.0,0.1],"controls":{"tolerance_m":1e-10,"force_tolerance_n":1e-7,"max_newton":100}})
}

fn call(dir: &Path, input: &Value, main: bool) -> Result<Value, Value> {
    let path = dir.join("input.json");
    let original = input.to_string();
    fs::write(&path, &original).unwrap();
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    if main {
        command.args(["--json", "materials"]);
    }
    let output = command.arg("strip-experiment").arg(&path).output().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), original);
    if output.status.success() {
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        if main {
            assert_eq!(report["ok"], true);
            Ok(report["result"].clone())
        } else {
            Ok(report)
        }
    } else {
        assert_eq!(output.status.code(), Some(if main { 3 } else { 2 }));
        let report: Value =
            serde_json::from_slice(if main { &output.stdout } else { &output.stderr })
                .unwrap_or_else(|error| {
                    panic!(
                        "{error}: stdout {:?}; stderr {:?}",
                        output.stdout, output.stderr
                    )
                });
        assert!(report.get("result").is_none());
        Err(report["refusal"].clone())
    }
}

fn directory() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ftx-strip-consumers-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn both_native_entry_points_preserve_source_conditions_and_exact_frozen_forward_replay() {
    let input = request();
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let direct = evaluate(&typed).unwrap();
    let expected = serde_json::to_value(&direct).unwrap();
    assert_eq!(
        direct.input_sha256,
        sha256_hex(&serde_json::to_vec(&direct.input).unwrap())
    );
    assert!(direct.input.material.expected_sha256.is_some());
    // For nu=0, the affine solution is exact: F = width * Y * lambda * (lambda²-1)/2.
    assert!((direct.states[1].force_n - 0.06 * 800.0 * 1.1 * 0.105).abs() < 1e-10);
    assert_eq!(
        expected["input"]["material"]["record"],
        serde_json::to_value(&typed.material.record).unwrap()
    );
    assert_eq!(
        direct
            .input
            .material
            .record
            .as_ref()
            .unwrap()
            .properties
            .len(),
        4
    );
    let dir = directory();
    for main in [false, true] {
        assert_eq!(call(&dir, &input, main).unwrap(), expected);
        assert_eq!(call(&dir, &expected["input"], main).unwrap(), expected);
    }
}

#[test]
fn native_consumers_preserve_real_source_domain_and_numerical_work_refusals() {
    let original = request();
    let mut denied = original.clone();
    denied["material"]["allow_estimates"] = json!(false);
    let mut outside = original.clone();
    outside["material"]["record"]["sources"]["test"]["reference"] =
        json!("local:ftx-strip-calibration/synthetic-consumer-domain");
    for quantity in outside["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        quantity["conditions"]["fitted_green_strain_range"] =
            json!("[[-0.01,0.03],[-0.01,0.03],[-0.01,0.01]]");
    }
    let mut exhausted = original.clone();
    exhausted["material"]["record"]["properties"]["poisson_warp_weft"]["value"] = json!(0.25);
    exhausted["controls"]["max_newton"] = json!(1);
    let mut excessive = original;
    excessive["extensions"] = json!(vec![0.0; 65]);
    let dir = directory();
    for input in [denied, outside, exhausted, excessive] {
        let typed: Request = serde_json::from_value(input.clone()).unwrap();
        let expected = serde_json::to_value(evaluate(&typed).unwrap_err()).unwrap();
        for main in [false, true] {
            assert_eq!(call(&dir, &input, main).unwrap_err(), expected);
        }
    }
}
