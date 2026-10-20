//! Synthetic evidence-policy checks; fixture values are not measured textile properties.
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, Selection},
    materials::{Catalog, Evidence},
};
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation-heat.json"
    ))
    .unwrap()
}
fn resolved(raw: Value) -> ResolvedMaterial {
    let selection: Selection = serde_json::from_value(raw).unwrap();
    Catalog::builtin().unwrap().resolve(&selection).unwrap()
}

#[test]
fn active_relaxation_requires_its_own_qualified_conversion_and_explicit_zero_is_valid() {
    let raw = fixture();
    let fraction = resolved(raw.clone())
        .membrane_relaxation_heat_fraction()
        .unwrap()
        .unwrap();
    assert_eq!(fraction.fraction, 0.75);
    assert_eq!(fraction.temperature_range_c, [-20., 80.]);
    let mut zero = raw.clone();
    zero["record"]["properties"]["membrane_relaxation_heat_fraction"]["value"] = json!(0.);
    assert_eq!(
        resolved(zero)
            .membrane_relaxation_heat_fraction()
            .unwrap()
            .unwrap()
            .fraction,
        0.
    );
    let mut missing = raw.clone();
    missing["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("membrane_relaxation_heat_fraction");
    let m = resolved(missing);
    assert!(m.mechanics().is_ok());
    assert!(m.membrane_relaxation().unwrap().is_some());
    assert!(m.membrane_relaxation_heat_fraction().is_err());
    let mut inactive = raw;
    inactive["record"]
        .as_object_mut()
        .unwrap()
        .remove("membrane_relaxation");
    assert!(
        resolved(inactive)
            .membrane_relaxation_heat_fraction()
            .unwrap()
            .is_none()
    );
}

#[test]
fn conversion_rejects_wrong_work_temperature_or_shared_specimen_without_disabling_mechanics() {
    for case in 0..12 {
        let mut raw = fixture();
        let q = &mut raw["record"]["properties"]["membrane_relaxation_heat_fraction"];
        match case {
            0 => q["value"] = json!(-0.1),
            1 => q["value"] = json!(1.1),
            2 => q["bounds"] = json!([-0.1, 1.]),
            3 => q["conditions"]["work_basis"] = json!("elastic_energy_drop"),
            4 => q["conditions"]["temperature_min_c"] = json!("NaN"),
            5 => q["conditions"]["temperature_min_c"] = json!("-274"),
            6 => q["conditions"]["temperature_max_c"] = json!("-21"),
            7 => q["conditions"]["specimen_id"] = json!("different-sheet"),
            8 => q["conditions"]["conditioning"] = json!("wet instead of dry"),
            9 => {
                raw["record"]["properties"]["warp_stiffness"]["conditions"]["specimen_id"] =
                    json!("other")
            }
            10 => {
                raw["record"]["membrane_relaxation"]["branches"][0]["relaxation_time"]["conditions"]
                    ["conditioning"] = json!("other")
            }
            _ => {
                raw["record"]["membrane_relaxation"]["conditions"]
                    .as_object_mut()
                    .unwrap()
                    .remove("specimen_id");
            }
        }
        let material = resolved(raw);
        assert!(material.mechanics().is_ok(), "mechanics changed: {case}");
        assert!(material.membrane_relaxation().is_ok());
        assert!(
            material.membrane_relaxation_heat_fraction().is_err(),
            "case {case}"
        );
    }
}

#[test]
fn conversion_consent_and_dimensional_validation_follow_the_existing_material_authority() {
    let mut selection: Selection = serde_json::from_value(fixture()).unwrap();
    // Synthetic published labels test policy, not empirical origin; all numbers remain artificial.
    let material = selection.record.as_mut().unwrap();
    for (key, q) in &mut material.properties {
        if key != "membrane_relaxation_heat_fraction" {
            q.evidence = Evidence::Published;
        }
    }
    for branch in &mut material.membrane_relaxation.as_mut().unwrap().branches {
        branch.stiffness_ratio.evidence = Evidence::Published;
        branch.relaxation_time.evidence = Evidence::Published;
    }
    selection.allow_estimates = false;
    let catalog = Catalog::builtin().unwrap();
    let m = catalog.resolve(&selection).unwrap();
    assert!(m.membrane_relaxation().is_ok());
    assert!(m.membrane_relaxation_heat_fraction().is_err());
    selection.allow_estimates = true;
    assert!(
        catalog
            .resolve(&selection)
            .unwrap()
            .membrane_relaxation_heat_fraction()
            .is_ok()
    );
    selection
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("membrane_relaxation_heat_fraction")
        .unwrap()
        .unit = "N/m".into();
    assert!(catalog.resolve(&selection).is_err());
}
