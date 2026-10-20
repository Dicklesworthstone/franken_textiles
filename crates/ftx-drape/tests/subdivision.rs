//! Synthetic convergence/transaction checks on the real implicit solver, not textile measurements.
use ftx_core::{RefusalCode, Vec2, Vec3};
use ftx_drape::stepping::SubdivisionOptions;
use ftx_drape::{Body, ClothModel, ContactParams, Membrane, Solver};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_membrane_plasticity::PlasticMembrane;
use ftx_pattern::materials::Catalog;

fn solver(history: usize) -> Solver {
    let mut material = Membrane::orthotropic(1000., 600., 0., 300., 1.);
    let snapshot = if history == 1 {
        let selection: Selection = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-relaxation.json"
        ))
        .unwrap();
        let snapshot = Catalog::builtin().unwrap().resolve(&selection).unwrap();
        let m = snapshot.mechanics().unwrap();
        material = Membrane::orthotropic(m.warp_n_m, m.weft_n_m, m.poisson, m.shear_n_m, 1.);
        Some(snapshot)
    } else {
        None
    };
    let model = ClothModel::new(
        vec![Vec2::ZERO, Vec2::new(0.1, 0.), Vec2::new(0., 0.1)],
        &[[0, 1, 2]],
        material,
        0.,
        0.2,
    )
    .unwrap();
    let mut s = Solver::new(
        model,
        vec![Vec3::ZERO, Vec3::new(0.11, 0., 0.), Vec3::new(0., 0.1, 0.)],
        &[[true; 3], [false, true, true], [true; 3]],
        Vec3::ZERO,
    );
    s.tol = 1e-6;
    s.pcg_iters = 10;
    if let Some(snapshot) = snapshot {
        s = s
            .with_relaxation(&[snapshot.membrane_relaxation().unwrap()])
            .unwrap();
    } else if history == 2 {
        s = s
            .with_membrane_plasticity(&[Some(
                PlasticMembrane::new(
                    material.q,
                    [20., 30., 10.],
                    [100., 80., 40.],
                    [0.4; 3],
                    [0.4; 3],
                )
                .unwrap(),
            )])
            .unwrap();
    }
    s
}

#[test]
fn converged_nominal_step_preserves_the_entire_existing_state_bit_for_bit() {
    for history in 0..3 {
        let mut original = solver(history);
        let mut adaptive = original.clone();
        let report = original.step(0.01).unwrap();
        assert!(report.converged);
        let subdivided = adaptive
            .step_subdivided(0.01, SubdivisionOptions::default())
            .unwrap();
        assert_eq!(subdivided.trials, 1);
        assert_eq!(subdivided.accepted.len(), 1);
        assert_eq!(subdivided.accepted[0].solve, report);
        assert_eq!(subdivided.accepted[0].step_s, 0.01);
        assert_eq!(subdivided.accepted[0].end_s, 0.01);
        assert_eq!(format!("{original:?}"), format!("{adaptive:?}"));
    }
}

#[test]
fn actual_failed_newton_steps_subdivide_and_replay_without_advancing_failed_history() {
    for history in 0..3 {
        let mut s = solver(history);
        s.max_newton = 3;
        let initial = s.clone();
        let direct = s.clone().step(0.04);
        assert!(
            !direct.as_ref().is_ok_and(|r| r.converged),
            "fixture must fail a nominal step: {direct:?}"
        );
        let policy = SubdivisionOptions {
            max_halvings: 12,
            ..SubdivisionOptions::default()
        };
        let report = s.step_subdivided(0.04, policy).unwrap();
        assert!(report.trials > report.accepted.len());
        assert!(report.accepted.len() > 1);
        let mut replay = initial.clone();
        let mut time = 0.;
        for accepted in &report.accepted {
            assert!(accepted.solve.converged);
            assert_eq!(replay.step(accepted.step_s).unwrap(), accepted.solve);
            assert!(accepted.end_s > time);
            time = accepted.end_s;
        }
        assert_eq!(time, 0.04);
        assert!((report.accepted.iter().map(|r| r.step_s).sum::<f64>() - 0.04).abs() < 1e-15);
        assert_eq!(format!("{s:?}"), format!("{replay:?}"));
        if history == 1 {
            let memory = s.relaxation_report().unwrap().unwrap();
            assert_eq!(memory["accepted_steps"], report.accepted.len());
            assert!((memory["time_seconds"].as_f64().unwrap() - 0.04).abs() < 1e-15);
        }
        if history == 2 {
            assert_eq!(
                s.membrane_plasticity_report().unwrap().unwrap()["accepted_steps"],
                report.accepted.len()
            );
        }
        // Stop one attempt before the successful trajectory finishes. Even already accepted
        // substeps and warm linear-solver caches must remain invisible to the caller.
        let mut failed = initial;
        let before = format!("{failed:?}");
        let error = failed
            .step_subdivided(
                0.04,
                SubdivisionOptions {
                    max_trials: report.trials - 1,
                    ..policy
                },
            )
            .unwrap_err();
        assert_eq!(error.code, RefusalCode::BudgetExceeded);
        assert_eq!(format!("{failed:?}"), before);
    }
}

#[test]
fn explicit_depth_limit_and_malformed_policies_never_mutate_state() {
    let mut s = solver(2);
    s.max_newton = 3;
    let before = format!("{s:?}");
    let error = s
        .step_subdivided(
            0.04,
            SubdivisionOptions {
                max_halvings: 0,
                max_trials: 4,
            },
        )
        .unwrap_err();
    assert_eq!(error.code, RefusalCode::NotConverged);
    assert_eq!(format!("{s:?}"), before);
    for (duration, policy) in [
        (0., SubdivisionOptions::default()),
        (f64::NAN, SubdivisionOptions::default()),
        (f64::MAX, SubdivisionOptions::default()),
        (f64::MIN_POSITIVE, SubdivisionOptions::default()),
        (
            0.01,
            SubdivisionOptions {
                max_halvings: 17,
                max_trials: 4,
            },
        ),
        (
            0.01,
            SubdivisionOptions {
                max_halvings: 4,
                max_trials: 0,
            },
        ),
        (
            0.01,
            SubdivisionOptions {
                max_halvings: 4,
                max_trials: 65536,
            },
        ),
    ] {
        assert_eq!(
            s.step_subdivided(duration, policy).unwrap_err().code,
            RefusalCode::OutOfRange
        );
        assert_eq!(format!("{s:?}"), before);
    }
    s.model.compression[0] = [0.5; 2];
    let before = format!("{s:?}");
    assert_eq!(
        s.step_subdivided(0.01, SubdivisionOptions::default())
            .unwrap_err()
            .code,
        RefusalCode::OutOfRange
    );
    assert_eq!(format!("{s:?}"), before);
}

#[test]
fn contact_friction_and_factor_caches_replay_and_rollback_with_the_physical_partition() {
    let mut s = solver(0);
    for x in &mut s.x {
        x.z = 0.0005;
    }
    let body = Body::new(
        vec![
            Vec3::new(-1., -1., 0.),
            Vec3::new(1., -1., 0.),
            Vec3::new(0., 1., 0.),
        ],
        vec![[0, 1, 2]],
        0.1,
    )
    .unwrap();
    let params = ContactParams {
        dhat: 0.001,
        thickness: 0.0002,
        kappa: 100.,
        mu: 0.4,
        eps_v: 0.001,
    };
    s = s
        .with_contact(body, params)
        .unwrap()
        .with_self_contact(params)
        .unwrap();
    assert!(s.step(0.00001).unwrap().converged); // nonempty lag and factor caches at entry
    s.max_newton = 3;
    s.friction_rounds = 0;
    let original = s.clone();
    let policy = SubdivisionOptions {
        max_halvings: 16,
        ..SubdivisionOptions::default()
    };
    let report = s.step_subdivided(0.001, policy).unwrap();
    assert!(report.trials > report.accepted.len());
    let mut replay = original.clone();
    for step in &report.accepted {
        assert_eq!(replay.step(step.step_s).unwrap(), step.solve);
        let c = replay.contact.as_ref().unwrap();
        c.audit_self_contact(&replay.x).unwrap();
        assert!(c.min_gap(&replay.x, params.dhat).unwrap() > 0.);
    }
    assert_eq!(format!("{s:?}"), format!("{replay:?}"));
    let mut failed = original;
    let before = format!("{failed:?}");
    assert_eq!(
        failed
            .step_subdivided(
                0.001,
                SubdivisionOptions {
                    max_trials: report.trials - 1,
                    ..policy
                }
            )
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(format!("{failed:?}"), before);
}
