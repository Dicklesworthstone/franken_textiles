//! Bounded authored geometry/material choices rerun canonical validation and real manufacturing.
//! All materials and callback readings are synthetic software fixtures, not empirical fit claims.
use ftx_core::{RefusalCode, receipt::sha256_hex};
use ftx_export::{
    Ctx, ExportOptions,
    cutting::CutPlan,
    export_cutter,
    material_study::{Request, matrix::Case},
    sewing::SewPlan,
};
use ftx_pattern::{Pattern, alter, material_binding::Changes};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const REQUEST: &str = include_str!("../../../examples/design-study.json");

fn request() -> Value {
    serde_json::from_str(REQUEST).unwrap()
}

fn evaluate(input: Value, pattern: &Pattern) -> Value {
    serde_json::to_value(
        serde_json::from_value::<Request>(input)
            .unwrap()
            .evaluate(pattern)
            .unwrap(),
    )
    .unwrap()
}

fn row<'a>(report: &'a Value, design: &str, material: &str) -> &'a Value {
    report["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["design"]["key"] == design && row["design"]["material_key"] == material)
        .unwrap()
}

fn key(design: &str, material: &str) -> Value {
    json!(serde_json::to_string(&(design, material)).unwrap())
}

#[test]
fn ease_and_length_cross_every_material_and_replay_real_markers_sewing_and_exports() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let original = pattern.raw().clone();
    let input = request();
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let report = evaluate(input.clone(), &pattern);
    assert_eq!(report["schema"], "ftx-design-study-report/1");
    assert_eq!(report["alternatives"].as_array().unwrap().len(), 8);
    assert_eq!(report["geometry_changed"], true);
    assert_eq!(report["hardware_execution_authorized"], false);
    assert_eq!(report["physical_validation_claim"], false);
    assert_eq!(report, evaluate(input.clone(), &pattern));
    for design in ["baseline", "more-ease", "longer"] {
        for material in ["light-sheet", "heavy-sheet"] {
            let row = row(&report, design, material);
            let receipt = &row["design"];
            let binding = &row["binding"]["result"];
            assert_eq!(row["key"], key(design, material));
            assert_eq!(receipt["validation"]["passed"], true);
            assert_eq!(receipt["validation"]["flat"]["status"], "CONVERGED");
            assert_eq!(
                receipt["source_pattern_sha256"],
                report["source_pattern_sha256"]
            );
            assert_eq!(receipt["geometry_changed"], design != "baseline");
            assert_eq!(
                binding["source_pattern_sha256"],
                receipt["altered_pattern_sha256"]
            );
            let altered =
                Pattern::from_json(receipt["altered_pattern_json"].as_str().unwrap()).unwrap();
            let frozen: Changes = serde_json::from_value(row["frozen_changes"].clone()).unwrap();
            assert_eq!(
                frozen.apply(&altered).unwrap()["pattern"],
                binding["pattern"]
            );
            let bound = Pattern::from_json(binding["pattern_json"].as_str().unwrap()).unwrap();
            assert_eq!(
                row["cutting"]["result"],
                serde_json::to_value(
                    CutPlan::build(&bound, typed.cutting.as_ref().unwrap()).unwrap()
                )
                .unwrap()
            );
            assert_eq!(
                row["sewing"]["result"],
                serde_json::to_value(
                    SewPlan::build(&bound, typed.sewing.as_ref().unwrap()).unwrap()
                )
                .unwrap()
            );
            let cutter =
                export_cutter(&Ctx::new(&bound, ExportOptions::default()).unwrap()).unwrap();
            assert_eq!(cutter.0.len(), 2);
            assert!(cutter.0.iter().all(|file| file.text.contains("<svg")));
        }
    }
    let baseline = row(&report, "baseline", "light-sheet");
    let wider = row(&report, "more-ease", "light-sheet");
    let longer = row(&report, "longer", "light-sheet");
    assert_eq!(
        wider["binding"]["result"]["pattern"]["pieces"][0]["vertices"]["b"],
        json!([28.0, 0.0])
    );
    assert_eq!(
        longer["binding"]["result"]["pattern"]["pieces"][0]["vertices"]["c"],
        json!([20.0, 120.0])
    );
    assert!(
        longer["metrics"]["feed_seconds"].as_f64().unwrap()
            > baseline["metrics"]["feed_seconds"].as_f64().unwrap()
    );
    assert!(
        wider["metrics"]["external_cut_area_m2"].as_f64().unwrap()
            > baseline["metrics"]["external_cut_area_m2"]
                .as_f64()
                .unwrap()
    );
    assert!(
        row(&report, "longer", "heavy-sheet")["metrics"]["stock_mass_kg"]
            .as_f64()
            .unwrap()
            > longer["metrics"]["stock_mass_kg"].as_f64().unwrap()
    );
    let mut fixed = input;
    fixed["schema"] = json!("ftx-material-study/1");
    fixed.as_object_mut().unwrap().remove("designs");
    let fixed = evaluate(fixed, &pattern);
    for old in fixed["alternatives"].as_array().unwrap() {
        let mut expanded = row(&report, "baseline", old["key"].as_str().unwrap()).clone();
        expanded.as_object_mut().unwrap().remove("design");
        expanded["key"] = old["key"].clone();
        assert_eq!(&expanded, old);
    }
    assert_eq!(pattern.raw(), &original);
}

#[test]
fn invalid_seam_is_retained_and_cannot_win_even_when_only_mass_is_requested() {
    let mut input = request();
    input.as_object_mut().unwrap().remove("sewing");
    input.as_object_mut().unwrap().remove("cutting");
    input["assembly_mass"] =
        json!({"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[]});
    input["comparison"] = json!({"basis":"nominal","objectives":[{"metric":"assembly_mass_kg","direction":"minimize"}]});
    let report = evaluate(input, &Pattern::from_json(PATTERN).unwrap());
    for material in ["light-sheet", "heavy-sheet"] {
        let invalid = row(&report, "unbalanced-seam", material);
        assert_eq!(invalid["binding"]["status"], "refused");
        assert_eq!(invalid["assembly_mass"]["status"], "refused");
        assert_eq!(
            invalid["design"]["validation"]["flat"]["summary"]["seams_failing"],
            1
        );
        assert_eq!(invalid["design"]["validation"]["passed"], false);
        assert!(invalid["design"]["altered_pattern_json"].as_str().is_some());
        assert!(invalid["frozen_changes"].is_null());
        assert!(invalid["metrics"]["assembly_mass_kg"].is_null());
        assert!(
            !report["comparison"]["frontier"]
                .as_array()
                .unwrap()
                .contains(&invalid["key"])
        );
    }
    assert_eq!(
        report["comparison"]["frontier"],
        json!([key("baseline", "light-sheet")])
    );
}

#[test]
fn self_intersection_is_blocked_even_when_canonical_signed_area_remains_positive() {
    let mut source: Value = serde_json::from_str(PATTERN).unwrap();
    source["seams"] = json!([]);
    let pattern = Pattern::new(source);
    let mut input = request();
    input["designs"] = json!([{"key":"crossed","operations":[
        {"op":"move","piece":"front","vertex":"b","dx_mm":-30.0,"dy_mm":60.0}]}]);
    input.as_object_mut().unwrap().remove("sewing");
    input["comparison"] =
        json!({"basis":"nominal","objectives":[{"metric":"stock_mass_kg","direction":"minimize"}]});
    let report = evaluate(input, &pattern);
    let crossed = row(&report, "crossed", "light-sheet");
    assert_eq!(
        crossed["design"]["validation"]["flat"]["status"],
        "CONVERGED"
    );
    assert_eq!(
        crossed["design"]["validation"]["outlines"]["status"],
        "refused"
    );
    assert_eq!(crossed["binding"]["refusal"]["code"], "degenerate_geometry");
    assert_eq!(crossed["cutting"]["status"], "refused");
    assert!(
        !report["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .contains(&crossed["key"])
    );
}

#[test]
fn explicit_vertex_and_seam_repairs_use_canonical_operations_in_authored_order() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let mut input = request();
    input["designs"] = json!([
        {"key":"repair","operations":[
            {"op":"move","piece":"front","vertex":"c","dx_mm":0.0,"dy_mm":10.0},
            {"op":"move","piece":"front","vertex":"d","dx_mm":0.0,"dy_mm":10.0},
            {"op":"true_seam","seam":"side","end":"start"},
            {"op":"move","piece":"back","vertex":"c","dx_mm":0.0,"dy_mm":10.0}]},
        {"key":"cannot-repair","operations":[
            {"op":"slash_spread","piece":"front","axis":"y","at_mm":50.0,"add_mm":200.0},
            {"op":"true_seam","seam":"side","end":"start"}]}
    ]);
    let report = evaluate(input, &pattern);
    let a = alter::move_vertex(pattern.raw(), "front", "c", 0.0, 10.0).unwrap();
    let b = alter::move_vertex(&a, "front", "d", 0.0, 10.0).unwrap();
    let c = alter::true_seam(&b, "side", "start").unwrap();
    let expected = alter::move_vertex(&c, "back", "c", 0.0, 10.0).unwrap();
    let repaired = row(&report, "repair", "light-sheet");
    assert_eq!(
        serde_json::from_str::<Value>(repaired["design"]["altered_pattern_json"].as_str().unwrap())
            .unwrap(),
        expected
    );
    assert_eq!(repaired["binding"]["status"], "available");
    assert_eq!(repaired["sewing"]["status"], "available");
    let refused = row(&report, "cannot-repair", "light-sheet");
    assert_eq!(refused["binding"]["refusal"]["code"], "no_root");
    assert!(refused["design"]["altered_pattern_json"].is_null());
    assert!(refused["design"]["validation"].is_null());
    assert_eq!(
        row(&report, "baseline", "light-sheet")["design"]["geometry_changed"],
        false
    );
}

#[test]
fn cap_ease_uses_declared_seam_ratio_without_rewriting_it_or_the_other_piece() {
    let mut source: Value = serde_json::from_str(PATTERN).unwrap();
    source["seams"][0]["ratio"] = json!(1.1);
    let pattern = Pattern::new(source);
    let mut input = request();
    input["designs"] = json!([{"key":"declared-ease","operations":[
        {"op":"cap_ease","piece":"front","seams":["side"],"target_ratio":1.1}]}]);
    let report = evaluate(input, &pattern);
    let row = row(&report, "declared-ease", "light-sheet");
    let expected = alter::cap_ease(pattern.raw(), "front", &["side".into()], 1.1).unwrap();
    let altered: Value =
        serde_json::from_str(row["design"]["altered_pattern_json"].as_str().unwrap()).unwrap();
    assert_eq!(altered, expected);
    assert_eq!(altered["seams"], pattern.raw()["seams"]);
    assert_eq!(altered["pieces"][1], pattern.raw()["pieces"][1]);
    assert_eq!(row["design"]["validation"]["passed"], true);
    assert_eq!(row["binding"]["status"], "available");
}

#[test]
fn canonical_mark_behavior_and_unknown_fields_survive_the_design_pipeline() {
    let mut source: Value = serde_json::from_str(PATTERN).unwrap();
    source["pieces"][0]["drills"] = json!([{"p":[16.0, 20.0],"d":1.0}]);
    source["pieces"][0]["placement"] = json!({"part":"torso","anchors":[[16.0,20.0]]});
    source["pieces"][0]["trace_marks"] = json!([{"pts":[[16.0,20.0],[18.0,30.0]]}]);
    source["pieces"][0]["unknown_extension"] = json!({"keep":"source annotation"});
    let pattern = Pattern::new(source);
    let report = evaluate(request(), &pattern);
    let mut expected = alter::slash_spread(pattern.raw(), "front", "x", 10.0, 8.0).unwrap();
    expected = alter::slash_spread(&expected, "back", "x", 10.0, 8.0).unwrap();
    let altered: Value = serde_json::from_str(
        row(&report, "more-ease", "light-sheet")["design"]["altered_pattern_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(altered, expected);
    for field in ["drills", "placement", "unknown_extension"] {
        assert_eq!(
            altered["pieces"][0][field],
            pattern.raw()["pieces"][0][field]
        );
    }
    assert_ne!(
        altered["pieces"][0]["trace_marks"],
        pattern.raw()["pieces"][0]["trace_marks"]
    );
}

#[test]
fn history_and_numeric_representation_changes_do_not_claim_changed_geometry() {
    let mut source: Value = serde_json::from_str(PATTERN).unwrap();
    source["pieces"][0]["grain"] = json!({"p0":[0,0],"p1":[0,10]});
    let pattern = Pattern::new(source);
    let mut input = request();
    input["designs"] = json!([{"key":"no-op","operations":[
        {"op":"slash_spread","piece":"front","axis":"x","at_mm":100.0,"add_mm":8.0}]}]);
    let report = evaluate(input, &pattern);
    let row = row(&report, "no-op", "light-sheet");
    assert_eq!(row["binding"]["status"], "available");
    assert_eq!(row["design"]["geometry_changed"], false);
    assert_eq!(report["geometry_changed"], false);
    assert_ne!(
        row["design"]["altered_pattern_sha256"],
        row["design"]["source_pattern_sha256"]
    );
    let altered: Value =
        serde_json::from_str(row["design"]["altered_pattern_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        altered["pieces"][0]["grain"],
        json!([[0.0, 0.0], [0.0, 10.0]])
    );
    assert_eq!(altered["meta"]["history"].as_array().unwrap().len(), 1);
}

#[test]
fn scenario_matrix_reserves_every_expanded_row_before_callbacks_and_reuses_actual_patterns() {
    let mut input = request();
    input.as_object_mut().unwrap().remove("comparison");
    let typed: Request = serde_json::from_value(input).unwrap();
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let cases = vec![
        Case {
            key: "cold".into(),
            scenario: json!({"temperature_c":0.0}),
            comparison: None,
        },
        Case {
            key: "warm".into(),
            scenario: json!({"temperature_c":30.0}),
            comparison: None,
        },
    ];
    let mut calls = 0;
    let refused = typed
        .evaluate_matrix_with(&pattern, &cases, 15, |_, _, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(refused.code, RefusalCode::BudgetExceeded);
    assert_eq!(calls, 0);
    let report = typed.evaluate_matrix_with(&pattern, &cases, 16, |index, key, pattern| {
        calls += 1;
        Ok(json!({"case":index,"key":key,"pattern_sha256":sha256_hex(&serde_json::to_vec(pattern.raw()).unwrap())}))
    }).unwrap();
    assert_eq!(report.schema, "ftx-design-scenario-matrix-study/1");
    assert_eq!(report.planned_evaluations, 16);
    assert_eq!(report.evaluations, 12);
    assert_eq!(calls, 12);
    let report = serde_json::to_value(report).unwrap();
    for case in report["cases"].as_array().unwrap() {
        for simulation in case["simulations"].as_array().unwrap() {
            let manufacturing = report["manufacturing"]["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["key"] == simulation["key"])
                .unwrap();
            if manufacturing["binding"]["status"] == "available" {
                assert_eq!(
                    simulation["evaluation"]["result"]["pattern_sha256"],
                    manufacturing["binding"]["result"]["pattern_sha256"]
                );
            } else {
                assert_eq!(simulation["evaluation"]["status"], "refused");
            }
        }
    }
}

#[test]
fn expanded_material_geometry_and_operation_budgets_cannot_be_bypassed_by_late_refusals() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let original = request();
    let mut oversized_rows = original.clone();
    oversized_rows["designs"] = json!((0..16).map(|i| json!({"key":format!("missing-{i}"),"operations":[{"op":"move","piece":"missing","vertex":"x","dx_mm":0.0,"dy_mm":1.0}]})).collect::<Vec<_>>());
    let mut oversized_operations = original.clone();
    let operations =
        vec![json!({"op":"move","piece":"missing","vertex":"x","dx_mm":0.0,"dy_mm":1.0}); 16];
    oversized_operations["designs"] = json!(
        (0..15)
            .map(|i| json!({"key":format!("missing-{i}"),"operations":operations}))
            .collect::<Vec<_>>()
    );
    let mut oversized_inventory = original;
    oversized_inventory["cutting"]["copies"] = json!(1025);
    for input in [oversized_rows, oversized_operations, oversized_inventory] {
        let typed: Request = serde_json::from_value(input).unwrap();
        let mut calls = 0;
        let error = typed
            .evaluate_with(&pattern, &json!({}), |_, _| {
                calls += 1;
                Ok(json!({}))
            })
            .unwrap_err();
        assert_eq!(error.code, RefusalCode::BudgetExceeded);
        assert_eq!(calls, 0);
    }
    // 16 rows and 224 operators fit their count limits, but each root search reserves its trials.
    let mut source = pattern.raw().clone();
    source["meta"]["annotation"] = json!("x".repeat(20_000));
    let mut input = request();
    let roots = vec![json!({"op":"true_seam","seam":"side","end":"start"}); 16];
    input["designs"] = json!(
        (0..7)
            .map(|i| json!({"key":format!("roots-{i}"),"operations":roots}))
            .collect::<Vec<_>>()
    );
    let typed: Request = serde_json::from_value(input).unwrap();
    let mut calls = 0;
    let error = typed
        .evaluate_with(&Pattern::new(source), &json!({}), |_, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(error.code, RefusalCode::BudgetExceeded);
    assert!(error.message.contains("document-processing"));
    assert_eq!(calls, 0);

    // One input record fits the native input cap, but repeating it across 32 designs must not
    // silently create hundreds of MiB of binding payloads after the reservation has passed.
    let mut input = request();
    input["variants"].as_array_mut().unwrap().truncate(1);
    input["variants"][0]["changes"]["default"]["record"]["sources"]["test"]["note"] =
        json!("x".repeat(8_400_000));
    input["designs"] = json!(
        (0..31)
            .map(|i| json!({"key":format!("payload-{i}"),"operations":[
        {"op":"move","piece":"missing","vertex":"x","dx_mm":0.0,"dy_mm":1.0}]}))
            .collect::<Vec<_>>()
    );
    let typed: Request = serde_json::from_value(input).unwrap();
    let error = typed
        .evaluate_with(&pattern, &json!({}), |_, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(error.code, RefusalCode::BudgetExceeded);
    assert!(error.message.contains("document-processing"));
    assert_eq!(calls, 0);
}

#[test]
fn opt_in_and_unique_canonical_pair_keys_preserve_fixed_pattern_meaning() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let mut fixed = request();
    fixed["schema"] = json!("ftx-material-study/1");
    let typed: Request = serde_json::from_value(fixed).unwrap();
    assert!(
        typed
            .evaluate(&pattern)
            .unwrap_err()
            .message
            .contains("geometry fixed")
    );
    let mut input = request();
    input["variants"][0]["key"] = json!("a\",\"b");
    input["variants"][1]["key"] = json!("b");
    input["designs"][0]["key"] = json!("x");
    input["designs"][1]["key"] = json!("x\",\"a");
    let report = evaluate(input.clone(), &pattern);
    let pairs: std::collections::BTreeSet<_> = report["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["key"].as_str().unwrap())
        .collect();
    assert_eq!(pairs.len(), 8);
    for row in report["alternatives"].as_array().unwrap() {
        let pair: Value = serde_json::from_str(row["key"].as_str().unwrap()).unwrap();
        assert_eq!(
            pair,
            json!([row["design"]["key"], row["design"]["material_key"]])
        );
    }
    for bad_key in ["baseline", "", "x\",\"a"] {
        let mut bad = input.clone();
        bad["designs"][0]["key"] = json!(bad_key);
        assert!(
            serde_json::from_value::<Request>(bad)
                .unwrap()
                .evaluate(&pattern)
                .is_err()
        );
    }
    input["variants"][1]["key"] = input["variants"][0]["key"].clone();
    assert!(
        serde_json::from_value::<Request>(input)
            .unwrap()
            .evaluate(&pattern)
            .is_err()
    );
}

#[test]
fn screened_and_heterogeneous_material_sources_share_the_same_design_expansion() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let mut input = request();
    let selections: Vec<_> = input["variants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| json!({"key":v["key"],"selection":v["changes"]["default"]}))
        .collect();
    input.as_object_mut().unwrap().remove("variants");
    input["combinations"] =
        json!({"groups":[{"key":"shell","target":{"scope":"default"},"choices":selections}]});
    let combinations = evaluate(input.clone(), &pattern);
    assert_eq!(combinations["alternatives"].as_array().unwrap().len(), 8);
    for row in combinations["alternatives"].as_array().unwrap() {
        assert_eq!(
            serde_json::from_str::<Value>(row["design"]["material_key"].as_str().unwrap()).unwrap(),
            row["choices"]
        );
    }
    input.as_object_mut().unwrap().remove("combinations");
    input["screened"] = json!({"request":{"schema":"ftx-material-screen/1","basis":"nominal","candidates":selections},"target":{"scope":"default"},"frontier_only":false});
    let screened = evaluate(input, &pattern);
    assert_eq!(screened["alternatives"].as_array().unwrap().len(), 8);
    assert_eq!(
        row(&screened, "longer", "light-sheet")["binding"]["result"]["pattern"],
        row(&evaluate(request(), &pattern), "longer", "light-sheet")["binding"]["result"]["pattern"]
    );
}
