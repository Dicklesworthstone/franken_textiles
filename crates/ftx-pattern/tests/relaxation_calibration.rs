//! Independent continuous-time convolution oracle. All specimens and observations are synthetic.
use std::collections::BTreeMap;

use ftx_pattern::material_calibration::Partition;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_relaxation::Initialization;
use ftx_pattern::material_relaxation::calibration::{Experiment, Input, SCHEMA, Sample, calibrate};
use ftx_pattern::materials::{Catalog, Evidence, Material};

fn material() -> Material {
    let mut s: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation.json"
    ))
    .unwrap();
    s.record.as_mut().unwrap().membrane_relaxation = None;
    s.record.unwrap()
}

fn input(modes: &[(f64, f64)], ramp: bool) -> Input {
    let m = material();
    let q = ftx_pattern::material_catalog::normalize(&m)
        .unwrap()
        .mechanics(true)
        .unwrap()
        .origin_tangent();
    let mut out = Input {
        schema: SCHEMA.into(),
        material: m,
        equilibrium_is_long_time: true,
        allow_estimated_equilibrium: true,
        allow_estimated_observations: true,
        replace_existing: false,
        method: "Synthetic exact continuous-time convolution; not laboratory measurements".into(),
        conditions: BTreeMap::from([("scope".into(), "synthetic".into())]),
        domain_source: "test".into(),
        strain_range: [[-0.2, 0.3], [-0.2, 0.3], [-0.3, 0.3]],
        initialization: Initialization::StressFreeReference,
        fitted_on: "2026-10-14".into(),
        time_bounds_seconds: modes.iter().map(|&(_, t)| [t / 3., t * 3.]).collect(),
        orthotropic: None,
        experiments: vec![],
        max_rms_scaled: 1e-6,
        max_peak_scaled: 1e-6,
    };
    out.experiments
        .push(experiment("training", Partition::Fit, modes, ramp, q));
    out.experiments
        .push(experiment("independent", Partition::Check, modes, !ramp, q));
    out
}

fn experiment(
    id: &str,
    partition: Partition,
    modes: &[(f64, f64)],
    ramp: bool,
    q: [[f64; 3]; 3],
) -> Experiment {
    let times: Vec<f64> = vec![
        0., 0.002, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1., 2., 4., 8., 16., 32., 64.,
    ];
    let strains: Vec<[f64; 3]> = times
        .iter()
        .map(|&t| {
            let factor = if !ramp {
                1.
            } else if t <= 0.2 {
                t / 0.2
            } else if t <= 4. {
                1.
            } else if t <= 8. {
                (8. - t) / 4.
            } else {
                0.
            };
            [0.06 * factor, 0.015 * factor, 0.035 * factor]
        })
        .collect();
    let samples = times
        .iter()
        .enumerate()
        .map(|(i, &time)| {
            let e = strains[i];
            let a = (1. + 2. * e[0]).sqrt();
            let b = e[2] / a;
            let f = [[a, b], [0., (1. + 2. * e[1] - b * b).sqrt()]];
            let mut effective = e;
            for &(ratio, tau) in modes {
                for j in 0..3 {
                    // Direct sum of the analytic integral over every prior strain segment, NOT the
                    // candidate's recurrence or its log-time derivatives.
                    let mut lag = strains[0][j] * (-time / tau).exp();
                    for k in 1..=i {
                        let slope = (strains[k][j] - strains[k - 1][j]) / (times[k] - times[k - 1]);
                        lag += slope
                            * tau
                            * ((-(time - times[k]) / tau).exp()
                                - (-(time - times[k - 1]) / tau).exp());
                    }
                    effective[j] += ratio * lag;
                }
            }
            let s = q.map(|r| {
                r.into_iter()
                    .zip(effective)
                    .map(|(a, b)| a * b)
                    .sum::<f64>()
            });
            let tensor = [[s[0], s[2]], [s[2], s[1]]];
            let p = std::array::from_fn(|a| {
                std::array::from_fn(|b| Some(f[a][0] * tensor[0][b] + f[a][1] * tensor[1][b]))
            });
            Sample {
                time_seconds: time,
                deformation: f,
                nominal_n_m: p,
                scale_n_m: 1.,
            }
        })
        .collect();
    Experiment {
        id: id.into(),
        source: "test".into(),
        evidence: Evidence::Estimate,
        partition,
        initialization: Initialization::StressFreeReference,
        samples,
    }
}

fn assert_modes(input: &Input, modes: &[(f64, f64)]) {
    let report = calibrate(input).unwrap();
    let branches = &report
        .material
        .membrane_relaxation
        .as_ref()
        .unwrap()
        .branches;
    for (branch, &(ratio, time)) in branches.iter().zip(modes) {
        assert!(
            (branch.stiffness_ratio.value / ratio - 1.).abs() < 1e-6,
            "{branch:?}"
        );
        assert!(
            (branch.relaxation_time.value / time - 1.).abs() < 1e-6,
            "{branch:?}"
        );
        assert_eq!(branch.stiffness_ratio.evidence, Evidence::Estimate);
        assert!(branch.relaxation_time.bounds.is_none()); // search intervals aren't confidence bounds
    }
    assert_eq!(
        serde_json::to_value(&input.material.properties).unwrap(),
        serde_json::to_value(&report.material.properties).unwrap()
    );
    assert!(report.excitation_pivot > 1e-7);
    assert!(report.fit.rms_scaled < 1e-7);
    assert!(report.check.unwrap().rms_scaled < 1e-7);
}

#[test]
fn exact_step_hold_identifies_ratio_and_time_not_instantaneous_modulus() {
    let modes = [(2.4, 0.7)];
    assert_modes(&input(&modes, false), &modes);
}

#[test]
fn finite_ramps_holds_and_unloading_identify_two_timescales() {
    let modes = [(1.7, 0.3), (0.65, 8.)];
    assert_modes(&input(&modes, true), &modes);
}

#[test]
fn three_resolvable_modes_have_independent_time_coverage() {
    let modes = [(0.8, 0.04), (1.1, 0.8), (0.5, 16.)];
    assert_modes(&input(&modes, false), &modes);
}

#[test]
fn noisy_forces_fit_with_explicit_residual_budgets_without_exact_interpolation() {
    let mut raw = input(&[(1.7, 0.3), (0.65, 8.)], true);
    for e in &mut raw.experiments {
        for (i, s) in e.samples.iter_mut().enumerate() {
            for (j, f) in s.nominal_n_m.iter_mut().flatten().enumerate() {
                *f.as_mut().unwrap() += ((i * 7 + j * 11) % 13) as f64 * 0.005 - 0.03;
            }
        }
    }
    raw.max_rms_scaled = 0.1;
    raw.max_peak_scaled = 0.2;
    let report = calibrate(&raw).unwrap();
    let branches = &report.material.membrane_relaxation.unwrap().branches;
    assert!((branches[0].relaxation_time.value / 0.3 - 1.).abs() < 0.01);
    assert!((branches[1].relaxation_time.value / 8. - 1.).abs() < 0.01);
    assert!(report.fit.rms_scaled > 0.001);
    assert!(report.check.unwrap().rms_scaled > 0.001);
}

#[test]
fn unobserved_force_nodes_still_advance_the_loading_history() {
    let mut raw = input(&[(1.2, 1.)], true);
    for e in &mut raw.experiments {
        e.samples[6].nominal_n_m = [[None; 2]; 2];
        e.samples[10].nominal_n_m = [[None; 2]; 2];
    }
    let fit = calibrate(&raw).unwrap();
    assert!(
        (fit.material.membrane_relaxation.unwrap().branches[0]
            .relaxation_time
            .value
            - 1.)
            .abs()
            < 1e-6
    );
    assert!(
        fit.predictions[6]
            .residual_n_m
            .iter()
            .flatten()
            .all(Option::is_none)
    );
    assert_eq!(fit.fit.components, 52);
}

#[test]
fn relaxation_preserves_scalar_fit_bounds_and_checks_every_history_node() {
    let modes = [(1.2, 1.)];
    let mut raw = input(&modes, true);
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
    ] {
        raw.material
            .properties
            .get_mut(key)
            .unwrap()
            .conditions
            .insert(
                "fitted_green_strain_range".into(),
                "[[-0.08,0.08],[-0.05,0.05],[-0.05,0.05]]".into(),
            );
    }
    assert_modes(&raw, &modes); // Includes exact preservation of the supplied equilibrium quantities.
    for history in 0..2 {
        for observed in [true, false] {
            let mut outside = raw.clone();
            let node = &mut outside.experiments[history].samples[6];
            node.deformation = [[1.2_f64.sqrt(), 0.], [0., 1.]]; // E11=0.1, inside only Maxwell's wider box.
            if !observed {
                node.nominal_n_m = [[None; 2]; 2];
            }
            let refusal = calibrate(&outside).unwrap_err();
            assert!(refusal.message.contains(&outside.experiments[history].id));
            assert!(
                refusal
                    .message
                    .contains("outside fitted equilibrium strain domain")
            );
        }
    }
}

#[test]
fn held_out_forces_never_influence_parameter_identification() {
    let mut raw = input(&[(2., 0.7)], true);
    let baseline = calibrate(&raw).unwrap();
    raw.experiments[1].samples[8].nominal_n_m[0][0] = Some(50000.);
    assert!(calibrate(&raw).unwrap_err().message.contains("held-out"));
    raw.max_rms_scaled = 1e6;
    raw.max_peak_scaled = 1e6;
    let fit = calibrate(&raw).unwrap();
    let a = &baseline.material.membrane_relaxation.unwrap().branches[0];
    let b = &fit.material.membrane_relaxation.unwrap().branches[0];
    assert_eq!(a.relaxation_time.value, b.relaxation_time.value);
    assert_eq!(a.stiffness_ratio.value, b.stiffness_ratio.value);
    raw.experiments.pop();
    assert!(calibrate(&raw).unwrap().check.is_none());
}

#[test]
fn rigid_frame_rotation_preserves_identified_material_law() {
    let modes = [(1.7, 0.3), (0.65, 8.)];
    let mut raw = input(&modes, true);
    for e in &mut raw.experiments {
        for s in &mut e.samples {
            s.deformation = [s.deformation[1].map(|x| -x), s.deformation[0]];
            s.nominal_n_m = [s.nominal_n_m[1].map(|v| v.map(|x| -x)), s.nominal_n_m[0]];
        }
    }
    assert_modes(&raw, &modes);
}

#[test]
fn missing_other_physics_remains_missing_and_estimate_policies_stay_separate() {
    let mut raw = input(&[(2., 0.7)], true);
    raw.allow_estimated_equilibrium = false;
    assert!(calibrate(&raw).is_err());
    raw.allow_estimated_equilibrium = true;
    raw.allow_estimated_observations = false;
    assert!(calibrate(&raw).is_err());
    raw.allow_estimated_observations = true;
    for key in [
        "areal_density",
        "thickness",
        "bending_rigidity",
        "friction_skin",
        "friction_self",
    ] {
        raw.material.properties.remove(key);
    }
    let report = calibrate(&raw).unwrap();
    assert_eq!(report.missing_mechanics.len(), 5);
    assert!(report.material.mechanics(true).is_err());
    assert_eq!(report.material.properties.len(), 4);
    let receipt = &report
        .material
        .membrane_relaxation
        .as_ref()
        .unwrap()
        .branches[0]
        .relaxation_time
        .source;
    assert!(
        report.material.sources[receipt]
            .reference
            .ends_with(&report.input_sha256)
    );
}

#[test]
fn source_complete_fits_replay_and_freeze_without_changing_existing_evidence() {
    let raw = input(&[(2., 0.7)], true);
    let fit = calibrate(&raw).unwrap();
    let replay: Input = serde_json::from_str(&serde_json::to_string(&raw).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_string(&fit).unwrap(),
        serde_json::to_string(&calibrate(&replay).unwrap()).unwrap()
    );
    let db = Catalog::builtin().unwrap();
    let selected = db
        .resolve(&Selection {
            id: fit.material.id.clone(),
            record: Some(fit.material.clone()),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap();
    assert!(selected.membrane_relaxation().unwrap().is_some());
    assert_eq!(
        db.resolve(&selected.frozen_selection()).unwrap().sha256(),
        selected.sha256()
    );
    let mut next = raw;
    next.material = fit.material;
    assert!(
        calibrate(&next)
            .unwrap_err()
            .message
            .contains("replace_existing")
    );
    next.replace_existing = true;
    assert!(calibrate(&next).is_ok());
}

#[test]
fn unexcited_or_time_window_limited_spectra_are_not_published() {
    let mut raw = input(&[(2., 0.7)], false);
    for e in &mut raw.experiments {
        for s in &mut e.samples {
            s.deformation = [[1., 0.], [0., 1.]];
            s.nominal_n_m = [[Some(0.); 2]; 2];
        }
    }
    assert!(calibrate(&raw).is_err());
    raw = input(&[(2., 0.7)], false);
    raw.time_bounds_seconds = vec![[4., 8.]];
    assert!(calibrate(&raw).is_err());
    raw = input(&[(2., 0.7)], false);
    raw.time_bounds_seconds = vec![[0.1, 2.], [0.5, 4.]];
    assert!(calibrate(&raw).is_err());
}

#[test]
fn malformed_histories_and_unsupported_physics_refuse_before_optimization() {
    for change in [
        (|i: &mut Input| i.equilibrium_is_long_time = false) as fn(&mut Input),
        |i| i.experiments[0].samples[0].time_seconds = 0.1,
        |i| i.experiments[0].samples[1].time_seconds = 0.,
        |i| i.experiments[0].samples[1].time_seconds = f64::NAN,
        |i| i.experiments[0].samples[0].deformation = [[1., 0.], [0., -1.]],
        |i| i.experiments[0].samples[1].scale_n_m = 0.,
        |i| i.experiments[0].samples[1].nominal_n_m[0][0] = Some(f64::INFINITY),
        |i| i.experiments[0].source = "missing".into(),
        |i| i.domain_source = "missing".into(),
        |i| i.experiments[1].id = i.experiments[0].id.clone(),
        |i| i.experiments[0].samples.truncate(1),
        |i| i.time_bounds_seconds.clear(),
        |i| i.time_bounds_seconds[0] = [-1., 2.],
        |i| i.strain_range[0] = [0., 1.],
        |i| i.max_rms_scaled = 0.,
        |i| i.material.form = ftx_pattern::materials::Form::Fill,
    ] {
        let mut raw = input(&[(2., 0.7)], true);
        change(&mut raw);
        assert!(calibrate(&raw).is_err());
    }
}

#[test]
fn pre_relaxed_initial_state_and_partial_load_components_are_supported() {
    let modes = [(1.4, 0.4)];
    let mut raw = input(&modes, true);
    // The training ramp begins at the stress-free shape: zero is also the relaxed initial state.
    raw.experiments[0].initialization = Initialization::RelaxedStart;
    for e in &mut raw.experiments {
        for s in &mut e.samples {
            s.nominal_n_m[0][1] = None;
            s.nominal_n_m[1][0] = None;
        }
    }
    assert_modes(&raw, &modes);
}

#[test]
fn nonlinear_equilibrium_is_not_replaced_with_its_origin_tangent() {
    let modes = [(1.3, 0.6)];
    let mut raw = input(&modes, true);
    let s: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-response.json"
    ))
    .unwrap();
    raw.material = s.record.unwrap();
    raw.domain_source = "synthetic".into();
    let equilibrium = raw.material.compiled_membrane(true).unwrap().unwrap();
    let q = raw.material.mechanics(true).unwrap().origin_tangent();
    raw.experiments = vec![
        experiment("train", Partition::Fit, &modes, true, q),
        experiment("check", Partition::Check, &modes, false, q),
    ];
    for e in &mut raw.experiments {
        e.source = "synthetic".into();
        for s in &mut e.samples {
            let f = s.deformation;
            let strain = [
                0.5 * (f[0][0] * f[0][0] + f[1][0] * f[1][0] - 1.),
                0.5 * (f[0][1] * f[0][1] + f[1][1] * f[1][1] - 1.),
                f[0][0] * f[0][1] + f[1][0] * f[1][1],
            ];
            let linear = q.map(|r| r.into_iter().zip(strain).map(|(a, b)| a * b).sum::<f64>());
            let nonlinear = equilibrium.try_evaluate(strain).unwrap().stress;
            let ds: [f64; 3] = std::array::from_fn(|j| nonlinear[j] - linear[j]);
            let tensor = [[ds[0], ds[2]], [ds[2], ds[1]]];
            for (a, row) in s.nominal_n_m.iter_mut().enumerate() {
                for (b, value) in row.iter_mut().enumerate() {
                    *value.as_mut().unwrap() += f[a][0] * tensor[0][b] + f[a][1] * tensor[1][b];
                }
            }
        }
    }
    assert_modes(&raw, &modes);
    let result = calibrate(&raw).unwrap();
    assert_eq!(
        serde_json::to_value(&result.material.membrane_response).unwrap(),
        serde_json::to_value(&raw.material.membrane_response).unwrap()
    );
}
