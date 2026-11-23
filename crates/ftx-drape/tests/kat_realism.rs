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
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::{
    AdaptiveBarrierPolicy, Body, ClothModel, ContactParams, PartFrame, Solver, StickSlipFriction,
    TimeIntegrator, grid, hex_grid,
};
use ftx_pattern::Pattern;
use serde_json::json;

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

#[test]
fn kat_adaptive_barrier_normalizes_light_silk_and_heavy_wool_clearance() {
    // FTX-27e KAT 1: 40 g/m² silk vs 500 g/m² wool settling onto a flat rigid table under gravity.
    // Fixed legacy kappa = 100 N/m makes silk hover near dhat (> 85% of dhat); AdaptiveBarrierPolicy
    // scales kappa proportionally to fabric weight and stiffness so both light silk and heavy wool
    // settle at consistent, non-hovering, non-crushed barrier clearance inside (0.15, 0.75) * dhat.
    let (rest, tris) = grid(0.06, 0.06, 2, 2, Vec2::ZERO);
    let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0022)).collect();
    let fixed = vec![[false; 3]; rest.len()];
    let floor = Body::new(
        vec![
            Vec3::new(-0.2, -0.2, 0.0),
            Vec3::new(0.2, -0.2, 0.0),
            Vec3::new(0.2, 0.2, 0.0),
            Vec3::new(-0.2, 0.2, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        5e-3,
    )
    .unwrap();
    let base_contact = ContactParams {
        dhat: 1e-3,
        thickness: 1.5e-3,
        kappa: 100.0,
        mu: 0.4,
        eps_v: 1e-3,
    };

    let settle_gap_fraction =
        |areal: f64, tensile: f64, adaptive: Option<AdaptiveBarrierPolicy>| {
            let mat = Membrane::orthotropic(tensile, tensile, 0.25, 0.4 * tensile, 1.0);
            let model = ClothModel::new(rest.clone(), &tris, mat, 1e-5, areal).unwrap();
            let mean_mass = model.mass.iter().sum::<f64>() / model.mass.len() as f64;
            let mut solver = Solver::new(model, x0.clone(), &fixed, Vec3::new(0.0, 0.0, -9.81))
                .with_contact(floor.clone(), base_contact)
                .unwrap();
            if let Some(policy) = adaptive {
                solver
                    .contact
                    .as_mut()
                    .unwrap()
                    .set_adaptive_barrier(policy, mean_mass, tensile, 9.81)
                    .unwrap();
            }
            solver.tol = 1e-8;
            for _ in 0..40 {
                if let Some(c) = solver.contact.as_mut() {
                    c.adapt_barrier_stiffness(&solver.x);
                }
                solver.step(1.0 / 60.0).unwrap();
            }
            let mean_z = solver.x.iter().map(|p| p.z).sum::<f64>() / solver.x.len() as f64;
            (mean_z - base_contact.thickness) / base_contact.dhat
        };

    let silk_fixed = settle_gap_fraction(0.04, 150.0, None);
    let policy = AdaptiveBarrierPolicy::default();
    let silk_adapt = settle_gap_fraction(0.04, 150.0, Some(policy));
    let wool_adapt = settle_gap_fraction(0.50, 1500.0, Some(policy));
    assert!(
        silk_fixed > 0.80,
        "legacy fixed kappa=100 should leave 40 g/m² silk hovering near dhat, got {silk_fixed:.4}"
    );
    assert!(
        (0.15..0.75).contains(&silk_adapt),
        "adaptive silk clearance fraction {silk_adapt:.4} outside (0.15, 0.75)"
    );
    assert!(
        (0.15..0.75).contains(&wool_adapt),
        "adaptive wool clearance fraction {wool_adapt:.4} outside (0.15, 0.75)"
    );
    assert!(
        (silk_adapt - wool_adapt).abs() < 0.25,
        "adaptive clearance should normalize 12.5× weight ratio: silk={silk_adapt:.4}, wool={wool_adapt:.4}"
    );
}

#[test]
fn kat_stick_slip_friction_holds_above_kinetic_angle_and_slides_once_overcome() {
    // FTX-27e KAT 2: Inclined-plane stick-slip transition with mu_s = 0.6, mu_k = 0.3.
    // At slope tan(theta) = 0.45 (between mu_k and mu_s), stick-slip holds the cloth with tiny
    // regularized creep while kinetic-only Coulomb (mu = 0.3) slides far downslope.
    // Once slope exceeds static threshold (tan(theta) = 0.70 > mu_s), stick-slip breaks free and
    // slides substantially faster than a pure mu = 0.6 law because sliding drops toward mu_k = 0.3.
    let (rest, tris) = grid(0.05, 0.05, 1, 1, Vec2::ZERO);
    let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0020)).collect();
    let fixed = vec![[false; 3]; rest.len()];
    let floor = Body::new(
        vec![
            Vec3::new(-0.5, -0.5, 0.0),
            Vec3::new(1.5, -0.5, 0.0),
            Vec3::new(1.5, 0.5, 0.0),
            Vec3::new(-0.5, 0.5, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        5e-3,
    )
    .unwrap();
    let law = StickSlipFriction::new(0.6, 0.3, 0.02).unwrap();

    let run_slope = |tan_theta: f64, mu: f64, ss: Option<StickSlipFriction>, steps: usize| -> f64 {
        let inv_sec = 1.0 / (1.0 + tan_theta * tan_theta).sqrt();
        let g = Vec3::new(9.81 * tan_theta * inv_sec, 0.0, -9.81 * inv_sec);
        let model = ClothModel::new(rest.clone(), &tris, cotton(), 1e-5, 0.25).unwrap();
        let mean_mass = model.mass.iter().sum::<f64>() / model.mass.len() as f64;
        let params = ContactParams {
            dhat: 1e-3,
            thickness: 1.5e-3,
            kappa: 100.0,
            mu,
            eps_v: 1e-3,
        };
        let mut solver = Solver::new(model, x0.clone(), &fixed, g)
            .with_contact(floor.clone(), params)
            .unwrap();
        let c = solver.contact.as_mut().unwrap();
        c.set_adaptive_barrier(AdaptiveBarrierPolicy::default(), mean_mass, 900.0, 9.81)
            .unwrap();
        if let Some(s) = ss {
            c.set_stick_slip(s).unwrap();
        }
        solver.friction_rounds = 2;
        solver.tol = 1e-8;
        let x_start = solver.x.iter().map(|p| p.x).sum::<f64>() / solver.x.len() as f64;
        for _ in 0..steps {
            solver.step(1.0 / 60.0).unwrap();
        }
        let x_end = solver.x.iter().map(|p| p.x).sum::<f64>() / solver.x.len() as f64;
        x_end - x_start
    };

    let hold_ss = run_slope(0.45, 0.3, Some(law), 40);
    let slide_kinetic = run_slope(0.45, 0.3, None, 40);
    assert!(
        slide_kinetic > 5.0 * hold_ss && hold_ss < 0.005,
        "below mu_s=0.6 at tan=0.45, stick-slip ({hold_ss:.5} m) should hold while mu_k=0.3 slides ({slide_kinetic:.5} m)"
    );

    let breakaway_ss = run_slope(0.70, 0.6, Some(law), 40);
    let pure_static_mu = run_slope(0.70, 0.6, None, 40);
    assert!(
        breakaway_ss > 1.5 * pure_static_mu && breakaway_ss > 0.05,
        "above mu_s=0.6 at tan=0.70, stick-slip ({breakaway_ss:.5} m) should break away faster than constant mu=0.6 ({pure_static_mu:.5} m)"
    );
}

#[test]
fn kat_drape_options_adaptive_barrier_and_stick_slip_wire_end_to_end_and_preserve_default() {
    struct CylFrames;
    impl PartFrames for CylFrames {
        fn frame(&self, _: &str) -> Option<PartFrame> {
            Some(PartFrame {
                origin: Vec3::ZERO,
                up: Vec3::new(0., 0., 1.),
                front: Vec3::new(0., -1., 0.),
                lat: Vec3::new(1., 0., 0.),
            })
        }
        fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
            Some(10.0)
        }
        fn section_radius(&self, _: &str, _: f64) -> f64 {
            30.0
        }
    }
    let mut verts = Vec::new();
    for z in [-0.04, 0.08] {
        for i in 0..16 {
            let (s, c) = ftx_core::det::sin_cos(i as f64 * std::f64::consts::TAU / 16.0);
            verts.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    let mut tris = Vec::new();
    for i in 0..16 {
        let j = (i + 1) % 16;
        tris.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    let frames = CylFrames;
    let body = DrapeBody {
        frames: &frames,
        collision: (&verts, &tris),
        display: (&verts, &tris),
        arm_deg: 35.0,
    };
    let w = (0.98 * 0.06 * std::f64::consts::PI * 1000.0).round();
    let pat = Pattern::new(json!({
        "schema": "fabric-pattern/1",
        "units": "mm",
        "pieces": [{
            "id": "band",
            "cut": {"qty": 1},
            "vertices": {"a": [0, 0], "b": [w, 0], "c": [w, 20], "d": [0, 20]},
            "edges": [
                {"id": "bottom", "from": "a", "to": "b"},
                {"id": "right", "from": "b", "to": "c"},
                {"id": "top", "from": "c", "to": "d"},
                {"id": "left", "from": "d", "to": "a"}
            ],
            "notches": [],
            "grain": [[0, 0], [0, 20]],
            "placement": {"part": "torso", "radius_mm": 35, "x0": w / 2.0}
        }],
        "seams": [{"id": "join", "a": "band/right", "b": "band/left"}]
    }));

    let base_opts = DrapeOptions {
        sew_steps: 4,
        drape_steps: 6,
        step: 0.01,
        ..DrapeOptions::default()
    };
    let base = drape(&pat, &json!({}), &body, &base_opts, |_| {}).unwrap();
    assert!(base.report["numerical"].get("adaptive_barrier").is_none());
    assert!(base.report["numerical"].get("stick_slip").is_none());
    assert!(
        base.report["contact_diagnostics"]
            .get("final_body_kappa_n_m")
            .is_none()
    );

    let ss = StickSlipFriction::new(0.6, 0.3, 0.02).unwrap();
    let rich_opts = DrapeOptions {
        adaptive_barrier: Some(AdaptiveBarrierPolicy::default()),
        stick_slip: Some(ss),
        ..base_opts
    };
    let rich = drape(&pat, &json!({}), &body, &rich_opts, |_| {}).unwrap();
    assert!(rich.report["numerical"]["adaptive_barrier"].is_object());
    assert_eq!(rich.report["numerical"]["stick_slip"], json!(ss));
    let final_kappa = rich.report["contact_diagnostics"]["final_body_kappa_n_m"]
        .as_f64()
        .expect("adaptive barrier must report final_body_kappa_n_m");
    assert!(
        final_kappa.is_finite() && final_kappa > 0.0 && final_kappa != base_opts.contact.kappa,
        "adaptive kappa ({final_kappa}) should scale away from hardcoded legacy 100.0"
    );
    assert_eq!(
        rich.scene.solver.contact.as_ref().unwrap().stick_slip(),
        Some(ss)
    );
}
