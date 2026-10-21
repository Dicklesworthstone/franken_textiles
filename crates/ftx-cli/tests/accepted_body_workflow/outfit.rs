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
    command_for(dir, "outfit.json", out, flags)
}

fn command_for(dir: &Path, pattern: &str, out: &str, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args([
            "drape",
            pattern,
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

fn accessorized() -> Value {
    let mut raw = input();
    for (g, mass) in raw["garments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip([0.0002, 0.0003])
    {
        g["pattern"]["attached_mass"] = json!({"schema":"ftx-attached-mass/1","groups":[{
            "extra":{"id":"closures","mass":{"value":mass,"unit":"kg",
                "evidence":"estimate","source":"test","method":"Synthetic CLI group"},
                "sources":{"test":{"reference":"test:outfit-extra","accessed_on":"2026-10-21",
                    "note":"Synthetic accessory mass; not measured hardware"}},"allow_estimates":true},
            "targets":[{"instance":"band","vertex":"a","fraction":0.25},
                {"instance":"band","vertex":"b","fraction":0.75}]}]});
    }
    raw
}

#[test]
fn joint_accessory_mass_is_published_once_and_replays_through_the_ordinary_drape_command() {
    let dir = joint_setup();
    save(&dir.join("outfit.json"), &accessorized());
    let before = std::fs::read(dir.join("outfit.json")).unwrap();
    let out = command(&dir, "loaded", &["--self-contact-mm", "2"]);
    assert!(
        matches!(out.status.code(), Some(0 | 2)),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let report = read(&dir.join("loaded/sim_report.json"));
    let composed = read(&dir.join("loaded/outfit_pattern.json"));
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["result"]["outfit"], report["outfit"]);
    assert_eq!(report["attached_mass"]["input"], composed["attached_mass"]);
    assert_eq!(
        report["attached_mass"]["groups"][0]["id"],
        "inner::closures"
    );
    assert_eq!(
        report["attached_mass"]["groups"][1]["id"],
        "outer::closures"
    );
    assert_eq!(
        report["outfit"]["garments"][0]["attached_mass"]["groups"]["closures"],
        "inner::closures"
    );
    assert_eq!(
        report["outfit"]["garments"][1]["attached_mass"]["groups"]["closures"],
        "outer::closures"
    );
    assert_eq!(
        report["outfit"]["garment_attached_mass_kg"],
        json!([0.0002, 0.0003])
    );
    let total: f64 = report["outfit"]["garment_mass_kg"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .sum();
    assert!(
        (total
            - report["attached_mass"]["total_inertial_mass_kg"]
                .as_f64()
                .unwrap())
        .abs()
            < 1e-12
    );
    assert_eq!(
        std::fs::read(dir.join("loaded/outfit_request.json")).unwrap(),
        before
    );
    assert_eq!(std::fs::read(dir.join("outfit.json")).unwrap(), before);
    // Replay the saved composed pattern unchanged: identical physical inputs, not another composer.
    let replay = command_for(
        &dir,
        "loaded/outfit_pattern.json",
        "replay",
        &["--self-contact-mm", "2"],
    );
    assert_eq!(
        replay.status.code(),
        out.status.code(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(
        read(&dir.join("replay/sim_report.json"))["attached_mass"],
        report["attached_mass"]
    );
    assert_eq!(
        std::fs::read(dir.join("loaded/fold.pc2")).unwrap(),
        std::fs::read(dir.join("replay/fold.pc2")).unwrap()
    );
    assert_eq!(
        std::fs::read(dir.join("loaded/garment.obj")).unwrap(),
        std::fs::read(dir.join("replay/garment.obj")).unwrap()
    );
    assert!(
        !command(&dir, "loaded", &["--self-contact-mm", "2"])
            .status
            .success()
    );
    assert_eq!(read(&dir.join("loaded/sim_report.json")), report);
}

#[test]
fn invalid_accessories_never_publish_a_joint_success_report() {
    let dir = joint_setup();
    for (i, mutate) in [
        (|raw: &mut Value| raw["garments"][0]["pattern"]["attached_mass"]["groups"][0]["extra"]["allow_estimates"] = json!(false)) as fn(&mut Value),
        |raw| raw["garments"][0]["pattern"]["attached_mass"]["groups"][0]["targets"][0]["instance"] = json!("outer::band"),
        |raw| raw["garments"][0]["pattern"]["attached_mass"]["groups"][0]["extra"]["mass"] = Value::Null,
    ].into_iter().enumerate() {
        let mut raw = accessorized();
        mutate(&mut raw);
        save(&dir.join("outfit.json"), &raw);
        let destination = format!("refused-{i}");
        let out = command(&dir, &destination, &["--self-contact-mm", "2"]);
        assert!(!out.status.success());
        let refusal: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(refusal["ok"], false);
        assert!(!dir.join(destination).exists());
    }
    // A known source target excluded from simulation is not silently reassigned to a retained panel.
    let mut raw = accessorized();
    let pieces = raw["garments"][0]["pattern"]["pieces"]
        .as_array_mut()
        .unwrap();
    let mut retained = pieces[0].clone();
    retained["id"] = json!("retained");
    pieces[0]["sim"] = json!(false);
    pieces.push(retained);
    save(&dir.join("outfit.json"), &raw);
    let out = command(&dir, "unmeshed", &["--self-contact-mm", "2"]);
    assert!(!out.status.success());
    assert!(!dir.join("unmeshed/sim_report.json").exists());
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
