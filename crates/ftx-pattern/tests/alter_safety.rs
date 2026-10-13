//! Refusals on inputs the reference silently saturates/ignores, alongside its valid worked repairs.
//! The captured reference fixture stays unchanged; these are explicit plan §11 safety deviations.

use ftx_core::RefusalCode;
use ftx_pattern::Pattern;
use ftx_pattern::alter::{cap_ease, move_vertex, true_seam};
use serde_json::{Value, json};

fn rectangles(a_height: f64) -> Value {
    let piece = |id: &str, h| {
        json!({
            "id": id, "vertices": {"a": [0, 0], "b": [100, 0], "c": [100, h], "d": [0, h]},
            "edges": [
                {"id": "bottom", "from": "a", "to": "b"},
                {"id": "right", "from": "b", "to": "c"},
                {"id": "top", "from": "c", "to": "d"},
                {"id": "left", "from": "d", "to": "a"}
            ]
        })
    };
    json!({"schema": "fabric-pattern/1", "units": "mm",
        "pieces": [piece("a", a_height), piece("b", 60.0)],
        "seams": [{"id": "s", "a": "a/right", "b": "b/left"}]})
}

fn bolero() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/oracle/pattern/drafts.json");
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap()["input"]["pattern"]
        .clone()
}

fn cap_ids() -> Vec<String> {
    vec!["armhole_f_R".into(), "armhole_b_R".into()]
}

#[test]
fn impossible_seam_is_not_a_successfully_trued_bound() {
    let doc = rectangles(500.0);
    let before = doc.clone();
    let err = true_seam(&doc, "s", "end").unwrap_err();
    assert_eq!(err.code, RefusalCode::NoRoot);
    assert!(err.message.contains("[-60, 60]"));
    assert_eq!(doc, before);
    // An actual endpoint root is allowed, not mistaken for an unbracketed target.
    let at_bound = true_seam(&rectangles(120.0), "s", "end").unwrap();
    assert_eq!(at_bound["pieces"][1]["vertices"]["a"], json!([0.0, -60.0]));
}

#[test]
fn invalid_ratios_bands_and_duplicate_seams_are_refused() {
    for value in [json!(0), json!(-1), json!(null), json!("oops")] {
        let mut doc = rectangles(50.0);
        doc["seams"][0]["ratio"] = value;
        assert!(true_seam(&doc, "s", "end").is_err());
    }
    for value in [
        json!([1]),
        json!([1, 2, 3]),
        json!([2, 1]),
        json!([0, 1]),
        json!(["a", 1]),
    ] {
        let mut doc = rectangles(50.0);
        doc["seams"][0]["ratio_band"] = value;
        assert!(true_seam(&doc, "s", "end").is_err());
    }
    let mut doc = rectangles(50.0);
    doc["seams"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "s", "a": "a/top", "b": "b/top"}));
    assert!(
        true_seam(&doc, "s", "end")
            .unwrap_err()
            .message
            .contains("duplicate seam")
    );
}

#[test]
fn zero_tangent_and_arithmetic_overflow_cannot_become_json_nulls() {
    let mut doc = rectangles(50.0);
    doc["pieces"][1]["vertices"]["d"] = json!([0, 0]);
    assert!(true_seam(&doc, "s", "end").is_err());
    doc["pieces"][1]["vertices"]["d"] = json!([f64::MAX, f64::MAX]);
    assert_eq!(
        true_seam(&doc, "s", "end").unwrap_err().code,
        RefusalCode::NonFinite
    );
}

#[test]
fn impossible_cap_ratios_and_wrong_seam_sets_are_refused() {
    let doc = bolero();
    let before = doc.clone();
    for target in [0.1, 10.0] {
        assert_eq!(
            cap_ease(&doc, "sleeve", &cap_ids(), target)
                .unwrap_err()
                .code,
            RefusalCode::NoRoot
        );
    }
    for target in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(cap_ease(&doc, "sleeve", &cap_ids(), target).is_err());
    }
    for seams in [
        vec![],
        vec!["armhole_f_R", "typo"],
        vec!["armhole_f_R", "armhole_b_L"],
        vec!["side_R"],
    ] {
        let ids: Vec<String> = seams.into_iter().map(str::to_string).collect();
        assert!(cap_ease(&doc, "sleeve", &ids, 1.06).is_err());
    }
    assert_eq!(doc, before);
}

#[test]
fn worked_bolero_repair_fixes_both_sides_without_editing_mirrored_vertices() {
    let damaged = move_vertex(&bolero(), "front", "side_hem", 0.0, -10.0).unwrap();
    let out = true_seam(&damaged, "side_R", "start").unwrap();
    let back = out["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "back")
        .unwrap();
    assert_eq!(back["vertices"]["side_hem"], json!([159.518, -19.978]));
    let before_rows = ftx_pattern::validate::seam_walk(&Pattern::new(damaged.clone()), 3.0);
    let after_rows = ftx_pattern::validate::seam_walk(&Pattern::new(out), 3.0);
    for sid in ["side_R", "side_L"] {
        assert_eq!(
            before_rows.iter().find(|r| r.seam == sid).unwrap().status,
            "FAIL"
        );
        assert_eq!(
            after_rows.iter().find(|r| r.seam == sid).unwrap().status,
            "PASS"
        );
    }
    assert!(true_seam(&damaged, "side_L", "start").is_err());
}

#[test]
fn cap_on_side_b_and_reversed_argument_order_match_the_worked_result() {
    let mut doc = bolero();
    let ids = cap_ids();
    for seam in doc["seams"].as_array_mut().unwrap() {
        if ids.iter().any(|id| seam["id"] == *id) {
            let a = seam["a"].clone();
            seam["a"] = seam["b"].clone();
            seam["b"] = a;
        }
    }
    let out = cap_ease(&doc, "sleeve", &ids, 1.06).unwrap();
    let sleeve = out["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "sleeve")
        .unwrap();
    assert!((sleeve["vertices"]["top"][1].as_f64().unwrap() - 2.961448104437153).abs() < 1e-9);
    let reversed = cap_ease(&doc, "sleeve", &[ids[1].clone(), ids[0].clone()], 1.06).unwrap();
    assert_eq!(out["pieces"], reversed["pieces"]);
    assert!(Pattern::new(out).structure_errors().is_empty());
}
