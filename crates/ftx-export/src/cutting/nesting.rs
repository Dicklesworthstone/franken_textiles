//! Bounded deterministic polygon compaction and optional whole-marker consolidation.
//!
//! Candidate anchors may be sampled; collision geometry never is. Grain reversal needs an explicit
//! material permission. Local compaction never raises a piece's bottom edge; whole-marker absorption
//! may relocate a complete donor inside an unchanged earlier marker. Full exterior paths determine
//! clearance and holes remain occupied. Work exhaustion retains the last COMPLETE feasible inventory.

use super::geometry::{Work, clear};
use super::{CutOptions, CutPlacement, RollMarker, bad, bbox, half_turn, positive, relocate};
use ftx_core::{Refusal, Vec2};
use serde::{Deserialize, Serialize};

/// Optional grain-constrained optimization. Defaults apply only when a nesting object is supplied.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NestingOptions {
    /// Maximum sweeps over all physical instances (1..8); stops sooner if no move is accepted.
    pub passes: usize,
    /// Bound on anchor pairs, candidate vertices, broad-phase pairs and edge/containment tests.
    /// 1..20 million units per marker, independent of wall-clock timing.
    pub work_limit: usize,
    /// Exact material SHA-256 identities for which the job permits 180-degree grain reversal.
    /// Empty by default: the material name or form never establishes nap/print symmetry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grain_reversal_materials: Vec<String>,
    /// Optional whole-marker absorption after independent compaction, under one job-wide budget.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consolidate_markers: Option<ConsolidationOptions>,
}
impl Default for NestingOptions {
    fn default() -> Self {
        Self {
            passes: 3,
            work_limit: 1_000_000,
            grain_reversal_materials: Vec::new(),
            consolidate_markers: None,
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
        if self
            .consolidate_markers
            .as_ref()
            .is_some_and(|policy| !(1..=20_000_000).contains(&policy.work_limit))
        {
            return Err(bad(
                "marker consolidation needs one shared work_limit in 1..20000000",
            ));
        }
        Ok(())
    }
}

/// Explicit additional work allowance; ordinary per-marker compaction retains its own policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationOptions {
    /// Shared across every material, destination, attempted whole transfer and geometric check.
    pub work_limit: usize,
}

/// Whole-job savings against the complete independently compacted markers, before consolidation.
#[derive(Clone, Debug, Serialize)]
pub struct MarkerConsolidationReport {
    /// Bounded heuristic identity; no complete partition search or optimum is implied.
    pub algorithm: &'static str,
    /// Number of complete markers before any whole-marker transfer.
    pub baseline_marker_count: usize,
    /// Number of complete markers after accepted transfers.
    pub marker_count: usize,
    /// Sum of admitted marker lengths before transfers, in metres.
    pub baseline_length_m: f64,
    /// Sum of final admitted marker lengths, in metres.
    pub length_m: f64,
    /// Complete stock length removed; retained markers never grow.
    pub saved_length_m: f64,
    /// Whole source markers successfully absorbed into an earlier same-material marker.
    pub accepted_merges: usize,
    /// Physical placements transferred by accepted complete merges.
    pub transferred_placements: usize,
    /// Candidate positions tested, including broad-phase rejections.
    pub candidates: usize,
    /// Caller-declared allowance, shared across this complete cutting job.
    pub work_limit: usize,
    /// Charged marker attempts, copied features, anchors and full-polygon checks.
    pub work_used: usize,
    /// Partial attempts were discarded; every retained marker remains complete and feasible.
    pub budget_exhausted: bool,
    /// Always false: this search does not prove minimal marker count or stock length.
    pub optimality_claim: bool,
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
    index: Option<usize>,
    moving: &CutPlacement,
    p: &[CutPlacement],
    width: f64,
    opts: &CutOptions,
    maximum_y: f64,
    work: &mut Work,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let [x, y, x1, _] = moving.bounds_mm;
    let [old_x, old_y, _, _] = index.map_or(moving.bounds_mm, |i| p[i].bounds_mm);
    let right = width - opts.edge_margin_mm - (x1 - x);
    let mut xs = vec![opts.edge_margin_mm, right, old_x];
    let mut ys = vec![opts.end_margin_mm, old_y, maximum_y];
    // Slightly padded anchors avoid rounding a requested gap just below its required value.
    // This is candidate construction only; the full actual translated paths are tested below.
    let guard = 128.0 * f64::EPSILON * (1.0 + width.abs() + old_y.abs());
    let gap = opts.gap_mm + guard;
    for (j, other) in p.iter().enumerate() {
        if Some(j) == index {
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
        dedup(ys, opts.end_margin_mm, maximum_y),
    ))
}

pub(super) fn compact(
    placements: &mut [CutPlacement],
    width: f64,
    opts: &CutOptions,
    search: &NestingOptions,
    baseline: f64,
    allow_grain_reversal: bool,
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
            let turned = if allow_grain_reversal {
                if work.spend(placements[index].cut_mm.len()).is_none() {
                    break 'search;
                }
                Some(half_turn(&placements[index]).ok_or_else(|| bad("grain reversal overflows"))?)
            } else {
                None
            };
            let [old_x, old_y, _, old_top] = placements[index].bounds_mm;
            let mut best: Option<CutPlacement> = None;
            for moving in std::iter::once(&placements[index]).chain(turned.as_ref()) {
                let Some((xs, ys)) = anchors(
                    Some(index),
                    moving,
                    placements,
                    width,
                    opts,
                    old_y,
                    &mut work,
                ) else {
                    break 'search;
                };
                'orientation: for y in ys {
                    for &x in &xs {
                        if y == old_y && x >= old_x {
                            continue;
                        }
                        if work.tick().is_none() {
                            break 'search;
                        }
                        work.candidates += 1;
                        if work.spend(moving.cut_mm.len()).is_none() {
                            break 'search;
                        }
                        let delta = Vec2::new(x - moving.bounds_mm[0], y - moving.bounds_mm[1]);
                        let ring: Vec<_> = moving.cut_mm.iter().map(|v| *v + delta).collect();
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
                        // Compare actual geometry; keep the current direction when positions tie.
                        let [bx, by, _, _] = best
                            .as_ref()
                            .map_or(placements[index].bounds_mm, |p| p.bounds_mm);
                        if !(b[1] < by || (b[1] == by && b[0] < bx)) {
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
                        if allowed && let Some(moved) = relocate(moving, false, delta, ring, b) {
                            best = Some(moved);
                            break 'orientation;
                        }
                    }
                }
            }
            if let Some(moved) = best {
                placements[index] = moved;
                moves += 1;
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
        algorithm: if allow_grain_reversal {
            "bounded_polygon_compaction/2"
        } else {
            "bounded_polygon_compaction/1"
        },
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

fn feature_work(placement: &CutPlacement) -> usize {
    placement
        .internal_cut_mm
        .iter()
        .fold(
            placement.cut_mm.len().saturating_add(12),
            |count, (path, _)| count.saturating_add(path.len()),
        )
        .saturating_add(placement.drills_mm.len())
}

// Stage the complete source inventory. A missed position or exhausted check cannot publish a prefix.
fn absorb(
    target: &RollMarker,
    source: &RollMarker,
    options: &CutOptions,
    allow_reversal: bool,
    work: &mut Work,
) -> Option<Vec<CutPlacement>> {
    work.spend(target.placements.iter().fold(0_usize, |count, placement| {
        count.saturating_add(feature_work(placement))
    }))?;
    let mut placements = target.placements.clone();
    let width = target
        .layout_width
        .as_ref()
        .map_or(target.width_m, |layout| layout.width_m)
        * 1000.0;
    let top = target.length_m * 1000.0 - options.end_margin_mm;
    for original in &source.placements {
        let mut accepted = None;
        'orientations: for reverse in
            [false, true]
                .into_iter()
                .take(if allow_reversal { 2 } else { 1 })
        {
            let turned;
            let moving = if reverse {
                work.spend(feature_work(original))?;
                turned = half_turn(original)?;
                &turned
            } else {
                original
            };
            let maximum_y = top - (moving.bounds_mm[3] - moving.bounds_mm[1]);
            let (xs, ys) = anchors(None, moving, &placements, width, options, maximum_y, work)?;
            for y in ys {
                for &x in &xs {
                    work.tick()?;
                    work.candidates += 1;
                    work.spend(moving.cut_mm.len())?;
                    let delta = Vec2::new(x - moving.bounds_mm[0], y - moving.bounds_mm[1]);
                    let ring: Vec<_> = moving.cut_mm.iter().map(|p| *p + delta).collect();
                    let Ok(bounds) = bbox(&ring) else { continue };
                    if bounds[0] < options.edge_margin_mm
                        || bounds[1] < options.end_margin_mm
                        || bounds[2] > width - options.edge_margin_mm
                        || bounds[3] > top
                    {
                        continue;
                    }
                    let mut allowed = true;
                    for other in &placements {
                        if !clear(
                            &ring,
                            bounds,
                            &other.cut_mm,
                            other.bounds_mm,
                            options.gap_mm,
                            work,
                        )? {
                            allowed = false;
                            break;
                        }
                    }
                    if allowed {
                        work.spend(feature_work(moving))?;
                        if let Some(placed) = relocate(moving, false, delta, ring, bounds) {
                            accepted = Some(placed);
                            break 'orientations;
                        }
                    }
                }
            }
        }
        placements.push(accepted?);
    }
    placements.sort_by(|a, b| a.instance.cmp(&b.instance).then(a.copy.cmp(&b.copy)));
    Some(placements)
}

/// Absorb whole later markers into earlier equal-material stock without increasing any kept length.
pub(super) fn consolidate(
    rolls: &mut Vec<RollMarker>,
    options: &CutOptions,
    search: &NestingOptions,
    policy: &ConsolidationOptions,
) -> Result<MarkerConsolidationReport, Refusal> {
    let baseline_marker_count = rolls.len();
    let baseline_length_m = positive(
        "complete marker length before consolidation",
        rolls.iter().map(|roll| roll.length_m).sum(),
    )?;
    let mut length_m = baseline_length_m;
    let mut work = Work::new(policy.work_limit);
    let mut accepted_merges = 0;
    let mut transferred_placements = 0;
    let mut target = 0;
    'search: while target < rolls.len() {
        let mut source = target + 1;
        while source < rolls.len() {
            if work.tick().is_none() {
                break 'search;
            }
            if rolls[source].material.sha256() != rolls[target].material.sha256() {
                source += 1;
                continue;
            }
            let area = rolls[target].external_cut_area_m2 + rolls[source].external_cut_area_m2;
            let utilization = area / rolls[target].stock_area_m2;
            if !area.is_finite() || !utilization.is_finite() || utilization > 1.0 + 1e-10 {
                source += 1;
                continue;
            }
            if work.spend(rolls.len()).is_none() {
                break 'search;
            }
            let candidate_length: f64 = rolls
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != source)
                .map(|(_, roll)| roll.length_m)
                .sum();
            if candidate_length >= length_m {
                source += 1;
                continue;
            }
            let allow_reversal = search
                .grain_reversal_materials
                .iter()
                .any(|hash| hash == rolls[source].material.sha256());
            if let Some(placements) = absorb(
                &rolls[target],
                &rolls[source],
                options,
                allow_reversal,
                &mut work,
            ) {
                transferred_placements += rolls[source].placements.len();
                accepted_merges += 1;
                rolls[target].placements = placements;
                rolls[target].external_cut_area_m2 = area;
                rolls[target].utilization = utilization;
                // This marker no longer has the same piece assignment as its old shelf baseline.
                rolls[target].nesting = None;
                rolls.remove(source);
                length_m = candidate_length;
            } else if work.exhausted {
                break 'search;
            } else {
                source += 1;
            }
        }
        target += 1;
    }
    Ok(MarkerConsolidationReport {
        algorithm: "bounded_whole_marker_consolidation/1",
        baseline_marker_count,
        marker_count: rolls.len(),
        baseline_length_m,
        length_m,
        saved_length_m: baseline_length_m - length_m,
        accepted_merges,
        transferred_placements,
        candidates: work.candidates,
        work_limit: policy.work_limit,
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
