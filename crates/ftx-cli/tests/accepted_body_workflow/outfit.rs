//! Actual joint drape publication using the enclosing synthetic model, never wearer data.
use super::*;

fn input() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-joint-outfit.json"
    ))
    .unwrap()
}

fn joint_setup() -> PathBuf {
    let dir = setup();
    let bytes = model_bytes_with_rig(true);
    save(&dir.join("fit.json"), &receipt(&sha256_hex(&bytes)));
    std::fs::write(dir.join("model.ftxbody"), bytes).unwrap();
    save(
        &dir.join("measurements.json"),
        &json!({"wearer":{"age_months":30,"sex":"F","stature_mm":1111}}),
    );
    save(&dir.join("outfit.json"), &input());
    dir
}

fn command(dir: &Path, out: &str, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args([
            "drape",
            "outfit.json",
            "--measurements",
            "measurements.json",
            "--body-fit",
            "fit.json",
            "--model",
            "model.ftxbody",
            "--sew",
            "3",
            "--drape",
            "3",
            "--spacing-mm",
            "10",
            "--out",
            out,
            "--json",
        ])
        .args(flags)
        .output()
        .unwrap()
}

#[test]
fn joint_drape_publishes_the_actual_contact_run_and_replayable_sources() {
    let dir = joint_setup();
    let before = std::fs::read(dir.join("outfit.json")).unwrap();
    let out = command(&dir, "joint", &["--self-contact-mm", "2"]);
    assert!(
        matches!(out.status.code(), Some(0 | 2)),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    let report = read(&dir.join("joint/sim_report.json"));
    assert_eq!(result["result"]["outfit"], report["outfit"]);
    assert_eq!(report["outfit"]["fit_claim"], false);
    assert_eq!(report["self_contact"]["enabled"], true);
    assert_eq!(report["body"]["shape_source"]["refitted"], false);
    assert_eq!(report["outfit"]["input_file_sha256"], sha256_hex(&before));
    let composed = read(&dir.join("joint/outfit_pattern.json"));
    assert_eq!(
        report["outfit"]["composed_pattern_sha256"],
        sha256_hex(&serde_json::to_vec(&composed).unwrap())
    );
    assert_eq!(composed["pieces"][0]["id"], "inner::band");
    assert_eq!(composed["pieces"][1]["id"], "outer::band");
    assert_eq!(
        composed["pieces"][0]["vertices"],
        input()["garments"][0]["pattern"]["pieces"][0]["vertices"]
    );
    assert_eq!(
        std::fs::read(dir.join("joint/outfit_request.json")).unwrap(),
        before
    );
    assert_eq!(std::fs::read(dir.join("outfit.json")).unwrap(), before);
    let cache = std::fs::read(dir.join("joint/fold.pc2")).unwrap();
    let pc2 = ftx_drape::point_cache::PointCache::read(&cache).unwrap();
    assert_eq!(pc2.points() as u64, report["vertices"].as_u64().unwrap());
    let failed = command(&dir, "joint", &["--self-contact-mm", "2"]);
    assert!(!failed.status.success());
    assert_eq!(read(&dir.join("joint/sim_report.json")), report);
    let repeat = command(&dir, "repeat", &["--self-contact-mm", "2"]);
    assert_eq!(repeat.status.code(), out.status.code());
    assert_eq!(std::fs::read(dir.join("repeat/fold.pc2")).unwrap(), cache);
}

#[test]
fn incomplete_joint_inputs_refuse_before_body_fitting_or_publication() {
    let dir = joint_setup();
    for (name, flags) in [
        ("disabled", vec![]),
        (
            "conflicting_start",
            vec!["--self-contact-mm", "2", "--two-start-sew"],
        ),
    ] {
        let out = command(&dir, name, &flags);
        assert!(!out.status.success());
        assert!(!dir.join(name).exists());
    }
    let mut raw = input();
    raw["garments"][0]["pattern"]["fabric"]["material"]["allow_estimates"] = json!(false);
    save(&dir.join("outfit.json"), &raw);
    let out = command(&dir, "denied", &["--self-contact-mm", "2"]);
    assert!(!out.status.success());
    assert!(!dir.join("denied").exists());
    assert!(!dir.join("denied/sim_report.json").exists());
}
