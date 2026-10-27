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

#[test]
fn known_plastic_equilibrium_calibrates_and_runs_a_locked_parallel_strip() {
    let w = Workspace::new();
    let source: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-parallel-membrane.json"
    ))
    .unwrap();
    let mut material = source["record"].clone();
    let network = material["membrane_relaxation"]["parallel_plasticity"].clone();
    material
        .as_object_mut()
        .unwrap()
        .remove("membrane_relaxation");

    // Independent one-dimensional return map and exponential convolution, not output from
    // either production history solver. The unobserved peak must still leave permanent set.
    let acquire = |id: &str, partition: &str, path: &[(f64, f64, bool)]| {
        let (mut plastic, mut accumulated, mut lag) = (0.0_f64, 0.0_f64, 0.0_f64);
        let (mut previous_time, mut previous_strain) = (0.0, 0.0);
        let mut samples = Vec::new();
        let mut permanent = Vec::new();
        for &(time, prescribed_strain, observed) in path {
            let stretch = (1.0 + 2.0 * prescribed_strain).sqrt();
            let strain = 0.5 * (stretch * stretch - 1.0);
            if time > 0.0 {
                let h = time - previous_time;
                let decay = (-h / 0.05).exp();
                lag = decay * lag + 0.05 * (1.0 - decay) / h * (strain - previous_strain);
            }
            let trial = 1000.0 * (strain - plastic);
            let excess = trial.abs() - (20.0 + 100.0 * accumulated);
            if excess > 0.0 {
                let increment = excess / 1100.0;
                plastic += trial.signum() * increment;
                accumulated += increment;
            }
            let force = stretch * (1000.0 * (strain - plastic) + 2000.0 * lag);
            let nominal = if observed {
                json!([[force, 0.0], [0.0, 0.0]])
            } else {
                json!([[null, null], [null, null]])
            };
            samples.push(
                json!({"time_seconds":time,"deformation":[[stretch,0.0],[0.0,1.0]],
                "nominal_n_m":nominal,"scale_n_m":1.0}),
            );
            permanent.push(plastic);
            previous_time = time;
            previous_strain = strain;
        }
        (
            json!({"id":id,"source":"test","evidence":"estimate","partition":partition,
                "initialization":"stress_free_reference","samples":samples}),
            permanent,
        )
    };
    let (fit_history, fit_permanent) = acquire(
        "fit-cycle",
        "fit",
        &[
            (0.0, 0.0, true),
            (0.015, 0.012, true),
            (0.03, 0.08, false),
            (0.06, 0.045, true),
            (0.1, 0.045, true),
            (0.15, 0.0, true),
            (0.3, 0.0, true),
            (0.4, 0.025, true),
            (0.6, 0.025, true),
        ],
    );
    let (check_history, check_permanent) = acquire(
        "held-out-cycle",
        "check",
        &[
            (0.0, 0.0, true),
            (0.025, 0.03, true),
            (0.08, 0.07, true),
            (0.12, 0.07, true),
            (0.2, 0.015, true),
            (0.4, 0.0, true),
            (0.6, 0.0, true),
        ],
    );
    let input: Value = serde_json::from_str(include_str!(
        "../../../examples/relaxation-plastic-equilibrium.json"
    ))
    .unwrap();
    assert_eq!(input["material"]["properties"], material["properties"]);
    assert!(input["material"].get("membrane_relaxation").is_none());
    assert_eq!(input["plastic_equilibrium"]["network"], network);
    assert_eq!(input["experiments"].as_array().unwrap().len(), 2);
    for (observed, independent) in input["experiments"]
        .as_array()
        .unwrap()
        .iter()
        .zip([fit_history, check_history])
    {
        for field in ["id", "partition", "source", "evidence", "initialization"] {
            assert_eq!(observed[field], independent[field]);
        }
        let samples = observed["samples"].as_array().unwrap();
        let expected = independent["samples"].as_array().unwrap();
        assert_eq!(samples.len(), expected.len());
        for (sample, expected) in samples.iter().zip(expected) {
            for field in ["time_seconds", "scale_n_m", "deformation"] {
                assert_eq!(sample[field], expected[field]);
            }
            for row in 0..2 {
                for column in 0..2 {
                    let observed = &sample["nominal_n_m"][row][column];
                    let expected = &expected["nominal_n_m"][row][column];
                    if let Some(force) = expected.as_f64() {
                        assert!(
                            (observed.as_f64().unwrap() - force).abs()
                                < 1e-12 * force.abs().max(1.0)
                        );
                    } else {
                        assert!(expected.is_null() && observed.is_null());
                    }
                }
            }
        }
    }
    w.write("histories.json", &input);

    let fitted = w.ok(FIT);
    assert_eq!(fitted["simulation_ready"], true, "{fitted}");
    let portable: Value = serde_json::from_str(
        &ftx_wasm::materials::calibrate_relaxation(&input.to_string(), true).unwrap(),
    )
    .unwrap();
    for field in [
        "calibration",
        "selection",
        "selection_json",
        "time_integration",
    ] {
        assert_eq!(fitted[field], portable[field], "{field}");
    }
    assert!(
        fitted["time_integration"]
            .as_str()
            .unwrap()
            .contains("every recorded node")
    );
    assert_eq!(
        fitted["calibration"]["material"]["properties"],
        fitted["calibration"]["input"]["material"]["properties"]
    );
    assert_eq!(
        fitted["selection"]["record"]["membrane_relaxation"]["parallel_plasticity"],
        input["plastic_equilibrium"]["network"]
    );
    let branch = &fitted["selection"]["record"]["membrane_relaxation"]["branches"][0];
    assert!((branch["stiffness_ratio"]["value"].as_f64().unwrap() - 2.0).abs() < 1e-6);
    assert!((branch["relaxation_time"]["value"].as_f64().unwrap() - 0.05).abs() < 1e-7);
    let predictions = fitted["calibration"]["predictions"].as_array().unwrap();
    let permanent: Vec<_> = fit_permanent
        .iter()
        .chain(&check_permanent)
        .copied()
        .collect();
    assert_eq!(predictions.len(), permanent.len());
    for (prediction, expected) in predictions.iter().zip(permanent) {
        assert!(
            (prediction["plastic_equilibrium"]["plastic_strain"][0]
                .as_f64()
                .unwrap()
                - expected)
                .abs()
                < 1e-12
        );
    }
    assert_eq!(
        predictions[2]["residual_n_m"],
        json!([[null, null], [null, null]])
    );
    assert!(fit_permanent[2] > 0.05);
    assert_eq!(check_permanent[0], 0.0);
    assert!(
        fitted["calibration"]["check"]["peak_scaled"]
            .as_f64()
            .unwrap()
            < 1e-4
    );

    let original = pattern();
    w.write("pattern.json", &original);
    fs::write(
        w.0.join("selection.json"),
        fitted["selection_json"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(
        w.ok(&["bind", "pattern.json", "selection.json", "bound.json"])["geometry_changed"],
        false
    );
    let bound: Value = serde_json::from_slice(&fs::read(w.0.join("bound.json")).unwrap()).unwrap();
    assert_eq!(bound["pieces"], original["pieces"]);
    assert_eq!(bound["seams"], original["seams"]);
    let strip = json!({"schema":"ftx-strip-history/1","material":bound["fabric"]["material"],
        "fixture":{"gauge_length_m":0.1,"width_m":0.06,"warp_axis":[0.0,1.0],"cells":[2,2]},
        "loading":[{"time_s":0.0,"extension_fraction":0.0},
            {"time_s":0.05,"extension_fraction":0.02},{"time_s":0.15,"extension_fraction":0.02},
            {"time_s":0.2,"extension_fraction":0.0},{"time_s":0.25,"extension_fraction":0.0}],
        "controls":{"max_step_s":0.01,"max_steps":32,"max_newton":100,
            "velocity_tolerance_m_s":1e-9,"force_tolerance_n":1e-6}});
    w.write("strip.json", &strip);
    let actual: Value =
        serde_json::from_str(&ftx_wasm::materials::strip_history(&strip.to_string()).unwrap())
            .unwrap();
    let last = actual["states"].as_array().unwrap().last().unwrap();
    assert!(last["maxwell_dissipation_j"].as_f64().unwrap() > 0.0);
    assert!(last["plastic_dissipation_j"].as_f64().unwrap() > 0.0);
    assert!(last["relaxation"].is_object() && last["plasticity"].is_object());
    assert_eq!(actual["input"]["material"], fitted["selection"]);
    assert_eq!(w.ok(&["strip-history", "strip.json"]), actual);
    for (args, expected) in [
        (FIT, &fitted),
        (&["strip-history", "strip.json"][..], &actual),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&w.0)
            .arg("materials")
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"],
            *expected
        );
    }

    let mut unqualified = input.clone();
    unqualified
        .as_object_mut()
        .unwrap()
        .remove("plastic_equilibrium");
    let mut denied_network = input.clone();
    denied_network["plastic_equilibrium"]["allow_estimated_network"] = json!(false);
    let mut failed_check = input;
    failed_check["experiments"][1]["samples"][2]["nominal_n_m"][0][0] = json!(50000.0);
    for rejected in [unqualified, denied_network, failed_check] {
        assert!(ftx_wasm::materials::calibrate_relaxation(&rejected.to_string(), true).is_err());
        w.write("rejected-histories.json", &rejected);
        let args = [
            "calibrate-relaxation",
            "rejected-histories.json",
            "--allow-estimates",
        ];
        let output = w.run(&args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&w.0)
            .arg("materials")
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
}
