//! Real-binary rounds over synthetic/public-prior fixtures; no private wearer data.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

fn setup() -> (PathBuf, Value, Value) {
    setup_program("fez-920")
}

fn setup_program(id: &str) -> (PathBuf, Value, Value) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-round-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/oracle/draft/programs.json");
    let fixtures: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap();
    let (m, p) = (
        case["input"]["measurements"].clone(),
        case["output"]["pattern"].clone(),
    );
    save(&dir.join("m.json"), &m);
    save(&dir.join("p.json"), &p);
    save(&dir.join("fit_spec.json"), &case["output"]["fit_spec"]);
    (dir, m, p)
}

fn save(path: &Path, v: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn run(dir: &Path, args: &[&str], code: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "{args:?}\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(envelope["exit"], code);
    envelope
}

const FIRST: &[&str] = &[
    "round",
    "ws",
    "--pattern",
    "p.json",
    "--measurements",
    "m.json",
    "--as-of",
    "2026-10-13",
];

#[test]
fn a_round_exports_checked_files_and_preserves_its_inputs() {
    let (dir, m, p) = setup();
    let mut args = FIRST.to_vec();
    args.extend([
        "--export",
        "--created",
        "2026-10-13T00:00",
        "--note",
        "first fitting",
    ]);
    let r = run(&dir, &args, 0)["result"].clone();
    assert_eq!(r["status"], "CONVERGED");
    assert_eq!(r["basis"]["measurements"], "READY_REFERENCE");
    assert_eq!(r["basis"]["sim"], "not run");
    assert_eq!(r["exports"]["pass"], true);
    assert_eq!(r["note"], "first fitting");
    let rd = dir.join("ws/rounds/000");
    assert_eq!(read(&rd.join("pattern.json")), p);
    assert_eq!(read(&rd.join("measurements.source.json")), m);
    assert_eq!(read(&rd.join("round.json")), r);
    assert_eq!(read(&rd.join("exports/checks.json"))["status"], "PASS");
    let log = std::fs::read_to_string(dir.join("ws/rounds.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 1);
    assert_eq!(serde_json::from_str::<Value>(&log).unwrap(), r);
}

#[test]
fn repeats_allocate_after_the_highest_round_and_skip_incomplete_drafts() {
    let (dir, _, _) = setup();
    run(&dir, FIRST, 0);
    let prior = std::fs::read(dir.join("ws/rounds/000/round.json")).unwrap();
    std::fs::create_dir(dir.join("ws/rounds/002")).unwrap();
    let r = run(&dir, &["round", "ws", "--as-of", "2026-10-13"], 0);
    assert_eq!(r["result"]["round"], 3);
    assert_eq!(
        std::fs::read(dir.join("ws/rounds/000/round.json")).unwrap(),
        prior
    );
    assert_eq!(
        read(&dir.join("ws/rounds/003/pattern.json")),
        read(&dir.join("p.json"))
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("ws/rounds.jsonl"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn body_changes_require_explicit_adoption_and_keep_collision_safe_backups() {
    let (dir, mut m, _) = setup();
    run(&dir, FIRST, 0);
    let old = std::fs::read(dir.join("ws/measurements.json")).unwrap();
    m["wearer"]["label"] = json!("another synthetic reference");
    save(&dir.join("new.json"), &m);
    let args = [
        "round",
        "ws",
        "--measurements",
        "new.json",
        "--as-of",
        "2026-10-13",
    ];
    let r = run(&dir, &args, 3);
    assert!(
        r["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("one body")
    );
    assert!(!dir.join("ws/rounds/001").exists());
    std::fs::write(
        dir.join("ws/measurements.before-round-001.json"),
        b"previous backup",
    )
    .unwrap();
    let mut update = args.to_vec();
    update.push("--update-measurements");
    run(&dir, &update, 0);
    assert_eq!(
        std::fs::read(dir.join("ws/measurements.before-round-001.001.json")).unwrap(),
        old
    );
    assert_eq!(
        std::fs::read(dir.join("ws/measurements.before-round-001.json")).unwrap(),
        b"previous backup"
    );
    assert_eq!(read(&dir.join("ws/measurements.json")), m);
    assert_eq!(
        std::fs::read(dir.join("ws/rounds/000/measurements.source.json")).unwrap(),
        old
    );
}

#[test]
fn priors_cannot_pass_as_measured_evidence_and_stale_fused_values_are_ignored() {
    let (dir, mut m, _) = setup();
    m["wearer"]["reference"] = json!(false);
    m["fused"]["head"]["value_mm"] = json!(9999);
    save(&dir.join("m.json"), &m);
    let r = run(&dir, FIRST, 2);
    assert_eq!(r["result"]["status"], "MEASUREMENTS_NOT_READY");
    assert_eq!(r["result"]["basis"]["flat"], "CONVERGED");
    assert!(
        r["result"]["readiness"]["blocking"]
            .as_array()
            .unwrap()
            .contains(&json!("head"))
    );
    assert!(
        read(&dir.join("ws/rounds/000/measurements.json"))["fused"]["head"]["value_mm"]
            .as_f64()
            .unwrap()
            < 1000.0
    );
}

#[test]
fn unscaled_or_blocked_patterns_never_get_true_size_exports() {
    for blocked in [false, true] {
        let (dir, _, mut p) = setup();
        p["meta"]["scale_status"] = json!("UNSCALED");
        if blocked {
            p["schema"] = json!("broken");
        }
        save(&dir.join("p.json"), &p);
        let mut args = FIRST.to_vec();
        args.push("--export");
        let r = run(&dir, &args, 2);
        assert_eq!(
            r["result"]["status"],
            if blocked { "BLOCKED" } else { "UNSCALED" }
        );
        assert!(r["result"]["exports"].is_null());
        assert!(!dir.join("ws/rounds/000/exports").exists());
    }
}

#[test]
fn preflight_refuses_bad_inputs_without_changing_the_workspace() {
    let (dir, mut m, _) = setup();
    run(&dir, &["round", "ws"], 3);
    assert!(!dir.join("ws").exists());
    m["records"] = json!("bad evidence");
    save(&dir.join("m.json"), &m);
    run(&dir, FIRST, 3);
    assert!(!dir.join("ws").exists());
}

#[test]
fn non_simulated_hat_and_invalid_patterns_do_not_load_a_body() {
    let (dir, _, _) = setup();
    let mut args = FIRST.to_vec();
    args.extend(["--sim", "--model", "missing.ftxbody"]);
    let r = run(&dir, &args, 0);
    assert!(
        r["result"]["basis"]["sim"]
            .as_str()
            .unwrap()
            .starts_with("not applicable")
    );
    assert!(r["result"]["sim"].is_null());
    assert!(!dir.join("ws/rounds/000/sim").exists());
    for blocked in [false, true] {
        let (dir, _, mut p) = setup_program("zouave_bolero-920");
        p["meta"]["scale_status"] = json!("UNSCALED");
        if blocked {
            p["schema"] = json!("broken");
        }
        save(&dir.join("p.json"), &p);
        let r = run(&dir, &args, 2);
        assert!(
            r["result"]["basis"]["sim"]
                .as_str()
                .unwrap()
                .starts_with("skipped")
        );
        assert!(r["result"]["sim"].is_null());
        assert!(!dir.join("ws/rounds/000/sim").exists());
    }
}

#[test]
fn failed_simulation_cannot_reuse_an_earlier_success() {
    let (dir, _, _) = setup_program("zouave_bolero-920");
    run(&dir, FIRST, 0);
    let old = dir.join("ws/rounds/000/sim");
    std::fs::create_dir(&old).unwrap();
    save(
        &old.join("sim_report.json"),
        &json!({"gating":[], "settled":true}),
    );
    std::fs::write(old.join("fold_viewer.html"), "old successful viewer").unwrap();
    let r = run(
        &dir,
        &[
            "round",
            "ws",
            "--sim",
            "--model",
            "missing.ftxbody",
            "--as-of",
            "2026-10-13",
        ],
        2,
    );
    let r = &r["result"];
    assert_eq!(r["status"], "SIM_GATING_FLAGS");
    assert_eq!(r["sim"]["settled"], false);
    assert!(r["sim"]["refusal"].is_object());
    assert!(r["viewer"].is_null());
    assert!(
        r["sim"]["gating"][0]
            .as_str()
            .unwrap()
            .contains("missing.ftxbody")
    );
    assert_eq!(read(&old.join("sim_report.json"))["settled"], true);
    assert_eq!(read(&dir.join("ws/rounds/001/sim_failure.json")), r["sim"]);
    assert!(!dir.join("ws/rounds/001/sim/fold_viewer.html").exists());
}

#[test]
fn simulation_options_are_checked_before_workspace_mutation() {
    for options in [
        vec!["--self-mu", "0.3"],
        vec!["--model", "model.ftxbody"],
        vec!["--sim", "--self-contact-mm", "NaN"],
        vec!["--sim", "--self-mu", "0.3"],
        vec!["--sim", "--sew", "0", "--drape", "0"],
    ] {
        let (dir, _, _) = setup();
        let mut args = FIRST.to_vec();
        args.extend(options);
        run(&dir, &args, 3);
        assert!(!dir.join("ws").exists());
    }
}

#[test]
fn stills_imply_simulation_but_do_not_reuse_old_images_on_skip_or_failure() {
    for (program, exit) in [("fez-920", 0), ("zouave_bolero-920", 2)] {
        let (dir, _, _) = setup_program(program);
        let mut args = FIRST.to_vec();
        args.extend([
            "--stills",
            "--arm-deg",
            "0",
            "--res",
            "48x32",
            "--model",
            "missing.ftxbody",
        ]);
        let r = run(&dir, &args, exit)["result"].clone();
        assert_eq!(r["rendering"]["status"], "SKIPPED");
        assert_eq!(r["images"], json!([]));
        assert!(r["video"].is_null());
        assert!(!dir.join("ws/rounds/000/sim/still_front.png").exists());
        if exit == 2 {
            assert!(
                r["sim"]["refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("missing.ftxbody")
            );
        } else {
            assert!(r["sim"].is_null());
        }
    }
}

#[test]
fn pose_and_still_options_are_validated_before_reserving_a_round() {
    for (options, exit) in [
        (vec!["--arm-deg", "50"], 3),
        (vec!["--sim", "--arm-deg", "NaN"], 3),
        (vec!["--stills", "--arm-deg", "181"], 3),
        (vec!["--stills", "--res", "48X32"], 3),
        (vec!["--stills", "--res", "0x32"], 3),
        (vec!["--res", "48x32"], 3),
        (vec!["--stills", "--res", "8192x8192"], 4),
    ] {
        let (dir, _, _) = setup();
        let mut args = FIRST.to_vec();
        args.extend(options);
        run(&dir, &args, exit);
        assert!(!dir.join("ws").exists());
    }
    let (dir, _, _) = setup();
    let refusal = run(&dir, &["render", "missing", "--res", "8192x8192"], 4);
    assert_eq!(refusal["refusal"]["code"], "budget_exceeded");
}

#[test]
fn report_rebuilds_review_html_without_adding_a_round_and_qualify_validates_inputs() {
    let (dir, _, _) = setup();
    run(&dir, &["report", "missing_ws"], 3);
    run(&dir, FIRST, 0);
    std::fs::write(dir.join("ws/report.html"), "stale placeholder").unwrap();
    let rep = run(&dir, &["report", "ws"], 0)["result"].clone();
    assert_eq!(rep["rounds"], 1);
    let html = std::fs::read_to_string(dir.join("ws/report.html")).unwrap();
    assert!(html.contains("Construction review"));
    assert!(html.contains("Round 0 — CONVERGED"));
    assert_eq!(
        std::fs::read_to_string(dir.join("ws/rounds.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );

    // ftx qualify preflight checks options and workspace arguments before loading a body.
    run(&dir, &["qualify"], 3);
    run(&dir, &["qualify", "ws", "--ws", "ws"], 3);
    run(&dir, &["qualify", "--ws", "ws", "--out", "custom"], 3);
    run(&dir, &["qualify", "--ws", "ws", "--arm-deg", "200"], 3);
    run(&dir, &["qualify", "--ws", "ws", "--legs", "unknown"], 3);
    run(&dir, &["qualify", "--ws", "ws", "--leg-fraction", "2"], 3);
    assert!(!dir.join("ws/qualification").exists());
}

#[test]
#[ignore = "needs the converted Anny asset"]
fn qualify_runs_three_chest_bands_and_writes_qualify_json() {
    let (dir, _, _) = setup_program("zouave_bolero-920");
    run(&dir, FIRST, 0);
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args(["qualify", "--ws", "ws", "--json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env: Value = serde_json::from_slice(&out.stdout).unwrap();
    let q = &env["result"];
    assert_eq!(q["schema"], "fabric-qualify/1");
    assert_eq!(q["engine"], "ftx");
    assert_eq!(q["pass"], true, "{q}");
    // The chart target selects the fit. The posed collider supplies the band dimensions.
    let target = q["input_chest_mm"].as_f64().unwrap();
    let girth = q["posed_chest_girth_mm"].as_f64().unwrap();
    assert_eq!(target, 540.0);
    assert!(girth > 20.0 && (girth - target).abs() > 0.01, "{q}");
    let rows = q["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for (row, ease) in rows.iter().zip([-20, 0, 20]) {
        assert_eq!(row["ease_mm"], f64::from(ease));
        assert_eq!(row["pass"], true, "{row}");
        let band = dir.join(format!("ws/qualification/band_{ease:+}"));
        let pattern = read(&band.join("pattern.json"));
        let girth_of_band: f64 = pattern["pieces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                p["vertices"]["b"][0].as_f64().unwrap() - p["vertices"]["a"][0].as_f64().unwrap()
            })
            .sum();
        assert!((girth_of_band - (girth + f64::from(ease))).abs() <= 0.01);
        assert!(band.join("sim/sim_report.json").is_file());
    }
    let measurements = std::fs::read(dir.join("ws/measurements.json")).unwrap();
    assert_eq!(
        q["measurements_sha256"],
        ftx_core::receipt::sha256_hex(&measurements)
    );
    assert!(dir.join("ws/qualification/qualify.json").is_file());
    // the next round on this body and fabric finds the qualification current
    let round = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args([
            "round", "ws", "--sim", "--sew", "40", "--drape", "0", "--json",
        ])
        .output()
        .unwrap();
    assert!(
        matches!(round.status.code(), Some(0 | 2)),
        "{}",
        String::from_utf8_lossy(&round.stderr)
    );
    let sim: Value = serde_json::from_slice(
        &std::fs::read(dir.join("ws/rounds/001/sim/sim_report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(sim["tightness_qualified"], q["pass"]);
    assert_eq!(sim["qualification_basis"], q["qualification_basis"]);
    for (number, arm_deg, legacy) in [(2, "55", false), (3, "35", true)] {
        if legacy {
            let mut old = q.clone();
            old.as_object_mut().unwrap().remove("qualification_basis");
            save(&dir.join("ws/qualification/qualify.json"), &old);
        }
        let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&dir)
            .args([
                "round",
                "ws",
                "--sim",
                "--sew",
                "6",
                "--drape",
                "0",
                "--arm-deg",
                arm_deg,
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            matches!(output.status.code(), Some(0 | 2)),
            "{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        let report = read(&dir.join(format!("ws/rounds/{number:03}/sim/sim_report.json")));
        assert_eq!(report["tightness_qualified"], false, "{report}");
        assert_eq!(
            report["qualification_basis"]["material_sha256"],
            q["qualification_basis"]["material_sha256"]
        );
        assert_eq!(
            report["qualification_basis"]["body_sha256"] == q["qualification_basis"]["body_sha256"],
            legacy
        );
    }
    save(&dir.join("ws/qualification/qualify.json"), q);
    let mut changed_pattern = read(&dir.join("p.json"));
    let acquisition: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-friction-sliding.json"
    ))
    .unwrap();
    changed_pattern["fabric"]["material"] = acquisition["target"]["materials"][0].clone();
    for key in ["areal_g_m2", "tensile_N_m", "bending_Nm"] {
        changed_pattern["fabric"]
            .as_object_mut()
            .unwrap()
            .remove(key);
    }
    assert_ne!(
        ftx_drape::qualify::fabric_key(changed_pattern.get("fabric")),
        q["fabric_key"]
    );
    save(&dir.join("changed_material.json"), &changed_pattern);
    let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args([
            "round",
            "ws",
            "--pattern",
            "changed_material.json",
            "--sim",
            "--sew",
            "6",
            "--drape",
            "0",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 2)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let report = read(&dir.join("ws/rounds/004/sim/sim_report.json"));
    assert_eq!(report["tightness_qualified"], false);
    assert_eq!(
        report["qualification_basis"]["body_sha256"],
        q["qualification_basis"]["body_sha256"]
    );
    assert_ne!(
        report["qualification_basis"]["material_sha256"],
        q["qualification_basis"]["material_sha256"]
    );
    // Re-running qualify on the same workspace rotates the previous qualification directory.
    let out2 = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args(["qualify", "--ws", "ws", "--pattern", "p.json", "--json"])
        .output()
        .unwrap();
    assert!(
        out2.status.success(),
        "{}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(
        dir.join("ws/qualification.before-000/qualify.json")
            .is_file()
    );
    let seated = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args(["qualify", "--ws", "ws", "--pattern", "p.json", "--json"])
        .args(["--legs", "seated", "--leg-fraction", "0.5"])
        .output()
        .unwrap();
    assert!(
        matches!(seated.status.code(), Some(0 | 2)),
        "{}",
        String::from_utf8_lossy(&seated.stderr)
    );
    let envelope: Value = serde_json::from_slice(&seated.stdout).unwrap();
    let seated_q = &envelope["result"];
    assert_ne!(
        seated_q["qualification_basis"]["body_sha256"],
        q["qualification_basis"]["body_sha256"]
    );
    let round = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args(["round", "ws", "--pattern", "p.json", "--sim", "--json"])
        .args(["--sew", "6", "--drape", "0"])
        .args(["--legs", "seated", "--leg-fraction", "0.5"])
        .output()
        .unwrap();
    assert!(
        matches!(round.status.code(), Some(0 | 2)),
        "{}\n{}",
        String::from_utf8_lossy(&round.stderr),
        String::from_utf8_lossy(&round.stdout)
    );
    let report = read(&dir.join("ws/rounds/005/sim/sim_report.json"));
    assert_eq!(report["body"]["legs"], "seated");
    assert_eq!(
        report["qualification_basis"],
        seated_q["qualification_basis"]
    );
    assert_eq!(report["tightness_qualified"], seated_q["pass"]);
}
