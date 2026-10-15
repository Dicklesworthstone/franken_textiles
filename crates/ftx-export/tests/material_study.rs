//! Fixed-pattern material substitution through actual manufacturing, not mocked planner outputs.
use ftx_export::{cutting::CutPlan, material_study::Request, sewing::SewPlan};
use ftx_pattern::{Pattern, material_binding::Changes};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const SEW: &str = include_str!("../../../tests/fixtures/materials/synthetic-sewing-options.json");

fn selection(id: &str, width: f64, thickness: f64) -> Value {
    let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
    raw = raw["fabric"]["material"].clone();
    raw["id"] = json!(id);
    raw["record"]["id"] = json!(id);
    for (key, value, unit) in [
        ("width", width, "m"),
        ("thickness", thickness, "mm"),
        ("areal_density", 0.2, "kg/m2"),
    ] {
        raw["record"]["properties"][key] = json!({"value":value,"unit":unit,"evidence":"estimate","source":"test","method":"Synthetic software test observation"});
    }
    raw
}
fn variant(key: &str, selection: Value) -> Value {
    json!({"key":key,"changes":{"schema":"ftx-material-bindings/1","default":selection}})
}
fn quote(id: &str) -> Value {
    json!({"offer_id":format!("synthetic/{id}"),"material_id":id,"supplier":"Synthetic only","sku":id,
        "price":10.,"quantity":1.,"unit":"linear_meter","minimum_quantity":0.1,"order_increment":0.1,
        "currency":"USD","region":"test","quality":"synthetic","terms":"no-shipping",
        "observed_on":"2026-10-15","available":true,"source":"test:study-quote","source_sha256":"0".repeat(64)})
}
fn request() -> Request {
    let mut missing = selection("missing-width", 0.1, 0.4);
    missing["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("width");
    serde_json::from_value(json!({"schema":"ftx-material-study/1",
        "variants":[variant("wide",selection("wide",0.1,0.4)),variant("narrow",selection("narrow",0.06,0.4)),
            variant("thick",selection("thick",0.1,2.)),variant("missing-width",missing),variant("unknown",json!({"id":"no-such-material"}))],
        "cutting":{"copies":1,"gap_mm":2.,"edge_margin_mm":0.,"end_margin_mm":0.},
        "sewing":serde_json::from_str::<Value>(SEW).unwrap(),
        "purchasing":{"quotes":{"schema":"ftx-textile-quotes/1","quotes":[quote("wide"),quote("narrow")]},
            "market":{"currency":"USD","region":"test","quality":"synthetic","terms":"no-shipping","as_of":"2026-10-15","max_age_days":1}}
    })).unwrap()
}
fn evaluate(r: &Request, p: &Pattern) -> Value {
    serde_json::to_value(r.evaluate(p).unwrap()).unwrap()
}
fn row<'a>(report: &'a Value, key: &str) -> &'a Value {
    report["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == key)
        .unwrap()
}

#[test]
fn material_changes_reach_real_markers_seams_mass_and_whole_orders_without_geometry_changes() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let original = p.raw().clone();
    let request = request();
    let report = evaluate(&request, &p);
    for key in ["wide", "narrow"] {
        let r = row(&report, key);
        let bound = &r["binding"]["result"];
        assert_eq!(
            bound["source_pattern_sha256"],
            report["source_pattern_sha256"]
        );
        assert_eq!(bound["pattern"]["pieces"], original["pieces"]);
        assert_eq!(bound["pattern"]["seams"], original["seams"]);
        let pattern = Pattern::new(bound["pattern"].clone());
        assert_eq!(
            r["cutting"]["result"],
            serde_json::to_value(
                CutPlan::build(&pattern, request.cutting.as_ref().unwrap()).unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            r["sewing"]["result"],
            serde_json::to_value(
                SewPlan::build(&pattern, request.sewing.as_ref().unwrap()).unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            r["cutting"]["result"]["pattern_sha256"],
            bound["pattern_sha256"]
        );
        assert!(r["metrics"]["stock_mass_kg"].as_f64().unwrap() > 0.);
        assert!(
            r["metrics"]["least_observed_independent_order_total"]
                .as_f64()
                .unwrap()
                > 0.
        );
    }
    let wide = row(&report, "wide");
    let narrow = row(&report, "narrow");
    assert!(
        narrow["cutting"]["result"]["rolls"][0]["length_m"]
            .as_f64()
            .unwrap()
            > wide["cutting"]["result"]["rolls"][0]["length_m"]
                .as_f64()
                .unwrap()
    );
    assert_ne!(
        wide["metrics"]["least_observed_independent_order_total"],
        narrow["metrics"]["least_observed_independent_order_total"]
    );
    assert_eq!(p.raw(), &original);
    assert_eq!(report["hardware_execution_authorized"], false);
    assert_eq!(report["geometry_changed"], false);
}

#[test]
fn unavailable_consumers_are_independent_and_missing_prices_are_never_zero() {
    let report = evaluate(&request(), &Pattern::from_json(PATTERN).unwrap());
    let thick = row(&report, "thick");
    assert_eq!(thick["cutting"]["status"], "available");
    assert_eq!(thick["sewing"]["status"], "refused");
    assert!(
        thick["sewing"]["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("stack")
    );
    assert_eq!(thick["purchasing"]["status"], "available");
    assert_eq!(
        thick["purchasing"]["result"]["complete_offer_coverage"],
        false
    );
    assert!(thick["metrics"]["least_observed_independent_order_total"].is_null());
    let missing = row(&report, "missing-width");
    assert_eq!(missing["cutting"]["status"], "refused");
    assert_eq!(missing["sewing"]["status"], "available");
    assert_eq!(missing["purchasing"]["status"], "refused");
    assert!(missing["metrics"]["stock_area_m2"].is_null());
    let unknown = row(&report, "unknown");
    assert_eq!(unknown["binding"]["status"], "refused");
    assert!(unknown["frozen_changes"].is_null());
    assert!(unknown["metrics"]["feed_seconds"].is_null());
}

#[test]
fn frozen_changes_replay_identical_bindings_and_reject_modified_evidence() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let r = request();
    let report = evaluate(&r, &p);
    for key in ["wide", "narrow", "thick", "missing-width"] {
        let row = row(&report, key);
        let changes: Changes = serde_json::from_value(row["frozen_changes"].clone()).unwrap();
        assert_eq!(changes.apply(&p).unwrap(), row["binding"]["result"]);
    }
    let mut r = r;
    r.variants.reverse();
    assert_eq!(evaluate(&r, &p)["alternatives"], report["alternatives"]);
    r.variants[0].changes =
        serde_json::from_value(row(&report, "wide")["frozen_changes"].clone()).unwrap();
    r.variants[0]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thickness")
        .unwrap()
        .value = 0.5;
    assert_eq!(
        row(&evaluate(&r, &p), "unknown")["binding"]["status"],
        "refused"
    );
}

#[test]
fn inherited_materials_stay_distinct_and_optional_mass_is_complete_or_unknown() {
    let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
    let mut lining = selection("lining", 0.2, 0.1);
    lining["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("areal_density");
    raw["pieces"][1]["fabric"] = json!({"material":lining,"role":"lining"});
    raw["pieces"][1]["sim"] = json!(false);
    let report = evaluate(&request(), &Pattern::new(raw.clone()));
    let r = row(&report, "wide");
    assert_eq!(r["cutting"]["result"]["rolls"].as_array().unwrap().len(), 2);
    assert_eq!(
        r["binding"]["result"]["pattern"]["pieces"][1]["fabric"]["role"],
        "lining"
    );
    assert_eq!(r["binding"]["result"]["pattern"]["pieces"][1]["sim"], false);
    assert!(r["metrics"]["stock_mass_kg"].is_null());
    assert!(r["metrics"]["stock_area_m2"].as_f64().unwrap() > 0.);
    let frozen: Changes = serde_json::from_value(r["frozen_changes"].clone()).unwrap();
    assert_eq!(
        frozen.apply(&Pattern::new(raw)).unwrap(),
        r["binding"]["result"]
    );
}

#[test]
fn screening_bridge_does_not_prune_manufacturing_alternatives_without_consent() {
    let mut r = request();
    let a = selection("a", 0.06, 0.4);
    let mut b = selection("b", 0.1, 0.4);
    b["record"]["properties"]["areal_density"]["value"] = json!(0.3);
    r.variants.clear();
    r.screened = Some(
        serde_json::from_value(
            json!({"request":{"schema":"ftx-material-screen/1","basis":"nominal",
        "candidates":[{"key":"a","selection":a},{"key":"b","selection":b}],
        "objectives":[{"attribute":{"key":"areal_density","unit":"kg/m2"},"direction":"minimize"}]},
        "target":{"scope":"default"},"frontier_only":false}),
        )
        .unwrap(),
    );
    let p = Pattern::from_json(PATTERN).unwrap();
    let report = evaluate(&r, &p);
    assert_eq!(report["screening"]["frontier"], json!(["a"]));
    assert_eq!(report["alternatives"].as_array().unwrap().len(), 2);
    r.screened.as_mut().unwrap().frontier_only = true;
    assert_eq!(
        evaluate(&r, &p)["alternatives"].as_array().unwrap().len(),
        1
    );
    r.screened.as_mut().unwrap().target = ftx_export::material_study::Target::Pieces {
        ids: vec!["front".into()],
    };
    let targeted = evaluate(&r, &p);
    let binding = &targeted["alternatives"][0]["binding"]["result"];
    assert_eq!(binding["pieces"][0]["material_id"], "a");
    assert_eq!(
        binding["pieces"][1]["material_id"],
        "synthetic-sewing-sheet"
    );
}

#[test]
fn malformed_targets_duplicate_keys_and_aggregate_work_are_refused() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let mut r = request();
    r.variants[1].key = r.variants[0].key.clone();
    assert!(r.evaluate(&p).is_err());
    let mut r = request();
    r.variants = vec![r.variants[0].clone(); 33];
    assert!(r.evaluate(&p).is_err());
    let mut r = request();
    r.cutting.as_mut().unwrap().copies = 4096;
    assert!(r.evaluate(&p).is_err());
    r.cutting = None;
    assert!(r.evaluate(&p).is_err());
    r.purchasing = None;
    let result = evaluate(&r, &p);
    assert_eq!(row(&result, "wide")["cutting"]["status"], "not_requested");
    assert_eq!(
        row(&result, "wide")["purchasing"]["status"],
        "not_requested"
    );
}
