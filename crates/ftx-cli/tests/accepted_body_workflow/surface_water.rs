//! Actual selected-model cooling and decision replay; all materials and liquid inputs are synthetic.
use super::*;
use ftx_pattern::{material_catalog::Selection, materials::Catalog};

#[test]
fn coupled_textile_water_objectives_replay_in_both_native_commands_and_portable_scenarios() {
    use ftx_wasm::materials::scenario::on_model;

    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body = json!({"schema":"ftx-scenario-body/1", "accepted_body_fit":receipt(&digest),
        "arm_abduction_deg":35.0});
    let pattern =
        include_str!("../../../../tests/fixtures/materials/synthetic-scenario-pattern.json");
    let mut pattern_value: Value = serde_json::from_str(pattern).unwrap();
    // Match the existing selected-model scenario fixture's explicit cylindrical placement.
    let width = std::f64::consts::TAU * 180.0;
    pattern_value["pieces"][0]["vertices"]["b"][0] = json!(width);
    pattern_value["pieces"][0]["vertices"]["c"][0] = json!(width);
    pattern_value["pieces"][0]["placement"]["radius_mm"] = json!(180.0);
    pattern_value["pieces"][0]["placement"]["x0"] = json!(width / 2.0);
    let pattern = pattern_value.to_string();
    let mut material = pattern_value["fabric"]["material"].clone();
    for (key, value, unit) in [
        ("specific_heat", 1000., "J/(kg*K)"),
        ("vapor_resistance", 8., "m2*Pa/W"),
    ] {
        material["record"]["properties"][key] = json!({"value":value, "unit":unit,
            "source":"test", "evidence":"estimate", "method":"Synthetic coupled liquid decision replay"});
    }
    let selected: Selection = serde_json::from_value(material.clone()).unwrap();
    let lock = Catalog::builtin()
        .unwrap()
        .resolve(&selected)
        .unwrap()
        .sha256()
        .to_owned();
    let mut missing = material.clone();
    missing["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("vapor_resistance");
    let study = json!({"schema":"ftx-material-study/1", "variants":[
        {"key":"qualified", "changes":{"schema":"ftx-material-bindings/1","default":material}},
        {"key":"missing-ret", "changes":{"schema":"ftx-material-bindings/1","default":missing}}],
        "cutting":{"copies":1,"gap_mm":1.,"edge_margin_mm":0.,"end_margin_mm":0.,"seam_allowance_mm":0.},
        "comparison":{"basis":"nominal", "required_consumers":["transient_surface"],
            "objectives":[{"metric":"transient_surface_final_min_temperature_c","direction":"minimize"}],
            "constraints":[{"metric":"textile_water_remaining_mass_kg","maximum":0.},
                {"metric":"textile_water_evaporated_mass_kg","minimum":0.},
                {"metric":"textile_water_latent_energy_j","minimum":0.}]}});
    let mut single: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario.json"
    ))
    .unwrap();
    // Explicit sewing-only geometry, not a gravity-equilibrium or measured wearer-fit claim.
    single["options"]["sew_steps"] = json!(120);
    single["options"]["drape_steps"] = json!(0);
    single["motion"] = json!({"schema":"ftx-drape-motion/1",
        "keyframes":[{"time_s":0.001,"arm_abduction_deg":35.}],
        "max_step_s":0.0005,"max_trials":8,"max_pairs":100000,"max_sample_vertices":100000,
        "transient_surface":{"schema":"ftx-motion-surface-heat/1", "initial_temperature_c":20.,
            "cells_per_layer":1,"max_step_s":0.00025,"max_cell_steps":1000000,
            "boundaries":[{"inside_c":20.,"outside_c":20.,"inside_resistance_m2_k_w":1.,"outside_resistance_m2_k_w":1.}],
            "surface_water":{"schema":"ftx-textile-surface-water/1",
                "model":"inner_cell_equilibrium_free_film_frozen_properties/1",
                "bindings":{"band":{"material_sha256":lock,"initial_liquid_kg_m2":5e-8}},
                "maximum_liquid_loading_kg_m2":1e-5,"liquid_specific_heat_j_kg_k":4200.,
                "latent_heat_j_kg":2500000.,"enthalpy_reference_c":0.,
                "wet_vapor_pressure_pa":[[-20.,0.],[80.,10000.]],"outside_vapor_pressure_pa":[1000.],
                "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":1.,
                "replenishment_kg_m2_s":0.,"source":{"reference":"test:selected-body-coupled-liquid",
                    "accessed_on":"2026-10-23","note":"Synthetic fluid and fixed-property evidence only"},
                "evidence":"estimate","allow_estimates":true,"method":"Synthetic decision replay",
                "qualification":"Synthetic fixed dry thermal and vapor properties across [-20,80] C and [0,1e-5] kg/m2"}}});
    let mut warm = single.clone();
    warm["motion"]["transient_surface"]["boundaries"][0]["outside_c"] = json!(25.);
    let matrix = json!({"schema":"ftx-drape-scenario-matrix/1", "max_evaluations":4,
        "cases":[{"key":"equal-reservoirs","scenario":single},{"key":"warm-outside","scenario":warm}]});
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    std::fs::write(dir.join("pattern.json"), &pattern).unwrap();
    save(&dir.join("study.json"), &study);
    save(&dir.join("body-input.json"), &body);
    let capture = std::env::var_os("FTX_COUPLED_WATER_REPLAY").map(PathBuf::from);
    if let Some(capture) = &capture {
        std::fs::create_dir_all(capture).unwrap();
        std::fs::write(capture.join("model.ftxbody"), &bytes).unwrap();
        std::fs::write(capture.join("pattern.json"), &pattern).unwrap();
        save(&capture.join("study.json"), &study);
        save(&capture.join("body-input.json"), &body);
    }
    for (name, scenario) in [("single", single), ("matrix", matrix)] {
        let report: Value = serde_json::from_str(
            &on_model(
                &model,
                &digest,
                &pattern,
                &study.to_string(),
                &scenario.to_string(),
                &body.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            report["comparison"]["frontier"],
            json!(["qualified"]),
            "{}",
            report["comparison"]
        );
        if name == "single" {
            let candidate = report["comparison"]["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == "qualified")
                .unwrap();
            let readings = &candidate["readings"];
            assert!(
                readings["transient_surface_final_min_temperature_c"]["value"]
                    .as_f64()
                    .unwrap()
                    < 20.
            );
            assert_eq!(readings["textile_water_remaining_mass_kg"]["value"], 0.);
            assert!(
                readings["textile_water_evaporated_mass_kg"]["value"]
                    .as_f64()
                    .unwrap()
                    > 0.
            );
            assert!(
                readings["textile_water_latent_energy_j"]["value"]
                    .as_f64()
                    .unwrap()
                    > 0.
            );
            for row in report["manufacturing"]["alternatives"].as_array().unwrap() {
                assert_eq!(row["cutting"]["status"], "available");
            }
        }
        save(&dir.join("scenario.json"), &scenario);
        for (binary, prefix) in [
            (
                env!("CARGO_BIN_EXE_ftx"),
                &["--json", "materials", "scenario-study"][..],
            ),
            (env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"][..]),
        ] {
            let output = Command::new(binary)
                .current_dir(&dir)
                .args(prefix)
                .args([
                    "pattern.json",
                    "study.json",
                    "scenario.json",
                    "body-input.json",
                    "model.ftxbody",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let native: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(native.get("result").unwrap_or(&native), &report);
        }
        if let Some(capture) = &capture {
            save(&capture.join(format!("{name}-scenario.json")), &scenario);
            save(&capture.join(format!("{name}-report.json")), &report);
        }
    }
}
