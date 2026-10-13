//! Known answers from spec 02 D.5, plus refusal and boundary regressions. All values synthetic.

use ftx_measure::{
    NAMES, canonical_name, catalogue, date_day, definition, fuse_one, make_record,
    normalize_record, parse_value, validate,
};
use serde_json::{Value, json};

fn record(name: &str, value: f64, sigma: f64, source: &str) -> Value {
    make_record(name, value, sigma, source, source, None, "").unwrap()
}

fn chest(value: f64, sigma: f64, source: &str) -> Value {
    record("chest", value, sigma, source)
}

fn prior() -> Value {
    let mut p = chest(540.0, 30.0, "CHART_PRIOR");
    p["p1_p99_mm"] = json!([470.2, 609.8]);
    p["auto_prior"] = json!(true);
    p
}

#[test]
fn catalogue_units_aliases_and_calendar() {
    assert_eq!(catalogue().as_object().unwrap().len(), 35);
    for name in NAMES {
        let d = definition(name).unwrap();
        assert!(d["how"].as_str().unwrap().len() > 5);
        assert!(d["definition_id"].as_str().is_some());
    }
    assert_eq!(canonical_name(" Upper-Arm Girth "), "upper_arm");
    assert_eq!(canonical_name("custom-field"), "custom_field");
    assert_eq!(parse_value("21.5in", "mm").unwrap(), 546.1);
    assert_eq!(parse_value("55CM", "mm").unwrap(), 550.0);
    assert_eq!(parse_value("18°", "deg").unwrap(), 18.0);
    assert!(parse_value("18", "mm").is_err());
    assert!(parse_value("18cm", "deg").is_err());
    for bad in ["NaNmm", "infmm", "1e3mm", ".5mm", "5.mm", "1 cm junk"] {
        assert!(parse_value(bad, "mm").is_err(), "{bad}");
    }
    assert_eq!(date_day("1970-01-01"), Some(719163));
    assert_eq!(
        date_day("2024-03-01").unwrap() - date_day("2024-02-28").unwrap(),
        2
    );
    for bad in [
        "2026-02-29",
        "1900-02-29",
        "0000-01-01",
        "2026-00-10",
        "99999-01-01",
        "é026-01-01",
    ] {
        assert!(date_day(bad).is_none());
    }
}

#[test]
fn validation_rejects_wrong_dimensions_booleans_and_nonpositive_sigma() {
    let mut data =
        json!({"schema": "fabric-measurements/1", "wearer": {}, "records": [], "fused": {}});
    assert!(validate(&data).is_empty());
    data["records"] = json!([{"name": "shoulder_slope", "value_mm": 18}]);
    assert!(validate(&data)[0].contains("is an angle"));
    data["records"] = json!([chest(550.0, 10.0, "MEASURED_TAPE")]);
    data["records"][0]["value_mm"] = json!(true);
    data["records"][0]["sigma_mm"] = json!(0);
    assert_eq!(validate(&data).len(), 2);
    for bad in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(make_record("chest", 550.0, bad, "MEASURED_TAPE", "tape", None, "").is_err());
    }
    assert!(make_record("chest", f64::NAN, 10.0, "MEASURED_TAPE", "tape", None, "").is_err());
}

#[test]
fn independent_sources_and_optional_prior_match_d5() {
    let tape = chest(550.0, 10.0, "MEASURED_TAPE");
    let photo = chest(570.0, 20.0, "PHOTO_ESTIMATED");
    let p = prior();
    let f = fuse_one("chest", &[tape.clone(), photo], Some(&p), false)
        .unwrap()
        .unwrap();
    assert_eq!(f["value_mm"], 554.0);
    assert_eq!(f["sigma_mm"], 8.9);
    assert_eq!(f["z_max"], 0.89);
    assert_eq!(f["sources"][0]["weight"], 0.8);
    assert_eq!(f["sources"][1]["weight"], 0.2);
    assert_eq!(f["sources"][2]["used"], false);
    let f = fuse_one("chest", &[tape], Some(&p), true).unwrap().unwrap();
    assert_eq!(f["value_mm"], 549.0);
    assert_eq!(f["sigma_mm"], 9.5);
}

#[test]
fn disagreement_and_outliers_do_not_hide_behind_the_prior() {
    for (photo, expected, flag) in [(640.0, 550.0, "REMEASURE"), (610.0, 562.0, "Z_WARN")] {
        let f = fuse_one(
            "chest",
            &[
                chest(550.0, 10.0, "MEASURED_TAPE"),
                chest(photo, 20.0, "PHOTO_ESTIMATED"),
            ],
            Some(&prior()),
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(f["value_mm"], expected);
        assert_eq!(f["flags"], json!([flag]));
        // notes are a list, as the reference writes them
        assert!(
            f["notes"][0]
                .as_str()
                .unwrap()
                .contains(if flag == "REMEASURE" {
                    "not fused"
                } else {
                    "fused, but check"
                })
        );
    }
    let f = fuse_one(
        "chest",
        &[chest(700.0, 10.0, "MEASURED_TAPE")],
        Some(&prior()),
        true,
    )
    .unwrap()
    .unwrap();
    assert_eq!(f["value_mm"], 700.0);
    assert_eq!(f["flags"], json!(["VERIFY_OUT_OF_BOUNDS"]));
    assert_eq!(f["sources"][1]["weight"], 0.0);
    assert_eq!(f["sources"][1]["used"], false);
}

#[test]
fn repeated_tape_measurements_are_not_independent() {
    let obs: Vec<Value> = [250.0, 255.0, 252.0]
        .into_iter()
        .map(|v| record("neck_base", v, 10.0, "MEASURED_TAPE"))
        .collect();
    let f = fuse_one("neck_base", &obs, None, false).unwrap().unwrap();
    assert_eq!(f["value_mm"], 252.0);
    assert_eq!(f["sigma_mm"], 10.0);
    assert_eq!(f["sources"][0]["n_repeats"], 3);
    assert_eq!(f["sources"][0]["note"], "3 repeats: median, spread 2.5");
    let wide = [
        chest(500.0, 10.0, "MEASURED_TAPE"),
        chest(600.0, 10.0, "MEASURED_TAPE"),
    ];
    let f = fuse_one("chest", &wide, None, false).unwrap().unwrap();
    assert_eq!(f["sigma_mm"], 70.7);
    assert_eq!(f["flags"], json!(["REPEATS_COLLAPSED", "REPEATS_SPREAD"]));
}

#[test]
fn definition_offsets_exclusions_and_prior_only() {
    let r = make_record(
        "waist",
        500.0,
        15.0,
        "MEASURED_TAPE",
        "tape",
        Some("CPSC77:66-navel"),
        "",
    )
    .unwrap();
    let (n, status) = normalize_record(&r).unwrap();
    assert_eq!(status, "normalized");
    let f = fuse_one("waist", &[n], None, false).unwrap().unwrap();
    assert_eq!(f["value_mm"], 490.0);
    assert_eq!(f["sigma_mm"], 15.8);
    let r = make_record(
        "inseam",
        330.0,
        20.0,
        "MEASURED_TAPE",
        "tape",
        Some("ISO8559-1:5.7.6"),
        "",
    )
    .unwrap();
    assert_eq!(normalize_record(&r).unwrap().1, "excluded");
    let p = prior();
    assert_eq!(
        fuse_one("chest", &[], Some(&p), true).unwrap().unwrap()["flags"],
        json!(["PRIOR_ONLY"])
    );
    assert!(fuse_one("chest", &[], None, false).unwrap().is_none());
}

#[test]
fn ties_extreme_uncertainty_and_invalid_prior() {
    let f = fuse_one(
        "chest",
        &[
            chest(550.0, 10.0, "MEASURED_TAPE"),
            chest(550.0, 10.0, "PHOTO_ESTIMATED"),
        ],
        None,
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(f["primary_provenance"], "MEASURED_TAPE");
    let f = fuse_one(
        "chest",
        &[chest(550.0, 1e-200, "MEASURED_TAPE")],
        None,
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(f["value_mm"], 550.0);
    let mut p = prior();
    p["p1_p99_mm"] = json!([600, 400]);
    assert!(fuse_one("chest", &[], Some(&p), true).is_err());
}
