//! Independent synthetic parallel-network answers through physical triangle kinematics.
//! The fixture is explicitly an estimate for software checks, never a measured textile.
use ftx_core::{RefusalCode, Vec2, Vec3};
use ftx_drape::{ClothModel, Membrane, Solver};
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, Selection},
    materials::{CATALOG_SCHEMA, Catalog},
};

const LENGTH: f64 = 0.1;
const AREA: f64 = LENGTH * LENGTH / 2.0;

fn selection() -> Selection {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-parallel-membrane.json"
    ))
    .unwrap()
}

fn resolve(selected: &Selection) -> ResolvedMaterial {
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
    .resolve(selected)
    .unwrap()
}

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance * (1.0 + actual.abs() + expected.abs()),
        "{actual} differs from {expected}"
    );
}

fn bare(fixed: bool, strain: f64) -> Solver {
    let model = ClothModel::new(
        vec![Vec2::ZERO, Vec2::new(LENGTH, 0.0), Vec2::new(0.0, LENGTH)],
        &[[0, 1, 2]],
        Membrane::orthotropic(1000.0, 600.0, 0.0, 300.0, 1.0),
        0.0,
        0.2,
    )
    .unwrap();
    let mut solver = Solver::new(
        model,
        vec![
            Vec3::ZERO,
            Vec3::new(LENGTH * (1.0 + 2.0 * strain).sqrt(), 0.0, 0.0),
            Vec3::new(0.0, LENGTH, 0.0),
        ],
        &[[true; 3], [fixed, true, true], [true; 3]],
        Vec3::ZERO,
    );
    solver.tol = 1e-8;
    solver
}

fn selected(fixed: bool, strain: f64) -> Solver {
    let material = resolve(&selection());
    bare(fixed, strain)
        .with_membrane_histories(
            &[material.membrane_relaxation().unwrap()],
            &[material.membrane_plasticity().unwrap()],
        )
        .unwrap()
}

#[test]
fn actual_triangle_reactions_storage_and_two_ledgers_follow_independent_parallel_answers() {
    let mut solver = selected(true, 0.0);
    let (mut permanent, mut accumulated, mut viscous): (f64, f64, f64) = (0.0, 0.0, 0.0);
    let (mut plastic_work, mut viscous_work) = (0.0, 0.0);
    let (h, tau) = (0.01, 0.05);
    for strain in [0.1_f64, 0.1, 0.1, 80.0 / 1100.0, 80.0 / 1100.0, 0.01, 0.01] {
        solver.x[1].x = LENGTH * (1.0 + 2.0 * strain).sqrt();
        let trial = 1000.0 * (strain - permanent);
        let delta = trial.signum() * (trial.abs() - 20.0 - 100.0 * accumulated).max(0.0) / 1100.0;
        permanent += delta;
        accumulated += delta.abs();
        plastic_work += AREA * 20.0 * delta.abs();
        let a = tau / (tau + h);
        let gap = strain - viscous;
        viscous_work += AREA * a * (1.0 - a) * 2000.0 * gap * gap;
        viscous = a * viscous + (1.0 - a) * strain;
        assert!(solver.step(h).unwrap().converged);
        let p = solver.membrane_plasticity_report().unwrap().unwrap();
        let r = solver.relaxation_report().unwrap().unwrap();
        near(
            p["elements"][0]["plastic_green_strain"][0]
                .as_f64()
                .unwrap(),
            permanent,
            1e-12,
        );
        near(
            p["elements"][0]["accumulated_plastic_green_strain"][0]
                .as_f64()
                .unwrap(),
            accumulated,
            1e-12,
        );
        near(
            r["elements"][0]["viscous_green_strain"][0][0]
                .as_f64()
                .unwrap(),
            viscous,
            1e-12,
        );
        near(
            p["plastic_dissipation_j"].as_f64().unwrap(),
            plastic_work,
            1e-12,
        );
        near(
            r["viscous_step_work_j"].as_f64().unwrap(),
            viscous_work,
            1e-12,
        );
        let stress = 1000.0 * (strain - permanent) + 2000.0 * (strain - viscous);
        let reaction = AREA / LENGTH * (1.0 + 2.0 * strain).sqrt() * stress;
        let forces = solver.checked_elastic_forces().unwrap();
        near(forces[1].x, -reaction, 1e-11);
        near(forces[0].x, reaction, 1e-11);
        near(
            solver.membrane_energy().unwrap(),
            AREA * (500.0 * (strain - permanent).powi(2)
                + 50.0 * accumulated.powi(2)
                + 1000.0 * (strain - viscous).powi(2)),
            1e-12,
        );
        assert_eq!(
            p["elements"][0]["parallel_network"]["material_sha256"],
            r["elements"][0]["parallel_network"]["material_sha256"]
        );
        assert_eq!(
            p["elements"][0]["parallel_network"]["stress_basis"],
            "elastoplastic_equilibrium_branch_only"
        );
        assert_eq!(
            r["elements"][0]["parallel_network"]["strain_basis"],
            "total_physical_green_strain"
        );
    }
    let p = solver.membrane_plasticity_report().unwrap();
    let r = solver.relaxation_report().unwrap();
    assert!(solver.static_solve().is_err());
    assert_eq!(p, solver.membrane_plasticity_report().unwrap());
    assert_eq!(r, solver.relaxation_report().unwrap());
}

#[test]
fn atomic_binding_refuses_half_networks_different_sources_and_softened_coordinates() {
    let material = resolve(&selection());
    let r = material.membrane_relaxation().unwrap();
    let p = material.membrane_plasticity().unwrap();
    assert!(
        bare(true, 0.0)
            .with_relaxation(std::slice::from_ref(&r))
            .is_err()
    );
    assert!(
        bare(true, 0.0)
            .with_membrane_plasticity(std::slice::from_ref(&p))
            .is_err()
    );
    assert!(
        bare(true, 0.0)
            .with_membrane_histories(std::slice::from_ref(&r), &[None])
            .is_err()
    );
    assert!(
        bare(true, 0.0)
            .with_membrane_histories(&[None], std::slice::from_ref(&p))
            .is_err()
    );
    let mut changed = selection();
    changed
        .record
        .as_mut()
        .unwrap()
        .sources
        .get_mut("test")
        .unwrap()
        .note
        .push_str(" Revised source.");
    let other = resolve(&changed).membrane_relaxation().unwrap();
    assert!(
        bare(true, 0.0)
            .with_membrane_histories(&[other], std::slice::from_ref(&p))
            .is_err()
    );
    let mut softened = bare(true, 0.0);
    softened.model.compression[0] = [0.1, 1.0];
    assert!(
        softened
            .with_membrane_histories(std::slice::from_ref(&r), std::slice::from_ref(&p))
            .is_err()
    );
    assert!(
        selected(true, 0.0)
            .with_membrane_histories(&[r], &[p])
            .is_err()
    );
}

#[test]
fn failed_physical_step_rolls_back_both_memories_and_retries_identically() {
    let mut solver = selected(false, 0.1);
    // Balance the first condensed endpoint exactly, then release it for a genuine free solve.
    let stress = 1000.0 * (0.1 - 80.0 / 1100.0) + 2000.0 * 0.1 * 0.05 / 0.06;
    let force = AREA / LENGTH * 1.2_f64.sqrt() * stress;
    solver.gravity.x = force / solver.model.mass[1];
    assert!(solver.step(0.01).unwrap().converged);
    solver.gravity = Vec3::ZERO;
    let mut clean = solver.clone();
    let x = solver.x.clone();
    let v = solver.v.clone();
    let p = solver.membrane_plasticity_report().unwrap();
    let r = solver.relaxation_report().unwrap();
    solver.max_newton = 0;
    assert_eq!(
        solver.step(0.001).unwrap_err().code,
        RefusalCode::NotConverged
    );
    assert_eq!(solver.x, x);
    assert_eq!(solver.v, v);
    assert_eq!(solver.membrane_plasticity_report().unwrap(), p);
    assert_eq!(solver.relaxation_report().unwrap(), r);
    solver.max_newton = clean.max_newton;
    assert!(solver.step(0.001).unwrap().converged);
    assert!(clean.step(0.001).unwrap().converged);
    assert_eq!(solver.x, clean.x);
    assert_eq!(solver.v, clean.v);
    assert_eq!(
        solver.membrane_plasticity_report().unwrap(),
        clean.membrane_plasticity_report().unwrap()
    );
    assert_eq!(
        solver.relaxation_report().unwrap(),
        clean.relaxation_report().unwrap()
    );
    let mut stale = solver.clone();
    stale.model.membranes[0].g[0][0] *= 2.0;
    assert!(stale.step(0.001).is_err());
    let mut outside = solver;
    outside.x[1].x = LENGTH * 1.8_f64.sqrt();
    assert!(outside.step(0.001).is_err());
}
