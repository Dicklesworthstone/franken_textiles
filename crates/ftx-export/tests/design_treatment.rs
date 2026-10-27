//! Treatment-aware design choices use synthetic qualified observations, actual pattern geometry,
//! fit gates and manufacturing consumers. They do not establish measured garment performance.

use ftx_core::{RefusalCode, receipt::sha256_hex};
use ftx_export::{
    cutting::CutPlan,
    material_study::{Request, design::fit::Context},
};
use ftx_pattern::{
    Pattern,
    material_binding::Changes,
    material_catalog::Selection,
    material_treatment::{self, geometry},
    materials::{CATALOG_SCHEMA, Catalog},
    validate::Measurements,
};
use serde_json::{Value, json};

fn catalog() -> Catalog {
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
}

fn selection(after: bool) -> Selection {
    let conditions = json!({"specimen":"synthetic study sheet",
        "conditioning":if after { "after" } else { "before" }});
    let quantity = |value, unit| {
        json!({"value":value,"unit":unit,"evidence":"estimate",
        "source":"test","method":"Synthetic study fixture, not a measured textile",
        "conditions":conditions})
    };
    let record = json!({"id":if after { "synthetic/study-after" } else { "synthetic/study-before" },
        "name":"Synthetic treatment study sheet","form":"woven",
        "structure":if after {json!({"treatment_process":"synthetic treatment","treatment_cycles":"1"})} else {json!({})},
        "properties":{"width":quantity(if after {0.4} else {0.5},"m"),
            "areal_density":quantity(if after {0.3} else {0.2},"kg/m2"),
            "thickness":quantity(if after {2.0} else {1.0},"mm")},
        "sources":{"test":{"reference":"test:design-treatment","accessed_on":"2026-10-27",
            "note":"Independent synthetic observations for software tests"}}});
    catalog()
        .resolve(&Selection {
            id: record["id"].as_str().unwrap().into(),
            record: Some(serde_json::from_value(record).unwrap()),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection()
}

fn transition() -> Value {
    let quantity = |value| {
        json!({"value":value,"unit":"1","evidence":"estimate",
        "source":"test","method":"Synthetic freely treated directional dimensions",
        "conditions":{"specimen":"synthetic study sheet","process":"synthetic treatment",
            "cycles":"1","conditioning_before":"before","conditioning_after":"after"}})
    };
    json!({"schema":material_treatment::SCHEMA,"before":selection(false),"after":selection(true),
        "specimen":"synthetic study sheet","process":"synthetic treatment","cycles":1,
        "conditioning_before":"before","conditioning_after":"after",
        "shrinkage_warp":quantity(0.1),"shrinkage_weft":quantity(0.2),"allow_estimates":true,
        "sources":{"test":{"reference":"test:design-treatment-dimensions","accessed_on":"2026-10-27",
            "note":"Synthetic dimensional response; no empirical garment claim"}}})
}

fn operation(mode: &str) -> Value {
    json!({"op":"treatment","request":{"schema":geometry::SCHEMA,"mode":mode,
        "transitions":{"wash":transition()},"pieces":{"front":"wash","back":"wash"}}})
}

fn pattern() -> Pattern {
    let mut raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    raw["fabric"]["material"] = serde_json::to_value(selection(false)).unwrap();
    raw["meta"]["sa_mm"] = json!(0.0);
    for piece in raw["pieces"].as_array_mut().unwrap() {
        piece["vertices"] = json!({"a":[0.,0.],"b":[100.,0.],"c":[100.,200.],"d":[0.,200.]});
    }
    Pattern::new(raw)
}

fn request() -> Value {
    json!({"schema":"ftx-design-study/1",
        "designs":[{"key":"treated","operations":[operation("treat")]},
            {"key":"compensated","operations":[operation("compensate")]},
            {"key":"recovered","operations":[operation("compensate"),operation("treat")]}],
        "variants":[{"key":"source","changes":{"schema":"ftx-material-bindings/1"}}],
        "cutting":{"copies":1,"gap_mm":2.,"edge_margin_mm":0.,"end_margin_mm":0.,"seam_allowance_mm":0.},
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[],"seam_allowance_mm":0.},
        "comparison":{"basis":"nominal","objectives":[{"metric":"assembly_mass_kg","direction":"minimize"}]}})
}

fn evaluate(input: &Value, pattern: &Pattern, fit: Option<&Context<'_>>) -> Value {
    let request: Request = serde_json::from_value(input.clone()).unwrap();
    serde_json::to_value(request.evaluate_with_fit_body(pattern, fit).unwrap()).unwrap()
}

fn row<'a>(report: &'a Value, design: &str, material: &str) -> &'a Value {
    report["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["design"]["key"] == design && row["design"]["material_key"] == material)
        .unwrap()
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[test]
fn treatment_compensation_and_recovery_recompute_real_cut_and_mass_with_exact_replay() {
    let source = pattern();
    let original = source.raw().clone();
    let input = request();
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let report = evaluate(&input, &source, None);
    assert_eq!(report, evaluate(&input, &source, None));
    assert_eq!(report["geometry_changed"], true);
    assert!(
        row(&report, "baseline", "source")["design"]
            .get("treatments")
            .is_none()
    );
    for (design, area, density, treatments, state) in [
        ("baseline", 0.04, 0.2, 0, "before"),
        ("treated", 0.04 * 0.72, 0.3, 1, "after"),
        ("compensated", 0.04 / 0.72, 0.2, 1, "before"),
        ("recovered", 0.04, 0.3, 2, "after"),
    ] {
        let candidate = row(&report, design, "source");
        assert_eq!(candidate["binding"]["status"], "available", "{candidate}");
        close(
            candidate["metrics"]["external_cut_area_m2"]
                .as_f64()
                .unwrap(),
            area,
        );
        close(
            candidate["metrics"]["assembly_mass_kg"].as_f64().unwrap(),
            area * density,
        );
        assert_eq!(
            candidate["design"]["treatments"]
                .as_array()
                .map_or(0, Vec::len),
            treatments
        );
        let bound = &candidate["binding"]["result"];
        let altered = Pattern::from_json(
            candidate["design"]["altered_pattern_json"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let changes: Changes = serde_json::from_value(candidate["frozen_changes"].clone()).unwrap();
        assert_eq!(changes.apply(&altered).unwrap(), *bound);
        let final_pattern = Pattern::from_json(bound["pattern_json"].as_str().unwrap()).unwrap();
        assert_eq!(
            candidate["cutting"]["result"],
            serde_json::to_value(
                CutPlan::build(&final_pattern, typed.cutting.as_ref().unwrap()).unwrap()
            )
            .unwrap()
        );
        for applied in candidate["design"]["treatments"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let treatment = &applied["report"];
            let source =
                Pattern::from_json(treatment["source_pattern_json"].as_str().unwrap()).unwrap();
            let request: geometry::Input =
                serde_json::from_str(treatment["frozen_request_json"].as_str().unwrap()).unwrap();
            assert_eq!(request.apply(&source).unwrap(), *treatment);
            assert_eq!(
                treatment["pattern_sha256"],
                sha256_hex(treatment["pattern_json"].as_str().unwrap().as_bytes())
            );
        }
        let expected = transition()[state]["expected_sha256"].clone();
        assert!(
            bound["pieces"]
                .as_array()
                .unwrap()
                .iter()
                .all(|piece| piece["material_sha256"] == expected)
        );
    }
    assert_eq!(source.raw(), &original);
}

#[test]
fn zero_dimensional_treatment_changes_material_mass_without_claiming_geometry_changed() {
    let source = pattern();
    let mut input = request();
    input["designs"].as_array_mut().unwrap().truncate(1);
    let treatment = &mut input["designs"][0]["operations"][0]["request"]["transitions"]["wash"];
    treatment["shrinkage_warp"]["value"] = json!(0.0);
    treatment["shrinkage_weft"]["value"] = json!(0.0);
    let report = evaluate(&input, &source, None);
    let candidate = row(&report, "treated", "source");
    assert_eq!(candidate["binding"]["status"], "available");
    assert_eq!(candidate["design"]["geometry_changed"], false);
    assert_eq!(report["geometry_changed"], false);
    close(
        candidate["metrics"]["assembly_mass_kg"].as_f64().unwrap(),
        0.04 * 0.3,
    );
    assert_ne!(
        candidate["design"]["altered_pattern_sha256"],
        report["source_pattern_sha256"]
    );
}

#[test]
fn final_material_bindings_preserve_last_treatment_state_and_allow_unrelated_lining() {
    let mut raw = pattern().raw().clone();
    let mut lining = raw["pieces"][0].clone();
    lining["id"] = json!("lining");
    raw["pieces"].as_array_mut().unwrap().push(lining);
    let source = Pattern::new(raw);
    let mut input = request();
    input["variants"] = json!([
        {"key":"replace-treated","changes":{"schema":"ftx-material-bindings/1","pieces":{"front":selection(false)}}},
        {"key":"replace-lining","changes":{"schema":"ftx-material-bindings/1","pieces":{"lining":selection(true)}}},
        {"key":"replace-compensated","changes":{"schema":"ftx-material-bindings/1","pieces":{"front":selection(true)}}}
    ]);
    let typed: Request = serde_json::from_value(input).unwrap();
    let mut evaluated = std::collections::BTreeMap::new();
    let scenarios = typed
        .evaluate_with(
            &source,
            &json!({"source":"synthetic callback identity"}),
            |key, pattern| {
                let hash = sha256_hex(&serde_json::to_vec(pattern.raw()).unwrap());
                evaluated.insert(key.to_owned(), hash.clone());
                Ok(json!({"pattern_sha256":hash}))
            },
        )
        .unwrap();
    let report = serde_json::to_value(scenarios.manufacturing).unwrap();
    for design in ["treated", "recovered"] {
        let refused = row(&report, design, "replace-treated");
        assert_eq!(refused["binding"]["status"], "refused");
        assert!(
            refused["binding"]["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("qualified treatment state")
        );
        assert_eq!(refused["cutting"]["status"], "refused");
        assert!(refused["metrics"]["assembly_mass_kg"].is_null());
        assert_eq!(
            row(&report, design, "replace-lining")["binding"]["status"],
            "available"
        );
    }
    // Compensation promises BEFORE, so an identical frozen BEFORE rebind remains valid.
    assert_eq!(
        row(&report, "compensated", "replace-treated")["binding"]["status"],
        "available"
    );
    assert_eq!(
        row(&report, "baseline", "replace-treated")["binding"]["status"],
        "available"
    );
    assert_eq!(
        row(&report, "compensated", "replace-compensated")["binding"]["status"],
        "refused"
    );
    assert_eq!(
        row(&report, "treated", "replace-compensated")["binding"]["status"],
        "available"
    );
    assert_eq!(evaluated.len(), 9);
    for candidate in report["alternatives"].as_array().unwrap() {
        let actual = evaluated.get(candidate["key"].as_str().unwrap());
        if candidate["binding"]["status"] == "available" {
            assert_eq!(
                actual.map(String::as_str),
                candidate["binding"]["result"]["pattern_sha256"].as_str()
            );
        } else {
            assert!(
                actual.is_none(),
                "a refused treatment reached the scenario consumer"
            );
        }
    }
}

#[test]
fn differential_treatment_fails_actual_seams_and_stale_requests_preserve_baseline() {
    let source = pattern();
    let mut input = request();
    input["designs"].as_array_mut().unwrap().truncate(1);
    input["designs"][0]["operations"][0]["request"]["pieces"]
        .as_object_mut()
        .unwrap()
        .remove("back");
    let report = evaluate(&input, &source, None);
    let failed = row(&report, "treated", "source");
    assert_eq!(
        failed["design"]["treatments"][0]["report"]["seams_after"][0]["status"],
        "FAIL"
    );
    assert_eq!(failed["design"]["validation"]["passed"], false);
    assert_eq!(failed["binding"]["status"], "refused");
    assert_eq!(
        row(&report, "baseline", "source")["cutting"]["status"],
        "available"
    );
    let mut stale = request();
    stale["designs"].as_array_mut().unwrap().truncate(1);
    stale["designs"][0]["operations"][0]["request"]["transitions"]["wash"]["before"]["record"]["name"] =
        json!("Different source lot with an old lock");
    let report = evaluate(&stale, &source, None);
    assert_eq!(
        row(&report, "treated", "source")["binding"]["status"],
        "refused"
    );
    assert_eq!(
        row(&report, "baseline", "source")["binding"]["status"],
        "available"
    );
    let mut fixed = input;
    fixed["schema"] = json!("ftx-material-study/1");
    let fixed: Request = serde_json::from_value(fixed).unwrap();
    let refused = fixed.evaluate(&source).unwrap_err();
    assert_eq!(refused.code, RefusalCode::InvalidDocument);
    assert!(refused.message.contains("keeps geometry fixed"));
}

fn fitted_request() -> Value {
    let mut input = request();
    input["design_fit"] = json!({"fit_spec":{"schema":"fabric-fit/1","points":[
        {"id":"finished-length","kind":"vertical","instance":"front","from":"a","to":"d",
            "body":"desired_length","target_mm":[-0.0001,0.,0.0001]}]}});
    input
}

fn grid_request() -> Value {
    let mut input = fitted_request();
    input.as_object_mut().unwrap().remove("designs");
    input["design_grid"] = json!({"variables":[{"key":"length","values_mm":[0.,20.,40.],
        "sites":[{"piece":"front","axis":"y","at_mm":50.,"multiplier":1.},
            {"piece":"back","axis":"y","at_mm":50.,"multiplier":1.}]}],
        "after":[operation("treat")],"max_designs":3});
    input
}

#[test]
fn flat_fit_and_grid_screening_use_the_final_treated_dimensions() {
    let source = pattern();
    let body = Measurements(json!({"desired_length":180.}));
    let identity = json!({"source":"synthetic post-treatment fit target"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let report = evaluate(&fitted_request(), &source, Some(&fit));
    assert_eq!(
        row(&report, "baseline", "source")["design"]["fit"]["result"]["passed"],
        false
    );
    assert_eq!(
        row(&report, "treated", "source")["design"]["fit"]["result"]["passed"],
        true
    );
    assert_eq!(
        row(&report, "compensated", "source")["binding"]["status"],
        "refused"
    );
    let body = Measurements(json!({"desired_length":198.}));
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    for screened in [false, true] {
        let mut input = grid_request();
        if screened {
            input["design_grid"]["max_designs"] = json!(1);
            input["design_grid"]["fit_screening"] = json!({"max_candidates":3});
        }
        let report = evaluate(&input, &source, Some(&fit));
        let chosen = row(&report, "grid:0001", "source");
        assert_eq!(chosen["design"]["variables_mm"]["length"], 20.);
        assert_eq!(chosen["design"]["fit"]["result"]["passed"], true);
        assert_eq!(chosen["design"]["treatments"][0]["operation_index"], 2);
        close(
            chosen["binding"]["result"]["pattern"]["pieces"][0]["vertices"]["d"][1]
                .as_f64()
                .unwrap(),
            198.,
        );
        // The retained intermediate is the actual pre-treatment cutting geometry, not the washed result.
        let before: Value = serde_json::from_str(
            chosen["design"]["treatments"][0]["report"]["source_pattern_json"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        close(
            before["pieces"][0]["vertices"]["d"][1].as_f64().unwrap(),
            220.,
        );
        if screened {
            assert_eq!(
                report["design_search"]["fit_screening"]["screened_designs"],
                3
            );
            assert_eq!(
                report["design_search"]["fit_screening"]["selected_designs"],
                1
            );
        }
    }
}

fn adaptive_request(evaluations: usize) -> Value {
    let mut input = fitted_request();
    input.as_object_mut().unwrap().remove("designs");
    input["design_adaptive"] = json!({"objective":"fit_band_violation_mm",
        "variables":[{"key":"length","bounds_mm":[0.,40.],"initial_mm":0.,
            "initial_step_mm":20.,"minimum_step_mm":0.001,
            "sites":[{"piece":"front","axis":"y","at_mm":50.,"multiplier":1.},
                {"piece":"back","axis":"y","at_mm":50.,"multiplier":1.}]}],
        "after":[operation("treat")],"max_evaluations":evaluations,"max_designs":1});
    input
}

#[test]
fn adaptive_search_finds_pre_treatment_ease_by_the_actual_post_treatment_fit() {
    let source = pattern();
    let body = Measurements(json!({"desired_length":198.}));
    let identity = json!({"source":"synthetic adaptive treatment fit"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let report = evaluate(&adaptive_request(16), &source, Some(&fit));
    let selected = report["design_search"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|trial| trial["selected"] == true)
        .unwrap();
    assert_eq!(selected["design"]["variables_mm"]["length"], 20.);
    close(
        selected["raw_fit"]["result"][0]["finished_mm"]
            .as_f64()
            .unwrap(),
        198.,
    );
    assert_eq!(selected["max_fit_band_violation_mm"], 0.);
    let candidate = row(
        &report,
        selected["design"]["key"].as_str().unwrap(),
        "source",
    );
    assert_eq!(candidate["cutting"]["status"], "available");
    assert_eq!(candidate["design"]["fit"]["result"]["passed"], true);
    assert_eq!(
        candidate["design"]["treatments"],
        selected["design"]["treatments"]
    );
}

#[test]
fn authored_screened_and_adaptive_treatment_work_is_reserved_before_any_consumer() {
    let source = pattern();
    let body = Measurements(json!({"desired_length":198.}));
    let identity = json!({"source":"synthetic bounded treatment search"});
    let fit = Context {
        body: &body,
        identity: &identity,
    };
    let mut authored = request();
    authored["designs"] = json!(
        (0..7)
            .map(|i| json!({"key":format!("candidate-{i}"),
        "operations":vec![operation("treat");9]}))
            .collect::<Vec<_>>()
    );
    authored["variants"] = json!(
        (0..4)
            .map(|i| json!({"key":format!("source-{i}"),
        "changes":{"schema":"ftx-material-bindings/1"}}))
            .collect::<Vec<_>>()
    );
    let mut screened = grid_request();
    screened["design_grid"]["variables"][0]["values_mm"] = json!((0..16).collect::<Vec<_>>());
    let mut width = screened["design_grid"]["variables"][0].clone();
    width["key"] = json!("width");
    for site in width["sites"].as_array_mut().unwrap() {
        site["axis"] = json!("x");
    }
    screened["design_grid"]["variables"]
        .as_array_mut()
        .unwrap()
        .push(width);
    screened["design_grid"]["max_designs"] = json!(1);
    screened["design_grid"]["fit_screening"] = json!({"max_candidates":256});
    for input in [authored, screened, adaptive_request(256)] {
        let typed: Request = serde_json::from_value(input).unwrap();
        let mut calls = 0;
        let refusal = typed
            .evaluate_with_fit(
                &source,
                &json!({"synthetic":"budget"}),
                Some(&fit),
                |_, _| {
                    calls += 1;
                    Ok(json!({}))
                },
            )
            .unwrap_err();
        assert_eq!(refusal.code, RefusalCode::BudgetExceeded);
        assert!(
            refusal.message.contains("treatment") && refusal.message.contains("geometry work"),
            "{refusal}"
        );
        assert_eq!(calls, 0);
    }
}

#[test]
fn large_source_records_repeated_on_many_panels_are_counted_before_report_allocation() {
    let mut raw = pattern().raw().clone();
    let panel = raw["pieces"][0].clone();
    raw["pieces"] = json!(
        (0..1000)
            .map(|i| {
                let mut piece = panel.clone();
                piece["id"] = json!(format!("panel-{i}"));
                piece
            })
            .collect::<Vec<_>>()
    );
    raw["seams"] = json!([]);
    let source = Pattern::new(raw);
    let mut after = selection(true);
    after.expected_sha256 = None;
    after
        .record
        .as_mut()
        .unwrap()
        .structure
        .insert("additional_source_description".into(), "x".repeat(50_000));
    let after = catalog().resolve(&after).unwrap().frozen_selection();
    let mut input = request();
    input["designs"].as_array_mut().unwrap().truncate(1);
    let treatment = &mut input["designs"][0]["operations"][0]["request"];
    treatment["transitions"]["wash"]["after"] = serde_json::to_value(after).unwrap();
    treatment["pieces"] = json!(
        (0..1000)
            .map(|i| (format!("panel-{i}"), "wash"))
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    let typed: Request = serde_json::from_value(input).unwrap();
    let refusal = typed.evaluate(&source).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::BudgetExceeded);
    assert!(refusal.message.contains("256 MiB"), "{refusal}");
}
