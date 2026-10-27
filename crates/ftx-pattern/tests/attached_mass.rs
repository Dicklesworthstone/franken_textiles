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

fn retained_liquid() -> Value {
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation.json"
    ))
    .unwrap();
    let wet = json!({"specimen_id":"synthetic-wet-coupon", "conditioning":"closed synthetic retained liquid",
        "retained_liquid_kg_m2":"0.1"});
    let record = &mut input["record"];
    record["structure"]["mechanical_mass_model"] =
        json!(ftx_pattern::material_mass::RETAINED_LIQUID_MODEL);
    for q in record["properties"].as_object_mut().unwrap().values_mut() {
        q["conditions"] = wet.clone();
    }
    record["properties"]["areal_density"]["conditions"]["reference_area_basis"] =
        json!("dry_textile_reference_area");
    record["properties"]["areal_density"]["conditions"]["mass_basis"] =
        json!("dry_textile_excluding_retained_liquid");
    record["properties"]["areal_density"]["bounds"] = json!([0.18, 0.22]);
    record["properties"]["retained_liquid_areal_density"] = json!({"value":100., "unit":"g/m2", "bounds":[80.,120.],
        "evidence":"estimate","source":"test","method":"Independent synthetic additional liquid loading",
        "conditions":{"specimen_id":"synthetic-wet-coupon", "conditioning":"closed synthetic retained liquid",
            "reference_area_basis":"dry_textile_reference_area", "mass_basis":"additional_retained_liquid",
            "retention_basis":"closed_co_moving_no_exchange", "qualification":"Synthetic closed co-moving liquid; no exchange or slip"}});
    record["membrane_relaxation"]["conditions"] = wet.clone();
    for branch in record["membrane_relaxation"]["branches"]
        .as_array_mut()
        .unwrap()
    {
        branch["stiffness_ratio"]["conditions"] = wet.clone();
        branch["relaxation_time"]["conditions"] = wet.clone();
    }
    input
}

fn resolve_liquid(value: Value) -> ftx_pattern::material_catalog::ResolvedMaterial {
    ftx_pattern::materials::Catalog {
        schema: ftx_pattern::materials::CATALOG_SCHEMA.into(),
        materials: vec![],
    }
    .resolve(&serde_json::from_value(value).unwrap())
    .unwrap()
}

#[test]
fn fixed_liquid_uses_independent_source_mass_and_complete_bounds_without_borrowing_heat() {
    let resolved = resolve_liquid(retained_liquid());
    let material = resolved.material();
    let mass = material.retained_liquid_mass(true).unwrap().unwrap();
    assert_eq!(mass.dry_textile_kg_m2, 0.2);
    assert_eq!(mass.retained_liquid_kg_m2, 0.1);
    assert!((mass.total_kg_m2 - 0.3).abs() < 1e-15);
    for (a, b) in mass
        .reported_total_bounds_kg_m2
        .unwrap()
        .into_iter()
        .zip([0.26, 0.34])
    {
        assert!((a - b).abs() < 1e-15);
    }
    assert_eq!(resolved.mechanics().unwrap().areal_kg_m2, mass.total_kg_m2);
    assert_eq!(
        material.sheet_areal_mass(true).unwrap(),
        (mass.total_kg_m2, mass.reported_total_bounds_kg_m2)
    );
    assert!(material.retained_liquid_mass(false).is_err());
    assert!(material.require_dry_mass_transport().is_err());
    let frozen = serde_json::to_value(resolved.frozen_selection()).unwrap();
    let replay = resolve_liquid(frozen);
    assert_eq!(resolved.sha256(), replay.sha256());
    let mut incomplete = retained_liquid();
    incomplete["record"]["properties"]["retained_liquid_areal_density"]
        .as_object_mut()
        .unwrap()
        .remove("bounds");
    let incomplete = resolve_liquid(incomplete);
    let (nominal, bounds) = incomplete.material().sheet_areal_mass(true).unwrap();
    assert_eq!(nominal, mass.total_kg_m2);
    assert_eq!(bounds, None);
    let mut dry = retained_liquid();
    dry["record"]["structure"]
        .as_object_mut()
        .unwrap()
        .remove("mechanical_mass_model");
    let dry = resolve_liquid(dry);
    assert_eq!(dry.mechanics().unwrap().areal_kg_m2, 0.2);
    assert!(dry.material().retained_liquid_mass(true).unwrap().is_none());
    assert!(dry.material().require_dry_mass_transport().is_ok());
}

#[test]
fn fixed_liquid_refuses_wrong_mass_bases_unqualified_retention_and_stale_wet_properties() {
    for (key, value) in [
        ("specimen_id", "another-coupon"),
        ("conditioning", "drained instead of retained"),
        ("reference_area_basis", "current_surface_area"),
        ("mass_basis", "total_wet_textile"),
        ("retention_basis", "evaporating"),
        ("qualification", ""),
    ] {
        let mut raw = retained_liquid();
        raw["record"]["properties"]["retained_liquid_areal_density"]["conditions"][key] =
            json!(value);
        let resolved = resolve_liquid(raw);
        assert!(
            resolved.material().retained_liquid_mass(true).is_err(),
            "{key}"
        );
        assert!(resolved.mechanics().is_err(), "{key}");
    }
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
        "bending_rigidity",
        "thickness",
        "friction_skin",
        "friction_self",
    ] {
        let mut raw = retained_liquid();
        raw["record"]["properties"][key]["conditions"]["retained_liquid_kg_m2"] = json!("0");
        let resolved = resolve_liquid(raw);
        assert!(resolved.material().sheet_areal_mass(true).is_ok());
        assert!(
            resolved
                .mechanics()
                .unwrap_err()
                .message
                .contains("wet mechanics"),
            "{key}"
        );
    }
    let mut stale = retained_liquid();
    stale["record"]["membrane_relaxation"]["branches"][0]["relaxation_time"]["conditions"]["retained_liquid_kg_m2"] =
        json!("0");
    assert!(resolve_liquid(stale).mechanics().is_err());
    let mut doubled = retained_liquid();
    doubled["record"]["properties"]["areal_density"]["conditions"]["mass_basis"] =
        json!("total_wet_textile");
    assert!(
        resolve_liquid(doubled)
            .material()
            .sheet_areal_mass(true)
            .is_err()
    );
}
