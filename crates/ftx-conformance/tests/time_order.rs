//! Strict temporal convergence-order and spectral-dissipation verification for
//! `ftx_drape::TimeIntegrator` using the `ftx_conformance::mms` `±0.2` order gate.
//!
//! Proves:
//! 1. `TimeIntegrator::ImplicitEuler` converges at order `1.0 ± 0.2` on a free-oscillating elastic
//!    cloth triangle under gravity;
//! 2. `TimeIntegrator::GeneralizedAlpha { rho_inf: 1.0 }` and `{ rho_inf: 0.5 }` converge at order
//!    `2.0 ± 0.2` on the exact same cloth dynamics;
//! 3. `rho_inf = 1.0` preserves mechanical energy closely while `rho_inf = 0.0` strongly damps
//!    high-frequency membrane jitter relative to `rho_inf = 1.0`.

use ftx_conformance::mms::{LadderSide, OrderGate, RefinementLadder};
use ftx_core::{Vec2, Vec3};
use ftx_drape::{ClothModel, Membrane, Solver, TimeIntegrator};

/// A single stretched right triangle with two pinned corners and one free vertex oscillating
/// purely in-plane under nonlinear Green-strain membrane elasticity + gravity.
fn oscillator_cloth(integrator: TimeIntegrator) -> Solver {
    let rest = vec![Vec2::ZERO, Vec2::new(0.10, 0.0), Vec2::new(0.0, 0.10)];
    // Initial gentle stretch along x/y so membrane tension is smooth and active.
    let x = vec![
        Vec3::ZERO,
        Vec3::new(0.10, 0.0, 0.0),
        Vec3::new(0.004, 0.106, -0.003),
    ];
    let fixed = [
        [true, true, true],
        [true, true, true],
        [false, false, false],
    ];
    let model = ClothModel::new(
        rest,
        &[[0, 1, 2]],
        Membrane::orthotropic(80.0, 80.0, 0.2, 25.0, 1.0),
        0.0,
        0.25,
    )
    .unwrap();
    let mut s = Solver::new(model, x, &fixed, Vec3::new(0.0, 0.0, -9.81));
    s.tol = 1e-7;
    s.integrator = integrator;
    s
}

fn integrate_to(integrator: TimeIntegrator, t_end: f64, steps: usize) -> Vec3 {
    let h = t_end / steps as f64;
    let mut s = oscillator_cloth(integrator);
    for _ in 0..steps {
        let rep = s.step(h).unwrap();
        assert!(rep.converged, "{rep:?}");
    }
    s.x[2]
}

#[test]
fn generalized_alpha_achieves_second_order_while_implicit_euler_is_first_order() {
    let t_end = 0.016;
    // Reference solution at fine step (128x finer than coarsest ladder rung) with 2nd-order midpoint.
    let x_ref = integrate_to(
        TimeIntegrator::GeneralizedAlpha { rho_inf: 1.0 },
        t_end,
        1024,
    );

    let step_counts = [8usize, 16, 32, 64];
    let hs: Vec<f64> = step_counts.iter().map(|&n| t_end / n as f64).collect();

    // 1. Implicit Euler -> order 1.0 ± 0.2.
    let euler_errs: Vec<f64> = step_counts
        .iter()
        .map(|&n| (integrate_to(TimeIntegrator::ImplicitEuler, t_end, n) - x_ref).norm())
        .collect();
    let euler_ladder = RefinementLadder::new(hs.clone(), euler_errs).unwrap();
    let euler_verdict = OrderGate { theoretical: 1.0 }
        .check("cloth-implicit-euler", LadderSide::Primal, &euler_ladder)
        .expect("Implicit Euler should converge at order 1.0 ± 0.2");
    assert!(
        (euler_verdict.fit.observed - 1.0).abs() <= 0.2,
        "{euler_verdict:?}"
    );

    // 2. Generalized-α (rho_inf = 1.0, undamped midpoint) -> order 2.0 ± 0.2.
    let ga1_errs: Vec<f64> = step_counts
        .iter()
        .map(|&n| {
            (integrate_to(TimeIntegrator::GeneralizedAlpha { rho_inf: 1.0 }, t_end, n) - x_ref)
                .norm()
        })
        .collect();
    let ga1_ladder = RefinementLadder::new(hs.clone(), ga1_errs).unwrap();
    let ga1_verdict = OrderGate { theoretical: 2.0 }
        .check("cloth-galpha-rho1", LadderSide::Primal, &ga1_ladder)
        .expect("Generalized-alpha rho=1.0 should converge at order 2.0 ± 0.2");
    assert!(
        (ga1_verdict.fit.observed - 2.0).abs() <= 0.2,
        "{ga1_verdict:?}"
    );

    // 3. Generalized-α (rho_inf = 0.5, moderate HF damping) -> STILL order 2.0 ± 0.2!
    let ga05_errs: Vec<f64> = step_counts
        .iter()
        .map(|&n| {
            (integrate_to(TimeIntegrator::GeneralizedAlpha { rho_inf: 0.5 }, t_end, n) - x_ref)
                .norm()
        })
        .collect();
    let ga05_ladder = RefinementLadder::new(hs.clone(), ga05_errs).unwrap();
    let ga05_verdict = OrderGate { theoretical: 2.0 }
        .check("cloth-galpha-rho05", LadderSide::Primal, &ga05_ladder)
        .expect("Generalized-alpha rho=0.5 should converge at order 2.0 ± 0.2");
    assert!(
        (ga05_verdict.fit.observed - 2.0).abs() <= 0.2,
        "{ga05_verdict:?}"
    );

    // 4. Generalized-α (rho_inf = 0.0, L-stable IPC contact default; larger β=1.0 error constant
    // enters the asymptotic O(h²) regime on the 2x-finer ladder) -> order 2.0 ± 0.2.
    let fine_counts = [16usize, 32, 64, 128];
    let fine_hs: Vec<f64> = fine_counts.iter().map(|&n| t_end / n as f64).collect();
    let ga0_errs: Vec<f64> = fine_counts
        .iter()
        .map(|&n| {
            (integrate_to(TimeIntegrator::GeneralizedAlpha { rho_inf: 0.0 }, t_end, n) - x_ref)
                .norm()
        })
        .collect();
    let ga0_ladder = RefinementLadder::new(fine_hs, ga0_errs).unwrap();
    let ga0_verdict = OrderGate { theoretical: 2.0 }
        .check("cloth-galpha-rho0", LadderSide::Primal, &ga0_ladder)
        .expect("Generalized-alpha rho=0.0 should converge at order 2.0 ± 0.2");
    assert!(
        (ga0_verdict.fit.observed - 2.0).abs() <= 0.2,
        "{ga0_verdict:?}"
    );
}

#[test]
fn generalized_alpha_controls_high_frequency_dissipation_from_rho1_to_rho0() {
    let total_energy = |s: &Solver| -> f64 {
        let ke: f64 = s
            .model
            .mass
            .iter()
            .zip(&s.v)
            .map(|(&m, v)| 0.5 * m * v.norm2())
            .sum();
        let pe = s.model.elastic_energy(&s.x)
            - s.model
                .mass
                .iter()
                .zip(&s.x)
                .map(|(&m, &x)| m * s.gravity.dot(x))
                .sum::<f64>();
        ke + pe
    };

    // High-stiffness membrane creates high-frequency oscillation (ω h > 2) where ρ∞ controls damping.
    let make_stiff = |integrator: TimeIntegrator| -> Solver {
        let rest = vec![Vec2::ZERO, Vec2::new(0.10, 0.0), Vec2::new(0.0, 0.10)];
        let x = vec![
            Vec3::ZERO,
            Vec3::new(0.10, 0.0, 0.0),
            Vec3::new(0.0, 0.102, 0.0),
        ];
        let fixed = [
            [true, true, true],
            [true, true, true],
            [false, false, false],
        ];
        let model = ClothModel::new(
            rest,
            &[[0, 1, 2]],
            Membrane::orthotropic(5_000.0, 5_000.0, 0.0, 1_000.0, 1.0),
            0.0,
            0.2,
        )
        .unwrap();
        let mut s = Solver::new(model, x, &fixed, Vec3::ZERO);
        s.tol = 1e-6;
        s.integrator = integrator;
        s
    };

    let mut s_undamped = make_stiff(TimeIntegrator::GeneralizedAlpha { rho_inf: 1.0 });
    let mut s_damped = make_stiff(TimeIntegrator::GeneralizedAlpha { rho_inf: 0.0 });
    let e0 = total_energy(&s_undamped);
    assert!(e0 > 0.0);

    for _ in 0..20 {
        s_undamped.step(0.002).unwrap();
        s_damped.step(0.002).unwrap();
    }
    let e_undamped = total_energy(&s_undamped);
    let e_damped = total_energy(&s_damped);

    // At ω h ≈ 5.5 on nonlinear quartic Green strain, undamped midpoint (rho_inf=1.0) retains
    // mechanical energy within a small symplectic O(h²) oscillation (< 5%), while rho_inf=0.0
    // annihilates the stiff high-frequency mode (> 95% energy damped).
    assert!(
        (e_undamped - e0).abs() / e0 < 5e-2,
        "rho_inf=1.0 should conserve energy up to small symplectic oscillation: e0={e0:.6e}, e_end={e_undamped:.6e}"
    );
    assert!(
        e_damped < 0.05 * e0,
        "rho_inf=0.0 should strongly damp HF mode: e0={e0:.6e}, e_damped={e_damped:.6e}"
    );
}
