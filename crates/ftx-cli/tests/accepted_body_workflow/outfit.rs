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

fn study_inputs() -> (Value, Value, Value) {
    let mut raw = accessorized();
    for garment in raw["garments"].as_array_mut().unwrap() {
        garment["pattern"]["fabric"]["material"]["record"]["properties"]["specific_heat"] = json!({
            "value":1000.0,"unit":"J/(kg*K)","source":"test","evidence":"estimate",
            "method":"Synthetic outfit heat storage, not a measured textile"});
    }
    let base = raw["garments"][0]["pattern"]["fabric"]["material"].clone();
    let mut thermal = base.clone();
    thermal["record"]["properties"]["thermal_conductivity"]["value"] = json!(0.1);
    let request = json!({"schema":"ftx-material-study/1", "variants":[
        {"key":"base","changes":{"schema":"ftx-material-bindings/1"}},
        {"key":"heat","changes":{"schema":"ftx-material-bindings/1",
            "pieces":{"inner::band":thermal,"outer::band":thermal}}},
        {"key":"unqualified-pair","changes":{"schema":"ftx-material-bindings/1",
            "pieces":{"outer::band":thermal}}}],
        "cutting":{"copies":1,"gap_mm":1.0,"edge_margin_mm":0.0,"end_margin_mm":0.0,"seam_allowance_mm":0.0}});
    let mut scenario: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario.json"
    ))
    .unwrap();
    scenario["options"]["spacing"] = json!(10.0);
    scenario["options"]["self_contact"] =
        serde_json::to_value(ftx_drape::run::DrapeOptions::default().contact).unwrap();
    scenario["coverage"] = Value::Null; // No body patch or interlayer thermal topology is guessed.
    let film = scenario["surface"]["boundaries"][0].clone();
    scenario["motion"] = json!({"schema":"ftx-drape-motion/1",
        "keyframes":[{"time_s":0.002,"arm_abduction_deg":35.1}],
        "max_step_s":0.001,"max_trials":8,"max_pairs":100000,"max_sample_vertices":10000,
        "transient_surface":{"schema":"ftx-motion-surface-heat/1",
            "initial_temperature_c":15.0,"cells_per_layer":1,"boundaries":[film],
            "max_step_s":0.001,"max_cell_steps":100000}});
    (raw, request, scenario)
}

fn study_command(dir: &Path, binary: &str, prefix: &[&str], source: &str) -> Output {
    Command::new(binary)
        .current_dir(dir)
        .args(prefix)
        .args([
            source,
            "study.json",
            "scenario.json",
            "body-input.json",
            "model.ftxbody",
        ])
        .output()
        .unwrap()
}

#[test]
fn joint_scenario_commands_recompute_motion_and_heat_without_material_or_source_leaks() {
    let dir = joint_setup();
    let (raw, request, scenario) = study_inputs();
    save(&dir.join("outfit.json"), &raw);
    save(&dir.join("study.json"), &request);
    save(&dir.join("scenario.json"), &scenario);
    save(
        &dir.join("body-input.json"),
        &json!({"schema":"ftx-scenario-body/1",
        "accepted_body_fit":read(&dir.join("fit.json")),"arm_abduction_deg":35.0,
        "measurements":read(&dir.join("measurements.json"))}),
    );
    let source_before = std::fs::read(dir.join("outfit.json")).unwrap();
    let run = |binary, prefix: &[&str], source| {
        let out = study_command(&dir, binary, prefix, source);
        assert!(
            out.status.success(),
            "{} / {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        value.get("result").unwrap_or(&value).clone()
    };
    let a = run(
        env!("CARGO_BIN_EXE_ftx"),
        &["--json", "materials", "scenario-study"],
        "outfit.json",
    );
    let b = run(
        env!("CARGO_BIN_EXE_ftx-materials"),
        &["scenario-study"],
        "outfit.json",
    );
    assert_eq!(a, b);
    assert_eq!(a["scenario"]["outfit"]["source_request"], raw);
    assert_eq!(a["scenario"]["body_fit"]["shape_source"]["refitted"], false);
    let first = &a["simulations"][0]["evaluation"];
    let second = &a["simulations"][1]["evaluation"];
    assert_eq!(first["status"], "available", "{first}");
    assert_eq!(second["status"], "available", "{second}");
    let first = &first["result"];
    let second = &second["result"];
    assert_eq!(first["positions_sha256"], second["positions_sha256"]);
    assert_eq!(
        first["drape"]["attached_mass"],
        second["drape"]["attached_mass"]
    );
    assert_eq!(first["drape"]["self_contact"]["enabled"], true);
    assert_eq!(first["surface"]["status"], "available");
    assert_eq!(second["surface"]["status"], "available");
    assert_ne!(
        first["metrics"]["independent_surface_power_w"],
        second["metrics"]["independent_surface_power_w"]
    );
    assert_eq!(
        first["motion"]["status"], "available",
        "{}",
        first["motion"]
    );
    assert_eq!(
        second["motion"]["status"], "available",
        "{}",
        second["motion"]
    );
    assert_eq!(
        first["motion"]["result"]["final_positions_sha256"],
        second["motion"]["result"]["final_positions_sha256"]
    );
    assert_eq!(
        first["motion"]["result"]["final_velocities_m_s"],
        second["motion"]["result"]["final_velocities_m_s"]
    );
    assert_eq!(
        first["motion"]["result"]["transient_surface"]["status"], "available",
        "{}",
        first["motion"]
    );
    assert_eq!(
        second["motion"]["result"]["transient_surface"]["status"],
        "available"
    );
    assert_ne!(
        first["motion"]["result"]["transient_surface"],
        second["motion"]["result"]["transient_surface"]
    );
    assert_eq!(a["simulations"][2]["evaluation"]["status"], "refused");
    assert!(
        a["simulations"][2]["evaluation"]["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("pairwise friction")
    );
    for row in a["manufacturing"]["alternatives"].as_array().unwrap() {
        assert_eq!(row["cutting"]["status"], "available");
    }
    // Replaying exact baseline composition uses the same binding and fresh-state physical paths.
    std::fs::write(
        dir.join("composed.json"),
        a["scenario"]["outfit"]["pattern_json"].as_str().unwrap(),
    )
    .unwrap();
    let replay = run(
        env!("CARGO_BIN_EXE_ftx-materials"),
        &["scenario-study"],
        "composed.json",
    );
    assert_eq!(a["manufacturing"], replay["manufacturing"]);
    assert_eq!(a["simulations"], replay["simulations"]);
    assert_eq!(
        std::fs::read(dir.join("outfit.json")).unwrap(),
        source_before
    );
}

#[test]
fn joint_scenario_requires_contact_before_attempting_body_loading() {
    let dir = joint_setup();
    let (raw, request, mut scenario) = study_inputs();
    scenario["options"]["self_contact"] = Value::Null;
    save(&dir.join("outfit.json"), &raw);
    save(&dir.join("study.json"), &request);
    save(&dir.join("scenario.json"), &scenario);
    save(&dir.join("body-input.json"), &json!({}));
    // Wrong model bytes must not conceal the source-composition refusal.
    std::fs::write(dir.join("model.ftxbody"), b"not a model").unwrap();
    for (binary, prefix) in [
        (
            env!("CARGO_BIN_EXE_ftx"),
            vec!["--json", "materials", "scenario-study"],
        ),
        (env!("CARGO_BIN_EXE_ftx-materials"), vec!["scenario-study"]),
    ] {
        let out = study_command(&dir, binary, &prefix, "outfit.json");
        assert!(!out.status.success());
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(text.contains("explicit cloth-cloth self-contact"), "{text}");
    }
}

fn elastic_band_outfit() -> Value {
    let mut request = input();
    let band: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-elastic-band.json"
    ))
    .unwrap();
    for (i, g) in request["garments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        let radius = [180.0, 183.0][i];
        let width = std::f64::consts::TAU * radius;
        let piece = &mut g["pattern"]["pieces"][0];
        piece["vertices"] = json!({"a":[0,0],"b":[width,0],"c":[width,20],"d":[0,20]});
        piece["edges"][1]["id"] = json!("end_r");
        piece["edges"][3]["id"] = json!("end_l");
        piece["placement"]["x0"] = json!(width / 2.0);
        let mut band = band.clone();
        band["record"]["properties"]["axial_stiffness"]["value"] = json!([15.0, 23.0][i]);
        g["pattern"]["seams"] =
            json!([{"id":"join","a":"band/end_r","b":"band/end_l","reverse":true}]);
        g["pattern"]["meta"] = json!({"solved":{"elastic_casings":[{
            "instance":"band","relaxed_m":width*0.99/1000.0,"load_instances":["band"],
            "elastic_material":band}]}});
    }
    request
}

fn elastic_band_command(dir: &Path, pattern: &str, output: &str) -> Output {
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
            "80",
            "--self-contact-mm",
            "2",
            "--elastic-ea",
            "1",
            "--out",
            output,
            "--json",
        ])
        .output()
        .unwrap()
}

#[test]
fn elastic_bands_keep_independent_materials_in_cli_outputs_and_exact_composed_replay() {
    let dir = joint_setup();
    let input = elastic_band_outfit();
    save(&dir.join("outfit.json"), &input);
    let original = std::fs::read(dir.join("outfit.json")).unwrap();
    let out = elastic_band_command(&dir, "outfit.json", "elastic");
    assert!(
        matches!(out.status.code(), Some(0 | 2)),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let report = read(&dir.join("elastic/sim_report.json"));
    let pattern = read(&dir.join("elastic/outfit_pattern.json"));
    for (i, row) in report["elastics"].as_array().unwrap().iter().enumerate() {
        assert_eq!(row["ea_n"], json!([15.0, 23.0][i])); // global option was 1, not an override
        assert!(row["band_material"]["snapshot"]["sha256"].is_string());
        assert_eq!(
            row["band_material"]["snapshot"]["sha256"],
            pattern["meta"]["solved"]["elastic_casings"][i]["elastic_material"]["expected_sha256"]
        );
        assert_eq!(
            pattern["pieces"][i]["vertices"],
            input["garments"][i]["pattern"]["pieces"][0]["vertices"]
        );
    }
    let replay = elastic_band_command(&dir, "elastic/outfit_pattern.json", "replay-elastic");
    assert_eq!(
        out.status.code(),
        replay.status.code(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay_report = read(&dir.join("replay-elastic/sim_report.json"));
    assert_eq!(report["elastics"], replay_report["elastics"]);
    for artifact in ["fold.pc2", "garment.obj", "body.obj"] {
        assert_eq!(
            std::fs::read(dir.join("elastic").join(artifact)).unwrap(),
            std::fs::read(dir.join("replay-elastic").join(artifact)).unwrap()
        );
    }
    assert_eq!(original, std::fs::read(dir.join("outfit.json")).unwrap());
    assert_eq!(
        original,
        std::fs::read(dir.join("elastic/outfit_request.json")).unwrap()
    );
}

#[test]
fn elastic_bands_with_missing_or_unadmitted_stiffness_fail_before_body_loading() {
    let dir = joint_setup();
    std::fs::write(
        dir.join("model.ftxbody"),
        b"bad model must not mask elastic evidence refusal",
    )
    .unwrap();
    for (case, field, value) in [
        ("denied", "allow_estimates", json!(false)),
        ("unknown", "record", Value::Null),
    ] {
        let mut input = elastic_band_outfit();
        input["garments"][0]["pattern"]["meta"]["solved"]["elastic_casings"][0]["elastic_material"]
            [field] = value;
        save(&dir.join("outfit.json"), &input);
        let out = elastic_band_command(&dir, "outfit.json", case);
        assert!(!out.status.success());
        assert!(!dir.join(case).exists());
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(!text.contains("model magic"), "{text}");
    }
}

#[test]
fn joint_casing_source_loads_survive_command_publication_and_exact_replay() {
    let dir = joint_setup();
    let mut request = elastic_band_outfit();
    let width = std::f64::consts::TAU * 180.0;
    let source = &mut request["garments"][0]["pattern"];
    let mut extension = source["pieces"][0].clone();
    extension["id"] = json!("extension");
    extension["vertices"] = json!({"a":[0,-40],"b":[width,-40],"c":[width,0],"d":[0,0]});
    source["pieces"].as_array_mut().unwrap().push(extension);
    source["seams"].as_array_mut().unwrap().extend([
        json!({"id":"close_extension","a":"extension/end_r","b":"extension/end_l","reverse":true}),
        json!({"id":"load_seam","a":"band/bottom","b":"extension/top","reverse":true}),
    ]);
    source["attached_mass"] = accessorized()["garments"][0]["pattern"]["attached_mass"].clone();
    source["attached_mass"]["groups"][0]["targets"][1]["instance"] = json!("extension");
    source["attached_mass"]["groups"][0]["targets"][1]["vertex"] = json!("d");
    save(&dir.join("scoped.json"), &request);
    let original = std::fs::read(dir.join("scoped.json")).unwrap();
    let out = elastic_band_command(&dir, "scoped.json", "scoped");
    assert!(
        matches!(out.status.code(), Some(0 | 2)),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let report = read(&dir.join("scoped/sim_report.json"));
    let load = &report["elastics"][0]["load_mass"];
    let cloth = width * 20e-6 * 0.2;
    let extra = 0.0002 * 0.25;
    assert!((load["cloth_kg"].as_f64().unwrap() - cloth).abs() < 1e-12);
    assert!((load["attached_kg"].as_f64().unwrap() - extra).abs() < 1e-12);
    assert!((load["inertial_kg"].as_f64().unwrap() - cloth - extra).abs() < 1e-12);
    let targets = &report["attached_mass"]["groups"][0]["targets"];
    assert_eq!(targets[0]["solver_vertex"], targets[1]["solver_vertex"]);
    assert_eq!(
        report["elastics"][0]["load_instances"],
        json!(["inner::band"])
    );
    assert_eq!(
        report["elastics"][0]["weight_n"],
        json!(ftx_core::py::round((cloth + extra) * 9.81, 3))
    );
    let replay = elastic_band_command(&dir, "scoped/outfit_pattern.json", "scoped-replay");
    assert_eq!(
        replay.status.code(),
        out.status.code(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(
        report["elastics"],
        read(&dir.join("scoped-replay/sim_report.json"))["elastics"]
    );
    for artifact in ["fold.pc2", "garment.obj", "body.obj"] {
        assert_eq!(
            std::fs::read(dir.join("scoped").join(artifact)).unwrap(),
            std::fs::read(dir.join("scoped-replay").join(artifact)).unwrap()
        );
    }
    assert_eq!(original, std::fs::read(dir.join("scoped.json")).unwrap());
    assert_eq!(
        original,
        std::fs::read(dir.join("scoped/outfit_request.json")).unwrap()
    );
}
