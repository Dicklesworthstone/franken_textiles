//! Fixed-order, bounded multistart least squares in log ratios and log relaxation times.
//! No force from a held-out acquisition is read while fitting or selecting a start.

use ftx_core::{Refusal, RefusalCode, det, linalg::lstsq};

use super::{History, Initialization, Partition, bad, piola};

pub(super) struct Prediction {
    pub force: [f64; 4],
    // Columns 0..3: log ratios; 3..6: log times. Unused columns are exactly zero.
    jacobian: [[f64; 6]; 4],
}

// a = exp(-h/tau), phi = (1-a)/(h/tau), and dphi/dlog(tau) = phi-a.
// The small-argument branch avoids cancellation in the time sensitivity, not just in phi.
pub(super) fn weights(h: f64, tau: f64) -> (f64, f64, f64, f64) {
    let r = h / tau;
    if !r.is_finite() {
        return (0., tau / h, tau / h, 0.);
    }
    let a = det::exp(-r);
    if r < 1e-4 {
        let phi = 1. + r * (-0.5 + r * (1. / 6. + r * (-1. / 24. + r / 120.)));
        let derivative = r * (0.5 + r * (-1. / 3. + r * (1. / 8. - r / 30.)));
        (a, phi, derivative, r * a)
    } else {
        let phi = -det::expm1(-r) / r;
        (a, phi, phi - a, r * a)
    }
}

pub(super) fn predict(
    history: &History,
    q: [[f64; 3]; 3],
    ratios: &[f64],
    times: &[f64],
) -> Result<Vec<Prediction>, Refusal> {
    let initial = history.points[0].strain;
    let mut lag = vec![
        if history.initialization == Initialization::StressFreeReference {
            initial
        } else {
            [0.; 3]
        };
        ratios.len()
    ];
    let mut derivative = vec![[0.; 3]; ratios.len()];
    let mut result = Vec::with_capacity(history.points.len());
    for (i, point) in history.points.iter().enumerate() {
        let mut prediction = Prediction {
            force: point.equilibrium,
            jacobian: [[0.; 6]; 4],
        };
        for b in 0..ratios.len() {
            if i > 0 {
                let previous = &history.points[i - 1];
                let (a, phi, dphi, da) = weights(point.time - previous.time, times[b]);
                for j in 0..3 {
                    let delta = point.strain[j] - previous.strain[j];
                    derivative[b][j] = a * derivative[b][j] + da * lag[b][j] + dphi * delta;
                    lag[b][j] = a * lag[b][j] + phi * delta;
                }
            }
            let stress = super::super::mul(&q, lag[b]).map(|v| v * ratios[b]);
            let sensitivity = super::super::mul(&q, derivative[b]).map(|v| v * ratios[b]);
            let force = piola(point.f, stress);
            let df = piola(point.f, sensitivity);
            for j in 0..4 {
                prediction.force[j] += force[j];
                prediction.jacobian[j][b] = force[j];
                prediction.jacobian[j][3 + b] = df[j];
            }
        }
        if prediction
            .force
            .iter()
            .chain(prediction.jacobian.iter().flatten())
            .any(|v| !v.is_finite())
        {
            return Err(bad("relaxation history arithmetic overflow"));
        }
        result.push(prediction);
    }
    Ok(result)
}

struct Evaluation {
    residual: Vec<f64>,
    jacobian: Vec<[f64; 6]>,
    cost: f64,
}

fn evaluate(
    histories: &[History],
    q: [[f64; 3]; 3],
    p: [f64; 6],
    n: usize,
) -> Result<Evaluation, Refusal> {
    let ratios: Vec<_> = p[..n].iter().map(|v| det::exp(*v)).collect();
    let times: Vec<_> = p[3..3 + n].iter().map(|v| det::exp(*v)).collect();
    if ratios
        .iter()
        .chain(&times)
        .any(|v| !v.is_finite() || *v <= 0.)
    {
        return Err(bad("unrepresentable positive relaxation parameter"));
    }
    let mut residual = Vec::new();
    let mut jacobian = Vec::new();
    for history in histories.iter().filter(|h| h.partition == Partition::Fit) {
        let predicted = predict(history, q, &ratios, &times)?;
        for (point, prediction) in history.points.iter().zip(predicted) {
            for j in 0..4 {
                if let Some(observed) = point.observed[j] {
                    residual.push((prediction.force[j] - observed) / point.scale);
                    jacobian.push(prediction.jacobian[j].map(|v| v / point.scale));
                }
            }
        }
    }
    let cost = residual.iter().map(|v| v * v).sum::<f64>();
    if !cost.is_finite() || jacobian.iter().flatten().any(|v| !v.is_finite()) {
        return Err(bad("relaxation residual/sensitivity overflow"));
    }
    Ok(Evaluation {
        residual,
        jacobian,
        cost,
    })
}

fn active(n: usize) -> Vec<usize> {
    (0..n).chain(3..3 + n).collect()
}

fn normal(e: &Evaluation, n: usize) -> ([[f64; 6]; 6], [f64; 6], [f64; 6]) {
    let mut scales = [1.; 6];
    let indices = active(n);
    for &j in &indices {
        scales[j] = e.jacobian.iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt();
    }
    let mut g = [[0.; 6]; 6];
    let mut rhs = [0.; 6];
    for (i, row) in g.iter_mut().enumerate() {
        row[i] = 1.;
    }
    for (row, &residual) in e.jacobian.iter().zip(&e.residual) {
        for &i in &indices {
            let ri = row[i] / scales[i].max(f64::MIN_POSITIVE);
            rhs[i] -= ri * residual;
            for &j in &indices {
                if i != j {
                    g[i][j] += ri * (row[j] / scales[j].max(f64::MIN_POSITIVE));
                }
            }
        }
    }
    (g, rhs, scales)
}

fn cholesky(g: [[f64; 6]; 6]) -> Option<([[f64; 6]; 6], f64)> {
    let mut l = [[0.; 6]; 6];
    let mut min_pivot = f64::INFINITY;
    for i in 0..6 {
        for j in 0..=i {
            let v = g[i][j] - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if i == j {
                if !v.is_finite() || v <= 0. {
                    return None;
                }
                min_pivot = min_pivot.min(v);
                l[i][j] = v.sqrt();
            } else {
                l[i][j] = v / l[j][j];
            }
        }
    }
    Some((l, min_pivot))
}

fn linear(l: [[f64; 6]; 6], rhs: [f64; 6]) -> [f64; 6] {
    let mut x = [0.; 6];
    for i in 0..6 {
        x[i] = (rhs[i] - (0..i).map(|j| l[i][j] * x[j]).sum::<f64>()) / l[i][i];
    }
    for i in (0..6).rev() {
        x[i] = (x[i] - (i + 1..6).map(|j| l[j][i] * x[j]).sum::<f64>()) / l[i][i];
    }
    x
}

fn optimize(
    histories: &[History],
    q: [[f64; 3]; 3],
    bounds: &[[f64; 2]],
    mut p: [f64; 6],
) -> Option<([f64; 6], Evaluation)> {
    let n = bounds.len();
    // A linear amplitude estimate is ONLY an optimizer seed; negative trial weights are not published.
    let initial = evaluate(histories, q, p, n).ok()?;
    let rows: Vec<[f64; 3]> = initial
        .jacobian
        .iter()
        .map(|r| [r[0], r[1], r[2]])
        .collect();
    let y: Vec<_> = rows
        .iter()
        .zip(initial.residual)
        .map(|(r, e)| r.iter().sum::<f64>() - e)
        .collect();
    let seed = lstsq(&rows, &y);
    for j in 0..n {
        p[j] = det::ln(if seed[j].is_finite() && seed[j] > 0. {
            seed[j]
        } else {
            0.1
        });
    }
    let mut e = evaluate(histories, q, p, n).ok()?;
    let mut damping = 1e-3;
    for _ in 0..160 {
        let (gram, rhs, scales) = normal(&e, n);
        if scales.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let gradient = rhs.iter().map(|v| v.abs()).fold(0., f64::max);
        if e.cost < 1e-24 || gradient <= 1e-10 * (1. + e.cost.sqrt()) {
            return Some((p, e));
        }
        let mut improved = false;
        for _ in 0..16 {
            let mut g = gram;
            for (i, row) in g.iter_mut().enumerate() {
                row[i] += damping;
            }
            let (l, _) = cholesky(g)?;
            let scaled = linear(l, rhs);
            let mut delta = [0.; 6];
            for j in active(n) {
                delta[j] = scaled[j] / scales[j].max(f64::MIN_POSITIVE);
            }
            // Numerical trust region in log parameters; physical inputs are never clamped.
            let size = delta.iter().map(|v| v.abs()).fold(0., f64::max);
            if !size.is_finite() {
                return None;
            }
            let fraction = if size > 1. { 1. / size } else { 1. };
            let trial = std::array::from_fn(|j| p[j] + fraction * delta[j]);
            let allowed = bounds
                .iter()
                .enumerate()
                .all(|(j, [lo, hi])| trial[3 + j] > *lo && trial[3 + j] < *hi);
            if allowed
                && let Ok(next) = evaluate(histories, q, trial, n)
                && next.cost < e.cost
            {
                let improvement = e.cost - next.cost;
                p = trial;
                e = next;
                damping = (damping / 3.).max(1e-12);
                if size < 1e-10 && improvement <= 1e-12 * (1. + e.cost) {
                    return Some((p, e));
                }
                improved = true;
                break;
            }
            damping *= 10.;
        }
        if !improved {
            return None;
        }
    }
    None
}

pub(super) struct Fit {
    pub parameters: [f64; 6],
    pub pivot: f64,
    pub sensitivity: Vec<f64>,
    pub starts: usize,
}

pub(super) fn solve(
    histories: &[History],
    q: [[f64; 3]; 3],
    bounds: &[[f64; 2]],
) -> Result<Fit, Refusal> {
    let bounds: Vec<[f64; 2]> = bounds.iter().map(|b| b.map(det::ln)).collect();
    if bounds
        .iter()
        .any(|[a, b]| !a.is_finite() || !b.is_finite() || b - a < 1e-6)
    {
        return Err(bad("time search window is not numerically resolvable"));
    }
    let n = bounds.len();
    let starts = 3usize.pow(n as u32);
    let mut best: Option<([f64; 6], Evaluation)> = None;
    for start in 0..starts {
        let mut p = [0.; 6];
        let mut code = start;
        for (j, [lo, hi]) in bounds.iter().enumerate() {
            p[3 + j] = lo + (0.2 + 0.3 * (code % 3) as f64) * (hi - lo);
            code /= 3;
        }
        if let Some(candidate) = optimize(histories, q, &bounds, p)
            && best.as_ref().is_none_or(|(_, e)| candidate.1.cost < e.cost)
        {
            best = Some(candidate);
        }
    }
    let (parameters, evaluation) = best.ok_or_else(|| Refusal::new(RefusalCode::NotConverged,
        "relaxation fit did not converge within the bounded search; review excitation, time windows and equilibrium"))?;
    if bounds.iter().enumerate().any(|(j, [lo, hi])| {
        let t = parameters[3 + j];
        (t - lo).min(hi - t) < 1e-5 * (hi - lo)
    }) {
        return Err(bad(
            "relaxation fit is limited by a time search boundary; no unconstrained timescale identified",
        ));
    }
    let (gram, _, scales) = normal(&evaluation, n);
    let indices = active(n);
    if indices
        .iter()
        .any(|&j| !scales[j].is_finite() || scales[j] <= 1e-10)
    {
        return Err(bad(
            "relaxation parameters are not excited by the measured history",
        ));
    }
    let (_, pivot) =
        cholesky(gram).ok_or_else(|| bad("rank-deficient relaxation identification"))?;
    if pivot <= 1e-7 {
        return Err(bad(
            "ill-conditioned relaxation spectrum; independent time coverage is required",
        ));
    }
    Ok(Fit {
        parameters,
        pivot,
        sensitivity: indices.into_iter().map(|i| scales[i]).collect(),
        starts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_force_jacobian_matches_independent_log_parameter_differences() {
        let history = History {
            initialization: Initialization::RelaxedStart,
            partition: Partition::Fit,
            points: [
                (0., [0.01, 0.02, 0.03]),
                (0.03, [0.05, 0.08, -0.01]),
                (2., [-0.01, 0.01, 0.04]),
            ]
            .into_iter()
            .map(|(time, strain)| super::super::Point {
                time,
                strain,
                f: [[1.1, 0.1], [-0.1, 1.]],
                equilibrium: [0.; 4],
                observed: [Some(0.); 4],
                scale: 1.,
            })
            .collect(),
        };
        let q = [[1000., 50., 0.], [50., 500., 0.], [0., 0., 200.]];
        let ratios = [0.5, 1.3];
        let times = [0.02, 2.];
        let baseline = predict(&history, q, &ratios, &times).unwrap();
        for column in [0, 1, 3, 4] {
            let mut pr = ratios;
            let mut mr = ratios;
            let mut pt = times;
            let mut mt = times;
            let h = 1e-5;
            if column < 3 {
                pr[column] *= det::exp(h);
                mr[column] *= det::exp(-h);
            } else {
                pt[column - 3] *= det::exp(h);
                mt[column - 3] *= det::exp(-h);
            }
            let plus = predict(&history, q, &pr, &pt).unwrap();
            let minus = predict(&history, q, &mr, &mt).unwrap();
            for i in 0..baseline.len() {
                for j in 0..4 {
                    let fd = (plus[i].force[j] - minus[i].force[j]) / (2. * h);
                    assert!((fd - baseline[i].jacobian[j][column]).abs() < 1e-6);
                }
            }
        }
    }
    #[test]
    fn history_and_log_time_derivatives_cover_small_and_large_intervals() {
        for r in [1e-12, 1e-5, 0.01, 0.5, 10., 100.] {
            let (a, phi, dp, da) = weights(r, 1.);
            assert!(a >= 0. && phi > 0. && dp >= 0.);
            let h = 1e-4;
            let plus = weights(r, det::exp(h));
            let minus = weights(r, det::exp(-h));
            assert!(((plus.0 - minus.0) / (2. * h) - da).abs() < 1e-8);
            assert!(((plus.1 - minus.1) / (2. * h) - dp).abs() < 1e-8);
        }
        let (_, phi, _, _) = weights(f64::MAX, f64::MIN_POSITIVE);
        assert_eq!(phi, 0.);
    }
}
