//! Actual selected-model cooling and decision replay; all materials and liquid inputs are synthetic.
use super::*;
use ftx_pattern::{material_catalog::Selection, materials::Catalog};

fn inputs() -> (String, Value, Value) {
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
    (pattern, study, single)
}

fn keyed<'a>(rows: &'a Value, key: &str) -> &'a Value {
    rows.as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == key)
        .unwrap_or_else(|| panic!("missing selected-body study row {key}"))
}

fn replay(
    pattern: &str,
    study: &Value,
    single: Value,
    capture_variable: &str,
    inspect: impl Fn(&Value),
) {
    use ftx_wasm::materials::scenario::on_model;

    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body = json!({"schema":"ftx-scenario-body/1", "accepted_body_fit":receipt(&digest),
        "arm_abduction_deg":35.0});
    let mut warm = single.clone();
    warm["motion"]["transient_surface"]["boundaries"][0]["outside_c"] = json!(25.);
    let matrix = json!({"schema":"ftx-drape-scenario-matrix/1", "max_evaluations":4,
        "cases":[{"key":"equal-reservoirs","scenario":single},{"key":"warm-outside","scenario":warm}]});
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    std::fs::write(dir.join("pattern.json"), pattern).unwrap();
    save(&dir.join("study.json"), study);
    save(&dir.join("body-input.json"), &body);
    let capture = std::env::var_os(capture_variable).map(PathBuf::from);
    if let Some(capture) = &capture {
        std::fs::create_dir_all(capture).unwrap();
        std::fs::write(capture.join("model.ftxbody"), &bytes).unwrap();
        std::fs::write(capture.join("pattern.json"), pattern).unwrap();
        save(&capture.join("study.json"), study);
        save(&capture.join("body-input.json"), &body);
    }
    for (name, scenario) in [("single", single), ("matrix", matrix)] {
        let report: Value = serde_json::from_str(
            &on_model(
                &model,
                &digest,
                pattern,
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
            "{}; thermal outcomes: {:?}",
            report["comparison"],
            report["simulations"].as_array().map(|rows| rows
                .iter()
                .map(|row| {
                    let motion = &row["evaluation"]["result"]["motion"];
                    let thermal = &motion["result"]["transient_surface"];
                    json!({"key":row["key"],"evaluation_status":row["evaluation"]["status"],
                    "motion_status":motion["status"],"thermal_status":thermal["status"],
                    "thermal_refusal":thermal["refusal"]})
                })
                .collect::<Vec<_>>())
        );
        for row in report["manufacturing"]["alternatives"].as_array().unwrap() {
            assert_eq!(row["cutting"]["status"], "available");
        }
        if name == "single" {
            inspect(&report);
        } else {
            assert_eq!(
                report["comparison"]["case_order"],
                json!(["equal-reservoirs", "warm-outside"])
            );
            for case in report["cases"].as_array().unwrap() {
                assert_eq!(case["comparison"]["frontier"], json!(["qualified"]));
                inspect(case);
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

fn qualify_mechanics(pattern: &str, study: &Value, single: &mut Value, exchange: bool) {
    use ftx_wasm::materials::scenario::on_model;
    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body = json!({"schema":"ftx-scenario-body/1","accepted_body_fit":receipt(&digest),
        "arm_abduction_deg":35.});
    let baseline: Value = serde_json::from_str(
        &on_model(
            &model,
            &digest,
            pattern,
            &study.to_string(),
            &single.to_string(),
            &body.to_string(),
            |_| {},
        )
        .unwrap(),
    )
    .unwrap();
    let selected = &keyed(&baseline["simulations"], "qualified")["evaluation"]["result"];
    let material = &selected["drape"]["material"];
    assert!(material.is_object());
    let heat = &mut single["motion"]["transient_surface"];
    heat["mechanical_coupling"] = json!({
        "schema":if exchange { "ftx-coupled-liquid-mechanics/1" } else { "ftx-evaporating-liquid-mechanics/1" },
        "model":if exchange { "co_moving_mass_exchange_frozen_mechanics/1" } else { "co_moving_evaporation_frozen_mechanics/1" },
        "material_report_sha256":sha256_hex(&serde_json::to_vec(material).unwrap()),
        "bindings":{"band":{"material_sha256":heat["surface_water"]["bindings"]["band"]["material_sha256"],
            "maximum_liquid_loading_kg_m2":heat["surface_water"]["maximum_liquid_loading_kg_m2"],"temperature_range_c":[-20.,80.]}},
        "source":{"reference":"test:selected-body-wet-mechanics","accessed_on":"2026-10-28",
            "note":"Synthetic invariant material and interface behavior, no empirical claim"},
        "evidence":"estimate","allow_estimates":true,"method":"Native/portable coupled replay",
        "qualification":if exchange { "Exact artificial mechanical laws retained over the complete declared wet thermal domain with co-moving incoming and outgoing mass" } else { "Exact artificial mechanical laws retained over the complete declared wet thermal domain with co-moving vapor loss" }
    });
}

#[test]
fn evaporating_mechanical_water_replays_in_native_portable_and_named_case_decisions() {
    let (pattern, study, mut single) = inputs();
    qualify_mechanics(&pattern, &study, &mut single, false);
    replay(
        &pattern,
        &study,
        single,
        "FTX_EVAPORATING_MECHANICS_REPLAY",
        |report| {
            let simulated = &keyed(&report["simulations"], "qualified")["evaluation"]["result"];
            let motion = &simulated["motion"]["result"];
            let heat = &motion["transient_surface"]["result"];
            let mechanics = &motion["liquid_mechanics"];
            assert_eq!(mechanics, &heat["mechanical_coupling"]);
            assert_eq!(mechanics["remaining_liquid_mass_kg"], 0.);
            assert!(mechanics["initial_liquid_mass_kg"].as_f64().unwrap() > 0.);
            assert!(mechanics["exported_kinetic_energy_j"].as_f64().unwrap() > 0.);
            assert!(
                mechanics["mass_balance_residual_kg"]
                    .as_f64()
                    .unwrap()
                    .abs()
                    < 1e-14
            );
            assert_eq!(
                mechanics["qualification"],
                heat["request"]["mechanical_coupling"]
            );
            assert_eq!(
                motion["initial_sample"]["liquid_mechanics"]["remaining_liquid_mass_kg"],
                mechanics["initial_liquid_mass_kg"]
            );
            assert_eq!(
                keyed(&report["simulations"], "missing-ret")["evaluation"]["result"]["motion"]["status"],
                "refused"
            );
        },
    );
}

#[test]
fn incoming_mechanical_water_replays_actual_native_portable_and_named_case_decisions() {
    replay_incoming_mechanical_water(1);
}

#[test]
fn incoming_water_impulse_replays_actual_native_portable_and_named_case_decisions() {
    replay_incoming_mechanical_water(2);
}

#[test]
fn separate_water_phase_impulses_replay_native_portable_and_named_case_decisions() {
    replay_incoming_mechanical_water(4);
}

#[test]
fn scheduled_incoming_water_replays_native_portable_and_named_case_decisions() {
    replay_incoming_mechanical_water(3);
}

#[test]
fn scheduled_phase_water_impulses_replay_native_portable_and_named_case_decisions() {
    replay_incoming_mechanical_water(5);
}

#[test]
fn outgoing_water_recoil_replays_native_portable_and_named_case_decisions() {
    replay_incoming_mechanical_water(6);
}

fn replay_incoming_mechanical_water(version: u8) {
    let prescribed_velocity = version >= 2;
    let phase_specific = version >= 4;
    let scheduled = matches!(version, 3 | 5 | 6);
    let recoil = version == 6;
    for condensation in [false, true] {
        let (pattern, mut study, mut single) = inputs();
        single["motion"]["keyframes"] = json!([
            {"time_s":0.0005,"arm_abduction_deg":35.},
            {"time_s":0.001,"arm_abduction_deg":35.}]);
        let heat = &mut single["motion"]["transient_surface"];
        heat["boundaries"] = json!([heat["boundaries"][0], heat["boundaries"][0]]);
        let water = &mut heat["surface_water"];
        water["schema"] = json!(if condensation {
            "ftx-textile-surface-water/3"
        } else {
            "ftx-textile-surface-water/2"
        });
        water
            .as_object_mut()
            .unwrap()
            .remove("replenishment_kg_m2_s");
        water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(0.);
        water["maximum_liquid_loading_kg_m2"] = json!(0.2);
        water["qualification"] =
            json!("Synthetic invariant thermal/vapor laws over [-20,80] C and [0,0.2] kg/m2");
        water["outside_vapor_pressure_pa"] =
            json!(if condensation && phase_specific && scheduled {
                [6000., 6500.]
            } else if condensation {
                [6000., 1000.]
            } else {
                [1000., 1000.]
            });
        water["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
            "bindings":{"band":{"rate_kg_m2_s":if phase_specific && condensation { [0.1,0.1] } else if condensation { [0.,0.] } else { [100.,100.] },
                "temperature_c":[20.,20.]}},
            "source":{"reference":"test:mechanical-water-delivery","accessed_on":"2026-10-28",
                "note":"Artificial feed for numerical feedback; not measured weather or wearer data"},
            "evidence":"estimate","allow_estimates":true,"method":"Prescribed co-moving supply"});
        if condensation {
            water["latent_heat_j_kg"] = json!(10.);
            water["condensation"] = json!({"schema":"ftx-textile-condensation/1","vapor_temperature_c":[40.,10.],
                "source":{"reference":"test:mechanical-condensation","accessed_on":"2026-10-28",
                    "note":"Artificial reverse Ret and low latent heat; no empirical textile claim"},
                "evidence":"estimate","allow_estimates":true,"method":"Two-interval phase transfer",
                "qualification":"Synthetic reverse Ret and wet/dry phase closure with constant latent plus cp_liquid effective sensible enthalpy over the full declared domain"});
        }
        if recoil {
            water["schema"] = json!("ftx-textile-surface-water/4");
            water["drainage"] = json!({"schema":"ftx-textile-liquid-drainage/1",
                "bindings":{"band":{"rate_kg_m2_s":[1.,2.]}},
                "source":{"reference":"test:scheduled-recoil-drainage","accessed_on":"2026-11-01",
                    "note":"Artificial nonzero outflow for conservation; no measured runoff claim"},
                "evidence":"estimate","allow_estimates":true,"method":"Prescribed reference-area drainage",
                "qualification":"Stock-limited removal with sensible enthalpy at the retained liquid temperature; velocities independently qualified by the mechanical request"});
        }
        let mass_metric = if condensation {
            "textile_water_condensed_mass_kg"
        } else {
            "textile_water_supplied_mass_kg"
        };
        study["comparison"]["objectives"] = json!([
            {"metric":mass_metric,"direction":"minimize"},
            {"metric":"transient_surface_final_min_temperature_c","direction":"minimize"}]);
        study["comparison"]["constraints"][0] =
            json!({"metric":"textile_water_remaining_mass_kg","minimum":0.});
        qualify_mechanics(&pattern, &study, &mut single, true);
        let velocity = [0.2, 0.1, -0.5];
        let vapor_velocity = [-0.1, 0.3, 0.4];
        let next_velocity = [-0.3, 0.4, 0.2];
        let next_vapor_velocity = [0.5, -0.2, 0.1];
        let evaporation_velocity = [0., 0., 0.4];
        let next_evaporation_velocity = [0., 0., 0.2];
        let drainage_velocity = [0.2, 0., -0.1];
        let next_drainage_velocity = [-0.1, 0.1, -0.2];
        if prescribed_velocity {
            let qualification = &mut single["motion"]["transient_surface"]["mechanical_coupling"];
            qualification["schema"] = json!("ftx-coupled-liquid-mechanics/2");
            qualification["model"] =
                json!("prescribed_incoming_velocity_mass_exchange_frozen_mechanics/1");
            qualification["bindings"]["band"]["incoming_velocity_world_m_s"] = json!(velocity);
            if phase_specific {
                qualification["schema"] = json!("ftx-coupled-liquid-mechanics/4");
                qualification["model"] =
                    json!("phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1");
                let binding = &mut qualification["bindings"]["band"];
                binding
                    .as_object_mut()
                    .unwrap()
                    .remove("incoming_velocity_world_m_s");
                binding["incoming_stream_velocities_world_m_s"] =
                    json!({"supply":velocity,"condensation":vapor_velocity});
            }
            qualification["method"] =
                json!("Synthetic prescribed incoming world velocity with inelastic capture");
            qualification["qualification"] = json!(
                "Exact artificial wet mechanical laws and prescribed incoming momentum over the complete declared thermal domain; numerical test, not measured rain or condensation"
            );
            if scheduled && phase_specific {
                qualification["schema"] = json!(if recoil {
                    "ftx-coupled-liquid-mechanics/6"
                } else {
                    "ftx-coupled-liquid-mechanics/5"
                });
                qualification["model"] = json!(if recoil {
                    "scheduled_phase_specific_incoming_and_outgoing_velocity_mass_exchange_frozen_mechanics/1"
                } else {
                    "scheduled_phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1"
                });
                let binding = &mut qualification["bindings"]["band"];
                binding
                    .as_object_mut()
                    .unwrap()
                    .remove("incoming_stream_velocities_world_m_s");
                binding["incoming_stream_velocities_world_m_s_by_interval"] = json!([
                    {"supply":velocity,"condensation":vapor_velocity},
                    {"supply":next_velocity,"condensation":next_vapor_velocity}]);
                if recoil {
                    binding["outgoing_stream_velocities_relative_to_cloth_m_s_by_interval"] = json!([
                        {"evaporation":evaporation_velocity,"drainage":drainage_velocity},
                        {"evaporation":next_evaporation_velocity,"drainage":next_drainage_velocity}]);
                }
                qualification["qualification"] = json!(
                    "Exact artificial wet mechanical laws with independently prescribed phase velocities in both intervals; outgoing relative velocities, when declared, are separately qualified synthetic boundary work, not inferred runoff"
                );
            } else if scheduled {
                qualification["schema"] = json!("ftx-coupled-liquid-mechanics/3");
                qualification["model"] =
                    json!("scheduled_incoming_velocity_mass_exchange_frozen_mechanics/1");
                qualification["bindings"]["band"]
                    .as_object_mut()
                    .unwrap()
                    .remove("incoming_velocity_world_m_s");
                qualification["bindings"]["band"]["incoming_velocity_world_m_s_by_interval"] =
                    json!([velocity, next_velocity]);
            }
        }
        let expected_qualification =
            single["motion"]["transient_surface"]["mechanical_coupling"].clone();
        replay(
            &pattern,
            &study,
            single,
            if recoil && condensation {
                "FTX_CONDENSING_OUTGOING_RECOIL_REPLAY"
            } else if recoil {
                "FTX_OUTGOING_RECOIL_REPLAY"
            } else if phase_specific && scheduled && condensation {
                "FTX_SCHEDULED_PHASE_CONDENSING_REPLAY"
            } else if phase_specific && scheduled {
                "FTX_SCHEDULED_PHASE_SUPPLY_REPLAY"
            } else if phase_specific && condensation {
                "FTX_PHASE_SPECIFIC_CONDENSING_REPLAY"
            } else if phase_specific {
                "FTX_PHASE_SPECIFIC_SUPPLY_REPLAY"
            } else if scheduled && condensation {
                "FTX_SCHEDULED_CONDENSING_IMPULSE_REPLAY"
            } else if scheduled {
                "FTX_SCHEDULED_WATER_IMPULSE_REPLAY"
            } else if prescribed_velocity && condensation {
                "FTX_CONDENSING_IMPULSE_REPLAY"
            } else if prescribed_velocity {
                "FTX_INCOMING_WATER_IMPULSE_REPLAY"
            } else if condensation {
                "FTX_CONDENSING_MECHANICS_REPLAY"
            } else {
                "FTX_MECHANICAL_WATER_REPLAY"
            },
            |report| {
                let simulation =
                    &keyed(&report["simulations"], "qualified")["evaluation"]["result"];
                let motion = &simulation["motion"]["result"];
                let heat = &motion["transient_surface"]["result"];
                let mechanics = &motion["liquid_mechanics"];
                assert_eq!(
                    mechanics["schema"],
                    if recoil {
                        "ftx-coupled-liquid-mechanics-result/6"
                    } else if phase_specific && scheduled {
                        "ftx-coupled-liquid-mechanics-result/5"
                    } else if phase_specific {
                        "ftx-coupled-liquid-mechanics-result/4"
                    } else if scheduled {
                        "ftx-coupled-liquid-mechanics-result/3"
                    } else if prescribed_velocity {
                        "ftx-coupled-liquid-mechanics-result/2"
                    } else {
                        "ftx-coupled-liquid-mechanics-result/1"
                    }
                );
                assert_eq!(mechanics, &heat["mechanical_coupling"]);
                assert_eq!(mechanics["qualification"], expected_qualification);
                assert_eq!(
                    heat["request"]["mechanical_coupling"],
                    expected_qualification
                );
                let water = &heat["surface_water"];
                let supplied = water["supplied_liquid_kg"].as_f64().unwrap();
                let condensed = if condensation {
                    water["condensed_liquid_kg"].as_f64().unwrap()
                } else {
                    0.
                };
                let incoming = mechanics["exchange"]["incoming_mass_kg"].as_f64().unwrap();
                assert!(incoming > 0.);
                if phase_specific && condensation {
                    assert!(supplied > 0. && condensed > 0.);
                }
                assert!((incoming - supplied - condensed).abs() < 1e-14);
                assert!(
                    (mechanics["remaining_liquid_mass_kg"].as_f64().unwrap()
                        - water["remaining_liquid_kg"].as_f64().unwrap())
                    .abs()
                        < 1e-14
                );
                assert!(
                    mechanics["exchange"]["incoming_kinetic_energy_j"]
                        .as_f64()
                        .unwrap()
                        > 0.
                );
                if prescribed_velocity {
                    let flow = &mechanics["exchange"];
                    assert!(flow["mixing_dissipation_j"].as_f64().unwrap() > 0.);
                    let first = if scheduled {
                        heat["trace"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .rev()
                            .find(|row| row["interval_index"] == 0)
                            .unwrap()
                    } else {
                        heat
                    };
                    let first_incoming =
                        first["mechanical_coupling"]["exchange"]["incoming_mass_kg"]
                            .as_f64()
                            .unwrap();
                    let later_incoming = incoming - first_incoming;
                    let first_supplied = first["surface_water"]["supplied_liquid_kg"]
                        .as_f64()
                        .unwrap();
                    let first_condensed = if condensation {
                        first["surface_water"]["condensed_liquid_kg"]
                            .as_f64()
                            .unwrap()
                    } else {
                        0.
                    };
                    if scheduled && phase_specific {
                        assert!(first_supplied > 0. && supplied > first_supplied);
                        if condensation {
                            assert!(first_condensed > 0. && condensed > first_condensed);
                        }
                    }
                    if scheduled && !condensation {
                        assert!(first_incoming > 0. && later_incoming > 0.);
                    }
                    for (axis, speed) in velocity.iter().enumerate() {
                        let momentum = if phase_specific && scheduled {
                            first_supplied * speed
                                + (supplied - first_supplied) * next_velocity[axis]
                                + first_condensed * vapor_velocity[axis]
                                + (condensed - first_condensed) * next_vapor_velocity[axis]
                        } else if phase_specific {
                            supplied * speed + condensed * vapor_velocity[axis]
                        } else {
                            first_incoming * speed + later_incoming * next_velocity[axis]
                        };
                        assert!(
                            (flow["incoming_momentum_kg_m_s"][axis].as_f64().unwrap() - momentum)
                                .abs()
                                < 1e-12
                        );
                    }
                    let kinetic = if phase_specific && scheduled {
                        0.5 * first_supplied * velocity.iter().map(|v| v * v).sum::<f64>()
                            + 0.5
                                * (supplied - first_supplied)
                                * next_velocity.iter().map(|v| v * v).sum::<f64>()
                            + 0.5
                                * first_condensed
                                * vapor_velocity.iter().map(|v| v * v).sum::<f64>()
                            + 0.5
                                * (condensed - first_condensed)
                                * next_vapor_velocity.iter().map(|v| v * v).sum::<f64>()
                    } else if phase_specific {
                        0.5 * supplied * velocity.iter().map(|v| v * v).sum::<f64>()
                            + 0.5 * condensed * vapor_velocity.iter().map(|v| v * v).sum::<f64>()
                    } else {
                        0.5 * first_incoming * velocity.iter().map(|v| v * v).sum::<f64>()
                            + 0.5
                                * later_incoming
                                * next_velocity.iter().map(|v| v * v).sum::<f64>()
                    };
                    assert!(
                        (flow["incoming_kinetic_energy_j"].as_f64().unwrap() - kinetic).abs()
                            < 1e-12
                    );
                    assert!(
                        mechanics["transfer_kinetic_residual_j"]
                            .as_f64()
                            .unwrap()
                            .abs()
                            < 1e-12
                    );
                    assert_eq!(
                        motion["initial_sample"]["liquid_mechanics"]["exchange"]["mixing_dissipation_j"],
                        0.
                    );
                    if recoil {
                        assert!(
                            mechanics["scope"]
                                .as_str()
                                .unwrap()
                                .contains("separation work")
                        );
                        assert!(
                            motion["thermal_basis"]
                                .as_str()
                                .unwrap()
                                .contains("separation work")
                        );
                        let evaporated = water["evaporated_liquid_kg"].as_f64().unwrap();
                        let drained = water["drained_liquid_kg"].as_f64().unwrap();
                        let first_evaporated = first["surface_water"]["evaporated_liquid_kg"]
                            .as_f64()
                            .unwrap();
                        let first_drained = first["surface_water"]["drained_liquid_kg"]
                            .as_f64()
                            .unwrap();
                        assert!(first_drained > 0. && drained > first_drained);
                        if !condensation {
                            assert!(first_evaporated > 0. && evaporated > first_evaporated);
                        }
                        assert!(
                            (flow["outgoing_mass_kg"].as_f64().unwrap() - evaporated - drained)
                                .abs()
                                < 1e-14
                        );
                        // Independent lower bound: separating the two outgoing phases carries
                        // this relative kinetic energy, in addition to retained-mass recoil.
                        let separation_minimum = 0.5
                            * (first_evaporated
                                * evaporation_velocity.iter().map(|v| v * v).sum::<f64>()
                                + (evaporated - first_evaporated)
                                    * next_evaporation_velocity.iter().map(|v| v * v).sum::<f64>()
                                + first_drained
                                    * drainage_velocity.iter().map(|v| v * v).sum::<f64>()
                                + (drained - first_drained)
                                    * next_drainage_velocity.iter().map(|v| v * v).sum::<f64>());
                        let separation = flow["outgoing_separation_work_j"].as_f64().unwrap();
                        assert!(separation_minimum > 0.);
                        assert!(separation >= separation_minimum * (1. - 1e-12));
                        assert_eq!(
                            motion["initial_sample"]["liquid_mechanics"]["exchange"]["outgoing_separation_work_j"],
                            0.
                        );
                        for residual in mechanics["transfer_momentum_residual_kg_m_s"]
                            .as_array()
                            .unwrap()
                        {
                            assert!(residual.as_f64().unwrap().abs() < 1e-12);
                        }
                        assert!(heat["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
                    } else {
                        assert!(flow.get("outgoing_separation_work_j").is_none());
                    }
                } else {
                    assert!(mechanics["exchange"].get("mixing_dissipation_j").is_none());
                }
                let alternative = keyed(&report["comparison"]["alternatives"], "qualified");
                assert_eq!(alternative["eligible"], true, "{alternative}");
                let readings = &alternative["readings"];
                assert!(readings[mass_metric]["value"].as_f64().unwrap() > 0.);
                assert_eq!(
                    keyed(&report["simulations"], "missing-ret")["evaluation"]["result"]["motion"]
                        ["status"],
                    "refused"
                );
            },
        );
    }
}

#[test]
fn variable_textile_water_replays_native_portable_and_named_case_enthalpy_decisions() {
    let (pattern, mut study, mut single) = inputs();
    for variant in study["variants"].as_array_mut().unwrap() {
        let record = &mut variant["changes"]["default"]["record"];
        let properties = record["properties"].as_object_mut().unwrap();
        properties.remove("specific_heat");
        properties.remove("thermal_conductivity");
        for key in ["areal_density", "thickness"] {
            properties.get_mut(key).unwrap()["conditions"] = json!({
                "specimen":"synthetic-scenario-sheet","conditioning":"synthetic-dry"});
        }
        record["thermal_response"] = json!({
            "specimen":"synthetic-scenario-sheet","conditioning":"synthetic-dry",
            "conductivity":{"unit":"W/(m*K)","evidence":"estimate","source":"test",
                "method":"Synthetic k(T)=.04+.0005*T, no measured textile claim",
                "points":[{"temperature_c":0.,"mean":0.04},{"temperature_c":60.,"mean":0.07}]},
            "specific_heat":{"unit":"J/(kg*K)","evidence":"estimate","source":"test",
                "method":"Synthetic Cp(T)=1000+20*T, no measured textile claim",
                "points":[{"temperature_c":0.,"mean":1000.},{"temperature_c":60.,"mean":2200.}]}});
    }
    let selection: Selection =
        serde_json::from_value(study["variants"][0]["changes"]["default"].clone()).unwrap();
    let material = Catalog::builtin().unwrap().resolve(&selection).unwrap();
    let triangles = ftx_drape::garment::build_garment(
        &ftx_pattern::Pattern::from_json(&pattern).unwrap(),
        single["options"]["spacing"].as_f64().unwrap(),
    )
    .unwrap()
    .faces
    .len();
    let duration = single["motion"]["keyframes"][0]["time_s"].as_f64().unwrap();
    let thermal_step = single["motion"]["transient_surface"]["max_step_s"]
        .as_f64()
        .unwrap();
    let regular_steps = (duration / thermal_step).ceil() as usize
        + single["motion"]["max_trials"].as_u64().unwrap() as usize;
    // Two cells per retained triangle, 129 operations per variable enthalpy inverse, at most
    // one dryout per initially wet triangle, plus regular steps split by accepted motion samples.
    // Four named-case/variant histories must still fit the existing 20-million session allowance.
    let work = 2 * triangles * 129 * (triangles + regular_steps);
    assert!(
        work <= 5_000_000,
        "variable fixture exceeds the scenario reservation: {work}"
    );
    let heat = &mut single["motion"]["transient_surface"];
    heat["cells_per_layer"] = json!(2);
    heat["max_cell_steps"] = json!(work);
    heat["surface_water"]["bindings"]["band"]["material_sha256"] = json!(material.sha256());
    heat["surface_water"]["qualification"] = json!(
        "Synthetic exact dry k(T)/Cp(T) profiles and fixed Ret retained over the declared water-loading domain; both dry laws restricted to [0,60] C"
    );
    replay(
        &pattern,
        &study,
        single,
        "FTX_VARIABLE_WATER_REPLAY",
        |report| {
            let result = &keyed(&report["simulations"], "qualified")["evaluation"]["result"];
            let outcome = &result["motion"]["result"]["transient_surface"];
            assert_eq!(outcome["status"], "available", "{outcome}");
            let heat = &outcome["result"];
            let water = &heat["surface_water"];
            let temperatures = heat["final"]["temperature_c"].as_array().unwrap();
            let mut dry_enthalpy = 0.;
            for (patch, ts) in heat["model"]["patches"]
                .as_array()
                .unwrap()
                .iter()
                .zip(temperatures)
            {
                let area = patch["reference_area_m2"].as_f64().unwrap();
                for t in ts.as_array().unwrap() {
                    let t = t.as_f64().unwrap();
                    // Independent integral of .2 kg/m² * Cp(T), divided between two cells.
                    dry_enthalpy += area * 0.1 * (1000. + 10. * (t + 20.)) * (t - 20.);
                }
            }
            assert!((dry_enthalpy - heat["stored_change_j"].as_f64().unwrap()).abs() < 1e-8);
            for column in heat["model"]["columns"].as_array().unwrap() {
                assert_eq!(column["schema"], "ftx-dry-thermal-model/2");
                assert_eq!(column["temperature_domain_c"], json!([0., 60.]));
                assert_eq!(column["layers"][0]["sha256"], material.sha256());
                assert!(column.get("cell_capacity_j_m2_k").is_none());
            }
            assert_eq!(water["remaining_liquid_kg"], 0.);
            let initial = water["initial_liquid_kg"].as_f64().unwrap();
            assert!(
                (initial - water["evaporated_liquid_kg"].as_f64().unwrap()).abs()
                    <= 1e-12 * initial
            );
            for sample in heat["trace"]
                .as_array()
                .unwrap()
                .iter()
                .chain(std::iter::once(heat))
            {
                let w = &sample["surface_water"];
                let energy = sample["stored_change_j"].as_f64().unwrap()
                    + w["liquid_stored_change_j"].as_f64().unwrap()
                    + w["exported_vapor_sensible_energy_j"].as_f64().unwrap()
                    + w["latent_energy_j"].as_f64().unwrap()
                    - sample["net_boundary_energy_j"].as_f64().unwrap();
                assert!(
                    energy.abs() < 1e-8,
                    "variable-film energy residual {energy}"
                );
            }
            let readings = &keyed(&report["comparison"]["alternatives"], "qualified")["readings"];
            assert_eq!(
                readings["textile_water_evaporated_mass_kg"]["value"],
                water["evaporated_liquid_kg"]
            );
            assert!(
                readings["transient_surface_final_min_temperature_c"]["value"]
                    .as_f64()
                    .unwrap()
                    < 20.
            );
            assert_eq!(
                keyed(&report["simulations"], "missing-ret")["evaluation"]["result"]["motion"]["result"]
                    ["transient_surface"]["status"],
                "refused"
            );
        },
    );
}

#[test]
fn coupled_textile_water_objectives_replay_in_both_native_commands_and_portable_scenarios() {
    let (pattern, study, single) = inputs();
    replay(
        &pattern,
        &study,
        single,
        "FTX_COUPLED_WATER_REPLAY",
        |report| {
            let readings = &keyed(&report["comparison"]["alternatives"], "qualified")["readings"];
            assert!(
                readings["transient_surface_final_min_temperature_c"]["value"]
                    .as_f64()
                    .unwrap()
                    < 20.
            );
            assert_eq!(readings["textile_water_remaining_mass_kg"]["value"], 0.);
            for metric in [
                "textile_water_evaporated_mass_kg",
                "textile_water_latent_energy_j",
            ] {
                assert!(readings[metric]["value"].as_f64().unwrap() > 0.);
            }
        },
    );
}

#[test]
fn condensation_and_reevaporation_replay_through_actual_selected_body_decisions() {
    let (pattern, mut study, mut single) = inputs();
    let heat = &mut single["motion"]["transient_surface"];
    heat["boundaries"] = json!([heat["boundaries"][0], heat["boundaries"][0]]);
    let water = &mut heat["surface_water"];
    water["schema"] = json!("ftx-textile-surface-water/3");
    water
        .as_object_mut()
        .unwrap()
        .remove("replenishment_kg_m2_s");
    water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(0.);
    water["outside_vapor_pressure_pa"] = json!([6000., 1000.]);
    water["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
        "bindings":{"band":{"rate_kg_m2_s":[0.,0.],"temperature_c":[20.,20.]}},
        "source":{"reference":"test:no-delivered-liquid","accessed_on":"2026-10-27",
            "note":"Synthetic zero liquid delivery; ambient vapor is the only mass source"},
        "evidence":"estimate","allow_estimates":true,"method":"Declared zero liquid feed"});
    let condensation = json!({"schema":"ftx-textile-condensation/1", "vapor_temperature_c":[40.,10.],
        "source":{"reference":"test:selected-body-condensation","accessed_on":"2026-10-27",
            "note":"Synthetic reversible Ret and wet/dry nucleation; no empirical garment claim"},
        "evidence":"estimate","allow_estimates":true,"method":"Prescribed two-interval vapor reservoir",
        "qualification":"Synthetic source closure for reverse locked Ret, wet/dry phase transfer and constant latent plus cp_liquid effective sensible enthalpy over the declared domains"});
    water["condensation"] = condensation.clone();
    single["motion"]["keyframes"] = json!([
        {"time_s":0.0005,"arm_abduction_deg":35.},
        {"time_s":0.001,"arm_abduction_deg":35.}]);
    study["comparison"]["objectives"] = json!([
        {"metric":"textile_water_condensed_mass_kg","direction":"minimize"},
        {"metric":"textile_water_condensation_latent_energy_j","direction":"minimize"},
        {"metric":"transient_surface_final_min_temperature_c","direction":"minimize"}]);
    replay(
        &pattern,
        &study,
        single,
        "FTX_CONDENSED_WATER_REPLAY",
        |report| {
            let result = &keyed(&report["simulations"], "qualified")["evaluation"]["result"];
            let outcome = &result["motion"]["result"]["transient_surface"];
            assert_eq!(outcome["status"], "available", "{outcome}");
            let heat = &outcome["result"];
            let water = &heat["surface_water"];
            assert_eq!(water["schema"], "ftx-textile-surface-water-result/3");
            assert_eq!(water["request"]["condensation"], condensation);
            assert_eq!(water["initial_liquid_kg"], 0.);
            assert_eq!(water["supplied_liquid_kg"], 0.);
            assert_eq!(water["remaining_liquid_kg"], 0.);
            let condensed = water["condensed_liquid_kg"].as_f64().unwrap();
            assert!(condensed > 0.);
            assert!(
                (water["evaporated_liquid_kg"].as_f64().unwrap() - condensed).abs()
                    < 1e-12 * condensed
            );
            let first = &heat["trace"][0]["surface_water"];
            assert!(first["remaining_liquid_kg"].as_f64().unwrap() > 0.);
            assert_eq!(first["evaporated_liquid_kg"], 0.);
            for sample in heat["trace"]
                .as_array()
                .unwrap()
                .iter()
                .chain(std::iter::once(heat))
            {
                let w = &sample["surface_water"];
                let mass = w["remaining_liquid_kg"].as_f64().unwrap()
                    + w["evaporated_liquid_kg"].as_f64().unwrap()
                    - w["condensed_liquid_kg"].as_f64().unwrap();
                assert!(mass.abs() <= 1e-12 * condensed);
                let energy = sample["stored_change_j"].as_f64().unwrap()
                    + w["liquid_stored_change_j"].as_f64().unwrap()
                    + w["exported_vapor_sensible_energy_j"].as_f64().unwrap()
                    + w["latent_energy_j"].as_f64().unwrap()
                    - w["incoming_vapor_sensible_energy_j"].as_f64().unwrap()
                    - w["condensation_latent_energy_j"].as_f64().unwrap()
                    - sample["net_boundary_energy_j"].as_f64().unwrap();
                assert!(
                    energy.abs() < 1e-10,
                    "phase-change energy residual {energy}"
                );
                assert!(
                    (w["incoming_vapor_sensible_energy_j"].as_f64().unwrap()
                        - w["condensed_liquid_kg"].as_f64().unwrap() * 4200. * 40.)
                        .abs()
                        < 1e-12
                );
            }
            let readings = &keyed(&report["comparison"]["alternatives"], "qualified")["readings"];
            assert_eq!(
                readings["textile_water_condensed_mass_kg"]["value"],
                water["condensed_liquid_kg"]
            );
            assert_eq!(
                readings["textile_water_condensation_latent_energy_j"]["value"],
                water["condensation_latent_energy_j"]
            );
            assert_eq!(
                keyed(&report["simulations"], "missing-ret")["evaluation"]["result"]["motion"]["result"]
                    ["transient_surface"]["status"],
                "refused"
            );
        },
    );
}

#[test]
fn drained_textile_water_replays_in_native_portable_and_named_case_decisions() {
    let (pattern, mut study, mut single) = inputs();
    let water = &mut single["motion"]["transient_surface"]["surface_water"];
    water["schema"] = json!("ftx-textile-surface-water/4");
    water
        .as_object_mut()
        .unwrap()
        .remove("replenishment_kg_m2_s");
    water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(0.02);
    water["maximum_liquid_loading_kg_m2"] = json!(0.1);
    water["qualification"] =
        json!("Synthetic fixed dry thermal and vapor laws across [-20,80] C and [0,0.1] kg/m2");
    water["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
        "bindings":{"band":{"rate_kg_m2_s":[0.02],"temperature_c":[30.]}},
        "source":{"reference":"test:drained-selected-body-feed","accessed_on":"2026-10-30",
            "note":"Synthetic liquid delivery for replay, not measured wearer transfer"},
        "evidence":"estimate","allow_estimates":true,"method":"Prescribed reference-area feed"});
    water["drainage"] = json!({"schema":"ftx-textile-liquid-drainage/1",
        "bindings":{"band":{"rate_kg_m2_s":[0.01]}},
        "source":{"reference":"test:drained-selected-body-outflow","accessed_on":"2026-10-30",
            "note":"Synthetic prescribed outflow, not measured textile runoff"},
        "evidence":"estimate","allow_estimates":true,"method":"Independent reference-area removal",
        "qualification":"Co-moving stock-limited drainage at old inner-cell temperature; proportional evaporation and drainage at empty stock"});
    study["comparison"]["objectives"] = json!([
        {"metric":"textile_water_drained_mass_kg","direction":"minimize"}]);
    study["comparison"]["constraints"][0] =
        json!({"metric":"textile_water_remaining_mass_kg","minimum":0.});
    qualify_mechanics(&pattern, &study, &mut single, true);
    replay(
        &pattern,
        &study,
        single,
        "FTX_DRAINED_WATER_MECHANICS_REPLAY",
        |report| {
            let simulated = &keyed(&report["simulations"], "qualified")["evaluation"]["result"];
            let motion = &simulated["motion"]["result"];
            let heat = &motion["transient_surface"]["result"];
            let water = &heat["surface_water"];
            let mechanics = &motion["liquid_mechanics"];
            assert_eq!(water["schema"], "ftx-textile-surface-water-result/4");
            assert_eq!(heat["mechanical_coupling"], *mechanics);
            let evaporated = water["evaporated_liquid_kg"].as_f64().unwrap();
            let drained = water["drained_liquid_kg"].as_f64().unwrap();
            assert!(evaporated > 0. && drained > 0.);
            assert!(
                (mechanics["evaporated_liquid_mass_kg"].as_f64().unwrap() - evaporated).abs()
                    < 1e-14
            );
            assert!(
                (mechanics["drained_liquid_mass_kg"].as_f64().unwrap() - drained).abs() < 1e-14
            );
            assert!(
                (mechanics["exchange"]["outgoing_mass_kg"].as_f64().unwrap()
                    - evaporated
                    - drained)
                    .abs()
                    < 1e-14
            );
            for sample in heat["trace"].as_array().unwrap() {
                let w = &sample["surface_water"];
                let e = w["evaporated_liquid_kg"].as_f64().unwrap();
                let d = w["drained_liquid_kg"].as_f64().unwrap();
                let balance = w["remaining_liquid_kg"].as_f64().unwrap() + e + d
                    - w["initial_liquid_kg"].as_f64().unwrap()
                    - w["supplied_liquid_kg"].as_f64().unwrap();
                assert!(balance.abs() < 1e-14);
                assert!(
                    (sample["mechanical_coupling"]["drained_liquid_kg"]
                        .as_f64()
                        .unwrap()
                        - d)
                        .abs()
                        < 1e-14
                );
                assert!(sample["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
            }
            assert!(heat["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
            assert_eq!(
                keyed(&report["comparison"]["alternatives"], "qualified")["readings"]["textile_water_drained_mass_kg"]
                    ["value"],
                water["drained_liquid_kg"]
            );
            assert_eq!(
                keyed(&report["simulations"], "missing-ret")["evaluation"]["result"]["motion"]["status"],
                "refused"
            );
        },
    );
}

#[test]
fn finite_vapor_inventory_replays_humidity_feedback_and_coupled_ledgers_in_native_and_portable_decisions()
 {
    use ftx_wasm::materials::scenario::on_model;

    let (pattern, mut study, mut single) = inputs();
    let capacitance = 1e-15;
    let initial_pressure = 1000.0;
    let initial_vapor = capacitance * initial_pressure;
    let reservoir = json!({"schema":"ftx-textile-vapor-reservoir/1",
        "model":"isothermal_constant_vapor_capacitance/1",
        "vapor_capacitance_kg_pa":capacitance,"initial_vapor_mass_kg":initial_vapor,
        "temperature_c":20.,"pressure_domain_pa":[0.,10000.],
        "max_iterations":128,"mass_tolerance_kg":1e-18,
        "source":{"reference":"test:selected-body-finite-vapor","accessed_on":"2026-11-01",
            "note":"Synthetic tiny isothermal inventory for an exhaustion control; no measured atmosphere or textile claim"},
        "evidence":"estimate","allow_estimates":true,"method":"Explicit finite mass per pressure and fixed effective enthalpy",
        "qualification":"Artificial well-mixed constant capacitance over [0,10000] Pa at 20 C, with constant latent plus cp_liquid effective sensible enthalpy; no gas heat capacity or inferred ideal-gas law"});
    let water = &mut single["motion"]["transient_surface"]["surface_water"];
    water["schema"] = json!("ftx-textile-surface-water/5");
    water
        .as_object_mut()
        .unwrap()
        .remove("replenishment_kg_m2_s");
    water
        .as_object_mut()
        .unwrap()
        .remove("outside_vapor_pressure_pa");
    water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(0.);
    // The constant zero curve isolates the inventory feedback from temperature-dependent vapor
    // pressure. It is an artificial constitutive control, not a physical saturation-pressure law.
    water["wet_vapor_pressure_pa"] = json!([[-20., 0.], [80., 0.]]);
    water["qualification"] = json!(
        "Synthetic fixed dry laws and artificial zero wet pressure over [-20,80] C and [0,1e-5] kg/m2; not measured condensation"
    );
    water["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
        "bindings":{"band":{"rate_kg_m2_s":[0.],"temperature_c":[20.]}},
        "source":{"reference":"test:finite-vapor-zero-feed","accessed_on":"2026-11-01",
            "note":"Explicit zero liquid delivery isolates the shared finite vapor stock"},
        "evidence":"estimate","allow_estimates":true,"method":"Declared zero co-moving liquid feed"});
    water["condensation"] = json!({"schema":"ftx-textile-condensation/1","vapor_temperature_c":[20.],
        "source":{"reference":"test:finite-vapor-condensation","accessed_on":"2026-11-01",
            "note":"Synthetic reverse Ret, wet/dry nucleation and effective vapor enthalpy only"},
        "evidence":"estimate","allow_estimates":true,"method":"Artificial bidirectional free-film transfer",
        "qualification":"Exact artificial Ret in both directions across the declared pressure, temperature and loading domains; constant latent plus cp_liquid sensible convention"});
    water["vapor_reservoir"] = reservoir.clone();
    let material_lock = water["bindings"]["band"]["material_sha256"].clone();
    study["comparison"]["objectives"] = json!([
        {"metric":"textile_water_condensed_mass_kg","direction":"minimize"}]);
    study["comparison"]["constraints"][0] =
        json!({"metric":"textile_water_remaining_mass_kg","minimum":0.});
    qualify_mechanics(&pattern, &study, &mut single, true);

    // The same real sewn geometry and source-qualified Ret against a fixed-pressure atmosphere
    // would draw far more than the finite initial stock. This is a production numerical control.
    let mut infinite = single.clone();
    let water = &mut infinite["motion"]["transient_surface"]["surface_water"];
    water["schema"] = json!("ftx-textile-surface-water/3");
    water.as_object_mut().unwrap().remove("vapor_reservoir");
    water["outside_vapor_pressure_pa"] = json!([initial_pressure]);
    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model = ftx_wasm::fold::load_body(&bytes).unwrap();
    let body = json!({"schema":"ftx-scenario-body/1","accepted_body_fit":receipt(&digest),
        "arm_abduction_deg":35.0});
    let baseline: Value = serde_json::from_str(
        &on_model(
            &model,
            &digest,
            &pattern,
            &study.to_string(),
            &infinite.to_string(),
            &body.to_string(),
            |_| {},
        )
        .unwrap(),
    )
    .unwrap();
    let original = &keyed(&baseline["simulations"], "qualified")["evaluation"]["result"];
    let baseline_motion = &original["motion"];
    assert_eq!(baseline_motion["status"], "available", "{baseline_motion}");
    let baseline_water = &baseline_motion["result"]["transient_surface"]["result"]["surface_water"];
    assert!(baseline_water["condensed_liquid_kg"].as_f64().unwrap() > 100. * initial_vapor);

    replay(
        &pattern,
        &study,
        single,
        "FTX_FINITE_VAPOR_RESERVOIR_REPLAY",
        |report| {
            let simulated = &keyed(&report["simulations"], "qualified")["evaluation"]["result"];
            let motion = &simulated["motion"]["result"];
            let heat = &motion["transient_surface"]["result"];
            let water = &heat["surface_water"];
            let gas = &water["vapor_reservoir"];
            let mechanics = &motion["liquid_mechanics"];
            assert_eq!(water["schema"], "ftx-textile-surface-water-result/5");
            assert_eq!(water["request"]["vapor_reservoir"], reservoir);
            assert!(water["request"].get("outside_vapor_pressure_pa").is_none());
            assert_eq!(
                water["request"]["bindings"]["band"]["material_sha256"],
                material_lock
            );
            assert_eq!(heat["mechanical_coupling"], *mechanics);
            assert_eq!(
                mechanics["qualification"],
                heat["request"]["mechanical_coupling"]
            );
            assert_eq!(
                simulated["drape"]["material"],
                original["drape"]["material"]
            );
            assert_eq!(simulated["positions_sha256"], original["positions_sha256"]);
            assert_eq!(
                report["scenario"]["body_fit"],
                baseline["scenario"]["body_fit"]
            );
            assert_eq!(
                report["scenario"]["body_geometry_sha256"],
                baseline["scenario"]["body_geometry_sha256"]
            );
            assert_eq!(
                motion["initial_sample"]["body_positions_m"],
                baseline_motion["result"]["initial_sample"]["body_positions_m"]
            );

            let condensed = water["condensed_liquid_kg"].as_f64().unwrap();
            assert!(condensed > 0.99 * initial_vapor && condensed <= initial_vapor);
            assert!(gas["remaining_vapor_mass_kg"].as_f64().unwrap() < initial_vapor * 1e-6);
            assert!(gas["vapor_pressure_pa"].as_f64().unwrap() < 0.001);
            assert_eq!(gas["elapsed_seconds"], motion["duration_s"]);
            assert_eq!(water["initial_liquid_kg"], 0.);
            assert_eq!(water["supplied_liquid_kg"], 0.);
            assert_eq!(water["evaporated_liquid_kg"], 0.);
            assert!(
                (mechanics["remaining_liquid_mass_kg"].as_f64().unwrap() - condensed).abs() < 1e-23
            );
            assert!(
                (mechanics["exchange"]["incoming_mass_kg"].as_f64().unwrap() - condensed).abs()
                    < 1e-23
            );
            assert_eq!(mechanics["exchange"]["outgoing_mass_kg"], 0.);

            let trace = heat["trace"].as_array().unwrap();
            assert!(trace.len() >= 2);
            let first_condensed = trace[0]["surface_water"]["condensed_liquid_kg"]
                .as_f64()
                .unwrap();
            assert!(first_condensed > 0.);
            assert!(
                condensed - first_condensed < first_condensed * 0.01,
                "depleted humidity must throttle subsequent condensation"
            );
            let mut previous_pressure = initial_pressure;
            for sample in trace.iter().chain(std::iter::once(heat)) {
                let w = &sample["surface_water"];
                let gas = &w["vapor_reservoir"];
                let remaining_vapor = gas["remaining_vapor_mass_kg"].as_f64().unwrap();
                let pressure = gas["vapor_pressure_pa"].as_f64().unwrap();
                assert!(pressure >= 0. && pressure <= previous_pressure);
                previous_pressure = pressure;
                assert!((remaining_vapor - capacitance * pressure).abs() < 1e-25);
                let mass_error =
                    remaining_vapor + w["remaining_liquid_kg"].as_f64().unwrap() - initial_vapor;
                assert!(
                    mass_error.abs() < 1e-23,
                    "combined vapor/liquid inventory {mass_error}"
                );
                assert!(gas["mass_balance_error_kg"].as_f64().unwrap().abs() < 1e-23);
                let vapor_storage = gas["vapor_enthalpy_change_j"].as_f64().unwrap();
                let thermostat = gas["isothermal_heat_removed_j"].as_f64().unwrap();
                let incoming = gas["net_vapor_enthalpy_received_j"].as_f64().unwrap();
                assert!((vapor_storage + thermostat - incoming).abs() < 1e-15);
                let energy_error = sample["stored_change_j"].as_f64().unwrap()
                    + w["liquid_stored_change_j"].as_f64().unwrap()
                    + vapor_storage
                    + thermostat
                    - sample["net_boundary_energy_j"].as_f64().unwrap();
                assert!(
                    energy_error.abs() < 1e-8,
                    "combined cloth/liquid/vapor energy {energy_error}"
                );
                assert!(sample["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
            }
            assert_eq!(
                keyed(&report["comparison"]["alternatives"], "qualified")["readings"]["textile_water_condensed_mass_kg"]
                    ["value"],
                water["condensed_liquid_kg"]
            );
            assert_eq!(
                keyed(&report["simulations"], "missing-ret")["evaluation"]["result"]["motion"]["status"],
                "refused"
            );
            assert_eq!(heat["physical_validation_claim"], false);
        },
    );
}

#[test]
fn supplied_textile_water_replays_in_native_commands_and_portable_scenario_matrices() {
    let (pattern, mut study, mut single) = inputs();
    single["material_strain"] = json!({"schema":"ftx-material-strain/1",
        "instances":["band"],"max_triangle_samples":100000});
    single["contact_loads"] = json!({"schema":"ftx-contact-loads/1",
        "instances":["band"],"max_vertex_samples":100000,"max_pair_evaluations":1000000});
    // Exercise retained per-piece sources in the same native/portable comparisons.
    for variant in study["variants"].as_array_mut().unwrap() {
        let selection = variant["changes"]["default"].clone();
        variant["changes"]["pieces"] = json!({"band":selection});
    }
    let water = &mut single["motion"]["transient_surface"]["surface_water"];
    water["schema"] = json!("ftx-textile-surface-water/2");
    water
        .as_object_mut()
        .unwrap()
        .remove("replenishment_kg_m2_s");
    water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(0.);
    let supply = json!({"schema":"ftx-textile-liquid-supply/1",
        "bindings":{"band":{"rate_kg_m2_s":[0.001],"temperature_c":[40.]}},
        "source":{"reference":"test:selected-body-liquid-feed","accessed_on":"2026-10-23",
            "note":"Synthetic delivery and temperature evidence; no wearer or garment validation"},
        "evidence":"estimate","allow_estimates":true,"method":"Prescribed reference-area delivery"});
    water["supply"] = supply.clone();
    study["comparison"]["objectives"] = json!([
        {"metric":"textile_water_supplied_mass_kg","direction":"minimize"},
        {"metric":"transient_surface_final_min_temperature_c","direction":"minimize"},
        {"metric":"motion_max_warp_green_strain","direction":"minimize"},
        {"metric":"motion_peak_body_contact_resultant_n","direction":"minimize"},
        {"metric":"motion_max_body_contact_vertex_force_n","direction":"minimize"},
        {"metric":"body_contact_resultant_n","direction":"minimize"},
        {"metric":"max_body_contact_vertex_force_n","direction":"minimize"}]);
    study["comparison"]["constraints"][0] =
        json!({"metric":"textile_water_remaining_mass_kg","minimum":0.});
    study["comparison"]["constraints"]
        .as_array_mut()
        .unwrap()
        .push(json!({"metric":"max_engineering_shear_green_strain","maximum":1.}));
    replay(
        &pattern,
        &study,
        single,
        "FTX_SUPPLIED_WATER_REPLAY",
        |report| {
            let result = &keyed(&report["simulations"], "qualified")["evaluation"]["result"];
            let outcome = &result["motion"]["result"]["transient_surface"];
            assert_eq!(outcome["status"], "available");
            let heat = &outcome["result"];
            let water = &heat["surface_water"];
            assert_eq!(water["schema"], "ftx-textile-surface-water-result/2");
            assert_eq!(water["request"]["supply"], supply);
            assert_eq!(water["initial_liquid_kg"], 0.);
            assert!(water["remaining_liquid_kg"].as_f64().unwrap() > 0.);
            assert!(water["evaporated_liquid_kg"].as_f64().unwrap() > 0.);
            let near = |actual: f64, expected: f64| {
                assert!(
                    (actual - expected).abs() <= 1e-11 * actual.abs().max(expected.abs()),
                    "{actual} != {expected}"
                );
            };
            let mut area = 0.;
            for (i, patch) in water["patches"].as_array().unwrap().iter().enumerate() {
                let reference_area = heat["model"]["patches"][i]["reference_area_m2"]
                    .as_f64()
                    .unwrap();
                area += reference_area;
                near(
                    patch["supplied_liquid_kg"].as_f64().unwrap(),
                    reference_area * 0.001 * 0.001,
                );
                near(
                    patch["supplied_liquid_sensible_energy_j"].as_f64().unwrap(),
                    reference_area * 0.001 * 0.001 * 4200. * 40.,
                );
            }
            for sample in heat["trace"].as_array().unwrap() {
                let w = &sample["surface_water"];
                let supplied = area * 0.001 * sample["time_s"].as_f64().unwrap();
                near(w["supplied_liquid_kg"].as_f64().unwrap(), supplied);
                near(
                    w["supplied_liquid_sensible_energy_j"].as_f64().unwrap(),
                    supplied * 4200. * 40.,
                );
                near(
                    w["remaining_liquid_kg"].as_f64().unwrap()
                        + w["evaporated_liquid_kg"].as_f64().unwrap(),
                    supplied,
                );
                let energy = sample["stored_change_j"].as_f64().unwrap()
                    + w["liquid_stored_change_j"].as_f64().unwrap()
                    + w["exported_vapor_sensible_energy_j"].as_f64().unwrap()
                    + w["latent_energy_j"].as_f64().unwrap()
                    - w["supplied_liquid_sensible_energy_j"].as_f64().unwrap()
                    - sample["net_boundary_energy_j"].as_f64().unwrap();
                assert!(
                    energy.abs() < 1e-10,
                    "supplied-film energy residual {energy}"
                );
            }
            let readings = &keyed(&report["comparison"]["alternatives"], "qualified")["readings"];
            assert!(
                readings["textile_water_supplied_mass_kg"]["value"]
                    .as_f64()
                    .unwrap()
                    > 0.
            );
            assert_eq!(
                readings["textile_water_supplied_mass_kg"]["value"],
                water["supplied_liquid_kg"]
            );
            assert_eq!(result["material_strain"]["status"], "available");
            assert_eq!(result["contact_loads"]["status"], "available");
            let material = &result["drape"]["material"];
            assert_eq!(material["basis"], "per_piece_characterization");
            let material_hash =
                ftx_core::receipt::sha256_hex(&serde_json::to_vec(material).unwrap());
            assert_eq!(
                result["drape"]["contact_diagnostics"]["final_reactions"]["result"]["material_report_sha256"],
                material_hash
            );
            assert_eq!(
                result["contact_loads"]["result"]["source"]["material_report_sha256"],
                material_hash
            );
            assert_eq!(
                result["contact_loads"]["result"]["source"]["instances"]["band"],
                material["instances"][0]["record_sha256"]
            );
            let contact = &result["motion"]["result"]["contact_loads"];
            assert_eq!(contact["status"], "available", "{contact}");
            for (metric, field) in [
                (
                    "motion_peak_body_contact_resultant_n",
                    "peak_body_resultant_n",
                ),
                (
                    "motion_max_body_contact_vertex_force_n",
                    "max_body_vertex_force_n",
                ),
            ] {
                assert_eq!(readings[metric]["value"], contact["result"][field]);
                assert_eq!(readings[metric]["unit"], "N");
            }
            assert_eq!(
                readings["body_contact_resultant_n"]["value"],
                result["contact_loads"]["result"]["peak_body_resultant_n"]
            );
            assert_eq!(
                readings["max_body_contact_vertex_force_n"]["value"],
                result["contact_loads"]["result"]["max_body_vertex_force_n"]
            );
            let strain = &result["motion"]["result"]["material_strain"];
            assert_eq!(strain["status"], "available");
            assert_eq!(
                readings["motion_max_warp_green_strain"]["value"],
                strain["result"]["green_strain_range"][0][1]
            );
            assert_eq!(
                readings["max_engineering_shear_green_strain"]["value"],
                result["material_strain"]["result"]["green_strain_range"][2][1]
            );
            assert_eq!(
                keyed(&report["simulations"], "missing-ret")["evaluation"]["result"]["motion"]["result"]
                    ["transient_surface"]["status"],
                "refused"
            );
        },
    );
}
