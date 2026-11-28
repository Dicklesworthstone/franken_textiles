//! Deterministic golden test vector suite for `fs-codim::kernel`.
//!
//! Exercises 4-DOF DER over Bishop frames, 6-DOF Cosserat `[w, x, y, z]`
//! kinematics, 6-DOF `SO(3)` [`JunctionNode`] branching, all four
//! [`RodSectionModel`] constitutive laws, canonical sorted full-symmetric CSR
//! [`SymCsr`] + [`SymbolicLdlt`] + [`LdltFactor`] + [`solve_pcg_jacobi`],
//! coupled [`solve_network`], and elastic single-solve IFT adjoints (plus
//! irreversible-history refusal) — parameterized solely by `M: DetMath`.

use super::adjoint::solve_elastic_ift_adjoint;
use super::cosserat::{CosseratRod, evaluate_cosserat_segment};
use super::der::{DerCurve, evaluate_der_curve};
use super::junction::{CurveEnd, JunctionArm, JunctionNode, RodArmKind};
use super::section_laws::{
    ElastoplasticYield, LinearElasticIsotropic, NonlinearFilamentBundle, RodSectionLaw,
    RodSectionModel, SuperelasticSma,
};
use super::solver::{NoContactBridge, RodNetwork, SolverConfig, solve_network};
use super::sparse_ldlt::{DirectOrdering, LdltOptions, SymCsr, SymbolicLdlt, solve_pcg_jacobi};
use super::{CodimError, DetMath, Fnv1a64, Sha256, quat_exp};

/// Pinned FNV-1a 64-bit golden digest for [`compute_golden_digests`].
pub const CODIM_KERNEL_GOLDEN_FNV64: u64 = 0x14cb_fef4_d2b6_5e84;

/// Pinned SHA-256 golden digest for [`compute_golden_digests`].
pub const CODIM_KERNEL_GOLDEN_SHA256: [u8; 32] = [
    0xe0, 0xd3, 0xf1, 0x87, 0x3c, 0x51, 0x5c, 0x9a, 0xcf, 0xb4, 0x0b, 0x33, 0xea, 0x4f, 0x1b, 0x05,
    0xae, 0x32, 0xad, 0xf7, 0xda, 0x72, 0x98, 0x9c, 0xd1, 0xf6, 0x2b, 0x30, 0x86, 0x03, 0xdb, 0xe9,
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

fn digest_section_laws<M: DetMath>(fnv: &mut Fnv1a64, sha: &mut Sha256) {
    let lin = LinearElasticIsotropic {
        ea: 1.0e4,
        ga: 5.0e3,
        gj: 2.0,
        ei: 3.0,
    };
    let h_lin = lin.initial_history();
    let (ev_lin, _) = lin
        .evaluate_section::<M>(0.01, [0.002, -0.001], [0.5, -0.25], 0.3, &h_lin)
        .expect("linear law");
    feed_f64_pair(fnv, sha, ev_lin.energy_density_j_m);
    for &st in &ev_lin.stress_6 {
        feed_f64_pair(fnv, sha, st);
    }

    let bundle = NonlinearFilamentBundle {
        ea: 5.0e3,
        ea_crimp: 2.0e4,
        crimp_strain: 0.02,
        ga: 2.5e3,
        gj: 1.5,
        ei: 2.2,
        bending_hardening: 0.4,
    };
    let h_bnd = bundle.initial_history();
    let (ev_bnd, _) = bundle
        .evaluate_section::<M>(0.03, [0.001, 0.0], [0.8, 0.2], 0.15, &h_bnd)
        .expect("bundle law");
    feed_f64_pair(fnv, sha, ev_bnd.energy_density_j_m);
    feed_f64_pair(fnv, sha, ev_bnd.tangent_6x6[0][0]);
    feed_f64_pair(fnv, sha, ev_bnd.tangent_6x6[4][5]);

    let sma = SuperelasticSma {
        ea_austenite: 1.0e5,
        ea_martensite: 4.0e4,
        sigma_ms: 400.0,
        sigma_mf: 500.0,
        sigma_as: 250.0,
        sigma_af: 150.0,
        max_transform_strain: 0.04,
        ga: 2.0e4,
        gj: 5.0,
        ei: 6.0,
    };
    let mut h_sma = sma.initial_history();
    let (ev_sma1, h_sma1) = sma
        .evaluate_section::<M>(0.02, [0.0; 2], [0.1, 0.0], 0.05, &h_sma)
        .expect("sma forward");
    h_sma = h_sma1;
    let (ev_sma2, _) = sma
        .evaluate_section::<M>(0.006, [0.0; 2], [0.1, 0.0], 0.05, &h_sma)
        .expect("sma reverse");
    feed_f64_pair(fnv, sha, ev_sma1.stress_6[0]);
    feed_f64_pair(fnv, sha, ev_sma1.tangent_6x6[0][0]);
    feed_f64_pair(fnv, sha, ev_sma2.stress_6[0]);

    let ep = ElastoplasticYield {
        ea: 5.0e4,
        ga: 2.0e4,
        gj: 4.0,
        ei: 8.0,
        yield_force_n: 300.0,
        yield_moment_nm: 12.0,
        hardening_ratio: 0.15,
    };
    let h_ep = ep.initial_history();
    let (ev_ep, h_ep2) = ep
        .evaluate_section::<M>(0.01, [0.0; 2], [1.2, 0.4], 0.1, &h_ep)
        .expect("ep yield");
    feed_f64_pair(fnv, sha, ev_ep.stress_6[0]);
    feed_f64_pair(fnv, sha, ev_ep.stress_6[4]);
    feed_f64_pair(fnv, sha, ev_ep.tangent_6x6[0][4]);
    feed_i64_pair(fnv, sha, i64::from(h_ep2.is_path_dependent()));
}

fn digest_der_and_cosserat<M: DetMath>(fnv: &mut Fnv1a64, sha: &mut Sha256) {
    let sec = RodSectionModel::LinearElasticIsotropic(LinearElasticIsotropic {
        ea: 500.0,
        ga: 500.0,
        gj: 1.5,
        ei: 2.0,
    });
    let der = DerCurve::from_polyline::<M>(
        vec![
            [0.0, 0.0, 0.0],
            [0.25, 0.03, 0.01],
            [0.5, 0.01, 0.05],
            [0.75, -0.02, 0.02],
        ],
        false,
        vec![0.1, -0.05, 0.2],
        vec![sec; 3],
    )
    .expect("der polyline");
    let eh = vec![sec.initial_history(); 3];
    let jh = vec![sec.initial_history(); 2];
    let dev = evaluate_der_curve::<M>(&der, &eh, &jh).expect("der eval");
    feed_f64_pair(fnv, sha, dev.energy_j);
    for &g in &dev.grad {
        feed_f64_pair(fnv, sha, g);
    }

    let qa = quat_exp::<M>([0.1, -0.2, 0.15]);
    let qb = quat_exp::<M>([0.25, 0.05, -0.1]);
    let cev = evaluate_cosserat_segment::<M, _>(
        [0.0, 0.0, 0.0],
        qa,
        [0.24, 0.04, -0.02],
        qb,
        0.25,
        [0.0; 3],
        [0.0; 3],
        &sec,
        &sec.initial_history(),
    )
    .expect("cosserat eval");
    feed_f64_pair(fnv, sha, cev.energy_j);
    for &g in &cev.grad_12 {
        feed_f64_pair(fnv, sha, g);
    }
    feed_f64_pair(fnv, sha, cev.hess_12[3][9]);
}

fn digest_sparse_and_solver<M: DetMath>(fnv: &mut Fnv1a64, sha: &mut Sha256) {
    // Indefinite symmetric saddle system requiring 2x2 pivoting.
    let csr = SymCsr::from_triplets(
        4,
        &[
            (0, 0, 4.0),
            (0, 1, 1.0),
            (1, 0, 1.0),
            (1, 1, 3.0),
            (0, 2, 1.0),
            (2, 0, 1.0),
            (1, 3, 1.0),
            (3, 1, 1.0),
            (2, 2, 0.0),
            (3, 3, 0.0),
        ],
    )
    .expect("saddle csr");
    let sym = SymbolicLdlt::analyze(&csr, DirectOrdering::Amd).expect("analyze");
    let fac = sym.factor(&csr, &LdltOptions::default()).expect("factor");
    let rhs = [1.0, 2.0, 0.5, -0.5];
    let x = fac.solve(&rhs);
    for &xi in &x {
        feed_f64_pair(fnv, sha, xi);
    }
    feed_i64_pair(fnv, sha, fac.inertia().positive as i64);
    feed_i64_pair(fnv, sha, fac.inertia().negative as i64);

    let spd = SymCsr::from_triplets(
        3,
        &[
            (0, 0, 4.0),
            (0, 1, -1.0),
            (1, 0, -1.0),
            (1, 1, 4.0),
            (1, 2, -1.0),
            (2, 1, -1.0),
            (2, 2, 4.0),
        ],
    )
    .expect("spd csr");
    let pcg = solve_pcg_jacobi::<M>(&spd, &[1.0, 2.0, 3.0], 1e-12, 200).expect("pcg");
    for &xi in &pcg.x {
        feed_f64_pair(fnv, sha, xi);
    }

    // Coupled Cosserat + DER + JunctionNode + IFT adjoint.
    let sec = RodSectionModel::LinearElasticIsotropic(LinearElasticIsotropic {
        ea: 200.0,
        ga: 200.0,
        gj: 1.0,
        ei: 1.0,
    });
    let rod = CosseratRod::straight(0.5, 4, sec).expect("straight rod");
    let der = DerCurve::from_polyline::<M>(
        vec![[0.5, 0.0, 0.0], [0.75, 0.0, 0.0], [1.0, 0.0, 0.0]],
        false,
        vec![0.0, 0.0],
        vec![sec; 2],
    )
    .expect("der arm");
    let junc = JunctionNode {
        position: [0.5, 0.0, 0.0],
        q_wxyz: [1.0, 0.0, 0.0, 0.0],
        arms: vec![
            JunctionArm {
                kind: RodArmKind::Cosserat {
                    rod_idx: 0,
                    node_idx: 4,
                },
                local_offset_m: [0.0; 3],
                q_offset_wxyz: [1.0, 0.0, 0.0, 0.0],
                k_trans_n_m: 1.0e4,
                k_rot_nm: 1.0e3,
            },
            JunctionArm {
                kind: RodArmKind::Der {
                    curve_idx: 0,
                    end: CurveEnd::Start,
                },
                local_offset_m: [0.0; 3],
                q_offset_wxyz: [1.0, 0.0, 0.0, 0.0],
                k_trans_n_m: 1.0e4,
                k_rot_nm: 1.0e3,
            },
        ],
    };
    let mut net = RodNetwork::new(vec![rod], vec![der], vec![junc]);
    for d in 0..6 {
        net.pinned_dofs.insert(d);
    }
    let layout = net.layout();
    let tip_y_dof = layout.der_offsets[0] + 3 * 2 + 1;
    net.external_forces.push((tip_y_dof, 0.05));
    let mut bridge = NoContactBridge;
    let out = solve_network::<M, _>(&mut net, &mut bridge, &SolverConfig::default())
        .expect("coupled solve");
    feed_f64_pair(fnv, sha, out.report.final_energy_j);
    feed_f64_pair(fnv, sha, net.der_curves[0].positions[2][1]);

    // Elastic IFT adjoint wrt tip_y force parameter.
    let mut grad_q = vec![0.0; out.total_dofs];
    grad_q[tip_y_dof] = 1.0;
    let mut dg_dp = vec![0.0; out.total_dofs];
    dg_dp[tip_y_dof] = -1.0;
    let adj = solve_elastic_ift_adjoint(&out, &grad_q, &[&dg_dp], &[0.0]).expect("ift adjoint");
    feed_f64_pair(fnv, sha, adj.d_obj_dp_total[0]);

    // Verify irreversible-history refusal deterministically.
    let mut out_irrev = out;
    out_irrev.all_sections_reversible = false;
    let refused = matches!(
        solve_elastic_ift_adjoint(&out_irrev, &grad_q, &[&dg_dp], &[0.0]),
        Err(CodimError::IrreversibleHistoryRequiresCheckpointAdjoint { .. })
    );
    feed_i64_pair(fnv, sha, i64::from(refused));
}

/// Evaluate the canonical deterministic `fs-codim` kernel vector suite and
/// return `(fnv64, sha256)`.
#[must_use]
pub fn compute_golden_digests<M: DetMath>() -> (u64, [u8; 32]) {
    let mut fnv = Fnv1a64::new();
    let mut sha = Sha256::new();
    digest_section_laws::<M>(&mut fnv, &mut sha);
    digest_der_and_cosserat::<M>(&mut fnv, &mut sha);
    digest_sparse_and_solver::<M>(&mut fnv, &mut sha);
    (fnv.finish(), sha.finalize())
}
