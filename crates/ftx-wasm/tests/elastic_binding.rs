//! Shared browser-facing binding/study adapters, executed natively; no browser parity claim.
use ftx_pattern::{Pattern, material_binding::Changes};
use serde_json::{Value, json};

#[test]
fn shared_adapters_preserve_independent_bands_and_exact_frozen_study_replay() {
    let mut raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let band: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-elastic-band.json"
    ))
    .unwrap();
    for piece in raw["pieces"].as_array_mut().unwrap() {
        piece["placement"] = json!({"part":"torso","radius_mm":40});
    }
    raw["fabric"]["material"]["record"]["properties"]["width"] = json!({"value":1.0,"unit":"m",
        "source":"test","evidence":"estimate","method":"Synthetic stock width"});
    raw["meta"]["solved"] = json!({"elastic_casings":[
        {"instance":"front","relaxed_m":0.08,"load_instances":["front"],"elastic_material":band},
        {"instance":"back","relaxed_m":0.09,"load_instances":["back"],"elastic_material":band}]});
    let mut firm = band.clone();
    firm["record"]["properties"]["axial_stiffness"]["value"] = json!(30.0);
    let changes = json!({"schema":"ftx-material-bindings/1","elastic_casings":{"front":firm}});
    let pattern = serde_json::to_string(&raw).unwrap();
    let bound: Value =
        serde_json::from_str(&ftx_wasm::materials::bind(&pattern, &changes.to_string()).unwrap())
            .unwrap();
    let native = serde_json::from_value::<Changes>(changes.clone())
        .unwrap()
        .apply(&Pattern::new(raw.clone()))
        .unwrap();
    assert_eq!(bound["pattern_json"], native["pattern_json"]);
    let parsed = Pattern::new(bound["pattern"].clone());
    let bands = ftx_drape::scene::elastic_casings(&parsed, 1.0).unwrap();
    assert_eq!(bands[0].ea, 30.0);
    assert_eq!(bands[1].ea, 15.0);
    let request = json!({"schema":"ftx-material-study/1","variants":[{"key":"firm","changes":changes}],
        "cutting":{"copies":1,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0}});
    let report: Value =
        serde_json::from_str(&ftx_wasm::materials::study(&pattern, &request.to_string()).unwrap())
            .unwrap();
    let row = &report["alternatives"][0];
    assert_eq!(row["cutting"]["status"], "available");
    let replay: Value = serde_json::from_str(
        &ftx_wasm::materials::bind(&pattern, &row["frozen_changes"].to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(bound, replay);
    assert_eq!(bound["pattern"]["pieces"], raw["pieces"]);
    let mut denied = changes;
    denied["elastic_casings"]["front"]["allow_estimates"] = json!(false);
    assert!(ftx_wasm::materials::bind(&pattern, &denied.to_string()).is_err());
}
