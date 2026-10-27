//! Actual command execution, source-locked replay and atomic JSON refusal behavior.
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/materials")
        .join(name)
}

#[test]
fn both_native_commands_preserve_source_qualified_bridge_power_and_refusals() {
    let read = |name| serde_json::from_slice::<Value>(&fs::read(fixture(name)).unwrap()).unwrap();
    let input = json!({"schema":"ftx-steady-coverage-request/1",
        "report":read("synthetic-steady-surface-report.json"),"map":read("steady-coverage-map.json"),
        "conditions":read("steady-coverage-conditions.json"),"thermal_bridges":read("synthetic-steady-thermal-bridges.json")});
    let expected: Value =
        serde_json::from_str(&ftx_wasm::thermal::steady_coverage(&input.to_string()).unwrap())
            .unwrap();
    assert_eq!(expected["steady"]["bridge_power_w"], 6.);
    let dir = std::env::temp_dir().join(format!(
        "ftx-thermal-bridges-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let call = |main: bool, name: &str, value: &Value| {
        let path = dir.join(format!("{main}-{name}.json"));
        let bytes = serde_json::to_vec(value).unwrap();
        fs::write(&path, &bytes).unwrap();
        let mut command = Command::new(if main {
            env!("CARGO_BIN_EXE_ftx")
        } else {
            env!("CARGO_BIN_EXE_ftx-materials")
        });
        if main {
            command.args(["--json", "materials"]);
        }
        let result = command
            .arg("thermal-steady-coverage")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        let value: Value = serde_json::from_slice(if result.status.success() || main {
            &result.stdout
        } else {
            &result.stderr
        })
        .unwrap();
        if result.status.success() {
            Ok(if main { value["result"].clone() } else { value })
        } else {
            Err(value["refusal"].clone())
        }
    };
    for main in [false, true] {
        assert_eq!(call(main, "input", &input).unwrap(), expected);
        assert_eq!(
            call(main, "frozen", &expected["request"]).unwrap(),
            expected
        );
        let mut stale = input.clone();
        stale["thermal_bridges"]["paths"][0]["qualifications"][0]["surface_model_sha256"] =
            json!("0".repeat(64));
        let refused = call(main, "stale", &stale).unwrap_err();
        assert_eq!(refused["code"], "invalid_document");
        assert!(
            refused["message"]
                .as_str()
                .unwrap()
                .contains("qualification")
        );
    }
    // The same canonical sources also support independently prescribed body-region films and
    // temperatures. One seam loses 6 W while the other gains 2 W; the signed total is 4 W.
    let mut spatial = expected["request"].clone();
    let front_film = spatial["conditions"]["boundaries"][0].clone();
    let mut back_film = front_film.clone();
    back_film["inside_c"] = json!(20.);
    back_film["outside_c"] = json!(30.);
    back_film["inside_resistance_m2_k_w"] = json!(0.2);
    back_film["outside_resistance_m2_k_w"] = json!(0.3);
    spatial["conditions"]["boundaries"] = json!([front_film, back_film]);
    let mut back_path = spatial["thermal_bridges"]["paths"][0].clone();
    back_path["id"] = json!("synthetic-back-seam");
    back_path["instance"] = json!("back");
    back_path["vertices"] = json!([1, 3]);
    let q = &mut back_path["qualifications"][0];
    q["id"] = json!("synthetic-back-reference");
    q["material_sha256"] = json!([spatial["report"]["material"]["instances"][1]["record_sha256"]]);
    q["inside_resistance_m2_k_w"] = json!(0.2);
    q["outside_resistance_m2_k_w"] = json!(0.3);
    q["excess_linear_conductance"]["conditions"]["specimen_id"] =
        json!("synthetic-back-seam-coupon");
    spatial["thermal_bridges"]["paths"]
        .as_array_mut()
        .unwrap()
        .push(back_path);
    let regional: Value =
        serde_json::from_str(&ftx_wasm::thermal::steady_coverage(&spatial.to_string()).unwrap())
            .unwrap();
    assert_eq!(regional["steady"]["bridge_power_w"], 4.);
    assert_eq!(regional["steady"]["body"][0]["bridge_power_w"], 6.);
    assert_eq!(regional["steady"]["body"][1]["bridge_power_w"], -2.);
    let baseline = 15. / 0.7 - 5. / 1.3;
    assert!((regional["steady"]["baseline_power_w"].as_f64().unwrap() - baseline).abs() < 1e-10);
    assert!((regional["steady"]["power_w"].as_f64().unwrap() - baseline - 4.).abs() < 1e-10);
    for main in [false, true] {
        assert_eq!(call(main, "regional-input", &spatial).unwrap(), regional);
        assert_eq!(
            call(main, "regional-frozen", &regional["request"]).unwrap(),
            regional
        );
        let mut stale = regional["request"].clone();
        stale["map"]["body_positions_m"][0][2] = json!(-0.06);
        assert!(call(main, "regional-stale-body", &stale).is_err());
        let mut unqualified = spatial.clone();
        unqualified["conditions"]["boundaries"][1]["outside_resistance_m2_k_w"] = json!(0.4);
        assert!(call(main, "regional-unqualified-film", &unqualified).is_err());
    }
}
#[test]
fn command_projects_frozen_drape_inputs_and_replays_the_same_request() {
    let paths = [
        fixture("synthetic-steady-surface-report.json"),
        fixture("steady-coverage-map.json"),
        fixture("steady-coverage-conditions.json"),
    ];
    let originals: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    let out = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-steady-coverage")
        .args(&paths)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["schema"], "ftx-steady-coverage-result/1");
    assert!((result["steady"]["power_w"].as_f64().unwrap() - 15.0 / 0.7 - 15.0 / 1.1).abs() < 1e-7);
    let primary = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .args(["materials", "thermal-steady-coverage", "--json"])
        .args(&paths)
        .output()
        .unwrap();
    assert!(
        primary.status.success(),
        "{}",
        String::from_utf8_lossy(&primary.stderr)
    );
    let envelope: Value = serde_json::from_slice(&primary.stdout).unwrap();
    assert_eq!(envelope["schema"], "ftx-cli/1");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["result"], result);
    for (p, bytes) in paths.iter().zip(originals) {
        assert_eq!(fs::read(p).unwrap(), bytes);
    }
    let temp =
        std::env::temp_dir().join(format!("ftx-steady-coverage-{}.json", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &result["request"]).unwrap();
    drop(file);
    let replay = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-steady-coverage")
        .arg(&temp)
        .output()
        .unwrap();
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&replay.stdout).unwrap(),
        result
    );
    let primary_replay = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .args(["materials", "thermal-steady-coverage", "--json"])
        .arg(&temp)
        .output()
        .unwrap();
    assert!(primary_replay.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&primary_replay.stdout).unwrap()["result"],
        result
    );
    // Global flags cannot weaken the per-material evidence policy or silently discard extra args.
    for extra in ["--allow-estimates", "--unknown", "extra.json"] {
        let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .args(["materials", "thermal-steady-coverage", "--json"])
            .arg(&temp)
            .arg(extra)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&out.stdout).unwrap()["ok"],
            false
        );
    }
    let mut bad = result["request"].clone();
    bad["map"]["body_triangles"] = json!([[0, 0, 2]]);
    fs::write(&temp, serde_json::to_vec(&bad).unwrap()).unwrap();
    let refused = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-steady-coverage")
        .arg(&temp)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(refused.stdout.is_empty());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("refusal"));
    // Keep the generated test file; this repository does not permit implicit file deletion.
}

#[test]
fn both_commands_share_exterior_coverage_and_refuse_implicit_bare_skin() {
    let read = |name| serde_json::from_slice::<Value>(&fs::read(fixture(name)).unwrap()).unwrap();
    let mut environment = read("synthetic-exterior-exchange.json")["exterior_exchange"].clone();
    environment["air_c"] = json!(0.);
    environment["convection_w_m2_k"] = json!(5.);
    environment.as_object_mut().unwrap().remove("radiation");
    let mut input = json!({"schema":"ftx-steady-coverage-request/1",
        "report":read("synthetic-steady-surface-report.json"),"map":read("steady-coverage-map.json"),
        "conditions":read("steady-coverage-conditions.json")});
    input["map"]["body_positions_m"][1][0] = json!(2.);
    input["map"]["body_positions_m"][3][0] = json!(2.);
    input["conditions"]["boundaries"][0]["outside_resistance_m2_k_w"] = json!(0.);
    input["conditions"]["exterior_exchange"] = json!([environment]);
    let dir = std::env::temp_dir().join(format!(
        "ftx-exterior-coverage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let execute = |main: bool, name: &str, value: &Value| {
        let path = dir.join(format!("{main}-{name}.json"));
        let bytes = serde_json::to_vec(value).unwrap();
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut file, &bytes).unwrap();
        drop(file);
        let mut cmd = Command::new(if main {
            env!("CARGO_BIN_EXE_ftx")
        } else {
            env!("CARGO_BIN_EXE_ftx-materials")
        });
        if main {
            cmd.args(["materials", "--json"]);
        }
        let out = cmd
            .arg("thermal-steady-coverage")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        out
    };
    let result = |out: std::process::Output| {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        if v["schema"] == "ftx-cli/1" {
            v["result"].clone()
        } else {
            v
        }
    };
    for main in [false, true] {
        assert!(!execute(main, "missing-bare", &input).status.success());
    }
    input["conditions"]["bare_boundaries"] = json!([{
        "inside_c":30.,"outside_c":0.,"inside_resistance_m2_k_w":0.2,"outside_resistance_m2_k_w":0.4
    }]);
    let r: ftx_drape::thermal::steady::coverage::request::Request =
        serde_json::from_value(input.clone()).unwrap();
    let expected = r.evaluate().unwrap();
    assert!(
        (expected["steady"]["power_w"].as_f64().unwrap() - 50. - 15. / 0.7 - 15. / 1.1).abs()
            < 1e-7
    );
    for main in [false, true] {
        let actual = result(execute(main, "qualified", &input));
        assert_eq!(actual, expected);
        assert_eq!(actual, result(execute(main, "replay", &actual["request"])));
    }
}
