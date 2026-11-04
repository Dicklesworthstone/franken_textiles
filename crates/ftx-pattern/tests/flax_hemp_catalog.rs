//! Published construction variants feed existing selection, mass and qualified screening consumers.
use ftx_pattern::{
    Pattern,
    material_binding::Changes,
    material_catalog::{ResolvedMaterial, Selection},
    material_screen::{Basis, Capability, Request},
    material_thermal::{ConductionLaw, ThermalLaw},
    materials::{Catalog, Evidence, Form},
    textile_import,
};
use serde_json::{Value, json};

const PREFIX: &str = "vasile/2024/";

fn selection(slug: &str) -> Selection {
    Selection {
        id: format!("{PREFIX}{slug}"),
        ..Selection::default()
    }
}

fn selected(catalog: &Catalog, slug: &str) -> ResolvedMaterial {
    catalog.resolve(&selection(slug)).unwrap()
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
}

#[test]
fn primary_rows_retain_original_means_spread_and_construction() {
    let catalog = Catalog::builtin().unwrap();
    let rows: Vec<_> = catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with(PREFIX))
        .collect();
    assert_eq!(rows.len(), 6);
    assert_eq!(rows.iter().map(|m| m.properties.len()).sum::<usize>(), 36);
    // Independently checked against Table 2 (PDF p6) and Table 4 (p10), not inferred
    // from fiber family, rounded bulk density or the plotted mechanical responses.
    for (slug, mass, gauge, density, cycles, mass_sd, gauge_sd, density_sd) in [
        (
            "hemp-plain",
            193.2,
            0.52,
            375.2,
            5000.0,
            "2.1 g/m2",
            "0.01 mm",
            "10.6 kg/m3",
        ),
        (
            "hemp-pan",
            181.3,
            0.54,
            338.4,
            4000.0,
            "2.8 g/m2",
            "0.01 mm",
            "9.7 kg/m3",
        ),
        (
            "hemp-twill",
            184.0,
            0.55,
            333.4,
            6000.0,
            "3.2 g/m2",
            "0.01 mm",
            "8.6 kg/m3",
        ),
        (
            "flax-plain",
            199.7,
            0.51,
            392.0,
            5000.0,
            "1.7 g/m2",
            "0.01 mm",
            "10.8 kg/m3",
        ),
        (
            "flax-pan",
            188.9,
            0.55,
            346.2,
            4000.0,
            "1.9 g/m2",
            "0.02 mm",
            "9.7 kg/m3",
        ),
        (
            "flax-twill",
            188.9,
            0.54,
            350.1,
            6000.0,
            "2.6 g/m2",
            "0.01 mm",
            "5.9 kg/m3",
        ),
    ] {
        let record = selected(&catalog, slug);
        let authored = record.authored();
        let material = record.material();
        assert_eq!(authored.form, Form::Woven);
        for (key, original, unit, normalized, si, evidence, sd) in [
            (
                "areal_density",
                mass,
                "g/m2",
                mass * 0.001,
                "kg/m2",
                Evidence::Measured,
                mass_sd,
            ),
            (
                "reported_thickness",
                gauge,
                "mm",
                gauge * 0.001,
                "m",
                Evidence::Measured,
                gauge_sd,
            ),
            (
                "bulk_density",
                density,
                "kg/m3",
                density,
                "kg/m3",
                Evidence::Published,
                density_sd,
            ),
        ] {
            let raw = &authored.properties[key];
            let normalized_q = &material.properties[key];
            assert_eq!(raw.value, original);
            assert_eq!(raw.unit, unit);
            near(normalized_q.value, normalized);
            assert_eq!(normalized_q.unit, si);
            assert_eq!(normalized_q.evidence, evidence);
            assert_eq!(normalized_q.conditions["reported_standard_deviation"], sd);
            assert!(normalized_q.bounds.is_none());
        }
        assert_eq!(material.properties["abrasion_cycles"].value, cycles);
        assert_eq!(
            material.properties["abrasion_cycles"].evidence,
            Evidence::Measured
        );
        assert_eq!(authored.properties["thread_density_warp"].value, 24.0);
        assert_eq!(authored.properties["thread_density_weft"].value, 23.0);
        assert_eq!(authored.properties["thread_density_weft"].unit, "1/cm");
        near(material.properties["thread_density_warp"].value, 2400.0);
        near(material.properties["thread_density_weft"].value, 2300.0);
        assert_eq!(material.properties["thread_density_warp"].unit, "1/m");
        assert!(material.properties.values().all(|q| q.bounds.is_none()));
        if slug.starts_with("hemp") {
            assert!(!material.composition_complete);
            assert!(material.composition.is_empty());
            assert!(material.structure["composition_scope"].contains("assumes 50:50"));
        } else {
            assert!(material.composition_complete);
            assert_eq!(material.composition.len(), 1);
            assert_eq!(material.composition[0].fiber, "flax (linen)");
            assert_eq!(material.composition[0].fraction, 1.0);
        }
    }
}

#[test]
fn gauge_pressure_and_abrasion_protocol_survive_frozen_replay_without_new_capabilities() {
    let original = selected(&Catalog::builtin().unwrap(), "hemp-plain");
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    let frozen = original.frozen_selection();
    let replay = empty.resolve(&frozen).unwrap();
    assert_eq!(original.sha256(), replay.sha256());
    let material = replay.material();
    let gauge = &material.properties["reported_thickness"];
    let abrasion = &material.properties["abrasion_cycles"];
    assert_eq!(gauge.conditions["pressure_pa"], "1000");
    assert_eq!(gauge.conditions["sample_count"], "10");
    assert_eq!(abrasion.conditions["pressure_pa"], "12000");
    assert_eq!(abrasion.conditions["sample_count"], "4");
    assert_eq!(abrasion.conditions["inspection_interval_cycles"], "1000");
    assert_eq!(abrasion.conditions["test_limit_cycles"], "6000");
    assert_eq!(abrasion.conditions["test_standard"], "ISO 12947-2:2016");
    assert!(
        material.properties["areal_density"].conditions["mass_basis"].starts_with("Conditioned")
    );
    assert!(gauge.conditions["conditioning"].contains("65 +/- 4"));
    assert!(material.sources["paper"].reference.ends_with("ma17071650"));
    assert_eq!(
        material.structure["source_pdf_sha256"],
        "823e79cdf4ebf902bfd4443524361817f9ca5610b041cf4b65ae3937a363849b"
    );
    assert!(replay.unloaded_thickness().is_err());
    for allow_estimates in [false, true] {
        assert!(material.mechanics(allow_estimates).is_err());
        assert!(ConductionLaw::from_material(material, allow_estimates).is_err());
        assert!(ThermalLaw::from_material(material, allow_estimates).is_err());
    }
    for absent in [
        "thickness",
        "unloaded_thickness",
        "width",
        "warp_stiffness",
        "bending_rigidity",
        "friction_skin",
        "thermal_conductivity",
        "specific_heat",
        "air_permeability",
    ] {
        assert!(!material.properties.contains_key(absent), "{absent}");
    }
    let mut changed = frozen;
    changed
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("reported_thickness")
        .unwrap()
        .conditions
        .insert("pressure_pa".into(), "0".into());
    assert!(empty.resolve(&changed).is_err());
}

#[test]
fn unchanged_pattern_substitution_reaches_the_shared_mass_consumer() {
    let raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let pattern = Pattern::new(raw.clone());
    let mut outcomes = Vec::new();
    for (slug, expected_mass_kg) in [("flax-plain", 0.000_798_8), ("hemp-twill", 0.000_736)] {
        let report = Changes {
            schema: "ftx-material-bindings/1".into(),
            default: Some(selection(slug)),
            ..Changes::default()
        }
        .apply(&pattern)
        .unwrap();
        assert_eq!(report["geometry_changed"], false);
        assert_eq!(report["pattern"]["pieces"], raw["pieces"]);
        assert_eq!(report["pattern"]["seams"], raw["seams"]);
        let material = textile_import::resolve_fabric(&report["pattern"]["fabric"])
            .unwrap()
            .unwrap();
        assert_eq!(material.authored().id, format!("{PREFIX}{slug}"));
        let (areal_mass, bounds) = material.material().sheet_areal_mass(false).unwrap();
        // The existing synthetic fixture has two 20 x 100 mm rectangles: 0.004 m2.
        // This is source propagation and mass arithmetic, not experimental garment validation.
        near(areal_mass * 0.004, expected_mass_kg);
        assert!(bounds.is_none());
        outcomes.push((expected_mass_kg, material.sha256().to_string()));
    }
    assert!(outcomes[0].0 > outcomes[1].0);
    assert_ne!(outcomes[0].1, outcomes[1].1);
}

#[test]
fn ordinary_screening_uses_actual_abrasion_method_and_never_promotes_sd_to_bounds() {
    let catalog = Catalog::builtin().unwrap();
    let mut request: Request = serde_json::from_value(json!({
        "schema":"ftx-material-screen/1", "basis":"nominal", "query":PREFIX,
        "constraints":[
            {"attribute":{"key":"abrasion_cycles","unit":"cycles",
                "conditions":{"test_standard":"ISO 12947-2:2016","pressure_pa":"12000"}}, "minimum":6000.0},
            {"attribute":{"key":"areal_density","unit":"kg/m2"}, "maximum":0.185}
        ]
    })).unwrap();
    let report = request.evaluate(&catalog).unwrap();
    assert_eq!(report.frontier, [format!("{PREFIX}hemp-twill")]);
    assert!(!report.physical_validation_claim);
    request.basis = Basis::ReportedBounds;
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
    request.basis = Basis::Nominal;
    request.required_capabilities = vec![Capability::Mechanics];
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
    request.required_capabilities.clear();
    request.constraints[0]
        .attribute
        .conditions
        .insert("pressure_pa".into(), "9000".into());
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
}
