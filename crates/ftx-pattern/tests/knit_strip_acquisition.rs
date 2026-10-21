//! Primary-source histories reach inverse consumers without a homogeneous-strain conversion.

use ftx_core::receipt::sha256_hex;
use ftx_pattern::material_catalog::knit_strips::{Branch, Direction, parse_acquisition};
use serde_json::Value;

const DATA: &str = include_str!("../src/material_catalog/knit-strip-measurements.json");

#[test]
fn original_histories_preserve_counts_signed_force_branches_and_exact_rows() {
    let acquisition = parse_acquisition(DATA).unwrap();
    assert_eq!(acquisition.acquisition_sha256, sha256_hex(DATA.as_bytes()));
    assert_eq!(acquisition.curves.len(), 9);
    assert_eq!(acquisition.protocol.free_gauge_length_m, 0.100);
    assert_eq!(acquisition.protocol.reference_width_m, 0.060);
    assert_eq!(acquisition.protocol.paper_working_force_limit_n, 5.0);
    assert_eq!(Direction::Weft.source_angle_degrees(), 0.0);
    assert_eq!(Direction::Bias.source_angle_degrees(), 45.0);
    assert_eq!(Direction::Warp.source_angle_degrees(), 90.0);
    assert!(acquisition.protocol.conditions["rate"].contains("not supplied"));
    assert!(acquisition.protocol.conditions["conditioning"].contains("not supplied"));
    assert_eq!(acquisition.protocol.unverified_model_assumptions.len(), 3);
    assert_eq!(
        sha256_hex(acquisition.metadata_json.as_bytes()),
        acquisition.metadata_sha256
    );
    assert!(acquisition.license.contains("THE SOFTWARE IS PROVIDED"));
    // Independently counted from original nine author CSVs; no 5 N truncation or unloading removal.
    for (curve, (specimen, direction, row_count, force_10, force_30)) in
        acquisition.curves.iter().zip([
            ("SJ1", Direction::Warp, 916, 0.40, 1.94),
            ("SJ1", Direction::Weft, 1620, 0.04, 0.84),
            ("SJ1", Direction::Bias, 1126, 0.27, 1.12),
            ("SJ3", Direction::Warp, 1124, 0.71, 2.11),
            ("SJ3", Direction::Weft, 1354, 0.80, 2.48),
            ("SJ3", Direction::Bias, 1158, 0.53, 1.78),
            ("DKIN8", Direction::Warp, 1050, 1.21, 3.39),
            ("DKIN8", Direction::Weft, 1012, 1.56, 5.00),
            ("DKIN8", Direction::Bias, 1024, 1.15, 3.39),
        ])
    {
        assert_eq!(curve.specimen, specimen);
        assert_eq!(curve.direction, direction);
        assert_eq!(curve.observations.len(), row_count);
        assert_eq!(curve.observations.first().unwrap().csv_line, 2);
        assert_eq!(curve.observations.last().unwrap().csv_line, row_count + 1);
        assert_eq!(curve.observations.first().unwrap().branch, Branch::Loading);
        assert_eq!(curve.observations.last().unwrap().branch, Branch::Unloading);
        assert_eq!(curve.exact_loading(0.1).unwrap().force_n, force_10);
        assert_eq!(curve.exact_loading(0.3).unwrap().force_n, force_30);
        assert!(curve.observations.iter().any(|row| row.force_n < 0.0));
        assert!(curve.observations.iter().any(|row| row.force_n > 5.0));
        assert!(curve.exact_loading(0.1001).is_err());
        assert!(curve.exact_loading(f64::NAN).is_err());
        assert!(curve.exact_loading(-0.1).is_err());
        assert!(curve.source.reference.ends_with(&curve.entry));
        assert!(curve.source.note.contains(&curve.sha256));
    }
}

#[test]
fn caller_can_select_known_curves_without_rewriting_their_sources() {
    let mut selected: Value = serde_json::from_str(DATA).unwrap();
    selected["curves"].as_array_mut().unwrap().truncate(3);
    let acquisition = parse_acquisition(&selected.to_string()).unwrap();
    assert_eq!(acquisition.curves.len(), 3);
    assert!(
        acquisition
            .curves
            .iter()
            .all(|curve| curve.specimen == "SJ1")
    );
    assert_eq!(
        acquisition.curves[2].exact_loading(0.3).unwrap().force_n,
        1.12
    );
    selected["curves"].as_array_mut().unwrap().clear();
    assert!(parse_acquisition(&selected.to_string()).is_err());
}

#[test]
fn copied_protocol_cannot_certify_changed_rehashed_or_mismatched_observations() {
    let original: Value = serde_json::from_str(DATA).unwrap();
    let mut changed = original.clone();
    let altered = changed["curves"][0]["csv"]
        .as_str()
        .unwrap()
        .replace("0.1,0.4,loading", "0.1,99,loading");
    assert_ne!(altered, changed["curves"][0]["csv"].as_str().unwrap());
    changed["curves"][0]["csv"] = altered.clone().into();
    assert!(parse_acquisition(&changed.to_string()).is_err());
    changed["curves"][0]["sha256"] = sha256_hex(altered.as_bytes()).into();
    assert!(parse_acquisition(&changed.to_string()).is_err());

    let mut changed = original.clone();
    changed["curves"][1] = changed["curves"][0].clone();
    assert!(parse_acquisition(&changed.to_string()).is_err());
    let mut changed = original.clone();
    changed["curves"][0]["specimen"] = "SJ3".into();
    assert!(parse_acquisition(&changed.to_string()).is_err());
    let mut changed = original.clone();
    changed["curves"][0]["direction"] = "weft".into();
    assert!(parse_acquisition(&changed.to_string()).is_err());
    let mut changed = original.clone();
    changed["curves"][0]["entry"] = "dataset/data/SJ3/stretch-warp.csv".into();
    assert!(parse_acquisition(&changed.to_string()).is_err());

    let mut changed = original.clone();
    changed["metadata_json"] = "{}".into();
    changed["metadata_sha256"] = sha256_hex(b"{}").into();
    assert!(parse_acquisition(&changed.to_string()).is_err());
    for (field, value) in [
        (
            "archive_url",
            "https://example.invalid/unrelated-dataset.zip",
        ),
        ("metadata_entry", "dataset/data/other.json"),
        ("retrieved_on", "2026-01-01"),
        ("license", "MIT License"),
    ] {
        let mut changed = original.clone();
        changed[field] = value.into();
        assert!(parse_acquisition(&changed.to_string()).is_err(), "{field}");
    }
}

#[test]
fn matched_transverse_curves_preserve_fractional_units_and_unknown_sampling() {
    let acquisition = parse_acquisition(DATA).unwrap();
    assert_eq!(acquisition.transverse_curves.len(), 9);
    let readme = acquisition.transverse_readme.as_ref().unwrap();
    assert_eq!(readme.entry, "dataset/README.md");
    assert_eq!(sha256_hex(readme.text.as_bytes()), readme.sha256);
    assert_eq!(readme.source.accessed_on, "2026-10-21");
    assert!(readme.text.contains("Elongations and compressions"));
    assert!(
        readme
            .text
            .contains("The data is released under the MIT license")
    );
    for (curve, (specimen, direction, rows, compression)) in
        acquisition.transverse_curves.iter().zip([
            ("SJ1", Direction::Warp, 6, 0.009691747029622395),
            ("SJ1", Direction::Weft, 10, 0.02771186828613281),
            ("SJ1", Direction::Bias, 7, 0.09286251068115234),
            ("SJ3", Direction::Warp, 7, 0.017545318603515624),
            ("SJ3", Direction::Weft, 8, 0.057208824157714847),
            ("SJ3", Direction::Bias, 7, 0.0976827621459961),
            ("DKIN8", Direction::Warp, 7, 0.017299143473307292),
            ("DKIN8", Direction::Weft, 7, 0.000162061055501302),
            ("DKIN8", Direction::Bias, 7, 0.04374720255533855),
        ])
    {
        assert_eq!(curve.specimen, specimen);
        assert_eq!(curve.direction, direction);
        assert_eq!(curve.observations.len(), rows);
        assert_eq!(curve.observations.first().unwrap().csv_line, 2);
        assert_eq!(curve.observations.last().unwrap().csv_line, rows + 1);
        assert_eq!(
            curve.exact_observation(0.2).unwrap().compression_fraction,
            compression
        );
        assert_eq!(curve.exact_observation(0.2).unwrap().csv_line, 3);
        assert!(curve.exact_observation(0.1).is_err());
        assert!(curve.exact_observation(f64::NAN).is_err());
        assert!(curve.exact_observation(-0.1).is_err());
        assert_eq!(curve.source.accessed_on, "2026-10-21");
        assert!(curve.source.reference.ends_with(&curve.entry));
        assert!(curve.source.note.contains(&curve.sha256));
        assert!(curve.measurement_scope.contains("no loading/unloading tag"));
        assert!(
            curve
                .measurement_scope
                .contains("material-midpoint distance is unverified")
        );
        // No inferred branch, homogeneous Poisson ratio or material-midpoint width is emitted.
        let row = serde_json::to_value(&curve.observations[1]).unwrap();
        assert_eq!(row.as_object().unwrap().len(), 3);
        assert!(row.get("branch").is_none());
        assert!(row.get("mid_width_m").is_none());
    }
}

#[test]
fn original_force_only_acquisition_and_known_transverse_subsets_remain_supported() {
    let mut selected: Value = serde_json::from_str(DATA).unwrap();
    selected["transverse_curves"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    assert_eq!(
        parse_acquisition(&selected.to_string())
            .unwrap()
            .transverse_curves
            .len(),
        1
    );
    selected
        .as_object_mut()
        .unwrap()
        .remove("transverse_curves");
    selected
        .as_object_mut()
        .unwrap()
        .remove("transverse_readme");
    let force_only = parse_acquisition(&selected.to_string()).unwrap();
    assert_eq!(force_only.curves.len(), 9);
    assert!(force_only.transverse_curves.is_empty());
    assert!(force_only.transverse_readme.is_none());
}

#[test]
fn transverse_identity_units_and_original_bytes_are_required() {
    let original: Value = serde_json::from_str(DATA).unwrap();
    let mut changed = original.clone();
    let csv = changed["transverse_curves"][0]["csv"]
        .as_str()
        .unwrap()
        .replace("0.009691747029622395", "0.99");
    changed["transverse_curves"][0]["csv"] = csv.clone().into();
    assert!(parse_acquisition(&changed.to_string()).is_err());
    changed["transverse_curves"][0]["sha256"] = sha256_hex(csv.as_bytes()).into();
    assert!(parse_acquisition(&changed.to_string()).is_err());
    let mut changed = original.clone();
    changed["transverse_curves"][1] = changed["transverse_curves"][0].clone();
    assert!(parse_acquisition(&changed.to_string()).is_err());
    for (field, value) in [
        ("specimen", "SJ3"),
        ("direction", "weft"),
        ("entry", "dataset/data/SJ3/poisson-warp.csv"),
        ("retrieved_on", "2026-10-20"),
    ] {
        let mut changed = original.clone();
        changed["transverse_curves"][0][field] = value.into();
        assert!(parse_acquisition(&changed.to_string()).is_err(), "{field}");
    }
    let mut changed = original.clone();
    changed.as_object_mut().unwrap().remove("transverse_readme");
    assert!(parse_acquisition(&changed.to_string()).is_err());
    let mut changed = original.clone();
    changed["transverse_readme"]["text"] = "compression is percent".into();
    changed["transverse_readme"]["sha256"] = sha256_hex(b"compression is percent").into();
    assert!(parse_acquisition(&changed.to_string()).is_err());
}
