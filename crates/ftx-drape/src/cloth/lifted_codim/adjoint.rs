//! Elastic-only single-solve Implicit Function Theorem (IFT) adjoints reusing
//! the converged symmetric CSR [`LdltFactor`] from [`super::solver::SolveOutput`].
//!
//! Explicitly refuses with
//! [`CodimError::IrreversibleHistoryRequiresCheckpointAdjoint`] whenever any
//! segment uses a path-dependent constitutive law (`SuperelasticSma` or
//! `ElastoplasticYield`), whenever tangential lagged friction is active, or
//! whenever non-conservative follower moments are active.

use super::CodimError;
use super::solver::SolveOutput;

/// Result of an elastic single-solve IFT sensitivity evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct AdjointResult {
    /// Full-DOF adjoint multiplier vector `lambda` (`0.0` on Dirichlet DOFs).
    pub lambda: Vec<f64>,
    /// Total objective derivatives `dJ/dp_k = d_obj_dp[k] + lambda^T * dg_dp[k]`.
    pub d_obj_dp_total: Vec<f64>,
}

/// Compute single-solve Implicit Function Theorem (IFT) sensitivities
/// `dJ/dp = partial_J/partial_p + lambda^T * partial_g/partial_p` where
/// `H^T * lambda = -grad_q_obj` at the converged conservative elastic
/// equilibrium `equilibrium`.
///
/// # Errors
/// Fails closed with [`CodimError::IrreversibleHistoryRequiresCheckpointAdjoint`]
/// if `equilibrium` contains any irreversible section history (`SuperelasticSma`,
/// `ElastoplasticYield`), active lagged tangential friction, or active
/// non-conservative follower moments.
pub fn solve_elastic_ift_adjoint(
    equilibrium: &SolveOutput,
    grad_q_obj: &[f64],
    dg_dp: &[&[f64]],
    d_obj_dp: &[f64],
) -> Result<AdjointResult, CodimError> {
    if !equilibrium.all_sections_reversible {
        return Err(CodimError::IrreversibleHistoryRequiresCheckpointAdjoint {
            reason: "irreversible constitutive section law (SuperelasticSma or ElastoplasticYield) active",
        });
    }
    if equilibrium.friction_active {
        return Err(CodimError::IrreversibleHistoryRequiresCheckpointAdjoint {
            reason: "outer lagged tangential friction active",
        });
    }
    if equilibrium.follower_moment_active {
        return Err(CodimError::IrreversibleHistoryRequiresCheckpointAdjoint {
            reason: "non-conservative follower moment active",
        });
    }
    if grad_q_obj.len() != equilibrium.total_dofs {
        return Err(CodimError::InvalidTopology {
            detail: "grad_q_obj length mismatch with total_dofs",
        });
    }
    if dg_dp.len() != d_obj_dp.len() {
        return Err(CodimError::InvalidTopology {
            detail: "dg_dp and d_obj_dp parameter counts mismatch",
        });
    }
    if !grad_q_obj.iter().all(|v| v.is_finite()) || !d_obj_dp.iter().all(|v| v.is_finite()) {
        return Err(CodimError::NonFiniteInput {
            context: "solve_elastic_ift_adjoint",
        });
    }
    for col in dg_dp {
        if col.len() != equilibrium.total_dofs {
            return Err(CodimError::InvalidTopology {
                detail: "dg_dp column length mismatch with total_dofs",
            });
        }
        if !col.iter().all(|v| v.is_finite()) {
            return Err(CodimError::NonFiniteInput {
                context: "solve_elastic_ift_adjoint::dg_dp",
            });
        }
    }

    let mut lambda = vec![0.0_f64; equilibrium.total_dofs];
    if equilibrium.free_dofs > 0 {
        let factor = equilibrium.ldlt_factor.as_ref().ok_or(CodimError::InvalidTopology {
            detail: "converged equilibrium has no factored symmetric LdltFactor (PCG mode or unfactored)",
        })?;
        let mut rhs_free = vec![0.0_f64; equilibrium.free_dofs];
        for (i, &fi) in equilibrium.free_dof_map.iter().enumerate() {
            if fi != usize::MAX {
                rhs_free[fi] = -grad_q_obj[i];
            }
        }
        // Symmetric conservative equilibrium: H^T = H, reuse converged LDLT factor.
        let lambda_free = factor.solve(&rhs_free);
        for (i, &fi) in equilibrium.free_dof_map.iter().enumerate() {
            if fi != usize::MAX {
                lambda[i] = lambda_free[fi];
            }
        }
    }

    let mut d_obj_dp_total = Vec::with_capacity(d_obj_dp.len());
    for (k, &base_d) in d_obj_dp.iter().enumerate() {
        let mut dot = 0.0_f64;
        for i in 0..equilibrium.total_dofs {
            dot += lambda[i] * dg_dp[k][i];
        }
        d_obj_dp_total.push(base_d + dot);
    }

    Ok(AdjointResult {
        lambda,
        d_obj_dp_total,
    })
}
