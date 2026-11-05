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
fn distinct_initial_temperatures_reach_heated_sheet_and_coverage_decisions() {
    let (pattern, mut input, mut raw) = inputs();
    let mut pattern = pattern.raw().clone();
    let mut upper = pattern["pieces"][0].clone();
    upper["id"] = json!("upper");
    upper["placement"]["y0"] = json!(-50.);
    pattern["pieces"].as_array_mut().unwrap().push(upper);
    pattern["seams"].as_array_mut().unwrap().push(json!({
        "id":"join-upper","a":"upper/right","b":"upper/left"}));
    let pattern = Pattern::new(pattern);
    input.variants.truncate(1);
    input.variants[0].changes.default = Some(
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-relaxation-heat.json"
        ))
        .unwrap(),
    );
    input.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal",
        "required_consumers":["transient_surface","transient_coverage"],
        "objectives":[
            {"metric":"transient_surface_min_sample_temperature_c","direction":"maximize"},
            {"metric":"transient_coverage_max_sample_temperature_c","direction":"minimize"}]}))
        .unwrap(),
    );
    let coverage = raw.as_object_mut().unwrap().remove("coverage").unwrap();
    let sheet = &mut raw["motion"]["transient_surface"];
    sheet.as_object_mut().unwrap().remove("surface_water");
    sheet["initial_temperature_c"] = json!(999.);
    sheet["initial_instance_temperatures_c"] = json!({"band":5.,"upper":35.});
    sheet["relaxation_dissipation_heat"] = json!("uniform_reference_thickness");
    sheet["max_cell_steps"] = json!(5000000);
    let boundary = sheet["boundaries"][0].clone();
    raw["motion"]["transient_coverage"] = json!({
        "schema":"ftx-motion-coverage-heat/1",
        "body_triangle_ids":(0..form().triangles.len()).collect::<Vec<_>>(),
        "instance_layers":{"band":"shell","upper":"shell"},"settings":coverage["settings"],
        "initial_temperature_c":999.,"initial_instance_temperatures_c":{"band":5.,"upper":35.},
        "relaxation_dissipation_heat":"uniform_reference_thickness",
        "boundaries":[boundary],"exposed_boundaries":[boundary],
        "max_step_s":0.00025,"max_cell_steps":5000000,"max_geometry_operations":1000000});
    let result = study(&raw, &input, &pattern);
    let simulation = &keyed(&result["simulations"], "qualified")["evaluation"]["result"];
    let motion = &simulation["motion"]["result"];
    assert_eq!(simulation["motion"]["status"], "available", "{simulation}");
    for name in ["transient_surface", "transient_coverage"] {
        let outcome = &motion[name];
        assert_eq!(outcome["status"], "available", "{outcome}");
        let heat = &outcome["result"];
        assert_eq!(
            heat["initial_condition"]["temperature_range_c"],
            json!([5., 35.])
        );
        assert_eq!(heat["request"], raw["motion"][name]);
        assert!(heat["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
        let work = &heat["relaxation_dissipation_heat"];
        let dissipated = work["viscous_dissipation_j"].as_f64().unwrap();
        assert!(dissipated > 0., "{work}");
        assert!((work["supplied_heat_j"].as_f64().unwrap() - 0.75 * dissipated).abs() < 1e-10);
    }
    assert_eq!(
        result["comparison"]["frontier"],
        json!(["qualified"]),
        "{}",
        result["comparison"]
    );
    let readings = &keyed(&result["comparison"]["alternatives"], "qualified")["readings"];
    assert_eq!(
        readings["transient_surface_min_sample_temperature_c"]["value"],
        5.
    );
    assert_eq!(
        readings["transient_coverage_max_sample_temperature_c"]["value"],
        motion["transient_coverage"]["result"]["trace"]
            .as_array()
            .unwrap()
            .iter()
            .fold(35_f64, |high, row| high
                .max(row["temperature_range_c"][1].as_f64().unwrap()))
    );
    let replay: Value = serde_json::to_value(
        input
            .evaluate_with(&pattern, &result["scenario"], |_, _| Ok(simulation.clone()))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(replay["comparison"], result["comparison"]);

    // Each independently requested consumer qualifies its own actual initial temperatures.
    // A refused conversion domain must leave the other thermal path and mechanics available.
    raw["motion"]["transient_surface"]["initial_instance_temperatures_c"]["upper"] = json!(90.);
    let refused = study(&raw, &input, &pattern);
    let independent = &keyed(&refused["simulations"], "qualified")["evaluation"]["result"];
    assert_eq!(independent["motion"]["status"], "available");
    assert_eq!(
        independent["motion"]["result"]["transient_surface"]["status"],
        "refused"
    );
    assert_eq!(
        independent["motion"]["result"]["transient_coverage"]["status"],
        "available"
    );
    assert_eq!(
        independent["motion"]["result"]["samples"],
        motion["samples"]
    );
    assert_eq!(
        keyed(&refused["comparison"]["alternatives"], "qualified")["eligible"],
        false
    );
}

#[test]
fn portable_dry_instance_temperatures_preserve_owners_fallback_replay_and_independent_motion() {
    let (pattern, mut input, mut raw) = inputs();
    let mut pattern = pattern.raw().clone();
    let mut upper = pattern["pieces"][0].clone();
    upper["id"] = json!("upper");
    upper["placement"]["y0"] = json!(-50.);
    pattern["pieces"].as_array_mut().unwrap().push(upper);
    pattern["seams"].as_array_mut().unwrap().push(json!({
        "id":"join-upper","a":"upper/right","b":"upper/left"}));
    let pattern = Pattern::new(pattern);
    input.variants.truncate(1);
    input.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal",
        "required_consumers":["transient_surface"],"objectives":[
            {"metric":"transient_surface_min_sample_temperature_c","direction":"maximize"},
            {"metric":"transient_surface_max_sample_temperature_c","direction":"minimize"}]}))
        .unwrap(),
    );
    raw.as_object_mut().unwrap().remove("coverage");
    let policy = &mut raw["motion"]["transient_surface"];
    policy.as_object_mut().unwrap().remove("surface_water");
    policy["cells_per_layer"] = json!(2);
    policy["initial_instance_temperatures_c"] = json!({"band":5.});
    let result = study(&raw, &input, &pattern);
    let simulation = &keyed(&result["simulations"], "qualified")["evaluation"]["result"];
    let motion = &simulation["motion"]["result"];
    let outcome = &motion["transient_surface"];
    assert_eq!(outcome["status"], "available", "{outcome}");
    let heat = &outcome["result"];
    let initial = &heat["initial_condition"];
    let frame = &simulation["drape"]["thermal_surface"];
    assert_eq!(heat["request"], raw["motion"]["transient_surface"]);
    assert_eq!(initial["model_sha256"], heat["model"]["sha256"]);
    assert_ne!(initial["model_sha256"], frame["model"]["sha256"]);
    assert_eq!(heat["model"]["columns"].as_array().unwrap().len(), 1);
    assert_eq!(initial["instances"], frame["instances"]);
    assert_eq!(initial["triangle_instance"], frame["triangle_instance"]);
    assert_eq!(initial["temperature_range_c"], json!([5., 20.]));
    let instances = initial["instances"].as_array().unwrap();
    let band = instances.iter().position(|id| id == "band").unwrap();
    let upper = instances.iter().position(|id| id == "upper").unwrap();
    assert_eq!(initial["instance_temperature_c"][band], 5.);
    assert_eq!(initial["instance_temperature_c"][upper], 20.);
    for (temperatures, owner) in heat["final"]["temperature_c"]
        .as_array()
        .unwrap()
        .iter()
        .zip(initial["triangle_instance"].as_array().unwrap())
    {
        let temperatures = temperatures.as_array().unwrap();
        assert_eq!(temperatures.len(), 2);
        for temperature in temperatures {
            let t = temperature.as_f64().unwrap();
            if owner.as_u64().unwrap() as usize == band {
                assert!(t > 5. && t < 20.);
            } else {
                assert!((t - 20.).abs() < 1e-10);
            }
        }
    }
    assert!(heat["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
    assert_eq!(
        result["comparison"]["frontier"],
        json!(["qualified"]),
        "{}",
        result["comparison"]
    );
    let readings = &keyed(&result["comparison"]["alternatives"], "qualified")["readings"];
    assert_eq!(
        readings["transient_surface_min_sample_temperature_c"]["value"],
        5.
    );
    assert!(
        (readings["transient_surface_max_sample_temperature_c"]["value"]
            .as_f64()
            .unwrap()
            - 20.)
            .abs()
            < 1e-10
    );
    let mut replay = input.clone();
    replay.variants[0].changes = serde_json::from_value(
        keyed(&result["manufacturing"]["alternatives"], "qualified")["frozen_changes"].clone(),
    )
    .unwrap();
    assert_eq!(
        study(&raw, &replay, &pattern)["simulations"],
        result["simulations"]
    );
    let mut unknown = raw.clone();
    unknown["motion"]["transient_surface"]["initial_instance_temperatures_c"] =
        json!({"unretained":5.});
    let refused = study(&unknown, &input, &pattern);
    let independent = &keyed(&refused["simulations"], "qualified")["evaluation"]["result"];
    assert_eq!(independent["motion"]["status"], "available");
    assert_eq!(
        independent["motion"]["result"]["transient_surface"]["status"],
        "refused"
    );
    assert_eq!(independent["drape"], simulation["drape"]);
    assert_eq!(
        independent["motion"]["result"]["samples"],
        motion["samples"]
    );
    assert_eq!(
        keyed(&refused["manufacturing"]["alternatives"], "qualified")["cutting"]["status"],
        "available"
    );
}

#[test]
fn portable_covered_instance_temperatures_keep_source_replay_and_independent_consumers() {
    let (pattern, mut input, mut raw) = inputs();
    let mut pattern = pattern.raw().clone();
    let mut upper = pattern["pieces"][0].clone();
    upper["id"] = json!("upper");
    upper["placement"]["y0"] = json!(-50.);
    pattern["pieces"].as_array_mut().unwrap().push(upper);
    pattern["seams"].as_array_mut().unwrap().push(json!({
        "id":"join-upper","a":"upper/right","b":"upper/left"}));
    let pattern = Pattern::new(pattern);
    input.variants.truncate(1);
    input.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal",
        "required_consumers":["transient_coverage"],"objectives":[
            {"metric":"transient_coverage_min_sample_temperature_c","direction":"maximize"},
            {"metric":"transient_coverage_max_sample_temperature_c","direction":"minimize"}]}))
        .unwrap(),
    );
    let coverage = raw.as_object_mut().unwrap().remove("coverage").unwrap();
    let sheet = &mut raw["motion"]["transient_surface"];
    sheet.as_object_mut().unwrap().remove("surface_water");
    let boundary = sheet["boundaries"][0].clone();
    let mut settings = coverage["settings"].clone();
    settings["cells_per_layer"] = json!(2);
    raw["motion"]["transient_coverage"] = json!({
        "schema":"ftx-motion-coverage-heat/1",
        "body_triangle_ids":(0..form().triangles.len()).collect::<Vec<_>>(),
        "instance_layers":{"band":"shell","upper":"shell"},"settings":settings,
        "initial_temperature_c":999.,"initial_instance_temperatures_c":{"band":5.,"upper":30.},
        "boundaries":[boundary],"exposed_boundaries":[boundary],
        "max_step_s":0.00025,"max_cell_steps":1000000,"max_geometry_operations":1000000});
    let result = study(&raw, &input, &pattern);
    let simulation = &keyed(&result["simulations"], "qualified")["evaluation"]["result"];
    let motion = &simulation["motion"]["result"];
    let outcome = &motion["transient_coverage"];
    assert_eq!(outcome["status"], "available", "{outcome}");
    let heat = &outcome["result"];
    let initial = &heat["initial_condition"];
    let source = &heat["checkpoint"]["frame"]["source"];
    assert_eq!(heat["request"], raw["motion"]["transient_coverage"]);
    assert_eq!(initial["model_sha256"], source["sha256"]);
    assert_eq!(initial["frame_sha256"], heat["initial_frame_sha256"]);
    assert_eq!(source["columns"].as_array().unwrap().len(), 1);
    assert_eq!(
        initial["instances"],
        simulation["drape"]["thermal_surface"]["instances"]
    );
    assert_eq!(
        initial["triangle_instance"],
        simulation["drape"]["thermal_surface"]["triangle_instance"]
    );
    assert_eq!(initial["temperature_range_c"], json!([5., 30.]));
    assert!(heat["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
    assert!(heat["absolute_remap_residual_j"].as_f64().unwrap().abs() < 1e-8);
    assert_eq!(
        result["comparison"]["frontier"],
        json!(["qualified"]),
        "{}",
        result["comparison"]
    );
    let readings = &keyed(&result["comparison"]["alternatives"], "qualified")["readings"];
    assert_eq!(
        readings["transient_coverage_min_sample_temperature_c"]["value"],
        5.
    );
    assert_eq!(
        readings["transient_coverage_max_sample_temperature_c"]["value"],
        30.
    );
    assert_eq!(motion["transient_surface"]["status"], "available");
    let mut replay = input.clone();
    replay.variants[0].changes = serde_json::from_value(
        keyed(&result["manufacturing"]["alternatives"], "qualified")["frozen_changes"].clone(),
    )
    .unwrap();
    assert_eq!(
        study(&raw, &replay, &pattern)["simulations"],
        result["simulations"]
    );
    let mut unknown = raw.clone();
    unknown["motion"]["transient_coverage"]["initial_instance_temperatures_c"] =
        json!({"unretained":5.});
    let refused = study(&unknown, &input, &pattern);
    let independent = &keyed(&refused["simulations"], "qualified")["evaluation"]["result"];
    assert_eq!(independent["motion"]["status"], "available");
    assert_eq!(
        independent["motion"]["result"]["transient_coverage"]["status"],
        "refused"
    );
    assert_eq!(
        independent["motion"]["result"]["samples"],
        motion["samples"]
    );
    for (scenario, evaluation) in [(&raw, simulation), (&unknown, independent)] {
        let policy: ftx_drape::study::motion::Request =
            serde_json::from_value(scenario["motion"].clone()).unwrap();
        let form = form();
        let endpoints = policy
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
        let path = ftx_drape::study::motion::Path::new(
            &policy,
            (&form.vertices, &form.triangles),
            endpoints,
        )
        .unwrap();
        let motion = &evaluation["motion"]["result"];
        let sheet = &motion["transient_surface"]["result"];
        assert_eq!(motion["path_sha256"], path.sha256());
        assert_eq!(sheet["path_sha256"], path.sha256());
        assert_eq!(sheet["request"], scenario["motion"]["transient_surface"]);
    }
    // The complete motion request changes its verified identity, while every physical sheet
    // result must remain exactly the same when only independent coverage is refused.
    let mut normalized = independent["motion"]["result"]["transient_surface"].clone();
    normalized["result"]["path_sha256"] =
        motion["transient_surface"]["result"]["path_sha256"].clone();
    assert_eq!(normalized, motion["transient_surface"]);
    assert_eq!(
        keyed(&refused["manufacturing"]["alternatives"], "qualified")["cutting"]["status"],
        "available"
    );
}

fn wet_instance_inputs() -> (Pattern, Request, Value) {
    let (pattern, mut input, mut raw) = inputs();
    let mut pattern = pattern.raw().clone();
    let mut upper = pattern["pieces"][0].clone();
    upper["id"] = json!("upper");
    upper["placement"]["y0"] = json!(-50.);
    pattern["pieces"].as_array_mut().unwrap().push(upper);
    pattern["seams"].as_array_mut().unwrap().push(json!({
        "id":"join-upper","a":"upper/right","b":"upper/left"}));
    let pattern = Pattern::new(pattern);
    input.variants.truncate(1);
    input.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal",
            "required_consumers":["transient_surface"],"objectives":[
                {"metric":"textile_water_evaporated_mass_kg","direction":"maximize"},
                {"metric":"transient_surface_final_min_temperature_c","direction":"minimize"}]}))
        .unwrap(),
    );
    raw.as_object_mut().unwrap().remove("coverage");
    let sheet = &mut raw["motion"]["transient_surface"];
    // Both actual instances are qualified. The unused fallback is deliberately outside
    // the water-pressure domain so scalar initialization cannot accidentally pass this case.
    sheet["initial_temperature_c"] = json!(999.);
    sheet["initial_instance_temperatures_c"] = json!({"band":5.,"upper":35.});
    let water = &mut sheet["surface_water"];
    water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(1e-6);
    water["bindings"]["upper"] = water["bindings"]["band"].clone();
    (pattern, input, raw)
}

#[test]
fn wet_instance_temperatures_conserve_and_replay_direct_and_selected_sources() {
    let (pattern, input, raw) = wet_instance_inputs();
    for (alternatives, condensation) in [(false, false), (true, false), (true, true)] {
        let mut case = raw.clone();
        let sheet = &mut case["motion"]["transient_surface"];
        if condensation {
            let water = &mut sheet["surface_water"];
            water["schema"] = json!("ftx-textile-surface-water/3");
            water
                .as_object_mut()
                .unwrap()
                .remove("replenishment_kg_m2_s");
            // One shared atmosphere condenses onto the cold piece while the warm piece
            // evaporates; equal material columns must retain their separate starting states.
            water["outside_vapor_pressure_pa"] = json!([4000.]);
            water["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
                "bindings":{"band":{"rate_kg_m2_s":[0.],"temperature_c":[20.]},
                    "upper":{"rate_kg_m2_s":[0.],"temperature_c":[20.]}},
                "source":{"reference":"test:mapped-wet-zero-supply","accessed_on":"2026-11-05",
                    "note":"Explicit synthetic zero supply; no empirical claim"},
                "evidence":"estimate","allow_estimates":true,"method":"Synthetic zero delivery"});
            water["condensation"] = json!({"schema":"ftx-textile-condensation/1",
                "vapor_temperature_c":[20.],
                "source":{"reference":"test:mapped-wet-condensation","accessed_on":"2026-11-05",
                    "note":"Synthetic reverse transport and effective sensible enthalpy"},
                "evidence":"estimate","allow_estimates":true,"method":"Synthetic bidirectional film",
                "qualification":"Artificial reverse use of the locked Ret and constant latent plus liquid-cp sensible enthalpy across the complete declared domain"});
        }
        let qualification = sheet["surface_water"].clone();
        if alternatives {
            let mut unmatched = qualification.clone();
            for instance in ["band", "upper"] {
                unmatched["bindings"][instance]["material_sha256"] = json!("0".repeat(64));
            }
            sheet.as_object_mut().unwrap().remove("surface_water");
            sheet["surface_water_alternatives"] = json!([unmatched, qualification]);
        }
        let result = study(&case, &input, &pattern);
        let simulation = &keyed(&result["simulations"], "qualified")["evaluation"]["result"];
        let motion = &simulation["motion"]["result"];
        assert_eq!(simulation["motion"]["status"], "available", "{simulation}");
        let outcome = &motion["transient_surface"];
        assert_eq!(outcome["status"], "available", "{outcome}");
        let heat = &outcome["result"];
        let water = &heat["surface_water"];
        assert_eq!(heat["request"], case["motion"]["transient_surface"]);
        assert_eq!(water["request"], qualification);
        assert_eq!(
            heat["initial_condition"]["temperature_range_c"],
            json!([5., 35.])
        );
        assert_eq!(heat["model"]["columns"].as_array().unwrap().len(), 1);
        if alternatives {
            assert_eq!(heat["surface_water_selection"]["index"], 1);
        }
        let mut evaporation = [0.; 2];
        let mut areas = [0.; 2];
        let mut initial_liquid_energy = 0.;
        let mut final_liquid_energy = 0.;
        for (index, patch) in water["patches"].as_array().unwrap().iter().enumerate() {
            let owner = match patch["instance"].as_str().unwrap() {
                "band" => 0,
                "upper" => 1,
                other => panic!("unexpected retained instance {other}"),
            };
            let initial_c = [5., 35.][owner];
            let final_c = heat["final"]["temperature_c"][index][0].as_f64().unwrap();
            areas[owner] += heat["model"]["patches"][index]["reference_area_m2"]
                .as_f64()
                .unwrap();
            evaporation[owner] += patch["evaporated_liquid_kg"].as_f64().unwrap();
            initial_liquid_energy +=
                patch["initial_liquid_kg"].as_f64().unwrap() * 4200. * initial_c;
            final_liquid_energy += patch["remaining_liquid_kg"].as_f64().unwrap() * 4200. * final_c;
            if condensation && owner == 0 {
                assert!(patch["condensed_liquid_kg"].as_f64().unwrap() > 0.);
                assert_eq!(patch["evaporated_liquid_kg"], 0.);
            }
        }
        assert!(areas.iter().all(|area| *area > 0.));
        assert!(evaporation[1] / areas[1] > evaporation[0] / areas[0]);
        assert!(
            (water["liquid_stored_change_j"].as_f64().unwrap()
                - (final_liquid_energy - initial_liquid_energy))
                .abs()
                < 1e-10
        );
        let optional = |field: &str| water[field].as_f64().unwrap_or(0.);
        let mass = water["remaining_liquid_kg"].as_f64().unwrap()
            + water["evaporated_liquid_kg"].as_f64().unwrap()
            - water["initial_liquid_kg"].as_f64().unwrap()
            - optional("supplied_liquid_kg")
            - optional("condensed_liquid_kg");
        assert!(mass.abs() < 1e-14);
        let energy = heat["stored_change_j"].as_f64().unwrap()
            + water["liquid_stored_change_j"].as_f64().unwrap()
            + water["exported_vapor_sensible_energy_j"].as_f64().unwrap()
            + water["latent_energy_j"].as_f64().unwrap()
            - optional("supplied_liquid_sensible_energy_j")
            - optional("incoming_vapor_sensible_energy_j")
            - optional("condensation_latent_energy_j")
            - heat["net_boundary_energy_j"].as_f64().unwrap();
        assert!(energy.abs() < 1e-8, "{energy}");
        assert_eq!(
            result["comparison"]["frontier"],
            json!(["qualified"]),
            "{}",
            result["comparison"]
        );
        let mut replay = input.clone();
        replay.variants[0].changes = serde_json::from_value(
            keyed(&result["manufacturing"]["alternatives"], "qualified")["frozen_changes"].clone(),
        )
        .unwrap();
        let replayed = study(&case, &replay, &pattern);
        assert_eq!(replayed["simulations"], result["simulations"]);
        assert_eq!(
            replayed["comparison"]["alternatives"],
            result["comparison"]["alternatives"]
        );

        let mut unknown = case.clone();
        unknown["motion"]["transient_surface"]["initial_instance_temperatures_c"]["upper"] =
            json!(90.);
        let refused = study(&unknown, &input, &pattern);
        let independent = &keyed(&refused["simulations"], "qualified")["evaluation"]["result"];
        assert_eq!(independent["motion"]["status"], "available");
        assert_eq!(
            independent["motion"]["result"]["transient_surface"]["status"],
            "refused"
        );
        assert_eq!(
            independent["motion"]["result"]["samples"],
            motion["samples"]
        );
        assert_eq!(
            keyed(&refused["manufacturing"]["alternatives"], "qualified")["cutting"]["status"],
            "available"
        );
        assert_eq!(
            keyed(&refused["comparison"]["alternatives"], "qualified")["eligible"],
            false
        );
    }
}

#[test]
fn coupled_instance_temperatures_preserve_phase_exchange_selection_and_motion_decisions() {
    let (pattern, mut input, mut raw) = wet_instance_inputs();
    let inertial_metrics = [
        "textile_water_final_inertial_mass_kg",
        "textile_water_max_sample_inertial_mass_kg",
    ];
    input.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal",
            "required_consumers":["motion","transient_surface"],"objectives":[
                {"metric":"textile_water_evaporated_mass_kg","direction":"maximize"},
                {"metric":"motion_max_absolute_edge_strain_pct","direction":"minimize"},
                {"metric":"textile_water_final_inertial_mass_kg","direction":"minimize"},
                {"metric":"textile_water_max_sample_inertial_mass_kg","direction":"minimize"}]}))
        .unwrap(),
    );
    raw["motion"]["keyframes"] = json!([
        {"time_s":0.001,"arm_abduction_deg":35.},
        {"time_s":0.002,"arm_abduction_deg":35.}]);
    raw["motion"]["max_trials"] = json!(16);
    let sheet = &mut raw["motion"]["transient_surface"];
    sheet["boundaries"] = json!([sheet["boundaries"][0], sheet["boundaries"][0]]);
    let water = &mut sheet["surface_water"];
    water["schema"] = json!("ftx-textile-surface-water/4");
    water
        .as_object_mut()
        .unwrap()
        .remove("replenishment_kg_m2_s");
    water["maximum_liquid_loading_kg_m2"] = json!(0.2);
    for instance in ["band", "upper"] {
        water["bindings"][instance]["initial_liquid_kg_m2"] = json!(0.04);
    }
    water["outside_vapor_pressure_pa"] = json!([4000., 4000.]);
    let provenance = json!({"reference":"test:nonuniform-coupled-thermal",
        "accessed_on":"2026-11-05","note":"Synthetic phase exchange; no measured garment claim"});
    water["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
        "bindings":{"band":{"rate_kg_m2_s":[0.002,0.001],"temperature_c":[20.,20.]},
            "upper":{"rate_kg_m2_s":[0.002,0.001],"temperature_c":[20.,20.]}},
        "source":provenance,"evidence":"estimate","allow_estimates":true,
        "method":"Synthetic independently prescribed delivery"});
    water["condensation"] = json!({"schema":"ftx-textile-condensation/1",
        "vapor_temperature_c":[20.,20.],"source":provenance,
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic bidirectional film",
        "qualification":"Artificial reverse Ret, nucleation and effective constant-latent/liquid-cp vapor enthalpy across the declared domain"});
    water["drainage"] = json!({"schema":"ftx-textile-liquid-drainage/1",
        "bindings":{"band":{"rate_kg_m2_s":[0.001,0.001]},
            "upper":{"rate_kg_m2_s":[0.001,0.001]}},
        "source":provenance,"evidence":"estimate","allow_estimates":true,
        "method":"Synthetic independently prescribed drainage",
        "qualification":"Stock-limited drainage at the source patch inner-cell temperature"});
    let water = water.clone();
    let uncoupled = study(&raw, &input, &pattern);
    let baseline = &keyed(&uncoupled["simulations"], "qualified")["evaluation"]["result"];
    assert_eq!(baseline["motion"]["status"], "available", "{baseline}");
    // Thermal-only liquid cannot provide the mass actually used by the mechanical solver.
    let uncoupled_decision = keyed(&uncoupled["comparison"]["alternatives"], "qualified");
    assert_eq!(uncoupled_decision["eligible"], false);
    for metric in inertial_metrics {
        assert!(uncoupled_decision["readings"].get(metric).is_none());
    }
    assert!(
        uncoupled_decision["readings"]["motion_max_absolute_edge_strain_pct"]["value"].is_number()
    );
    let binding = json!({"material_sha256":water["bindings"]["band"]["material_sha256"],
        "maximum_liquid_loading_kg_m2":0.2,"temperature_range_c":[-20.,80.],
        "incoming_stream_velocities_world_m_s_by_interval":[
            {"supply":[0.2,0.1,-0.5],"condensation":[-0.1,0.3,0.4]},
            {"supply":[-0.3,0.4,0.2],"condensation":[0.5,-0.2,0.1]}],
        "outgoing_stream_velocities_relative_to_cloth_m_s_by_interval":[
            {"evaporation":[0.,0.,0.4],"drainage":[0.2,0.,-0.1]},
            {"evaporation":[0.,0.,0.2],"drainage":[-0.1,0.1,-0.2]}]});
    let qualification = json!({"schema":"ftx-coupled-liquid-mechanics/6",
        "model":"scheduled_phase_specific_incoming_and_outgoing_velocity_mass_exchange_frozen_mechanics/1",
        "material_report_sha256":ftx_core::receipt::sha256_hex(&serde_json::to_vec(&baseline["drape"]["material"]).unwrap()),
        "bindings":{"band":binding,"upper":binding},
        "source":{"reference":"test:nonuniform-coupled-mechanics","accessed_on":"2026-11-05",
            "note":"Independent synthetic frozen mechanics and phase velocities"},
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic scheduled phase recoil",
        "qualification":"Exact material/interface laws over the complete water temperature and loading domain, with separately prescribed incoming and outgoing phase velocities"});
    let selected = json!({"surface_water":water,"mechanical_coupling":qualification});
    let mut decoy = selected.clone();
    for instance in ["band", "upper"] {
        decoy["surface_water"]["bindings"][instance]["material_sha256"] = json!("a".repeat(64));
        decoy["mechanical_coupling"]["bindings"][instance]["material_sha256"] =
            json!("a".repeat(64));
    }
    decoy["mechanical_coupling"]["material_report_sha256"] = json!("b".repeat(64));
    let mut direct_motion: Option<Value> = None;
    for paired in [false, true] {
        let mut case = raw.clone();
        let sheet = &mut case["motion"]["transient_surface"];
        if paired {
            sheet.as_object_mut().unwrap().remove("surface_water");
            sheet["coupled_water_alternatives"] = json!([decoy, selected]);
        } else {
            sheet["mechanical_coupling"] = qualification.clone();
        }
        let result = study(&case, &input, &pattern);
        let simulation = &keyed(&result["simulations"], "qualified")["evaluation"]["result"];
        assert_eq!(simulation["motion"]["status"], "available", "{simulation}");
        let motion = &simulation["motion"]["result"];
        let heat = &motion["transient_surface"]["result"];
        let water = &heat["surface_water"];
        let mechanics = &motion["liquid_mechanics"];
        assert_eq!(
            heat["initial_condition"]["temperature_range_c"],
            json!([5., 35.])
        );
        assert_eq!(heat["request"], case["motion"]["transient_surface"]);
        assert_eq!(water["request"], selected["surface_water"]);
        assert_eq!(mechanics["qualification"], qualification);
        assert_eq!(heat["mechanical_coupling"], *mechanics);
        assert_eq!(mechanics["schema"], "ftx-coupled-liquid-mechanics-result/6");
        let substrate_mass = simulation["drape"]["mass_kg"].as_f64().unwrap();
        let initial_mass = motion["initial_sample"]["liquid_mechanics"]["total_inertial_mass_kg"]
            .as_f64()
            .unwrap();
        let final_mass = mechanics["total_inertial_mass_kg"].as_f64().unwrap();
        let peak_mass = heat["trace"]
            .as_array()
            .unwrap()
            .iter()
            .fold(initial_mass, |peak, row| {
                peak.max(
                    row["mechanical_coupling"]["total_inertial_mass_kg"]
                        .as_f64()
                        .unwrap(),
                )
            });
        assert_eq!(mechanics["substrate_and_attached_mass_kg"], substrate_mass);
        assert!(initial_mass > substrate_mass);
        assert!(
            (initial_mass - substrate_mass - water["initial_liquid_kg"].as_f64().unwrap()).abs()
                < 1e-14
        );
        assert!(
            (final_mass - substrate_mass - water["remaining_liquid_kg"].as_f64().unwrap()).abs()
                < 1e-14
        );
        assert!(peak_mass >= initial_mass && peak_mass >= final_mass);
        for (mechanical, thermal) in [
            ("remaining_liquid_mass_kg", "remaining_liquid_kg"),
            ("evaporated_liquid_mass_kg", "evaporated_liquid_kg"),
            ("drained_liquid_mass_kg", "drained_liquid_kg"),
        ] {
            let mass = water[thermal].as_f64().unwrap();
            assert!(mass > 0.);
            assert!((mechanics[mechanical].as_f64().unwrap() - mass).abs() < 1e-14);
        }
        for patch in water["patches"].as_array().unwrap() {
            let phase = if patch["instance"] == "band" {
                "condensed_liquid_kg"
            } else {
                "evaporated_liquid_kg"
            };
            assert!(patch[phase].as_f64().unwrap() > 0., "{patch}");
        }
        for field in [
            "mass_balance_residual_kg",
            "transfer_kinetic_residual_j",
            "transfer_gravitational_residual_j",
        ] {
            assert!(
                mechanics[field].as_f64().unwrap().abs() < 1e-12,
                "{mechanics}"
            );
        }
        for residual in mechanics["transfer_momentum_residual_kg_m_s"]
            .as_array()
            .unwrap()
        {
            assert!(residual.as_f64().unwrap().abs() < 1e-12);
        }
        for field in ["mixing_dissipation_j", "outgoing_separation_work_j"] {
            assert!(mechanics["exchange"][field].as_f64().unwrap() > 0.);
        }
        assert!(heat["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
        assert_ne!(
            motion["final_positions_sha256"],
            baseline["motion"]["result"]["final_positions_sha256"]
        );
        assert_eq!(
            result["comparison"]["frontier"],
            json!(["qualified"]),
            "{}",
            result["comparison"]
        );
        let decision = keyed(&result["comparison"]["alternatives"], "qualified");
        for (metric, expected) in inertial_metrics.into_iter().zip([final_mass, peak_mass]) {
            assert_eq!(decision["readings"][metric]["value"], expected, "{metric}");
            assert_eq!(decision["readings"][metric]["unit"], "kg");
            assert!(decision["readings"][metric]["reported_bounds"].is_null());
        }
        // Reuse the complete actual producer output to test inclusive guards and violations.
        // The callback does not solve another trajectory or fabricate a mass history.
        for violation in 0..3 {
            let mut policy = serde_json::to_value(input.comparison.as_ref().unwrap()).unwrap();
            policy["constraints"] = json!([
                {"metric":inertial_metrics[0],"minimum":final_mass,"maximum":final_mass},
                {"metric":inertial_metrics[1],"minimum":peak_mass,"maximum":peak_mass}]);
            if violation == 1 {
                policy["constraints"][0]["minimum"] = json!(final_mass * 2.);
                policy["constraints"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("maximum");
            } else if violation == 2 {
                policy["constraints"][1]["maximum"] = json!(peak_mass * 0.5);
                policy["constraints"][1]
                    .as_object_mut()
                    .unwrap()
                    .remove("minimum");
            }
            let mut guarded = input.clone();
            guarded.comparison = Some(serde_json::from_value(policy).unwrap());
            let guarded = serde_json::to_value(
                guarded
                    .evaluate_with(&pattern, &result["scenario"], |_, _| Ok(simulation.clone()))
                    .unwrap(),
            )
            .unwrap();
            let guarded = keyed(&guarded["comparison"]["alternatives"], "qualified");
            assert_eq!(guarded["eligible"], violation == 0, "{guarded}");
            assert_eq!(guarded["readings"], decision["readings"]);
        }
        if let Some(direct) = &direct_motion {
            for field in [
                "initial_sample",
                "samples",
                "final_positions_sha256",
                "final_velocities_m_s",
                "liquid_mechanics",
            ] {
                assert_eq!(motion[field], direct[field], "{field}");
            }
            for field in [
                "initial_condition",
                "final",
                "trace",
                "surface_water",
                "stored_change_j",
                "net_boundary_energy_j",
                "balance_error_j",
            ] {
                assert_eq!(
                    heat[field], direct["transient_surface"]["result"][field],
                    "{field}"
                );
            }
            assert_eq!(heat["coupled_water_selection"]["index"], 1);
        } else {
            direct_motion = Some(motion.clone());
        }
        let mut replay = input.clone();
        replay.variants[0].changes = serde_json::from_value(
            keyed(&result["manufacturing"]["alternatives"], "qualified")["frozen_changes"].clone(),
        )
        .unwrap();
        let replayed = study(&case, &replay, &pattern);
        assert_eq!(replayed["simulations"], result["simulations"]);
        assert_eq!(
            replayed["comparison"]["alternatives"],
            result["comparison"]["alternatives"]
        );

        // Even a motion-only decision depends on the full actual coupled initial state.
        // A forged receipt must not become eligible by dropping thermal objectives.
        let mut motion_only = input.clone();
        motion_only.comparison = Some(
            serde_json::from_value(json!({"basis":"nominal",
            "required_consumers":["motion"],"objectives":[
                {"metric":"motion_max_absolute_edge_strain_pct","direction":"minimize"}]}))
            .unwrap(),
        );
        for forged in [false, true] {
            let mut retained = simulation.clone();
            if forged {
                retained["motion"]["result"]["transient_surface"]["result"]["initial_condition"]
                    ["instance_temperature_c"][0] = json!(20.);
            }
            let admitted = serde_json::to_value(
                motion_only
                    .evaluate_with(&pattern, &result["scenario"], |_, _| Ok(retained.clone()))
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                keyed(&admitted["comparison"]["alternatives"], "qualified")["eligible"],
                !forged
            );
        }
        let mut bad = case.clone();
        bad["motion"]["transient_surface"]["initial_instance_temperatures_c"]["upper"] = json!(90.);
        let refused = study(&bad, &input, &pattern);
        let independent = &keyed(&refused["simulations"], "qualified")["evaluation"]["result"];
        assert_eq!(independent["motion"]["status"], "refused");
        assert!(independent["motion"]["result"].is_null());
        assert_eq!(independent["drape"], simulation["drape"]);
        assert_eq!(
            keyed(&refused["manufacturing"]["alternatives"], "qualified")["cutting"]["status"],
            "available"
        );
        let refused = keyed(&refused["comparison"]["alternatives"], "qualified");
        assert_eq!(refused["eligible"], false);
        for metric in inertial_metrics {
            assert!(refused["readings"].get(metric).is_none());
        }
    }
}

#[test]
fn source_selected_water_cools_two_materials_under_one_unchanged_scenario() {
    let (pattern, input, mut raw) = inputs();
    let original_pattern = pattern.raw().clone();
    let mut input = serde_json::to_value(input).unwrap();
    let template = input["variants"][0].clone();
    let mut variants = Vec::new();
    let mut qualifications = Vec::new();
    for (key, resistance) in [("qualified", 8.), ("slower", 32.), ("unqualified", 16.)] {
        let mut variant = template.clone();
        variant["key"] = json!(key);
        let material = &mut variant["changes"]["default"];
        // The unmatched revision deliberately keeps the first material's public ID; only its
        // changed source hash can prevent silently reusing that revision's qualification.
        let id = format!(
            "synthetic-water-{}",
            if key == "unqualified" {
                "qualified"
            } else {
                key
            }
        );
        material["id"] = json!(id);
        material["record"]["id"] = json!(id);
        material["record"]["properties"]["vapor_resistance"]["value"] = json!(resistance);
        let selection: Selection = serde_json::from_value(material.clone()).unwrap();
        let selected = Catalog::builtin().unwrap().resolve(&selection).unwrap();
        if key != "unqualified" {
            let mut water = raw["motion"]["transient_surface"]["surface_water"].clone();
            water["bindings"]["band"]["material_sha256"] = json!(selected.sha256());
            // Both films stay wet: evaporation remains sensitive to the selected Ret instead
            // of tying because an identical finite inventory has completely dried out.
            water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(1e-6);
            water["source"]["reference"] = json!(format!("test:water-qualification/{key}"));
            qualifications.push(water);
        }
        variants.push(variant);
    }
    input["variants"] = json!(variants);
    input["comparison"]["objectives"] = json!([
        {"metric":"textile_water_evaporated_mass_kg","direction":"maximize"},
        {"metric":"transient_surface_final_min_temperature_c","direction":"minimize"}]);
    input["comparison"]["constraints"][0] =
        json!({"metric":"textile_water_remaining_mass_kg","minimum":0.});
    let input: Request = serde_json::from_value(input).unwrap();
    let heat_policy = &mut raw["motion"]["transient_surface"];
    heat_policy.as_object_mut().unwrap().remove("surface_water");
    heat_policy["surface_water_alternatives"] = json!(qualifications);
    let original_scenario = raw.clone();
    let result = study(&raw, &input, &pattern);
    assert_eq!(pattern.raw(), &original_pattern);
    assert_eq!(raw, original_scenario);
    assert_eq!(
        result["scenario"]["scenario"]["motion"]["transient_surface"]["surface_water_alternatives"],
        json!(qualifications)
    );
    let heat = |key| {
        &keyed(&result["simulations"], key)["evaluation"]["result"]["motion"]["result"]["transient_surface"]
            ["result"]
    };
    for (index, key) in ["qualified", "slower"].into_iter().enumerate() {
        let simulation = &keyed(&result["simulations"], key)["evaluation"]["result"];
        assert_eq!(simulation["motion"]["status"], "available");
        let outcome = &simulation["motion"]["result"]["transient_surface"];
        assert_eq!(outcome["status"], "available", "{outcome}");
        let heat = heat(key);
        assert_eq!(
            heat["request"]["surface_water_alternatives"],
            json!(qualifications)
        );
        assert_eq!(heat["surface_water"]["request"], qualifications[index]);
        assert_eq!(heat["surface_water_selection"]["index"], index);
        assert_eq!(
            heat["surface_water_selection"]["request_sha256"],
            ftx_core::receipt::sha256_hex(&serde_json::to_vec(&qualifications[index]).unwrap())
        );
        let water = &heat["surface_water"];
        assert!(water["evaporated_liquid_kg"].as_f64().unwrap() > 0.);
        assert!(water["remaining_liquid_kg"].as_f64().unwrap() > 0.);
        let reading = keyed(&result["comparison"]["alternatives"], key);
        assert_eq!(reading["eligible"], true, "{reading}");
        assert_eq!(
            reading["readings"]["textile_water_evaporated_mass_kg"]["value"],
            water["evaporated_liquid_kg"]
        );
        let balance = heat["stored_change_j"].as_f64().unwrap()
            + water["liquid_stored_change_j"].as_f64().unwrap()
            + water["exported_vapor_sensible_energy_j"].as_f64().unwrap()
            + water["latent_energy_j"].as_f64().unwrap()
            - heat["net_boundary_energy_j"].as_f64().unwrap();
        assert!(balance.abs() < 1e-10);
    }
    assert!(
        heat("qualified")["surface_water"]["evaporated_liquid_kg"]
            .as_f64()
            .unwrap()
            > heat("slower")["surface_water"]["evaporated_liquid_kg"]
                .as_f64()
                .unwrap()
    );
    assert!(
        heat("qualified")["final"]["temperature_c"][0][0]
            .as_f64()
            .unwrap()
            < heat("slower")["final"]["temperature_c"][0][0]
                .as_f64()
                .unwrap()
    );
    assert_eq!(result["comparison"]["frontier"], json!(["qualified"]));
    let unmatched = &keyed(&result["simulations"], "unqualified")["evaluation"]["result"];
    assert_eq!(unmatched["motion"]["status"], "available");
    assert_eq!(unmatched["surface"]["status"], "available");
    assert_eq!(
        unmatched["motion"]["result"]["transient_surface"]["status"],
        "refused"
    );
    assert_eq!(
        keyed(&result["comparison"]["alternatives"], "unqualified")["eligible"],
        false
    );
    let expected_motion =
        &keyed(&result["simulations"], "qualified")["evaluation"]["result"]["motion"]["result"];
    for key in ["slower", "unqualified"] {
        let motion =
            &keyed(&result["simulations"], key)["evaluation"]["result"]["motion"]["result"];
        for field in [
            "initial_positions_sha256",
            "final_positions_sha256",
            "initial_body_collision_sha256",
            "path_sha256",
        ] {
            assert_eq!(motion[field], expected_motion[field]);
        }
    }
    let mut replay = input.clone();
    for variant in &mut replay.variants {
        let row = keyed(&result["manufacturing"]["alternatives"], &variant.key);
        assert_eq!(row["cutting"]["status"], "available");
        variant.changes = serde_json::from_value(row["frozen_changes"].clone()).unwrap();
    }
    let replayed = study(&raw, &replay, &pattern);
    assert_eq!(replayed["simulations"], result["simulations"]);
    assert_eq!(
        replayed["comparison"]["alternatives"],
        result["comparison"]["alternatives"]
    );
}

#[test]
fn source_selected_coupled_water_changes_cooling_and_motion_from_ret_alone() {
    let (pattern, input, mut raw) = inputs();
    let original_pattern = pattern.raw().clone();
    raw["motion"]["keyframes"] = json!([
        {"time_s":0.0005,"arm_abduction_deg":35.},
        {"time_s":0.001,"arm_abduction_deg":35.}]);
    let heat = &mut raw["motion"]["transient_surface"];
    heat["boundaries"] = json!([heat["boundaries"][0], heat["boundaries"][0]]);
    let mut water = heat
        .as_object_mut()
        .unwrap()
        .remove("surface_water")
        .unwrap();
    // Reuse the existing native /6 recoil fixture's physical inputs. The two materials differ
    // only in Ret; incoming supply, drainage, phase velocities and dry mechanics stay identical.
    water["schema"] = json!("ftx-textile-surface-water/4");
    water
        .as_object_mut()
        .unwrap()
        .remove("replenishment_kg_m2_s");
    water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(0.);
    water["maximum_liquid_loading_kg_m2"] = json!(0.2);
    water["outside_vapor_pressure_pa"] = json!([1000., 1000.]);
    water["qualification"] =
        json!("Synthetic invariant thermal/vapor laws over [-20,80] C and [0,0.2] kg/m2");
    water["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
        "bindings":{"band":{"rate_kg_m2_s":[100.,100.],"temperature_c":[20.,20.]}},
        "source":{"reference":"test:paired-portable-supply","accessed_on":"2026-11-04",
            "note":"Artificial delivery shared by every material, no empirical claim"},
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic prescribed supply"});
    water["drainage"] = json!({"schema":"ftx-textile-liquid-drainage/1",
        "bindings":{"band":{"rate_kg_m2_s":[1.,2.]}},
        "source":{"reference":"test:paired-portable-drainage","accessed_on":"2026-11-04",
            "note":"Artificial removal shared by every material, no inferred runoff"},
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic prescribed drainage",
        "qualification":"Stock-limited removal at the retained liquid temperature"});

    let mut input = serde_json::to_value(input).unwrap();
    let template = input["variants"][0].clone();
    let mut variants = Vec::new();
    let mut waters = Vec::new();
    for (key, resistance) in [("qualified", 8.), ("slower", 32.), ("stale", 16.)] {
        let mut variant = template.clone();
        variant["key"] = json!(key);
        let material = &mut variant["changes"]["default"];
        // Reusing a public ID must not authorize a changed material record.
        let id = format!(
            "synthetic-paired-{}",
            if key == "stale" { "qualified" } else { key }
        );
        material["id"] = json!(id);
        material["record"]["id"] = json!(id);
        material["record"]["properties"]["vapor_resistance"]["value"] = json!(resistance);
        if key != "stale" {
            let selected: Selection = serde_json::from_value(material.clone()).unwrap();
            let selected = Catalog::builtin().unwrap().resolve(&selected).unwrap();
            let mut water = water.clone();
            water["bindings"]["band"]["material_sha256"] = json!(selected.sha256());
            water["source"]["reference"] = json!(format!("test:paired-portable-thermal/{key}"));
            waters.push(water);
        }
        variants.push(variant);
    }
    input["variants"] = json!(variants);
    input["comparison"]["required_consumers"] = json!(["motion", "transient_surface"]);
    input["comparison"]["objectives"] = json!([
        {"metric":"textile_water_evaporated_mass_kg","direction":"maximize"},
        {"metric":"transient_surface_final_min_temperature_c","direction":"minimize"}]);
    input["comparison"]["constraints"][0] =
        json!({"metric":"textile_water_remaining_mass_kg","minimum":0.});
    let input: Request = serde_json::from_value(input).unwrap();
    // Each qualification binds the actual complete dry material/interface report. Equal dry
    // trajectories establish that later differences arise from material-dependent wet coupling.
    let mut dry_request = input.clone();
    dry_request.comparison = None;
    let baseline = study(&raw, &dry_request, &pattern);
    let dry_motion =
        &keyed(&baseline["simulations"], "qualified")["evaluation"]["result"]["motion"]["result"];
    let mut pairs = Vec::new();
    for (index, key) in ["qualified", "slower"].into_iter().enumerate() {
        let dry = &keyed(&baseline["simulations"], key)["evaluation"]["result"];
        assert_eq!(dry["motion"]["status"], "available");
        assert_eq!(
            dry["motion"]["result"]["final_positions_sha256"],
            dry_motion["final_positions_sha256"]
        );
        let mechanics = json!({"schema":"ftx-coupled-liquid-mechanics/6",
            "model":"scheduled_phase_specific_incoming_and_outgoing_velocity_mass_exchange_frozen_mechanics/1",
            "material_report_sha256":ftx_core::receipt::sha256_hex(&serde_json::to_vec(&dry["drape"]["material"]).unwrap()),
            "bindings":{"band":{"material_sha256":waters[index]["bindings"]["band"]["material_sha256"],
                "maximum_liquid_loading_kg_m2":0.2,"temperature_range_c":[-20.,80.],
                "incoming_stream_velocities_world_m_s_by_interval":[
                    {"supply":[0.2,0.1,-0.5],"condensation":[-0.1,0.3,0.4]},
                    {"supply":[-0.3,0.4,0.2],"condensation":[0.5,-0.2,0.1]}],
                "outgoing_stream_velocities_relative_to_cloth_m_s_by_interval":[
                    {"evaporation":[0.,0.,0.4],"drainage":[0.2,0.,-0.1]},
                    {"evaporation":[0.,0.,0.2],"drainage":[-0.1,0.1,-0.2]}]}},
            "source":{"reference":format!("test:paired-portable-mechanics/{key}"),"accessed_on":"2026-11-04",
                "note":"Independent artificial mechanical qualification, no measured wet laws"},
            "evidence":"estimate","allow_estimates":true,"method":"Synthetic scheduled phase recoil",
            "qualification":"Exact artificial material and interface laws over the complete thermal domain; prescribed phase velocities and separate mechanical boundary work"});
        pairs.push(json!({"surface_water":waters[index],"mechanical_coupling":mechanics}));
    }
    raw["motion"]["transient_surface"]["coupled_water_alternatives"] = json!(pairs);
    let original_scenario = raw.clone();
    let result = study(&raw, &input, &pattern);
    assert_eq!(pattern.raw(), &original_pattern);
    assert_eq!(raw, original_scenario);
    assert_eq!(
        result["scenario"]["scenario"]["motion"]["transient_surface"]["coupled_water_alternatives"],
        json!(pairs)
    );

    let shared_motion =
        &keyed(&result["simulations"], "qualified")["evaluation"]["result"]["motion"]["result"];
    let mut evaporation = Vec::new();
    let mut temperature = Vec::new();
    let mut trajectories = Vec::new();
    let mut outgoing_momenta = Vec::new();
    for (index, key) in ["qualified", "slower"].into_iter().enumerate() {
        let simulated = &keyed(&result["simulations"], key)["evaluation"]["result"];
        assert_eq!(
            simulated["motion"]["status"], "available",
            "{}",
            simulated["motion"]
        );
        let motion = &simulated["motion"]["result"];
        let outcome = &motion["transient_surface"];
        assert_eq!(outcome["status"], "available", "{outcome}");
        let heat = &outcome["result"];
        let water = &heat["surface_water"];
        let mechanics = &motion["liquid_mechanics"];
        assert_eq!(mechanics, &heat["mechanical_coupling"]);
        assert_eq!(mechanics["schema"], "ftx-coupled-liquid-mechanics-result/6");
        assert_eq!(
            mechanics["qualification"],
            pairs[index]["mechanical_coupling"]
        );
        assert_eq!(water["request"], pairs[index]["surface_water"]);
        assert_eq!(heat["request"]["coupled_water_alternatives"], json!(pairs));
        assert_eq!(heat["coupled_water_selection"]["index"], index);
        assert_eq!(
            heat["coupled_water_selection"]["pair_sha256"],
            ftx_core::receipt::sha256_hex(&serde_json::to_vec(&pairs[index]).unwrap())
        );
        assert_eq!(
            mechanics["qualification"]["material_report_sha256"],
            ftx_core::receipt::sha256_hex(
                &serde_json::to_vec(&simulated["drape"]["material"]).unwrap(),
            )
        );
        assert_eq!(
            simulated["drape"]["material"]["record_sha256"],
            water["request"]["bindings"]["band"]["material_sha256"]
        );
        for (mechanical, thermal) in [
            ("remaining_liquid_mass_kg", "remaining_liquid_kg"),
            ("evaporated_liquid_mass_kg", "evaporated_liquid_kg"),
            ("drained_liquid_mass_kg", "drained_liquid_kg"),
        ] {
            let mass = water[thermal].as_f64().unwrap();
            assert!(mass > 0.);
            assert!((mechanics[mechanical].as_f64().unwrap() - mass).abs() < 1e-14);
        }
        assert!(
            mechanics["exchange"]["outgoing_separation_work_j"]
                .as_f64()
                .unwrap()
                > 0.
        );
        assert!(
            mechanics["mass_balance_residual_kg"]
                .as_f64()
                .unwrap()
                .abs()
                < 1e-14
        );
        assert!(
            mechanics["transfer_kinetic_residual_j"]
                .as_f64()
                .unwrap()
                .abs()
                < 1e-12
        );
        for residual in mechanics["transfer_momentum_residual_kg_m_s"]
            .as_array()
            .unwrap()
        {
            assert!(residual.as_f64().unwrap().abs() < 1e-12);
        }
        assert!(heat["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
        for field in [
            "initial_positions_sha256",
            "initial_body_collision_sha256",
            "final_body_collision_sha256",
        ] {
            assert_eq!(motion[field], dry_motion[field]);
        }
        assert_eq!(motion["path_sha256"], shared_motion["path_sha256"]);
        assert_ne!(
            motion["final_positions_sha256"],
            dry_motion["final_positions_sha256"]
        );
        evaporation.push(water["evaporated_liquid_kg"].as_f64().unwrap());
        temperature.push(heat["final"]["temperature_c"][0][0].as_f64().unwrap());
        trajectories.push(&motion["final_positions_sha256"]);
        outgoing_momenta.push(&mechanics["exported_momentum_kg_m_s"]);
        let decision = keyed(&result["comparison"]["alternatives"], key);
        assert_eq!(decision["eligible"], true, "{decision}");
        assert_eq!(
            decision["readings"]["textile_water_evaporated_mass_kg"]["value"],
            water["evaporated_liquid_kg"]
        );
    }
    assert!(evaporation[0] > evaporation[1]);
    assert!(temperature[0] < temperature[1]);
    assert_ne!(trajectories[0], trajectories[1]);
    assert_ne!(outgoing_momenta[0], outgoing_momenta[1]);
    assert_eq!(result["comparison"]["frontier"], json!(["qualified"]));
    let stale = &keyed(&result["simulations"], "stale")["evaluation"]["result"];
    assert_eq!(stale["surface"]["status"], "available");
    assert_eq!(stale["motion"]["status"], "refused");
    assert!(stale["motion"]["result"].is_null());
    assert_eq!(
        keyed(&result["comparison"]["alternatives"], "stale")["eligible"],
        false
    );
    let mut replay = input.clone();
    for variant in &mut replay.variants {
        let row = keyed(&result["manufacturing"]["alternatives"], &variant.key);
        assert_eq!(row["cutting"]["status"], "available");
        variant.changes = serde_json::from_value(row["frozen_changes"].clone()).unwrap();
    }
    let replayed = study(&raw, &replay, &pattern);
    assert_eq!(replayed["simulations"], result["simulations"]);
    assert_eq!(
        replayed["comparison"]["alternatives"],
        result["comparison"]["alternatives"]
    );

    // Requesting only a motion objective must still admit the complete coupled heat history.
    // It must not make stale wet mechanics eligible by omitting a thermal comparison consumer.
    let mut motion_only = input.clone();
    motion_only.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal","required_consumers":["motion"],
            "objectives":[{"metric":"motion_max_absolute_edge_strain_pct","direction":"minimize"}]}))
        .unwrap(),
    );
    let motion_result = study(&raw, &motion_only, &pattern);
    for key in ["qualified", "slower"] {
        let decision = keyed(&motion_result["comparison"]["alternatives"], key);
        assert_eq!(decision["eligible"], true, "{decision}");
        assert!(
            decision["readings"]["motion_max_absolute_edge_strain_pct"]["value"]
                .as_f64()
                .unwrap()
                .is_finite()
        );
    }
    assert_eq!(
        keyed(&motion_result["comparison"]["alternatives"], "stale")["eligible"],
        false
    );
}

#[test]
fn finite_vapor_reservoir_decisions_follow_sampled_pressure_and_conserve_the_whole_system() {
    let (pattern, mut input, mut raw) = inputs();
    input.variants.truncate(1);
    let metrics = [
        "textile_water_final_vapor_pressure_pa",
        "textile_water_min_sample_vapor_pressure_pa",
        "textile_water_max_sample_vapor_pressure_pa",
        "textile_water_remaining_vapor_mass_kg",
        "textile_water_isothermal_heat_removed_j",
    ];
    let mut objectives: Vec<Value> = metrics
        .iter()
        .map(|metric| json!({"metric":metric,"direction":"minimize"}))
        .collect();
    for metric in [
        "motion_max_absolute_edge_strain_pct",
        "transient_surface_final_min_temperature_c",
    ] {
        objectives.push(json!({"metric":metric,"direction":"minimize"}));
    }
    let policy = json!({"basis":"nominal","required_consumers":["transient_surface"],
        "objectives":objectives});
    input.comparison = Some(serde_json::from_value(policy.clone()).unwrap());
    raw.as_object_mut().unwrap().remove("coverage");
    raw["motion"]["keyframes"] = json!([
        {"time_s":0.001,"arm_abduction_deg":35.},
        {"time_s":0.002,"arm_abduction_deg":35.}]);
    raw["motion"]["max_trials"] = json!(16);
    let sheet = &mut raw["motion"]["transient_surface"];
    sheet["initial_temperature_c"] = json!(10.);
    sheet["boundaries"] = json!([sheet["boundaries"][0], sheet["boundaries"][0]]);
    let water = &mut sheet["surface_water"];
    water["schema"] = json!("ftx-textile-surface-water/5");
    water
        .as_object_mut()
        .unwrap()
        .remove("replenishment_kg_m2_s");
    water
        .as_object_mut()
        .unwrap()
        .remove("outside_vapor_pressure_pa");
    water["bindings"]["band"]["initial_liquid_kg_m2"] = json!(0.01);
    water["maximum_liquid_loading_kg_m2"] = json!(0.2);
    water["qualification"] = json!(
        "Synthetic invariant thermal and vapor laws over [-20,80] C and [0,0.2] kg/m2; no empirical garment claim"
    );
    let provenance = json!({"reference":"test:portable-finite-vapor-storage",
        "accessed_on":"2026-11-05","note":"Artificial well-mixed storage and prescribed cold feed; no physiological or empirical claim"});
    // Cold evaporation raises pressure first. Independently prescribed cold liquid then
    // reduces wet-film pressure and causes condensation: the retained maximum is interior.
    water["supply"] = json!({"schema":"ftx-textile-liquid-supply/1",
        "bindings":{"band":{"rate_kg_m2_s":[0.,50.],"temperature_c":[-20.,-20.]}},
        "source":provenance,"evidence":"estimate","allow_estimates":true,
        "method":"Synthetic piecewise reference-area liquid delivery"});
    water["condensation"] = json!({"schema":"ftx-textile-condensation/1",
        "vapor_temperature_c":[20.,20.],"source":provenance,
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic reverse vapor transfer",
        "qualification":"Artificial reverse Ret, nucleation and effective constant-latent/liquid-cp vapor enthalpy over the declared domain"});
    water["vapor_reservoir"] = json!({"schema":"ftx-textile-vapor-reservoir/1",
        "model":"isothermal_constant_vapor_capacitance/1",
        "vapor_capacitance_kg_pa":1e-13,"initial_vapor_mass_kg":1e-10,
        "temperature_c":20.,"pressure_domain_pa":[0.,10000.],
        "max_iterations":128,"mass_tolerance_kg":1e-15,
        "source":provenance,"evidence":"estimate","allow_estimates":true,
        "method":"Synthetic explicit pressure capacitance and isothermal energy exchange",
        "qualification":"Constant storage over the complete pressure domain with the declared effective vapor enthalpy; no inferred ideal-gas law"});
    let water_policy = water.clone();
    let direct = study(&raw, &input, &pattern);
    let direct_simulation = &keyed(&direct["simulations"], "qualified")["evaluation"]["result"];
    assert_eq!(
        direct_simulation["motion"]["status"], "available",
        "{direct_simulation}"
    );
    let mechanical = json!({"schema":"ftx-coupled-liquid-mechanics/1",
        "model":"co_moving_mass_exchange_frozen_mechanics/1",
        "material_report_sha256":ftx_core::receipt::sha256_hex(&serde_json::to_vec(&direct_simulation["drape"]["material"]).unwrap()),
        "bindings":{"band":{"material_sha256":water_policy["bindings"]["band"]["material_sha256"],
            "maximum_liquid_loading_kg_m2":0.2,"temperature_range_c":[-20.,80.]}},
        "source":provenance,"evidence":"estimate","allow_estimates":true,
        "method":"Synthetic conservative co-moving mass exchange",
        "qualification":"Exact dry material and interface laws qualified over the complete liquid loading and temperature domains"});
    let mut decoy = water_policy.clone();
    decoy["bindings"]["band"]["material_sha256"] = json!("a".repeat(64));
    let pair = json!({"surface_water":water_policy,"mechanical_coupling":mechanical});
    let mut decoy_pair = pair.clone();
    decoy_pair["surface_water"] = decoy.clone();
    decoy_pair["mechanical_coupling"]["bindings"]["band"]["material_sha256"] =
        json!("a".repeat(64));
    decoy_pair["mechanical_coupling"]["material_report_sha256"] = json!("b".repeat(64));
    for selection in 0..3 {
        let mut case = raw.clone();
        let sheet = &mut case["motion"]["transient_surface"];
        if selection > 0 {
            sheet.as_object_mut().unwrap().remove("surface_water");
            if selection == 1 {
                sheet["surface_water_alternatives"] = json!([decoy, water_policy]);
            } else {
                sheet["coupled_water_alternatives"] = json!([decoy_pair, pair]);
            }
        }
        let result = if selection == 0 {
            direct.clone()
        } else {
            study(&case, &input, &pattern)
        };
        let simulation = &keyed(&result["simulations"], "qualified")["evaluation"]["result"];
        let motion = &simulation["motion"]["result"];
        assert_eq!(simulation["motion"]["status"], "available", "{simulation}");
        let outcome = &motion["transient_surface"];
        assert_eq!(outcome["status"], "available", "{outcome}");
        let heat = &outcome["result"];
        let water = &heat["surface_water"];
        let gas = &water["vapor_reservoir"];
        assert_eq!(water["request"], water_policy);
        assert_eq!(water["schema"], "ftx-textile-surface-water-result/5");
        assert!(water["evaporated_liquid_kg"].as_f64().unwrap() > 0.);
        assert!(water["condensed_liquid_kg"].as_f64().unwrap() > 0.);
        assert!(gas["isothermal_heat_removed_j"].as_f64().unwrap() < 0.);
        let initial_pressure = 1e-10 / 1e-13;
        let final_pressure = gas["vapor_pressure_pa"].as_f64().unwrap();
        let mut minimum = initial_pressure;
        let mut maximum = initial_pressure;
        let area: f64 = heat["model"]["patches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|patch| patch["reference_area_m2"].as_f64().unwrap())
            .sum();
        let samples = heat["trace"].as_array().unwrap();
        assert!(samples.len() >= 2);
        for sample in samples {
            let w = &sample["surface_water"];
            let g = &w["vapor_reservoir"];
            let pressure = g["vapor_pressure_pa"].as_f64().unwrap();
            minimum = f64::min(minimum, pressure);
            maximum = f64::max(maximum, pressure);
            let supplied = area * 50. * (sample["time_s"].as_f64().unwrap() - 0.001).max(0.);
            assert!((w["supplied_liquid_kg"].as_f64().unwrap() - supplied).abs() < 1e-14);
            let gas_mass = g["remaining_vapor_mass_kg"].as_f64().unwrap();
            let evaporated = w["evaporated_liquid_kg"].as_f64().unwrap();
            let condensed = w["condensed_liquid_kg"].as_f64().unwrap();
            assert!((gas_mass - (1e-10 + evaporated - condensed)).abs() < 1e-20);
            assert!((pressure - gas_mass / 1e-13).abs() < 1e-10);
            let mass_error = w["remaining_liquid_kg"].as_f64().unwrap() + gas_mass
                - (area * 0.01 + 1e-10 + supplied);
            assert!(
                mass_error.abs() < 1e-13,
                "whole-system mass error {mass_error}"
            );
            let vapor_energy = g["vapor_enthalpy_change_j"].as_f64().unwrap();
            let removed = g["isothermal_heat_removed_j"].as_f64().unwrap();
            assert!((vapor_energy - (2500000. + 4200. * 20.) * (gas_mass - 1e-10)).abs() < 1e-12);
            let energy_error = sample["stored_change_j"].as_f64().unwrap()
                + w["liquid_stored_change_j"].as_f64().unwrap()
                + vapor_energy
                + removed
                - supplied * 4200. * -20.
                - sample["net_boundary_energy_j"].as_f64().unwrap();
            assert!(
                energy_error.abs() < 1e-8,
                "whole-system energy error {energy_error}"
            );
        }
        assert!(maximum > initial_pressure && maximum > final_pressure);
        let values = [
            final_pressure,
            minimum,
            maximum,
            gas["remaining_vapor_mass_kg"].as_f64().unwrap(),
            gas["isothermal_heat_removed_j"].as_f64().unwrap(),
        ];
        let row = keyed(&result["comparison"]["alternatives"], "qualified");
        assert_eq!(row["eligible"], true, "{row}");
        for ((metric, expected), unit) in metrics
            .iter()
            .zip(values)
            .zip(["Pa", "Pa", "Pa", "kg", "J"])
        {
            assert_eq!(row["readings"][metric]["value"], expected, "{metric}");
            assert_eq!(row["readings"][metric]["unit"], unit);
            assert!(row["readings"][metric]["reported_bounds"].is_null());
        }
        if selection == 1 {
            assert_eq!(heat["surface_water_selection"]["index"], 1);
            assert_eq!(
                water,
                &direct_simulation["motion"]["result"]["transient_surface"]["result"]["surface_water"]
            );
        } else if selection == 2 {
            assert_eq!(heat["coupled_water_selection"]["index"], 1);
            assert_eq!(motion["liquid_mechanics"]["qualification"], mechanical);
            assert!(
                motion["liquid_mechanics"]["mass_balance_residual_kg"]
                    .as_f64()
                    .unwrap()
                    .abs()
                    < 1e-12
            );
        }
        // Re-admit the complete actual result for exact inclusive limits and both directions
        // of violation. This tests decision policy without a second fabricated physics trace.
        let constraints: Vec<Value> = metrics
            .iter()
            .zip(values)
            .map(|(metric, value)| json!({"metric":metric,"minimum":value,"maximum":value}))
            .collect();
        for violation in 0..3 {
            let mut guarded = policy.clone();
            guarded["constraints"] = json!(constraints);
            if violation == 1 {
                guarded["constraints"][0]["minimum"] = json!(final_pressure + 1.);
                guarded["constraints"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("maximum");
            } else if violation == 2 {
                guarded["constraints"][2]["maximum"] = json!(maximum - 1.);
                guarded["constraints"][2]
                    .as_object_mut()
                    .unwrap()
                    .remove("minimum");
            }
            let mut guarded_input = input.clone();
            guarded_input.comparison = Some(serde_json::from_value(guarded).unwrap());
            let guarded = serde_json::to_value(
                guarded_input
                    .evaluate_with(&pattern, &result["scenario"], |_, _| Ok(simulation.clone()))
                    .unwrap(),
            )
            .unwrap();
            let guarded_row = keyed(&guarded["comparison"]["alternatives"], "qualified");
            assert_eq!(guarded_row["eligible"], violation == 0, "{guarded_row}");
            assert_eq!(guarded_row["readings"], row["readings"]);
        }
        let mut replay = input.clone();
        replay.variants[0].changes = serde_json::from_value(
            keyed(&result["manufacturing"]["alternatives"], "qualified")["frozen_changes"].clone(),
        )
        .unwrap();
        let replayed = study(&case, &replay, &pattern);
        assert_eq!(replayed["simulations"], result["simulations"]);
        assert_eq!(
            replayed["comparison"]["alternatives"],
            result["comparison"]["alternatives"]
        );
        assert_eq!(
            keyed(&result["manufacturing"]["alternatives"], "qualified")["cutting"]["status"],
            "available"
        );
    }
    let mut older = raw.clone();
    let water = &mut older["motion"]["transient_surface"]["surface_water"];
    water["schema"] = json!("ftx-textile-surface-water/3");
    water.as_object_mut().unwrap().remove("vapor_reservoir");
    water["outside_vapor_pressure_pa"] = json!([1000., 1000.]);
    let result = study(&older, &input, &pattern);
    let simulation = &keyed(&result["simulations"], "qualified")["evaluation"]["result"];
    assert_eq!(
        simulation["motion"]["result"]["transient_surface"]["status"],
        "available"
    );
    let row = keyed(&result["comparison"]["alternatives"], "qualified");
    assert_eq!(row["eligible"], false);
    for metric in metrics {
        assert!(row["readings"].get(metric).is_none(), "{metric}");
    }
    assert!(row["readings"]["transient_surface_final_min_temperature_c"]["value"].is_number());
    let mut refused = raw.clone();
    refused["motion"]["transient_surface"]["surface_water"]["vapor_reservoir"]["allow_estimates"] =
        json!(false);
    let result = study(&refused, &input, &pattern);
    let simulation = &keyed(&result["simulations"], "qualified")["evaluation"]["result"];
    assert_eq!(simulation["motion"]["status"], "available");
    assert_eq!(
        simulation["motion"]["result"]["transient_surface"]["status"],
        "refused"
    );
    assert_eq!(simulation["surface"]["status"], "available");
    assert_eq!(simulation["drape"], direct_simulation["drape"]);
    assert_eq!(
        simulation["motion"]["result"]["final_positions_sha256"],
        direct_simulation["motion"]["result"]["final_positions_sha256"]
    );
    let row = keyed(&result["comparison"]["alternatives"], "qualified");
    assert_eq!(row["eligible"], false);
    for metric in metrics {
        assert!(row["readings"].get(metric).is_none());
    }
    assert!(row["readings"]["motion_max_absolute_edge_strain_pct"]["value"].is_number());
    assert_eq!(
        keyed(&result["manufacturing"]["alternatives"], "qualified")["cutting"]["status"],
        "available"
    );
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
