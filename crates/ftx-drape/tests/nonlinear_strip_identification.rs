//! Synthetic finite-strip identification, with separately authored signed stress curves.
//! These specimens are numerical fixtures and make no measured-knit or garment claim.

use std::{collections::BTreeMap, sync::OnceLock};

use ftx_core::RefusalCode;
use ftx_drape::strip::{
    Controls, Fixture, Request, evaluate,
    fit::{Experiment, Input, Observation, Policy, Report, Width, calibrate, nonlinear::Nonlinear},
};
use ftx_pattern::{
    material_calibration::Partition,
    material_catalog::Selection,
    material_response::{Interpolation, StressCurve, TabulatedMembrane},
    materials::{Evidence, Form, Material, Quantity, Source},
};

const PARAMETERS: [f64; 9] = [260.0, 300.0, 500.0, 150.0, 180.0, 320.0, 70.0, 90.0, 25.0];

fn controls() -> Controls {
    Controls {
        tolerance_m: 1e-9,
        force_tolerance_n: 1e-6,
        max_newton: 100,
    }
}

fn material() -> Material {
    Material {
        id: "synthetic-nonlinear-strip".into(),
        name: "Synthetic signed nonlinear strip identification; no real textile".into(),
        form: Form::Knit,
        composition: vec![], composition_complete: false,
        structure: BTreeMap::new(), properties: BTreeMap::new(),
        membrane_response: None, membrane_relaxation: None, thermal_response: None,
        optical_faces: None,
        sources: BTreeMap::from([("planted".into(), Source {
            reference: "test:synthetic-nonlinear-strip/planted".into(),
            accessed_on: "2026-10-21".into(),
            note: "Synthetic independent signed tangent parameters [260,300,500,150,180,320,70,90,25] N/m; production forward observations are numerical checks, not measured specimen evidence".into(),
        })]),
    }
}

fn planted() -> Material {
    let mut material = material();
    let curve = |points| StressCurve {
        points,
        source: "planted".into(),
        evidence: Evidence::Estimate,
        method: "Separately authored exact signed piecewise-linear synthetic stress curve".into(),
        conditions: BTreeMap::new(),
    };
    material.membrane_response = Some(TabulatedMembrane {
        model: "tabulated_green_strain/1".into(),
        interpolation: Interpolation::PiecewiseLinear,
        warp: curve(vec![[-0.2, -52.0], [0.0, 0.0], [0.04, 12.0], [0.25, 117.0]]),
        weft: curve(vec![[-0.2, -30.0], [0.0, 0.0], [0.04, 7.2], [0.25, 74.4]]),
        shear: curve(vec![[-0.3, -21.0], [0.0, 0.0], [0.3, 27.0]]),
        coupling: Quantity {
            value: 25.0,
            unit: "N/m".into(),
            evidence: Evidence::Estimate,
            source: "planted".into(),
            method: "Synthetic independently authored coupling".into(),
            conditions: BTreeMap::new(),
            bounds: None,
        },
    });
    material
}

fn input() -> Input {
    let mut input: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-clamped-strip.json"
    ))
    .unwrap();
    input.material = material();
    input
        .conditions
        .insert("specimen_id".into(), input.material.id.clone());
    input.parameter_bounds.clear();
    input.nonlinear = Some(Nonlinear {
        strain_knots: [
            vec![-0.2, 0.0, 0.04, 0.25],
            vec![-0.2, 0.0, 0.04, 0.25],
            vec![-0.3, 0.0, 0.3],
        ],
        tangent_bounds: [
            vec![[220.0, 300.0], [260.0, 340.0], [440.0, 560.0]],
            vec![[120.0, 180.0], [150.0, 210.0], [280.0, 360.0]],
            vec![[50.0, 90.0], [70.0, 110.0]],
        ],
        coupling_bounds: [15.0, 35.0],
    });
    input.method = "Synthetic full-width ideal planar clamped strips; every signed tangent interval is independent, positive branches stiffen at declared Green strain 0.04; no assumed symmetry, measured textile, rate or conditioning".into();
    input.controls = controls();
    input.policy = Policy {
        max_rms_scaled: 2.0,
        max_peak_scaled: 2.0,
        max_mesh_scaled: 2.0,
        min_excitation_pivot: 1e-7,
        max_iterations: 40,
        max_forward_solves: 32768,
    };
    input.experiments.clear();
    let bias = std::f64::consts::FRAC_1_SQRT_2;
    for (id, axis, partition, extensions) in [
        (
            "fit-warp",
            [0.0, 1.0],
            Partition::Fit,
            vec![0.03, 0.08, 0.14],
        ),
        (
            "fit-weft",
            [1.0, 0.0],
            Partition::Fit,
            vec![0.03, 0.08, 0.14],
        ),
        (
            "fit-bias",
            [bias, bias],
            Partition::Fit,
            vec![0.03, 0.08, 0.14],
        ),
        (
            "fit-reverse-bias",
            [-bias, bias],
            Partition::Fit,
            vec![0.03, 0.08, 0.14],
        ),
        (
            "fit-oblique",
            [0.6, 0.8],
            Partition::Fit,
            vec![0.03, 0.08, 0.14],
        ),
        ("check-warp", [0.0, 1.0], Partition::Check, vec![0.06]),
        ("check-bias", [bias, bias], Partition::Check, vec![0.06]),
    ] {
        let fixture = Fixture {
            gauge_length_m: 0.1,
            width_m: 0.06,
            warp_axis: axis,
            cells: [2, 2],
        };
        let truth = planted();
        let forward = evaluate(&Request {
            schema: "ftx-strip-experiment/1".into(),
            material: Selection {
                id: truth.id.clone(),
                record: Some(truth),
                allow_estimates: true,
                ..Selection::default()
            },
            fixture: fixture.clone(),
            extensions,
            controls: controls(),
        })
        .unwrap();
        let observations = forward
            .states
            .into_iter()
            .enumerate()
            .map(|(i, state)| Observation {
                id: format!("{id}-{i}"),
                extension_fraction: state.extension_fraction,
                force_n: state.force_n,
                scale_n: 0.2,
                mid_width: Some(Width {
                    value_m: state.mid_width_m,
                    scale_m: 0.002,
                }),
            })
            .collect();
        input.material.sources.insert(id.into(), Source {
            reference: format!("test:synthetic-nonlinear-strip/{id}"), accessed_on: "2026-10-21".into(),
            note: "Synthetic solver-generated acquisition; whole CHECK acquisition excluded from identification; no empirical independence claim".into(),
        });
        input.experiments.push(Experiment {
            id: id.into(),
            fixture,
            source: id.into(),
            evidence: Evidence::Estimate,
            partition,
            observations,
        });
    }
    input
}

fn baseline() -> &'static Report {
    static REPORT: OnceLock<Report> = OnceLock::new();
    REPORT.get_or_init(|| {
        let input = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-nonlinear-clamped-strip.json"
        ))
        .unwrap();
        calibrate(&input).unwrap()
    })
}

#[test]
fn retained_synthetic_acquisition_reconstructs_the_authored_nonlinear_law() {
    let retained: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear-clamped-strip.json"
    ))
    .unwrap();
    assert_eq!(
        serde_json::to_value(retained).unwrap(),
        serde_json::to_value(input()).unwrap()
    );
}

#[test]
fn identifies_signed_nonlinearity_and_replays_the_published_ordinary_selection() {
    let report = baseline();
    assert!(
        report.accepted,
        "{:?}; parameters {:?}; fit {:?}; check {:?}; mesh {}; pivot {}; starts {:?}",
        report.reasons,
        report.parameters,
        report.fit,
        report.check,
        report.mesh_change_peak_scaled,
        report.excitation_pivot,
        report.starts
    );
    assert!(report.domain_replay_verified);
    assert_eq!(report.parameters.len(), 9);
    assert_eq!(report.parameter_descriptors.as_ref().unwrap().len(), 9);
    let descriptors = report.parameter_descriptors.as_ref().unwrap();
    assert_eq!(descriptors[0].id, "warp_tangent_0");
    assert_eq!(descriptors[8].id, "normal_coupling");
    assert!(descriptors.iter().all(|parameter| parameter.unit == "N/m"));
    assert_eq!(report.parameter_bounds.len(), 9);
    for (actual, expected) in report.parameters.iter().zip(PARAMETERS) {
        assert!(
            (actual - expected).abs() < 0.002 * expected,
            "{actual} != {expected}"
        );
    }
    let material = report.material.as_ref().unwrap();
    assert!(
        material.properties.is_empty(),
        "nonlinear fit must not retain competing scalar coefficients"
    );
    let response = material.membrane_response.as_ref().unwrap();
    for (curve, range) in [&response.warp, &response.weft, &response.shear]
        .into_iter()
        .zip(report.fitted_green_strain_range)
    {
        assert_eq!(curve.points[0][0], range[0]);
        assert_eq!(curve.points.last().unwrap()[0], range[1]);
        assert_eq!(curve.evidence, Evidence::Estimate);
    }
    let selection = report.selection.as_ref().unwrap();
    assert_eq!(
        report.selection_json.as_deref().unwrap(),
        serde_json::to_string(selection).unwrap()
    );
    assert!(
        material.mechanics(true).is_err(),
        "unidentified bending/contact/mass stay unavailable"
    );
    for experiment in &report.input.experiments {
        let mut fixture = experiment.fixture.clone();
        fixture.cells.iter_mut().for_each(|n| *n *= 2);
        let replay = evaluate(&Request {
            schema: "ftx-strip-experiment/1".into(),
            material: selection.clone(),
            fixture,
            extensions: experiment
                .observations
                .iter()
                .map(|p| p.extension_fraction)
                .collect(),
            controls: controls(),
        })
        .unwrap();
        for (point, state) in experiment.observations.iter().zip(replay.states) {
            let prediction = report
                .predictions
                .iter()
                .find(|p| p.experiment == experiment.id && p.observation == point.id)
                .unwrap();
            assert!((state.force_n - prediction.state.force_n).abs() < 1e-7);
            assert!((state.mid_width_m - prediction.state.mid_width_m).abs() < 1e-9);
        }
    }
}

#[test]
fn nonlinear_bounds_and_aggregate_work_are_not_silently_repaired() {
    let mut input = input();
    input.nonlinear.as_mut().unwrap().coupling_bounds = [15.0, 300.0];
    assert!(
        calibrate(&input)
            .unwrap_err()
            .message
            .contains("positive-definite")
    );
    input.nonlinear.as_mut().unwrap().coupling_bounds = [15.0, 35.0];
    input.parameter_bounds = vec![[1.0, 2.0]; 4];
    assert!(
        calibrate(&input)
            .unwrap_err()
            .message
            .contains("omit scalar")
    );
    input.parameter_bounds.clear();
    let response = planted().membrane_response.unwrap();
    input.response_family = Some(ftx_drape::strip::fit::family::Family {
        model: ftx_drape::strip::fit::family::MODEL.into(),
        warp: response.warp,
        weft: response.weft,
        shear: response.shear,
        allow_estimated_shapes: true,
    });
    assert!(
        calibrate(&input)
            .unwrap_err()
            .message
            .contains("mutually exclusive")
    );
    input.response_family = None;
    input.policy.max_forward_solves = 3;
    assert_eq!(
        calibrate(&input).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn unused_nonlinear_segments_cannot_be_published() {
    let mut input = input();
    let model = input.nonlinear.as_mut().unwrap();
    model.strain_knots[0].insert(3, 0.2);
    model.tangent_bounds[0].push([440.0, 560.0]);
    let report = calibrate(&input).unwrap();
    assert!(!report.accepted);
    assert!(report.material.is_none() && report.selection.is_none());
    assert!(
        report
            .reasons
            .iter()
            .any(|r| r.contains("independently identify")),
        "{:?}",
        report.reasons
    );
}

#[test]
fn nonlinear_checks_never_select_parameters_or_extend_inherited_support() {
    let baseline = baseline();
    let mut changed = baseline.input.clone();
    for experiment in &mut changed.experiments {
        if experiment.partition == Partition::Check {
            for point in &mut experiment.observations {
                point.force_n += 3.0;
            }
        }
    }
    let rejected = calibrate(&changed).unwrap();
    assert_eq!(rejected.parameters, baseline.parameters);
    assert!(!rejected.accepted && rejected.material.is_none());
    assert!(
        rejected
            .reasons
            .iter()
            .any(|reason| reason.contains("CHECK force/width"))
    );

    let mut replacement = baseline.input.clone();
    let mut prior = planted();
    prior.sources.extend(replacement.material.sources.clone());
    prior.membrane_response.as_mut().unwrap().warp.points =
        vec![[-0.01, -2.6], [0.0, 0.0], [0.01, 3.0]];
    replacement.material = prior;
    assert!(
        calibrate(&replacement)
            .unwrap_err()
            .message
            .contains("replace_existing")
    );
    replacement.replace_existing = true;
    assert_eq!(
        calibrate(&replacement).unwrap_err().code,
        RefusalCode::OutOfRange
    );
}
