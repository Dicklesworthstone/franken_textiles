//! Prescribed mesh motion with adaptive implicit cloth response (plan §9.5, §11.4).
//!
//! Each accepted substep admits the body displacement, solves against its endpoint with
//! body-relative friction, then checks the simultaneous cloth/body chord. The entire request
//! commits or refuses, including collider, velocities, friction history and solver caches.
//! This bounded continuation is not IPC's augmented-Lagrangian large-motion method.

use ftx_core::{Refusal, RefusalCode, Vec3};

use crate::collider::audit_surface;
use crate::{Body, SolveReport, Solver};

/// Work and temporal-resolution policy. These are numerical limits, not fitted fabric data.
#[derive(Clone, Copy, Debug)]
pub struct MotionOptions {
    /// Maximum physical time per accepted substep (s).
    pub max_step_s: f64,
    /// Total candidate attempts, including rejected trials and subdivisions.
    pub max_trials: usize,
    /// Candidate-pair cap per swept query or body-surface audit. Overflow refuses the whole
    /// request, never truncates.
    pub max_pairs: usize,
    /// Maximum retained vertex positions across all output samples (body + cloth). When using
    /// `advance_body_with_dissipation`, each retained triangle-work scalar also consumes one
    /// vertex-equivalent unit; ordinary motion does not retain or charge those extra arrays.
    pub max_sample_vertices: usize,
}

impl Default for MotionOptions {
    fn default() -> Self {
        Self {
            max_step_s: 1.0 / 60.0,
            max_trials: 512,
            max_pairs: 1_000_000,
            max_sample_vertices: 4_000_000,
        }
    }
}

/// One accepted substep. Render these samples rather than an untested chord over the whole request.
#[derive(Clone, Debug)]
pub struct MotionSample {
    /// Physical time since this request began (s).
    pub time_s: f64,
    /// Cloth positions (m).
    pub cloth: Vec<Vec3>,
    /// Prescribed body positions (m), in the original vertex order.
    pub body: Vec<Vec3>,
    /// Converged implicit solve at this substep, before any qualified endpoint mass/impulse transfer.
    pub solve: SolveReport,
    /// Cumulative accepted irreversible membrane work in retained triangle order, joules.
    /// Present only when explicitly captured and a permanent membrane law is bound. This includes
    /// work before the motion request; heat consumers must subtract their initial accepted baseline.
    pub membrane_plastic_dissipation_j: Option<Vec<f64>>,
    /// Cumulative accepted physical Maxwell dashpot work, joules in retained triangle order.
    /// Only explicitly requested captures allocate this array. Initial drape work is included;
    /// consumers must subtract their own baseline. Numerical integration loss is excluded.
    pub membrane_relaxation_dissipation_j: Option<Vec<f64>>,
    /// Cumulative accepted irreversible source-panel bending work in retained triangle order.
    /// Inactive triangles are zero; initial drape work is included for explicit baseline removal.
    pub bending_plastic_dissipation_j: Option<Vec<f64>>,
}

/// Independently requested physical-work samples. Each retained scalar counts against the existing
/// motion sample budget; inactive laws allocate no array. No thermal conversion is inferred.
#[derive(Clone, Copy, Debug, Default)]
pub struct DissipationCapture {
    /// Capture accepted irreversible permanent membrane work.
    pub plastic: bool,
    /// Capture accepted physical Maxwell viscous work, excluding numerical dissipation.
    pub relaxation: bool,
    /// Capture accepted irreversible bending work, excluding recoverable panel storage.
    pub bending: bool,
}

/// Returned only after the ENTIRE target motion succeeds.
#[derive(Clone, Debug)]
pub struct MotionReport {
    /// Attempt count; rejected attempts are included.
    pub trials: usize,
    /// CCD and body-surface candidates tested in successful queries, including rejected trials.
    pub pairs_tested: u64,
    /// Accepted endpoint samples; the initial state is the caller's original solver state.
    pub samples: Vec<MotionSample>,
}

fn invalid(message: &str) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, message)
}

// A sufficient (not necessary) no-collapse check on the entire linear face trajectory:
// n(t)=n0+t*n1+t²*n2. Require its projection on n0 to stay positively separated from zero.
// Conservative rounding margin deliberately refuses ill-conditioned/large-turning chords.
fn regular_path(start: &[Vec3], end: &[Vec3], tris: &[[usize; 3]]) -> bool {
    tris.iter().all(|&[a, b, c]| {
        let (u, v) = (start[b] - start[a], start[c] - start[a]);
        let (du, dv) = ((end[b] - end[a]) - u, (end[c] - end[a]) - v);
        let n = u.cross(v);
        let a0 = n.dot(n);
        let a1 = n.dot(du.cross(v) + u.cross(dv));
        let a2 = n.dot(du.cross(dv));
        let mut min = a0.min(a0 + a1 + a2);
        if a2 > 0.0 {
            let t = -a1 / (2.0 * a2);
            if t > 0.0 && t < 1.0 {
                min = min.min(a0 + t * (a1 + t * a2));
            }
        }
        let magnitude = n.norm() * (u.norm() + du.norm()) * (v.norm() + dv.norm());
        magnitude.is_finite() && min.is_finite() && min > 256.0 * f64::EPSILON * magnitude
    })
}

/// Advance cloth while the current body's vertices follow a linear trajectory to `target` over
/// `duration_s`. Topology and thickness stay fixed. Use successive short keyframes to describe
/// articulated poses/rotations; no anatomical mapping, pose interpolation or inverse dynamics is
/// guessed here. Prescribed work can increase cloth energy; this is not a free-system energy test.
///
/// The existing frankensim-derived BVH, sparse LDLT/Newton, material, self-contact and friction
/// kernels are reused. Body meshes are rebuilt at accepted candidates rather than refitted in place.
/// Rejects motion it cannot complete within the stated budgets. Caller-visible state is unchanged
/// on ANY error, including late errors after internally accepted steps. The initial, target and
/// trial body surfaces are audited for self-intersection with exact shared-feature predicates.
/// Nonincident body features additionally pass additive CCD on every accepted linear chord,
/// separately from face noncollapse. Anatomical validity and curved paths are not certified.
///
/// # Errors
/// Invalid state/options/topology, non-finite arithmetic, invalid start contact, exhausted work or
/// output budgets, and motion that cannot be advanced with converged, collision-free steps.
pub fn advance_body(
    solver: &mut Solver,
    target: &[Vec3],
    duration_s: f64,
    options: MotionOptions,
) -> Result<MotionReport, Refusal> {
    advance_body_impl(
        solver,
        target,
        duration_s,
        options,
        DissipationCapture::default(),
    )
}

/// Advance the same checked body motion while retaining cumulative accepted irreversible membrane
/// work at every accepted sample. Rejected trials contribute no work. Ordinary `advance_body`
/// callers retain their existing geometry-only output allocation and no triangle-work arrays.
/// Each captured scalar counts as one vertex-equivalent against `options.max_sample_vertices`.
/// Capture supplies no conversion fraction and does not itself heat any thermal state.
///
/// # Errors
/// The same physical, topology and budget failures as `advance_body`, plus stale material history.
/// Any failure leaves the complete caller-visible solver state unchanged.
pub fn advance_body_with_dissipation(
    solver: &mut Solver,
    target: &[Vec3],
    duration_s: f64,
    options: MotionOptions,
) -> Result<MotionReport, Refusal> {
    advance_body_impl(
        solver,
        target,
        duration_s,
        options,
        DissipationCapture {
            plastic: true,
            relaxation: false,
            bending: false,
        },
    )
}

/// Advance the ordinary checked motion while capturing only the requested material-work channels.
/// Selected histories are read after accepted substeps; no work from failed trials is retained. The
/// geometry-only and existing plastic-only APIs preserve their original output allocation policy.
///
/// # Errors
/// As `advance_body`, including stale history or exhaustion of the combined geometry/work budget.
/// Every error leaves the entire input solver unchanged.
pub fn advance_body_with_work(
    solver: &mut Solver,
    target: &[Vec3],
    duration_s: f64,
    options: MotionOptions,
    capture: DissipationCapture,
) -> Result<MotionReport, Refusal> {
    advance_body_impl(solver, target, duration_s, options, capture)
}

fn advance_body_impl(
    solver: &mut Solver,
    target: &[Vec3],
    duration_s: f64,
    options: MotionOptions,
    capture: DissipationCapture,
) -> Result<MotionReport, Refusal> {
    advance_body_coupled(
        solver,
        target,
        duration_s,
        options,
        capture,
        &mut (),
        |_, _, _| Ok(()),
    )
}

// A coupled consumer runs only after the geometric/contact checks have accepted a trial. Its
// private state and the solver commit together at the end of the entire segment. The consumer
// may update mass, velocity and history through a source-qualified endpoint transfer. It must not
// change already checked geometry; the next mechanical interval sees the accepted transfer impulse.
pub(crate) fn advance_body_coupled<S: Clone>(
    solver: &mut Solver,
    target: &[Vec3],
    duration_s: f64,
    options: MotionOptions,
    capture: DissipationCapture,
    state: &mut S,
    mut accept: impl FnMut(&mut Solver, &MotionSample, &mut S) -> Result<(), Refusal>,
) -> Result<MotionReport, Refusal> {
    solver.model.check_attached_mass()?;
    if !duration_s.is_finite()
        || !(1e-9..=60.0).contains(&duration_s)
        || !options.max_step_s.is_finite()
        || !(1e-9..=1.0).contains(&options.max_step_s)
        || !(1..=100_000).contains(&options.max_trials)
        || !(1..=10_000_000).contains(&options.max_pairs)
        || !(1..=100_000_000).contains(&options.max_sample_vertices)
    {
        return Err(invalid("invalid motion time or work/sample budget"));
    }
    let original = solver
        .contact
        .as_ref()
        .ok_or_else(|| invalid("body motion requires a collider"))?;
    if original.body.triangles.is_empty()
        || original.body.vertices.len() != target.len()
        || solver.x.len() != solver.model.rest.len()
        || solver.v.len() != solver.x.len()
        || solver
            .model
            .membranes
            .iter()
            .map(|e| e.v)
            .ne(original.cloth.triangles.iter().copied())
    {
        return Err(invalid(
            "body motion requires unchanged body and cloth topology",
        ));
    }
    if !solver.gravity.is_finite()
        || solver.v.iter().any(|p| !p.is_finite())
        || !solver.tol.is_finite()
        || solver.tol <= 0.0
        || solver.max_newton == 0
    {
        return Err(invalid(
            "body motion requires a finite state and positive Newton policy",
        ));
    }
    let start_audit = original.body.audit_self_intersections(options.max_pairs)?;
    let target_audit = audit_surface(target, &original.body.triangles, options.max_pairs)?;
    let initial_bound = original.motion_bound(
        &solver.x,
        &solver.x,
        &original.body.vertices,
        options.max_pairs,
    )?;
    original.audit_self_contact(&solver.x)?;
    if original.first_intersection(&solver.x).is_some() {
        return Err(Refusal::new(
            RefusalCode::IntersectingStart,
            "body motion starts with intersecting cloth",
        ));
    }
    let start_body = original.body.vertices.clone();
    let triangles = original.body.triangles.clone();
    let min_cell = original.params.dhat;
    let plastic_count = if capture.plastic {
        solver
            .membrane_plastic_dissipation_j()?
            .map_or(0, |values| values.len())
    } else {
        0
    };
    let relaxation_count = if capture.relaxation {
        solver
            .membrane_relaxation_dissipation_j()?
            .map_or(0, |values| values.len())
    } else {
        0
    };
    let bending_count = if capture.bending {
        solver
            .bending_plastic_dissipation_j()?
            .map_or(0, |v| v.len())
    } else {
        0
    };
    let count = target
        .len()
        .checked_add(solver.x.len())
        .and_then(|n| n.checked_add(plastic_count))
        .and_then(|n| n.checked_add(relaxation_count))
        .and_then(|n| n.checked_add(bending_count))
        .ok_or_else(|| invalid("sample size overflow"))?;
    let mut work = solver.clone();
    let mut coupled = state.clone();
    let mut report = MotionReport {
        trials: 0,
        pairs_tested: start_audit.pairs_tested as u64
            + target_audit.pairs_tested as u64
            + initial_bound.pairs_tested as u64,
        samples: Vec::new(),
    };
    let mut time = 0.0;
    let mut step = options.max_step_s.min(duration_s);
    while time < duration_s {
        if report.trials == options.max_trials {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "body motion trial budget exceeded; state unchanged",
            ));
        }
        if report.samples.len() >= options.max_sample_vertices / count {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "body motion sample budget exceeded; state unchanged",
            ));
        }
        report.trials += 1;
        let remaining = duration_s - time;
        // Land exactly on a nominal final frame instead of creating an unresolvable roundoff tail.
        let h = if remaining <= step * (1.0 + 16.0 * f64::EPSILON) {
            remaining
        } else {
            step
        };
        if h <= duration_s * 1e-12 || time + h == time {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                "body motion stalled; use slower, shorter pose keyframes",
            ));
        }
        let end_time = if h == duration_s - time {
            duration_s
        } else {
            time + h
        };
        let end: Vec<_> = if end_time == duration_s {
            target.to_vec()
        } else {
            start_body
                .iter()
                .zip(target)
                .map(|(a, b)| *a + (*b - *a) * (end_time / duration_s))
                .collect()
        };
        let c = work.contact.as_ref().expect("checked collider is retained");
        // A non-regular body chord is subdivided before any distance derivative runs on it.
        if !regular_path(&c.body.vertices, &end, &triangles) {
            step = h * 0.5;
            continue;
        }
        // Subdivision cannot repair an invalid point on the prescribed body trajectory. Refuse
        // the entire request, including any prior internally accepted cloth or thermal steps.
        let endpoint_audit = audit_surface(&end, &triangles, options.max_pairs)?;
        report.pairs_tested += endpoint_audit.pairs_tested as u64;
        let body_safe = c.body.self_motion_bound(&end, options.max_pairs)?;
        report.pairs_tested += body_safe.pairs_tested as u64;
        if body_safe.fraction < 1.0 {
            step = h * (0.8 * body_safe.fraction).min(0.5);
            continue;
        }
        let safe = c.motion_bound(&work.x, &work.x, &end, options.max_pairs)?;
        report.pairs_tested += safe.pairs_tested as u64;
        if safe.fraction < 1.0 {
            step = h * (0.8 * safe.fraction).min(0.5);
            continue;
        }
        let previous_body = c.body.vertices.clone();
        let mut trial = work.clone();
        trial
            .contact
            .as_mut()
            .expect("checked collider is retained")
            .body = Body::new(end, triangles.clone(), min_cell)?;
        let solved = trial.step_at_body(h, Some(&previous_body));
        let solve = match solved {
            Ok(r)
                if r.converged
                    && r.energy_start.is_finite()
                    && r.energy_end.is_finite()
                    && r.residual.is_finite()
                    && trial.v.iter().all(|p| p.is_finite()) =>
            {
                r
            }
            Ok(_) => {
                step = h * 0.5;
                continue;
            }
            Err(e)
                if matches!(
                    e.code,
                    RefusalCode::NotConverged | RefusalCode::IntersectingStart
                ) =>
            {
                step = h * 0.5;
                continue;
            }
            Err(e) => return Err(e),
        };
        let new_c = trial
            .contact
            .as_ref()
            .expect("checked collider is retained");
        let continuous =
            c.motion_bound(&work.x, &trial.x, &new_c.body.vertices, options.max_pairs)?;
        report.pairs_tested += continuous.pairs_tested as u64;
        if continuous.fraction < 1.0
            || !regular_path(&work.x, &trial.x, &c.cloth.triangles)
            || new_c.first_intersection(&trial.x).is_some()
        {
            step = h * 0.5;
            continue;
        }
        // Explicit endpoint gap audit also catches representational changes after CCD arithmetic.
        if new_c
            .min_gap(&trial.x, new_c.params.dhat)
            .is_some_and(|g| !g.is_finite() || g <= 0.0)
        {
            step = h * 0.5;
            continue;
        }
        time = end_time;
        let sample = MotionSample {
            time_s: time,
            cloth: trial.x.clone(),
            body: new_c.body.vertices.clone(),
            solve,
            membrane_plastic_dissipation_j: if capture.plastic {
                trial.membrane_plastic_dissipation_j()?
            } else {
                None
            },
            membrane_relaxation_dissipation_j: if capture.relaxation {
                trial.membrane_relaxation_dissipation_j()?
            } else {
                None
            },
            bending_plastic_dissipation_j: if capture.bending {
                trial.bending_plastic_dissipation_j()?
            } else {
                None
            },
        };
        accept(&mut trial, &sample, &mut coupled)?;
        report.samples.push(sample);
        work = trial;
        step = (h * 2.0).min(options.max_step_s);
    }
    *solver = work;
    *state = coupled;
    Ok(report)
}
