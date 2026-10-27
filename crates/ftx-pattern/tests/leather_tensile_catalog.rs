//! Released specimen observations reach ordinary screening without inventing a cloth law.
use ftx_pattern::{
    material_catalog::Selection,
    material_screen::Request,
    material_thermal::{ConductionLaw, ThermalLaw},
    materials::{Catalog, Evidence, Form},
};
use serde_json::json;

const PREFIX: &str = "schneider/2026/bovine-";

fn selected(catalog: &Catalog, suffix: &str) -> ftx_pattern::material_catalog::ResolvedMaterial {
    catalog
        .resolve(&Selection {
            id: format!("{PREFIX}{suffix}"),
            ..Selection::default()
        })
        .unwrap()
}

#[test]
fn primary_bovine_trial_rows_keep_original_fields_and_units() {
    let catalog = Catalog::builtin().unwrap();
    let rows: Vec<_> = catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with(PREFIX))
        .collect();
    assert_eq!(rows.len(), 10);
    assert_eq!(rows.iter().map(|m| m.properties.len()).sum::<usize>(), 40);
    // Independently transcribed Results Table 1, sheets Bovine D1/D2, rows 4..8.
    for (suffix, thickness, maximum, at_break, displacement) in [
        ("d1-1", 1.90, 19.24, 18.44, 39.87),
        ("d1-2", 1.96, 17.68, 16.70, 38.37),
        ("d1-3", 1.94, 18.55, 17.15, 38.51),
        ("d1-4", 1.97, 19.21, 17.83, 37.47),
        ("d1-5", 1.92, 19.39, 17.47, 38.21),
        ("d2-1", 1.93, 22.31, 21.55, 42.17),
        ("d2-2", 1.99, 19.42, 18.56, 43.64),
        ("d2-3", 1.92, 18.52, 17.69, 37.11),
        ("d2-4", 1.98, 17.71, 16.80, 35.64),
        ("d2-5", 1.91, 17.83, 16.40, 36.44),
    ] {
        let record = selected(&catalog, suffix);
        let material = record.authored();
        assert_eq!(material.form, Form::Hide);
        assert!(!material.composition_complete);
        assert_eq!(material.properties["reported_thickness"].value, thickness);
        assert_eq!(material.properties["reported_thickness"].unit, "mm");
        assert_eq!(material.properties["tensile_maximum_force"].value, maximum);
        assert_eq!(material.properties["tensile_maximum_force"].unit, "kgf");
        assert_eq!(
            material.properties["tensile_force_at_break"].value,
            at_break
        );
        assert_eq!(
            material.properties["tensile_displacement_at_break"].value,
            displacement
        );
        assert!(
            material
                .properties
                .values()
                .all(|q| q.evidence == Evidence::Measured && q.bounds.is_none())
        );
        assert!(
            material.sources["workbook"]
                .note
                .contains("56794ef58b20d7da55c2f9155561ee0a38784dbe424b2581266ac034919f387d")
        );
    }
    let first = selected(&catalog, "d1-1");
    let p = &first.material().properties;
    assert!((p["tensile_maximum_force"].value - 188.679_946).abs() < 1e-12);
    assert!((p["tensile_force_at_break"].value - 180.834_626).abs() < 1e-12);
    assert_eq!(p["tensile_maximum_force"].unit, "N");
    assert!((p["reported_thickness"].value - 0.00190).abs() < 1e-15);
    assert!((p["tensile_displacement_at_break"].value - 0.03987).abs() < 1e-15);
    assert_eq!(p["tensile_displacement_at_break"].unit, "m");
    assert_eq!(
        p["tensile_maximum_force"].conditions["workbook_cell"],
        "'Bovine D1'!G4"
    );
}

#[test]
fn force_screen_selects_actual_trial_and_keeps_source_directions() {
    let catalog = Catalog::builtin().unwrap();
    let mut request: Request = serde_json::from_value(json!({
        "schema":"ftx-material-screen/1", "basis":"nominal", "query":PREFIX,
        "constraints":[{"attribute":{"key":"tensile_maximum_force","unit":"N",
            "conditions":{"source_direction":"D2"}}, "minimum":200.0}]
    }))
    .unwrap();
    let report = request.evaluate(&catalog).unwrap();
    assert_eq!(report.frontier, vec![format!("{PREFIX}d2-1")]);
    // A source D1 trial is not silently treated as a warp-direction observation.
    request.constraints[0]
        .attribute
        .conditions
        .insert("source_direction".into(), "warp".into());
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
    request.constraints[0]
        .attribute
        .conditions
        .insert("source_direction".into(), "D1".into());
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
}

#[test]
fn frozen_observations_replay_offline_but_conflicting_geometry_does_not_admit_physics() {
    let catalog = Catalog::builtin().unwrap();
    let original = selected(&catalog, "d2-1");
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    let frozen = original.frozen_selection();
    let replay = empty.resolve(&frozen).unwrap();
    assert_eq!(replay.sha256(), original.sha256());
    let material = replay.material();
    assert!(
        material.properties["tensile_maximum_force"].conditions["geometry_discrepancy"]
            .contains("33 mm")
    );
    assert!(
        material.properties["tensile_maximum_force"].conditions["geometry_discrepancy"]
            .contains("65 mm")
    );
    assert!(
        material.properties["reported_thickness"].conditions["measurement_pressure"]
            .contains("not reported")
    );
    assert!(
        material.properties["tensile_displacement_at_break"].conditions["displacement_scope"]
            .contains("crosshead")
    );
    for absent in [
        "areal_density",
        "thickness",
        "warp_stiffness",
        "weft_stiffness",
        "shear_stiffness",
        "poisson_warp_weft",
        "bending_rigidity",
        "friction_skin",
        "elongation_break_warp",
        "elongation_break_weft",
    ] {
        assert!(!material.properties.contains_key(absent));
    }
    assert!(material.membrane_response.is_none());
    assert!(material.membrane_relaxation.is_none());
    assert!(replay.mechanics().is_err());
    assert!(ConductionLaw::from_material(material, true).is_err());
    assert!(ThermalLaw::from_material(material, true).is_err());
    let mut changed = frozen;
    changed
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("tensile_maximum_force")
        .unwrap()
        .value += 1.0;
    assert!(empty.resolve(&changed).is_err());
}
