use super::*;
use crate::{
    study::heat::{SurfaceHistory, SurfaceRequest},
    thermal::{DryThermalModel, surface::Patch},
};
use ftx_pattern::material_catalog::Selection;

fn fixture(loading: f64, step: f64) -> SurfaceHistory {
    let selected: Selection = serde_json::from_value(json!({"id":"surface-liquid-test","allow_estimates":true,
        "record":{"id":"surface-liquid-test","name":"Synthetic analytic film", "form":"woven",
            "sources":{"test":{"reference":"test:surface-water","accessed_on":"2026-10-23",
                "note":"Artificial conservation case, no measured textile or fluid values"}},
            "properties":{
                "areal_density":{"value":0.2,"unit":"kg/m2","evidence":"estimate","source":"test","method":"synthetic"},
                "specific_heat":{"value":1000.,"unit":"J/(kg*K)","evidence":"estimate","source":"test","method":"synthetic"},
                "thermal_resistance":{"value":0.4,"unit":"m2*K/W","evidence":"estimate","source":"test","method":"synthetic"},
                "vapor_resistance":{"value":8.,"unit":"m2*Pa/W","evidence":"estimate","source":"test","method":"synthetic"}
            }}})).unwrap();
    let record = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&selected)
    .unwrap();
    let model = Surface::new(
        vec![DryThermalModel::new(vec![record.clone()], vec![], 1).unwrap()],
        3,
        vec![Patch {
            vertices: [0, 1, 2],
            reference_area_m2: 0.5,
            column: 0,
        }],
    )
    .unwrap();
    let water: Request = serde_json::from_value(json!({"schema":"ftx-textile-surface-water/1",
        "model":"inner_cell_equilibrium_free_film_frozen_properties/1",
        "bindings":{"sheet":{"material_sha256":record.sha256(),"initial_liquid_kg_m2":loading}},
        "maximum_liquid_loading_kg_m2":0.1,"liquid_specific_heat_j_kg_k":1000.,
        "latent_heat_j_kg":10000.,"enthalpy_reference_c":0.,
        "wet_vapor_pressure_pa":[[-20.,0.],[80.,10000.]],"outside_vapor_pressure_pa":[1000.],
        "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":1.,
        "replenishment_kg_m2_s":0.,"source":{"reference":"test:synthetic-free-liquid",
            "accessed_on":"2026-10-23","note":"Artificial fluid and applicability values only"},
        "evidence":"estimate","allow_estimates":true,"method":"Synthetic explicit rate and sensible outflow",
        "qualification":"Synthetic fixed material properties over [-20,80] degrees and [0,0.1] kg per reference square metre"})).unwrap();
    let policy: SurfaceRequest =
        serde_json::from_value(json!({"schema":"ftx-motion-surface-heat/1",
        "initial_temperature_c":20.,"cells_per_layer":1,"max_step_s":step,"max_cell_steps":100000,
        "surface_water":water,"boundaries":[{"inside_c":20.,"outside_c":20.,
            "inside_resistance_m2_k_w":1.,"outside_resistance_m2_k_w":1.}]}))
        .unwrap();
    let bound =
        Water::from_materials(water, &model, &[record], &["sheet".into()], &[0], 20.).unwrap();
    let mut history = SurfaceHistory::new(policy, model).unwrap();
    history.water = Some(bound);
    history
}

fn geometry(area: f64) -> [Vec3; 3] {
    [
        Vec3::ZERO,
        Vec3::new(2. * area, 0., 0.),
        Vec3::new(0., 1., 0.),
    ]
}
fn advance(history: &mut SurfaceHistory, area: f64, seconds: f64) -> Result<(), Refusal> {
    history.advance(&geometry(area), seconds, 0, 0, None, None, None)
}
fn near(a: f64, b: f64) {
    assert!(
        (a - b).abs() <= 1e-10 * (1. + a.abs() + b.abs()),
        "{a} != {b}"
    );
}

#[test]
fn explicit_temperature_evaporation_balances_dry_and_liquid_sensible_plus_vapor_export() {
    let mut history = fixture(0.1, 1.);
    // 0.5 m² reference/current area: Cd=100 J/K, liquid starts at 0.05 kg.
    // Pressure(20)=4000 Pa, ambient=1000, Ret=10, L=10000 => 0.015 kg/s.
    let remaining = 0.05 - 0.015;
    let capacity = 100. + remaining * 1000.;
    let conductance = 0.5 / 1.2;
    let expected = (capacity * 20. + 2. * conductance * 20. - 150.) / (capacity + 2. * conductance);
    advance(&mut history, 0.5, 1.).unwrap();
    near(history.state.temperature_c()[0][0], expected);
    let result = history.finish("path", "initial", 1.).unwrap();
    let water = &result["surface_water"];
    near(water["remaining_liquid_kg"].as_f64().unwrap(), remaining);
    near(water["evaporated_liquid_kg"].as_f64().unwrap(), 0.015);
    near(water["latent_energy_j"].as_f64().unwrap(), 150.);
    // Outflow is at OLD 20 °C, never silently at the backward-Euler endpoint.
    near(
        water["exported_vapor_sensible_energy_j"].as_f64().unwrap(),
        300.,
    );
    near(
        water["liquid_stored_change_j"].as_f64().unwrap(),
        1000. * (remaining * expected - 0.05 * 20.),
    );
    near(
        result["stored_change_j"].as_f64().unwrap(),
        100. * (expected - 20.),
    );
    near(result["balance_error_j"].as_f64().unwrap(), 0.);
    near(water["mass_balance_error_kg"].as_f64().unwrap(), 0.);
    assert_eq!(
        water["unit_pressure_networks"][0]["layers"][0]["observation"]["value"],
        8.
    );
}

#[test]
fn dryout_splits_exact_time_then_uses_the_unchanged_dry_heat_authority() {
    let mut history = fixture(0.015, 1.);
    // 0.0075 kg / 0.015 kg/s = exactly 0.5 s, leaving 0.5 s dry.
    let conductance = 0.5 / 1.2;
    let dryout_temperature =
        (100. * 20. + 0.5 * 2. * conductance * 20. - 75.) / (100. + 0.5 * 2. * conductance);
    let dry_initial = history.model.initial(dryout_temperature).unwrap();
    let dry_next = history
        .model
        .advance(
            &dry_initial,
            &geometry(0.5),
            0.5,
            &[super::super::boundary(history.request.boundaries[0]).unwrap()],
        )
        .unwrap();
    advance(&mut history, 0.5, 1.).unwrap();
    near(
        history.state.temperature_c()[0][0],
        dry_next.state.temperature_c()[0][0],
    );
    assert_eq!(history.thermal_steps, 2);
    let result = history.finish("path", "initial", 1.).unwrap();
    assert_eq!(result["surface_water"]["remaining_liquid_kg"], 0.);
    assert_eq!(result["surface_water"]["dryout_events"], 1);
    near(
        result["surface_water"]["latent_energy_j"].as_f64().unwrap(),
        75.,
    );
    near(result["balance_error_j"].as_f64().unwrap(), 0.);
}

#[test]
fn reference_water_mass_is_fixed_while_current_area_controls_evaporation_and_refinement() {
    let mut small = fixture(0.1, 0.1);
    let mut stretched = fixture(0.1, 0.1);
    advance(&mut small, 0.5, 0.1).unwrap();
    advance(&mut stretched, 1., 0.1).unwrap();
    let a = small.water.as_ref().unwrap();
    let b = stretched.water.as_ref().unwrap();
    assert_eq!(a.initial, b.initial);
    near(b.evaporated[0], 2. * a.evaporated[0]);
    assert!(stretched.state.temperature_c()[0][0] < small.state.temperature_c()[0][0]);
    let temperature = |step| {
        let mut history = fixture(0.1, step);
        advance(&mut history, 0.5, 1.).unwrap();
        assert!(history.balance_error_j().abs() < 1e-8);
        history.state.temperature_c()[0][0]
    };
    let reference = temperature(1. / 512.);
    let coarse_error = (temperature(0.25) - reference).abs();
    let fine_error = (temperature(0.125) - reference).abs();
    assert!(
        fine_error < 0.6 * coarse_error,
        "{fine_error} !< {coarse_error}"
    );
    let mut feedback = fixture(0.1, 0.5);
    advance(&mut feedback, 0.5, 0.5).unwrap();
    let first = feedback.water.as_ref().unwrap().evaporated[0];
    advance(&mut feedback, 0.5, 1.).unwrap();
    let second = feedback.water.as_ref().unwrap().evaporated[0] - first;
    assert!(
        second > 0. && second < first,
        "cooling must reduce the next sampled vapor demand"
    );
}

#[test]
fn a_sensible_enthalpy_reference_change_preserves_temperatures_and_full_energy() {
    let mut reference = fixture(0.1, 0.25);
    let mut shifted = reference.clone();
    shifted.water.as_mut().unwrap().request.enthalpy_reference_c = 15.;
    advance(&mut reference, 0.5, 1.).unwrap();
    advance(&mut shifted, 0.5, 1.).unwrap();
    assert_eq!(
        reference.state.temperature_c(),
        shifted.state.temperature_c()
    );
    let a = reference.water.as_ref().unwrap();
    let b = shifted.water.as_ref().unwrap();
    assert_ne!(a.exported_sensible_j, b.exported_sensible_j);
    near(a.energy_correction_j(), b.energy_correction_j());
    near(reference.balance_error_j(), 0.);
    near(shifted.balance_error_j(), 0.);
}

#[test]
fn invalid_source_condensation_and_late_domain_or_event_budget_failure_are_transactional() {
    let mut history = fixture(0.015, 1.);
    let mut request = history.request.surface_water.clone().unwrap();
    request.allow_estimates = false;
    assert!(request.validate(1).is_err());
    request.allow_estimates = true;
    request.replenishment_kg_m2_s = 1e-9;
    assert!(request.validate(1).is_err());
    request.replenishment_kg_m2_s = 0.;
    request.wet_vapor_pressure_pa[1][0] = request.wet_vapor_pressure_pa[0][0];
    assert!(request.validate(1).is_err());
    // Initial minimum work fits, but dryout creates a second necessary step. Roll back its first
    // successful wet solve as well as time/counters/trace when that additional step is refused.
    history.request.max_cell_steps = 1;
    let before = history
        .water
        .as_ref()
        .unwrap()
        .snapshot(history.state.temperature_c(), true, 0.)
        .unwrap();
    assert!(advance(&mut history, 0.5, 1.).is_err());
    assert_eq!(history.state.temperature_c(), &[vec![20.]]);
    assert_eq!(history.cell_steps, 0);
    assert_eq!(history.time_s, 0.);
    assert!(history.trace.is_empty());
    assert_eq!(
        history
            .water
            .as_ref()
            .unwrap()
            .snapshot(history.state.temperature_c(), true, 0.)
            .unwrap(),
        before
    );
    let mut history = fixture(0.1, 1.);
    history
        .water
        .as_mut()
        .unwrap()
        .request
        .wet_vapor_pressure_pa = vec![[19.9, 3990.], [20.1, 4010.]];
    assert!(advance(&mut history, 0.5, 1.).is_err());
    assert_eq!(history.state.temperature_c(), &[vec![20.]]);
    assert_eq!(history.water.as_ref().unwrap().remaining, vec![0.05]);
    history
        .water
        .as_mut()
        .unwrap()
        .request
        .outside_vapor_pressure_pa[0] = 5000.;
    assert!(advance(&mut history, 0.5, 1.).is_err());
}

#[test]
fn dry_opt_in_and_zero_pressure_drop_do_not_create_evaporation() {
    let mut history = fixture(0., 1.);
    let expected = history
        .model
        .advance(
            &history.state,
            &geometry(1.),
            1.,
            &[super::super::boundary(history.request.boundaries[0]).unwrap()],
        )
        .unwrap();
    advance(&mut history, 1., 1.).unwrap();
    assert_eq!(
        history.state.temperature_c(),
        expected.state.temperature_c()
    );
    assert_eq!(history.water.as_ref().unwrap().evaporated, vec![0.]);
    let mut history = fixture(0.1, 1.);
    history
        .water
        .as_mut()
        .unwrap()
        .request
        .outside_vapor_pressure_pa[0] = 4000.;
    advance(&mut history, 0.5, 1.).unwrap();
    near(history.state.temperature_c()[0][0], 20.);
    assert_eq!(history.water.as_ref().unwrap().remaining, vec![0.05]);
    assert_eq!(history.water.as_ref().unwrap().latent_j, 0.);
}

#[test]
fn nonbinary_regular_steps_have_no_phantom_event_or_extra_budget_charge() {
    for loading in [0., 0.1] {
        let mut history = fixture(loading, 0.1);
        history.request.max_cell_steps = 10;
        advance(&mut history, 0.5, 1.).unwrap();
        assert_eq!(history.cell_steps, 10);
        assert_eq!(history.thermal_steps, 10);
        assert_eq!(history.time_s, 1.);
        near(history.balance_error_j(), 0.);
    }
}
