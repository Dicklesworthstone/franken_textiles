//! Identified continuous-time parameters drive actual triangles and the implicit cloth solver.
//! Analytic force answers and timed swatches are synthetic, not physical textile measurements.
use ftx_core::{Vec2, Vec3};
use ftx_drape::material::resolve;
use ftx_drape::run::DrapeOptions;
use ftx_drape::{ClothModel, Solver};
use ftx_pattern::Pattern;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_relaxation::calibration::{Input, calibrate};
use ftx_pattern::textile_import::catalog;
use serde_json::json;

fn identified(time_scale: f64) -> Selection {
    let mut input: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation-history.json"
    ))
    .unwrap();
    for e in &mut input.experiments {
        for s in &mut e.samples {
            s.time_seconds *= time_scale;
        }
    }
    for b in &mut input.time_bounds_seconds {
        for t in b {
            *t *= time_scale;
        }
    }
    let report = calibrate(&input).unwrap();
    catalog()
        .unwrap()
        .resolve(&Selection {
            id: report.material.id.clone(),
            record: Some(report.material),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection()
}

fn identified_directional(time_scale: f64, identify_times: bool) -> Selection {
    let mut input: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-orthotropic-relaxation-history.json"
    ))
    .unwrap();
    for experiment in &mut input.experiments {
        for sample in &mut experiment.samples {
            sample.time_seconds *= time_scale;
        }
    }
    if identify_times {
        input.orthotropic = Some(
            serde_json::from_value(json!({
                "time_bounds_seconds": [
                    [0.1 * time_scale, 0.9 * time_scale],
                    [1.0 * time_scale, 4.0 * time_scale],
                    [5.0 * time_scale, 24.0 * time_scale]
                ],
                "zero_threshold_n_m": 1e-7
            }))
            .unwrap(),
        );
    } else {
        for time in &mut input.orthotropic.as_mut().unwrap().times_seconds {
            *time *= time_scale;
        }
    }
    let report = calibrate(&input).unwrap();
    catalog()
        .unwrap()
        .resolve(&Selection {
            id: report.material.id.clone(),
            record: Some(report.material),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection()
}

fn solver(select: &Selection, fixed: bool) -> Solver {
    let record = catalog().unwrap().resolve(select).unwrap();
    let pattern = Pattern::new(json!({"fabric":{"material":select}}));
    let inputs = resolve(&pattern, &DrapeOptions::default()).unwrap();
    let fabric = inputs.fabric;
    let rest = vec![Vec2::ZERO, Vec2::new(0.1, 0.), Vec2::new(0., 0.1)];
    let mut model = ClothModel::new(
        rest,
        &[[0, 1, 2]],
        fabric.membrane(1.),
        fabric.bending_nm,
        fabric.areal_kg_m2,
    )
    .unwrap();
    model
        .set_membrane_responses(&[record.membrane_response().unwrap()])
        .unwrap();
    let mut solver = Solver::new(
        model,
        vec![Vec3::ZERO, Vec3::new(0.11, 0., 0.), Vec3::new(0., 0.1, 0.)],
        &[[true; 3], [fixed, true, true], [true; 3]],
        Vec3::ZERO,
    )
    .with_relaxation(&[record.membrane_relaxation().unwrap()])
    .unwrap();
    solver.tol = 1e-8;
    solver.max_newton = 100;
    solver
}

fn equilibrium(select: &Selection) -> Selection {
    let mut out = select.clone();
    out.record.as_mut().unwrap().membrane_relaxation = None;
    out.expected_sha256 = None;
    out
}

#[test]
fn identified_time_constants_reproduce_actual_fixed_geometry_reactions() {
    let selection = identified(1.);
    let equilibrium = solver(&equilibrium(&selection), true).elastic_forces();
    let mut s = solver(&selection, true);
    let initial = s.x.clone();
    for step in 1..=30 {
        assert!(s.step(0.01).unwrap().converged);
        let decay = 1.7 * (0.3_f64 / 0.31).powi(step) + 0.65 * (8.0_f64 / 8.01).powi(step);
        let f = s.elastic_forces();
        for (observed, eq) in f.iter().zip(&equilibrium) {
            assert!((observed.x - eq.x * (1. + decay)).abs() < 1e-7);
            assert!((observed.y - eq.y * (1. + decay)).abs() < 1e-7);
        }
        assert_eq!(s.x, initial);
    }
    assert_eq!(
        s.relaxation_report().unwrap().unwrap()["accepted_steps"],
        30
    );
}

#[test]
fn changing_only_acquisition_times_changes_material_memory_and_actual_free_motion() {
    let a = identified(1.);
    let b = identified(4.);
    assert_eq!(
        serde_json::to_value(&a.record.as_ref().unwrap().properties).unwrap(),
        serde_json::to_value(&b.record.as_ref().unwrap().properties).unwrap()
    );
    let mut fast = solver(&a, false);
    let mut slow = solver(&b, false);
    // A shared synthetic body force sustains load after the initial release. Waiting until an
    // unloaded sheet has returned to rest would not test a sustained-load timescale difference.
    fast.gravity = Vec3::new(100., 0., 0.);
    slow.gravity = fast.gravity;
    assert_eq!(fast.x, slow.x);
    assert_eq!(fast.model.mass, slow.model.mass);
    assert_eq!(fast.model.membrane, slow.model.membrane);
    assert_eq!(fast.model.bending, slow.model.bending);
    for _ in 0..40 {
        assert!(fast.step(0.01).unwrap().converged);
        assert!(slow.step(0.01).unwrap().converged);
    }
    assert!(
        (fast.x[1].x - slow.x[1].x).abs() > 1e-8,
        "fast {:?}, slow {:?}",
        fast.x[1],
        slow.x[1]
    );
    assert_ne!(
        fast.relaxation_report().unwrap(),
        slow.relaxation_report().unwrap()
    );
    assert_ne!(a.expected_sha256, b.expected_sha256);
}

#[test]
fn solver_time_refinement_converges_to_the_independent_continuous_spectrum() {
    let select = identified(1.);
    let eq = solver(&equilibrium(&select), true).elastic_forces()[1].x;
    let exact = eq * (1. + 1.7 * (-0.6_f64 / 0.3).exp() + 0.65 * (-0.6_f64 / 8.).exp());
    let integrate = |dt: f64, steps: usize| {
        let mut s = solver(&select, true);
        for _ in 0..steps {
            s.step(dt).unwrap();
        }
        (s.elastic_forces()[1].x - exact).abs()
    };
    let coarse = integrate(0.1, 6);
    let fine = integrate(0.01, 60);
    assert!(coarse > 1e-6);
    assert!(fine < coarse / 8., "coarse {coarse}, fine {fine}");
}

#[test]
fn identified_directional_tensors_reproduce_independent_mixed_mode_triangle_forces() {
    for identify_times in [false, true] {
        assert_directional_reactions(&identified_directional(1., identify_times));
    }
}

fn assert_directional_reactions(selected: &Selection) {
    let mut s = solver(selected, true);
    // Explicit F excites warp, weft, signed coupling and shear simultaneously. For the
    // 0.1 m right reference triangle, nodal forces are -0.05 times the columns of P=F*S.
    let (a, b, d) = (1.08_f64, 0.05_f64, 1.02_f64);
    s.x[1] = Vec3::new(0.1 * a, 0., 0.);
    s.x[2] = Vec3::new(0.1 * b, 0.1 * d, 0.);
    let initial = s.x.clone();
    let strain = [(a * a - 1.) / 2., (b * b + d * d - 1.) / 2., a * b];
    for step in 1..=20 {
        assert!(s.step(0.05).unwrap().converged);
        let mut stress = [800. * strain[0], 500. * strain[1], 120. * strain[2]];
        for (tau, [warp, weft, coupling, shear]) in [
            (0.3_f64, [240., 60., -30., 0.]),
            (2., [0., 0., 0., 90.]),
            (8., [0., 120., 0., 0.]),
        ] {
            let decay = (tau / (tau + 0.05)).powi(step);
            stress[0] += decay * (warp * strain[0] + coupling * strain[1]);
            stress[1] += decay * (coupling * strain[0] + weft * strain[1]);
            stress[2] += decay * shear * strain[2];
        }
        let first = Vec3::new(
            -0.05 * (a * stress[0] + b * stress[2]),
            -0.05 * d * stress[2],
            0.,
        );
        let second = Vec3::new(
            -0.05 * (a * stress[2] + b * stress[1]),
            -0.05 * d * stress[1],
            0.,
        );
        for (force, expected) in s
            .elastic_forces()
            .iter()
            .zip([-(first + second), first, second])
        {
            assert!((force.x - expected.x).abs() < 1e-8);
            assert!((force.y - expected.y).abs() < 1e-8);
            assert!(force.z.abs() < 1e-12);
        }
        assert_eq!(s.x, initial);
    }
    assert_eq!(
        s.relaxation_report().unwrap().unwrap()["accepted_steps"],
        20
    );
}

#[test]
fn independently_calibrated_recovery_times_change_actual_free_motion_only_through_materials() {
    for identify_times in [false, true] {
        assert_acquisition_times_drive_motion(
            identified_directional(1., identify_times),
            identified_directional(4., identify_times),
        );
    }
}

fn assert_acquisition_times_drive_motion(fast_material: Selection, slow_material: Selection) {
    assert_eq!(
        serde_json::to_value(&fast_material.record.as_ref().unwrap().properties).unwrap(),
        serde_json::to_value(&slow_material.record.as_ref().unwrap().properties).unwrap()
    );
    let mut fast = solver(&fast_material, false);
    let mut slow = solver(&slow_material, false);
    fast.gravity = Vec3::new(100., 0., 0.);
    slow.gravity = fast.gravity;
    assert_eq!(fast.x, slow.x);
    assert_eq!(fast.model.mass, slow.model.mass);
    assert_eq!(fast.model.membrane, slow.model.membrane);
    assert_eq!(fast.model.bending, slow.model.bending);
    for _ in 0..40 {
        assert!(fast.step(0.01).unwrap().converged);
        assert!(slow.step(0.01).unwrap().converged);
    }
    assert!((fast.x[1].x - slow.x[1].x).abs() > 1e-8);
    assert_ne!(fast_material.expected_sha256, slow_material.expected_sha256);
}
