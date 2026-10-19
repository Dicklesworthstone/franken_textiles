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
fn explicit_pair_evidence_reaches_manufacturing_and_frozen_variant_replay_without_rewriting_sources()
 {
    let original_shell = selection("original-shell", 0.1, 0.4);
    let changed_shell = selection("changed-shell", 0.08, 0.6);
    let lining = selection("lining", 0.1, 0.2);
    let db = ftx_pattern::textile_import::catalog().unwrap();
    let hash = |selection: &Value| {
        db.resolve(&serde_json::from_value(selection.clone()).unwrap())
            .unwrap()
            .sha256()
            .to_owned()
    };
    let pair = |shell: &Value| {
        json!({"materials":[hash(&lining),hash(shell)],
        "friction":{"value":0,"unit":"1","evidence":"estimate","source":"test",
            "method":"Synthetic interface substitution; no empirical garment claim"},
        "sources":{"test":{"reference":"test:study-pair","accessed_on":"2026-10-19",
            "note":"Synthetic scalar pair evidence"}}})
    };
    let original_pair = pair(&original_shell);
    let mut changed_pair = pair(&changed_shell);
    changed_pair["friction"]["bounds"] = serde_json::from_str("[-0.0, 0]").unwrap();
    let mut raw: Value = serde_json::from_str(PATTERN).unwrap();
    raw["fabric"]["material"] = original_shell;
    raw["fabric"]["interfaces"] = json!([original_pair]);
    raw["pieces"][1]["fabric"] = json!({"material":lining,"role":"lining"});
    let source = Pattern::new(raw.clone());
    let mut request = request();
    request.purchasing = None;
    request.variants = serde_json::from_value(json!([
        {"key":"inherited","changes":{"schema":"ftx-material-bindings/1"}},
        {"key":"stale-retained","changes":{"schema":"ftx-material-bindings/1","default":changed_shell}},
        {"key":"replaced","changes":{"schema":"ftx-material-bindings/1","default":changed_shell,"interfaces":[changed_pair]}},
        {"key":"cleared","changes":{"schema":"ftx-material-bindings/1","default":changed_shell,"interfaces":[]}},
        {"key":"wrong-pair","changes":{"schema":"ftx-material-bindings/1","default":changed_shell,"interfaces":[original_pair]}}
    ])).unwrap();
    let report = evaluate(&request, &source);
    for key in ["inherited", "stale-retained", "replaced", "cleared"] {
        let row = row(&report, key);
        assert_eq!(row["binding"]["status"], "available");
        assert_eq!(row["cutting"]["status"], "available");
        assert_eq!(row["sewing"]["status"], "available");
        let frozen: Changes = serde_json::from_value(row["frozen_changes"].clone()).unwrap();
        assert_eq!(frozen.apply(&source).unwrap(), row["binding"]["result"]);
        let bound = &row["binding"]["result"]["pattern"];
        assert_eq!(bound["seams"], raw["seams"]);
        assert_eq!(bound["pieces"][1]["fabric"]["role"], "lining");
        if matches!(key, "inherited" | "stale-retained") {
            assert!(row["frozen_changes"].get("interfaces").is_none());
            assert_eq!(bound["fabric"]["interfaces"], raw["fabric"]["interfaces"]);
        } else {
            assert_eq!(
                row["frozen_changes"]["interfaces"],
                bound["fabric"]["interfaces"]
            );
        }
    }
    let replacement = row(&report, "replaced");
    assert_eq!(
        replacement["frozen_changes"]["interfaces"],
        json!([changed_pair])
    );
    assert!(replacement["frozen_changes"]["interfaces"][0]["friction"]["value"].is_u64());
    assert!(
        replacement["frozen_changes"]["interfaces"][0]
            .get("allow_estimates")
            .is_none()
    );
    assert_eq!(
        row(&report, "cleared")["frozen_changes"]["interfaces"],
        json!([])
    );
    let refused = row(&report, "wrong-pair");
    for consumer in ["binding", "cutting", "sewing"] {
        assert_eq!(refused[consumer]["status"], "refused");
    }
    assert!(refused["frozen_changes"].is_null());
    assert_eq!(source.raw(), &raw);
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

#[test]
fn additional_production_consumer_uses_frozen_bindings_and_preserves_independent_failures() {
    let source = Pattern::from_json(PATTERN).unwrap();
    let request = request();
    let mut called = Vec::new();
    let scenario = json!({"body_sha256":"synthetic-fixed-body", "environment":{"outside_c":10.0}});
    let result = request
        .evaluate_with(&source, &scenario, |key, bound| {
            called.push(key.to_string());
            if key == "wide" {
                return Err(ftx_core::Refusal::new(
                    ftx_core::RefusalCode::NotConverged,
                    "synthetic independent refusal",
                ));
            }
            Ok(bound.raw().clone())
        })
        .unwrap();
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(called, vec!["missing-width", "narrow", "thick", "wide"]);
    assert_eq!(result["manufacturing"], evaluate(&request, &source));
    for row in result["simulations"].as_array().unwrap() {
        let key = row["key"].as_str().unwrap();
        if key == "unknown" || key == "wide" {
            assert_eq!(row["evaluation"]["status"], "refused");
        } else {
            assert_eq!(
                row["evaluation"]["result"],
                self::row(&result["manufacturing"], key)["binding"]["result"]["pattern"]
            );
        }
    }
    let changed = request
        .evaluate_with(
            &source,
            &json!({"environment":{"outside_c":11.0}}),
            |_, _| Ok(()),
        )
        .unwrap();
    assert_ne!(result["request_sha256"], changed.request_sha256);
}

#[test]
fn explicit_additional_consumer_allows_mechanics_only_without_weakening_ordinary_studies() {
    let source = Pattern::from_json(PATTERN).unwrap();
    let mut request = request();
    request.cutting = None;
    request.sewing = None;
    request.purchasing = None;
    assert!(request.evaluate(&source).is_err());
    let result = request
        .evaluate_with(&source, &json!({"test":"additional consumer"}), |_, p| {
            Ok(p.raw().clone())
        })
        .unwrap();
    assert_eq!(result.simulations.len(), 5);
    let raw = serde_json::to_value(result).unwrap();
    assert!(
        raw["manufacturing"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["cutting"]["status"] == "not_requested"
                && r["sewing"]["status"] == "not_requested")
    );
}

fn comparison(r: &mut Request, objectives: Value, constraints: Value, consumers: Value) {
    r.comparison = Some(
        serde_json::from_value(json!({"basis":"nominal", "objectives":objectives,
        "constraints":constraints, "required_consumers":consumers}))
        .unwrap(),
    );
}

fn mass_request() -> ftx_export::assembly_mass::Request {
    serde_json::from_value(json!({"schema":"ftx-assembly-mass/1", "inventory_complete":true,
        "extras":[{"id":"all-thread-and-closures", "allow_estimates":true,
        "mass":{"value":0.02,"unit":"kg","evidence":"estimate","source":"test","method":"Synthetic retained extras"},
        "sources":{"test":{"reference":"test:study-extras","accessed_on":"2026-10-16","note":"Synthetic only"}}}]})).unwrap()
}

#[test]
fn full_assembly_mass_matches_direct_consumer_without_changing_other_plans() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let mut r = request();
    let without = evaluate(&r, &p);
    assert!(row(&without, "wide").get("assembly_mass").is_none());
    r.assembly_mass = Some(mass_request());
    let report = evaluate(&r, &p);
    for key in ["wide", "narrow", "thick", "missing-width"] {
        let a = row(&report, key);
        for consumer in ["binding", "cutting", "sewing", "purchasing"] {
            assert_eq!(a[consumer], row(&without, key)[consumer]);
        }
        let bound = Pattern::new(a["binding"]["result"]["pattern"].clone());
        assert_eq!(
            a["assembly_mass"]["result"],
            serde_json::to_value(r.assembly_mass.as_ref().unwrap().evaluate(&bound).unwrap())
                .unwrap()
        );
        let total = a["metrics"]["assembly_mass_kg"].as_f64().unwrap();
        assert!((total - (0.0096 * 0.2 + 0.02)).abs() < 1e-12);
    }
    assert_eq!(
        row(&report, "unknown")["assembly_mass"]["status"],
        "refused"
    );
    assert_eq!(
        row(&report, "missing-width")["cutting"]["status"],
        "refused"
    );
    assert_eq!(row(&report, "thick")["sewing"]["status"], "refused");
    assert_eq!(report, evaluate(&r, &p));
    r.cutting.as_mut().unwrap().copies = 3;
    let many = evaluate(&r, &p);
    assert_eq!(
        row(&many, "wide")["assembly_mass"],
        row(&report, "wide")["assembly_mass"]
    );
    assert!(
        row(&many, "wide")["metrics"]["stock_mass_kg"]
            .as_f64()
            .unwrap()
            > row(&report, "wide")["metrics"]["stock_mass_kg"]
                .as_f64()
                .unwrap()
    );
}

#[test]
fn mass_only_frontier_counts_unsimulated_lining_and_does_not_require_width_or_body() {
    let mut p: Value = serde_json::from_str(PATTERN).unwrap();
    p["pieces"][1]["sim"] = json!(false);
    p["pieces"][1]["fabric"] = json!({"material":selection("fixed-lining", 0.1, 0.4)});
    let p = Pattern::new(p);
    let mut r = request();
    r.cutting = None;
    r.sewing = None;
    r.purchasing = None;
    r.assembly_mass = Some(mass_request());
    r.variants.clear();
    for (key, density) in [
        ("light", Some(0.1)),
        ("heavy", Some(0.3)),
        ("unknown", None),
    ] {
        let mut material = selection(key, 0.1, 0.4);
        material["record"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("width");
        if let Some(density) = density {
            material["record"]["properties"]["areal_density"]["value"] = json!(density);
        } else {
            material["record"]["properties"]
                .as_object_mut()
                .unwrap()
                .remove("areal_density");
        }
        r.variants
            .push(serde_json::from_value(variant(key, material)).unwrap());
    }
    comparison(
        &mut r,
        json!([{"metric":"assembly_mass_kg", "direction":"minimize"}]),
        json!([]),
        json!([]),
    );
    let report = evaluate(&r, &p);
    assert_eq!(report["comparison"]["frontier"], json!(["light"]));
    assert_eq!(
        row(&report["comparison"], "heavy")["dominated_by"],
        json!(["light"])
    );
    assert_eq!(row(&report["comparison"], "unknown")["eligible"], false);
    assert_eq!(row(&report, "light")["cutting"]["status"], "not_requested");
    let mass = &row(&report, "light")["assembly_mass"]["result"];
    assert_eq!(mass["pieces"].as_array().unwrap().len(), 2);
    assert!(
        (mass["assembly_mass"]["kg"].as_f64().unwrap() - (0.0048 * (0.1 + 0.2) + 0.02)).abs()
            < 1e-12
    );
    let failed_sim = r
        .evaluate_with(&p, &json!({"body":"unavailable"}), |_, _| {
            Err::<Value, _>(ftx_core::Refusal::new(
                ftx_core::RefusalCode::NotConverged,
                "synthetic body failure",
            ))
        })
        .unwrap();
    let failed_sim = serde_json::to_value(failed_sim).unwrap();
    assert_eq!(
        failed_sim["comparison"]["frontier"],
        report["comparison"]["frontier"]
    );
    assert!(
        failed_sim["simulations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["evaluation"]["status"] == "refused")
    );
}

#[test]
fn incomplete_mass_is_not_a_winning_zero_or_a_successful_required_consumer() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let mut r = request();
    r.assembly_mass = Some(mass_request());
    comparison(
        &mut r,
        json!([{"metric":"assembly_mass_kg", "direction":"minimize"}]),
        json!([]),
        json!([]),
    );
    assert!(
        !evaluate(&r, &p)["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    r.assembly_mass.as_mut().unwrap().inventory_complete = false;
    let incomplete = evaluate(&r, &p);
    assert_eq!(incomplete["comparison"]["frontier"], json!([]));
    assert!(row(&incomplete, "wide")["assembly_mass"]["result"]["sheet_mass"]["kg"].is_number());
    assert!(row(&incomplete, "wide")["metrics"]["assembly_mass_kg"].is_null());
    comparison(
        &mut r,
        json!([{"metric":"stock_area_m2", "direction":"minimize"}]),
        json!([]),
        json!(["assembly_mass"]),
    );
    assert_eq!(evaluate(&r, &p)["comparison"]["frontier"], json!([]));
    r.assembly_mass.as_mut().unwrap().inventory_complete = true;
    r.assembly_mass.as_mut().unwrap().extras[0].mass = None;
    let unknown = evaluate(&r, &p);
    assert_eq!(unknown["comparison"]["frontier"], json!([]));
    assert_ne!(unknown["request_sha256"], incomplete["request_sha256"]);
    r.comparison.as_mut().unwrap().required_consumers.clear();
    assert!(
        !evaluate(&r, &p)["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn mass_comparison_respects_density_protocols_constraints_and_same_cut_geometry() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let mut r = request();
    r.variants
        .retain(|v| ["wide", "narrow"].contains(&v.key.as_str()));
    r.assembly_mass = Some(mass_request());
    comparison(
        &mut r,
        json!([{"metric":"assembly_mass_kg", "direction":"minimize"}]),
        json!([]),
        json!([]),
    );
    // Width qualifiers are unrelated to retained mass and must not block this comparison.
    let material = r.variants[0]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap();
    material.properties.get_mut("width").unwrap().method =
        "Different irrelevant width method".into();
    assert_eq!(evaluate(&r, &p)["comparison"]["incomparable"], json!([]));
    let material = r.variants[0]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap();
    material
        .properties
        .get_mut("areal_density")
        .unwrap()
        .conditions
        .insert("conditioning".into(), "different".into());
    assert_eq!(
        evaluate(&r, &p)["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    r.assembly_mass.as_mut().unwrap().seam_allowance_mm = Some(10.);
    assert!(
        !evaluate(&r, &p)["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    r.assembly_mass.as_mut().unwrap().seam_allowance_mm = Some(11.);
    let mismatch = evaluate(&r, &p);
    assert_eq!(mismatch["comparison"]["frontier"], json!([]));
    assert_eq!(row(&mismatch, "wide")["cutting"]["status"], "available");
    assert_eq!(row(&mismatch, "wide")["assembly_mass"]["status"], "refused");
    r.cutting.as_mut().unwrap().seam_allowance_mm = Some(11.);
    let matched = evaluate(&r, &p);
    let mass = row(&matched, "wide")["metrics"]["assembly_mass_kg"]
        .as_f64()
        .unwrap();
    r.comparison.as_mut().unwrap().constraints = vec![
        serde_json::from_value(json!({"metric":"assembly_mass_kg","minimum":mass,"maximum":mass}))
            .unwrap(),
    ];
    assert!(
        !evaluate(&r, &p)["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    r.comparison.as_mut().unwrap().constraints[0].minimum = None;
    r.comparison.as_mut().unwrap().constraints[0].maximum = Some(mass * 0.99);
    assert_eq!(evaluate(&r, &p)["comparison"]["frontier"], json!([]));
    r.assembly_mass = None;
    assert!(r.evaluate(&p).is_err());
}

#[test]
fn aggregate_extra_inventory_is_admitted_before_any_study_evaluation() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let mut r = request();
    r.assembly_mass = Some(mass_request());
    let extra = r.assembly_mass.as_ref().unwrap().extras[0].clone();
    r.assembly_mass.as_mut().unwrap().extras = (0..1024)
        .map(|i| {
            let mut e = extra.clone();
            e.id = format!("group-{i}");
            e
        })
        .collect();
    assert_eq!(
        r.evaluate(&p).unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
}

#[test]
fn computed_pareto_set_keeps_real_cost_mass_tradeoffs_ties_and_frozen_outputs() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let mut raw = serde_json::to_value(request()).unwrap();
    let mut variants = Vec::new();
    let mut quotes = Vec::new();
    for (key, density, price) in [
        ("a-light", 0.1, 30.),
        ("b-cheap", 0.3, 10.),
        ("c-dominated", 0.4, 40.),
        ("d-tie", 0.1, 30.),
    ] {
        let mut material = selection(key, 0.1, 0.4);
        material["record"]["properties"]["areal_density"]["value"] = json!(density);
        variants.push(variant(key, material));
        let mut q = quote(key);
        q["price"] = json!(price);
        quotes.push(q);
    }
    raw["variants"] = json!(variants);
    raw["purchasing"]["quotes"]["quotes"] = json!(quotes);
    let mut r: Request = serde_json::from_value(raw).unwrap();
    let baseline = evaluate(&r, &p);
    assert!(baseline.get("comparison").is_none());
    comparison(
        &mut r,
        json!([
        {"metric":"stock_mass_kg","direction":"minimize"},
        {"metric":"observed_order_cost","direction":"minimize"}]),
        json!([]),
        json!(["sewing"]),
    );
    let report = evaluate(&r, &p);
    assert_eq!(report["alternatives"], baseline["alternatives"]);
    assert_eq!(
        report["comparison"]["frontier"],
        json!(["a-light", "b-cheap", "d-tie"])
    );
    assert_eq!(
        row(&report["comparison"], "c-dominated")["dominated_by"],
        json!(["a-light", "b-cheap", "d-tie"])
    );
    assert_eq!(report["comparison"]["uncertainty_propagated"], false);
    for candidate in report["comparison"]["alternatives"].as_array().unwrap() {
        let original = row(&report, candidate["key"].as_str().unwrap());
        assert_eq!(
            candidate["readings"]["stock_mass_kg"]["value"],
            original["metrics"]["stock_mass_kg"]
        );
        assert_eq!(
            candidate["readings"]["observed_order_cost"]["value"],
            original["metrics"]["least_observed_independent_order_total"]
        );
        assert_eq!(candidate["readings"]["observed_order_cost"]["unit"], "USD");
    }
    assert_eq!(report, evaluate(&r, &p));
    r.comparison.as_mut().unwrap().objectives[1].direction =
        ftx_pattern::material_screen::Direction::Maximize;
    let different = evaluate(&r, &p);
    assert_ne!(
        report["comparison"]["context_sha256"],
        different["comparison"]["context_sha256"]
    );
    assert_ne!(
        report["comparison"]["frontier"],
        different["comparison"]["frontier"]
    );
    assert_eq!(report["alternatives"], different["alternatives"]);
}

#[test]
fn comparison_excludes_missing_required_values_not_unrelated_consumer_refusals() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let mut r = request();
    comparison(
        &mut r,
        json!([{ "metric":"stock_mass_kg", "direction":"minimize" }]),
        json!([]),
        json!([]),
    );
    let report = evaluate(&r, &p);
    assert_eq!(row(&report["comparison"], "thick")["eligible"], true);
    assert_eq!(row(&report, "thick")["sewing"]["status"], "refused");
    assert_eq!(
        row(&report["comparison"], "missing-width")["eligible"],
        false
    );
    assert!(
        row(&report["comparison"], "missing-width")["readings"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    r.comparison
        .as_mut()
        .unwrap()
        .required_consumers
        .push(ftx_export::material_study::comparison::Consumer::Sewing);
    let required = evaluate(&r, &p);
    assert_eq!(row(&required["comparison"], "thick")["eligible"], false);
    assert_eq!(required["alternatives"], report["alternatives"]);
    comparison(
        &mut r,
        json!([{ "metric":"observed_order_cost", "direction":"minimize" }]),
        json!([]),
        json!([]),
    );
    let priced = evaluate(&r, &p);
    assert_eq!(row(&priced["comparison"], "thick")["eligible"], false);
    assert!(
        row(&priced["comparison"], "thick")["readings"]
            .get("observed_order_cost")
            .is_none()
    );
}

#[test]
fn comparison_protocol_mismatches_remain_explicit_and_unrelated_properties_do_not_block() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let mut r = request();
    r.variants
        .retain(|v| ["wide", "narrow"].contains(&v.key.as_str()));
    let narrow = r.variants.iter_mut().find(|v| v.key == "narrow").unwrap();
    narrow
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("width")
        .unwrap()
        .method = "different width protocol".into();
    comparison(
        &mut r,
        json!([{ "metric":"observed_order_cost", "direction":"minimize" }]),
        json!([]),
        json!([]),
    );
    let report = evaluate(&r, &p);
    assert_eq!(report["comparison"]["frontier"], json!(["narrow", "wide"]));
    assert_eq!(
        report["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    r.comparison.as_mut().unwrap().allow_mixed_protocols = true;
    let admitted = evaluate(&r, &p);
    assert_eq!(
        admitted["comparison"]["frontier"].as_array().unwrap().len(),
        1
    );
    assert!(
        admitted["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // With sewing-only objectives, width protocols (and width-dependent failed cuts) are irrelevant.
    comparison(
        &mut r,
        json!([{ "metric":"feed_seconds", "direction":"minimize" }]),
        json!([]),
        json!([]),
    );
    let sewing = evaluate(&r, &p);
    assert!(
        sewing["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn comparison_constraints_use_inclusive_computed_values_and_refuse_invalid_policy() {
    let p = Pattern::from_json(PATTERN).unwrap();
    let mut r = request();
    let value = row(&evaluate(&r, &p), "wide")["metrics"]["stock_mass_kg"]
        .as_f64()
        .unwrap();
    comparison(
        &mut r,
        json!([{ "metric":"stock_mass_kg", "direction":"minimize" }]),
        json!([{ "metric":"stock_mass_kg", "minimum":value, "maximum":value }]),
        json!([]),
    );
    assert_eq!(
        row(&evaluate(&r, &p)["comparison"], "wide")["eligible"],
        true
    );
    r.comparison.as_mut().unwrap().constraints[0].minimum = None;
    r.comparison.as_mut().unwrap().constraints[0].maximum = Some(value * 0.5);
    assert_eq!(
        row(&evaluate(&r, &p)["comparison"], "wide")["eligible"],
        false
    );
    r.comparison.as_mut().unwrap().constraints[0].maximum = Some(f64::NAN);
    assert!(r.evaluate(&p).is_err());
    r.comparison.as_mut().unwrap().constraints.clear();
    let objective = r.comparison.as_ref().unwrap().objectives[0].clone();
    r.comparison.as_mut().unwrap().objectives.push(objective);
    assert!(r.evaluate(&p).is_err());
    r.comparison.as_mut().unwrap().objectives.clear();
    assert!(r.evaluate(&p).is_err());
    comparison(
        &mut r,
        json!([{ "metric":"stock_mass_kg", "direction":"minimize" }]),
        json!([]),
        json!([]),
    );
    r.cutting = None;
    assert!(r.evaluate(&p).is_err());
    assert!(
        serde_json::from_value::<ftx_export::material_study::comparison::Policy>(
            json!({"basis":"reported_bounds","objectives":[]})
        )
        .is_err()
    );
}

fn finite_inventory_item(id: &str, material: Value, width: f64, height: f64) -> Value {
    json!({"id":id,"material":material,"outline_mm":[[0,0],[width,0],[width,height],[0,height]],
        "grain_mm":[[0,0],[0,10]],"geometry_source":{"reference":"test:study-inventory",
        "accessed_on":"2026-10-16","note":"Synthetic inventory geometry, not measured stock"}})
}

fn finite_inventory_study() -> Request {
    let mut hide = selection("hide", 0.1, 0.4);
    hide["record"]["form"] = json!("hide");
    hide["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("width");
    hide["record"]["properties"]["areal_density"]["value"] = json!(0.5);
    let remnant = selection("remnant", 0.1, 0.4);
    let mut unused = selection("unused-unknown-mass", 0.1, 0.4);
    unused["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("areal_density");
    let mut raw = serde_json::to_value(request()).unwrap();
    raw["variants"] = json!([
        variant("hide", hide.clone()),
        variant("remnant", remnant.clone()),
        variant("unstocked", selection("unstocked", 0.1, 0.4)),
        variant("unknown", json!({"id":"no-such-material"}))
    ]);
    raw["finite_stock"] = json!({"schema":"ftx-finite-stock-request/1","stocks":[
        finite_inventory_item("hide-001",hide,80.,260.),
        finite_inventory_item("remnant-001",remnant,100.,150.),
        finite_inventory_item("unused-001",unused,100.,150.)],
        "options":{"copies":1,"gap_mm":2.,"boundary_margin_mm":2.,"step_mm":2.,"work_limit":2000000}});
    raw["purchasing"]["quotes"]["quotes"] = json!([quote("remnant"), quote("unstocked")]);
    serde_json::from_value(raw).unwrap()
}

#[test]
fn finite_stock_studies_recompute_real_hide_and_remnant_allocations_with_independent_consumers() {
    let source = Pattern::from_json(PATTERN).unwrap();
    let original = source.raw().clone();
    let mut request = finite_inventory_study();
    let mut identical = request.variants[1].clone();
    identical.key = "same-remnant-again".into();
    request.variants.push(identical);
    let inventory = serde_json::to_value(&request.finite_stock).unwrap();
    let report = evaluate(&request, &source);
    for key in ["hide", "remnant", "same-remnant-again"] {
        let row = row(&report, key);
        let binding = &row["binding"]["result"];
        let bound = Pattern::new(binding["pattern"].clone());
        let actual = request
            .finite_stock
            .as_ref()
            .unwrap()
            .build(&bound)
            .unwrap();
        let direct = serde_json::to_value(&actual).unwrap();
        assert_eq!(row["finite_stock"]["status"], "available");
        assert_eq!(row["finite_stock"]["result"], direct);
        assert_eq!(direct["pattern_sha256"], binding["pattern_sha256"]);
        assert_eq!(binding["pattern"]["pieces"], original["pieces"]);
        assert_eq!(binding["pattern"]["seams"], original["seams"]);
        assert_eq!(
            row["metrics"]["used_stock_count"],
            direct["used_stock_count"]
        );
        assert_eq!(
            row["metrics"]["used_stock_gross_area_m2"],
            direct["used_gross_area_m2"]
        );
        assert_eq!(
            row["metrics"]["finite_stock_external_cut_area_m2"],
            direct["external_cut_area_m2"]
        );
        assert_eq!(
            serde_json::to_value(actual.request().build(&bound).unwrap()).unwrap(),
            direct
        );
        // The unrelated unused stock has unknown density; it cannot erase known used-stock mass.
        assert!(row["metrics"]["used_stock_mass_kg"].as_f64().unwrap() > 0.);
        assert_eq!(row["sewing"]["status"], "available");
    }
    assert_eq!(row(&report, "hide")["cutting"]["status"], "refused");
    assert_eq!(row(&report, "hide")["purchasing"]["status"], "refused");
    assert_eq!(row(&report, "remnant")["cutting"]["status"], "available");
    assert_eq!(
        row(&report, "unstocked")["finite_stock"]["status"],
        "refused"
    );
    assert_eq!(row(&report, "unstocked")["cutting"]["status"], "available");
    assert_eq!(
        row(&report, "unstocked")["purchasing"]["result"]["complete_offer_coverage"],
        true
    );
    assert_eq!(row(&report, "unknown")["finite_stock"]["status"], "refused");
    assert_eq!(
        row(&report, "remnant")["finite_stock"],
        row(&report, "same-remnant-again")["finite_stock"]
    );
    assert_eq!(source.raw(), &original);
    assert_eq!(
        serde_json::to_value(&request.finite_stock).unwrap(),
        inventory
    );
    assert_eq!(report, evaluate(&request, &source));
}

#[test]
fn finite_stock_comparison_uses_exact_used_items_and_rejects_missing_or_stale_materials() {
    let source = Pattern::from_json(PATTERN).unwrap();
    let mut request = finite_inventory_study();
    comparison(
        &mut request,
        json!([{"metric":"used_stock_gross_area_m2","direction":"minimize"},
            {"metric":"used_stock_mass_kg","direction":"minimize"}]),
        json!([{"metric":"used_stock_count","minimum":1,"maximum":1},
            {"metric":"finite_stock_external_cut_area_m2","minimum":0.0096}]),
        json!(["finite_stock", "sewing"]),
    );
    let result = evaluate(&request, &source);
    assert_eq!(result["comparison"]["frontier"], json!(["remnant"]));
    assert_eq!(row(&result["comparison"], "hide")["eligible"], true);
    assert_eq!(
        row(&result["comparison"], "hide")["dominated_by"],
        json!(["remnant"])
    );
    assert_eq!(row(&result["comparison"], "unstocked")["eligible"], false);
    assert!(
        row(&result["comparison"], "unstocked")["readings"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let readings = &row(&result["comparison"], "remnant")["readings"];
    for (metric, unit) in [
        ("used_stock_count", "count"),
        ("used_stock_gross_area_m2", "m2"),
        ("used_stock_mass_kg", "kg"),
        ("finite_stock_external_cut_area_m2", "m2"),
    ] {
        assert_eq!(
            readings[metric]["value"].as_f64(),
            row(&result, "remnant")["metrics"][metric].as_f64()
        );
        assert_eq!(readings[metric]["unit"], unit);
        assert_eq!(readings[metric]["consumer"], "finite_stock");
    }
    // A same-name source edit is a different material. Shared inventory is never rebound for it.
    request.variants[1]
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
    let changed = evaluate(&request, &source);
    assert_eq!(
        row(&changed, "remnant")["finite_stock"]["status"],
        "refused"
    );
    assert_eq!(row(&changed, "remnant")["sewing"]["status"], "available");
    assert_eq!(changed["comparison"]["frontier"], json!(["hide"]));
    assert_ne!(
        result["comparison"]["context_sha256"],
        changed["comparison"]["context_sha256"]
    );
}

#[test]
fn heterogeneous_finite_stock_mass_is_unknown_when_any_used_item_lacks_density() {
    let mut source: Value = serde_json::from_str(PATTERN).unwrap();
    let mut missing = selection("lining-without-density", 0.1, 0.1);
    missing["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("areal_density");
    source["pieces"][1]["fabric"] = json!({"material":missing,"role":"lining"});
    let mut request = finite_inventory_study();
    request.finite_stock.as_mut().unwrap().stocks.push(
        serde_json::from_value(finite_inventory_item("lining-001", missing, 100., 150.)).unwrap(),
    );
    comparison(
        &mut request,
        json!([{"metric":"used_stock_mass_kg","direction":"minimize"}]),
        json!([]),
        json!([]),
    );
    let result = evaluate(&request, &Pattern::new(source));
    for key in ["hide", "remnant"] {
        let row = row(&result, key);
        assert_eq!(row["finite_stock"]["status"], "available");
        assert_eq!(row["metrics"]["used_stock_count"], 2);
        assert!(row["metrics"]["used_stock_mass_kg"].is_null());
        assert!(row["metrics"]["used_stock_gross_area_m2"].as_f64().unwrap() > 0.);
        assert_eq!(self::row(&result["comparison"], key)["eligible"], false);
    }
    assert!(
        result["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn finite_stock_and_assembly_mass_share_cut_geometry_and_preserve_both_consumers() {
    let source = Pattern::from_json(PATTERN).unwrap();
    let mut request = finite_inventory_study();
    request.assembly_mass = Some(mass_request());
    comparison(
        &mut request,
        json!([{"metric":"used_stock_mass_kg","direction":"minimize"},
            {"metric":"assembly_mass_kg","direction":"minimize"}]),
        json!([]),
        json!(["finite_stock", "assembly_mass"]),
    );
    let result = evaluate(&request, &source);
    for key in ["hide", "remnant"] {
        let candidate = row(&result, key);
        assert_eq!(candidate["finite_stock"]["status"], "available");
        assert_eq!(candidate["assembly_mass"]["status"], "available");
        assert_eq!(row(&result["comparison"], key)["eligible"], true);
        let bound = Pattern::new(candidate["binding"]["result"]["pattern"].clone());
        assert_eq!(
            candidate["assembly_mass"]["result"],
            serde_json::to_value(
                request
                    .assembly_mass
                    .as_ref()
                    .unwrap()
                    .evaluate(&bound)
                    .unwrap()
            )
            .unwrap()
        );
    }
    // A finite-stock-only cut must use the same outline as the assembly whose mass is compared.
    request.cutting = None;
    request.purchasing = None;
    request
        .finite_stock
        .as_mut()
        .unwrap()
        .options
        .seam_allowance_mm = Some(11.);
    let mismatch = evaluate(&request, &source);
    assert_eq!(
        row(&mismatch, "remnant")["finite_stock"]["status"],
        "available"
    );
    assert_eq!(
        row(&mismatch, "remnant")["assembly_mass"]["status"],
        "refused"
    );
    assert_eq!(mismatch["comparison"]["frontier"], json!([]));
    request.assembly_mass.as_mut().unwrap().seam_allowance_mm = Some(11.);
    assert_eq!(
        row(&evaluate(&request, &source), "remnant")["assembly_mass"]["status"],
        "available"
    );

    // The combined production surface now has eight independently requestable consumers.
    request.cutting = self::request().cutting;
    request.purchasing = self::request().purchasing;
    request.comparison.as_mut().unwrap().required_consumers = serde_json::from_value(json!([
        "cutting",
        "finite_stock",
        "sewing",
        "purchasing",
        "assembly_mass",
        "drape",
        "surface",
        "coverage"
    ]))
    .unwrap();
    request
        .comparison
        .as_ref()
        .unwrap()
        .validate(&request, true)
        .unwrap();
}

#[test]
fn finite_stock_only_studies_preserve_scenario_composition_and_bound_aggregate_work() {
    let source = Pattern::from_json(PATTERN).unwrap();
    let mut request = finite_inventory_study();
    request.cutting = None;
    request.sewing = None;
    request.purchasing = None;
    comparison(
        &mut request,
        json!([{"metric":"used_stock_count","direction":"minimize"}]),
        json!([]),
        json!([]),
    );
    let standalone = evaluate(&request, &source);
    let scenario = request
        .evaluate_with(&source, &json!({"fixed":"test scenario"}), |_, _| {
            Err::<Value, _>(ftx_core::Refusal::new(
                ftx_core::RefusalCode::NotConverged,
                "synthetic independent simulation refusal",
            ))
        })
        .unwrap();
    let scenario = serde_json::to_value(scenario).unwrap();
    assert_eq!(
        scenario["manufacturing"]["alternatives"],
        standalone["alternatives"]
    );
    assert_eq!(
        scenario["comparison"]["frontier"],
        standalone["comparison"]["frontier"]
    );
    assert_eq!(
        scenario["simulations"][0]["evaluation"]["status"],
        "refused"
    );
    request.finite_stock.as_mut().unwrap().options.work_limit = 20_000_000;
    assert_eq!(
        request.evaluate(&source).unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    request.finite_stock.as_mut().unwrap().options.work_limit = 2_000_000;
    request.finite_stock.as_mut().unwrap().options.copies = 4096;
    assert_eq!(
        request.evaluate(&source).unwrap_err().code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    request.finite_stock = None;
    assert!(request.evaluate(&source).is_err());
    // Old requests keep their serialization and report shape when this consumer is omitted.
    let legacy = self::request();
    assert!(
        serde_json::to_value(&legacy)
            .unwrap()
            .get("finite_stock")
            .is_none()
    );
    let legacy_report = evaluate(&legacy, &source);
    assert!(row(&legacy_report, "wide").get("finite_stock").is_none());
    assert!(
        row(&legacy_report, "wide")["metrics"]
            .get("used_stock_count")
            .is_none()
    );
}
