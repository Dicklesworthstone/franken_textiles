//! Explicit multi-start discovery over real geometry and manufacturing, with synthetic sources.
use std::collections::BTreeMap;

use ftx_core::{RefusalCode, receipt::sha256_hex};
use ftx_export::{
    cutting::CutPlan,
    material_study::{Request, design::fit::Context, matrix::Case},
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
