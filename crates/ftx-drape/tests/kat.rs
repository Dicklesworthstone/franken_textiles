//! Known-answer tests for the cloth core (plan §11.9, gate G2 items 2 and 3, plus the bending scale).

use ftx_core::{Mat3, Vec2, Vec3};
use ftx_drape::{ClothModel, Membrane, Solver, grid, hex_grid};

fn cotton() -> Membrane {
    Membrane::orthotropic(2000.0, 2000.0, 0.0, 200.0, 1.0)
}

/// A sheet wrapped isometrically onto a cylinder of radius R stores ½·B·A/R² of bending energy (the
/// hinge constant and the boundary-area correction; converges as O(h)).
#[test]
fn bending_energy_of_a_cylinder() {
    let (w, l) = (0.06, 0.04);
    for (n, r, tol) in [(24usize, 0.05, 0.02), (48, 0.05, 0.01), (48, 0.02, 0.01)] {
        let (rest, tris) = grid(w, l, n, n * 2 / 3, Vec2::ZERO);
        let model = ClothModel::new(rest.clone(), &tris, cotton(), 1e-5, 0.2).unwrap();
        let x: Vec<Vec3> = rest
            .iter()
            .map(|p| Vec3::new(r * (p.x / r).sin(), p.y, r * (1.0 - (p.x / r).cos())))
            .collect();
        let bend: f64 = model.hinges.iter().map(|h| h.energy(&x, 1e-5)).sum();
        let want = 0.5 * 1e-5 * w * l / (r * r);
        println!("cylinder n={n} R={r}: bending/theory = {:.4}", bend / want);
        assert!((bend / want - 1.0).abs() < tol, "ratio {}", bend / want);
    }
}

/// Uniaxial-stress modulus of an orthotropic sheet at angle θ to the warp, from the compliance S = Q⁻¹.
fn uniaxial_modulus(q: &[[f64; 3]; 3], theta: f64) -> f64 {
    let m = Mat3::from_rows(*q);
    let det = m.det();
    let r = &m.rows;
    let cof = |i: usize, j: usize| {
        let (a, b) = ((i + 1) % 3, (i + 2) % 3);
        let (c, d) = ((j + 1) % 3, (j + 2) % 3);
        r[a][c] * r[b][d] - r[a][d] * r[b][c]
    };
    let s: Vec<Vec<f64>> = (0..3)
        .map(|i| (0..3).map(|j| cof(j, i) / det).collect())
        .collect();
    let (c, sn) = (theta.cos(), -theta.sin()); // the load direction in the material frame
    let sigma = [c * c, sn * sn, c * sn];
    let eps: Vec<f64> = (0..3)
        .map(|i| (0..3).map(|j| s[i][j] * sigma[j]).sum())
        .collect();
    1.0 / (eps[0] * c * c + eps[1] * sn * sn + eps[2] * c * sn)
}

/// Stretch a square sheet along world x by `strain` with roller ends (x fixed, y free; one corner
/// pinned in y; z fixed everywhere) and return σ/ε (N/m) from the end reaction.
fn measured_modulus(mat: Membrane, theta: f64, strain: f64) -> f64 {
    end_stress(mat, theta, strain) / strain
}

/// The end reaction per unit reference width (N/m) of the uniaxial test at `strain`.
fn end_stress(mat: Membrane, theta: f64, strain: f64) -> f64 {
    let (side, n) = (0.1, 10);
    let (world, tris) = grid(side, side, n, n, Vec2::ZERO);
    let (c, s) = (theta.cos(), theta.sin());
    let rest: Vec<Vec2> = world
        .iter()
        .map(|p| Vec2::new(c * p.x + s * p.y, -s * p.x + c * p.y))
        .collect();
    let model = ClothModel::new(rest, &tris, mat, 1e-6, 0.2).unwrap();
    let mut x: Vec<Vec3> = world.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    let mut fixed = vec![[false, false, true]; x.len()];
    for (i, p) in world.iter().enumerate() {
        if p.x == 0.0 {
            fixed[i][0] = true;
        }
        if p.x == side {
            fixed[i][0] = true;
            x[i].x = side * (1.0 + strain);
        }
    }
    fixed[0][1] = true;
    let mut solver = Solver::new(model, x, &fixed, Vec3::ZERO);
    solver.tol = 1e-12;
    let rep = solver.static_solve().unwrap();
    assert!(rep.converged, "{rep:?}");
    let f = solver.elastic_forces();
    let reaction: f64 = world
        .iter()
        .zip(&f)
        .filter(|(p, _)| p.x == side)
        .map(|(_, f)| -f.x)
        .sum();
    reaction / side
}

/// KAT 2: uniaxial stretch along warp, weft and bias reproduces the moduli implied by Q.
#[test]
fn membrane_anisotropy_reproduces_input_moduli() {
    let mat = Membrane::orthotropic(800.0, 400.0, 0.2, 60.0, 1.0);
    for deg in [0.0f64, 90.0, 45.0, 30.0] {
        let theta = deg.to_radians();
        let got = measured_modulus(mat, theta, 1e-5);
        let want = uniaxial_modulus(&mat.q, theta);
        println!("anisotropy θ={deg}°: measured {got:.4} N/m, theory {want:.4} N/m");
        assert!((got / want - 1.0).abs() < 1e-3, "θ={deg}: {got} vs {want}");
    }
    // Warp is E1, weft E2, and the bias is far softer than either (shear-dominated): what isotropic
    // springs cannot represent.
    assert!((uniaxial_modulus(&mat.q, 0.0) - 800.0).abs() < 1e-9);
    assert!((uniaxial_modulus(&mat.q, std::f64::consts::FRAC_PI_2) - 400.0).abs() < 1e-9);
    assert!(uniaxial_modulus(&mat.q, std::f64::consts::FRAC_PI_4) < 0.6 * 400.0);
}

/// KAT 2b: compression follows the declared softening law, and a rigid rotation stores nothing.
/// For ν = 0 and uniaxial stretch λ along the warp, the end stress is P = Q₁₁·φ(E)·φ'(E)·λ with
/// E = (λ² − 1)/2 and φ the blend `r·e + (1 − r)·δ·tanh(e/δ)` below zero (r = √s): deep in compression
/// the tangent stiffness is s·Q₁₁, across zero strain it is continuous.
#[test]
fn compression_softening_and_rigid_rotation() {
    let soft = Membrane::orthotropic(800.0, 800.0, 0.0, 60.0, 0.04);
    let (r, delta) = (0.2, soft.transition);
    let phi = |e: f64| {
        if e >= 0.0 {
            (e, 1.0)
        } else {
            (
                r * e + (1.0 - r) * delta * (e / delta).tanh(),
                r + (1.0 - r) / (e / delta).cosh().powi(2),
            )
        }
    };
    for strain in [-0.02, -5e-4, 1e-3] {
        let lambda: f64 = 1.0 + strain;
        let (p, dp) = phi(0.5 * (lambda * lambda - 1.0));
        let want = 800.0 * p * dp * lambda;
        let got = end_stress(soft, 0.0, strain);
        println!("softening: strain {strain:+}: end stress {got:.6} N/m, law {want:.6} N/m");
        assert!((got / want - 1.0).abs() < 1e-6, "{got} vs {want}");
    }
    let tangent = (end_stress(soft, 0.0, -0.0199) - end_stress(soft, 0.0, -0.0201)) / 2e-4;
    println!("softening: tangent at −2 % {tangent:.3} N/m (s·E1 = 32, minus finite-strain terms)");
    assert!(tangent > 0.9 * 0.04 * 800.0 && tangent < 1.0 * 0.04 * 800.0);
    let tension = measured_modulus(soft, 0.0, 1e-5);
    assert!((tension / 800.0 - 1.0).abs() < 1e-3);

    let (rest, tris) = grid(0.05, 0.03, 8, 5, Vec2::ZERO);
    let model = ClothModel::new(rest.clone(), &tris, soft, 1e-5, 0.2).unwrap();
    let r = Mat3::rotation(Vec3::new(1.0, 2.0, -0.5).normalized(), 2.3);
    let x: Vec<Vec3> = rest
        .iter()
        .map(|p| r.mul_vec(Vec3::new(p.x, p.y, 0.0)) + Vec3::new(0.1, -0.2, 0.3))
        .collect();
    assert!(
        model.elastic_energy(&x) < 1e-18,
        "{}",
        model.elastic_energy(&x)
    );
}

/// The heavy elastica: a clamped, horizontally launched strip of length `l` under its own weight with
/// `κ = w/B`. Solves `φ'' = −κ(l − s)cos φ`, φ(0) = 0, φ'(l) = 0 by shooting; returns the tip (x, z).
fn heavy_elastica_tip(l: f64, kappa: f64) -> (f64, f64) {
    let integrate = |phi_p0: f64| -> (f64, f64, f64) {
        let n = 4000;
        let ds = l / n as f64;
        let (mut phi, mut dphi, mut x, mut z) = (0.0f64, phi_p0, 0.0f64, 0.0f64);
        let f = |s: f64, phi: f64| -kappa * (l - s) * phi.cos();
        for i in 0..n {
            let s = i as f64 * ds;
            // RK4 on (φ, φ', x, z).
            let k1 = (dphi, f(s, phi), phi.cos(), -phi.sin());
            let p2 = (phi + 0.5 * ds * k1.0, dphi + 0.5 * ds * k1.1);
            let k2 = (p2.1, f(s + 0.5 * ds, p2.0), p2.0.cos(), -p2.0.sin());
            let p3 = (phi + 0.5 * ds * k2.0, dphi + 0.5 * ds * k2.1);
            let k3 = (p3.1, f(s + 0.5 * ds, p3.0), p3.0.cos(), -p3.0.sin());
            let p4 = (phi + ds * k3.0, dphi + ds * k3.1);
            let k4 = (p4.1, f(s + ds, p4.0), p4.0.cos(), -p4.0.sin());
            phi += ds / 6.0 * (k1.0 + 2.0 * k2.0 + 2.0 * k3.0 + k4.0);
            dphi += ds / 6.0 * (k1.1 + 2.0 * k2.1 + 2.0 * k3.1 + k4.1);
            x += ds / 6.0 * (k1.2 + 2.0 * k2.2 + 2.0 * k3.2 + k4.2);
            z += ds / 6.0 * (k1.3 + 2.0 * k2.3 + 2.0 * k3.3 + k4.3);
        }
        (dphi, x, z)
    };
    let (mut lo, mut hi) = (0.0, kappa * l * l); // φ'(l) > 0 at lo, < 0 at hi
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if integrate(mid).0 > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let (_, x, z) = integrate(0.5 * (lo + hi));
    (x, z)
}

/// KAT 3: the Peirce cantilever (ASTM D1388 style). A strip of bending length c = (B/w)^⅓ overhanging
/// l = 2c droops to the exact heavy-elastica tip (within 2 % of l at 1 mm spacing, converging as O(h)),
/// and the ASTM recovery of B from the droop angle lands within a few percent of the input. Uses the hex
/// mesh: on right-triangle grids a zig-zag mode along the clamp makes the strip ~25 % too soft.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "about a minute in release; run with --release"
)]
fn peirce_cantilever_recovers_the_bending_stiffness() {
    let (b, rho, g): (f64, f64, f64) = (1e-5, 0.15, 9.81);
    let w = rho * g;
    let c = (b / w).cbrt();
    let l = 2.0 * c;
    let (clamp, width): (f64, f64) = (0.006, 0.025);
    let (rest, tris) = hex_grid(l + clamp, width, 0.001, Vec2::new(-clamp, 0.0));
    let model = ClothModel::new(rest.clone(), &tris, cotton(), b, rho).unwrap();
    let x: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
    let fixed: Vec<[bool; 3]> = rest.iter().map(|p| [p.x <= 1e-12; 3]).collect();
    let mut solver = Solver::new(model, x, &fixed, Vec3::new(0.0, 0.0, -g));
    let settle = solver.settle(0.01, 0.5, 1e-4, 3, 400).unwrap();
    solver.tol = 1e-9;
    let fin = solver.static_solve().unwrap();
    assert!(fin.converged, "{settle:?} {fin:?}");
    let tip: Vec<Vec3> = rest
        .iter()
        .zip(&solver.x)
        .filter(|(p, _)| (p.x - l).abs() < 1e-9)
        .map(|(_, x)| *x)
        .collect();
    let tip = tip.iter().fold(Vec3::ZERO, |a, p| a + *p) / tip.len() as f64;
    let (ex, ez) = heavy_elastica_tip(l, w / b);
    let theta = (-tip.z).atan2(tip.x);
    let c_rec = l * ((theta / 2.0).cos() / (8.0 * theta.tan())).cbrt();
    let b_rec = w * c_rec.powi(3);
    println!(
        "cantilever: c = {:.2} mm, l = {:.2} mm, {} DOF, {} steps / {} Newton iterations ({} projected); tip ({:.3}, {:.3}) mm vs elastica ({:.3}, {:.3}) mm; \
         droop {:.2}° (ASTM 41.5°); B recovered {:.4e} vs {:.4e} N·m ({:+.2} %)",
        c * 1e3,
        l * 1e3,
        solver.free_dofs(),
        settle.steps,
        settle.newton_iterations + fin.iterations,
        fin.projected_iterations,
        tip.x * 1e3,
        tip.z * 1e3,
        ex * 1e3,
        ez * 1e3,
        theta.to_degrees(),
        b_rec,
        b,
        100.0 * (b_rec / b - 1.0)
    );
    assert!(
        (tip.x - ex).abs() < 0.02 * l && (tip.z - ez).abs() < 0.02 * l,
        "tip vs exact elastica"
    );
    assert!(
        (b_rec / b - 1.0).abs() < 0.05,
        "recovered B off by {:.1} %",
        100.0 * (b_rec / b - 1.0)
    );
}

/// A small drape whose final state is pinned by its SHA-256: a 50 × 50 mm hex-meshed sheet hung from
/// two corners, 20 implicit steps of 1/30 s. The same digest must come out of every target (plan §11.9
/// item 9: native debug, native release, wasm).
#[test]
fn drape_digest_is_pinned() {
    let (rest, tris) = hex_grid(0.05, 0.05, 0.005, Vec2::ZERO);
    let mat = Membrane::orthotropic(1500.0, 900.0, 0.15, 80.0, 0.05);
    let model = ClothModel::new(rest.clone(), &tris, mat, 2e-5, 0.18).unwrap();
    let x: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, 0.0, p.y)).collect();
    let top = rest.iter().map(|p| p.y).fold(f64::MIN, f64::max);
    let fixed: Vec<[bool; 3]> = rest
        .iter()
        .map(|p| [(p.y == top && (p.x == 0.0 || p.x == 0.05)); 3])
        .collect();
    let mut s = Solver::new(model, x, &fixed, Vec3::new(0.0, 0.0, -9.81));
    for _ in 0..20 {
        s.step(1.0 / 30.0).unwrap();
    }
    let bytes: Vec<u8> =
        s.x.iter()
            .flat_map(|p| [p.x, p.y, p.z])
            .flat_map(f64::to_le_bytes)
            .collect();
    let digest = ftx_core::receipt::sha256_hex(&bytes);
    println!("drape digest {digest}");
    assert!(s.x.iter().all(|p| p.is_finite()));
    assert_eq!(
        digest,
        "1c2f4efd062e33426fe019a9eaedf55a015d8e3bbcc5914a669f46b66bf23272"
    );
}
