//! Independent permanent-set answers through physical triangle kinematics and accepted Newton steps.
//! All coefficients, domains and applied loads are synthetic software qualification fixtures.
use ftx_core::{RefusalCode, Vec2, Vec3};
use ftx_drape::{ClothModel, Membrane, Solver};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_membrane_plasticity::PlasticMembrane;
use ftx_pattern::materials::{Catalog, Material};
use serde_json::Value;

const LENGTH: f64 = 0.1;
const AREA: f64 = LENGTH * LENGTH / 2.;
const LOAD: f64 = 0.1;
const YIELD: f64 = 20.;
const HARDENING: f64 = 100.;

fn near(a: f64, b: f64, tolerance: f64) {
    assert!(
        (a - b).abs() <= tolerance * (1. + a.abs() + b.abs()),
        "{a} != {b}"
    );
}

fn membrane() -> Membrane {
    Membrane::orthotropic(1000., 600., 0., 300., 1.)
}

fn law(m: Membrane) -> PlasticMembrane {
    PlasticMembrane::new(
        m.q,
        [YIELD, 30., 10.],
        [HARDENING, 80., 40.],
        [0.4; 3],
        [0.4; 3],
    )
    .unwrap()
}

fn bare(fixed: bool, strain: f64, material: Membrane) -> Solver {
    let model = ClothModel::new(
        vec![Vec2::ZERO, Vec2::new(LENGTH, 0.), Vec2::new(0., LENGTH)],
        &[[0, 1, 2]],
        material,
        0.,
        0.2,
    )
    .unwrap();
    let mut solver = Solver::new(
        model,
        vec![
            Vec3::ZERO,
            Vec3::new(LENGTH * (1. + 2. * strain).sqrt(), 0., 0.),
            Vec3::new(0., LENGTH, 0.),
        ],
        &[[true; 3], [fixed, true, true], [true; 3]],
        Vec3::ZERO,
    );
    solver.tol = 1e-8;
    solver
}

fn plastic(fixed: bool) -> Solver {
    bare(fixed, LOAD, membrane())
        .with_membrane_plasticity(&[Some(law(membrane()))])
        .unwrap()
}

fn history(solver: &Solver) -> Value {
    solver.membrane_plasticity_report().unwrap().unwrap()
}

fn permanent_set() -> f64 {
    (1000. * LOAD - YIELD) / (1000. + HARDENING)
}

fn check_uniaxial(solver: &Solver, strain: f64, permanent: f64, accumulated: f64) {
    let report = history(solver);
    let element = &report["elements"][0];
    let stress = 1000. * (strain - permanent);
    let stored =
        AREA * (0.5 * 1000. * (strain - permanent).powi(2) + 0.5 * HARDENING * accumulated.powi(2));
    near(
        element["plastic_green_strain"][0].as_f64().unwrap(),
        permanent,
        1e-12,
    );
    near(
        element["accumulated_plastic_green_strain"][0]
            .as_f64()
            .unwrap(),
        accumulated,
        1e-12,
    );
    near(
        element["elastic_stress_n_m"][0].as_f64().unwrap(),
        stress,
        1e-11,
    );
    near(solver.membrane_energy().unwrap(), stored, 1e-12);
    near(
        report["stored_elastic_and_hardening_energy_j"]
            .as_f64()
            .unwrap(),
        stored,
        1e-12,
    );
    near(
        report["plastic_dissipation_j"].as_f64().unwrap(),
        AREA * YIELD * accumulated,
        1e-12,
    );
    // Nominal reaction is F*S integrated over the reference triangle; S alone is not force.
    let reaction = AREA / LENGTH * (1. + 2. * strain).sqrt() * stress;
    let forces = solver.elastic_forces();
    near(forces[1].x, -reaction, 1e-11);
    near(forces[0].x, reaction, 1e-11);
    for (i, force) in forces.iter().enumerate() {
        near(force.y, 0., 1e-11);
        near(force.z, 0., 1e-11);
        if i == 2 {
            near(force.x, 0., 1e-11);
        }
    }
}

#[test]
fn clamped_load_unload_and_reverse_cycle_has_analytic_residual_set_storage_and_reactions() {
    let mut solver = plastic(true);
    let pristine = history(&solver);
    check_uniaxial(&solver, LOAD, 0., 0.);
    // Reading force, storage and the report never performs a trial return-map update.
    assert_eq!(history(&solver), pristine);
    let x = solver.x.clone();
    let mut slower = solver.clone();
    solver.step(0.001).unwrap();
    slower.step(0.02).unwrap();
    assert_eq!(solver.x, x);
    assert_eq!(history(&solver), history(&slower)); // rate-independent, not a relaxation clock
    let permanent = permanent_set();
    check_uniaxial(&solver, LOAD, permanent, permanent);
    assert_eq!(history(&solver)["accepted_steps"], 1);
    for strain in [permanent, LOAD, LOAD] {
        solver.x[1].x = LENGTH * (1. + 2. * strain).sqrt();
        solver.step(0.003).unwrap();
        check_uniaxial(&solver, strain, permanent, permanent);
    }
    // Reverse plastic flow increases accumulated strain even when signed permanent strain reverses.
    let reverse = -0.04;
    let reversed = (1000. * reverse + YIELD + 2. * HARDENING * permanent) / (1000. + HARDENING);
    solver.x[1].x = LENGTH * (1. + 2. * reverse).sqrt();
    solver.step(0.003).unwrap();
    check_uniaxial(&solver, reverse, reversed, 2. * permanent - reversed);
    assert!(solver.static_solve().is_err());
    assert!(solver.settle(0.001, 0.01, 1e-4, 3, 10).is_err());
}

fn loaded_free_solver() -> Solver {
    let mut solver = plastic(false);
    // An independently balanced synthetic dead load accepts the desired stretched state while
    // keeping the pulling vertex free for the subsequent unloaded dynamics.
    let stress = 1000. * (LOAD - permanent_set());
    let force = AREA / LENGTH * (1. + 2. * LOAD).sqrt() * stress;
    solver.gravity = Vec3::new(force / solver.model.mass[1], 0., 0.);
    solver.step(0.01).unwrap();
    near(solver.x[1].x, LENGTH * (1. + 2. * LOAD).sqrt(), 1e-10);
    check_uniaxial(&solver, LOAD, permanent_set(), permanent_set());
    solver.gravity = Vec3::ZERO;
    solver
}

#[test]
fn unloading_actual_free_dynamics_retains_extension_while_the_elastic_control_recovers() {
    let mut solver = loaded_free_solver();
    let mut elastic = bare(false, LOAD, membrane());
    assert_eq!(solver.model.membranes, elastic.model.membranes);
    assert_eq!(solver.model.mass, elastic.model.mass);
    let dissipation = history(&solver)["plastic_dissipation_j"].as_f64().unwrap();
    for _ in 0..12 {
        assert!(solver.step(0.01).unwrap().converged);
        assert!(elastic.step(0.01).unwrap().converged);
    }
    near(
        solver.x[1].x,
        LENGTH * (1. + 2. * permanent_set()).sqrt(),
        2e-8,
    );
    near(elastic.x[1].x, LENGTH, 2e-8);
    assert!(solver.x[1].x > elastic.x[1].x + 0.005);
    near(solver.elastic_forces()[1].x, 0., 2e-6);
    near(
        history(&solver)["plastic_dissipation_j"].as_f64().unwrap(),
        dissipation,
        1e-12,
    );
    near(
        solver.membrane_energy().unwrap(),
        AREA * 0.5 * HARDENING * permanent_set().powi(2),
        1e-10,
    );
    assert!(elastic.membrane_plasticity_report().unwrap().is_none());
}

#[test]
fn accepted_triangle_energy_and_reactions_are_objective_in_three_dimensions() {
    let material = Membrane {
        q: [[1000., 120., 0.], [120., 600., 0.], [0., 0., 300.]],
        ..membrane()
    };
    let rest = [Vec2::ZERO, Vec2::new(0.1, 0.03), Vec2::new(0.02, 0.12)];
    let model = ClothModel::new(rest.to_vec(), &[[0, 1, 2]], material, 0., 0.2).unwrap();
    // This deformation realizes Green strain [0.06,0.07,0.08] and yields all three
    // material coordinates. Accept it through the public physical-step API.
    let a = 1.12_f64.sqrt();
    let b = 0.08 / a;
    let initial = rest.map(|p| Vec3::new(a * p.x + b * p.y, (1.14 - b * b).sqrt() * p.y, 0.));
    let mut solver = Solver::new(model, initial.to_vec(), &[[true; 3]; 3], Vec3::ZERO)
        .with_membrane_plasticity(&[Some(law(material))])
        .unwrap();
    solver.step(0.001).unwrap();
    assert!(
        history(&solver)["elements"][0]["plastic_green_strain"]
            .as_array()
            .unwrap()
            .iter()
            .all(|value| value.as_f64().unwrap() > 0.)
    );
    solver.x = rest
        .map(|p| Vec3::new(1.08, 0.01, 0.07) * p.x + Vec3::new(0.1, 1.04, 0.03) * p.y)
        .to_vec();
    let before = history(&solver);
    let energy = solver.membrane_energy().unwrap();
    let forces = solver.checked_elastic_forces().unwrap();
    // Differentiate actual accepted storage; no internal kernel or trial history import is used.
    for j in 0..9 {
        let mut plus = solver.clone();
        let mut minus = solver.clone();
        let delta = Vec3::new(
            f64::from(j % 3 == 0),
            f64::from(j % 3 == 1),
            f64::from(j % 3 == 2),
        ) * 1e-7;
        plus.x[j / 3] += delta;
        minus.x[j / 3] -= delta;
        near(
            -forces[j / 3][j % 3],
            (plus.membrane_energy().unwrap() - minus.membrane_energy().unwrap()) / 2e-7,
            2e-6,
        );
    }
    let rotate = |p: Vec3| Vec3::new(p.z, p.x, p.y);
    let mut moved = solver.clone();
    moved.x = moved
        .x
        .iter()
        .map(|p| rotate(*p) + Vec3::new(2., -3., 4.))
        .collect();
    near(moved.membrane_energy().unwrap(), energy, 1e-10);
    for (actual, original) in moved.checked_elastic_forces().unwrap().iter().zip(forces) {
        let expected = rotate(original);
        for k in 0..3 {
            near(actual[k], expected[k], 1e-9);
        }
    }
    assert_eq!(history(&solver), before);
}

fn joined(material: Membrane, rotated_grain: bool) -> Solver {
    let rest = [
        Vec2::ZERO,
        Vec2::new(LENGTH, 0.),
        Vec2::new(0., LENGTH),
        Vec2::new(LENGTH, LENGTH),
    ];
    let triangles = [[0, 1, 2], [1, 3, 2]];
    let mut frames = triangles.map(|triangle| triangle.map(|i| rest[i]));
    if rotated_grain {
        frames[1] = frames[1].map(|p| Vec2::new(p.y, -p.x));
    }
    let model = ClothModel::from_triangles(4, &triangles, &frames, material, 0., 0.2).unwrap();
    let stretch = (1. + 2. * LOAD).sqrt();
    Solver::new(
        model,
        rest.map(|p| Vec3::new(stretch * p.x, p.y, 0.)).to_vec(),
        &[[true; 3]; 4],
        Vec3::ZERO,
    )
}

#[test]
fn welded_vertices_keep_independent_directional_permanent_strain_in_each_grain_frame() {
    let mut solver = joined(membrane(), true)
        .with_membrane_plasticity(&[Some(law(membrane())), Some(law(membrane()))])
        .unwrap();
    let (geometry, mass, triangles) = (
        solver.x.clone(),
        solver.model.mass.clone(),
        solver.model.membranes.clone(),
    );
    solver.step(0.005).unwrap();
    assert_eq!(solver.x, geometry);
    assert_eq!(solver.model.mass, mass);
    assert_eq!(solver.model.membranes, triangles);
    assert_eq!(solver.x.len(), 4); // two shared seam nodes, not separate per-material positions
    let report = history(&solver);
    let expected = [
        [permanent_set(), 0., 0.],
        [0., (600. * LOAD - 30.) / 680., 0.],
    ];
    for (i, expected) in expected.iter().enumerate() {
        assert_eq!(report["elements"][i]["triangle"], i);
        for (axis, value) in expected.iter().enumerate() {
            near(
                report["elements"][i]["plastic_green_strain"][axis]
                    .as_f64()
                    .unwrap(),
                *value,
                1e-12,
            );
        }
    }
    let forces = solver.elastic_forces();
    let stored = solver.membrane_energy().unwrap();
    let rotate = |p: Vec3| Vec3::new(p.z, p.x, p.y);
    let mut moved = solver.clone();
    moved.x = moved
        .x
        .iter()
        .map(|p| rotate(*p) + Vec3::new(1., -2., 3.))
        .collect();
    near(moved.membrane_energy().unwrap(), stored, 1e-11);
    for (actual, original) in moved.elastic_forces().iter().zip(forces) {
        let expected = rotate(original);
        for k in 0..3 {
            near(actual[k], expected[k], 1e-9);
        }
    }
}

#[test]
fn failed_trial_rolls_back_positions_velocity_and_history_and_can_retry_deterministically() {
    let mut solver = loaded_free_solver();
    solver.step(0.001).unwrap();
    let mut clean = solver.clone();
    let (x, v, before) = (solver.x.clone(), solver.v.clone(), history(&solver));
    solver.max_newton = 0;
    assert_eq!(
        solver.step(0.001).unwrap_err().code,
        RefusalCode::NotConverged
    );
    assert_eq!(solver.x, x);
    assert_eq!(solver.v, v);
    assert_eq!(history(&solver), before);
    solver.max_newton = clean.max_newton;
    for _ in 0..3 {
        solver.step(0.001).unwrap();
        clean.step(0.001).unwrap();
    }
    assert_eq!(solver.x, clean.x);
    assert_eq!(solver.v, clean.v);
    assert_eq!(history(&solver), history(&clean));
    let before = history(&solver);
    for seconds in [0., -1., f64::NAN, f64::INFINITY] {
        assert!(solver.step(seconds).is_err());
    }
    assert_eq!(history(&solver), before);
}

#[test]
fn changed_material_rest_frame_and_source_domain_cannot_reuse_accepted_history() {
    let mut solver = plastic(true);
    solver.step(0.001).unwrap();
    assert!(solver.clone().with_membrane_plasticity(&[None]).is_err());
    assert!(
        bare(true, LOAD, membrane())
            .with_membrane_plasticity(&[])
            .is_err()
    );
    for change in [
        (|s: &mut Solver| s.model.membrane.q[0][0] *= 2.) as fn(&mut Solver),
        |s| s.model.membranes[0].g[0][0] += 1.,
        |s| s.model.membranes[0].area *= 2.,
        |s| s.model.compression[0] = [0.5, 1.],
        |s| s.x[1].x = LENGTH * 2.,
    ] {
        let mut changed = solver.clone();
        change(&mut changed);
        let (x, v) = (changed.x.clone(), changed.v.clone());
        assert!(changed.membrane_plasticity_report().is_err());
        assert!(changed.membrane_energy().is_err());
        assert!(changed.step(0.001).is_err());
        assert_eq!(changed.x, x);
        assert_eq!(changed.v, v);
    }
    let bounded = PlasticMembrane::new(
        membrane().q,
        [YIELD, 30., 10.],
        [HARDENING, 80., 40.],
        [0.4; 3],
        [0.02; 3],
    )
    .unwrap();
    let mut limited = bare(true, LOAD, membrane())
        .with_membrane_plasticity(&[Some(bounded)])
        .unwrap();
    let (x, v, before) = (limited.x.clone(), limited.v.clone(), history(&limited));
    assert!(limited.step(0.001).is_err()); // source history bound, not a clamped plateau
    assert_eq!(limited.x, x);
    assert_eq!(limited.v, v);
    assert_eq!(history(&limited), before);
}

#[test]
fn checked_reactions_agree_when_valid_and_refuse_stale_material_or_reference_history() {
    let mut solver = plastic(true);
    solver.step(0.001).unwrap();
    let before = history(&solver);
    assert_eq!(
        solver.checked_elastic_forces().unwrap(),
        solver.elastic_forces()
    );
    assert_eq!(history(&solver), before);
    let elastic = bare(true, LOAD, membrane());
    assert_eq!(
        elastic.checked_elastic_forces().unwrap(),
        elastic.elastic_forces()
    );
    for change in [
        (|s: &mut Solver| s.model.membrane.q[0][0] *= 2.) as fn(&mut Solver),
        |s| s.model.membranes[0].g[0][0] += 1.,
    ] {
        let mut changed = solver.clone();
        change(&mut changed);
        let (x, v) = (changed.x.clone(), changed.v.clone());
        assert!(changed.checked_elastic_forces().is_err());
        let invalid = changed.elastic_forces();
        assert_eq!(invalid.len(), changed.x.len());
        assert!(
            invalid
                .iter()
                .all(|force| force.x.is_nan() && force.y.is_nan() && force.z.is_nan())
        );
        assert_eq!(changed.x, x);
        assert_eq!(changed.v, v);
    }
}

fn maxwell_selection() -> Selection {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation.json"
    ))
    .unwrap()
}

#[test]
fn compression_nonlinear_and_same_triangle_maxwell_cannot_silently_change_the_plastic_law() {
    let mut softened = bare(true, LOAD, membrane());
    softened.model.set_compression(&[0.03]).unwrap();
    assert!(
        softened
            .with_membrane_plasticity(&[Some(law(membrane()))])
            .unwrap_err()
            .message
            .contains("compression")
    );
    let db = Catalog::builtin().unwrap();
    let selected = db.resolve(&maxwell_selection()).unwrap();
    let mechanics = selected.mechanics().unwrap();
    let material = Membrane::orthotropic(
        mechanics.warp_n_m,
        mechanics.weft_n_m,
        mechanics.poisson,
        mechanics.shear_n_m,
        1.,
    );
    let relaxation = selected.membrane_relaxation().unwrap();
    let base = bare(true, LOAD, material);
    assert!(
        base.clone()
            .with_relaxation(std::slice::from_ref(&relaxation))
            .unwrap()
            .with_membrane_plasticity(&[Some(law(material))])
            .unwrap_err()
            .message
            .contains("Maxwell")
    );
    assert!(
        base.with_membrane_plasticity(&[Some(law(material))])
            .unwrap()
            .with_relaxation(&[relaxation])
            .unwrap_err()
            .message
            .contains("plastic")
    );
    let mut select = maxwell_selection();
    let record: Material = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear.json"
    ))
    .unwrap();
    select.id.clone_from(&record.id);
    select.record = Some(record);
    let record = db.resolve(&select).unwrap();
    let mechanics = record.mechanics().unwrap();
    let material = Membrane::orthotropic(
        mechanics.warp_n_m,
        mechanics.weft_n_m,
        mechanics.poisson,
        mechanics.shear_n_m,
        1.,
    );
    let response = record.membrane_response().unwrap();
    let mut nonlinear = bare(true, LOAD, material);
    nonlinear
        .model
        .set_membrane_responses(std::slice::from_ref(&response))
        .unwrap();
    assert!(
        nonlinear
            .with_membrane_plasticity(&[Some(law(material))])
            .unwrap_err()
            .message
            .contains("nonlinear")
    );
    let mut changed = bare(true, LOAD, material)
        .with_membrane_plasticity(&[Some(law(material))])
        .unwrap();
    changed.model.set_membrane_responses(&[response]).unwrap();
    assert!(changed.step(0.001).is_err());
    assert!(changed.membrane_plasticity_report().is_err());
}

#[test]
fn other_triangle_maxwell_history_coexists_without_losing_or_double_counting_storage() {
    let selected = Catalog::builtin()
        .unwrap()
        .resolve(&maxwell_selection())
        .unwrap();
    let mechanics = selected.mechanics().unwrap();
    let material = Membrane::orthotropic(
        mechanics.warp_n_m,
        mechanics.weft_n_m,
        mechanics.poisson,
        mechanics.shear_n_m,
        1.,
    );
    let relaxation = selected.membrane_relaxation().unwrap();
    let plastic_laws = [Some(law(material)), None];
    let relaxation_laws = [None, relaxation];
    let base = joined(material, false);
    let mut a = base
        .clone()
        .with_membrane_plasticity(&plastic_laws)
        .unwrap()
        .with_relaxation(&relaxation_laws)
        .unwrap();
    let mut b = base
        .with_relaxation(&relaxation_laws)
        .unwrap()
        .with_membrane_plasticity(&plastic_laws)
        .unwrap();
    for _ in 0..3 {
        a.step(0.01).unwrap();
        b.step(0.01).unwrap();
    }
    assert_eq!(history(&a), history(&b));
    assert_eq!(
        a.relaxation_report().unwrap(),
        b.relaxation_report().unwrap()
    );
    let plastic_report = history(&a);
    let maxwell_report = a.relaxation_report().unwrap().unwrap();
    assert_eq!(plastic_report["elements"].as_array().unwrap().len(), 1);
    assert_eq!(plastic_report["elements"][0]["triangle"], 0);
    assert_eq!(maxwell_report["elements"].as_array().unwrap().len(), 1);
    assert_eq!(maxwell_report["elements"][0]["triangle"], 1);
    let stored = plastic_report["stored_elastic_and_hardening_energy_j"]
        .as_f64()
        .unwrap()
        + a.model.membranes[1].energy(&a.x, &a.model.material(1))
        + maxwell_report["stored_branch_energy_j"].as_f64().unwrap();
    near(a.membrane_energy().unwrap(), stored, 1e-13);
    assert!(plastic_report["plastic_dissipation_j"].as_f64().unwrap() > 0.);
    assert!(maxwell_report["viscous_step_work_j"].as_f64().unwrap() > 0.);
}
