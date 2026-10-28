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
