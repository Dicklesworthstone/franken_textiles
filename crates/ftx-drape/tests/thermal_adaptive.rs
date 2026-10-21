//! Time refinement against independently integrated RC equations, not an empirical textile test.
use ftx_core::RefusalCode;
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    adaptive::{Budget, Options},
};
use ftx_pattern::{
    material_catalog::Selection,
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde_json::json;

fn layer(
    id: &str,
    resistance: f64,
    capacity: f64,
) -> ftx_pattern::material_catalog::ResolvedMaterial {
    let selection: Selection = serde_json::from_value(json!({"id":id,"allow_estimates":true,"record":{
        "id":id,"name":"Synthetic RC element","form":"fill","sources":{
            "test":{"reference":"test:adaptive-thermal","accessed_on":"2026-10-21","note":"Invented numerical coefficients"}},
        "properties":{
            "areal_density":{"value":0.1,"unit":"kg/m2","source":"test","evidence":"estimate","method":"synthetic"},
            "specific_heat":{"value":capacity/0.1,"unit":"J/(kg*K)","source":"test","evidence":"estimate","method":"synthetic"},
            "thermal_resistance":{"value":resistance,"unit":"m2*K/W","source":"test","evidence":"estimate","method":"synthetic"}}}})).unwrap();
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&selection)
    .unwrap()
}

fn model() -> DryThermalModel {
    DryThermalModel::new(vec![layer("one", 0.25, 100.0)], vec![], 1).unwrap()
}

fn boundary() -> Boundary {
    Boundary {
        inside_c: 40.0,
        outside_c: 0.0,
        inside_resistance: 0.0,
        outside_resistance: 0.0,
    }
}

fn options(tolerance: f64) -> Options {
    Options {
        absolute_temperature_tolerance_k: tolerance,
        absolute_heat_tolerance_j_m2: 100.0 * tolerance,
        relative_tolerance: 0.0,
        min_step_s: 1e-9,
        max_step_s: 1000.0,
        max_trials: 50_000,
        max_cell_solves: 1_000_000,
    }
}

fn near(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} != {b} (tol {tolerance})");
}

#[test]
fn long_hold_converges_to_exponential_not_one_implicit_step() {
    let m = model();
    let snapshot = m.snapshot();
    let old = [0.0];
    let seconds = 10.0;
    let exact = 20.0 * (1.0 - (-seconds / 6.25_f64).exp());
    let coarse = m.advance(&old, seconds, boundary()).unwrap();
    assert!((coarse.temperature_c[0] - exact).abs() > 3.0);
    let loose = m
        .advance_adaptive(
            &old,
            seconds,
            boundary(),
            &mut Budget::new(options(0.05)).unwrap(),
        )
        .unwrap();
    let tight = m
        .advance_adaptive(
            &old,
            seconds,
            boundary(),
            &mut Budget::new(options(1e-5)).unwrap(),
        )
        .unwrap();
    near(tight.state.temperature_c[0], exact, 0.005);
    assert!(
        (tight.state.temperature_c[0] - exact).abs()
            < 0.03 * (loose.state.temperature_c[0] - exact).abs()
    );
    assert!(tight.integration.rejected_trials > 0);
    assert!(tight.integration.max_accepted_error_ratio <= 1.0);
    assert_eq!(tight.integration.cell_solves, 3 * tight.integration.trials);
    let ti = 20.0 * seconds - 20.0 * 6.25 * (1.0 - (-seconds / 6.25).exp());
    near(tight.inside_heat_j_m2, 8.0 * (40.0 * seconds - ti), 0.3);
    near(tight.outside_heat_j_m2, 8.0 * ti, 0.3);
    near(
        tight.state.stored_change_j_m2,
        100.0 * tight.state.temperature_c[0],
        1e-10,
    );
    near(tight.state.balance_error_j_m2, 0.0, 1e-8);
    assert!((tight.inside_heat_j_m2 - seconds * tight.state.inside_flux_w_m2).abs() > 100.0);
    assert_eq!(snapshot, m.snapshot());
    assert_eq!(old, [0.0]);
    let replay = m
        .advance_adaptive(
            &old,
            seconds,
            boundary(),
            &mut Budget::new(options(1e-5)).unwrap(),
        )
        .unwrap();
    assert_eq!(replay.state.temperature_c, tight.state.temperature_c);
    assert_eq!(replay.inside_heat_j_m2, tight.inside_heat_j_m2);
    assert_eq!(
        serde_json::to_value(replay.integration).unwrap(),
        serde_json::to_value(tight.integration).unwrap()
    );
}

#[test]
fn both_boundary_integrals_are_error_controlled_even_with_loose_temperature_tolerance() {
    let m = model();
    let mut limits = options(1000.0);
    let unchecked = m
        .advance_adaptive(
            &[0.0],
            1000.0,
            boundary(),
            &mut Budget::new(limits.clone()).unwrap(),
        )
        .unwrap();
    limits.absolute_heat_tolerance_j_m2 = 0.001;
    let checked = m
        .advance_adaptive(
            &[0.0],
            1000.0,
            boundary(),
            &mut Budget::new(limits).unwrap(),
        )
        .unwrap();
    let exact_inside = 8.0 * (20.0 * 1000.0 + 20.0 * 6.25);
    assert!((unchecked.inside_heat_j_m2 - exact_inside).abs() > 0.1);
    assert!(
        (checked.inside_heat_j_m2 - exact_inside).abs()
            < (unchecked.inside_heat_j_m2 - exact_inside).abs()
    );
    assert!(checked.integration.accepted_substeps > unchecked.integration.accepted_substeps);
    near(checked.state.balance_error_j_m2, 0.0, 1e-7);
}

#[test]
fn unequal_layer_transient_matches_independent_two_by_two_matrix_exponential() {
    let m = DryThermalModel::new(
        vec![layer("inner", 0.2, 100.0), layer("outer", 0.6, 300.0)],
        vec![0.1],
        1,
    )
    .unwrap();
    let t = 20.0;
    let a = -0.12_f64;
    let b = 0.02_f64;
    let c = 2.0 / 300.0;
    let d = -(2.0 + 1.0 / 0.3) / 300.0;
    let tr = (a + d) / 2.0;
    let disc = (((a - d) / 2.0).powi(2) + b * c).sqrt();
    let steady = [40.0 - 40.0 / 0.9 * 0.1, 40.0 / 0.9 * 0.3];
    let x = [-steady[0], -steady[1]];
    let factor = (tr * t).exp();
    let co = (disc * t).cosh();
    let si = (disc * t).sinh() / disc;
    let expected = [
        steady[0] + factor * (co * x[0] + si * ((a - tr) * x[0] + b * x[1])),
        steady[1] + factor * (co * x[1] + si * (c * x[0] + (d - tr) * x[1])),
    ];
    let solved = m
        .advance_adaptive(
            &[0.0, 0.0],
            t,
            boundary(),
            &mut Budget::new(options(1e-5)).unwrap(),
        )
        .unwrap();
    for (actual, target) in solved.state.temperature_c.iter().zip(expected) {
        near(*actual, target, 0.006);
    }
    let determinant = a * d - b * c;
    let integral = [
        steady[0] * t + (d * expected[0] - b * expected[1]) / determinant,
        steady[1] * t + (-c * expected[0] + a * expected[1]) / determinant,
    ];
    near(
        solved.inside_heat_j_m2,
        10.0 * (40.0 * t - integral[0]),
        1.0,
    );
    near(solved.outside_heat_j_m2, integral[1] / 0.3, 1.0);
    near(
        solved.state.stored_change_j_m2,
        100.0 * solved.state.temperature_c[0] + 300.0 * solved.state.temperature_c[1],
        1e-9,
    );
    near(solved.state.balance_error_j_m2, 0.0, 1e-7);
}

#[test]
fn variable_capacity_preserves_integrated_enthalpy_and_converges_to_exact_ode() {
    let input: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    let s: Selection = serde_json::from_value(input["layers"][0].clone()).unwrap();
    let resolved = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&s)
    .unwrap();
    let m = DryThermalModel::new(vec![resolved], vec![], 1).unwrap();
    let b = Boundary {
        inside_c: 30.0,
        outside_c: 0.0,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    };
    let (g, steady) = (
        1.0 / 0.3 + 1.0 / 0.4,
        (30.0 / 0.3) / (1.0 / 0.3 + 1.0 / 0.4),
    );
    let (mut lo, mut hi) = (10.0_f64, steady);
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        let elapsed = (-4.0 * (mid - 10.0)
            - (200.0 + 4.0 * steady) * ((steady - mid) / (steady - 10.0)).ln())
            / g;
        if elapsed < 50.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let solved = m
        .advance_adaptive(&[10.0], 50.0, b, &mut Budget::new(options(1e-5)).unwrap())
        .unwrap();
    let next = solved.state.temperature_c[0];
    near(next, (lo + hi) / 2.0, 0.004);
    let enthalpy = 200.0 * (next - 10.0) + 2.0 * (next * next - 100.0);
    near(solved.state.stored_change_j_m2, enthalpy, 1e-7);
    near(
        solved.inside_heat_j_m2 - solved.outside_heat_j_m2,
        enthalpy,
        1e-6,
    );
    let mut work = Budget::new(options(1e-5)).unwrap();
    assert!(m.advance_adaptive(&[100.0], 50.0, b, &mut work).is_err());
    assert_eq!(work.used(), [0, 0]);
}

#[test]
fn work_and_minimum_interval_refusals_never_mutate_inputs_or_refund_attempts() {
    let m = model();
    let original = m.snapshot();
    let old = [0.0];
    let mut limits = options(1e-8);
    limits.max_trials = 1;
    let mut budget = Budget::new(limits).unwrap();
    assert_eq!(
        m.advance_adaptive(&old, 10.0, boundary(), &mut budget)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(budget.used(), [1, 3]);
    assert_eq!(
        m.advance_adaptive(&old, 10.0, boundary(), &mut budget)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(budget.used(), [1, 3]);
    let mut limits = options(1e-8);
    limits.max_cell_solves = 2;
    let mut budget = Budget::new(limits).unwrap();
    assert_eq!(
        m.advance_adaptive(&old, 10.0, boundary(), &mut budget)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(budget.used(), [1, 2]);
    let mut limits = options(1e-8);
    limits.min_step_s = 10.0;
    assert_eq!(
        m.advance_adaptive(&old, 10.0, boundary(), &mut Budget::new(limits).unwrap())
            .unwrap_err()
            .code,
        RefusalCode::NotConverged
    );
    assert_eq!(old, [0.0]);
    assert_eq!(m.snapshot(), original);
    let mut budget = Budget::new(options(0.01)).unwrap();
    assert!(m.advance_adaptive(&[f64::NAN], 10.0, boundary(), &mut budget).is_err());
    assert!(m.advance_adaptive(&[], 10.0, boundary(), &mut budget).is_err());
    assert!(m.advance_adaptive(&old, f64::INFINITY, boundary(), &mut budget).is_err());
    assert_eq!(budget.used(), [0, 0]);
}

#[test]
fn constant_state_and_maximum_interval_have_exact_work_counts_and_shared_budget() {
    let m = model();
    let mut limits = options(0.01);
    limits.max_step_s = 2.0;
    limits.max_trials = 8;
    let mut budget = Budget::new(limits).unwrap();
    let b = Boundary {
        inside_c: 20.0,
        outside_c: 20.0,
        ..boundary()
    };
    let solved = m.advance_adaptive(&[20.0], 10.0, b, &mut budget).unwrap();
    assert_eq!(solved.state.temperature_c, [20.0]);
    assert_eq!(solved.inside_heat_j_m2, 0.0);
    assert_eq!(solved.outside_heat_j_m2, 0.0);
    assert_eq!(solved.integration.trials, 8);
    assert_eq!(solved.integration.accepted_substeps, 16);
    assert_eq!(solved.integration.min_accepted_step_s, 0.625);
    assert_eq!(solved.integration.max_accepted_step_s, 0.625);
    assert_eq!(solved.integration.max_accepted_error_ratio, 0.0);
    assert_eq!(
        m.advance_adaptive(&[20.0], 1.0, b, &mut budget)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    for mutate in [
        |o: &mut Options| o.relative_tolerance = f64::NAN,
        |o: &mut Options| o.relative_tolerance = 0.5,
        |o: &mut Options| o.absolute_temperature_tolerance_k = 0.0,
        |o: &mut Options| o.absolute_heat_tolerance_j_m2 = -1.0,
        |o: &mut Options| o.max_step_s = 0.0,
        |o: &mut Options| o.max_trials = 0,
        |o: &mut Options| o.max_cell_solves = 16_777_217,
        |o: &mut Options| o.min_step_s = f64::from_bits(1),
    ] {
        let mut o = options(0.01);
        mutate(&mut o);
        assert!(Budget::new(o).is_err());
    }
}
