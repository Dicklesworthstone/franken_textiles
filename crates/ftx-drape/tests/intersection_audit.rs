//! Geometric admission and exact contact regressions, independent of body-model assets and cloth laws.
use ftx_core::{RefusalCode, Vec2, Vec3};
use ftx_drape::contact::{Body, Contact, ContactParams, Intersection};
use ftx_drape::{ClothModel, Membrane, Solver};

fn params() -> ContactParams {
    ContactParams {
        dhat: 0.001,
        thickness: 0.0005,
        kappa: 100.0,
        mu: 0.0,
        eps_v: 0.001,
    }
}

fn patch() -> Vec<Vec3> {
    vec![
        Vec3::ZERO,
        Vec3::new(0.04, 0.0, 0.0),
        Vec3::new(0.0, 0.04, 0.0),
    ]
}

#[test]
fn duplicate_faces_are_not_legal_shared_features() {
    let x = patch();
    for t in [[0, 1, 2], [2, 1, 0]] {
        let c = Contact::new(Body::empty(), params(), vec![[0, 1, 2], t], |_, _| 0.04);
        let err = c.with_self_contact(&x, params()).unwrap_err();
        assert_eq!(err.code, RefusalCode::IntersectingStart);
        assert!(err.message.contains("triangles 0 and 1"), "{err}");
    }
}

#[test]
fn coplanar_body_overlap_is_decided_in_both_containment_directions() {
    let big = patch();
    let small: Vec<_> = big
        .iter()
        .map(|p| *p * 0.25 + Vec3::new(0.005, 0.005, 0.0))
        .collect();
    for (body, cloth, expected) in [
        (
            big.clone(),
            small.clone(),
            Intersection::ClothEdge { ce: 0, bt: 0 },
        ),
        (small, big, Intersection::BodyEdge { ct: 0, be: 0 }),
    ] {
        let c = Contact::new(
            Body::new(body, vec![[0, 1, 2]], 0.001).unwrap(),
            params(),
            vec![[0, 1, 2]],
            |_, _| 0.04,
        );
        assert_eq!(c.crossings(&cloth, 1), vec![expected]);
        assert_eq!(c.first_intersection(&cloth), Some(expected));
        assert!(c.crossings(&cloth, 0).is_empty());
        let lifted: Vec<_> = cloth
            .iter()
            .map(|p| *p + Vec3::new(0.0, 0.0, 1e-12))
            .collect();
        assert!(
            c.crossings(&lifted, usize::MAX).is_empty(),
            "positive separation must not be snapped to contact"
        );
    }
}

#[test]
fn shared_edge_hinge_passes_but_coplanar_backfold_is_refused() {
    let mut x = patch();
    x.push(Vec3::new(0.01, -0.01, 0.0));
    let c = Contact::new(
        Body::empty(),
        params(),
        vec![[0, 1, 2], [1, 0, 3]],
        |_, _| 0.04,
    );
    let c = c.with_self_contact(&x, params()).unwrap();
    c.audit_self_contact(&x).unwrap();
    x[3] = Vec3::new(0.01, 0.01, 0.01);
    c.audit_self_contact(&x).unwrap();
    x[3].z = 0.0;
    assert_eq!(
        c.audit_self_contact(&x).unwrap_err().code,
        RefusalCode::IntersectingStart
    );
}

#[test]
fn body_admission_refuses_degenerate_faces_before_grids_or_distance_queries() {
    for face in [[0, 1, 1], [2, 0, 2]] {
        assert_eq!(
            Body::new(patch(), vec![face], 0.001).unwrap_err().code,
            RefusalCode::DegenerateGeometry
        );
    }
    let line = vec![
        Vec3::ZERO,
        Vec3::new(0.01, 0.01, 0.01),
        Vec3::new(0.02, 0.02, 0.02),
    ];
    assert_eq!(
        Body::new(line, vec![[0, 1, 2]], 0.001).unwrap_err().code,
        RefusalCode::DegenerateGeometry
    );
}

#[test]
fn solver_refuses_a_duplicate_face_added_after_contact_configuration() {
    let x = patch();
    let rest: Vec<_> = x.iter().map(|p| Vec2::new(p.x, p.y)).collect();
    let model = ClothModel::new(
        rest,
        &[[0, 1, 2]],
        Membrane::orthotropic(1000.0, 1000.0, 0.0, 100.0, 0.05),
        1e-5,
        0.2,
    )
    .unwrap();
    let mut solver = Solver::new(model, x, &[[true; 3]; 3], Vec3::ZERO)
        .with_self_contact(params())
        .unwrap();
    solver
        .contact
        .as_mut()
        .unwrap()
        .cloth
        .triangles
        .push([2, 1, 0]);
    let (x, v) = (solver.x.clone(), solver.v.clone());
    assert_eq!(
        solver.step(0.01).unwrap_err().code,
        RefusalCode::IntersectingStart
    );
    assert_eq!(solver.x, x);
    assert_eq!(solver.v, v);
    assert_eq!(
        solver.static_solve().unwrap_err().code,
        RefusalCode::IntersectingStart
    );
    assert_eq!(solver.x, x);
    assert_eq!(solver.v, v);
}
