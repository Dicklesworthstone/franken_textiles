//! Synthetic/specification known answers for the garment measurement gate, spec 02 D.5.

use ftx_measure::{ReadinessOptions, definition, make_record, readiness, reference_wearer};
use serde_json::{Value, json};

const TODAY: &str = "2026-10-12";

fn tape(name: &str, value: f64) -> Value {
    make_record(
        name,
        value,
        definition(name).unwrap()["tape_sigma"].as_f64().unwrap(),
        "MEASURED_TAPE",
        "tape",
        None,
        "",
    )
    .unwrap()
}
fn data(records: Vec<Value>) -> Value {
    json!({"schema":"fabric-measurements/1", "wearer":{"stature_mm":920.0,"age_months":30.0},
        "taken_on":"2026-10-01", "records":records, "fused":{}})
}
fn check(d: &Value, kind: &str) -> Value {
    readiness(d, kind, &ReadinessOptions::default(), TODAY).unwrap()
}
fn full_bodice() -> Value {
    data(
        [
            ("stature", 921.0),
            ("chest", 548.0),
            ("back_neck_to_waist", 228.0),
            ("neck_base", 252.0),
            ("across_back", 226.0),
            ("shoulder_length", 81.0),
        ]
        .into_iter()
        .map(|(n, v)| tape(n, v))
        .collect(),
    )
}

#[test]
fn missing_bodice_measurements_match_spec_requests() {
    let d = data(vec![tape("stature", 921.0), tape("chest", 548.0)]);
    let r = check(&d, "bolero");
    assert_eq!(r["status"], "NOT_READY");
    assert_eq!(r["garment"], "bodice");
    assert_eq!(
        r["blocking"],
        json!([
            "back_neck_to_waist",
            "neck_base",
            "across_back",
            "shoulder_length"
        ])
    );
    assert_eq!(
        r["remeasure_request"][0],
        "back_neck_to_waist: Head bent forward to find C7; tape straight down the spine to the ribbon's lower edge. Need ±1.0 cm; only a size-chart guess (±1.5 cm)."
    );
    assert_eq!(
        r["remeasure_request"][3],
        "shoulder_length: From where the neck meets the shoulder to the bony shoulder tip. Need ±0.5 cm; only a size-chart guess (±0.6 cm)."
    );
    assert_eq!(r["items"][0]["status"], "OK");
    assert_eq!(r["items"][1]["status"], "OK");
    assert_eq!(r["advisories"].as_array().unwrap().len(), 3);
    assert!(
        r["advisories"][1]
            .as_str()
            .unwrap()
            .contains("±3.0°; only a size-chart guess (±5.0°)")
    );
}

#[test]
fn full_taped_bodice_and_tape_limited_advisories_match_spec() {
    let r = check(&full_bodice(), "bodice");
    assert_eq!(r["status"], "READY");
    assert_eq!(r["blocking"], json!([]));
    for i in [3, 4, 5] {
        assert_eq!(r["items"][i]["status"], "OK_TAPE_LIMITED");
    }
    assert_eq!(
        r["advisories"][0],
        "neck_base: ±0.84 cm is the tape's limit (target ±0.50 cm); check it on the muslin."
    );
    assert_eq!(
        r["advisories"][1],
        "across_back: ±1.09 cm is the tape's limit (target ±1.00 cm); check it on the muslin."
    );
    assert_eq!(
        r["advisories"][2],
        "shoulder_length: ±0.51 cm is the tape's limit (target ±0.50 cm); check it on the muslin."
    );
    assert_eq!(
        r["advisories"][3],
        "shoulder_length: the size-chart prior outweighs the measurement here; fine for drafting, re-check on the muslin."
    );
}

#[test]
fn reference_is_not_misreported_as_measured_even_when_sigma_meets_target() {
    let d = reference_wearer(920.0, Some(30.0), None, None).unwrap();
    for (kind, count) in [
        ("bodice", 6),
        ("set_in_sleeve", 2),
        ("trousers", 5),
        ("hat_fez", 1),
        ("full_trousers_gathered", 7),
    ] {
        let r = check(&d, kind);
        assert_eq!(r["status"], "READY_REFERENCE");
        assert_eq!(r["blocking"], json!([]));
        assert_eq!(r["remeasure_request"], json!([]));
        assert_eq!(r["for_real_child"].as_array().unwrap().len(), count);
    }
    let r = check(&d, "full_trousers_gathered");
    let names: Vec<&str> = r["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "waist",
            "hip",
            "outseam",
            "body_rise",
            "total_crotch_length",
            "calf",
            "foot_pass",
            "thigh",
            "inseam",
            "belly_depth"
        ]
    );
    assert_eq!(
        r["for_real_child"][5],
        "calf: Tape level round the fullest part of the calf. Need ±1.5 cm; only a size-chart guess (±1.5 cm)."
    );
    let mut not_reference = d;
    not_reference["wearer"]["reference"] = json!(false);
    assert_eq!(
        check(&not_reference, "full_trousers_gathered")["status"],
        "NOT_READY"
    );
}

#[test]
fn garment_options_require_pass_over_measurements_and_allow_openings() {
    let mut d = data(vec![]);
    d["wearer"]["nappies"] = json!(true);
    let options = ReadinessOptions {
        cuff_level: "knee",
        ..Default::default()
    };
    let r = readiness(&d, "zouave", &options, TODAY).unwrap();
    assert_eq!(
        r["blocking"],
        json!([
            "waist",
            "hip",
            "outseam",
            "body_rise",
            "total_crotch_length",
            "seat_over_nappy",
            "knee",
            "foot_pass",
            "calf"
        ])
    );
    let options = ReadinessOptions {
        cuff_opening: true,
        nappies: Some(false),
        ..options
    };
    let r = readiness(&d, "zouave", &options, TODAY).unwrap();
    assert_eq!(
        r["blocking"],
        json!([
            "waist",
            "hip",
            "outseam",
            "body_rise",
            "total_crotch_length",
            "knee"
        ])
    );
    let options = ReadinessOptions {
        closed_cuff: true,
        ..Default::default()
    };
    assert_eq!(
        readiness(&d, "sleeve", &options, TODAY).unwrap()["blocking"],
        json!(["arm_length", "upper_arm", "hand_pass"])
    );
    let options = ReadinessOptions {
        passes_hip: true,
        ..Default::default()
    };
    let r = readiness(&full_bodice(), "jacket", &options, TODAY).unwrap();
    assert_eq!(r["blocking"], json!(["hip"]));
    let options = ReadinessOptions {
        elastic_waist: true,
        ..Default::default()
    };
    let r = readiness(&d, "pants", &options, TODAY).unwrap();
    assert_eq!(r["items"][0]["target_sigma_mm"], 30.0);
    let options = ReadinessOptions {
        cuff_level: "elbow",
        ..Default::default()
    };
    assert!(readiness(&d, "zouave", &options, TODAY).is_err());
    assert!(readiness(&d, "unknown", &options, TODAY).is_err());
}

#[test]
fn source_quality_disagreement_and_missing_values_cannot_bypass_gate() {
    let mut d = data(vec![
        make_record("head", 490.0, 8.0, "PHOTO_ESTIMATED", "photo", None, "").unwrap(),
    ]);
    let options = ReadinessOptions {
        use_priors: false,
        ..Default::default()
    };
    let r = readiness(&d, "hat", &options, TODAY).unwrap();
    assert_eq!(r["items"][0]["status"], "TOO_UNCERTAIN");
    d["records"][0]["provenance"] = json!("SCAN_MEASURED");
    assert_eq!(
        readiness(&d, "hat", &options, TODAY).unwrap()["items"][0]["status"],
        "OK_TAPE_LIMITED"
    );
    d["records"] = json!([
        tape("head", 490.0),
        make_record("head", 560.0, 8.0, "PHOTO_ESTIMATED", "photo", None, "").unwrap()
    ]);
    let r = check(&d, "hat");
    assert_eq!(r["items"][0]["status"], "REMEASURE");
    assert_eq!(r["status"], "NOT_READY");
    d["records"] = json!([]);
    let r = readiness(&d, "hat", &options, TODAY).unwrap();
    assert_eq!(r["items"][0]["status"], "MISSING");
    assert!(
        r["remeasure_request"][0]
            .as_str()
            .unwrap()
            .ends_with("not measured yet.")
    );
}

#[test]
fn stale_dates_are_advisories_at_the_exact_threshold() {
    let mut d = full_bodice();
    d["taken_on"] = json!("2026-08-31"); // Exactly 42 days before TODAY.
    let r = check(&d, "bodice");
    assert!(
        r["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| !a.as_str().unwrap().starts_with("STALE"))
    );
    d["taken_on"] = json!("2026-08-30");
    let r = check(&d, "bodice");
    assert_eq!(r["status"], "READY");
    assert_eq!(
        r["advisories"].as_array().unwrap().last().unwrap(),
        "STALE: measured 43 days ago; a toddler grows ~0.7 cm/month — re-take lengths."
    );
    assert!(readiness(&d, "bodice", &ReadinessOptions::default(), "2026-02-29").is_err());
}

#[test]
fn precise_model_estimates_never_qualify_as_independent_measurements() {
    let mut d = full_bodice();
    for record in d["records"].as_array_mut().unwrap() {
        record["provenance"] = json!("MODEL_FIT");
        record["sigma_mm"] = json!(0.01);
    }
    for use_priors in [false, true] {
        let options = ReadinessOptions {
            use_priors,
            ..Default::default()
        };
        let result = readiness(&d, "bodice", &options, TODAY).unwrap();
        assert_eq!(result["status"], "NOT_READY");
        assert_eq!(result["blocking"].as_array().unwrap().len(), 6);
        assert!(
            result["items"]
                .as_array()
                .unwrap()
                .iter()
                .take(6)
                .all(|item| item["status"] == "MODEL_FIT")
        );
        assert!(
            result["remeasure_request"][0]
                .as_str()
                .unwrap()
                .contains("model estimate")
        );
    }
    d["wearer"]["reference"] = json!(true);
    let result = check(&d, "bodice");
    assert_eq!(result["status"], "READY_REFERENCE");
    assert_eq!(result["for_real_child"].as_array().unwrap().len(), 6);
}

#[test]
fn used_model_sources_cannot_hide_behind_direct_primary_provenance() {
    let mut d = data(vec![
        make_record("head", 490.0, 1.0, "MEASURED_TAPE", "tape", None, "").unwrap(),
    ]);
    assert_eq!(check(&d, "hat")["status"], "READY");
    d["records"].as_array_mut().unwrap().push(
        make_record(
            "head",
            490.0,
            10.0,
            "MODEL_FIT",
            "synthetic model",
            None,
            "",
        )
        .unwrap(),
    );
    let result = check(&d, "hat");
    assert_eq!(result["items"][0]["primary_provenance"], "MEASURED_TAPE");
    assert_eq!(result["items"][0]["status"], "MODEL_FIT");
    assert_eq!(result["status"], "NOT_READY");
}

#[test]
fn optional_and_excluded_model_estimates_do_not_block_independent_required_evidence() {
    let mut d = full_bodice();
    d["records"].as_array_mut().unwrap().push(
        make_record(
            "waist",
            530.0,
            0.1,
            "MODEL_FIT",
            "synthetic model",
            None,
            "",
        )
        .unwrap(),
    );
    let result = check(&d, "bodice");
    assert_eq!(result["status"], "READY");
    assert!(
        result["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line.as_str().unwrap().contains("optional — waist:")
                && line.as_str().unwrap().contains("model estimate"))
    );
    let mut excluded =
        make_record("head", 490.0, 0.1, "MODEL_FIT", "synthetic model", None, "").unwrap();
    excluded["definition_id"] = json!("unmapped-model-section");
    let d = data(vec![tape("head", 490.0), excluded]);
    assert_eq!(check(&d, "hat")["status"], "READY");
}
