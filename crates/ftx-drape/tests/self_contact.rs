//! Synthetic cloth–cloth contact checks. These exercise solver mechanics, not garment fit or a
//! calibrated fabric. No body-model assets are needed.
use ftx_core::{Vec2, Vec3};
use ftx_drape::contact::{Body, Contact, ContactParams};
use ftx_drape::{ClothModel, Membrane, Solver};

fn params() -> ContactParams {
    ContactParams {
        dhat: 0.001,
        thickness: 0.0005,
        kappa: 100.0,
        mu: 0.2,
        eps_v: 0.001,
    }
}

/// A single rectangular strip folded into a U. Its lower half is pinned and the other rows are
/// guided vertically: the top cannot escape sideways instead of responding to self-contact.
fn folded_strip() -> Solver {
    let stations = [
        (0.0, 0.0, 0.0),
        (0.012, 0.012, 0.0),
        (0.024, 0.024, 0.0),
        (0.030, 0.024, 0.006),
        (0.042, 0.012, 0.006),
        (0.054, 0.0, 0.006),
    ];
    let mut rest = Vec::new();
    let mut x = Vec::new();
    for (u, px, z) in stations {
        for y in [-0.005, 0.005] {
            rest.push(Vec2::new(u, y));
            x.push(Vec3::new(px, y, z));
        }
    }
    let tris: Vec<_> = (0..5)
        .flat_map(|i| {
            let a = 2 * i;
            [[a, a + 2, a + 3], [a, a + 3, a + 1]]
        })
        .collect();
    let fixed: Vec<_> = (0..x.len()).map(|i| [true, true, i < 6]).collect();
    let model = ClothModel::new(
        rest,
        &tris,
        Membrane::orthotropic(1.0, 1.0, 0.0, 0.2, 0.05),
        1e-10,
        0.2,
    )
    .unwrap();
    let mut s = Solver::new(model, x, &fixed, Vec3::new(0.0, 0.0, -9.81))
        .with_self_contact(params())
        .unwrap();
    s.tol = 1e-6;
    s.max_newton = 200;
    for v in &mut s.v[6..] {
        v.z = -0.3;
    }
    s
}

#[test]
fn connected_folded_strip_cannot_pass_through_its_pinned_half() {
    let mut s = folded_strip();
    let pinned = s.x[..6].to_vec();
    let mut active = false;
    for step in 0..30 {
        let report = s.step(0.005).unwrap();
        assert!(report.converged, "step {step}: {report:?}");
        assert_eq!(s.x[..6], pinned);
        let c = s.contact.as_ref().unwrap();
        c.audit_self_contact(&s.x).unwrap();
        if let Some(g) = c.min_self_gap(&s.x, params().dhat) {
            assert!(g > 0.0, "step {step}: gap {g}");
            active |= g < params().dhat;
        }
        assert!(s.x[8..].iter().all(|p| p.z > params().thickness));
    }
    assert!(
        active,
        "the fold must actually encounter the contact barrier"
    );

    // The same fold without contact must cross: material stiffness alone cannot pass this test.
    let mut unchecked = folded_strip();
    unchecked.contact = None;
    for _ in 0..30 {
        assert!(unchecked.step(0.005).unwrap().converged);
    }
    assert!(unchecked.x[8..].iter().any(|p| p.z < params().thickness));
}

#[test]
fn folded_strip_repeated_runs_are_bit_identical() {
    let mut a = folded_strip();
    let mut b = folded_strip();
    for _ in 0..12 {
        assert_eq!(a.step(0.005).unwrap(), b.step(0.005).unwrap());
        for (a, b) in a.x.iter().zip(&b.x) {
            for k in 0..3 {
                assert_eq!(a[k].to_bits(), b[k].to_bits());
            }
        }
    }
}

/// The folded strip after twelve steps, pinned by its SHA-256: the same on every target (native
/// debug, native release and wasm32-wasip1), self-contact barrier, CCD and friction included.
#[test]
fn folded_strip_digest_is_pinned() {
    let mut s = folded_strip();
    for _ in 0..12 {
        assert!(s.step(0.005).unwrap().converged);
    }
    let bytes: Vec<u8> =
        s.x.iter()
            .flat_map(|p| [p.x, p.y, p.z])
            .flat_map(f64::to_le_bytes)
            .collect();
    let digest = ftx_core::receipt::sha256_hex(&bytes);
    println!("self-contact digest {digest}");
    assert_eq!(
        digest,
        "4ba9868318408e1615df8a1cd22907196ca03d49cb8dd8d9c724ef6e28bdff5a"
    );
}

#[test]
fn body_clearance_and_self_clearance_use_separate_offsets() {
    let vertices = vec![
        Vec3::new(-1.0, -1.0, -0.02),
        Vec3::new(1.0, -1.0, -0.02),
        Vec3::new(0.0, 1.0, -0.02),
    ];
    let body = Body::new(vertices, vec![[0, 1, 2]], 0.01).unwrap();
    let x: Vec<_> = [0.0, 0.001]
        .into_iter()
        .flat_map(|z| {
            [
                Vec3::new(-0.01, -0.01, z),
                Vec3::new(0.01, -0.01, z),
                Vec3::new(0.0, 0.01, z),
            ]
        })
        .collect();
    let body_params = ContactParams {
        thickness: 0.0015,
        ..params()
    };
    let c = Contact::new(body, body_params, vec![[0, 1, 2], [3, 4, 5]], |a, b| {
        (x[a] - x[b]).norm()
    });
    let body_gap = c.min_gap(&x, 0.03).unwrap();
    assert!((body_gap - 0.0185).abs() < 1e-12);
    let mut c = c.with_self_contact(&x, params()).unwrap();
    assert_eq!(c.min_gap(&x, 0.03), Some(body_gap));
    let self_gap = c.min_self_gap(&x, 0.001).unwrap();
    assert!((self_gap - 0.0005).abs() < 1e-12);
    c.params.thickness += 0.00025; // the scene's body-offset ramp must not shrink the cloth–cloth gap
    assert_eq!(c.min_self_gap(&x, 0.001), Some(self_gap));
}
