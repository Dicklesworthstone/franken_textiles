//! Identification checks against independently constructed scalar and coupled yield equations.
//! These synthetic histories are not oracle fixtures or measurements of real textiles.
use ftx_pattern::{
    material_calibration::Partition,
    material_membrane_plasticity::{
        AXES, MODEL,
        calibration::{Input, Sample, Status, calibrate},
    },
    materials::Evidence,
};
use serde_json::json;

fn input() -> Input {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-membrane-plasticity-calibration.json"
    ))
    .unwrap()
}

fn near(a: f64, b: f64, tolerance: f64) {
    assert!(
        (a - b).abs() <= tolerance * (1.0 + a.abs() + b.abs()),
        "{a} != {b}"
    );
}

fn fitted_equilibrium(input: &mut Input) {
    // Synthetic source-qualified scalar domain, narrower than the independent plastic limits.
    let bounds = "[[-0.1,0.1],[-0.1,0.1],[-0.1,0.1]]";
    input
        .conditions
        .insert("fitted_green_strain_range".into(), bounds.into());
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
    ] {
        input
            .material
            .properties
            .get_mut(key)
            .unwrap()
            .conditions
            .insert("fitted_green_strain_range".into(), bounds.into());
    }
}

#[test]
fn fixed_scalar_fit_bounds_reject_observed_and_unobserved_plastic_fit_nodes() {
    let mut source = input();
    fitted_equilibrium(&mut source);
    let accepted = calibrate(&source).unwrap();
    assert_eq!(accepted.status, Status::Accepted);
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
    ] {
        assert_eq!(
            serde_json::to_value(&accepted.material.as_ref().unwrap().properties[key]).unwrap(),
            serde_json::to_value(&source.material.properties[key]).unwrap()
        );
    }
    for index in [1, 4] {
        let mut outside = source.clone();
        outside.experiments[0].samples[index].green_strain[0] = 0.11;
        if index == 4 {
            assert_eq!(
                outside.experiments[0].samples[index].second_piola_n_m,
                [None; 3]
            );
        }
        let refusal = calibrate(&outside).unwrap_err();
        assert!(refusal.message.contains("warp-cycle"));
        assert!(
            refusal
                .message
                .contains("outside fitted equilibrium strain domain")
        );
    }
}

#[test]
fn held_out_scalar_domain_refusal_keeps_the_same_plastic_fit_candidate() {
    let mut source = input();
    fitted_equilibrium(&mut source);
    let original = calibrate(&source).unwrap();
    let node = &mut source.experiments.last_mut().unwrap().samples[1];
    node.green_strain[0] = 0.11;
    node.second_piola_n_m = [None; 3];
    let refused = calibrate(&source).unwrap();
    assert_eq!(refused.status, Status::Rejected);
    assert!(refused.material.is_none());
    assert_eq!(refused.parameters, original.parameters);
    assert_eq!(
        serde_json::to_value(&refused.fit).unwrap(),
        serde_json::to_value(&original.fit).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&refused.diagnostics.starts).unwrap(),
        serde_json::to_value(&original.diagnostics.starts).unwrap()
    );
    assert_eq!(
        refused.diagnostics.evaluations,
        original.diagnostics.evaluations
    );
    assert_eq!(refused.check_status, "REFUSED");
    let refusal = refused.check_refusal.unwrap();
    assert_eq!(refusal.code, ftx_core::RefusalCode::OutOfRange);
    assert!(refusal.message.contains("held-out-combined"));
    assert!(
        refusal
            .message
            .contains("outside fitted equilibrium strain domain")
    );
    assert_eq!(refused.predictions.len(), 25);
}

#[test]
fn identifies_signed_cycles_and_replays_missing_load_nodes_without_resetting_memory() {
    let source = input();
    let original = serde_json::to_value(&source).unwrap();
    let r = calibrate(&source).unwrap();
    assert_eq!(r.status, Status::Accepted, "{r:#?}");
    let p = r.parameters.as_ref().unwrap();
    for i in 0..3 {
        near(p.yield_n_m[i], [10., 8., 5.][i], 1e-7);
        near(p.hardening_n_m[i], [100., 50., 25.][i], 1e-7);
    }
    assert_eq!(r.fit.as_ref().unwrap().components, 72);
    assert_eq!(r.check.as_ref().unwrap().components, 15);
    assert_eq!(r.check_status, "ACCEPTED");
    assert_eq!(r.predictions.len(), 30);
    assert_eq!(r.diagnostics.starts_completed, 4);
    assert!(r.diagnostics.excitation_pivot > 1e-6);
    assert!(r.diagnostics.evaluations <= 4096);
    assert_eq!(
        r.diagnostics.history_steps,
        (r.diagnostics.evaluations - 1) * 25 + 5
    );
    // Independent diagonal closed form, preserving every recorded incremental state.
    let mut cursor = 0;
    for history in &source.experiments {
        let mut z = [0.0; 3];
        let mut a = [0.0; 3];
        let mut dissipation = 0.0;
        for sample in &history.samples {
            let predicted = &r.predictions[cursor];
            let mut storage = 0.0;
            for i in 0..3 {
                let q = [1000.0, 500.0, 250.0][i];
                let y = [10.0, 8.0, 5.0][i];
                let h = [100.0, 50.0, 25.0][i];
                let trial = q * (sample.green_strain[i] - z[i]);
                let dz = trial.signum() * (trial.abs() - y - h * a[i]).max(0.0) / (q + h);
                z[i] += dz;
                a[i] += dz.abs();
                dissipation += y * dz.abs();
                let stress = q * (sample.green_strain[i] - z[i]);
                storage += 0.5 * stress * stress / q + 0.5 * h * a[i] * a[i];
                near(predicted.second_piola_n_m[i], stress, 1e-7);
                near(predicted.plastic_strain[i], z[i], 1e-7);
                near(predicted.accumulated_strain[i], a[i], 1e-7);
                assert_eq!(
                    predicted.residual_n_m[i].is_none(),
                    sample.second_piola_n_m[i].is_none()
                );
            }
            near(predicted.plastic_dissipation_j_m2, dissipation, 1e-7);
            near(predicted.stored_j_m2, storage, 1e-7);
            cursor += 1;
        }
    }
    assert_eq!(r.predictions[4].residual_n_m, [None; 3]);
    assert!(r.predictions[4].plastic_strain[0] > r.predictions[3].plastic_strain[0]);
    assert!(r.predictions[6].plastic_strain[0] > 0.0);
    assert_eq!(serde_json::to_value(&source).unwrap(), original);
    assert_eq!(serde_json::to_value(&r.input).unwrap(), original);
    let material = r.material.unwrap();
    assert_eq!(material.structure["membrane_plasticity_model"], MODEL);
    for family in ["membrane_yield", "membrane_hardening"] {
        for axis in AXES {
            let q = &material.properties[&format!("{family}_{axis}")];
            assert_eq!(q.evidence, Evidence::Estimate);
            assert!(q.bounds.is_none());
            assert!(
                material.sources[&q.source]
                    .reference
                    .contains(&r.input_sha256)
            );
        }
    }
    for family in ["membrane_strain_limit", "membrane_accumulation_limit"] {
        for axis in AXES {
            let key = format!("{family}_{axis}");
            assert_eq!(
                serde_json::to_value(&material.properties[&key]).unwrap(),
                serde_json::to_value(&source.material.properties[&key]).unwrap()
            );
        }
    }
    assert!(material.compiled_membrane_plasticity(false).is_err());
    assert!(
        material
            .compiled_membrane_plasticity(true)
            .unwrap()
            .is_some()
    );
}

#[test]
fn check_rejection_keeps_exact_fit_parameters_and_cannot_select_a_different_start() {
    let mut source = input();
    let original = calibrate(&source).unwrap();
    for sample in &mut source.experiments.last_mut().unwrap().samples[1..] {
        sample.second_piola_n_m[0] = Some(100.0);
    }
    let rejected = calibrate(&source).unwrap();
    assert_eq!(rejected.status, Status::Rejected);
    assert!(rejected.material.is_none());
    assert_eq!(rejected.check_status, "REJECTED");
    assert_eq!(rejected.parameters, original.parameters);
    assert_eq!(
        serde_json::to_value(&rejected.fit).unwrap(),
        serde_json::to_value(&original.fit).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&rejected.diagnostics).unwrap(),
        serde_json::to_value(&original.diagnostics).unwrap()
    );
    assert_ne!(rejected.input_sha256, original.input_sha256);
}

#[test]
fn check_outside_characterization_is_a_retained_refusal_of_the_fixed_fit_candidate() {
    let mut source = input();
    let original = calibrate(&source).unwrap();
    source.experiments.last_mut().unwrap().samples[1].green_strain[0] = 0.26;
    let refused = calibrate(&source).unwrap();
    assert_eq!(refused.status, Status::Rejected);
    assert!(refused.material.is_none());
    assert_eq!(refused.parameters, original.parameters);
    assert_eq!(refused.check_status, "REFUSED");
    assert!(
        refused
            .check_refusal
            .as_ref()
            .unwrap()
            .message
            .contains("held-out-combined")
    );
    assert!(refused.check.is_none());
    assert_eq!(refused.predictions.len(), 25);
}

#[test]
fn absent_checks_stay_not_provided_and_unexcited_directions_stay_ambiguous() {
    let mut source = input();
    source.experiments.retain(|e| e.partition == Partition::Fit);
    let fit = calibrate(&source).unwrap();
    assert_eq!(fit.status, Status::Accepted, "{fit:#?}");
    assert!(fit.check.is_none());
    assert_eq!(fit.check_status, "NOT_PROVIDED");
    for e in &mut source.experiments {
        for s in &mut e.samples {
            s.green_strain[2] = 0.0;
            s.second_piola_n_m[2] = Some(0.0);
        }
    }
    let r = calibrate(&source).unwrap();
    assert_eq!(r.status, Status::Ambiguous, "{r:#?}");
    assert!(r.material.is_none());
    assert_eq!(r.diagnostics.excitation_pivot, 0.0);
    assert_eq!(r.diagnostics.parameter_sensitivity[2], 0.0);
    assert_eq!(r.diagnostics.parameter_sensitivity[5], 0.0);
}

#[test]
fn partial_force_channels_identify_each_direction_and_admit_explicit_perfect_plasticity() {
    for hardening in [[100.0, 50.0, 25.0], [0.0; 3]] {
        let mut source = input();
        for (index, history) in source.experiments.iter_mut().enumerate() {
            let mut z = [0.0; 3];
            let mut accumulated = [0.0; 3];
            for sample in &mut history.samples {
                for i in 0..3 {
                    let q = [1000.0, 500.0, 250.0][i];
                    let y = [10.0, 8.0, 5.0][i];
                    let trial = q * (sample.green_strain[i] - z[i]);
                    let dz = trial.signum()
                        * (trial.abs() - y - hardening[i] * accumulated[i]).max(0.0)
                        / (q + hardening[i]);
                    z[i] += dz;
                    accumulated[i] += dz.abs();
                    if sample.second_piola_n_m[i].is_some() {
                        sample.second_piola_n_m[i] = Some(q * (sample.green_strain[i] - z[i]));
                    }
                    // Only the loaded axis was observed in each directional FIT experiment.
                    if index < 3 && i != index {
                        sample.second_piola_n_m[i] = None;
                    }
                }
            }
        }
        let r = calibrate(&source).unwrap();
        assert_eq!(r.status, Status::Accepted, "{r:#?}");
        assert_eq!(r.fit.as_ref().unwrap().components, 24);
        let p = r.parameters.unwrap();
        for i in 0..3 {
            near(p.yield_n_m[i], [10., 8., 5.][i], 1e-6);
            near(p.hardening_n_m[i], hardening[i], 1e-6);
        }
        if hardening == [0.0; 3] {
            assert!(r.diagnostics.at_bounds[3..].iter().all(|v| *v));
        }
    }
}

#[test]
fn work_limits_include_final_diagnostics_and_check_without_publishing_partial_materials() {
    let mut source = input();
    let complete = calibrate(&source).unwrap();
    assert_eq!(complete.status, Status::Accepted);
    for limit in [
        1,
        complete.diagnostics.evaluations - 1,
        complete.diagnostics.evaluations - 2,
    ] {
        source.max_evaluations = limit;
        let r = calibrate(&source).unwrap();
        assert_eq!(r.status, Status::BudgetExhausted, "{r:#?}");
        assert!(r.material.is_none());
        assert_eq!(r.diagnostics.evaluations, limit);
        assert!(r.diagnostics.history_steps <= r.diagnostics.history_step_limit);
    }
}

#[test]
fn input_gates_preserve_domains_identity_evidence_and_replacement_consent() {
    let original = serde_json::to_value(input()).unwrap();
    let mut cases = Vec::new();
    for field in ["allow_estimated_material", "allow_estimated_observations"] {
        let mut v = original.clone();
        v[field] = json!(false);
        cases.push(v);
    }
    for (pointer, value) in [
        (
            "/material/properties/membrane_strain_limit_warp/value",
            json!(0.05),
        ),
        (
            "/material/properties/membrane_accumulation_limit_warp/conditions/specimen_id",
            json!("different-lot"),
        ),
        ("/experiments/0/source", json!("absent")),
        ("/experiments/0/samples/0/green_strain/0", json!(0.01)),
        ("/experiments/0/samples/1/time_seconds", json!(0)),
        ("/experiments/0/samples/1/green_strain/0", json!(-0.5)),
        ("/yield_bounds_n_m/0", json!([1.0, 1.0])),
        ("/hardening_bounds_n_m/0", json!([-1.0, 10.0])),
    ] {
        let mut v = original.clone();
        *v.pointer_mut(pointer).unwrap() = value;
        cases.push(v);
    }
    for v in cases {
        assert!(
            calibrate(&serde_json::from_value(v.clone()).unwrap()).is_err(),
            "{v}"
        );
    }
    let mut source = input();
    source.material.properties.insert(
        "membrane_yield_warp".into(),
        source.material.properties["warp_stiffness"].clone(),
    );
    assert!(calibrate(&source).is_err());
    source.replace_existing = true;
    assert_eq!(calibrate(&source).unwrap().status, Status::Accepted);
}

#[test]
fn coupled_equilibrium_recovers_yield_and_hardening_from_independent_active_equations() {
    let mut source = input();
    source
        .material
        .properties
        .get_mut("poisson_warp_weft")
        .unwrap()
        .value = 0.2;
    source.experiments.truncate(1);
    let history = &mut source.experiments[0];
    history.samples.truncate(1);
    let d = 1.0 - 0.2 * (0.2 * 500.0 / 1000.0);
    let q00 = 1000.0 / d;
    let q01 = 100.0 / d;
    let q11 = 500.0 / d;
    let mut z = [0.0; 3];
    let mut accumulated = [0.0; 3];
    for (index, delta) in [
        [0.02_f64, 0.03, 0.025],
        [0.03, 0.02, 0.015],
        [-0.06, -0.07, -0.06],
        [0.04, 0.05, 0.045],
    ]
    .into_iter()
    .enumerate()
    {
        let mut stress = [0.0; 3];
        for i in 0..3 {
            z[i] += delta[i];
            accumulated[i] += delta[i].abs();
            stress[i] =
                delta[i].signum() * ([10., 8., 5.][i] + [100., 50., 25.][i] * accumulated[i]);
        }
        // Construct e=z+Q^-1*S from independently chosen active plastic increments and yield forces.
        let elastic1 = (stress[1] - q01 / q00 * stress[0]) / (q11 - q01 * q01 / q00);
        let strain = [
            z[0] + stress[0] / q00 - q01 / q00 * elastic1,
            z[1] + elastic1,
            z[2] + stress[2] / 250.0,
        ];
        history.samples.push(Sample {
            time_seconds: (2 * index + 1) as f64,
            green_strain: strain,
            second_piola_n_m: stress.map(Some),
            scale_n_m: 1.0,
        });
        history.samples.push(Sample {
            time_seconds: (2 * index + 2) as f64,
            green_strain: z,
            second_piola_n_m: [Some(0.0); 3],
            scale_n_m: 1.0,
        });
    }
    let r = calibrate(&source).unwrap();
    assert_eq!(r.status, Status::Accepted, "{r:#?}");
    let p = r.parameters.unwrap();
    for i in 0..3 {
        near(p.yield_n_m[i], [10., 8., 5.][i], 1e-6);
        near(p.hardening_n_m[i], [100., 50., 25.][i], 1e-6);
    }
}

#[test]
fn constrained_solution_does_not_accept_a_clipped_nonstationary_corner() {
    let mut source = input();
    source.experiments.retain(|e| e.partition == Partition::Fit);
    source.experiments[0].samples.truncate(1);
    for (time, e, s) in [(1.0, 0.02, 120.0 / 11.0), (2.0, 0.10, 200.0 / 11.0)] {
        source.experiments[0].samples.push(Sample {
            time_seconds: time,
            green_strain: [e, 0., 0.],
            second_piola_n_m: [Some(s), Some(0.), Some(0.)],
            scale_n_m: 1.0,
        });
    }
    source.yield_bounds_n_m[0] = [11.0, 20.0];
    source.hardening_bounds_n_m[0] = [0.0, 50.0];
    source.max_rms_scaled = 10.0;
    source.max_peak_scaled = 10.0;
    let r = calibrate(&source).unwrap();
    let p = r.parameters.as_ref().unwrap();
    assert!(p.yield_n_m[0] > 11.5, "clipped corner retained: {r:#?}");
    if r.status == Status::Accepted {
        near(p.yield_n_m[0], 135.0 / 11.0, 1e-5);
        near(p.hardening_n_m[0], 50.0, 1e-7);
        assert!(r.diagnostics.at_bounds[3]);
    } else {
        assert!(r.material.is_none());
    }
}
