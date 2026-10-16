//! Bounded, deterministic bottom-left polygon compaction of a complete shelf marker.
//!
//! Candidate anchors may be sampled; collision geometry never is. New moves preserve grain/nap and
//! never raise a piece's bottom edge. Every move is checked against full exterior paths; holes remain
//! occupied. Work exhaustion retains the last COMPLETE feasible marker, not a partial purchase list.

use super::geometry::{Work, clear};
use super::{CutOptions, CutPlacement, bad, bbox, positive};
use ftx_core::{Refusal, Vec2};
use serde::{Deserialize, Serialize};

/// Optional translation-only optimization. Defaults apply only when a nesting object is supplied.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NestingOptions {
    /// Maximum sweeps over all physical instances (1..8); stops sooner if no move is accepted.
    pub passes: usize,
    /// Bound on anchor pairs, candidate vertices, broad-phase pairs and edge/containment tests.
    /// 1..20 million units per material roll, independent of wall-clock timing.
    pub work_limit: usize,
}
impl Default for NestingOptions {
    fn default() -> Self {
        Self {
            passes: 3,
            work_limit: 1_000_000,
        }
    }
}
impl NestingOptions {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if !(1..=8).contains(&self.passes) || !(1..=20_000_000).contains(&self.work_limit) {
            return Err(bad(
                "nesting needs passes in 1..8 and work_limit in 1..20000000",
            ));
        }
        Ok(())
    }
}

/// Search diagnostics; bounds and measured saving are relative to this same input's shelf layout.
#[derive(Clone, Debug, Serialize)]
pub struct NestingReport {
    /// Fixed implementation identity, not an assertion of globally optimal nesting.
    pub algorithm: &'static str,
    /// Unoptimized complete marker length including end margins.
    pub baseline_length_m: f64,
    /// Final complete marker length including end margins.
    pub length_m: f64,
    /// Nonnegative stock length removed, compared to the shelf baseline.
    pub saved_length_m: f64,
    /// Fully checked moves; rejected or partially checked candidates never enter the marker.
    pub accepted_moves: usize,
    /// Candidate positions actually considered, including broad-phase rejections.
    pub candidates: usize,
    /// Number of budgeted operations consumed.
    pub work_used: usize,
    /// True if search was cut short; the output is still a complete feasible marker.
    pub budget_exhausted: bool,
    /// Always false: this bounded heuristic provides no optimality certificate.
    pub optimality_claim: bool,
}
// At most 16 uniformly indexed outline vertices generate anchors; ALL vertices determine clearance.
fn probes(p: &[Vec2]) -> impl Iterator<Item = Vec2> + '_ {
    let n = p.len().min(16);
    (0..n).map(move |i| p[i * p.len() / n])
}
fn dedup(mut a: Vec<f64>, lo: f64, hi: f64) -> Vec<f64> {
    a.retain(|v| v.is_finite() && *v >= lo && *v <= hi);
    a.sort_by(f64::total_cmp);
    a.dedup_by(|a, b| *a == *b);
    // Limit the candidate grid, not the physical paths. Endpoints are always included.
    if a.len() > 256 {
        (0..256).map(|i| a[i * (a.len() - 1) / 255]).collect()
    } else {
        a
    }
}
fn anchors(
    index: usize,
    p: &[CutPlacement],
    width: f64,
    opts: &CutOptions,
    work: &mut Work,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let moving = &p[index];
    let [x, y, x1, _] = moving.bounds_mm;
    let right = width - opts.edge_margin_mm - (x1 - x);
    let mut xs = vec![opts.edge_margin_mm, right, x];
    let mut ys = vec![opts.end_margin_mm, y];
    // Slightly padded anchors avoid rounding a requested gap just below its required value.
    // This is candidate construction only; the full actual translated paths are tested below.
    let guard = 128.0 * f64::EPSILON * (1.0 + width.abs() + y.abs());
    let gap = opts.gap_mm + guard;
    for (j, other) in p.iter().enumerate() {
        if j == index {
            continue;
        }
        work.tick()?;
        for a in probes(&moving.cut_mm) {
            for b in probes(&other.cut_mm) {
                work.tick()?;
                for shift in [-gap, 0.0, gap] {
                    xs.push(b.x - (a.x - x) + shift);
                    ys.push(b.y - (a.y - y) + shift);
                }
            }
        }
    }
    Some((
        dedup(xs, opts.edge_margin_mm, right),
        dedup(ys, opts.end_margin_mm, y),
    ))
}
fn translate(
    p: &CutPlacement,
    delta: Vec2,
    ring: Vec<Vec2>,
    bounds: [f64; 4],
) -> Option<CutPlacement> {
    let mut p = p.clone();
    p.cut_mm = ring;
    p.bounds_mm = bounds;
    for v in &mut p.grain_mm {
        *v += delta;
    }
    for (path, _) in &mut p.internal_cut_mm {
        for v in path {
            *v += delta;
        }
    }
    for (v, _) in &mut p.drills_mm {
        *v += delta;
    }
    p.transform[0][2] += delta.x;
    p.transform[1][2] += delta.y;
    if p.grain_mm
        .iter()
        .chain(p.internal_cut_mm.iter().flat_map(|(p, _)| p))
        .chain(p.drills_mm.iter().map(|(p, _)| p))
        .any(|v| !v.is_finite())
        || p.transform.iter().flatten().any(|v| !v.is_finite())
    {
        return None;
    }
    Some(p)
}

pub(super) fn compact(
    placements: &mut [CutPlacement],
    width: f64,
    opts: &CutOptions,
    search: &NestingOptions,
    baseline: f64,
) -> Result<NestingReport, Refusal> {
    let mut work = Work {
        used: 0,
        limit: search.work_limit,
        exhausted: false,
        candidates: 0,
    };
    let mut moves = 0;
    'search: for _ in 0..search.passes {
        let before = moves;
        let mut order: Vec<_> = (0..placements.len()).collect();
        order.sort_by(|&a, &b| {
            placements[b].bounds_mm[3]
                .total_cmp(&placements[a].bounds_mm[3])
                .then_with(|| placements[a].instance.cmp(&placements[b].instance))
                .then(placements[a].copy.cmp(&placements[b].copy))
        });
        for index in order {
            let Some((xs, ys)) = anchors(index, placements, width, opts, &mut work) else {
                break 'search;
            };
            let [old_x, old_y, _, old_top] = placements[index].bounds_mm;
            'piece: for y in ys {
                for &x in &xs {
                    if y == old_y && x >= old_x {
                        continue;
                    }
                    if work.tick().is_none() {
                        break 'search;
                    }
                    work.candidates += 1;
                    if work.spend(placements[index].cut_mm.len()).is_none() {
                        break 'search;
                    }
                    let delta = Vec2::new(x - old_x, y - old_y);
                    let ring: Vec<_> = placements[index]
                        .cut_mm
                        .iter()
                        .map(|v| *v + delta)
                        .collect();
                    let Ok(b) = bbox(&ring) else {
                        continue;
                    };
                    if b[0] < opts.edge_margin_mm
                        || b[1] < opts.end_margin_mm
                        || b[2] > width - opts.edge_margin_mm
                        || b[3] > old_top
                    {
                        continue;
                    }
                    // The actual translated origin must decrease, not just the requested one.
                    if !(b[1] < old_y || (b[1] == old_y && b[0] < old_x)) {
                        continue;
                    }
                    let mut allowed = true;
                    for (j, p) in placements.iter().enumerate() {
                        if j == index {
                            continue;
                        }
                        let Some(ok) =
                            clear(&ring, b, &p.cut_mm, p.bounds_mm, opts.gap_mm, &mut work)
                        else {
                            break 'search;
                        };
                        if !ok {
                            allowed = false;
                            break;
                        }
                    }
                    if allowed && let Some(moved) = translate(&placements[index], delta, ring, b) {
                        placements[index] = moved;
                        moves += 1;
                        break 'piece;
                    }
                }
            }
        }
        if before == moves {
            break;
        }
    }
    let top = placements
        .iter()
        .map(|p| p.bounds_mm[3])
        .fold(f64::NEG_INFINITY, f64::max);
    let length = positive("nested marker length", (top + opts.end_margin_mm) * 0.001)?;
    // The baseline was computed by shelf heights, so roundoff in an unchanged translated polygon can
    // exceed it by a few ulps. Never advertise that as an improvement or silently shorten the marker.
    let length = if moves == 0 { baseline } else { length };
    if length > baseline + 128.0 * f64::EPSILON * (1.0 + baseline) {
        return Err(bad("nesting increased the shelf marker length"));
    }
    Ok(NestingReport {
        algorithm: "bounded_polygon_compaction/1",
        baseline_length_m: baseline,
        length_m: length,
        saved_length_m: (baseline - length).max(0.0),
        accepted_moves: moves,
        candidates: work.candidates,
        work_used: work.used,
        budget_exhausted: work.exhausted,
        optimality_claim: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn poly(p: &[[f64; 2]]) -> Vec<Vec2> {
        p.iter().map(|p| Vec2::new(p[0], p[1])).collect()
    }
    fn accepts(a: &[Vec2], b: &[Vec2], g: f64) -> bool {
        clear(
            a,
            bbox(a).unwrap(),
            b,
            bbox(b).unwrap(),
            g,
            &mut Work {
                used: 0,
                limit: 10000,
                exhausted: false,
                candidates: 0,
            },
        )
        .unwrap()
    }
    #[test]
    fn full_polygon_tests_catch_crossing_containment_identical_and_boundary_contact() {
        let a = poly(&[[0., 0.], [4., 0.], [4., 4.], [0., 4.]]);
        for b in [
            poly(&[[1., 1.], [2., 1.], [2., 2.], [1., 2.]]),
            a.clone(),
            poly(&[[2., -1.], [5., 2.], [2., 5.], [-1., 2.]]),
        ] {
            assert!(!accepts(&a, &b, 0.));
            assert!(!accepts(&b, &a, 0.));
        }
        let concave = poly(&[[0., 0.], [4., 0.], [4., 1.], [1., 1.], [1., 4.], [0., 4.]]);
        let pocket = poly(&[[2., 2.], [3., 2.], [3., 3.], [2., 3.]]);
        assert!(accepts(&concave, &pocket, 0.99));
        assert!(!accepts(&concave, &pocket, 1.01));
        let reversed: Vec<_> = concave.iter().rev().copied().collect();
        assert!(accepts(&reversed, &pocket, 0.99));
    }
    #[test]
    fn diagonal_clearance_is_euclidean_not_just_separating_boxes() {
        let a = poly(&[[0., 0.], [4., 0.], [0., 4.]]);
        let b = poly(&[[4., 4.], [1., 4.], [4., 1.]]);
        assert!(accepts(&a, &b, 0.70)); // diagonal separation 1/sqrt(2)
        assert!(!accepts(&a, &b, 0.71));
    }
    #[test]
    fn exhaustion_never_completes_a_partial_clearance_check() {
        let a = poly(&[[0., 0.], [4., 0.], [0., 4.]]);
        let b = poly(&[[4., 4.], [1., 4.], [4., 1.]]);
        let mut work = Work {
            used: 0,
            limit: 2,
            exhausted: false,
            candidates: 0,
        };
        assert!(clear(&a, bbox(&a).unwrap(), &b, bbox(&b).unwrap(), 0.1, &mut work).is_none());
        assert!(work.exhausted);
        assert_eq!(work.used, 2);
    }
}
