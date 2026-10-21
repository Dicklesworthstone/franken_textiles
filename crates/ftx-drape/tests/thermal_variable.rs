//! Temperature-dependent material-to-transport checks against independent analytic energy laws.
use ftx_core::Vec3;
use ftx_drape::thermal::surface::{Patch, Surface};
use ftx_drape::thermal::{Boundary, DryThermalModel};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::materials::Catalog;
use serde_json::{Value, json};

fn selection() -> Selection {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    serde_json::from_value(fixture["layers"][0].clone()).unwrap()
}
fn model(s: &Selection, cells: usize) -> DryThermalModel {
    DryThermalModel::new(
        vec![Catalog::builtin().unwrap().resolve(s).unwrap()],
        vec![],
        cells,
    )
    .unwrap()
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
    assert!((a - b).abs() <= tol, "{a} != {b} (tol {tol})");
}

#[test]
fn variable_heat_capacity_matches_independent_quadratic_not_endpoint_capacity() {
    let model = model(&selection(), 1);
    let solved = model.advance(&[10.0], 5.0, boundary()).unwrap();
    // Integrate 0.2*(1000+20*T) dT, equate to 5*(100-(1/.3+1/.4)*T).
    let b: f64 = 200.0 + 5.0 * (1.0 / 0.3 + 1.0 / 0.4);
    let expected = 5400.0 / (b + (b * b + 8.0 * 2700.0).sqrt());
    near(solved.temperature_c[0], expected, 1e-9);
    let heat = 200.0 * (expected - 10.0) + 2.0 * (expected * expected - 100.0);
    near(solved.stored_change_j_m2, heat, 1e-7);
    near(solved.balance_error_j_m2, 0.0, 1e-7);
    assert!((heat - 0.2 * (1000.0 + 20.0 * expected) * (expected - 10.0)).abs() > 0.1);
    assert_eq!(model.snapshot()["schema"], "ftx-dry-thermal-model/2");
    assert!(model.snapshot().get("cell_capacity_j_m2_k").is_none());
}

#[test]
fn internal_heat_uses_integrated_variable_enthalpy_and_refuses_domain_extrapolation() {
    let model = model(&selection(), 1);
    let old = [10.0];
    let b = Boundary {
        inside_c: 10.0,
        outside_c: 10.0,
        ..boundary()
    };
    let supplied = 2000.0;
    let state = model.advance_with_heat(&old, 5.0, b, &[supplied]).unwrap();
    // H(10+d)-H(10) = 240*d + 2*d²; both reservoirs remove 5*(1/.3+1/.4)*d.
    let linear: f64 = 240.0 + 5.0 * (1.0 / 0.3 + 1.0 / 0.4);
    let delta = 2.0 * supplied / (linear + (linear * linear + 8.0 * supplied).sqrt());
    let expected = 10.0 + delta;
    let enthalpy = 240.0 * delta + 2.0 * delta * delta;
    near(state.temperature_c[0], expected, 1e-9);
    near(state.stored_change_j_m2, enthalpy, 1e-7);
    near(
        state.stored_change_j_m2,
        5.0 * (state.inside_flux_w_m2 - state.outside_flux_w_m2) + supplied,
        1e-7,
    );
    near(state.balance_error_j_m2, 0.0, 1e-7);
    assert!((enthalpy - (200.0 + 4.0 * expected) * delta).abs() > 1.0);
    assert!(model.advance_with_heat(&old, 5.0, b, &[1e9]).is_err());
    assert!(model.advance_with_heat(&[59.0], 5.0, b, &[1000.0]).is_err());
    assert_eq!(old, [10.0]);
    assert_eq!(
        model
            .advance_with_heat(&old, 5.0, b, &[supplied])
            .unwrap()
            .temperature_c,
        state.temperature_c
    );
}

#[test]
fn conductivity_refinement_converges_to_integrated_fourier_flux() {
    let mut s = selection();
    s.record
        .as_mut()
        .unwrap()
        .thermal_response
        .as_mut()
        .unwrap()
        .conductivity
        .as_mut()
        .unwrap()
        .points[1]
        .mean = 0.01;
    let b = Boundary {
        inside_resistance: 0.0,
        outside_resistance: 0.0,
        ..boundary()
    };
    // Independent continuum integral: q = (k0*30 + slope*30^2/2) / thickness.
    let exact = (0.0025 * 30.0 + 0.000125 * 30.0 * 30.0 / 2.0) / 0.001;
    let mut errors = Vec::new();
    for n in [8, 16, 32, 64] {
        let m = model(&s, n);
        let steady = m.steady(b).unwrap();
        near(steady.inside_flux_w_m2, steady.outside_flux_w_m2, 1e-6);
        // With linear k(T), the midpoint cell closure integrates total flux exactly. The
        // cell-centre temperature profile still has spatial error and must converge on refinement.
        near(steady.inside_flux_w_m2, exact, 1e-7);
        let error = steady
            .temperature_c
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let potential = exact * 0.001 * (1.0 - (i as f64 + 0.5) / n as f64);
                let expected = 2.0 * potential
                    / (0.0025 + (0.0025_f64.powi(2) + 2.0 * 0.000125 * potential).sqrt());
                (t - expected).abs()
            })
            .fold(0.0_f64, f64::max);
        errors.push(error);
        let advanced = m.advance(&steady.temperature_c, 0.1, b).unwrap();
        for (&a, &b) in advanced.temperature_c.iter().zip(&steady.temperature_c) {
            near(a, b, 1e-8);
        }
    }
    for pair in errors.windows(2) {
        assert!(pair[1] < pair[0] / 3.0, "{errors:?}");
    }
    assert!(errors[3] < 0.005, "{errors:?}");
}

#[test]
fn heterogeneous_layers_conserve_enthalpy_during_boundary_reversal() {
    let a = selection();
    let mut b = a.clone();
    b.id = "second".into();
    let record = b.record.as_mut().unwrap();
    record.id = b.id.clone();
    record.properties.get_mut("areal_density").unwrap().value *= 3.0;
    record
        .thermal_response
        .as_mut()
        .unwrap()
        .conductivity
        .as_mut()
        .unwrap()
        .points[1]
        .mean = 0.008;
    let c = Catalog::builtin().unwrap();
    let m = DryThermalModel::new(
        vec![c.resolve(&a).unwrap(), c.resolve(&b).unwrap()],
        vec![0.07],
        4,
    )
    .unwrap();
    let mut t = vec![10.0; 8];
    let mut integrated_boundary = 0.0;
    let mut stored = 0.0;
    for i in 0..80 {
        let b = if i < 40 {
            boundary()
        } else {
            Boundary {
                inside_c: 0.0,
                outside_c: 30.0,
                ..boundary()
            }
        };
        let step = m.advance(&t, 0.7, b).unwrap();
        assert!(
            step.temperature_c
                .iter()
                .all(|&t| (0.0..=30.0).contains(&t))
        );
        integrated_boundary += 0.7 * (step.inside_flux_w_m2 - step.outside_flux_w_m2);
        stored += step.stored_change_j_m2;
        t = step.temperature_c;
    }
    let independent: f64 = t
        .iter()
        .enumerate()
        .map(|(i, &t)| {
            let scale = if i < 4 { 1.0 } else { 3.0 };
            scale / 4.0 * (200.0 * (t - 10.0) + 2.0 * (t * t - 100.0))
        })
        .sum();
    near(stored, independent, 1e-8);
    near(integrated_boundary, independent, 1e-6);
}

#[test]
fn scalar_and_tabulated_layers_share_one_energy_network() {
    let a = selection();
    let mut b = a.clone();
    b.id = "constant".into();
    let record = b.record.as_mut().unwrap();
    record.id = b.id.clone();
    record.thermal_response = None;
    for (key, value, unit) in [
        ("specific_heat", 1000.0, "J/(kg*K)"),
        ("thermal_conductivity", 0.0025, "W/(m*K)"),
    ] {
        let mut q = record.properties["areal_density"].clone();
        q.value = value;
        q.unit = unit.into();
        record.properties.insert(key.into(), q);
    }
    let db = Catalog::builtin().unwrap();
    let m = DryThermalModel::new(
        vec![db.resolve(&a).unwrap(), db.resolve(&b).unwrap()],
        vec![0.1],
        2,
    )
    .unwrap();
    let step = m.advance(&[10.0; 4], 5.0, boundary()).unwrap();
    near(step.balance_error_j_m2, 0.0, 1e-7);
    near(
        m.steady(boundary()).unwrap().inside_flux_w_m2,
        30.0 / 1.2,
        1e-8,
    );
}

fn surface(selection: &Selection) -> Surface {
    Surface::new(
        vec![model(selection, 1)],
        3,
        vec![Patch {
            vertices: [0, 1, 2],
            reference_area_m2: 0.5,
            column: 0,
        }],
    )
    .unwrap()
}

#[test]
fn moving_surface_keeps_reference_mass_and_replays_temperature_laws() {
    let s = surface(&selection());
    let x = [
        Vec3::ZERO,
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ];
    let initial = s.initial(10.0).unwrap();
    let step = s.advance(&initial, &x, 5.0, &[boundary()]).unwrap();
    let t = step.state.temperature_c()[0][0];
    let expected = 0.5 * (200.0 * (t - 10.0) + 2.0 * (t * t - 100.0));
    near(step.stored_change_j, expected, 1e-8);
    near(step.balance_error_j, 0.0, 1e-7);
    assert_eq!(step.area_m2, 1.0);
    let restored = Surface::from_snapshot(&s.snapshot()).unwrap();
    assert_eq!(restored.snapshot(), s.snapshot());
    let replay = restored
        .advance(&restored.initial(10.0).unwrap(), &x, 5.0, &[boundary()])
        .unwrap();
    assert_eq!(step.state.temperature_c(), replay.state.temperature_c());
    let mut changed = s.snapshot();
    changed["columns"][0]["layers"][0]["authored"]["thermal_response"]["specific_heat"]["points"]
        [0]["mean"] = json!(999.0);
    assert!(Surface::from_snapshot(&changed).is_err());
}

#[test]
fn material_only_substitution_recomputes_transport_and_invalidates_surface_state() {
    let a = selection();
    let mut b = a.clone();
    for p in &mut b
        .record
        .as_mut()
        .unwrap()
        .thermal_response
        .as_mut()
        .unwrap()
        .conductivity
        .as_mut()
        .unwrap()
        .points
    {
        p.mean *= 2.0;
    }
    let s = surface(&a);
    let u = surface(&b);
    let x = [
        Vec3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ];
    assert_ne!(s.snapshot()["sha256"], u.snapshot()["sha256"]);
    assert!(
        u.steady(&x, &[boundary()]).unwrap().inside_heat_w
            > s.steady(&x, &[boundary()]).unwrap().inside_heat_w
    );
    assert!(
        u.advance(&s.initial(10.0).unwrap(), &x, 1.0, &[boundary()])
            .is_err()
    );
}

#[test]
fn domain_evidence_conditioning_and_nonfinite_inputs_are_refused_atomically() {
    let s = selection();
    let m = model(&s, 1);
    assert_eq!(m.temperature_domain(), Some([0.0, 60.0]));
    for t in [-1.0, 61.0, f64::NAN, f64::INFINITY] {
        assert!(m.advance(&[t], 1.0, boundary()).is_err());
        assert!(
            m.steady(Boundary {
                inside_c: t,
                ..boundary()
            })
            .is_err()
        );
        assert!(surface(&s).initial(t).is_err());
    }
    let old = [10.0];
    for dt in [0.0, f64::NAN, f64::INFINITY] {
        assert!(m.advance(&old, dt, boundary()).is_err());
    }
    assert_eq!(old, [10.0]);
    assert!(m.advance(&[], 1.0, boundary()).is_err());
    let db = Catalog::builtin().unwrap();
    let mut denied = s.clone();
    denied.allow_estimates = false;
    assert!(DryThermalModel::new(vec![db.resolve(&denied).unwrap()], vec![], 1).is_err());
    denied = s;
    denied
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thickness")
        .unwrap()
        .conditions
        .insert("conditioning".into(), "wet".into());
    assert!(DryThermalModel::new(vec![db.resolve(&denied).unwrap()], vec![], 1).is_err());
}

#[test]
fn characterized_domains_intersect_across_layers_and_uniform_states_stay_exact() {
    let a = selection();
    let mut b = a.clone();
    let r = b
        .record
        .as_mut()
        .unwrap()
        .thermal_response
        .as_mut()
        .unwrap();
    r.specific_heat.as_mut().unwrap().points[0].temperature_c = 15.0;
    let db = Catalog::builtin().unwrap();
    let m = DryThermalModel::new(
        vec![db.resolve(&a).unwrap(), db.resolve(&b).unwrap()],
        vec![0.0],
        2,
    )
    .unwrap();
    assert_eq!(m.temperature_domain(), Some([15.0, 60.0]));
    let boundary = Boundary {
        inside_c: 20.0,
        outside_c: 20.0,
        ..boundary()
    };
    let result = m.advance(&[20.0; 4], 5.0, boundary).unwrap();
    assert_eq!(result.temperature_c, [20.0; 4]);
    assert_eq!(result.stored_change_j_m2, 0.0);
    assert_eq!(result.inside_flux_w_m2, 0.0);
    assert!(m.advance(&[10.0; 4], 5.0, boundary).is_err());
}

#[test]
fn polynomial_enthalpy_drives_the_existing_solver_and_source_locked_surface() {
    use ftx_pattern::material_thermal::PolynomialFit;
    let mut selection = selection();
    let cp = selection
        .record
        .as_mut()
        .unwrap()
        .thermal_response
        .as_mut()
        .unwrap()
        .specific_heat
        .as_mut()
        .unwrap();
    cp.points.clear();
    cp.polynomial = Some(PolynomialFit {
        coefficients: [1000.0, 20.0, 2.0, 0.1],
        domain_c: [0.0, 60.0],
    });
    let model = model(&selection, 1);
    let step = model.advance(&[10.0], 5.0, boundary()).unwrap();
    // Independent quartic primitive and scalar balance; no use of ThermalLaw or solver integration.
    let h = |t: f64| 200.0 * t + 2.0 * t * t + (0.4 / 3.0) * t * t * t + 0.005 * t * t * t * t;
    let residual = |t: f64| h(t) - h(10.0) - 5.0 * ((30.0 - t) / 0.3 - t / 0.4);
    let (mut lo, mut hi) = (10.0, 30.0);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if residual(mid) > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    near(step.temperature_c[0], (lo + hi) / 2.0, 1e-9);
    near(
        step.stored_change_j_m2,
        h(step.temperature_c[0]) - h(10.0),
        1e-8,
    );
    near(step.balance_error_j_m2, 0.0, 1e-7);
    assert!(
        model.snapshot()["storage"]
            .as_str()
            .unwrap()
            .contains("polynomial")
    );
    let s = surface(&selection);
    let restored = Surface::from_snapshot(&s.snapshot()).unwrap();
    assert_eq!(s.snapshot(), restored.snapshot());
    let x = [
        Vec3::ZERO,
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ];
    let step = restored
        .advance(&restored.initial(10.0).unwrap(), &x, 2.5, &[boundary()])
        .unwrap();
    near(step.state.temperature_c()[0][0], (lo + hi) / 2.0, 1e-9);
    near(
        step.stored_change_j,
        0.5 * (h(step.state.temperature_c()[0][0]) - h(10.0)),
        1e-8,
    );
    let mut changed = s.snapshot();
    changed["columns"][0]["layers"][0]["authored"]["thermal_response"]["specific_heat"]["polynomial"]
        ["coefficients"][0] = json!(999.0);
    assert!(Surface::from_snapshot(&changed).is_err());
}
