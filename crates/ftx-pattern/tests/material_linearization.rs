//! Independent work-conjugacy, source-replay and approximation-domain checks.
use std::collections::BTreeMap;

use ftx_pattern::{
    material_catalog::Selection,
    material_linearization::{Input, linearize},
    materials::{Catalog, Evidence, Form, Material, Quantity, Source},
};

fn input() -> Input {
    let source = Source {
        reference: "local:synthetic-origin-tangent".into(),
        accessed_on: "2026-11-03".into(),
        note: "Synthetic tensor for independent energy/work checks; no textile measurement".into(),
    };
    let conditions = BTreeMap::from([
        ("specimen".into(), "synthetic-sheet".into()),
        ("conditioning".into(), "synthetic dry state".into()),
        ("strain_basis".into(), "green_lagrange_tensor_shear".into()),
        ("source_node".into(), "lambda_max=0".into()),
    ]);
    let properties = [
        ("wang2011_origin_c11", 100.0),
        ("wang2011_origin_c12", -10.0),
        ("wang2011_origin_c22", 40.0),
        ("wang2011_origin_c33", 30.0),
    ]
    .into_iter()
    .map(|(key, value)| {
        (
            key.into(),
            Quantity {
                value,
                unit: "N/m".into(),
                evidence: Evidence::Published,
                source: "synthetic".into(),
                method: "Synthetic exact origin tensor".into(),
                conditions: conditions.clone(),
                bounds: None,
            },
        )
    })
    .collect();
    Input {
        schema: "ftx-wang2011-linearization/1".into(),
        material: Selection {
            id: "synthetic-origin".into(),
            record: Some(Material {
                id: "synthetic-origin".into(),
                name: "Synthetic origin tensor".into(),
                form: Form::Sheet,
                composition: Vec::new(),
                composition_complete: false,
                structure: BTreeMap::new(),
                properties,
                membrane_response: None,
                membrane_relaxation: None,
                thermal_response: None,
                optical_faces: None,
                sources: BTreeMap::from([("synthetic".into(), source)]),
            }),
            allow_estimates: true,
            ..Selection::default()
        },
        green_strain_domain: [[-0.04, 0.04]; 3],
        approximation_source: Source {
            reference: "local:synthetic-approximation-choice".into(),
            accessed_on: "2026-11-03".into(),
            note: "Explicit synthetic local-linear domain for this test only".into(),
        },
    }
}

fn empty_catalog() -> Catalog {
    Catalog {
        schema: ftx_pattern::materials::CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-10 * (1.0 + b.abs()), "{a} != {b}");
}

#[test]
fn engineering_shear_work_and_coupling_follow_the_source_tensor() {
    let report = linearize(&input()).unwrap();
    let resolved = empty_catalog().resolve(&report.selection).unwrap();
    let law = resolved.membrane_response().unwrap().unwrap();
    let strain = [0.01, -0.02, 0.03];
    let response = law.evaluate(strain).unwrap();
    near(response.energy, 0.02175);
    for (actual, expected) in response.stress.into_iter().zip([1.2, -0.9, 0.45]) {
        near(actual, expected);
    }
    assert_eq!(
        response.tangent,
        [[100., -10., 0.], [-10., 40., 0.], [0., 0., 15.]]
    );
    assert_eq!(response.tangent, report.origin_tangent_n_m);
    for axis in 0..3 {
        let mut plus = strain;
        let mut minus = strain;
        plus[axis] += 1e-6;
        minus[axis] -= 1e-6;
        let gradient =
            (law.evaluate(plus).unwrap().energy - law.evaluate(minus).unwrap().energy) / 2e-6;
        near(gradient, response.stress[axis]);
    }
    assert!(law.check_domain([0.040_000_1, 0., 0.]).is_err());
    assert!(resolved.mechanics().is_err());
    assert!(resolved.thermal_resistance().is_err());
    assert!(!resolved.material().properties.contains_key("thickness"));
    assert!(!resolved.material().properties.contains_key("friction_skin"));
    assert!(
        !resolved
            .material()
            .properties
            .contains_key("bending_rigidity")
    );
}

#[test]
fn replay_retains_original_observations_policy_and_exact_material_identity() {
    let request = input();
    let report = linearize(&request).unwrap();
    let replay = linearize(&report.input).unwrap();
    assert_eq!(report.selection_json, replay.selection_json);
    assert_eq!(report.input_sha256, replay.input_sha256);
    let selected: Selection = serde_json::from_str(&report.selection_json).unwrap();
    let resolved = empty_catalog().resolve(&selected).unwrap();
    for (key, original) in &request.material.record.as_ref().unwrap().properties {
        assert_eq!(
            serde_json::to_value(original).unwrap(),
            serde_json::to_value(&resolved.authored().properties[key]).unwrap()
        );
    }
    let mut strict = resolved.frozen_selection();
    strict.allow_estimates = false;
    strict.expected_sha256 = None;
    assert!(
        empty_catalog()
            .resolve(&strict)
            .unwrap()
            .membrane_response()
            .is_err()
    );
    let mut changed = request.clone();
    changed.green_strain_domain[0][1] = 0.03;
    let other = linearize(&changed).unwrap();
    assert_ne!(
        other.selection.expected_sha256,
        report.selection.expected_sha256
    );
    assert_eq!(other.source_sha256, report.source_sha256);
    let mut stale = report.input;
    stale
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("wang2011_origin_c11")
        .unwrap()
        .value = 200.0;
    assert!(linearize(&stale).is_err());
}

#[test]
fn no_consent_cross_specimen_wrong_basis_and_indefinite_tensors_refuse() {
    let mut request = input();
    request.material.allow_estimates = false;
    assert!(
        linearize(&request)
            .unwrap_err()
            .message
            .contains("allow_estimates")
    );
    for (key, value) in [
        ("specimen", "another sheet"),
        ("conditioning", "wet"),
        ("strain_basis", "engineering_shear"),
        ("source_node", "lambda_max=0.2"),
    ] {
        let mut request = input();
        request
            .material
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("wang2011_origin_c12")
            .unwrap()
            .conditions
            .insert(key.into(), value.into());
        assert!(linearize(&request).is_err(), "accepted {key}");
    }
    for (key, value) in [
        ("wang2011_origin_c12", 100.),
        ("wang2011_origin_c33", 0.),
        ("wang2011_origin_c11", -1.),
    ] {
        let mut request = input();
        request
            .material
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut(key)
            .unwrap()
            .value = value;
        assert!(linearize(&request).is_err(), "accepted nonphysical {key}");
    }
    let mut wrong_unit = input();
    wrong_unit
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("wang2011_origin_c11")
        .unwrap()
        .unit = "Pa".into();
    assert!(linearize(&wrong_unit).is_err());
}

#[test]
fn domains_are_explicit_and_existing_laws_are_never_replaced() {
    for domain in [
        [[0., 0.04]; 3],
        [[-0.51, 0.04]; 3],
        [[-0.49, 0.04]; 3],
        [[-0.04, f64::INFINITY]; 3],
    ] {
        let mut request = input();
        request.green_strain_domain = domain;
        assert!(linearize(&request).is_err());
    }
    let report = linearize(&input()).unwrap();
    let mut repeated = input();
    repeated.material = report.selection;
    assert!(linearize(&repeated).is_err());
    let mut uncredited = input();
    uncredited.approximation_source.note.clear();
    assert!(linearize(&uncredited).is_err());
}
