//! Opposing synthetic scenario readings exercise robust polling; manufacturing is real.
use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_export::{
    cutting::CutPlan,
    material_study::{Request, design::fit::Context, matrix::Case},
};
use ftx_pattern::{Pattern, validate::Measurements};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");

fn request(evaluations: usize, direction: &str) -> Request {
    let mut value: Value =
        serde_json::from_str(include_str!("../../../examples/design-study.json")).unwrap();
    value.as_object_mut().unwrap().remove("designs");
    value["design_adaptive"] = json!({"objective":"consumer_worst_case", "material_key":"light-sheet",
        "variables":[{"key":"length","bounds_mm":[0,60],"initial_mm":40,
            "initial_step_mm":20,"minimum_step_mm":5,
            "sites":[{"piece":"front","axis":"y","at_mm":50,"multiplier":1},
                     {"piece":"back","axis":"y","at_mm":50,"multiplier":1}]}],
        "max_evaluations":evaluations,"max_designs":1});
    value["design_fit"] = json!({"fit_spec":{"schema":"fabric-fit/1","points":[
        {"id":"length","kind":"vertical","instance":"front","from":"a","to":"d",
         "body":"desired_length","target_mm":[0,20,40]}]}});
    value["assembly_mass"] =
        json!({"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[]});
    value["comparison"]["objectives"] = json!([
        {"metric":"simulated_cloth_mass_kg","direction":direction}]);
    value["comparison"]["required_consumers"] = json!([]);
    serde_json::from_value(value).unwrap()
}

fn cases(request: &Request) -> Vec<Case> {
    let mut cases: Vec<_> = ["guard", "guide"]
        .into_iter()
        .map(|key| Case {
            key: key.into(),
            scenario: json!({"scenario_sha256":sha256_hex(key.as_bytes()),
            "body_geometry_sha256":"2".repeat(64),"declared_case":key}),
            comparison: None,
        })
        .collect();
    // Different required consumers deliberately give the cases distinct source protocols.
    let mut policy = serde_json::to_value(request.comparison.as_ref().unwrap()).unwrap();
    policy["required_consumers"] = json!(["cutting"]);
    cases[0].comparison = Some(serde_json::from_value(policy).unwrap());
    cases[1].comparison = None;
    cases
}

fn output(pattern: &Pattern, case: &Case) -> Value {
    let x = pattern.raw()["pieces"][0]["vertices"]["d"][1]
        .as_f64()
        .unwrap()
        - 100.0;
    json!({"schema":"ftx-drape-scenario-result/1",
        "pattern_sha256":sha256_hex(&serde_json::to_vec(pattern.raw()).unwrap()),
        "scenario_sha256":case.scenario["scenario_sha256"],
        "body_geometry_sha256":case.scenario["body_geometry_sha256"],
        "metrics":{"simulation_gating_clear":true},
        "drape":{"settled":true,"gating":[],
            "mass_kg":if case.key == "guard" { x + 1.0 } else { 61.0 - x }}})
}

#[test]
fn worst_case_search_balances_opposing_cases_and_replays_cached_manufacturing() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let identity = json!({"source":"test:worst-case-polling"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    for direction in ["minimize", "maximize"] {
        for reverse in [false, true] {
            let request = request(20, direction);
            let mut cases = cases(&request);
            if reverse {
                cases.reverse();
            }
            let mut calls = std::collections::BTreeMap::new();
            let report = request
                .evaluate_matrix_with_fit(&pattern, &cases, 48, Some(&fit), |index, key, bound| {
                    *calls.entry((index, key.to_owned())).or_insert(0) += 1;
                    Ok(output(bound, &cases[index]))
                })
                .unwrap();
            let report = serde_json::to_value(report).unwrap();
            let search = &report["manufacturing"]["design_search"];
            let guidance = &search["consumer_guidance"];
            let selected = search["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["selected"] == true)
                .unwrap();
            assert_eq!(selected["design"]["variables_mm"]["length"], 30.0);
            assert_eq!(selected["consumer_case_key"], cases[0].key);
            assert_eq!(guidance["best_case_key"], cases[0].key);
            assert_eq!(guidance["best_reading"]["value"], 31.0);
            assert!(report["notes"].as_array().unwrap().iter().any(|note| {
                note.as_str().is_some_and(|note| {
                    note.contains(&format!("limiting case '{}'", cases[0].key))
                        && note.contains("31 kg")
                        && note.contains("not an average")
                })
            }));
            assert_eq!(guidance["case_aggregation"], "worst_case");
            assert!(guidance.get("case_key").is_none());
            assert!(guidance["input_protocol_sha256"].is_null());
            assert_ne!(
                selected["consumer_cases"][0]["comparison"]["input_protocol_sha256"],
                selected["consumer_cases"][1]["comparison"]["input_protocol_sha256"]
            );
            assert!(
                selected["consumer_cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|case| case["comparison"]["eligible"] == true)
            );
            assert!(calls.values().all(|&count| count == 1));
            assert_eq!(report["planned_evaluations"], 48);
            assert_eq!(
                report["evaluations"].as_u64().unwrap() as usize,
                calls.values().sum::<usize>()
            );
            let selected_key = json!(
                serde_json::to_string(&(
                    selected["design"]["key"].as_str().unwrap(),
                    "light-sheet"
                ))
                .unwrap()
            );
            let manufacture = report["manufacturing"]["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == selected_key)
                .unwrap();
            let bound = Pattern::new(manufacture["binding"]["result"]["pattern"].clone());
            assert_eq!(
                manufacture["cutting"]["result"],
                serde_json::to_value(
                    CutPlan::build(&bound, request.cutting.as_ref().unwrap()).unwrap()
                )
                .unwrap()
            );
            for (index, case) in cases.iter().enumerate() {
                let replay = report["cases"][index]["simulations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["key"] == selected_key)
                    .unwrap();
                assert_eq!(replay["evaluation"]["result"], output(&bound, case));
                assert_eq!(
                    calls[&(index, selected_key.as_str().unwrap().to_owned())],
                    1
                );
            }
            assert_eq!(search["complete_search"], false);
            assert_eq!(search["global_optimum_claim"], false);
            assert_eq!(report["comparison"]["physical_validation_claim"], false);
        }
    }
}

#[test]
fn worst_case_search_preserves_incumbent_when_a_limiting_case_fails_or_excludes() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let identity = json!({"source":"test:worst-case-failure"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    for callback_failure in [true, false] {
        let request = request(4, "minimize");
        let mut cases = cases(&request);
        if !callback_failure {
            let mut policy = serde_json::to_value(cases[0].comparison.as_ref().unwrap()).unwrap();
            policy["constraints"] = json!([{"metric":"simulated_cloth_mass_kg","minimum":35}]);
            cases[0].comparison = Some(serde_json::from_value(policy).unwrap());
        }
        let report = request
            .evaluate_matrix_with_fit(&pattern, &cases, 16, Some(&fit), |index, _, bound| {
                if callback_failure
                    && cases[index].key == "guard"
                    && bound.raw()["pieces"][0]["vertices"]["d"][1] == 130.0
                {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "synthetic case work exhausted",
                    ));
                }
                Ok(output(bound, &cases[index]))
            })
            .unwrap();
        let report = serde_json::to_value(report).unwrap();
        let search = &report["manufacturing"]["design_search"];
        let rows = search["rows"].as_array().unwrap();
        let selected = rows.iter().find(|row| row["selected"] == true).unwrap();
        assert_eq!(selected["design"]["variables_mm"]["length"], 40.0);
        assert_eq!(search["consumer_guidance"]["best_reading"]["value"], 41.0);
        let rejected = rows
            .iter()
            .find(|row| row["design"]["variables_mm"]["length"] == 30.0)
            .unwrap();
        assert_eq!(rejected["selected"], false);
        assert_eq!(
            rejected["consumer_cases"][0]["comparison"]["eligible"],
            false
        );
        assert_eq!(
            rejected["consumer_cases"][1]["comparison"]["eligible"],
            true
        );
        assert!(rejected.get("consumer_case_key").is_none());
        if callback_failure {
            assert_eq!(
                rejected["consumer_cases"][0]["refusal"]["message"],
                "synthetic case work exhausted"
            );
        }
        assert_eq!(search["budget_exhausted"], true);
    }
}

#[test]
fn worst_case_search_preflights_modes_policies_and_complete_callback_reservation() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let identity = json!({"source":"test:worst-case-admission"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let request = request(20, "minimize");
    let cases = cases(&request);
    let mut calls = 0;
    assert!(
        request
            .evaluate_with_fit(&pattern, &json!({}), Some(&fit), |_, _| {
                calls += 1;
                Ok(json!({}))
            })
            .unwrap_err()
            .message
            .contains("named scenario matrix")
    );
    assert!(
        request
            .evaluate_with_fit_body(&pattern, Some(&fit))
            .is_err()
    );
    let error = request
        .evaluate_matrix_with_fit(&pattern, &cases, 47, Some(&fit), |_, _, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(error.code, RefusalCode::BudgetExceeded);
    let mut invalids = Vec::new();
    let mut nominated = request.clone();
    nominated.design_adaptive.as_mut().unwrap().case_key = Some("guard".into());
    invalids.push((nominated, cases.clone()));
    let mut missing = request.clone();
    missing.comparison = None;
    invalids.push((missing, cases.clone()));
    for replacement in [
        json!({"basis":"reported_bounds","objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}]}),
        json!({"basis":"nominal","objectives":[{"metric":"assembly_mass_kg","direction":"minimize"}]}),
        json!({"basis":"nominal","objectives":[{"metric":"simulated_cloth_mass_kg","direction":"maximize"}]}),
        json!({"basis":"nominal","objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"},
            {"metric":"assembly_mass_kg","direction":"minimize"}]}),
    ] {
        let mut different = cases.clone();
        different[0].comparison = Some(serde_json::from_value(replacement).unwrap());
        invalids.push((request.clone(), different));
    }
    for (bad, cases) in invalids {
        assert!(
            bad.evaluate_matrix_with_fit(&pattern, &cases, 48, Some(&fit), |_, _, _| {
                calls += 1;
                Ok(json!({}))
            })
            .is_err()
        );
    }
    assert_eq!(calls, 0);
}

#[test]
fn worst_case_without_an_admitted_trial_reports_unknown_instead_of_zero() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let identity = json!({"source":"test:no-admitted-worst-case"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let request = request(4, "minimize");
    let report = request
        .evaluate_matrix_with_fit::<Value>(&pattern, &cases(&request), 16, Some(&fit), |_, _, _| {
            Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "no scenario allowance",
            ))
        })
        .unwrap();
    let report = serde_json::to_value(report).unwrap();
    let search = &report["manufacturing"]["design_search"];
    assert_eq!(search["selected_designs"], 0);
    assert!(search["consumer_guidance"]["best_reading"].is_null());
    assert!(search["consumer_guidance"]["best_trial_key"].is_null());
    assert!(search["consumer_guidance"].get("best_case_key").is_none());
    assert!(report["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .is_some_and(|note| note.starts_with("No worst-case trial was admitted"))
    }));
    assert!(
        search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| { row.get("consumer_case_key").is_none() && row["selected"] == false })
    );
}
