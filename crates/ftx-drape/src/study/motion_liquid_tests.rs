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
    rollback_after_liquid_transfer(false);
}

#[test]
fn late_supply_budget_failure_rolls_back_liquid_and_attached_mass_together() {
    rollback_after_liquid_transfer(true);
}

fn rollback_after_liquid_transfer(exchange: bool) {
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
    }
    let thermal = &complete["transient_surface"]["result"];
    let trace = thermal["trace"].as_array().unwrap();
    let first_interval: Vec<_> = trace.iter().filter(|r| r["interval_index"] == 0).collect();
    assert_eq!(first_interval.len(), 2);
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
    let limited_path = Path::new(&limited, body.collision, endpoints).unwrap();
    run.scene.solver = original;
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
}
