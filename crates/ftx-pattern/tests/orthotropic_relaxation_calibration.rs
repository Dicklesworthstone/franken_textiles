//! Synthetic independent convolutions identify directional springs, not measured fabric laws.
use ftx_pattern::{
    material_calibration::Partition,
    material_catalog::Selection,
    material_relaxation::{
        Initialization,
        calibration::{Input, Report, calibrate},
    },
    materials::{Catalog, Evidence},
};

const TENSORS: [[f64; 4]; 4] = [
    [240., 60., -30., 0.],
    [0., 0., 0., 90.],
    [0., 120., 0., 0.],
    [0.; 4],
];

fn input() -> Input {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-orthotropic-relaxation-history.json"
    ))
    .unwrap()
}

fn green([[a, b], [c, d]]: [[f64; 2]; 2]) -> [f64; 3] {
    [
        0.5 * (a * a + c * c - 1.),
        0.5 * (b * b + d * d - 1.),
        a * b + c * d,
    ]
}

fn nominal([[a, b], [c, d]]: [[f64; 2]; 2], [x, y, s]: [f64; 3]) -> [[Option<f64>; 2]; 2] {
    [
        [Some(a * x + b * s), Some(a * s + b * y)],
        [Some(c * x + d * s), Some(c * s + d * y)],
    ]
}

// Independent direct sum over every earlier ramp segment. This does not use the fitter's lag
// recurrence, cone optimizer, production Maxwell state update, or reported predictions.
fn observations(input: &mut Input, tensors: &[[f64; 4]]) {
    let times = input.orthotropic.as_ref().unwrap().times_seconds.clone();
    let nonlinear = input.material.compiled_membrane(true).unwrap();
    for experiment in &mut input.experiments {
        let strains: Vec<_> = experiment
            .samples
            .iter()
            .map(|s| green(s.deformation))
            .collect();
        let nodes: Vec<_> = experiment.samples.iter().map(|s| s.time_seconds).collect();
        for (i, sample) in experiment.samples.iter_mut().enumerate() {
            let e = strains[i];
            let mut stress = nonlinear
                .as_ref()
                .map_or([800. * e[0], 500. * e[1], 120. * e[2]], |c| {
                    c.try_evaluate(e).unwrap().stress
                });
            for (&tau, &[a, b, c, g]) in times.iter().zip(tensors) {
                let mut lag = strains[0].map(|v| {
                    if experiment.initialization == Initialization::StressFreeReference {
                        v * (-nodes[i] / tau).exp()
                    } else {
                        0.
                    }
                });
                for k in 1..=i {
                    for axis in 0..3 {
                        let rate =
                            (strains[k][axis] - strains[k - 1][axis]) / (nodes[k] - nodes[k - 1]);
                        lag[axis] += rate
                            * tau
                            * ((-(nodes[i] - nodes[k]) / tau).exp()
                                - (-(nodes[i] - nodes[k - 1]) / tau).exp());
                    }
                }
                stress[0] += a * lag[0] + c * lag[1];
                stress[1] += c * lag[0] + b * lag[1];
                stress[2] += g * lag[2];
            }
            sample.nominal_n_m = nominal(sample.deformation, stress);
        }
    }
}

fn tensors(report: &Report) -> Vec<[f64; 4]> {
    report
        .orthotropic_fit
        .as_ref()
        .unwrap()
        .candidates
        .iter()
        .map(|c| c.stiffness_n_m)
        .collect()
}

fn assert_tensors(actual: &[[f64; 4]], expected: &[[f64; 4]], tolerance: f64) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().flatten().zip(expected.iter().flatten()) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} != {expected}"
        );
    }
}

#[test]
fn distinct_directional_spectra_signed_coupling_and_inactive_candidates_are_identified() {
    let input = input();
    let report = calibrate(&input).unwrap();
    assert_tensors(&tensors(&report), &TENSORS, 1e-7);
    assert_eq!(report.fit.components, 84);
    assert_eq!(report.check.as_ref().unwrap().components, 28);
    assert!(report.fit.rms_scaled < 1e-8);
    assert!(report.check.as_ref().unwrap().rms_scaled < 1e-8);
    assert_eq!(report.parameter_sensitivity.len(), 16);
    assert!(report.excitation_pivot > 1e-8);
    let diagnostics = report.orthotropic_fit.as_ref().unwrap();
    assert_eq!(
        diagnostics
            .candidates
            .iter()
            .map(|c| c.active)
            .collect::<Vec<_>>(),
        [true, true, true, false]
    );
    assert_eq!(diagnostics.candidates[3].stiffness_n_m, [0.; 4]);
    let law = report.material.membrane_relaxation.as_ref().unwrap();
    assert!(law.branches.is_empty());
    assert_eq!(law.orthotropic_branches.len(), 3);
    assert_eq!(law.orthotropic_branches[0].stiffness.shear.value, 0.);
    assert_eq!(law.orthotropic_branches[1].stiffness.warp.value, 0.);
    assert_eq!(law.orthotropic_branches[2].stiffness.coupling.value, 0.);
    assert_eq!(law.orthotropic_branches[2].relaxation_time.value, 8.);
    for branch in &law.orthotropic_branches {
        for q in [
            &branch.stiffness.warp,
            &branch.stiffness.weft,
            &branch.stiffness.coupling,
            &branch.stiffness.shear,
            &branch.relaxation_time,
        ] {
            assert_eq!(q.evidence, Evidence::Estimate);
            assert_eq!(q.conditions, input.conditions);
            assert!(q.bounds.is_none());
            assert!(
                report.material.sources[&q.source]
                    .reference
                    .ends_with(&report.input_sha256)
            );
        }
    }
    assert_eq!(
        serde_json::to_value(&report.material.properties).unwrap(),
        serde_json::to_value(&input.material.properties).unwrap()
    );
}

#[test]
fn partial_sensors_and_spatial_rotation_preserve_full_tensor_identification() {
    let mut input = input();
    for e in &mut input.experiments[..2] {
        for sample in &mut e.samples {
            sample.nominal_n_m[0][1] = None;
            sample.nominal_n_m[1][0] = None;
        }
    }
    for sample in &mut input.experiments[2].samples {
        sample.nominal_n_m = [[None, sample.nominal_n_m[0][1]], [None, None]];
    }
    let baseline = calibrate(&input).unwrap();
    assert_eq!(baseline.fit.components, 35);
    assert_eq!(baseline.predictions[0].residual_n_m[0][1], None);
    assert_tensors(&tensors(&baseline), &TENSORS, 1e-7);
    for e in &mut input.experiments {
        for s in &mut e.samples {
            s.deformation = [s.deformation[1].map(|v| -v), s.deformation[0]];
            s.nominal_n_m = [s.nominal_n_m[1].map(|v| v.map(|v| -v)), s.nominal_n_m[0]];
        }
    }
    assert_tensors(&tensors(&calibrate(&input).unwrap()), &TENSORS, 1e-7);
}

#[test]
fn finite_ramp_nodes_advance_history_even_when_no_force_sensor_was_read() {
    let mut input = input();
    input.experiments[3].partition = Partition::Fit;
    input.experiments[3].samples[1].nominal_n_m = [[None; 2]; 2];
    input.experiments[3].samples[3].nominal_n_m = [[None; 2]; 2];
    let report = calibrate(&input).unwrap();
    assert_tensors(&tensors(&report), &TENSORS, 1e-7);
    assert!(report.check.is_none());
    let unobserved = report
        .predictions
        .iter()
        .find(|p| p.experiment == "independent-ramp-unload" && p.time_seconds == 3.)
        .unwrap();
    assert!(
        unobserved
            .residual_n_m
            .iter()
            .flatten()
            .all(Option::is_none)
    );
    assert!((unobserved.nominal_n_m[0][0] + 17.70667034347975).abs() < 1e-8);
}

#[test]
fn held_out_forces_and_budgets_do_not_choose_coefficients_or_candidates() {
    let mut input = input();
    let original = calibrate(&input).unwrap();
    input.experiments[3].samples[1].nominal_n_m[0][0] = Some(10000.);
    assert!(calibrate(&input).unwrap_err().message.contains("held-out"));
    input.max_rms_scaled = 1e6;
    input.max_peak_scaled = 1e6;
    let report = calibrate(&input).unwrap();
    assert_eq!(
        serde_json::to_value(&report.orthotropic_fit).unwrap(),
        serde_json::to_value(&original.orthotropic_fit).unwrap()
    );
    assert_eq!(report.excitation_pivot, original.excitation_pivot);
    assert!(report.check.unwrap().rms_scaled > 100.);
}

#[test]
fn physical_psd_constraints_are_fitted_reported_and_checked_against_actual_forces() {
    let mut input = input();
    let mut perturbed = TENSORS;
    perturbed[0][2] = -125.; // Deliberately beyond sqrt(240*60), testing the PSD boundary.
    perturbed[0][3] = -2.; // Deliberate negative transient shear acquisition error.
    observations(&mut input, &perturbed);
    assert!(
        calibrate(&input)
            .unwrap_err()
            .message
            .contains("residual budget")
    );
    input.max_rms_scaled = 1.;
    input.max_peak_scaled = 2.;
    let report = calibrate(&input).unwrap();
    let diagnostics = report.orthotropic_fit.as_ref().unwrap();
    assert!(diagnostics.psd_projection_applied);
    assert!(diagnostics.iterations > 0);
    assert!(diagnostics.projected_step_relative <= 1e-11);
    assert!((diagnostics.candidates[0].unconstrained_stiffness_n_m[3] + 2.).abs() < 1e-7);
    assert!(report.fit.rms_scaled > 1e-4);
    for c in &diagnostics.candidates {
        let [a, b, k, g] = c.stiffness_n_m;
        assert!(a >= 0. && b >= 0. && g >= 0. && k * k <= a * b + 1e-8);
    }
    let db = Catalog::builtin().unwrap();
    let selected = db
        .resolve(&Selection {
            id: report.material.id.clone(),
            record: Some(report.material),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap();
    assert!(
        selected
            .membrane_relaxation()
            .unwrap()
            .unwrap()
            .has_orthotropic_branches()
    );
}

#[test]
fn unequal_directional_force_scales_reach_the_independent_constrained_minimum() {
    let mut input = input();
    let options = input.orthotropic.as_mut().unwrap();
    options.times_seconds = vec![1.];
    options.zero_threshold_n_m = 0.;
    input.experiments.truncate(3);
    // Deliberately indefinite acquisition, then downweight the weft sensor by 100. The oracle
    // minimizes the boundary problem independently with b=c^2/a, rather than using this fitter.
    observations(&mut input, &[[1., 1., 10., 2.]]);
    for sample in &mut input.experiments[1].samples {
        sample.scale_n_m = 100.;
    }
    input.max_rms_scaled = 0.01;
    input.max_peak_scaled = 0.03;
    let report = calibrate(&input).unwrap();
    assert_tensors(
        &tensors(&report),
        &[[1.2628682, 78.0451699, 9.92777735, 2.]],
        1e-4,
    );
    let objective = report.fit.components as f64 * report.fit.rms_scaled.powi(2);
    assert!((objective - 0.001737491384).abs() < 1e-9);
    assert!(report.orthotropic_fit.unwrap().psd_projection_applied);
}

#[test]
fn explicit_zero_threshold_cannot_hide_errors_and_zero_data_yields_no_relaxation() {
    let mut input = input();
    input.orthotropic.as_mut().unwrap().zero_threshold_n_m = 1000.;
    assert!(
        calibrate(&input)
            .unwrap_err()
            .message
            .contains("fit relaxation residual budget")
    );
    input.orthotropic.as_mut().unwrap().zero_threshold_n_m = 0.;
    observations(&mut input, &[[0.; 4]; 4]);
    let report = calibrate(&input).unwrap();
    assert!(report.material.membrane_relaxation.is_none());
    assert!(
        report
            .orthotropic_fit
            .unwrap()
            .candidates
            .iter()
            .all(|c| !c.active && c.stiffness_n_m == [0.; 4])
    );
    assert_eq!(report.fit.rms_scaled, 0.);
    assert_eq!(report.check.unwrap().rms_scaled, 0.);
}

#[test]
fn exact_source_replay_estimate_policy_and_missing_properties_remain_independent() {
    let mut input = input();
    let report = calibrate(&input).unwrap();
    let replay: Input = serde_json::from_str(&serde_json::to_string(&input).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_string(&report).unwrap(),
        serde_json::to_string(&calibrate(&replay).unwrap()).unwrap()
    );
    let db = Catalog::builtin().unwrap();
    let selected = db
        .resolve(&Selection {
            id: report.material.id.clone(),
            record: Some(report.material.clone()),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap();
    let frozen = selected.frozen_selection();
    let replay: Selection = serde_json::from_str(&serde_json::to_string(&frozen).unwrap()).unwrap();
    assert_eq!(selected.sha256(), db.resolve(&replay).unwrap().sha256());
    let mut denied = frozen;
    denied.allow_estimates = false;
    denied.expected_sha256 = None; // Consent is part of the prior frozen identity.
    assert!(db.resolve(&denied).unwrap().membrane_relaxation().is_err());
    input.material = report.material;
    assert!(
        calibrate(&input)
            .unwrap_err()
            .message
            .contains("replace_existing")
    );
    input.replace_existing = true;
    input.material.properties.remove("areal_density");
    let report = calibrate(&input).unwrap();
    assert!(
        report
            .missing_mechanics
            .iter()
            .any(|s| s == "areal_density")
    );
    assert!(report.material.mechanics(true).is_err());
    assert_tensors(&tensors(&report), &TENSORS, 1e-7);
}

#[test]
fn unobserved_directions_times_and_mismatched_evidence_refuse() {
    for change in [
        (|i: &mut Input| i.experiments[2].partition = Partition::Check) as fn(&mut Input),
        |i| i.orthotropic.as_mut().unwrap().times_seconds[3] = 8.0000000001,
        |i| i.orthotropic.as_mut().unwrap().times_seconds[1] = 0.3,
        |i| i.orthotropic.as_mut().unwrap().times_seconds[0] = 0.,
        |i| i.orthotropic.as_mut().unwrap().times_seconds = (1..=9).map(f64::from).collect(),
        |i| i.orthotropic.as_mut().unwrap().zero_threshold_n_m = -1.,
        |i| i.time_bounds_seconds = vec![[0.1, 1.]],
        |i| {
            i.conditions.remove("conditioning");
        },
        |i| {
            i.conditions
                .insert("specimen_id".into(), "different specimen".into());
        },
        |i| {
            i.material
                .properties
                .get_mut("warp_stiffness")
                .unwrap()
                .conditions
                .insert("conditioning".into(), "wet".into());
        },
        |i| i.experiments[0].source = "absent".into(),
        |i| i.allow_estimated_equilibrium = false,
        |i| i.allow_estimated_observations = false,
    ] {
        let mut input = input();
        change(&mut input);
        assert!(calibrate(&input).is_err());
    }
}

#[test]
fn source_qualified_nonlinear_equilibrium_is_preserved_during_directional_identification() {
    let mut input = input();
    let curve = |slope: f64| {
        serde_json::json!({"points":[[-0.2,-0.2*slope],[0.,0.],[0.02,0.02*slope],[0.3,0.58*slope]],
        "source":"synthetic","evidence":"estimate","method":"Independent synthetic toe/stiffening law","conditions":input.conditions})
    };
    input.material.membrane_response = Some(serde_json::from_value(serde_json::json!({
        "model":"tabulated_green_strain/1", "interpolation":"piecewise_linear",
        "warp":curve(800.), "weft":curve(500.), "shear":curve(120.),
        "coupling":{"value":0.,"unit":"N/m","source":"synthetic","evidence":"estimate","method":"Synthetic zero coupling","conditions":input.conditions}
    })).unwrap());
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
    ] {
        input.material.properties.remove(key);
    }
    observations(&mut input, &TENSORS);
    let report = calibrate(&input).unwrap();
    assert_tensors(&tensors(&report), &TENSORS, 1e-7);
    assert_eq!(
        serde_json::to_value(report.material.membrane_response).unwrap(),
        serde_json::to_value(input.material.membrane_response).unwrap()
    );
}
