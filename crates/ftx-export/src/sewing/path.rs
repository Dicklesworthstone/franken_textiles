//! Offset stitch lines on the existing 200-sample seam-side geometry. Mitres preserve vertex
//! correspondence; collapsed/reversed/self-crossing offsets refuse rather than trimming material.
//! Positive offset is inward in the original CCW instance, before any seam or pass reversal.

use ftx_core::curve::{CurveKind, EDGE_DENSE};
use ftx_core::polyline::{at_fractions, cumulative, point_in_polygon, signed_area};
use ftx_core::{Refusal, RefusalCode, Vec2, np};
use ftx_pattern::{Pattern, Seam, SeamPairs};
use serde::{Deserialize, Serialize};

use super::{MAX_STATIONS, bad, positive};
use crate::Ctx;
use crate::outline::{cut_outline, edge_sa, ring_is_simple, segments_touch};

#[path = "obstacles.rs"]
pub(super) mod obstacles;
use obstacles::Obstacles;

const MAX_VERTICES: usize = 8192;
const MAX_GEOMETRY_WORK: usize = 50_000_000;
const EPS_MM: f64 = 1e-8;

/// Explicit flat stitch-line geometry, not a folded seam recipe or tool-radius compensation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetPath {
    /// Signed offsets [a, b] in mm: positive into each CCW piece, negative into its allowance.
    pub offset_mm: [f64; 2],
    /// Largest allowed mitre length divided by the absolute offset; refusal, never bevel/clamp.
    pub mitre_limit: f64,
    /// Required clearance to the exterior, internal cuts/slits and drill rims (mm), positive.
    pub minimum_cut_clearance_mm: f64,
}

/// Reproducible offset geometry and links back to the authored seam, independent of feed direction.
#[derive(Clone, Debug, Serialize)]
pub struct PathReceipt {
    /// Full offset side polylines, in paired canonical order and separate local millimetre frames.
    pub polylines_mm: [Vec<Vec2>; 2],
    /// Original seam-side lengths [a, b] before offset, in mm.
    pub source_lengths_mm: [f64; 2],
    /// Original arclength fractions at each output station, in paired canonical order.
    pub source_fractions: [Vec<f64>; 2],
    /// Smallest cut/drill-rim clearance of the full path AND between-stitch chords, by side (mm).
    pub minimum_cut_clearance_mm: [f64; 2],
}

pub(super) fn charge(work: &mut usize, amount: usize) -> Result<(), Refusal> {
    *work = work.saturating_add(amount);
    if *work > MAX_GEOMETRY_WORK {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "offset stitch geometry budget exceeded",
        ));
    }
    Ok(())
}

/// An orientation-preserving piecewise-linear correspondence between source and offset arclength.
struct Line {
    points: Vec<Vec2>,
    source_f: Vec<f64>,
    offset_f: Vec<f64>,
    source_len: f64,
    length: f64,
}

impl Line {
    fn new(
        mut source: Vec<Vec2>,
        offset: f64,
        limit: f64,
        work: &mut usize,
    ) -> Result<Self, Refusal> {
        // An exactly repeated endpoint is not a geometric segment. Never collapse near-zero spans.
        source.dedup();
        if source.len() < 2 || source.len() > MAX_VERTICES || source.iter().any(|p| !p.is_finite())
        {
            return Err(bad(
                "offset stitch line needs bounded finite source geometry",
            ));
        }
        let closed = source.first() == source.last();
        simple(&source, work)?;
        let mut tangents = Vec::with_capacity(source.len() - 1);
        for w in source.windows(2) {
            let delta = w[1] - w[0];
            if !positive(delta.norm()) || delta.norm() <= EPS_MM {
                return Err(bad("degenerate source span in offset stitch line"));
            }
            tangents.push(delta / delta.norm());
        }
        let last = source.len() - 1;
        let mut points = source.clone();
        if offset != 0.0 {
            for (i, point) in points.iter_mut().enumerate() {
                let normal = if !closed && i == 0 {
                    tangents[0].perp()
                } else if !closed && i == last {
                    tangents[last - 1].perp()
                } else {
                    let before = tangents[if i == 0 { last - 1 } else { i - 1 }];
                    let after = tangents[if i == last { 0 } else { i }];
                    let den = 1.0 + before.dot(after);
                    if den <= 1e-12 {
                        return Err(bad("offset stitch line has a singular reversing corner"));
                    }
                    (before.perp() + after.perp()) / den
                };
                if normal.norm() > limit * (1.0 + 1e-12) {
                    return Err(bad("offset stitch line exceeds the declared mitre limit"));
                }
                *point += normal * offset;
                if !point.is_finite() {
                    return Err(bad("non-finite offset stitch coordinate"));
                }
            }
            for (w, tangent) in points.windows(2).zip(&tangents) {
                if (w[1] - w[0]).dot(*tangent) <= EPS_MM {
                    return Err(bad(
                        "offset stitch line collapses or reverses a source span",
                    ));
                }
            }
            simple(&points, work)?;
        }
        let mut source_f = cumulative(&source);
        let mut offset_f = cumulative(&points);
        let source_len = source_f[last];
        let length = offset_f[last];
        if !positive(length) || length >= 1e5 || !positive(source_len) || source_len >= 1e5 {
            return Err(bad("implausible offset/source stitch-line length"));
        }
        for f in &mut source_f {
            *f /= source_len;
        }
        for f in &mut offset_f {
            *f /= length;
        }
        Ok(Self {
            points,
            source_f,
            offset_f,
            source_len,
            length,
        })
    }

    fn project(&self, fractions: &[f64]) -> Vec<f64> {
        fractions
            .iter()
            .map(|&f| np::interp(f, &self.source_f, &self.offset_f))
            .collect()
    }

    fn unproject(&self, fractions: &[f64]) -> Vec<f64> {
        fractions
            .iter()
            .map(|&f| np::interp(f, &self.offset_f, &self.source_f))
            .collect()
    }
}

/// Non-adjacent contact is invalid, including overlapping spans. Adjacent reversal is checked too.
pub(super) fn simple(points: &[Vec2], work: &mut usize) -> Result<(), Refusal> {
    let count = points.len().saturating_sub(1);
    charge(work, count.saturating_mul(count))?;
    let closed = points.first() == points.last();
    for i in 0..count {
        for j in i + 1..count {
            if j == i + 1 || (closed && i == 0 && j == count - 1) {
                let (a, shared, b) = if j == i + 1 {
                    (points[i], points[j], points[j + 1])
                } else {
                    (points[1], points[0], points[j])
                };
                if segments_touch(a, shared, b, b) || segments_touch(shared, b, a, a) {
                    return Err(bad("offset stitch line doubles back"));
                }
            } else if segments_touch(points[i], points[i + 1], points[j], points[j + 1]) {
                return Err(bad("offset stitch line self-intersects"));
            }
        }
    }
    Ok(())
}

fn point_distance(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let t = ((p - a).dot(d) / d.norm2()).clamp(0.0, 1.0);
    (p - (a + d * t)).norm()
}

/// Entire segments, not just stations: otherwise a chord can jump outside a concave piece.
pub(super) fn clearance(
    points: &[Vec2],
    ring: &[Vec2],
    required: f64,
    work: &mut usize,
) -> Result<f64, Refusal> {
    charge(work, points.len().saturating_mul(ring.len()))?;
    if !point_in_polygon(points[0], ring) {
        return Err(bad("offset stitch line is outside the cut outline"));
    }
    let mut minimum = f64::INFINITY;
    for w in points.windows(2) {
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            if segments_touch(w[0], w[1], a, b) {
                return Err(bad("offset stitch line crosses or touches the cut outline"));
            }
            let distance = point_distance(w[0], a, b)
                .min(point_distance(w[1], a, b))
                .min(point_distance(a, w[0], w[1]))
                .min(point_distance(b, w[0], w[1]));
            minimum = minimum.min(distance);
        }
    }
    if !minimum.is_finite() || minimum + EPS_MM < required {
        return Err(bad(
            "offset stitch line lacks the required cut-edge clearance",
        ));
    }
    Ok(minimum)
}

impl OffsetPath {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if self
            .offset_mm
            .iter()
            .any(|x| !x.is_finite() || x.abs() > 10000.0)
            || !self.mitre_limit.is_finite()
            || !(1.0..=100.0).contains(&self.mitre_limit)
            || !positive(self.minimum_cut_clearance_mm)
        {
            return Err(bad("invalid offset stitch-line policy"));
        }
        Ok(())
    }

    pub(super) fn build(
        &self,
        pattern: &Pattern,
        seam: &Seam,
        pitch: f64,
        ctx: &Ctx<'_>,
        work: &mut usize,
    ) -> Result<(SeamPairs, Option<PathReceipt>), Refusal> {
        let ra = seam
            .side_ref("a")
            .ok_or_else(|| bad("missing seam side a"))?;
        let rb = seam
            .side_ref("b")
            .ok_or_else(|| bad("missing seam side b"))?;
        let mut lines = Vec::with_capacity(2);
        let mut rings = Vec::with_capacity(2);
        let mut obstacles = Vec::with_capacity(2);
        let mut junctions = Vec::with_capacity(2);
        let mut notches = Vec::with_capacity(2);
        for (side, reference) in [ra, rb].into_iter().enumerate() {
            let (instance, _) = pattern.side(reference).map_err(|e| bad(e.to_string()))?;
            let piece =
                &pattern.doc.as_ref().expect("validated pattern").pieces[instance.piece_index];
            if instance.edges.iter().any(|edge| {
                let sa = edge_sa(piece, edge.source, ctx.sa);
                !sa.is_finite() || sa < 0.0
            }) {
                return Err(bad(
                    "offset stitching needs finite nonnegative cut allowances",
                ));
            }
            // Bound both seam densification (200) and export contour densification (at most 800).
            let estimate = instance.edges.iter().try_fold(0usize, |sum, edge| {
                let count = match &edge.curve {
                    None => 2,
                    Some(c) => match c.kind().map_err(|e| bad(e.to_string()))? {
                        CurveKind::Line => 2,
                        CurveKind::Poly => c.controls().len().saturating_add(2),
                        _ => 800,
                    },
                };
                Ok::<_, Refusal>(sum.saturating_add(count))
            })?;
            if estimate > MAX_VERTICES {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "offset contour vertex budget exceeded",
                ));
            }
            charge(work, estimate.saturating_mul(estimate))?;
            let source_ring = instance
                .outline(EDGE_DENSE)
                .map_err(|e| bad(e.to_string()))?;
            if source_ring.iter().any(|p| !p.is_finite())
                || signed_area(&source_ring) <= 0.0
                || !ring_is_simple(&source_ring)
            {
                return Err(bad(
                    "offset stitching needs a simple counter-clockwise source piece",
                ));
            }
            let (ring, _) = cut_outline(instance, piece, None, ctx.sa, ctx.opts.mitre_limit)?;
            if ring.len() > MAX_VERTICES
                || ring.iter().any(|p| !p.is_finite())
                || !ring_is_simple(&ring)
            {
                return Err(bad("invalid/oversized offset stitching cut outline"));
            }
            obstacles.push(Obstacles::load(pattern, instance, &ring, work)?);
            let (mut points, mut js) = pattern
                .side_polyline(reference)
                .map_err(|e| bad(e.to_string()))?;
            let mut ns = pattern
                .notch_fractions(reference)
                .map_err(|e| bad(e.to_string()))?;
            let reverse = side == 1 && seam.reverse();
            if reverse {
                points.reverse();
                js = js.into_iter().rev().map(|f| 1.0 - f).collect();
                ns = ns.into_iter().rev().map(|f| 1.0 - f).collect();
            }
            let offset = self.offset_mm[side] * if reverse { -1.0 } else { 1.0 };
            let line = Line::new(points, offset, self.mitre_limit, work)?;
            junctions.push(line.project(&js));
            notches.push(line.project(&ns));
            lines.push(line);
            rings.push(ring);
        }
        if notches[0].len() != notches[1].len() {
            return Err(bad("offset stitch-line notches must pair before sewing"));
        }
        let wrap = |ns: &[f64]| [&[0.0][..], ns, &[1.0][..]].concat();
        let ka = wrap(&notches[0]);
        let kb = wrap(&notches[1]);
        let to_b = |f| np::interp(f, &ka, &kb);
        let to_a = |f| np::interp(f, &kb, &ka);
        let mut breaks: Vec<_> = ka
            .iter()
            .chain(&junctions[0])
            .copied()
            .chain(junctions[1].iter().map(|&f| to_a(f)))
            .collect();
        breaks.sort_by(f64::total_cmp);
        // Distinct authored boundaries are never rounded together to manufacture a feasible stitch.
        breaks.dedup();
        let mut counts = Vec::with_capacity(breaks.len());
        let mut total = 1usize;
        for w in breaks.windows(2) {
            let span =
                ((w[1] - w[0]) * lines[0].length).max((to_b(w[1]) - to_b(w[0])) * lines[1].length);
            let count = (span / pitch).ceil().max(1.0);
            if !count.is_finite()
                || count > MAX_STATIONS as f64
                || total.saturating_add(count as usize) > MAX_STATIONS
            {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "offset stitch station budget exceeded",
                ));
            }
            total += count as usize;
            counts.push(count as usize);
        }
        let mut fa = Vec::with_capacity(total);
        let mut fb = Vec::with_capacity(total);
        fa.push(0.0);
        fb.push(0.0);
        for (w, count) in breaks.windows(2).zip(counts) {
            for &u in &np::linspace(w[0], w[1], count + 1)[1..] {
                fa.push(u);
                fb.push(to_b(u));
            }
        }
        let mut stations = SeamPairs {
            a: at_fractions(&lines[0].points, &fa),
            b: at_fractions(&lines[1].points, &fb),
            fa,
            fb,
            len_a: lines[0].length,
            len_b: lines[1].length,
            a_junctions: junctions[0].clone(),
            b_junctions: junctions[1].clone(),
            notches_a: notches[0].clone(),
            notches_b: notches[1].clone(),
            notch_match: if notches[0].is_empty() {
                "none"
            } else {
                "piecewise"
            }
            .into(),
        };
        // Explicit zero offsets add a clearance check but keep original station arithmetic unchanged.
        if self.offset_mm == [0.0, 0.0] {
            stations = pattern
                .seam_pairs(seam, pitch)
                .map_err(|e| bad(e.to_string()))?;
        }
        let mut minimum = [0.0; 2];
        for (i, points) in [&stations.a, &stations.b].into_iter().enumerate() {
            minimum[i] = clearance(
                &lines[i].points,
                &rings[i],
                self.minimum_cut_clearance_mm,
                work,
            )?
            .min(clearance(
                points,
                &rings[i],
                self.minimum_cut_clearance_mm,
                work,
            )?)
            .min(obstacles[i].clearance(&lines[i].points, self.minimum_cut_clearance_mm, work)?)
            .min(obstacles[i].clearance(
                points,
                self.minimum_cut_clearance_mm,
                work,
            )?);
        }
        let receipt = PathReceipt {
            polylines_mm: [lines[0].points.clone(), lines[1].points.clone()],
            source_lengths_mm: [lines[0].source_len, lines[1].source_len],
            source_fractions: [
                lines[0].unproject(&stations.fa),
                lines[1].unproject(&stations.fb),
            ],
            minimum_cut_clearance_mm: minimum,
        };
        Ok((stations, Some(receipt)))
    }
}
