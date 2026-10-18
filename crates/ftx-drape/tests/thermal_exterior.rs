//! Independent manufactured boundary answers, not measured fabrics or wearer safety predictions.
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    steady::{
        SteadyConductionModel,
        exterior::{Exterior, STEFAN_BOLTZMANN},
    },
};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde_json::{Value, json};

fn record(r: f64, eps: f64) -> Selection {
    let conditions = json!({"specimen":"synthetic-coupon","conditioning":"synthetic-dry"});
    let q = |value, unit| json!({"value":value,"unit":unit,"source":"test","evidence":"estimate","method":"Synthetic independent known answer","conditions":conditions});
    let mut s = json!({"id":"coupon","allow_estimates":true,"record":{
        "id":"coupon","name":"Synthetic sheet, NOT measured material","form":"woven",
        "properties":{"thermal_resistance":q(r,"m2*K/W"),"emissivity":q(eps,"1")},
        "sources":{"test":{"reference":"test:exterior","accessed_on":"2026-10-15","note":"Synthetic test data only"}}
    }});
    s["record"]["properties"]["emissivity"]["conditions"] = json!({
        "specimen":"synthetic-coupon","conditioning":"synthetic-dry","face":"public",
        "spectral_basis":"total_hemispherical","temperature_min_c":"-200","temperature_max_c":"200"});
    serde_json::from_value(s).unwrap()
}
fn environment(h: f64, air: f64, radiant: Option<f64>) -> Exterior {
    serde_json::from_value(json!({
        "air_c":air,"convection_w_m2_k":h,"surface_temperature_range_c":[-200.,200.],
        "convection_source":{"reference":"test:boundary","accessed_on":"2026-10-15","note":"Synthetic coefficient, NOT measured convection"},
        "convection_evidence":"estimate","convection_conditions":"Synthetic prescribed convection for an analytic answer",
        "allow_estimates":true,"radiation":radiant.map(|t| json!({"ambient_c":t,"emissivity_property":"emissivity","exposed_face":"public","allow_opaque_gray_approximation":true}))
    })).unwrap()
}
fn build(s: &[Selection], interfaces: Vec<f64>) -> SteadyConductionModel {
    let db = Catalog::builtin().unwrap();
    SteadyConductionModel::new(
        s.iter().map(|s| db.resolve(s).unwrap()).collect(),
        interfaces,
    )
    .unwrap()
}
fn boundary(inside: f64, air: f64, r: f64) -> Boundary {
    Boundary {
        inside_c: inside,
        outside_c: air,
        inside_resistance: r,
        outside_resistance: 0.,
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8 * (1. + b.abs()), "{a} != {b}");
}
fn fourth(t: f64) -> f64 {
    let t = t + 273.15;
    t * t * t * t
}

#[test]
fn convection_reproduces_existing_films_and_missing_storage_stays_missing() {
    let s = record(0.4, 0.8);
    let m = Catalog::builtin().unwrap().resolve(&s).unwrap();
    assert!(m.mechanics().is_err());
    assert!(DryThermalModel::new(vec![m], vec![], 1).is_err());
    let model = build(&[s], vec![]);
    for (a, b) in [(30., 0.), (0., 30.), (20., 20.)] {
        let old = model
            .solve(Boundary {
                outside_resistance: 0.2,
                ..boundary(a, b, 0.1)
            })
            .unwrap();
        let new = model
            .solve_exterior(boundary(a, b, 0.1), &environment(5., b, None))
            .unwrap();
        near(new.conduction.flux_w_m2, (a - b) / 0.7);
        near(new.conduction.flux_w_m2, old.flux_w_m2);
        near(new.surface_c, b + old.flux_w_m2 * 0.2);
        assert_eq!(new.radiative_flux_w_m2, 0.);
    }
}

#[test]
fn manufactured_radiation_and_mixed_reversed_flows_satisfy_original_equations() {
    for (ts, air, radiant, h, eps) in [
        (26.85, 6.85, 6.85, 0., 1.),
        (20., 30., 0., 4., 0.8),
        (15., 30., 35., 7., 0.9),
    ] {
        let radiation = eps * STEFAN_BOLTZMANN * (fourth(ts) - fourth(radiant));
        let convection = h * (ts - air);
        let q = radiation + convection;
        let inside = ts + q * 0.6;
        let model = build(&[record(0.1, 0.2), record(0.3, eps)], vec![0.1]);
        let result = model
            .solve_exterior(
                boundary(inside, air, 0.1),
                &environment(h, air, Some(radiant)),
            )
            .unwrap();
        near(result.surface_c, ts);
        near(result.conduction.flux_w_m2, q);
        near(result.radiative_flux_w_m2, radiation);
        near(result.convective_flux_w_m2, convection);
        near(result.conduction.layer_faces_c[0][0], inside - q * 0.1);
        near(
            result.conduction.layer_faces_c[0][1] - result.conduction.layer_faces_c[1][0],
            q * 0.1,
        );
        near(result.balance_residual_w_m2, 0.);
    }
}

#[test]
fn finish_only_substitution_changes_exchange_and_frozen_replay() {
    let b = boundary(35., 5., 0.1);
    let e = environment(3., 5., Some(-10.));
    let mut powers = Vec::new();
    let mut hashes = Vec::new();
    for eps in [0., 0.05, 0.8, 1.] {
        let model = build(&[record(0.4, eps)], vec![]);
        let snapshot = model.snapshot();
        let result = model.solve_exterior(b, &e).unwrap();
        let replay = SteadyConductionModel::from_snapshot(&snapshot)
            .unwrap()
            .solve_exterior(b, &e)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::to_value(replay).unwrap()
        );
        powers.push(result.conduction.flux_w_m2);
        hashes.push(snapshot["layers"][0]["expected_sha256"].clone());
    }
    assert!(powers.windows(2).all(|v| v[1] > v[0]));
    assert!(hashes.windows(2).all(|v| v[1] != v[0]));
}

fn variable(domain: [f64; 2], k: [f64; 2]) -> Selection {
    let mut v: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    let s = &mut v["layers"][0];
    s["record"]["thermal_response"]["conductivity"]["points"] = json!([
        {"temperature_c":domain[0],"mean":k[0]}, {"temperature_c":domain[1],"mean":k[1]}
    ]);
    s["record"]["thermal_response"]
        .as_object_mut()
        .unwrap()
        .remove("specific_heat");
    serde_json::from_value(s.clone()).unwrap()
}

#[test]
fn nonlinear_conductivity_and_separated_material_domains_are_not_mean_temperature_approximations() {
    // k(T)=0.01+0.0001*T; integral from 10 to 40 divided by 1 mm is 375 W/m².
    let model = build(&[variable([0., 60.], [0.01, 0.016])], vec![]);
    let result = model
        .solve_exterior(boundary(40., 0., 0.), &environment(37.5, 0., None))
        .unwrap();
    near(result.surface_c, 10.);
    near(result.conduction.flux_w_m2, 375.);
    // Valid temperatures occupy distinct layer domains connected by an explicit contact jump.
    let model = build(
        &[
            variable([40., 60.], [0.005, 0.005]),
            variable([0., 20.], [0.005, 0.005]),
        ],
        vec![0.2],
    );
    let result = model
        .solve_exterior(boundary(70., -10., 0.1), &environment(10., -10., None))
        .unwrap();
    near(result.conduction.flux_w_m2, 100.);
    for (got, want) in result
        .conduction
        .layer_faces_c
        .iter()
        .zip([[60., 40.], [20., 0.]])
    {
        near(got[0], want[0]);
        near(got[1], want[1]);
    }
    assert!(
        model
            .solve_exterior(boundary(70., -10., 0.), &environment(10., -10., None))
            .is_err()
    );
}

#[test]
fn adiabatic_and_balanced_opposing_boundary_terms_do_not_create_heat() {
    let model = build(&[record(0.4, 0.8)], vec![]);
    let result = model
        .solve_exterior(boundary(20., -100., 0.1), &environment(0., -100., None))
        .unwrap();
    assert_eq!(result.surface_c, 20.);
    assert_eq!(result.conduction.flux_w_m2, 0.);
    // Explicit opposing surface fluxes cancel; source temperatures need not coincide.
    let ts = 20.;
    let tr = 0.;
    let ta = 40.;
    let a = ts + 273.15;
    let b = tr + 273.15;
    let rad = 0.8 * STEFAN_BOLTZMANN * (ts - tr) * (a + b) * (a * a + b * b);
    let result = model
        .solve_exterior(
            boundary(ts, ta, 0.),
            &environment(rad / (ta - ts), ta, Some(tr)),
        )
        .unwrap();
    near(result.surface_c, ts);
    near(result.conduction.flux_w_m2, 0.);
    assert!(result.convective_flux_w_m2 < 0.);
    assert!(result.radiative_flux_w_m2 > 0.);
}

#[test]
fn missing_spectral_face_conditioning_domains_and_evidence_refuse_without_affecting_convection() {
    let b = boundary(30., 0., 0.1);
    let e = environment(5., 0., Some(0.));
    for (key, value) in [
        ("face", "reverse"),
        ("spectral_basis", "8-14um camera band"),
        ("specimen", "different lot"),
        ("conditioning", "wet"),
        ("temperature_min_c", "bad"),
    ] {
        let mut s = record(0.4, 0.8);
        s.record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("emissivity")
            .unwrap()
            .conditions
            .insert(key.into(), value.into());
        assert!(build(&[s], vec![]).solve_exterior(b, &e).is_err(), "{key}");
    }
    let mut s = record(0.4, 0.8);
    s.record.as_mut().unwrap().properties.remove("emissivity");
    let model = build(&[s], vec![]);
    assert!(model.solve_exterior(b, &e).is_err());
    assert!(model.solve_exterior(b, &environment(5., 0., None)).is_ok());
    let model = build(&[record(0.4, 0.8)], vec![]);
    let mut denied = e.clone();
    denied.allow_estimates = false;
    assert!(model.solve_exterior(b, &denied).is_err());
    let mut denied = e.clone();
    denied
        .radiation
        .as_mut()
        .unwrap()
        .allow_opaque_gray_approximation = false;
    assert!(model.solve_exterior(b, &denied).is_err());
    let mut denied = e.clone();
    denied.surface_temperature_range_c = [20., 30.];
    assert!(model.solve_exterior(b, &denied).is_err());
    let mut denied = e.clone();
    denied.radiation.as_mut().unwrap().ambient_c = -250.;
    assert!(model.solve_exterior(b, &denied).is_err());
    assert!(
        model
            .solve_exterior(
                Boundary {
                    outside_resistance: 0.2,
                    ..b
                },
                &e
            )
            .is_err()
    );
    assert!(
        model
            .solve_exterior(Boundary { outside_c: 1., ..b }, &e)
            .is_err()
    );
    for h in [-1., f64::NAN, f64::INFINITY] {
        let mut bad = e.clone();
        bad.convection_w_m2_k = h;
        assert!(model.solve_exterior(b, &bad).is_err());
    }
    for eps in [-0.1, 1.1] {
        assert!(
            build(&[record(0.4, eps)], vec![])
                .solve_exterior(b, &e)
                .is_err()
        );
    }
}

#[test]
fn surface_exchange_uses_current_areas_and_binds_the_outermost_layer() {
    use ftx_core::Vec3;
    use ftx_drape::thermal::{steady::surface::SteadySurface, surface::Patch};
    let make = |outer_eps| {
        SteadySurface::new(
            vec![build(
                &[record(0.1, 0.01), record(0.3, outer_eps)],
                vec![0.1],
            )],
            3,
            vec![Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            }],
        )
        .unwrap()
    };
    let s = make(0.8);
    let b = [boundary(35., 5., 0.1)];
    let e = [environment(3., 5., Some(-10.))];
    let x = [Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)];
    let first = s.solve_exterior(&x, &b, &e).unwrap();
    let mut larger = x;
    larger[1].x = 2.;
    let doubled = s.solve_exterior(&larger, &b, &e).unwrap();
    near(doubled.steady.power_w, first.steady.power_w * 2.);
    near(doubled.convective_power_w, first.convective_power_w * 2.);
    near(doubled.radiative_power_w, first.radiative_power_w * 2.);
    near(
        first.steady.power_w,
        first.convective_power_w + first.radiative_power_w,
    );
    let moved = x.map(|v| Vec3::new(v.z, v.x, v.y) + Vec3::new(0.4, 0.5, 0.6));
    near(
        s.solve_exterior(&moved, &b, &e).unwrap().steady.power_w,
        first.steady.power_w,
    );
    assert!(
        make(0.05)
            .solve_exterior(&x, &b, &e)
            .unwrap()
            .steady
            .power_w
            < first.steady.power_w
    );
    assert!(s.solve_exterior(&x, &b, &[]).is_err());
    assert!(s.solve_exterior(&x, &[], &e).is_err());
    assert!(s.solve_exterior(&[Vec3::ZERO; 3], &b, &e).is_err());
}

#[test]
fn explicit_boundary_estimate_permission_does_not_relax_material_evidence() {
    use ftx_pattern::materials::Evidence;
    let mut s = record(0.4, 0.8);
    s.allow_estimates = false;
    s.record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thermal_resistance")
        .unwrap()
        .evidence = Evidence::Published;
    let model = build(&[s], vec![]);
    assert!(
        model
            .solve_exterior(boundary(35., 5., 0.1), &environment(3., 5., Some(-10.)))
            .is_err()
    );
    assert!(
        model
            .solve_exterior(boundary(35., 5., 0.1), &environment(3., 5., None))
            .is_ok()
    );
}

fn storage_record(r: f64, eps: f64) -> Selection {
    let mut s = record(r, eps);
    let properties = &mut s.record.as_mut().unwrap().properties;
    let mut mass = properties["thermal_resistance"].clone();
    mass.value = 0.2;
    mass.unit = "kg/m2".into();
    properties.insert("areal_density".into(), mass.clone());
    mass.value = 1000.;
    mass.unit = "J/(kg*K)".into();
    properties.insert("specific_heat".into(), mass);
    s
}

fn storage(s: &[Selection], interfaces: Vec<f64>, cells: usize) -> DryThermalModel {
    let db = Catalog::builtin().unwrap();
    DryThermalModel::new(
        s.iter().map(|s| db.resolve(s).unwrap()).collect(),
        interfaces,
        cells,
    )
    .unwrap()
}

fn exterior_balance(exchange: &ftx_drape::thermal::exterior::ExteriorFlux, outgoing: f64) {
    let scale = 1.
        + outgoing.abs()
        + exchange.convective_flux_w_m2.abs()
        + exchange.radiative_flux_w_m2.abs();
    assert!(exchange.balance_residual_w_m2.abs() <= 1e-10 * scale);
    near(
        outgoing,
        exchange.convective_flux_w_m2 + exchange.radiative_flux_w_m2,
    );
}

#[test]
fn transient_convection_is_exactly_the_existing_film_step() {
    let model = storage(&[storage_record(0.4, 0.8)], vec![], 4);
    for (inside, air, initial) in [(30., 0., 10.), (0., 30., 20.), (20., 20., 20.)] {
        let b = boundary(inside, air, 0.1);
        let old = vec![initial; model.cells()];
        let film = model
            .advance(
                &old,
                3.,
                Boundary {
                    outside_resistance: 0.2,
                    ..b
                },
            )
            .unwrap();
        let exterior = model
            .advance_exterior(&old, 3., b, &environment(5., air, None))
            .unwrap();
        assert_eq!(exterior.state.temperature_c, film.temperature_c);
        assert_eq!(exterior.state.stored_change_j_m2, film.stored_change_j_m2);
        assert_eq!(exterior.state.outside_flux_w_m2, film.outside_flux_w_m2);
        assert_eq!(exterior.exchange.radiative_flux_w_m2, 0.);
        assert!(!exterior.exchange.radiation_enabled);
        near(
            exterior.exchange.surface_c,
            air + film.outside_flux_w_m2 * 0.2,
        );
        exterior_balance(&exterior.exchange, exterior.state.outside_flux_w_m2);
        let steady = model
            .steady_exterior(b, &environment(5., air, None))
            .unwrap();
        near(steady.state.outside_flux_w_m2, (inside - air) / 0.7);
    }
}

#[test]
fn manufactured_transient_radiation_preserves_signed_fluxes_and_storage() {
    // Manufacture Ts and independent q_in, then recover the old temperature from the single-cell
    // backward-Euler energy equation. Radiation is evaluated independently using fourth powers.
    for (ts, air, radiant, h, eps, q_in) in [
        (20., 30., 0., 4., 0.8, 5.),
        (15., 30., 35., 7., 0.9, 60.),
        (26.85, 6.85, 6.85, 0., 1., 0.),
    ] {
        let radiation = eps * STEFAN_BOLTZMANN * (fourth(ts) - fourth(radiant));
        let convection = h * (ts - air);
        let q_out = convection + radiation;
        let next = ts + 0.2 * q_out;
        let inside = next + 0.3 * q_in;
        let old = [next - 2. * (q_in - q_out) / 200.];
        let original = old;
        let result = storage(&[storage_record(0.4, eps)], vec![], 1)
            .advance_exterior(
                &old,
                2.,
                boundary(inside, air, 0.1),
                &environment(h, air, Some(radiant)),
            )
            .unwrap();
        near(result.exchange.surface_c, ts);
        near(result.state.temperature_c[0], next);
        near(result.state.inside_flux_w_m2, q_in);
        near(result.state.outside_flux_w_m2, q_out);
        near(result.exchange.convective_flux_w_m2, convection);
        near(result.exchange.radiative_flux_w_m2, radiation);
        near(result.state.stored_change_j_m2, 200. * (next - old[0]));
        near(result.state.balance_error_j_m2, 0.);
        exterior_balance(&result.exchange, result.state.outside_flux_w_m2);
        assert_eq!(old, original);
    }
}

#[test]
fn transient_face_domain_applies_to_material_and_enthalpy_not_air() {
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    let mut optical =
        serde_json::to_value(record(0.4, 0.8)).unwrap()["record"]["properties"]["emissivity"]
            .clone();
    optical["conditions"]["specimen"] = json!("test:temperature-sheet");
    optical["conditions"]["temperature_min_c"] = json!("0");
    optical["conditions"]["temperature_max_c"] = json!("60");
    input["layers"][0]["record"]["properties"]["emissivity"] = optical;
    let selection: Selection = serde_json::from_value(input["layers"][0].clone()).unwrap();
    let model = storage(&[selection], vec![], 4);
    let old = vec![20.; 4];
    let b = boundary(35., -10., 0.1);
    let e = environment(3., -10., Some(20.));
    assert!(model.advance(&old, 1., b).is_err());
    let step = model.advance_exterior(&old, 1., b, &e).unwrap();
    assert!((0. ..=60.).contains(&step.exchange.surface_c));
    // Cp(T)=1000+20*T and mass=0.2 kg/m²; integrate its polynomial independently per cell.
    let enthalpy = step
        .state
        .temperature_c
        .iter()
        .map(|&t| 0.2 / 4. * (1000. * (t - 20.) + 10. * (t * t - 400.)))
        .sum::<f64>();
    near(step.state.stored_change_j_m2, enthalpy);
    near(
        enthalpy,
        step.state.inside_flux_w_m2 - step.state.outside_flux_w_m2,
    );
    exterior_balance(&step.exchange, step.state.outside_flux_w_m2);
    assert!(
        model
            .advance_exterior(&old, 1., b, &environment(3., -10., Some(-1.)))
            .is_err()
    );
    let mut denied = e;
    denied.allow_estimates = false;
    assert!(model.advance_exterior(&old, 1., b, &denied).is_err());
    assert_eq!(old, vec![20.; 4]);
}

#[test]
fn transient_hot_initial_state_and_insulated_face_are_not_reservoir_clamped() {
    let model = storage(&[storage_record(0.4, 0.8)], vec![], 2);
    let b = boundary(20., 0., 0.1);
    let e = environment(2., 0., Some(5.));
    let old = [100.; 2];
    let next = model.advance_exterior(&old, 0.01, b, &e).unwrap();
    assert!(next.exchange.surface_c > 20.);
    assert!(
        next.state
            .temperature_c
            .iter()
            .all(|&t| t < 100. && t > 20.)
    );
    near(next.state.balance_error_j_m2, 0.);
    exterior_balance(&next.exchange, next.state.outside_flux_w_m2);

    let insulated = storage(&[storage_record(0.4, 0.)], vec![], 1);
    let steady = insulated
        .advance_exterior(
            &[10.],
            1.,
            boundary(10., -100., 0.1),
            &environment(0., -100., Some(100.)),
        )
        .unwrap();
    near(steady.exchange.surface_c, 10.);
    near(steady.state.temperature_c[0], 10.);
    near(steady.state.outside_flux_w_m2, 0.);
    near(steady.state.stored_change_j_m2, 0.);
    assert!(steady.exchange.radiation_enabled);

    assert!(model.advance_exterior(&old, 0., b, &e).is_err());
    assert!(model.advance_exterior(&[10.], 1., b, &e).is_err());
    assert!(model.advance_exterior(&[10., f64::NAN], 1., b, &e).is_err());
    assert!(
        model
            .advance_exterior(
                &old,
                1.,
                Boundary {
                    outside_resistance: 0.1,
                    ..b
                },
                &e
            )
            .is_err()
    );
    let mut unqualified = e;
    unqualified.surface_temperature_range_c = [150., 200.];
    assert!(model.advance_exterior(&old, 1., b, &unqualified).is_err());
    assert_eq!(old, [100.; 2]);
}

#[test]
fn transient_surface_radiation_uses_reference_mass_current_area_and_new_material_identity() {
    use ftx_core::Vec3;
    use ftx_drape::thermal::surface::{Patch, Surface};
    let make = |inner_eps, outer_eps| {
        Surface::new(
            vec![storage(
                &[
                    storage_record(0.1, inner_eps),
                    storage_record(0.3, outer_eps),
                ],
                vec![0.1],
                2,
            )],
            3,
            vec![Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            }],
        )
        .unwrap()
    };
    let model = make(0.01, 0.8);
    let positions = [Vec3::ZERO, Vec3::new(2., 0., 0.), Vec3::new(0., 1., 0.)];
    let old = model.initial(30.).unwrap();
    let original = old.temperature_c().to_vec();
    let b = [boundary(35., 5., 0.1)];
    let e = [environment(3., 5., Some(-10.))];
    let step = model
        .advance_exterior(&old, &positions, 2., &b, &e)
        .unwrap();
    let field = step.exterior_exchange.as_ref().unwrap();
    // Two layers, 2 cells/layer, 200 J/(m² K)/layer, and 0.5 m² reference area.
    let stored = step.state.temperature_c()[0]
        .iter()
        .map(|t| (t - 30.) * 50.)
        .sum::<f64>();
    near(step.area_m2, 1.);
    near(step.stored_change_j, stored);
    near(stored, 2. * (step.inside_heat_w - step.outside_heat_w));
    near(
        step.outside_heat_w,
        field.convective_heat_w + field.radiative_heat_w,
    );
    assert!(
        field.balance_residual_w.abs()
            <= 1e-10
                * (1.
                    + step.outside_heat_w.abs()
                    + field.convective_heat_w.abs()
                    + field.radiative_heat_w.abs())
    );
    exterior_balance(&field.patches[0], step.patches[0].outside_flux_w_m2);
    near(step.balance_error_j, 0.);
    let column = storage(
        &[storage_record(0.1, 0.01), storage_record(0.3, 0.8)],
        vec![0.1],
        2,
    )
    .advance_exterior(&[30.; 4], 4., b[0], &e[0])
    .unwrap();
    assert_eq!(step.state.temperature_c()[0], column.state.temperature_c);
    near(
        field.convective_heat_w,
        column.exchange.convective_flux_w_m2,
    );
    near(field.radiative_heat_w, column.exchange.radiative_flux_w_m2);
    let inside_changed = make(0.9, 0.8);
    let unchanged_exchange = inside_changed
        .advance_exterior(
            &inside_changed.initial(30.).unwrap(),
            &positions,
            2.,
            &b,
            &e,
        )
        .unwrap();
    assert_eq!(
        step.state.temperature_c(),
        unchanged_exchange.state.temperature_c()
    );
    let reflective = make(0.01, 0.05);
    assert!(
        reflective
            .advance_exterior(&old, &positions, 2., &b, &e)
            .is_err()
    );
    let changed = reflective
        .advance_exterior(&reflective.initial(30.).unwrap(), &positions, 2., &b, &e)
        .unwrap();
    assert!(changed.outside_heat_w < step.outside_heat_w);
    assert!(
        model
            .advance_exterior(&old, &positions, 2., &b, &[])
            .is_err()
    );
    assert!(
        model
            .advance_exterior(&old, &[Vec3::ZERO; 3], 2., &b, &e)
            .is_err()
    );
    assert_eq!(old.temperature_c(), original);

    // An environment change continues the retained material temperatures; it does not reinitialize.
    let warm_b = [boundary(35., 25., 0.1)];
    let warm_e = [environment(1., 25., Some(30.))];
    let next = model
        .advance_exterior(&step.state, &positions, 2., &warm_b, &warm_e)
        .unwrap();
    let reset = model
        .advance_exterior(&old, &positions, 2., &warm_b, &warm_e)
        .unwrap();
    assert_ne!(next.state.temperature_c(), reset.state.temperature_c());
    near(next.balance_error_j, 0.);
    let restored = Surface::from_snapshot(&model.snapshot()).unwrap();
    let replay = restored
        .advance_exterior(&restored.initial(30.).unwrap(), &positions, 2., &b, &e)
        .unwrap();
    assert_eq!(step.state.temperature_c(), replay.state.temperature_c());
}
