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
        plastic_equilibrium: None,
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

fn green(f: [[f64; 2]; 2]) -> [f64; 3] {
    [
        0.5 * (f[0][0] * f[0][0] + f[1][0] * f[1][0] - 1.),
        0.5 * (f[0][1] * f[0][1] + f[1][1] * f[1][1] - 1.),
        f[0][0] * f[0][1] + f[1][0] * f[1][1],
    ]
}

struct PlasticOracle {
    plastic: [f64; 3],
    accumulated: [f64; 3],
    stress: [f64; 3],
    dissipated: f64,
}

// Independent diagonal scalar yield/hardening formulas, not the candidate's active-set return map.
fn plastic_oracle(samples: &[Sample]) -> Vec<PlasticOracle> {
    let mut plastic = [0.; 3];
    let mut accumulated = [0.; 3];
    samples
        .iter()
        .map(|sample| {
            let strain = green(sample.deformation);
            let mut stress = [0.; 3];
            let mut dissipated = 0.;
            for (axis, (q, yield_value, hardening)) in
                [(1000., 20., 100.), (600., 30., 80.), (300., 10., 40.)]
                    .into_iter()
                    .enumerate()
            {
                let trial = q * (strain[axis] - plastic[axis]);
                let excess = (trial.abs() - yield_value - hardening * accumulated[axis]).max(0.);
                let increment = trial.signum() * excess / (q + hardening);
                plastic[axis] += increment;
                accumulated[axis] += increment.abs();
                dissipated += yield_value * increment.abs();
                stress[axis] = q * (strain[axis] - plastic[axis]);
            }
            PlasticOracle {
                plastic,
                accumulated,
                stress,
                dissipated,
            }
        })
        .collect()
}

// Every number is a synthetic software oracle. Each acquisition begins independently pristine;
// the hidden peak and reversal still alter both branch histories. Maxwell forces use direct
// integration over every preceding ramp, without the candidate's recurrence or fitting code.
fn plastic_input() -> Input {
    use ftx_pattern::material_relaxation::calibration::{
        PlasticEquilibrium, PlasticInitialization,
    };
    let selection: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-parallel-membrane.json"
    ))
    .unwrap();
    let mut material = selection.record.unwrap();
    let old = material.membrane_relaxation.take().unwrap();
    let mut input = input(&[(1.4, 0.7)], true);
    input.material = material;
    input.conditions = old.conditions;
    input.plastic_equilibrium = Some(PlasticEquilibrium {
        initialization: PlasticInitialization::PristineReference,
        allow_estimated_network: true,
        network: old.parallel_plasticity.unwrap(),
    });
    input.method = "Synthetic independent diagonal yield law plus analytic Maxwell ramp integrals; not physical acquisitions".into();
    input.strain_range = [[-0.2, 0.3]; 3];
    input.experiments.clear();
    for (index, amplitude) in [
        [0.08, 0., 0.],
        [0., 0.1, 0.],
        [0., 0., 0.12],
        [0.06, 0.08, 0.1],
    ]
    .into_iter()
    .enumerate()
    {
        let factor = [0., 0.3, 0.8, 1., 0.7, -0.4, -0.4, 0., 0.5, 0.5, 0., 0., 0.];
        let times = [
            0., 0.01, 0.03, 0.1, 0.3, 0.7, 1.5, 3., 5., 8., 12., 20., 40.,
        ];
        let mut samples: Vec<_> = times
            .into_iter()
            .zip(factor)
            .map(|(time, factor)| {
                let strain: [f64; 3] = amplitude.map(|a| a * factor);
                let a = (1. + 2. * strain[0]).sqrt();
                let b = strain[2] / a;
                Sample {
                    time_seconds: time * if index == 3 { 1.3 } else { 1. },
                    deformation: [[a, b], [0., (1. + 2. * strain[1] - b * b).sqrt()]],
                    nominal_n_m: [[None; 2]; 2],
                    scale_n_m: 1.,
                }
            })
            .collect();
        let strains: Vec<_> = samples.iter().map(|s| green(s.deformation)).collect();
        let times: Vec<_> = samples.iter().map(|s| s.time_seconds).collect();
        let known = plastic_oracle(&samples);
        for (i, sample) in samples.iter_mut().enumerate() {
            let mut stress = known[i].stress;
            for axis in 0..3 {
                let mut lag = 0.;
                for node in 1..=i {
                    let rate = (strains[node][axis] - strains[node - 1][axis])
                        / (times[node] - times[node - 1]);
                    lag += rate
                        * 0.7
                        * ((-(times[i] - times[node]) / 0.7).exp()
                            - (-(times[i] - times[node - 1]) / 0.7).exp());
                }
                stress[axis] += [1000., 600., 300.][axis] * 1.4 * lag;
            }
            let [[a, b], [c, d]] = sample.deformation;
            let [x, y, s] = stress;
            sample.nominal_n_m = [
                [Some(a * x + b * s), Some(a * s + b * y)],
                [Some(c * x + d * s), Some(c * s + d * y)],
            ];
            if i == 3 {
                sample.nominal_n_m = [[None; 2]; 2];
            }
        }
        input.experiments.push(Experiment {
            id: format!("independent-{index}"),
            source: "test".into(),
            evidence: Evidence::Estimate,
            partition: if index == 3 {
                Partition::Check
            } else {
                Partition::Fit
            },
            initialization: Initialization::StressFreeReference,
            samples,
        });
    }
    input.max_rms_scaled = 1e-5;
    input.max_peak_scaled = 1e-5;
    input
}

#[test]
fn known_plastic_equilibrium_identifies_recovery_and_retains_every_acquisition_state() {
    let input = plastic_input();
    assert_modes(&input, &[(1.4, 0.7)]);
    let report = calibrate(&input).unwrap();
    assert_eq!(
        serde_json::to_value(&report.material.properties).unwrap(),
        serde_json::to_value(&input.material.properties).unwrap()
    );
    let catalog = Catalog::builtin().unwrap();
    let resolved = catalog
        .resolve(&Selection {
            id: report.material.id.clone(),
            record: Some(report.material.clone()),
            allow_estimates: true,
            ..Default::default()
        })
        .unwrap();
    let plastic = resolved.membrane_plasticity().unwrap().unwrap();
    let relaxation = resolved.membrane_relaxation().unwrap().unwrap();
    assert!(plastic.accepts_parallel(&relaxation));
    let denied = catalog
        .resolve(&Selection {
            id: report.material.id.clone(),
            record: Some(report.material),
            ..Default::default()
        })
        .unwrap();
    assert!(denied.membrane_relaxation().is_err());
    for experiment in &input.experiments {
        let known = plastic_oracle(&experiment.samples);
        let mut cumulative = 0.;
        for (prediction, expected) in report
            .predictions
            .iter()
            .filter(|p| p.experiment == experiment.id)
            .zip(known)
        {
            let state = prediction.plastic_equilibrium.as_ref().unwrap();
            cumulative += expected.dissipated;
            for axis in 0..3 {
                assert!((state.plastic_strain[axis] - expected.plastic[axis]).abs() < 1e-13);
                assert!(
                    (state.accumulated_strain[axis] - expected.accumulated[axis]).abs() < 1e-13
                );
            }
            assert!((state.step_dissipated_j_m2 - expected.dissipated).abs() < 1e-12);
            assert!((state.cumulative_dissipated_j_m2 - cumulative).abs() < 1e-12);
        }
        let first = report
            .predictions
            .iter()
            .find(|p| p.experiment == experiment.id)
            .unwrap();
        assert_eq!(
            first
                .plastic_equilibrium
                .as_ref()
                .unwrap()
                .accumulated_strain,
            [0.; 3]
        );
    }
    let hidden = &report.predictions[3];
    assert!(hidden.residual_n_m.iter().flatten().all(Option::is_none));
    assert!(
        hidden
            .plastic_equilibrium
            .as_ref()
            .unwrap()
            .step_dissipated_j_m2
            > 0.
    );
}

#[test]
fn known_plastic_equilibrium_supports_fixed_and_identified_directional_times() {
    use ftx_pattern::material_relaxation::calibration::OrthotropicInput;
    for searched in [false, true] {
        let mut input = plastic_input();
        input.time_bounds_seconds.clear();
        input.orthotropic = Some(OrthotropicInput {
            times_seconds: if searched { vec![] } else { vec![0.7] },
            time_bounds_seconds: if searched { vec![[0.2, 2.]] } else { vec![] },
            zero_threshold_n_m: 1e-7,
        });
        let report = calibrate(&input).unwrap();
        let branch = &report
            .material
            .membrane_relaxation
            .as_ref()
            .unwrap()
            .orthotropic_branches[0];
        assert!((branch.relaxation_time.value - 0.7).abs() < 1e-5);
        for (actual, expected) in [
            (branch.stiffness.warp.value, 1400.),
            (branch.stiffness.weft.value, 840.),
            (branch.stiffness.coupling.value, 0.),
            (branch.stiffness.shear.value, 420.),
        ] {
            assert!((actual - expected).abs() < 1e-3, "{actual} != {expected}");
        }
        assert!(report.check.unwrap().rms_scaled < 1e-5);
        assert!(
            report
                .predictions
                .iter()
                .all(|p| p.plastic_equilibrium.is_some())
        );
    }
}

#[test]
fn known_plastic_equilibrium_requires_qualified_sources_domains_and_pristine_preparation() {
    for change in [
        |i: &mut Input| i.plastic_equilibrium = None,
        |i: &mut Input| {
            i.material.structure.remove("membrane_plasticity_model");
        },
        |i: &mut Input| i.allow_estimated_equilibrium = false,
        |i: &mut Input| i.allow_estimated_observations = false,
        |i: &mut Input| {
            i.plastic_equilibrium
                .as_mut()
                .unwrap()
                .allow_estimated_network = false
        },
        |i: &mut Input| i.plastic_equilibrium.as_mut().unwrap().network.source = "missing".into(),
        |i: &mut Input| {
            i.conditions
                .insert("conditioning".into(), "unknown temperature".into());
        },
        |i: &mut Input| {
            i.material
                .properties
                .get_mut("membrane_yield_weft")
                .unwrap()
                .conditions
                .insert("specimen_id".into(), "different specimen".into());
        },
        |i: &mut Input| {
            i.material
                .properties
                .get_mut("membrane_strain_limit_warp")
                .unwrap()
                .value = 0.01
        },
        |i: &mut Input| {
            i.material
                .properties
                .get_mut("membrane_accumulation_limit_warp")
                .unwrap()
                .value = 0.001
        },
        |i: &mut Input| i.experiments[0].samples[0].deformation[0][0] = 1.01,
        |i: &mut Input| i.strain_range[0] = [-0.01, 0.01],
    ] {
        let mut input = plastic_input();
        change(&mut input);
        assert!(calibrate(&input).is_err());
    }
    let mut missing = serde_json::to_value(plastic_input()).unwrap();
    missing["plastic_equilibrium"]
        .as_object_mut()
        .unwrap()
        .remove("initialization");
    assert!(serde_json::from_value::<Input>(missing).is_err());
    let mut omitted = serde_json::to_value(input(&[(1.4, 0.7)], true)).unwrap();
    assert!(omitted.get("plastic_equilibrium").is_none());
    omitted["plastic_equilibrium"] = serde_json::json!({"initialization":"infer_preload"});
    assert!(serde_json::from_value::<Input>(omitted).is_err());
}

#[test]
fn known_plastic_equilibrium_model_choice_is_fit_only_when_check_history_fails() {
    use ftx_pattern::material_relaxation::calibration::comparison::{Status, compare};
    let mut input = plastic_input();
    input.time_bounds_seconds.clear();
    let mut compared = serde_json::from_value(serde_json::json!({
        "schema":"ftx-relaxation-model-comparison/1", "calibration":input,
        "candidates":[
            {"id":"known-time", "orthotropic":{"times_seconds":[0.7]}},
            {"id":"different-time", "orthotropic":{"times_seconds":[1.2]}}
        ], "fit_rms_equivalence_scaled":1e-6
    }))
    .unwrap();
    let accepted = compare(&compared).unwrap();
    assert_eq!(accepted.status, Status::Accepted);
    assert_eq!(
        accepted.fit_decision.chosen_candidate_id.as_deref(),
        Some("known-time")
    );
    compared.calibration.experiments[3].samples[4].nominal_n_m[0][0] = Some(1e5);
    let rejected = compare(&compared).unwrap();
    assert_eq!(rejected.status, Status::ChosenCandidateRejected);
    assert_eq!(
        serde_json::to_value(&rejected.fit_decision).unwrap(),
        serde_json::to_value(&accepted.fit_decision).unwrap()
    );
    assert!(rejected.selected_calibration().is_none());
    assert_eq!(
        rejected.objective_evaluations,
        accepted.objective_evaluations
    );
    compared.calibration.experiments[3].samples[0].deformation[0][0] = 1.01;
    let inadmissible_check = compare(&compared).unwrap();
    assert_eq!(inadmissible_check.status, Status::ChosenCandidateRejected);
    assert_eq!(
        serde_json::to_value(&inadmissible_check.fit_decision).unwrap(),
        serde_json::to_value(&accepted.fit_decision).unwrap()
    );
}
