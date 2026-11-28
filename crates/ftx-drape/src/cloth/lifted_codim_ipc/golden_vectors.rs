//! Deterministic golden test vector suite for `fs-contact::kernel`.
//!
//! Exercises `ScalarBarrier`, mollified EE/PT/PE/PP C-IPC potentials + exact
//! `12x12` gradients/Hessians + Jacobi SPD projection, `NormalContactLaw`
//! variants, tight-inclusion CCD + topological arc exclusion + start-pair
//! offset, `TangentialFrictionLaw` + outer lagged fixed-point loop, and
//! sheared/screw/cylindrical `PeriodicDomain` BVH broadphase — parameterized
//! solely by `M: DetMath`.

use super::ccd::{
    CcdConfig, SegmentTopology, ccd_edge_edge, ccd_point_edge, ccd_point_point, ccd_point_triangle,
    is_intra_filament_excluded, reduced_start_pair_offset,
};
use super::codim_ipc::{
    ExponentialYarn, HertzianWire, NormalContactLaw, RibbonFlattening, ScalarBarrier,
    evaluate_ee_barrier, evaluate_pe_barrier, evaluate_pp_barrier, evaluate_pt_barrier,
    rest_mollifier_threshold,
};
use super::friction::{
    DirectionalAnisotropicScale, FrozenFrictionLag, InnerPairOutcome, OuterFrictionConfig,
    SmoothCoulombStribeck, TangentialFrictionLaw, WireFrettingMicroSlip,
    evaluate_frozen_friction_12, run_outer_lagged_fixed_point,
};
use super::periodic_bvh::{
    CylindricalBendFrame, PeriodicDomain, PeriodicSegment, query_periodic_candidates,
};
use super::{DetMath, Fnv1a64, Sha256};

/// Pinned FNV-1a 64-bit golden digest for [`compute_golden_digests`].
pub const CONTACT_KERNEL_GOLDEN_FNV64: u64 = 0x3f46_0900_947c_535f;

/// Pinned SHA-256 golden digest for [`compute_golden_digests`].
pub const CONTACT_KERNEL_GOLDEN_SHA256: [u8; 32] = [
    0xc3, 0xd7, 0x77, 0xc3, 0x22, 0xd5, 0x88, 0x52, 0xc3, 0xa2, 0x90, 0xfe, 0xda, 0xde, 0xad, 0xf1,
    0x9e, 0x50, 0x48, 0x5e, 0x90, 0xdc, 0x3f, 0xd8, 0x6b, 0x92, 0xda, 0xe9, 0x7f, 0x6f, 0xd6, 0x88,
];

fn feed_f64_pair(f: &mut Fnv1a64, s: &mut Sha256, v: f64) {
    f.feed_f64(v);
    s.feed_f64(v);
}

fn feed_i64_pair(f: &mut Fnv1a64, s: &mut Sha256, v: i64) {
    let bytes = v.to_le_bytes();
    f.update(&bytes);
    s.update(&bytes);
}

fn digest_barrier_and_cipc<M: DetMath>(fnv: &mut Fnv1a64, sha: &mut Sha256) -> [[f64; 12]; 12] {
    let barrier = ScalarBarrier {
        kappa: 120.0,
        dhat: 0.01,
    };
    for &g in &[0.002, 0.005, 0.009, 0.012] {
        feed_f64_pair(fnv, sha, barrier.value::<M>(g));
        feed_f64_pair(fnv, sha, barrier.d1::<M>(g));
        feed_f64_pair(fnv, sha, barrier.d2::<M>(g));
    }

    let q_rest = [
        [-0.01, 0.0, 0.0],
        [0.01, 0.0, 0.0],
        [0.0, -0.01, 0.006],
        [0.0, 0.01, 0.006],
    ];
    let m_bar = rest_mollifier_threshold(q_rest[0], q_rest[1], q_rest[2], q_rest[3])
        .expect("canonical rest edges");
    let q_small_angle = [
        [-0.01, 0.0, 0.0],
        [0.01, 0.0, 0.0],
        [-0.01, 0.0002, 0.005],
        [0.01, -0.0002, 0.005],
    ];
    let ee_eval =
        evaluate_ee_barrier::<M>(q_small_angle, m_bar, 0.001, &barrier).expect("canonical EE eval");
    feed_f64_pair(fnv, sha, ee_eval.distance_m);
    feed_f64_pair(fnv, sha, ee_eval.mollifier);
    feed_f64_pair(fnv, sha, ee_eval.energy_j);
    for &g in &ee_eval.grad {
        feed_f64_pair(fnv, sha, g);
    }
    for row in &ee_eval.hess_spd {
        for &h in row {
            feed_f64_pair(fnv, sha, h);
        }
    }

    let pt_eval = evaluate_pt_barrier::<M>(
        [0.002, 0.003, 0.0045],
        [0.0, 0.0, 0.0],
        [0.02, 0.0, 0.0],
        [0.0, 0.02, 0.0],
        0.001,
        &barrier,
    )
    .expect("canonical PT eval");
    feed_f64_pair(fnv, sha, pt_eval.energy_j);
    for &g in &pt_eval.grad {
        feed_f64_pair(fnv, sha, g);
    }

    let pe_eval = evaluate_pe_barrier::<M>(
        [0.005, 0.004, 0.0],
        [0.0, 0.0, 0.0],
        [0.01, 0.0, 0.0],
        0.0005,
        &barrier,
    )
    .expect("canonical PE eval");
    feed_f64_pair(fnv, sha, pe_eval.energy_j);

    let pp_eval = evaluate_pp_barrier::<M>([0.0, 0.0, 0.0], [0.003, 0.004, 0.0], 0.0005, &barrier)
        .expect("canonical PP eval");
    feed_f64_pair(fnv, sha, pp_eval.energy_j);

    ee_eval.hess_spd
}

fn digest_normal_laws<M: DetMath>(fnv: &mut Fnv1a64, sha: &mut Sha256) {
    let hertz = HertzianWire {
        reduced_modulus_pa: 2.0e10,
        effective_radius_m: 0.001,
    }
    .evaluate_normal::<M>(2.0e-5, 0.002)
    .expect("hertzian");
    let exp_yarn = ExponentialYarn {
        stiffness_pa: 5.0e5,
        decay_length_m: 2.0e-4,
        reference_radius_m: 0.001,
    }
    .evaluate_normal::<M>(3.0e-4, 0.004)
    .expect("exponential");
    let ribbon = RibbonFlattening {
        stiffness_n_per_m: 1.5e4,
        width_m: 0.004,
        thickness_m: 0.001,
        aspect_limit: 8.0,
    }
    .evaluate_normal::<M>(1.5e-4, 0.005)
    .expect("ribbon");
    for law in [hertz, exp_yarn, ribbon] {
        feed_f64_pair(fnv, sha, law.energy_j);
        feed_f64_pair(fnv, sha, law.force_n);
        feed_f64_pair(fnv, sha, law.stiffness_n_m);
    }
}

fn digest_ccd<M: DetMath>(fnv: &mut Fnv1a64, sha: &mut Sha256) {
    let d_pair = reduced_start_pair_offset(0.002, 0.003).expect("reduced offset");
    feed_f64_pair(fnv, sha, d_pair);

    let ee_ccd = ccd_edge_edge::<M>(
        [
            [-0.01, 0.0, 0.003],
            [0.01, 0.0, 0.003],
            [0.0, -0.01, 0.0],
            [0.0, 0.01, 0.0],
        ],
        [
            [-0.01, 0.0, -0.003],
            [0.01, 0.0, -0.003],
            [0.0, -0.01, 0.0],
            [0.0, 0.01, 0.0],
        ],
        d_pair,
        CcdConfig::default(),
    )
    .expect("EE CCD");
    feed_f64_pair(fnv, sha, ee_ccd.alpha_max);
    feed_i64_pair(fnv, sha, i64::from(ee_ccd.collided));

    let pt_ccd = ccd_point_triangle::<M>(
        [
            [0.005, 0.005, 0.004],
            [0.0, 0.0, 0.0],
            [0.02, 0.0, 0.0],
            [0.0, 0.02, 0.0],
        ],
        [
            [0.005, 0.005, -0.002],
            [0.0, 0.0, 0.0],
            [0.02, 0.0, 0.0],
            [0.0, 0.02, 0.0],
        ],
        0.0005,
        CcdConfig::default(),
    )
    .expect("PT CCD");
    feed_f64_pair(fnv, sha, pt_ccd.alpha_max);

    let pe_ccd = ccd_point_edge::<M>(
        [[0.005, 0.003, 0.0], [0.0, 0.0, 0.0], [0.01, 0.0, 0.0]],
        [[0.005, -0.003, 0.0], [0.0, 0.0, 0.0], [0.01, 0.0, 0.0]],
        0.0004,
        CcdConfig::default(),
    )
    .expect("PE CCD");
    feed_f64_pair(fnv, sha, pe_ccd.alpha_max);

    let pp_ccd = ccd_point_point::<M>(
        [[0.0, 0.0, 0.004], [0.0, 0.0, 0.0]],
        [[0.0, 0.0, -0.004], [0.0, 0.0, 0.0]],
        0.0005,
        CcdConfig::default(),
    )
    .expect("PP CCD");
    feed_f64_pair(fnv, sha, pp_ccd.alpha_max);

    let topo_a = SegmentTopology {
        filament_id: 1,
        seg_idx: 0,
        seg_count: 10,
        closed: false,
        periodic_shift: [0, 0, 0],
        arc_start_m: 0.0,
        arc_end_m: 0.002,
        total_arc_m: 0.02,
        radius_m: 0.001,
    };
    let topo_b = SegmentTopology {
        filament_id: 1,
        seg_idx: 3,
        seg_count: 10,
        closed: false,
        periodic_shift: [0, 0, 0],
        arc_start_m: 0.006,
        arc_end_m: 0.008,
        total_arc_m: 0.02,
        radius_m: 0.001,
    };
    feed_i64_pair(
        fnv,
        sha,
        i64::from(is_intra_filament_excluded(&topo_a, &topo_b, [0, 0, 0])),
    );
}

fn digest_friction<M: DetMath>(fnv: &mut Fnv1a64, sha: &mut Sha256) {
    let stribeck = SmoothCoulombStribeck {
        mu_static: 0.35,
        mu_kinetic: 0.22,
        v_stribeck_m_s: 0.02,
        eps_v_m_s: 0.001,
    };
    let (mu_s, eps_s, k_m) = stribeck
        .effective_parameters::<M>([0.0005, 0.0, 0.0], [1.0, 0.0, 0.0], 0.01, 5.0)
        .expect("stribeck params");
    feed_f64_pair(fnv, sha, mu_s);

    let aniso = DirectionalAnisotropicScale {
        mu_forward: 0.18,
        mu_reverse: 0.42,
        eps_v_m_s: 0.001,
    };
    let (mu_rev, _, _) = aniso
        .effective_parameters::<M>([-0.001, 0.0, 0.0], [1.0, 0.0, 0.0], 0.01, 5.0)
        .expect("aniso params");
    feed_f64_pair(fnv, sha, mu_rev);

    let fretting = WireFrettingMicroSlip {
        mu_gross: 0.28,
        k_tangential_n_per_m: 1.0e4,
        slip_limit_m: 5.0e-5,
        eps_v_m_s: 0.001,
    };
    let (mu_fret, _, k_fret) = fretting
        .effective_parameters::<M>([2.0e-5, 0.0, 0.0], [1.0, 0.0, 0.0], 0.01, 5.0)
        .expect("fretting params");
    feed_f64_pair(fnv, sha, mu_fret);
    feed_f64_pair(fnv, sha, k_fret);

    let init_lag =
        FrozenFrictionLag::from_normal::<M>(5.0, [0.0, 0.0, 1.0], mu_s, eps_s, k_m, [0.0; 3])
            .expect("init lag");
    let dq = [
        [0.0002, 0.0001, 0.0],
        [0.0002, 0.0001, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
    ];
    let (final_lags, outer_rep) = run_outer_lagged_fixed_point::<M, _, _>(
        &stribeck,
        0.01,
        OuterFrictionConfig {
            tol_f: 1e-9,
            max_outer_iters: 8,
        },
        vec![init_lag],
        |lags| {
            let eval = evaluate_frozen_friction_12::<M>(&lags[0], &dq, [0.5, 0.5], [0.5, 0.5])?;
            Ok(vec![InnerPairOutcome {
                lambda_n: lags[0].lambda_n,
                normal: lags[0].normal,
                u_t: [0.0002, 0.0001, 0.0],
                strand_tangent: [1.0, 0.0, 0.0],
                lambda_f: eval.lambda_f,
            }])
        },
    )
    .expect("outer friction converge");
    feed_i64_pair(fnv, sha, i64::from(outer_rep.outer_iters));
    feed_f64_pair(fnv, sha, outer_rep.final_delta_lambda_inf);
    for &lf in &final_lags[0].prev_lambda_f {
        feed_f64_pair(fnv, sha, lf);
    }
}

fn digest_periodic<M: DetMath>(fnv: &mut Fnv1a64, sha: &mut Sha256, ee_hess_spd: &[[f64; 12]; 12]) {
    let sheared = PeriodicDomain::Planar2D {
        u_vec: [0.02, 0.0, 0.0],
        v_vec: [0.008, 0.02, 0.0],
    };
    let segs_planar = [
        PeriodicSegment {
            p0: [0.001, 0.001, 0.0],
            p1: [0.019, 0.015, 0.0],
            radius_m: 0.001,
        },
        PeriodicSegment {
            p0: [0.026, 0.019, 0.0005],
            p1: [0.027, 0.021, 0.0005],
            radius_m: 0.001,
        },
    ];
    let cand_planar =
        query_periodic_candidates::<M>(&sheared, &segs_planar, 0.002).expect("sheared planar BVH");
    feed_i64_pair(fnv, sha, cand_planar.len() as i64);
    for c in &cand_planar {
        feed_i64_pair(fnv, sha, c.seg_a as i64);
        feed_i64_pair(fnv, sha, c.seg_b as i64);
        for &s in &c.image_shift {
            feed_i64_pair(fnv, sha, i64::from(s));
        }
    }

    let tubular = PeriodicDomain::TubularHelical2D {
        radius_m: 0.02,
        pitch_m: 0.015,
        circumferential_count: 6,
    };
    let segs_tubular = [
        PeriodicSegment {
            p0: [0.02, 0.0, 0.001],
            p1: [0.019, 0.003, 0.004],
            radius_m: 0.001,
        },
        PeriodicSegment {
            p0: [0.01, 0.017, 0.015],
            p1: [0.011, 0.016, 0.017],
            radius_m: 0.001,
        },
    ];
    let cand_tubular =
        query_periodic_candidates::<M>(&tubular, &segs_tubular, 0.002).expect("tubular BVH");
    feed_i64_pair(fnv, sha, cand_tubular.len() as i64);
    for c in &cand_tubular {
        feed_i64_pair(fnv, sha, c.seg_a as i64);
        feed_i64_pair(fnv, sha, c.seg_b as i64);
        for &s in &c.image_shift {
            feed_i64_pair(fnv, sha, i64::from(s));
        }
    }

    let bend = CylindricalBendFrame {
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 1.0, 0.0],
        delta_phi_rad: 0.25,
    };
    let pulled_h = bend
        .pull_back_hessian_12::<M>(ee_hess_spd, [false, false, true, true])
        .expect("bend pullback");
    feed_f64_pair(fnv, sha, pulled_h[6][6]);
    feed_f64_pair(fnv, sha, pulled_h[8][11]);
}

/// Evaluate the canonical deterministic kernel suite and return `(fnv64, sha256)`.
///
/// # Panics
/// Panics only if the canonical valid inputs fail internal invariants.
#[must_use]
pub fn compute_golden_digests<M: DetMath>() -> (u64, [u8; 32]) {
    let mut fnv = Fnv1a64::new();
    let mut sha = Sha256::new();
    let ee_hess_spd = digest_barrier_and_cipc::<M>(&mut fnv, &mut sha);
    digest_normal_laws::<M>(&mut fnv, &mut sha);
    digest_ccd::<M>(&mut fnv, &mut sha);
    digest_friction::<M>(&mut fnv, &mut sha);
    digest_periodic::<M>(&mut fnv, &mut sha, &ee_hess_spd);
    (fnv.finish(), sha.finalize())
}
