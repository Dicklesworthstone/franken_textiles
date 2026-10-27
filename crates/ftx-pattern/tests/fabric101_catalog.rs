//! Exact release observations, ordinary discovery, and immutable replay without invented mechanics.
use ftx_pattern::{
    material_catalog::Selection,
    material_screen::{Capability, Request},
    materials::{CATALOG_SCHEMA, Catalog, Evidence, Form},
};
use serde_json::json;

const PREFIX: &str = "fabric101-2026/";
const REVISION: &str = "510519be0657a341a1a84fdb5c24be156ac83c9f";

fn selection(id: &str) -> Selection {
    Selection {
        id: format!("{PREFIX}{id}"),
        ..Selection::default()
    }
}

#[test]
fn release_subset_excludes_conflicting_and_unreleased_identities() {
    let catalog = Catalog::builtin().unwrap();
    let rows: Vec<_> = catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with(PREFIX))
        .collect();
    assert_eq!(rows.len(), 96);
    assert_eq!(rows.iter().map(|m| m.properties.len()).sum::<usize>(), 192);
    assert_eq!(rows.iter().filter(|m| m.form == Form::Knit).count(), 9);
    for excluded in [
        "drape-4", "e-kn06", "ex-03", "r04", "ex-15", "paper-2", "paper-3", "cotton", "linen",
        "wool", "pet", "silk",
    ] {
        assert!(catalog.get(&format!("{PREFIX}{excluded}")).is_err());
    }
    let cotton = catalog.get(&selection("c01").id).unwrap();
    assert_eq!(
        cotton.structure["raw_metadata_path"],
        "data/C01/002/head.dat"
    );
    assert_eq!(cotton.structure["specimen"], "110_C01_002");
    assert_eq!(
        catalog.get(&selection("drape-12").id).unwrap().structure["raw_metadata_path"],
        "data/Drape 12/002/head.dat"
    );
}

#[test]
fn independently_checked_release_rows_preserve_original_and_canonical_units() {
    let catalog = Catalog::builtin().unwrap();
    // Exact release CSV values, independently checked against its own raw instrument header.
    for (id, gsm, mm) in [
        ("c01", 134.84613212874083, 0.7),
        ("l01", 117.3836096362914, 0.4),
        ("l03", 65.770_939_430_057_42, 0.3),
        ("kn01", 174.33653303218523, 0.7),
        ("e-c05", 102.97251417632825, 0.4),
        ("ex-08", 128.73440285204993, 0.6),
        ("ex-05", 251.12299465240642, 0.6),
        ("e-f02", 126.12417379997834, 0.2),
        ("s08", 85.561_497_326_203_21, 0.5),
        ("e-kn07", 321.03, 1.2),
        ("drape-8", 240.25974025974025, 0.8),
    ] {
        let resolved = catalog.resolve(&selection(id)).unwrap();
        let original = resolved.authored();
        assert_eq!(original.properties["areal_density"].value, gsm, "{id}");
        assert_eq!(original.properties["areal_density"].unit, "g/m2");
        assert_eq!(original.properties["reported_thickness"].value, mm, "{id}");
        assert_eq!(original.properties["reported_thickness"].unit, "mm");
        let normalized = resolved.material();
        assert!(
            (normalized.require("areal_density", "kg/m2", false).unwrap() - gsm * 0.001).abs()
                < 1e-15
        );
        assert!(
            (normalized
                .require("reported_thickness", "m", false)
                .unwrap()
                - mm * 0.001)
                .abs()
                < 1e-15
        );
        assert!(normalized.require("thickness", "m", false).is_err());
    }
}

#[test]
fn descriptors_and_composition_never_cross_conflicting_article_rows() {
    let catalog = Catalog::builtin().unwrap();
    let get = |id| catalog.get(&selection(id).id).unwrap();
    assert_eq!(get("l01").form, Form::Woven);
    assert_eq!(get("kn01").form, Form::Knit);
    assert_eq!(get("ex-08").form, Form::Knit);
    assert_eq!(get("e-f02").form, Form::Laminate);
    assert_eq!(get("e-f01").form, Form::Sheet);
    assert_eq!(get("e-c05").composition[0].fiber, "polyester");
    assert_eq!(get("e-c05").composition[0].fraction, 0.65);
    assert_eq!(get("e-c05").composition[1].fraction, 0.35);
    assert_eq!(
        get("ex-05").composition[0].fiber,
        "viscose from bamboo (regenerated cellulose)"
    );
    assert_eq!(get("ex-17").composition[0].fraction, 0.70);
    assert_eq!(get("ex-17").composition[1].fraction, 0.30);
    for id in [
        "s08", "s09", "s10", "s11", "s12", "e-kn07", "ex-06", "e-l01", "e-f02",
    ] {
        assert!(get(id).composition.is_empty(), "{id}");
        assert!(!get(id).composition_complete, "{id}");
    }
    assert!(get("s08").structure["source_discrepancy"].contains("different brocade"));
    assert!(get("e-kn07").structure["source_discrepancy"].contains("231.71"));
    assert!(get("l01").structure["source_discrepancy"].contains("16.14125"));
}

#[test]
fn every_record_is_source_bounded_frozen_replayable_and_incomplete_for_physics() {
    let catalog = Catalog::builtin().unwrap();
    let empty = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    for row in catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with(PREFIX))
    {
        assert!(row.sources["release"].reference.contains(REVISION));
        assert!(
            row.sources["release"]
                .note
                .contains("6a460054c3031554733c7706a8327c89e323ff4a4657352f5bcec0494d7cf96a")
        );
        assert!(
            row.sources["mapping"]
                .note
                .contains("57d7e0225b37ee7ca556b91f1e20d0d89450dd1927f3992ac467a42b4d36933c")
        );
        assert_eq!(row.structure["raw_metadata_git_blob_sha1"].len(), 40);
        for source in row.sources.values() {
            assert_eq!(source.accessed_on, "2026-10-27");
            assert!(
                [&source.reference, &source.note]
                    .iter()
                    .all(|s| !s.is_empty() && s.len() <= 4096)
            );
        }
        assert!(
            row.structure
                .values()
                .all(|v| !v.is_empty() && v.len() <= 4096)
        );
        for q in row.properties.values() {
            assert_eq!(q.evidence, Evidence::Measured);
            assert!(q.bounds.is_none());
            assert!(q.conditions.len() <= 64);
            assert!(
                q.conditions
                    .values()
                    .all(|v| !v.is_empty() && v.len() <= 4096)
            );
            assert!(q.conditions["conditioning"].contains("24 h"));
            assert_eq!(q.conditions["specimen"], row.structure["specimen"]);
        }
        let selected = catalog
            .resolve(&Selection {
                id: row.id.clone(),
                ..Selection::default()
            })
            .unwrap();
        let frozen = selected.frozen_selection();
        let replay = empty.resolve(&frozen).unwrap();
        assert_eq!(replay.sha256(), selected.sha256());
        assert_eq!(
            serde_json::to_value(replay.authored()).unwrap(),
            serde_json::to_value(row).unwrap()
        );
        assert!(selected.mechanics().is_err());
        assert!(selected.thermal_resistance().is_err());
        assert!(row.membrane_response.is_none());
        assert!(row.membrane_relaxation.is_none());
        assert!(row.thermal_response.is_none());
        for absent in [
            "thickness",
            "warp_stiffness",
            "weft_stiffness",
            "shear_stiffness",
            "bending_rigidity",
            "friction_self",
            "specific_heat",
            "thermal_resistance",
        ] {
            assert!(!row.properties.contains_key(absent), "{}: {absent}", row.id);
        }
        let mut changed = frozen;
        changed
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("areal_density")
            .unwrap()
            .value += 1.0;
        assert!(
            empty.resolve(&changed).is_err(),
            "changed snapshot must fail the digest lock"
        );
    }
}

#[test]
fn ordinary_screening_uses_measured_mass_and_refuses_unsupplied_mechanics() {
    let catalog = Catalog::builtin().unwrap();
    let mut request: Request = serde_json::from_value(json!({
        "schema":"ftx-material-screen/1", "basis":"nominal", "allow_mixed_protocols":true,
        "candidates":[
            {"key":"linen", "selection":selection("l01")},
            {"key":"performance-jersey", "selection":selection("ex-08")}
        ],
        "constraints":[{"attribute":{"key":"areal_density","unit":"kg/m2"},"maximum":0.12}],
        "objectives":[{"attribute":{"key":"areal_density","unit":"kg/m2"},"direction":"minimize"}]
    }))
    .unwrap();
    let report = request.evaluate(&catalog).unwrap();
    assert_eq!(report.frontier, ["linen"]);
    assert_eq!(report.alternatives.iter().filter(|a| a.eligible).count(), 1);
    assert!(
        report.alternatives[0]
            .selection
            .as_ref()
            .unwrap()
            .record
            .is_some()
    );
    assert!(!report.physical_validation_claim);
    request.constraints.clear();
    request.required_capabilities = vec![Capability::Mechanics];
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
}

#[test]
fn jersey_discovery_matches_specimens_without_generic_source_note_leakage() {
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Catalog::builtin()
            .unwrap()
            .materials
            .into_iter()
            .filter(|m| m.id.starts_with(PREFIX))
            .collect(),
    };
    let request: Request = serde_json::from_value(json!({
        "schema":"ftx-material-screen/1", "basis":"nominal", "query":"jersey",
        "constraints":[], "objectives":[]
    }))
    .unwrap();
    let report = request.evaluate(&catalog).unwrap();
    let mut selected: Vec<_> = report
        .alternatives
        .iter()
        .filter(|a| a.eligible)
        .map(|a| a.material_id.as_str())
        .collect();
    selected.sort();
    assert_eq!(
        selected,
        [
            "fabric101-2026/e-kn01",
            "fabric101-2026/ex-04",
            "fabric101-2026/ex-08",
            "fabric101-2026/kn01"
        ]
    );
}
