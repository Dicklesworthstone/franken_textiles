//! Explicit multi-start discovery over real geometry and manufacturing, with synthetic sources.
use std::collections::BTreeMap;

use ftx_core::{RefusalCode, receipt::sha256_hex};
use ftx_export::{
    cutting::CutPlan,
    material_study::{Request, design::fit::Context, matrix::Case},
    sewing::SewPlan,
};
use ftx_pattern::{Pattern, validate::Measurements};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");

fn request(bounded: bool, evaluations: usize, restarts: Value) -> Request {
    let mut input: Value =
        serde_json::from_str(include_str!("../../../examples/design-study.json")).unwrap();
    input.as_object_mut().unwrap().remove("designs");
    input["design_adaptive"] = json!({"objective":"consumer_comparison","material_key":"light-sheet",
        "variables":[{"key":"length","bounds_mm":[0,60],"initial_mm":0,
            "initial_step_mm":20,"minimum_step_mm":20,"sites":[
                {"piece":"front","axis":"y","at_mm":50,"multiplier":1},
                {"piece":"back","axis":"y","at_mm":50,"multiplier":1}]}],
        "restart_points_mm":restarts,"max_evaluations":evaluations,"max_designs":1});
    input["design_fit"] = json!({"fit_spec":{"schema":"fabric-fit/1","points":[
        {"id":"length","kind":"vertical","instance":"front","from":"a","to":"d",
         "body":"desired_length","target_mm":[0,30,60]}]}});
    input["assembly_mass"] = json!({"schema":"ftx-assembly-mass/1","inventory_complete":true,
        "seam_allowance_mm":0,"extras":[]});
    input["cutting"]["seam_allowance_mm"] = json!(0);
    input["comparison"] = json!({"basis":if bounded { "reported_bounds" } else { "nominal" },
        "required_consumers":["cutting","sewing"],
        "objectives":[{"metric":"assembly_mass_kg","direction":"minimize"}],
        "constraints":[{"metric":"assembly_mass_kg","minimum":0.001}]});
    for (index, scale) in [(0, 1.0), (1, 2.0)] {
        input["variants"][index]["changes"]["default"]["record"]["properties"]["areal_density"]["bounds"] =
            json!([0.17 * scale, 0.19 * scale]);
    }
    serde_json::from_value(input).unwrap()
}

fn run(request: &Request) -> Value {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let identity = json!({"source":"test:restart-body"});
    serde_json::to_value(
        request
            .evaluate_with_fit_body(
                &pattern,
                Some(&Context {
                    body: &body,
                    identity: &identity,
                }),
            )
            .unwrap(),
    )
    .unwrap()
}

fn selected(report: &Value) -> &Value {
    report["design_search"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["selected"] == true)
        .unwrap()
}

#[test]
fn restarts_reach_a_separated_feasible_region_without_promoting_failed_constraints() {
    for bounded in [false, true] {
        let original = run(&request(bounded, 12, json!([])));
        let search = &original["design_search"];
        assert_eq!(search["selected_designs"], 0);
        assert_eq!(search["stop_reason"], "minimum_steps");
        assert_eq!(search["evaluations"], 2);
        assert!(search.get("search_starts").is_none());
        assert!(search["request"].get("restart_points_mm").is_none());
        assert!(
            search["rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["max_fit_band_violation_mm"] == 0.0
                    && row["consumer_comparison"]["eligible"] == false)
        );

        let request = request(bounded, 12, json!([{"length":60},{"length":40}]));
        let report = run(&request);
        assert_eq!(report, run(&request));
        let search = &report["design_search"];
        let chosen = selected(&report);
        assert_eq!(
            chosen["design"]["variables_mm"]["length"],
            if bounded { 60.0 } else { 40.0 }
        );
        assert_eq!(search["evaluations"], 4);
        assert_eq!(search["selected_designs"], 1);
        assert_eq!(search["search_starts"].as_array().unwrap().len(), 3);
        assert_eq!(search["search_starts"][0]["evaluations"], 2);
        assert_eq!(search["search_starts"][1]["evaluations"], 2);
        assert_eq!(search["search_starts"][2]["evaluations"], 0);
        assert_eq!(search["consumer_guidance"]["reserved_evaluations"], 16);
        assert_eq!(search["complete_search"], false);
        assert_eq!(search["global_optimum_claim"], false);
        let key =
            serde_json::to_string(&(chosen["design"]["key"].as_str().unwrap(), "light-sheet"))
                .unwrap();
        let row = report["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == key)
            .unwrap();
        let bound = Pattern::new(row["binding"]["result"]["pattern"].clone());
        assert_eq!(
            row["cutting"]["result"],
            serde_json::to_value(
                CutPlan::build(&bound, request.cutting.as_ref().unwrap()).unwrap()
            )
            .unwrap()
        );
        let mass = chosen["consumer_comparison"]["readings"]["assembly_mass_kg"]["value"]
            .as_f64()
            .unwrap();
        let length = if bounded { 160.0 } else { 140.0 };
        assert!((mass - 2.0 * 20.0 * length * 1e-6 * 0.18).abs() < 1e-12);
    }
}

#[test]
fn restart_trials_share_the_global_budget_and_preserve_the_admitted_incumbent() {
    let request = request(false, 3, json!([{"length":60},{"length":40}]));
    let report = run(&request);
    let search = &report["design_search"];
    assert_eq!(search["evaluations"], 3);
    assert_eq!(search["stop_reason"], "evaluation_budget");
    assert_eq!(search["budget_exhausted"], true);
    assert_eq!(search["search_starts"].as_array().unwrap().len(), 2);
    assert_eq!(search["search_starts"][1]["evaluations"], 1);
    assert_eq!(
        search["search_starts"][1]["stop_reason"],
        "evaluation_budget"
    );
    assert_eq!(selected(&report)["design"]["variables_mm"]["length"], 60.0);
    assert_eq!(search["consumer_guidance"]["reserved_evaluations"], 7);
    let report = run(&request_with_budget(&request, 2));
    assert_eq!(report["design_search"]["selected_designs"], 0);
    assert_eq!(
        report["design_search"]["search_starts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

fn request_with_budget(request: &Request, evaluations: usize) -> Request {
    let mut request = request.clone();
    request.design_adaptive.as_mut().unwrap().max_evaluations = evaluations;
    request
}

#[test]
fn cached_restart_coordinates_do_not_rerun_named_case_consumers() {
    for worst in [false, true] {
        let mut request = request(false, 12, json!([{"length":60},{"length":40}]));
        let adaptive = request.design_adaptive.as_mut().unwrap();
        if worst {
            adaptive.objective =
                ftx_export::material_study::design::adaptive::Objective::ConsumerWorstCase;
        } else {
            adaptive.case_key = Some("first".into());
        }
        let cases: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|key| Case {
                key: key.into(),
                scenario: json!({"name":key}),
                comparison: None,
            })
            .collect();
        let pattern = Pattern::from_json(PATTERN).unwrap();
        let body = Measurements(json!({"desired_length":100}));
        let identity = json!({"source":"test:restart-body"});
        let fit = Context {
            body: &body,
            identity: &identity,
        };
        let mut calls = BTreeMap::new();
        let report=request.evaluate_matrix_with_fit(&pattern,&cases,32,Some(&fit),|case,key,bound|{
            *calls.entry((case,key.to_owned())).or_insert(0)+=1;
            Ok(json!({"original":sha256_hex(&serde_json::to_vec(bound.raw()).unwrap()),"case":case}))
        }).unwrap();
        let report = serde_json::to_value(report).unwrap();
        let search = &report["manufacturing"]["design_search"];
        assert_eq!(search["selected_designs"], 1);
        assert_eq!(search["evaluations"], 4);
        assert_eq!(report["planned_evaluations"], 32);
        assert_eq!(
            report["evaluations"].as_u64().unwrap() as usize,
            calls.len()
        );
        assert!(calls.values().all(|&count| count == 1));
        assert_eq!(search["search_starts"][2]["evaluations"], 0);
        assert_eq!(search["consumer_guidance"]["evaluations"], 4);
        let chosen = selected(&report["manufacturing"]);
        let key =
            serde_json::to_string(&(chosen["design"]["key"].as_str().unwrap(), "light-sheet"))
                .unwrap();
        for case in 0..2 {
            assert_eq!(calls[&(case, key.clone())], 1);
        }
    }
}

#[test]
fn invalid_restart_sets_refuse_before_any_consumer_work() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let identity = json!({"source":"test:restart-body"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let mut calls = 0;
    for points in [
        json!([{}]),
        json!([{"unknown":40}]),
        json!([{"length":40,"unknown":2}]),
        json!([{"length":-1}]),
        json!([{"length":61}]),
        json!([{"length":0}]),
        json!([{"length":-0.0}]),
        json!([{"length":40},{"length":40}]),
        json!((1..=32).map(|x| json!({"length":x})).collect::<Vec<_>>()),
    ] {
        let bad = request(false, 12, points);
        assert!(
            bad.evaluate_with_fit(&pattern, &json!({}), Some(&fit), |_, _| {
                calls += 1;
                Ok(json!({}))
            })
            .is_err()
        );
    }
    for value in [f64::INFINITY, f64::NAN] {
        let mut bad = request(false, 12, json!([{"length":40}]));
        bad.design_adaptive.as_mut().unwrap().restart_points_mm[0].insert("length".into(), value);
        assert!(
            bad.evaluate_with_fit(&pattern, &json!({}), Some(&fit), |_, _| {
                calls += 1;
                Ok(json!({}))
            })
            .is_err()
        );
    }
    assert_eq!(calls, 0);
}

#[test]
fn restarts_preserve_cached_incumbent_after_shared_consumer_budget_failure() {
    let mut request = request(false, 12, json!([{"length":60}]));
    // An admitted initial incumbent anchors protocols even across later local searches.
    let mut value = serde_json::to_value(&request).unwrap();
    value["comparison"]["constraints"] = json!([]);
    value["comparison"]["objectives"] =
        json!([{"metric":"simulated_cloth_mass_kg","direction":"minimize"}]);
    request = serde_json::from_value(value).unwrap();
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let identity = json!({"source":"test:restart-body"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let descriptor =
        json!({"scenario_sha256":"1".repeat(64),"body_geometry_sha256":"2".repeat(64)});
    let report=request.evaluate_with_fit(&pattern,&descriptor,Some(&fit),|_,bound| {
        let length=bound.raw()["pieces"][0]["vertices"]["d"][1].as_f64().unwrap();
        if length>=140.0 { return Err(ftx_core::Refusal::new(RefusalCode::BudgetExceeded,"no remaining shared case budget")); }
        Ok(json!({"schema":"ftx-drape-scenario-result/1",
            "pattern_sha256":sha256_hex(&serde_json::to_vec(bound.raw()).unwrap()),
            "scenario_sha256":descriptor["scenario_sha256"],"body_geometry_sha256":descriptor["body_geometry_sha256"],
            "metrics":{"simulation_gating_clear":true},"drape":{"settled":true,"gating":[],"mass_kg":length}}))
    }).unwrap();
    let report = serde_json::to_value(report).unwrap();
    let search = &report["manufacturing"]["design_search"];
    assert_eq!(
        selected(&report["manufacturing"])["design"]["variables_mm"]["length"],
        0.0
    );
    assert!(
        search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["consumer_refusal"]["code"] == "budget_exceeded"
                && row["selected"] == false)
    );
}

fn restoring_request(bounded: bool, evaluations: usize) -> Request {
    let mut value = serde_json::to_value(request(bounded, evaluations, json!([]))).unwrap();
    value["design_adaptive"]["constraint_restoration"] = json!("componentwise");
    serde_json::from_value(value).unwrap()
}

#[test]
fn constraint_restoration_reaches_real_manufacturing_feasibility_without_guessed_restarts() {
    for bounded in [false, true] {
        let original = run(&request(bounded, 12, json!([])));
        let original_search = &original["design_search"];
        assert_eq!(original_search["selected_designs"], 0);
        assert_eq!(original_search["evaluations"], 2);
        assert!(
            original_search["request"]
                .get("constraint_restoration")
                .is_none()
        );
        assert!(
            original_search["rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| {
                    row.get("constraint_violations").is_none()
                        && row["consumer_comparison"]
                            .get("constraint_violations")
                            .is_none()
                })
        );

        let request = restoring_request(bounded, 12);
        let report = run(&request);
        assert_eq!(report, run(&request));
        let search = &report["design_search"];
        assert_eq!(search["evaluations"], 4);
        assert_eq!(search["selected_designs"], 1);
        assert_eq!(search["stop_reason"], "minimum_steps");
        assert_eq!(search["complete_search"], false);
        assert_eq!(search["global_optimum_claim"], false);
        assert!(search.get("search_starts").is_none());
        let chosen = selected(&report);
        assert_eq!(
            chosen["design"]["variables_mm"]["length"],
            if bounded { 60.0 } else { 40.0 }
        );
        for trial in search["rows"].as_array().unwrap() {
            let reading = &trial["consumer_comparison"]["readings"]["assembly_mass_kg"];
            let low = reading[if bounded { "reported_bounds" } else { "value" }].clone();
            let low = if bounded {
                low[0].as_f64().unwrap()
            } else {
                low.as_f64().unwrap()
            };
            let guard = &trial["constraint_violations"][0];
            assert_eq!(guard["metric"], "assembly_mass_kg");
            assert_eq!(guard["material_key"], "light-sheet");
            assert!(guard.get("case_key").is_none());
            assert_eq!(guard["unit"], "kg");
            assert_eq!(
                guard["basis"],
                if bounded {
                    "reported_bounds"
                } else {
                    "nominal"
                }
            );
            assert_eq!(guard["minimum_shortfall"], json!((0.001 - low).max(0.0)));
            assert!(guard.get("maximum_excess").is_none());
            if low < 0.001 {
                assert_eq!(trial["consumer_comparison"]["eligible"], false);
                assert_eq!(trial["selected"], false);
                assert!(trial["refusal"].is_object());
            }
        }
        let key =
            serde_json::to_string(&(chosen["design"]["key"].as_str().unwrap(), "light-sheet"))
                .unwrap();
        let row = report["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == key)
            .unwrap();
        let bound = Pattern::new(row["binding"]["result"]["pattern"].clone());
        assert_eq!(
            row["cutting"]["result"],
            serde_json::to_value(
                CutPlan::build(&bound, request.cutting.as_ref().unwrap()).unwrap()
            )
            .unwrap()
        );
        let length = if bounded { 160.0 } else { 140.0 };
        assert!(
            (chosen["consumer_comparison"]["readings"]["assembly_mass_kg"]["value"]
                .as_f64()
                .unwrap()
                - 2.0 * 20.0 * length * 1e-6 * 0.18)
                .abs()
                < 1e-12
        );
    }
}

#[test]
fn constraint_restoration_keeps_guard_sides_and_physical_units_independent() {
    for guards in [
        json!([{"metric":"assembly_mass_kg","minimum":0.0008,"maximum":0.00082}]),
        json!([{"metric":"assembly_mass_kg","minimum":0.001},{"metric":"feed_seconds","maximum":0}]),
    ] {
        let mut value = serde_json::to_value(restoring_request(false, 12)).unwrap();
        value["comparison"]["constraints"] = guards;
        let report = run(&serde_json::from_value(value).unwrap());
        let search = &report["design_search"];
        assert_eq!(search["evaluations"], 2);
        assert_eq!(search["selected_designs"], 0);
        assert_eq!(search["stop_reason"], "minimum_steps");
        let vectors: Vec<_> = search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["constraint_violations"].as_array().unwrap())
            .collect();
        let mass = |index: usize| {
            vectors[index]
                .iter()
                .find(|guard| guard["metric"] == "assembly_mass_kg")
                .unwrap()
        };
        assert!(
            mass(1)["minimum_shortfall"].as_f64().unwrap()
                < mass(0)["minimum_shortfall"].as_f64().unwrap()
        );
        let worsening = |index: usize| {
            vectors[index]
                .iter()
                .find(|guard| guard.get("maximum_excess").is_some())
                .unwrap()
        };
        assert!(
            worsening(1)["maximum_excess"].as_f64().unwrap()
                > worsening(0)["maximum_excess"].as_f64().unwrap()
        );
    }
}

#[test]
fn constraint_restoration_preserves_budgets_missing_evidence_and_required_consumer_refusals() {
    for (bounded, allowance, expected) in [(false, 2, 0), (false, 3, 1), (true, 3, 0)] {
        let report = run(&restoring_request(bounded, allowance));
        assert_eq!(report["design_search"]["evaluations"], allowance);
        assert_eq!(report["design_search"]["selected_designs"], expected);
        assert_eq!(report["design_search"]["budget_exhausted"], true);
        if expected > 0 {
            assert_eq!(selected(&report)["design"]["variables_mm"]["length"], 40.0);
        }
    }
    for missing in ["bounds", "consumer"] {
        let mut value = serde_json::to_value(restoring_request(true, 12)).unwrap();
        if missing == "bounds" {
            value["variants"][0]["changes"]["default"]["record"]["properties"]["areal_density"]
                .as_object_mut()
                .unwrap()
                .remove("bounds");
        } else {
            // The declared roll widths are 100 and 80 mm; two 60 mm edge margins
            // leave no usable stock. Cutting is requested but refuses per alternative,
            // while the independent assembly-mass consumer retains its source evidence.
            value["cutting"]["edge_margin_mm"] = json!(60);
        }
        let report = run(&serde_json::from_value(value).unwrap());
        assert_eq!(report["design_search"]["evaluations"], 2, "{missing}");
        assert_eq!(report["design_search"]["selected_designs"], 0, "{missing}");
        for row in report["design_search"]["rows"].as_array().unwrap() {
            assert!(row.get("constraint_violations").is_none(), "{missing}");
            if missing == "consumer" {
                let mass = &row["consumer_comparison"]["readings"]["assembly_mass_kg"];
                assert!(mass["value"].as_f64().unwrap() > 0.0);
                assert_eq!(mass["reported_bounds"].as_array().unwrap().len(), 2);
            }
            assert!(
                row["consumer_comparison"]["exclusions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| {
                        e["message"]
                            .as_str()
                            .unwrap()
                            .contains(if missing == "bounds" {
                                "bounds"
                            } else {
                                "Cutting"
                            })
                    })
            );
        }
    }
}

#[test]
fn constraint_restoration_uses_every_named_case_and_reuses_only_eligible_cached_outputs() {
    for mode in [
        "consumer_comparison",
        "consumer_worst_case",
        "consumer_pareto",
        "consumer_matrix_pareto",
    ] {
        let mut value = serde_json::to_value(restoring_request(false, 12)).unwrap();
        value["design_adaptive"]["objective"] = json!(mode);
        if matches!(mode, "consumer_comparison" | "consumer_pareto") {
            value["design_adaptive"]["case_key"] = json!("first");
        }
        if mode == "consumer_pareto" {
            value["comparison"]["objectives"]
                .as_array_mut()
                .unwrap()
                .push(json!({"metric":"feed_seconds","direction":"minimize"}));
        }
        let request: Request = serde_json::from_value(value).unwrap();
        let mut range_guard = request.comparison.clone().unwrap();
        range_guard.constraints[0].basis =
            Some(ftx_export::material_study::comparison::Basis::ReportedBounds);
        let cases = vec![
            Case {
                key: "first".into(),
                scenario: json!({"case":"nominal"}),
                comparison: None,
            },
            Case {
                key: "range-guard".into(),
                scenario: json!({"case":"source-range"}),
                comparison: Some(range_guard),
            },
        ];
        let pattern = Pattern::from_json(PATTERN).unwrap();
        let body = Measurements(json!({"desired_length":100}));
        let identity = json!({"source":"test:restoration-body"});
        let fit = Context {
            body: &body,
            identity: &identity,
        };
        let mut calls = BTreeMap::new();
        let report = request
            .evaluate_matrix_with_fit(&pattern, &cases, 32, Some(&fit), |case, key, bound| {
                *calls.entry((case, key.to_owned())).or_insert(0) += 1;
                Ok(json!({"original":sha256_hex(&serde_json::to_vec(bound.raw()).unwrap()),"case":case}))
            })
            .unwrap();
        let report = serde_json::to_value(report).unwrap();
        let search = &report["manufacturing"]["design_search"];
        assert_eq!(search["evaluations"], 4, "{mode}");
        assert_eq!(search["selected_designs"], 1, "{mode}");
        let chosen = selected(&report["manufacturing"]);
        assert_eq!(chosen["design"]["variables_mm"]["length"], 60.0, "{mode}");
        let rows = search["rows"].as_array().unwrap();
        assert_eq!(
            rows[0]["constraint_violations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| (
                    v["case_key"].as_str().unwrap(),
                    v["basis"].as_str().unwrap()
                ))
                .collect::<Vec<_>>(),
            vec![("first", "nominal"), ("range-guard", "reported_bounds")]
        );
        let blocked = rows
            .iter()
            .find(|row| row["design"]["variables_mm"]["length"] == 40.0)
            .unwrap();
        assert_eq!(blocked["consumer_cases"][0]["comparison"]["eligible"], true);
        assert_eq!(
            blocked["consumer_cases"][1]["comparison"]["eligible"],
            false
        );
        assert_eq!(blocked["selected"], false);
        assert!(calls.values().all(|&count| count == 1), "{mode}");
        let key =
            serde_json::to_string(&(chosen["design"]["key"].as_str().unwrap(), "light-sheet"))
                .unwrap();
        for case in 0..cases.len() {
            assert_eq!(calls[&(case, key.clone())], 1);
        }
    }
}

#[test]
fn constraint_restoration_requires_complete_evidence_from_every_material_nominee() {
    for missing in [false, true] {
        let mut value = serde_json::to_value(restoring_request(true, 12)).unwrap();
        value["design_adaptive"]
            .as_object_mut()
            .unwrap()
            .remove("material_key");
        value["design_adaptive"]["material_keys"] = json!(["light-sheet", "heavy-sheet"]);
        value["design_adaptive"]["objective"] = json!("consumer_pareto");
        value["comparison"]["constraints"][0]["minimum"] = json!(0.0021);
        if missing {
            value["variants"][0]["changes"]["default"]["record"]["properties"]["areal_density"]
                .as_object_mut()
                .unwrap()
                .remove("bounds");
        }
        let report = run(&serde_json::from_value(value).unwrap());
        let search = &report["design_search"];
        assert_eq!(search["evaluations"], if missing { 2 } else { 4 });
        assert_eq!(search["selected_designs"], if missing { 0 } else { 1 });
        if missing {
            assert!(search["rows"][0].get("constraint_violations").is_none());
        } else {
            assert_eq!(selected(&report)["design"]["variables_mm"]["length"], 60.0);
            assert_eq!(
                search["rows"][0]["constraint_violations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v["material_key"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                vec!["light-sheet", "heavy-sheet"]
            );
            assert_eq!(search["consumer_guidance"]["evaluations"], 8);
        }
    }
}

#[test]
fn constraint_restoration_refuses_empty_or_nonconsumer_policies_before_callbacks() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let identity = json!({"source":"test:restoration-body"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    for mode in ["no-guards", "fit-only"] {
        let mut value = serde_json::to_value(restoring_request(false, 12)).unwrap();
        if mode == "no-guards" {
            value["comparison"]["constraints"] = json!([]);
        } else {
            value["design_adaptive"]["objective"] = json!("fit_band_violation_mm");
            value["design_adaptive"]
                .as_object_mut()
                .unwrap()
                .remove("material_key");
        }
        let request: Request = serde_json::from_value(value).unwrap();
        let mut calls = 0;
        let error = request
            .evaluate_with_fit(&pattern, &json!({}), Some(&fit), |_, _| {
                calls += 1;
                Ok(json!({}))
            })
            .unwrap_err();
        assert!(
            error.message.contains("constraint_restoration"),
            "{mode}: {error:?}"
        );
        assert_eq!(calls, 0);
    }
    let mut unknown = serde_json::to_value(restoring_request(false, 12)).unwrap();
    unknown["design_adaptive"]["constraint_restoration"] = json!("weighted_sum");
    assert!(serde_json::from_value::<Request>(unknown).is_err());
}

fn independent_lengths(evaluations: usize, joint: bool) -> Request {
    let mut value = serde_json::to_value(request(false, evaluations, json!([]))).unwrap();
    value["design_adaptive"]["objective"] = json!("fit_band_violation_mm");
    value["design_adaptive"]
        .as_object_mut()
        .unwrap()
        .remove("material_key");
    let original = value["design_adaptive"]["variables"][0].clone();
    value["design_adaptive"]["variables"] = Value::Array(
        ["front", "back"]
            .into_iter()
            .enumerate()
            .map(|(index, piece)| {
                let mut variable = original.clone();
                variable["key"] = json!(piece);
                variable["sites"] = json!([original["sites"][index]]);
                variable
            })
            .collect(),
    );
    if joint {
        value["design_adaptive"]["polling"] = json!("axis_and_joint");
    }
    value["design_fit"]["fit_spec"]["points"][0]["target_mm"] = json!([40, 40, 40]);
    serde_json::from_value(value).unwrap()
}

#[test]
fn joint_polling_crosses_a_seam_corridor_and_replays_real_manufacturing() {
    let old = run(&independent_lengths(24, false));
    assert_eq!(old["design_search"]["selected_designs"], 0);
    assert_eq!(old["design_search"]["evaluations"], 3);
    assert!(old["design_search"]["request"].get("polling").is_none());
    for row in old["design_search"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
    {
        assert_eq!(
            row["design"]["validation"]["flat"]["summary"]["seams_failing"],
            1
        );
        assert!(row["max_fit_band_violation_mm"].is_null());
        assert_eq!(row["selected"], false);
    }

    let request = independent_lengths(24, true);
    let report = run(&request);
    assert_eq!(report, run(&request));
    let search = &report["design_search"];
    assert_eq!(search["request"]["polling"], "axis_and_joint");
    assert_eq!(search["evaluations"], 9);
    assert_eq!(search["stop_reason"], "qualified_design_limit");
    assert_eq!(search["selected_designs"], 1);
    assert_eq!(search["complete_search"], false);
    assert_eq!(search["global_optimum_claim"], false);
    let chosen = selected(&report);
    assert_eq!(
        chosen["design"]["variables_mm"],
        json!({"front":40.0,"back":40.0})
    );
    assert_eq!(chosen["raw_fit"]["result"][0]["finished_mm"], 140.0);
    for (material, density) in [("light-sheet", 0.18), ("heavy-sheet", 0.36)] {
        let key =
            serde_json::to_string(&(chosen["design"]["key"].as_str().unwrap(), material)).unwrap();
        let row = report["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == key)
            .unwrap();
        let bound = Pattern::new(row["binding"]["result"]["pattern"].clone());
        assert_eq!(
            row["cutting"]["result"],
            serde_json::to_value(
                CutPlan::build(&bound, request.cutting.as_ref().unwrap()).unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            row["sewing"]["result"],
            serde_json::to_value(SewPlan::build(&bound, request.sewing.as_ref().unwrap()).unwrap())
                .unwrap()
        );
        assert_eq!(
            row["assembly_mass"]["result"],
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
        assert!(
            (row["metrics"]["assembly_mass_kg"].as_f64().unwrap()
                - 2.0 * 20.0 * 140.0 * 1e-6 * density)
                .abs()
                < 1e-12
        );
    }
}

#[test]
fn joint_polling_keeps_independent_controls_along_a_nonlinear_seam_relation() {
    let mut input = serde_json::to_value(independent_lengths(256, false)).unwrap();
    input["design_adaptive"]["variables"][0]["bounds_mm"] = json!([0, 40]);
    input["design_adaptive"]["variables"][0]["minimum_step_mm"] = json!(2.5);
    input["design_adaptive"]["variables"][1] = json!({
        "key":"back","bounds_mm":[0,120],"initial_mm":0,
        "initial_step_mm":66.332,"minimum_step_mm":8.2915,
        "sites":[{"op":"move","piece":"back","vertex":"a",
            "dx_multiplier":-1,"dy_multiplier":0},
            {"op":"move","piece":"back","vertex":"b",
                "dx_multiplier":-1,"dy_multiplier":-0.2},
            {"op":"move","piece":"back","vertex":"c",
                "dx_multiplier":0,"dy_multiplier":-0.2}]});
    input["sewing"]["seam_tolerance_mm"] = json!(3);
    for variant in input["variants"].as_array_mut().unwrap() {
        variant["changes"]["default"]["record"]["properties"]["width"]["value"] = json!(0.2);
    }
    let old = run(&serde_json::from_value(input.clone()).unwrap());
    assert_eq!(old["design_search"]["selected_designs"], 0);
    input["design_adaptive"]["polling"] = json!("axis_and_joint");
    let mut corner_only = input.clone();
    corner_only["design_adaptive"]["variables"][1]["sites"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    let refused = run(&serde_json::from_value(corner_only).unwrap());
    assert_eq!(refused["design_search"]["selected_designs"], 0);
    assert!(
        refused["design_search"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |row| row["design"]["validation"]["flat"]["summary"]["truing_failing"]
                    .as_u64()
                    .is_some_and(|n| n > 0)
            )
    );
    let request: Request = serde_json::from_value(input).unwrap();
    let report = run(&request);
    let chosen = selected(&report);
    let front = chosen["design"]["variables_mm"]["front"].as_f64().unwrap();
    let altered: Value =
        serde_json::from_str(chosen["design"]["altered_pattern_json"].as_str().unwrap()).unwrap();
    let back = -altered["pieces"][1]["vertices"]["a"][0].as_f64().unwrap();
    // The front seam is 100 + front; moving the back's bottom vertex makes its seam
    // sqrt(100^2 + back^2). A fixed 66.332/20 linked ratio cannot retain this curve.
    // Its continuation (20, -0.2 * back) stays perpendicular to (back, 100),
    // so this positive corridor also satisfies the unchanged corner-truing gate.
    assert_eq!(front, 40.0);
    assert!(((10000.0 + back * back).sqrt() - 140.0).abs() <= 3.0);
    assert!((back - front * 66.332 / 20.0).abs() > 20.0);
    assert_eq!(
        chosen["design"]["validation"]["flat"]["summary"]["seams_failing"],
        0
    );
    assert_eq!(
        chosen["design"]["validation"]["flat"]["summary"]["truing_failing"],
        0
    );
    let key =
        serde_json::to_string(&(chosen["design"]["key"].as_str().unwrap(), "light-sheet")).unwrap();
    let row = report["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == key)
        .unwrap();
    assert_eq!(row["cutting"]["status"], "available");
    assert_eq!(row["sewing"]["status"], "available");
    let bottom_y = altered["pieces"][1]["vertices"]["b"][1].as_f64().unwrap();
    let independent_area = 20.0 * 140.0 + 20.0 * 100.0 - back * bottom_y;
    assert!(
        (row["metrics"]["assembly_mass_kg"].as_f64().unwrap() - independent_area * 1e-6 * 0.18)
            .abs()
            < 1e-12
    );
}

#[test]
fn joint_polling_reaches_a_three_control_corridor_with_both_signed_directions() {
    let mut source: Value = serde_json::from_str(PATTERN).unwrap();
    let mut third = source["pieces"][1].clone();
    third["id"] = json!("lining");
    source["pieces"].as_array_mut().unwrap().push(third);
    source["seams"].as_array_mut().unwrap().push(json!({
        "id":"lining-side","a":"back/right","b":"lining/left","reverse":true}));
    let pattern = Pattern::new(source);
    let body = Measurements(json!({"desired_length":100}));
    let identity = json!({"source":"test:joint-three-controls"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    for target in [-20.0, 20.0] {
        let mut input = serde_json::to_value(independent_lengths(32, true)).unwrap();
        let mut third = input["design_adaptive"]["variables"][1].clone();
        third["key"] = json!("lining");
        third["sites"][0]["piece"] = json!("lining");
        input["design_adaptive"]["variables"]
            .as_array_mut()
            .unwrap()
            .push(third);
        for variable in input["design_adaptive"]["variables"]
            .as_array_mut()
            .unwrap()
        {
            variable["bounds_mm"] = json!([-20, 20]);
        }
        input["design_fit"]["fit_spec"]["points"][0]["target_mm"] = json!([target, target, target]);
        let request: Request = serde_json::from_value(input).unwrap();
        let report = serde_json::to_value(
            request
                .evaluate_with_fit_body(&pattern, Some(&fit))
                .unwrap(),
        )
        .unwrap();
        let chosen = selected(&report);
        assert_eq!(
            chosen["design"]["variables_mm"],
            json!({"front":target,"back":target,"lining":target})
        );
        // Six axes and twelve pairs precede the eight ordered triple directions.
        assert_eq!(
            report["design_search"]["evaluations"],
            if target < 0.0 { 20 } else { 27 }
        );
        for row in report["design_search"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
        {
            if row["design"]["variables_mm"]["front"] != row["design"]["variables_mm"]["back"]
                || row["design"]["variables_mm"]["front"] != row["design"]["variables_mm"]["lining"]
            {
                assert_eq!(row["selected"], false);
                assert!(row["max_fit_band_violation_mm"].is_null());
            }
        }
    }
}

#[test]
fn joint_polling_shares_boundary_cache_restarts_and_exact_trial_budgets() {
    let mut input = serde_json::to_value(independent_lengths(12, true)).unwrap();
    for variable in input["design_adaptive"]["variables"]
        .as_array_mut()
        .unwrap()
    {
        variable["bounds_mm"] = json!([0, 20]);
    }
    input["design_adaptive"]["restart_points_mm"] = json!([{"front":20,"back":20}]);
    let request: Request = serde_json::from_value(input.clone()).unwrap();
    let report = run(&request);
    let search = &report["design_search"];
    assert_eq!(search["evaluations"], 4);
    assert_eq!(search["selected_designs"], 0);
    assert_eq!(search["stop_reason"], "minimum_steps");
    assert_eq!(search["search_starts"][1]["evaluations"], 0);
    let coordinates: std::collections::BTreeSet<_> = search["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["design"]["variables_mm"].to_string())
        .collect();
    assert_eq!(coordinates.len(), 4);
    for limit in [3, 4] {
        let report = run(&request_with_budget(&request, limit));
        assert_eq!(report["design_search"]["evaluations"], limit);
        assert_eq!(report["design_search"]["stop_reason"], "evaluation_budget");
        assert_eq!(report["design_search"]["selected_designs"], 0);
    }

    input["design_adaptive"]["max_evaluations"] = json!(4);
    input["design_adaptive"]["objective"] = json!("consumer_comparison");
    input["design_adaptive"]["material_key"] = json!("light-sheet");
    input["comparison"]["constraints"] = json!([]);
    input["design_fit"]["fit_spec"]["points"][0]["target_mm"] = json!([0, 30, 60]);
    let report = run(&serde_json::from_value(input).unwrap());
    assert_eq!(report["design_search"]["stop_reason"], "evaluation_budget");
    assert_eq!(report["design_search"]["evaluations"], 4);
    assert_eq!(report["design_search"]["selected_designs"], 1);
    assert_eq!(
        selected(&report)["design"]["variables_mm"],
        json!({"front":0.0,"back":0.0})
    );
    assert_eq!(
        report["design_search"]["consumer_guidance"]["evaluations"],
        2
    );
}

#[test]
fn joint_polling_composes_with_every_consumer_mode_restoration_and_exact_output_reuse() {
    for mode in [
        "consumer_comparison",
        "consumer_worst_case",
        "consumer_pareto",
        "consumer_matrix_pareto",
    ] {
        let mut value = serde_json::to_value(independent_lengths(24, true)).unwrap();
        value["design_adaptive"]["objective"] = json!(mode);
        value["design_adaptive"]["material_key"] = json!("light-sheet");
        value["design_adaptive"]["constraint_restoration"] = json!("componentwise");
        value["design_fit"]["fit_spec"]["points"][0]["target_mm"] = json!([0, 30, 60]);
        if matches!(mode, "consumer_comparison" | "consumer_pareto") {
            value["design_adaptive"]["case_key"] = json!("first");
        }
        if mode == "consumer_pareto" {
            value["comparison"]["objectives"]
                .as_array_mut()
                .unwrap()
                .push(json!({"metric":"feed_seconds","direction":"minimize"}));
        }
        let request: Request = serde_json::from_value(value).unwrap();
        let mut guard = request.comparison.clone().unwrap();
        guard.constraints[0].basis =
            Some(ftx_export::material_study::comparison::Basis::ReportedBounds);
        let cases = vec![
            Case {
                key: "first".into(),
                scenario: json!({"case":"nominal"}),
                comparison: None,
            },
            Case {
                key: "range-guard".into(),
                scenario: json!({"case":"source-range"}),
                comparison: Some(guard),
            },
        ];
        let pattern = Pattern::from_json(PATTERN).unwrap();
        let body = Measurements(json!({"desired_length":100}));
        let identity = json!({"source":"test:joint-consumer-body"});
        let fit = Context {
            body: &body,
            identity: &identity,
        };
        let mut calls = BTreeMap::new();
        let report = request
            .evaluate_matrix_with_fit(&pattern, &cases, 64, Some(&fit), |case, key, bound| {
                *calls.entry((case, key.to_owned())).or_insert(0) += 1;
                Ok(json!({"original":sha256_hex(&serde_json::to_vec(bound.raw()).unwrap()),"case":case}))
            })
            .unwrap();
        let report = serde_json::to_value(report).unwrap();
        let search = &report["manufacturing"]["design_search"];
        assert_eq!(search["evaluations"], 14, "{mode}");
        assert_eq!(search["selected_designs"], 1, "{mode}");
        assert_eq!(search["consumer_guidance"]["evaluations"], 4, "{mode}");
        let chosen = selected(&report["manufacturing"]);
        assert_eq!(
            chosen["design"]["variables_mm"],
            json!({"front":60.0,"back":60.0}),
            "{mode}"
        );
        let blocked = search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["design"]["variables_mm"] == json!({"front":40.0,"back":40.0}))
            .unwrap();
        assert_eq!(blocked["consumer_cases"][0]["comparison"]["eligible"], true);
        assert_eq!(
            blocked["consumer_cases"][1]["comparison"]["eligible"],
            false
        );
        assert_eq!(blocked["selected"], false);
        assert_eq!(
            blocked["constraint_violations"].as_array().unwrap().len(),
            2
        );
        for row in search["rows"].as_array().unwrap() {
            if row["max_fit_band_violation_mm"].is_null() {
                assert!(row.get("constraint_violations").is_none());
                let key =
                    serde_json::to_string(&(row["design"]["key"].as_str().unwrap(), "light-sheet"))
                        .unwrap();
                for case in 0..cases.len() {
                    assert!(!calls.contains_key(&(case, key.clone())));
                }
            }
        }
        assert!(calls.values().all(|&count| count == 1), "{mode}");
        let key =
            serde_json::to_string(&(chosen["design"]["key"].as_str().unwrap(), "light-sheet"))
                .unwrap();
        for case in 0..cases.len() {
            assert_eq!(calls[&(case, key.clone())], 1);
        }
    }
}

#[test]
fn joint_polling_preserves_single_variable_results_and_rejects_unbounded_modes() {
    let original = request(false, 12, json!([]));
    let before = run(&original);
    let mut value = serde_json::to_value(original).unwrap();
    assert!(value["design_adaptive"].get("polling").is_none());
    value["design_adaptive"]["polling"] = json!("axis_and_joint");
    let after = run(&serde_json::from_value(value.clone()).unwrap());
    assert_eq!(
        before["design_search"]["evaluations"],
        after["design_search"]["evaluations"]
    );
    for (a, b) in before["design_search"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .zip(after["design_search"]["rows"].as_array().unwrap())
    {
        assert_eq!(a["design"]["variables_mm"], b["design"]["variables_mm"]);
        assert_eq!(a["raw_fit"], b["raw_fit"]);
        assert_eq!(a["selected"], b["selected"]);
        assert_eq!(
            a["consumer_comparison"]["readings"],
            b["consumer_comparison"]["readings"]
        );
        assert_eq!(
            a["consumer_comparison"]["eligible"],
            b["consumer_comparison"]["eligible"]
        );
    }
    value["design_adaptive"]["polling"] = json!("diagonal_only");
    assert!(serde_json::from_value::<Request>(value).is_err());

    let mut invalid = serde_json::to_value(independent_lengths(24, true)).unwrap();
    let base = invalid["design_adaptive"]["variables"][0].clone();
    for key in ["third", "fourth"] {
        let mut variable = base.clone();
        variable["key"] = json!(key);
        invalid["design_adaptive"]["variables"]
            .as_array_mut()
            .unwrap()
            .push(variable);
    }
    let request: Request = serde_json::from_value(invalid).unwrap();
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let identity = json!({"source":"test:joint-admission"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let mut calls = 0;
    let error = request
        .evaluate_with_fit(&pattern, &json!({}), Some(&fit), |_, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(error.code, RefusalCode::InvalidDocument);
    assert_eq!(calls, 0);
}

#[test]
fn domain_restarts_find_remote_feasible_designs_without_authored_coordinates() {
    for bounded in [false, true] {
        let original = request(bounded, 12, json!([]));
        let before = run(&original);
        assert_eq!(before["design_search"]["selected_designs"], 0);
        assert!(
            before["design_search"]
                .get("domain_restart_points_mm")
                .is_none()
        );
        assert!(before["design_search"].get("search_starts").is_none());

        let mut request = original;
        request.design_adaptive.as_mut().unwrap().domain_restarts = Some(1);
        let report = run(&request);
        assert_eq!(report, run(&request));
        let search = &report["design_search"];
        assert_eq!(search["domain_restart_points_mm"], json!([{"length":30.0}]));
        assert_eq!(search["evaluations"], 6);
        assert_eq!(search["selected_designs"], 1);
        assert_eq!(search["stop_reason"], "minimum_steps");
        assert_eq!(search["complete_search"], false);
        assert_eq!(search["global_optimum_claim"], false);
        assert_eq!(search["search_starts"][0]["evaluations"], 2);
        assert!(
            search["search_starts"][0]
                .get("domain_restart_index")
                .is_none()
        );
        assert_eq!(search["search_starts"][1]["domain_restart_index"], 0);
        let chosen = selected(&report);
        assert_eq!(chosen["design"]["variables_mm"]["length"], 50.0);
        let key =
            serde_json::to_string(&(chosen["design"]["key"].as_str().unwrap(), "light-sheet"))
                .unwrap();
        let row = report["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == key)
            .unwrap();
        let bound = Pattern::new(row["binding"]["result"]["pattern"].clone());
        assert_eq!(
            row["cutting"]["result"],
            serde_json::to_value(
                CutPlan::build(&bound, request.cutting.as_ref().unwrap()).unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            row["sewing"]["result"],
            serde_json::to_value(SewPlan::build(&bound, request.sewing.as_ref().unwrap()).unwrap())
                .unwrap()
        );
        assert_eq!(
            row["assembly_mass"]["result"],
            serde_json::to_value(request.assembly_mass.as_ref().unwrap().evaluate(&bound).unwrap())
                .unwrap()
        );
    }
}

#[test]
fn domain_restarts_follow_authored_starts_and_reuse_the_shared_coordinate_cache() {
    let mut request = request(false, 24, json!([{"length":60},{"length":40}]));
    let original = run(&request);
    assert!(
        original["design_search"]
            .get("domain_restart_points_mm")
            .is_none()
    );
    assert!(
        original["design_search"]["search_starts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|start| start.get("domain_restart_index").is_none())
    );
    request.design_adaptive.as_mut().unwrap().domain_restarts = Some(3);
    let report = run(&request);
    let search = &report["design_search"];
    assert_eq!(
        search["domain_restart_points_mm"],
        json!([{"length":30.0},{"length":15.0},{"length":45.0}])
    );
    assert_eq!(
        search["request"]["restart_points_mm"],
        json!([{"length":60.0},{"length":40.0}])
    );
    let rows = search["rows"].as_array().unwrap();
    let starts = search["search_starts"].as_array().unwrap();
    assert_eq!(starts.len(), 6);
    for (index, expected) in [0.0, 60.0, 40.0, 30.0, 15.0, 45.0].into_iter().enumerate() {
        let row = rows
            .iter()
            .find(|row| row["design"]["key"] == starts[index]["initial_trial_key"])
            .unwrap();
        assert_eq!(row["design"]["variables_mm"]["length"], expected);
        if index >= 3 {
            assert_eq!(starts[index]["domain_restart_index"], index - 3);
        } else {
            assert!(starts[index].get("domain_restart_index").is_none());
        }
    }
    assert_eq!(starts[2]["evaluations"], 0);
    let identities: std::collections::BTreeSet<_> = rows
        .iter()
        .map(|row| row["design"]["variables_mm"].to_string())
        .collect();
    assert_eq!(identities.len(), rows.len());
    assert_eq!(selected(&report)["design"]["variables_mm"]["length"], 40.0);
}

#[test]
fn domain_restarts_keep_unvisited_seeds_and_complete_incumbents_at_the_shared_budget() {
    for (allowance, retained, starts) in [(2, 0, 1), (5, 1, 2)] {
        let mut request = request(false, allowance, json!([]));
        request.design_adaptive.as_mut().unwrap().domain_restarts = Some(3);
        let report = run(&request);
        let search = &report["design_search"];
        assert_eq!(search["evaluations"], allowance);
        assert_eq!(search["selected_designs"], retained);
        assert_eq!(search["stop_reason"], "evaluation_budget");
        assert_eq!(search["budget_exhausted"], true);
        assert_eq!(search["search_starts"].as_array().unwrap().len(), starts);
        assert_eq!(
            search["domain_restart_points_mm"].as_array().unwrap().len(),
            3
        );
        assert_eq!(
            search["consumer_guidance"]["reserved_evaluations"],
            allowance + 4
        );
        if retained > 0 {
            assert_eq!(selected(&report)["design"]["variables_mm"]["length"], 50.0);
        }
    }
}

#[test]
fn domain_restarts_reach_a_joint_fit_region_and_share_every_named_case_consumer_mode() {
    let mut value = serde_json::to_value(independent_lengths(20, false)).unwrap();
    value["design_fit"]["fit_spec"]["points"][0]["target_mm"] = json!([30, 30, 30]);
    let mut fit_request: Request = serde_json::from_value(value).unwrap();
    assert_eq!(run(&fit_request)["design_search"]["selected_designs"], 0);
    fit_request.design_adaptive.as_mut().unwrap().domain_restarts = Some(1);
    let report = run(&fit_request);
    assert_eq!(
        selected(&report)["design"]["variables_mm"],
        json!({"front":30.0,"back":30.0})
    );
    assert_eq!(report["design_search"]["stop_reason"], "qualified_design_limit");

    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let identity = json!({"source":"test:domain-restart-body"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let cases: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|key| Case {
            key: key.into(),
            scenario: json!({"name":key}),
            comparison: None,
        })
        .collect();
    for mode in [
        "consumer_comparison",
        "consumer_worst_case",
        "consumer_pareto",
        "consumer_matrix_pareto",
    ] {
        let mut value = serde_json::to_value(request(false, 12, json!([]))).unwrap();
        value["design_adaptive"]["objective"] = json!(mode);
        value["design_adaptive"]["domain_restarts"] = json!(1);
        if matches!(mode, "consumer_comparison" | "consumer_pareto") {
            value["design_adaptive"]["case_key"] = json!("first");
        }
        if mode == "consumer_pareto" {
            value["comparison"]["objectives"]
                .as_array_mut()
                .unwrap()
                .push(json!({"metric":"feed_seconds","direction":"minimize"}));
        }
        let request: Request = serde_json::from_value(value).unwrap();
        let mut calls = BTreeMap::new();
        let report = request
            .evaluate_matrix_with_fit(&pattern, &cases, 32, Some(&fit), |case, key, bound| {
                *calls.entry((case, key.to_owned())).or_insert(0) += 1;
                Ok(json!({"case":case,
                    "pattern":sha256_hex(&serde_json::to_vec(bound.raw()).unwrap())}))
            })
            .unwrap();
        let report = serde_json::to_value(report).unwrap();
        let search = &report["manufacturing"]["design_search"];
        assert_eq!(search["selected_designs"], 1, "{mode}");
        assert_eq!(search["domain_restart_points_mm"], json!([{"length":30.0}]));
        assert!(calls.values().all(|&count| count == 1), "{mode}");
        let chosen = selected(&report["manufacturing"]);
        assert_eq!(chosen["design"]["variables_mm"]["length"], 50.0, "{mode}");
        let key =
            serde_json::to_string(&(chosen["design"]["key"].as_str().unwrap(), "light-sheet"))
                .unwrap();
        for case in 0..cases.len() {
            assert_eq!(calls[&(case, key.clone())], 1, "{mode}");
        }
    }

    let mut value = serde_json::to_value(request(false, 12, json!([]))).unwrap();
    value["design_adaptive"]
        .as_object_mut()
        .unwrap()
        .remove("material_key");
    value["design_adaptive"]["material_keys"] = json!(["light-sheet", "heavy-sheet"]);
    value["design_adaptive"]["objective"] = json!("consumer_pareto");
    value["design_adaptive"]["max_designs"] = json!(2);
    value["design_adaptive"]["domain_restarts"] = json!(1);
    let report = run(&serde_json::from_value(value).unwrap());
    let rows = report["design_search"]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|row| {
        row["selected"] == true
            && row["design"]["variables_mm"]["length"] == 50.0
            && row["consumer_materials"][0]["eligible"] == true
    }));
    assert_eq!(report["design_search"]["selected_designs"], 2);
}

#[test]
fn domain_restart_counts_resolution_and_generated_site_errors_refuse_before_consumers() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let identity = json!({"source":"test:domain-restart-admission"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    for case in ["zero", "too-many", "resolution", "generated-underflow"] {
        let mut value = serde_json::to_value(request(false, 1, json!([]))).unwrap();
        value["design_adaptive"]["domain_restarts"] = match case {
            "zero" => json!(0),
            "too-many" => json!(33),
            "resolution" => json!(2),
            _ => json!(3),
        };
        let variable = &mut value["design_adaptive"]["variables"][0];
        if case == "resolution" {
            variable["bounds_mm"] = json!([1.0, 1.0 + f64::EPSILON]);
            variable["initial_mm"] = json!(1.0);
            variable["initial_step_mm"] = json!(f64::EPSILON);
            variable["minimum_step_mm"] = json!(f64::EPSILON);
        } else if case == "generated-underflow" {
            variable["bounds_mm"] = json!([-1.0, 1.0]);
            variable["initial_mm"] = json!(-1.0);
            variable["initial_step_mm"] = json!(1.0);
            variable["minimum_step_mm"] = json!(0.5);
            for site in variable["sites"].as_array_mut().unwrap() {
                site["multiplier"] = json!(f64::from_bits(1));
            }
        }
        let request: Request = serde_json::from_value(value).unwrap();
        let mut calls = 0;
        let error = request
            .evaluate_with_fit(&pattern, &json!({}), Some(&fit), |_, _| {
                calls += 1;
                Ok(json!({}))
            })
            .unwrap_err();
        assert_eq!(error.code, RefusalCode::InvalidDocument, "{case}");
        assert!(
            error.message.contains(match case {
                "resolution" => "256 proposals",
                "generated-underflow" => "underflowed",
                _ => "domain_restarts",
            }),
            "{case}: {error:?}"
        );
        assert_eq!(calls, 0, "{case}");
    }
}
