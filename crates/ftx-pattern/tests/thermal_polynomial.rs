//! Independent polynomial/enthalpy answers and material admission checks.
use std::collections::BTreeMap;

use ftx_pattern::material_thermal::TemperatureSeries;
use ftx_pattern::materials::Source;
use serde_json::json;

fn sources() -> BTreeMap<String, Source> {
    BTreeMap::from([(
        "test".into(),
        Source {
            reference: "test:synthetic-polynomial".into(),
            accessed_on: "2026-10-14".into(),
            note: "Synthetic mathematical test, not measured fabric".into(),
        },
    )])
}
fn series(coefficients: [f64; 4], domain: [f64; 2]) -> TemperatureSeries {
    serde_json::from_value(json!({"unit":"J/(kg*K)", "evidence":"published", "source":"test",
        "method":"Synthetic polynomial", "polynomial":{"coefficients":coefficients,"domain_c":domain}
    })).unwrap()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-11 * (1.0 + b.abs()), "{a} != {b}");
}
#[test]
fn evaluates_and_integrates_cubic_without_resampling() {
    let s = series([1000.0, 2.0, 0.03, -0.0001], [0.0, 100.0]);
    let c = s.compile("J/(kg*K)", &sources(), true).unwrap();
    assert_eq!(c.domain(), [0.0, 100.0]);
    near(c.at(50.0).unwrap(), 1162.5);
    let primitive = |t: f64| 1000.0 * t + t * t + 0.01 * t * t * t - 0.000025 * t * t * t * t;
    let expected = primitive(80.0) - primitive(20.0);
    near(c.integral(20.0, 80.0).unwrap(), expected);
    assert_eq!(
        c.integral(80.0, 20.0).unwrap(),
        -c.integral(20.0, 80.0).unwrap()
    );
    assert_eq!(c.integral(20.0, 20.0).unwrap(), 0.0);
    let t = 80.0_f64;
    let next = f64::from_bits(t.to_bits() + 1);
    near(c.integral(t, next).unwrap() / (next - t), c.at(t).unwrap());
    near(
        c.integral(20.0, 50.0).unwrap() + c.integral(50.0, 80.0).unwrap(),
        expected,
    );
}
#[test]
fn checks_interior_extrema_not_just_endpoints() {
    // Both endpoints positive, interior negative; includes quadratic and two-root cubic paths.
    for (coefficients, domain) in [
        ([24.0, -10.0, 1.0, 0.0], [0.0, 10.0]),
        ([0.2, -3.0, 0.0, 1.0], [0.0, 2.0]),
        ([0.0, 0.0, 1.0, 0.0], [-1.0, 1.0]),
        ([0.0; 4], [0.0, 10.0]),
        ([1.0, -1e-14, 1e-28, 0.0], [0.0, 1e14]), // positive, handled separately below
    ] {
        let s = series(coefficients, domain);
        if coefficients[0] == 1.0 {
            assert!(s.validate("J/(kg*K)", &sources()).is_ok());
        } else {
            assert!(s.validate("J/(kg*K)", &sources()).is_err());
        }
    }
    assert!(
        series([26.0, -10.0, 1.0, 0.0], [0.0, 10.0])
            .validate("J/(kg*K)", &sources())
            .is_ok()
    );
    // Positive endpoints and a repeated derivative root; also exercises q == 0.
    assert!(
        series([10.0, 0.0, 0.0, 1.0], [-1.0, 1.0])
            .validate("J/(kg*K)", &sources())
            .is_ok()
    );
}
#[test]
fn domain_units_provenance_and_fit_consent_are_enforced() {
    let s = series([100.0, 0.0, 0.0, 0.0], [0.0, 50.0]);
    assert!(s.compile("J/(kg*K)", &sources(), false).is_err());
    assert!(s.compile("W/(m*K)", &sources(), true).is_err());
    assert!(s.compile("J/(kg*K)", &BTreeMap::new(), true).is_err());
    let c = s.compile("J/(kg*K)", &sources(), true).unwrap();
    for t in [-1.0, 51.0, f64::NAN, f64::INFINITY] {
        assert!(c.at(t).is_err());
        assert!(c.integral(t, t).is_err());
    }
    for domain in [[-274.0, 50.0], [50.0, 0.0], [0.0, 0.0]] {
        assert!(
            series([1.0, 0.0, 0.0, 0.0], domain)
                .validate("J/(kg*K)", &sources())
                .is_err()
        );
    }
    let mut s = s;
    s.polynomial.as_mut().unwrap().coefficients[1] = f64::NAN;
    assert!(s.validate("J/(kg*K)", &sources()).is_err());
}
#[test]
fn fits_cannot_impersonate_measured_points_or_compete_with_them() {
    let s = series([100.0, 1.0, 0.0, 0.0], [0.0, 50.0]);
    let mut v = serde_json::to_value(&s).unwrap();
    v["evidence"] = json!("measured");
    assert!(
        serde_json::from_value::<TemperatureSeries>(v)
            .unwrap()
            .validate("J/(kg*K)", &sources())
            .is_err()
    );
    let mut v = serde_json::to_value(&s).unwrap();
    v["points"] = json!([{"temperature_c":0.0,"mean":100.0},{"temperature_c":50.0,"mean":150.0}]);
    assert!(
        serde_json::from_value::<TemperatureSeries>(v)
            .unwrap()
            .validate("J/(kg*K)", &sources())
            .is_err()
    );
}
#[test]
fn old_series_serialize_without_a_new_null_and_keep_their_arithmetic() {
    let v = json!({"unit":"J/(kg*K)","source":"test","method":"synthetic","evidence":"estimate", "conditions":{},
        "points":[{"temperature_c":0.0,"mean":100.0},{"temperature_c":50.0,"mean":150.0}]});
    let s: TemperatureSeries = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(serde_json::to_value(&s).unwrap(), v);
    let c = s.compile("J/(kg*K)", &sources(), true).unwrap();
    assert_eq!(c.at(25.0).unwrap(), 125.0);
    assert_eq!(c.integral(0.0, 50.0).unwrap(), 6250.0);
}
