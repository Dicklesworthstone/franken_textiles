//! Shared JSON boundary and actual compressed-material consumers. Synthetic sources only.
use ftx_pattern::Pattern;
use ftx_wasm::materials;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");

#[test]
fn compression_stack_api_replays_every_frozen_layer_through_shared_binding() {
    let mut preparation: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    preparation["pressure_pa"] = json!(0);
    let mut input = json!({"schema":"ftx-compression-stack/1", "gap_m":0.030,
        "reference_area_m2":0.25, "direction":"loading", "allow_estimates":true,
        "layers":[{"id":"outer","preparation":preparation},{"id":"inner","preparation":preparation}]});
    let raw = input.to_string();
    let report: Value = serde_json::from_str(&materials::compression_stack(&raw).unwrap()).unwrap();
    let core =
        ftx_pattern::material_compression::stack::close(&serde_json::from_str(&raw).unwrap())
            .unwrap();
    assert_eq!(report, serde_json::to_value(&core).unwrap());
    for layer in &core.layers {
        let selection = &layer.conditioned.selection_json;
        let inspected: Value =
            serde_json::from_str(&materials::inspect(selection).unwrap()).unwrap();
        assert_eq!(inspected["selection_json"], *selection);
        let edits = format!(r#"{{"schema":"ftx-material-bindings/1","default":{selection}}}"#);
        let bound: Value =
            serde_json::from_str(&materials::bind(PATTERN, &edits).unwrap()).unwrap();
        assert_eq!(bound["geometry_changed"], false);
        let resolved = ftx_drape::material::resolve(
            &Pattern::from_json(bound["pattern_json"].as_str().unwrap()).unwrap(),
            &ftx_drape::run::DrapeOptions::default(),
        )
        .unwrap();
        assert!((resolved.options.contact.thickness - 0.0075).abs() < 1e-15);
        assert!((resolved.fabric.areal_kg_m2 - 0.1).abs() < 1e-15);
    }
    input["gap_m"] = json!(0.050);
    assert!(materials::compression_stack(&input.to_string()).is_err());
    input["gap_m"] = json!(0.030);
    input["layers"][1]["preparation"]["allow_estimates"] = json!(false);
    assert!(materials::compression_stack(&input.to_string()).is_err());
}
