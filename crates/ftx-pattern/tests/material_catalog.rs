//! Source-preserving catalog selection extends the existing material model, not a second physics API.
use ftx_pattern::{
    material_catalog::{Selection, normalize},
    materials::Catalog,
};
use serde_json::{Value, json};

fn full() -> Catalog {
    let mut raw = json!({"schema":"ftx-textile-catalog/1","materials":[{"id":"test","name":"Artificial sheet","form":"woven","sources":{"test":{"reference":"local:synthetic-test","accessed_on":"2026-10-13","note":"Synthetic known answers, not empirical fabric measurements"}},"properties":{}}]});
    for (key, value, unit) in [
        ("areal_density", 200., "g/m2"),
        ("thickness", 1., "mm"),
        ("warp_stiffness", 8000., "N/m"),
        ("weft_stiffness", 4000., "N/m"),
        ("shear_stiffness", 600., "N/m"),
        ("bending_rigidity", 3e-5, "N*m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("friction_skin", 0.4, "1"),
        ("friction_self", 0.3, "1"),
    ] {
        raw["materials"][0]["properties"][key] = json!({"value":value,"unit":unit,"evidence":"estimate","source":"test","method":"Synthetic fixture chosen for tests, not calibration","conditions":{"scope":"Artificial dry sheet, no empirical validity claim"}});
    }
    Catalog::from_json(&raw.to_string()).unwrap()
}
fn selection() -> Selection {
    Selection {
        id: "test".into(),
        allow_estimates: true,
        ..Selection::default()
    }
}

#[test]
fn factual_seed_does_not_invent_physics_or_midpoints() {
    let db = Catalog::builtin().unwrap();
    assert_eq!(db.materials.len(), 30);
    let linen = db.get("fs/il019-dusty-lotus-softened").unwrap();
    assert!(
        (linen.require("areal_density", "kg/m2", false).unwrap()
            - 5.3 * 0.028_349_523_125 / (0.9144 * 0.9144))
            .abs()
            < 1e-15
    );
    assert!(linen.mechanics(false).is_err());
    let leather = db.get("tandy/9114-84").unwrap();
    assert!(leather.require("thickness", "m", false).is_err());
    assert!(leather.structure.contains_key("thickness_reported_range"));
    assert!(
        !db.get("dharma/silk-charmeuse-19.5-momme")
            .unwrap()
            .properties
            .contains_key("thickness")
    );
}

#[test]
fn normalized_units_feed_existing_mechanics_and_thermal_consumers() {
    let mut db = full();
    let mut s = selection();
    s.allow_estimates = false;
    assert!(db.resolve(&s).unwrap().mechanics().is_err());
    s.allow_estimates = true;
    let m = db.resolve(&s).unwrap().mechanics().unwrap();
    assert_eq!(
        (m.areal_kg_m2, m.thickness_m, m.warp_n_m, m.weft_n_m),
        (0.2, 0.001, 8000., 4000.)
    );
    let mut q = db.materials[0].properties["thickness"].clone();
    q.value = 1.;
    q.unit = "clo".into();
    db.materials[0]
        .properties
        .insert("thermal_resistance".into(), q);
    assert_eq!(db.resolve(&s).unwrap().thermal_resistance().unwrap(), 0.155);
}

#[test]
fn overrides_preserve_originals_and_fingerprint_conditions_sources_and_policy() {
    let db = full();
    let before = serde_json::to_value(&db).unwrap();
    let s = selection();
    let a = db.resolve(&s).unwrap();
    let mut s2 = s.clone();
    let mut d = db.materials[0].properties["weft_stiffness"].clone();
    d.value = 2000.;
    s2.overrides.insert("weft_stiffness".into(), d);
    let b = db.resolve(&s2).unwrap();
    assert_ne!(a.sha256(), b.sha256());
    assert_eq!(serde_json::to_value(&db).unwrap(), before);
    assert_eq!(b.mechanics().unwrap().weft_n_m, 2000.);
    s2.overrides
        .get_mut("weft_stiffness")
        .unwrap()
        .conditions
        .insert("humidity".into(), "Different RH".into());
    assert_ne!(b.sha256(), db.resolve(&s2).unwrap().sha256());
    assert_eq!(a.sha256(), db.resolve(&s).unwrap().sha256());
    s2.sources
        .insert("test".into(), db.materials[0].sources["test"].clone());
    s2.sources.get_mut("test").unwrap().reference = "local:other".into();
    assert!(db.resolve(&s2).is_err());
}

#[test]
fn snapshot_roundtrip_is_locked_and_not_dependent_on_mutable_catalog() {
    let mut db = full();
    let r = db.resolve(&selection()).unwrap();
    let frozen = r.frozen_selection();
    let encoded = serde_json::to_string(&frozen).unwrap();
    let frozen: Selection = serde_json::from_str(&encoded).unwrap();
    db.materials[0]
        .properties
        .get_mut("thickness")
        .unwrap()
        .value = 500.;
    assert_eq!(r.sha256(), db.resolve(&frozen).unwrap().sha256());
    let mut changed = frozen.clone();
    changed
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thickness")
        .unwrap()
        .value = 2.;
    assert!(db.resolve(&changed).is_err());
    changed = frozen;
    changed.id = "other".into();
    assert!(db.resolve(&changed).is_err());
}

#[test]
fn wrong_dimensions_nonfinite_overflow_and_invalid_bounds_refuse() {
    for mutation in 0..5 {
        let mut db = full();
        let q = db.materials[0]
            .properties
            .get_mut("warp_stiffness")
            .unwrap();
        match mutation {
            0 => q.unit = "Pa".into(),
            1 => q.value = f64::INFINITY,
            2 => q.value = f64::NAN,
            3 => {
                q.value = f64::MAX;
                q.unit = "kN/m".into()
            }
            _ => q.bounds = Some([9000., 10000.]),
        }
        assert!(db.resolve(&selection()).is_err(), "mutation {mutation}");
    }
}

#[test]
fn unit_bounds_normalize_without_losing_original_observation() {
    let mut db = full();
    db.materials[0]
        .properties
        .get_mut("thickness")
        .unwrap()
        .bounds = Some([0.8, 1.2]);
    let r = db.resolve(&selection()).unwrap();
    assert_eq!(r.authored().properties["thickness"].unit, "mm");
    assert_eq!(
        r.authored().properties["thickness"].bounds,
        Some([0.8, 1.2])
    );
    assert_eq!(
        r.material().properties["thickness"].bounds,
        Some([0.0008, 0.0012])
    );
    let normalized = normalize(r.material()).unwrap();
    assert_eq!(
        serde_json::to_value(normalized).unwrap(),
        serde_json::to_value(r.material()).unwrap()
    );
}

#[test]
fn unknown_extensions_are_preserved_without_defaulting_missing_required_fields() {
    let mut raw = serde_json::to_value(full()).unwrap();
    let q = raw["materials"][0]["properties"]["weft_stiffness"].take();
    raw["materials"][0]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("weft_stiffness");
    raw["materials"][0]["properties"]["x-custom"] = q;
    let db = Catalog::from_json(&raw.to_string()).unwrap();
    let r = db.resolve(&selection()).unwrap();
    assert!(r.material().properties.contains_key("x-custom"));
    assert!(r.mechanics().is_err());
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(v["sha256"].is_string());
}
