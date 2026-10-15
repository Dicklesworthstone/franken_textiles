//! Real binary material binding/resolution, with synthetic source-bearing bending coefficients.
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn directory() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "ftx-panel-bending-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&p).unwrap();
    p
}
fn call(words: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .args(words)
        .output()
        .unwrap()
}
fn write(p: &Path, v: &Value) {
    std::fs::write(p, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}
fn selection() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-panel-bending.json"
    ))
    .unwrap()
}
fn pattern() -> Value {
    json!({"schema":"fabric-pattern/1","units":"mm","style":"synthetic-panel","seams":[],
        "fabric":{"areal_g_m2":200,"user_note":"keep this"},
        "pieces":[{"id":"panel","material":"shell label","cut":{"qty":1},
            "vertices":{"a":[0,0],"b":[100,0],"c":[0,100]},
            "edges":[{"id":"ab","from":"a","to":"b"},{"id":"bc","from":"b","to":"c"},{"id":"ca","from":"c","to":"a"}],
            "grain":[[0,0],[100,0]]}]})
}
#[test]
fn bind_preserves_pattern_then_real_resolver_consumes_locked_tensor() {
    let d = directory();
    let p = d.join("pattern.json");
    let s = d.join("selection.json");
    let out = d.join("bound.json");
    write(&p, &pattern());
    write(&s, &selection());
    let run = call(&[
        "bind",
        p.to_str().unwrap(),
        s.to_str().unwrap(),
        out.to_str().unwrap(),
    ]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let report: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(report["simulation_ready"], true);
    let bound: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(bound["pieces"], pattern()["pieces"]);
    assert_eq!(bound["fabric"]["user_note"], "keep this");
    assert!(bound["fabric"]["material"]["expected_sha256"].is_string());
    let resolved = call(&["resolve", out.to_str().unwrap()]);
    assert!(resolved.status.success());
    let resolved: Value = serde_json::from_slice(&resolved.stdout).unwrap();
    assert_eq!(resolved["bending_model"], "orthotropic_quadratic_patch/1");
    assert_eq!(resolved["panel_bending_matrix_nm"][0][0], 0.0002);
    assert_eq!(resolved["panel_bending_matrix_nm"][2][2], 0.000025);
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        !call(&[
            "bind",
            p.to_str().unwrap(),
            s.to_str().unwrap(),
            out.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(bytes, std::fs::read(&out).unwrap());
}
#[test]
fn incomplete_panel_data_is_not_reported_ready_or_consumed_by_drape() {
    let d = directory();
    let p = d.join("pattern.json");
    let s = d.join("selection.json");
    let out = d.join("incomplete.json");
    let mut select = selection();
    select["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("bending_twist");
    write(&p, &pattern());
    write(&s, &select);
    let run = call(&[
        "bind",
        p.to_str().unwrap(),
        s.to_str().unwrap(),
        out.to_str().unwrap(),
    ]);
    assert!(run.status.success()); // incomplete evidence can still be stored
    let report: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(report["simulation_ready"], false);
    let run = call(&["resolve", out.to_str().unwrap()]);
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("bending_twist"));
}

#[test]
fn normal_projected_selection_reaches_the_production_resolver_without_solver_flags() {
    let d = directory();
    let p = d.join("pattern.json");
    let s = d.join("normal-selection.json");
    let out = d.join("bound-normal.json");
    let mut select = selection();
    select["record"]["structure"]["bending_model"] = json!("orthotropic_normal_patch/1");
    write(&p, &pattern());
    write(&s, &select);
    let run = call(&[
        "bind",
        p.to_str().unwrap(),
        s.to_str().unwrap(),
        out.to_str().unwrap(),
    ]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let bound: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(bound["pieces"], pattern()["pieces"]);
    let result = call(&["resolve", out.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["bending_model"], "orthotropic_normal_patch/1");
    assert_eq!(result["panel_bending_matrix_nm"][0][0], 0.0002);
    assert_eq!(
        result["record_sha256"],
        bound["fabric"]["material"]["expected_sha256"]
    );
}

#[test]
fn preferred_curvature_is_carried_by_binding_and_incomplete_targets_are_not_ready() {
    let d = directory();
    let p = d.join("pattern.json");
    let s = d.join("curved-selection.json");
    let out = d.join("curved-pattern.json");
    let mut select = selection();
    select["record"]["structure"]["bending_model"] = json!("orthotropic_normal_patch/1");
    for (key, value) in ftx_pattern::material_bending::PREFERRED_CURVATURE_PROPERTIES
        .into_iter()
        .zip([0.05, -0.02, 0.01])
    {
        let mut datum = select["record"]["properties"]["bending_warp"].clone();
        datum["value"] = json!(value);
        datum["unit"] = json!("1/cm");
        select["record"]["properties"][key] = datum;
    }
    write(&p, &pattern());
    write(&s, &select);
    let bound = call(&[
        "bind",
        p.to_str().unwrap(),
        s.to_str().unwrap(),
        out.to_str().unwrap(),
    ]);
    assert!(
        bound.status.success(),
        "{}",
        String::from_utf8_lossy(&bound.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&bound.stdout).unwrap()["simulation_ready"],
        true
    );
    let value: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(value["pieces"], pattern()["pieces"]);
    let result = call(&["resolve", out.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        result["record"]["normalized"]["properties"]["preferred_curvature_warp"]["value"],
        5.0
    );
    assert_eq!(
        result["record"]["authored"]["properties"]["preferred_curvature_warp"]["unit"],
        "1/cm"
    );
    select["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("preferred_curvature_twist");
    write(&s, &select);
    let missing = d.join("partial-curvature.json");
    let run = call(&[
        "bind",
        p.to_str().unwrap(),
        s.to_str().unwrap(),
        missing.to_str().unwrap(),
    ]);
    assert!(run.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&run.stdout).unwrap()["simulation_ready"],
        false
    );
    let run = call(&["resolve", missing.to_str().unwrap()]);
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("preferred_curvature_twist"));
}
