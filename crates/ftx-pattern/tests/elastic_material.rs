//! Source and dimensional checks, not empirical elastic-band characterization.
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_elastic::ElasticBand;
use ftx_pattern::materials::Catalog;
use serde_json::{Value, json};

fn input() -> Value {
    json!({"id":"test-band","allow_estimates":true,"record":{
        "id":"test-band","name":"Synthetic elastic cord","form":"yarn",
        "structure":{"elastic_model":"linear_tension_only/1"},
        "properties":{"axial_stiffness":{"value":0.015,"unit":"kN","source":"test",
            "evidence":"estimate","method":"Synthetic EA; not measured cord data",
            "conditions":{"specimen":"complete cord cross section"},"bounds":[0.012,0.018]}},
        "sources":{"test":{"reference":"test:elastic-band","accessed_on":"2026-10-21",
            "note":"Entire fixture synthetic, not a catalog recommendation"}}}})
}
fn db() -> Catalog {
    Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap()
}
fn resolve(raw: Value) -> Result<ElasticBand, ftx_core::Refusal> {
    ElasticBand::resolve(&db(), &serde_json::from_value(raw).unwrap())
}

#[test]
fn actual_band_stiffness_normalizes_without_requiring_or_inventing_sheet_physics() {
    let band = resolve(input()).unwrap();
    assert_eq!(band.axial_stiffness_n(), 15.0);
    assert_eq!(
        band.snapshot().authored().properties["axial_stiffness"].unit,
        "kN"
    );
    assert_eq!(
        band.snapshot().material().properties["axial_stiffness"].bounds,
        Some([12.0, 18.0])
    );
    assert!(band.snapshot().mechanics().is_err());
    assert!(
        !band
            .snapshot()
            .material()
            .properties
            .contains_key("areal_density")
    );
    assert_eq!(
        ElasticBand::resolve(&db(), &band.frozen_selection()).unwrap(),
        band
    );
}

#[test]
fn no_conversion_from_strength_sheet_stiffness_or_fiber_names() {
    for change in [
        (|v: &mut Value| v["record"]["properties"]["axial_stiffness"]["unit"] = json!("N/m"))
            as fn(&mut Value),
        |v| v["record"]["properties"]["axial_stiffness"]["unit"] = json!("Pa"),
        |v| v["record"]["properties"].as_object_mut().unwrap().clear(),
        |v| v["record"]["structure"] = json!({}),
        |v| v["record"]["structure"]["elastic_model"] = json!("rubber"),
        |v| v["record"]["form"] = json!("fiber"),
        |v| v["record"]["form"] = json!("fill"),
        |v| v["allow_estimates"] = json!(false),
        |v| v["record"]["properties"]["axial_stiffness"]["source"] = json!("absent"),
        |v| v["record"]["properties"]["axial_stiffness"]["value"] = json!(0),
        |v| v["record"]["properties"]["axial_stiffness"]["bounds"] = json!([-1, 1]),
    ] {
        let mut raw = input();
        change(&mut raw);
        assert!(resolve(raw).is_err());
    }
}

#[test]
fn exact_source_identity_and_estimate_policy_survive_portable_selection() {
    let band = resolve(input()).unwrap();
    let mut frozen = band.frozen_selection();
    frozen
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("axial_stiffness")
        .unwrap()
        .value = 0.016;
    assert!(ElasticBand::resolve(&db(), &frozen).is_err());
    let mut frozen = band.frozen_selection();
    frozen.allow_estimates = false;
    assert!(ElasticBand::resolve(&db(), &frozen).is_err());
    let raw: Selection = serde_json::from_value(input()).unwrap();
    let before = serde_json::to_string(&raw).unwrap();
    ElasticBand::resolve(&db(), &raw).unwrap();
    assert_eq!(before, serde_json::to_string(&raw).unwrap());
}
