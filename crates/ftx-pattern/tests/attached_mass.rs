//! Shared source-group mass and physical-instance allocation checks; inputs are synthetic.
use ftx_pattern::{
    Pattern,
    material_mass::{Attachments, Extra},
};
use serde_json::{Value, json};

fn pattern() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap()
}
fn extra() -> Value {
    json!({"id":"closure-set","mass":{"value":0.03,"unit":"kg","bounds":[0.028,0.032],
        "evidence":"estimate","source":"test","method":"Synthetic total retained group"},
        "sources":{"test":{"reference":"test:attached-mass","accessed_on":"2026-10-20","note":"Synthetic software input"}},"allow_estimates":true})
}
fn attached() -> Value {
    json!({"schema":"ftx-attached-mass/1","groups":[{"extra":extra(),"targets":[
        {"instance":"front","vertex":"a","fraction":0.25},
        {"instance":"back","vertex":"a","fraction":0.75}]}]})
}

#[test]
fn shared_extra_retains_source_bounds_and_independent_estimate_consent() {
    let valid: Extra = serde_json::from_value(extra()).unwrap();
    assert_eq!(valid.evaluate().unwrap(), (0.03, Some([0.028, 0.032])));
    assert!(valid.same_observation(&valid.clone()).unwrap());
    let mut bad = valid.clone();
    bad.allow_estimates = false;
    assert!(bad.evaluate().is_err());
    assert!(!valid.same_observation(&bad).unwrap());
    bad = valid.clone();
    bad.mass = None;
    assert!(bad.evaluate().unwrap_err().message.contains("unknown"));
    bad = valid.clone();
    bad.mass.as_mut().unwrap().unit = "g".into();
    assert!(bad.evaluate().is_err()); // kg contract is unchanged; no hidden per-item/unit inference.
    bad = valid.clone();
    bad.sources.clear();
    assert!(bad.evaluate().is_err());
    bad = valid;
    bad.mass.as_mut().unwrap().bounds = Some([0.04, 0.05]);
    assert!(bad.evaluate().is_err());
}

#[test]
fn explicit_targets_allocate_one_group_and_require_physical_instance_names() {
    let mut raw = pattern();
    raw["attached_mass"] = attached();
    let parsed = Attachments::from_pattern(&Pattern::new(raw.clone()))
        .unwrap()
        .unwrap();
    assert_eq!(parsed.groups.len(), 1);
    assert_eq!(
        parsed.groups[0]
            .targets
            .iter()
            .map(|t| t.fraction)
            .sum::<f64>(),
        1.0
    );
    raw["pieces"][0]["cut"] = json!({"qty":2});
    raw["seams"] = json!([]);
    assert!(Attachments::from_pattern(&Pattern::new(raw.clone())).is_err());
    raw["attached_mass"]["groups"][0]["targets"] = json!([
        {"instance":"front.R","vertex":"a","fraction":0.5},
        {"instance":"front.L","vertex":"a","fraction":0.5}]);
    assert!(Attachments::from_pattern(&Pattern::new(raw)).is_ok());
}

#[test]
fn unknown_mass_stays_a_structural_inventory_while_invalid_allocations_refuse() {
    let mut raw = pattern();
    raw["attached_mass"] = attached();
    raw["attached_mass"]["groups"][0]["extra"]["mass"] = Value::Null;
    let parsed = Attachments::from_pattern(&Pattern::new(raw.clone()))
        .unwrap()
        .unwrap();
    assert!(parsed.groups[0].extra.evaluate().is_err());
    for invalid in [
        json!([{"instance":"front","vertex":"a","fraction":0.9}]),
        json!([{"instance":"front","vertex":"a","fraction":0.5},{"instance":"front","vertex":"a","fraction":0.5}]),
        json!([{"instance":"front","vertex":"unknown","fraction":1.0}]),
        json!([{"instance":"missing","vertex":"a","fraction":1.0}]),
        json!([{"instance":"front","vertex":"a","fraction":0.0}]),
    ] {
        let mut bad = raw.clone();
        bad["attached_mass"]["groups"][0]["targets"] = invalid;
        assert!(Attachments::from_pattern(&Pattern::new(bad)).is_err());
    }
    raw["attached_mass"]["groups"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(&parsed.groups[0]).unwrap());
    assert!(Attachments::from_pattern(&Pattern::new(raw)).is_err());
}
