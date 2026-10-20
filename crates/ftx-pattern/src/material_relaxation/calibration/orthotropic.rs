//! Independent positive-semidefinite Maxwell tensors at fixed or bounded identified times.
//! The continuous history weights and compiled physical response are shared with the existing law.

use std::f64::consts::SQRT_2;

use ftx_core::{Refusal, RefusalCode, linalg::lstsq};
use serde::{Deserialize, Serialize};

use super::{History, Initialization, Input, Partition, Report, bad, datum, fit, piola};
use crate::material_relaxation::{
    CompiledRelaxation, Memory, OrthotropicBranch, OrthotropicStiffness, Relaxation,
};

const MAX_PARAMETERS: usize = 32;
type Vector = [f64; MAX_PARAMETERS];
type Matrix = [Vector; MAX_PARAMETERS];

mod search;
pub(super) use search::Budget as ComparisonBudget;
pub use search::TimeSearchReport;

/// Identify spring tensors at fixed times or jointly identify times inside explicit search windows.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrthotropicInput {
    /// One to eight finite, positive, strictly increasing candidate times in seconds.
    /// Omit or leave empty when time_bounds_seconds supplies the directional time search instead.
    #[serde(default)]
    pub times_seconds: Vec<f64>,
    /// One to three ordered, nonoverlapping positive time windows. These are search limits, not
    /// confidence intervals. Mutually exclusive with fixed times_seconds; omitted in fixed mode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub time_bounds_seconds: Vec<[f64; 2]>,
    /// Explicit coefficient threshold in N/m for retaining inactive directions/candidates as zero.
    /// Defaults to no threshold. The resulting compiled law must still pass both residual budgets.
    #[serde(default)]
    pub zero_threshold_n_m: f64,
}

/// Every supplied candidate remains visible even if its fitted tensor is inactive.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    /// Caller-supplied fixed time, or the identified time when time_search diagnostics are present.
    pub time_seconds: f64,
    /// Unconstrained least-squares [warp,weft,coupling,shear], N/m; may be physically inadmissible.
    pub unconstrained_stiffness_n_m: [f64; 4],
    /// Positive-semidefinite optimum before the caller's explicit zero threshold, N/m.
    pub fitted_stiffness_n_m: [f64; 4],
    /// Actual published [warp,weft,coupling,shear] after that threshold, N/m.
    pub stiffness_n_m: [f64; 4],
    /// Whether a nonzero tensor at this candidate time appears in the ordinary material law.
    pub active: bool,
}

/// Directional identification diagnostics, not uncertainty or empirical material validation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrthotropicFitReport {
    /// All times and coefficient decisions in input order; no candidate disappears from the receipt.
    pub candidates: Vec<Candidate>,
    /// Whether the unconstrained solution required the declared PSD/nonnegative physical constraints.
    pub psd_projection_applied: bool,
    /// Projected-gradient iterations; zero means the unconstrained optimum was already admissible.
    pub iterations: usize,
    /// Relative norm of x-project(x-gradient/L), in Frobenius tensor coordinates, before thresholding.
    pub projected_step_relative: f64,
    /// Optional bounded time-search diagnostics. Omitted to preserve fixed-time report identities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_search: Option<TimeSearchReport>,
}

fn physical(x: &[f64]) -> [f64; 4] {
    [x[0], x[1], x[2] / SQRT_2, x[3]]
}

fn feasible(q: [f64; 4]) -> bool {
    let [a, b, c, g] = q;
    a >= 0.
        && b >= 0.
        && g >= 0.
        && if a == 0. || b == 0. {
            c == 0.
        } else {
            let l21 = c / a.sqrt();
            b - l21 * l21 >= 0.
        }
}

// Frobenius coordinates [Q11,Q22,sqrt(2)*Q12,Q66] make the ordinary Euclidean projection the
// exact two-dimensional PSD eigenprojection, independently of the nonnegative shear direction.
fn project(mut x: Vector, count: usize) -> Result<Vector, Refusal> {
    for values in x[..count].as_chunks_mut::<4>().0 {
        if values.iter().any(|v| !v.is_finite()) {
            return Err(bad("orthotropic relaxation constraint arithmetic overflow"));
        }
        values[3] = values[3].max(0.);
        let [a, b, c, g] = physical(values);
        if !feasible([a, b, c, g]) {
            let scale = a.abs().max(b.abs()).max(c.abs());
            let (a, b, c) = (a / scale, b / scale, c / scale);
            let mid = 0.5 * (a + b);
            let diff = 0.5 * (a - b);
            let radius = (diff * diff + c * c).sqrt();
            let high = mid + radius;
            if high <= 0. {
                values[..3].fill(0.);
            } else if mid < radius {
                let (u, v) = if diff >= 0. {
                    (radius + diff, c)
                } else {
                    (c, radius - diff)
                };
                let norm = (u * u + v * v).sqrt();
                let (u, v) = (u / norm, v / norm);
                let eigenvalue = scale * high;
                values[0] = eigenvalue * u * u;
                values[1] = eigenvalue * v * v;
                values[2] = (eigenvalue * u * v) * SQRT_2;
            }
            // A rank-one eigenprojection may round a coupling one ULP outside the production
            // Cholesky test. Move only that rounding inward; larger failures remain refusals.
            for _ in 0..8 {
                if feasible(physical(values)) {
                    break;
                }
                values[2] = if values[2] > 0. {
                    values[2].next_down()
                } else {
                    values[2].next_up()
                };
            }
        }
        if values.iter().any(|v| !v.is_finite()) || !feasible(physical(values)) {
            return Err(bad(
                "orthotropic relaxation PSD projection is unrepresentable",
            ));
        }
    }
    Ok(x)
}

fn design(histories: &[History], times: &[f64]) -> Result<(Vec<Vector>, Vec<f64>), Refusal> {
    let mut rows = Vec::new();
    let mut target = Vec::new();
    for history in histories.iter().filter(|h| h.partition == Partition::Fit) {
        let initial = if history.initialization == Initialization::StressFreeReference {
            history.points[0].strain
        } else {
            [0.; 3]
        };
        let mut lag = vec![initial; times.len()];
        for (i, point) in history.points.iter().enumerate() {
            let mut basis = [[0.; MAX_PARAMETERS]; 4];
            for (branch, (&tau, lag)) in times.iter().zip(&mut lag).enumerate() {
                if i > 0 {
                    let previous = &history.points[i - 1];
                    let (a, phi, _, _) = fit::weights(point.time - previous.time, tau);
                    for (axis, value) in lag.iter_mut().enumerate() {
                        *value = a * *value + phi * (point.strain[axis] - previous.strain[axis]);
                    }
                }
                for (j, stress) in [
                    [lag[0], 0., 0.],
                    [0., lag[1], 0.],
                    [lag[1] / SQRT_2, lag[0] / SQRT_2, 0.],
                    [0., 0., lag[2]],
                ]
                .into_iter()
                .enumerate()
                {
                    for (row, force) in basis.iter_mut().zip(piola(point.f, stress)) {
                        row[4 * branch + j] = force / point.scale;
                    }
                }
            }
            for (j, &observed) in point.observed.iter().enumerate() {
                if let Some(force) = observed {
                    rows.push(basis[j]);
                    target.push((force - point.equilibrium[j]) / point.scale);
                }
            }
        }
    }
    if rows.len() <= 4 * times.len() || rows.iter().flatten().chain(&target).any(|v| !v.is_finite())
    {
        return Err(bad(
            "insufficient or unrepresentable orthotropic fit force observations",
        ));
    }
    Ok((rows, target))
}

struct Solution {
    unconstrained: Vector,
    fitted: Vector,
    sensitivity: Vec<f64>,
    pivot: f64,
    iterations: usize,
    projected: bool,
    relative: f64,
}

fn gradient_step(x: Vector, gram: &Matrix, rhs: &Vector, lipschitz: f64, count: usize) -> Vector {
    let mut step = x;
    for i in 0..count {
        let gradient = gram[i][..count]
            .iter()
            .zip(&x[..count])
            .map(|(a, b)| a * b)
            .sum::<f64>()
            - rhs[i];
        step[i] -= gradient / lipschitz;
    }
    step
}

fn relative_step(x: &Vector, projected: &Vector, count: usize) -> f64 {
    let scale = x[..count].iter().map(|v| v.abs()).fold(1., f64::max);
    x[..count]
        .iter()
        .zip(&projected[..count])
        .map(|(a, b)| ((a - b) / scale).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn solve(rows: &[Vector], target: &[f64], count: usize) -> Result<Solution, Refusal> {
    solve_budgeted(rows, target, count, None)
}

fn solve_budgeted(
    rows: &[Vector],
    target: &[f64],
    count: usize,
    mut remaining_steps: Option<&mut usize>,
) -> Result<Solution, Refusal> {
    // A profile-objective gradient needs a more accurate inner stationary point. Keep the original
    // standalone fixed-time arithmetic; compared fixed and searched models use the same refinement.
    let tolerance = if remaining_steps.is_some() {
        1e-13
    } else {
        1e-11
    };
    let scales: Vector = std::array::from_fn(|j| {
        if j < count {
            rows.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt()
        } else {
            1.
        }
    });
    if scales.iter().any(|v| !v.is_finite() || *v <= 1e-12) {
        return Err(bad(
            "orthotropic histories do not excite every candidate tensor coefficient",
        ));
    }
    let normalized: Vec<Vector> = rows
        .iter()
        .map(|r| std::array::from_fn(|j| r[j] / scales[j]))
        .collect();
    let mut gram = [[0.; MAX_PARAMETERS]; MAX_PARAMETERS];
    let mut l = gram;
    let mut pivot = 1_f64;
    for i in 0..count {
        for j in 0..=i {
            let g = normalized.iter().map(|r| r[i] * r[j]).sum::<f64>();
            gram[i][j] = g;
            gram[j][i] = g;
            let v = g - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if i == j {
                if !v.is_finite() || v <= 1e-8 {
                    return Err(bad(
                        "rank-deficient orthotropic spectrum; add independent directional/time histories or reduce candidates",
                    ));
                }
                pivot = pivot.min(v);
                l[i][j] = v.sqrt();
            } else {
                l[i][j] = v / l[j][j];
            }
        }
    }
    let fitted = lstsq(&normalized, target);
    let unconstrained: Vector = std::array::from_fn(|j| fitted[j] / scales[j]);
    let mut x = project(unconstrained, count)?;
    let projected = x != unconstrained;
    let common_scale = scales[..count].iter().copied().fold(0., f64::max);
    let mut rhs = [0.; MAX_PARAMETERS];
    for i in 0..count {
        rhs[i] = normalized
            .iter()
            .zip(target)
            .map(|(r, y)| r[i] * y)
            .sum::<f64>()
            / common_scale
            * (scales[i] / common_scale);
        for j in 0..count {
            gram[i][j] *= (scales[i] / common_scale) * (scales[j] / common_scale);
        }
    }
    let lipschitz = gram[..count]
        .iter()
        .map(|r| r[..count].iter().map(|v| v.abs()).sum::<f64>())
        .fold(0., f64::max);
    if !lipschitz.is_finite() || lipschitz <= 0. || rhs.iter().any(|v| !v.is_finite()) {
        return Err(bad("orthotropic relaxation normal-equation overflow"));
    }
    let mut y = x;
    let mut momentum = 1_f64;
    let mut iterations = 0;
    let mut relative;
    loop {
        let projected_step = project(gradient_step(x, &gram, &rhs, lipschitz, count), count)?;
        relative = relative_step(&x, &projected_step, count);
        if relative <= tolerance {
            break;
        }
        if iterations == 8192 {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                "orthotropic PSD fit did not converge within 8192 projected-gradient steps",
            ));
        }
        if let Some(remaining) = remaining_steps.as_deref_mut() {
            if *remaining == 0 {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "directional time search exhausted its total PSD iteration budget",
                ));
            }
            *remaining -= 1;
        }
        let next = project(gradient_step(y, &gram, &rhs, lipschitz, count), count)?;
        // Restart oscillating momentum against the actual projected search direction. Without
        // this, differently scaled warp/weft force sensors can spend the budget circling a PSD
        // boundary even when the constrained optimum is well resolved by their observations.
        let restart = y[..count]
            .iter()
            .zip(&next[..count])
            .zip(&x[..count])
            .map(|((y, next), x)| (y - next) * (next - x))
            .sum::<f64>()
            > 0.;
        let next_momentum = if restart {
            1.
        } else {
            0.5 * (1. + (1. + 4. * momentum * momentum).sqrt())
        };
        if restart {
            momentum = 1.;
        }
        y = std::array::from_fn(|j| next[j] + ((momentum - 1.) / next_momentum) * (next[j] - x[j]));
        x = next;
        momentum = next_momentum;
        iterations += 1;
    }
    let sensitivity = scales[..count]
        .iter()
        .enumerate()
        .map(|(j, &s)| if j % 4 == 2 { s * SQRT_2 } else { s })
        .collect();
    Ok(Solution {
        unconstrained,
        fitted: x,
        sensitivity,
        pivot,
        iterations,
        projected,
        relative,
    })
}

fn threshold(mut q: [f64; 4], limit: f64) -> [f64; 4] {
    if q[2].abs() <= limit {
        q[2] = 0.;
        if q[0] <= limit {
            q[0] = 0.;
        }
        if q[1] <= limit {
            q[1] = 0.;
        }
    }
    if q[3] <= limit {
        q[3] = 0.;
    }
    q
}

fn law(input: &Input, candidates: &[Candidate], source: &str) -> Relaxation {
    let searched = input
        .orthotropic
        .as_ref()
        .is_some_and(|o| !o.time_bounds_seconds.is_empty());
    let quantity = |value, unit| {
        let mut q = datum(value, unit, source);
        q.conditions = input.conditions.clone();
        q.method = if searched {
            "Bounded-time orthotropic PSD identification from complete nominal-force histories; tensors and times are fitted model estimates, not measured constants or confidence bounds"
        } else {
            "Fixed-candidate-time orthotropic PSD identification from complete nominal-force histories; candidate times are supplied, not optimized; all fitted coefficients remain estimates"
        }.into();
        q
    };
    Relaxation {
        model: "green_maxwell/1".into(),
        branches: Vec::new(),
        orthotropic_branches: candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.active)
            .map(|(i, c)| {
                let [a, b, k, g] = c.stiffness_n_m;
                OrthotropicBranch {
                    id: format!("fitted-orthotropic-{}", i + 1),
                    stiffness: OrthotropicStiffness {
                        warp: quantity(a, "N/m"),
                        weft: quantity(b, "N/m"),
                        coupling: quantity(k, "N/m"),
                        shear: quantity(g, "N/m"),
                    },
                    relaxation_time: quantity(c.time_seconds, "s"),
                }
            })
            .collect(),
        initialization: input.initialization,
        strain_range: input.strain_range,
        source: input.domain_source.clone(),
        method: if searched {
            format!(
                "{}; bounded-time orthotropic Maxwell PSD identification; exact piecewise-linear Green-strain history integration",
                input.method
            )
        } else {
            format!(
                "{}; fixed-time orthotropic Maxwell PSD fit; exact piecewise-linear Green-strain history integration",
                input.method
            )
        },
        conditions: input.conditions.clone(),
    }
}

// Advance the exact continuous acquisition state, then ask the actual compiled physical law for
// its endpoint stress. Production garment steps remain backward Euler; no second cloth solver.
fn replay(
    history: &History,
    compiled: Option<&CompiledRelaxation>,
) -> Result<Vec<[f64; 4]>, Refusal> {
    let Some(compiled) = compiled else {
        return Ok(history.points.iter().map(|p| p.equilibrium).collect());
    };
    let initial = history.points[0].strain;
    let z = if history.initialization == Initialization::StressFreeReference {
        [0.; 3]
    } else {
        initial
    };
    let mut memory = Memory {
        law: compiled.clone(),
        viscous: vec![z; compiled.branches.len()],
    };
    let mut forces = Vec::new();
    for (i, point) in history.points.iter().enumerate() {
        if i > 0 {
            let previous = &history.points[i - 1];
            for (branch, z) in compiled.branches.iter().zip(&mut memory.viscous) {
                let (a, phi, _, _) = fit::weights(point.time - previous.time, branch.tau);
                for axis in 0..3 {
                    let lag = a * (previous.strain[axis] - z[axis])
                        + phi * (point.strain[axis] - previous.strain[axis]);
                    z[axis] = point.strain[axis] - lag;
                }
            }
        }
        let transient = piola(point.f, memory.response(point.strain, None)?.stress);
        let force = std::array::from_fn(|j| point.equilibrium[j] + transient[j]);
        if force.iter().any(|v| !v.is_finite()) {
            return Err(bad("orthotropic replay force overflow"));
        }
        forces.push(force);
    }
    Ok(forces)
}

pub(super) fn calibrate(input: &Input, options: &OrthotropicInput) -> Result<Report, Refusal> {
    calibrate_with_policy(input, options, None)
}

pub(super) fn calibrate_candidate(
    input: &Input,
    options: &OrthotropicInput,
    budget: &mut ComparisonBudget,
) -> Result<Report, Refusal> {
    calibrate_with_policy(input, options, Some(budget))
}

// The original single-model API enforces both residual gates. Comparison candidates defer CHECK
// replay until every FIT-only choice has been fixed, and share one numerical work allowance.
fn calibrate_with_policy(
    input: &Input,
    options: &OrthotropicInput,
    mut budget: Option<&mut ComparisonBudget>,
) -> Result<Report, Refusal> {
    let comparing = budget.is_some();
    let searched = !options.time_bounds_seconds.is_empty();
    if (!searched
        && (!(1..=8).contains(&options.times_seconds.len())
            || options
                .times_seconds
                .iter()
                .any(|t| !t.is_finite() || *t <= 0.)
            || options.times_seconds.windows(2).any(|t| t[0] >= t[1])))
        || (searched && !options.times_seconds.is_empty())
        || !options.zero_threshold_n_m.is_finite()
        || options.zero_threshold_n_m < 0.
    {
        return Err(bad(
            "orthotropic fitting needs fixed candidate times OR separate bounded time windows, and a nonnegative finite N/m zero threshold",
        ));
    }
    if searched {
        search::bounds(&options.time_bounds_seconds)?;
    }
    let (histories, q) = super::prepare(input)?;
    // Invoke the existing directional source/condition validator before numerical work, even for
    // an eventual all-zero fit. This unit tensor is only a validation scaffold, never a material.
    let scaffold = Candidate {
        time_seconds: if searched {
            options.time_bounds_seconds[0][0]
        } else {
            options.times_seconds[0]
        },
        unconstrained_stiffness_n_m: [0.; 4],
        fitted_stiffness_n_m: [0.; 4],
        stiffness_n_m: [1., 1., 0., 1.],
        active: true,
    };
    let source_check = law(input, &[scaffold], &input.domain_source);
    source_check.validate(&input.material.sources, true)?;
    source_check.validate_equilibrium(&input.material)?;
    if input
        .material
        .structure
        .get("specimen_id")
        .is_some_and(|s| input.conditions.get("specimen_id") != Some(s))
    {
        return Err(bad(
            "orthotropic histories and material name different specimens",
        ));
    }
    let (times, fit, time_search) = if searched {
        let result = match budget.as_deref_mut() {
            Some(budget) => {
                search::solve_budgeted(&histories, &options.time_bounds_seconds, budget)?
            }
            None => search::solve(&histories, &options.time_bounds_seconds)?,
        };
        (result.times, result.fit, Some(result.report))
    } else {
        let fit = match budget {
            Some(budget) => budget.fixed(&histories, &options.times_seconds)?,
            None => {
                let (rows, target) = design(&histories, &options.times_seconds)?;
                solve(&rows, &target, 4 * options.times_seconds.len())?
            }
        };
        (options.times_seconds.clone(), fit, None)
    };
    let candidates: Vec<_> = times
        .iter()
        .enumerate()
        .map(|(i, &time_seconds)| {
            let fitted_stiffness_n_m = physical(&fit.fitted[4 * i..4 * i + 4]);
            let stiffness_n_m = threshold(fitted_stiffness_n_m, options.zero_threshold_n_m);
            Candidate {
                time_seconds,
                unconstrained_stiffness_n_m: physical(&fit.unconstrained[4 * i..4 * i + 4]),
                fitted_stiffness_n_m,
                stiffness_n_m,
                active: stiffness_n_m.iter().any(|v| *v != 0.),
            }
        })
        .collect();
    if searched && candidates.iter().any(|c| !c.active) {
        return Err(bad(
            "an optimized directional branch is inactive after the explicit threshold; its time is not identified",
        ));
    }
    let (mut material, source, digest) = super::receipt(input)?;
    let fitted = law(input, &candidates, &source);
    let compiled = if fitted.orthotropic_branches.is_empty() {
        None
    } else {
        Some(fitted.compile(&material.sources, true, q)?)
    };
    let (predictions, fit_quality, check) = if comparing {
        super::evaluate_residuals(input, &histories, false, false, |h| {
            replay(h, compiled.as_ref())
        })?
    } else {
        super::residuals(input, &histories, |h| replay(h, compiled.as_ref()))?
    };
    material.membrane_relaxation = compiled.map(|_| fitted);
    material.validate()?;
    let missing_mechanics = material
        .missing_mechanics()
        .into_iter()
        .map(str::to_owned)
        .collect();
    Ok(Report {
        schema: "ftx-relaxation-calibration-report/1".into(),
        input: input.clone(),
        input_sha256: digest,
        material,
        fit: fit_quality,
        check,
        predictions,
        excitation_pivot: fit.pivot,
        parameter_sensitivity: fit.sensitivity,
        starts_attempted: time_search.as_ref().map_or(1, |s| s.starts_attempted),
        orthotropic_fit: Some(OrthotropicFitReport {
            candidates,
            psd_projection_applied: fit.projected,
            iterations: fit.iterations,
            projected_step_relative: fit.relative,
            time_search,
        }),
        missing_mechanics,
    })
}

pub(super) fn replay_checks(report: &mut Report) -> Result<(), Refusal> {
    let (histories, q) = super::prepare(&report.input)?;
    let compiled = report
        .material
        .membrane_relaxation
        .as_ref()
        .map(|law| law.compile(&report.material.sources, true, q))
        .transpose()?;
    let (predictions, fit, check) =
        super::evaluate_residuals(&report.input, &histories, true, false, |history| {
            replay(history, compiled.as_ref())
        })?;
    report.predictions = predictions;
    report.fit = fit;
    report.check = check;
    Ok(())
}
