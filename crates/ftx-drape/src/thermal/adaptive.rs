//! Error-controlled time integration of the existing conservative thermal model.
//!
//! Compare one backward-Euler step with two half steps (first-order step doubling). Keep the two
//! actual half steps, never Richardson-extrapolated temperatures or a synthetic flux. Local error
//! estimates control temperature AND each signed boundary heat separately; they are not global
//! accuracy certificates. Reference: GNU GSL ODE manual, step doubling and adaptive error control.
//! https://www.gnu.org/software/gsl/doc/html/ode-initval.html

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::{Boundary, DryThermalModel, State, invalid, nonnegative, positive};

/// Local step-doubling tolerances and aggregate work limits; no global error guarantee.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// Positive absolute temperature-difference tolerance, K.
    pub absolute_temperature_tolerance_k: f64,
    /// Positive absolute tolerance for each signed boundary heat, J/m².
    pub absolute_heat_tolerance_j_m2: f64,
    /// Relative tolerance, 0..=0.1; temperature magnitudes use absolute kelvin.
    pub relative_tolerance: f64,
    /// Minimum candidate interval, s. Its accepted half steps may be half this length.
    pub min_step_s: f64,
    /// Maximum candidate interval, s; longer holds are subdivided.
    pub max_step_s: f64,
    /// Maximum attempted coarse-versus-fine comparisons across this budget's holds.
    pub max_trials: usize,
    /// Maximum charged cells across all attempted advances, including rejected trials.
    pub max_cell_solves: usize,
}

impl Options {
    /// Validate finite tolerances, representable intervals and bounded work.
    ///
    /// # Errors
    /// Invalid tolerance, interval ordering, underflowing half interval or work bounds.
    pub fn validate(&self) -> Result<(), Refusal> {
        positive(
            "absolute temperature tolerance",
            self.absolute_temperature_tolerance_k,
        )?;
        positive(
            "absolute boundary heat tolerance",
            self.absolute_heat_tolerance_j_m2,
        )?;
        nonnegative("relative thermal tolerance", self.relative_tolerance)?;
        positive("minimum thermal interval", self.min_step_s)?;
        positive("maximum thermal interval", self.max_step_s)?;
        if self.relative_tolerance > 0.1
            || self.max_step_s < self.min_step_s
            || self.min_step_s / 2.0 == 0.0
            || !(1..=100_000).contains(&self.max_trials)
            || !(1..=16_777_216).contains(&self.max_cell_solves)
        {
            return Err(invalid(
                "invalid adaptive thermal tolerances, interval or work bounds",
            ));
        }
        Ok(())
    }
}

/// Shared attempt accounting for one or more adaptive holds; failed attempts remain charged.
#[derive(Clone, Debug)]
pub struct Budget {
    options: Options,
    trials: usize,
    cell_solves: usize,
}

impl Budget {
    /// Start an unused budget after validating its options.
    ///
    /// # Errors
    /// The same invalid-option refusals as [`Options::validate`].
    pub fn new(options: Options) -> Result<Self, Refusal> {
        options.validate()?;
        Ok(Self {
            options,
            trials: 0,
            cell_solves: 0,
        })
    }

    #[must_use]
    /// Cumulative `[trial attempts, charged cell solves]`, including failed work.
    pub fn used(&self) -> [usize; 2] {
        [self.trials, self.cell_solves]
    }

    fn trial(&mut self) -> Result<(), Refusal> {
        if self.trials == self.options.max_trials {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "adaptive thermal trial budget exhausted",
            ));
        }
        self.trials += 1;
        Ok(())
    }

    fn solve(
        &mut self,
        model: &DryThermalModel,
        old: &[f64],
        seconds: f64,
        boundary: Boundary,
    ) -> Result<State, Refusal> {
        if model.cells() > self.options.max_cell_solves - self.cell_solves {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "adaptive thermal cell-solve budget exhausted",
            ));
        }
        self.cell_solves += model.cells();
        model.advance(old, seconds, boundary)
    }
}

/// Actual work and accepted local error estimates for a completed hold.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Trial comparisons charged during this hold.
    pub trials: usize,
    /// Comparisons rejected by local error or a nonlinear convergence failure.
    pub rejected_trials: usize,
    /// Accepted physical half steps; two per accepted comparison.
    pub accepted_substeps: usize,
    /// Cells charged to attempted advances during this hold.
    pub cell_solves: usize,
    /// Shortest accepted physical half step, s.
    pub min_accepted_step_s: f64,
    /// Longest accepted physical half step, s.
    pub max_accepted_step_s: f64,
    /// Largest accepted normalized local error, at most one; not a global bound.
    pub max_accepted_error_ratio: f64,
    /// Trial attempts charged across all holds using the shared budget.
    pub cumulative_trials: usize,
    /// Cells charged across all holds using the shared budget.
    pub cumulative_cell_solves: usize,
}

/// Final temperatures and separately integrated signed boundary heat for a whole hold.
#[derive(Clone, Debug)]
pub struct IntegratedState {
    /// Final half-step state with whole-hold storage/balance; fluxes remain final-step values.
    pub state: State,
    /// Signed heat entering the inside boundary over the whole hold, J/m².
    pub inside_heat_j_m2: f64,
    /// Signed heat leaving the outside boundary over the whole hold, J/m².
    pub outside_heat_j_m2: f64,
    /// Local error and work accounting for this completed hold.
    pub integration: Report,
}

fn heat(seconds: f64, flux: f64) -> Result<f64, Refusal> {
    let value = seconds * flux;
    if !value.is_finite() || (flux != 0.0 && value == 0.0) {
        return Err(invalid("adaptive boundary heat is not representable"));
    }
    Ok(value)
}

fn ratio(a: f64, b: f64, absolute: f64, relative: f64, offset: f64) -> Result<f64, Refusal> {
    let tolerance = absolute + relative * (a + offset).abs().max((b + offset).abs());
    let error = (a - b).abs();
    let value = error / tolerance;
    if !tolerance.is_finite() || !error.is_finite() || !value.is_finite() {
        return Err(invalid(
            "adaptive thermal error estimate is not representable",
        ));
    }
    Ok(value)
}

fn split(
    pending: &mut Vec<(f64, f64, usize)>,
    start: f64,
    end: f64,
    depth: usize,
    min: f64,
) -> Result<(), Refusal> {
    let mid = start + (end - start) / 2.0;
    if depth == 60 || mid <= start || mid >= end || (mid - start).min(end - mid) < min {
        return Err(Refusal::new(
            RefusalCode::NotConverged,
            "adaptive thermal tolerance or maximum interval cannot be met within the minimum interval/60 subdivisions",
        ));
    }
    pending.push((mid, end, depth + 1));
    pending.push((start, mid, depth + 1));
    Ok(())
}

impl DryThermalModel {
    /// Integrate a constant-boundary hold with accepted backward-Euler half steps.
    /// Inputs are temperatures in °C and duration in seconds; heat follows reference area.
    /// No partial state is returned on refusal; attempted work stays charged to `budget`.
    ///
    /// # Errors
    /// Invalid cells, temperatures, films or duration; exhausted work; unrepresentable steps/heat;
    /// unattainable local tolerances; nonlinear or whole-hold conservation failure.
    pub fn advance_adaptive(
        &self,
        old: &[f64],
        seconds: f64,
        boundary: Boundary,
        budget: &mut Budget,
    ) -> Result<IntegratedState, Refusal> {
        positive("thermal hold duration", seconds)?;
        if old.len() != self.cells() || seconds < budget.options.min_step_s {
            return Err(invalid(
                "adaptive hold needs matching cells and duration at least min_step_s",
            ));
        }
        for &t in old
            .iter()
            .chain([boundary.inside_c, boundary.outside_c].iter())
        {
            self.check_temperature(t)?;
        }
        nonnegative("inner film resistance", boundary.inside_resistance)?;
        nonnegative("outer film resistance", boundary.outside_resistance)?;
        let before = budget.used();
        let options = budget.options.clone();
        let mut pending = vec![(0.0, seconds, 0)];
        let mut next = old.to_vec();
        let (mut inside, mut outside, mut round) = (0.0, 0.0, 0.0);
        let mut last = None;
        let mut report = Report {
            trials: 0,
            rejected_trials: 0,
            accepted_substeps: 0,
            cell_solves: 0,
            min_accepted_step_s: f64::MAX,
            max_accepted_step_s: 0.0,
            max_accepted_error_ratio: 0.0,
            cumulative_trials: 0,
            cumulative_cell_solves: 0,
        };
        while let Some((start, end, depth)) = pending.pop() {
            let dt = end - start;
            if dt > options.max_step_s {
                split(&mut pending, start, end, depth, options.min_step_s)?;
                continue;
            }
            let mid = start + dt / 2.0;
            let (h1, h2) = (mid - start, end - mid);
            if h1 <= 0.0 || h2 <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::NotConverged,
                    "thermal half-step time is not representable",
                ));
            }
            budget.trial()?;
            let trial = (|| {
                let coarse = budget.solve(self, &next, dt, boundary)?;
                let first = budget.solve(self, &next, h1, boundary)?;
                let fine = budget.solve(self, &first.temperature_c, h2, boundary)?;
                Ok::<_, Refusal>((coarse, first, fine))
            })();
            let (coarse, first, fine) = match trial {
                Ok(states) => states,
                Err(error) if error.code == RefusalCode::NotConverged => {
                    report.rejected_trials += 1;
                    split(&mut pending, start, end, depth, options.min_step_s)?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let qi = heat(h1, first.inside_flux_w_m2)? + heat(h2, fine.inside_flux_w_m2)?;
            let qo = heat(h1, first.outside_flux_w_m2)? + heat(h2, fine.outside_flux_w_m2)?;
            let mut error: f64 = 0.0;
            for (&a, &b) in fine.temperature_c.iter().zip(&coarse.temperature_c) {
                error = error.max(ratio(
                    a,
                    b,
                    options.absolute_temperature_tolerance_k,
                    options.relative_tolerance,
                    273.15,
                )?);
            }
            for (a, b) in [
                (qi, heat(dt, coarse.inside_flux_w_m2)?),
                (qo, heat(dt, coarse.outside_flux_w_m2)?),
            ] {
                error = error.max(ratio(
                    a,
                    b,
                    options.absolute_heat_tolerance_j_m2,
                    options.relative_tolerance,
                    0.0,
                )?);
            }
            if error > 1.0 {
                report.rejected_trials += 1;
                split(&mut pending, start, end, depth, options.min_step_s)?;
                continue;
            }
            for (i, ((&a, &b), &c)) in next
                .iter()
                .zip(&first.temperature_c)
                .zip(&fine.temperature_c)
                .enumerate()
            {
                round += self
                    .cell_capacity(i, a)?
                    .max(self.cell_capacity(i, b)?)
                    .max(self.cell_capacity(i, c)?)
                    * (1.0 + a.abs() + b.abs() + c.abs());
            }
            inside += qi;
            outside += qo;
            if [inside, outside, round].iter().any(|v| !v.is_finite()) {
                return Err(invalid(
                    "adaptive integrated heat or rounding bound overflows",
                ));
            }
            report.accepted_substeps += 2;
            report.min_accepted_step_s = report.min_accepted_step_s.min(h1.min(h2));
            report.max_accepted_step_s = report.max_accepted_step_s.max(h1.max(h2));
            report.max_accepted_error_ratio = report.max_accepted_error_ratio.max(error);
            next.clone_from(&fine.temperature_c);
            last = Some(fine);
        }
        let stored = old
            .iter()
            .zip(&next)
            .enumerate()
            .try_fold(0.0, |sum, (i, (&a, &b))| {
                self.cell_heat(i, a, b).map(|q| sum + q)
            })?;
        let error = stored - (inside - outside);
        let tolerance = 1024.0 * f64::EPSILON * round
            + 1e-9
                * (report.accepted_substeps as f64 + stored.abs() + inside.abs() + outside.abs());
        if [stored, error, tolerance].iter().any(|v| !v.is_finite()) || error.abs() > tolerance {
            return Err(invalid("adaptive whole-hold energy conservation failed"));
        }
        let mut state =
            last.ok_or_else(|| invalid("adaptive hold has no accepted thermal state"))?;
        state.stored_change_j_m2 = stored;
        state.balance_error_j_m2 = error;
        report.trials = budget.trials - before[0];
        report.cell_solves = budget.cell_solves - before[1];
        report.cumulative_trials = budget.trials;
        report.cumulative_cell_solves = budget.cell_solves;
        Ok(IntegratedState {
            state,
            inside_heat_j_m2: inside,
            outside_heat_j_m2: outside,
            integration: report,
        })
    }
}
