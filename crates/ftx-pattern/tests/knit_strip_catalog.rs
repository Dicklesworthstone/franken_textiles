//! Exact primary-source grip forces support material screening without implying a homogeneous law.

use ftx_core::receipt::sha256_hex;
use ftx_pattern::{
    material_catalog::Selection,
    material_screen::{Basis, Capability, Request},
    materials::{Catalog, Evidence, Form},
};
use serde_json::json;

#[test]
fn matched_knits_retain_exact_directional_acquisitions_and_mass() {
    let catalog = Catalog::builtin().unwrap();
    let rows: Vec<_> = catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with("sperl2022/"))
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows.iter().map(|m| m.properties.len()).sum::<usize>(), 24);
    // Independently transcribed exact loading rows: warp, weft, bias at 10%, then 30%.
    for (code, mass, gauge, forces) in [
        ("SJ1", 0.159, 0.43, [[0.40, 0.04, 0.27], [1.94, 0.84, 1.12]]),
        ("SJ3", 0.238, 0.63, [[0.71, 0.80, 0.53], [2.11, 2.48, 1.78]]),
        (
            "DKIN8",
            0.267,
            0.62,
            [[1.21, 1.56, 1.15], [3.39, 5.00, 3.39]],
        ),
    ] {
        let resolved = catalog
            .resolve(&Selection {
                id: format!("sperl2022/{code}"),
                ..Selection::default()
            })
            .unwrap();
        let material = resolved.material();
        assert_eq!(material.form, Form::Knit);
        assert!((material.require("areal_density", "kg/m2", false).unwrap() - mass).abs() < 1e-15);
        assert_eq!(resolved.authored().properties["areal_density"].unit, "g/m2");
        assert_eq!(
            material
                .require("specimen_gauge_thickness", "mm", false)
                .unwrap(),
            gauge
        );
        assert!(!material.properties.contains_key("thickness"));
        assert!(material.composition_complete);
        assert!((material.composition.iter().map(|c| c.fraction).sum::<f64>() - 1.0).abs() < 1e-15);
        for (i, percent) in [10, 30].into_iter().enumerate() {
            for (j, direction) in ["warp", "weft", "bias"].into_iter().enumerate() {
                let key = format!("strip_force_{percent}pct_{direction}");
                let quantity = &material.properties[&key];
                assert_eq!(quantity.value, forces[i][j]);
                assert_eq!(quantity.unit, "N");
                assert_eq!(quantity.evidence, Evidence::Measured);
                assert!(quantity.bounds.is_none());
                assert_eq!(quantity.conditions["direction"], direction);
                assert_eq!(quantity.conditions["branch"], "loading");
                assert_eq!(quantity.conditions["free_gauge_length_mm"], "100");
                assert_eq!(quantity.conditions["reference_strip_width_mm"], "60");
                assert!(
                    material.sources[&quantity.source]
                        .reference
                        .ends_with(&format!("{code}/stretch-{direction}.csv"))
                );
            }
        }
        assert!(resolved.mechanics().is_err());
        assert!(resolved.thermal_resistance().is_err());
        assert!(material.membrane_response.is_none());
        assert!(material.membrane_relaxation.is_none());
        assert!(!material.properties.contains_key("shear_stiffness"));
        assert!(!material.properties.contains_key("bending_rigidity"));
        assert_eq!(
            catalog
                .resolve(&resolved.frozen_selection())
                .unwrap()
                .sha256(),
            resolved.sha256()
        );
    }
}

#[test]
fn original_source_bytes_and_license_replay_without_resampling() {
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../src/material_catalog/knit-strip-measurements.json"
    ))
    .unwrap();
    assert_eq!(
        data["metadata_sha256"],
        "258e3cc2a40ba40d1e0152f80b1fb08c67d14c08db10efbc2ec385669887a6d4"
    );
    assert_eq!(
        sha256_hex(data["metadata_json"].as_str().unwrap().as_bytes()),
        data["metadata_sha256"].as_str().unwrap()
    );
    assert!(
        data["license"]
            .as_str()
            .unwrap()
            .contains("Copyright (c) [2022] [Georg Sperl")
    );
    assert!(
        data["license"]
            .as_str()
            .unwrap()
            .contains("THE SOFTWARE IS PROVIDED")
    );
    let expected = [
        (
            "SJ1",
            "warp",
            "0f60fa0dbc3f99bcd0f0ba19c41f6988d7e8931bbfc6790003e12b326bddf0b6",
        ),
        (
            "SJ1",
            "weft",
            "0fe20ce39b5632ccf2437abc2646d70b5f65406fb5b19f8ea3fb804a26a26249",
        ),
        (
            "SJ1",
            "bias",
            "3a2abad579ebc91afbb2ae801365d69bc266b197ff5f11a9d737b63a38c55c1b",
        ),
        (
            "SJ3",
            "warp",
            "38332753d6bf437b5294f99e07c7cda87e1898c57d0be236891b17f662235688",
        ),
        (
            "SJ3",
            "weft",
            "ad9fa8d6a07a700348684813d5bf87212de2dba43a2577d2930922aec57780f9",
        ),
        (
            "SJ3",
            "bias",
            "fcb93afd8c919d3a6189f6622cb8fe8573a90afd09a04bb90b78b762dda327bf",
        ),
        (
            "DKIN8",
            "warp",
            "2e1e6c775fb40ac69df48eac521c6926463799753cf1d35a7e8bbf8999d34f59",
        ),
        (
            "DKIN8",
            "weft",
            "491b997ec5ce460c9f2018d7a791b63a3b656fa5654efb316212888cd3c3d10f",
        ),
        (
            "DKIN8",
            "bias",
            "d6b48f9f457932ffec430546f1660f4819fd316106b8bfc42aee3421e9f20365",
        ),
    ];
    for (curve, (code, direction, hash)) in data["curves"].as_array().unwrap().iter().zip(expected)
    {
        assert_eq!(curve["specimen"], code);
        assert_eq!(curve["direction"], direction);
        assert_eq!(curve["sha256"], hash);
        let raw = curve["csv"].as_str().unwrap();
        assert_eq!(sha256_hex(raw.as_bytes()), hash);
        assert!(raw.contains(",loading\r\n") && raw.contains(",unloading\r\n"));
        // Negative measured zero-load noise is retained, never clamped into a fabricated force law.
        assert!(raw.contains(",-0.01,"));
    }
}

#[test]
fn real_grip_forces_reach_existing_condition_qualified_screening() {
    let catalog = Catalog::builtin().unwrap();
    let mut request: Request = serde_json::from_value(json!({
        "schema":"ftx-material-screen/1", "basis":"nominal", "query":"sperl2022/",
        "forms":["knit"], "allow_mixed_protocols":true,
        "constraints":[{"attribute":{"key":"strip_force_30pct_weft","unit":"N",
            "conditions":{"branch":"loading","free_gauge_length_mm":"100","reference_strip_width_mm":"60"}},
            "minimum":2.0,"maximum":null}],
        "objectives":[{"attribute":{"key":"areal_density","unit":"kg/m2"},"direction":"minimize"}]
    })).unwrap();
    let report = request.evaluate(&catalog).unwrap();
    assert_eq!(report.frontier, ["sperl2022/SJ3"]);
    assert!(!report.physical_validation_claim);
    let winner = report
        .alternatives
        .iter()
        .find(|a| a.key == "sperl2022/SJ3")
        .unwrap();
    assert_eq!(winner.readings["strip_force_30pct_weft"].value, 2.48);
    assert!(winner.selection.as_ref().unwrap().expected_sha256.is_some());
    assert!(!winner.selection.as_ref().unwrap().allow_estimates);
    // Specimen/protocol equivalence is never inferred merely because the paper and rig match.
    request.allow_mixed_protocols = false;
    assert_eq!(
        request.evaluate(&catalog).unwrap().frontier,
        ["sperl2022/DKIN8", "sperl2022/SJ3"]
    );
    request.allow_mixed_protocols = true;
    request.basis = Basis::ReportedBounds;
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
    request.basis = Basis::Nominal;
    request.required_capabilities = vec![Capability::Mechanics];
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
    request.required_capabilities.clear();
    request.constraints[0]
        .attribute
        .conditions
        .insert("conditioning".into(), "20 C, 65% RH".into());
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
}
