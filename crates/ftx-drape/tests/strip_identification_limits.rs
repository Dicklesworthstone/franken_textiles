//! Inverse-limit and evidence-policy checks using explicitly synthetic strip acquisitions.
//! Generated observations test software behavior, not measured textile characterization.

use ftx_core::receipt::sha256_hex;
use ftx_drape::strip::{
    Controls, Fixture, Request, evaluate,
    fit::{Experiment, Input, Observation, Report, Width, calibrate},
    solve,
};
use ftx_pattern::{material_calibration::Partition, materials::Evidence, textile_import};
use serde_json::json;

const PARAMETERS: [f64; 4] = [300.0, 180.0, 0.2, 70.0];

fn controls() -> Controls {
    Controls {
        tolerance_m: 1e-9,
        force_tolerance_n: 1e-6,
        max_newton: 100,
    }
}

fn synthetic(parameters: [f64; 4]) -> Input {
    let mut input: Input = serde_json::from_value(json!({
        "schema": "ftx-strip-calibration/1",
        "material": {
            "id": "synthetic-strip-limits", "name": "Synthetic inverse-limit software test",
            "form": "woven", "sources": {}
        },
        "method": "Synthetic planar ideal-grip elastic experiments; no real specimen or empirical validation",
        "conditions": {"provenance": "numerical synthetic acquisition"},
        "fitted_on": "2026-10-21", "experiments": [],
        "parameter_bounds": [[200.0,400.0],[100.0,260.0],[0.05,0.35],[40.0,100.0]],
        "admit_planar_elastic_model": true, "allow_estimated_observations": true,
        "allow_estimates": true, "controls": controls(),
        "policy": {
            "max_rms_scaled": 1.0, "max_peak_scaled": 1.0, "max_mesh_scaled": 1.0,
            "min_excitation_pivot": 1e-4, "max_iterations": 40, "max_forward_solves": 8000
        }
    })).unwrap();
    let bias = std::f64::consts::FRAC_1_SQRT_2;
    for (id, axis, partition, extensions) in [
        ("fit-warp", [0.0, 1.0], Partition::Fit, vec![0.04, 0.08]),
        ("fit-weft", [1.0, 0.0], Partition::Fit, vec![0.04, 0.08]),
        ("fit-bias", [bias, bias], Partition::Fit, vec![0.04, 0.08]),
        ("check-warp", [0.0, 1.0], Partition::Check, vec![0.03]),
    ] {
        // Binary-exact dimensions make the later zero-load rank-deficiency case exact too.
        let fixture = Fixture {
            gauge_length_m: 0.125,
            width_m: 0.0625,
            warp_axis: axis,
            cells: [2, 2],
        };
        let observations = extensions
            .into_iter()
            .enumerate()
            .map(|(i, extension)| {
                let state = solve(&fixture, parameters, extension, &controls()).unwrap();
                Observation {
                    id: format!("{id}-{i}"),
                    extension_fraction: extension,
                    force_n: state.force_n,
                    scale_n: 0.1,
                    mid_width: Some(Width {
                        value_m: state.mid_width_m,
                        scale_m: 0.001,
                    }),
                }
            })
            .collect();
        input.material.sources.insert(id.into(), serde_json::from_value(json!({
            "reference": format!("test:synthetic-strip-limits/{id}"),
            "accessed_on": "2026-10-21",
            "note": "Numerical synthetic acquisition; not a measured coupon or empirical independence claim"
        })).unwrap());
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

fn unpublished(report: &Report) {
    assert!(
        !report.accepted,
        "unexpected acceptance: {:?}",
        report.parameters
    );
    assert!(report.material.is_none());
    assert!(report.selection.is_none());
    assert!(report.selection_json.is_none());
    assert!(!report.domain_replay_verified);
    assert!(!report.predictions.is_empty());
    assert!(report.parameters.iter().all(|p| p.is_finite()));
}

#[test]
fn many_exact_zero_load_channels_cannot_identify_any_stiffness() {
    let mut input = synthetic(PARAMETERS);
    for experiment in &mut input.experiments {
        experiment.fixture.warp_axis = [0.0, 1.0];
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
    // Six FIT scalar channels and independent CHECK evidence do not create load excitation.
    let report = calibrate(&input).unwrap();
    unpublished(&report);
    assert_eq!(report.fit.components, 6);
    assert_eq!(report.fit.rms_scaled, 0.0);
    assert_eq!(report.check.as_ref().unwrap().rms_scaled, 0.0);
    assert_eq!(report.excitation_pivot, 0.0);
    assert!(report.starts.iter().all(|start| start.converged));
    assert!(
        report
            .reasons
            .iter()
            .any(|reason| reason.contains("do not independently identify"))
    );
    assert!(
        report
            .reasons
            .iter()
            .any(|reason| reason.contains("equivalent FIT minima"))
    );
    assert!(report.forward_solves <= input.policy.max_forward_solves);
}

#[test]
fn iteration_exhaustion_and_boundary_minima_keep_candidates_without_publication() {
    let mut limited = synthetic(PARAMETERS);
    limited.policy.max_iterations = 1;
    let report = calibrate(&limited).unwrap();
    unpublished(&report);
    assert!(report.starts.iter().all(|start| start.iterations == 1));
    assert!(report.starts.iter().any(|start| !start.converged));
    assert!(
        report
            .reasons
            .iter()
            .any(|reason| reason.contains("objective stationarity"))
    );
    assert!(report.starts.iter().any(|start| start.fit_sse < 1e-16));

    // Zero true Poisson coupling lies on this explicitly declared search boundary. Even an
    // accurate residual and independently excited data cannot justify an interior-fit claim.
    let mut bounded = synthetic([300.0, 180.0, 0.0, 70.0]);
    bounded.parameter_bounds[2] = [0.0, 0.35];
    let report = calibrate(&bounded).unwrap();
    unpublished(&report);
    assert!(report.parameters[2] < 1e-4);
    assert!(
        report
            .reasons
            .iter()
            .any(|reason| reason.contains("parameter bound"))
    );
}

#[test]
fn output_estimate_admission_is_separate_from_fit_and_is_bound_into_the_snapshot() {
    let mut input = synthetic(PARAMETERS);
    input.allow_estimates = false;
    // Renaming a source key, changing the numerical mesh and reversing both material axes
    // cannot turn the same physical acquisition into independent held-out evidence.
    let mut reused = input.clone();
    let original_source = reused.material.sources["fit-warp"].clone();
    reused
        .material
        .sources
        .insert("renamed-copy".into(), original_source);
    let check = reused.experiments.last_mut().unwrap();
    check.source = "renamed-copy".into();
    check.fixture.warp_axis = [-0.0, -1.0];
    check.fixture.cells = [4, 4];
    assert!(
        calibrate(&reused)
            .unwrap_err()
            .message
            .contains("same source acquisition")
    );

    let report = calibrate(&input).unwrap();
    assert!(report.accepted, "{:?}", report.reasons);
    assert!(report.domain_replay_verified);
    let digest = sha256_hex(&serde_json::to_vec(&input).unwrap());
    assert_eq!(report.input_sha256, digest);
    let material = report.material.as_ref().unwrap();
    let source = format!("strip-{digest}");
    assert_eq!(
        material.sources[&source].reference,
        format!("local:ftx-strip-calibration/{digest}")
    );
    assert!(
        material
            .properties
            .values()
            .all(|q| q.evidence == Evidence::Estimate && q.source == source && q.bounds.is_none())
    );
    let selection = report.selection.as_ref().unwrap();
    assert!(!selection.allow_estimates);
    assert_eq!(
        report.selection_json.as_deref().unwrap(),
        serde_json::to_string(selection).unwrap()
    );
    let mut forward = Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selection.clone(),
        fixture: input.experiments[0].fixture.clone(),
        extensions: vec![0.04],
        controls: controls(),
    };
    assert!(evaluate(&forward).unwrap_err().message.contains("estimate"));
    forward.material.allow_estimates = true;
    assert!(
        evaluate(&forward)
            .unwrap_err()
            .message
            .contains("expected_sha256")
    );
    // Explicitly resolve a new policy-bearing snapshot; never edit the old lock in place.
    forward.material.expected_sha256 = None;
    forward.material = textile_import::catalog()
        .unwrap()
        .resolve(&forward.material)
        .unwrap()
        .frozen_selection();
    assert_ne!(forward.material.expected_sha256, selection.expected_sha256);
    assert!(evaluate(&forward).is_ok());
}
