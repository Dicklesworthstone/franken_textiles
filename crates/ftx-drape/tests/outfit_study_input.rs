//! Source composition/replay, not empirical layered-costume evidence.
use ftx_core::receipt::sha256_hex;
use ftx_drape::{run::DrapeOptions, study_input::PreparedInput};
use serde_json::{Value, json};

fn source() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-joint-outfit.json"
    ))
    .unwrap()
}
fn options() -> DrapeOptions {
    DrapeOptions {
        self_contact: Some(DrapeOptions::default().contact),
        ..DrapeOptions::default()
    }
}

#[test]
fn joint_input_freezes_physics_and_keeps_original_garment_correspondence() {
    let raw = source();
    let prepared = PreparedInput::new(raw.clone(), &options()).unwrap();
    let mut descriptor = json!({"scenario":"unchanged"});
    prepared.annotate(&mut descriptor).unwrap();
    assert_eq!(descriptor["scenario"], "unchanged");
    let receipt = &descriptor["outfit"];
    assert_eq!(receipt["source_request"], raw);
    assert_eq!(receipt["fit_claim"], false);
    let frozen = receipt["pattern_json"].as_str().unwrap();
    assert_eq!(
        receipt["composed_pattern_sha256"],
        sha256_hex(frozen.as_bytes())
    );
    let replay: Value = serde_json::from_str(frozen).unwrap();
    assert_eq!(&replay, prepared.pattern().raw());
    for (i, name) in ["inner::band", "outer::band"].into_iter().enumerate() {
        assert_eq!(replay["pieces"][i]["id"], name);
        assert_eq!(
            replay["pieces"][i]["vertices"],
            raw["garments"][i]["pattern"]["pieces"][0]["vertices"]
        );
        assert!(replay["pieces"][i]["fabric"]["material"]["expected_sha256"].is_string());
    }
    let previous = descriptor.clone();
    assert!(prepared.annotate(&mut descriptor).is_err());
    assert_eq!(descriptor, previous);
}

#[test]
fn ordinary_inputs_preserve_source_and_descriptor_without_a_new_policy() {
    let raw = source()["garments"][0]["pattern"].clone();
    let prepared = PreparedInput::new(raw.clone(), &DrapeOptions::default()).unwrap();
    assert_eq!(prepared.pattern().raw(), &raw);
    let mut descriptor = json!({"unchanged":true});
    let before = descriptor.clone();
    prepared.annotate(&mut descriptor).unwrap();
    assert_eq!(descriptor, before);
    // The ordinary study may recover incomplete mechanics through explicit alternative bindings.
    let incomplete = json!({"schema":"fabric-pattern/1"});
    assert!(PreparedInput::new(incomplete, &DrapeOptions::default()).is_ok());
}

#[test]
fn joint_studies_cannot_silently_disable_contact_or_relax_material_consent() {
    assert!(PreparedInput::new(source(), &DrapeOptions::default()).is_err());
    let mut raw = source();
    raw["garments"][0]["pattern"]["fabric"]["material"]["allow_estimates"] = json!(false);
    assert!(PreparedInput::new(raw, &options()).is_err());
    let mut raw = source();
    raw["ignored_layer_offsets"] = json!([1, 2]);
    assert!(PreparedInput::new(raw, &options()).is_err());
}
