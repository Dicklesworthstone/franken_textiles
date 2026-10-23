//! Real shared runtime path used by native and portable material scenarios, with no synthetic
//! callback receipt. The caller supplies the same explicit motion meshes as existing water tests.
use super::*;
use ftx_pattern::{material_catalog::Selection, materials::Catalog};

fn keyed<'a>(rows: &'a Value, key: &str) -> &'a Value {
    rows.as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == key)
        .unwrap_or_else(|| panic!("missing study row {key}"))
}

#[test]
fn portable_surface_water_scenario_preserves_actual_cooling_replay_and_independent_readiness() {
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let mut material = pattern.raw()["fabric"]["material"].clone();
    for (key, value, unit) in [
        ("specific_heat", 1000., "J/(kg*K)"),
        ("vapor_resistance", 8., "m2*Pa/W"),
    ] {
        material["record"]["properties"][key] = json!({"value":value,"unit":unit,
            "source":"test","evidence":"estimate","method":"Synthetic portable liquid cooling case"});
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
    let input: Request = serde_json::from_value(json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"qualified","changes":{"schema":"ftx-material-bindings/1","default":material}},
            {"key":"missing-ret","changes":{"schema":"ftx-material-bindings/1","default":missing}}],
        "cutting":{"copies":1,"gap_mm":1.,"edge_margin_mm":0.,"end_margin_mm":0.,"seam_allowance_mm":0.},
        "comparison":{"basis":"nominal","required_consumers":["transient_surface"],
            "objectives":[{"metric":"transient_surface_final_min_temperature_c","direction":"minimize"}]}})).unwrap();
    let mut raw: Value = serde_json::from_str(SCENARIO).unwrap();
    raw["options"]["sew_steps"] = json!(120);
    raw["options"]["drape_steps"] = json!(0);
    raw["motion"] = json!({"schema":"ftx-drape-motion/1",
        "keyframes":[{"time_s":0.001,"arm_abduction_deg":35.}],
        "max_step_s":0.0005,"max_trials":8,"max_pairs":100000,"max_sample_vertices":100000,
        "transient_surface":{"schema":"ftx-motion-surface-heat/1","initial_temperature_c":20.,
            "cells_per_layer":1,"max_step_s":0.00025,"max_cell_steps":1000000,
            "boundaries":[{"inside_c":20.,"outside_c":20.,
                "inside_resistance_m2_k_w":1.,"outside_resistance_m2_k_w":1.}],
            "surface_water":{"schema":"ftx-textile-surface-water/1",
                "model":"inner_cell_equilibrium_free_film_frozen_properties/1",
                "bindings":{"band":{"material_sha256":lock,"initial_liquid_kg_m2":5e-8}},
                "maximum_liquid_loading_kg_m2":1e-5,"liquid_specific_heat_j_kg_k":4200.,
                "latent_heat_j_kg":2500000.,"enthalpy_reference_c":0.,
                "wet_vapor_pressure_pa":[[-20.,0.],[80.,10000.]],"outside_vapor_pressure_pa":[1000.],
                "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":1.,
                "replenishment_kg_m2_s":0.,"source":{"reference":"test:portable-free-liquid",
                    "accessed_on":"2026-10-23","note":"Synthetic prescribed inputs; no measured or garment claim"},
                "evidence":"estimate","allow_estimates":true,"method":"Synthetic conservative cooling",
                "qualification":"Artificial fixed dry thermal and Ret observations qualified across [-20,80] C and [0,1e-5] kg per reference square metre"}}});
    let evaluate = |scenario: &Value, request: &Request| {
        let scenario: Scenario = serde_json::from_str(&scenario.to_string()).unwrap();
        let mut session = Session::new(&scenario, &body).unwrap();
        let motion = scenario.motion.as_ref().unwrap();
        let endpoint = form
            .vertices
            .iter()
            .map(|p| *p + Vec3::new(0., 0., 0.00005))
            .collect();
        session
            .set_motion_path(ftx_drape::study::motion::Path::new(
                motion,
                body.collision,
                vec![endpoint],
            ))
            .unwrap();
        let descriptor = session.descriptor();
        serde_json::to_value(
            request
                .evaluate_with(&pattern, &descriptor, |_, p| session.evaluate(p, |_| {}))
                .unwrap(),
        )
        .unwrap()
    };
    let result = evaluate(&raw, &input);
    let simulated = &keyed(&result["simulations"], "qualified")["evaluation"]["result"];
    assert_eq!(
        simulated["motion"]["status"], "available",
        "{}",
        simulated["motion"]
    );
    let outcome = &simulated["motion"]["result"]["transient_surface"];
    assert_eq!(outcome["status"], "available", "{outcome}");
    let heat = &outcome["result"];
    assert_eq!(
        heat["request"]["surface_water"],
        raw["motion"]["transient_surface"]["surface_water"]
    );
    let water = &heat["surface_water"];
    assert!(water["evaporated_liquid_kg"].as_f64().unwrap() > 0.);
    assert_eq!(water["remaining_liquid_kg"], 0.);
    assert!(
        heat["final"]["temperature_c"]
            .as_array()
            .unwrap()
            .iter()
            .all(|cells| cells[0].as_f64().unwrap() < 20.)
    );
    let balance = heat["stored_change_j"].as_f64().unwrap()
        + water["liquid_stored_change_j"].as_f64().unwrap()
        + water["exported_vapor_sensible_energy_j"].as_f64().unwrap()
        + water["latent_energy_j"].as_f64().unwrap()
        - heat["net_boundary_energy_j"].as_f64().unwrap();
    assert!(balance.abs() < 1e-10);
    assert_eq!(
        keyed(&result["simulations"], "missing-ret")["evaluation"]["result"]["motion"]["result"]["transient_surface"]
            ["status"],
        "refused"
    );
    for alternative in result["manufacturing"]["alternatives"].as_array().unwrap() {
        assert_eq!(alternative["cutting"]["status"], "available");
    }
    // Source-qualified simulation is available now; a different enthalpy ownership model cannot
    // silently pass the existing dry-heat decision gate merely because its residual is small.
    assert_eq!(result["comparison"]["frontier"], json!([]));
    assert_eq!(
        keyed(&result["comparison"]["alternatives"], "qualified")["eligible"],
        false
    );
    let mut replay = input.clone();
    for variant in &mut replay.variants {
        let row = keyed(&result["manufacturing"]["alternatives"], &variant.key);
        variant.changes = serde_json::from_value(row["frozen_changes"].clone()).unwrap();
    }
    let replayed = evaluate(&raw, &replay);
    assert_eq!(replayed["simulations"], result["simulations"]);
    let mut dry = raw.clone();
    dry["motion"]["transient_surface"]["surface_water"] = Value::Null;
    let dry_result = evaluate(&dry, &input);
    let dry_simulated = &keyed(&dry_result["simulations"], "qualified")["evaluation"]["result"];
    assert_eq!(simulated["drape"], dry_simulated["drape"]);
    assert_eq!(
        simulated["motion"]["result"]["final_positions_sha256"],
        dry_simulated["motion"]["result"]["final_positions_sha256"]
    );
}
