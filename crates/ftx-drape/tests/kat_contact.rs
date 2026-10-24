//! Known-answer tests for contact and friction (plan §11.9 items 4, 5, 6; item 1 is a unit test in
//! `contact.rs`). Bodies are surfaces of revolution; garments are welded hex tubes.

use std::f64::consts::PI;

use ftx_core::Vec3;
use ftx_core::det::sin_cos;
use ftx_drape::{Body, ClothModel, ContactParams, Membrane, Solver, hex_tube};

const E: f64 = 1000.0; // hoop and axial modulus (N/m)

/// A surface of revolution r(z) between z0 and z1 (open ends), `segs` around.
fn revolution(radius: impl Fn(f64) -> f64, z0: f64, z1: f64, rings: usize, segs: usize) -> Body {
    let mut v = Vec::new();
    for k in 0..=rings {
        let z = z0 + (z1 - z0) * k as f64 / rings as f64;
        let r = radius(z);
        for j in 0..segs {
            // det::sin_cos, not the platform libm, so the body (and the pinned digest) is the same on
            // every target.
            let (s, c) = sin_cos(2.0 * PI * j as f64 / segs as f64);
            v.push(Vec3::new(r * c, r * s, z));
        }
    }
    let mut tris = Vec::new();
    for k in 0..rings {
        for j in 0..segs {
            let (a, b) = (k * segs + j, k * segs + (j + 1) % segs);
            let (c, d) = (a + segs, b + segs);
            tris.push([a, b, d]);
            tris.push([a, d, c]);
        }
    }
    Body::new(v, tris, 2e-3).unwrap()
}

fn params(mu: f64) -> ContactParams {
    // ĝ ≈ 1e-3 of the scene, as IPC recommends: clearances are exact to within ĝ.
    ContactParams {
        dhat: 2e-4,
        thickness: 5e-4,
        kappa: 10.0,
        mu,
        eps_v: 1e-3,
    }
}

/// A band of rest circumference `c` and height `h`, each vertex placed at radius `start(z)` with its
/// bottom at `z0`; z of the bottom row fixed when `hold_bottom`, one vertex pinned tangentially.
fn band(
    c: f64,
    h: f64,
    spacing: f64,
    z0: f64,
    start: impl Fn(f64) -> f64,
    rho: f64,
    hold_bottom: bool,
) -> (Solver, Vec<usize>) {
    let (n, tris, tri_rest, rest) = hex_tube(c, h, spacing);
    let model = ClothModel::from_triangles(
        n,
        &tris,
        &tri_rest,
        Membrane::orthotropic(E, E, 0.0, 100.0, 0.05),
        1e-5,
        rho,
    )
    .unwrap();
    let x: Vec<Vec3> = rest
        .iter()
        .map(|p| {
            let z = z0 + p.y;
            let ((s, co), r) = (sin_cos(2.0 * PI * p.x / c), start(z));
            Vec3::new(r * co, r * s, z)
        })
        .collect();
    let mut fixed = vec![[false; 3]; n];
    let bottom: Vec<usize> = (0..n).filter(|&i| rest[i].y == 0.0).collect();
    if hold_bottom {
        for &i in &bottom {
            fixed[i][2] = true;
        }
    }
    fixed[bottom[0]][1] = true; // no spin about the axis (vertex 0 sits at angle 0, so y is tangential)
    (Solver::new(model, x, &fixed, Vec3::ZERO), bottom)
}

fn mean_radius(x: &[Vec3]) -> f64 {
    x.iter()
        .map(|p| (p.x * p.x + p.y * p.y).sqrt())
        .sum::<f64>()
        / x.len() as f64
}

fn mean_z(x: &[Vec3]) -> f64 {
    x.iter().map(|p| p.z).sum::<f64>() / x.len() as f64
}

/// KAT 6: on a torso tube of girth G, a garment tube with negative ease settles onto the body at the
/// thickness-offset radius with hoop strain ≈ (2π(R + ξ) − C)/C, never inside it and with no collider
/// inset; one with positive ease keeps its clearance (C − G)/2π − ξ.
#[test]
#[cfg_attr(debug_assertions, ignore = "140 s unoptimised; run with --release")]
fn ease_sweep_on_a_tube() {
    let r = 0.08;
    let g = 2.0 * PI * r;
    let body = revolution(|_| r, -0.05, 0.10, 15, 192);
    let p = params(0.0);
    for ease in [-0.010, -0.020] {
        let c = g + ease;
        let (solver, _) = band(c, 0.04, 0.008, 0.0, |_| r + p.thickness + 0.8e-3, 0.2, true);
        let mut s = solver.with_contact(body.clone(), p).unwrap();
        s.tol = 1e-7; // 0.1 µm: the barrier's projected Hessian converges linearly at the end
        s.max_newton = 300;
        let rep = s.static_solve().unwrap();
        let rbar = mean_radius(&s.x);
        let gap = s.contact.as_ref().unwrap().min_gap(&s.x, 5e-3).unwrap();
        let hoop = (2.0 * PI * rbar - c) / c;
        let want = (2.0 * PI * (r + p.thickness) - c) / c;
        println!(
            "ease {:+.0} mm: {} its ({} projected), mean radius {:.3} mm (body {:.1} + ξ {:.1}), min gap {:.3} mm, hoop strain {:.4} (≈ {:.4})",
            ease * 1e3,
            rep.iterations,
            rep.projected_iterations,
            rbar * 1e3,
            r * 1e3,
            p.thickness * 1e3,
            gap * 1e3,
            hoop,
            want
        );
        assert!(rep.converged, "{rep:?}");
        assert!(gap > 0.0, "never inside the offset surface");
        // The tube's flat triangles sit ~0.1 mm inside its vertices (a 62-gon), so allow that on top of ĝ.
        assert!(
            rbar - r - p.thickness > 0.0 && rbar - r - p.thickness < p.dhat + 0.15e-3,
            "rests within the barrier band"
        );
        assert!((hoop - want).abs() < 0.003, "hoop strain {hoop} vs {want}");
    }
    let c = g + 0.030;
    // Refine this unconstrained ring so polygonal chord error stays below the same 20 µm
    // continuum-clearance bound; at 8 mm spacing the converged discretization differs by 29 µm.
    let (solver, bottom) = band(c, 0.04, 0.004, 0.0, |_| c / (2.0 * PI), 0.2, true);
    // With no contact, the static tube has rigid in-plane translations and rotation. Fix their
    // gauge explicitly; the single tangential pin in band() leaves a singular static Hessian.
    let mut fixed = vec![[false; 3]; solver.x.len()];
    for &i in &bottom {
        fixed[i][2] = true;
    }
    fixed[bottom[0]][0] = true;
    fixed[bottom[0]][1] = true;
    fixed[bottom[bottom.len() / 2]][1] = true;
    let solver = Solver::new(solver.model, solver.x, &fixed, Vec3::ZERO);
    let mut s = solver.with_contact(body, p).unwrap();
    // The loose tube's bending relaxation is slow; budget iterations rather than accepting an
    // exhausted solve. Keep the default displacement tolerance and the analytic clearance check.
    s.max_newton = 1000;
    let rep = s.static_solve().unwrap();
    let clearance = mean_radius(&s.x) - r - p.thickness;
    let want = (c - g) / (2.0 * PI) - p.thickness;
    println!(
        "ease +30 mm: {} Newton iterations, clearance {:.4} mm vs {:.4} mm",
        rep.iterations,
        clearance * 1e3,
        want * 1e3
    );
    assert!(
        (clearance - want).abs() < 2e-5,
        "a loose tube keeps its clearance"
    );
}

/// Mean axial travel (m) of a band hanging a weight `w_fraction · 2πμT` on a cylinder, over `t` s.
fn band_slip_on_cylinder(mu: f64, w_fraction: f64) -> (f64, f64) {
    let r = 0.08;
    let body = revolution(|_| r, -0.25, 0.10, 35, 192);
    let p = params(mu);
    let c = 2.0 * PI * r - 0.02;
    let (h, rho) = (0.02, 1.0);
    // Relax onto the body without friction or gravity, bottom row held.
    let (solver, _) = band(c, h, 0.008, 0.0, |_| r + p.thickness + 0.8e-3, rho, true);
    let mut s = solver
        .with_contact(body.clone(), ContactParams { mu: 0.0, ..p })
        .unwrap();
    s.tol = 1e-9;
    s.max_newton = 200;
    s.static_solve().unwrap();
    let relaxed = s.x.clone();
    let tension = E * h * (2.0 * PI * mean_radius(&relaxed) - c) / c;
    let capacity = 2.0 * PI * mu * tension;
    let mass: f64 = s.model.mass.iter().sum();
    // Free to slide, now with friction and a weight.
    let (mut s, _) = band(c, h, 0.008, 0.0, |_| r, rho, false);
    s.x = relaxed;
    s.gravity = Vec3::new(0.0, 0.0, -w_fraction * capacity / mass);
    let mut s = s.with_contact(body, p).unwrap();
    let z0 = mean_z(&s.x);
    for _ in 0..10 {
        s.step(0.002).unwrap();
    }
    let contact = s.contact.as_ref().unwrap();
    let sample = contact.accepted_sample(&s.x);
    assert!(sample.body.unwrap().minimum_active_gap_m.unwrap() > 0.0);
    assert!(sample.self_contact.is_none());
    let map = contact.reaction_map(&s.x).unwrap();
    let forces: Vec<Vec3> = serde_json::from_value(map["body_force_on_cloth_n"].clone()).unwrap();
    // Continuum cylinder normals are radial. Discrete closest features/edge mollifiers can
    // also contribute axially: check their full energy derivative, rather than asserting zero.
    let radial: f64 = forces
        .iter()
        .zip(&s.x)
        .map(|(f, x)| (f.x * x.x + f.y * x.y) / (x.x * x.x + x.y * x.y).sqrt())
        .sum();
    assert!(radial > 0.0);
    let eps = 1e-7;
    let energy = |dz| {
        let shifted: Vec<_> = s.x.iter().map(|p| *p + Vec3::new(0., 0., dz)).collect();
        contact.reaction_map(&shifted).unwrap()["barrier_energy_j"]
            .as_f64()
            .unwrap()
    };
    let axial = forces.iter().map(|f| f.z).sum::<f64>();
    let difference = (energy(-eps) - energy(eps)) / (2. * eps);
    assert!(
        (axial - difference).abs() < 1e-6 * radial,
        "axial {axial}, finite difference {difference}, radial {radial}"
    );
    assert!(map["pressure_pa"].is_null());
    (mean_z(&s.x) - z0, capacity)
}

/// KAT 4: a ring of tension T on a cylinder presses 2πT in total, so it holds a hanging weight W iff
/// W < 2πμT (checked at 0.9 and 1.1 of the threshold).
#[test]
#[cfg_attr(debug_assertions, ignore = "dynamic contact; run with --release")]
fn ring_on_a_cylinder_holds_below_two_pi_mu_t() {
    let mu = 0.4;
    let (hold, capacity) = band_slip_on_cylinder(mu, 0.9);
    let (slide, _) = band_slip_on_cylinder(mu, 1.1);
    println!(
        "ring on cylinder, μ = {mu}, 2πμT = {capacity:.3} N: travel at 0.9 W* {:.4} mm, at 1.1 W* {:.4} mm",
        hold * 1e3,
        slide * 1e3
    );
    assert!(hold.abs() < 0.2e-3, "holds below the threshold");
    assert!(slide < -1.0e-3, "slides above it");
}

/// Mean axial travel of a tight band on a cone r = r₀ + z·tan α (narrowing downward), no gravity.
fn band_slip_on_cone(mu: f64, tan_alpha: f64) -> f64 {
    let r0 = 0.08;
    let radius = move |z: f64| r0 + z * tan_alpha;
    let body = revolution(radius, -0.06, 0.03, 30, 192);
    let p = params(mu);
    let h = 0.012;
    let c = 2.0 * PI * radius(h / 2.0) - 0.015;
    let cos_a = 1.0 / (1.0 + tan_alpha * tan_alpha).sqrt();
    let (solver, _) = band(
        c,
        h,
        0.006,
        0.0,
        |z| radius(z) + (p.thickness + 0.8e-3) / cos_a,
        0.2,
        false,
    );
    let mut s = solver.with_contact(body, p).unwrap();
    // Projected contact steps can converge linearly near the friction threshold.
    s.max_newton = 1000;
    let z0 = mean_z(&s.x);
    for _ in 0..15 {
        s.step(0.002).unwrap();
    }
    mean_z(&s.x) - z0
}

/// KAT 5: a tight ring on a cone narrowing downward slides iff tan α > μ, whatever its tension.
#[test]
#[cfg_attr(debug_assertions, ignore = "dynamic contact; run with --release")]
fn ring_on_a_cone_slides_iff_tan_alpha_exceeds_mu() {
    let mu = 0.4;
    let hold = band_slip_on_cone(mu, 0.5 * mu);
    let slide = band_slip_on_cone(mu, 2.0 * mu);
    println!(
        "ring on cone, μ = {mu}: travel at tan α = μ/2 {:.4} mm, at tan α = 2μ {:.4} mm",
        hold * 1e3,
        slide * 1e3
    );
    assert!(hold.abs() < 0.3e-3, "holds when tan α < μ");
    assert!(slide < -1.0e-3, "slides down when tan α > μ");
}

/// Contact is deterministic across targets (plan §11.9 item 9): a band relaxing onto a cylinder, then
/// five frictional steps, pinned by the SHA-256 of the final positions.
#[test]
fn contact_digest_is_pinned() {
    let r = 0.03;
    let body = revolution(|_| r, -0.02, 0.03, 6, 64);
    let p = params(0.3);
    let c = 2.0 * PI * r - 0.004;
    let (solver, _) = band(
        c,
        0.01,
        0.006,
        0.0,
        |_| r + p.thickness + 0.4e-3,
        0.2,
        false,
    );
    let mut s = solver.with_contact(body, p).unwrap();
    s.gravity = Vec3::new(0.0, 0.0, -9.81);
    for _ in 0..5 {
        s.step(0.004).unwrap();
    }
    let bytes: Vec<u8> =
        s.x.iter()
            .flat_map(|q| [q.x, q.y, q.z])
            .flat_map(f64::to_le_bytes)
            .collect();
    let digest = ftx_core::receipt::sha256_hex(&bytes);
    println!("contact digest {digest}");
    assert!(s.contact.as_ref().unwrap().min_gap(&s.x, 1e-3).unwrap() > 0.0);
    // Re-pinned when projected Newton stopped projecting element Hessians that are PSD by construction
    // (hinges always, membranes without compression or negative stress): Jacobi had rebuilt some from
    // eigenvectors after a rounding-level negative eigenvalue (the old digest, 62e5d4da…, held that
    // rounding). Native and wasm32-wasip1 agree on the new value; on the bolero the positions after six
    // sewing steps moved by at most 9e-13 m.
    assert_eq!(
        digest,
        "b53580a348be63329e3248d102a9937e8f44b7d1ec7599a956a856860a610493"
    );
}
