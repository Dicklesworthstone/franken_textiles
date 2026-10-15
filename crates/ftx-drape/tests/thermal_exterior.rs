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
