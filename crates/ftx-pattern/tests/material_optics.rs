//! Synthetic face identities and evidence-policy checks; no characterized textile coefficients.
use ftx_pattern::{
    material_catalog::Selection,
    material_optics::Side,
    materials::{CATALOG_SCHEMA, Catalog, Evidence, Material},
};
use serde_json::{Value, json};

fn specimen() -> Material {
    let q = |value, face| {
        json!({"value":value, "unit":"1", "source":"measurement", "evidence":"published",
            "method":"Synthetic observation for testing evidence branches, not empirical data",
            "conditions":{"face":face, "specimen":"synthetic:one-coupon", "conditioning":"dry",
                "spectral_basis":"total_hemispherical", "temperature_min_c":"-20", "temperature_max_c":"60"}})
    };
    serde_json::from_value(json!({
        "id":"synthetic-two-faces", "name":"Synthetic asymmetric sheet, not measured textile data", "form":"woven",
        "properties":{"emissivity_finish":q(0.2,"finish"), "emissivity_back":q(0.8,"back")},
        "optical_faces":{
            "positive":{"face":"finish", "emissivity_property":"emissivity_finish"},
            "negative":{"face":"back", "emissivity_property":"emissivity_back"},
            "source":"orientation", "evidence":"published", "method":"Synthetic source-normal assignment",
            "specimen":"synthetic:one-coupon", "conditioning":"dry"
        },
        "sources":{
            "orientation":{"reference":"test:synthetic-orientation", "accessed_on":"2026-10-19", "note":"Synthetic assignment; no empirical claim"},
            "measurement":{"reference":"test:synthetic-optical-observation", "accessed_on":"2026-10-19", "note":"Synthetic numeric observations; no empirical claim"}
        }
    })).unwrap()
}

fn catalog() -> Catalog {
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
}

#[test]
fn distinct_sides_select_exact_observations_from_separately_sourced_assignment() {
    let m = specimen();
    let positive = m.optical_face(Side::Positive, false).unwrap();
    let negative = m.optical_face(Side::Negative, false).unwrap();
    assert_eq!(positive.face, "finish");
    assert_eq!(negative.face, "back");
    assert_eq!(m.properties[&positive.emissivity_property].value, 0.2);
    assert_eq!(m.properties[&negative.emissivity_property].value, 0.8);
    assert_ne!(
        m.optical_faces.as_ref().unwrap().source,
        m.properties[&positive.emissivity_property].source
    );
}

#[test]
fn uncharacterized_opposite_side_never_inherits_a_positive_observation() {
    let mut m = specimen();
    m.optical_faces.as_mut().unwrap().negative = None;
    m.validate().unwrap();
    assert!(m.optical_face(Side::Positive, false).is_ok());
    assert!(m.optical_face(Side::Negative, true).is_err());
    m.optical_faces.as_mut().unwrap().positive = None;
    assert!(m.validate().is_err());
    m.optical_faces = None;
    m.validate().unwrap();
    assert!(m.optical_face(Side::Positive, true).is_err());
    assert!(
        serde_json::to_value(&m)
            .unwrap()
            .get("optical_faces")
            .is_none()
    );
}

#[test]
fn mapping_and_selected_observation_require_separate_estimate_consent() {
    let mut m = specimen();
    m.optical_faces.as_mut().unwrap().evidence = Evidence::Estimate;
    m.validate().unwrap();
    assert!(m.optical_face(Side::Positive, false).is_err());
    assert!(m.optical_face(Side::Positive, true).is_ok());
    m.optical_faces.as_mut().unwrap().evidence = Evidence::Published;
    m.properties.get_mut("emissivity_back").unwrap().evidence = Evidence::Estimate;
    assert!(m.optical_face(Side::Positive, false).is_ok());
    assert!(m.optical_face(Side::Negative, false).is_err());
    assert!(m.optical_face(Side::Negative, true).is_ok());
}

#[test]
fn observation_specimen_face_conditioning_and_both_sources_cannot_be_rebound() {
    for (pointer, replacement) in [
        ("/optical_faces/source", json!("missing")),
        ("/optical_faces/method", json!(" ")),
        ("/optical_faces/specimen", json!("different-coupon")),
        ("/optical_faces/conditioning", json!("wet")),
        ("/optical_faces/positive/face", json!("back")),
        (
            "/optical_faces/negative/emissivity_property",
            json!("missing"),
        ),
        ("/properties/emissivity_finish/source", json!("missing")),
        (
            "/properties/emissivity_finish/conditions/specimen",
            Value::Null,
        ),
        (
            "/properties/emissivity_finish/conditions/conditioning",
            json!("wet"),
        ),
        (
            "/properties/emissivity_back/conditions/face",
            json!("finish"),
        ),
        ("/sources/orientation/note", json!("")),
        ("/sources/measurement/reference", json!("")),
    ] {
        let mut v = serde_json::to_value(specimen()).unwrap();
        *v.pointer_mut(pointer).unwrap() = replacement;
        let result = serde_json::from_value::<Material>(v);
        assert!(
            result.is_err() || result.unwrap().validate().is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn partial_band_observations_remain_data_without_acquiring_solver_qualification() {
    let mut m = specimen();
    let q = m.properties.get_mut("emissivity_finish").unwrap();
    q.conditions
        .insert("spectral_basis".into(), "camera_band_8_12_um".into());
    q.conditions.remove("temperature_min_c");
    q.conditions.remove("temperature_max_c");
    // An apparent band measurement outside [0,1] is retained, never silently clamped for a solver.
    q.value = 1.006;
    m.validate().unwrap();
    let face = m.optical_face(Side::Positive, false).unwrap();
    assert_eq!(m.properties[&face.emissivity_property].value, 1.006);
    assert_eq!(
        m.properties[&face.emissivity_property].conditions["spectral_basis"],
        "camera_band_8_12_um"
    );
}

#[test]
fn mapping_and_its_source_are_frozen_into_material_identity_and_replay() {
    let m = specimen();
    let resolved = catalog()
        .resolve(&Selection {
            id: m.id.clone(),
            record: Some(m),
            ..Selection::default()
        })
        .unwrap();
    let frozen = resolved.frozen_selection();
    assert_eq!(
        catalog().resolve(&frozen).unwrap().sha256(),
        resolved.sha256()
    );
    for change_source in [false, true] {
        let mut changed = frozen.clone();
        let m = changed.record.as_mut().unwrap();
        if change_source {
            m.sources
                .get_mut("orientation")
                .unwrap()
                .note
                .push_str("; revised source assignment");
        } else {
            let faces = m.optical_faces.as_mut().unwrap();
            std::mem::swap(&mut faces.positive, &mut faces.negative);
        }
        assert!(catalog().resolve(&changed).is_err());
        changed.expected_sha256 = None;
        assert_ne!(
            catalog().resolve(&changed).unwrap().sha256(),
            resolved.sha256()
        );
    }
}

#[test]
fn builtin_data_does_not_invent_optical_faces() {
    for m in Catalog::builtin().unwrap().materials {
        assert!(m.optical_faces.is_none(), "{}", m.id);
        assert!(m.optical_face(Side::Positive, true).is_err(), "{}", m.id);
    }
}
