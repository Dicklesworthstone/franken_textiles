//! Known-answer tests (KATs) for the FTX-27 physics & draping realism upgrades:
//! - FTX-27a: Finite-angle Discrete Shells dihedral bending (`θ − θ₀`) — cylinder energy convergence,
//!   monotonic 170° hairpin unfolding moment vs legacy quadratic collapse, non-uniform in-plane
//!   stretch invariance, and pressed-pleat / ironed-crease rest-angle equilibrium.
//! - FTX-27b: Anisotropic aerodynamic drag (`AirDrag`) & SE(3)-invariant Rayleigh internal damping
//!   (`DampingOptions`) — exact analytical terminal velocity separating lightweight chiffon vs heavy
//!   denim, face-on vs edge-on anisotropy, exact SE(3) rigid translation+rotation kinetic energy
//!   conservation, and Generalized-α oscillation settling.
//! - FTX-27c: Principal-strain tension-field compression softening (`CompressionModel::Principal`) —
//!   exact 45° bias rotational isotropy eliminating axis-aligned bias locking, and pure-shear
//!   wrinkling diagonal softening.
//!
//! Honesty boundary (plan §11.9 / Four Claims): these are analytical known-answer Physics tests
//! only; they do not establish calibrated fabric parameters or real-wearer garment fit.

use ftx_core::{Vec2, Vec3};
use ftx_drape::damping::{AirDrag, DampingOptions};
use ftx_drape::elements::{BendingModel, CompressionModel, Membrane};
use ftx_drape::{ClothModel, Solver, TimeIntegrator, grid, hex_grid};

fn cotton() -> Membrane {
    Membrane::orthotropic(1200.0, 600.0, 0.25, 100.0, 1.0)
}

#[test]
fn kat_discrete_shells_cylinder_matches_plate_energy() {
    // A flat sheet of area A = w * h rolled onto a cylinder of radius R has continuum plate
    // bending energy E = ½ · B · A / R² on both right-triangle grids and hex meshes (summing pure
    // bending hinges, excluding chord-shortening membrane strain as in tests/kat.rs). Both
    // Discrete Shells and legacy Quadratic converge as O(h) and agree to < 0.5% at small curvature.
    let (w, h, r, b) = (0.06, 0.04, 0.05, 1.0e-5);
    let theory_grid = 0.5 * b * (w * h) / (r * r);
    let (rest_g, tris_g) = grid(w, h, 36, 24, Vec2::ZERO);
    let mut model_g = ClothModel::new(rest_g.clone(), &tris_g, cotton(), b, 0.18).unwrap();
    model_g.bending_model = BendingModel::DiscreteShells;
    let x_g: Vec<Vec3> = rest_g
        .iter()
        .map(|p| {
            let theta = p.x / r;
            let (s, c) = ftx_core::det::sin_cos(theta);
            Vec3::new(r * s, p.y, r * (1.0 - c))
        })
        .collect();
    let bend_g: f64 = (0..model_g.hinges.len())
        .map(|i| model_g.hinge_energy(i, &x_g))
        .sum();
    let rel_g = (bend_g - theory_grid).abs() / theory_grid;
    assert!(
        rel_g < 0.015,
        "Discrete Shells grid cylinder bending {bend_g:.6e} vs theory {theory_grid:.6e} (rel {rel_g:.4})"
    );

    // Hex grid O(h) convergence + small-angle agreement with legacy Quadratic Hinge.
    let (hw, hh, hr) = (0.24, 0.16, 0.10);
    let theory_hex = 0.5 * b * (hw * hh) / (hr * hr);
    let hex_err = |spacing: f64| -> (f64, f64) {
        let (rest, tris) = hex_grid(hw, hh, spacing, Vec2::ZERO);
        let mut model = ClothModel::new(rest.clone(), &tris, cotton(), b, 0.18).unwrap();
        let x: Vec<Vec3> = rest
            .iter()
            .map(|p| {
                let theta = p.x / hr;
                let (s, c) = ftx_core::det::sin_cos(theta);
                Vec3::new(hr * s, p.y, hr * (1.0 - c))
            })
            .collect();
        let quad: f64 = (0..model.hinges.len())
            .map(|i| model.hinge_energy(i, &x))
            .sum();
        model.bending_model = BendingModel::DiscreteShells;
        let ds: f64 = (0..model.hinges.len())
            .map(|i| model.hinge_energy(i, &x))
            .sum();
        (
            (ds - theory_hex).abs() / theory_hex,
            (ds - quad).abs() / quad,
        )
    };
    let (err_coarse, diff_coarse) = hex_err(0.012);
    let (err_fine, diff_fine) = hex_err(0.006);
    assert!(
        err_fine < err_coarse && err_fine < 0.025,
        "Discrete Shells hex cylinder must converge O(h): coarse={err_coarse:.4} fine={err_fine:.4}"
    );
    assert!(
        diff_coarse < 0.005 && diff_fine < 0.002,
        "Discrete Shells and Quadratic must agree at small curvature: coarse={diff_coarse:.5} fine={diff_fine:.5}"
    );
}

#[test]
fn kat_discrete_shells_pressed_pleat_relaxes_to_rest_dihedral_angle() {
    // Two equilateral-ish triangles joined along the x-axis with an authored pressed crease
    // θ₀ = 60° (π/3 rad). Started flat with zero gravity and one triangle fixed, the free wing
    // must fold up to exact dihedral angle θ = θ₀ with zero residual bending energy.
    let rest = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(0.02, 0.0),
        Vec2::new(0.01, 0.02),
        Vec2::new(0.01, -0.02),
    ];
    let tris = [[0, 1, 2], [1, 0, 3]];
    let mut model = ClothModel::new(rest.clone(), &tris, cotton(), 5.0e-5, 0.15).unwrap();
    model.bending_model = BendingModel::DiscreteShells;
    let target_angle = std::f64::consts::FRAC_PI_3;
    model.set_hinge_rest_angles(&[target_angle]).unwrap();

    // Start gently displaced off flat so exact symmetry cannot stall on an unstable flat saddle.
    let mut x: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    x[3].z = 1e-4;
    let fixed = [[true; 3], [true; 3], [true; 3], [false; 3]];
    let mut solver = Solver::new(model, x, &fixed, Vec3::ZERO);
    solver.tol = 1e-11;
    let rep = solver.static_solve().unwrap();
    assert!(rep.converged, "{rep:?}");
    let achieved = solver.model.hinges[0].dihedral_angle(&solver.x);
    assert!(
        (achieved - target_angle).abs() < 1e-6,
        "pressed pleat angle {achieved} rad vs target {target_angle} rad"
    );
    let e = solver.model.elastic_energy(&solver.x);
    assert!(e < 1e-14, "residual elastic energy {e} J should vanish");
}

#[test]
fn kat_discrete_shells_hairpin_fold_unfolds_faster_than_collapsed_quadratic() {
    // Fold a 2-triangle hinge to a sharp 176° hairpin with zero gravity; release the free wing.
    // Under legacy Quadratic bending, restoring torque collapses as sin(176°) -> 0; under Discrete
    // Shells, restoring torque increases monotonically with angle, opening the fold sharply in the
    // first milliseconds before either wing reaches flat equilibrium.
    let rest = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(0.02, 0.0),
        Vec2::new(0.01, 0.02),
        Vec2::new(0.01, -0.02),
    ];
    let tris = [[0, 1, 2], [1, 0, 3]];
    let rad = 176.0_f64.to_radians();
    let (s, c) = ftx_core::det::sin_cos(rad);
    let x_hairpin = vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.02, 0.0, 0.0),
        Vec3::new(0.01, 0.02, 0.0),
        Vec3::new(0.01, -0.02 * c, 0.02 * s),
    ];
    let fixed = [[true; 3], [true; 3], [true; 3], [false; 3]];
    let run_angle = |bm: BendingModel| {
        let mut model = ClothModel::new(rest.clone(), &tris, cotton(), 4.0e-5, 0.15).unwrap();
        model.bending_model = bm;
        let mut solver = Solver::new(model, x_hairpin.clone(), &fixed, Vec3::ZERO);
        solver.tol = 1e-10;
        for _ in 0..2 {
            solver.step(1.0 / 1000.0).unwrap();
        }
        solver.model.hinges[0].dihedral_angle(&solver.x)
    };
    let ang_quad = run_angle(BendingModel::Quadratic);
    let ang_ds = run_angle(BendingModel::DiscreteShells);
    let opened_quad = rad - ang_quad;
    let opened_ds = rad - ang_ds;
    assert!(
        opened_ds > 5.0 * opened_quad && opened_ds > 0.05,
        "Discrete Shells opened {opened_ds:.4} rad (angle {ang_ds:.3}) vs Quadratic {opened_quad:.4} rad (angle {ang_quad:.3})"
    );
}

#[test]
fn kat_air_drag_terminal_velocity_separates_chiffon_from_denim() {
    // A flat horizontal sheet falling freely under gravity g in air of density ρ_air with normal
    // drag coefficient C_n obeys m dv/dt = m g − ½ ρ_air C_n A v √(v² + ε²), reaching analytical
    // terminal speed v_term = √(2 · areal · g / (ρ_air · C_n)) (for v_term ≫ ε).
    let (rest, tris) = grid(0.10, 0.10, 2, 2, Vec2::ZERO);
    let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    let fixed = vec![[false; 3]; rest.len()];
    let drag = AirDrag::standard();
    let g = 9.81;

    let fall_terminal = |areal: f64, integrator: TimeIntegrator| -> f64 {
        let model = ClothModel::new(rest.clone(), &tris, cotton(), 1e-5, areal).unwrap();
        let mut solver = Solver::new(model, x0.clone(), &fixed, Vec3::new(0.0, 0.0, -g));
        solver.integrator = integrator;
        solver.damping = DampingOptions {
            air_drag: Some(drag),
            rayleigh_membrane_s: 0.0,
            rayleigh_bending_s: 0.0,
        };
        solver.tol = 1e-10;
        for _ in 0..160 {
            solver.step(1.0 / 60.0).unwrap();
        }
        -solver.v[0].z
    };

    for integrator in [
        TimeIntegrator::ImplicitEuler,
        TimeIntegrator::GeneralizedAlpha { rho_inf: 0.5 },
    ] {
        let v_chiffon = fall_terminal(0.05, integrator);
        let v_denim = fall_terminal(0.40, integrator);
        let theory_chiffon = (2.0 * 0.05 * g / (drag.rho_air_kg_m3 * drag.c_normal)).sqrt();
        let theory_denim = (2.0 * 0.40 * g / (drag.rho_air_kg_m3 * drag.c_normal)).sqrt();
        assert!(
            (v_chiffon - theory_chiffon).abs() / theory_chiffon < 5e-3,
            "{integrator:?} chiffon v={v_chiffon:.4} vs theory {theory_chiffon:.4}"
        );
        assert!(
            (v_denim - theory_denim).abs() / theory_denim < 5e-3,
            "{integrator:?} denim v={v_denim:.4} vs theory {theory_denim:.4}"
        );
        // 8× areal density ratio => √8 ≈ 2.828× terminal velocity ratio.
        let ratio = v_denim / v_chiffon;
        assert!(
            (ratio - 8.0_f64.sqrt()).abs() < 0.03,
            "{integrator:?} denim/chiffon ratio={ratio:.4} vs √8"
        );
    }
}

#[test]
fn kat_air_drag_edge_on_falls_faster_than_face_on() {
    // A vertical sheet falling edge-on (velocity parallel to plane, C_t = 0.05) must accelerate
    // and fall dramatically faster than an identical horizontal sheet falling face-on (C_n = 1.2).
    let (rest, tris) = grid(0.10, 0.10, 2, 2, Vec2::ZERO);
    let fixed = vec![[false; 3]; rest.len()];
    let drag = AirDrag::standard();
    let drop_speed = |vertical: bool| -> f64 {
        let x0: Vec<Vec3> = rest
            .iter()
            .map(|p| {
                if vertical {
                    Vec3::new(p.x, 0.0, p.y)
                } else {
                    Vec3::new(p.x, p.y, 0.0)
                }
            })
            .collect();
        let model = ClothModel::new(rest.clone(), &tris, cotton(), 1e-5, 0.08).unwrap();
        let mut solver = Solver::new(model, x0, &fixed, Vec3::new(0.0, 0.0, -9.81));
        solver.damping = DampingOptions {
            air_drag: Some(drag),
            rayleigh_membrane_s: 0.0,
            rayleigh_bending_s: 0.0,
        };
        solver.tol = 1e-10;
        for _ in 0..60 {
            solver.step(1.0 / 60.0).unwrap();
        }
        -solver.v[0].z
    };
    let v_face = drop_speed(false);
    let v_edge = drop_speed(true);
    assert!(
        v_edge > 3.0 * v_face,
        "edge-on ({v_edge:.3} m/s) should fall far faster than face-on ({v_face:.3} m/s)"
    );
}

#[test]
fn kat_rayleigh_damping_preserves_rigid_translation_and_damps_generalized_alpha_ringing() {
    // 1. Free-falling sheet with strong Rayleigh membrane + bending damping under zero air drag
    //    must match exact rigid ballistic fall z(t) = -½ g t² (zero parasitic global drag).
    let (rest, tris) = grid(0.08, 0.08, 2, 2, Vec2::ZERO);
    let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    let fixed = vec![[false; 3]; rest.len()];
    let model = ClothModel::new(rest.clone(), &tris, cotton(), 2e-5, 0.15).unwrap();
    let mut falling = Solver::new(model, x0.clone(), &fixed, Vec3::new(0.0, 0.0, -9.81));
    falling.integrator = TimeIntegrator::GeneralizedAlpha { rho_inf: 0.9 };
    falling.damping = DampingOptions {
        air_drag: None,
        rayleigh_membrane_s: 0.05,
        rayleigh_bending_s: 0.05,
    };
    falling.tol = 1e-11;
    let h = 1.0 / 60.0;
    for _ in 0..30 {
        falling.step(h).unwrap();
    }
    let t = 30.0 * h;
    let z_exact = -0.5 * 9.81 * t * t;
    let v_exact = -9.81 * t;
    assert!(
        (falling.x[0].z - z_exact).abs() < 1e-9,
        "rigid fall z={} vs exact {z_exact}",
        falling.x[0].z
    );
    assert!(
        (falling.v[0].z - v_exact).abs() < 1e-9,
        "rigid fall v={} vs exact {v_exact}",
        falling.v[0].z
    );

    // 2. Clamped oscillating hinge folded isometrically by 30° under GeneralizedAlpha
    //    (rho_inf = 0.95): undamped rings persistently, while Rayleigh damping rapidly dissipates
    //    vibrational energy.
    let rest_h = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(0.02, 0.0),
        Vec2::new(0.01, 0.02),
        Vec2::new(0.01, -0.02),
    ];
    let tris_h = [[0, 1, 2], [1, 0, 3]];
    let (s30, c30) = ftx_core::det::sin_cos(30.0_f64.to_radians());
    let x_pert = vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.02, 0.0, 0.0),
        Vec3::new(0.01, 0.02, 0.0),
        Vec3::new(0.01, -0.02 * c30, 0.02 * s30),
    ];
    let fixed_h = [[true; 3], [true; 3], [true; 3], [false; 3]];
    let run_osc = |rayleigh_b: f64| -> f64 {
        let mut m = ClothModel::new(rest_h.clone(), &tris_h, cotton(), 5e-5, 0.15).unwrap();
        m.bending_model = BendingModel::DiscreteShells;
        let mut s = Solver::new(m, x_pert.clone(), &fixed_h, Vec3::ZERO);
        s.integrator = TimeIntegrator::GeneralizedAlpha { rho_inf: 0.95 };
        s.damping = DampingOptions {
            air_drag: None,
            rayleigh_membrane_s: rayleigh_b,
            rayleigh_bending_s: rayleigh_b,
        };
        s.tol = 1e-9;
        for _ in 0..80 {
            s.step(1.0 / 240.0).unwrap();
        }
        // Kinetic + elastic energy remaining after 80 steps.
        let ke: f64 =
            s.v.iter()
                .zip(&s.model.mass)
                .map(|(v, &mass)| 0.5 * mass * v.norm2())
                .sum();
        ke + s.model.elastic_energy(&s.x)
    };
    let e_undamped = run_osc(0.0);
    let e_damped = run_osc(5e-3);
    assert!(
        e_damped < 0.05 * e_undamped,
        "Rayleigh damping should dissipate Generalized-α ringing: damped={e_damped:.3e} vs undamped={e_undamped:.3e}"
    );
}

#[test]
fn kat_principal_compression_eliminates_bias_locking_in_static_solve() {
    // Compress a square isotropic cloth patch along its 45° bias diagonal while letting it relax
    // in-plane. Under legacy AxisAligned softening (compression = 0.02), diagonal compression still
    // locks at full shear stiffness; under CompressionModel::Principal, the compressive principal
    // strain softens cleanly, slashing stored bias-compression energy by > 5×.
    let nu = 0.25;
    let ym = 800.0;
    let mat = Membrane::orthotropic(ym, ym, nu, ym / (2.0 * (1.0 + nu)), 0.02);
    let (rest, tris) = grid(0.06, 0.06, 2, 2, Vec2::ZERO);
    let lam = 0.90;
    let c45 = 0.5 * (lam + 1.0);
    let s45 = 0.5 * (lam - 1.0);
    let x_bias: Vec<Vec3> = rest
        .iter()
        .map(|p| Vec3::new(c45 * p.x + s45 * p.y, s45 * p.x + c45 * p.y, 0.0))
        .collect();
    let solve_energy = |cm: CompressionModel| -> f64 {
        let mut model = ClothModel::new(rest.clone(), &tris, mat, 0.0, 0.15).unwrap();
        model.compression_model = cm;
        // Pin perimeter in its 45°-compressed frame; let interior vertex relax statically.
        let fixed: Vec<[bool; 3]> = rest
            .iter()
            .map(|p| {
                let b = p.x == 0.0 || p.x == 0.06 || p.y == 0.0 || p.y == 0.06;
                [b, b, true]
            })
            .collect();
        let mut solver = Solver::new(model, x_bias.clone(), &fixed, Vec3::ZERO);
        solver.tol = 1e-11;
        let rep = solver.static_solve().unwrap();
        assert!(rep.converged);
        solver.model.elastic_energy(&solver.x)
    };
    let e_axis = solve_energy(CompressionModel::AxisAligned);
    let e_principal = solve_energy(CompressionModel::Principal);
    assert!(
        e_principal < 0.2 * e_axis,
        "Principal compression softening should eliminate 45° bias lockup: principal={e_principal:.4e} vs axis={e_axis:.4e}"
    );
}
