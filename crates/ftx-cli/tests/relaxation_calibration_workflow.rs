//! Timed acquisitions through the real workbench binary, locked selection and actual drape resolver.
//! Every specimen and observation is synthetic; this is not independent textile characterization.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{Value, json};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ftx-relaxation-calibration-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, value: &Value) {
        fs::write(self.0.join(path), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation-history.json"
    ))
    .unwrap()
}
fn pattern() -> Value {
    json!({"schema":"fabric-pattern/1","units":"mm","seams":[],
        "fabric":{"areal_g_m2":200,"note":"preserve unrelated metadata"},
        "pieces":[{"id":"swatch","material":"shell","cut":{"qty":1},
            "vertices":{"a":[0,0],"b":[100,0],"c":[0,100]},
            "edges":[{"id":"ab","from":"a","to":"b"},{"id":"bc","from":"b","to":"c"},
                {"id":"ca","from":"c","to":"a"}],"grain":[[0,0],[0,100]]}]})
}
const FIT: &[&str] = &[
    "calibrate-relaxation",
    "histories.json",
    "--allow-estimates",
];

#[test]
fn acquisition_to_locked_selection_to_pattern_keeps_geometry_and_estimate_policy() {
    let w = Workspace::new();
    w.write("histories.json", &fixture());
    let denied = w.ok(&["calibrate-relaxation", "histories.json"]);
    assert_eq!(denied["simulation_ready"], false);
    assert_eq!(denied["selection"]["allow_estimates"], false);
    assert!(denied["mechanics_refusal"].is_object());
    let fitted = w.ok(FIT);
    let primary = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&w.0)
        .arg("materials")
        .args(FIT)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        primary.status.success(),
        "{}",
        String::from_utf8_lossy(&primary.stderr)
    );
    let primary: Value = serde_json::from_slice(&primary.stdout).unwrap();
    assert_eq!(primary["result"], fitted);
    assert_eq!(fitted["simulation_ready"], true);
    assert_eq!(fitted["fit_claim"], false);
    assert_eq!(fitted["empirical_validation_claim"], false);
    for (i, (ratio, time)) in [(1.7, 0.3), (0.65, 8.)].into_iter().enumerate() {
        let b = &fitted["selection"]["record"]["membrane_relaxation"]["branches"][i];
        assert!((b["stiffness_ratio"]["value"].as_f64().unwrap() - ratio).abs() < 1e-6);
        assert!((b["relaxation_time"]["value"].as_f64().unwrap() - time).abs() < 1e-6);
        assert_eq!(b["relaxation_time"]["evidence"], "estimate");
    }
    assert!(
        fitted["calibration"]["check"]["rms_scaled"]
            .as_f64()
            .unwrap()
            < 1e-6
    );
    assert_eq!(
        fitted["calibration"]["predictions"]
            .as_array()
            .unwrap()
            .len(),
        26
    );
    assert_eq!(fitted, w.ok(FIT));
    let original = pattern();
    w.write("pattern.json", &original);
    fs::write(
        w.0.join("selection.json"),
        fitted["selection_json"].as_str().unwrap(),
    )
    .unwrap();
    let bound = w.ok(&["bind", "pattern.json", "selection.json", "bound.json"]);
    assert_eq!(bound["geometry_changed"], false);
    let bytes = fs::read(w.0.join("bound.json")).unwrap();
    let saved: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved["pieces"], original["pieces"]);
    assert_eq!(saved["seams"], original["seams"]);
    assert_eq!(saved["fabric"]["note"], original["fabric"]["note"]);
    let physical = w.ok(&["resolve", "bound.json"]);
    assert_eq!(
        physical["record_sha256"],
        fitted["selection"]["expected_sha256"]
    );
    assert!(
        !w.run(&["bind", "pattern.json", "selection.json", "bound.json"])
            .status
            .success()
    );
    assert_eq!(bytes, fs::read(w.0.join("bound.json")).unwrap());

    // Changing the fitted time without changing the lock cannot quietly enter the real consumer.
    let mut altered = saved;
    altered["fabric"]["material"]["record"]["membrane_relaxation"]["branches"][0]["relaxation_time"]
        ["value"] = json!(20.);
    w.write("altered.json", &altered);
    assert!(!w.run(&["resolve", "altered.json"]).status.success());
    // Keep outputs: the repository forbids file deletion without express permission.
}

#[test]
fn held_out_failure_and_unknown_permission_do_not_publish_partial_results() {
    let w = Workspace::new();
    let mut raw = fixture();
    raw["experiments"][1]["samples"][8]["nominal_n_m"][0][0] = json!(50000.);
    w.write("histories.json", &raw);
    let out = w.run(FIT);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("held-out"));
    let out = w.run(&["calibrate-relaxation", "histories.json", "--allow-anything"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&w.0)
        .args([
            "materials",
            "calibrate-relaxation",
            "histories.json",
            "--allow-anything",
            "true",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
}

#[test]
fn successful_identification_does_not_invent_missing_simulation_properties() {
    let w = Workspace::new();
    let mut raw = fixture();
    raw["material"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("thickness");
    w.write("histories.json", &raw);
    let out = w.ok(FIT);
    assert_eq!(out["simulation_ready"], false);
    assert_eq!(
        out["calibration"]["missing_mechanics"],
        json!(["thickness"])
    );
    assert!(
        out["selection"]["record"]["properties"]
            .get("thickness")
            .is_none()
    );
    assert!(
        out["mechanics_refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("thickness")
    );
}

#[test]
fn binding_checks_transient_estimates_even_when_equilibrium_evidence_is_admitted() {
    let w = Workspace::new();
    let mut raw = fixture();
    // Synthetic policy test: only the fitted relaxation needs consent, not the equilibrium bundle.
    for q in raw["material"]["properties"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        q["evidence"] = json!("published");
    }
    raw["allow_estimated_equilibrium"] = json!(false);
    w.write("histories.json", &raw);
    let out = w.ok(&["calibrate-relaxation", "histories.json"]);
    assert_eq!(out["simulation_ready"], false);
    w.write("selection.json", &out["selection"]);
    w.write("pattern.json", &pattern());
    let bound = w.ok(&["bind", "pattern.json", "selection.json", "bound.json"]);
    assert_eq!(bound["simulation_ready"], false);
    assert!(!w.run(&["resolve", "bound.json"]).status.success());
    // Explicit simulation admission still works, with the identical fitted parameters.
    let admitted = w.ok(FIT);
    assert_eq!(admitted["simulation_ready"], true);
    assert_eq!(out["calibration"], admitted["calibration"]);
}

#[test]
fn competing_recovery_models_require_a_reviewed_choice_before_actual_binding() {
    let w = Workspace::new();
    let mut calibration: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-orthotropic-relaxation-history.json"
    ))
    .unwrap();
    calibration.as_object_mut().unwrap().remove("orthotropic");
    let mut request = json!({"schema":"ftx-relaxation-model-comparison/1", "calibration":calibration,
    "fit_rms_equivalence_scaled":1e-7,"candidates":[
        {"id":"complete","orthotropic":{"times_seconds":[0.3,2,8],"zero_threshold_n_m":1e-7}},
        {"id":"extra","orthotropic":{"times_seconds":[0.3,2,8,32],"zero_threshold_n_m":1e-7}}
    ]});
    w.write("histories.json", &request);
    let unresolved = w.ok(FIT);
    assert_eq!(unresolved["comparison"]["status"], "AMBIGUOUS");
    assert!(unresolved["calibration"].is_null());
    assert!(unresolved["selection_json"].is_null());
    assert_eq!(unresolved["simulation_ready"], false);
    assert_eq!(
        unresolved["comparison"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    request["select_candidate_id"] = json!("complete");
    w.write("histories.json", &request);
    let selected = w.ok(FIT);
    assert_eq!(selected["comparison"]["status"], "ACCEPTED");
    assert_eq!(selected["comparison"]["fit_decision"]["ambiguous"], true);
    assert_eq!(selected["simulation_ready"], true);
    let primary = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&w.0)
        .arg("materials")
        .args(FIT)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        primary.status.success(),
        "{}",
        String::from_utf8_lossy(&primary.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&primary.stdout).unwrap()["result"],
        selected
    );
    fs::write(
        w.0.join("selection.json"),
        selected["selection_json"].as_str().unwrap(),
    )
    .unwrap();
    let original = pattern();
    w.write("pattern.json", &original);
    let bound = w.ok(&["bind", "pattern.json", "selection.json", "bound.json"]);
    assert_eq!(bound["geometry_changed"], false);
    let saved: Value = serde_json::from_slice(&fs::read(w.0.join("bound.json")).unwrap()).unwrap();
    assert_eq!(saved["pieces"], original["pieces"]);
    let resolved = w.ok(&["resolve", "bound.json"]);
    assert_eq!(
        resolved["record_sha256"],
        selected["selection"]["expected_sha256"]
    );

    request["calibration"]["experiments"][3]["samples"][1]["nominal_n_m"][0][0] = json!(50000.);
    w.write("histories.json", &request);
    let rejected = w.ok(FIT);
    assert_eq!(
        rejected["comparison"]["status"],
        "CHOSEN_CANDIDATE_REJECTED"
    );
    assert_eq!(
        rejected["comparison"]["fit_decision"],
        selected["comparison"]["fit_decision"]
    );
    assert!(rejected["calibration"].is_null());
    assert!(rejected["selection_json"].is_null());
}
