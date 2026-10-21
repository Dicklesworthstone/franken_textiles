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
    assert_eq!(
        report.input_sha256,
        "955c44f4e507d016a82d133fabebdbd9cb560c24cf309fc39aee246bcbfd6be1"
    );
    assert!(
        report
            .orthotropic_fit
            .as_ref()
            .unwrap()
            .time_search
            .is_none()
    );
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

fn automatic_input() -> Input {
    let mut input = input();
    let options = input.orthotropic.as_mut().unwrap();
    options.times_seconds.clear();
    options.time_bounds_seconds = vec![[0.1, 0.9], [1., 4.], [5., 24.]];
    input
}

#[test]
fn automatic_directional_times_and_tensors_recover_independent_three_branch_histories() {
    let report = calibrate(&automatic_input()).unwrap();
    assert_tensors(&tensors(&report), &TENSORS[..3], 1e-6);
    let diagnostics = report.orthotropic_fit.as_ref().unwrap();
    for (candidate, expected) in diagnostics.candidates.iter().zip([0.3, 2., 8.]) {
        assert!((candidate.time_seconds / expected - 1.).abs() < 1e-7);
        assert!(candidate.active);
    }
    let search = diagnostics.time_search.as_ref().unwrap();
    assert_eq!(search.starts_attempted, 27);
    assert!(search.starts_converged > 0);
    assert!(search.objective_evaluations <= 1024);
    assert!(search.projected_iterations <= 262_144);
    assert!(search.gradient_scaled_max <= 1e-10);
    assert!((search.joint_excitation_pivot - 0.0506243822453).abs() < 1e-6);
    for (actual, expected) in
        search
            .time_sensitivity
            .iter()
            .zip([4.05625775340, 3.10283848192, 1.54307243742])
    {
        assert!((actual - expected).abs() < 1e-6);
    }
    assert!(search.boundary_distance_fraction.iter().all(|v| *v > 1e-5));
    assert!(report.fit.rms_scaled < 1e-8);
    assert!(report.check.unwrap().rms_scaled < 1e-8);
}

#[test]
fn automatic_one_and_two_branch_directional_spectra_keep_inactive_axes() {
    for (times, expected, windows) in [
        (vec![0.6], vec![[300., 0., 0., 0.]], vec![[0.2, 1.8]]),
        (
            vec![0.3, 8.],
            vec![TENSORS[0], [0., 120., 0., 90.]],
            vec![[0.1, 0.9], [3., 20.]],
        ),
    ] {
        let mut input = input();
        input.orthotropic.as_mut().unwrap().times_seconds = times.clone();
        observations(&mut input, &expected);
        let options = input.orthotropic.as_mut().unwrap();
        options.times_seconds.clear();
        options.time_bounds_seconds = windows;
        let report = calibrate(&input).unwrap();
        assert_tensors(&tensors(&report), &expected, 1e-6);
        for (candidate, expected) in report.orthotropic_fit.unwrap().candidates.iter().zip(times) {
            assert!((candidate.time_seconds / expected - 1.).abs() < 1e-7);
        }
    }
}

#[test]
fn automatic_psd_boundary_fit_finds_time_with_unequal_sensor_scales() {
    let mut input = input();
    let options = input.orthotropic.as_mut().unwrap();
    options.times_seconds = vec![1.];
    options.zero_threshold_n_m = 0.;
    input.experiments.truncate(3);
    // Independent constrained oracle: all three acquisitions share the exponential kernel, so
    // Cauchy-Schwarz places the optimum at the generating time even though its tensor is indefinite.
    observations(&mut input, &[[1., 1., 10., 2.]]);
    for sample in &mut input.experiments[1].samples {
        sample.scale_n_m = 100.;
    }
    let options = input.orthotropic.as_mut().unwrap();
    options.times_seconds.clear();
    options.time_bounds_seconds = vec![[0.1, 5.]];
    input.max_rms_scaled = 0.01;
    input.max_peak_scaled = 0.03;
    let report = calibrate(&input).unwrap();
    assert_tensors(
        &tensors(&report),
        &[[1.2628682, 78.0451699, 9.92777735, 2.]],
        1e-4,
    );
    let diagnostics = report.orthotropic_fit.as_ref().unwrap();
    assert!((diagnostics.candidates[0].time_seconds - 1.).abs() < 1e-6);
    assert!(diagnostics.psd_projection_applied);
    let objective = report.fit.components as f64 * report.fit.rms_scaled.powi(2);
    assert!((objective - 0.001737491384).abs() < 1e-9);
}

#[test]
fn automatic_time_selection_never_reads_held_out_forces_or_residual_budgets() {
    let mut input = automatic_input();
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
    assert_ne!(report.input_sha256, original.input_sha256);
}

#[test]
fn automatic_times_refuse_inactive_branches_and_conflicting_or_unresolved_windows() {
    for change in [
        (|i: &mut Input| i.orthotropic.as_mut().unwrap().times_seconds = vec![1.])
            as fn(&mut Input),
        |i| i.orthotropic.as_mut().unwrap().time_bounds_seconds = vec![[0.1, 1.], [0.5, 2.]],
        |i| i.orthotropic.as_mut().unwrap().time_bounds_seconds = vec![[0., 1.]],
        |i| i.orthotropic.as_mut().unwrap().time_bounds_seconds = vec![[1., 1.0000000001]],
        |i| {
            i.orthotropic.as_mut().unwrap().time_bounds_seconds =
                vec![[0.1, 1.], [2., 3.], [4., 5.], [6., 7.]]
        },
    ] {
        let mut input = automatic_input();
        change(&mut input);
        assert!(calibrate(&input).is_err());
    }
    let mut zero = input();
    observations(&mut zero, &[[0.; 4]; 4]);
    let options = zero.orthotropic.as_mut().unwrap();
    options.times_seconds.clear();
    options.time_bounds_seconds = vec![[0.1, 5.]];
    assert!(
        calibrate(&zero)
            .unwrap_err()
            .message
            .contains("not observably")
    );
    let mut thresholded = automatic_input();
    thresholded.orthotropic.as_mut().unwrap().zero_threshold_n_m = 1000.;
    assert!(
        calibrate(&thresholded)
            .unwrap_err()
            .message
            .contains("inactive")
    );
}

fn comparison_input() -> ftx_pattern::material_relaxation::calibration::comparison::ComparisonInput
{
    use ftx_pattern::material_relaxation::calibration::comparison::{
        ComparisonInput, Model, SCHEMA,
    };
    let mut calibration = input();
    let mut complete = calibration.orthotropic.take().unwrap();
    complete.times_seconds = vec![0.3, 2., 8.];
    let mut underspecified = complete.clone();
    underspecified.times_seconds = vec![0.3, 2.];
    ComparisonInput {
        schema: SCHEMA.into(),
        calibration,
        candidates: vec![
            Model {
                id: "complete".into(),
                orthotropic: complete,
            },
            Model {
                id: "underspecified".into(),
                orthotropic: underspecified,
            },
        ],
        fit_rms_equivalence_scaled: 1e-7,
        select_candidate_id: None,
        max_objective_evaluations: 1024,
        max_projected_iterations: 262_144,
    }
}

fn fitted_equilibrium(input: &mut Input) {
    let bounds = "[[-0.08,0.08],[-0.08,0.08],[-0.08,0.08]]";
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
fn directional_relaxation_checks_scalar_equilibrium_bounds_in_fixed_and_searched_modes() {
    for searched in [false, true] {
        let mut raw = input();
        fitted_equilibrium(&mut raw);
        if searched {
            let options = raw.orthotropic.as_mut().unwrap();
            options.times_seconds.clear();
            options.time_bounds_seconds = vec![[0.1, 0.6], [1., 3.], [5., 12.]];
        }
        let node = &mut raw.experiments[0].samples[1];
        node.deformation = [[1.2_f64.sqrt(), 0.], [0., 1.]];
        node.nominal_n_m = [[None; 2]; 2];
        assert!(
            calibrate(&raw)
                .unwrap_err()
                .message
                .contains("outside fitted equilibrium strain domain")
        );
    }
}

#[test]
fn check_equilibrium_domain_refusal_cannot_change_directional_model_selection() {
    use ftx_pattern::material_relaxation::calibration::comparison::{CheckStatus, Status, compare};
    let mut request = comparison_input();
    fitted_equilibrium(&mut request.calibration);
    let original = compare(&request).unwrap();
    assert_eq!(original.status, Status::Accepted);
    let node = &mut request.calibration.experiments.last_mut().unwrap().samples[1];
    node.deformation = [[1.2_f64.sqrt(), 0.], [0., 1.]];
    node.nominal_n_m = [[None; 2]; 2];
    // CHECK may appear before FIT in the authored order; no zip/index correspondence may be lost.
    request.calibration.experiments.rotate_right(1);
    let refused = compare(&request).unwrap();
    assert_eq!(refused.status, Status::ChosenCandidateRejected);
    assert_eq!(
        serde_json::to_value(&refused.fit_decision).unwrap(),
        serde_json::to_value(&original.fit_decision).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&refused.prediction_differences).unwrap(),
        serde_json::to_value(&original.prediction_differences).unwrap()
    );
    assert_eq!(
        refused.objective_evaluations,
        original.objective_evaluations
    );
    assert_eq!(refused.projected_iterations, original.projected_iterations);
    assert_eq!(refused.candidates[0].check_status, CheckStatus::Rejected);
    assert!(
        refused.candidates[0]
            .check_refusal
            .as_ref()
            .unwrap()
            .message
            .contains("outside fitted equilibrium strain domain")
    );
    assert!(refused.selected_calibration().is_none());
    for (a, b) in refused.candidates.iter().zip(&original.candidates) {
        let (a, b) = (
            a.calibration.as_ref().unwrap(),
            b.calibration.as_ref().unwrap(),
        );
        assert_eq!(
            serde_json::to_value(&a.fit).unwrap(),
            serde_json::to_value(&b.fit).unwrap()
        );
        assert_tensors(&tensors(a), &tensors(b), 0.);
    }
}

#[test]
fn comparison_selects_a_supported_order_and_retains_underfit_forces_and_real_material() {
    use ftx_pattern::material_relaxation::calibration::comparison::{
        CheckStatus, FitStatus, Status, compare,
    };
    let request = comparison_input();
    let report = compare(&request).unwrap();
    assert_eq!(report.status, Status::Accepted);
    assert!(report.fit_decision.comparison_complete);
    assert_eq!(
        report.fit_decision.chosen_candidate_id.as_deref(),
        Some("complete")
    );
    assert!(!report.fit_decision.ambiguous);
    assert_eq!(report.candidates[0].fit_status, FitStatus::Accepted);
    assert_eq!(
        report.candidates[1].fit_status,
        FitStatus::ResidualsExceeded
    );
    let rejected = report.candidates[1].calibration.as_ref().unwrap();
    assert!(rejected.fit.rms_scaled > request.calibration.max_rms_scaled);
    assert_eq!(rejected.predictions.len(), 28);
    assert_eq!(report.candidates[0].check_status, CheckStatus::Accepted);
    let selected = report.selected_calibration().unwrap();
    assert_tensors(&tensors(selected), &TENSORS[..3], 1e-7);
    assert_eq!(
        serde_json::to_value(&selected.material.properties).unwrap(),
        serde_json::to_value(&request.calibration.material.properties).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&selected.input.experiments).unwrap(),
        serde_json::to_value(&request.calibration.experiments).unwrap()
    );
    let db = Catalog::builtin().unwrap();
    let material = db
        .resolve(&Selection {
            id: selected.material.id.clone(),
            record: Some(selected.material.clone()),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap();
    let frozen = material.frozen_selection();
    let material = db.resolve(&frozen).unwrap();
    material.mechanics().unwrap();
    let recovery = material.membrane_relaxation().unwrap().unwrap();
    assert!(recovery.has_orthotropic_branches());
    assert_eq!(
        recovery
            .initialize([0.; 3])
            .unwrap()
            .viscous_strains()
            .len(),
        3
    );
    let mut reordered = request;
    reordered.candidates.reverse();
    let repeated = compare(&reordered).unwrap();
    assert_eq!(
        serde_json::to_value(&report.fit_decision).unwrap(),
        serde_json::to_value(&repeated.fit_decision).unwrap()
    );
    assert_eq!(report.objective_evaluations, repeated.objective_evaluations);
    assert_eq!(report.projected_iterations, repeated.projected_iterations);
}

#[test]
fn comparison_never_reselects_on_held_out_forces_even_when_another_model_passes_them() {
    use ftx_pattern::material_relaxation::calibration::comparison::{CheckStatus, Status, compare};
    let mut request = comparison_input();
    let original = compare(&request).unwrap();
    let alternate = original.candidates[1].calibration.as_ref().unwrap();
    for experiment in request
        .calibration
        .experiments
        .iter_mut()
        .filter(|e| e.partition == Partition::Check)
    {
        for sample in &mut experiment.samples {
            let prediction = alternate
                .predictions
                .iter()
                .find(|p| p.experiment == experiment.id && p.time_seconds == sample.time_seconds)
                .unwrap();
            for i in 0..2 {
                for j in 0..2 {
                    if sample.nominal_n_m[i][j].is_some() {
                        sample.nominal_n_m[i][j] = Some(prediction.nominal_n_m[i][j]);
                    }
                }
            }
        }
    }
    let rejected = compare(&request).unwrap();
    assert_eq!(rejected.status, Status::ChosenCandidateRejected);
    assert_eq!(
        serde_json::to_value(&original.fit_decision).unwrap(),
        serde_json::to_value(&rejected.fit_decision).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&original.prediction_differences).unwrap(),
        serde_json::to_value(&rejected.prediction_differences).unwrap()
    );
    assert_eq!(
        original.objective_evaluations,
        rejected.objective_evaluations
    );
    assert_eq!(original.projected_iterations, rejected.projected_iterations);
    assert_eq!(rejected.candidates[0].check_status, CheckStatus::Rejected);
    assert_eq!(rejected.candidates[1].check_status, CheckStatus::Accepted);
    assert!(rejected.selected_calibration().is_none());
}

#[test]
fn equivalent_spectra_report_ambiguity_and_parsimony_without_silent_selection() {
    use ftx_pattern::material_relaxation::calibration::comparison::{Status, compare};
    let mut request = comparison_input();
    request.candidates[1].id = "extra".into();
    request.candidates[1].orthotropic.times_seconds = vec![0.3, 2., 8., 32.];
    let report = compare(&request).unwrap();
    assert_eq!(report.status, Status::Ambiguous);
    assert!(report.fit_decision.ambiguous);
    assert_eq!(
        report.fit_decision.equivalent_candidate_ids,
        ["complete", "extra"]
    );
    assert_eq!(
        report.fit_decision.parsimonious_candidate_id.as_deref(),
        Some("complete")
    );
    assert!(report.fit_decision.chosen_candidate_id.is_none());
    assert!(report.selected_calibration().is_none());
    assert!(
        report.prediction_differences[0]
            .fit_prediction_difference
            .rms_scaled
            < 1e-7
    );
    assert_eq!(report.candidates[0].declared_parameters, 12);
    assert_eq!(report.candidates[1].declared_parameters, 16);
    request.select_candidate_id = Some("complete".into());
    let chosen = compare(&request).unwrap();
    assert_eq!(chosen.status, Status::Accepted);
    assert!(chosen.fit_decision.ambiguous);
    assert_eq!(chosen.fit_decision.basis, "explicit_candidate");
    assert!(chosen.selected_calibration().is_some());
}

#[test]
fn failed_comparison_candidates_consume_shared_work_and_no_partial_selection_escapes() {
    use ftx_pattern::material_relaxation::calibration::comparison::{
        CheckStatus, FitStatus, Status, compare,
    };
    let mut request = comparison_input();
    request.max_objective_evaluations = 1;
    let stopped = compare(&request).unwrap();
    assert_eq!(stopped.status, Status::BudgetStop);
    assert_eq!(stopped.objective_evaluations, 1);
    assert!(stopped.candidates[0].calibration.is_some());
    assert_eq!(stopped.candidates[0].check_status, CheckStatus::NotRun);
    assert!(stopped.selected_calibration().is_none());
    // A rank-deficient force design still consumes its attempted objective evaluation.
    request.candidates[0].orthotropic.times_seconds = vec![0.3, 0.3000000001];
    let failed = compare(&request).unwrap();
    assert_eq!(failed.status, Status::BudgetStop);
    assert_eq!(failed.candidates[0].fit_status, FitStatus::Refused);
    assert_eq!(failed.candidates[0].objective_evaluations, 1);
    assert!(
        failed.candidates[0]
            .refusal
            .as_ref()
            .unwrap()
            .message
            .contains("rank-deficient")
    );
    assert_eq!(failed.objective_evaluations, 1);
    assert!(failed.selected_calibration().is_none());
}

#[test]
fn fixed_and_optimized_orders_share_the_same_total_comparison_budget() {
    use ftx_pattern::material_relaxation::calibration::comparison::{Status, compare};
    let mut acquisition = input();
    acquisition.orthotropic.as_mut().unwrap().times_seconds = vec![0.6];
    observations(&mut acquisition, &[[300., 120., -30., 90.]]);
    let mut request = comparison_input();
    acquisition.orthotropic = None;
    request.calibration = acquisition;
    request.candidates[0].orthotropic.times_seconds = vec![0.6, 8.];
    request.candidates[1].orthotropic.times_seconds.clear();
    request.candidates[1].orthotropic.time_bounds_seconds = vec![[0.2, 1.8]];
    let report = compare(&request).unwrap();
    assert_eq!(report.status, Status::Ambiguous);
    let searched = report.candidates[1]
        .calibration
        .as_ref()
        .unwrap()
        .orthotropic_fit
        .as_ref()
        .unwrap()
        .time_search
        .as_ref()
        .unwrap();
    assert_eq!(
        report.candidates[1].objective_evaluations,
        searched.objective_evaluations
    );
    assert_eq!(
        report.objective_evaluations,
        1 + searched.objective_evaluations
    );
    assert_eq!(
        report.projected_iterations,
        report
            .candidates
            .iter()
            .map(|c| c.projected_iterations)
            .sum::<usize>()
    );
    request.max_objective_evaluations = report.objective_evaluations - 1;
    let stopped = compare(&request).unwrap();
    assert_eq!(stopped.status, Status::BudgetStop);
    assert_eq!(
        stopped.objective_evaluations,
        request.max_objective_evaluations
    );
    assert!(stopped.selected_calibration().is_none());
}

#[test]
fn constrained_candidates_cannot_restart_the_shared_projected_iteration_allowance() {
    use ftx_pattern::material_relaxation::calibration::comparison::{CheckStatus, Status, compare};
    let mut acquisition = input();
    let mut perturbed = TENSORS;
    perturbed[0][2] = -125.;
    perturbed[0][3] = -2.;
    observations(&mut acquisition, &perturbed);
    acquisition.orthotropic = None;
    acquisition.max_rms_scaled = 1.;
    acquisition.max_peak_scaled = 2.;
    let mut request = comparison_input();
    request.calibration = acquisition;
    request.candidates[1].orthotropic.times_seconds = vec![0.3, 2., 8., 32.];
    let complete = compare(&request).unwrap();
    assert!(complete.fit_decision.comparison_complete);
    assert!(
        complete
            .candidates
            .iter()
            .all(|c| c.projected_iterations > 0)
    );
    assert_eq!(
        complete.projected_iterations,
        complete
            .candidates
            .iter()
            .map(|c| c.projected_iterations)
            .sum::<usize>()
    );
    request.max_projected_iterations = complete.projected_iterations - 1;
    let stopped = compare(&request).unwrap();
    assert_eq!(stopped.status, Status::BudgetStop);
    assert_eq!(
        stopped.projected_iterations,
        request.max_projected_iterations
    );
    assert_eq!(
        stopped.candidates[0].projected_iterations,
        complete.candidates[0].projected_iterations
    );
    assert_eq!(stopped.candidates[0].check_status, CheckStatus::NotRun);
    assert!(
        stopped.candidates[1]
            .refusal
            .as_ref()
            .unwrap()
            .message
            .contains("PSD iteration budget")
    );
    assert!(stopped.selected_calibration().is_none());
}

#[test]
fn distinct_recovery_times_can_remain_ambiguous_at_the_declared_force_resolution() {
    use ftx_pattern::material_relaxation::calibration::comparison::{Status, compare};
    let mut acquisition = input();
    acquisition.orthotropic.as_mut().unwrap().times_seconds = vec![0.6];
    observations(&mut acquisition, &[[300., 120., -30., 90.]]);
    acquisition.orthotropic = None;
    acquisition.max_rms_scaled = 1.;
    acquisition.max_peak_scaled = 2.;
    let mut request = comparison_input();
    request.calibration = acquisition;
    request.candidates[0].orthotropic.times_seconds = vec![0.6];
    request.candidates[1].orthotropic.times_seconds = vec![0.7];
    request.fit_rms_equivalence_scaled = 1.;
    let report = compare(&request).unwrap();
    assert_eq!(report.status, Status::Ambiguous);
    assert!(
        report.prediction_differences[0]
            .fit_prediction_difference
            .rms_scaled
            > 1e-4
    );
    assert!(report.fit_decision.chosen_candidate_id.is_none());
    request.fit_rms_equivalence_scaled = 1e-8;
    let resolved = compare(&request).unwrap();
    assert_eq!(resolved.status, Status::Accepted);
    assert_eq!(
        resolved.fit_decision.chosen_candidate_id.as_deref(),
        Some("complete")
    );
}
