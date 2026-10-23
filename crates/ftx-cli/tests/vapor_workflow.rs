//! Both real CLI entry points consume measured leather Ret under illustrative boundary inputs.
use ftx_pattern::{
    material_vapor::Request,
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

const INPUT: &str = include_str!("../../../tests/fixtures/materials/leather-vapor-stack.json");

fn invoke(main: bool, input: &str, extra: &[&str]) -> Output {
    invoke_action(main, "thermal-vapor", input, extra)
}

fn invoke_action(main: bool, action: &str, input: &str, extra: &[&str]) -> Output {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "ftx-vapor-{}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, input).unwrap();
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    if main {
        command.args(["materials", "--json"]);
    }
    let output = command.arg(action).arg(&path).args(extra).output().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), input);
    output
}

fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    if value["schema"] == "ftx-cli/1" {
        assert_eq!(value["command"], "materials");
        assert_eq!(value["ok"], true);
        assert_eq!(value["exit"], 0);
        value["result"].clone()
    } else {
        value
    }
}

fn refusal(main: bool, output: &Output) {
    assert!(!output.status.success());
    let document = if main { &output.stdout } else { &output.stderr };
    let value: Value = serde_json::from_slice(document).unwrap();
    assert!(value["refusal"].is_object());
    assert!(value.get("result").is_none());
    if !main {
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn both_binaries_replay_geometry_derived_vapor_without_a_body_model_or_dry_heat_data() {
    let input = include_str!("../../../examples/vapor-coverage/layered.json");
    let request: ftx_drape::vapor::coverage::Request = serde_json::from_str(input).unwrap();
    let expected = serde_json::to_value(request.evaluate().unwrap()).unwrap();
    let replay = expected["replay_request_json"].as_str().unwrap();
    assert!(expected["coverage"]["covered_area_m2"].as_f64().unwrap() > 0.0);
    assert!(
        expected["coverage"]["covered_potential_mass_flow_kg_s"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    let mut tampered: Value = serde_json::from_str(replay).unwrap();
    tampered["source"]["materials"][0]["record"]["properties"]["vapor_resistance"]["value"] =
        json!(999.0);
    for main in [false, true] {
        assert_eq!(
            success(&invoke_action(main, "thermal-vapor-coverage", input, &[])),
            expected
        );
        assert_eq!(
            success(&invoke_action(main, "thermal-vapor-coverage", replay, &[])),
            expected
        );
        refusal(
            main,
            &invoke_action(main, "thermal-vapor-coverage", &tampered.to_string(), &[]),
        );
    }
}

#[test]
fn both_binaries_replay_moving_layered_potential_with_the_exact_shared_integral() {
    let initial: Value = serde_json::from_str(include_str!(
        "../../../examples/vapor-coverage/layered.json"
    ))
    .unwrap();
    let mut stretched_cloth = initial["source"]["positions_m"].clone();
    let mut stretched_body = initial["map"]["body_positions_m"].clone();
    for positions in [&mut stretched_cloth, &mut stretched_body] {
        for vertex in positions.as_array_mut().unwrap() {
            vertex[0] = json!(vertex[0].as_f64().unwrap() * 2.0);
        }
    }
    let input = json!({"schema":"ftx-vapor-coverage-history/1",
        "policy":{"schema":"ftx-motion-vapor-coverage/1","integration":"right_endpoint","max_geometry_operations":10000},
        "samples":[
            {"time_s":1,"positions_m":initial["source"]["positions_m"],"body_positions_m":initial["map"]["body_positions_m"]},
            {"time_s":3,"positions_m":stretched_cloth,"body_positions_m":stretched_body}],
        "initial":initial}).to_string();
    let request: ftx_drape::vapor::coverage::history::Request =
        serde_json::from_str(&input).unwrap();
    let expected = request.evaluate().unwrap();
    let energy = 1000.0 * 0.002 / (2.0 * 11.27 + 5.0) * 5.0;
    assert!((expected["totals"]["potential_energy_j"].as_f64().unwrap() - energy).abs() < 1e-12);
    assert!(
        (expected["totals"]["potential_mass_kg"].as_f64().unwrap() - energy / 2_500_000.0).abs()
            < 1e-18
    );
    let replay = expected["replay_request_json"].as_str().unwrap();
    let mut exceeded: Value = serde_json::from_str(replay).unwrap();
    exceeded["policy"]["max_geometry_operations"] = json!(1);
    for main in [false, true] {
        for document in [&input, replay] {
            assert_eq!(
                success(&invoke_action(
                    main,
                    "thermal-vapor-coverage-history",
                    document,
                    &[]
                )),
                expected
            );
        }
        refusal(
            main,
            &invoke_action(
                main,
                "thermal-vapor-coverage-history",
                &exceeded.to_string(),
                &[],
            ),
        );
    }

    let mut trapezoid: Value = serde_json::from_str(&input).unwrap();
    trapezoid["policy"]["integration"] = json!("composite_trapezoid");
    let request: ftx_drape::vapor::coverage::history::Request =
        serde_json::from_value(trapezoid.clone()).unwrap();
    let expected = request.evaluate().unwrap();
    let energy = 1000.0 * 0.002 / (2.0 * 11.27 + 5.0) * 4.0;
    assert!((expected["totals"]["potential_energy_j"].as_f64().unwrap() - energy).abs() < 1e-12);
    for main in [false, true] {
        for document in [
            trapezoid.to_string(),
            expected["replay_request_json"].as_str().unwrap().into(),
        ] {
            assert_eq!(
                success(&invoke_action(
                    main,
                    "thermal-vapor-coverage-history",
                    &document,
                    &[]
                )),
                expected
            );
        }
    }
}

#[test]
fn both_binaries_conserve_finite_wet_film_across_moving_coverage_and_replay() {
    let initial: Value = serde_json::from_str(include_str!(
        "../../../examples/vapor-coverage/layered.json"
    ))
    .unwrap();
    let coverage: ftx_drape::vapor::coverage::Request =
        serde_json::from_value(initial.clone()).unwrap();
    let initial_report = coverage.evaluate().unwrap();
    let mut cloth = initial["source"]["positions_m"].clone();
    let mut body = initial["map"]["body_positions_m"].clone();
    for positions in [&mut cloth, &mut body] {
        for vertex in positions.as_array_mut().unwrap() {
            vertex[0] = json!(2.0 * vertex[0].as_f64().unwrap());
        }
    }
    // Two measured-Ret layers share ONE synthetic body-side reservoir. Its initial water
    // and supply are deliberately below the independent series-network potential demand.
    let rate = 1000.0 * 0.002 / (2.0 * 11.27 + 5.0) / 2_500_000.0;
    let input = json!({"schema":"ftx-vapor-coverage-history/1",
        "policy":{"schema":"ftx-motion-vapor-coverage/1","integration":"right_endpoint",
            "max_geometry_operations":10000,"wet_film":{"schema":"ftx-body-wet-film/1",
                "body_mesh_sha256":initial_report["body_mesh_sha256"],
                "source":"test:finite-body-film","basis":"Synthetic finite film and constant supply; no wearer measurement",
                "initial_liquid_mass_kg":[0.5*rate],"supply_rate_kg_s":[0.25*rate]}},
        "samples":[
            {"time_s":1,"positions_m":initial["source"]["positions_m"],"body_positions_m":initial["map"]["body_positions_m"]},
            {"time_s":3,"positions_m":cloth,"body_positions_m":body}],"initial":initial});
    let request: ftx_drape::vapor::coverage::history::Request =
        serde_json::from_value(input.clone()).unwrap();
    let expected = request.evaluate().unwrap();
    let water = &expected["wet_film"]["totals"];
    assert!((water["evaporated_mass_kg"].as_f64().unwrap() - 1.25 * rate).abs() < 1e-18);
    assert!((water["supplied_mass_kg"].as_f64().unwrap() - 0.75 * rate).abs() < 1e-18);
    assert_eq!(water["remaining_liquid_mass_kg"], 0.0);
    assert!(water["mass_balance_error_kg"].as_f64().unwrap().abs() < 1e-18);
    assert!(
        (water["body_reservoir_latent_energy_j"].as_f64().unwrap() - 1.25 * rate * 2_500_000.0)
            .abs()
            < 1e-12
    );
    assert_eq!(expected["wet_film"]["body"].as_array().unwrap().len(), 1);
    assert_eq!(
        expected["samples"][1]["wet_film"]["body"][0]["starting_liquid_mass_kg"],
        0.0
    );
    assert_eq!(expected["wet_film"]["textile_temperature_coupled"], false);
    let replay = expected["replay_request_json"].as_str().unwrap();
    let mut stale = input.clone();
    stale["policy"]["wet_film"]["body_mesh_sha256"] = json!("0".repeat(64));
    let mut malformed = input.clone();
    malformed["policy"]["wet_film"]["initial_liquid_mass_kg"] = json!([rate, rate]);
    for main in [false, true] {
        for document in [input.to_string(), replay.into()] {
            assert_eq!(
                success(&invoke_action(
                    main,
                    "thermal-vapor-coverage-history",
                    &document,
                    &[]
                )),
                expected
            );
        }
        for invalid in [&stale, &malformed] {
            refusal(
                main,
                &invoke_action(
                    main,
                    "thermal-vapor-coverage-history",
                    &invalid.to_string(),
                    &[],
                ),
            );
        }
    }
}

#[test]
fn both_binaries_match_core_preserve_source_conditions_and_replay_offline() {
    let request: Request = serde_json::from_str(INPUT).unwrap();
    let expected = serde_json::to_value(
        request
            .evaluate(&ftx_pattern::textile_import::catalog().unwrap())
            .unwrap(),
    )
    .unwrap();
    let replay = expected["replay_request_json"].as_str().unwrap();
    let frozen: Request = serde_json::from_str(replay).unwrap();
    let empty = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    let replayed = serde_json::to_value(frozen.evaluate(&empty).unwrap()).unwrap();
    for main in [false, true] {
        assert_eq!(success(&invoke(main, INPUT, &[])), expected);
        assert_eq!(success(&invoke(main, replay, &[])), replayed);
    }
    // The Ret mean is transcribed independently from Kopitar2022 Table 5; boundary data are
    // illustrative inputs, not a measured wearer condition or a validated stack experiment.
    let expected_power = 0.5 * 2000.0 / (10.0 + 11.27 + 20.0);
    assert!((expected["potential_heat_flow_w"].as_f64().unwrap() - expected_power).abs() < 1e-12);
    assert_eq!(expected["layers"][0]["observation"]["evidence"], "measured");
    assert_eq!(
        expected["layers"][0]["observation"]["conditions"]["air_temperature_c"],
        "35"
    );
    assert_eq!(expected["physical_validation_claim"], false);
    assert_eq!(replayed["layers"], expected["layers"]);
    assert_eq!(
        replayed["potential_heat_flow_w"],
        expected["potential_heat_flow_w"]
    );
}

#[test]
fn changing_only_canonical_material_changes_both_entry_points() {
    let original = success(&invoke(true, INPUT, &[]));
    let mut changed: Value = serde_json::from_str(INPUT).unwrap();
    changed["layers"][0]["selection"]["id"] = json!("zagreb/leather-2022/syn-d-h-neck");
    let second = success(&invoke(true, &changed.to_string(), &[]));
    assert_eq!(second, success(&invoke(false, &changed.to_string(), &[])));
    assert_ne!(second["request_sha256"], original["request_sha256"]);
    assert_ne!(
        second["layers"][0]["material_sha256"],
        original["layers"][0]["material_sha256"]
    );
    assert!((second["potential_heat_flow_w"].as_f64().unwrap() - 1000.0 / 60.38).abs() < 1e-12);
    assert!(
        second["potential_heat_flow_w"].as_f64().unwrap()
            < original["potential_heat_flow_w"].as_f64().unwrap()
    );
}

#[test]
fn malformed_unqualified_and_changed_frozen_sources_refuse_without_partial_reports() {
    let report = success(&invoke(false, INPUT, &[]));
    let mut tampered = report["replay_request"].clone();
    tampered["layers"][0]["selection"]["record"]["properties"]["vapor_resistance"]["value"] =
        json!(1.0);
    let mut unsupported: Value = serde_json::from_str(INPUT).unwrap();
    unsupported["required_conditions"]["air_temperature_c"] = json!("20");
    let mut reverse: Value = serde_json::from_str(INPUT).unwrap();
    reverse["outside_vapor_pressure_pa"] = json!(4000.0);
    let mut unknown: Value = serde_json::from_str(INPUT).unwrap();
    unknown["infer_sweat_supply"] = json!(true);
    let mut missing: Value = serde_json::from_str(INPUT).unwrap();
    missing["layers"][0]["selection"]["id"] = json!("fsri/hfm/cotton-sheet/dry/220330-r1");
    for main in [false, true] {
        for input in [
            "{oops".into(),
            tampered.to_string(),
            unsupported.to_string(),
            reverse.to_string(),
            unknown.to_string(),
            missing.to_string(),
        ] {
            refusal(main, &invoke(main, &input, &[]));
        }
        for extra in [
            vec!["--allow-estimates"],
            vec!["extra.json"],
            vec!["--infer-sweat"],
        ] {
            refusal(main, &invoke(main, INPUT, &extra));
        }
    }
}
