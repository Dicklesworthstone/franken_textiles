//! Synthetic constitutive known answers and independently transcribed FSRI data checks.
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_thermal::{TemperatureCurve, TemperatureSeries, ThermalLaw};
use ftx_pattern::materials::{Catalog, Material};
use serde_json::{Value, json};

fn record() -> Material {
    let q = |value, unit| {
        json!({"value":value,"unit":unit,"source":"test","evidence":"estimate",
        "method":"synthetic known answer", "conditions":{"specimen":"test:sheet","conditioning":"dry"}})
    };
    serde_json::from_value(json!({"id":"synthetic","name":"Not measured textile data","form":"woven",
        "properties":{"areal_density":q(0.2,"kg/m2"),"thickness":q(0.001,"m")},
        "thermal_response":{"specimen":"test:sheet","conditioning":"dry",
            "conductivity":{"unit":"W/(m*K)","source":"test","evidence":"estimate","method":"synthetic",
                "points":[{"temperature_c":0.0,"mean":0.1},{"temperature_c":50.0,"mean":0.2}]},
            "specific_heat":{"unit":"J/(kg*K)","source":"test","evidence":"estimate","method":"synthetic",
                "points":[{"temperature_c":0.0,"mean":100.0},{"temperature_c":20.0,"mean":200.0},{"temperature_c":50.0,"mean":500.0}]}},
        "sources":{"test":{"reference":"test:thermal","accessed_on":"2026-10-14","note":"Synthetic law, no real-world material claim"}}})).unwrap()
}
fn curve(m: &Material) -> TemperatureCurve {
    m.thermal_response
        .as_ref()
        .unwrap()
        .specific_heat
        .as_ref()
        .unwrap()
        .compile("J/(kg*K)", &m.sources, true)
        .unwrap()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-10 * (1.0 + b.abs()), "{a} != {b}");
}

#[test]
fn exact_knots_linear_values_and_segment_local_integrals() {
    let c = curve(&record());
    assert_eq!(c.domain(), [0.0, 50.0]);
    assert_eq!(c.at(0.0).unwrap(), 100.0);
    assert_eq!(c.at(20.0).unwrap(), 200.0);
    assert_eq!(c.at(50.0).unwrap(), 500.0);
    near(c.at(35.0).unwrap(), 350.0);
    near(c.integral(10.0, 35.0).unwrap(), 1750.0 + 4125.0);
    near(c.integral(35.0, 10.0).unwrap(), -5875.0);
    near(c.integral(0.0, 50.0).unwrap(), 13500.0);
    assert_eq!(c.integral(20.0, 20.0).unwrap(), 0.0);
    near(c.integral(19.999_999, 20.000_001).unwrap(), 0.0004);
}

#[test]
fn no_extrapolation_including_zero_length_integrals_outside_domain() {
    let c = curve(&record());
    for t in [-1.0, 51.0, f64::NAN, f64::INFINITY] {
        assert!(c.at(t).is_err());
        assert!(c.integral(t, t).is_err());
        assert!(c.integral(20.0, t).is_err());
    }
}

#[test]
fn duplicate_temperatures_bad_units_variability_and_provenance_refuse() {
    let original = record();
    let series = original
        .thermal_response
        .as_ref()
        .unwrap()
        .specific_heat
        .as_ref()
        .unwrap();
    for (path, replacement) in [
        ("unit", json!("J/K")),
        ("source", json!("missing")),
        ("method", json!("")),
    ] {
        let mut value = serde_json::to_value(series).unwrap();
        value[path] = replacement;
        let s: TemperatureSeries = serde_json::from_value(value).unwrap();
        assert!(s.validate("J/(kg*K)", &original.sources).is_err());
    }
    for (key, value) in [
        ("temperature_c", json!(0.0)),
        ("mean", json!(0.0)),
        ("standard_deviation", json!(-1.0)),
    ] {
        let mut v = serde_json::to_value(series).unwrap();
        v["points"][1][key] = value;
        let s: TemperatureSeries = serde_json::from_value(v).unwrap();
        assert!(s.validate("J/(kg*K)", &original.sources).is_err());
    }
    let mut s = series.clone();
    s.points[0].mean = f64::NAN;
    assert!(s.validate("J/(kg*K)", &original.sources).is_err());
}

#[test]
fn measured_knots_do_not_implicitly_admit_interpolation() {
    let mut m = record();
    let s = m
        .thermal_response
        .as_mut()
        .unwrap()
        .specific_heat
        .as_mut()
        .unwrap();
    s.evidence = ftx_pattern::materials::Evidence::Measured;
    assert!(s.compile("J/(kg*K)", &m.sources, false).is_err());
    assert!(s.compile("J/(kg*K)", &m.sources, true).is_ok());
}

#[test]
fn response_conflicts_and_mismatched_conditioning_are_not_silently_ignored() {
    let mut m = record();
    let q = m.properties["thickness"].clone();
    m.properties.insert("thermal_resistance".into(), q);
    assert!(m.validate().is_err());
    let mut m = record();
    m.properties
        .get_mut("areal_density")
        .unwrap()
        .conditions
        .insert("conditioning".into(), "wet".into());
    assert!(m.validate().is_ok()); // data can be retained, but is not a qualified consumer input
    assert!(ThermalLaw::from_material(&m, true).is_err());
    let mut m = record();
    m.properties
        .get_mut("thickness")
        .unwrap()
        .conditions
        .clear();
    assert!(ThermalLaw::from_material(&m, true).is_err());
}

#[test]
fn enthalpy_uses_reference_mass_and_not_endpoint_heat_capacity() {
    let law = ThermalLaw::from_material(&record(), true).unwrap();
    near(law.sensible_change(10.0, 35.0).unwrap(), 1175.0);
    near(law.capacity_at(35.0).unwrap(), 70.0);
    near(law.resistance_at(25.0).unwrap(), 0.001 / 0.15);
    assert!((law.sensible_change(10.0, 35.0).unwrap() - 70.0 * 25.0).abs() > 500.0);
    assert!(ThermalLaw::from_material(&record(), false).is_err());
}

#[test]
fn different_property_ranges_intersect_and_never_extend_one_another() {
    let mut m = record();
    let k = m
        .thermal_response
        .as_mut()
        .unwrap()
        .conductivity
        .as_mut()
        .unwrap();
    k.points[0].temperature_c = 15.0;
    k.points[1].temperature_c = 65.0;
    let law = ThermalLaw::from_material(&m, true).unwrap();
    assert_eq!(law.domain(), Some([15.0, 50.0]));
    assert!(law.resistance_at(60.0).is_err());
    assert!(law.sensible_change(10.0, 20.0).is_err());
    m.thermal_response
        .as_mut()
        .unwrap()
        .conductivity
        .as_mut()
        .unwrap()
        .points[0]
        .temperature_c = 55.0;
    assert!(ThermalLaw::from_material(&m, true).is_err());
}

#[test]
fn source_locked_selection_carries_observations_variability_and_conditioning() {
    let c = Catalog::builtin().unwrap();
    let s = Selection {
        id: "synthetic".into(),
        record: Some(record()),
        allow_estimates: true,
        ..Selection::default()
    };
    let a = c.resolve(&s).unwrap();
    let frozen = a.frozen_selection();
    let text = serde_json::to_string(&frozen).unwrap();
    let replay = c.resolve(&serde_json::from_str(&text).unwrap()).unwrap();
    assert_eq!(a.sha256(), replay.sha256());
    let mut changed = frozen;
    changed
        .record
        .as_mut()
        .unwrap()
        .thermal_response
        .as_mut()
        .unwrap()
        .specific_heat
        .as_mut()
        .unwrap()
        .points[0]
        .standard_deviation = Some(10.0);
    assert!(c.resolve(&changed).is_err());
    changed.expected_sha256 = None;
    assert_ne!(a.sha256(), c.resolve(&changed).unwrap().sha256());
}

#[test]
fn five_fsri_records_retain_25_means_without_fabricating_missing_inputs() {
    let c = Catalog::builtin().unwrap();
    let records: Vec<_> = c
        .materials
        .iter()
        .filter(|m| m.id.starts_with("fsri."))
        .collect();
    assert_eq!(records.len(), 5);
    let mut count = 0;
    for m in &records {
        assert!(!m.missing_mechanics().is_empty());
        assert!(ThermalLaw::from_material(m, true).is_err());
        let r = m.thermal_response.as_ref().unwrap();
        for s in [&r.conductivity, &r.specific_heat].into_iter().flatten() {
            count += s.points.len();
            assert!(s.points.iter().all(|p| p.standard_deviation.is_some()));
            assert!(
                m.sources[&s.source]
                    .reference
                    .starts_with("https://materials.fsri.org/")
            );
        }
    }
    assert_eq!(count, 25);
    let wet = c.get("fsri.cotton-sheet.unconditioned.2026-10-14").unwrap();
    let dry = c.get("fsri.cotton-sheet.dried.2026-10-14").unwrap();
    assert_eq!(curve(wet).at(20.0).unwrap(), 1314.0);
    assert_eq!(curve(dry).at(20.0).unwrap(), 1273.0);
    assert_eq!(
        wet.thermal_response
            .as_ref()
            .unwrap()
            .specific_heat
            .as_ref()
            .unwrap()
            .points[2]
            .standard_deviation,
        Some(648.0)
    );
    let polyester = c
        .get("fsri.chair-polyester-batting.unconditioned.2026-10-14")
        .unwrap();
    assert!(
        polyester
            .thermal_response
            .as_ref()
            .unwrap()
            .specific_heat
            .is_none()
    );
    assert!(
        c.get("fsri.cotton-batting.dried.2026-10-14")
            .unwrap()
            .thermal_response
            .as_ref()
            .unwrap()
            .specific_heat
            .is_none()
    );
}

#[test]
fn legacy_material_serialization_does_not_gain_a_null_field() {
    let mut value = serde_json::to_value(record()).unwrap();
    value.as_object_mut().unwrap().remove("thermal_response");
    let m: Material = serde_json::from_value(value.clone()).unwrap();
    let output: Value = serde_json::to_value(m).unwrap();
    assert!(output.get("thermal_response").is_none());
}
