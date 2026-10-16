//! Real native commands: setup failures remain independent from real material manufacturing.
use ftx_core::receipt::sha256_hex;
use ftx_export::material_study::Request;
use ftx_pattern::Pattern;
use serde_json::{Value, json};
use std::{
    path::Path,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[test]
fn both_native_commands_preserve_manufacturing_when_the_model_is_unavailable() {
    let dir = std::env::temp_dir().join(format!(
        "ftx-scenario-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let scenario = dir.join("scenario.json");
    std::fs::write(
        &scenario,
        include_str!("../../../tests/fixtures/materials/synthetic-scenario.json"),
    )
    .unwrap();
    let measurements = dir.join("measurements.json");
    std::fs::write(&measurements, b"{}").unwrap();
    let model = dir.join("model.ftxbody");
    let bad_model = b"Not a converted body model; intentional failure test";
    std::fs::write(&model, bad_model).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/materials");
    let pattern = root.join("synthetic-sewing-pattern.json");
    let study = root.join("study-insulation.json");
    let p = Pattern::from_json(&std::fs::read_to_string(&pattern).unwrap()).unwrap();
    let r: Request = serde_json::from_slice(&std::fs::read(&study).unwrap()).unwrap();
    let expected = serde_json::to_value(r.evaluate(&p).unwrap()).unwrap();
    let mut outputs = Vec::new();
    for (binary, prefix) in [
        (env!("CARGO_BIN_EXE_ftx-materials"), vec!["scenario-study"]),
        (
            env!("CARGO_BIN_EXE_ftx"),
            vec!["--json", "materials", "scenario-study"],
        ),
    ] {
        let out = Command::new(binary)
            .args(prefix)
            .args([&pattern, &study, &scenario, &measurements, &model])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let raw: Value = serde_json::from_slice(&out.stdout).unwrap();
        let result = raw.get("result").unwrap_or(&raw);
        assert_eq!(result["manufacturing"], expected);
        assert_eq!(result["scenario"]["model_sha256"], sha256_hex(bad_model));
        assert!(result["scenario"]["body_setup_refusal"].is_object());
        for row in result["simulations"].as_array().unwrap() {
            assert_eq!(row["evaluation"]["status"], "refused");
        }
        outputs.push(result.clone());
    }
    assert_eq!(outputs[0], outputs[1]);
    let mut invalid: Value = serde_json::from_slice(&std::fs::read(&scenario).unwrap()).unwrap();
    invalid["options"]["sew_steps"] = json!(1000000);
    std::fs::write(&scenario, invalid.to_string()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .args(["--json", "materials", "scenario-study"])
        .args([
            &pattern,
            &study,
            &scenario,
            &measurements,
            &dir.join("does-not-exist"),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("work limits"), "{text}");
    assert!(
        !text.contains("does-not-exist"),
        "scenario validation must precede opening the model"
    );
    assert_eq!(std::fs::read(&model).unwrap(), bad_model);
}
