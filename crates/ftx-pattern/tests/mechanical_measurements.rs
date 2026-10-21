//! Primary-source transcriptions preserve dimensions, evidence and unresolved specimen gaps.

use ftx_pattern::{
    material_catalog::{Selection, property_unit},
    materials::{Catalog, Evidence},
};

const IDS: [&str; 3] = [
    "fujii/2026/sheeting-a",
    "fujii/2026/oxford-b",
    "fujii/2026/twill-c",
];

fn selection(id: &str) -> Selection {
    Selection {
        id: id.into(),
        ..Selection::default()
    }
}

#[test]
fn source_shear_and_construction_normalize_without_changing_axes() {
    let catalog = Catalog::builtin().unwrap();
    // Independent Table 1/Table 3 known answers: x is weft and y is warp.
    let expected = [
        (
            0.000_660, 2480., 2260., 64.7262, 98_070., 295_900., 174_900.,
        ),
        (
            0.000_822, 2300., 1420., 215.4462, 262_100., 652_000., 287_900.,
        ),
        (
            0.000_769, 4500., 2340., 72.64743, 94_470., 325_300., 158_100.,
        ),
    ];
    for (id, (h, warp, weft, shear, source_shear, bx, by)) in IDS.into_iter().zip(expected) {
        let resolved = catalog.resolve(&selection(id)).unwrap();
        let material = resolved.material();
        for (key, unit, value) in [
            ("thickness", "m", h),
            ("thread_density_warp", "1/m", warp),
            ("thread_density_weft", "1/m", weft),
            ("shear_stiffness", "N/m", shear),
            ("fujii_shear_low", "Pa", source_shear),
            ("fujii_kes_bending_weft", "Pa", bx),
            ("fujii_kes_bending_warp", "Pa", by),
        ] {
            let actual = material.require(key, unit, false).unwrap();
            assert!((actual - value).abs() < 1e-10, "{id}: {key}");
        }
        assert!((shear - source_shear * h).abs() < 1e-10);
        assert_eq!(resolved.authored().properties["thickness"].unit, "mm");
        let slope = &material.properties["shear_stiffness"];
        assert_eq!(slope.conditions["slope_interval_degrees"], "0.5 to 2.5");
        assert_eq!(slope.evidence, Evidence::Measured);
        assert!(slope.method.contains("Eq (43)"));
        let thickness = &material.properties["thickness"];
        assert_eq!(thickness.conditions["pressure_gf_cm2"], "0.5");
    }
}

#[test]
fn original_fits_and_ambiguous_units_never_complete_a_mechanical_law() {
    let catalog = Catalog::builtin().unwrap();
    for (id, weight) in IDS.into_iter().zip(["0.0142", "0.0222", "0.0216"]) {
        let mut selected = selection(id);
        selected.allow_estimates = true;
        let resolved = catalog.resolve(&selected).unwrap();
        let material = resolved.material();
        assert_eq!(material.properties.len(), 33);
        assert_eq!(
            material.structure["source_weight_value_without_unit"],
            weight
        );
        for absent in [
            "areal_density",
            "warp_stiffness",
            "weft_stiffness",
            "poisson_warp_weft",
            "bending_rigidity",
            "bending_warp",
            "bending_weft",
            "bending_coupling",
            "bending_twist",
            "friction_skin",
            "friction_self",
        ] {
            assert!(!material.properties.contains_key(absent), "{id}: {absent}");
        }
        let bending_scope = &material.structure["bending_scope"];
        assert!(bending_scope.contains("10^-1"));
        assert!(bending_scope.contains("twisting was not measured"));
        assert!(resolved.mechanics().is_err());
        assert!(resolved.membrane_response().unwrap().is_none());
        assert!(resolved.bending_response().unwrap().is_none());
        assert!(resolved.thermal_resistance().is_err());
        for (key, quantity) in &material.properties {
            if !key.starts_with("fujii_") {
                continue;
            }
            if key != "fujii_shear_low" && !key.starts_with("fujii_kes_bending_") {
                assert_eq!(quantity.evidence, Evidence::Estimate, "{id}: {key}");
                assert!(material.require(key, &quantity.unit, false).is_err());
                let permitted = material.require(key, &quantity.unit, true).unwrap();
                assert_eq!(permitted, quantity.value);
            }
            assert_eq!(property_unit(key), None, "source model extension: {key}");
        }
    }
    let oxford = catalog.get(IDS[1]).unwrap();
    assert_eq!(oxford.structure["cotton_count_warp_reported"], "20//2");
    for (key, expected) in [
        ("fujii_tensile_weft_a", 214.8),
        ("fujii_tensile_warp_d", -138_600_000.),
        ("fujii_shear_a", -0.007_293),
        ("fujii_poisson_xy_corrected", 0.049_14),
    ] {
        assert_eq!(oxford.properties[key].value, expected, "{key}");
    }
}

#[test]
fn frozen_source_records_preserve_conditions_coefficients_and_missing_units() {
    let catalog = Catalog::builtin().unwrap();
    let resolved = catalog.resolve(&selection(IDS[2])).unwrap();
    let frozen = resolved.frozen_selection();
    let encoded = serde_json::to_string(&frozen).unwrap();
    let replay: Selection = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        catalog.resolve(&replay).unwrap().sha256(),
        resolved.sha256()
    );
    assert!(encoded.contains("10.1007/s00366-026-02343-z"));
    assert!(encoded.contains("source_weight_value_without_unit"));
    let mut changed = replay.clone();
    changed
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("fujii_tensile_weft_c")
        .unwrap()
        .value += 1.;
    assert!(catalog.resolve(&changed).is_err());
    changed = replay;
    changed
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("shear_stiffness")
        .unwrap()
        .conditions
        .insert("slope_interval_degrees".into(), "different interval".into());
    assert!(catalog.resolve(&changed).is_err());
}
