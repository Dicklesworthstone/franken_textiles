//! Convergence-driven time subdivision of the existing implicit step, not a new integrator.
//! Every failed attempt is discarded, including friction lag, factors and material memories.

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::{SolveReport, Solver};

/// Explicit work limits for convergence-driven bisection. This controls solver feasibility,
/// not truncation error: different accepted time partitions can produce different dynamics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubdivisionOptions {
    /// Maximum bisections of a nominal interval, in 0..=16. Zero requires one converged step.
    pub max_halvings: usize,
    /// Maximum attempted implicit steps, including failed attempts, in 1..=65535.
    /// The ordinary drape consumer shares this budget across both sewing and draping.
    pub max_trials: usize,
}

impl Default for SubdivisionOptions {
    fn default() -> Self {
        Self {
            max_halvings: 8,
            max_trials: 4096,
        }
    }
}

impl SubdivisionOptions {
    /// Validate the policy and the smallest time interval it permits, before any simulation.
    ///
    /// # Errors
    /// Invalid work limits, nonpositive/nonfinite time, or an unrepresentable squared step size.
    pub fn validate(&self, duration_s: f64) -> Result<(), Refusal> {
        if self.max_halvings > 16 || !(1..=65535).contains(&self.max_trials) {
            return Err(invalid(
                "subdivision needs max_halvings in 0..=16 and max_trials in 1..=65535",
            ));
        }
        let smallest = duration_s / (1_u32 << self.max_halvings) as f64;
        if !duration_s.is_finite()
            || duration_s <= 0.0
            || !(duration_s * duration_s).is_finite()
            || smallest * smallest == 0.0
        {
            return Err(invalid(
                "subdivision needs a positive finite duration and representable squared substeps",
            ));
        }
        Ok(())
    }
}

/// One accepted physical substep. Its endpoint is relative to the nominal interval's start.
#[derive(Clone, Debug, PartialEq)]
pub struct AcceptedSubstep {
    /// Endpoint in seconds, obtained from an exact dyadic fraction of the nominal duration.
    pub end_s: f64,
    /// Actual time increment passed to the existing implicit solver, in seconds.
    pub step_s: f64,
    /// Bisection depth; zero means the nominal interval was accepted without subdivision.
    pub depth: usize,
    /// The unchanged implicit solver's report for this accepted physical substep.
    pub solve: SolveReport,
}

/// Returned only after the whole nominal interval succeeds. Failed trials never supply samples.
#[derive(Clone, Debug, PartialEq)]
pub struct SubdividedStep {
    /// Requested elapsed physical time; never shortened to make the solve pass.
    pub duration_s: f64,
    /// Actual implicit-step attempts, including unconverged attempts discarded before bisection.
    pub trials: usize,
    /// Accepted chronological time partition and solver reports, bounded by `max_trials`.
    pub accepted: Vec<AcceptedSubstep>,
}

fn invalid(message: &str) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, message)
}

impl Solver {
    /// Advance one nominal interval under the current constant loads, fixed constraints and body.
    /// Retry ONLY nonconvergence by deterministic left-before-right time bisection. The same
    /// constitutive, contact, CCD, friction and frankensim-derived sparse Newton kernels run at
    /// each actual step size. No tolerance, thickness, source domain or material law is relaxed.
    ///
    /// The entire interval commits or refuses. Errors after internally accepted substeps preserve
    /// the caller's positions, velocities, contact lag, factor caches and every material history.
    /// Even the memoryless solver's `Ok` with `converged=false` is NOT an accepted substep here.
    /// Constant constraints do not interpolate prescribed loading; use the existing moving-body
    /// API for moving colliders. This is not local-error control or a time-convergence certificate.
    ///
    /// # Errors
    /// Invalid policy/state, exhausted work or bisection depth, or any nonconvergence-independent
    /// refusal from the existing solver (in particular invalid material domains and contact).
    pub fn step_subdivided(
        &mut self,
        duration_s: f64,
        options: SubdivisionOptions,
    ) -> Result<SubdividedStep, Refusal> {
        options.validate(duration_s)?;
        if self.x.len() != self.model.rest.len()
            || self.v.len() != self.x.len()
            || self.x.iter().chain(&self.v).any(|v| !v.is_finite())
            || !self.gravity.is_finite()
            || !self.tol.is_finite()
            || self.tol <= 0.0
            || self.max_newton == 0
        {
            return Err(invalid(
                "subdivision needs consistent finite state and positive solver limits",
            ));
        }
        // Reject an invalid starting state once; subdivision is not a material-domain repair.
        self.membrane_energy()?;
        self.panel_bending_energy()?;
        if let Some(contact) = &self.contact {
            contact.audit_self_contact(&self.x)?;
        }

        let ticks = 1_u32 << options.max_halvings;
        let mut pending = vec![(0_u32, ticks, 0_usize)];
        let mut state = self.clone();
        let mut result = SubdividedStep {
            duration_s,
            trials: 0,
            accepted: Vec::new(),
        };
        // Integer dyadic intervals cover exactly [0,ticks]; no accumulated floating-time residue
        // can silently omit the end or request an extra tiny step. The stack is depth-bounded.
        while let Some((start, end, depth)) = pending.pop() {
            if result.trials == options.max_trials {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    format!(
                        "subdivision exhausted {} trials after {} accepted substeps; the whole interval is unchanged",
                        result.trials,
                        result.accepted.len()
                    ),
                ));
            }
            let step_s = duration_s * ((end - start) as f64 / ticks as f64);
            let mut candidate = state.clone();
            result.trials += 1;
            let failure = match candidate.step(step_s) {
                Ok(solve) if solve.converged => {
                    if candidate
                        .x
                        .iter()
                        .chain(&candidate.v)
                        .any(|v| !v.is_finite())
                    {
                        return Err(Refusal::new(
                            RefusalCode::NonFinite,
                            "subdivision produced non-finite state",
                        ));
                    }
                    state = candidate;
                    result.accepted.push(AcceptedSubstep {
                        end_s: duration_s * (end as f64 / ticks as f64),
                        step_s,
                        depth,
                        solve,
                    });
                    continue;
                }
                Ok(solve) => Refusal::new(
                    RefusalCode::NotConverged,
                    format!(
                        "implicit step did not converge in {} iterations (residual {} N)",
                        solve.iterations, solve.residual
                    ),
                ),
                Err(error) if error.code == RefusalCode::NotConverged => error,
                Err(error) => return Err(error),
            };
            if depth == options.max_halvings {
                return Err(Refusal::new(
                    RefusalCode::NotConverged,
                    format!(
                        "subdivision reached depth {depth} at {step_s} s after {} trials; whole interval unchanged: {}",
                        result.trials, failure.message
                    ),
                ));
            }
            let middle = start + (end - start) / 2;
            pending.push((middle, end, depth + 1));
            pending.push((start, middle, depth + 1));
        }
        *self = state;
        Ok(result)
    }
}
