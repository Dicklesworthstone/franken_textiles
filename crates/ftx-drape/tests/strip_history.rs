//! Synthetic known answers for the ordinary timed coupon consumer, not measured fabric evidence.
use ftx_core::RefusalCode;
use ftx_drape::strip::{
    Fixture,
    history::{Controls, Loading, Request, evaluate},
};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::materials::Catalog;

fn selection(plastic: bool) -> Selection {
    let source = if plastic {
        include_str!("../../../tests/fixtures/materials/synthetic-membrane-plasticity.json")
    } else {
        include_str!("../../../tests/fixtures/materials/synthetic-relaxation.json")
    };
    let mut selected: Selection = serde_json::from_str(source).unwrap();
    let material = selected.record.as_mut().unwrap();
    material.properties.retain(|key, _| {
        matches!(
            key.as_str(),
            "areal_density"
                | "warp_stiffness"
                | "weft_stiffness"
                | "poisson_warp_weft"
                | "shear_stiffness"
        ) || key.starts_with("membrane_")
    });
    material
        .properties
        .get_mut("poisson_warp_weft")
        .unwrap()
        .value = 0.0;
    // A declared low-mass synthetic sheet approaches the analytical homogeneous quasistatic bar.
    // The finite production solve still retains and reports this real, nonzero numerical mass.
    material.properties.get_mut("areal_density").unwrap().value = 1e-8;
    selected
}

fn request(plastic: bool) -> Request {
    Request {
        schema: "ftx-strip-history/1".into(),
        material: selection(plastic),
        fixture: Fixture {
            gauge_length_m: 0.1,
            width_m: 0.06,
            warp_axis: [0., 1.],
            cells: [2, 2],
        },
        loading: [(0., 0.), (0.05, 0.02), (0.15, 0.02), (0.2, 0.), (0.25, 0.)]
            .into_iter()
            .map(|(time_s, extension_fraction)| Loading {
                time_s,
                extension_fraction,
            })
            .collect(),
        controls: Controls::default(),
    }
}

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} differs from {expected}"
    );
}

#[test]
fn maxwell_load_hold_unload_matches_bar_reactions_storage_and_irreversible_work() {
    let input = request(false);
    let result = evaluate(&input).unwrap();
    let (width, area, young, branch, tau) = (0.06, 0.006, 1000., 2000., 0.05);
    let mut viscous = 0.0;
    let mut dissipated = 0.0;
    for (i, state) in result.states.iter().enumerate() {
        let stretch = 1.0 + state.extension_fraction;
        let strain = 0.5 * (stretch * stretch - 1.0);
        if i > 0 {
            let ratio = tau / (tau + state.time_step_s);
            let delta = strain - viscous;
            dissipated += area * ratio * (1.0 - ratio) * branch * delta * delta;
            viscous = ratio * viscous + (1.0 - ratio) * strain;
        }
        let stress = young * strain + branch * (strain - viscous);
        let energy = 0.5 * area * (young * strain * strain + branch * (strain - viscous).powi(2));
        near(state.force_n, width * stretch * stress, 2e-7);
        near(state.fixed_grip_force_n, -state.force_n, 2e-7);
        near(state.stored_membrane_energy_j, energy, 2e-9);
        near(state.maxwell_dissipation_j, dissipated, 2e-9);
        near(state.mid_width_m, width, 2e-9);
        assert_eq!(state.plastic_dissipation_j, 0.0);
        assert!(state.free_dynamic_residual_n <= input.controls.force_tolerance_n);
        if i > 0 {
            assert!(state.solve.unwrap().converged);
        }
    }
    let at = |t| result.states.iter().find(|s| s.time_s == t).unwrap();
    assert!(at(0.15).force_n < at(0.05).force_n);
    assert!(
        at(0.2).force_n < 0.0,
        "retained transient stress must survive unloading"
    );
    assert!(at(0.25).force_n.abs() < at(0.2).force_n.abs());
    near(result.reference_mass_kg, area * 1e-8, 1e-24);
}

#[test]
fn step_refinement_approaches_continuous_ramp_and_hold_solution() {
    let mut input = request(false);
    input.loading.truncate(3);
    let mut errors = Vec::new();
    // Green strain during linear grip motion is r*t + (r*t)^2/2. Convolve its derivative
    // r + r^2*t with the Maxwell exponential analytically, then decay over the hold.
    let (rate, ramp, hold, tau): (f64, f64, f64, f64) = (0.4, 0.05, 0.1, 0.05);
    let factor = 1.0 - (-ramp / tau).exp();
    let transient = 2000.
        * (rate * tau * factor + rate * rate * (tau * ramp - tau * tau * factor))
        * (-hold / tau).exp();
    let strain = 0.5 * (1.02_f64.powi(2) - 1.0);
    let exact_force = 0.06 * 1.02 * (1000. * strain + transient);
    for step in [0.01, 0.005, 0.0025] {
        input.controls.max_step_s = step;
        let result = evaluate(&input).unwrap();
        errors.push((result.states.last().unwrap().force_n - exact_force).abs());
    }
    assert!(errors[1] < errors[0], "{errors:?}");
    assert!(errors[2] < 0.6 * errors[0], "{errors:?}");
}

#[test]
fn permanent_strain_survives_loading_and_reversal_with_analytical_bar_work() {
    let input = request(true);
    let result = evaluate(&input).unwrap();
    let (young, yield_n_m, hardening, area) = (1000., 0.2, 20., 0.006);
    let (mut permanent, mut accumulated, mut dissipated) = (0., 0., 0.);
    for (i, state) in result.states.iter().enumerate() {
        let stretch = 1.0 + state.extension_fraction;
        let strain = 0.5 * (stretch * stretch - 1.0);
        if i > 0 {
            let trial = young * (strain - permanent);
            let delta = trial.signum()
                * (trial.abs() - yield_n_m - hardening * accumulated).max(0.)
                / (young + hardening);
            permanent += delta;
            accumulated += delta.abs();
            dissipated += area * yield_n_m * delta.abs();
        }
        near(
            state.force_n,
            0.06 * stretch * young * (strain - permanent),
            2e-7,
        );
        near(state.plastic_dissipation_j, dissipated, 2e-9);
        near(
            state.stored_membrane_energy_j,
            0.5 * area
                * (young * (strain - permanent).powi(2) + hardening * accumulated * accumulated),
            2e-9,
        );
        assert_eq!(state.maxwell_dissipation_j, 0.0);
        assert!(state.relaxation.is_none());
    }
    let last = result.states.last().unwrap();
    assert!(last.force_n < 0.0);
    assert!(last.plastic_dissipation_j > 0.0);
    assert!(
        last.plasticity.as_ref().unwrap()["elements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["plastic_green_strain"][0].as_f64().unwrap().abs() > 1e-4)
    );
}

#[test]
fn source_mass_changes_inertia_without_changing_fixture_and_missing_other_properties_is_allowed() {
    let mut light = request(false);
    light.loading.truncate(2);
    light.loading[1].time_s = 0.001;
    light.material.record.as_mut().unwrap().membrane_relaxation = None;
    light
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("areal_density")
        .unwrap()
        .value = 0.2;
    let mut heavy = light.clone();
    heavy
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("areal_density")
        .unwrap()
        .value = 20.;
    let a = evaluate(&light).unwrap();
    let b = evaluate(&heavy).unwrap();
    assert_eq!(a.reference_positions_m, b.reference_positions_m);
    assert_eq!(a.triangles, b.triangles);
    near(b.reference_mass_kg, 100. * a.reference_mass_kg, 1e-15);
    assert_ne!(a.input_sha256, b.input_sha256);
    assert_ne!(
        a.states.last().unwrap().positions_m,
        b.states.last().unwrap().positions_m
    );
    assert_ne!(
        a.states.last().unwrap().force_n,
        b.states.last().unwrap().force_n
    );
    assert!(a.states.last().unwrap().free_kinetic_energy_j > 0.);
    assert!(
        !light
            .material
            .record
            .as_ref()
            .unwrap()
            .properties
            .contains_key("thickness")
    );
}

#[test]
fn frozen_replay_is_exact_and_missing_evidence_domain_or_late_budget_refuses_atomically() {
    let input = request(false);
    let report = evaluate(&input).unwrap();
    let replay: Request = serde_json::from_str(&report.replay_request_json).unwrap();
    assert_eq!(
        ftx_core::receipt::sha256_hex(report.replay_request_json.as_bytes()),
        report.input_sha256
    );
    assert_eq!(
        serde_json::to_vec(&report).unwrap(),
        serde_json::to_vec(&evaluate(&replay).unwrap()).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&report).unwrap(),
        serde_json::to_vec(&evaluate(&report.input).unwrap()).unwrap()
    );
    let mut stale = report.input.clone();
    stale
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("areal_density")
        .unwrap()
        .value *= 2.;
    assert!(evaluate(&stale).is_err());
    let mut missing = input.clone();
    missing
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .remove("areal_density");
    assert!(evaluate(&missing).is_err());
    let mut denied = input.clone();
    denied.material.allow_estimates = false;
    assert!(evaluate(&denied).is_err());
    let mut over_budget = input.clone();
    over_budget.controls.max_steps = 1;
    assert_eq!(
        evaluate(&over_budget).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    let mut large_history = input.clone();
    large_history.fixture.cells = [16, 16];
    large_history.controls.max_step_s = 0.001;
    let spectrum = large_history
        .material
        .record
        .as_mut()
        .unwrap()
        .membrane_relaxation
        .as_mut()
        .unwrap();
    let branch = spectrum.branches[0].clone();
    spectrum.branches = (0..16)
        .map(|i| {
            let mut b = branch.clone();
            b.id = format!("synthetic-branch-{i}");
            b
        })
        .collect();
    let refused = evaluate(&large_history).unwrap_err();
    assert_eq!(refused.code, RefusalCode::BudgetExceeded);
    assert!(refused.message.contains("geometry/history"));
    let mut exhausted = input.clone();
    exhausted.controls.max_newton = 1;
    assert_eq!(
        evaluate(&exhausted).unwrap_err().code,
        RefusalCode::NotConverged
    );
    let mut outside = input.clone();
    outside
        .material
        .record
        .as_mut()
        .unwrap()
        .membrane_relaxation
        .as_mut()
        .unwrap()
        .strain_range[0] = [-0.1, 0.03];
    outside.loading.last_mut().unwrap().extension_fraction = 0.5;
    let before = serde_json::to_vec(&outside).unwrap();
    assert!(evaluate(&outside).is_err());
    assert_eq!(before, serde_json::to_vec(&outside).unwrap());
    assert_eq!(
        serde_json::to_vec(&report).unwrap(),
        serde_json::to_vec(&evaluate(&input).unwrap()).unwrap()
    );
}

#[test]
fn membrane_only_maxwell_compilation_preserves_the_full_mechanics_tensor_and_rejects_bad_laws() {
    let full: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation.json"
    ))
    .unwrap();
    let db = Catalog::builtin().unwrap();
    let resolved = db.resolve(&full).unwrap();
    let expected = resolved.mechanics().unwrap().origin_tangent();
    let mut partial = full;
    partial
        .record
        .as_mut()
        .unwrap()
        .properties
        .retain(|key, _| {
            matches!(
                key.as_str(),
                "warp_stiffness" | "weft_stiffness" | "poisson_warp_weft" | "shear_stiffness"
            )
        });
    let resolved = db.resolve(&partial).unwrap();
    assert!(resolved.mechanics().is_err());
    assert_eq!(
        resolved
            .membrane_relaxation()
            .unwrap()
            .unwrap()
            .origin_tangent(),
        expected
    );
    partial
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("shear_stiffness")
        .unwrap()
        .value = -1.;
    assert!(db.resolve(&partial).unwrap().membrane_relaxation().is_err());
}
