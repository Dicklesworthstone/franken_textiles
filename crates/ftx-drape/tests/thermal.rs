//! Independent dry-network known answers; these are not textile measurements or comfort tests.

use ftx_drape::thermal::{Boundary, DryThermalModel};
use ftx_pattern::material_catalog::{ResolvedMaterial, Selection};
use ftx_pattern::materials::{Catalog, ThermalStack};
use serde_json::json;

fn selection(id: &str, r: f64, mass: f64, heat: f64, allow: bool) -> Selection {
    let mut props = serde_json::Map::new();
    for (key, value, unit) in [
        ("thermal_resistance", r, "m2*K/W"),
        ("areal_density", mass, "kg/m2"),
        ("specific_heat", heat, "J/(kg*K)"),
    ] {
        props.insert(key.into(), json!({"value":value,"unit":unit,"evidence":"estimate","source":"test","method":"synthetic known answer"}));
    }
    serde_json::from_value(json!({"id":id,"allow_estimates":allow,"record":{
        "id":id,"name":"Synthetic thermal layer","form":"fill","properties":props,
        "sources":{"test":{"reference":"test:thermal","accessed_on":"2026-10-14","note":"Not physical textile data"}}}})).unwrap()
}

fn resolved(s: &Selection) -> ResolvedMaterial {
    Catalog::builtin().unwrap().resolve(s).unwrap()
}
fn layer(id: &str, r: f64, c: f64) -> ResolvedMaterial {
    resolved(&selection(id, r, 0.2, c / 0.2, true))
}
fn boundary() -> Boundary {
    Boundary {
        inside_c: 30.0,
        outside_c: 0.0,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    }
}
fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} != {b}");
}

#[test]
fn one_cell_matches_backward_euler_and_conserves_boundary_heat() {
    let model = DryThermalModel::new(vec![layer("a", 0.4, 200.0)], vec![], 1).unwrap();
    let b = boundary();
    let g1 = 1.0 / (0.1 + 0.2);
    let g2 = 1.0 / (0.2 + 0.2);
    let t = model.advance(&[10.0], 5.0, b).unwrap();
    let expected = (200.0 / 5.0 * 10.0 + g1 * 30.0 + g2 * 0.0) / (200.0 / 5.0 + g1 + g2);
    near(t.temperature_c[0], expected, 1e-12);
    near(t.stored_change_j_m2, 200.0 * (expected - 10.0), 1e-10);
    near(
        t.stored_change_j_m2,
        5.0 * (t.inside_flux_w_m2 - t.outside_flux_w_m2),
        1e-10,
    );
    near(t.balance_error_j_m2, 0.0, 1e-10);
}

#[test]
fn internal_cell_energy_obeys_backward_euler_above_equal_reservoir_temperatures() {
    let model = DryThermalModel::new(vec![layer("heated", 0.4, 200.0)], vec![], 1).unwrap();
    let receipt = model.snapshot();
    let old = [20.0];
    let supplied = [1000.0];
    let b = Boundary {
        inside_c: 20.0,
        outside_c: 20.0,
        ..boundary()
    };
    let state = model.advance_with_heat(&old, 5.0, b, &supplied).unwrap();
    let expected = 20.0 + 1000.0 / (200.0 + 5.0 * (1.0 / 0.3 + 1.0 / 0.4));
    near(state.temperature_c[0], expected, 1e-12);
    assert!(state.temperature_c[0] > 20.0);
    near(state.stored_change_j_m2, 200.0 * (expected - 20.0), 1e-10);
    near(
        state.stored_change_j_m2,
        5.0 * (state.inside_flux_w_m2 - state.outside_flux_w_m2) + supplied[0],
        1e-10,
    );
    near(state.balance_error_j_m2, 0.0, 1e-10);
    assert_eq!(old, [20.0]);
    assert_eq!(supplied, [1000.0]);
    assert_eq!(model.snapshot(), receipt);
}

#[test]
fn internal_energy_validation_is_atomic_and_zero_sources_preserve_existing_steps() {
    let model = DryThermalModel::new(vec![layer("heated", 0.4, 200.0)], vec![], 2).unwrap();
    let old = [10.0, 15.0];
    let original = model.advance(&old, 5.0, boundary()).unwrap();
    let zero = model
        .advance_with_heat(&old, 5.0, boundary(), &[0.0, 0.0])
        .unwrap();
    assert_eq!(zero.temperature_c, original.temperature_c);
    assert_eq!(zero.stored_change_j_m2, original.stored_change_j_m2);
    assert_eq!(zero.balance_error_j_m2, original.balance_error_j_m2);
    assert_eq!(zero.inside_flux_w_m2, original.inside_flux_w_m2);
    assert_eq!(zero.outside_flux_w_m2, original.outside_flux_w_m2);
    for heat in [
        vec![],
        vec![1.0],
        vec![1.0, -1.0],
        vec![1.0, f64::NAN],
        vec![1.0, f64::INFINITY],
        vec![f64::MAX, f64::MAX],
    ] {
        assert!(
            model
                .advance_with_heat(&old, 5.0, boundary(), &heat)
                .is_err()
        );
    }
    assert!(
        model
            .advance_with_heat(&old, 0.0, boundary(), &[1.0; 2])
            .is_err()
    );
    assert!(
        model
            .advance_with_heat(&[10.0], 5.0, boundary(), &[1.0; 2])
            .is_err()
    );
    assert_eq!(old, [10.0, 15.0]);
    assert_eq!(
        model.advance(&old, 5.0, boundary()).unwrap().temperature_c,
        original.temperature_c
    );
}

#[test]
fn steady_limit_matches_series_stack_and_interfaces_locate_cell_centres() {
    let layers = vec![layer("a", 0.2, 100.0), layer("b", 0.4, 300.0)];
    let refs: Vec<_> = layers.iter().map(|m| m.material()).collect();
    let original = ThermalStack::evaluate(&refs, 1.0, 30.0, 0.0, 0.3, true).unwrap();
    let model = DryThermalModel::new(layers, vec![0.0], 4).unwrap();
    let steady = model.steady(boundary()).unwrap();
    near(steady.inside_flux_w_m2, 30.0 / 0.9, 1e-12);
    // Cross-check the existing static consumer rather than an independent material path.
    near(steady.inside_flux_w_m2, original.heat_flow_w, 1e-12);
    let mut state = vec![0.0; model.cells()];
    for _ in 0..200 {
        state = model
            .advance(&state, 10.0, boundary())
            .unwrap()
            .temperature_c;
    }
    for (a, b) in state.iter().zip(&steady.temperature_c) {
        near(*a, *b, 1e-8);
    }
    let with_gap = DryThermalModel::new(
        vec![layer("a", 0.2, 100.0), layer("b", 0.4, 300.0)],
        vec![0.3],
        1,
    )
    .unwrap();
    let steady = with_gap.steady(boundary()).unwrap();
    near(steady.inside_flux_w_m2, 25.0, 1e-12);
    near(steady.temperature_c[0], 25.0, 1e-12);
    near(steady.temperature_c[1], 10.0, 1e-12);
    let unchanged = with_gap
        .advance(&steady.temperature_c, 0.1, boundary())
        .unwrap();
    for (a, b) in unchanged.temperature_c.iter().zip(steady.temperature_c) {
        near(*a, b, 1e-12);
    }
}

#[test]
fn smaller_time_steps_converge_to_the_independent_one_cell_exponential() {
    let model = DryThermalModel::new(vec![layer("a", 0.4, 200.0)], vec![], 1).unwrap();
    let b = boundary();
    let eq = model.steady(b).unwrap().temperature_c[0];
    let exact = eq * (1.0 - (-30.0 * (1.0 / 0.3 + 1.0 / 0.4) / 200.0_f64).exp());
    let integrate = |dt: f64, count: usize| {
        let mut t = vec![0.0];
        for _ in 0..count {
            t = model.advance(&t, dt, b).unwrap().temperature_c;
        }
        (t[0] - exact).abs()
    };
    assert!(integrate(0.5, 60) < integrate(5.0, 6) / 8.0);
}

#[test]
fn material_only_capacity_or_resistance_edits_change_their_actual_consumers() {
    let a = DryThermalModel::new(vec![layer("a", 0.2, 100.0)], vec![], 2).unwrap();
    let b = DryThermalModel::new(vec![layer("b", 0.2, 400.0)], vec![], 2).unwrap();
    let c = DryThermalModel::new(vec![layer("c", 0.6, 100.0)], vec![], 2).unwrap();
    let ba = a.steady(boundary()).unwrap();
    let bb = b.steady(boundary()).unwrap();
    let bc = c.steady(boundary()).unwrap();
    assert_eq!(ba.temperature_c, bb.temperature_c);
    assert!(ba.inside_flux_w_m2 > bc.inside_flux_w_m2);
    let ta = a.advance(&[0.0; 2], 1.0, boundary()).unwrap();
    let tb = b.advance(&[0.0; 2], 1.0, boundary()).unwrap();
    assert!(ta.temperature_c[0] > tb.temperature_c[0]);
    assert_ne!(
        a.snapshot()["layers"][0]["sha256"],
        b.snapshot()["layers"][0]["sha256"]
    );
    near(
        a.snapshot()["cell_capacity_j_m2_k"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .sum(),
        100.0,
        1e-12,
    );
}

#[test]
fn changing_boundary_preserves_energy_and_maximum_principle_over_many_steps() {
    let model = DryThermalModel::new(
        vec![
            layer("a", 0.05, 40.0),
            layer("b", 0.6, 320.0),
            layer("c", 0.1, 100.0),
        ],
        vec![0.02, 0.04],
        8,
    )
    .unwrap();
    let mut t = vec![20.0; model.cells()];
    let mut stored = 0.0;
    let mut net = 0.0;
    for i in 0..80 {
        let b = Boundary {
            inside_c: if i < 40 { 35.0 } else { -5.0 },
            outside_c: -10.0,
            ..boundary()
        };
        let step = model.advance(&t, 0.4, b).unwrap();
        assert!(step.temperature_c.iter().all(|t| *t >= -10.0 && *t <= 35.0));
        stored += step.stored_change_j_m2;
        net += 0.4 * (step.inside_flux_w_m2 - step.outside_flux_w_m2);
        t = step.temperature_c;
    }
    near(stored, net, 1e-7);
}

#[test]
fn every_layer_enforces_its_own_evidence_policy_and_specific_heat_is_not_guessed() {
    let a = layer("a", 0.2, 100.0);
    let denied = resolved(&selection("b", 0.2, 0.2, 500.0, false));
    assert!(DryThermalModel::new(vec![a.clone(), denied], vec![0.0], 1).is_err());
    let mut s = selection("missing", 0.2, 0.2, 500.0, true);
    s.record
        .as_mut()
        .unwrap()
        .properties
        .remove("specific_heat");
    let e = DryThermalModel::new(vec![resolved(&s)], vec![], 1).unwrap_err();
    assert!(e.message.contains("specific_heat"));
    assert!(DryThermalModel::new(vec![a], vec![], 1).is_ok());
}

#[test]
fn invalid_configuration_or_state_never_changes_a_callers_state() {
    assert!(DryThermalModel::new(vec![], vec![], 1).is_err());
    assert!(DryThermalModel::new(vec![layer("a", 0.2, 100.0)], vec![], 0).is_err());
    assert!(DryThermalModel::new(vec![layer("a", 0.2, 100.0)], vec![], 4097).is_err());
    assert!(DryThermalModel::new(vec![layer("a", 0.2, 100.0)], vec![0.0], 1).is_err());
    assert!(
        DryThermalModel::new(
            vec![layer("a", 0.2, 100.0), layer("b", 0.2, 100.0)],
            vec![-1.0],
            1
        )
        .is_err()
    );
    let model = DryThermalModel::new(vec![layer("a", 0.2, 100.0)], vec![], 1).unwrap();
    let old = vec![20.0];
    for dt in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(model.advance(&old, dt, boundary()).is_err());
    }
    assert!(model.advance(&[], 1.0, boundary()).is_err());
    assert!(model.advance(&[f64::NAN], 1.0, boundary()).is_err());
    assert!(model.advance(&[-274.0], 1.0, boundary()).is_err());
    assert!(
        model
            .advance(
                &old,
                1.0,
                Boundary {
                    inside_resistance: -1.0,
                    ..boundary()
                }
            )
            .is_err()
    );
    assert_eq!(old, vec![20.0]);
}
