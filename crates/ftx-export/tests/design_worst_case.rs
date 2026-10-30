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

fn bounded_request(evaluations: usize, direction: &str, density_bounds: [f64; 2]) -> Request {
    let mut value = serde_json::to_value(request(evaluations, direction)).unwrap();
    value["comparison"]["basis"] = json!("reported_bounds");
    value["comparison"]["objectives"][0]["metric"] = json!("assembly_mass_kg");
    value["comparison"]["required_consumers"] = json!(["drape"]);
    for (index, scale) in [(0, 1.0), (1, 2.0)] {
        value["variants"][index]["changes"]["default"]["record"]["properties"]["areal_density"]["bounds"] =
            json!(density_bounds.map(|bound| bound * scale));
    }
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
            assert!(guidance.get("best_reported_bound_case_keys").is_none());
            assert!(selected.get("consumer_worst_case").is_none());
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

fn range_request(bounds: Option<[f64; 2]>, direction: &str) -> Request {
    let mut value = serde_json::to_value(request(12, direction)).unwrap();
    value["comparison"]["basis"] = json!("reported_bounds");
    value["comparison"]["objectives"][0]["metric"] = json!("assembly_mass_kg");
    value["assembly_mass"]["seam_allowance_mm"] = json!(0.0);
    value["cutting"]["seam_allowance_mm"] = json!(0.0);
    if let Some(bounds) = bounds {
        for (index, factor) in [(0, 1.0), (1, 2.0)] {
            value["variants"][index]["changes"]["default"]["record"]["properties"]["areal_density"]
                ["bounds"] = json!(bounds.map(|bound| bound * factor));
        }
    }
    serde_json::from_value(value).unwrap()
}

#[test]
fn worst_case_ranges_guide_real_assembly_mass_and_preserve_exact_cached_outputs() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let identity = json!({"source":"test:worst-case-source-ranges"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    for (bounds, direction, coordinate) in [
        ([0.179, 0.181], "minimize", 20.0),
        ([0.179, 0.181], "maximize", 60.0),
        // All in-band designs overlap: a better nominal value is not robust improvement.
        ([0.10, 0.26], "minimize", 40.0),
        ([0.10, 0.26], "maximize", 40.0),
    ] {
        let request = range_request(Some(bounds), direction);
        let cases = cases(&request);
        let mut calls = std::collections::BTreeMap::new();
        let report = request
            .evaluate_matrix_with_fit(&pattern, &cases, 32, Some(&fit), |index, key, bound| {
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
        assert_eq!(selected["design"]["variables_mm"]["length"], coordinate);
        assert_eq!(guidance["basis"], "reported_bounds");
        assert_eq!(guidance["best_case_key"], "guard");
        assert_eq!(
            guidance["best_reported_bound_case_keys"],
            json!(["guard", "guard"])
        );
        assert_eq!(
            guidance["best_reading"],
            selected["consumer_worst_case"]["reading"]
        );
        assert!(guidance["input_protocol_sha256"].is_null());
        let area = 2.0 * 20.0 * (100.0 + coordinate) * 1e-6;
        for endpoint in 0..2 {
            assert!(
                (guidance["best_reading"]["reported_bounds"][endpoint]
                    .as_f64()
                    .unwrap()
                    - area * bounds[endpoint])
                    .abs()
                    < 1e-15
            );
        }
        assert!(calls.values().all(|&count| count == 1));
        assert_eq!(report["planned_evaluations"], 32);
        assert_eq!(
            report["evaluations"].as_u64().unwrap() as usize,
            calls.values().sum::<usize>()
        );
        let key =
            serde_json::to_string(&(selected["design"]["key"].as_str().unwrap(), "light-sheet"))
                .unwrap();
        let manufacturing = report["manufacturing"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == key)
            .unwrap();
        assert_eq!(
            guidance["best_reading"]["reported_bounds"],
            manufacturing["assembly_mass"]["result"]["assembly_mass"]["reported_bounds_kg"]
        );
        let bound = Pattern::new(manufacturing["binding"]["result"]["pattern"].clone());
        for (index, case) in cases.iter().enumerate() {
            let replay = report["cases"][index]["simulations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == key)
                .unwrap();
            assert_eq!(replay["evaluation"]["result"], output(&bound, case));
            assert_eq!(calls[&(index, key.clone())], 1);
            assert_eq!(
                selected["consumer_cases"][index]["comparison"]["readings"]["assembly_mass_kg"],
                guidance["best_reading"]
            );
        }
        assert!(report["notes"].as_array().unwrap().iter().any(|note| {
            note.as_str().is_some_and(|note| {
                note.contains("reported worst-case range")
                    && note.contains("not statistical confidence")
            })
        }));
        assert_eq!(search["global_optimum_claim"], false);
    }
}

#[test]
fn worst_case_ranges_retain_missing_bounds_and_full_range_constraint_failures() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let identity = json!({"source":"test:worst-case-range-refusals"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    for missing in [false, true] {
        let request = range_request((!missing).then_some([0.17, 0.19]), "minimize");
        let mut cases = cases(&request);
        if !missing {
            let mut policy = serde_json::to_value(cases[0].comparison.as_ref().unwrap()).unwrap();
            policy["constraints"] = json!([{"metric":"assembly_mass_kg","minimum":0.00084}]);
            cases[0].comparison = Some(serde_json::from_value(policy).unwrap());
        }
        let report = request
            .evaluate_matrix_with_fit(&pattern, &cases, 32, Some(&fit), |index, _, bound| {
                Ok(output(bound, &cases[index]))
            })
            .unwrap();
        let report = serde_json::to_value(report).unwrap();
        let search = &report["manufacturing"]["design_search"];
        let rejected = search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["design"]["variables_mm"]["length"] == 20.0)
            .unwrap();
        let reading = &rejected["consumer_cases"][0]["comparison"]["readings"]["assembly_mass_kg"];
        assert!(reading["value"].as_f64().unwrap() > 0.00084);
        assert_eq!(rejected["selected"], false);
        assert_eq!(
            rejected["consumer_cases"][0]["comparison"]["eligible"],
            false
        );
        assert!(rejected.get("consumer_worst_case").is_none());
        if missing {
            assert!(reading.get("reported_bounds").is_none());
            assert_eq!(search["selected_designs"], 0);
            assert!(search["consumer_guidance"]["best_reading"].is_null());
        } else {
            assert!(reading["reported_bounds"][0].as_f64().unwrap() < 0.00084);
            assert_eq!(
                rejected["consumer_cases"][1]["comparison"]["eligible"],
                true
            );
            assert_eq!(search["rows"][0]["selected"], true);
        }
    }
    let request = range_request(Some([0.179, 0.181]), "minimize");
    let mut cases = cases(&request);
    cases[0].comparison.as_mut().unwrap().basis =
        ftx_export::material_study::comparison::Basis::Nominal;
    let mut calls = 0;
    assert!(
        request
            .evaluate_matrix_with_fit(&pattern, &cases, 32, Some(&fit), |_, _, _| {
                calls += 1;
                Ok(json!({}))
            })
            .unwrap_err()
            .message
            .contains("comparison basis")
    );
    assert_eq!(calls, 0);
}

#[test]
fn bounded_worst_case_search_uses_real_mass_ranges_and_replays_cached_consumers() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let fit = Context {
        body: &body,
        identity: &json!({"source":"test:bounded-worst-case"}),
    };
    for (direction, expected_length) in [("minimize", 20.0), ("maximize", 60.0)] {
        for reverse in [false, true] {
            let request = bounded_request(20, direction, [0.179, 0.181]);
            let mut cases = cases(&request);
            if reverse {
                cases.reverse();
            }
            let mut calls = std::collections::BTreeMap::new();
            let report = serde_json::to_value(
                request
                    .evaluate_matrix_with_fit(
                        &pattern,
                        &cases,
                        48,
                        Some(&fit),
                        |index, key, bound| {
                            *calls.entry((index, key.to_owned())).or_insert(0) += 1;
                            Ok(output(bound, &cases[index]))
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
            let search = &report["manufacturing"]["design_search"];
            let guidance = &search["consumer_guidance"];
            let selected = search["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["selected"] == true)
                .unwrap();
            assert_eq!(
                selected["design"]["variables_mm"]["length"],
                expected_length
            );
            assert_eq!(guidance["basis"], "reported_bounds");
            let witnesses = json!([cases[0].key, cases[0].key]);
            assert_eq!(
                selected["consumer_worst_case"]["reported_bound_case_keys"],
                witnesses
            );
            assert_eq!(guidance["best_reported_bound_case_keys"], witnesses);
            assert_eq!(guidance["best_case_key"], cases[0].key);
            let reading = &guidance["best_reading"];
            assert_eq!(*reading, selected["consumer_worst_case"]["reading"]);
            assert_eq!(
                selected["consumer_comparison"],
                selected["consumer_cases"][0]["comparison"]
            );
            for (endpoint, density) in [0.179, 0.181].into_iter().enumerate() {
                // Two 40 mm-wide cutter rectangles, with 20 mm total length allowance.
                let expected = 2.0 * 40.0 * (120.0 + expected_length) * 1e-6 * density;
                assert!(
                    (reading["reported_bounds"][endpoint].as_f64().unwrap() - expected).abs()
                        < 1e-12
                );
            }
            assert!(report["notes"].as_array().unwrap().iter().any(|note| {
                note.as_str().is_some_and(|note| {
                    note.contains("reported worst-case range")
                        && note.contains("lower endpoint from")
                        && note.contains("not a single case's interval or an average")
                })
            }));
            assert_ne!(
                selected["consumer_cases"][0]["comparison"]["input_protocol_sha256"],
                selected["consumer_cases"][1]["comparison"]["input_protocol_sha256"]
            );
            assert!(calls.values().all(|&count| count == 1));
            assert_eq!(
                report["evaluations"].as_u64().unwrap() as usize,
                calls.values().sum::<usize>()
            );
            let selected_key = serde_json::to_string(&(
                selected["design"]["key"].as_str().unwrap(),
                "light-sheet",
            ))
            .unwrap();
            let manufacture = report["manufacturing"]["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == selected_key)
                .unwrap();
            assert_eq!(
                reading["reported_bounds"],
                manufacture["assembly_mass"]["result"]["assembly_mass"]["reported_bounds_kg"]
            );
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
                assert_eq!(calls[&(index, selected_key.clone())], 1);
            }
            assert_eq!(search["complete_search"], false);
            assert_eq!(search["global_optimum_claim"], false);
        }
    }
}

#[test]
fn bounded_worst_case_preserves_guards_failed_cases_and_missing_source_ranges() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let fit = Context {
        body: &body,
        identity: &json!({"source":"test:bounded-case-admission"}),
    };
    for callback_failure in [false, true] {
        let request = bounded_request(4, "minimize", [0.179, 0.181]);
        let mut cases = cases(&request);
        let guard = cases[0].comparison.as_mut().unwrap();
        guard
            .required_consumers
            .push(ftx_export::material_study::comparison::Consumer::Drape);
        if !callback_failure {
            guard.constraints.push(
                serde_json::from_value(json!({
                    "metric":"simulated_cloth_mass_kg","basis":"nominal","minimum":35
                }))
                .unwrap(),
            );
        }
        let report = serde_json::to_value(
            request
                .evaluate_matrix_with_fit(&pattern, &cases, 16, Some(&fit), |index, _, bound| {
                    if callback_failure
                        && index == 0
                        && bound.raw()["pieces"][0]["vertices"]["d"][1]
                            .as_f64()
                            .unwrap()
                            < 140.0
                    {
                        return Err(Refusal::new(
                            RefusalCode::BudgetExceeded,
                            "bounded guard unavailable",
                        ));
                    }
                    Ok(output(bound, &cases[index]))
                })
                .unwrap(),
        )
        .unwrap();
        let search = &report["manufacturing"]["design_search"];
        let selected = search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["selected"] == true)
            .unwrap();
        assert_eq!(selected["design"]["variables_mm"]["length"], 40.0);
        let refused = search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["design"]["variables_mm"]["length"] == 20.0)
            .unwrap();
        assert_eq!(
            refused["consumer_cases"][0]["comparison"]["eligible"],
            false
        );
        assert_eq!(refused["consumer_cases"][1]["comparison"]["eligible"], true);
        assert!(refused.get("consumer_worst_case").is_none());
        assert!(refused.get("consumer_case_key").is_none());
        assert_eq!(search["budget_exhausted"], true);
    }

    let mut missing = bounded_request(4, "minimize", [0.179, 0.181]);
    missing.variants[0]
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("areal_density")
        .unwrap()
        .bounds = None;
    let cases = cases(&missing);
    let report = serde_json::to_value(
        missing
            .evaluate_matrix_with_fit(&pattern, &cases, 16, Some(&fit), |index, _, bound| {
                Ok(output(bound, &cases[index]))
            })
            .unwrap(),
    )
    .unwrap();
    let search = &report["manufacturing"]["design_search"];
    assert_eq!(search["selected_designs"], 0);
    assert!(search["consumer_guidance"]["best_reading"].is_null());
    assert!(
        search["consumer_guidance"]
            .get("best_reported_bound_case_keys")
            .is_none()
    );
    for row in search["rows"].as_array().unwrap() {
        if let Some(candidate) = row.get("consumer_comparison") {
            assert_eq!(candidate["eligible"], false);
            assert!(candidate["readings"]["assembly_mass_kg"]["value"].is_number());
            assert!(
                candidate["readings"]["assembly_mass_kg"]
                    .get("reported_bounds")
                    .is_none()
            );
        }
    }
    let valid = bounded_request(4, "minimize", [0.179, 0.181]);
    let mut mixed = cases.clone();
    mixed[0].comparison.as_mut().unwrap().basis =
        ftx_export::material_study::comparison::Basis::Nominal;
    let mut calls = 0;
    assert!(
        valid
            .evaluate_matrix_with_fit(&pattern, &mixed, 16, Some(&fit), |_, _, _| {
                calls += 1;
                Ok(json!({}))
            })
            .unwrap_err()
            .message
            .contains("basis")
    );
    assert_eq!(calls, 0);
}

#[test]
fn bounded_worst_case_cache_replaces_only_a_strictly_dominated_interval() {
    let mut value = serde_json::to_value(bounded_request(4, "minimize", [0.16, 0.192])).unwrap();
    value["design_adaptive"]["max_designs"] = json!(2);
    let mut length = value["design_adaptive"]["variables"][0].clone();
    length["initial_step_mm"] = json!(40);
    length["minimum_step_mm"] = json!(40);
    value["design_adaptive"]["variables"] = json!([
        {"key":"width","bounds_mm":[-5,5],"initial_mm":0,
         "initial_step_mm":5,"minimum_step_mm":5,
         "sites":[{"piece":"front","axis":"x","at_mm":10,"multiplier":1},
                  {"piece":"back","axis":"x","at_mm":10,"multiplier":1}]}, length]);
    value["design_fit"]["fit_spec"]["points"][0]["target_mm"] = json!([0, 20, 60]);
    let request: Request = serde_json::from_value(value).unwrap();
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let cases = cases(&request);
    let body = Measurements(json!({"desired_length":100}));
    let fit = Context {
        body: &body,
        identity: &json!({"source":"test:worst-case-range-retention"}),
    };
    let report = serde_json::to_value(
        request
            .evaluate_matrix_with_fit(&pattern, &cases, 20, Some(&fit), |index, _, bound| {
                Ok(output(bound, &cases[index]))
            })
            .unwrap(),
    )
    .unwrap();
    let search = &report["manufacturing"]["design_search"];
    let rows = search["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows.iter()
            .map(|row| row["selected"].as_bool().unwrap())
            .collect::<Vec<_>>(),
        vec![false, true, false, true]
    );
    assert_eq!(
        rows[1]["design"]["variables_mm"],
        json!({"width":-5.0,"length":40.0})
    );
    assert_eq!(
        rows[3]["design"]["variables_mm"],
        json!({"width":0.0,"length":0.0})
    );
    let bounds = |index: usize| -> [f64; 2] {
        serde_json::from_value(
            rows[index]["consumer_worst_case"]["reading"]["reported_bounds"].clone(),
        )
        .unwrap()
    };
    // The late shorter geometry strictly dominates the first interval, but overlaps the
    // narrower piece's interval. The cache must keep that incomparable earlier candidate.
    assert!(bounds(3)[1] < bounds(0)[0]);
    assert!(bounds(3)[1] > bounds(1)[0] && bounds(3)[0] < bounds(1)[1]);
    assert_eq!(
        search["consumer_guidance"]["best_trial_key"],
        rows[3]["design"]["key"]
    );
    assert_eq!(search["selected_designs"], 2);
}

#[test]
fn bounded_worst_case_polling_does_not_order_exactly_touching_ranges() {
    let mut source: Value = serde_json::from_str(PATTERN).unwrap();
    source["meta"]["sa_mm"] = json!(0);
    for piece in source["pieces"].as_array_mut().unwrap() {
        piece["vertices"] = json!({"a":[0,0],"b":[125,0],"c":[125,125],"d":[0,125]});
    }
    let pattern = Pattern::new(source);
    let body = Measurements(json!({"desired_length":125}));
    let fit = Context {
        body: &body,
        identity: &json!({"source":"test:exact-worst-case-touching"}),
    };
    for (direction, initial) in [("minimize", 125), ("maximize", 0)] {
        let mut value = serde_json::to_value(bounded_request(2, direction, [0.125, 0.25])).unwrap();
        value["cutting"] = Value::Null;
        value["sewing"] = Value::Null;
        value["variants"].as_array_mut().unwrap().truncate(1);
        value["design_fit"]["fit_spec"]["points"][0]["target_mm"] = json!([0, 60, 125]);
        let variable = &mut value["design_adaptive"]["variables"][0];
        variable["bounds_mm"] = json!([0, 125]);
        variable["initial_mm"] = json!(initial);
        variable["initial_step_mm"] = json!(125);
        variable["minimum_step_mm"] = json!(125);
        let request: Request = serde_json::from_value(value).unwrap();
        let mut cases = cases(&request);
        cases[0].comparison = None;
        let report = serde_json::to_value(
            request
                .evaluate_matrix_with_fit(&pattern, &cases, 8, Some(&fit), |index, _, bound| {
                    let mut result = output(bound, &cases[index]);
                    result["drape"]["mass_kg"] = json!(1.0);
                    Ok(result)
                })
                .unwrap(),
        )
        .unwrap();
        let search = &report["manufacturing"]["design_search"];
        let rows = search["rows"].as_array().unwrap();
        let initial_endpoint = usize::from(initial == 0);
        let initial_bounds = &rows[0]["consumer_worst_case"]["reading"]["reported_bounds"];
        let later_bounds = &rows[1]["consumer_worst_case"]["reading"]["reported_bounds"];
        // Binary-exact area and density values make the touching boundary exact, with no tolerance.
        assert_eq!(initial_bounds[initial_endpoint], json!(0.0078125));
        assert_eq!(later_bounds[1 - initial_endpoint], json!(0.0078125));
        assert_eq!(rows[0]["selected"], true);
        assert_eq!(rows[1]["selected"], false);
        assert_eq!(rows[1]["consumer_comparison"]["eligible"], true);
        assert_eq!(
            search["consumer_guidance"]["best_trial_key"],
            rows[0]["design"]["key"]
        );
    }
}
