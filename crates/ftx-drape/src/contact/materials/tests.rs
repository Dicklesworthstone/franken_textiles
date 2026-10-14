//! Synthetic contact known answers; not measurements of textile friction or seam geometry.
use super::*;

fn params() -> ContactParams {
    ContactParams {
        dhat: 0.002,
        thickness: 0.0,
        kappa: 100.0,
        mu: 0.0,
        eps_v: 0.001,
    }
}
fn floor() -> Body {
    Body::new(
        vec![
            Vec3::new(-1., -1., 0.),
            Vec3::new(1., -1., 0.),
            Vec3::new(0., 1., 0.),
        ],
        vec![[0, 1, 2]],
        0.1,
    )
    .unwrap()
}
fn triangle(x: f64, z: f64) -> [Vec3; 3] {
    [
        Vec3::new(x - 0.01, -0.01, z),
        Vec3::new(x + 0.01, -0.01, z),
        Vec3::new(x, 0.01, z),
    ]
}
fn contact(body: Body) -> Contact {
    Contact::new(body, params(), vec![[0, 1, 2], [3, 4, 5]], |_, _| 0.02)
}
fn surfaces() -> [SurfaceMaterial; 2] {
    [
        SurfaceMaterial {
            half_thickness_m: 0.001,
            friction_body: 0.1,
        },
        SurfaceMaterial {
            half_thickness_m: 0.003,
            friction_body: 0.7,
        },
    ]
}
#[test]
fn body_contact_uses_local_thickness_inside_global_broad_phase_envelope() {
    let x = [triangle(-0.03, 0.0015), triangle(0.03, 0.0035)].concat();
    let c = contact(floor())
        .with_surface_materials(&x, &[0, 1], &surfaces(), None)
        .unwrap();
    assert_eq!(c.params.thickness, 0.003);
    assert_eq!(c.params.mu, 0.7);
    assert!((c.min_gap(&x, 0.002).unwrap() - 0.0005).abs() < 1e-15);
    for (cv, t, mu) in [(0, 0.001, 0.1), (3, 0.003, 0.7)] {
        let p = c.pair_params(Pair::VertexTri { cv, bt: 0 });
        assert_eq!((p.thickness, p.mu), (t, mu));
    }
    let terms = c.terms(&x, true, false);
    assert!(terms.energy.is_finite() && terms.energy > 0.0);
    assert!(
        terms
            .blocks
            .iter()
            .any(|b| b.dofs.iter().any(|(v, _)| *v >= 3))
    );
    // Body moving upward would hit the thin sheet first, despite the thick sheet's larger offset.
    let end: Vec<_> = c
        .body
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., 0.003))
        .collect();
    let bound = c.clone().motion_bound(&x, &x, &end, 1000).unwrap();
    assert!(bound.fraction > 0.0 && bound.fraction < 1.0 / 6.0);
}
fn layers(mu: f64) -> (Contact, Vec<Vec3>) {
    let x = [triangle(0., 0.), triangle(0., 0.005)].concat();
    let c = contact(Body::empty())
        .with_surface_materials(
            &x,
            &[0, 1],
            &surfaces(),
            Some(&[vec![0., mu], vec![mu, 0.]]),
        )
        .unwrap()
        .with_self_contact(&x, params())
        .unwrap();
    (c, x)
}
#[test]
fn unlike_layers_sum_half_thicknesses_and_use_the_explicit_interface() {
    let (c, x) = layers(0.9);
    let p = c.pair_params(Pair::SelfVertexTri { cv: 0, ct: 1 });
    assert_eq!((p.thickness, p.mu), (0.004, 0.9));
    assert_eq!(c.self_params.unwrap().thickness, 0.006); // broad phase only
    assert!((c.min_self_gap(&x, 0.002).unwrap() - 0.001).abs() < 1e-15);
    c.audit_self_contact(&x).unwrap();
    let dp: Vec<_> = (0..6)
        .map(|v| Vec3::new(0., 0., if v < 3 { 0.01 } else { -0.01 }))
        .collect();
    let alpha = c.step_bound(&x, &dp);
    assert!(alpha > 0.0 && alpha < 0.05);
    let end: Vec<_> = x.iter().zip(&dp).map(|(p, d)| *p + *d * alpha).collect();
    c.audit_self_contact(&end).unwrap();
}
#[test]
fn mixed_barrier_gradient_matches_energy_and_balances_force() {
    let (c, x) = layers(0.0);
    let terms = c.terms(&x, true, false);
    let mut g = [0.0; 18];
    for b in &terms.blocks {
        for (i, &(v, k)) in b.dofs.iter().enumerate() {
            g[v * 3 + k] += b.grad[i];
        }
    }
    let h = 1e-7;
    for v in 0..6 {
        let mut a = x.clone();
        let mut b = x.clone();
        a[v].z += h;
        b[v].z -= h;
        let fd = (c.terms(&a, false, false).energy - c.terms(&b, false, false).energy) / (2.0 * h);
        assert!(
            (fd - g[v * 3 + 2]).abs() < 1e-5,
            "{v}: {fd} != {}",
            g[v * 3 + 2]
        );
    }
    for k in 0..3 {
        assert!((0..6).map(|v| g[v * 3 + k]).sum::<f64>().abs() < 1e-10);
    }
}
#[test]
fn friction_lag_uses_cross_coefficient_even_when_diagonal_coefficients_are_zero() {
    let (mut a, x) = layers(0.3);
    let (mut b, _) = layers(0.9);
    assert!(a.has_friction());
    a.update_lag(&x, &x, 0.1);
    b.update_lag(&x, &x, 0.1);
    let mut trial = x;
    for p in &mut trial[3..] {
        p.x += 0.0002;
    }
    let ea = a.terms(&trial, false, true).energy - a.terms(&trial, false, false).energy;
    let eb = b.terms(&trial, false, true).energy - b.terms(&trial, false, false).energy;
    assert!(ea > 0.0);
    assert!((eb / ea - 3.0).abs() < 1e-10);
}
#[test]
fn missing_pairs_bad_dimensions_and_stale_topology_refuse() {
    let x = [triangle(-0.03, 0.01), triangle(0.03, 0.01)].concat();
    let c = contact(floor())
        .with_surface_materials(&x, &[0, 1], &surfaces(), None)
        .unwrap();
    assert!(c.with_self_contact(&x, params()).is_err());
    for matrix in [
        vec![vec![0.]],
        vec![vec![0., 0.3], vec![0.4, 0.]],
        vec![vec![0., f64::NAN], vec![f64::NAN, 0.]],
    ] {
        assert!(
            contact(floor())
                .with_surface_materials(&x, &[0, 1], &surfaces(), Some(&matrix))
                .is_err()
        );
    }
    assert!(
        contact(floor())
            .with_surface_materials(&x, &[0, 2], &surfaces(), None)
            .is_err()
    );
    let mut c = contact(floor())
        .with_surface_materials(&x, &[0, 1], &surfaces(), None)
        .unwrap();
    c.cloth.triangles[0] = [0, 2, 1];
    assert!(
        c.audit_self_contact(&x)
            .unwrap_err()
            .message
            .contains("topology changed")
    );
}
#[test]
fn homogeneous_contact_retains_the_original_coefficients_and_energy() {
    let x = [triangle(-0.03, 0.0015), triangle(0.03, 0.0015)].concat();
    let s = [surfaces()[0]];
    let old = Contact::new(
        floor(),
        ContactParams {
            thickness: 0.001,
            mu: 0.1,
            ..params()
        },
        vec![[0, 1, 2], [3, 4, 5]],
        |_, _| 0.02,
    );
    let new = old
        .clone()
        .with_surface_materials(&x, &[0, 0], &s, Some(&[vec![0.1]]))
        .unwrap();
    assert_eq!(old.min_gap(&x, 0.002), new.min_gap(&x, 0.002));
    assert_eq!(
        old.terms(&x, false, false).energy,
        new.terms(&x, false, false).energy
    );
    let a = old.terms(&x, true, false);
    let b = new.terms(&x, true, false);
    assert_eq!(a.blocks.len(), b.blocks.len());
    for (a, b) in a.blocks.iter().zip(&b.blocks) {
        assert_eq!(a.dofs, b.dofs);
        assert_eq!(a.grad, b.grad);
        assert_eq!(a.hess, b.hess);
    }
}

fn solver_with_materials(c: Contact, x: Vec<Vec3>) -> crate::Solver {
    use crate::{ClothModel, Membrane, Solver};
    use ftx_core::Vec2;
    let rest: Vec<_> = c
        .cloth
        .triangles
        .iter()
        .map(|t| t.map(|i| Vec2::new(x[i].x, x[i].y)))
        .collect();
    let model = ClothModel::from_triangles(
        x.len(),
        &c.cloth.triangles,
        &rest,
        Membrane::orthotropic(100., 50., 0.2, 20., 1.),
        1e-4,
        0.2,
    )
    .unwrap();
    let fixed = vec![[false; 3]; x.len()];
    let mut solver = Solver::new(model, x, &fixed, Vec3::ZERO);
    solver.contact = Some(c);
    solver
}

#[test]
fn collider_replacement_preserves_unlike_sheet_interfaces_and_dynamics() {
    let (c, x) = layers(0.9);
    let before = c.pair_params(Pair::SelfVertexTri { cv: 0, ct: 1 });
    // The hand-built contact fixture used constant edge lengths. Reconstruct those from the
    // membrane rest geometry before comparing solver arithmetic across identical rebuilds.
    let original = solver_with_materials(c.clone(), x.clone())
        .with_contact(Body::empty(), params())
        .unwrap();
    let replaced = original
        .clone()
        .with_contact(Body::empty(), params())
        .unwrap();
    let contact = replaced.contact.as_ref().unwrap();
    assert!(contact.has_surface_materials());
    assert_eq!(
        before,
        contact.pair_params(Pair::SelfVertexTri { cv: 0, ct: 1 })
    );
    assert_eq!(c.min_self_gap(&x, 0.002), contact.min_self_gap(&x, 0.002));
    let (mut a, mut b) = (original, replaced);
    a.step(0.001).unwrap();
    b.step(0.001).unwrap();
    assert_eq!(a.x, b.x);
    assert_eq!(a.v, b.v);
}

#[test]
fn replacement_body_audits_thin_panels_at_their_own_offset() {
    let x = [triangle(-0.03, 0.0015), triangle(0.03, 0.0035)].concat();
    let c = contact(Body::empty())
        .with_surface_materials(&x, &[0, 1], &surfaces(), None)
        .unwrap();
    let mut numerical = params();
    numerical.dhat = 0.0008;
    numerical.mu = 99.; // Material friction remains the authority.
    let solver = solver_with_materials(c, x.clone())
        .with_contact(floor(), numerical)
        .unwrap();
    let c = solver.contact.as_ref().unwrap();
    assert!(c.has_surface_materials());
    assert!((c.min_gap(&x, 0.002).unwrap() - 0.0005).abs() < 1e-15);
    assert_eq!(c.pair_params(Pair::VertexTri { cv: 0, bt: 0 }).mu, 0.1);
    assert_eq!(c.params.dhat, numerical.dhat);
    let mut overlapping = floor();
    for p in &mut overlapping.vertices {
        p.z += 0.0006;
    }
    assert!(solver.with_contact(overlapping, numerical).is_err());
}

#[test]
fn collider_replacement_and_swept_motion_refuse_stale_material_topology() {
    let (c, x) = layers(0.9);
    let mut solver = solver_with_materials(c.clone(), x.clone());
    solver.model.membranes.swap(0, 1);
    assert!(
        solver
            .with_contact(Body::empty(), params())
            .unwrap_err()
            .message
            .contains("topology changed")
    );
    let mut c = c;
    c.cloth.triangles.swap(0, 1); // Same edge set but different source-material ownership.
    assert!(
        c.motion_bound(&x, &x, &[], 1000)
            .unwrap_err()
            .message
            .contains("topology changed")
    );
}
