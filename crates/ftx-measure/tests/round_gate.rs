//! Whole-pattern role dispatch, against the independently tested individual garment gates.

use ftx_measure::{
    ReadinessOptions, format_round_readiness, readiness, reference_wearer, round_readiness,
};
use serde_json::{Value, json};

const TODAY: &str = "2026-10-13";
fn pattern(roles: &[&str]) -> Value {
    json!({"pieces":roles.iter().map(|r| json!({"role":r})).collect::<Vec<_>>()})
}
fn measurements() -> Value {
    json!({"schema":"fabric-measurements/1","wearer":{},"records":[],"fused":{}})
}

#[test]
fn roles_select_checks_once_and_combine_blockers() {
    let p = pattern(&["bodice_front", "bodice_back", "sleeve", "sleeve", "hat_tip"]);
    let m = measurements();
    let r = round_readiness(&p, &m, TODAY);
    assert_eq!(r["status"], "NOT_READY");
    assert_eq!(r["checks"].as_array().unwrap().len(), 3);
    for (i, kind) in ["bodice", "set_in_sleeve", "hat_fez"].iter().enumerate() {
        let one = readiness(
            &m,
            kind,
            &ReadinessOptions {
                closed_cuff: true,
                ..Default::default()
            },
            TODAY,
        )
        .unwrap();
        assert_eq!(
            r["checks"][i],
            json!({"garment":kind,"status":one["status"],"blocking":one["blocking"]})
        );
    }
    assert!(
        r["blocking"]
            .as_array()
            .unwrap()
            .contains(&json!("hand_pass"))
    );
    let text = format_round_readiness(&r);
    assert!(text.contains("NOT_READY for bodice, set_in_sleeve, hat_fez"));
    assert!(text.contains("hand_pass:"));
}

#[test]
fn trouser_cuffs_nappies_and_elastic_waist_are_inherited() {
    let mut p = pattern(&["pant_front", "pant_back", "cuff", "waistband"]);
    p["meta"] = json!({"style":{"cuff_level":"knee","nappy":true}});
    let m = measurements();
    let r = round_readiness(&p, &m, TODAY);
    assert_eq!(r["checks"][0]["garment"], "full_trousers_gathered");
    for name in ["seat_over_nappy", "knee", "calf", "foot_pass"] {
        assert!(
            r["blocking"].as_array().unwrap().contains(&json!(name)),
            "{name}"
        );
    }
    let one = readiness(
        &m,
        "full_trousers_gathered",
        &ReadinessOptions {
            elastic_waist: true,
            cuff_level: "knee",
            nappies: Some(true),
            ..Default::default()
        },
        TODAY,
    )
    .unwrap();
    assert_eq!(r["blocking"], one["blocking"]);
    assert_eq!(
        round_readiness(&pattern(&["pant_front"]), &m, TODAY)["checks"][0]["garment"],
        "trousers"
    );
    let mut inherited = m;
    inherited["wearer"]["nappies"] = json!(true);
    assert!(
        round_readiness(&pattern(&["pant_back"]), &inherited, TODAY)["blocking"]
            .as_array()
            .unwrap()
            .contains(&json!("seat_over_nappy"))
    );
}

#[test]
fn references_preserve_real_wearer_instructions_without_advisory_noise() {
    let m = reference_wearer(920.0, Some(30.0), None, None).unwrap();
    let p = pattern(&["bodice_front", "sleeve", "hat_crown"]);
    let r = round_readiness(&p, &m, TODAY);
    assert_eq!(r["status"], "READY_REFERENCE");
    assert_eq!(r["blocking"], json!([]));
    assert_eq!(r["advisories"], json!([]));
    assert_eq!(r["for_real_child"].as_array().unwrap().len(), 10);
    assert!(format_round_readiness(&r).contains("a real wearer needs 10 measurements"));
}

#[test]
fn unknown_or_failed_checks_are_never_presented_as_measured_ready() {
    for (p, m, date) in [
        (pattern(&["facing"]), measurements(), TODAY),
        (pattern(&["sleeve"]), json!({"arm_length":300}), TODAY),
        (pattern(&["sleeve"]), json!({"records":"broken"}), TODAY),
        (pattern(&["sleeve"]), measurements(), "2026-02-30"),
    ] {
        let r = round_readiness(&p, &m, date);
        assert_eq!(r["status"], "NOT_CHECKED");
        assert!(r["note"].as_str().unwrap().len() > 10);
        assert!(format_round_readiness(&r).contains("not checked"));
    }
}

/// A garment whose check fails keeps the checks gathered before it, as the reference's
/// `measurement_readiness` returns them (its output for this case: two READY_REFERENCE checks, nine
/// requests for a real child, and the note).
#[test]
fn a_failed_check_keeps_what_was_gathered() {
    let m = reference_wearer(920.0, Some(30.0), None, None).unwrap();
    let mut p = pattern(&["bodice_front", "sleeve", "pant_front", "cuff"]);
    p["meta"] = json!({"style":{"cuff_level":"mid"}});
    let r = round_readiness(&p, &m, TODAY);
    assert_eq!(r["status"], "NOT_CHECKED");
    let kinds: Vec<_> = r["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["garment"].clone())
        .collect();
    assert_eq!(kinds, [json!("bodice"), json!("set_in_sleeve")]);
    assert!(
        r["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["status"] == "READY_REFERENCE")
    );
    assert_eq!(r["for_real_child"].as_array().unwrap().len(), 9);
    assert_eq!(
        r["note"],
        "full_trousers_gathered: readiness could not be computed (cuff_level must be knee, calf or ankle)"
    );
}

/// Style values as the reference reads them: `nappy` by Python truthiness, a present non-string
/// `cuff_level` passed on to the gate (which refuses it) rather than replaced by the default.
#[test]
fn style_values_are_read_as_the_reference_reads_them() {
    let m = measurements();
    let mut p = pattern(&["pant_front", "pant_back", "cuff"]);
    p["meta"] = json!({"style":{"nappy":"yes"}});
    let r = round_readiness(&p, &m, TODAY);
    assert!(
        r["blocking"]
            .as_array()
            .unwrap()
            .contains(&json!("seat_over_nappy"))
    );
    p["meta"] = json!({"style":{"cuff_level":null}});
    let r = round_readiness(&p, &m, TODAY);
    assert_eq!(r["status"], "NOT_CHECKED");
    assert_eq!(
        r["note"],
        "full_trousers_gathered: readiness could not be computed (cuff_level must be knee, calf or ankle)"
    );
    p["meta"] = json!({"style":"not a mapping"});
    let r = round_readiness(&p, &m, TODAY);
    assert_eq!(r["checks"][0]["garment"], "full_trousers_gathered"); // treated as no style: calf
}
