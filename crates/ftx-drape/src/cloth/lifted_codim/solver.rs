//! Coupled implicit damped Newton solver with CCD step capping, Armijo
//! energy line search, trial-safe section history commitment, outer lagged
//! friction loop, and non-conservative follower-moment compatibility.

use std::collections::BTreeSet;

use super::cosserat::{CosseratRod, evaluate_cosserat_segment};
use super::der::{DerCurve, evaluate_der_curve};
use super::junction::{JunctionNode, NetworkDofLayout, evaluate_junctions};
use super::section_laws::{RodSectionLaw, SectionHistory};
use super::sparse_ldlt::{
    DirectOrdering, LdltFactor, LdltOptions, SymCsr, SymbolicLdlt, solve_dense_lu,
    solve_pcg_block_jacobi,
};
use super::{CodimError, DetMath, all_finite3, skew3};

/// One evaluated contact + frozen-friction stencil contribution over 4
/// positional vertices (12 global positional DOFs).
#[derive(Clone, Debug, PartialEq)]
pub struct ContactStencilEval {
    /// Global positional DOF indices `[v0x, v0y, v0z, ..., v3z]`.
    pub dofs_12: [usize; 12],
    /// Combined barrier, normal compliance, and frozen-friction potential (`J`).
    pub energy_j: f64,
    /// Analytical 12-DOF gradient vector.
    pub grad_12: [f64; 12],
    /// Symmetric SPD-projected 12x12 Hessian block.
    pub hess_spd_12: [[f64; 12]; 12],
}

/// Pure-kernel contact/CCD/friction bridge decoupling `fs-codim::kernel`
/// from `fs-contact` and `fs-tangle`.
pub trait CodimContactBridge {
    /// Evaluate all active contact & frozen-lag friction stencils at the
    /// current network state relative to `outer_start_positions`.
    fn evaluate_contact(
        &self,
        network: &RodNetwork,
        layout: &NetworkDofLayout,
        outer_start_positions: &[[f64; 3]],
    ) -> Result<Vec<ContactStencilEval>, CodimError>;

    /// Compute conservative penetration-free CCD step fraction `alpha_max in (0, 1]`
    /// from `current` positions to `trial` positions.
    fn ccd_max_step(
        &self,
        current: &RodNetwork,
        trial: &RodNetwork,
        layout: &NetworkDofLayout,
    ) -> Result<f64, CodimError>;

    /// Returns `true` if tangential lagged friction is active on this bridge.
    fn has_friction(&self) -> bool;

    /// Advance outer lagged friction state after an inner Newton solve;
    /// returns `(converged, infinity_norm_delta_lambda_f)`.
    fn update_outer_friction(
        &mut self,
        network: &RodNetwork,
        layout: &NetworkDofLayout,
        outer_start_positions: &[[f64; 3]],
    ) -> Result<(bool, f64), CodimError>;
}

/// Default contact-free [`CodimContactBridge`] for pure-kernel rod networks.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoContactBridge;

impl CodimContactBridge for NoContactBridge {
    fn evaluate_contact(
        &self,
        _network: &RodNetwork,
        _layout: &NetworkDofLayout,
        _outer_start_positions: &[[f64; 3]],
    ) -> Result<Vec<ContactStencilEval>, CodimError> {
        Ok(Vec::new())
    }

    fn ccd_max_step(
        &self,
        _current: &RodNetwork,
        _trial: &RodNetwork,
        _layout: &NetworkDofLayout,
    ) -> Result<f64, CodimError> {
        Ok(1.0)
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

/// Unified 1D rod network combining 6-DOF Cosserat rods, 4-DOF DER curves,
/// 6-DOF `SO(3)` junctions, Dirichlet pins, external loads, and committed
/// constitutive section histories.
#[derive(Clone, Debug, PartialEq)]
pub struct RodNetwork {
    /// 6-DOF Cosserat rods in the network.
    pub cosserat_rods: Vec<CosseratRod>,
    /// 4-DOF DER curves in the network.
    pub der_curves: Vec<DerCurve>,
    /// 6-DOF `SO(3)` junctions coupling incident arms or rigid clamps.
    pub junctions: Vec<JunctionNode>,
    /// Deterministic sorted set of Dirichlet-constrained global DOF indices.
    pub pinned_dofs: BTreeSet<usize>,
    /// Conservative external generalized forces `(global_dof, force_value)`.
    pub external_forces: Vec<(usize, f64)>,
    /// Non-conservative body-frame follower moments `(rot_dof_start, moment_body_nm)`.
    pub follower_moments: Vec<(usize, [f64; 3])>,
    /// Committed constitutive histories for each Cosserat rod segment.
    pub cosserat_histories: Vec<Vec<SectionHistory>>,
    /// Committed axial constitutive histories for each DER curve edge.
    pub der_edge_histories: Vec<Vec<SectionHistory>>,
    /// Committed bending/twist histories for each DER curve joint.
    pub der_joint_histories: Vec<Vec<SectionHistory>>,
}

impl RodNetwork {
    /// Construct a [`RodNetwork`] and initialize committed section histories
    /// from each segment's [`RodSectionLaw::initial_history`].
    pub fn new(
        cosserat_rods: Vec<CosseratRod>,
        der_curves: Vec<DerCurve>,
        junctions: Vec<JunctionNode>,
    ) -> Self {
        let cosserat_histories = cosserat_rods
            .iter()
            .map(|r| {
                r.sections
                    .iter()
                    .map(RodSectionLaw::initial_history)
                    .collect()
            })
            .collect();
        let der_edge_histories = der_curves
            .iter()
            .map(|c| {
                c.edge_sections
                    .iter()
                    .map(RodSectionLaw::initial_history)
                    .collect()
            })
            .collect();
        let der_joint_histories = der_curves
            .iter()
            .map(|c| {
                let j_count = c.joint_count();
                (0..j_count)
                    .map(|j| c.edge_sections[j].initial_history())
                    .collect()
            })
            .collect();
        Self {
            cosserat_rods,
            der_curves,
            junctions,
            pinned_dofs: BTreeSet::new(),
            external_forces: Vec::new(),
            follower_moments: Vec::new(),
            cosserat_histories,
            der_edge_histories,
            der_joint_histories,
        }
    }

    /// Compute deterministic global DOF layout offsets.
    #[must_use]
    pub fn layout(&self) -> NetworkDofLayout {
        let mut total = 0usize;
        let mut cosserat_offsets = Vec::with_capacity(self.cosserat_rods.len());
        for rod in &self.cosserat_rods {
            cosserat_offsets.push(total);
            total += 6 * rod.positions.len();
        }
        let mut der_offsets = Vec::with_capacity(self.der_curves.len());
        for curve in &self.der_curves {
            der_offsets.push(total);
            total += curve.dof_count();
        }
        let mut junction_offsets = Vec::with_capacity(self.junctions.len());
        for _ in &self.junctions {
            junction_offsets.push(total);
            total += 6;
        }
        NetworkDofLayout {
            cosserat_offsets,
            der_offsets,
            junction_offsets,
            total_dofs: total,
        }
    }

    /// Collect all nodal positions across Cosserat rods, DER curves, and
    /// junctions in deterministic layout order.
    #[must_use]
    pub fn collect_positions(&self) -> Vec<[f64; 3]> {
        let mut out = Vec::new();
        for r in &self.cosserat_rods {
            out.extend_from_slice(&r.positions);
        }
        for c in &self.der_curves {
            out.extend_from_slice(&c.positions);
        }
        for j in &self.junctions {
            out.push(j.position);
        }
        out
    }

    /// Returns `true` iff every segment across all Cosserat rods and DER curves
    /// uses a reversible section law (`is_reversible() == true`).
    #[must_use]
    pub fn all_sections_reversible(&self) -> bool {
        let c_rev = self
            .cosserat_rods
            .iter()
            .all(|r| r.sections.iter().all(RodSectionLaw::is_reversible));
        let d_rev = self
            .der_curves
            .iter()
            .all(|c| c.edge_sections.iter().all(RodSectionLaw::is_reversible));
        c_rev && d_rev
    }

    /// Validate network topology, DOFs, loads, and history lengths.
    pub fn validate<M: DetMath>(&self) -> Result<(), CodimError> {
        for r in &self.cosserat_rods {
            r.validate::<M>()?;
        }
        for c in &self.der_curves {
            c.validate::<M>()?;
        }
        for j in &self.junctions {
            j.validate::<M>()?;
        }
        if self.cosserat_histories.len() != self.cosserat_rods.len()
            || self.der_edge_histories.len() != self.der_curves.len()
            || self.der_joint_histories.len() != self.der_curves.len()
        {
            return Err(CodimError::InvalidTopology {
                detail: "RodNetwork history arrays length mismatch",
            });
        }
        let total = self.layout().total_dofs;
        for &dof in &self.pinned_dofs {
            if dof >= total {
                return Err(CodimError::InvalidTopology {
                    detail: "pinned DOF index out of bounds",
                });
            }
        }
        for &(dof, f) in &self.external_forces {
            if dof >= total {
                return Err(CodimError::InvalidTopology {
                    detail: "external_forces DOF index out of bounds",
                });
            }
            if !f.is_finite() {
                return Err(CodimError::NonFiniteInput {
                    context: "external_forces",
                });
            }
        }
        for &(dof, m) in &self.follower_moments {
            if dof + 2 >= total {
                return Err(CodimError::InvalidTopology {
                    detail: "follower_moments DOF index out of bounds",
                });
            }
            if !all_finite3(m) {
                return Err(CodimError::NonFiniteInput {
                    context: "follower_moments",
                });
            }
        }
        Ok(())
    }

    /// Apply a full-DOF increment `delta_full` scaled by `alpha`.
    pub fn apply_increment<M: DetMath>(
        &mut self,
        delta_full: &[f64],
        alpha: f64,
    ) -> Result<(), CodimError> {
        let layout = self.layout();
        if delta_full.len() != layout.total_dofs {
            return Err(CodimError::InvalidTopology {
                detail: "RodNetwork increment length mismatch",
            });
        }
        for (idx, rod) in self.cosserat_rods.iter_mut().enumerate() {
            let start = layout.cosserat_offsets[idx];
            let len = 6 * rod.positions.len();
            rod.apply_increment::<M>(&delta_full[start..start + len], alpha)?;
        }
        for (idx, curve) in self.der_curves.iter_mut().enumerate() {
            let start = layout.der_offsets[idx];
            let len = curve.dof_count();
            curve.apply_increment(&delta_full[start..start + len], alpha)?;
        }
        for (idx, junc) in self.junctions.iter_mut().enumerate() {
            let start = layout.junction_offsets[idx];
            let mut d6 = [0.0; 6];
            d6.copy_from_slice(&delta_full[start..start + 6]);
            junc.apply_increment::<M>(&d6, alpha)?;
        }
        Ok(())
    }

    /// Commit DER parallel-transport Bishop reference frames on all curves.
    pub fn commit_der_frames<M: DetMath>(&mut self) -> Result<(), CodimError> {
        for curve in &mut self.der_curves {
            curve.commit_reference_frames::<M>()?;
        }
        Ok(())
    }
}

/// Solver hyperparameters for [`solve_network`].
#[derive(Clone, Debug, PartialEq)]
pub struct SolverConfig {
    /// Number of proportional external load steps (`>= 1`).
    pub load_steps: usize,
    /// Maximum Newton iterations per outer round and load step.
    pub max_newton_iters: u32,
    /// Maximum Armijo / merit backtracking halvings per Newton iteration.
    pub max_line_search_iters: u32,
    /// Absolute free-DOF residual Euclidean norm tolerance.
    pub tol_residual: f64,
    /// Armijo sufficient-decrease slope parameter `c1 in (0, 0.5)`.
    pub armijo_c1: f64,
    /// Maximum outer lagged friction fixed-point rounds (`>= 1`).
    pub max_outer_friction_iters: u32,
    /// Sparse LDLT options.
    pub ldlt_options: LdltOptions,
    /// Use deterministic block-Jacobi PCG instead of sparse LDLT when `true`.
    pub use_pcg_fallback: bool,
    /// Block size for PCG preconditioning (`1`, `3`, or `6`).
    pub pcg_block_size: usize,
    /// Relative residual tolerance for PCG.
    pub pcg_tol_rel: f64,
    /// Maximum PCG iterations.
    pub pcg_max_iters: usize,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            load_steps: 1,
            max_newton_iters: 50,
            max_line_search_iters: 24,
            tol_residual: 1e-9,
            armijo_c1: 1e-4,
            max_outer_friction_iters: 16,
            ldlt_options: LdltOptions::default(),
            use_pcg_fallback: false,
            pcg_block_size: 3,
            pcg_tol_rel: 1e-12,
            pcg_max_iters: 4000,
        }
    }
}

/// Convergence and contact telemetry emitted by [`solve_network`].
#[derive(Clone, Debug, PartialEq)]
pub struct SolveReport {
    /// Total Newton iterations executed across all load steps and outer rounds.
    pub newton_iters: u32,
    /// Total outer lagged friction rounds executed in the final load step.
    pub outer_friction_iters: u32,
    /// Final free-DOF residual Euclidean norm.
    pub final_residual_norm: f64,
    /// Final potential energy (or internal strain energy when follower moments active) in Joules.
    pub final_energy_j: f64,
    /// Minimum CCD step fraction observed across all Newton steps (`1.0` if unobstructed).
    pub min_ccd_alpha: f64,
    /// Final infinity-norm friction update residual (`0.0` if frictionless).
    pub final_delta_lambda_inf: f64,
    /// Residual norms recorded per load step.
    pub step_residuals: Vec<f64>,
}

/// Converged equilibrium output from [`solve_network`], retaining the factored
/// tangent matrix for elastic single-solve IFT adjoints.
#[derive(Clone, Debug)]
pub struct SolveOutput {
    /// Factored symmetric CSR LDLT matrix at equilibrium (present when
    /// conservative and `free_dofs > 0` without PCG override).
    pub ldlt_factor: Option<LdltFactor>,
    /// Maps global DOF index `0..total_dofs` to free DOF index `0..free_dofs`,
    /// or `usize::MAX` if Dirichlet pinned.
    pub free_dof_map: Vec<usize>,
    /// Total global DOF count.
    pub total_dofs: usize,
    /// Free DOF count.
    pub free_dofs: usize,
    /// `true` iff every segment uses a reversible constitutive law.
    pub all_sections_reversible: bool,
    /// `true` iff tangential lagged friction was active.
    pub friction_active: bool,
    /// `true` iff non-conservative follower moments were active.
    pub follower_moment_active: bool,
    /// Convergence report.
    pub report: SolveReport,
}

struct TrialEvaluation {
    energy_j: f64,
    grad_full: Vec<f64>,
    internal_grad_full: Vec<f64>,
    hess_triplets: Vec<(usize, usize, f64)>,
    next_cosserat_hist: Vec<Vec<SectionHistory>>,
    next_der_edge_hist: Vec<Vec<SectionHistory>>,
    next_der_joint_hist: Vec<Vec<SectionHistory>>,
}

#[allow(clippy::too_many_lines)]
fn evaluate_network<M: DetMath, B: CodimContactBridge>(
    network: &RodNetwork,
    bridge: &B,
    layout: &NetworkDofLayout,
    outer_start_positions: &[[f64; 3]],
    load_scale: f64,
) -> Result<TrialEvaluation, CodimError> {
    let total_dofs = layout.total_dofs;
    let mut energy_j = 0.0;
    let mut internal_grad_full = vec![0.0_f64; total_dofs];
    let mut hess_triplets: Vec<(usize, usize, f64)> = Vec::new();

    let mut next_cosserat_hist = Vec::with_capacity(network.cosserat_rods.len());
    for (r_idx, rod) in network.cosserat_rods.iter().enumerate() {
        let base = layout.cosserat_offsets[r_idx];
        let segs = rod.segment_count();
        if network.cosserat_histories[r_idx].len() != segs {
            return Err(CodimError::InvalidTopology {
                detail: "cosserat_histories segment length mismatch",
            });
        }
        let mut seg_hists = Vec::with_capacity(segs);
        for seg in 0..segs {
            let eval = evaluate_cosserat_segment::<M, _>(
                rod.positions[seg],
                rod.quats[seg],
                rod.positions[seg + 1],
                rod.quats[seg + 1],
                rod.rest_lengths_m[seg],
                rod.rest_gamma[seg],
                rod.rest_kappa[seg],
                &rod.sections[seg],
                &network.cosserat_histories[r_idx][seg],
            )?;
            energy_j += eval.energy_j;
            seg_hists.push(eval.next_history);
            let d0 = base + 6 * seg;
            for i in 0..12 {
                internal_grad_full[d0 + i] += eval.grad_12[i];
                for j in 0..12 {
                    let v = eval.hess_12[i][j];
                    if v != 0.0 {
                        hess_triplets.push((d0 + i, d0 + j, v));
                    }
                }
            }
        }
        next_cosserat_hist.push(seg_hists);
    }

    let mut next_der_edge_hist = Vec::with_capacity(network.der_curves.len());
    let mut next_der_joint_hist = Vec::with_capacity(network.der_curves.len());
    for (c_idx, curve) in network.der_curves.iter().enumerate() {
        let base = layout.der_offsets[c_idx];
        let eval = evaluate_der_curve::<M>(
            curve,
            &network.der_edge_histories[c_idx],
            &network.der_joint_histories[c_idx],
        )?;
        energy_j += eval.energy_j;
        for (i, &g) in eval.grad.iter().enumerate() {
            internal_grad_full[base + i] += g;
        }
        for &(r, c, v) in &eval.hess_triplets {
            hess_triplets.push((base + r, base + c, v));
        }
        next_der_edge_hist.push(eval.next_edge_histories);
        next_der_joint_hist.push(eval.next_joint_histories);
    }

    if !network.junctions.is_empty() {
        let jeval = evaluate_junctions::<M>(
            &network.junctions,
            &network.cosserat_rods,
            &network.der_curves,
            layout,
        )?;
        energy_j += jeval.energy_j;
        for (i, &g) in jeval.grad.iter().enumerate() {
            internal_grad_full[i] += g;
        }
        hess_triplets.extend(jeval.hess_triplets);
    }

    let contacts = bridge.evaluate_contact(network, layout, outer_start_positions)?;
    for c in &contacts {
        energy_j += c.energy_j;
        for i in 0..12 {
            internal_grad_full[c.dofs_12[i]] += c.grad_12[i];
            for j in 0..12 {
                let v = c.hess_spd_12[i][j];
                if v != 0.0 {
                    hess_triplets.push((c.dofs_12[i], c.dofs_12[j], v));
                }
            }
        }
    }

    let mut grad_full = internal_grad_full.clone();
    // Conservative dead generalized forces: Pi = E_int - load_scale * f^T q.
    for &(dof, f) in &network.external_forces {
        let f_scaled = load_scale * f;
        grad_full[dof] -= f_scaled;
        // Note: potential Pi subtracts f_scaled * nodal coordinate (linear in q).
        energy_j -= f_scaled * get_dof_coord(network, layout, dof);
    }
    // Non-conservative body-frame follower moments enter residual directly.
    for &(dof0, m) in &network.follower_moments {
        grad_full[dof0] -= load_scale * m[0];
        grad_full[dof0 + 1] -= load_scale * m[1];
        grad_full[dof0 + 2] -= load_scale * m[2];
    }

    Ok(TrialEvaluation {
        energy_j,
        grad_full,
        internal_grad_full,
        hess_triplets,
        next_cosserat_hist,
        next_der_edge_hist,
        next_der_joint_hist,
    })
}

fn get_dof_coord(network: &RodNetwork, layout: &NetworkDofLayout, dof: usize) -> f64 {
    for (idx, rod) in network.cosserat_rods.iter().enumerate() {
        let start = layout.cosserat_offsets[idx];
        let len = 6 * rod.positions.len();
        if dof >= start && dof < start + len {
            let local = dof - start;
            let node = local / 6;
            let comp = local % 6;
            return if comp < 3 {
                rod.positions[node][comp]
            } else {
                0.0
            };
        }
    }
    for (idx, curve) in network.der_curves.iter().enumerate() {
        let start = layout.der_offsets[idx];
        let len = curve.dof_count();
        if dof >= start && dof < start + len {
            let local = dof - start;
            let pos_dofs = 3 * curve.positions.len();
            return if local < pos_dofs {
                curve.positions[local / 3][local % 3]
            } else {
                curve.edge_twists_rad[local - pos_dofs]
            };
        }
    }
    for (idx, junc) in network.junctions.iter().enumerate() {
        let start = layout.junction_offsets[idx];
        if dof >= start && dof < start + 6 {
            let comp = dof - start;
            return if comp < 3 { junc.position[comp] } else { 0.0 };
        }
    }
    0.0
}

fn build_free_dof_map(total_dofs: usize, pinned: &BTreeSet<usize>) -> (Vec<usize>, usize) {
    let mut map = vec![usize::MAX; total_dofs];
    let mut free = 0usize;
    for (i, slot) in map.iter_mut().enumerate() {
        if !pinned.contains(&i) {
            *slot = free;
            free += 1;
        }
    }
    (map, free)
}

fn extract_free_vec(v: &[f64], free_map: &[usize], n_free: usize) -> Vec<f64> {
    let mut out = vec![0.0_f64; n_free];
    for (i, &fi) in free_map.iter().enumerate() {
        if fi != usize::MAX {
            out[fi] = v[i];
        }
    }
    out
}

fn scatter_free_vec(v_free: &[f64], free_map: &[usize]) -> Vec<f64> {
    let mut out = vec![0.0_f64; free_map.len()];
    for (i, &fi) in free_map.iter().enumerate() {
        if fi != usize::MAX {
            out[i] = v_free[fi];
        }
    }
    out
}

fn build_free_sym_csr(
    triplets: &[(usize, usize, f64)],
    free_map: &[usize],
    n_free: usize,
) -> Result<SymCsr, CodimError> {
    let mut free_trips: Vec<(usize, usize, f64)> = Vec::new();
    for &(r, c, v) in triplets {
        let fr = free_map[r];
        let fc = free_map[c];
        if fr != usize::MAX && fc != usize::MAX {
            free_trips.push((fr, fc, v));
        }
    }
    SymCsr::from_triplets(n_free, &free_trips)
}

fn collect_rot_blocks(network: &RodNetwork, layout: &NetworkDofLayout) -> Vec<usize> {
    let mut rot_starts = Vec::new();
    for (idx, rod) in network.cosserat_rods.iter().enumerate() {
        let base = layout.cosserat_offsets[idx];
        for n in 0..rod.positions.len() {
            rot_starts.push(base + 6 * n + 3);
        }
    }
    for idx in 0..network.junctions.len() {
        rot_starts.push(layout.junction_offsets[idx] + 3);
    }
    rot_starts
}

fn dot_slice(a: &[f64], b: &[f64]) -> f64 {
    let mut s = 0.0;
    for (&x, &y) in a.iter().zip(b.iter()) {
        s += x * y;
    }
    s
}

/// Solve a coupled [`RodNetwork`] equilibrium using load-stepped implicit
/// damped Newton + CCD step capping + Armijo backtracking + outer lagged
/// friction loop, committing `SectionHistory` strictly upon step acceptance.
#[allow(clippy::too_many_lines)]
pub fn solve_network<M: DetMath, B: CodimContactBridge>(
    network: &mut RodNetwork,
    bridge: &mut B,
    cfg: &SolverConfig,
) -> Result<SolveOutput, CodimError> {
    if cfg.load_steps == 0 || cfg.max_outer_friction_iters == 0 {
        return Err(CodimError::InvalidSolverOptions {
            field: "load_steps and max_outer_friction_iters must be >= 1",
        });
    }
    if !cfg.tol_residual.is_finite()
        || cfg.tol_residual <= 0.0
        || !cfg.armijo_c1.is_finite()
        || cfg.armijo_c1 <= 0.0
        || cfg.armijo_c1 >= 0.5
    {
        return Err(CodimError::InvalidSolverOptions {
            field: "tol_residual > 0 and armijo_c1 in (0, 0.5) required",
        });
    }
    network.validate::<M>()?;
    let layout = network.layout();
    let (free_dof_map, n_free) = build_free_dof_map(layout.total_dofs, &network.pinned_dofs);
    let follower_active = network
        .follower_moments
        .iter()
        .any(|&(_, m)| m[0] != 0.0 || m[1] != 0.0 || m[2] != 0.0);
    let friction_active = bridge.has_friction();
    let all_rev = network.all_sections_reversible();
    let rot_starts = collect_rot_blocks(network, &layout);

    let mut total_newton_iters = 0_u32;
    let mut final_outer_iters = 0_u32;
    let mut final_delta_lambda = 0.0_f64;
    let mut min_ccd_alpha = 1.0_f64;
    let mut step_residuals = Vec::with_capacity(cfg.load_steps);
    let mut last_energy = 0.0_f64;
    let mut last_res_norm = 0.0_f64;
    let mut last_ldlt: Option<LdltFactor> = None;
    let mut cached_sym: Option<SymbolicLdlt> = None;

    for step in 1..=cfg.load_steps {
        let load_scale = step as f64 / cfg.load_steps as f64;
        let max_outer = if friction_active {
            cfg.max_outer_friction_iters
        } else {
            1
        };

        let mut outer_converged = !friction_active;
        let mut outer_rounds = 0_u32;

        for outer in 1..=max_outer {
            outer_rounds = outer;
            let outer_start_positions = network.collect_positions();
            let mut inner_converged = false;

            for _ in 0..cfg.max_newton_iters {
                let eval = evaluate_network::<M, B>(
                    network,
                    bridge,
                    &layout,
                    &outer_start_positions,
                    load_scale,
                )?;
                last_energy = eval.energy_j;
                let r_free = extract_free_vec(&eval.grad_full, &free_dof_map, n_free);
                let r_norm = M::sqrt(dot_slice(&r_free, &r_free));
                last_res_norm = r_norm;

                if r_norm < cfg.tol_residual || n_free == 0 {
                    // Commit trial histories at converged state (if any).
                    network.cosserat_histories = eval.next_cosserat_hist;
                    network.der_edge_histories = eval.next_der_edge_hist;
                    network.der_joint_histories = eval.next_der_joint_hist;
                    network.commit_der_frames::<M>()?;

                    if !follower_active && n_free > 0 && !cfg.use_pcg_fallback {
                        let csr = build_free_sym_csr(&eval.hess_triplets, &free_dof_map, n_free)?;
                        let sym = match &cached_sym {
                            Some(s)
                                if s.n() == n_free
                                    && SymbolicLdlt::factor(s, &csr, &cfg.ldlt_options).is_ok() =>
                            {
                                last_ldlt = Some(SymbolicLdlt::factor(s, &csr, &cfg.ldlt_options)?);
                                None
                            }
                            _ => Some(SymbolicLdlt::analyze(&csr, DirectOrdering::Amd)?),
                        };
                        if let Some(s) = sym {
                            last_ldlt = Some(s.factor(&csr, &cfg.ldlt_options)?);
                            cached_sym = Some(s);
                        }
                    } else {
                        last_ldlt = None;
                    }
                    inner_converged = true;
                    break;
                }

                total_newton_iters += 1;
                let neg_r: Vec<f64> = r_free.iter().map(|x| -x).collect();

                let delta_free = if follower_active {
                    let mut kmat = vec![0.0_f64; n_free * n_free];
                    for &(r, c, v) in &eval.hess_triplets {
                        let fr = free_dof_map[r];
                        let fc = free_dof_map[c];
                        if fr != usize::MAX && fc != usize::MAX {
                            kmat[fr * n_free + fc] += v;
                        }
                    }
                    // Left-trivialized Lie-bracket skew term: +0.5 * [g_int_rot]_x.
                    for &rs in &rot_starts {
                        let sk = skew3([
                            eval.internal_grad_full[rs],
                            eval.internal_grad_full[rs + 1],
                            eval.internal_grad_full[rs + 2],
                        ]);
                        for i in 0..3 {
                            let fr = free_dof_map[rs + i];
                            if fr == usize::MAX {
                                continue;
                            }
                            for j in 0..3 {
                                let fc = free_dof_map[rs + j];
                                if fc != usize::MAX {
                                    kmat[fr * n_free + fc] += 0.5 * sk[i][j];
                                }
                            }
                        }
                    }
                    solve_dense_lu(&kmat, &neg_r, n_free)?
                } else {
                    let csr = build_free_sym_csr(&eval.hess_triplets, &free_dof_map, n_free)?;
                    if cfg.use_pcg_fallback {
                        solve_pcg_block_jacobi::<M>(
                            &csr,
                            &neg_r,
                            cfg.pcg_block_size,
                            cfg.pcg_tol_rel,
                            cfg.pcg_max_iters,
                        )?
                        .x
                    } else {
                        let factor = match &cached_sym {
                            Some(s) if s.n() == n_free => {
                                if let Ok(f) = s.factor(&csr, &cfg.ldlt_options) {
                                    f
                                } else {
                                    let s_new = SymbolicLdlt::analyze(&csr, DirectOrdering::Amd)?;
                                    let f = s_new.factor(&csr, &cfg.ldlt_options)?;
                                    cached_sym = Some(s_new);
                                    f
                                }
                            }
                            _ => {
                                let s_new = SymbolicLdlt::analyze(&csr, DirectOrdering::Amd)?;
                                let f = s_new.factor(&csr, &cfg.ldlt_options)?;
                                cached_sym = Some(s_new);
                                f
                            }
                        };
                        factor.solve(&neg_r)
                    }
                };

                let delta_full = scatter_free_vec(&delta_free, &free_dof_map);

                // Compute CCD penetration-free cap on full step.
                let mut ccd_probe = network.clone();
                ccd_probe.apply_increment::<M>(&delta_full, 1.0)?;
                let alpha_ccd = bridge.ccd_max_step(network, &ccd_probe, &layout)?;
                if !alpha_ccd.is_finite() || alpha_ccd <= 0.0 {
                    return Err(CodimError::NewtonStalled {
                        iters: total_newton_iters,
                        residual: r_norm,
                    });
                }
                let capped_alpha = if alpha_ccd < 1.0 {
                    min_ccd_alpha = min_ccd_alpha.min(0.99 * alpha_ccd);
                    0.99 * alpha_ccd
                } else {
                    1.0
                };

                let dir_deriv = dot_slice(&r_free, &delta_free);
                let use_energy_armijo = !follower_active && dir_deriv < 0.0;
                let mut alpha = capped_alpha;
                let mut accepted = false;

                for _ in 0..cfg.max_line_search_iters {
                    let mut trial = network.clone();
                    trial.apply_increment::<M>(&delta_full, alpha)?;
                    if let Ok(t_eval) = evaluate_network::<M, B>(
                        &trial,
                        bridge,
                        &layout,
                        &outer_start_positions,
                        load_scale,
                    ) {
                        let rt_free = extract_free_vec(&t_eval.grad_full, &free_dof_map, n_free);
                        let rt_norm = M::sqrt(dot_slice(&rt_free, &rt_free));
                        let ok = if use_energy_armijo {
                            t_eval.energy_j <= eval.energy_j + cfg.armijo_c1 * alpha * dir_deriv
                                || (rt_norm < r_norm
                                    && t_eval.energy_j
                                        <= eval.energy_j + 1e-12 * eval.energy_j.abs().max(1.0))
                        } else {
                            rt_norm < r_norm
                        };
                        if ok {
                            // Strictly commit histories & DER reference frames ONLY on acceptance.
                            trial.cosserat_histories = t_eval.next_cosserat_hist;
                            trial.der_edge_histories = t_eval.next_der_edge_hist;
                            trial.der_joint_histories = t_eval.next_der_joint_hist;
                            trial.commit_der_frames::<M>()?;
                            *network = trial;
                            accepted = true;
                            break;
                        }
                    }
                    alpha *= 0.5;
                }

                if !accepted {
                    return Err(CodimError::NewtonStalled {
                        iters: total_newton_iters,
                        residual: r_norm,
                    });
                }
            }

            if !inner_converged {
                return Err(CodimError::NewtonStalled {
                    iters: total_newton_iters,
                    residual: last_res_norm,
                });
            }

            if friction_active {
                let (conv, d_lam) =
                    bridge.update_outer_friction(network, &layout, &outer_start_positions)?;
                final_delta_lambda = d_lam;
                if conv {
                    outer_converged = true;
                    break;
                }
            }
        }

        if !outer_converged {
            return Err(CodimError::OuterLagDidNotConverge {
                residual: final_delta_lambda,
                rounds: outer_rounds,
            });
        }
        final_outer_iters = outer_rounds;
        step_residuals.push(last_res_norm);
    }

    Ok(SolveOutput {
        ldlt_factor: last_ldlt,
        free_dof_map,
        total_dofs: layout.total_dofs,
        free_dofs: n_free,
        all_sections_reversible: all_rev,
        friction_active,
        follower_moment_active: follower_active,
        report: SolveReport {
            newton_iters: total_newton_iters,
            outer_friction_iters: final_outer_iters,
            final_residual_norm: last_res_norm,
            final_energy_j: last_energy,
            min_ccd_alpha,
            final_delta_lambda_inf: final_delta_lambda,
            step_residuals,
        },
    })
}
