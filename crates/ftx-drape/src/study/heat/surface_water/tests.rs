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

fn variable_fixture(
    history: SurfaceHistory,
    cells: usize,
    conductivity: [f64; 2],
    domain: [f64; 2],
) -> SurfaceHistory {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    let mut selected = fixture["layers"][0].clone();
    selected["record"]["properties"]["vapor_resistance"] = json!({
        "value":8.,"unit":"m2*Pa/W","evidence":"estimate","source":"test",
        "method":"Synthetic fixed Ret for independent enthalpy checks"});
    selected["record"]["thermal_response"]["conductivity"]["points"] = json!([
        {"temperature_c":domain[0],"mean":conductivity[0]},
        {"temperature_c":domain[1],"mean":conductivity[1]}]);
    selected["record"]["thermal_response"]["specific_heat"]["points"] = json!([
        {"temperature_c":domain[0],"mean":1000. + 20. * domain[0]},
        {"temperature_c":domain[1],"mean":1000. + 20. * domain[1]}]);
    let record = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&serde_json::from_value(selected).unwrap())
    .unwrap();
    let model = Surface::new(
        vec![DryThermalModel::new(vec![record.clone()], vec![], cells).unwrap()],
        3,
        vec![Patch {
            vertices: [0, 1, 2],
            reference_area_m2: 0.5,
            column: 0,
        }],
    )
    .unwrap();
    let mut policy = history.request;
    policy.cells_per_layer = cells;
    let water = policy.surface_water.as_mut().unwrap();
    water.bindings.get_mut("sheet").unwrap().material_sha256 = record.sha256().into();
    water.qualification = "Synthetic dry conductivity/Cp profiles and fixed Ret retained throughout the declared loading domain; no measured wet-property evidence".into();
    let bound = Water::from_materials(
        water.clone(),
        &model,
        &[record],
        &["sheet".into()],
        &[0],
        policy.initial_temperature_c,
    )
    .unwrap();
    let mut history = SurfaceHistory::new(policy, model).unwrap();
    history.water = Some(bound);
    history
}

#[test]
fn variable_dry_enthalpy_and_evaporating_liquid_match_an_independent_quadratic() {
    for area in [0.5, 1.0] {
        let mut history = variable_fixture(fixture(0.1, 1.), 1, [0.0025; 2], [0., 60.]);
        let before = history.model.snapshot();
        // Cp=1000+20*T, reference mass=.1 kg. With d=T-20 the dry change is 140*d+d².
        // At old T=20, p=4000 Pa and R=10 gives evaporation .03*current_area kg/s.
        let evaporated = 0.03 * area;
        let remaining = 0.05 - evaporated;
        let latent = 10000. * evaporated;
        let linear: f64 = 140. + remaining * 1000. + area * 2. / 1.2;
        let delta = -2. * latent / (linear + (linear * linear - 4. * latent).sqrt());
        advance(&mut history, area, 1.).unwrap();
        let temperature = history.state.temperature_c()[0][0];
        near(temperature, 20. + delta);
        near(history.stored_j, 140. * delta + delta * delta);
        near(history.water.as_ref().unwrap().evaporated[0], evaporated);
        near(history.water.as_ref().unwrap().remaining[0], remaining);
        variable_energy_balance(&history);
        assert_eq!(history.model.snapshot(), before);
        // Endpoint Cp*delta would double-count the quadratic term and does not pass this answer.
        assert!((history.stored_j - 0.1 * (1000. + 20. * temperature) * delta).abs() > 0.1);
    }
}

#[test]
fn variable_enthalpy_condensation_and_hot_supply_share_the_same_conserved_state() {
    let mut base = condensing_fixture(0., 1., vec![6000.], vec![40.]);
    let supply = base
        .request
        .surface_water
        .as_mut()
        .unwrap()
        .supply
        .as_mut()
        .unwrap();
    supply.bindings.get_mut("sheet").unwrap().rate_kg_m2_s[0] = 0.01;
    supply.bindings.get_mut("sheet").unwrap().temperature_c[0] = 30.;
    let mut history = variable_fixture(base, 1, [0.0025; 2], [0., 60.]);
    // .01 kg condenses and .005 kg of liquid is delivered. Latent+mixing is 100+200+50 J.
    // Remaining liquid has 15 J/K; the exact dry enthalpy change stays 140*d+d².
    let linear: f64 = 140. + 15. + 1. / 1.2;
    let delta = 700. / (linear + (linear * linear + 1400.).sqrt());
    advance(&mut history, 0.5, 1.).unwrap();
    near(history.state.temperature_c()[0][0], 20. + delta);
    near(history.stored_j, 140. * delta + delta * delta);
    let water = history.water.as_ref().unwrap();
    near(water.condensed[0], 0.01);
    near(water.supplied[0], 0.005);
    near(water.remaining[0], 0.015);
    near(water.condensation_latent_j, 100.);
    near(water.incoming_vapor_sensible_j[0], 400.);
    near(water.supplied_sensible_j[0], 150.);
    variable_energy_balance(&history);
}

#[test]
fn variable_liquid_dryout_reuses_the_source_law_and_charges_bounded_inversion_work() {
    let mut history = variable_fixture(fixture(0.015, 1.), 1, [0.0025; 2], [0., 60.]);
    // .0075 kg dries exactly at .5 s. Solve that quadratic independently, then ask the ordinary
    // dry authority for the remaining .5 s; no liquid capacity survives the event.
    let linear: f64 = 140. + 0.5 / 1.2;
    let delta = -150. / (linear + (linear * linear - 300.).sqrt());
    let dry_state = history.model.initial(20. + delta).unwrap();
    let expected = history
        .model
        .advance(
            &dry_state,
            &geometry(0.5),
            0.5,
            &[super::super::boundary(history.request.boundaries[0]).unwrap()],
        )
        .unwrap();
    history.request.max_cell_steps = 258;
    advance(&mut history, 0.5, 1.).unwrap();
    near(
        history.state.temperature_c()[0][0],
        expected.state.temperature_c()[0][0],
    );
    assert_eq!(history.water.as_ref().unwrap().remaining, vec![0.]);
    assert_eq!(history.water.as_ref().unwrap().dryout_events, 1);
    assert_eq!(history.cell_steps, 258);
    assert_eq!(history.thermal_steps, 2);
    variable_energy_balance(&history);

    let mut refused = variable_fixture(fixture(0.015, 1.), 1, [0.0025; 2], [0., 60.]);
    refused.request.max_cell_steps = 257;
    assert!(advance(&mut refused, 0.5, 1.).is_err());
    assert_eq!(refused.state.temperature_c(), &[vec![20.]]);
    assert_eq!(refused.water.as_ref().unwrap().remaining, vec![0.0075]);
    assert_eq!(refused.cell_steps, 0);
    assert_eq!(refused.time_s, 0.);
}

#[test]
fn variable_liquid_source_domains_refuse_cooling_and_heating_transactionally() {
    for (base, domain) in [
        (fixture(0.1, 0.1), [19.5, 60.]),
        (
            condensing_fixture(0., 0.1, vec![6000.], vec![40.]),
            [0., 20.5],
        ),
    ] {
        let mut history = variable_fixture(base, 1, [0.0025; 2], domain);
        let before = history
            .water
            .as_ref()
            .unwrap()
            .snapshot(history.state.temperature_c(), true, 0.)
            .unwrap();
        // Several source-admitted subdivisions precede the boundary crossing. The complete
        // accepted mechanical interval must retain its original temperature and water inventory.
        assert!(advance(&mut history, 0.5, 1.).is_err());
        assert_eq!(history.state.temperature_c(), &[vec![20.]]);
        assert_eq!(history.thermal_steps, 0);
        assert_eq!(history.cell_steps, 0);
        assert_eq!(history.time_s, 0.);
        assert_eq!(
            history
                .water
                .as_ref()
                .unwrap()
                .snapshot(history.state.temperature_c(), true, 0.)
                .unwrap(),
            before
        );
    }
}

#[test]
fn temperature_dependent_transport_and_storage_remain_active_across_wet_cells() {
    let run = |conductivity, step| {
        let mut history = variable_fixture(fixture(0.1, step), 3, conductivity, [0., 60.]);
        history.request.max_cell_steps = 1_000_000;
        advance(&mut history, 0.5, 1.).unwrap();
        variable_energy_balance(&history);
        let dry = history.state.temperature_c()[0]
            .iter()
            .map(|&t| (140. * (t - 20.) + (t - 20.).powi(2)) / 3.)
            .sum::<f64>();
        near(history.stored_j, dry);
        history
    };
    let constant = run([0.0025; 2], 0.125);
    let variable = run([0.00125, 0.005], 0.125);
    assert!(
        (constant.state.temperature_c()[0][0] - variable.state.temperature_c()[0][0]).abs() > 1e-5
    );
    let reference = run([0.00125, 0.005], 1. / 256.).state.temperature_c()[0][0];
    let coarse = run([0.00125, 0.005], 0.25).state.temperature_c()[0][0];
    let fine = variable.state.temperature_c()[0][0];
    assert!((fine - reference).abs() < 0.7 * (coarse - reference).abs());
}

fn variable_energy_balance(history: &SurfaceHistory) {
    let water = history.water.as_ref().unwrap();
    // The enthalpy inverse retains its numerical residual. The coupled solver's declared bound
    // is 1e-8 times the step's absolute energy terms, accumulated over the accepted substeps.
    // These monotone synthetic source paths have no hidden cancellation in phase/supply totals.
    // Temperature, independent integrated storage and mass keep their separate tight checks.
    let scale = 1.0
        + [
            history.stored_j,
            history.inside_j,
            history.outside_j,
            water.liquid_stored_j,
            water.exported_sensible_j,
            water.latent_j,
            water.condensation_latent_j,
        ]
        .iter()
        .chain(&water.supplied_sensible_j)
        .chain(&water.incoming_vapor_sensible_j)
        .map(|value| value.abs())
        .sum::<f64>();
    let tolerance = 1e-8 * history.thermal_steps as f64 * scale;
    assert!(
        history.balance_error_j().abs() <= tolerance,
        "variable enthalpy residual {} exceeds energy/work bound {tolerance}",
        history.balance_error_j()
    );
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

fn supplied_fixture(
    loading: f64,
    step: f64,
    rates: Vec<f64>,
    temperatures: Vec<f64>,
) -> SurfaceHistory {
    let mut history = fixture(loading, step);
    let intervals = rates.len();
    let mut request = history.water.as_ref().unwrap().request.clone();
    request.schema = "ftx-textile-surface-water/2".into();
    request.replenishment_kg_m2_s = None;
    request.outside_vapor_pressure_pa = vec![1000.; intervals];
    request.supply = Some(serde_json::from_value(json!({
        "schema":"ftx-textile-liquid-supply/1", "bindings":{"sheet":{
            "rate_kg_m2_s":rates,"temperature_c":temperatures}},
        "source":{"reference":"test:prescribed-textile-delivery","accessed_on":"2026-10-23",
            "note":"Synthetic reference-area delivery of the declared liquid, no body transfer"},
        "evidence":"estimate","allow_estimates":true,"method":"Artificial piecewise constant feed"
    })).unwrap());
    request.validate(intervals).unwrap();
    history.water.as_mut().unwrap().request = request.clone();
    history.request.surface_water = Some(request);
    history.request.boundaries = vec![history.request.boundaries[0]; intervals];
    history
}

fn condensing_fixture(
    loading: f64,
    step: f64,
    ambient: Vec<f64>,
    vapor: Vec<f64>,
) -> SurfaceHistory {
    let intervals = ambient.len();
    let mut history = supplied_fixture(loading, step, vec![0.; intervals], vec![20.; intervals]);
    let mut request = history.water.as_ref().unwrap().request.clone();
    request.schema = "ftx-textile-surface-water/3".into();
    request.outside_vapor_pressure_pa = ambient;
    request.condensation = Some(serde_json::from_value(json!({
        "schema":"ftx-textile-condensation/1", "vapor_temperature_c":vapor,
        "source":{"reference":"test:reversible-free-film","accessed_on":"2026-10-27",
            "note":"Synthetic reverse Ret, wet/dry nucleation and effective enthalpy closure"},
        "evidence":"estimate", "allow_estimates":true,
        "method":"Prescribed vapor reservoir; artificial fixed sensible coefficient and latent heat",
        "qualification":"Synthetic exact Ret in both directions; wet/dry phase transfer and cp_liquid sensible plus constant latent convention over the full declared domains"
    })).unwrap());
    request.validate(intervals).unwrap();
    history.water.as_mut().unwrap().request = request.clone();
    history.request.surface_water = Some(request);
    history
}

#[test]
fn condensation_warms_a_dry_textile_and_conserves_incoming_vapor_enthalpy() {
    let mut history = condensing_fixture(0., 1., vec![6000.], vec![40.]);
    // T_old=20 gives p_wet=4000 Pa. R=10, area=.5 and L=10000 => .01 kg enters.
    // The new film adds 10 J/K; latent release is 100 J and vapor mixing is 200 J.
    let capacity = 100. + 0.01 * 1000.;
    let conductance = 0.5 / 1.2;
    let expected = (capacity * 20. + 2. * conductance * 20. + 300.) / (capacity + 2. * conductance);
    advance(&mut history, 0.5, 1.).unwrap();
    near(history.state.temperature_c()[0][0], expected);
    let report = history.finish("path", "initial", 1.).unwrap();
    let water = &report["surface_water"];
    assert_eq!(water["schema"], "ftx-textile-surface-water-result/3");
    near(water["initial_liquid_kg"].as_f64().unwrap(), 0.);
    near(water["condensed_liquid_kg"].as_f64().unwrap(), 0.01);
    near(water["remaining_liquid_kg"].as_f64().unwrap(), 0.01);
    near(water["evaporated_liquid_kg"].as_f64().unwrap(), 0.);
    near(
        water["condensation_latent_energy_j"].as_f64().unwrap(),
        100.,
    );
    near(
        water["incoming_vapor_sensible_energy_j"].as_f64().unwrap(),
        400.,
    );
    near(water["mass_balance_error_kg"].as_f64().unwrap(), 0.);
    let independent = report["stored_change_j"].as_f64().unwrap()
        + water["liquid_stored_change_j"].as_f64().unwrap()
        - 400.
        - 100.
        - report["net_boundary_energy_j"].as_f64().unwrap();
    near(independent, 0.);
    near(report["balance_error_j"].as_f64().unwrap(), independent);
    near(
        water["patches"][0]["condensed_liquid_kg"].as_f64().unwrap(),
        0.01,
    );
    near(
        water["patches"][0]["incoming_vapor_sensible_energy_j"]
            .as_f64()
            .unwrap(),
        400.,
    );
}

#[test]
fn condensed_water_can_reevaporate_after_a_boundary_reversal() {
    let mut history = condensing_fixture(0., 0.1, vec![6000., 1000.], vec![30., 10.]);
    history
        .advance(&geometry(0.5), 0.5, 0, 0, None, None, None)
        .unwrap();
    let condensed = history.water.as_ref().unwrap().condensed[0];
    let warm = history.state.temperature_c()[0][0];
    assert!(condensed > 0. && warm > 20.);
    history
        .advance(&geometry(0.5), 1., 1, 1, None, None, None)
        .unwrap();
    let water = history.water.as_ref().unwrap();
    assert_eq!(water.condensed[0], condensed);
    near(water.evaporated[0], condensed);
    assert_eq!(water.remaining[0], 0.);
    assert_eq!(water.dryout_events, 1);
    assert!(history.state.temperature_c()[0][0] < warm);
    near(history.balance_error_j(), 0.);
}

#[test]
fn condensation_uses_current_transfer_area_and_reference_inventory_domain() {
    let mut small = condensing_fixture(0., 1., vec![6000.], vec![20.]);
    let mut large = condensing_fixture(0., 1., vec![6000.], vec![20.]);
    advance(&mut small, 0.5, 1.).unwrap();
    advance(&mut large, 1., 1.).unwrap();
    near(small.water.as_ref().unwrap().condensed[0], 0.01);
    near(large.water.as_ref().unwrap().condensed[0], 0.02);
    // Domain capacity remains .1 kg/reference m² * .5 reference m², not stretched area.
    let mut overflow = condensing_fixture(0.08, 1., vec![6000.], vec![20.]);
    let initial_temperature = overflow.state.temperature_c().to_vec();
    assert!(advance(&mut overflow, 1., 1.).is_err());
    assert_eq!(overflow.state.temperature_c(), initial_temperature);
    assert_eq!(overflow.water.as_ref().unwrap().remaining[0], 0.04);
    assert_eq!(overflow.water.as_ref().unwrap().condensed[0], 0.);
    assert_eq!(overflow.water.as_ref().unwrap().condensation_latent_j, 0.);
    assert_eq!(overflow.thermal_steps, 0);

    // Several accepted thermal subdivisions precede this overflow; the containing mechanical
    // interval still publishes none of their liquid, heat, temperatures or source counters.
    let mut late = condensing_fixture(0., 0.1, vec![6000.], vec![20.]);
    late.water
        .as_mut()
        .unwrap()
        .request
        .maximum_liquid_loading_kg_m2 = 0.005;
    let before = late.state.temperature_c().to_vec();
    assert!(advance(&mut late, 0.5, 1.).is_err());
    assert_eq!(late.state.temperature_c(), before);
    assert_eq!(late.water.as_ref().unwrap().remaining, vec![0.]);
    assert_eq!(late.water.as_ref().unwrap().condensed, vec![0.]);
    assert_eq!(late.water.as_ref().unwrap().condensation_latent_j, 0.);
    assert_eq!(late.thermal_steps, 0);
}

#[test]
fn condensation_effective_enthalpy_reference_only_relabels_the_complete_energy_ledger() {
    let mut zero = condensing_fixture(0.01, 0.1, vec![6000.], vec![40.]);
    let mut shifted = condensing_fixture(0.01, 0.1, vec![6000.], vec![40.]);
    shifted.water.as_mut().unwrap().request.enthalpy_reference_c = 100.;
    shifted
        .request
        .surface_water
        .as_mut()
        .unwrap()
        .enthalpy_reference_c = 100.;
    advance(&mut zero, 0.5, 1.).unwrap();
    advance(&mut shifted, 0.5, 1.).unwrap();
    assert_eq!(zero.state.temperature_c(), shifted.state.temperature_c());
    assert_eq!(
        zero.water.as_ref().unwrap().remaining,
        shifted.water.as_ref().unwrap().remaining
    );
    assert_eq!(
        zero.water.as_ref().unwrap().condensed,
        shifted.water.as_ref().unwrap().condensed
    );
    near(zero.balance_error_j(), 0.);
    near(shifted.balance_error_j(), 0.);
    let a = zero.water.as_ref().unwrap();
    let b = shifted.water.as_ref().unwrap();
    near(
        b.incoming_vapor_sensible_j[0] - a.incoming_vapor_sensible_j[0],
        -a.condensed[0] * 1000. * 100.,
    );
}

#[test]
fn condensation_requires_its_independent_source_and_cannot_leak_into_older_versions() {
    let history = condensing_fixture(0., 1., vec![6000.], vec![20.]);
    let valid = history.request.surface_water.unwrap();
    for version in ["ftx-textile-surface-water/1", "ftx-textile-surface-water/2"] {
        let mut wrong = valid.clone();
        wrong.schema = version.into();
        assert!(wrong.validate(1).is_err());
    }
    for mutation in 0..5 {
        let mut wrong = valid.clone();
        let condensation = wrong.condensation.as_mut().unwrap();
        match mutation {
            0 => condensation.allow_estimates = false,
            1 => condensation.qualification.clear(),
            2 => condensation.source.reference.clear(),
            3 => condensation.vapor_temperature_c = vec![81.],
            _ => condensation.vapor_temperature_c.clear(),
        }
        assert!(wrong.validate(1).is_err());
    }
    let mut old = supplied_fixture(0.01, 1., vec![0.], vec![20.]);
    old.water
        .as_mut()
        .unwrap()
        .request
        .outside_vapor_pressure_pa[0] = 6000.;
    assert!(advance(&mut old, 0.5, 1.).is_err());
}

#[test]
fn condensation_refines_toward_the_same_coupled_temperature_and_mass() {
    let run = |step| {
        let mut history = condensing_fixture(0., step, vec![6000.], vec![40.]);
        advance(&mut history, 0.5, 1.).unwrap();
        near(history.balance_error_j(), 0.);
        (
            history.state.temperature_c()[0][0],
            history.water.as_ref().unwrap().condensed[0],
        )
    };
    let coarse = run(0.1);
    let medium = run(0.05);
    let fine = run(0.025);
    let reference = run(0.001);
    assert!((fine.0 - reference.0).abs() < (medium.0 - reference.0).abs());
    assert!((medium.0 - reference.0).abs() < (coarse.0 - reference.0).abs());
    assert!((fine.1 - reference.1).abs() < (medium.1 - reference.1).abs());
    assert!((medium.1 - reference.1).abs() < (coarse.1 - reference.1).abs());
}

#[test]
fn supplied_liquid_enthalpy_enters_the_actual_heat_solve_and_balances_independently() {
    let mut warm = supplied_fixture(0.05, 1., vec![0.01], vec![40.]);
    let mut cold = supplied_fixture(0.05, 1., vec![0.01], vec![0.]);
    // Reference area .5: .025 kg initially, .005 kg delivered, .015 kg evaporated.
    // Warm incoming liquid adds 100 J of mixing heat; latent removal is 150 J.
    let remaining = 0.025 + (0.005 - 0.015);
    let capacity = 100. + remaining * 1000.;
    let conductance = 0.5 / 1.2;
    let expected =
        (capacity * 20. + 2. * conductance * 20. + 100. - 150.) / (capacity + 2. * conductance);
    advance(&mut warm, 0.5, 1.).unwrap();
    advance(&mut cold, 0.5, 1.).unwrap();
    near(warm.state.temperature_c()[0][0], expected);
    assert!(cold.state.temperature_c()[0][0] < warm.state.temperature_c()[0][0]);
    let result = warm.finish("path", "initial", 1.).unwrap();
    let water = &result["surface_water"];
    assert_eq!(water["schema"], "ftx-textile-surface-water-result/2");
    near(water["supplied_liquid_kg"].as_f64().unwrap(), 0.005);
    near(
        water["supplied_liquid_sensible_energy_j"].as_f64().unwrap(),
        200.,
    );
    near(
        water["patches"][0]["supplied_liquid_sensible_energy_j"]
            .as_f64()
            .unwrap(),
        200.,
    );
    near(water["remaining_liquid_kg"].as_f64().unwrap(), remaining);
    near(water["evaporated_liquid_kg"].as_f64().unwrap(), 0.015);
    near(water["mass_balance_error_kg"].as_f64().unwrap(), 0.);
    let balance = result["stored_change_j"].as_f64().unwrap()
        + water["liquid_stored_change_j"].as_f64().unwrap()
        + water["exported_vapor_sensible_energy_j"].as_f64().unwrap()
        + water["latent_energy_j"].as_f64().unwrap()
        - water["supplied_liquid_sensible_energy_j"].as_f64().unwrap()
        - result["net_boundary_energy_j"].as_f64().unwrap();
    near(balance, 0.);
    near(result["balance_error_j"].as_f64().unwrap(), balance);
    near(
        result["trace"][0]["surface_water"]["supplied_liquid_kg"]
            .as_f64()
            .unwrap(),
        0.005,
    );
}

#[test]
fn continuous_supply_survives_exact_dryout_and_rewets_an_empty_patch() {
    let mut history = supplied_fixture(0.015, 1., vec![0.01], vec![20.]);
    // Initial .0075 kg / (.015 evaporation - .005 supply) kg/s = .75 s to dryout.
    // The remaining .25 s is supply limited, not an omitted dry interval.
    advance(&mut history, 0.5, 1.).unwrap();
    assert_eq!(history.thermal_steps, 2);
    let water = history.water.as_ref().unwrap();
    assert_eq!(water.dryout_events, 1);
    assert_eq!(water.remaining, vec![0.]);
    near(water.supplied[0], 0.005);
    near(water.evaporated[0], 0.0125);
    near(water.latent_j, 125.);
    near(history.balance_error_j(), 0.);

    let mut dry = supplied_fixture(0., 1., vec![0.01, 0.08], vec![20., 20.]);
    dry.advance(&geometry(0.5), 0.5, 0, 0, None, None, None)
        .unwrap();
    assert_eq!(dry.water.as_ref().unwrap().remaining, vec![0.]);
    near(dry.water.as_ref().unwrap().evaporated[0], 0.0025);
    dry.advance(&geometry(0.5), 1., 1, 1, None, None, None)
        .unwrap();
    assert!(dry.water.as_ref().unwrap().remaining[0] > 0.);
    near(dry.water.as_ref().unwrap().supplied[0], 0.0025 + 0.02);
    near(dry.balance_error_j(), 0.);
}

#[test]
fn mechanical_interval_flows_come_from_accepted_plans_after_prior_dryout() {
    let mut history = supplied_fixture(0.015, 1., vec![0.01], vec![20.]);
    history.water.as_mut().unwrap().begin_mechanical_interval();
    advance(&mut history, 0.5, 1.).unwrap();
    let water = history.water.as_ref().unwrap();
    assert_eq!(water.remaining, vec![0.]);
    let prior_evaporation = water.evaporated[0];
    assert!(prior_evaporation > 0.);
    let flow = water.mechanical_flow.as_ref().unwrap();
    near(flow.incoming[0], 0.005);
    near(flow.outgoing[0], 0.0125);

    history.water.as_mut().unwrap().begin_mechanical_interval();
    history
        .advance(&geometry(0.5), 2., 0, 1, None, None, None)
        .unwrap();
    let water = history.water.as_ref().unwrap();
    let flow = water.mechanical_flow.as_ref().unwrap();
    assert_eq!(water.remaining, vec![0.]);
    assert_eq!(flow.incoming, vec![0.005]);
    assert_eq!(flow.outgoing, flow.incoming);
    // Cumulative subtraction rounds differently; it must not invent a dry-mass loss.
    assert_ne!(water.evaporated[0] - prior_evaporation, flow.outgoing[0]);
    near(history.balance_error_j(), 0.);
}

#[test]
fn interval_supply_uses_reference_area_and_preserves_sensible_reference_and_refinement() {
    let mut reference = supplied_fixture(0.05, 0.1, vec![0.01, 0.02], vec![40., 5.]);
    let mut shifted = reference.clone();
    shifted.water.as_mut().unwrap().request.enthalpy_reference_c = 15.;
    for history in [&mut reference, &mut shifted] {
        history
            .advance(&geometry(0.5), 0.5, 0, 0, None, None, None)
            .unwrap();
        history
            .advance(&geometry(1.), 1., 1, 1, None, None, None)
            .unwrap();
        near(
            history.water.as_ref().unwrap().supplied[0],
            0.5 * (0.01 * 0.5 + 0.02 * 0.5),
        );
        near(history.balance_error_j(), 0.);
    }
    assert_eq!(
        reference.state.temperature_c(),
        shifted.state.temperature_c()
    );
    near(
        reference.water.as_ref().unwrap().energy_correction_j(),
        shifted.water.as_ref().unwrap().energy_correction_j(),
    );
    near(
        reference.water.as_ref().unwrap().supplied_sensible_j[0],
        1000. * 0.5 * (0.01 * 0.5 * 40. + 0.02 * 0.5 * 5.),
    );
    let temperature = |step| {
        let mut history = supplied_fixture(0.05, step, vec![0.01], vec![40.]);
        advance(&mut history, 0.5, 1.).unwrap();
        history.state.temperature_c()[0][0]
    };
    let refined = temperature(1. / 512.);
    assert!((temperature(0.125) - refined).abs() < 0.6 * (temperature(0.25) - refined).abs());
}

#[test]
fn supply_requires_independent_complete_sources_and_overflow_rolls_back_the_interval() {
    let mut history = supplied_fixture(0., 0.5, vec![0.15], vec![20.]);
    let mut request = history.water.as_ref().unwrap().request.clone();
    let supply = request.supply.as_mut().unwrap();
    supply.allow_estimates = false;
    assert!(request.validate(1).is_err());
    request.supply.as_mut().unwrap().allow_estimates = true;
    request
        .supply
        .as_mut()
        .unwrap()
        .bindings
        .get_mut("sheet")
        .unwrap()
        .temperature_c[0] = 81.;
    assert!(request.validate(1).is_err());
    request
        .supply
        .as_mut()
        .unwrap()
        .bindings
        .get_mut("sheet")
        .unwrap()
        .temperature_c[0] = 20.;
    request
        .supply
        .as_mut()
        .unwrap()
        .bindings
        .get_mut("sheet")
        .unwrap()
        .rate_kg_m2_s
        .push(0.);
    assert!(request.validate(1).is_err());
    request
        .supply
        .as_mut()
        .unwrap()
        .bindings
        .get_mut("sheet")
        .unwrap()
        .rate_kg_m2_s
        .pop();
    request.replenishment_kg_m2_s = Some(0.);
    assert!(request.validate(1).is_err());
    request.replenishment_kg_m2_s = None;
    request.schema = "ftx-textile-surface-water/1".into();
    assert!(request.validate(1).is_err());
    // The first half second wets the patch within its .05 kg limit. The second overfills it,
    // so neither successful prefix state nor delivered mass/enthalpy may survive.
    let before = history
        .water
        .as_ref()
        .unwrap()
        .snapshot(history.state.temperature_c(), true, 0.)
        .unwrap();
    assert!(advance(&mut history, 0.5, 1.).is_err());
    assert_eq!(history.state.temperature_c(), &[vec![20.]]);
    assert_eq!(history.time_s, 0.);
    assert_eq!(history.cell_steps, 0);
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
    request.replenishment_kg_m2_s = Some(1e-9);
    assert!(request.validate(1).is_err());
    request.replenishment_kg_m2_s = Some(0.);
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
