//! Regression coverage for heterogeneous textile sheets in the production cloth solver.
//! Synthetic coefficients only; this is numerical/assembly coverage, not material validation.

use ftx_core::{Vec2, Vec3};
use ftx_drape::cloth::SheetMaterial;
use ftx_drape::{ClothModel, Membrane, Solver};

fn sheet(scale: f64) -> SheetMaterial {
    SheetMaterial {
        membrane: Membrane::orthotropic(
            100.0 * scale,
            40.0 * scale,
            0.2,
            20.0 * scale,
            1.0,
        ),
        bending_nm: 0.01 * scale,
        areal_kg_m2: 0.2 * scale,
    }
}

fn mesh() -> ClothModel {
    let m = sheet(1.0);
    ClothModel::new(
        vec![
            Vec2::ZERO,
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(0.0, -2.0),
        ],
        &[[0, 1, 2], [1, 0, 3]],
        m.membrane,
        m.bending_nm,
        m.areal_kg_m2,
    )
    .unwrap()
}

fn positions() -> Vec<Vec3> {
    vec![
        Vec3::ZERO,
        Vec3::new(1.1, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.2),
        Vec3::new(0.0, -2.0, 0.1),
    ]
}

#[test]
fn heterogeneous_binding_updates_mass_membrane_and_bending_together() {
    let mut model = mesh();
    model.set_materials(&[sheet(1.0), sheet(3.0)]).unwrap();

    // Triangle areas are 0.5 and 1.0 m^2. Mass assembly must use each triangle's
    // own areal density rather than the first/global material.
    assert!((model.mass.iter().sum::<f64>() - 0.7).abs() < 1e-15);
    assert_eq!(model.material(0), sheet(1.0).membrane);
    assert_eq!(model.material(1), sheet(3.0).membrane);

    // The shared hinge receives the adjacent triangles' area-weighted bending law.
    let expected_b = (0.01 * 0.5 + 0.03 * 1.0) / 1.5;
    assert!((model.hinge_rigidity(0) - expected_b).abs() < 1e-16);

    let x = positions();
    let expected = model.membranes[0].energy(&x, &sheet(1.0).membrane)
        + model.membranes[1].energy(&x, &sheet(3.0).membrane)
        + model.hinges[0].energy(&x, expected_b);
    assert!((model.elastic_energy(&x) - expected).abs() < 1e-12);
}

#[test]
fn homogeneous_rebinding_preserves_solver_results_bit_for_bit() {
    let original = mesh();
    let mut rebound = original.clone();
    rebound.set_materials(&[sheet(1.0), sheet(1.0)]).unwrap();

    assert_eq!(original.mass, rebound.mass);
    let x = positions();
    assert_eq!(original.elastic_energy(&x), rebound.elastic_energy(&x));

    let fixed = [[true; 3], [true; 3], [false; 3], [false; 3]];
    let mut a = Solver::new(original, x.clone(), &fixed, Vec3::new(0.0, 0.0, -9.81));
    let mut b = Solver::new(rebound, x, &fixed, Vec3::new(0.0, 0.0, -9.81));

    for _ in 0..3 {
        assert_eq!(a.step(0.01).unwrap(), b.step(0.01).unwrap());
        assert_eq!(a.x, b.x);
        assert_eq!(a.v, b.v);
    }
}

#[test]
fn heterogeneous_newton_force_matches_energy_gradient() {
    let mut model = mesh();
    model.set_materials(&[sheet(1.0), sheet(4.0)]).unwrap();
    let x = positions();
    let solver = Solver::new(
        model,
        x.clone(),
        &[[true; 3], [true; 3], [false; 3], [false; 3]],
        Vec3::ZERO,
    );
    let force = solver.elastic_forces();
    let h = 1e-6;

    for vertex in 0..4 {
        for axis in 0..3 {
            let d = match axis {
                0 => Vec3::new(h, 0.0, 0.0),
                1 => Vec3::new(0.0, h, 0.0),
                _ => Vec3::new(0.0, 0.0, h),
            };
            let mut plus = x.clone();
            let mut minus = x.clone();
            plus[vertex] += d;
            minus[vertex] -= d;
            let derivative = (solver.model.elastic_energy(&plus)
                - solver.model.elastic_energy(&minus))
                / (2.0 * h);
            let f = [force[vertex].x, force[vertex].y, force[vertex].z][axis];
            assert!(
                (derivative + f).abs() < 1e-6,
                "vertex {vertex} axis {axis}: dE={derivative}, force={f}"
            );
        }
    }
}
