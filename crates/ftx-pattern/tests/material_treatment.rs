//! Independently authored synthetic treatment arithmetic and ordinary material consumers.
//! These checks are not laundering measurements or empirical garment validation.

use std::collections::BTreeMap;

use ftx_pattern::{
    material_catalog::{Selection, laundered_denim},
    material_thermal::ThermalLaw,
    material_treatment::{Input, SCHEMA, evaluate},
    materials::{
        CATALOG_SCHEMA, Catalog, Evidence, Form, Material, Quantity, Source, ThermalStack,
    },
};
use serde_json::json;

const SPECIMEN: &str = "synthetic treatment specimen";
const PROCESS: &str = "synthetic wash/dry protocol, not a measured textile";

fn source() -> Source {
    Source {
        reference: "local:ftx/synthetic-treatment-test/1".into(),
        accessed_on: "2026-10-26".into(),
        note: "Independently authored numerical fixture; no physical material claim".into(),
    }
}

fn catalog() -> Catalog {
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
}

fn freeze(material: Material) -> Selection {
    catalog()
        .resolve(&Selection {
            id: material.id.clone(),
            record: Some(material),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection()
}

fn material(after: bool) -> Material {
    let condition = if after {
        "after-conditioned"
    } else {
        "before-conditioned"
    };
    let values = if after {
        [
            210., 130., 50., 0.25, 30., 0.004, 0.7, 0.3, 0.35, 0.08, 1100., 1.4,
        ]
    } else {
        [
            180., 100., 60., 0.2, 25., 0.002, 0.5, 0.2, 0.25, 0.06, 1200., 1.5,
        ]
    };
    let properties = [
        ("areal_density", "g/m2"),
        ("warp_stiffness", "N/m"),
        ("weft_stiffness", "N/m"),
        ("poisson_warp_weft", "1"),
        ("shear_stiffness", "N/m"),
        ("bending_rigidity", "N*m"),
        ("thickness", "mm"),
        ("friction_skin", "1"),
        ("friction_self", "1"),
        ("thermal_conductivity", "W/(m*K)"),
        ("specific_heat", "J/(kg*K)"),
        ("width", "m"),
    ]
    .into_iter()
    .zip(values)
    .map(|((key, unit), value)| {
        (
            key.into(),
            Quantity {
                value,
                unit: unit.into(),
                evidence: Evidence::Estimate,
                source: "synthetic".into(),
                method: "Synthetic independent before/after coefficients".into(),
                bounds: None,
                conditions: BTreeMap::from([
                    ("specimen".into(), SPECIMEN.into()),
                    ("conditioning".into(), condition.into()),
                ]),
            },
        )
    })
    .collect();
    Material {
        id: if after {
            "synthetic/after"
        } else {
            "synthetic/before"
        }
        .into(),
        name: format!("Synthetic {condition} sheet"),
        form: Form::Woven,
        composition: Vec::new(),
        composition_complete: false,
        structure: if after {
            BTreeMap::from([
                ("treatment_process".into(), PROCESS.into()),
                ("treatment_cycles".into(), "5".into()),
            ])
        } else {
            BTreeMap::new()
        },
        properties,
        membrane_response: None,
        membrane_relaxation: None,
        thermal_response: None,
        optical_faces: None,
        sources: BTreeMap::from([("synthetic".into(), source())]),
    }
}

fn fixture() -> Input {
    let quantity = |value| Quantity {
        value,
        unit: "%".into(),
        evidence: Evidence::Estimate,
        source: "dimensions".into(),
        method: "Synthetic freely treated length observations; positive shrinkage is contraction"
            .into(),
        bounds: None,
        conditions: BTreeMap::from([
            ("specimen".into(), SPECIMEN.into()),
            ("process".into(), PROCESS.into()),
            ("cycles".into(), "5".into()),
            ("conditioning_before".into(), "before-conditioned".into()),
            ("conditioning_after".into(), "after-conditioned".into()),
        ]),
    };
    Input {
        schema: SCHEMA.into(),
        before: freeze(material(false)),
        after: freeze(material(true)),
        specimen: SPECIMEN.into(),
        process: PROCESS.into(),
        cycles: 5,
        conditioning_before: "before-conditioned".into(),
        conditioning_after: "after-conditioned".into(),
        shrinkage_warp: quantity(10.),
        shrinkage_weft: quantity(-5.),
        sources: BTreeMap::from([("dimensions".into(), source())]),
        allow_estimates: true,
    }
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-12 * expected.abs().max(1.),
        "{actual} != {expected}"
    );
}

fn response_material(after: bool) -> Material {
    let mut record = material(after);
    let conditions = record.properties["thickness"].conditions.clone();
    let curve = |points| {
        json!({"points": points, "source": "synthetic", "evidence": "estimate",
            "method": "Independent synthetic state response", "conditions": conditions})
    };
    record.membrane_response = Some(
        serde_json::from_value(json!({
            "model": "tabulated_green_strain/1", "interpolation": "piecewise_linear",
            "warp": curve(json!([[-0.2,-20.],[0.,0.],[0.2,30.]])),
            "weft": curve(json!([[-0.2,-10.],[0.,0.],[0.2,20.]])),
            "shear": curve(json!([[-0.2,-5.],[0.,0.],[0.2,5.]])),
            "coupling": {"value":0.,"unit":"N/m","source":"synthetic","evidence":"estimate",
                "method":"Synthetic independent zero coupling", "conditions":conditions}
        }))
        .unwrap(),
    );
    record.thermal_response = Some(
        serde_json::from_value(json!({
            "specimen": SPECIMEN, "conditioning":conditions["conditioning"],
            "specific_heat": {"unit":"J/(kg*K)", "source":"synthetic", "evidence":"estimate",
                "method":"Synthetic temperature observations", "conditions":conditions,
                "points":[{"temperature_c":10.,"mean":1000.},{"temperature_c":30.,"mean":1200.}]}
        }))
        .unwrap(),
    );
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
        "specific_heat",
    ] {
        record.properties.remove(key);
    }
    record
}

#[test]
fn one_observed_transition_updates_normal_mechanics_and_heat_without_guessed_mass() {
    let input = fixture();
    let original = serde_json::to_value(&input).unwrap();
    let report = evaluate(&input).unwrap();
    close(report.stretch[0], 0.9);
    close(report.stretch[1], 1.05);
    close(report.compensation[0], 1. / 0.9);
    close(report.compensation[1], 1. / 1.05);
    close(report.area_scale, 0.945);
    // Five is the observed complete cycle count, not an instruction to apply 0.9 five times.
    assert_eq!(report.input.cycles, 5);
    close(report.shrinkage_warp.value, 0.1);
    assert_eq!(report.shrinkage_warp.unit, "1");
    assert_eq!(report.input.shrinkage_warp.unit, "%");
    let before = catalog().resolve(&report.before).unwrap();
    let after = catalog().resolve(&report.after).unwrap();
    let mechanics = after.mechanics().unwrap();
    close(mechanics.areal_kg_m2, 0.21);
    close(mechanics.warp_n_m, 130.);
    close(mechanics.thickness_m, 0.0007);
    // The source AFTER mass is authoritative even though the implied group mass changed.
    assert_ne!(
        mechanics.areal_kg_m2,
        before.mechanics().unwrap().areal_kg_m2 / report.area_scale
    );
    let heat = ThermalLaw::from_material(after.material(), true).unwrap();
    close(heat.capacity_at(20.).unwrap(), 231.);
    close(heat.resistance_at(20.).unwrap(), 0.0007 / 0.08);
    let steady = ThermalStack::evaluate(&[after.material()], 2., 40., 20., 0., true).unwrap();
    close(steady.heat_flow_w, 40. / (0.0007 / 0.08));
    assert_eq!(serde_json::to_value(input).unwrap(), original);
    assert_eq!(
        serde_json::to_value(report.after.record).unwrap(),
        original["after"]["record"]
    );
}

#[test]
fn exact_frozen_replay_keeps_original_numbers_conditions_and_source_intervals() {
    let mut input = fixture();
    input.shrinkage_warp.bounds = Some([8., 12.]);
    input.shrinkage_weft.bounds = Some([-6., -4.]);
    let report = evaluate(&input).unwrap();
    assert_eq!(
        report.reported_stretch_bounds,
        [Some([0.88, 0.92]), Some([1.04, 1.06])]
    );
    let before: Selection = serde_json::from_str(&report.before_selection_json).unwrap();
    let after: Selection = serde_json::from_str(&report.after_selection_json).unwrap();
    assert_eq!(
        catalog().resolve(&before).unwrap().sha256(),
        report.before_sha256
    );
    assert_eq!(
        catalog().resolve(&after).unwrap().sha256(),
        report.after_sha256
    );
    let replay: Input =
        serde_json::from_str(&serde_json::to_string(&report.input).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(evaluate(&replay).unwrap()).unwrap(),
        serde_json::to_value(report).unwrap()
    );
    let mut changed = input;
    changed
        .sources
        .get_mut("dimensions")
        .unwrap()
        .note
        .push_str(" revised evidence");
    assert_ne!(
        evaluate(&changed).unwrap().input_sha256,
        evaluate(&replay).unwrap().input_sha256
    );
}

#[test]
fn absent_after_evidence_stays_absent_and_independent_heat_still_works() {
    let mut input = fixture();
    let mut after = material(true);
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
        "width",
    ] {
        after.properties.remove(key);
    }
    input.after = freeze(after);
    let report = evaluate(&input).unwrap();
    assert_eq!(
        report.properties_no_longer_available,
        [
            "poisson_warp_weft",
            "shear_stiffness",
            "warp_stiffness",
            "weft_stiffness",
            "width"
        ]
    );
    let resolved = catalog().resolve(&report.after).unwrap();
    assert!(resolved.mechanics().is_err());
    assert!(resolved.thermal_resistance().is_ok());
    assert!(!resolved.material().properties.contains_key("width"));
}

#[test]
fn qualified_post_treatment_nonlinear_and_temperature_laws_reach_existing_compilers() {
    let mut input = fixture();
    input.after = freeze(response_material(true));
    let report = evaluate(&input).unwrap();
    let resolved = catalog().resolve(&report.after).unwrap();
    assert!(resolved.mechanics().is_ok());
    let law = resolved.membrane_response().unwrap().unwrap();
    let response = law.evaluate([0.1, 0., 0.]).unwrap();
    close(response.stress[0], 15.);
    close(response.energy, 0.75);
    let thermal = ThermalLaw::from_material(resolved.material(), true).unwrap();
    close(thermal.sensible_change(10., 30.).unwrap(), 0.21 * 22000.);
    let mut mismatched = response_material(true);
    mismatched
        .membrane_response
        .as_mut()
        .unwrap()
        .warp
        .conditions
        .insert("conditioning".into(), "before-conditioned".into());
    input.after = freeze(mismatched);
    assert!(evaluate(&input).is_err());
}

#[test]
fn original_response_blocks_are_reported_missing_instead_of_inherited() {
    let mut input = fixture();
    let mut before = response_material(false);
    before.id = "synthetic/response-before".into();
    input.before = freeze(before);
    let report = evaluate(&input).unwrap();
    assert_eq!(
        report.responses_no_longer_available,
        ["membrane_response", "thermal_response"]
    );
    assert!(
        report
            .after
            .record
            .as_ref()
            .unwrap()
            .membrane_response
            .is_none()
    );
    assert!(
        report
            .after
            .record
            .as_ref()
            .unwrap()
            .thermal_response
            .is_none()
    );
}

#[test]
fn source_qualification_and_material_locks_are_not_replaceable_by_an_identity_label() {
    for field in [
        "specimen",
        "process",
        "cycles",
        "conditioning_before",
        "conditioning_after",
    ] {
        let mut input = fixture();
        input
            .shrinkage_warp
            .conditions
            .insert(field.into(), "wrong".into());
        assert!(evaluate(&input).is_err(), "{field}");
    }
    let mut input = fixture();
    input.before.expected_sha256 = None;
    assert!(evaluate(&input).is_err());
    let mut input = fixture();
    input
        .after
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thickness")
        .unwrap()
        .value *= 2.;
    assert!(evaluate(&input).is_err());
    let mut input = fixture();
    input.after = input.before.clone();
    assert!(evaluate(&input).is_err());
    let mut input = fixture();
    input.before = input.after.clone();
    let mut changed_policy = input.after.clone();
    changed_policy.expected_sha256 = None;
    changed_policy.allow_estimates = false;
    input.after = catalog()
        .resolve(&changed_policy)
        .unwrap()
        .frozen_selection();
    assert!(evaluate(&input).is_err());
    let mut input = fixture();
    let mut after = material(true);
    after.structure.remove("treatment_process");
    input.after = freeze(after);
    assert!(evaluate(&input).is_err());
    let mut input = fixture();
    let mut after = material(true);
    after
        .properties
        .get_mut("specific_heat")
        .unwrap()
        .conditions
        .insert("conditioning".into(), "before-conditioned".into());
    input.after = freeze(after);
    assert!(evaluate(&input).is_err());
}

#[test]
fn freshly_locked_before_observations_must_match_the_observed_specimen_and_conditioning() {
    let original = fixture();
    for (key, different) in [
        ("specimen", "independent unrelated source specimen"),
        (
            "conditioning",
            "another initial moisture and temperature state",
        ),
    ] {
        let mut before = material(false);
        before
            .properties
            .get_mut("thickness")
            .unwrap()
            .conditions
            .insert(key.into(), different.into());
        let mut input = original.clone();
        input.before = freeze(before);
        assert_ne!(
            input.before.expected_sha256,
            original.before.expected_sha256
        );
        // The ordinary source record and fresh digest are valid; treatment-pair admission refuses.
        assert!(catalog().resolve(&input.before).is_ok());
        let refused = evaluate(&input).unwrap_err();
        assert!(refused.message.contains("BEFORE"), "{refused}");
    }
    let mut input = original;
    let mut before = response_material(false);
    input.before = freeze(before.clone());
    assert!(evaluate(&input).is_ok());
    before
        .membrane_response
        .as_mut()
        .unwrap()
        .warp
        .conditions
        .insert("conditioning".into(), "different conditioning".into());
    input.before = freeze(before);
    assert!(catalog().resolve(&input.before).is_ok());
    let refused = evaluate(&input).unwrap_err();
    assert!(
        refused.message.contains("BEFORE warp response"),
        "{refused}"
    );
}

#[test]
fn measured_transition_preserves_its_pair_and_cannot_borrow_another_denim_specimen() {
    let mut input = laundered_denim::treatment_request(1).unwrap();
    let original = serde_json::to_value(&input).unwrap();
    let report = evaluate(&input).unwrap();
    assert_eq!(
        serde_json::to_value(&report.before).unwrap(),
        original["before"]
    );
    assert_eq!(
        serde_json::to_value(&report.after).unwrap(),
        original["after"]
    );
    assert_eq!(serde_json::to_value(&input).unwrap(), original);
    input.before = laundered_denim::treatment_request(5).unwrap().before;
    assert!(catalog().resolve(&input.before).is_ok());
    let refused = evaluate(&input).unwrap_err();
    assert!(refused.message.contains("BEFORE"), "{refused}");
}

#[test]
fn dimensional_estimate_permission_is_separate_from_material_consumer_permission() {
    let mut input = fixture();
    input.allow_estimates = false;
    assert!(evaluate(&input).is_err());
    input.shrinkage_warp.evidence = Evidence::Published;
    input.shrinkage_weft.evidence = Evidence::Published;
    let mut selection = input.after.clone();
    selection.expected_sha256 = None;
    selection.allow_estimates = false;
    input.after = catalog().resolve(&selection).unwrap().frozen_selection();
    let report = evaluate(&input).unwrap();
    let after = catalog().resolve(&report.after).unwrap();
    assert!(!after.allows_estimates());
    assert!(after.mechanics().is_err());
    assert!(after.thermal_resistance().is_err());
}

#[test]
fn zero_and_expansion_are_valid_but_collapsed_or_unbounded_lengths_refuse() {
    let mut input = fixture();
    input.shrinkage_warp.value = 0.;
    input.shrinkage_weft.value = -20.;
    let report = evaluate(&input).unwrap();
    assert_eq!(report.stretch, [1., 1.2]);
    for value in [100., 101., f64::NAN, f64::INFINITY] {
        let mut input = fixture();
        input.shrinkage_warp.value = value;
        assert!(evaluate(&input).is_err());
    }
    let mut input = fixture();
    input.shrinkage_warp.bounds = Some([0., 100.]);
    assert!(evaluate(&input).is_err());
    let mut input = fixture();
    input.shrinkage_weft.unit = "mm".into();
    assert!(evaluate(&input).is_err());
    let mut input = fixture();
    input.cycles = 0;
    assert!(evaluate(&input).is_err());
    let mut raw = serde_json::to_value(fixture()).unwrap();
    raw["cycles"] = json!(2.5);
    assert!(serde_json::from_value::<Input>(raw).is_err());
}
