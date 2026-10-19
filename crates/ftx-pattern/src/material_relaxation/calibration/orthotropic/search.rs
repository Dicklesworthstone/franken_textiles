//! Bounded variable projection: the existing convex tensor fit inside at most three log-times.
//! Envelope gradients differentiate the minimized FIT objective, not a frozen-amplitude residual
//! Jacobian. Held-out forces, residual acceptance budgets and zero thresholds never enter this search.

use ftx_core::{Refusal, RefusalCode, det};
use serde::{Deserialize, Serialize};

use super::{History, Initialization, Partition, Solution, bad, fit, physical, piola};

const MAX_EVALUATIONS: usize = 1024;
const MAX_PSD_STEPS: usize = 262_144;
const MAX_OUTER_STEPS: usize = 120;
const GRADIENT_TOLERANCE: f64 = 1e-10;

/// Bounded directional time identification diagnostics, not confidence intervals or global proof.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeSearchReport {
    /// Deterministic 3^n initial log-time combinations attempted.
    pub starts_attempted: usize,
    /// Starts that reached the numerical objective-stationarity criterion.
    pub starts_converged: usize,
    /// Accepted outer iterations for the selected FIT minimum.
    pub iterations: usize,
    /// All attempted objective evaluations, including failed/backtracked trials, at most 1024.
    pub objective_evaluations: usize,
    /// Total projected tensor iterations across all evaluations, at most 262144.
    pub projected_iterations: usize,
    /// Max |dSSE/dlog(tau)| / [2 * log-time force sensitivity * (1+sqrt(SSE))].
    pub gradient_scaled_max: f64,
    /// Norms of scaled FIT force derivatives with respect to each log-time, at fixed fitted tensors.
    pub time_sensitivity: Vec<f64>,
    /// Smallest normalized Gram Cholesky pivot for all tensor coefficients followed by log-times.
    pub joint_excitation_pivot: f64,
    /// Minimum distance to either log-window boundary divided by window width, for each time.
    pub boundary_distance_fraction: Vec<f64>,
}

pub(super) struct SearchSolution {
    pub times: Vec<f64>,
    pub fit: Solution,
    pub report: TimeSearchReport,
}

pub(super) fn bounds(windows: &[[f64; 2]]) -> Result<Vec<[f64; 2]>, Refusal> {
    if !(1..=3).contains(&windows.len()) {
        return Err(bad(
            "directional time identification needs one to three separate search windows",
        ));
    }
    let mut previous = 0.;
    let mut result = Vec::new();
    for &[lo, hi] in windows {
        if !lo.is_finite() || !hi.is_finite() || lo <= previous || hi <= lo {
            return Err(bad(
                "directional time windows must be positive, ordered and nonoverlapping",
            ));
        }
        let window = [det::ln(lo), det::ln(hi)];
        if window.iter().any(|v| !v.is_finite()) || window[1] - window[0] < 1e-6 {
            return Err(bad("directional time window is not numerically resolvable"));
        }
        result.push(window);
        previous = hi;
    }
    Ok(result)
}

fn time_jacobian(
    histories: &[History],
    times: &[f64],
    tensors: &super::Vector,
) -> Result<Vec<[f64; 3]>, Refusal> {
    let mut rows = Vec::new();
    for history in histories.iter().filter(|h| h.partition == Partition::Fit) {
        let initial = if history.initialization == Initialization::StressFreeReference {
            history.points[0].strain
        } else {
            [0.; 3]
        };
        let mut lag = vec![initial; times.len()];
        let mut derivative = vec![[0.; 3]; times.len()];
        for (i, point) in history.points.iter().enumerate() {
            let mut jacobian = [[0.; 3]; 4];
            for branch in 0..times.len() {
                if i > 0 {
                    let previous = &history.points[i - 1];
                    let (a, phi, dphi, da) =
                        fit::weights(point.time - previous.time, times[branch]);
                    for axis in 0..3 {
                        let change = point.strain[axis] - previous.strain[axis];
                        derivative[branch][axis] =
                            a * derivative[branch][axis] + da * lag[branch][axis] + dphi * change;
                        lag[branch][axis] = a * lag[branch][axis] + phi * change;
                    }
                }
                let [a, b, c, g] = physical(&tensors[4 * branch..4 * branch + 4]);
                let [x, y, shear] = derivative[branch];
                let force = piola(point.f, [a * x + c * y, c * x + b * y, g * shear]);
                for (row, value) in jacobian.iter_mut().zip(force) {
                    row[branch] = value / point.scale;
                }
            }
            for (observed, row) in point.observed.iter().zip(jacobian) {
                if observed.is_some() {
                    rows.push(row);
                }
            }
        }
    }
    if rows.iter().flatten().any(|v| !v.is_finite()) {
        return Err(bad("directional time sensitivity overflow"));
    }
    Ok(rows)
}

struct Evaluation {
    times: Vec<f64>,
    fit: Solution,
    cost: f64,
    gradient: [f64; 3],
    sensitivity: [f64; 3],
    gradient_scaled_max: f64,
}

struct Budget {
    evaluations: usize,
    remaining_psd: usize,
}

fn evaluate(
    histories: &[History],
    p: [f64; 3],
    n: usize,
    budget: &mut Budget,
) -> Result<Evaluation, Refusal> {
    if budget.evaluations == MAX_EVALUATIONS {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "directional time search exhausted 1024 total objective evaluations",
        ));
    }
    budget.evaluations += 1;
    let times: Vec<_> = p[..n].iter().map(|&v| det::exp(v)).collect();
    if times.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err(bad("unrepresentable directional relaxation time"));
    }
    let (rows, target) = super::design(histories, &times)?;
    let fitted = super::solve_budgeted(&rows, &target, 4 * n, Some(&mut budget.remaining_psd))?;
    let residual: Vec<_> = rows
        .iter()
        .zip(target)
        .map(|(row, observed)| {
            row[..4 * n]
                .iter()
                .zip(&fitted.fitted[..4 * n])
                .map(|(a, b)| a * b)
                .sum::<f64>()
                - observed
        })
        .collect();
    let jacobian = time_jacobian(histories, &times, &fitted.fitted)?;
    let cost = residual.iter().map(|v| v * v).sum::<f64>();
    let gradient = std::array::from_fn(|j| {
        2. * residual
            .iter()
            .zip(&jacobian)
            .map(|(r, row)| r * row[j])
            .sum::<f64>()
    });
    let sensitivity: [f64; 3] =
        std::array::from_fn(|j| jacobian.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt());
    if !cost.is_finite() || gradient.iter().chain(&sensitivity).any(|v| !v.is_finite()) {
        return Err(bad("directional time objective overflow"));
    }
    let gradient_scaled_max = (0..n)
        .map(|j| gradient[j].abs() / (2. * sensitivity[j].max(1e-30) * (1. + cost.sqrt())))
        .fold(0., f64::max);
    Ok(Evaluation {
        times,
        fit: fitted,
        cost,
        gradient,
        sensitivity,
        gradient_scaled_max,
    })
}

fn initial_inverse(e: &Evaluation, n: usize) -> [[f64; 3]; 3] {
    let mut inverse = [[0.; 3]; 3];
    for (i, row) in inverse.iter_mut().enumerate().take(n) {
        row[i] = 1. / (2. * e.sensitivity[i] * e.sensitivity[i]).max(1e-20);
    }
    inverse
}

struct Minimum {
    p: [f64; 3],
    evaluation: Evaluation,
    iterations: usize,
}

fn optimize(
    histories: &[History],
    bounds: &[[f64; 2]],
    mut p: [f64; 3],
    budget: &mut Budget,
) -> Result<Option<Minimum>, Refusal> {
    let n = bounds.len();
    let mut current = match evaluate(histories, p, n, budget) {
        Ok(e) => e,
        Err(e) if e.code == RefusalCode::BudgetExceeded => return Err(e),
        Err(_) => return Ok(None),
    };
    let mut inverse = initial_inverse(&current, n);
    for iteration in 0..=MAX_OUTER_STEPS {
        if current.gradient_scaled_max <= GRADIENT_TOLERANCE {
            return Ok(Some(Minimum {
                p,
                evaluation: current,
                iterations: iteration,
            }));
        }
        if iteration == MAX_OUTER_STEPS {
            break;
        }
        let mut direction: [f64; 3] = std::array::from_fn(|i| {
            -(0..n)
                .map(|j| inverse[i][j] * current.gradient[j])
                .sum::<f64>()
        });
        let mut slope: f64 = (0..n).map(|j| direction[j] * current.gradient[j]).sum();
        if !slope.is_finite() || slope >= 0. {
            inverse = initial_inverse(&current, n);
            direction = std::array::from_fn(|j| -inverse[j][j] * current.gradient[j]);
            slope = (0..n).map(|j| direction[j] * current.gradient[j]).sum();
        }
        if !slope.is_finite() || slope >= 0. {
            return Ok(None);
        }
        // The trust step bounds numerical log-time movement, never clamps a fitted physical time.
        let largest = direction[..n].iter().map(|v| v.abs()).fold(1., f64::max);
        direction = direction.map(|v| v / largest);
        slope /= largest;
        let mut fraction = 1_f64;
        for j in 0..n {
            let allowed = if direction[j] > 0. {
                (bounds[j][1] - p[j]) / direction[j]
            } else if direction[j] < 0. {
                (bounds[j][0] - p[j]) / direction[j]
            } else {
                f64::INFINITY
            };
            fraction = fraction.min(0.95 * allowed);
        }
        let mut accepted = None;
        for _ in 0..20 {
            let trial = std::array::from_fn(|j| p[j] + fraction * direction[j]);
            if bounds
                .iter()
                .enumerate()
                .any(|(j, [lo, hi])| trial[j] <= *lo || trial[j] >= *hi)
            {
                fraction *= 0.5;
                continue;
            }
            match evaluate(histories, trial, n, budget) {
                Ok(next)
                    if next.cost < current.cost
                        && next.cost <= current.cost + 1e-4 * fraction * slope =>
                {
                    accepted = Some((trial, next));
                    break;
                }
                Err(e) if e.code == RefusalCode::BudgetExceeded => return Err(e),
                _ => {
                    fraction *= 0.5;
                }
            }
        }
        let Some((next_p, next)) = accepted else {
            return Ok(None);
        };
        let step: [f64; 3] = std::array::from_fn(|j| next_p[j] - p[j]);
        let change: [f64; 3] = std::array::from_fn(|j| next.gradient[j] - current.gradient[j]);
        let curvature: f64 = (0..n).map(|j| step[j] * change[j]).sum();
        let magnitude = ((0..n).map(|j| step[j] * step[j]).sum::<f64>()
            * (0..n).map(|j| change[j] * change[j]).sum::<f64>())
        .sqrt();
        if curvature.is_finite() && curvature > 1e-12 * magnitude && curvature > 0. {
            let hy: [f64; 3] =
                std::array::from_fn(|i| (0..n).map(|j| inverse[i][j] * change[j]).sum::<f64>());
            let yhy: f64 = (0..n).map(|j| change[j] * hy[j]).sum();
            for i in 0..n {
                for j in 0..n {
                    inverse[i][j] += ((1. + yhy / curvature) * step[i] * step[j]
                        - hy[i] * step[j]
                        - step[i] * hy[j])
                        / curvature;
                }
            }
            if inverse.iter().flatten().any(|v| !v.is_finite()) {
                inverse = initial_inverse(&next, n);
            }
        } else {
            inverse = initial_inverse(&next, n);
        }
        p = next_p;
        current = next;
    }
    Ok(None)
}

fn joint_excitation(histories: &[History], e: &Evaluation) -> Result<f64, Refusal> {
    let n = e.times.len();
    let (amplitude, _) = super::design(histories, &e.times)?;
    let time = time_jacobian(histories, &e.times, &e.fit.fitted)?;
    let count = 5 * n;
    if amplitude.len() <= count {
        return Err(bad(
            "insufficient independent observations for directional tensors and times",
        ));
    }
    let rows: Vec<[f64; 15]> = amplitude
        .iter()
        .zip(time)
        .map(|(a, t)| {
            std::array::from_fn(|j| {
                if j < 4 * n {
                    a[j]
                } else if j < count {
                    t[j - 4 * n]
                } else {
                    0.
                }
            })
        })
        .collect();
    let scales: [f64; 15] = std::array::from_fn(|j| {
        if j < count {
            rows.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt()
        } else {
            1.
        }
    });
    if scales[..count]
        .iter()
        .any(|v| !v.is_finite() || *v <= 1e-10)
    {
        return Err(bad(
            "directional branch amplitude or optimized time is not observably excited",
        ));
    }
    let mut l = [[0.; 15]; 15];
    let mut pivot = 1_f64;
    for i in 0..count {
        for j in 0..=i {
            let v = rows
                .iter()
                .map(|r| (r[i] / scales[i]) * (r[j] / scales[j]))
                .sum::<f64>()
                - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if i == j {
                if !v.is_finite() || v <= 1e-7 {
                    return Err(bad(
                        "directional times are ambiguous with tensor amplitudes or one another; add independent time coverage",
                    ));
                }
                l[i][j] = v.sqrt();
                pivot = pivot.min(v);
            } else {
                l[i][j] = v / l[j][j];
            }
        }
    }
    Ok(pivot)
}

pub(super) fn solve(
    histories: &[History],
    windows: &[[f64; 2]],
) -> Result<SearchSolution, Refusal> {
    let bounds = bounds(windows)?;
    let n = bounds.len();
    let starts = 3usize.pow(n as u32);
    let mut budget = Budget {
        evaluations: 0,
        remaining_psd: MAX_PSD_STEPS,
    };
    let mut best: Option<Minimum> = None;
    let mut converged = 0;
    for start in 0..starts {
        let mut p = [0.; 3];
        let mut code = start;
        for (j, [lo, hi]) in bounds.iter().enumerate() {
            p[j] = lo + (0.2 + 0.3 * (code % 3) as f64) * (hi - lo);
            code /= 3;
        }
        if let Some(candidate) = optimize(histories, &bounds, p, &mut budget)? {
            converged += 1;
            if best
                .as_ref()
                .is_none_or(|b| candidate.evaluation.cost < b.evaluation.cost)
            {
                best = Some(candidate);
            }
        }
    }
    let best = best.ok_or_else(||Refusal::new(RefusalCode::NotConverged,
        "directional time search did not reach objective stationarity; review excitation, window boundaries and equilibrium"))?;
    let boundary_distance_fraction: Vec<_> = bounds
        .iter()
        .enumerate()
        .map(|(j, [lo, hi])| (best.p[j] - lo).min(hi - best.p[j]) / (hi - lo))
        .collect();
    if boundary_distance_fraction.iter().any(|v| *v < 1e-5) {
        return Err(bad(
            "directional time identification is limited by a search boundary",
        ));
    }
    let joint_excitation_pivot = joint_excitation(histories, &best.evaluation)?;
    let report = TimeSearchReport {
        starts_attempted: starts,
        starts_converged: converged,
        iterations: best.iterations,
        objective_evaluations: budget.evaluations,
        projected_iterations: MAX_PSD_STEPS - budget.remaining_psd,
        gradient_scaled_max: best.evaluation.gradient_scaled_max,
        time_sensitivity: best.evaluation.sensitivity[..n].to_vec(),
        joint_excitation_pivot,
        boundary_distance_fraction,
    };
    Ok(SearchSolution {
        times: best.evaluation.times,
        fit: best.evaluation.fit,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_gradient_matches_reoptimized_psd_objective_differences() {
        let input: crate::material_relaxation::calibration::Input = serde_json::from_str(include_str!(
            "../../../../../../tests/fixtures/materials/synthetic-orthotropic-relaxation-history.json"
        )).unwrap();
        let (histories, _) = super::super::super::prepare(&input).unwrap();
        let mut budget = Budget {
            evaluations: 0,
            remaining_psd: MAX_PSD_STEPS,
        };
        let p = [det::ln(0.4), det::ln(1.5), det::ln(10.)];
        let baseline = evaluate(&histories, p, 3, &mut budget).unwrap();
        assert!(baseline.fit.projected);
        for j in 0..3 {
            let mut left = p;
            let mut right = p;
            left[j] -= 1e-5;
            right[j] += 1e-5;
            let derivative = (evaluate(&histories, right, 3, &mut budget).unwrap().cost
                - evaluate(&histories, left, 3, &mut budget).unwrap().cost)
                / 2e-5;
            assert!(
                (derivative - baseline.gradient[j]).abs() < 2e-5 * (1. + derivative.abs()),
                "axis{j}: {derivative} != {}",
                baseline.gradient[j]
            );
        }
    }
}
