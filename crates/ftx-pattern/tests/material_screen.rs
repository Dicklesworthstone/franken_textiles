//! Synthetic known answers for selection, not physical characterization of commercial textiles.
use ftx_pattern::{
    material_catalog::Selection,
    material_screen::{Basis, Capability, Request},
    materials::Catalog,
};
use serde_json::{Value, json};

fn database() -> Catalog {
    let mut records = Vec::new();
    for (id, mass, warmth) in [
        ("a", 200., 0.2),
        ("b", 300., 0.3),
        ("c", 400., 0.15),
        ("d", 200., 0.2),
    ] {
        let mut m = json!({"id":id,"name":format!("Synthetic {id}"),"form":"woven",
            "structure":{"weave":"test twill"},"properties":{},
            "sources":{"test":{"reference":"local:synthetic-screen","accessed_on":"2026-10-15","note":"Artificial numerical test, not a measured textile"}}});
        for (key, value, unit) in [
            ("areal_density", mass, "g/m2"),
            ("thermal_resistance", warmth, "m2*K/W"),
        ] {
            m["properties"][key] = json!({"value":value,"unit":unit,"evidence":"estimate","source":"test","method":"synthetic scalar","conditions":{"conditioning":"dry"},"bounds":[value*0.95,value*1.05]});
        }
        records.push(m);
    }
    Catalog::from_json(&json!({"schema":"ftx-textile-catalog/1","materials":records}).to_string())
        .unwrap()
}

fn request() -> Request {
    serde_json::from_value(json!({"schema":"ftx-material-screen/1","basis":"nominal","allow_catalog_estimates":true,
        "objectives":[
            {"attribute":{"key":"areal_density","unit":"kg/m2","conditions":{"conditioning":"dry"}},"direction":"minimize"},
            {"attribute":{"key":"thermal_resistance","unit":"m2*K/W"},"direction":"maximize"}
        ]})).unwrap()
}

#[test]
fn tradeoffs_ties_units_and_sources_are_preserved() {
    let db = database();
    let before = serde_json::to_value(&db).unwrap();
    let report = request().evaluate(&db).unwrap();
    assert_eq!(report.frontier, ["a", "b", "d"]);
    assert_eq!(report.alternatives[2].dominated_by, ["a", "b", "d"]);
    assert_eq!(report.alternatives[0].readings["areal_density"].value, 0.2);
    assert_eq!(
        report.alternatives[0].readings["areal_density"].bounds,
        Some([0.19, 0.21])
    );
    let frozen = report.alternatives[0].selection.as_ref().unwrap();
    assert!(frozen.expected_sha256.is_some());
    assert_eq!(
        frozen.record.as_ref().unwrap().properties["areal_density"].unit,
        "g/m2"
    );
    assert!(!report.physical_validation_claim);
    assert_eq!(serde_json::to_value(&db).unwrap(), before);
}

#[test]
fn bounded_dominance_never_orders_overlapping_intervals() {
    let db = database();
    let mut r = request();
    r.basis = Basis::ReportedBounds;
    let report = r.evaluate(&db).unwrap();
    assert_eq!(report.frontier, ["a", "b", "d"]);
    assert!(report.alternatives[0].dominated_by.is_empty());
    let mut db = db;
    db.materials[2]
        .properties
        .get_mut("areal_density")
        .unwrap()
        .bounds = Some([180., 450.]);
    assert!(r.evaluate(&db).unwrap().frontier.contains(&"c".to_string()));
}

#[test]
fn constraints_use_worst_reported_bound_not_just_mean() {
    let mut r = request();
    r.constraints = serde_json::from_value(json!([{"attribute":{"key":"areal_density","unit":"kg/m2","conditions":{"conditioning":"dry"}},"maximum":0.205}])).unwrap();
    assert_eq!(r.evaluate(&database()).unwrap().frontier, ["a", "d"]);
    r.basis = Basis::ReportedBounds;
    assert!(r.evaluate(&database()).unwrap().frontier.is_empty());
}

#[test]
fn missing_fields_bounds_evidence_and_qualifiers_are_not_scores() {
    let mut db = database();
    db.materials[0].properties.remove("thermal_resistance");
    db.materials[1]
        .properties
        .get_mut("areal_density")
        .unwrap()
        .bounds = None;
    db.materials[2]
        .properties
        .get_mut("areal_density")
        .unwrap()
        .conditions
        .insert("conditioning".into(), "wet".into());
    let mut r = request();
    r.basis = Basis::ReportedBounds;
    let result = r.evaluate(&db).unwrap();
    assert_eq!(result.frontier, ["d"]);
    assert!(
        result.alternatives[..3]
            .iter()
            .all(|a| !a.eligible && !a.exclusions.is_empty() && a.dominated_by.is_empty())
    );
    r.allow_catalog_estimates = false;
    assert!(r.evaluate(&db).unwrap().frontier.is_empty());
    r.allow_catalog_estimates = true;
    r.objectives[1].attribute.method = Some("different method".into());
    assert!(r.evaluate(&db).unwrap().frontier.is_empty());
}

#[test]
fn capability_checks_are_independent_and_invoke_real_laws() {
    let db = database();
    let mut r = request();
    r.required_capabilities = vec![Capability::SteadyConduction];
    assert_eq!(r.evaluate(&db).unwrap().frontier, ["a", "b", "d"]);
    r.required_capabilities = vec![Capability::TransientThermal];
    assert!(r.evaluate(&db).unwrap().frontier.is_empty());
    r.required_capabilities = vec![Capability::Mechanics];
    assert!(
        r.evaluate(&db)
            .unwrap()
            .alternatives
            .iter()
            .all(|a| !a.eligible)
    );
}

#[test]
fn request_errors_are_not_misrepresented_as_no_matching_materials() {
    let db = database();
    let mut r = request();
    r.objectives[0].attribute.unit = "g/m2".into();
    assert!(r.evaluate(&db).is_err());
    let mut r = request();
    r.constraints = serde_json::from_value(
        json!([{"attribute":{"key":"areal_density","unit":"kg/m2"},"minimum":2.0,"maximum":1.0}]),
    )
    .unwrap();
    assert!(r.evaluate(&db).is_err());
    r.constraints[0].maximum = None;
    assert!(r.evaluate(&db).is_err()); // Conflicting conditions for the same key.
    r.constraints.clear();
    r.objectives = vec![r.objectives[0].clone(); 17];
    assert!(r.evaluate(&db).is_err());
}

#[test]
fn frozen_alternatives_replay_offline_and_refuse_tampering() {
    let report = request().evaluate(&database()).unwrap();
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    let mut r = request();
    r.allow_catalog_estimates = false;
    r.candidates = report
        .alternatives
        .iter()
        .map(|a| ftx_pattern::material_screen::Candidate {
            key: a.key.clone(),
            selection: a.selection.clone().unwrap(),
        })
        .collect();
    r.candidates.reverse();
    assert_eq!(r.evaluate(&empty).unwrap().frontier, report.frontier);
    let roundtrip: Request = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(r.evaluate(&empty).unwrap()).unwrap(),
        serde_json::to_value(roundtrip.evaluate(&empty).unwrap()).unwrap()
    );
    r.candidates[0]
        .selection
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("areal_density")
        .unwrap()
        .value = 199.;
    let tampered = r.evaluate(&empty).unwrap();
    assert!(!tampered.alternatives[3].eligible);
    assert!(tampered.alternatives[3].selection.is_none());
}

#[test]
fn exact_keys_allow_distinct_overrides_but_not_ambiguous_duplicates() {
    let mut r = request();
    r.allow_catalog_estimates = false;
    r.candidates = vec![
        ftx_pattern::material_screen::Candidate {
            key: "first".into(),
            selection: Selection {
                id: "a".into(),
                allow_estimates: true,
                ..Selection::default()
            },
        };
        2
    ];
    assert!(r.evaluate(&database()).is_err());
    r.candidates[1].key = "second".into();
    assert_eq!(
        r.evaluate(&database()).unwrap().frontier,
        ["first", "second"]
    );
    r.allow_catalog_estimates = true;
    assert!(r.evaluate(&database()).is_err());
}

#[test]
fn discovery_filters_keep_explicit_exclusions_and_empty_objectives_do_not_rank() {
    let mut r = request();
    r.objectives.clear();
    r.query = "TWILL".into();
    assert_eq!(r.evaluate(&database()).unwrap().frontier.len(), 4);
    r.forms = vec![ftx_pattern::materials::Form::Knit];
    let report = r.evaluate(&database()).unwrap();
    assert!(report.frontier.is_empty());
    assert_eq!(report.alternatives.len(), 4);
    assert!(serde_json::from_value::<Request>(json!({"schema":"ftx-material-screen/1"})).is_err());
    let _: Value = serde_json::to_value(report).unwrap();
}

#[test]
fn different_test_protocols_are_incomparable_without_explicit_permission() {
    let mut db = database();
    db.materials[2]
        .properties
        .get_mut("areal_density")
        .unwrap()
        .method = "a different protocol".into();
    let mut r = request();
    assert!(r.evaluate(&db).unwrap().frontier.contains(&"c".into()));
    r.allow_mixed_protocols = true;
    assert!(!r.evaluate(&db).unwrap().frontier.contains(&"c".into()));
    db.materials[2]
        .properties
        .get_mut("areal_density")
        .unwrap()
        .method = "synthetic scalar".into();
    db.materials[2]
        .properties
        .get_mut("thermal_resistance")
        .unwrap()
        .conditions
        .insert("temperature".into(), "different test temperature".into());
    r.allow_mixed_protocols = false;
    assert!(r.evaluate(&db).unwrap().frontier.contains(&"c".into()));
}
