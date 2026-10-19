//! Direct Table 4-9 observations and their use by existing steady thermal consumers.
use ftx_pattern::{
    material_catalog::{Selection, property_unit},
    material_compression,
    material_thermal::{ConductionLaw, ThermalLaw},
    materials::{Catalog, Evidence, Form, ThermalStack},
};

fn selection(weft: &str, moisture: &str) -> Selection {
    Selection {
        id: format!("tul/mangat-2012/denim-{weft}/mu-{moisture}"),
        ..Selection::default()
    }
}

#[test]
fn source_rows_preserve_units_moisture_basis_and_known_limits() {
    let catalog = Catalog::builtin().unwrap();
    assert_eq!(
        catalog
            .materials
            .iter()
            .filter(|m| m.id.starts_with("tul/mangat-2012/"))
            .count(),
        6
    );
    // Independently transcribed from the dissertation's Table 4-9, pages 90-93.
    for (weft, moisture, thickness_mm, r, k, b, page) in [
        ("cotton", "0.012", 0.86, 0.0128, 0.067, 197.0, 91),
        ("cotton", "0.447", 0.67, 0.0032, 0.209, 854.0, 92),
        ("spun-pp", "0.017", 0.91, 0.0149, 0.0609, 146.0, 90),
        ("spun-pp", "0.557", 0.79, 0.0048, 0.187, 992.0, 90),
        ("pet", "0.045", 0.78, 0.0108, 0.072, 193.0, 92),
        ("pet", "0.400", 0.67, 0.0040, 0.168, 751.0, 93),
    ] {
        let resolved = catalog.resolve(&selection(weft, moisture)).unwrap();
        let original = resolved.authored();
        let material = resolved.material();
        assert_eq!(material.form, Form::Woven);
        assert_eq!(original.properties["thickness"].value, thickness_mm);
        assert_eq!(original.properties["thickness"].unit, "mm");
        assert!(
            (material.require("thickness", "m", false).unwrap() - thickness_mm * 0.001).abs()
                < 1e-15
        );
        assert_eq!(
            material
                .require("thermal_resistance", "m2*K/W", false)
                .unwrap(),
            r
        );
        assert_eq!(
            material.properties["reported_thermal_conductivity"].value,
            k
        );
        assert_eq!(
            material.properties["reported_thermal_conductivity"].unit,
            "W/(m*K)"
        );
        assert_eq!(
            material.properties["reported_thermal_absorptivity"].value,
            b
        );
        assert_eq!(
            material.properties["water_mass_fraction_wet_basis"].value,
            moisture.parse::<f64>().unwrap()
        );
        for q in material.properties.values() {
            assert_eq!(q.evidence, Evidence::Measured);
            assert!(q.bounds.is_none());
            assert!(q.method.contains(&format!("page {page}")));
            assert_eq!(q.conditions["pressure_pa"], "1000");
            assert_eq!(q.conditions["water_mass_fraction_wet_basis"], moisture);
            assert!(q.conditions["moisture_definition"].contains("(Fw - Fd) / Fw"));
            assert!(q.conditions["laboratory_temperature"].contains("unit not printed"));
            assert_eq!(
                q.conditions["laboratory_relative_humidity_percent"],
                "20-25; section 3.5, page 71"
            );
            assert!(q.conditions["replicates_and_uncertainty"].contains("not supplied"));
            assert!(q.conditions["washing_and_lot"].contains("not identified"));
        }
        assert_eq!(
            material.sources["thesis"].reference,
            "https://dspace.tul.cz/bitstreams/57667674-5146-46c9-a46a-6aaeabbbedd0/download"
        );
        assert_eq!(material.sources["thesis"].accessed_on, "2026-10-19");
        assert!(
            material.sources["thesis"]
                .note
                .contains("Muhammad Mushtaq Ahmed Mangat (2012)")
        );
        assert_eq!(
            material.structure["compression_model"],
            "fixed_pressure_observation/1"
        );
        assert_eq!(material.structure["selected_pressure_pa"], "1000");
        assert_eq!(material.composition_complete, weft == "cotton");
        if weft == "cotton" {
            assert_eq!(material.composition[0].fiber, "cotton");
            assert_eq!(material.composition[0].fraction, 1.0);
        } else {
            assert!(material.composition.is_empty());
        }
        for absent in [
            "areal_density",
            "bulk_density",
            "specific_heat",
            "moisture_regain",
            "thermal_conductivity",
            "warp_stiffness",
            "friction_skin",
            "friction_self",
        ] {
            assert!(!material.properties.contains_key(absent));
        }
        assert!(material.thermal_response.is_none());
        assert!(ThermalLaw::from_material(material, true).is_err());
        assert!(material.mechanics(true).is_err());
    }
    for extension in [
        "reported_thermal_conductivity",
        "reported_thermal_absorptivity",
        "water_mass_fraction_wet_basis",
    ] {
        assert!(property_unit(extension).is_none());
    }
}

#[test]
fn wet_state_substitution_changes_heat_flow_using_the_reported_resistance() {
    let catalog = Catalog::builtin().unwrap();
    for (weft, low, high, low_r, high_r) in [
        ("cotton", "0.012", "0.447", 0.0128, 0.0032),
        ("spun-pp", "0.017", "0.557", 0.0149, 0.0048),
        ("pet", "0.045", "0.400", 0.0108, 0.0040),
    ] {
        let lower = catalog.resolve(&selection(weft, low)).unwrap();
        let higher = catalog.resolve(&selection(weft, high)).unwrap();
        let solve =
            |material| ThermalStack::evaluate(&[material], 0.6, 35.0, 20.0, 0.08, false).unwrap();
        let a = solve(lower.material());
        let b = solve(higher.material());
        // Area, temperatures and boundary film below are numerical choices, not source conditions.
        assert!((a.heat_flow_w - 9.0 / (0.08 + low_r)).abs() < 1e-10);
        assert!((b.heat_flow_w - 9.0 / (0.08 + high_r)).abs() < 1e-10);
        assert!(b.heat_flow_w > a.heat_flow_w);
        let law = ConductionLaw::from_material(higher.material(), false).unwrap();
        assert_eq!(law.resistance_at(25.0).unwrap(), high_r);
        assert!((law.flux_between(35.0, 20.0).unwrap() - 15.0 / high_r).abs() < 1e-10);
        // No estimate consent or reconstruction from independently rounded k and thickness.
        let derived_r = higher.material().require("thickness", "m", false).unwrap()
            / higher.material().properties["reported_thermal_conductivity"].value;
        assert!((derived_r - high_r).abs() > 1e-6);
        assert_ne!(lower.sha256(), higher.sha256());
    }
}

#[test]
fn state_and_manufacturing_limits_survive_freezing_without_an_invented_curve() {
    let catalog = Catalog::builtin().unwrap();
    let resolved = catalog.resolve(&selection("cotton", "0.447")).unwrap();
    let frozen = resolved.frozen_selection();
    assert_eq!(
        catalog.resolve(&frozen).unwrap().sha256(),
        resolved.sha256()
    );
    let mut tampered = frozen.clone();
    tampered
        .record
        .as_mut()
        .unwrap()
        .structure
        .remove("compression_model");
    assert!(catalog.resolve(&tampered).is_err());
    let mut tampered = frozen;
    tampered
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thermal_resistance")
        .unwrap()
        .conditions
        .insert("pressure_pa".into(), "200".into());
    assert!(catalog.resolve(&tampered).is_err());
    assert!(catalog.resolve(&selection("cotton", "0.100")).is_err());

    let mut input: material_compression::Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    input.material = resolved.authored().clone();
    let refused = material_compression::condition(&input).unwrap_err();
    assert!(
        refused
            .message
            .contains("original input before reconditioning")
    );
}
