//! Actual source-shell CLI execution, exact shared artifacts and refusal before publication.

use std::path::Path;
use std::process::Command;

use ftx_export::knit::{ShapedKnitSource, export_shaped_knit};
use ftx_pattern::knit::{AutoKnitConfig, GaugeSpec, KnitMachineCert, MachineProfile};
use serde_json::{Value, json};

#[test]
fn shape_compiles_source_geometry_and_preserves_exact_artifacts() {
    let shell = include_str!("../../../tests/fixtures/knit/synthetic-annular-shell.json");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ftx-knit-shape-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let shell_path = root.join("shell.json");
    let config_path = root.join("config.json");
    let gauge_path = root.join("gauge.json");
    let config: AutoKnitConfig = serde_json::from_value(json!({
        "name":"synthetic-cli-annulus", "wearer_age_class":"adult",
        "yarn_id":"synthetic-yarn", "carrier_id":"1", "cam_setting":13.0,
        "max_idt_flips":1024, "has_hood_or_neck_drawstring":false
    }))
    .unwrap();
    let gauge: GaugeSpec = serde_json::from_value(json!({
        "wales_per_100mm":20.0, "courses_per_100mm":25.0, "needle_pitch_mm":4.0,
        "yarn_tex":180.0, "thickness_mm":2.0, "relaxed_after_wash":true
    }))
    .unwrap();
    std::fs::write(&shell_path, shell).unwrap();
    std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    std::fs::write(&gauge_path, serde_json::to_vec(&gauge).unwrap()).unwrap();
    let invoke = |out: &Path| {
        Command::new(env!("CARGO_BIN_EXE_ftx"))
            .args(["knit", "shape"])
            .arg(&shell_path)
            .arg("--config")
            .arg(&config_path)
            .arg("--gauge")
            .arg(&gauge_path)
            .args([
                "--machine",
                "hand-knit",
                "--created",
                "2026-11-27T12:00",
                "--out",
            ])
            .arg(out)
            .arg("--json")
            .output()
            .unwrap()
    };
    let output = root.join("accepted");
    let run = invoke(&output);
    assert!(
        run.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let envelope: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(envelope["schema"], "ftx-cli/1");
    assert_eq!(envelope["result"]["fit_claimed"], false);
    assert!(
        (envelope["result"]["shape"]["shell_height_mm"]
            .as_f64()
            .unwrap()
            - 80.0)
            .abs()
            < 1e-8
    );
    let expected = export_shaped_knit(
        ShapedKnitSource {
            shell_json: shell,
            config: &config,
            gauge: &gauge,
        },
        &MachineProfile::hand_knit(4.0),
        None,
        None,
        None,
        "2026-11-27T12:00",
    )
    .unwrap();
    for (name, bytes) in expected.export.files {
        assert_eq!(std::fs::read(output.join(&name)).unwrap(), bytes, "{name}");
    }
    assert_eq!(
        std::fs::read(output.join("source_shell.json")).unwrap(),
        shell.as_bytes()
    );
    let cert: KnitMachineCert =
        serde_json::from_slice(&std::fs::read(output.join("cert.json")).unwrap()).unwrap();
    cert.verify_integrity().unwrap();
    assert!(!cert.hardware_execution_authorized);
    assert!(
        !invoke(&output).status.success(),
        "completed artifacts must not be overwritten"
    );

    let mut invalid: Value = serde_json::from_str(shell).unwrap();
    invalid["boundary_rings"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "name":"undeclared_branch", "is_start":true, "vertices":[4,5,6,7], "perimeter_m":0.08
        }));
    std::fs::write(&shell_path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    let refused = root.join("refused");
    assert!(!invoke(&refused).status.success());
    assert!(
        !refused.exists(),
        "unsupported topology must leave no apparent successful export"
    );
}
