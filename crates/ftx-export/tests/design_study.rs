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

fn grid_request(max_designs: usize) -> Request {
    let mut input = request();
    input.as_object_mut().unwrap().remove("designs");
    input["design_grid"] = json!({"variables":[{"key":"ease","values_mm":[0,4,8,12],
        "sites":[{"piece":"front","axis":"x","at_mm":10,"multiplier":1},
                 {"piece":"back","axis":"x","at_mm":10,"multiplier":1}]}],
        "max_designs":max_designs});
    input["design_fit"] = json!({"fit_spec":{"schema":"fabric-fit/1","points":[
        {"id":"front-width","kind":"width","instance":"front","at_vertex":"a","dy":50,
         "body":"declared_width","target_mm":[3,6,10]}]}});
    input["assembly_mass"] =
        json!({"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[]});
    input["comparison"]["objectives"] =
        json!([{"metric":"assembly_mass_kg","direction":"minimize"}]);
    serde_json::from_value(input).unwrap()
}

#[test]
fn grid_synthesis_meets_explicit_ease_then_compares_real_material_and_manufacturing_outputs() {
    use ftx_export::material_study::design::fit::Context;
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = ftx_pattern::validate::Measurements(json!({"declared_width":20}));
    let identity = json!({"schema":"synthetic-flat-fit-body/1","source":"test:design-grid"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let request = grid_request(4);
    let report = serde_json::to_value(
        request
            .evaluate_with_fit_body(&pattern, Some(&fit))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(report["alternatives"].as_array().unwrap().len(), 10);
    assert_eq!(report["design_search"]["total_grid_points"], 4);
    assert_eq!(report["design_search"]["complete_grid_enumeration"], true);
    assert_eq!(report["design_search"]["global_optimum_claim"], false);
    for material in ["light-sheet", "heavy-sheet"] {
        for refused in ["baseline", "grid:0000", "grid:0003"] {
            let candidate = row(&report, refused, material);
            assert_eq!(candidate["design"]["fit"]["result"]["passed"], false);
            assert_eq!(candidate["binding"]["status"], "refused");
            assert_eq!(candidate["cutting"]["status"], "refused");
            assert_eq!(candidate["sewing"]["status"], "refused");
        }
        for (design, ease) in [("grid:0001", 4.0), ("grid:0002", 8.0)] {
            let candidate = row(&report, design, material);
            assert_eq!(candidate["design"]["variables_mm"]["ease"], ease);
            let fit = &candidate["design"]["fit"]["result"];
            assert_eq!(fit["body_identity"], identity);
            assert_eq!(fit["validation"]["points"][0]["ease_mm"], ease);
            assert_eq!(fit["validation"]["points"][0]["status"], "IN_BAND");
            assert_eq!(candidate["binding"]["status"], "available");
            assert_eq!(candidate["cutting"]["status"], "available");
            assert_eq!(candidate["sewing"]["status"], "available");
            let source = Pattern::from_json(
                candidate["design"]["altered_pattern_json"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            let changes: Changes =
                serde_json::from_value(candidate["frozen_changes"].clone()).unwrap();
            assert_eq!(
                changes.apply(&source).unwrap()["pattern"],
                candidate["binding"]["result"]["pattern"]
            );
        }
    }
    assert_eq!(
        report["comparison"]["frontier"],
        json!([key("grid:0001", "light-sheet")])
    );
    let partial = serde_json::to_value(
        grid_request(2)
            .evaluate_with_fit_body(&pattern, Some(&fit))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(partial["design_search"]["omitted_designs"], 2);
    assert_eq!(partial["design_search"]["budget_exhausted"], true);
    assert_eq!(partial["design_search"]["complete_grid_enumeration"], false);
    assert_eq!(
        partial["comparison"]["frontier"],
        json!([key("grid:0001", "light-sheet")])
    );
    let other_identity =
        json!({"schema":"synthetic-flat-fit-body/1","source":"test:different-body"});
    let changed = request
        .evaluate_with_fit_body(
            &pattern,
            Some(&Context {
                body: &body,
                identity: &other_identity,
            }),
        )
        .unwrap();
    assert_ne!(report["request_sha256"], changed.request_sha256);
}

#[test]
fn missing_fit_body_never_falls_back_and_cannot_invoke_a_scenario() {
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let request = grid_request(4);
    let mut calls = 0;
    let report = request
        .evaluate_with(&pattern, &json!({"scenario":"test"}), |_, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap();
    assert_eq!(calls, 0);
    assert!(report.manufacturing.alternatives.iter().all(|r| matches!(
        r.binding,
        ftx_export::material_study::Outcome::Refused { .. }
    )));
    assert!(report.comparison.unwrap().frontier.is_empty());
}

#[test]
fn fit_gates_once_before_named_cases_and_expanded_work_is_reserved_before_callbacks() {
    use ftx_export::material_study::design::fit::Context;
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = ftx_pattern::validate::Measurements(json!({"declared_width":20}));
    let identity = json!({"schema":"synthetic-flat-fit-body/1","source":"test:design-grid"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let request = grid_request(4);
    let cases = vec![
        Case {
            key: "one".into(),
            scenario: json!({"case":1}),
            comparison: None,
        },
        Case {
            key: "two".into(),
            scenario: json!({"case":2}),
            comparison: None,
        },
    ];
    let mut calls = Vec::new();
    let report = request
        .evaluate_matrix_with_fit(&pattern, &cases, 20, Some(&fit), |case, key, pattern| {
            calls.push((
                case,
                key.to_owned(),
                pattern.raw()["pieces"][0]["vertices"]["b"][0]
                    .as_f64()
                    .unwrap(),
            ));
            Ok(json!({"case":case}))
        })
        .unwrap();
    assert_eq!(report.planned_evaluations, 20);
    assert_eq!(report.evaluations, 8);
    assert_eq!(calls.len(), 8);
    assert!(
        calls
            .iter()
            .all(|(_, _, width)| *width == 24.0 || *width == 28.0)
    );
    let mut calls = 0;
    let refused = request
        .evaluate_matrix_with_fit(&pattern, &cases, 19, Some(&fit), |_, _, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(refused.code, RefusalCode::BudgetExceeded);
    assert_eq!(calls, 0);
}

fn screened_grid_request(max_designs: usize, max_candidates: usize) -> Request {
    let mut request = grid_request(max_designs);
    request.design_grid.as_mut().unwrap().fit_screening =
        Some(ftx_export::material_study::design::grid::FitScreening { max_candidates });
    request.design_fit.as_mut().unwrap().fit_spec["points"][0]["target_mm"] = json!([7, 8, 9]);
    request
}

#[test]
fn fit_screening_reaches_a_late_candidate_and_replays_real_consumers_without_changing_prefix_mode()
{
    use ftx_export::material_study::design::fit::Context;
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let original = pattern.raw().clone();
    let body = ftx_pattern::validate::Measurements(json!({"declared_width":20}));
    let identity = json!({"source":"test:late-flat-fit-candidate"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let mut request = screened_grid_request(1, 4);
    request.comparison = None;
    let mut prefix = request.clone();
    prefix.design_grid.as_mut().unwrap().fit_screening = None;
    let mut prefix_calls = 0;
    let default = prefix
        .evaluate_with_fit(&pattern, &json!({}), Some(&fit), |_, _| {
            prefix_calls += 1;
            Ok(json!({}))
        })
        .unwrap();
    assert_eq!(prefix_calls, 0);
    let default = serde_json::to_value(default.manufacturing).unwrap();
    assert!(default["design_search"].get("fit_screening").is_none());
    assert_eq!(default["design_search"]["generated_designs"], 1);
    assert_eq!(default["design_search"]["omitted_designs"], 3);
    assert_eq!(default["alternatives"].as_array().unwrap().len(), 4);

    let mut calls = Vec::new();
    let result = request
        .evaluate_with_fit(&pattern, &json!({}), Some(&fit), |key, altered| {
            calls.push(key.to_owned());
            assert_eq!(
                altered.raw()["pieces"][0]["vertices"]["b"][0].as_f64(),
                Some(28.0)
            );
            assert_eq!(
                altered.raw()["pieces"][0]["vertices"]["b"][1].as_f64(),
                Some(0.0)
            );
            Ok(json!({"pattern_sha256":sha256_hex(&serde_json::to_vec(altered.raw()).unwrap())}))
        })
        .unwrap();
    assert_eq!(calls.len(), 2);
    let result = serde_json::to_value(result).unwrap();
    let report = &result["manufacturing"];
    let search = &report["design_search"];
    let screened = &search["fit_screening"];
    assert_eq!(search["generated_designs"], 1);
    assert_eq!(search["omitted_designs"], 3);
    assert_eq!(search["complete_grid_enumeration"], true);
    assert_eq!(search["budget_exhausted"], false);
    assert_eq!(screened["screened_designs"], 4);
    assert_eq!(screened["qualified_designs"], 1);
    assert_eq!(screened["rejected_designs"], 3);
    assert_eq!(screened["qualified_designs_omitted"], 0);
    assert_eq!(screened["unvisited_designs"], 0);
    for (i, screened) in screened["rows"].as_array().unwrap().iter().enumerate() {
        assert_eq!(screened["design"]["key"], format!("grid:{i:04}"));
        assert!(screened["design"].get("material_key").is_none());
        assert_eq!(
            screened["design"]["fit"]["result"]["body_identity"],
            identity
        );
        assert_eq!(screened["selected"], i == 2);
        assert_eq!(screened.get("refusal").is_none(), i == 2);
    }
    for material in ["light-sheet", "heavy-sheet"] {
        let selected = row(report, "grid:0002", material);
        assert_eq!(selected["design"]["validation"]["passed"], true);
        assert_eq!(selected["cutting"]["status"], "available");
        assert_eq!(selected["sewing"]["status"], "available");
        assert_eq!(selected["assembly_mass"]["status"], "available");
        let altered =
            Pattern::from_json(selected["design"]["altered_pattern_json"].as_str().unwrap())
                .unwrap();
        let changes: Changes = serde_json::from_value(selected["frozen_changes"].clone()).unwrap();
        assert_eq!(
            changes.apply(&altered).unwrap()["pattern"],
            selected["binding"]["result"]["pattern"]
        );
        let replay = CutPlan::build(
            &Pattern::new(selected["binding"]["result"]["pattern"].clone()),
            request.cutting.as_ref().unwrap(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(replay).unwrap(),
            selected["cutting"]["result"]
        );
        let simulation = result["simulations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["key"] == selected["key"])
            .unwrap();
        assert_eq!(
            simulation["evaluation"]["result"]["pattern_sha256"],
            selected["binding"]["result"]["pattern_sha256"]
        );
    }
    assert_eq!(pattern.raw(), &original);
}

#[test]
fn fit_screening_distinguishes_unvisited_qualified_omissions_and_a_complete_failed_domain() {
    use ftx_export::material_study::design::fit::Context;
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = ftx_pattern::validate::Measurements(json!({"declared_width":20}));
    let identity = json!({"source":"test:screened-domain"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let evaluate = |request: &Request| {
        serde_json::to_value(
            request
                .evaluate_with_fit_body(&pattern, Some(&fit))
                .unwrap(),
        )
        .unwrap()
    };
    let early = evaluate(&screened_grid_request(1, 2));
    assert_eq!(early["alternatives"].as_array().unwrap().len(), 2);
    assert_eq!(
        early["design_search"]["fit_screening"]["unvisited_designs"],
        2
    );
    assert_eq!(
        early["design_search"]["fit_screening"]["qualified_designs"],
        0
    );
    assert_eq!(early["design_search"]["complete_grid_enumeration"], false);
    let incumbent = evaluate(&screened_grid_request(1, 3));
    assert_eq!(
        incumbent["design_search"]["fit_screening"]["selected_designs"],
        1
    );
    assert_eq!(
        incumbent["design_search"]["fit_screening"]["unvisited_designs"],
        1
    );
    assert_eq!(incumbent["design_search"]["budget_exhausted"], true);
    assert_eq!(
        incumbent["comparison"]["frontier"],
        json!([key("grid:0002", "light-sheet")])
    );

    let mut request = screened_grid_request(1, 4);
    request.design_fit.as_mut().unwrap().fit_spec["points"][0]["target_mm"] = json!([3, 6, 10]);
    let omitted = evaluate(&request);
    let screening = &omitted["design_search"]["fit_screening"];
    assert_eq!(screening["qualified_designs"], 2);
    assert_eq!(screening["selected_designs"], 1);
    assert_eq!(screening["qualified_designs_omitted"], 1);
    assert_eq!(screening["complete_fit_screening"], true);
    assert_eq!(screening["all_qualified_designs_selected"], false);
    assert_eq!(screening["rows"][2]["selected"], false);
    assert!(screening["rows"][2].get("refusal").is_none());
    assert_eq!(omitted["design_search"]["complete_grid_enumeration"], false);
    assert_eq!(omitted["design_search"]["budget_exhausted"], true);

    request.design_fit.as_mut().unwrap().fit_spec["points"][0]["target_mm"] =
        json!([100, 101, 102]);
    let none = evaluate(&request);
    assert_eq!(
        none["design_search"]["fit_screening"]["rejected_designs"],
        4
    );
    assert_eq!(none["design_search"]["generated_designs"], 0);
    assert_eq!(none["design_search"]["complete_grid_enumeration"], true);
    assert_eq!(none["design_search"]["budget_exhausted"], false);
    assert_eq!(none["alternatives"].as_array().unwrap().len(), 2);
    assert_eq!(none["comparison"]["frontier"], json!([]));
}

#[test]
fn fit_screening_reuses_one_body_evaluation_across_materials_and_named_cases() {
    use ftx_export::material_study::design::fit::Context;
    use ftx_pattern::{BodyMeasure, instance::PyError};
    use std::cell::Cell;
    struct CountedBody {
        calls: Cell<usize>,
    }
    impl BodyMeasure for CountedBody {
        fn value(&self, _: &str) -> Result<f64, PyError> {
            self.calls.set(self.calls.get() + 1);
            Ok(20.0)
        }
        fn sigma(&self, _: &str) -> Option<f64> {
            None
        }
        fn landmark_h(&self, _: &str, _: &str) -> Result<f64, PyError> {
            Err(PyError::Key(
                "explicit vertex does not need a body landmark".into(),
            ))
        }
    }
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let body = CountedBody {
        calls: Cell::new(0),
    };
    let identity = json!({"source":"test:single-screening"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let mut request = screened_grid_request(1, 4);
    request.comparison = None;
    request
        .evaluate_with_fit_body(&pattern, Some(&fit))
        .unwrap();
    let two_material_reads = body.calls.replace(0);
    assert!(two_material_reads > 0);
    let mut one_material = request.clone();
    one_material.variants.truncate(1);
    one_material
        .evaluate_with_fit_body(&pattern, Some(&fit))
        .unwrap();
    assert_eq!(body.calls.replace(0), two_material_reads);
    let cases = ["one", "two"].map(|key| Case {
        key: key.into(),
        scenario: json!({}),
        comparison: None,
    });
    let result = request
        .evaluate_matrix_with_fit(&pattern, &cases, 8, Some(&fit), |_, _, _| Ok(json!({})))
        .unwrap();
    assert_eq!(result.evaluations, 4);
    assert_eq!(body.calls.get(), two_material_reads);
    let single = request
        .evaluate_with_fit_body(&pattern, Some(&fit))
        .unwrap();
    assert_eq!(
        serde_json::to_value(result.manufacturing.design_search).unwrap(),
        serde_json::to_value(single.design_search).unwrap()
    );
}

#[test]
fn fit_screening_requires_explicit_body_fit_and_reserves_unvisited_root_search_work() {
    use ftx_export::material_study::design::fit::Context;
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let request = screened_grid_request(1, 4);
    assert!(
        request
            .evaluate(&pattern)
            .unwrap_err()
            .message
            .contains("fitted-body")
    );
    let mut missing_fit = request.clone();
    missing_fit.design_fit = None;
    assert!(
        missing_fit
            .evaluate(&pattern)
            .unwrap_err()
            .message
            .contains("explicit design_fit")
    );
    let body = ftx_pattern::validate::Measurements(json!({"declared_width":20}));
    let identity = json!({"source":"test:screening-reservations"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    for max_candidates in [0, 257] {
        let mut invalid = request.clone();
        invalid
            .design_grid
            .as_mut()
            .unwrap()
            .fit_screening
            .as_mut()
            .unwrap()
            .max_candidates = max_candidates;
        assert!(
            invalid
                .evaluate_with_fit_body(&pattern, Some(&fit))
                .is_err()
        );
    }
    let mut costly = request;
    let grid = costly.design_grid.as_mut().unwrap();
    grid.variables[0].values_mm = (0..64).map(f64::from).collect();
    grid.fit_screening.as_mut().unwrap().max_candidates = 64;
    grid.after = vec![
        ftx_export::material_study::design::Operation::TrueSeam {
            seam: "side".into(),
            end: "start".into()
        };
        14
    ];
    let mut source = pattern.raw().clone();
    source["meta"]["annotation"] = json!("x".repeat(20_000));
    let mut calls = 0;
    let error = costly
        .evaluate_with_fit(&Pattern::new(source), &json!({}), Some(&fit), |_, _| {
            calls += 1;
            Ok(json!({}))
        })
        .unwrap_err();
    assert_eq!(error.code, RefusalCode::BudgetExceeded);
    assert!(error.message.contains("document-processing"));
    assert_eq!(calls, 0);
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
        {"op":"move","piece":"front","vertex":"b","dx_mm":-30.0,"dy_mm":40.0}]}]);
    input.as_object_mut().unwrap().remove("sewing");
    input["comparison"] =
        json!({"basis":"nominal","objectives":[{"metric":"stock_mass_kg","direction":"minimize"}]});
    let report = evaluate(input, &pattern);
    let crossed = row(&report, "crossed", "light-sheet");
    let altered =
        Pattern::from_json(crossed["design"]["altered_pattern_json"].as_str().unwrap()).unwrap();
    // The right and left edges cross at (0, 60), despite a positive signed area.
    assert_eq!(altered.instance("front").unwrap().area().unwrap(), 100.0);
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
