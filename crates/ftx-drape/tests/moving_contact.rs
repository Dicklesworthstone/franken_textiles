//! Moving-collider regressions use synthetic surfaces, never production body assets.
use ftx_core::{RefusalCode, Vec3};
use ftx_drape::motion::{MotionOptions, advance_body};
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
