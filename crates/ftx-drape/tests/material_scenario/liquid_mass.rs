//! Actual sewn motion with source-qualified evaporation feedback, not a replayed heat-only path.
use super::*;
use ftx_core::receipt::sha256_hex;
use ftx_pattern::{material_catalog::Selection, materials::Catalog};

fn inputs(loading: f64) -> (Pattern, Scenario) {
    let mut raw = heat_pattern().raw().clone();
    raw["fabric"]["material"]["record"]["properties"]["vapor_resistance"] = json!({
        "value":8.,"unit":"m2*Pa/W","source":"test","evidence":"estimate",
        "method":"Artificial evaporation case, no measured material claim"});
    let pattern = Pattern::new(raw);
    let material: Selection =
        serde_json::from_value(pattern.raw()["fabric"]["material"].clone()).unwrap();
    let locked = Catalog::builtin().unwrap().resolve(&material).unwrap();
    let mut scenario = heat_scenario();
    let form = form();
    let run = drape(
        &pattern,
        &scenario.fit_spec,
        &body(&form),
        &scenario.options,
        |_| {},
    )
    .unwrap();
    let heat = scenario
        .motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap();
    heat.max_cell_steps = 2_000_000;
    heat.surface_water = Some(serde_json::from_value(json!({
        "schema":"ftx-textile-surface-water/1",
        "model":"inner_cell_equilibrium_free_film_frozen_properties/1",
        "bindings":{"band":{"material_sha256":locked.sha256(),
            "initial_liquid_kg_m2":loading}},
        "maximum_liquid_loading_kg_m2":0.1,"liquid_specific_heat_j_kg_k":1000.,
        "latent_heat_j_kg":10.,"enthalpy_reference_c":0.,
        "wet_vapor_pressure_pa":[[-20.,0.],[80.,10000.]],
        "outside_vapor_pressure_pa":[1000.,1000.],
        "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":1.,
        "replenishment_kg_m2_s":0.,
        "source":{"reference":"test:coupled-evaporation","accessed_on":"2026-10-28",
            "note":"Synthetic low latent heat chosen to exercise mass loss in short solver steps"},
        "evidence":"estimate","allow_estimates":true,"method":"Artificial discrete coupling case",
        "qualification":"Synthetic invariant dry thermal and vapor laws over [-20,80] C and [0,0.1] kg/m2"
    })).unwrap());
    heat.mechanical_coupling = Some(serde_json::from_value(json!({
        "schema":"ftx-evaporating-liquid-mechanics/1",
        "model":"co_moving_evaporation_frozen_mechanics/1",
        "material_report_sha256":sha256_hex(&serde_json::to_vec(&run.report["material"]).unwrap()),
        "bindings":{"band":{"material_sha256":locked.sha256(),
            "maximum_liquid_loading_kg_m2":0.1,"temperature_range_c":[-20.,80.]}},
        "source":{"reference":"test:coupled-wet-mechanics","accessed_on":"2026-10-28",
            "note":"Synthetic invariant membrane, bending, friction and co-moving mass-loss closure"},
        "evidence":"estimate","allow_estimates":true,"method":"Artificial coupled software case",
        "qualification":"Exact synthetic material and contact laws held invariant across the full thermal loading and temperature domain"
    })).unwrap());
    (pattern, scenario)
}

#[test]
fn sewn_evaporation_changes_motion_and_reconciles_both_mass_authorities() {
    let (pattern, scenario) = inputs(0.1);
    let result = moving(&scenario, &pattern);
    assert_eq!(
        result["motion"]["status"], "available",
        "{}",
        result["motion"]
    );
    let motion = &result["motion"]["result"];
    let heat = &motion["transient_surface"]["result"];
    let mechanics = &motion["liquid_mechanics"];
    let water = &heat["surface_water"];
    assert_eq!(mechanics, &heat["mechanical_coupling"]);
    for (mechanical, thermal) in [
        ("initial_liquid_mass_kg", "initial_liquid_kg"),
        ("remaining_liquid_mass_kg", "remaining_liquid_kg"),
        ("evaporated_liquid_mass_kg", "evaporated_liquid_kg"),
    ] {
        let a = mechanics[mechanical].as_f64().unwrap();
        let b = water[thermal].as_f64().unwrap();
        assert!((a - b).abs() < 1e-15, "{mechanical}: {a} != {b}");
    }
    assert!(mechanics["evaporated_liquid_mass_kg"].as_f64().unwrap() > 0.);
    assert!(mechanics["exported_kinetic_energy_j"].as_f64().unwrap() > 0.);
    assert!(
        mechanics["mass_balance_residual_kg"]
            .as_f64()
            .unwrap()
            .abs()
            < 1e-15
    );
    assert!(heat["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
    let trace = heat["trace"].as_array().unwrap();
    assert_eq!(trace.len(), motion["samples"].as_array().unwrap().len());
    for sample in trace {
        assert_eq!(
            sample["mechanical_coupling"]["remaining_liquid_kg"],
            sample["surface_water"]["remaining_liquid_kg"]
        );
        let m = &sample["mechanical_coupling"];
        let expected = m["substrate_and_attached_mass_kg"].as_f64().unwrap()
            + m["remaining_liquid_kg"].as_f64().unwrap();
        assert!((m["total_inertial_mass_kg"].as_f64().unwrap() - expected).abs() < 1e-15);
    }
    let mut uncoupled = scenario.clone();
    uncoupled
        .motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .mechanical_coupling = None;
    let independent = moving(&uncoupled, &pattern);
    assert_eq!(result["drape"], independent["drape"]);
    assert_eq!(
        motion["initial_sample"]["positions_m"],
        independent["motion"]["result"]["initial_sample"]["positions_m"]
    );
    assert_ne!(
        motion["final_positions_sha256"],
        independent["motion"]["result"]["final_positions_sha256"]
    );
    assert_eq!(moving(&scenario, &pattern), result);
}

#[test]
fn zero_liquid_preserves_dry_motion_and_failed_coupling_cannot_publish_a_dry_fallback() {
    let (pattern, mut scenario) = inputs(0.);
    let coupled = moving(&scenario, &pattern);
    assert_eq!(
        coupled["motion"]["status"], "available",
        "{}",
        coupled["motion"]
    );
    let mut dry = scenario.clone();
    dry.motion.as_mut().unwrap().transient_surface = None;
    let baseline = moving(&dry, &pattern);
    assert_eq!(
        coupled["motion"]["result"]["final_positions_sha256"],
        baseline["motion"]["result"]["final_positions_sha256"]
    );
    assert_eq!(
        coupled["motion"]["result"]["final_velocities_m_s"],
        baseline["motion"]["result"]["final_velocities_m_s"]
    );
    scenario
        .motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .max_cell_steps = 1;
    let refused = moving(&scenario, &pattern);
    assert_eq!(refused["motion"]["status"], "refused");
    assert!(refused["motion"]["result"].is_null());
    assert_eq!(refused["drape"], baseline["drape"]);
}

#[test]
fn wet_mechanics_requires_exact_sources_and_the_complete_thermal_domain() {
    let (pattern, scenario) = inputs(0.1);
    for mutation in 0..5 {
        let mut changed = scenario.clone();
        let heat = changed
            .motion
            .as_mut()
            .unwrap()
            .transient_surface
            .as_mut()
            .unwrap();
        let mechanics = heat.mechanical_coupling.as_mut().unwrap();
        match mutation {
            0 => mechanics.material_report_sha256 = "0".repeat(64),
            1 => mechanics.bindings.get_mut("band").unwrap().material_sha256 = "0".repeat(64),
            2 => {
                mechanics
                    .bindings
                    .get_mut("band")
                    .unwrap()
                    .maximum_liquid_loading_kg_m2 = 0.09
            }
            3 => {
                mechanics
                    .bindings
                    .get_mut("band")
                    .unwrap()
                    .temperature_range_c = [10., 30.]
            }
            _ => mechanics.allow_estimates = false,
        }
        let result = moving(&changed, &pattern);
        assert_eq!(
            result["motion"]["status"], "refused",
            "mutation {mutation}: {}",
            result["motion"]
        );
        assert!(result["motion"]["result"].is_null());
        assert!(result["drape"].is_object());
    }
}
