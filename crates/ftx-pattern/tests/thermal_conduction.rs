//! Transport-only admission and independent Fourier answers; no physical garment validation.
use ftx_pattern::{
    material_catalog::Selection,
    material_thermal::{ConductionLaw, ReportedConductionBounds, ThermalLaw},
    materials::{Catalog, Form, Material},
};
use serde_json::{Value, json};

fn material() -> Material {
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-variable-thermal.json"
    ))
    .unwrap();
    let record = &mut input["layers"][0]["record"];
    record["properties"]
        .as_object_mut()
        .unwrap()
        .remove("areal_density");
    record["thermal_response"]
        .as_object_mut()
        .unwrap()
        .remove("specific_heat");
    serde_json::from_value(record.clone()).unwrap()
}
fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-11 * (1.0 + expected.abs()),
        "{actual} != {expected}"
    );
}

fn resistance(value: f64, bounds: Option<[f64; 2]>) -> Material {
    let mut m = material();
    m.thermal_response = None;
    let mut quantity = m.properties["thickness"].clone();
    quantity.value = value;
    quantity.unit = "m2*K/W".into();
    quantity.bounds = bounds;
    m.properties.clear();
    m.properties.insert("thermal_resistance".into(), quantity);
    m
}

#[test]
fn source_resistance_endpoints_follow_unequal_areas_films_and_signed_flux() {
    let laws = [
        ConductionLaw::from_material(&resistance(0.2, Some([0.1, 0.3])), true).unwrap(),
        ConductionLaw::from_material(&resistance(0.4, Some([0.3, 0.5])), true).unwrap(),
    ];
    let solve = |temperatures| {
        ConductionLaw::reported_stack_bounds(&laws, &[1., 0.5], &[0.1], temperatures, [0.1, 0.2])
            .unwrap()
    };
    let forward = solve([30., 0.]);
    for (actual, expected) in forward
        .layer_resistance_m2_k_w
        .into_iter()
        .zip([0.25, 0.55])
    {
        near(actual, expected);
    }
    for (actual, expected) in forward
        .total_resistance_m2_k_w
        .into_iter()
        .zip([0.65, 0.95])
    {
        near(actual, expected);
    }
    near(forward.flux_w_m2[0], 30. / 0.95);
    near(forward.flux_w_m2[1], 30. / 0.65);
    let reverse = solve([0., 30.]);
    near(reverse.flux_w_m2[0], -forward.flux_w_m2[1]);
    near(reverse.flux_w_m2[1], -forward.flux_w_m2[0]);
    assert_eq!(solve([15., 15.]).flux_w_m2, [0., 0.]);
    let power = forward.power_bounds(0.5).unwrap();
    near(power[0], 15. / 0.95);
    near(power[1], 15. / 0.65);
    let mut combined = power;
    ReportedConductionBounds::add_power_bounds(&mut combined, reverse.power_bounds(0.25).unwrap())
        .unwrap();
    near(combined[0], 15. / 0.95 - 7.5 / 0.65);
    near(combined[1], 15. / 0.65 - 7.5 / 0.95);
}

#[test]
fn ratio_ranges_require_both_selected_observations_and_never_override_direct_resistance() {
    let mut m = material();
    m.thermal_response = None;
    let thickness = m.properties.get_mut("thickness").unwrap();
    thickness.value = 0.002;
    thickness.bounds = Some([0.001, 0.003]);
    let mut conductivity = thickness.clone();
    conductivity.value = 0.04;
    conductivity.unit = "W/(m*K)".into();
    conductivity.bounds = Some([0.02, 0.06]);
    m.properties
        .insert("thermal_conductivity".into(), conductivity);
    let law = ConductionLaw::from_material(&m, true).unwrap();
    let bounds = law.reported_resistance_bounds().unwrap();
    near(bounds[0], 0.001 / 0.06);
    near(bounds[1], 0.003 / 0.02);
    assert!(ConductionLaw::from_material(&m, false).is_err());
    let mut wrong_unit = m.clone();
    wrong_unit.properties.get_mut("thickness").unwrap().unit = "mm".into();
    assert!(ConductionLaw::from_material(&wrong_unit, true).is_err());
    for key in ["thickness", "thermal_conductivity"] {
        let mut missing = m.clone();
        missing.properties.get_mut(key).unwrap().bounds = None;
        let law = ConductionLaw::from_material(&missing, true).unwrap();
        near(law.resistance_at(20.).unwrap(), 0.05);
        assert!(law.reported_resistance_bounds().is_err());
    }
    m.properties.insert(
        "thermal_resistance".into(),
        resistance(0.4, Some([0.3, 0.5])).properties["thermal_resistance"].clone(),
    );
    let direct = ConductionLaw::from_material(&m, true).unwrap();
    assert_eq!(direct.reported_resistance_bounds().unwrap(), [0.3, 0.5]);
    near(direct.resistance_at(20.).unwrap(), 0.4);
    m.properties.get_mut("thermal_resistance").unwrap().bounds = None;
    assert!(
        ConductionLaw::from_material(&m, true)
            .unwrap()
            .reported_resistance_bounds()
            .is_err()
    );
}

#[test]
fn incomplete_nonpositive_and_temperature_series_ranges_stay_unavailable() {
    for bounds in [None, Some([0., 0.5]), Some([-0.1, 0.5])] {
        let law = ConductionLaw::from_material(&resistance(0.4, bounds), true).unwrap();
        near(law.flux_between(30., 0.).unwrap(), 75.);
        assert!(law.reported_resistance_bounds().is_err());
    }
    let mut m = material();
    // Constant-valued series retain their source domain and never acquire scalar endpoints.
    let law = ConductionLaw::from_material(&m, true).unwrap();
    assert_eq!(law.domain(), Some([0., 60.]));
    assert!(law.reported_resistance_bounds().is_err());
    // The existing source authority refuses a conflicting scalar rather than selecting its range.
    m.properties.insert(
        "thermal_resistance".into(),
        resistance(0.4, Some([0.3, 0.5])).properties["thermal_resistance"].clone(),
    );
    assert!(ConductionLaw::from_material(&m, true).is_err());
}

#[test]
fn unrepresentable_range_operations_refuse_without_overwriting_totals() {
    let law = ConductionLaw::from_material(&resistance(0.4, Some([0.1, f64::MAX])), true).unwrap();
    near(law.resistance_at(20.).unwrap(), 0.4);
    assert!(ConductionLaw::reported_stack_bounds(&[law], &[2.], &[], [30., 0.], [0., 0.]).is_err());
    let mut total = [f64::MAX / 2.; 2];
    let before = total;
    assert!(ReportedConductionBounds::add_power_bounds(&mut total, [f64::MAX; 2]).is_err());
    assert_eq!(total, before);
    assert!(ReportedConductionBounds::add_power_bounds(&mut total, [1.; 2]).is_err());
    assert_eq!(total, before);
}

#[test]
fn transport_does_not_fabricate_mass_or_heat_capacity() {
    let m = material();
    let law = ConductionLaw::from_material(&m, true).unwrap();
    near(law.resistance_at(30.0).unwrap(), 0.4);
    near(law.flux_between(30.0, 10.0).unwrap(), 50.0);
    assert_eq!(
        law.flux_between(10.0, 30.0).unwrap(),
        -law.flux_between(30.0, 10.0).unwrap()
    );
    assert_eq!(law.flux_between(20.0, 20.0).unwrap(), 0.0);
    assert!(ThermalLaw::from_material(&m, true).is_err());
    assert!(!m.properties.contains_key("areal_density"));
    assert!(m.thermal_response.as_ref().unwrap().specific_heat.is_none());
}

#[test]
fn integrates_bounded_polynomials_instead_of_using_mean_temperature() {
    let mut m = material();
    let series = m
        .thermal_response
        .as_mut()
        .unwrap()
        .conductivity
        .as_mut()
        .unwrap();
    series.points.clear();
    series.polynomial = Some(
        serde_json::from_value(json!({
            "coefficients": [0.01, 0.0, 0.00001, 0.0], "domain_c": [0.0, 60.0]
        }))
        .unwrap(),
    );
    let law = ConductionLaw::from_material(&m, true).unwrap();
    // k(T)=0.01+1e-5*T², thickness=1e-3: q=(0.01*60+1e-5*60³/3)/1e-3.
    near(law.flux_between(60.0, 0.0).unwrap(), 1320.0);
    let mean_temperature_approximation = 60.0 / law.resistance_at(30.0).unwrap();
    assert!((mean_temperature_approximation - 1320.0).abs() > 100.0);
}

#[test]
fn scalar_resistance_uses_the_same_qualification_path() {
    let mut m = material();
    m.thermal_response = None;
    let mut resistance = m.properties["thickness"].clone();
    resistance.value = 0.5;
    resistance.unit = "m2*K/W".into();
    m.properties.clear();
    m.properties.insert("thermal_resistance".into(), resistance);
    let law = ConductionLaw::from_material(&m, true).unwrap();
    assert_eq!(law.domain(), None);
    near(law.flux_between(-10.0, 40.0).unwrap(), -100.0);
    assert!(law.flux_between(-274.0, 0.0).is_err());
}

#[test]
fn conditions_evidence_forms_and_extrapolation_still_refuse() {
    let mut m = material();
    assert!(ConductionLaw::from_material(&m, false).is_err());
    let law = ConductionLaw::from_material(&m, true).unwrap();
    for t in [-1.0, 61.0, f64::NAN, f64::INFINITY] {
        assert!(law.flux_between(t, 20.0).is_err());
        assert!(law.flux_between(20.0, t).is_err());
    }
    for form in [Form::Fiber, Form::Yarn] {
        m.form = form;
        assert!(ConductionLaw::from_material(&m, true).is_err());
    }
    m = material();
    m.properties
        .get_mut("thickness")
        .unwrap()
        .conditions
        .insert("conditioning".into(), "different-lot".into());
    assert!(ConductionLaw::from_material(&m, true).is_err());
    m.properties.remove("thickness");
    assert!(ConductionLaw::from_material(&m, true).is_err());
}

#[test]
fn conductivity_does_not_inherit_specific_heat_temperature_limits() {
    let mut m = material();
    let mut cp = m
        .thermal_response
        .as_ref()
        .unwrap()
        .conductivity
        .clone()
        .unwrap();
    cp.unit = "J/(kg*K)".into();
    cp.points[0].temperature_c = 100.0;
    cp.points[1].temperature_c = 120.0;
    m.thermal_response.as_mut().unwrap().specific_heat = Some(cp);
    let law = ConductionLaw::from_material(&m, true).unwrap();
    assert_eq!(law.domain(), Some([0.0, 60.0]));
    near(law.flux_between(30.0, 10.0).unwrap(), 50.0);
}

#[test]
fn real_conductivity_only_records_are_admitted_without_upgrading_other_capabilities() {
    let db = Catalog::builtin().unwrap();
    // NIST's conflicting Cp and density observations remain withheld, not needed for conduction.
    for id in [
        "nist/7282/cotton-duck",
        "nist/7282/nomex-e89-crosstech",
        "nist/7282/scotchlite",
    ] {
        let resolved = db
            .resolve(&Selection {
                id: id.into(),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap();
        let law = ConductionLaw::from_material(resolved.material(), true).unwrap();
        assert!(law.flux_between(60.0, 30.0).unwrap() > 0.0);
        assert!(ThermalLaw::from_material(resolved.material(), true).is_err());
        assert!(resolved.mechanics().is_err());
        let replay = db.resolve(&resolved.frozen_selection()).unwrap();
        assert_eq!(
            law.flux_between(60.0, 30.0).unwrap(),
            ConductionLaw::from_material(replay.material(), true)
                .unwrap()
                .flux_between(60.0, 30.0)
                .unwrap()
        );
    }
}

#[test]
fn matched_fsri_runs_use_their_own_thickness_and_printed_conductivities() {
    let db = Catalog::builtin().unwrap();
    for (id, thickness, low, high) in [
        (
            "fsri/hfm/cotton-sheet/dry/220330-r1",
            0.0003,
            (0.02232 + 0.02276 + 0.02282) / 3.0,
            (0.02681 + 0.02685 + 0.02688) / 3.0,
        ),
        (
            "fsri/hfm/cotton-sheet/wet/220318-r1",
            0.00043,
            (0.02513 + 0.02578 + 0.02597) / 3.0,
            (0.03076 + 0.03081 + 0.03078) / 3.0,
        ),
    ] {
        let resolved = db
            .resolve(&Selection {
                id: id.into(),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap();
        let law = ConductionLaw::from_material(resolved.material(), true).unwrap();
        near(
            law.flux_between(65.0, 15.0).unwrap(),
            50.0 * (low + high) / 2.0 / thickness,
        );
        assert!(ThermalLaw::from_material(resolved.material(), true).is_err());
        assert!(resolved.mechanics().is_err());
        assert!(law.flux_between(65.001, 15.0).is_err());
        assert!(ConductionLaw::from_material(resolved.material(), false).is_err());
    }
}
