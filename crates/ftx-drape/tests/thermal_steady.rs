//! Integrated steady heat-flow answers. Synthetic constructions never establish garment accuracy.
use ftx_drape::thermal::{Boundary, DryThermalModel, steady::SteadyConductionModel};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde_json::{Value, json};

fn selection() -> Selection {
    let input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    let mut s: Selection = serde_json::from_value(input["layers"][0].clone()).unwrap();
    let m = s.record.as_mut().unwrap();
    m.properties.remove("areal_density");
    m.thermal_response.as_mut().unwrap().specific_heat = None;
    s
}
fn constant(r: f64, domain: [f64; 2]) -> Selection {
    let mut s = selection();
    let series = s
        .record
        .as_mut()
        .unwrap()
        .thermal_response
        .as_mut()
        .unwrap()
        .conductivity
        .as_mut()
        .unwrap();
    for (point, t) in series.points.iter_mut().zip(domain) {
        point.temperature_c = t;
        point.mean = 0.001 / r;
    }
    s
}
fn build(selections: &[Selection], interfaces: Vec<f64>) -> SteadyConductionModel {
    let db = Catalog::builtin().unwrap();
    SteadyConductionModel::new(
        selections.iter().map(|s| db.resolve(s).unwrap()).collect(),
        interfaces,
    )
    .unwrap()
}
fn boundary(
    inside_c: f64,
    outside_c: f64,
    inside_resistance: f64,
    outside_resistance: f64,
) -> Boundary {
    Boundary {
        inside_c,
        outside_c,
        inside_resistance,
        outside_resistance,
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8 * (1.0 + b.abs()), "{a} != {b}");
}

#[test]
fn constant_stack_contacts_and_films_match_resistance_sum_and_reverse() {
    let model = build(
        &[constant(0.2, [0.0, 100.0]), constant(0.4, [0.0, 100.0])],
        vec![0.1],
    );
    let b = boundary(90.0, 0.0, 0.05, 0.15);
    let state = model.solve(b).unwrap();
    near(state.flux_w_m2, 100.0);
    for (got, expected) in state.layer_faces_c.iter().zip([[85.0, 65.0], [55.0, 15.0]]) {
        near(got[0], expected[0]);
        near(got[1], expected[1]);
    }
    near(state.max_flux_residual_w_m2, 0.0);
    near(state.max_temperature_residual_c, 0.0);
    let opposite = build(
        &[constant(0.4, [0.0, 100.0]), constant(0.2, [0.0, 100.0])],
        vec![0.1],
    )
    .solve(boundary(0.0, 90.0, 0.15, 0.05))
    .unwrap();
    assert_eq!(state.flux_w_m2, -opposite.flux_w_m2);
    for (forward, back) in state
        .layer_faces_c
        .iter()
        .zip(opposite.layer_faces_c.iter().rev())
    {
        assert_eq!(*forward, [back[1], back[0]]);
    }
}

#[test]
fn films_and_contacts_allow_distinct_local_domains_without_extrapolation() {
    let model = build(
        &[constant(0.2, [40.0, 60.0]), constant(0.2, [0.0, 20.0])],
        vec![0.2],
    );
    let state = model.solve(boundary(70.0, -10.0, 0.1, 0.1)).unwrap();
    near(state.flux_w_m2, 100.0);
    for (got, expected) in state.layer_faces_c.iter().zip([[60.0, 40.0], [20.0, 0.0]]) {
        near(got[0], expected[0]);
        near(got[1], expected[1]);
    }
    // The reservoir temperatures are deliberately outside both local material domains.
    assert!(model.solve(boundary(70.0, -10.0, 0.0, 0.0)).is_err());
    assert!(
        build(
            &[constant(0.2, [40.0, 60.0]), constant(0.2, [0.0, 20.0])],
            vec![0.0]
        )
        .solve(boundary(70.0, -10.0, 0.1, 0.1))
        .is_err()
    );
}

#[test]
fn integrates_nonconstant_cubic_and_layer_interface_temperature_exactly() {
    let mut s = selection();
    let series = s
        .record
        .as_mut()
        .unwrap()
        .thermal_response
        .as_mut()
        .unwrap()
        .conductivity
        .as_mut()
        .unwrap();
    series.points.clear();
    series.polynomial = Some(
        serde_json::from_value(json!({
            "coefficients":[0.01,0.0,0.00001,0.0],"domain_c":[0.0,60.0]
        }))
        .unwrap(),
    );
    let whole = build(&[s.clone()], vec![])
        .solve(boundary(60.0, 0.0, 0.0, 0.0))
        .unwrap();
    near(whole.flux_w_m2, 1320.0);
    // Split the same homogeneous slab into two half-thickness material records, no fake mass.
    s.record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thickness")
        .unwrap()
        .value = 0.0005;
    let split = build(&[s.clone(), s], vec![0.0])
        .solve(boundary(60.0, 0.0, 0.0, 0.0))
        .unwrap();
    near(split.flux_w_m2, whole.flux_w_m2);
    let t = split.layer_faces_c[0][1];
    near(0.01 * t + 0.00001 * t * t * t / 3.0, 0.66);
    assert_eq!(t, split.layer_faces_c[1][0]);
    assert!((t - 30.0).abs() > 5.0);
}

#[test]
fn tabulated_slope_changes_satisfy_independent_multisegment_integral() {
    let mut s = selection();
    let series = s
        .record
        .as_mut()
        .unwrap()
        .thermal_response
        .as_mut()
        .unwrap()
        .conductivity
        .as_mut()
        .unwrap();
    series.points = serde_json::from_value(json!([
        {"temperature_c":0.0,"mean":0.01},
        {"temperature_c":20.0,"mean":0.05},
        {"temperature_c":60.0,"mean":0.02}
    ]))
    .unwrap();
    let model = build(&[s], vec![]);
    near(
        model
            .solve(boundary(60.0, 0.0, 0.0, 0.0))
            .unwrap()
            .flux_w_m2,
        (20.0 * (0.01 + 0.05) / 2.0 + 40.0 * (0.05 + 0.02) / 2.0) / 0.001,
    );
    let mixed = model.solve(boundary(55.0, 5.0, 0.001, 0.002)).unwrap();
    let [a, b] = mixed.layer_faces_c[0];
    let primitive = |t: f64| {
        if t <= 20.0 {
            0.01 * t + 0.001 * t * t
        } else {
            let d = t - 20.0;
            0.6 + 0.05 * d - 0.000375 * d * d
        }
    };
    near((primitive(a) - primitive(b)) / 0.001, mixed.flux_w_m2);
}

#[test]
fn actual_hfm_substitution_and_portable_replay_preserve_capability_boundaries() {
    let db = Catalog::builtin().unwrap();
    let mut outputs = Vec::new();
    for id in [
        "fsri/hfm/cotton-sheet/dry/220330-r1",
        "fsri/hfm/cotton-sheet/wet/220318-r1",
    ] {
        let resolved = db
            .resolve(&Selection {
                id: id.into(),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap();
        assert!(DryThermalModel::new(vec![resolved.clone()], vec![], 1).is_err());
        assert!(resolved.mechanics().is_err());
        let model = SteadyConductionModel::new(vec![resolved], vec![]).unwrap();
        let b = boundary(60.0, 20.0, 0.01, 0.02);
        let state = model.solve(b).unwrap();
        let snapshot = model.snapshot();
        let replay = SteadyConductionModel::from_snapshot(&snapshot).unwrap();
        assert_eq!(snapshot, replay.snapshot());
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            serde_json::to_value(replay.solve(b).unwrap()).unwrap()
        );
        outputs.push(state.flux_w_m2);
        let mut tampered = snapshot.clone();
        tampered["layers"][0]["record"]["properties"]["thickness"]["value"] = json!(0.9);
        assert!(SteadyConductionModel::from_snapshot(&tampered).is_err());
        tampered = snapshot;
        tampered["layers"][0]
            .as_object_mut()
            .unwrap()
            .remove("record");
        assert!(SteadyConductionModel::from_snapshot(&tampered).is_err());
    }
    assert!((outputs[0] - outputs[1]).abs() > 1.0);
}

#[test]
fn isothermal_requires_valid_material_temperatures_but_not_mass() {
    let model = build(&[selection()], vec![]);
    let state = model.solve(boundary(30.0, 30.0, 0.0, 0.0)).unwrap();
    assert_eq!(state.flux_w_m2, 0.0);
    assert_eq!(state.layer_faces_c, vec![[30.0, 30.0]]);
    assert!(model.solve(boundary(-1.0, -1.0, 1.0, 1.0)).is_err());
}

#[test]
fn malformed_boundaries_layers_and_unresolvable_domains_refuse() {
    let model = build(&[selection()], vec![]);
    for b in [
        boundary(70.0, 0.0, 0.0, 0.0),
        boundary(20.0, -274.0, 0.0, 0.0),
        boundary(30.0, 20.0, -1.0, 0.0),
        boundary(f64::NAN, 20.0, 0.0, 0.0),
        boundary(30.0, 20.0, 0.0, f64::INFINITY),
    ] {
        assert!(model.solve(b).is_err());
    }
    assert!(SteadyConductionModel::new(vec![], vec![]).is_err());
    let layer = Catalog::builtin().unwrap().resolve(&selection()).unwrap();
    assert!(SteadyConductionModel::new(vec![layer.clone()], vec![0.0]).is_err());
    assert!(SteadyConductionModel::new(vec![layer.clone(), layer.clone()], vec![-0.1]).is_err());
    assert!(SteadyConductionModel::new(vec![layer; 257], vec![0.0; 256]).is_err());
    let mut s = selection();
    s.allow_estimates = false;
    let layer = Catalog::builtin().unwrap().resolve(&s).unwrap();
    assert!(SteadyConductionModel::new(vec![layer], vec![]).is_err());
}

#[test]
fn distinct_nonlinear_layers_recover_a_manufactured_flux_at_domain_edges() {
    // Choose faces and q independently, then derive each synthetic thickness from its exact
    // polynomial antiderivative. Films and contacts account for the remaining temperature jumps.
    let faces = [[55.0, 44.0], [42.0, 30.0], [26.0, 19.0]];
    let coefficients = [
        [0.01, 0.0001, 0.00001, 0.0],
        [0.04, -0.0002, 0.000003, 0.00000001],
        [0.02, 0.0002, 0.0, 0.0],
    ];
    let mut selections = Vec::new();
    for ([a, b], c) in faces.into_iter().zip(coefficients) {
        let mut s = selection();
        let m = s.record.as_mut().unwrap();
        let primitive = |t: f64| {
            c[0] * t + c[1] * t * t / 2.0 + c[2] * t * t * t / 3.0 + c[3] * t * t * t * t / 4.0
        };
        m.properties.get_mut("thickness").unwrap().value = (primitive(a) - primitive(b)) / 80.0;
        let series = m
            .thermal_response
            .as_mut()
            .unwrap()
            .conductivity
            .as_mut()
            .unwrap();
        series.points.clear();
        series.polynomial =
            Some(serde_json::from_value(json!({"coefficients":c,"domain_c":[b,a]})).unwrap());
        selections.push(s);
    }
    let model = build(&selections, vec![2.0 / 80.0, 4.0 / 80.0]);
    let solved = model
        .solve(boundary(61.0, 15.0, 6.0 / 80.0, 4.0 / 80.0))
        .unwrap();
    near(solved.flux_w_m2, 80.0);
    for (got, expected) in solved.layer_faces_c.iter().zip(faces) {
        near(got[0], expected[0]);
        near(got[1], expected[1]);
    }
}

#[test]
fn mixed_constant_resistance_and_source_series_work_without_spurious_cp() {
    let mut s = selection();
    let m = s.record.as_mut().unwrap();
    m.thermal_response = None;
    let mut r = m.properties["thickness"].clone();
    m.properties.clear();
    r.value = 0.2;
    r.unit = "m2*K/W".into();
    m.properties.insert("thermal_resistance".into(), r);
    let model = build(&[s, constant(0.3, [0.0, 60.0])], vec![0.1]);
    near(
        model
            .solve(boundary(60.0, 0.0, 0.0, 0.0))
            .unwrap()
            .flux_w_m2,
        100.0,
    );
}
