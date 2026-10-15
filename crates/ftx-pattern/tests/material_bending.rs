//! Source and dimensional checks, not empirical textile observations.
use ftx_pattern::{
    material_bending::{BendingTensor, MODEL, NORMAL_MODEL},
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

#[test]
fn normal_kinematics_are_explicit_source_locked_and_do_not_change_the_tensor() {
    let db = Catalog::builtin().unwrap();
    let a = db.resolve(&selection()).unwrap();
    let mut s = selection();
    s.record
        .as_mut()
        .unwrap()
        .structure
        .insert("bending_model".into(), NORMAL_MODEL.into());
    let b = db.resolve(&s).unwrap();
    assert_eq!(
        a.bending_response().unwrap().unwrap().matrix(),
        b.bending_response().unwrap().unwrap().matrix()
    );
    assert!(!a.bending_response().unwrap().unwrap().uses_normal());
    assert!(b.bending_response().unwrap().unwrap().uses_normal());
    assert_eq!(b.bending_response().unwrap().unwrap().model(), NORMAL_MODEL);
    assert_ne!(a.sha256(), b.sha256());
    assert_eq!(
        db.resolve(&b.frozen_selection()).unwrap().sha256(),
        b.sha256()
    );
    let mut frozen = b.frozen_selection();
    frozen
        .record
        .as_mut()
        .unwrap()
        .structure
        .insert("bending_model".into(), MODEL.into());
    assert!(db.resolve(&frozen).is_err());
    s.allow_estimates = false;
    assert!(db.resolve(&s).unwrap().bending_response().is_err());
}

fn curved_selection() -> Selection {
    let mut s = selection();
    let m = s.record.as_mut().unwrap();
    m.structure
        .insert("bending_model".into(), NORMAL_MODEL.into());
    // Published synthetic coefficients here only isolate consent on the estimated target curvature.
    for q in m.properties.values_mut() {
        q.evidence = ftx_pattern::materials::Evidence::Published;
    }
    for (key, value) in ftx_pattern::material_bending::PREFERRED_CURVATURE_PROPERTIES
        .into_iter()
        .zip([0.05, -0.02, 0.01])
    {
        let mut q = m.properties["bending_warp"].clone();
        q.value = value;
        q.unit = "1/cm".into();
        q.method = "Synthetic preferred curvature, not physical shaping evidence".into();
        q.evidence = ftx_pattern::materials::Evidence::Estimate;
        m.properties.insert(key.into(), q);
    }
    s
}

#[test]
fn preferred_curvature_keeps_signed_units_sources_and_identity() {
    let db = Catalog::builtin().unwrap();
    let s = curved_selection();
    let m = db.resolve(&s).unwrap();
    assert_eq!(
        m.bending_response().unwrap().unwrap().preferred_curvature(),
        [5., -2., 1.]
    );
    assert_eq!(
        m.authored().properties["preferred_curvature_warp"].unit,
        "1/cm"
    );
    assert_eq!(
        m.material().properties["preferred_curvature_warp"].unit,
        "1/m"
    );
    assert_eq!(
        db.resolve(&m.frozen_selection()).unwrap().sha256(),
        m.sha256()
    );
    let mut changed = m.frozen_selection();
    changed
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("preferred_curvature_warp")
        .unwrap()
        .value = -0.05;
    assert!(db.resolve(&changed).is_err());
    changed.expected_sha256 = None;
    let other = db.resolve(&changed).unwrap();
    assert_eq!(
        other
            .bending_response()
            .unwrap()
            .unwrap()
            .preferred_curvature()[0],
        -5.
    );
    assert_ne!(m.sha256(), other.sha256());
}

#[test]
fn preferred_curvature_requires_complete_evidence_and_oriented_kinematics() {
    use ftx_pattern::material_bending::PREFERRED_CURVATURE_PROPERTIES as KEYS;
    let db = Catalog::builtin().unwrap();
    for key in KEYS {
        let mut s = curved_selection();
        s.record.as_mut().unwrap().properties.remove(key);
        assert!(
            db.resolve(&s)
                .unwrap()
                .bending_response()
                .unwrap_err()
                .message
                .contains(key)
        );
    }
    let mut s = curved_selection();
    s.allow_estimates = false;
    let e = db.resolve(&s).unwrap().bending_response().unwrap_err();
    assert!(e.message.contains("preferred_curvature") && e.message.contains("estimate"));
    s.allow_estimates = true;
    s.record
        .as_mut()
        .unwrap()
        .structure
        .insert("bending_model".into(), MODEL.into());
    assert!(db.resolve(&s).unwrap().bending_response().is_err());
    let mut s = curved_selection();
    s.record
        .as_mut()
        .unwrap()
        .properties
        .get_mut(KEYS[0])
        .unwrap()
        .unit = "m".into();
    assert!(db.resolve(&s).is_err());
    let d = BendingTensor::new(1., 1., 0., 1.).unwrap();
    assert!(d.with_preferred_curvature([0.; 3]).is_err());
    for value in [f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(
            d.normal_projected()
                .with_preferred_curvature([value, 0., 0.])
                .is_err()
        );
    }
    // No declaration preserves the existing serialized law; explicit zero remains physically equal.
    assert_eq!(d.normal_projected().preferred_curvature(), [0.; 3]);
    assert!(
        serde_json::to_value(d.normal_projected())
            .unwrap()
            .get("preferred_curvature_m_inv")
            .is_none()
    );
}
