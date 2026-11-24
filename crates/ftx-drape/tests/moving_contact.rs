//! Moving-collider regressions use synthetic surfaces, never production body assets.
use ftx_core::{RefusalCode, Vec3};
use ftx_drape::motion::{
    DissipationCapture, MotionOptions, advance_body, advance_body_carried,
    advance_body_carried_with_work,
};
use ftx_drape::{Body, ClothModel, Contact, ContactParams, Membrane, Solver};

fn parameters(mu: f64) -> ContactParams {
    ContactParams {
        dhat: 0.001,
        thickness: 0.0005,
        kappa: 100.0,
        mu,
        eps_v: 0.001,
    }
}

fn triangle(z: f64, size: f64) -> Vec<Vec3> {
    vec![
        Vec3::new(-size, -size, z),
        Vec3::new(size, -size, z),
        Vec3::new(0.0, size, z),
    ]
}

fn contact() -> Contact {
    Contact::new(
        Body::new(triangle(0.0, 1.0), vec![[0, 1, 2]], 0.1).unwrap(),
        parameters(0.0),
        vec![[0, 1, 2]],
        |_, _| 0.1,
    )
}

#[test]
fn moving_body_cannot_tunnel_between_clear_endpoints() {
    let c = contact();
    let x = triangle(0.01, 0.1);
    let end = triangle(0.02, 1.0);
    assert!(c.first_intersection(&x).is_none());
    let final_c = Contact::new(
        Body::new(end.clone(), vec![[0, 1, 2]], 0.1).unwrap(),
        parameters(0.0),
        vec![[0, 1, 2]],
        |_, _| 0.1,
    );
    assert!(final_c.first_intersection(&x).is_none());
    let bound = c.motion_bound(&x, &x, &end, 1000).unwrap();
    let first_touch = (0.01 - c.params.thickness) / 0.02;
    assert!(
        bound.fraction > 0.0 && bound.fraction < first_touch,
        "{bound:?}"
    );
    assert!(bound.pairs_tested >= 3);
    assert_eq!(bound, c.motion_bound(&x, &x, &end, 1000).unwrap());
}

#[test]
fn co_translation_keeps_relative_motion_clear() {
    let c = contact();
    let x = triangle(0.01, 0.1);
    let displacement = Vec3::new(2.0, -3.0, 5.0);
    let end: Vec<_> = c.body.vertices.iter().map(|p| *p + displacement).collect();
    let y: Vec<_> = x.iter().map(|p| *p + displacement).collect();
    let bound = c.motion_bound(&x, &y, &end, 1000).unwrap();
    assert_eq!(bound.fraction, 1.0);
    assert!(
        bound.pairs_tested > 0,
        "must test swept overlap, not only endpoint boxes"
    );
}

#[test]
fn candidate_and_admission_failures_never_return_a_clear_motion() {
    let c = contact();
    let x = triangle(0.01, 0.1);
    let end = triangle(0.02, 1.0);
    assert_eq!(
        c.motion_bound(&x, &x, &end, 1).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(
        c.motion_bound(&x, &x, &end[..2], 100).unwrap_err().code,
        RefusalCode::InvalidDocument
    );
    let touching = triangle(c.params.thickness, 0.1);
    assert_eq!(
        c.motion_bound(&touching, &touching, &end, 100)
            .unwrap_err()
            .code,
        RefusalCode::IntersectingStart
    );
    let mut invalid = end;
    invalid[0].z = f64::NAN;
    assert_eq!(
        c.motion_bound(&x, &x, &invalid, 100).unwrap_err().code,
        RefusalCode::NonFinite
    );
    let mut invalid_c = c.clone();
    invalid_c.body.edges[0][0] = 999;
    assert_eq!(
        invalid_c
            .motion_bound(&x, &x, &c.body.vertices, 100)
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn swept_motion_includes_self_contact_not_just_the_prescribed_body() {
    let mut x = triangle(-0.01, 0.1);
    x.extend(triangle(0.01, 0.1));
    let c = Contact::new(
        Body::empty(),
        parameters(0.0),
        vec![[0, 1, 2], [3, 4, 5]],
        |_, _| 0.1,
    )
    .with_self_contact(&x, parameters(0.0))
    .unwrap();
    let y: Vec<_> = x.iter().map(|p| Vec3::new(p.x, p.y, -p.z)).collect();
    let bound = c.motion_bound(&x, &y, &[], 1000).unwrap();
    assert!(bound.fraction > 0.0 && bound.fraction < 0.5);
    assert!(bound.pairs_tested >= 6);
}

fn sliding_patch(mu: f64, fixed_height: bool) -> Solver {
    let x = triangle(0.0007, 0.03);
    let rest: Vec<_> = x.iter().map(|p| ftx_core::Vec2::new(p.x, p.y)).collect();
    let model = ClothModel::from_triangles(
        3,
        &[[0, 1, 2]],
        &[[rest[0], rest[1], rest[2]]],
        Membrane::orthotropic(1000.0, 1000.0, 0.0, 100.0, 0.05),
        0.0,
        0.2,
    )
    .unwrap();
    let mut s = Solver::new(model, x, &[[false, false, fixed_height]; 3], Vec3::ZERO)
        .with_contact(
            Body::new(triangle(0.0, 1.0), vec![[0, 1, 2]], 0.1).unwrap(),
            parameters(mu),
        )
        .unwrap();
    s.tol = 1e-8;
    s.max_newton = 200;
    s
}

#[test]
fn first_sewing_contact_resists_tangential_slip() {
    let travel = |mu| {
        let mut patch = sliding_patch(mu, false);
        for p in &mut patch.x {
            p.z = 0.002; // initially outside the 0.5 mm offset plus the 1 mm barrier
        }
        patch.v.fill(Vec3::new(0.1, 0.0, -0.2));
        patch.friction_rounds = ftx_drape::run::DrapeOptions::default().lag_rounds[0];
        let x0 = patch.x[0].x;
        assert!(patch.step(0.01).unwrap().converged);
        assert!(patch.x.iter().all(|p| p.z > 0.0005 && p.z < 0.0015));
        patch.x[0].x - x0
    };
    let smooth = travel(0.0);
    assert!((smooth - 0.001).abs() < 1e-8);
    let rough = travel(0.4);
    assert!(
        rough > 0.0 && rough < smooth - 1e-5,
        "{rough} vs {smooth} m"
    );
}

#[test]
fn moving_surface_transports_cloth_by_friction_not_absolute_world_drag() {
    let mut slippery = sliding_patch(0.0, true);
    let mut grippy = sliding_patch(0.5, true);
    let x0 = grippy.x.clone();
    let target: Vec<_> = grippy
        .contact
        .as_ref()
        .unwrap()
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.002, 0.0, 0.0))
        .collect();
    advance_body(&mut slippery, &target, 0.2, MotionOptions::default()).unwrap();
    let r = advance_body(&mut grippy, &target, 0.2, MotionOptions::default()).unwrap();
    let dx = grippy
        .x
        .iter()
        .zip(&x0)
        .map(|(a, b)| a.x - b.x)
        .sum::<f64>()
        / 3.0;
    assert!(
        dx > 0.0019 && dx < 0.00201,
        "transport {dx} m, {} substeps",
        r.samples.len()
    );
    for (x, original) in slippery.x.iter().zip(x0) {
        assert!((*x - original).norm() < 1e-12);
    }
    for sample in &r.samples {
        assert!(sample.solve.converged);
    }
    assert_eq!(r.samples.last().unwrap().time_s, 0.2);
    assert_eq!(grippy.contact.as_ref().unwrap().body.vertices, target);

    // Matching body/cloth velocities must cause no spurious tangential friction.
    let mut together = sliding_patch(0.5, true);
    together.v.fill(Vec3::new(0.01, 0.0, 0.0));
    let initial = together.x.clone();
    advance_body(&mut together, &target, 0.2, MotionOptions::default()).unwrap();
    for (x, x0) in together.x.iter().zip(initial) {
        assert!((*x - x0 - Vec3::new(0.002, 0.0, 0.0)).norm() < 1e-10);
    }
}

#[test]
fn raising_collider_lifts_cloth_with_checked_continuous_substeps() {
    let mut s = sliding_patch(0.0, false);
    s.gravity = Vec3::new(0.0, 0.0, -9.81);
    let mut x0 = s.x.clone();
    let mut c0 = s.contact.as_ref().unwrap().clone();
    let target: Vec<_> = c0
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.0, 0.0, 0.004))
        .collect();
    let report = advance_body(&mut s, &target, 0.2, MotionOptions::default()).unwrap();
    assert!(report.samples.len() > 1);
    for frame in &report.samples {
        assert_eq!(
            c0.motion_bound(&x0, &frame.cloth, &frame.body, 1000)
                .unwrap()
                .fraction,
            1.0
        );
        c0.body = Body::new(frame.body.clone(), c0.body.triangles.clone(), 0.1).unwrap();
        assert!(c0.first_intersection(&frame.cloth).is_none());
        assert!(c0.min_gap(&frame.cloth, 0.01).unwrap() > 0.0);
        x0 = frame.cloth.clone();
    }
    assert!(s.x.iter().all(|p| p.z > 0.0045));
}

#[test]
fn carried_start_lifts_cloth_through_a_larger_checked_body_move() {
    checked_carried_lift(ftx_drape::TimeIntegrator::ImplicitEuler);
}

#[test]
fn generalized_alpha_carried_start_lifts_cloth_and_rolls_back_late_motion() {
    checked_carried_lift(ftx_drape::TimeIntegrator::GeneralizedAlpha { rho_inf: 0.5 });
}

fn checked_carried_lift(integrator: ftx_drape::TimeIntegrator) {
    let mut s = sliding_patch(0.0, false);
    s.gravity = Vec3::new(0.0, 0.0, -9.81);
    s.integrator = integrator;
    let options = MotionOptions {
        max_step_s: if integrator.is_default() {
            1. / 60.
        } else {
            0.001
        },
        ..MotionOptions::default()
    };
    let original = s.clone();
    let mut x0 = s.x.clone();
    let mut c0 = s.contact.as_ref().unwrap().clone();
    let target: Vec<_> = c0
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.0, 0.0, 0.004))
        .collect();
    let report = advance_body_carried_with_work(
        &mut s,
        &target,
        0.2,
        options,
        DissipationCapture::default(),
        0.02,
    )
    .unwrap();
    for frame in &report.samples {
        assert!(frame.solve.converged);
        assert_eq!(
            c0.motion_bound(&x0, &frame.cloth, &frame.body, 1000)
                .unwrap()
                .fraction,
            1.0
        );
        c0.body = Body::new(frame.body.clone(), c0.body.triangles.clone(), 0.1).unwrap();
        assert!(c0.first_intersection(&frame.cloth).is_none());
        // The entire collider lies in this horizontal plane. Positive normal clearance at
        // every cloth vertex bounds every affine cloth face, even beyond a proximity-query window.
        let body_height = frame.body[0].z;
        assert!(frame.body.iter().all(|p| p.z == body_height));
        assert!(
            frame
                .cloth
                .iter()
                .all(|p| p.z > body_height + c0.params.thickness)
        );
        x0 = frame.cloth.clone();
    }
    assert!(s.x.iter().all(|p| p.z > 0.0045));
    assert_eq!(s.contact.as_ref().unwrap().body.vertices, target);
    assert_eq!(report.samples.last().unwrap().time_s, 0.2);

    // Geometry-only capture and the public carried wrapper produce the same accepted solve.
    let mut legacy = original.clone();
    let old = advance_body_carried(&mut legacy, &target, 0.2, options, 0.02).unwrap();
    assert_eq!(format!("{old:?}"), format!("{report:?}"));
    assert_eq!(format!("{legacy:?}"), format!("{s:?}"));

    // A sample budget reached after an accepted substep publishes none of the carried motion.
    let mut limited = original.clone();
    assert_eq!(
        advance_body_carried_with_work(
            &mut limited,
            &target,
            0.2,
            MotionOptions {
                max_sample_vertices: 6,
                ..options
            },
            DissipationCapture::default(),
            0.02,
        )
        .unwrap_err()
        .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(format!("{limited:?}"), format!("{original:?}"));
}

#[test]
fn carried_start_never_prescribes_frictionless_tangential_cloth_motion() {
    let original = sliding_patch(0.0, true);
    let target: Vec<_> = original
        .contact
        .as_ref()
        .unwrap()
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.002, 0.0, 0.0))
        .collect();
    let mut s = original.clone();
    let report = advance_body_carried_with_work(
        &mut s,
        &target,
        0.2,
        MotionOptions::default(),
        DissipationCapture::default(),
        0.02,
    )
    .unwrap();
    for frame in &report.samples {
        assert!(frame.solve.converged);
        for (position, start) in frame.cloth.iter().zip(&original.x) {
            assert!((*position - *start).norm() < 1e-12);
        }
    }
    assert!(s.v.iter().all(|velocity| velocity.norm() < 1e-10));
    assert_eq!(s.contact.as_ref().unwrap().body.vertices, target);

    for radius in [0.0, -0.01, f64::NAN, f64::INFINITY] {
        let mut refused = original.clone();
        assert_eq!(
            advance_body_carried_with_work(
                &mut refused,
                &target,
                0.2,
                MotionOptions::default(),
                DissipationCapture::default(),
                radius,
            )
            .unwrap_err()
            .code,
            RefusalCode::OutOfRange
        );
        assert_eq!(format!("{refused:?}"), format!("{original:?}"));
    }
}

#[test]
fn late_motion_failures_roll_back_every_solver_and_collider_field() {
    let mut s = sliding_patch(0.5, true);
    let before = format!("{s:?}");
    let b = &s.contact.as_ref().unwrap().body.vertices;
    let target: Vec<_> = b.iter().map(|p| *p + Vec3::new(0.002, 0.0, 0.0)).collect();
    for options in [
        MotionOptions {
            max_trials: 1,
            ..MotionOptions::default()
        },
        MotionOptions {
            max_sample_vertices: 6,
            ..MotionOptions::default()
        },
    ] {
        assert_eq!(
            advance_body(&mut s, &target, 0.2, options)
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
        assert_eq!(format!("{s:?}"), before);
    }
    let rising: Vec<_> = s
        .contact
        .as_ref()
        .unwrap()
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.0, 0.0, 0.004))
        .collect();
    assert_eq!(
        advance_body(
            &mut s,
            &rising,
            0.2,
            MotionOptions {
                max_pairs: 1,
                ..MotionOptions::default()
            }
        )
        .unwrap_err()
        .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(format!("{s:?}"), before);
    let mut invalid = target.clone();
    invalid[0].z = f64::NAN;
    assert_eq!(
        advance_body(&mut s, &invalid, 0.2, MotionOptions::default())
            .unwrap_err()
            .code,
        RefusalCode::NonFinite
    );
    assert_eq!(format!("{s:?}"), before);

    // Once a valid motion has converted/refitted the body indices, late failure must preserve
    // those live indices too. A subsequent valid retry must still transport the same cloth.
    advance_body(&mut s, &target, 0.2, MotionOptions::default()).unwrap();
    let after_motion = format!("{s:?}");
    let next: Vec<_> = target
        .iter()
        .map(|p| *p + Vec3::new(0.002, 0.0, 0.0))
        .collect();
    assert_eq!(
        advance_body(
            &mut s,
            &next,
            0.2,
            MotionOptions {
                max_trials: 1,
                ..MotionOptions::default()
            },
        )
        .unwrap_err()
        .code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(format!("{s:?}"), after_motion);
    advance_body(&mut s, &next, 0.2, MotionOptions::default()).unwrap();
    assert_eq!(s.contact.as_ref().unwrap().body.vertices, next);
}

#[test]
fn body_face_collapse_inside_a_motion_is_refused_not_skipped() {
    let mut s = sliding_patch(0.0, true);
    let before = format!("{s:?}");
    let mut target = s.contact.as_ref().unwrap().body.vertices.clone();
    target.swap(1, 2); // Both endpoints are triangles; their interpolation collapses at t=1/2.
    let err = advance_body(
        &mut s,
        &target,
        0.1,
        MotionOptions {
            max_trials: 80,
            ..MotionOptions::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        err.code,
        RefusalCode::NotConverged | RefusalCode::BudgetExceeded
    ));
    assert_eq!(format!("{s:?}"), before);
}

// Two body patches far above the cloth: only body/body admission can reject these cases.
fn separated_body_patches() -> Solver {
    let mut s = sliding_patch(0.0, true);
    let mut vertices = triangle(2.0, 1.0);
    vertices.extend(triangle(3.0, 1.0));
    s.contact.as_mut().unwrap().body =
        Body::new(vertices, vec![[0, 1, 2], [3, 4, 5]], 0.1).unwrap();
    s
}

#[test]
fn malformed_body_topology_refuses_before_motion_and_keeps_the_full_solver() {
    for mode in 0..3 {
        let mut s = sliding_patch(0.0, true);
        let mut vertices = vec![
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.0, 0.0, 2.0),
            Vec3::new(0.0, 1.0, 2.0),
            Vec3::new(0.0, 0.0, 3.0),
        ];
        let mut faces = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        match mode {
            0 => faces[0].swap(1, 2),
            1 => {
                for face in &mut faces {
                    face.swap(1, 2);
                }
            }
            _ => {
                vertices.extend([
                    Vec3::new(3.0, 0.0, 2.0),
                    Vec3::new(4.0, 0.0, 2.0),
                    Vec3::new(3.0, 1.0, 2.0),
                ]);
                faces.push([4, 5, 6]);
            }
        }
        let target: Vec<_> = vertices
            .iter()
            .map(|p| *p + Vec3::new(0.01, 0.0, 0.0))
            .collect();
        s.contact.as_mut().unwrap().body = Body::new(vertices, faces, 0.1).unwrap();
        let before = format!("{s:?}");
        let error = advance_body(&mut s, &target, 0.01, MotionOptions::default()).unwrap_err();
        assert_eq!(
            error.code,
            if mode == 1 {
                RefusalCode::InwardWound
            } else {
                RefusalCode::DegenerateGeometry
            }
        );
        assert_eq!(format!("{s:?}"), before);
    }
}

#[test]
fn intersecting_body_start_and_target_refuse_without_changing_the_solver() {
    for invalid_start in [false, true] {
        let mut s = separated_body_patches();
        let mut target = s.contact.as_ref().unwrap().body.vertices.clone();
        for p in &mut target[3..] {
            p.z = 2.0;
        }
        if invalid_start {
            s.contact.as_mut().unwrap().body =
                Body::new(target.clone(), vec![[0, 1, 2], [3, 4, 5]], 0.1).unwrap();
        }
        let before = format!("{s:?}");
        let refusal = advance_body(&mut s, &target, 0.1, MotionOptions::default()).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::IntersectingStart);
        assert!(refusal.message.contains("body triangles 0 and 1"));
        assert_eq!(format!("{s:?}"), before);
    }
}

#[test]
fn crossing_body_keyframes_refuse_even_when_both_endpoints_are_valid() {
    let mut s = separated_body_patches();
    let mut target = s.contact.as_ref().unwrap().body.vertices.clone();
    for p in &mut target {
        p.z = 5.0 - p.z;
    }
    ftx_drape::collider::audit_surface(&target, &[[0, 1, 2], [3, 4, 5]], 1).unwrap();
    let before = format!("{s:?}");
    let refusal = advance_body(
        &mut s,
        &target,
        1.0,
        MotionOptions {
            max_step_s: 0.25,
            ..MotionOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(refusal.code, RefusalCode::IntersectingStart);
    assert!(refusal.message.contains("body triangles 0 and 1"));
    assert_eq!(format!("{s:?}"), before);
}

#[test]
fn body_crossing_between_all_nominal_samples_is_refused_transactionally() {
    let mut s = separated_body_patches();
    // Enter with an already-refitted collider, then retain continuous rejection through a
    // second motion whose intermediate body crossing is absent from every nominal sample.
    let warm: Vec<_> = s
        .contact
        .as_ref()
        .unwrap()
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.1, 0.0, 0.0))
        .collect();
    advance_body(&mut s, &warm, 0.04, MotionOptions::default()).unwrap();
    let initial = s.contact.as_ref().unwrap().body.vertices.clone();
    let mut target = initial.clone();
    // The moving patch crosses the static patch at t=0.37, not at any nominal endpoint.
    for p in &mut target[3..] {
        p.z = 3.0 - 1.0 / 0.37;
    }
    for t in [0.0, 0.3, 0.6, 0.9, 1.0] {
        let pose: Vec<_> = initial
            .iter()
            .zip(&target)
            .map(|(a, b)| *a + (*b - *a) * t)
            .collect();
        ftx_drape::collider::audit_surface(&pose, &[[0, 1, 2], [3, 4, 5]], 1).unwrap();
    }
    let before = format!("{s:?}");
    let error = advance_body(
        &mut s,
        &target,
        1.0,
        MotionOptions {
            max_step_s: 0.3,
            max_trials: 64,
            ..MotionOptions::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        error.code,
        RefusalCode::NotConverged | RefusalCode::BudgetExceeded | RefusalCode::IntersectingStart
    ));
    assert_eq!(format!("{s:?}"), before);
}

#[test]
fn valid_body_motion_retains_legal_shared_features_and_reports_audit_work() {
    let mut s = sliding_patch(0.0, true);
    let vertices = vec![
        Vec3::new(-1.0, -1.0, 2.0),
        Vec3::new(1.0, -1.0, 2.0),
        Vec3::new(1.0, 1.0, 2.0),
        Vec3::new(-1.0, 1.0, 2.0),
    ];
    let target: Vec<_> = vertices
        .iter()
        .map(|p| *p + Vec3::new(0.1, 0.0, 0.0))
        .collect();
    s.contact.as_mut().unwrap().body =
        Body::new(vertices, vec![[0, 1, 2], [0, 2, 3]], 0.1).unwrap();
    let report = advance_body(&mut s, &target, 0.01, MotionOptions::default()).unwrap();
    assert_eq!(report.samples.len(), 1);
    // Three exact face-pair audits and two nonincident vertex/triangle CCD pairs.
    assert_eq!(report.pairs_tested, 5);
    assert_eq!(s.contact.as_ref().unwrap().body.vertices, target);
}

#[test]
fn moving_body_adapts_barrier_stiffness_between_substeps_and_rolls_back_on_refusal() {
    use ftx_drape::AdaptiveBarrierPolicy;

    let mut legacy = sliding_patch(0.0, true);
    let mut adaptive = sliding_patch(0.0, true);
    let policy = AdaptiveBarrierPolicy {
        target_gap_fraction: 0.5,
        min_gap_fraction: 0.3,
        growth_factor: 2.0,
        membrane_floor_scale: 0.0,
        kappa_min: 1e-3,
        kappa_max: 1e5,
    };
    adaptive
        .contact
        .as_mut()
        .unwrap()
        .set_adaptive_barrier(policy, 1e-3, 1000.0, 9.81)
        .unwrap();
    let legacy_kappa0 = legacy.contact.as_ref().unwrap().params.kappa;
    let adaptive_kappa0 = adaptive.contact.as_ref().unwrap().params.kappa;
    let target: Vec<_> = legacy
        .contact
        .as_ref()
        .unwrap()
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.002, 0.0, 0.0))
        .collect();
    let opts = MotionOptions {
        max_step_s: 0.01,
        ..MotionOptions::default()
    };
    advance_body(&mut legacy, &target, 0.02, opts).unwrap();
    let rep = advance_body(&mut adaptive, &target, 0.02, opts).unwrap();
    assert!(rep.samples.len() >= 2);
    assert_eq!(
        legacy.contact.as_ref().unwrap().params.kappa,
        legacy_kappa0,
        "legacy moving-body steps must leave fixed kappa untouched"
    );
    let escalated = adaptive.contact.as_ref().unwrap().params.kappa;
    assert!(
        escalated > adaptive_kappa0,
        "adaptive barrier must escalate kappa across compressed moving-body substeps ({adaptive_kappa0} -> {escalated})"
    );

    // A refused moving-body step must roll back any trial barrier escalation transactionally.
    let mut bad_target = target.clone();
    bad_target[0].z = f64::NAN;
    assert!(advance_body(&mut adaptive, &bad_target, 0.02, opts).is_err());
    assert_eq!(adaptive.contact.as_ref().unwrap().params.kappa, escalated);

    // Incompatible constitutive pair (e.g. authored crease under Quadratic bending) must refuse
    // fail-closed before spending audit work.
    let (rest, tris) = ftx_drape::hex_grid(0.06, 0.06, 0.03, ftx_core::Vec2::ZERO);
    let mut creased = ClothModel::new(
        rest.clone(),
        &tris,
        Membrane::orthotropic(1000.0, 1000.0, 0.0, 200.0, 1.0),
        1e-4,
        0.2,
    )
    .unwrap();
    let mut angles = vec![0.0; creased.hinges.len()];
    angles[0] = 0.5;
    creased.set_hinge_rest_angles(&angles).unwrap();
    let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.01)).collect();
    let mut bad_solver = Solver::new(creased, x0, &vec![[false; 3]; rest.len()], Vec3::ZERO)
        .with_contact(
            Body::new(triangle(0.0, 1.0), vec![[0, 1, 2]], 0.1).unwrap(),
            parameters(0.0),
        )
        .unwrap();
    let err = advance_body(&mut bad_solver, &target, 0.01, opts).unwrap_err();
    assert_eq!(err.code, RefusalCode::InvalidDocument);
}

#[test]
fn carried_motion_subdivides_when_guess_exits_characterized_membrane_domain() {
    use ftx_drape::TimeIntegrator;
    use ftx_pattern::material_response::StrainDomain;

    // Right triangle above a flat frictionless body translating +6 mm in x.
    // Vertices 0 and 2 are fixed while vertex 1 is free and sits within carry_radius_m.
    // On the initial full substep (share = 1.0), the geometric carry stretches the 50 mm
    // edge by 6 mm (Green strain E11 = +0.1272 > 0.08), exiting the characterized domain,
    // while the real frictionless equilibrium stays unstretched (E11 ≈ 0). Adaptive motion
    // must halve the substep (share = 0.5, carry E11 = +0.0618 < 0.08) and succeed rather
    // than aborting the whole motion with OutOfRange.
    let rest = [
        ftx_core::Vec2::new(0.0, 0.0),
        ftx_core::Vec2::new(0.05, 0.0),
        ftx_core::Vec2::new(0.025, 0.05),
    ];
    let x0 = vec![
        Vec3::new(0.0, 0.0, 0.002),
        Vec3::new(0.05, 0.0, 0.002),
        Vec3::new(0.025, 0.05, 0.002),
    ];
    let domain = StrainDomain::new([[-0.08, 0.08]; 3]).unwrap();
    let make_solver = |integrator: TimeIntegrator, start_x: Vec<Vec3>| {
        let mut model = ClothModel::from_triangles(
            3,
            &[[0, 1, 2]],
            &[[rest[0], rest[1], rest[2]]],
            Membrane::orthotropic(1000.0, 1000.0, 0.0, 100.0, 1.0),
            0.0,
            0.2,
        )
        .unwrap();
        model.set_membrane_domains(&[Some(domain)]).unwrap();
        let mut s = Solver::new(
            model,
            start_x,
            &[[true; 3], [false; 3], [true; 3]],
            Vec3::ZERO,
        )
        .with_contact(
            Body::new(triangle(0.0, 1.0), vec![[0, 1, 2]], 0.1).unwrap(),
            parameters(0.0),
        )
        .unwrap();
        s.integrator = integrator;
        s.tol = 1e-8;
        s.max_newton = 200;
        s
    };

    let target: Vec<_> = triangle(0.0, 1.0)
        .into_iter()
        .map(|p| p + Vec3::new(0.006, 0.0, 0.0))
        .collect();
    let opts = MotionOptions {
        max_step_s: 0.02,
        ..MotionOptions::default()
    };
    for integrator in [
        TimeIntegrator::ImplicitEuler,
        TimeIntegrator::GeneralizedAlpha { rho_inf: 0.5 },
    ] {
        let mut solver = make_solver(integrator, x0.clone());
        let rep = advance_body_carried(&mut solver, &target, 0.02, opts, 0.01).unwrap();
        assert!(
            rep.rejected_unsolved >= 1,
            "{integrator:?}: expected initial out-of-domain carry guess to trigger substep halving, got {rep:?}"
        );
        assert!(
            rep.samples.len() >= 2,
            "{integrator:?}: expected subdivided motion, got {rep:?}"
        );
        solver.model.check_membrane_domains(&solver.x).unwrap();

        // Fail-closed: an accepted start state x0 that is ALREADY outside the domain must
        // refuse with OutOfRange rather than being masked as NotConverged.
        let mut bad_x0 = x0.clone();
        bad_x0[1].x = 0.056;
        let mut invalid = make_solver(integrator, bad_x0);
        let before = format!("{invalid:?}");
        let err = advance_body_carried(&mut invalid, &target, 0.02, opts, 0.01).unwrap_err();
        assert_eq!(err.code, RefusalCode::OutOfRange, "{integrator:?}: {err}");
        assert_eq!(format!("{invalid:?}"), before);
    }
}
