//! Conditional nonlinear identification with explicitly synthetic shapes and solver observations.
//! No fixture value is a measurement of a textile or an independent physical validation claim.

use std::{collections::BTreeMap, sync::OnceLock};

use ftx_core::{RefusalCode, receipt::sha256_hex};
use ftx_drape::strip::{
    Request, evaluate,
    fit::{
        Input, Observation, Report, Width, calibrate,
        family::{Family, MODEL},
    },
};
use ftx_pattern::{
    material_calibration::{Partition, fitted_domain},
    material_catalog::Selection,
    material_response::{Interpolation, StressCurve, TabulatedMembrane},
    materials::{Evidence, Quantity, Source},
    textile_import,
};
use serde_json::json;

const PARAMETERS: [f64; 4] = [1.1, 0.9, 1.2, 40.0];

fn input() -> Input {
    let mut input: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-clamped-strip.json"
    ))
    .unwrap();
    input.material.id = "synthetic-nonlinear-strip-family".into();
    input.material.name = "Synthetic declared nonlinear shape identification software test".into();
    input.material.properties.clear();
    input.material.structure.clear();
    input.conditions = BTreeMap::from([(
        "provenance".into(),
        "Explicit synthetic nonlinear shape-family software test; no physical specimen".into(),
    )]);
    input.method = "Synthetic independent elastic ideal-grip equilibria with declared midpoint chord widths. Nonlinear shapes are assumptions, not identified knots or measured fabric response.".into();
    for source in input.material.sources.values_mut() {
        source.note = "Synthetic nonlinear-family numerical acquisition; not a measured coupon or empirical independence claim".into();
    }
    input.material.sources.insert(
        "shape".into(),
        Source {
            reference: "test:declared-nonlinear-strip-shapes".into(),
            accessed_on: "2026-10-21".into(),
            note: "Explicit synthetic signed shapes and true scales [1.1,0.9,1.2], normal coupling 40 N/m; no textile observations or area-preserving term.".into(),
        },
    );
    let curve = |points: &[[f64; 2]]| StressCurve {
        points: points.to_vec(),
        source: "shape".into(),
        evidence: Evidence::Estimate,
        method:
            "Explicit fixed synthetic piecewise-linear shape; its relative ordinates are assumed"
                .into(),
        conditions: input.conditions.clone(),
    };
    let family = Family {
        model: MODEL.into(),
        warp: curve(&[
            [-0.3, -90.0],
            [0.0, 0.0],
            [0.025, 7.5],
            [0.1, 45.0],
            [0.3, 185.0],
        ]),
        weft: curve(&[
            [-0.3, -60.0],
            [0.0, 0.0],
            [0.025, 5.0],
            [0.1, 27.5],
            [0.3, 107.5],
        ]),
        shear: curve(&[
            [-0.3, -24.0],
            [0.0, 0.0],
            [0.03, 2.4],
            [0.15, 14.4],
            [0.3, 38.4],
        ]),
        allow_estimated_shapes: true,
    };
    input.parameter_bounds = vec![[0.7, 1.5], [0.5, 1.3], [0.8, 1.6], [15.0, 65.0]];
    input.response_family = Some(family.clone());
    let mut truth = input.material.clone();
    let mut response = TabulatedMembrane {
        model: ftx_pattern::material_response::MODEL.into(),
        interpolation: Interpolation::PiecewiseLinear,
        warp: family.warp,
        weft: family.weft,
        shear: family.shear,
        coupling: Quantity {
            value: PARAMETERS[3],
            unit: "N/m".into(),
            evidence: Evidence::Estimate,
            source: "shape".into(),
            method: "Declared synthetic generating coupling; not a measured property".into(),
            conditions: input.conditions.clone(),
            bounds: None,
        },
    };
    for (curve, scale) in [&mut response.warp, &mut response.weft, &mut response.shear]
        .into_iter()
        .zip(PARAMETERS)
    {
        for point in &mut curve.points {
            point[1] *= scale;
        }
    }
    truth.membrane_response = Some(response);
    let material = Selection {
        id: truth.id.clone(),
        record: Some(truth),
        allow_estimates: true,
        ..Selection::default()
    };
    for experiment in &mut input.experiments {
        let extensions = if experiment.partition == Partition::Fit {
            vec![0.02, 0.06, 0.1]
        } else {
            vec![0.04]
        };
        let report = evaluate(&Request {
            schema: "ftx-strip-experiment/1".into(),
            material: material.clone(),
            fixture: experiment.fixture.clone(),
            extensions,
            controls: input.controls.clone(),
        })
        .unwrap();
        experiment.observations = report
            .states
            .into_iter()
            .enumerate()
            .map(|(i, state)| Observation {
                id: format!("{}-nonlinear-{i}", experiment.id),
                extension_fraction: state.extension_fraction,
                force_n: state.force_n,
                scale_n: 0.1,
                mid_width: Some(Width {
                    value_m: state.mid_width_m,
                    scale_m: 0.001,
                }),
            })
            .collect();
    }
    input
}

fn baseline() -> &'static Report {
    static REPORT: OnceLock<Report> = OnceLock::new();
    REPORT.get_or_init(|| calibrate(&input()).unwrap())
}

#[test]
fn identifies_declared_nonlinear_amplitudes_and_replays_the_actual_published_law() {
    let report = baseline();
    assert!(report.accepted, "{:?}", report.reasons);
    assert!(report.domain_replay_verified);
    for (actual, known) in report.parameters.iter().zip(PARAMETERS) {
        assert!((actual - known).abs() < 1e-4, "{actual} != {known}");
    }
    let descriptors = report.parameter_descriptors.as_ref().unwrap();
    assert_eq!(
        descriptors
            .iter()
            .map(|p| p.unit.as_str())
            .collect::<Vec<_>>(),
        ["1", "1", "1", "N/m"]
    );
    assert_eq!(descriptors[3].id, "normal_coupling");
    let material = report.material.as_ref().unwrap();
    assert!(material.properties.is_empty());
    let compiled = material.compiled_membrane(true).unwrap().unwrap();
    assert_eq!(compiled.strain_range(), report.fitted_green_strain_range);
    assert_eq!(
        fitted_domain(material).unwrap().unwrap().ranges(),
        report.fitted_green_strain_range
    );
    // Independent integration of the declared low-strain segments, including cross energy.
    let response = compiled.try_evaluate([0.04, 0.02, 0.01]).unwrap();
    assert!((response.energy - 0.36155).abs() < 1e-6);
    for (actual, known) in response.stress.into_iter().zip([17.3, 5.2, 0.96]) {
        assert!((actual - known).abs() < 1e-5, "{actual} != {known}");
    }
    let selected = report.selection.as_ref().unwrap();
    let snapshot = textile_import::catalog()
        .unwrap()
        .resolve(selected)
        .unwrap();
    assert_eq!(
        serde_json::to_string(&snapshot.frozen_selection()).unwrap(),
        report.selection_json.as_deref().unwrap()
    );
    for experiment in &report.input.experiments {
        let mut fixture = experiment.fixture.clone();
        fixture.cells.iter_mut().for_each(|n| *n *= 2);
        let actual = evaluate(&Request {
            schema: "ftx-strip-experiment/1".into(),
            material: selected.clone(),
            fixture,
            extensions: experiment
                .observations
                .iter()
                .map(|o| o.extension_fraction)
                .collect(),
            controls: report.input.controls.clone(),
        })
        .unwrap();
        for (state, prediction) in actual.states.iter().zip(
            report
                .predictions
                .iter()
                .filter(|p| p.experiment == experiment.id),
        ) {
            assert_eq!(
                serde_json::to_value(state).unwrap(),
                serde_json::to_value(&prediction.state).unwrap()
            );
        }
    }
    let serialized = serde_json::to_value(report).unwrap();
    assert_eq!(serialized["input"]["response_family"]["model"], MODEL);
    assert_eq!(serialized["parameter_descriptors"][3]["unit"], json!("N/m"));
}

fn unpublished(report: &Report) {
    assert!(!report.accepted);
    assert!(report.material.is_none());
    assert!(report.selection.is_none());
    assert!(report.selection_json.is_none());
    assert!(!report.domain_replay_verified);
    assert!(!report.reasons.is_empty());
    assert!(!report.predictions.is_empty());
}

#[test]
fn shape_acquisition_and_output_estimate_permissions_are_independent() {
    let original = input();
    let mut shapes = original.clone();
    shapes
        .response_family
        .as_mut()
        .unwrap()
        .allow_estimated_shapes = false;
    let refusal = calibrate(&shapes).unwrap_err();
    assert!(refusal.message.contains("allow_estimated_shapes"));
    assert!(shapes.allow_estimated_observations && shapes.allow_estimates);

    let mut observations = original.clone();
    observations.allow_estimated_observations = false;
    assert!(
        calibrate(&observations)
            .unwrap_err()
            .message
            .contains("evidence")
    );
    assert!(
        observations
            .response_family
            .as_ref()
            .unwrap()
            .allow_estimated_shapes
    );

    let mut output = original;
    output.allow_estimates = false;
    let report = calibrate(&output).unwrap();
    assert!(report.accepted, "{:?}", report.reasons);
    assert_eq!(report.parameters, baseline().parameters);
    assert_ne!(report.input_sha256, baseline().input_sha256);
    assert_eq!(
        report.input_sha256,
        sha256_hex(&serde_json::to_vec(&output).unwrap())
    );
    let selected = report.selection.as_ref().unwrap();
    assert!(!selected.allow_estimates);
    let request = Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selected.clone(),
        fixture: output.experiments[0].fixture.clone(),
        extensions: vec![0.02],
        controls: output.controls.clone(),
    };
    assert!(evaluate(&request).unwrap_err().message.contains("estimate"));
    let mut changed = request;
    changed.material.allow_estimates = true;
    assert!(
        evaluate(&changed)
            .unwrap_err()
            .message
            .contains("expected_sha256")
    );
}

#[test]
fn held_out_errors_do_not_choose_coefficients_and_work_gates_do_not_publish() {
    let original = input();
    let mut changed = original.clone();
    changed.experiments.last_mut().unwrap().observations[0].force_n += 10.0;
    let rejected = calibrate(&changed).unwrap();
    unpublished(&rejected);
    assert_eq!(rejected.parameters, baseline().parameters);
    assert_eq!(
        rejected.fitted_green_strain_range,
        baseline().fitted_green_strain_range
    );
    assert!(rejected.check.unwrap().peak_scaled > 90.0);

    let mut unfinished = original.clone();
    unfinished.policy.max_iterations = 1;
    let rejected = calibrate(&unfinished).unwrap();
    unpublished(&rejected);
    assert!(rejected.reasons.iter().any(|r| r.contains("stationarity")));

    let mut mesh = original.clone();
    mesh.policy.max_mesh_scaled = 1e-12;
    let rejected = calibrate(&mesh).unwrap();
    unpublished(&rejected);
    assert!(rejected.reasons.iter().any(|r| r.contains("doubling")));

    // Removing one admitted attempt from the complete successful operation exhausts replay;
    // its earlier stationary fit cannot be returned as an accepted partial result.
    let mut limited = original;
    limited.policy.max_forward_solves = baseline().forward_solves - 1;
    assert_eq!(
        calibrate(&limited).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn zero_load_channels_cannot_identify_declared_shape_amplitudes() {
    let mut input = input();
    // Signed coupling is admissible in this family; zero loading still supplies no information.
    input.parameter_bounds[3] = [-65.0, -15.0];
    for experiment in &mut input.experiments {
        experiment.observations = vec![Observation {
            id: format!("{}-zero", experiment.id),
            extension_fraction: 0.0,
            force_n: 0.0,
            scale_n: 0.1,
            mid_width: Some(Width {
                value_m: experiment.fixture.width_m,
                scale_m: 0.001,
            }),
        }];
    }
    let rejected = calibrate(&input).unwrap();
    unpublished(&rejected);
    assert!(rejected.parameters[3] < 0.0);
    assert_eq!(rejected.excitation_pivot, 0.0);
    assert!(
        rejected
            .reasons
            .iter()
            .any(|r| r.contains("do not independently identify"))
    );
}

#[test]
fn supplied_shapes_and_entire_parameter_box_remain_authoritative() {
    let original = input();
    let mut unstable = original.clone();
    unstable.parameter_bounds[3] = [-1000.0, 1000.0];
    assert!(
        calibrate(&unstable)
            .unwrap_err()
            .message
            .contains("coupling")
    );

    let mut negative_scale = original.clone();
    negative_scale.parameter_bounds[2][0] = 0.0;
    assert!(calibrate(&negative_scale).is_err());

    let mut one_sided = original.clone();
    one_sided.response_family.as_mut().unwrap().warp.points =
        vec![[0.0, 0.0], [0.025, 7.5], [0.1, 45.0]];
    assert!(
        calibrate(&one_sided)
            .unwrap_err()
            .message
            .contains("negative and positive")
    );

    let mut narrow = original.clone();
    let family = narrow.response_family.as_mut().unwrap();
    for curve in [&mut family.warp, &mut family.weft, &mut family.shear] {
        curve.points = vec![[-0.001, -0.3], [0.0, 0.0], [0.001, 0.3]];
    }
    assert_eq!(
        calibrate(&narrow).unwrap_err().code,
        RefusalCode::OutOfRange
    );

    let mut partial_prior = original.clone();
    partial_prior
        .response_family
        .as_mut()
        .unwrap()
        .warp
        .conditions
        .insert(
            "fitted_green_strain_range".into(),
            "[[-0.001,0.001],[-0.001,0.001],[-0.001,0.001]]".into(),
        );
    assert!(
        calibrate(&partial_prior)
            .unwrap_err()
            .message
            .contains("coupling/support group")
    );
    // Stripping the range does not turn previously fitted source evidence into an unrestricted
    // shape declaration. The original complete coupled authority must remain distinguishable.
    let mut stripped = original;
    stripped
        .material
        .sources
        .get_mut("shape")
        .unwrap()
        .reference = "local:ftx-strip-calibration/synthetic-prior-receipt".into();
    assert!(
        calibrate(&stripped)
            .unwrap_err()
            .message
            .contains("coupling/support group")
    );
}

#[test]
fn nonlinear_replacement_preserves_prior_curve_and_fitted_support() {
    let mut input = input();
    let family = input.response_family.as_ref().unwrap();
    let mut prior = baseline().material.as_ref().unwrap().clone();
    let response = prior.membrane_response.as_mut().unwrap();
    response.warp = family.warp.clone();
    response.weft = family.weft.clone();
    response.shear = family.shear.clone();
    response.coupling.source = "shape".into();
    response.coupling.conditions = input.conditions.clone();
    response.coupling.method =
        "Explicit synthetic prior authored response, not a measurement".into();
    input.material = prior;
    assert!(
        calibrate(&input)
            .unwrap_err()
            .message
            .contains("replace_existing")
    );
    input.replace_existing = true;
    let accepted = calibrate(&input).unwrap();
    assert!(accepted.accepted, "{:?}", accepted.reasons);
    assert_eq!(accepted.parameters, baseline().parameters);
    assert!(accepted.domain_replay_verified);
    assert!(accepted.material.as_ref().unwrap().properties.is_empty());

    // The original standalone nonlinear curve support also constrains replacement, even when
    // no fitted-domain metadata exists and the new declared shapes themselves are broader.
    let mut narrow = input.clone();
    let response = narrow.material.membrane_response.as_mut().unwrap();
    for curve in [&mut response.warp, &mut response.weft, &mut response.shear] {
        curve.points = vec![[-0.001, -0.3], [0.0, 0.0], [0.001, 0.3]];
    }
    assert!(fitted_domain(&narrow.material).unwrap().is_none());
    assert_eq!(
        calibrate(&narrow).unwrap_err().code,
        RefusalCode::OutOfRange
    );

    // An independently narrower prior fitted domain remains active on top of broad curve knots.
    input.material = baseline().material.as_ref().unwrap().clone();
    let response = input.material.membrane_response.as_mut().unwrap();
    for conditions in [
        &mut response.warp.conditions,
        &mut response.weft.conditions,
        &mut response.shear.conditions,
        &mut response.coupling.conditions,
    ] {
        conditions.insert(
            "fitted_green_strain_range".into(),
            "[[-0.001,0.001],[-0.001,0.001],[-0.001,0.001]]".into(),
        );
    }
    assert_eq!(calibrate(&input).unwrap_err().code, RefusalCode::OutOfRange);
}

#[test]
fn absent_family_preserves_existing_input_hash_and_frozen_scalar_selection() {
    let input: Input = serde_json::from_str(include_str!(
        "../../../examples/strip-identification/synthetic-calibration.json"
    ))
    .unwrap();
    let forward: Request = serde_json::from_str(include_str!(
        "../../../examples/strip-identification/synthetic-forward.json"
    ))
    .unwrap();
    let report = calibrate(&input).unwrap();
    assert!(report.accepted);
    assert!(report.parameter_descriptors.is_none());
    assert!(
        serde_json::to_value(&report)
            .unwrap()
            .get("parameter_descriptors")
            .is_none()
    );
    assert!(
        serde_json::to_value(&input)
            .unwrap()
            .get("response_family")
            .is_none()
    );
    assert_eq!(
        report.input_sha256,
        sha256_hex(&serde_json::to_vec(&input).unwrap())
    );
    assert_eq!(
        serde_json::to_value(report.selection.unwrap()).unwrap(),
        serde_json::to_value(forward.material).unwrap()
    );
}
