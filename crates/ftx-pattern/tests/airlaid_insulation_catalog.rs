//! Independently transcribed article values and ordinary selection consumers.
//! These regressions establish software behavior, not measured garment fit or comfort.
use ftx_pattern::{
    Pattern,
    material_binding::Changes,
    material_catalog::Selection,
    material_screen::{Basis, Capability, Request},
    material_thermal::{ConductionLaw, ThermalLaw},
    materials::{Catalog, Evidence, Form, ThermalStack},
    textile_import,
};
use serde_json::{Value, json};

fn selection(slug: &str) -> Selection {
    Selection {
        id: format!("lupescu/2025/{slug}"),
        ..Selection::default()
    }
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
}

#[test]
fn table_values_normalize_source_units_without_reinterpreting_spread() {
    let db = Catalog::builtin().unwrap();
    // Tables 3 and 4: g/m², mm, mW/(m K), m² K/W. The source rounds R separately.
    let rows = [
        ("sample-1", 55.34, 5.05, 35.32, 0.143),
        ("sample-2", 60.75, 4.80, 33.13, 0.145),
        ("sample-3", 60.51, 5.12, 33.96, 0.151),
        ("sample-4", 161.45, 8.40, 32.81, 0.256),
        ("sample-5", 113.20, 6.65, 33.44, 0.199),
        ("sample-6", 170.15, 10.45, 33.65, 0.311),
        ("sample-7", 56.90, 4.55, 32.55, 0.140),
        ("thinsulate", 166.84, 5.02, 33.31, 0.151),
        ("primaloft", 86.86, 5.08, 34.14, 0.149),
    ];
    assert_eq!(
        db.materials
            .iter()
            .filter(|m| m.id.starts_with("lupescu/2025/"))
            .count(),
        rows.len()
    );
    for (slug, mass, thickness, conductivity, resistance) in rows {
        let resolved = db.resolve(&selection(slug)).unwrap();
        let authored = resolved.authored();
        let normalized = resolved.material();
        assert_eq!(authored.properties.len(), 12);
        for (key, raw, original_unit, si_unit, factor) in [
            ("areal_density", mass, "g/m2", "kg/m2", 0.001),
            ("reported_thickness", thickness, "mm", "m", 0.001),
            (
                "thermal_conductivity",
                conductivity,
                "mW/(m*K)",
                "W/(m*K)",
                0.001,
            ),
        ] {
            assert_eq!(authored.properties[key].value, raw);
            assert_eq!(authored.properties[key].unit, original_unit);
            assert_eq!(normalized.properties[key].unit, si_unit);
            near(normalized.properties[key].value, raw * factor);
            assert_eq!(
                authored.properties[key].conditions,
                normalized.properties[key].conditions
            );
            assert!(normalized.properties[key].bounds.is_none());
        }
        assert_eq!(resolved.thermal_resistance().unwrap(), resistance);
        assert_eq!(
            authored.sources["paper"].reference,
            "https://doi.org/10.3390/textiles5010005"
        );
        assert_eq!(
            authored.structure["source_pdf_sha256"],
            "21d5a283bb6ccc65b737d38a9e1f03ad8efcc8c471270bb45e1143a9893a3644"
        );
    }
    let mut bad = selection("sample-3");
    let mut record = db.get(&bad.id).unwrap().clone();
    record
        .properties
        .get_mut("thermal_conductivity")
        .unwrap()
        .unit = "mW/K".into();
    bad.record = Some(record);
    assert!(db.resolve(&bad).is_err());
}

#[test]
fn measured_scopes_do_not_manufacture_composition_mechanics_or_wet_laws() {
    let db = Catalog::builtin().unwrap();
    for m in db
        .materials
        .iter()
        .filter(|m| m.id.starts_with("lupescu/2025/"))
    {
        let resolved = db
            .resolve(&Selection {
                id: m.id.clone(),
                ..Selection::default()
            })
            .unwrap();
        let material = resolved.material();
        assert_eq!(material.form, Form::Nonwoven);
        assert!(material.properties.values().all(|q| q.bounds.is_none()));
        assert!(material.sheet_areal_mass(false).is_ok());
        assert!(ConductionLaw::from_material(material, false).is_ok());
        assert!(ThermalLaw::from_material(material, true).is_err());
        assert!(resolved.mechanics().is_err());
        for key in [
            "thickness",
            "specific_heat",
            "retained_liquid_areal_density",
            "free_liquid_areal_density",
        ] {
            assert!(!material.properties.contains_key(key));
        }
        assert!(material.thermal_response.is_none());
        assert_eq!(
            material.properties["reported_thickness"].conditions["pressure_kpa"],
            "0.1"
        );
        assert_eq!(
            material.properties["thermal_conductivity"].evidence,
            Evidence::Measured
        );
        assert_eq!(
            material.properties["thermal_resistance"].evidence,
            Evidence::Published
        );
        assert_eq!(
            material.properties["thermal_conductivity_rh90"].conditions["relative_humidity_percent"],
            "90"
        );
        assert_eq!(
            material.properties["thermal_conductivity_rh90"].unit,
            "mW/(m*K)"
        );
        assert!(
            !material.properties["thermal_conductivity"]
                .conditions
                .contains_key("relative_humidity_percent")
        );
        if m.id.contains("/sample-") {
            assert!(material.composition.is_empty());
            assert!(!material.composition_complete);
            assert!(
                material
                    .structure
                    .contains_key("nominal_feed_recipe_mass_percent")
            );
        } else {
            assert!(material.composition_complete);
            assert_eq!(material.composition[0].fiber, "polyester");
            assert_eq!(material.composition[0].fraction, 1.0);
            assert!(material.structure.contains_key("commercial_identity_scope"));
        }
    }
    let sample = db.resolve(&selection("sample-3")).unwrap();
    let p = &sample.material().properties;
    assert_eq!(
        p["areal_density"].conditions["reported_standard_deviation"],
        "1.56 g/m2"
    );
    assert_eq!(
        p["compression_ratio"].conditions["reported_plus_minus"],
        "2.9 %"
    );
    assert!(
        !p["compression_ratio"]
            .conditions
            .contains_key("reported_standard_deviation")
    );
    near(p["moisture_regain"].value, 0.198);
    assert_eq!(p["moisture_regain"].unit, "1");
    assert!(p["moisture_regain"].conditions["mass_basis"].contains("dry specimen mass"));
    assert!(
        p["short_term_compression_recovery"]
            .method
            .contains("J/E*100")
    );
    assert_eq!(p["thermal_conductivity_rh90"].value, 34.02);
}

#[test]
fn same_pattern_substitution_replays_source_mass_and_steady_heat_consumers() {
    let raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let pattern = Pattern::new(raw.clone());
    let mut results = Vec::new();
    // Two 20 by 100 mm rectangles in the unchanged synthetic pattern: 0.004 m² total.
    let area = 0.004;
    for (slug, areal_mass, resistance) in [
        ("sample-3", 0.06051, 0.151),
        ("thinsulate", 0.16684, 0.151),
        ("sample-6", 0.17015, 0.311),
    ] {
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
        let resolved = textile_import::resolve_fabric(&report["pattern"]["fabric"])
            .unwrap()
            .unwrap();
        assert!(report["pattern"]["fabric"]["material"]["expected_sha256"].is_string());
        assert_eq!(resolved.authored().id, selection(slug).id);
        let (mass, bounds) = resolved.material().sheet_areal_mass(false).unwrap();
        near(mass, areal_mass);
        assert!(bounds.is_none());
        let stack =
            ThermalStack::evaluate(&[resolved.material()], area, 37.0, 21.0, 0.0, false).unwrap();
        near(stack.resistance_m2_k_w, resistance);
        near(stack.heat_flow_w, area * 16.0 / resistance);
        let law = ConductionLaw::from_material(resolved.material(), false).unwrap();
        near(
            law.flux_between(37.0, 21.0).unwrap() * area,
            stack.heat_flow_w,
        );
        results.push((mass * area, stack.heat_flow_w));
    }
    assert!(results[0].0 < results[1].0);
    near(results[0].1, results[1].1); // Source reports the same rounded R, different mass.
    assert!(results[2].0 > results[1].0);
    assert!(results[2].1 < results[1].1);
}

#[test]
fn ordinary_screening_respects_units_conditions_and_missing_bounds() {
    let db = Catalog::builtin().unwrap();
    let mut request: Request = serde_json::from_value(json!({
        "schema":"ftx-material-screen/1", "basis":"nominal", "query":"lupescu/2025/",
        "required_capabilities":["steady_conduction"],
        "constraints":[
            {"attribute":{"key":"thermal_resistance", "unit":"m2*K/W",
                "conditions":{"warm_plate_temperature_c":"37", "cold_plate_temperature_c":"21"}}, "minimum":0.2},
            {"attribute":{"key":"areal_density", "unit":"kg/m2"}, "maximum":0.165}
        ]
    })).unwrap();
    assert_eq!(
        request.evaluate(&db).unwrap().frontier,
        ["lupescu/2025/sample-4"]
    );
    request.basis = Basis::ReportedBounds;
    assert!(request.evaluate(&db).unwrap().frontier.is_empty());
    request.basis = Basis::Nominal;
    request.required_capabilities = vec![Capability::TransientThermal];
    assert!(request.evaluate(&db).unwrap().frontier.is_empty());
    request.required_capabilities.clear();
    request.constraints = serde_json::from_value(json!([
        {"attribute":{"key":"thermal_conductivity_rh90", "unit":"mW/(m*K)",
            "conditions":{"relative_humidity_percent":"90"}}, "maximum":33.4}
    ]))
    .unwrap();
    assert_eq!(
        request.evaluate(&db).unwrap().frontier,
        [
            "lupescu/2025/sample-2",
            "lupescu/2025/sample-4",
            "lupescu/2025/sample-7"
        ]
    );
    request.constraints[0]
        .attribute
        .conditions
        .insert("relative_humidity_percent".into(), "65".into());
    assert!(request.evaluate(&db).unwrap().frontier.is_empty());
}

#[test]
fn frozen_records_replay_without_catalog_and_lock_source_conditions() {
    let db = Catalog::builtin().unwrap();
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    for m in db
        .materials
        .iter()
        .filter(|m| m.id.starts_with("lupescu/2025/"))
    {
        let resolved = db
            .resolve(&Selection {
                id: m.id.clone(),
                ..Selection::default()
            })
            .unwrap();
        let frozen = resolved.frozen_selection();
        let mut restored: Selection =
            serde_json::from_str(&serde_json::to_string(&frozen).unwrap()).unwrap();
        let replay = empty.resolve(&restored).unwrap();
        assert_eq!(replay.sha256(), resolved.sha256());
        assert_eq!(
            serde_json::to_value(replay.authored()).unwrap(),
            serde_json::to_value(resolved.authored()).unwrap()
        );
        assert_eq!(
            replay.thermal_resistance().unwrap(),
            resolved.thermal_resistance().unwrap()
        );
        restored
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("thermal_conductivity_rh90")
            .unwrap()
            .conditions
            .insert("relative_humidity_percent".into(), "65".into());
        assert!(empty.resolve(&restored).is_err());
    }
}
