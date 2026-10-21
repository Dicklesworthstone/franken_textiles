//! Synthetic assignment/replay checks, not characterized elastic or real garment evidence.
use ftx_pattern::{
    Pattern, material_binding::Changes, material_elastic::ElasticBand, textile_import,
};
use serde_json::{Value, json};

fn band(ea: f64) -> Value {
    json!({"id":"synthetic-band","allow_estimates":true,"record":{
        "id":"synthetic-band","name":"Synthetic band, not textile data","form":"yarn",
        "structure":{"elastic_model":"linear_tension_only/1"},
        "properties":{"axial_stiffness":{"value":ea,"unit":"N","source":"test","evidence":"estimate",
            "method":"Synthetic force law for software tests"}},
        "sources":{"test":{"reference":"test:band-binding","accessed_on":"2026-10-21","note":"Invented software fixture"}}
    }})
}
fn pattern() -> Value {
    let mut raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    for p in raw["pieces"].as_array_mut().unwrap() {
        p["placement"] = json!({"part":"torso","radius_mm":40});
    }
    raw["meta"]["solved"] = json!({"elastic_casings":[
        {"instance":"front","relaxed_m":0.08,"load_instances":["front"],"elastic_material":band(6.0)},
        {"instance":"back","relaxed_m":0.09,"load_instances":["back"],"elastic_material":band(9.0)}
    ]});
    raw
}
fn apply(raw: &Value, bands: Value) -> Result<Value, ftx_core::Refusal> {
    let changes: Changes =
        serde_json::from_value(json!({"schema":"ftx-material-bindings/1","elastic_casings":bands}))
            .unwrap();
    changes.apply(&Pattern::new(raw.clone()))
}

#[test]
fn independent_bands_freeze_without_changing_cloth_geometry_lengths_or_loads() {
    let raw = pattern();
    let before = raw.clone();
    let unchanged = apply(&raw, json!({})).unwrap();
    let result = apply(&raw, json!({"front":band(24.0)})).unwrap();
    assert_eq!(raw, before);
    for field in ["pieces", "seams", "fabric"] {
        assert_eq!(result["pattern"][field], unchanged["pattern"][field]);
    }
    let db = textile_import::catalog().unwrap();
    for (i, (id, ea)) in [("front", 24.0), ("back", 9.0)].into_iter().enumerate() {
        let selection =
            serde_json::from_value(result["frozen_elastic_casings"][id].clone()).unwrap();
        let material = ElasticBand::resolve(&db, &selection).unwrap();
        assert_eq!(material.axial_stiffness_n(), ea);
        assert_eq!(
            Some(material.snapshot().sha256()),
            selection.expected_sha256.as_deref()
        );
        let saved = &result["pattern"]["meta"]["solved"]["elastic_casings"][i];
        assert_eq!(
            saved["elastic_material"],
            result["frozen_elastic_casings"][id]
        );
        assert_eq!(
            saved["relaxed_m"],
            raw["meta"]["solved"]["elastic_casings"][i]["relaxed_m"]
        );
        assert_eq!(
            saved["load_instances"],
            raw["meta"]["solved"]["elastic_casings"][i]["load_instances"]
        );
    }
    let replay = apply(&raw, result["frozen_elastic_casings"].clone()).unwrap();
    assert_eq!(result, replay);
    assert_eq!(
        serde_json::from_str::<Value>(result["pattern_json"].as_str().unwrap()).unwrap(),
        result["pattern"]
    );
}

#[test]
fn legacy_binding_keeps_original_cut_overlap_and_casing_geometry() {
    let mut raw = pattern();
    raw["meta"]["solved"] = json!({"elastic_cut_mm":90.0,"user_annotation":"retained"});
    raw["meta"]["style"] = json!({"elastic_overlap":10.0});
    raw["pieces"][0]["role"] = json!("waistband");
    let result = apply(&raw, json!({"front":band(12.0)})).unwrap();
    assert_eq!(result["pattern"]["pieces"], raw["pieces"]);
    assert_eq!(result["pattern"]["meta"]["style"], raw["meta"]["style"]);
    assert_eq!(result["pattern"]["meta"]["solved"]["elastic_cut_mm"], 90.0);
    assert_eq!(
        result["pattern"]["meta"]["solved"]["user_annotation"],
        "retained"
    );
    assert!(
        result["pattern"]["meta"]["solved"]
            .get("elastic_casings")
            .is_none()
    );
    assert_eq!(
        apply(&raw, result["frozen_elastic_casings"].clone()).unwrap(),
        result
    );
    assert!(apply(&raw, json!({"back":band(12.0)})).is_err());
    raw["pieces"][1]["role"] = json!("waistband");
    assert!(apply(&raw, json!({"front":band(12.0)})).is_err());
}

#[test]
fn omission_or_empty_map_preserves_previous_schema_bytes_and_fallbacks() {
    let raw = pattern();
    let original: Changes =
        serde_json::from_value(json!({"schema":"ftx-material-bindings/1"})).unwrap();
    assert!(
        serde_json::to_value(&original)
            .unwrap()
            .get("elastic_casings")
            .is_none()
    );
    let baseline = original.apply(&Pattern::new(raw.clone())).unwrap();
    assert_eq!(baseline, apply(&raw, json!({})).unwrap());
    assert!(baseline.get("frozen_elastic_casings").is_none());
    let mut raw = raw;
    raw["meta"]["solved"]["elastic_casings"][1]
        .as_object_mut()
        .unwrap()
        .remove("elastic_material");
    let result = apply(&raw, json!({"front":band(12.0)})).unwrap();
    assert!(result["frozen_elastic_casings"].get("back").is_none());
    assert_eq!(
        result["pattern"]["meta"]["solved"]["elastic_casings"][1],
        raw["meta"]["solved"]["elastic_casings"][1]
    );
}

#[test]
fn targets_are_physical_casings_not_source_piece_names_or_cutting_labels() {
    let mut raw = pattern();
    raw["pieces"][0]["cut"] = json!({"qty":2,"mirror":true});
    let p = Pattern::new(raw.clone());
    let instances: Vec<_> = p
        .instances
        .iter()
        .filter(|i| i.piece == "front")
        .map(|i| i.id.clone())
        .collect();
    assert_eq!(instances.len(), 2);
    raw["seams"][0]["a"] = json!(format!("{}/right", instances[0]));
    raw["meta"]["solved"]["elastic_casings"] = json!([
        {"instance":instances[0],"relaxed_m":0.08,"load_instances":[instances[0]],"elastic_material":band(6.0)},
        {"instance":instances[1],"relaxed_m":0.08,"load_instances":[instances[1]],"elastic_material":band(9.0)}]);
    assert!(apply(&raw, json!({"front":band(20.0)})).is_err());
    let result = apply(&raw, json!({instances[0].clone():band(20.0)})).unwrap();
    assert_eq!(
        result["frozen_elastic_casings"][&instances[1]]["record"]["properties"]["axial_stiffness"]
            ["value"],
        9.0
    );
    assert_eq!(result["pattern"]["pieces"], raw["pieces"]);
}

#[test]
fn invalid_identity_evidence_and_target_changes_refuse_atomically() {
    let raw = pattern();
    let before = raw.clone();
    let mut denied = band(12.0);
    denied["allow_estimates"] = json!(false);
    assert!(apply(&raw, json!({"front":denied})).is_err());
    let mut wrong = band(12.0);
    wrong["record"]["properties"]["axial_stiffness"]["unit"] = json!("N/m");
    assert!(apply(&raw, json!({"front":wrong})).is_err());
    let mut unknown = band(12.0);
    unknown["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("axial_stiffness");
    assert!(apply(&raw, json!({"front":unknown})).is_err());
    assert!(apply(&raw, json!({"not-a-casing":band(12.0)})).is_err());
    let admitted = apply(&raw, json!({"front":band(12.0)})).unwrap();
    let mut stale = admitted["frozen_elastic_casings"].clone();
    stale["back"]["record"]["properties"]["axial_stiffness"]["value"] = json!(100.0);
    assert!(apply(&raw, stale).is_err());
    assert_eq!(raw, before);
    for variant in 0..5 {
        let mut bad = raw.clone();
        match variant {
            0 => bad["pieces"][0]["sim"] = json!(false),
            1 => bad["pieces"][0]["placement"] = json!({}),
            2 => bad["meta"]["solved"]["elastic_casings"][1]["instance"] = json!("front"),
            3 => bad["meta"]["solved"]["elastic_cut_mm"] = json!(90.0),
            _ => bad["meta"]["solved"]["elastic_casings"][1]["elastic_material"] = Value::Null,
        }
        assert!(
            apply(&bad, json!({"front":band(12.0)})).is_err(),
            "case {variant}"
        );
    }
}

#[test]
fn band_substitution_does_not_rebind_existing_sheet_contact_evidence() {
    let mut raw = pattern();
    // Existing bindings preserve inherited interfaces, leaving completeness to contact consumers.
    raw["fabric"]["interfaces"] = json!([{"uninterpreted":"inherited sentinel"}]);
    let result = apply(&raw, json!({"front":band(12.0)})).unwrap();
    assert_eq!(
        result["pattern"]["fabric"]["interfaces"],
        raw["fabric"]["interfaces"]
    );
    let bands = serde_json::from_value(result["frozen_elastic_casings"].clone()).unwrap();
    let changes = Changes {
        schema: "ftx-material-bindings/1".into(),
        interfaces: Some(vec![]),
        elastic_casings: bands,
        ..Changes::default()
    };
    assert_eq!(
        changes.apply(&Pattern::new(raw)).unwrap()["pattern"]["fabric"]["interfaces"],
        json!([])
    );
}
