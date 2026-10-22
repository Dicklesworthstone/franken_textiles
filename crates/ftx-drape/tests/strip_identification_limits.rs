//! Inverse-limit and evidence-policy checks using explicitly synthetic strip acquisitions.
//! Generated observations test software behavior, not measured textile characterization.

use ftx_core::receipt::sha256_hex;
use ftx_drape::strip::{
    Controls, Fixture, Request, evaluate,
    fit::{Experiment, Input, Observation, Report, Width, calibrate},
    solve,
};
use ftx_pattern::{
    material_calibration::{Partition, fitted_domain},
    material_catalog::Selection,
    materials::{Evidence, Quantity, Source},
    textile_import,
};
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
                    extensometers: vec![],
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
            extensometers: vec![],
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

fn add_prior_domain(input: &mut Input, ranges: [[f64; 2]; 3]) {
    let mut conditions = input.conditions.clone();
    conditions.insert(
        "fitted_green_strain_range".into(),
        serde_json::to_string(&ranges).unwrap(),
    );
    for (key, value) in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
    ]
    .into_iter()
    .zip(PARAMETERS)
    {
        input.material.properties.insert(
            key.into(),
            Quantity {
                value,
                unit: if key == "poisson_warp_weft" {
                    "1"
                } else {
                    "N/m"
                }
                .into(),
                evidence: Evidence::Estimate,
                source: "fit-warp".into(),
                method: "Explicit synthetic prior characterized support for replacement regression"
                    .into(),
                conditions: conditions.clone(),
                bounds: None,
            },
        );
    }
}

#[test]
fn replacing_coefficients_cannot_erase_the_original_strain_domain() {
    let mut narrow = synthetic(PARAMETERS);
    add_prior_domain(&mut narrow, [[-0.001, 0.001]; 3]);
    assert!(
        calibrate(&narrow)
            .unwrap_err()
            .message
            .contains("replace_existing")
    );
    narrow.replace_existing = true;
    let before = serde_json::to_value(&narrow).unwrap();
    let refused = calibrate(&narrow).unwrap_err();
    assert_eq!(refused.code, ftx_core::RefusalCode::OutOfRange);
    assert!(
        refused.message.contains("outside fitted ranges"),
        "{refused:?}"
    );
    assert_eq!(serde_json::to_value(&narrow).unwrap(), before);

    // Even a numerically admissible positive-only prior cannot be silently extended to rest.
    add_prior_domain(&mut narrow, [[0.001, 1.0]; 3]);
    assert!(
        calibrate(&narrow)
            .unwrap_err()
            .message
            .contains("undeformed reference")
    );

    let mut bounded = synthetic(PARAMETERS);
    let original_domain = [[-1.0, 1.0]; 3];
    add_prior_domain(&mut bounded, original_domain);
    bounded.replace_existing = true;
    let before = serde_json::to_value(&bounded).unwrap();
    let report = calibrate(&bounded).unwrap();
    assert!(report.accepted, "{:?}", report.reasons);
    assert!(report.domain_replay_verified);
    assert_eq!(serde_json::to_value(&report.input).unwrap(), before);
    for (fitted, prior) in report.fitted_green_strain_range.iter().zip(original_domain) {
        assert!(fitted[0] >= prior[0] && fitted[1] <= prior[1]);
        assert!(fitted[0] <= 0.0 && fitted[1] >= 0.0);
    }
    let mut original_support = report.material.as_ref().unwrap().clone();
    for property in original_support.properties.values_mut() {
        property.conditions.insert(
            "fitted_green_strain_range".into(),
            serde_json::to_string(&original_domain).unwrap(),
        );
    }
    assert_eq!(
        fitted_domain(&original_support).unwrap().unwrap().ranges(),
        original_domain
    );
    let inherited_selection = textile_import::catalog()
        .unwrap()
        .resolve(&Selection {
            id: original_support.id.clone(),
            record: Some(original_support),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection();
    for experiment in &report.input.experiments {
        for factor in [1, 2] {
            let mut fixture = experiment.fixture.clone();
            fixture.cells.iter_mut().for_each(|n| *n *= factor);
            let mut request = Request {
                schema: "ftx-strip-experiment/1".into(),
                material: report.selection.as_ref().unwrap().clone(),
                fixture,
                extensions: experiment
                    .observations
                    .iter()
                    .map(|o| o.extension_fraction)
                    .collect(),
                controls: controls(),
            };
            let fitted = evaluate(&request).unwrap();
            request.material = inherited_selection.clone();
            let inherited = evaluate(&request).unwrap();
            assert_eq!(
                serde_json::to_value(&fitted.states).unwrap(),
                serde_json::to_value(&inherited.states).unwrap()
            );
        }
    }
}

#[test]
fn canonical_fixture_retains_known_mass_without_inventing_other_mechanics() {
    let mut input = synthetic(PARAMETERS);
    input.material.id = "synthetic-strip-4coefficient".into();
    input.material.name =
        "Synthetic finite-strip inverse numerical test (not physical textile data)".into();
    input
        .material
        .structure
        .insert("scope".into(), "synthetic numerical recovery test".into());
    input
        .conditions
        .insert("specimen_id".into(), input.material.id.clone());
    input.conditions.insert(
        "conditioning".into(),
        "synthetic numerical test; no physical coupon".into(),
    );
    input.method = "Synthetic independent planar ideal-grip elastic states; 100 mm free gauge, 60 mm width, explicit midpoint side-point chord width. No physical coupon, source metrology, history or empirical validation is inferred.".into();
    input.material.sources.insert("synthetic".into(), Source {
        reference: "local:synthetic-clamped-strip-4coefficient".into(), accessed_on: "2026-10-21".into(),
        note: "Explicit synthetic constitutive values [warp 300 N/m, weft 180 N/m, major Poisson 0.2, shear 70 N/m] generate actual finite-strip force and midpoint chord-width observations. Independent source IDs denote fresh numerical acquisitions, not physical validation. Areal mass 0.2 kg/m2 is an independent synthetic observation and does not enter these zero-gravity static equilibria.".into(),
    });
    input.material.properties.insert("areal_density".into(), Quantity {
        value: 0.2, unit: "kg/m2".into(), evidence: Evidence::Estimate, source: "synthetic".into(),
        method: "Explicit synthetic areal mass, retained independently of static force identification".into(),
        conditions: input.conditions.clone(), bounds: None,
    });
    for experiment in &mut input.experiments {
        experiment.fixture.width_m = 0.06;
        experiment.fixture.gauge_length_m = 0.1;
        for observation in &mut experiment.observations {
            let state = solve(
                &experiment.fixture,
                PARAMETERS,
                observation.extension_fraction,
                &input.controls,
            )
            .unwrap();
            observation.force_n = state.force_n;
            observation.mid_width.as_mut().unwrap().value_m = state.mid_width_m;
        }
    }
    let report = calibrate(&input).unwrap();
    assert!(report.accepted, "{:?}", report.reasons);
    assert!(report.domain_replay_verified);
    assert!(report.check.is_some());
    assert_eq!(report.material.as_ref().unwrap().properties.len(), 5);
    assert_eq!(
        serde_json::to_value(&report.material.as_ref().unwrap().properties["areal_density"])
            .unwrap(),
        serde_json::to_value(&input.material.properties["areal_density"]).unwrap()
    );
    assert!(report.material.as_ref().unwrap().mechanics(true).is_err());
    let fixture: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-clamped-strip.json"
    ))
    .unwrap();
    assert_eq!(
        serde_json::to_value(&fixture).unwrap(),
        serde_json::to_value(&input).unwrap()
    );
}
