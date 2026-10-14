//! Source and dimensional checks, not empirical textile observations.
use ftx_pattern::{
    material_bending::{BendingTensor, MODEL},
    material_catalog::Selection,
    materials::Catalog,
};
use serde_json::json;

fn selection() -> Selection {
    let mut properties = serde_json::Map::new();
    for (key, value) in [
        ("bending_warp", 2.0),
        ("bending_weft", 0.5),
        ("bending_coupling", -0.1),
        ("bending_twist", 0.3),
    ] {
        properties.insert(
            key.into(),
            json!({"value":value, "unit":"mN*m", "source":"test",
            "evidence":"estimate", "method":"synthetic tensor, not a measured textile"}),
        );
    }
    serde_json::from_value(json!({"id":"test", "allow_estimates":true, "record":{
        "id":"test", "name":"synthetic panel", "form":"woven", "structure":{"bending_model":MODEL},
        "properties":properties, "sources":{"test":{"reference":"test:bending", "accessed_on":"2026-10-14",
            "note":"synthetic known answer"}}}})).unwrap()
}

#[test]
fn source_units_and_negative_but_stable_coupling_survive_resolution() {
    let db = Catalog::builtin().unwrap();
    let m = db.resolve(&selection()).unwrap();
    let q = m.bending_response().unwrap().unwrap().matrix();
    assert_eq!(
        q,
        [
            [0.002, -0.0001, 0.0],
            [-0.0001, 0.0005, 0.0],
            [0.0, 0.0, 0.0003]
        ]
    );
    assert_eq!(m.authored().properties["bending_warp"].unit, "mN*m");
    assert_eq!(
        m.sha256(),
        db.resolve(&m.frozen_selection()).unwrap().sha256()
    );
    assert!(m.mechanics().is_err()); // bending measurements never manufacture missing membrane data
}

#[test]
fn consent_full_coverage_and_explicit_model_are_required() {
    let db = Catalog::builtin().unwrap();
    let mut s = selection();
    s.allow_estimates = false;
    assert!(db.resolve(&s).unwrap().bending_response().is_err());
    for key in [
        "bending_warp",
        "bending_weft",
        "bending_coupling",
        "bending_twist",
    ] {
        let mut s = selection();
        s.record.as_mut().unwrap().properties.remove(key);
        assert!(db.resolve(&s).unwrap().bending_response().is_err());
    }
    let mut s = selection();
    s.record.as_mut().unwrap().structure.clear();
    assert!(
        db.resolve(&s)
            .unwrap()
            .bending_response()
            .unwrap()
            .is_none()
    );
    s.record
        .as_mut()
        .unwrap()
        .structure
        .insert("bending_model".into(), "typo".into());
    assert!(db.resolve(&s).unwrap().bending_response().is_err());
}

#[test]
fn dimensional_and_physical_mistakes_refuse() {
    for q in [
        [1., 1., 1., 1.],
        [1., 1., -2., 1.],
        [0., 1., 0., 1.],
        [1., 1., 0., 0.],
        [f64::NAN, 1., 0., 1.],
    ] {
        assert!(BendingTensor::new(q[0], q[1], q[2], q[3]).is_err());
    }
    let mut s = selection();
    s.record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("bending_warp")
        .unwrap()
        .unit = "Pa".into();
    assert!(Catalog::builtin().unwrap().resolve(&s).is_err());
}
