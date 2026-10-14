//! Independent nodal differentiation checks for the actual assembled incremental potential.
use super::*;
use ftx_core::Vec2;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::materials::{Catalog, Material};

fn solver(compression: f64, nonlinear: bool) -> Solver {
    let mut selection: Selection = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/materials/synthetic-relaxation.json"
    ))
    .unwrap();
    if nonlinear {
        let mut record: Material = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-nonlinear.json"
        ))
        .unwrap();
        record.membrane_relaxation = selection
            .record
            .as_ref()
            .unwrap()
            .membrane_relaxation
            .clone();
        selection.id.clone_from(&record.id);
        selection.record = Some(record);
    }
    let snapshot = Catalog::builtin().unwrap().resolve(&selection).unwrap();
    let m = snapshot.mechanics().unwrap();
    let membrane =
        Membrane::orthotropic(m.warp_n_m, m.weft_n_m, m.poisson, m.shear_n_m, compression);
    let mut model = ClothModel::new(
        vec![Vec2::ZERO, Vec2::new(0.1, 0.), Vec2::new(0., 0.1)],
        &[[0, 1, 2]],
        membrane,
        0.,
        m.areal_kg_m2,
    )
    .unwrap();
    model
        .set_membrane_responses(&[snapshot.membrane_response().unwrap()])
        .unwrap();
    let x = vec![
        Vec3::ZERO,
        Vec3::new(0.102, 0., 0.),
        Vec3::new(0., 0.101, 0.),
    ];
    Solver::new(model, x, &[[false; 3]; 3], Vec3::ZERO)
        .with_relaxation(&[snapshot.membrane_relaxation().unwrap()])
        .unwrap()
}
fn perturb(x: &[Vec3], index: usize, delta: f64) -> Vec<Vec3> {
    let mut p = x.to_vec();
    match index % 3 {
        0 => p[index / 3].x += delta,
        1 => p[index / 3].y += delta,
        _ => p[index / 3].z += delta,
    }
    p
}
fn near(a: f64, b: f64, tol: f64) {
    assert!(
        (a - b).abs() <= tol * (1. + a.abs() + b.abs()),
        "{a} != {b}"
    );
}

#[test]
fn assembled_maxwell_gradient_and_hessian_include_grain_geometry_and_compression_chain_rule() {
    for (compression, nonlinear) in [(1., false), (0.03, false), (1., true)] {
        let mut s = solver(compression, nonlinear);
        s.relaxation.as_mut().unwrap().advance(&s.x, 0.017).unwrap();
        let x = vec![
            Vec3::new(0.001, 0.001, -0.003),
            Vec3::new(0.1008, 0.002, -0.002),
            Vec3::new(0.002, 0.1009, -0.001),
        ];
        let inertia = Some((&s.x[..], 0.02));
        let (gradient, hessian) = s.assemble(&x, inertia, false);
        let h = 1e-8;
        for (j, expected) in gradient.iter().enumerate() {
            let plus = perturb(&x, j, h);
            let minus = perturb(&x, j, -h);
            near(
                (s.potential(&plus, inertia).unwrap() - s.potential(&minus, inertia).unwrap())
                    / (2. * h),
                *expected,
                1e-6,
            );
            let (gp, _) = s.assemble(&plus, inertia, false);
            let (gm, _) = s.assemble(&minus, inertia, false);
            for i in 0..9 {
                let (indices, values) = hessian.row(i);
                let hij = indices
                    .iter()
                    .position(|k| *k == j)
                    .map_or(0., |k| values[k]);
                near((gp[i] - gm[i]) / (2. * h), hij, 1e-5);
            }
        }
    }
}

#[test]
fn reported_elastic_reactions_use_nonlinear_equilibrium_plus_accepted_branch_stress() {
    for history in [false, true] {
        let mut s = solver(1., true);
        if history {
            s.relaxation.as_mut().unwrap().advance(&s.x, 0.017).unwrap();
        } else {
            s.relaxation = None;
        }
        s.x[1].x = 0.12;
        let f = s.elastic_forces();
        for j in 0..9 {
            let p = perturb(&s.x, j, 1e-8);
            let n = perturb(&s.x, j, -1e-8);
            let derivative =
                (s.potential(&p, None).unwrap() - s.potential(&n, None).unwrap()) / 2e-8;
            near(-derivative, f[j / 3][j % 3], 1e-6);
        }
    }
}
