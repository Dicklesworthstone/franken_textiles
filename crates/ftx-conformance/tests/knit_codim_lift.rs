//! Conformance tests for `FTX-K09` lifted `fs-codim` & `fs-contact` Pure-Kernel Leaves
//! (`ftx_drape::cloth::{lifted_codim, lifted_codim_ipc}`).

use ftx_drape::cloth::lifted_codim::adjoint::solve_elastic_ift_adjoint;
use ftx_drape::cloth::lifted_codim::cosserat::CosseratRod;
use ftx_drape::cloth::lifted_codim::golden_vectors::{
    CODIM_KERNEL_GOLDEN_FNV64, CODIM_KERNEL_GOLDEN_SHA256,
    compute_golden_digests as codim_compute_golden_digests,
};
use ftx_drape::cloth::lifted_codim::junction::NetworkDofLayout;
use ftx_drape::cloth::lifted_codim::section_laws::{LinearElasticIsotropic, RodSectionModel};
use ftx_drape::cloth::lifted_codim::solver::{
    CodimContactBridge, ContactStencilEval, NoContactBridge, RodNetwork, SolverConfig,
    solve_network,
};
use ftx_drape::cloth::lifted_codim::{CodimError, FtxDetMath as CodimMath};
use ftx_drape::cloth::lifted_codim_ipc::FtxDetMath as ContactMath;
use ftx_drape::cloth::lifted_codim_ipc::ccd::{CcdConfig, ccd_edge_edge};
use ftx_drape::cloth::lifted_codim_ipc::codim_ipc::{
    ScalarBarrier, evaluate_ee_barrier, rest_mollifier_threshold,
};
use ftx_drape::cloth::lifted_codim_ipc::golden_vectors::{
    CONTACT_KERNEL_GOLDEN_FNV64, CONTACT_KERNEL_GOLDEN_SHA256,
    compute_golden_digests as contact_compute_golden_digests,
};
use ftx_drape::cloth::lifted_codim_ipc::periodic_bvh::{
    PeriodicDomain, PeriodicSegment, query_periodic_candidates,
};

#[test]
fn lifted_codim_ipc_matches_pinned_frankensim_golden_digests() {
    let (fnv_a, sha_a) = contact_compute_golden_digests::<ContactMath>();
    let (fnv_b, sha_b) = contact_compute_golden_digests::<ContactMath>();
    assert_eq!(
        fnv_a, fnv_b,
        "lifted_codim_ipc digest must be deterministic"
    );
    assert_eq!(
        sha_a, sha_b,
        "lifted_codim_ipc sha256 must be deterministic"
    );
    assert_eq!(
        fnv_a, CONTACT_KERNEL_GOLDEN_FNV64,
        "lifted_codim_ipc FNV-1a64 drifted from pinned fs-contact golden vector"
    );
    assert_eq!(
        sha_a, CONTACT_KERNEL_GOLDEN_SHA256,
        "lifted_codim_ipc SHA-256 drifted from pinned fs-contact golden vector"
    );
}

#[test]
fn lifted_codim_golden_vectors_are_deterministic_and_physics_checks_pass() {
    let (fnv_a, sha_a) = codim_compute_golden_digests::<CodimMath>();
    let (fnv_b, sha_b) = codim_compute_golden_digests::<CodimMath>();
    assert_eq!(fnv_a, fnv_b, "lifted_codim digest must be deterministic");
    assert_eq!(sha_a, sha_b, "lifted_codim sha256 must be deterministic");
    assert_eq!(
        fnv_a, CODIM_KERNEL_GOLDEN_FNV64,
        "lifted_codim FNV-1a64 drifted from pinned fs-codim golden vector"
    );
    assert_eq!(
        sha_a, CODIM_KERNEL_GOLDEN_SHA256,
        "lifted_codim SHA-256 drifted from pinned fs-codim golden vector"
    );

    // Verify cantilevered Cosserat rod equilibrium + elastic IFT adjoint against finite difference.
    let sec = RodSectionModel::LinearElasticIsotropic(LinearElasticIsotropic {
        ea: 500.0,
        ga: 500.0,
        gj: 2.0,
        ei: 2.0,
    });
    let solve_tip_y = |tip_force: f64| -> (f64, f64) {
        let rod = CosseratRod::straight(0.4, 4, sec).unwrap();
        let mut net = RodNetwork::new(vec![rod], vec![], vec![]);
        for d in 0..6 {
            net.pinned_dofs.insert(d);
        }
        let tip_y_dof = 6 * 4 + 1;
        net.external_forces.push((tip_y_dof, tip_force));
        let mut bridge = NoContactBridge;
        let out =
            solve_network::<CodimMath, _>(&mut net, &mut bridge, &SolverConfig::default()).unwrap();
        assert!(out.report.final_residual_norm < 1e-9);
        let mut grad_q = vec![0.0; out.total_dofs];
        grad_q[tip_y_dof] = 1.0;
        let mut dg_dp = vec![0.0; out.total_dofs];
        dg_dp[tip_y_dof] = -1.0;
        let adj = solve_elastic_ift_adjoint(&out, &grad_q, &[&dg_dp], &[0.0]).unwrap();
        (net.cosserat_rods[0].positions[4][1], adj.d_obj_dp_total[0])
    };

    let (y0, adj_dy_df) = solve_tip_y(0.10);
    let h = 1e-5;
    let (yp, _) = solve_tip_y(0.10 + h);
    let (ym, _) = solve_tip_y(0.10 - h);
    let fd = (yp - ym) / (2.0 * h);
    assert!(y0 > 0.0, "tip force must deflect cantilever positively");
    assert!(
        (adj_dy_df - fd).abs() < 1e-5 * fd.abs().max(1e-6),
        "single-solve IFT adjoint mismatch: adj={adj_dy_df} fd={fd}"
    );
}

struct TwoCrossedFilamentsBridge {
    barrier: ScalarBarrier,
    d_offset: f64,
    m_bar: f64,
}

impl CodimContactBridge for TwoCrossedFilamentsBridge {
    fn ccd_max_step(
        &self,
        current: &RodNetwork,
        trial: &RodNetwork,
        _layout: &NetworkDofLayout,
    ) -> Result<f64, CodimError> {
        let seg_q0 = [
            current.cosserat_rods[0].positions[0],
            current.cosserat_rods[0].positions[1],
            current.cosserat_rods[1].positions[0],
            current.cosserat_rods[1].positions[1],
        ];
        let seg_q1 = [
            trial.cosserat_rods[0].positions[0],
            trial.cosserat_rods[0].positions[1],
            trial.cosserat_rods[1].positions[0],
            trial.cosserat_rods[1].positions[1],
        ];
        let ccd = ccd_edge_edge::<ContactMath>(seg_q0, seg_q1, self.d_offset, CcdConfig::default())
            .map_err(|_| CodimError::ContactBridgeError {
                detail: "ccd_edge_edge failed",
            })?;
        Ok(ccd.alpha_max)
    }

    fn evaluate_contact(
        &self,
        network: &RodNetwork,
        layout: &NetworkDofLayout,
        _outer_start_positions: &[[f64; 3]],
    ) -> Result<Vec<ContactStencilEval>, CodimError> {
        let segs = [
            PeriodicSegment {
                p0: network.cosserat_rods[0].positions[0],
                p1: network.cosserat_rods[0].positions[1],
                radius_m: 0.5 * self.d_offset,
            },
            PeriodicSegment {
                p0: network.cosserat_rods[1].positions[0],
                p1: network.cosserat_rods[1].positions[1],
                radius_m: 0.5 * self.d_offset,
            },
        ];
        let cands = query_periodic_candidates::<ContactMath>(
            &PeriodicDomain::Bounded,
            &segs,
            self.barrier.dhat,
        )
        .map_err(|_| CodimError::ContactBridgeError {
            detail: "periodic BVH query failed",
        })?;
        if cands.is_empty() {
            return Ok(Vec::new());
        }
        let quad = [segs[0].p0, segs[0].p1, segs[1].p0, segs[1].p1];
        let ev = evaluate_ee_barrier::<ContactMath>(quad, self.m_bar, self.d_offset, &self.barrier)
            .map_err(|_| CodimError::InitialPenetration {
                min_clearance_m: 0.0,
            })?;
        let node_dofs = [
            layout.cosserat_offsets[0],
            layout.cosserat_offsets[0] + 6,
            layout.cosserat_offsets[1],
            layout.cosserat_offsets[1] + 6,
        ];
        Ok(vec![ContactStencilEval {
            dofs_12: std::array::from_fn(|i| node_dofs[i / 3] + i % 3),
            energy_j: ev.energy_j,
            grad_12: ev.grad,
            hess_spd_12: ev.hess_spd,
        }])
    }

    fn has_friction(&self) -> bool {
        false
    }

    fn update_outer_friction(
        &mut self,
        _network: &RodNetwork,
        _layout: &NetworkDofLayout,
        _outer_start_positions: &[[f64; 3]],
    ) -> Result<(bool, f64), CodimError> {
        Ok((true, 0.0))
    }
}

#[test]
fn lifted_codim_and_codim_ipc_coupled_crossing_maintains_strict_clearance() {
    let sec = RodSectionModel::LinearElasticIsotropic(LinearElasticIsotropic {
        ea: 400.0,
        ga: 400.0,
        gj: 0.5,
        ei: 0.5,
    });
    let mut r0 = CosseratRod::straight(0.08, 1, sec).unwrap();
    r0.positions = vec![[-0.04, 0.0, 0.0], [0.04, 0.0, 0.0]];
    let mut r1 = CosseratRod::straight(0.08, 1, sec).unwrap();
    r1.positions = vec![[0.0, -0.04, 0.008], [0.0, 0.04, 0.008]];

    let mut net = RodNetwork::new(vec![r0, r1], vec![], vec![]);
    let lay = net.layout();
    for r in 0..2 {
        for n in 0..2 {
            let b = lay.cosserat_offsets[r] + 6 * n;
            for k in [0, 1, 3, 4, 5] {
                net.pinned_dofs.insert(b + k);
            }
            if r == 0 {
                net.pinned_dofs.insert(b + 2);
            } else {
                net.external_forces.push((b + 2, -0.25));
            }
        }
    }
    let m_bar = rest_mollifier_threshold(
        [-0.04, 0.0, 0.0],
        [0.04, 0.0, 0.0],
        [0.0, -0.04, 0.008],
        [0.0, 0.04, 0.008],
    )
    .unwrap();
    let mut bridge = TwoCrossedFilamentsBridge {
        barrier: ScalarBarrier {
            kappa: 10.0,
            dhat: 0.006,
        },
        d_offset: 0.004,
        m_bar,
    };
    let out =
        solve_network::<CodimMath, _>(&mut net, &mut bridge, &SolverConfig::default()).unwrap();
    assert!(out.report.final_residual_norm < 1e-8);
    let z1 = net.cosserat_rods[1].positions[0][2];
    assert!(
        z1 > 0.004 && z1 < 0.008,
        "codimensional IPC barrier must arrest downward rod strictly above d_offset=0.004 m, got z1={z1}"
    );
}
