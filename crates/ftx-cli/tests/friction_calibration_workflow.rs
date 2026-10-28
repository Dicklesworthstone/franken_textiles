//! Real binary acquisition/selection/resolution; all specimens and forces are synthetic.
use serde_json::{Value, json};
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-friction-sliding.json"
    ))
    .unwrap()
}
fn workspace() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "ftx-friction-cli-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    p
}
fn run(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}
fn ok(dir: &std::path::Path, args: &[&str]) -> Value {
    let out = run(dir, args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn write(dir: &std::path::Path, name: &str, v: &Value) {
    fs::write(dir.join(name), serde_json::to_vec(v).unwrap()).unwrap();
}
const FIT: &[&str] = &["calibrate-friction", "sliding.json", "--allow-estimates"];

#[test]
fn real_pair_command_keeps_interface_permission_separate_and_reports_partial_materials() {
    let dir = workspace();
    let mut raw = fixture();
    write(&dir, "sliding.json", &raw);
    let denied = ok(&dir, &["calibrate-friction", "sliding.json"]);
    assert_eq!(denied["readiness"]["scope"], "interface_only");
    assert_eq!(denied["readiness"]["ready"], false);
    let fitted = ok(&dir, FIT);
    assert_eq!(fitted, ok(&dir, FIT));
    assert_eq!(fitted["readiness"]["ready"], true);
    assert_eq!(fitted["readiness"]["whole_garment_checked"], false);
    assert!(
        (fitted["binding"]["interface"]["friction"]["value"]
            .as_f64()
            .unwrap()
            - 0.4)
            .abs()
            < 1e-12
    );
    assert_eq!(
        fitted["binding"]["interface"]["friction"]["evidence"],
        "estimate"
    );
    raw["target"]["materials"][0]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("thickness");
    write(&dir, "sliding.json", &raw);
    let partial = ok(&dir, FIT);
    assert_eq!(partial["readiness"]["ready"], true); // only the interface is qualified numerically
    assert_eq!(partial["readiness"]["sheets"][0]["ready"], false);
    assert!(partial["readiness"]["sheets"][0]["refusal"].is_object());
}

#[test]
fn body_acquisition_binding_preserves_geometry_and_reaches_real_contact_parameters() {
    let dir = workspace();
    let mut raw = fixture();
    raw["target"] = json!({"kind":"body","material":raw["target"]["materials"][0],
        "counterface":{"reference":"test:body-surrogate","accessed_on":"2026-10-18","note":"Synthetic, not a measurement on human skin"}});
    raw["replace_existing"] = json!(true);
    write(&dir, "sliding.json", &raw);
    let fitted = ok(&dir, FIT);
    assert_eq!(fitted["readiness"]["scope"], "material_mechanics");
    assert_eq!(fitted["readiness"]["ready"], true);
    write(&dir, "selection.json", &fitted["binding"]["selection"]);
    let pattern = json!({"schema":"fabric-pattern/1","units":"mm","seams":[],"fabric":{},
        "pieces":[{"id":"coupon","cut":{"qty":1},"vertices":{"a":[0,0],"b":[10,0],"c":[0,10]},
            "edges":[{"id":"ab","from":"a","to":"b"},{"id":"bc","from":"b","to":"c"},{"id":"ca","from":"c","to":"a"}],"grain":[[0,0],[0,10]]}]});
    write(&dir, "pattern.json", &pattern);
    ok(
        &dir,
        &["bind", "pattern.json", "selection.json", "bound.json"],
    );
    let bytes = fs::read(dir.join("bound.json")).unwrap();
    let bound: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(pattern["pieces"], bound["pieces"]);
    assert_eq!(pattern["seams"], bound["seams"]);
    let actual = ok(&dir, &["resolve", "bound.json"]);
    assert!((actual["mechanics"]["friction_skin"].as_f64().unwrap() - 0.4).abs() < 1e-12);
    assert_eq!(actual["mechanics"]["friction_self"], 0.3);
    assert_eq!(actual["body_offset_m"], 0.0003);
    assert!(
        !run(
            &dir,
            &["bind", "pattern.json", "selection.json", "bound.json"]
        )
        .status
        .success()
    );
    assert_eq!(bytes, fs::read(dir.join("bound.json")).unwrap());
}

#[test]
fn failed_checks_and_unknown_flags_do_not_emit_partial_bindings() {
    let dir = workspace();
    let mut raw = fixture();
    raw["experiments"][1]["samples"][0]["pull_n"] = json!([0, 500]);
    write(&dir, "sliding.json", &raw);
    let out = run(&dir, FIT);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("held-out"));
    let out = run(
        &dir,
        &["calibrate-friction", "sliding.json", "--allow-anything"],
    );
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}
