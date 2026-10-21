//! Source-frozen band variants through the actual fixed-pattern study and planner.
use ftx_export::material_study::Request;
use ftx_pattern::{Pattern, material_binding::Changes};
use serde_json::{Value, json};

fn band(ea: f64) -> Value {
    json!({"id":"synthetic-study-band","allow_estimates":true,"record":{
        "id":"synthetic-study-band","name":"Synthetic, not measured elastic","form":"yarn",
        "structure":{"elastic_model":"linear_tension_only/1"},
        "properties":{"axial_stiffness":{"value":ea,"unit":"N","evidence":"estimate","source":"test","method":"Synthetic study fixture"}},
        "sources":{"test":{"reference":"test:elastic-study","accessed_on":"2026-10-21","note":"Not a textile measurement"}}
    }})
}
fn source() -> Pattern {
    let mut raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    for p in raw["pieces"].as_array_mut().unwrap() {
        p["placement"] = json!({"part":"torso","radius_mm":40});
    }
    raw["fabric"]["material"]["record"]["properties"]["width"] = json!({"value":1.0,"unit":"m","evidence":"estimate","source":"test","method":"Synthetic width"});
    raw["meta"]["solved"] = json!({"elastic_casings":[
        {"instance":"front","relaxed_m":0.08,"load_instances":["front"],"elastic_material":band(6.0)},
        {"instance":"back","relaxed_m":0.08,"load_instances":["back"],"elastic_material":band(9.0)}]});
    Pattern::new(raw)
}
fn request() -> Request {
    serde_json::from_value(json!({"schema":"ftx-material-study/1","variants":[
        {"key":"soft","changes":{"schema":"ftx-material-bindings/1","elastic_casings":{"front":band(6.0)}}},
        {"key":"firm","changes":{"schema":"ftx-material-bindings/1","elastic_casings":{"front":band(24.0)}}}],
        "cutting":{"copies":1,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0}
    })).unwrap()
}

#[test]
fn band_studies_replay_exact_frozen_changes_and_keep_manufacturing_unchanged() {
    let pattern = source();
    let original = pattern.raw().clone();
    let report = request().evaluate(&pattern).unwrap();
    let report = serde_json::to_value(report).unwrap();
    let rows = report["alternatives"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row["binding"]["status"], "available");
        assert_eq!(row["cutting"]["status"], "available");
        let frozen: Changes = serde_json::from_value(row["frozen_changes"].clone()).unwrap();
        assert_eq!(frozen.elastic_casings.len(), 2);
        let rebound = frozen.apply(&pattern).unwrap();
        assert_eq!(rebound, row["binding"]["result"]);
        assert_eq!(rebound["pattern"]["pieces"], original["pieces"]);
        assert_eq!(rebound["pattern"]["seams"], original["seams"]);
        assert_eq!(
            frozen.elastic_casings["back"]
                .record
                .as_ref()
                .unwrap()
                .properties["axial_stiffness"]
                .value,
            9.0
        );
        let mut stale = frozen;
        stale
            .elastic_casings
            .get_mut("front")
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("axial_stiffness")
            .unwrap()
            .value += 1.0;
        assert!(stale.apply(&pattern).is_err());
    }
    assert_eq!(
        rows[0]["cutting"]["result"]["rolls"],
        rows[1]["cutting"]["result"]["rolls"]
    );
    assert_ne!(
        rows[0]["binding"]["result"]["pattern_sha256"],
        rows[1]["binding"]["result"]["pattern_sha256"]
    );
    assert_eq!(pattern.raw(), &original);
}

#[test]
fn band_failures_are_variant_local_and_do_not_leak_into_the_next_candidate() {
    let pattern = source();
    let mut request = request();
    request.variants[0]
        .changes
        .elastic_casings
        .get_mut("front")
        .unwrap()
        .allow_estimates = false;
    let a = serde_json::to_value(request.evaluate(&pattern).unwrap()).unwrap();
    let row = |key: &str| {
        a["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["key"] == key)
            .unwrap()
    };
    assert_eq!(row("soft")["binding"]["status"], "refused");
    assert!(row("soft")["frozen_changes"].is_null());
    assert_eq!(row("firm")["cutting"]["status"], "available");
    assert_eq!(
        row("firm")["frozen_changes"]["elastic_casings"]["front"]["record"]["properties"]["axial_stiffness"]
            ["value"],
        24.0
    );
    request.variants.reverse();
    let b = serde_json::to_value(request.evaluate(&pattern).unwrap()).unwrap();
    assert_eq!(a["alternatives"], b["alternatives"]);
}
