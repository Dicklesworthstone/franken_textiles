//! Real geometry/manufacturing and explicitly synthetic scenario decisions for bounded Pareto
//! discovery. These fixtures establish software behavior, not empirical garment performance.
use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_export::material_study::{Request, design::fit::Context, matrix::Case};
use ftx_pattern::{Pattern, validate::Measurements};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");

fn request(evaluations: usize, retained: usize) -> Value {
    let mut value: Value =
        serde_json::from_str(include_str!("../../../examples/design-study.json")).unwrap();
    value.as_object_mut().unwrap().remove("designs");
    value["design_adaptive"] = json!({
        "objective":"consumer_pareto", "material_key":"light-sheet",
        "variables":[{"key":"length","bounds_mm":[0,60],"initial_mm":20,
            "initial_step_mm":20,"minimum_step_mm":20,
            "sites":[{"piece":"front","axis":"y","at_mm":50,"multiplier":1},
                     {"piece":"back","axis":"y","at_mm":50,"multiplier":1}]}],
        "max_evaluations":evaluations,"max_designs":retained});
    value["design_fit"] = json!({"fit_spec":{"schema":"fabric-fit/1","points":[
        {"id":"length","kind":"vertical","instance":"front","from":"a","to":"d",
         "body":"desired_length","target_mm":[0,30,60]}]}});
    value["assembly_mass"] =
        json!({"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[]});
    // Deliberately opposing actual manufacturing quantities test tradeoff exploration without
    // inventing a weighted score or a physiological preference for either quantity.
    value["comparison"]["objectives"] = json!([
        {"metric":"assembly_mass_kg","direction":"minimize"},
        {"metric":"feed_seconds","direction":"maximize"}]);
    value
}

fn run(value: Value) -> Value {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let request: Request = serde_json::from_value(value).unwrap();
    serde_json::to_value(
        request
            .evaluate_with_fit_body(
                &pattern,
                Some(&Context {
                    body: &body,
                    identity: &json!({"source":"test:pareto-fit","values":body.0}),
                }),
            )
            .unwrap(),
    )
    .unwrap()
}

fn rows(search: &Value) -> &[Value] {
    search["rows"].as_array().unwrap()
}

fn spread(row: &Value) -> f64 {
    row["design"]["variables_mm"]["length"].as_f64().unwrap()
}

fn keys<'a>(rows: impl IntoIterator<Item = &'a Value>) -> Value {
    json!(
        rows.into_iter()
            .map(|row| &row["design"]["key"])
            .collect::<Vec<_>>()
    )
}

fn scenario(key: &str) -> Value {
    json!({"scenario_sha256":sha256_hex(key.as_bytes()),
        "body_geometry_sha256":"2".repeat(64),"declared_case":key})
}

fn output(pattern: &Pattern, scenario: &Value, mass: f64) -> Value {
    json!({"schema":"ftx-drape-scenario-result/1",
        "pattern_sha256":sha256_hex(&serde_json::to_vec(pattern.raw()).unwrap()),
        "scenario_sha256":scenario["scenario_sha256"],
        "body_geometry_sha256":scenario["body_geometry_sha256"],
        "metrics":{"simulation_gating_clear":true},
        "drape":{"settled":true,"gating":[],"mass_kg":mass}})
}

#[test]
fn incomparable_actual_centers_continue_beyond_the_retained_output_capacity() {
    let report = run(request(16, 2));
    let search = &report["design_search"];
    let guidance = &search["consumer_guidance"];
    let trials = rows(search);
    assert_eq!(
        trials.iter().map(spread).collect::<Vec<_>>(),
        [20., 0., 40., 60.]
    );
    assert_eq!(search["stop_reason"], "minimum_steps");
    assert_eq!(search["final_steps_mm"], json!({"length":20.}));
    assert_eq!(search["qualified_designs"], 4);
    assert_eq!(search["selected_designs"], 2);
    assert!(guidance.get("objective").is_none());
    assert_eq!(
        guidance["objectives"],
        request(16, 2)["comparison"]["objectives"]
    );
    for field in ["best_trial_key", "best_reading"] {
        assert!(guidance[field].is_null());
    }
    assert!(search["best_trial_key"].is_null());
    assert!(search["best_max_fit_band_violation_mm"].is_null());
    assert!(search["best_total_fit_band_violation_mm"].is_null());
    assert_eq!(search["complete_search"], false);
    assert_eq!(search["global_optimum_claim"], false);
    assert_eq!(report["physical_validation_claim"], false);
    let frontier = &guidance["pareto_frontier"];
    assert_eq!(frontier["retention_capacity"], 2);
    assert_eq!(frontier["evaluated_trial_keys"], keys(trials));
    assert_eq!(frontier["retained_trial_keys"], keys(&trials[..2]));
    assert_eq!(frontier["unretained_trial_keys"], keys(&trials[2..]));
    for trial in trials {
        let readings = &trial["consumer_comparison"]["readings"];
        // Two rectangles including the authored 10 mm allowance, with a synthetic 0.18 kg/m²
        // sheet. One joined side traverses (100 + spread)/2.5 stitch intervals at 600/minute.
        let mass = 2.0 * 40.0 * (120.0 + spread(trial)) * 1e-6 * 0.18;
        let feed = (100.0 + spread(trial)) / 2.5 / 10.0;
        assert!((readings["assembly_mass_kg"]["value"].as_f64().unwrap() - mass).abs() < 1e-12);
        assert!((readings["feed_seconds"]["value"].as_f64().unwrap() - feed).abs() < 1e-12);
        assert_eq!(trial["consumer_comparison"]["eligible"], true);
        assert!(trial.get("refusal").is_none());
        if trial["selected"] != true {
            continue;
        }
        for material in ["light-sheet", "heavy-sheet"] {
            let key = serde_json::to_string(&(trial["design"]["key"].as_str().unwrap(), material))
                .unwrap();
            let alternative = report["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == key)
                .unwrap();
            assert_eq!(
                alternative["design"]["variables_mm"],
                trial["design"]["variables_mm"]
            );
            let expected = mass * if material == "light-sheet" { 1.0 } else { 2.0 };
            assert!(
                (alternative["metrics"]["assembly_mass_kg"].as_f64().unwrap() - expected).abs()
                    < 1e-12
            );
            assert!(
                (alternative["metrics"]["feed_seconds"].as_f64().unwrap() - feed).abs() < 1e-12
            );
        }
    }
    // A capacity-excluded 40 mm center discovers 60 mm even when only the initial output fits.
    let smaller = run(request(16, 1));
    assert_eq!(
        smaller["design_search"]["consumer_guidance"]["pareto_frontier"]["evaluated_trial_keys"],
        frontier["evaluated_trial_keys"]
    );
    assert_eq!(smaller["design_search"]["selected_designs"], 1);
    assert_eq!(report, run(request(16, 2)));
}

#[test]
fn a_later_dominating_trial_displaces_every_earlier_cached_tradeoff() {
    let mut value = request(12, 3);
    value["design_adaptive"]["variables"][0]["initial_mm"] = json!(40);
    value["comparison"]["objectives"][1] =
        json!({"metric":"simulated_cloth_mass_kg","direction":"minimize"});
    let request: Request = serde_json::from_value(value).unwrap();
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let scenario = scenario("displacement");
    let mut calls = BTreeMap::new();
    let mut outputs = BTreeMap::new();
    let report = request
        .evaluate_with_fit(
            &pattern,
            &scenario,
            Some(&Context {
                body: &body,
                identity: &json!({"source":"test:pareto-displacement"}),
            }),
            |key, bound| {
                *calls.entry(key.to_owned()).or_insert(0) += 1;
                let x = bound.raw()["pieces"][0]["vertices"]["d"][1]
                    .as_f64()
                    .unwrap()
                    - 100.0;
                // Independent synthetic decisions create three tradeoffs followed by one point that
                // strictly improves both actual assembly mass and this explicit scenario metric.
                let mass = if x == 0.0 { 0.5 } else { 4.0 - x / 20.0 };
                let mut result = output(bound, &scenario, mass);
                result["test_probe_identity"] = json!(outputs.len());
                outputs.insert(key.to_owned(), result.clone());
                Ok(result)
            },
        )
        .unwrap();
    let report = serde_json::to_value(report).unwrap();
    let search = &report["manufacturing"]["design_search"];
    let trials = rows(search);
    assert_eq!(
        trials.iter().map(spread).collect::<Vec<_>>(),
        [40., 20., 60., 0.]
    );
    assert!(trials[..3].iter().all(|row| row["selected"] == false));
    assert_eq!(trials[3]["selected"], true);
    assert_eq!(search["selected_designs"], 1);
    let frontier = &search["consumer_guidance"]["pareto_frontier"];
    assert_eq!(frontier["evaluated_trial_keys"], keys(&trials[3..]));
    assert_eq!(frontier["retained_trial_keys"], keys(&trials[3..]));
    assert_eq!(frontier["unretained_trial_keys"], json!([]));
    let key = serde_json::to_string(&(trials[3]["design"]["key"].as_str().unwrap(), "light-sheet"))
        .unwrap();
    let retained = report["simulations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == key)
        .unwrap();
    assert_eq!(retained["evaluation"]["result"], outputs[&key]);
    assert!(calls.values().all(|&count| count == 1));
}

#[test]
fn explicit_restarts_poll_dominated_seeds_without_resetting_the_frontier_cache_or_budget() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let scenario = scenario("pareto-restarts");
    for allowance in [3, 12] {
        let mut value = request(allowance, 2);
        value["design_adaptive"]["variables"][0]["initial_mm"] = json!(0);
        value["design_adaptive"]["restart_points_mm"] = json!([{"length":60},{"length":40}]);
        value["comparison"]["objectives"][1] =
            json!({"metric":"simulated_cloth_mass_kg","direction":"maximize"});
        let request: Request = serde_json::from_value(value).unwrap();
        let mut calls = BTreeMap::new();
        let mut outputs = BTreeMap::new();
        let report = request
            .evaluate_with_fit(
                &pattern,
                &scenario,
                Some(&Context {
                    body: &body,
                    identity: &json!({"source":"test:pareto-restart-fit"}),
                }),
                |key, bound| {
                    *calls.entry(key.to_owned()).or_insert(0) += 1;
                    let x = bound.raw()["pieces"][0]["vertices"]["d"][1]
                        .as_f64()
                        .unwrap()
                        - 100.0;
                    // Initial 0 mm dominates its 20 mm neighbor and the explicit 60 mm start.
                    // Only polling around that dominated restart exposes the 40 mm tradeoff.
                    let mass = if x == 0.0 {
                        5.0
                    } else if x == 20.0 {
                        1.0
                    } else if x == 40.0 {
                        10.0
                    } else {
                        0.5
                    };
                    let mut result = output(bound, &scenario, mass);
                    result["test_probe_identity"] = json!(outputs.len());
                    outputs.insert(key.to_owned(), result.clone());
                    Ok(result)
                },
            )
            .unwrap();
        let report = serde_json::to_value(report).unwrap();
        let search = &report["manufacturing"]["design_search"];
        let trials = rows(search);
        let starts = search["search_starts"].as_array().unwrap();
        let frontier = &search["consumer_guidance"]["pareto_frontier"];
        let selected: Vec<_> = trials
            .iter()
            .filter(|row| row["selected"] == true)
            .collect();
        let expected: &[f64] = if allowance == 3 { &[0.0] } else { &[0.0, 40.0] };
        assert_eq!(
            selected.iter().map(|row| spread(row)).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            frontier["evaluated_trial_keys"],
            keys(selected.iter().copied())
        );
        assert_eq!(
            frontier["retained_trial_keys"],
            keys(selected.iter().copied())
        );
        assert_eq!(frontier["unretained_trial_keys"], json!([]));
        assert!(search["best_trial_key"].is_null());
        assert!(search["consumer_guidance"]["best_trial_key"].is_null());
        assert_eq!(starts[0]["evaluations"], 2);
        assert_eq!(starts[1]["initial_trial_key"], trials[2]["design"]["key"]);
        assert_eq!(spread(&trials[2]), 60.0);
        assert_eq!(trials[2]["consumer_comparison"]["eligible"], true);
        assert_eq!(trials[2]["selected"], false);
        assert_eq!(
            starts
                .iter()
                .map(|start| start["evaluations"].as_u64().unwrap())
                .sum::<u64>(),
            search["evaluations"].as_u64().unwrap()
        );
        if allowance == 3 {
            assert_eq!(starts.len(), 2);
            assert_eq!(starts[1]["evaluations"], 1);
            assert_eq!(starts[1]["stop_reason"], "evaluation_budget");
            assert_eq!(search["budget_exhausted"], true);
            assert_eq!(trials.len(), 3);
        } else {
            assert_eq!(starts.len(), 3);
            assert_eq!(starts[1]["evaluations"], 2);
            assert_eq!(starts[2]["evaluations"], 0);
            assert_eq!(starts[2]["initial_trial_key"], trials[3]["design"]["key"]);
            assert_eq!(search["stop_reason"], "minimum_steps");
            assert_eq!(trials.len(), 4);
        }
        for trial in selected {
            let key =
                serde_json::to_string(&(trial["design"]["key"].as_str().unwrap(), "light-sheet"))
                    .unwrap();
            let retained = report["simulations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == key)
                .unwrap();
            assert_eq!(retained["evaluation"]["result"], outputs[&key]);
            assert_eq!(calls[&key], 1);
        }
        assert!(calls.values().all(|&count| count == 1));
    }
}

fn bounded_request(vapor_bounds: [f64; 2]) -> Value {
    let mut value = request(16, 2);
    value["comparison"]["basis"] = json!("reported_bounds");
    value["comparison"]["objectives"][1] =
        json!({"metric":"vapor_potential_heat_flow_w","direction":"maximize"});
    for (index, scale) in [(0, 1.0), (1, 2.0)] {
        let properties =
            &mut value["variants"][index]["changes"]["default"]["record"]["properties"];
        properties["areal_density"]["bounds"] = json!([0.17 * scale, 0.19 * scale]);
        properties["vapor_resistance"] = json!({"value":2.0,"bounds":vapor_bounds,
            "unit":"m2*Pa/W","evidence":"estimate","source":"test",
            "method":"Synthetic constant resistance for Pareto software tests"});
    }
    value["vapor"] = json!({"layers":[{"key":"shell","target":{"scope":"piece","id":"front"}}],
        "area_m2":0.5,"inside_vapor_pressure_pa":2000.0,"outside_vapor_pressure_pa":1000.0,
        "inside_film_resistance_m2_pa_w":1.0,"outside_film_resistance_m2_pa_w":4.0,
        "latent_heat_j_kg":2000000.0});
    value
}

#[test]
fn reported_range_frontiers_keep_overlap_unknowns_and_independent_nominal_guards() {
    let narrow = run(bounded_request([2.0, 2.0]));
    let search = &narrow["design_search"];
    assert_eq!(search["consumer_guidance"]["basis"], "reported_bounds");
    let selected: Vec<_> = rows(search)
        .iter()
        .filter(|row| row["selected"] == true)
        .collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(spread(selected[0]), 0.0);
    let heat = &selected[0]["consumer_comparison"]["readings"]["vapor_potential_heat_flow_w"];
    assert!((heat["value"].as_f64().unwrap() - 500.0 / 7.0).abs() < 1e-12);
    assert_eq!(heat["reported_bounds"][0], heat["value"]);
    assert_eq!(heat["reported_bounds"][1], heat["value"]);

    let overlapping = run(bounded_request([1.0, 3.0]));
    let search = &overlapping["design_search"];
    let trials = rows(search);
    assert_eq!(trials.len(), 4);
    assert_eq!(
        search["consumer_guidance"]["pareto_frontier"]["evaluated_trial_keys"],
        keys(trials)
    );
    for trial in trials {
        let bounds = &trial["consumer_comparison"]["readings"]["vapor_potential_heat_flow_w"]["reported_bounds"];
        assert!((bounds[0].as_f64().unwrap() - 500.0 / 8.0).abs() < 1e-12);
        assert!((bounds[1].as_f64().unwrap() - 500.0 / 6.0).abs() < 1e-12);
    }

    let mut guarded = bounded_request([2.0, 2.0]);
    guarded["comparison"]["constraints"] = json!([
        {"metric":"feed_seconds","basis":"nominal","minimum":4.7}]);
    let guarded = run(guarded);
    let trials = rows(&guarded["design_search"]);
    assert_eq!(
        spread(trials.iter().find(|row| row["selected"] == true).unwrap()),
        20.0
    );
    let excluded = trials.iter().find(|row| spread(row) == 0.0).unwrap();
    assert_eq!(excluded["consumer_comparison"]["eligible"], false);
    assert!(
        excluded["consumer_comparison"]["readings"]["feed_seconds"]["reported_bounds"].is_null()
    );

    for property in ["areal_density", "vapor_resistance"] {
        let mut missing = bounded_request([2.0, 2.0]);
        for variant in missing["variants"].as_array_mut().unwrap() {
            variant["changes"]["default"]["record"]["properties"][property]
                .as_object_mut()
                .unwrap()
                .remove("bounds");
        }
        let missing = run(missing);
        let search = &missing["design_search"];
        assert_eq!(search["selected_designs"], 0);
        assert_eq!(
            search["consumer_guidance"]["pareto_frontier"]["evaluated_trial_keys"],
            json!([])
        );
        assert!(
            rows(search)
                .iter()
                .all(|row| row["consumer_comparison"]["eligible"] == false)
        );
    }
}

#[test]
fn every_named_case_still_guards_pareto_trials_and_cached_results_survive_callback_refusal() {
    let mut value = request(8, 2);
    value["design_adaptive"]["case_key"] = json!("guide");
    value["design_adaptive"]["variables"][0]["initial_mm"] = json!(40);
    value["comparison"]["objectives"][1] =
        json!({"metric":"simulated_cloth_mass_kg","direction":"minimize"});
    let request: Request = serde_json::from_value(value).unwrap();
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    for reverse in [false, true] {
        let mut cases = vec![
            Case {
                key: "guard".into(),
                scenario: scenario("guard"),
                comparison: Some(
                    serde_json::from_value(json!({"basis":"nominal",
                "required_consumers":["drape"],
                "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
                "constraints":[{"metric":"simulated_cloth_mass_kg","maximum":40}]}))
                    .unwrap(),
                ),
            },
            Case {
                key: "guide".into(),
                scenario: scenario("guide"),
                comparison: None,
            },
        ];
        if reverse {
            cases.reverse();
        }
        let mut calls = BTreeMap::new();
        let mut outputs = BTreeMap::new();
        let report = request
            .evaluate_matrix_with_fit(
                &pattern,
                &cases,
                28,
                Some(&Context {
                    body: &body,
                    identity: &json!({"source":"test:pareto-cases"}),
                }),
                |index, key, bound| {
                    let case = &cases[index];
                    *calls.entry((case.key.clone(), key.to_owned())).or_insert(0) += 1;
                    let length = bound.raw()["pieces"][0]["vertices"]["d"][1]
                        .as_f64()
                        .unwrap();
                    if case.key == "guard" && length == 160.0 {
                        return Err(Refusal::new(
                            RefusalCode::BudgetExceeded,
                            "synthetic guarded callback quota",
                        ));
                    }
                    let mut result = output(bound, &case.scenario, 170.0 - length);
                    result["test_probe_identity"] = json!(outputs.len());
                    outputs.insert((case.key.clone(), key.to_owned()), result.clone());
                    Ok(result)
                },
            )
            .unwrap();
        let report = serde_json::to_value(report).unwrap();
        let search = &report["manufacturing"]["design_search"];
        let guidance = &search["consumer_guidance"];
        assert_eq!(guidance["case_key"], "guide");
        assert_eq!(
            guidance["case_order"],
            json!(cases.iter().map(|case| &case.key).collect::<Vec<_>>())
        );
        assert_eq!(guidance["reserved_evaluations"], 28);
        assert_eq!(report["planned_evaluations"], 28);
        let trials = rows(search);
        let selected = trials.iter().find(|row| row["selected"] == true).unwrap();
        assert_eq!(spread(selected), 40.0);
        assert_ne!(
            selected["consumer_cases"][0]["comparison"]["input_protocol_sha256"],
            selected["consumer_cases"][1]["comparison"]["input_protocol_sha256"]
        );
        for x in [20.0, 60.0] {
            let rejected = trials.iter().find(|row| spread(row) == x).unwrap();
            assert_eq!(rejected["selected"], false);
            assert_eq!(rejected["consumer_comparison"]["eligible"], true);
            assert!(
                rejected["refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("guard")
            );
        }
        let key =
            serde_json::to_string(&(selected["design"]["key"].as_str().unwrap(), "light-sheet"))
                .unwrap();
        for (index, case) in cases.iter().enumerate() {
            let output = report["cases"][index]["simulations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == key)
                .unwrap();
            assert_eq!(
                output["evaluation"]["result"],
                outputs[&(case.key.clone(), key.clone())]
            );
            assert_eq!(calls[&(case.key.clone(), key.clone())], 1);
            let comparison = report["cases"][index]["comparison"]["alternatives"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["key"] == key)
                .unwrap();
            for field in [
                "readings",
                "eligible",
                "exclusions",
                "input_protocol_sha256",
            ] {
                assert_eq!(
                    comparison[field],
                    selected["consumer_cases"][index]["comparison"][field]
                );
            }
        }
        assert!(calls.values().all(|&count| count == 1));
        assert_eq!(
            report["evaluations"].as_u64().unwrap() as usize,
            calls.values().sum::<usize>()
        );
    }
}

#[test]
fn raw_fit_bootstraps_the_frontier_and_incomplete_geometry_never_reaches_consumers() {
    let mut value = request(16, 2);
    value["design_adaptive"]["variables"][0]["initial_mm"] = json!(0);
    value["design_adaptive"]["variables"][0]["minimum_step_mm"] = json!(10);
    value["design_fit"]["fit_spec"]["points"][0]["target_mm"] = json!([30, 30, 30]);
    let report = run(value.clone());
    let search = &report["design_search"];
    assert_eq!(search["consumer_guidance"]["evaluations"], 1);
    let selected = rows(search)
        .iter()
        .find(|row| row["selected"] == true)
        .unwrap();
    assert_eq!(spread(selected), 30.0);
    assert!(
        rows(search)
            .iter()
            .filter(|row| spread(row) != 30.0)
            .all(|row| row.get("consumer_comparison").is_none())
    );
    value["design_adaptive"]["variables"][0]["sites"]
        .as_array_mut()
        .unwrap()
        .pop();
    let broken = run(value);
    assert_eq!(
        broken["design_search"]["consumer_guidance"]["evaluations"],
        0
    );
    assert_eq!(broken["design_search"]["selected_designs"], 0);
    assert_eq!(
        broken["design_search"]["consumer_guidance"]["pareto_frontier"]["evaluated_trial_keys"],
        json!([])
    );
}

#[test]
fn explicit_mode_validation_preflights_work_and_preserves_scalar_wire_fields() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = Measurements(json!({"desired_length":100}));
    let identity = json!({"source":"test:pareto-preflight"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let mut invalids = Vec::new();
    let mut one = request(8, 2);
    one["comparison"]["objectives"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    invalids.push(one.clone());
    let mut scalar_multiple = request(8, 2);
    scalar_multiple["design_adaptive"]["objective"] = json!("consumer_comparison");
    invalids.push(scalar_multiple);
    let mut named = request(8, 2);
    named["design_adaptive"]["case_key"] = json!("absent");
    invalids.push(named);
    let mut missing = request(8, 2);
    missing["design_adaptive"]
        .as_object_mut()
        .unwrap()
        .remove("material_key");
    invalids.push(missing);
    invalids.push(request(8, 31));
    let mut unsupported = request(8, 2);
    unsupported["comparison"]["basis"] = json!("reported_bounds");
    invalids.push(unsupported);
    let mut calls = 0;
    for value in invalids {
        let request: Request = serde_json::from_value(value).unwrap();
        assert!(
            request
                .evaluate_with_fit(&pattern, &scenario("invalid"), Some(&fit), |_, _| {
                    calls += 1;
                    Ok(json!({}))
                })
                .is_err()
        );
        assert_eq!(calls, 0);
    }
    let mut matrix_value = request(8, 2);
    matrix_value["design_adaptive"]["case_key"] = json!("guide");
    let matrix: Request = serde_json::from_value(matrix_value).unwrap();
    let cases = vec![Case {
        key: "guide".into(),
        scenario: scenario("guide"),
        comparison: None,
    }];
    assert_eq!(
        matrix
            .evaluate_matrix_with_fit(&pattern, &cases, 13, Some(&fit), |_, _, _| {
                calls += 1;
                Ok(json!({}))
            })
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(calls, 0);

    one["design_adaptive"]["objective"] = json!("consumer_comparison");
    let scalar = run(one);
    let guidance = &scalar["design_search"]["consumer_guidance"];
    assert_eq!(guidance["objective"]["metric"], "assembly_mass_kg");
    assert!(guidance.get("objectives").is_none());
    assert!(guidance.get("pareto_frontier").is_none());
    assert!(guidance["best_trial_key"].is_string());
    // Unique coordinates, including cached boundary revisits, remain a single execution.
    let coordinates: BTreeSet<_> = rows(&scalar["design_search"])
        .iter()
        .map(|row| spread(row).to_bits())
        .collect();
    assert_eq!(
        coordinates.len(),
        scalar["design_search"]["evaluations"].as_u64().unwrap() as usize
    );
}
