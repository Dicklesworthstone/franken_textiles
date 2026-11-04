//! Whole-path rollback after accepted evaporation and a later thermal refusal.
use super::*;
use crate::{PartFrame, scene::PartFrames};
use ftx_core::receipt::sha256_hex;
use ftx_pattern::{Pattern, material_catalog::Selection, materials::Catalog};

struct Form;
impl PartFrames for Form {
    fn frame(&self, _: &str) -> Option<PartFrame> {
        Some(PartFrame {
            origin: Vec3::ZERO,
            up: Vec3::new(0., 0., 1.),
            front: Vec3::new(0., -1., 0.),
            lat: Vec3::new(1., 0., 0.),
        })
    }
    fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
        Some(20.)
    }
    fn section_radius(&self, _: &str, _: f64) -> f64 {
        30.
    }
}

#[test]
fn late_evaporation_budget_failure_restores_whole_solver_and_replays_identically() {
    rollback_after_liquid_transfer(false, false, false, false, false, false);
}

#[test]
fn late_supply_budget_failure_rolls_back_liquid_and_attached_mass_together() {
    rollback_after_liquid_transfer(true, false, false, false, false, false);
}

#[test]
fn prescribed_supply_impulse_and_mixing_roll_back_and_replay_with_attached_mass() {
    rollback_after_liquid_transfer(true, true, false, false, false, false);
}

#[test]
fn phase_specific_supply_impulse_rolls_back_and_replays_with_attached_mass() {
    rollback_after_liquid_transfer(true, true, true, false, false, false);
}

#[test]
fn scheduled_supply_follows_keyframes_and_rolls_back_with_attached_mass() {
    rollback_after_liquid_transfer(true, true, false, true, false, false);
}

#[test]
fn scheduled_incoming_water_and_drainage_roll_back_and_replay_together() {
    rollback_after_liquid_transfer(true, true, false, true, true, false);
}

#[test]
fn phase_specific_incoming_water_and_drainage_roll_back_and_replay_together() {
    rollback_after_liquid_transfer(true, true, true, false, true, false);
}

#[test]
fn paired_water_scheduled_phase_velocities_roll_back_and_replay() {
    rollback_after_liquid_transfer(true, true, true, true, true, false);
}

#[test]
fn paired_water_outgoing_recoil_rolls_back_and_replays() {
    rollback_after_liquid_transfer(true, true, true, true, true, true);
}

fn rollback_after_liquid_transfer(
    exchange: bool,
    prescribed: bool,
    phase_specific: bool,
    scheduled: bool,
    drainage: bool,
    recoil: bool,
) {
    let mut raw: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    for (key, value, unit) in [
        ("specific_heat", 1000., "J/(kg*K)"),
        ("vapor_resistance", 8., "m2*Pa/W"),
    ] {
        raw["fabric"]["material"]["record"]["properties"][key] = json!({
            "value":value,"unit":unit,"source":"test","evidence":"estimate",
            "method":"Synthetic rollback case; not measured textile data"});
    }
    if exchange {
        raw["attached_mass"] = json!({"schema":"ftx-attached-mass/1","groups":[{
            "extra":{"id":"synthetic-closure","mass":{"value":0.0003,"unit":"kg",
                "evidence":"estimate","source":"test","method":"Artificial retained accessory"},
                "sources":{"test":{"reference":"test:incoming-water-trim","accessed_on":"2026-10-28",
                    "note":"Synthetic mass only; no attachment strength claim"}},"allow_estimates":true},
            "targets":[{"instance":"band","vertex":"a","fraction":0.25},
                {"instance":"band","vertex":"b","fraction":0.75}]}]});
    }
    let selection: Selection = serde_json::from_value(raw["fabric"]["material"].clone()).unwrap();
    let material = Catalog::builtin().unwrap().resolve(&selection).unwrap();
    let pattern = Pattern::new(raw);
    let scenario: Scenario = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario.json"
    ))
    .unwrap();
    let mut vertices = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = ftx_core::det::sin_cos(i as f64 * std::f64::consts::TAU / 16.);
            vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    let triangles: Vec<_> = (0..16)
        .flat_map(|i| {
            let j = (i + 1) % 16;
            [[i, j, j + 16], [i, j + 16, i + 16]]
        })
        .collect();
    let body = DrapeBody {
        frames: &Form,
        collision: (&vertices, &triangles),
        display: (&vertices, &triangles),
        arm_deg: 35.,
    };
    let mut run = crate::run::drape(
        &pattern,
        &scenario.fit_spec,
        &body,
        &scenario.options,
        |_| {},
    )
    .unwrap();
    let original = run.scene.solver.clone();
    let original_debug = format!("{original:?}");
    let material_report_sha256 = sha256_hex(&serde_json::to_vec(&run.report["material"]).unwrap());
    let mut request: Request = serde_json::from_value(json!({
        "schema":"ftx-drape-motion/1",
        "keyframes":[{"time_s":0.002,"arm_abduction_deg":35.},
            {"time_s":0.004,"arm_abduction_deg":36.}],
        "max_step_s":0.001,"max_trials":16,"max_pairs":100000,
        "max_sample_vertices":100000,
        "transient_surface":{
            "schema":"ftx-motion-surface-heat/1","initial_temperature_c":15.,
            "cells_per_layer":1,"max_step_s":0.0005,"max_cell_steps":1000000,
            "boundaries":[
                {"inside_c":40.,"outside_c":5.,"inside_resistance_m2_k_w":0.1,
                    "outside_resistance_m2_k_w":0.2},
                {"inside_c":5.,"outside_c":40.,"inside_resistance_m2_k_w":0.1,
                    "outside_resistance_m2_k_w":0.2}],
            "surface_water":{
                "schema":"ftx-textile-surface-water/1",
                "model":"inner_cell_equilibrium_free_film_frozen_properties/1",
                "bindings":{"band":{"material_sha256":material.sha256(),
                    "initial_liquid_kg_m2":0.1}},
                "maximum_liquid_loading_kg_m2":0.1,"liquid_specific_heat_j_kg_k":1000.,
                "latent_heat_j_kg":10.,"enthalpy_reference_c":0.,
                "wet_vapor_pressure_pa":[[-20.,0.],[80.,10000.]],
                "outside_vapor_pressure_pa":[1000.,1000.],
                "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":1.,
                "replenishment_kg_m2_s":0.,
                "source":{"reference":"test:rollback-water","accessed_on":"2026-10-28",
                    "note":"Synthetic low latent heat for bounded short-path evaporation"},
                "evidence":"estimate","allow_estimates":true,
                "method":"Artificial conservation regression",
                "qualification":"Synthetic invariant thermal and vapor laws over the declared domain"},
            "mechanical_coupling":{
                "schema":"ftx-evaporating-liquid-mechanics/1",
                "model":"co_moving_evaporation_frozen_mechanics/1",
                "material_report_sha256":material_report_sha256,
                "bindings":{"band":{"material_sha256":material.sha256(),
                    "maximum_liquid_loading_kg_m2":0.1,"temperature_range_c":[-20.,80.]}},
                "source":{"reference":"test:rollback-mechanics","accessed_on":"2026-10-28",
                    "note":"Artificial invariant mechanics; no empirical wet-garment claim"},
                "evidence":"estimate","allow_estimates":true,
                "method":"Whole-solver transactional replay",
                "qualification":"Exact synthetic membrane, bending and contact laws retained over the complete thermal domain"}}}))
    .unwrap();
    if exchange {
        let heat = request.transient_surface.as_mut().unwrap();
        let q = heat.mechanical_coupling.as_mut().unwrap();
        q.schema = "ftx-coupled-liquid-mechanics/1".into();
        q.model = "co_moving_mass_exchange_frozen_mechanics/1".into();
        q.qualification = "Exact synthetic mechanics with co-moving incoming/outgoing flow over the declared domain".into();
        if prescribed {
            q.schema = "ftx-coupled-liquid-mechanics/2".into();
            q.model = "prescribed_incoming_velocity_mass_exchange_frozen_mechanics/1".into();
            q.bindings
                .get_mut("band")
                .unwrap()
                .incoming_velocity_world_m_s = Some(Vec3::new(0.1, -0.2, 0.05));
            q.qualification = "Synthetic prescribed incoming velocity and inelastic mixing with invariant mechanical laws over the declared domain".into();
            if phase_specific {
                q.schema = "ftx-coupled-liquid-mechanics/4".into();
                q.model =
                    "phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1".into();
                let binding = q.bindings.get_mut("band").unwrap();
                binding.incoming_velocity_world_m_s = None;
                binding.incoming_stream_velocities_world_m_s =
                    Some(crate::liquid_mass::IncomingVelocities {
                        supply: Vec3::new(0.1, -0.2, 0.05),
                        condensation: Vec3::new(-0.3, 0.2, 0.1),
                    });
            }
            if scheduled {
                let domain = q.bindings.get_mut("band").unwrap();
                domain.incoming_velocity_world_m_s = None;
                if phase_specific {
                    q.schema = "ftx-coupled-liquid-mechanics/5".into();
                    q.model =
                        "scheduled_phase_specific_incoming_velocity_mass_exchange_frozen_mechanics/1".into();
                    domain.incoming_stream_velocities_world_m_s = None;
                    domain.incoming_stream_velocities_world_m_s_by_interval = Some(vec![
                        crate::liquid_mass::IncomingVelocities {
                            supply: Vec3::new(0.1, -0.2, 0.05),
                            condensation: Vec3::new(-0.3, 0.2, 0.1),
                        },
                        crate::liquid_mass::IncomingVelocities {
                            supply: Vec3::new(-0.3, 0.4, -0.1),
                            condensation: Vec3::new(0.2, -0.1, 0.3),
                        },
                    ]);
                    if recoil {
                        q.schema = "ftx-coupled-liquid-mechanics/6".into();
                        q.model =
                            "scheduled_phase_specific_incoming_and_outgoing_velocity_mass_exchange_frozen_mechanics/1".into();
                        domain.outgoing_stream_velocities_relative_to_cloth_m_s_by_interval =
                            Some(vec![
                                crate::liquid_mass::OutgoingVelocities {
                                    evaporation: Vec3::new(0.02, -0.01, 0.005),
                                    drainage: Vec3::new(-0.01, 0.02, -0.005),
                                },
                                crate::liquid_mass::OutgoingVelocities {
                                    evaporation: Vec3::new(-0.01, 0.02, -0.005),
                                    drainage: Vec3::new(0.02, -0.01, 0.005),
                                },
                            ]);
                    }
                } else {
                    q.schema = "ftx-coupled-liquid-mechanics/3".into();
                    q.model = "scheduled_incoming_velocity_mass_exchange_frozen_mechanics/1".into();
                    domain.incoming_velocity_world_m_s_by_interval =
                        Some(vec![Vec3::new(0.1, -0.2, 0.05), Vec3::new(-0.3, 0.4, -0.1)]);
                }
            }
        }
        let mut water = serde_json::to_value(heat.surface_water.as_ref().unwrap()).unwrap();
        water["schema"] = json!("ftx-textile-surface-water/2");
        water
            .as_object_mut()
            .unwrap()
            .remove("replenishment_kg_m2_s");
        water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(0.);
        water["latent_heat_j_kg"] = json!(2500000.);
        water["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
            "bindings":{"band":{"rate_kg_m2_s":[10.,10.],"temperature_c":[15.,15.]}},
            "source":{"reference":"test:rollback-liquid-feed","accessed_on":"2026-10-28",
                "note":"Synthetic numerical delivery, not a measured wearer flux"},
            "evidence":"estimate","allow_estimates":true,"method":"Prescribed co-moving feed"});
        if drainage {
            water["schema"] = json!("ftx-textile-surface-water/4");
            water["drainage"] = json!({"schema":"ftx-textile-liquid-drainage/1",
                "bindings":{"band":{"rate_kg_m2_s":[5.,3.]}},
                "source":{"reference":"test:rollback-drainage","accessed_on":"2026-10-30",
                    "note":"Synthetic prescribed outflow, not measured textile runoff"},
                "evidence":"estimate","allow_estimates":true,"method":"Independent reference-area removal",
                "qualification":"Co-moving stock-limited drainage at old inner-cell temperature; proportional evaporation and drainage at empty stock"});
        }
        heat.surface_water = Some(serde_json::from_value(water).unwrap());
    }
    let endpoints: Vec<Vec<_>> = [0.0001, 0.0002]
        .into_iter()
        .map(|z| vertices.iter().map(|p| *p + Vec3::new(0., 0., z)).collect())
        .collect();
    let path = Path::new(&request, body.collision, endpoints.clone()).unwrap();
    let complete = evaluate(
        &request,
        &path,
        &scenario,
        &body,
        &mut run,
        &mut heat::Budget::default(),
        None,
        None,
        |_| {},
    )
    .unwrap();
    let complete_debug = format!("{:?}", run.scene.solver);
    assert_ne!(run.scene.solver.x, original.x);
    if exchange {
        let old = original.model.attached_mass_report().unwrap().unwrap();
        let current = run
            .scene
            .solver
            .model
            .attached_mass_report()
            .unwrap()
            .unwrap();
        assert_eq!(old.input_sha256, current.input_sha256);
        assert_eq!(old.cloth_mass_kg, current.cloth_mass_kg);
        assert_eq!(old.attached_mass_kg, current.attached_mass_kg);
        assert_eq!(current.evaporating_liquid_mass_kg, None);
        let remaining = complete["liquid_mechanics"]["remaining_liquid_mass_kg"]
            .as_f64()
            .unwrap();
        assert_eq!(current.exchanging_liquid_mass_kg, Some(remaining));
        assert!(remaining > 0.);
        assert!(
            (current.total_inertial_mass_kg - old.total_inertial_mass_kg - remaining).abs() < 1e-15
        );
        if prescribed {
            let report = &complete["liquid_mechanics"];
            assert_eq!(
                report["schema"],
                if recoil {
                    "ftx-coupled-liquid-mechanics-result/6"
                } else if phase_specific && scheduled {
                    "ftx-coupled-liquid-mechanics-result/5"
                } else if phase_specific {
                    "ftx-coupled-liquid-mechanics-result/4"
                } else if scheduled {
                    "ftx-coupled-liquid-mechanics-result/3"
                } else {
                    "ftx-coupled-liquid-mechanics-result/2"
                }
            );
            let flow = &report["exchange"];
            assert!(flow["mixing_dissipation_j"].as_f64().unwrap() > 0.);
            let incoming = flow["incoming_mass_kg"].as_f64().unwrap();
            let momentum: Vec3 =
                serde_json::from_value(flow["incoming_momentum_kg_m_s"].clone()).unwrap();
            if !scheduled && !phase_specific {
                assert!((momentum - Vec3::new(0.1, -0.2, 0.05) * incoming).norm() < 1e-15);
            }
        }
    }
    let liquid = &complete["liquid_mechanics"];
    for field in [
        "mass_balance_residual_kg",
        "transfer_kinetic_residual_j",
        "transfer_gravitational_residual_j",
    ] {
        assert!(
            liquid[field].as_f64().unwrap().abs() < 1e-12,
            "{field}: {liquid}"
        );
    }
    let momentum_residual: Vec3 =
        serde_json::from_value(liquid["transfer_momentum_residual_kg_m_s"].clone()).unwrap();
    assert!(momentum_residual.norm() < 1e-12);
    if recoil {
        assert!(
            liquid["exchange"]["outgoing_separation_work_j"]
                .as_f64()
                .unwrap()
                > 0.
        );
    }
    let thermal = &complete["transient_surface"]["result"];
    if drainage {
        let water = &thermal["surface_water"];
        let mechanics = &complete["liquid_mechanics"];
        assert_eq!(water["schema"], "ftx-textile-surface-water-result/4");
        let drained = water["drained_liquid_kg"].as_f64().unwrap();
        let evaporated = water["evaporated_liquid_kg"].as_f64().unwrap();
        assert!(drained > 0. && evaporated > 0.);
        assert!((mechanics["drained_liquid_mass_kg"].as_f64().unwrap() - drained).abs() < 1e-15);
        assert!(
            (mechanics["evaporated_liquid_mass_kg"].as_f64().unwrap() - evaporated).abs() < 1e-15
        );
        assert!(
            (mechanics["exchange"]["outgoing_mass_kg"].as_f64().unwrap() - drained - evaporated)
                .abs()
                < 1e-15
        );
        assert!(thermal["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
    }
    let trace = thermal["trace"].as_array().unwrap();
    let first_interval: Vec<_> = trace.iter().filter(|r| r["interval_index"] == 0).collect();
    assert_eq!(first_interval.len(), 2);
    if scheduled {
        let first_incoming =
            first_interval.last().unwrap()["mechanical_coupling"]["exchange"]["incoming_mass_kg"]
                .as_f64()
                .unwrap();
        let flow = &complete["liquid_mechanics"]["exchange"];
        let second_incoming = flow["incoming_mass_kg"].as_f64().unwrap() - first_incoming;
        assert!(first_incoming > 0. && second_incoming > 0.);
        let momentum: Vec3 =
            serde_json::from_value(flow["incoming_momentum_kg_m_s"].clone()).unwrap();
        let expected = Vec3::new(0.1, -0.2, 0.05) * first_incoming
            + Vec3::new(-0.3, 0.4, -0.1) * second_incoming;
        assert!((momentum - expected).norm() < 1e-15);
        let mut short = request.clone();
        let domain = short
            .transient_surface
            .as_mut()
            .unwrap()
            .mechanical_coupling
            .as_mut()
            .unwrap()
            .bindings
            .get_mut("band")
            .unwrap();
        if phase_specific {
            domain
                .incoming_stream_velocities_world_m_s_by_interval
                .as_mut()
                .unwrap()
                .pop();
        } else {
            domain
                .incoming_velocity_world_m_s_by_interval
                .as_mut()
                .unwrap()
                .pop();
        }
        assert!(
            short
                .transient_surface
                .as_ref()
                .unwrap()
                .start(&run, &short.keyframes, &mut heat::Budget::default())
                .is_err()
        );
    }
    assert!(
        first_interval.last().unwrap()["surface_water"]["evaporated_liquid_kg"]
            .as_f64()
            .unwrap()
            > 0.
    );
    let first_substeps: u64 = first_interval
        .iter()
        .map(|r| r["thermal_substeps"].as_u64().unwrap())
        .sum();
    let all_substeps = thermal["thermal_steps"].as_u64().unwrap();
    let all_cost = thermal["cell_steps"].as_u64().unwrap();
    assert!(all_substeps > first_substeps && first_substeps > 0);
    assert_eq!(all_cost % all_substeps, 0);
    // Derive the exact first endpoint's cost from the successful physical history. The next
    // endpoint cannot begin thermal work, but all first-endpoint mass transfers have committed.
    let first_cost = (first_substeps * (all_cost / all_substeps)) as usize;
    let mut limited = request.clone();
    limited.transient_surface.as_mut().unwrap().max_cell_steps = first_cost;
    let limited_path = Path::new(&limited, body.collision, endpoints.clone()).unwrap();
    run.scene.solver = original.clone();
    let mut reached = Vec::new();
    let refusal = evaluate(
        &limited,
        &limited_path,
        &scenario,
        &body,
        &mut run,
        &mut heat::Budget::default(),
        None,
        None,
        |message| reached.push(message.to_owned()),
    )
    .unwrap_err();
    assert!(
        reached.iter().any(|s| s == "motion endpoint 2/2"),
        "{reached:?}"
    );
    assert!(
        refusal
            .message
            .contains("motion heat cell-step budget exhausted"),
        "{refusal:?}"
    );
    assert_eq!(format!("{:?}", run.scene.solver), original_debug);
    assert!(run.scene.solver.liquid_mass_report().unwrap().is_none());
    let replay = evaluate(
        &request,
        &path,
        &scenario,
        &body,
        &mut run,
        &mut heat::Budget::default(),
        None,
        None,
        |_| {},
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&replay).unwrap(),
        serde_json::to_vec(&complete).unwrap()
    );
    assert_eq!(format!("{:?}", run.scene.solver), complete_debug);

    // Reuse the exact independently qualified laws through a material-source collection.
    // The decoy is complete and valid, but its hashes belong to a different material/report.
    let mut paired_request = request.clone();
    let heat = paired_request.transient_surface.as_mut().unwrap();
    let pair = heat::CoupledWaterAlternative {
        surface_water: heat.surface_water.take().unwrap(),
        mechanical_coupling: heat.mechanical_coupling.take().unwrap(),
    };
    let mut decoy = pair.clone();
    decoy
        .surface_water
        .bindings
        .get_mut("band")
        .unwrap()
        .material_sha256 = "a".repeat(64);
    decoy.surface_water.source.reference = "test:other-thermal-material-source".into();
    decoy
        .mechanical_coupling
        .bindings
        .get_mut("band")
        .unwrap()
        .material_sha256 = "a".repeat(64);
    decoy.mechanical_coupling.material_report_sha256 = "b".repeat(64);
    decoy.mechanical_coupling.source.reference = "test:other-mechanical-material-source".into();
    heat.coupled_water_alternatives = Some(vec![decoy, pair.clone()]);
    let authored = serde_json::to_value(&paired_request).unwrap();
    let paired_path = Path::new(&paired_request, body.collision, endpoints.clone()).unwrap();
    run.scene.solver = original.clone();
    let paired = evaluate(
        &paired_request,
        &paired_path,
        &scenario,
        &body,
        &mut run,
        &mut heat::Budget::default(),
        None,
        None,
        |_| {},
    )
    .unwrap();
    assert_eq!(format!("{:?}", run.scene.solver), complete_debug);
    assert_eq!(paired["liquid_mechanics"], complete["liquid_mechanics"]);
    assert_eq!(paired["samples"], complete["samples"]);
    let paired_heat = &paired["transient_surface"]["result"];
    assert_eq!(paired_heat["request"], authored["transient_surface"]);
    assert_eq!(paired_heat["surface_water"], thermal["surface_water"]);
    assert_eq!(paired_heat["trace"], thermal["trace"]);
    assert_eq!(paired_heat["final"], thermal["final"]);
    assert_eq!(
        paired_heat["coupled_water_selection"],
        json!({"index":1,"pair_sha256":hash(&serde_json::to_value(&pair).unwrap()).unwrap()})
    );
    assert!(paired_heat.get("surface_water_selection").is_none());
    assert_eq!(serde_json::to_value(&paired_request).unwrap(), authored);

    // A source collection must stay coupled even when its selection or independent mechanical
    // qualification fails. These failures occur before any motion endpoint or mass mutation.
    for failure in [
        "no_source_match",
        "stale_full_material_report",
        "unpaired_instance",
    ] {
        let mut bad = paired_request.clone();
        let choices = bad
            .transient_surface
            .as_mut()
            .unwrap()
            .coupled_water_alternatives
            .as_mut()
            .unwrap();
        match failure {
            "no_source_match" => {
                choices.pop();
            }
            "stale_full_material_report" => {
                choices[1].mechanical_coupling.material_report_sha256 = "c".repeat(64);
            }
            "unpaired_instance" => {
                choices[0].mechanical_coupling.bindings.remove("band");
            }
            _ => unreachable!(),
        }
        let bad_path = Path::new(&bad, body.collision, endpoints.clone()).unwrap();
        run.scene.solver = original.clone();
        let mut attempted = Vec::new();
        let refusal = evaluate(
            &bad,
            &bad_path,
            &scenario,
            &body,
            &mut run,
            &mut heat::Budget::default(),
            None,
            None,
            |message| attempted.push(message.to_owned()),
        )
        .unwrap_err();
        if failure == "no_source_match" {
            assert!(
                refusal.message.contains("no water qualification"),
                "{refusal:?}"
            );
        } else if failure == "stale_full_material_report" {
            assert!(
                refusal
                    .message
                    .contains("complete material/interface/instance"),
                "{refusal:?}"
            );
        }
        assert!(
            attempted
                .iter()
                .all(|message| !message.starts_with("motion endpoint"))
        );
        assert_eq!(
            format!("{:?}", run.scene.solver),
            original_debug,
            "{failure}"
        );
        assert!(run.scene.solver.liquid_mass_report().unwrap().is_none());
    }

    // After the first accepted paired transfer, exhausting the later thermal budget must
    // restore all liquid, momentum, work and attached-mass state before an identical replay.
    let mut paired_limited = paired_request.clone();
    paired_limited
        .transient_surface
        .as_mut()
        .unwrap()
        .max_cell_steps = first_cost;
    let limited_path = Path::new(&paired_limited, body.collision, endpoints).unwrap();
    run.scene.solver = original;
    let mut reached = Vec::new();
    let refusal = evaluate(
        &paired_limited,
        &limited_path,
        &scenario,
        &body,
        &mut run,
        &mut heat::Budget::default(),
        None,
        None,
        |message| reached.push(message.to_owned()),
    )
    .unwrap_err();
    assert!(
        reached
            .iter()
            .any(|message| message == "motion endpoint 2/2")
    );
    assert!(
        refusal
            .message
            .contains("motion heat cell-step budget exhausted"),
        "{refusal:?}"
    );
    assert_eq!(format!("{:?}", run.scene.solver), original_debug);
    assert!(run.scene.solver.liquid_mass_report().unwrap().is_none());
    let replay = evaluate(
        &paired_request,
        &paired_path,
        &scenario,
        &body,
        &mut run,
        &mut heat::Budget::default(),
        None,
        None,
        |_| {},
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&replay).unwrap(),
        serde_json::to_vec(&paired).unwrap()
    );
    assert_eq!(format!("{:?}", run.scene.solver), complete_debug);
}
