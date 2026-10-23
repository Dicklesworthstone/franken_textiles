//! Real material binding/manufacturing with declared synthetic callback readings tests matrix
//! decisions and failure isolation. These fixtures make no cloth-physics or measured-fit claim.
use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_export::material_study::{Request, comparison::Policy, matrix::Case};
use ftx_pattern::{Pattern, material_screen::Direction};
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-scenario-pattern.json");

fn pattern() -> Pattern {
    Pattern::from_json(PATTERN).unwrap()
}

fn policy() -> Policy {
    serde_json::from_value(json!({"basis":"nominal","objectives":[
        {"metric":"simulated_cloth_mass_kg","direction":"minimize"}]}))
    .unwrap()
}

fn request() -> Request {
    let raw: Value = serde_json::from_str(PATTERN).unwrap();
    let selection = &raw["fabric"]["material"];
    serde_json::from_value(json!({"schema":"ftx-material-study/1","variants":[
        {"key":"b","changes":{"schema":"ftx-material-bindings/1","default":selection}},
        {"key":"a","changes":{"schema":"ftx-material-bindings/1","default":selection}}],
        "cutting":{"copies":1,"gap_mm":1.,"edge_margin_mm":0.,"end_margin_mm":0.},
        "comparison":policy()}))
    .unwrap()
}

fn cases(count: usize) -> Vec<Case> {
    (0..count)
        .map(|index| Case {
            key: format!("case-{index}"),
            scenario: json!({
                "scenario_sha256":sha256_hex(format!("declared synthetic case {index}").as_bytes()),
                "body_geometry_sha256":"2".repeat(64),
                "declared_case":index,
            }),
            comparison: None,
        })
        .collect()
}

fn simulated(pattern: &Pattern, descriptor: &Value, mass: f64) -> Value {
    json!({"schema":"ftx-drape-scenario-result/1",
        "pattern_sha256":sha256_hex(&serde_json::to_vec(pattern.raw()).unwrap()),
        "scenario_sha256":descriptor["scenario_sha256"],
        "body_geometry_sha256":descriptor["body_geometry_sha256"],
        "metrics":{"simulation_gating_clear":true},
        "drape":{"settled":true,"gating":[],"mass_kg":mass}})
}

fn run(request: &Request, cases: &[Case], readings: &[[f64; 2]]) -> Value {
    serde_json::to_value(
        request
            .evaluate_matrix_with(&pattern(), cases, 64, |index, key, pattern| {
                Ok(simulated(
                    pattern,
                    &cases[index].scenario,
                    readings[index][usize::from(key == "b")],
                ))
            })
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn one_case_matches_existing_study_outputs_and_context_exactly() {
    let request = request();
    let cases = cases(1);
    let pattern = pattern();
    let single = serde_json::to_value(
        request
            .evaluate_with(&pattern, &cases[0].scenario, |key, pattern| {
                Ok(simulated(
                    pattern,
                    &cases[0].scenario,
                    if key == "a" { 1. } else { 2. },
                ))
            })
            .unwrap(),
    )
    .unwrap();
    let matrix = run(&request, &cases, &[[1., 2.]]);
    assert_eq!(matrix["manufacturing"], single["manufacturing"]);
    for field in ["request_sha256", "scenario", "simulations", "comparison"] {
        assert_eq!(matrix["cases"][0][field], single[field], "{field}");
    }
    assert_eq!(
        matrix["comparison"]["frontier"],
        single["comparison"]["frontier"]
    );
    assert_eq!(matrix["planned_evaluations"], 2);
    assert_eq!(matrix["evaluations"], 2);

    let smaller = request
        .evaluate_matrix_with(&pattern, &cases, 2, |_, key, pattern| {
            Ok(simulated(
                pattern,
                &cases[0].scenario,
                if key == "a" { 1. } else { 2. },
            ))
        })
        .unwrap();
    assert_ne!(json!(smaller.request_sha256), matrix["request_sha256"]);
    assert_eq!(
        json!(smaller.cases[0].request_sha256),
        single["request_sha256"]
    );
}

#[test]
fn ties_in_one_case_and_strict_improvement_in_another_establish_joint_dominance() {
    let request = request();
    let mut cases = cases(2);
    cases[0].key = "z-first".into();
    cases[1].key = "a-second".into();
    let mut calls = Vec::new();
    let report = request
        .evaluate_matrix_with(&pattern(), &cases, 4, |index, key, pattern| {
            calls.push((index, key.to_owned()));
            Ok(simulated(
                pattern,
                &cases[index].scenario,
                if index == 1 && key == "b" { 2. } else { 1. },
            ))
        })
        .unwrap();
    assert_eq!(
        calls,
        [
            (0, "a".to_owned()),
            (0, "b".to_owned()),
            (1, "a".to_owned()),
            (1, "b".to_owned())
        ]
    );
    let result = serde_json::to_value(report).unwrap();
    assert_eq!(
        result["comparison"]["case_order"],
        json!(["z-first", "a-second"])
    );
    assert_eq!(
        result["cases"][0]["comparison"]["frontier"],
        json!(["a", "b"])
    );
    assert_eq!(result["comparison"]["frontier"], json!(["a"]));
    assert_eq!(
        result["comparison"]["alternatives"][1]["dominated_by"],
        json!(["a"])
    );
    assert_eq!(
        result["manufacturing"]["alternatives"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(result["cases"][0].get("manufacturing").is_none());
}

#[test]
fn opposing_case_results_and_explicit_policy_directions_preserve_tradeoffs() {
    let request = request();
    let mut cases = cases(2);
    let result = run(&request, &cases, &[[1., 2.], [2., 1.]]);
    assert_eq!(result["cases"][0]["comparison"]["frontier"], json!(["a"]));
    assert_eq!(result["cases"][1]["comparison"]["frontier"], json!(["b"]));
    assert_eq!(result["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        result["comparison"]["incomparable"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let mut maximize = policy();
    maximize.objectives[0].direction = Direction::Maximize;
    cases[1].comparison = Some(maximize.clone());
    let reversed = run(&request, &cases, &[[1., 2.], [1., 2.]]);
    assert_eq!(reversed["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        reversed["comparison"]["policies"]["case-1"],
        json!(maximize)
    );
    let mut standalone = request.clone();
    standalone.comparison = Some(maximize);
    let expected = standalone
        .evaluate_with(&pattern(), &cases[1].scenario, |key, pattern| {
            Ok(simulated(
                pattern,
                &cases[1].scenario,
                if key == "a" { 1. } else { 2. },
            ))
        })
        .unwrap();
    assert_eq!(
        reversed["cases"][1]["request_sha256"],
        json!(expected.request_sha256)
    );
    assert_eq!(
        reversed["cases"][1]["comparison"],
        json!(expected.comparison)
    );
}

#[test]
fn a_case_constraint_or_callback_refusal_excludes_only_the_affected_joint_alternative() {
    let request = request();
    let mut cases = cases(2);
    cases[1].comparison = Some(
        serde_json::from_value(json!({"basis":"nominal",
        "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
        "constraints":[{"metric":"simulated_cloth_mass_kg","maximum":1.5}]}))
        .unwrap(),
    );
    let constrained = run(&request, &cases, &[[0.5, 1.], [2., 1.]]);
    assert_eq!(constrained["comparison"]["frontier"], json!(["b"]));
    assert_eq!(
        constrained["comparison"]["alternatives"][0]["cases"]["case-0"]["eligible"],
        true
    );
    assert_eq!(
        constrained["comparison"]["alternatives"][0]["cases"]["case-1"]["eligible"],
        false
    );
    let report = request
        .evaluate_matrix_with(&pattern(), &cases, 4, |index, key, pattern| {
            if index == 1 && key == "a" {
                Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "synthetic case unavailable",
                ))
            } else {
                Ok(simulated(pattern, &cases[index].scenario, 1.))
            }
        })
        .unwrap();
    let failed = serde_json::to_value(report).unwrap();
    assert_eq!(failed["comparison"]["frontier"], json!(["b"]));
    assert_eq!(failed["evaluations"], 4);
    assert_eq!(
        failed["cases"][0]["simulations"][0]["evaluation"]["status"],
        "available"
    );
    assert_eq!(
        failed["cases"][1]["simulations"][0]["evaluation"]["status"],
        "refused"
    );
    assert_eq!(
        failed["manufacturing"]["alternatives"][0]["cutting"]["status"],
        "available"
    );
}

#[test]
fn protocol_permission_is_case_specific_and_never_erases_another_cases_incompatibility() {
    let mut request = request();
    let record = request
        .variants
        .iter_mut()
        .find(|v| v.key == "b")
        .unwrap()
        .changes
        .default
        .as_mut()
        .unwrap()
        .record
        .as_mut()
        .unwrap();
    record.properties.get_mut("warp_stiffness").unwrap().method =
        "another synthetic protocol".into();
    let mut cases = cases(2);
    let mut mixed = policy();
    mixed.allow_mixed_protocols = true;
    cases[0].comparison = Some(mixed.clone());
    let result = run(&request, &cases, &[[1., 2.], [1., 2.]]);
    assert_eq!(result["cases"][0]["comparison"]["frontier"], json!(["a"]));
    assert_eq!(result["comparison"]["frontier"], json!(["a", "b"]));
    assert_eq!(
        result["comparison"]["incomparable"][0]["cases"],
        json!(["case-1"])
    );
    cases[1].comparison = Some(mixed);
    assert_eq!(
        run(&request, &cases, &[[1., 2.], [1., 2.]])["comparison"]["frontier"],
        json!(["a"])
    );
}

#[test]
fn actual_assembly_ranges_keep_overlap_unresolved_and_missing_bounds_unavailable() {
    let mut request = request();
    request.assembly_mass = Some(
        serde_json::from_value(json!({"schema":"ftx-assembly-mass/1",
        "inventory_complete":true,"extras":[]}))
        .unwrap(),
    );
    request.comparison = Some(
        serde_json::from_value(json!({"basis":"reported_bounds",
        "objectives":[{"metric":"assembly_mass_kg","direction":"minimize"}]}))
        .unwrap(),
    );
    for variant in &mut request.variants {
        let density = variant
            .changes
            .default
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("areal_density")
            .unwrap();
        if variant.key == "a" {
            density.value = 0.2;
            density.bounds = Some([0.1, 0.3]);
        } else {
            density.value = 0.3;
            density.bounds = Some([0.2, 0.4]);
        }
    }
    let cases = cases(2);
    let result = run(&request, &cases, &[[1., 2.], [1., 2.]]);
    assert_eq!(result["comparison"]["frontier"], json!(["a", "b"]));
    assert!(
        result["comparison"]["alternatives"][0]["cases"]["case-1"]["readings"]["assembly_mass_kg"]
            ["reported_bounds"]
            .is_array()
    );
    request
        .variants
        .iter_mut()
        .find(|v| v.key == "b")
        .unwrap()
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
    let missing = run(&request, &cases, &[[1., 2.], [1., 2.]]);
    assert_eq!(missing["comparison"]["frontier"], json!(["a"]));
    let reading = &missing["comparison"]["alternatives"][1]["cases"]["case-0"]["readings"]["assembly_mass_kg"];
    assert!(reading["value"].as_f64().unwrap() > 0.);
    assert!(reading.get("reported_bounds").is_none());
}

#[test]
fn missing_policies_preserve_requested_case_comparisons_without_synthesizing_an_aggregate() {
    let mut request = request();
    request.comparison = None;
    let mut cases = cases(2);
    cases[0].comparison = Some(policy());
    let result = run(&request, &cases, &[[1., 2.], [1., 2.]]);
    assert!(result.get("comparison").is_none());
    assert!(result["cases"][0]["comparison"].is_object());
    assert!(result["cases"][1].get("comparison").is_none());
    assert!(result["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("no explicit effective comparison")
    }));
    let mut unused = policy();
    unused.objectives.clear();
    request.comparison = Some(unused);
    cases[1].comparison = Some(policy());
    assert_eq!(
        run(&request, &cases, &[[1., 2.], [1., 2.]])["comparison"]["frontier"],
        json!(["a"])
    );
}

#[test]
fn binding_failures_retain_all_case_rows_without_spending_callback_attempts() {
    let mut request = request();
    request
        .variants
        .iter_mut()
        .find(|v| v.key == "a")
        .unwrap()
        .changes
        .default =
        Some(serde_json::from_value(json!({"id":"missing-matrix-test-material"})).unwrap());
    let cases = cases(2);
    let mut keys = Vec::new();
    let report = request
        .evaluate_matrix_with(&pattern(), &cases, 4, |index, key, pattern| {
            keys.push(key.to_owned());
            Ok(simulated(pattern, &cases[index].scenario, 1.))
        })
        .unwrap();
    assert_eq!(keys, ["b", "b"]);
    assert_eq!(report.planned_evaluations, 4);
    assert_eq!(report.evaluations, 2);
    let result = serde_json::to_value(report).unwrap();
    for case in result["cases"].as_array().unwrap() {
        assert_eq!(case["simulations"][0]["key"], "a");
        assert_eq!(case["simulations"][0]["evaluation"]["status"], "refused");
    }
    assert_eq!(result["comparison"]["frontier"], json!(["b"]));
}

#[test]
fn aggregate_work_and_all_case_policies_are_validated_before_callbacks() {
    let request = request();
    let mut named = cases(2);
    let refused = request
        .evaluate_matrix_with::<Value>(&pattern(), &named, 3, |_, _, _| {
            panic!("over-budget matrix must not call a production consumer")
        })
        .unwrap_err();
    assert_eq!(refused.code, RefusalCode::BudgetExceeded);
    for maximum in [0, 65] {
        assert!(
            request
                .evaluate_matrix_with::<Value>(&pattern(), &named, maximum, |_, _, _| {
                    panic!("invalid allowance must be preflighted")
                })
                .is_err()
        );
    }
    named[1].key = named[0].key.clone();
    assert!(
        request
            .evaluate_matrix_with::<Value>(&pattern(), &named, 4, |_, _, _| {
                panic!("duplicate case must be preflighted")
            })
            .is_err()
    );
    named[1].key = " ".into();
    assert!(
        request
            .evaluate_matrix_with::<Value>(&pattern(), &named, 4, |_, _, _| {
                panic!("empty case must be preflighted")
            })
            .is_err()
    );
    for named in [Vec::new(), cases(9)] {
        assert!(
            request
                .evaluate_matrix_with::<Value>(&pattern(), &named, 64, |_, _, _| {
                    panic!("case count must be preflighted")
                })
                .is_err()
        );
    }
    let mut named = cases(2);
    let mut invalid = policy();
    invalid.objectives.clear();
    named[1].comparison = Some(invalid);
    assert!(
        request
            .evaluate_matrix_with::<Value>(&pattern(), &named, 4, |_, _, _| {
                panic!("later invalid policy must refuse before the first callback")
            })
            .is_err()
    );
}
