use super::*;
use ftx_pattern::{material_catalog::Selection, materials::Catalog};

fn inputs() -> (Pattern, Scenario) {
    let mut material = maxwell(0.75);
    material["record"]["properties"]["vapor_resistance"] = json!({"value":8.,"unit":"m2*Pa/W",
        "evidence":"estimate","source":"test","method":"Synthetic free-liquid constant transport"});
    let selected: Selection = serde_json::from_value(material.clone()).unwrap();
    let resolved = Catalog::builtin().unwrap().resolve(&selected).unwrap();
    let mut raw = garment(material.clone()).raw().clone();
    raw["fabric"]["material"] = material;
    let mut s = serde_json::to_value(scenario()).unwrap();
    s["motion"]["transient_surface"]["relaxation_dissipation_heat"] = Value::Null;
    s["motion"]["transient_surface"]["surface_water"] = json!({
        "schema":"ftx-textile-surface-water/1","model":"inner_cell_equilibrium_free_film_frozen_properties/1",
        "bindings":{"front":{"material_sha256":resolved.sha256(),"initial_liquid_kg_m2":5e-8},
            "back":{"material_sha256":resolved.sha256(),"initial_liquid_kg_m2":0.}},
        "maximum_liquid_loading_kg_m2":1e-5,"liquid_specific_heat_j_kg_k":4200.,
        "latent_heat_j_kg":2500000.,"enthalpy_reference_c":0.,
        "wet_vapor_pressure_pa":[[-20.,0.],[80.,10000.]],"outside_vapor_pressure_pa":[1000.],
        "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":1.,
        "replenishment_kg_m2_s":0.,"source":{"reference":"test:moving-textile-liquid",
            "accessed_on":"2026-10-23","note":"Synthetic water and constant-property applicability only"},
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic initial-film cooling experiment",
        "qualification":"Synthetic fixed dry thermal and Ret properties throughout [-20,80] C and [0,1e-5] kg per reference square metre"});
    (Pattern::new(raw), serde_json::from_value(s).unwrap())
}

#[test]
fn moving_textile_surface_water_cools_material_and_keeps_reference_mass_and_mechanics() {
    let (pattern, scenario) = inputs();
    let result = evaluate(&scenario, &pattern);
    let h = heat(&result);
    let water = &h["surface_water"];
    assert!(water["initial_liquid_kg"].as_f64().unwrap() > 0.);
    assert_eq!(water["remaining_liquid_kg"], 0.);
    assert!(water["dryout_events"].as_u64().unwrap() > 0);
    let latent = water["latent_energy_j"].as_f64().unwrap();
    assert!((latent - water["evaporated_liquid_kg"].as_f64().unwrap() * 2500000.).abs() < 1e-12);
    let dry_storage = h["stored_change_j"].as_f64().unwrap();
    assert!(dry_storage < 0.);
    let balance = dry_storage
        + water["liquid_stored_change_j"].as_f64().unwrap()
        + water["exported_vapor_sensible_energy_j"].as_f64().unwrap()
        + latent
        - h["net_boundary_energy_j"].as_f64().unwrap();
    assert!(balance.abs() < 1e-10);
    assert!(water["mass_balance_error_kg"].as_f64().unwrap().abs() < 1e-20);
    let mut expected_initial = 0.;
    for (i, p) in water["patches"].as_array().unwrap().iter().enumerate() {
        let area = h["model"]["patches"][i]["reference_area_m2"]
            .as_f64()
            .unwrap();
        if p["instance"] == "front" {
            expected_initial += area * 5e-8;
            assert!(h["final"]["temperature_c"][i][0].as_f64().unwrap() < 20.);
        } else {
            assert_eq!(p["initial_liquid_kg"], 0.);
            assert_eq!(p["evaporated_liquid_kg"], 0.);
        }
    }
    assert!((water["initial_liquid_kg"].as_f64().unwrap() - expected_initial).abs() < 1e-20);
    let mut dry = scenario.clone();
    dry.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .surface_water = None;
    let dry_result = evaluate(&dry, &pattern);
    unchanged_mechanics(&result, &dry_result);
    assert!(heat(&dry_result)["stored_change_j"].as_f64().unwrap().abs() < 1e-10);
    assert_eq!(
        heat(&evaluate(&scenario, &pattern)),
        h,
        "exact frozen input replay"
    );
}

#[test]
fn textile_water_and_maxwell_heat_have_separate_conservative_ledgers_and_local_refusals() {
    let (pattern, mut scenario) = inputs();
    let policy = scenario
        .motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap();
    policy.relaxation_dissipation_heat =
        Some(ftx_drape::study::heat::DissipationHeatDistribution::UniformReferenceThickness);
    let result = evaluate(&scenario, &pattern);
    let h = heat(&result);
    let supplied = h["relaxation_dissipation_heat"]["supplied_heat_j"]
        .as_f64()
        .unwrap();
    assert!(supplied > 0.);
    let water = &h["surface_water"];
    let residual = h["stored_change_j"].as_f64().unwrap()
        + water["liquid_stored_change_j"].as_f64().unwrap()
        + water["exported_vapor_sensible_energy_j"].as_f64().unwrap()
        + water["latent_energy_j"].as_f64().unwrap()
        - h["net_boundary_energy_j"].as_f64().unwrap()
        - supplied;
    assert!(
        residual.abs() < 1e-10,
        "source-inclusive balance {residual}"
    );
    let mut bad = scenario.clone();
    bad.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .surface_water
        .as_mut()
        .unwrap()
        .bindings
        .get_mut("front")
        .unwrap()
        .material_sha256 = "0".repeat(64);
    let refused = evaluate(&bad, &pattern);
    assert_eq!(
        refused["motion"]["result"]["transient_surface"]["status"],
        "refused"
    );
    unchanged_mechanics(&result, &refused);
    let mut raw = pattern.raw().clone();
    raw["fabric"]["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("vapor_resistance");
    let missing = evaluate(&scenario, &Pattern::new(raw));
    assert_eq!(
        missing["motion"]["result"]["transient_surface"]["status"],
        "refused"
    );
    assert_eq!(missing["motion"]["status"], "available");
}
