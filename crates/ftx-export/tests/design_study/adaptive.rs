//! Real geometry, fit and manufacturing consumers under bounded adaptive candidate discovery.
use super::*;
use std::collections::BTreeSet;

use ftx_export::material_study::design::fit::Context;
use ftx_pattern::validate::Measurements;

fn adaptive_request(evaluations: usize, designs: usize) -> Request {
    let mut value = serde_json::to_value(grid_request(1)).unwrap();
    value.as_object_mut().unwrap().remove("design_grid");
    value["design_adaptive"] = json!({
        "objective":"fit_band_violation_mm",
        "variables":[{"key":"length","bounds_mm":[0,60],"initial_mm":0,
            "initial_step_mm":20,"minimum_step_mm":0.0001,
            "sites":[{"piece":"front","axis":"y","at_mm":50,"multiplier":1},
                     {"piece":"back","axis":"y","at_mm":50,"multiplier":1}]}],
        "max_evaluations":evaluations,"max_designs":designs});
    value["design_fit"]["fit_spec"]["points"] = json!([
        {"id":"length","kind":"vertical","instance":"front","from":"a","to":"d",
         "body":"desired_length","target_mm":[-0.0001,0,0.0001]}]);
    serde_json::from_value(value).unwrap()
}

fn run(request: &Request, body: &Measurements) -> Value {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    serde_json::to_value(
        request
            .evaluate_with_fit_body(
                &pattern,
                Some(&Context {
                    body,
                    identity: &json!({"source":"synthetic-adaptive-fit","values":body.0}),
                }),
            )
            .unwrap(),
    )
    .unwrap()
}

fn consumer_request(evaluations: usize) -> Request {
    let mut value = serde_json::to_value(adaptive_request(evaluations, 1)).unwrap();
    value["design_adaptive"]["objective"] = json!("consumer_comparison");
    value["design_adaptive"]["material_key"] = json!("light-sheet");
    value["design_adaptive"]["variables"][0]["initial_mm"] = json!(40);
    value["design_adaptive"]["variables"][0]["minimum_step_mm"] = json!(5);
    value["design_fit"]["fit_spec"]["points"][0]["target_mm"] = json!([0, 20, 40]);
    serde_json::from_value(value).unwrap()
}

fn bounded_consumer_request(evaluations: usize, density_bounds: [f64; 2]) -> Request {
    let mut value = serde_json::to_value(consumer_request(evaluations)).unwrap();
    value["comparison"]["basis"] = json!("reported_bounds");
    for (index, scale) in [(0, 1.0), (1, 2.0)] {
        value["variants"][index]["changes"]["default"]["record"]["properties"]["areal_density"]["bounds"] =
            json!(density_bounds.map(|bound| bound * scale));
    }
    serde_json::from_value(value).unwrap()
}

fn matrix_request(evaluations: usize) -> Request {
    let mut request = consumer_request(evaluations);
    request.design_adaptive.as_mut().unwrap().case_key = Some("guide".into());
    request
}

fn consumer_cases() -> Vec<Case> {
    ["guard", "guide"]
        .into_iter()
        .map(|key| Case {
            key: key.into(),
            scenario: json!({
                "scenario_sha256":sha256_hex(key.as_bytes()),
                "body_geometry_sha256":"2".repeat(64),
                "declared_case":key}),
            comparison: (key == "guard").then(|| {
                serde_json::from_value(json!({"basis":"nominal",
                    "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
                    "constraints":[{"metric":"simulated_cloth_mass_kg","maximum":40}]}))
                .unwrap()
            }),
        })
        .collect()
}

fn case_output(pattern: &Pattern, case: &Case) -> Value {
    let length = pattern.raw()["pieces"][0]["vertices"]["d"][1]
        .as_f64()
        .unwrap();
    // Deliberately synthetic callback response, separate from real assembly/cutting consumers.
    // Its independent guard excludes lengths below 130 mm while flat fit admits 120..160 mm.
    json!({"schema":"ftx-drape-scenario-result/1",
        "pattern_sha256":sha256_hex(&serde_json::to_vec(pattern.raw()).unwrap()),
        "scenario_sha256":case.scenario["scenario_sha256"],
        "body_geometry_sha256":case.scenario["body_geometry_sha256"],
        "metrics":{"simulation_gating_clear":true},
        "drape":{"settled":true,"gating":[],"mass_kg":170.0-length}})
}

#[test]
fn named_consumer_search_obeys_every_case_and_reuses_exact_selected_probes() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let identity = json!({"source":"test:adaptive-matrix"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let request = matrix_request(20);
    let cases = consumer_cases();
    let mut calls = std::collections::BTreeMap::new();
    let report = request
        .evaluate_matrix_with_fit(&pattern, &cases, 48, Some(&fit), |index, key, bound| {
            *calls.entry((index, key.to_owned())).or_insert(0) += 1;
            Ok(case_output(bound, &cases[index]))
        })
        .unwrap();
    let report = serde_json::to_value(report).unwrap();
    let search = &report["manufacturing"]["design_search"];
    let guidance = &search["consumer_guidance"];
    assert_eq!(guidance["case_key"], "guide");
    assert_eq!(guidance["case_order"], json!(["guard", "guide"]));
    assert_eq!(guidance["objective"]["metric"], "assembly_mass_kg");
    assert_eq!(guidance["reserved_evaluations"], 48);
    assert_eq!(report["planned_evaluations"], 48);
    assert_eq!(
        report["evaluations"].as_u64().unwrap() as usize,
        calls.values().sum::<usize>()
    );
    assert!(calls.values().all(|&count| count == 1));
    let trials = search["rows"].as_array().unwrap();
    let selected = trials
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    assert_eq!(selected["design"]["variables_mm"]["length"], 30.0);
    let rejected = trials
        .iter()
        .find(|trial| trial["design"]["variables_mm"]["length"] == 20.0)
        .unwrap();
    assert_eq!(rejected["consumer_comparison"]["eligible"], true);
    assert_eq!(
        rejected["consumer_cases"][0]["comparison"]["eligible"],
        false
    );
    assert_eq!(
        rejected["consumer_cases"][1]["comparison"]["eligible"],
        true
    );
    assert_eq!(rejected["selected"], false);
    assert!(
        rejected["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("guard")
    );
    let selected_key = key(selected["design"]["key"].as_str().unwrap(), "light-sheet");
    for (index, case) in cases.iter().enumerate() {
        let output = report["cases"][index]["simulations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == selected_key)
            .unwrap();
        let manufacture = report["manufacturing"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == selected_key)
            .unwrap();
        let bound = Pattern::new(manufacture["binding"]["result"]["pattern"].clone());
        assert_eq!(output["evaluation"]["result"], case_output(&bound, case));
        assert_eq!(
            calls[&(index, selected_key.as_str().unwrap().to_owned())],
            1
        );
        let decision = report["cases"][index]["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["key"] == selected_key)
            .unwrap();
        for field in [
            "readings",
            "eligible",
            "exclusions",
            "input_protocol_sha256",
        ] {
            assert_eq!(
                selected["consumer_cases"][index]["comparison"][field],
                decision[field]
            );
        }
        assert_eq!(
            manufacture["cutting"]["result"],
            serde_json::to_value(
                CutPlan::build(&bound, request.cutting.as_ref().unwrap()).unwrap()
            )
            .unwrap()
        );
    }
    assert_eq!(search["global_optimum_claim"], false);
    assert_eq!(report["comparison"]["physical_validation_claim"], false);

    // Case identities, rather than vector positions or an unused fallback, select the objective.
    let mut other = request.clone();
    other.design_adaptive.as_mut().unwrap().case_key = Some("guard".into());
    let other = serde_json::to_value(
        other
            .evaluate_matrix_with_fit(&pattern, &cases, 48, Some(&fit), |index, _, bound| {
                Ok(case_output(bound, &cases[index]))
            })
            .unwrap(),
    )
    .unwrap();
    let selected = other["manufacturing"]["design_search"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    assert_eq!(selected["design"]["variables_mm"]["length"], 60.0);
    assert_eq!(
        other["manufacturing"]["design_search"]["consumer_guidance"]["objective"]["metric"],
        "simulated_cloth_mass_kg"
    );
}

#[test]
fn named_consumer_search_retains_prior_incumbents_after_case_refusal_and_work_limit() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let identity = json!({"source":"test:adaptive-matrix-partial"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let request = matrix_request(3);
    let cases = consumer_cases();
    let mut calls = std::collections::BTreeMap::new();
    let report = serde_json::to_value(
        request
            .evaluate_matrix_with_fit(&pattern, &cases, 14, Some(&fit), |index, key, bound| {
                *calls.entry((index, key.to_owned())).or_insert(0) += 1;
                if index == 0 && bound.raw()["pieces"][0]["vertices"]["d"][1] == 120.0 {
                    return Err(ftx_core::Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "synthetic guard callback exhausted its numerical work",
                    ));
                }
                Ok(case_output(bound, &cases[index]))
            })
            .unwrap(),
    )
    .unwrap();
    let search = &report["manufacturing"]["design_search"];
    assert_eq!(search["budget_exhausted"], true);
    assert_eq!(search["stop_reason"], "evaluation_budget");
    let rows = search["rows"].as_array().unwrap();
    let selected = rows.iter().find(|trial| trial["selected"] == true).unwrap();
    assert_eq!(selected["design"]["variables_mm"]["length"], 40.0);
    let failed = rows
        .iter()
        .find(|trial| trial["design"]["variables_mm"]["length"] == 20.0)
        .unwrap();
    assert!(
        failed["consumer_cases"][0]["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("numerical work")
    );
    assert_eq!(failed["consumer_cases"][1]["comparison"]["eligible"], true);
    assert_eq!(failed["consumer_comparison"]["eligible"], true);
    assert_eq!(failed["selected"], false);
    assert_eq!(
        report["evaluations"].as_u64().unwrap() as usize,
        calls.values().sum::<usize>()
    );
    assert!(calls.values().all(|&count| count == 1));
    assert!(
        !report["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn named_consumer_search_preflights_case_selection_all_policies_and_every_callback_slot() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let identity = json!({"source":"test:adaptive-matrix-admission"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let request = matrix_request(20);
    let cases = consumer_cases();
    let mut calls = 0;
    let error = request
        .evaluate_matrix_with_fit(&pattern, &cases, 47, Some(&fit), |_, _, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(error.code, RefusalCode::BudgetExceeded);
    assert_eq!(calls, 0);
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
    assert_eq!(calls, 0);
    let mut invalids = Vec::new();
    for key in [None, Some("absent"), Some("")] {
        let mut bad = request.clone();
        bad.design_adaptive.as_mut().unwrap().case_key = key.map(str::to_owned);
        invalids.push((bad, cases.clone()));
    }
    let mut missing = request.clone();
    missing.comparison = None;
    invalids.push((missing, cases.clone()));
    let mut unsupported_bounds = cases.clone();
    let mut policy = request.comparison.clone().unwrap();
    policy.basis = ftx_export::material_study::comparison::Basis::ReportedBounds;
    policy.objectives[0].metric =
        ftx_export::material_study::comparison::Metric::SimulatedClothMassKg;
    unsupported_bounds[1].comparison = Some(policy);
    invalids.push((request.clone(), unsupported_bounds));
    let mut multiple = cases.clone();
    let mut policy = request.comparison.clone().unwrap();
    policy.objectives.push(
        serde_json::from_value(json!({
        "metric":"simulated_cloth_mass_kg","direction":"minimize"}))
        .unwrap(),
    );
    multiple[1].comparison = Some(policy);
    invalids.push((request.clone(), multiple));
    for (bad, cases) in invalids {
        assert!(
            bad.evaluate_matrix_with_fit(&pattern, &cases, 64, Some(&fit), |_, _, _| {
                calls += 1;
                Ok(json!({}))
            })
            .is_err()
        );
        assert_eq!(calls, 0);
    }
    let mut explicit = request.clone();
    explicit.comparison = None;
    let mut explicit_cases = cases.clone();
    explicit_cases[1].comparison = request.comparison.clone();
    let output = explicit
        .evaluate_matrix_with_fit(
            &pattern,
            &explicit_cases,
            48,
            Some(&fit),
            |index, _, bound| Ok(case_output(bound, &explicit_cases[index])),
        )
        .unwrap();
    assert_eq!(
        output.cases[1]
            .comparison
            .as_ref()
            .unwrap()
            .policy
            .objectives
            .len(),
        1
    );
    assert!(output.manufacturing.comparison.is_none());
}

#[test]
fn consumer_polling_replaces_flat_fit_incumbents_with_lighter_real_assemblies() {
    let body = Measurements(json!({"desired_length":120}));
    let request = consumer_request(32);
    let report = run(&request, &body);
    assert_eq!(report, run(&request, &body));
    let search = &report["design_search"];
    let guidance = &search["consumer_guidance"];
    assert_eq!(search["selected_designs"], 1);
    assert_eq!(search["stop_reason"], "minimum_steps");
    assert!(search["qualified_designs"].as_u64().unwrap() > 1);
    assert_eq!(guidance["material_key"], "light-sheet");
    assert_eq!(guidance["reserved_evaluations"], 36);
    assert_eq!(guidance["objective"]["metric"], "assembly_mass_kg");
    assert_eq!(guidance["best_reading"]["unit"], "kg");
    let trials = search["rows"].as_array().unwrap();
    assert_eq!(trials[0]["max_fit_band_violation_mm"], 0.0);
    assert_eq!(trials[0]["consumer_comparison"]["eligible"], true);
    assert_eq!(trials[0]["selected"], false);
    let selected = trials
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    assert_eq!(selected["design"]["variables_mm"]["length"], 20.0);
    assert_eq!(guidance["best_trial_key"], selected["design"]["key"]);
    let design = selected["design"]["key"].as_str().unwrap();
    // Two 20 x 120 mm rectangles plus the source's 10 mm allowance on each side.
    // Areal densities are explicitly synthetic; this is an independent dimensional oracle.
    let area = 2.0 * 40.0 * 140.0 * 1e-6;
    for (material, density) in [("light-sheet", 0.18), ("heavy-sheet", 0.36)] {
        let candidate = row(&report, design, material);
        assert!(
            (candidate["metrics"]["assembly_mass_kg"].as_f64().unwrap() - area * density).abs()
                < 1e-12
        );
        let altered = Pattern::from_json(
            candidate["design"]["altered_pattern_json"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let frozen: Changes = serde_json::from_value(candidate["frozen_changes"].clone()).unwrap();
        assert_eq!(
            frozen.apply(&altered).unwrap()["pattern"],
            candidate["binding"]["result"]["pattern"]
        );
        let bound = Pattern::new(candidate["binding"]["result"]["pattern"].clone());
        assert_eq!(
            candidate["cutting"]["result"],
            serde_json::to_value(
                CutPlan::build(&bound, request.cutting.as_ref().unwrap()).unwrap()
            )
            .unwrap()
        );
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
    assert_eq!(
        report["comparison"]["frontier"],
        json!([key(design, "light-sheet")])
    );
    let mut fit_only = request;
    let adaptive = fit_only.design_adaptive.as_mut().unwrap();
    adaptive.objective =
        ftx_export::material_study::design::adaptive::Objective::FitBandViolationMm;
    adaptive.material_key = None;
    let original = run(&fit_only, &body);
    assert_eq!(original["design_search"]["evaluations"], 1);
    assert_eq!(
        original["design_search"]["rows"][0]["design"]["variables_mm"]["length"],
        40.0
    );
    assert!(original["design_search"].get("consumer_guidance").is_none());
}

#[test]
fn consumer_constraints_admit_only_real_feasible_metrics_and_unknown_mass_is_not_zero() {
    let body = Measurements(json!({"desired_length":120}));
    let mut request = consumer_request(32);
    request.comparison.as_mut().unwrap().constraints.push(
        serde_json::from_value(json!({
            "metric":"assembly_mass_kg", "minimum":0.0022
        }))
        .unwrap(),
    );
    let report = run(&request, &body);
    let trials = report["design_search"]["rows"].as_array().unwrap();
    let too_light = trials
        .iter()
        .find(|trial| trial["design"]["variables_mm"]["length"] == 20.0)
        .unwrap();
    assert_eq!(too_light["consumer_comparison"]["eligible"], false);
    assert!(too_light["consumer_comparison"]["readings"]["assembly_mass_kg"]["value"].is_number());
    assert_eq!(too_light["selected"], false);
    let selected = trials
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    assert!(
        selected["consumer_comparison"]["readings"]["assembly_mass_kg"]["value"]
            .as_f64()
            .unwrap()
            >= 0.0022
    );

    let mut missing = serde_json::to_value(consumer_request(8)).unwrap();
    missing["variants"][0]["changes"]["default"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("areal_density");
    let report = run(&serde_json::from_value(missing).unwrap(), &body);
    let search = &report["design_search"];
    assert_eq!(search["selected_designs"], 0);
    assert!(search["qualified_designs"].as_u64().unwrap() > 0);
    assert!(search["consumer_guidance"]["best_reading"].is_null());
    for trial in search["rows"].as_array().unwrap() {
        assert_eq!(trial["selected"], false);
        if let Some(comparison) = trial.get("consumer_comparison") {
            assert_eq!(comparison["eligible"], false);
            assert!(comparison["readings"].get("assembly_mass_kg").is_none());
        }
    }
}

#[test]
fn bounded_consumer_guidance_uses_complete_mass_ranges_and_preserves_missing_endpoints() {
    use ftx_export::material_study::comparison::Basis;
    let body = Measurements(json!({"desired_length":120}));
    let mut request = bounded_consumer_request(24, [0.17, 0.19]);
    request.comparison.as_mut().unwrap().constraints.push(
        serde_json::from_value(json!({"metric":"assembly_mass_kg","maximum":0.00235})).unwrap(),
    );
    let report = run(&request, &body);
    assert_eq!(report, run(&request, &body));
    let search = &report["design_search"];
    assert_eq!(search["consumer_guidance"]["basis"], "reported_bounds");
    let trials = search["rows"].as_array().unwrap();
    let initial = &trials[0]["consumer_comparison"];
    let initial_mass = &initial["readings"]["assembly_mass_kg"];
    assert!(initial_mass["value"].as_f64().unwrap() < 0.00235);
    assert!(initial_mass["reported_bounds"][1].as_f64().unwrap() > 0.00235);
    assert_eq!(initial["eligible"], false);
    let selected = trials
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    assert_eq!(selected["design"]["variables_mm"]["length"], 20.0);
    let reading = &selected["consumer_comparison"]["readings"]["assembly_mass_kg"];
    for (endpoint, density) in [0.17, 0.19].into_iter().enumerate() {
        let expected = 2.0 * 40.0 * 140.0 * 1e-6 * density;
        assert!((reading["reported_bounds"][endpoint].as_f64().unwrap() - expected).abs() < 1e-12);
    }
    let actual = row(
        &report,
        selected["design"]["key"].as_str().unwrap(),
        "light-sheet",
    );
    assert_eq!(
        reading["reported_bounds"],
        actual["assembly_mass"]["result"]["assembly_mass"]["reported_bounds_kg"]
    );
    assert_eq!(search["consumer_guidance"]["best_reading"], *reading);
    assert_eq!(report["comparison"]["uncertainty_propagated"], false);
    assert_eq!(report["comparison"]["physical_validation_claim"], false);

    let mut nominal = request.clone();
    nominal.comparison.as_mut().unwrap().basis = Basis::Nominal;
    let nominal = run(&nominal, &body);
    assert_eq!(
        nominal["design_search"]["rows"][0]["consumer_comparison"]["eligible"],
        true
    );
    assert!(
        nominal["design_search"]["consumer_guidance"]
            .get("basis")
            .is_none()
    );

    let mut maximize = bounded_consumer_request(12, [0.179, 0.181]);
    maximize.comparison.as_mut().unwrap().objectives[0].direction =
        ftx_pattern::material_screen::Direction::Maximize;
    let maximized = run(&maximize, &body);
    let selected = maximized["design_search"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    assert_eq!(selected["design"]["variables_mm"]["length"], 60.0);

    request.variants[0]
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
    let missing = run(&request, &body);
    assert_eq!(missing["design_search"]["selected_designs"], 0);
    for trial in missing["design_search"]["rows"].as_array().unwrap() {
        if let Some(comparison) = trial.get("consumer_comparison") {
            assert_eq!(comparison["eligible"], false);
            assert!(comparison["readings"]["assembly_mass_kg"]["value"].is_number());
            assert!(
                comparison["readings"]["assembly_mass_kg"]
                    .get("reported_bounds")
                    .is_none()
            );
            assert!(
                comparison["exclusions"]
                    .to_string()
                    .contains("no complete valid reported bounds")
            );
        }
    }
}

#[test]
fn bounded_consumer_retention_replaces_only_a_strictly_dominated_range() {
    let mut value = serde_json::to_value(bounded_consumer_request(4, [0.16, 0.192])).unwrap();
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
    let body = Measurements(json!({"desired_length":100}));
    let report = run(&request, &body);
    assert_eq!(report, run(&request, &body));
    let search = &report["design_search"];
    let trials = search["rows"].as_array().unwrap();
    assert_eq!(trials.len(), 4);
    assert_eq!(search["selected_designs"], 2);
    assert_eq!(search["stop_reason"], "evaluation_budget");
    assert_eq!(trials[0]["selected"], false);
    assert_eq!(trials[1]["selected"], true);
    assert_eq!(trials[2]["selected"], false);
    assert_eq!(trials[3]["selected"], true);
    assert_eq!(
        trials[1]["design"]["variables_mm"],
        json!({"width":-5.0,"length":40.0})
    );
    assert_eq!(
        trials[3]["design"]["variables_mm"],
        json!({"width":0.0,"length":0.0})
    );
    let bounds = |index: usize| -> [f64; 2] {
        serde_json::from_value(
            trials[index]["consumer_comparison"]["readings"]["assembly_mass_kg"]["reported_bounds"]
                .clone(),
        )
        .unwrap()
    };
    // The last trial strictly improves the initial range but overlaps the later width trial.
    // There is no single "worst" range: evicting or retaining by midpoint would give a false order.
    assert!(bounds(3)[1] < bounds(0)[0]);
    assert!(bounds(3)[1] > bounds(1)[0] && bounds(3)[0] < bounds(1)[1]);
    assert_eq!(
        search["consumer_guidance"]["best_trial_key"],
        trials[3]["design"]["key"]
    );
    assert_eq!(search["global_optimum_claim"], false);
}

#[test]
fn bounded_consumer_polling_keeps_overlapping_and_exactly_touching_ranges_unordered() {
    let body = Measurements(json!({"desired_length":120}));
    for maximum in [1, 2] {
        let mut request = bounded_consumer_request(16, [0.16, 0.20]);
        request.design_adaptive.as_mut().unwrap().max_designs = maximum;
        let report = run(&request, &body);
        let search = &report["design_search"];
        let trials = search["rows"].as_array().unwrap();
        assert_eq!(search["selected_designs"], maximum);
        assert_eq!(trials[0]["selected"], true);
        assert_eq!(trials[1]["selected"], maximum == 2);
        assert_eq!(trials[1]["design"]["variables_mm"]["length"], 20.0);
        assert_eq!(
            search["consumer_guidance"]["best_trial_key"],
            trials[0]["design"]["key"]
        );
        assert!(
            trials[1]["consumer_comparison"]["readings"]["assembly_mass_kg"]["value"]
                .as_f64()
                .unwrap()
                < trials[0]["consumer_comparison"]["readings"]["assembly_mass_kg"]["value"]
                    .as_f64()
                    .unwrap()
        );
    }

    let mut source: Value = serde_json::from_str(PATTERN).unwrap();
    source["meta"]["sa_mm"] = json!(0);
    for piece in source["pieces"].as_array_mut().unwrap() {
        piece["vertices"] = json!({"a":[0,0],"b":[125,0],"c":[125,125],"d":[0,125]});
    }
    let mut value = serde_json::to_value(bounded_consumer_request(2, [0.125, 0.25])).unwrap();
    value["cutting"] = Value::Null;
    value["sewing"] = Value::Null;
    value["comparison"]["required_consumers"] = json!([]);
    value["variants"].as_array_mut().unwrap().truncate(1);
    value["design_fit"]["fit_spec"]["points"][0]["target_mm"] = json!([0, 60, 125]);
    let variable = &mut value["design_adaptive"]["variables"][0];
    variable["bounds_mm"] = json!([0, 125]);
    variable["initial_mm"] = json!(125);
    variable["initial_step_mm"] = json!(125);
    variable["minimum_step_mm"] = json!(125);
    let request: Request = serde_json::from_value(value).unwrap();
    let body = Measurements(json!({"desired_length":125}));
    let report = serde_json::to_value(
        request
            .evaluate_with_fit_body(
                &Pattern::new(source),
                Some(&Context {
                    body: &body,
                    identity: &json!({"source":"test:exact-touching-mass-ranges"}),
                }),
            )
            .unwrap(),
    )
    .unwrap();
    let trials = report["design_search"]["rows"].as_array().unwrap();
    // These power-of-two areas/densities yield exactly equal endpoints, without a tolerance.
    assert_eq!(
        trials[0]["consumer_comparison"]["readings"]["assembly_mass_kg"]["reported_bounds"][0],
        json!(0.0078125)
    );
    assert_eq!(
        trials[1]["consumer_comparison"]["readings"]["assembly_mass_kg"]["reported_bounds"][1],
        json!(0.0078125)
    );
    assert_eq!(trials[0]["selected"], true);
    assert_eq!(trials[1]["selected"], false);
    assert_eq!(trials[1]["consumer_comparison"]["eligible"], true);
    assert!(
        !report["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn named_bounded_guidance_keeps_range_guards_and_exact_cached_case_decisions() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let mut request = bounded_consumer_request(20, [0.179, 0.181]);
    request.design_adaptive.as_mut().unwrap().case_key = Some("guide".into());
    let mut cases = consumer_cases();
    let mut guard = request.comparison.clone().unwrap();
    guard.constraints.push(
        serde_json::from_value(json!({
        "metric":"assembly_mass_kg","minimum":0.002012}))
        .unwrap(),
    );
    cases[0].comparison = Some(guard);
    let mut calls = std::collections::BTreeMap::new();
    let report = serde_json::to_value(
        request
            .evaluate_matrix_with_fit(
                &pattern,
                &cases,
                48,
                Some(&Context {
                    body: &body,
                    identity: &json!({"source":"test:range-guided-cases"}),
                }),
                |index, key, bound| {
                    *calls.entry((index, key.to_owned())).or_insert(0) += 1;
                    Ok(case_output(bound, &cases[index]))
                },
            )
            .unwrap(),
    )
    .unwrap();
    let search = &report["manufacturing"]["design_search"];
    assert_eq!(search["consumer_guidance"]["basis"], "reported_bounds");
    assert_eq!(search["consumer_guidance"]["reserved_evaluations"], 48);
    assert_eq!(report["planned_evaluations"], 48);
    assert_eq!(
        report["evaluations"].as_u64().unwrap() as usize,
        calls.values().sum::<usize>()
    );
    assert!(calls.values().all(|&count| count == 1));
    let trials = search["rows"].as_array().unwrap();
    let excluded = trials
        .iter()
        .find(|trial| trial["design"]["variables_mm"]["length"] == 20.0)
        .unwrap();
    let mass = &excluded["consumer_comparison"]["readings"]["assembly_mass_kg"];
    assert!(mass["value"].as_f64().unwrap() > 0.002012);
    assert!(mass["reported_bounds"][0].as_f64().unwrap() < 0.002012);
    assert_eq!(excluded["consumer_comparison"]["eligible"], true);
    assert_eq!(
        excluded["consumer_cases"][0]["comparison"]["eligible"],
        false
    );
    assert_eq!(excluded["selected"], false);
    let selected = trials
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    assert_eq!(selected["design"]["variables_mm"]["length"], 25.0);
    let selected_key = key(selected["design"]["key"].as_str().unwrap(), "light-sheet");
    for index in 0..cases.len() {
        let decision = report["cases"][index]["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["key"] == selected_key)
            .unwrap();
        for field in [
            "readings",
            "eligible",
            "exclusions",
            "input_protocol_sha256",
        ] {
            assert_eq!(
                selected["consumer_cases"][index]["comparison"][field],
                decision[field]
            );
        }
        assert_eq!(
            calls[&(index, selected_key.as_str().unwrap().to_owned())],
            1
        );
    }
}

#[test]
fn consumer_search_preflights_all_probe_work_and_requires_explicit_single_case_policy() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":120}));
    let identity = json!({"source":"test:consumer-reservation"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let mut excessive = consumer_request(32);
    excessive.cutting.as_mut().unwrap().copies = 256;
    let mut calls = 0;
    let error = excessive
        .evaluate_with_fit(&pattern, &json!({}), Some(&fit), |_, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(error.code, RefusalCode::BudgetExceeded);
    assert_eq!(calls, 0);
    let mut invalids = Vec::new();
    let mut missing_policy = consumer_request(8);
    missing_policy.comparison = None;
    invalids.push(missing_policy);
    let mut unknown = consumer_request(8);
    unknown.design_adaptive.as_mut().unwrap().material_key = Some("absent".into());
    invalids.push(unknown);
    let mut multiple = consumer_request(8);
    let objective = multiple.comparison.as_ref().unwrap().objectives[0].clone();
    multiple
        .comparison
        .as_mut()
        .unwrap()
        .objectives
        .push(objective);
    invalids.push(multiple);
    let mut bounds = consumer_request(8);
    bounds.comparison.as_mut().unwrap().basis =
        ftx_export::material_study::comparison::Basis::ReportedBounds;
    bounds.comparison.as_mut().unwrap().objectives[0].metric =
        ftx_export::material_study::comparison::Metric::FeedSeconds;
    invalids.push(bounds);
    for request in invalids {
        assert!(
            request
                .evaluate_with_fit_body(&pattern, Some(&fit))
                .is_err()
        );
    }
    let error = consumer_request(8)
        .evaluate_matrix_with_fit(
            &pattern,
            &[Case {
                key: "single-named-case".into(),
                scenario: json!({}),
                comparison: None,
            }],
            32,
            Some(&fit),
            |_, _, _| {
                calls += 1;
                Ok(json!({}))
            },
        )
        .unwrap_err();
    assert!(error.message.contains("case_key"));
    assert_eq!(calls, 0);
}

#[test]
fn adaptive_finds_between_grid_values_and_replays_actual_cutting_sewing_and_mass() {
    let body = Measurements(json!({"desired_length":133.7}));
    let request = adaptive_request(96, 1);
    let report = run(&request, &body);
    assert_eq!(report, run(&request, &body));
    let search = &report["design_search"];
    assert_eq!(search["schema"], "ftx-design-adaptive-search/1");
    assert_eq!(search["selected_designs"], 1);
    assert_eq!(search["stop_reason"], "qualified_design_limit");
    assert_eq!(search["budget_exhausted"], false);
    assert_eq!(search["complete_search"], false);
    assert_eq!(search["global_optimum_claim"], false);
    let selected = search["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    let spread = selected["design"]["variables_mm"]["length"]
        .as_f64()
        .unwrap();
    // The canonical operator rounds edited points to 0.001 mm, while variables retain the
    // exact command. This narrower band admits only the 133.700 mm geometry near our target.
    assert!((spread - 33.7).abs() <= 0.001, "{selected}");
    assert_eq!(selected["raw_fit"]["result"][0]["finished_mm"], 133.7);
    assert_eq!(selected["max_fit_band_violation_mm"], 0.0);
    assert_eq!(selected["raw_fit"]["result"][0]["status"], "IN_BAND");
    let design_key = selected["design"]["key"].as_str().unwrap();
    assert_eq!(
        report["comparison"]["frontier"],
        json!([key(design_key, "light-sheet")])
    );
    for material in ["light-sheet", "heavy-sheet"] {
        let candidate = row(&report, design_key, material);
        assert_eq!(candidate["design"]["fit"]["result"]["passed"], true);
        let altered = Pattern::from_json(
            candidate["design"]["altered_pattern_json"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let frozen: Changes = serde_json::from_value(candidate["frozen_changes"].clone()).unwrap();
        assert_eq!(
            frozen.apply(&altered).unwrap()["pattern"],
            candidate["binding"]["result"]["pattern"]
        );
        let bound = Pattern::from_json(
            candidate["binding"]["result"]["pattern_json"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            candidate["cutting"]["result"],
            serde_json::to_value(
                CutPlan::build(&bound, request.cutting.as_ref().unwrap()).unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            candidate["sewing"]["result"],
            serde_json::to_value(SewPlan::build(&bound, request.sewing.as_ref().unwrap()).unwrap())
                .unwrap()
        );
        let direct = request
            .assembly_mass
            .as_ref()
            .unwrap()
            .evaluate(&bound)
            .unwrap();
        assert_eq!(
            candidate["assembly_mass"]["result"],
            serde_json::to_value(direct).unwrap()
        );
        assert_eq!(
            row(&report, "baseline", material)["binding"]["status"],
            "refused"
        );
    }
    // The old finite grid remains an honest empty result for the same narrow target.
    let mut grid = request.clone();
    grid.design_adaptive = None;
    grid.design_grid = Some(
        serde_json::from_value(json!({"variables":[{"key":"length",
        "values_mm":[20,30,40],"sites":[{"piece":"front","axis":"y","at_mm":50,"multiplier":1},
        {"piece":"back","axis":"y","at_mm":50,"multiplier":1}]}],"max_designs":3}))
        .unwrap(),
    );
    assert!(
        run(&grid, &body)["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn exact_requested_spread_replays_the_canonical_rounded_geometry() {
    let mut request = adaptive_request(1, 1);
    let spread = 33.69873046875;
    request.design_adaptive.as_mut().unwrap().variables[0].initial_mm = spread;
    request.design_fit.as_mut().unwrap().fit_spec["points"][0]["target_mm"] =
        json!([-0.001, 0, 0.001]);
    let report = run(&request, &Measurements(json!({"desired_length":133.7})));
    let trial = &report["design_search"]["rows"][0];
    assert_eq!(trial["selected"], true, "{trial}");
    assert_eq!(trial["design"]["variables_mm"]["length"], spread);
    assert!((spread - 33.7).abs() > 0.001);
    assert_eq!(trial["raw_fit"]["result"][0]["finished_mm"], 133.699);
    assert_eq!(trial["raw_fit"]["result"][0]["status"], "IN_BAND");
    assert_eq!(trial["max_fit_band_violation_mm"], 0.0);
}

#[test]
fn rounded_zero_residual_never_stops_search_or_selects_an_out_of_band_trial() {
    let mut request = adaptive_request(96, 1);
    let variable = &mut request.design_adaptive.as_mut().unwrap().variables[0];
    variable.bounds_mm = [0.0, 0.1];
    variable.initial_step_mm = 0.05;
    variable.minimum_step_mm = 0.00001;
    request.design_fit.as_mut().unwrap().fit_spec["points"][0]["target_mm"] =
        json!([-0.0001, 0, 0.0001]);
    let report = run(&request, &Measurements(json!({"desired_length":100.04})));
    let first = &report["design_search"]["rows"][0];
    assert_eq!(
        first["design"]["fit"]["result"]["validation"]["points"][0]["residual_mm"],
        0.0
    );
    assert_eq!(
        first["design"]["fit"]["result"]["validation"]["points"][0]["status"],
        "LOW"
    );
    assert_eq!(first["selected"], false);
    assert!(first["max_fit_band_violation_mm"].as_f64().unwrap() > 0.039);
    assert_eq!(report["design_search"]["selected_designs"], 1);
}

#[test]
fn evaluation_limits_preserve_qualified_incumbents_and_minimum_steps_do_not_prove_infeasibility() {
    let body = Measurements(json!({"desired_length":100}));
    let budgeted = run(&adaptive_request(1, 2), &body);
    let search = &budgeted["design_search"];
    assert_eq!(search["evaluations"], 1);
    assert_eq!(search["selected_designs"], 1);
    assert_eq!(search["stop_reason"], "evaluation_budget");
    assert_eq!(search["budget_exhausted"], true);
    assert_eq!(search["complete_search"], false);
    assert_eq!(
        row(&budgeted, "adaptive:0000", "light-sheet")["cutting"]["status"],
        "available"
    );
    let mut limited = adaptive_request(96, 1);
    limited.design_adaptive.as_mut().unwrap().variables[0].minimum_step_mm = 20.0;
    let missed = run(&limited, &Measurements(json!({"desired_length":133.7})));
    assert_eq!(missed["design_search"]["selected_designs"], 0);
    assert_eq!(missed["design_search"]["stop_reason"], "minimum_steps");
    assert_eq!(missed["design_search"]["budget_exhausted"], false);
    assert_eq!(missed["design_search"]["complete_search"], false);
    assert!(
        missed["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Repeated boundary coordinates are cached rather than consuming additional trial slots.
    let points: BTreeSet<_> = missed["design_search"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|trial| trial["design"]["variables_mm"].to_string())
        .collect();
    assert_eq!(
        points.len(),
        missed["design_search"]["evaluations"].as_u64().unwrap() as usize
    );
}

#[test]
fn simultaneous_fit_points_use_the_explicit_secondary_residual_to_cross_a_maximum_plateau() {
    let mut request = adaptive_request(160, 1);
    let adaptive = request.design_adaptive.as_mut().unwrap();
    let mut width = adaptive.variables[0].clone();
    width.key = "width".into();
    for site in &mut width.sites {
        if let ftx_export::material_study::design::grid::Site::SlashSpread { axis, at_mm, .. } =
            site
        {
            *axis = "x".into();
            *at_mm = 10.0;
        }
    }
    adaptive.variables.push(width);
    request.design_fit.as_mut().unwrap().fit_spec["points"]
        .as_array_mut()
        .unwrap()
        .push(json!({
        "id":"width","kind":"width","instance":"front","at_vertex":"a","dy":50,
        "body":"desired_width","target_mm":[-0.0001,0,0.0001]}));
    let report = run(
        &request,
        &Measurements(json!({"desired_length":133.7,"desired_width":53.7})),
    );
    assert_eq!(report["design_search"]["selected_designs"], 1);
    let selected = report["design_search"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    for variable in ["length", "width"] {
        assert!(
            (selected["design"]["variables_mm"][variable]
                .as_f64()
                .unwrap()
                - 33.7)
                .abs()
                <= 0.001,
            "{selected}"
        );
    }
    assert_eq!(selected["raw_fit"]["result"].as_array().unwrap().len(), 2);
    assert_eq!(selected["raw_fit"]["result"][0]["finished_mm"], 133.7);
    assert_eq!(selected["raw_fit"]["result"][1]["finished_mm"], 53.7);
}

#[test]
fn selected_body_changes_search_and_no_body_or_missing_reading_cannot_fall_back() {
    let request = adaptive_request(96, 1);
    let first = run(&request, &Measurements(json!({"desired_length":133.7})));
    let second = run(&request, &Measurements(json!({"desired_length":140.0})));
    assert_ne!(first["request_sha256"], second["request_sha256"]);
    let selected = second["design_search"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    assert_eq!(selected["design"]["variables_mm"]["length"], 40.0);
    assert_eq!(
        selected["design"]["fit"]["result"]["body_identity"]["values"]["desired_length"],
        140.0
    );
    let pattern = Pattern::from_json(PATTERN).unwrap();
    assert!(
        request
            .evaluate(&pattern)
            .unwrap_err()
            .message
            .contains("fitted-body measurement context")
    );
    let missing = run(&adaptive_request(8, 1), &Measurements(json!({})));
    assert_eq!(missing["design_search"]["selected_designs"], 0);
    for trial in missing["design_search"]["rows"].as_array().unwrap() {
        assert_eq!(trial["raw_fit"]["status"], "refused");
        assert!(trial["max_fit_band_violation_mm"].is_null());
    }
}

#[test]
fn broken_seam_trials_remain_unscored_and_cannot_invoke_production_callbacks() {
    let mut request = adaptive_request(12, 1);
    request.design_adaptive.as_mut().unwrap().variables[0]
        .sites
        .pop();
    let body = Measurements(json!({"desired_length":133.7}));
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let mut calls = 0;
    let report = request
        .evaluate_with_fit(
            &pattern,
            &json!({"case":"synthetic"}),
            Some(&Context {
                body: &body,
                identity: &json!({"source":"synthetic-broken-seams"}),
            }),
            |_, _| {
                calls += 1;
                Ok(json!({}))
            },
        )
        .unwrap();
    assert_eq!(calls, 0);
    let value = serde_json::to_value(report).unwrap();
    let rows = value["manufacturing"]["design_search"]["rows"]
        .as_array()
        .unwrap();
    assert!(
        rows.iter()
            .any(|trial| trial["design"]["validation"]["flat"]["summary"]["seams_failing"] == 1)
    );
    for trial in rows
        .iter()
        .filter(|trial| trial["design"]["validation"]["flat"]["summary"]["seams_failing"] == 1)
    {
        assert!(trial["max_fit_band_violation_mm"].is_null());
        assert_eq!(trial["selected"], false);
    }
}

#[test]
fn adaptive_search_is_shared_once_across_materials_and_named_cases() {
    let request = adaptive_request(96, 1);
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":133.7}));
    let identity = json!({"source":"synthetic-adaptive-matrix"});
    let context = Context {
        body: &body,
        identity: &identity,
    };
    let single = request
        .evaluate_with_fit_body(&pattern, Some(&context))
        .unwrap();
    let cases = vec![
        Case {
            key: "one".into(),
            scenario: json!({}),
            comparison: None,
        },
        Case {
            key: "two".into(),
            scenario: json!({}),
            comparison: None,
        },
    ];
    let mut calls = 0;
    let matrix = request
        .evaluate_matrix_with_fit(&pattern, &cases, 8, Some(&context), |_, _, actual| {
            calls += 1;
            let length = actual.raw()["pieces"][0]["vertices"]["d"][1]
                .as_f64()
                .unwrap();
            assert!((length - 133.7).abs() <= 0.001);
            Ok(json!({}))
        })
        .unwrap();
    assert_eq!(calls, 4);
    assert_eq!(
        serde_json::to_value(matrix.manufacturing.design_search).unwrap(),
        serde_json::to_value(single.design_search).unwrap()
    );
}

#[test]
fn malformed_domains_and_full_reservations_refuse_before_production_work() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":133.7}));
    let identity = json!({"source":"synthetic-admission"});
    let context = Context {
        body: &body,
        identity: &identity,
    };
    for (field, invalid_value) in [
        ("bounds_mm", json!([2, 1])),
        ("initial_mm", json!(61)),
        ("minimum_step_mm", json!(0)),
        ("initial_step_mm", json!(0.00001)),
        ("bounds_mm", json!([-1.7e308, 1.7e308])),
    ] {
        let mut value = serde_json::to_value(adaptive_request(96, 1)).unwrap();
        value["design_adaptive"]["variables"][0][field] = invalid_value;
        let bad: Request = serde_json::from_value(value).unwrap();
        assert!(
            bad.evaluate_with_fit_body(&pattern, Some(&context))
                .is_err()
        );
    }
    let mut excessive = adaptive_request(96, 31);
    let mut calls = 0;
    let error = excessive
        .evaluate_with_fit(&pattern, &json!({}), Some(&context), |_, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(error.code, RefusalCode::BudgetExceeded);
    assert_eq!(calls, 0);
    excessive.design_adaptive.as_mut().unwrap().max_designs = 1;
    excessive.design_adaptive.as_mut().unwrap().after = vec![
            ftx_export::material_study::design::Operation::TrueSeam {
                seam: "side".into(),
                end: "end".into()
            };
            14
        ];
    let mut source = pattern.raw().clone();
    source["meta"]["annotation"] = json!("x".repeat(20_000));
    let error = excessive
        .evaluate_with_fit_body(&Pattern::new(source), Some(&context))
        .unwrap_err();
    assert_eq!(error.code, RefusalCode::BudgetExceeded);
    assert!(error.message.contains("document-processing"));
}
