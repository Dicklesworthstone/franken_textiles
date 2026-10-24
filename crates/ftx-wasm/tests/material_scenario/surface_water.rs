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

fn inputs() -> (Pattern, Request, Value) {
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
            "objectives":[{"metric":"transient_surface_final_min_temperature_c","direction":"minimize"}],
            "constraints":[{"metric":"textile_water_remaining_mass_kg","maximum":0.},
                {"metric":"textile_water_evaporated_mass_kg","minimum":0.},
                {"metric":"textile_water_latent_energy_j","minimum":0.}]}})).unwrap();
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
    (pattern, input, raw)
}

fn study(raw: &Value, request: &Request, pattern: &Pattern) -> Value {
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    };
    let scenario: Scenario = serde_json::from_value(raw.clone()).unwrap();
    let mut session = Session::new(&scenario, &body).unwrap();
    let motion = scenario.motion.as_ref().unwrap();
    let endpoints = motion
        .keyframes
        .iter()
        .enumerate()
        .map(|(i, _)| {
            form.vertices
                .iter()
                .map(|p| *p + Vec3::new(0., 0., 0.00005 * (i + 1) as f64))
                .collect()
        })
        .collect();
    session
        .set_motion_path(ftx_drape::study::motion::Path::new(
            motion,
            body.collision,
            endpoints,
        ))
        .unwrap();
    let descriptor = session.descriptor();
    serde_json::to_value(
        request
            .evaluate_with(pattern, &descriptor, |_, p| session.evaluate(p, |_| {}))
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn portable_surface_water_scenario_preserves_actual_cooling_replay_and_independent_readiness() {
    let (pattern, input, raw) = inputs();
    let evaluate = |scenario: &Value, request: &Request| study(scenario, request, &pattern);
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
    // The full conservative water/enthalpy model, not merely a small residual, qualifies the
    // actual production result for temperature and textile-water decisions.
    assert_eq!(result["comparison"]["frontier"], json!(["qualified"]));
    assert_eq!(
        keyed(&result["comparison"]["alternatives"], "qualified")["eligible"],
        true
    );
    let readings = &keyed(&result["comparison"]["alternatives"], "qualified")["readings"];
    for (metric, field) in [
        ("textile_water_remaining_mass_kg", "remaining_liquid_kg"),
        ("textile_water_evaporated_mass_kg", "evaporated_liquid_kg"),
        ("textile_water_latent_energy_j", "latent_energy_j"),
    ] {
        assert_eq!(readings[metric]["value"], water[field]);
    }
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

#[test]
fn portable_surface_water_supply_rewets_real_cloth_and_qualifies_complete_decisions() {
    let (pattern, base, mut raw) = inputs();
    raw["motion"]["keyframes"] = json!([
        {"time_s":0.001,"arm_abduction_deg":35.},
        {"time_s":0.002,"arm_abduction_deg":35.}]);
    let heat_policy = &mut raw["motion"]["transient_surface"];
    heat_policy["boundaries"] = json!([heat_policy["boundaries"][0], heat_policy["boundaries"][0]]);
    let water = &mut heat_policy["surface_water"];
    water["schema"] = json!("ftx-textile-surface-water/2");
    water
        .as_object_mut()
        .unwrap()
        .remove("replenishment_kg_m2_s");
    water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(0.);
    water["outside_vapor_pressure_pa"] = json!([1000., 1000.]);
    water["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
        "bindings":{"band":{"rate_kg_m2_s":[1e-5,0.001],"temperature_c":[40.,30.]}},
        "source":{"reference":"test:portable-textile-feed","accessed_on":"2026-10-23",
            "note":"Synthetic delivery and temperatures only; no physiological or empirical claim"},
        "evidence":"estimate","allow_estimates":true,"method":"Prescribed piecewise reference-area feed"});
    let mut request = serde_json::to_value(base).unwrap();
    request["comparison"]["objectives"] = json!([
        {"metric":"textile_water_supplied_mass_kg","direction":"minimize"},
        {"metric":"transient_surface_final_min_temperature_c","direction":"minimize"}]);
    request["comparison"]["constraints"][0] =
        json!({"metric":"textile_water_remaining_mass_kg","minimum":0.});
    let input: Request = serde_json::from_value(request).unwrap();
    let result = study(&raw, &input, &pattern);
    let simulated = &keyed(&result["simulations"], "qualified")["evaluation"]["result"];
    let outcome = &simulated["motion"]["result"]["transient_surface"];
    assert_eq!(outcome["status"], "available", "{outcome}");
    let heat = &outcome["result"];
    let water = &heat["surface_water"];
    assert_eq!(water["schema"], "ftx-textile-surface-water-result/2");
    assert_eq!(water["initial_liquid_kg"], 0.);
    assert!(water["remaining_liquid_kg"].as_f64().unwrap() > 0.);
    assert!(water["evaporated_liquid_kg"].as_f64().unwrap() > 0.);
    let near = |actual: f64, expected: f64| {
        assert!(
            (actual - expected).abs() <= 1e-11 * actual.abs().max(expected.abs()),
            "{actual} != {expected}"
        );
    };
    let mut reference_area = 0.;
    for (i, patch) in water["patches"].as_array().unwrap().iter().enumerate() {
        let area = heat["model"]["patches"][i]["reference_area_m2"]
            .as_f64()
            .unwrap();
        reference_area += area;
        near(
            patch["supplied_liquid_kg"].as_f64().unwrap(),
            area * (1e-5 * 0.001 + 0.001 * 0.001),
        );
        near(
            patch["supplied_liquid_sensible_energy_j"].as_f64().unwrap(),
            area * 4200. * (1e-5 * 0.001 * 40. + 0.001 * 0.001 * 30.),
        );
    }
    let mut dry_after_first_interval = false;
    for sample in heat["trace"].as_array().unwrap() {
        let t = sample["time_s"].as_f64().unwrap();
        let first = t.min(0.001);
        let second = (t - 0.001).max(0.);
        let w = &sample["surface_water"];
        near(
            w["supplied_liquid_kg"].as_f64().unwrap(),
            reference_area * (1e-5 * first + 0.001 * second),
        );
        near(
            w["supplied_liquid_sensible_energy_j"].as_f64().unwrap(),
            reference_area * 4200. * (1e-5 * first * 40. + 0.001 * second * 30.),
        );
        if t == 0.001 {
            assert_eq!(w["remaining_liquid_kg"], 0.);
            dry_after_first_interval = true;
        }
        let mass = w["remaining_liquid_kg"].as_f64().unwrap()
            + w["evaporated_liquid_kg"].as_f64().unwrap()
            - w["supplied_liquid_kg"].as_f64().unwrap();
        assert!(mass.abs() < 1e-11 * w["supplied_liquid_kg"].as_f64().unwrap());
        let energy = sample["stored_change_j"].as_f64().unwrap()
            + w["liquid_stored_change_j"].as_f64().unwrap()
            + w["exported_vapor_sensible_energy_j"].as_f64().unwrap()
            + w["latent_energy_j"].as_f64().unwrap()
            - w["supplied_liquid_sensible_energy_j"].as_f64().unwrap()
            - sample["net_boundary_energy_j"].as_f64().unwrap();
        assert!(
            energy.abs() < 1e-10,
            "full supplied-film energy residual {energy}"
        );
    }
    assert!(dry_after_first_interval);
    assert_eq!(
        result["comparison"]["frontier"],
        json!(["qualified"]),
        "{}",
        result["comparison"]
    );
    let row = keyed(&result["comparison"]["alternatives"], "qualified");
    assert_eq!(
        row["readings"]["textile_water_supplied_mass_kg"]["value"],
        water["supplied_liquid_kg"]
    );
    let mut replay = input.clone();
    for variant in &mut replay.variants {
        variant.changes = serde_json::from_value(
            keyed(&result["manufacturing"]["alternatives"], &variant.key)["frozen_changes"].clone(),
        )
        .unwrap();
    }
    let replayed = study(&raw, &replay, &pattern);
    assert_eq!(replayed["simulations"], result["simulations"]);
    // Freezing the same resolved selections changes the authored request identity, while every
    // source-qualified decision and all retained physical results must remain identical.
    assert_ne!(
        replayed["comparison"]["context_sha256"],
        result["comparison"]["context_sha256"]
    );
    for field in [
        "schema",
        "source_pattern_sha256",
        "policy",
        "alternatives",
        "frontier",
        "incomparable",
        "uncertainty_propagated",
        "physical_validation_claim",
        "motion_required",
        "notes",
    ] {
        assert_eq!(
            replayed["comparison"][field], result["comparison"][field],
            "replayed comparison {field}"
        );
    }
    for fault in 0..3 {
        let mut bad = raw.clone();
        let w = &mut bad["motion"]["transient_surface"]["surface_water"];
        match fault {
            0 => w["supply"]["allow_estimates"] = json!(false),
            1 => w["supply"]["bindings"]["band"]["rate_kg_m2_s"] = json!([1e-5]),
            _ => w["maximum_liquid_loading_kg_m2"] = json!(1e-8),
        }
        let refused = study(&bad, &input, &pattern);
        let r = &keyed(&refused["simulations"], "qualified")["evaluation"]["result"];
        assert_eq!(r["motion"]["status"], "available");
        assert_eq!(
            r["motion"]["result"]["transient_surface"]["status"], "refused",
            "fault {fault}"
        );
        assert_eq!(r["drape"], simulated["drape"]);
        assert_eq!(
            r["motion"]["result"]["final_positions_sha256"],
            simulated["motion"]["result"]["final_positions_sha256"]
        );
        assert_eq!(refused["manufacturing"], result["manufacturing"]);
        assert_eq!(refused["comparison"]["frontier"], json!([]));
    }
}
