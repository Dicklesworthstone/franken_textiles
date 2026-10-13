//! The planar operations the reference's soutache layout runs through shapely (GEOS 3.14.1): a line's offset
//! curve with mitre joins, the part of a line inside a polygon, and whether a line keeps a clearance inside one.
//!
//! The offset's raw curve is GEOS's own construction, ported line for line: the input simplified on the offset
//! side (`BufferInputLineSimplifier`, tolerance distance/100, with its quirk of sampling against the middle
//! vertex), then `OffsetSegmentGenerator` (outside turns mitred, limited mitres and bevels; inside turns at the
//! offsets' crossing or with closing segments through the midpoints; points nearer than distance·10⁻⁴ to the
//! last one dropped). GEOS then keeps the stretches of that curve on its buffer's ring; here they are found
//! directly: the curve is split where it crosses itself and a piece is kept when it lies at least the distance
//! from the line (the loops at inside corners lie nearer). Valid while the line does not come back within twice
//! the distance of itself, as the drafted braid lines never do. Crossing points are computed in doubles (GEOS
//! uses double-double), so they can differ in the last bits; the layout rounds to 0.01 mm.

use ftx_core::Vec2;
use ftx_core::exact::{Sign, orient2d};

/// GEOS `Orientation::index`: 1 when `q` is left of `p1 → p2`, −1 right, 0 on the line (exact).
fn orientation(p1: Vec2, p2: Vec2, q: Vec2) -> i32 {
    match orient2d([p1.x, p1.y], [p2.x, p2.y], [q.x, q.y]) {
        Sign::Positive => 1,
        Sign::Negative => -1,
        Sign::Zero => 0,
    }
}

fn dist(a: Vec2, b: Vec2) -> f64 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    (dx * dx + dy * dy).sqrt()
}

/// GEOS `Distance::pointToSegment`.
fn point_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    if a == b {
        return dist(p, a);
    }
    let len2 = (b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y);
    let r = ((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y)) / len2;
    if r <= 0.0 {
        return dist(p, a);
    }
    if r >= 1.0 {
        return dist(p, b);
    }
    let s = ((a.y - p.y) * (b.x - a.x) - (a.x - p.x) * (b.y - a.y)) / len2;
    s.abs() * len2.sqrt()
}

/// GEOS `BufferInputLineSimplifier` for the left side (positive tolerance): shallow left turns removed.
fn simplify_left(pts: &[Vec2], tol: f64) -> Vec<Vec2> {
    let n = pts.len();
    let mut deleted = vec![false; n];
    let next = |deleted: &[bool], i: usize| {
        let mut j = i + 1;
        while j < n && deleted[j] {
            j += 1;
        }
        j
    };
    let deletable = |i0: usize, i1: usize, i2: usize| {
        let (p0, p1, p2) = (pts[i0], pts[i1], pts[i2]);
        if orientation(p0, p1, p2) != 1 || point_to_segment(p1, p0, p2) >= tol {
            return false;
        }
        // isShallowSampled(p0, p1, i0, i2): GEOS passes the middle vertex as the segment's far end
        let inc = ((i2 - i0) / 10).max(1);
        (i0..i2)
            .step_by(inc)
            .all(|i| point_to_segment(p1, p0, pts[i]) < tol)
    };
    loop {
        let mut changed = false;
        let mut index = 1;
        let mut mid = next(&deleted, index);
        let mut last = next(&deleted, mid);
        while last < n {
            let del = deletable(index, mid, last);
            if del {
                deleted[mid] = true;
                changed = true;
            }
            index = if del { last } else { mid };
            mid = next(&deleted, index);
            last = next(&deleted, mid);
        }
        if !changed {
            break;
        }
    }
    pts.iter()
        .zip(&deleted)
        .filter(|(_, d)| !**d)
        .map(|(p, _)| *p)
        .collect()
}

/// Where the lines through `p0 p1` and `q0 q1` cross (None when parallel).
fn line_crossing(p0: Vec2, p1: Vec2, q0: Vec2, q1: Vec2) -> Option<Vec2> {
    let (r, s) = (p1 - p0, q1 - q0);
    let den = r.cross(s);
    if den == 0.0 {
        return None;
    }
    let t = (q0 - p0).cross(s) / den;
    let x = p0 + r * t;
    x.is_finite().then_some(x)
}

/// GEOS `LineIntersector::computeIntersection` on two segments: its first intersection point, if any.
fn segment_intersection(p1: Vec2, p2: Vec2, q1: Vec2, q2: Vec2) -> Option<Vec2> {
    let env = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| {
        a.x.min(b.x) <= c.x.max(d.x)
            && c.x.min(d.x) <= a.x.max(b.x)
            && a.y.min(b.y) <= c.y.max(d.y)
            && c.y.min(d.y) <= a.y.max(b.y)
    };
    if !env(p1, p2, q1, q2) {
        return None;
    }
    let (pq1, pq2) = (orientation(p1, p2, q1), orientation(p1, p2, q2));
    if (pq1 > 0 && pq2 > 0) || (pq1 < 0 && pq2 < 0) {
        return None;
    }
    let (qp1, qp2) = (orientation(q1, q2, p1), orientation(q1, q2, p2));
    if (qp1 > 0 && qp2 > 0) || (qp1 < 0 && qp2 < 0) {
        return None;
    }
    if pq1 == 0 && pq2 == 0 && qp1 == 0 && qp2 == 0 {
        let inside = |a: Vec2, b: Vec2, c: Vec2| env(a, b, c, c);
        return [
            (inside(p1, p2, q1), q1),
            (inside(q1, q2, p1), p1),
            (inside(p1, p2, q2), q2),
            (inside(q1, q2, p2), p2),
        ]
        .into_iter()
        .find_map(|(ok, p)| ok.then_some(p));
    }
    if pq1 == 0 || pq2 == 0 || qp1 == 0 || qp2 == 0 {
        return Some(if p1 == q1 || p1 == q2 {
            p1
        } else if p2 == q1 || p2 == q2 {
            p2
        } else if pq1 == 0 {
            q1
        } else if pq2 == 0 {
            q2
        } else if qp1 == 0 {
            p1
        } else {
            p2
        });
    }
    let x = line_crossing(p1, p2, q1, q2)?;
    // GEOS falls back to the nearest endpoint when rounding puts the point outside both envelopes
    if env(p1, p2, x, x) && env(q1, q2, x, x) {
        Some(x)
    } else {
        [p1, p2, q1, q2].into_iter().min_by(|a, b| {
            let da = point_to_segment(*a, q1, q2).min(point_to_segment(*a, p1, p2));
            let db = point_to_segment(*b, q1, q2).min(point_to_segment(*b, p1, p2));
            da.total_cmp(&db)
        })
    }
}

/// GEOS `OffsetSegmentGenerator` for one side (LEFT) with mitre joins.
struct SegGen {
    d: f64,
    limit: f64,
    pts: Vec<Vec2>,
    s0: Vec2,
    s1: Vec2,
    s2: Vec2,
    o0: [Vec2; 2],
    o1: [Vec2; 2],
}

impl SegGen {
    fn offset(&self, a: Vec2, b: Vec2) -> [Vec2; 2] {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        let (ux, uy) = (self.d * dx / len, self.d * dy / len);
        [Vec2::new(a.x - uy, a.y + ux), Vec2::new(b.x - uy, b.y + ux)]
    }

    fn add(&mut self, p: Vec2) {
        if let Some(&last) = self.pts.last()
            && dist(p, last) < self.d * 1.0e-4
        {
            return;
        }
        self.pts.push(p);
    }

    fn next(&mut self, p: Vec2) {
        if self.s2 == p {
            return;
        }
        (self.s0, self.s1, self.s2) = (self.s1, self.s2, p);
        self.o0 = self.offset(self.s0, self.s1);
        self.o1 = self.offset(self.s1, self.s2);
        let [o0p0, o0p1] = self.o0;
        let [o1p0, o1p1] = self.o1;
        match orientation(self.s0, self.s1, self.s2) {
            0 => {
                // collinear: only a reversal adds points (the offsets then run back)
                if (self.s1 - self.s0).dot(self.s2 - self.s1) < 0.0 {
                    self.add(o0p1);
                    self.add(o1p0);
                }
            }
            -1 => {
                // outside turn
                if dist(o0p1, o1p0) < self.d * 1.0e-3 {
                    self.add(o0p1);
                } else {
                    self.mitre(o0p0, o0p1, o1p0, o1p1);
                }
            }
            _ => {
                // inside turn
                if let Some(x) = segment_intersection(o0p0, o0p1, o1p0, o1p1) {
                    self.add(x);
                } else if dist(o0p1, o1p0) < self.d * 1.0e-3 {
                    self.add(o0p1);
                } else {
                    let s1 = self.s1;
                    let mid = |q: Vec2| Vec2::new((q.x + s1.x) / 2.0, (q.y + s1.y) / 2.0);
                    self.add(o0p1);
                    self.add(mid(o0p1));
                    self.add(mid(o1p0));
                    self.add(o1p0);
                }
            }
        }
    }

    fn mitre(&mut self, o0p0: Vec2, o0p1: Vec2, o1p0: Vec2, o1p1: Vec2) {
        let limit_d = self.limit * self.d;
        let corner = self.s1;
        if let Some(x) = line_crossing(o0p0, o0p1, o1p0, o1p1)
            && dist(x, corner) <= limit_d
        {
            self.add(x);
            return;
        }
        if point_to_segment(corner, o0p1, o1p0) >= limit_d {
            self.add(o0p1);
            self.add(o1p0);
            return;
        }
        // limited mitre: a bevel square to the bisector at the limit distance
        let ang = |a: Vec2, b: Vec2| ftx_core::det::atan2(b.y - a.y, b.x - a.x);
        let norm = |a: f64| {
            let mut a = a;
            while a > std::f64::consts::PI {
                a -= 2.0 * std::f64::consts::PI;
            }
            while a <= -std::f64::consts::PI {
                a += 2.0 * std::f64::consts::PI;
            }
            a
        };
        let a1 = ang(corner, self.s0);
        let a2 = ang(corner, self.s2);
        let interior = {
            let d = a2 - a1;
            if d <= -std::f64::consts::PI {
                d + 2.0 * std::f64::consts::PI
            } else if d > std::f64::consts::PI {
                d - 2.0 * std::f64::consts::PI
            } else {
                d
            }
        };
        let bis = norm(a1 + interior / 2.0);
        let bis_out = norm(bis + std::f64::consts::PI);
        let project = |p: Vec2, d: f64, a: f64| {
            let (s, c) = ftx_core::det::sin_cos(a);
            Vec2::new(p.x + d * c, p.y + d * s)
        };
        let mid = project(corner, limit_d, bis_out);
        let dir = norm(bis_out + std::f64::consts::FRAC_PI_2);
        let (b0, b1) = (
            project(mid, self.d, dir),
            project(mid, self.d, dir + std::f64::consts::PI),
        );
        match (
            line_crossing(o0p0, o0p1, b0, b1),
            line_crossing(o1p0, o1p1, b0, b1),
        ) {
            (Some(i0), Some(i1)) => {
                self.add(i0);
                self.add(i1);
            }
            _ => {
                self.add(o0p1);
                self.add(o1p0);
            }
        }
    }
}

/// GEOS's raw left offset curve of an open line at distance `d` (mitre joins, `limit`), open (the ring closure
/// GEOS appends is left out).
fn raw_left_offset(line: &[Vec2], d: f64, limit: f64) -> Vec<Vec2> {
    let mut clean: Vec<Vec2> = Vec::with_capacity(line.len());
    for &p in line {
        if p.is_finite() && clean.last() != Some(&p) {
            clean.push(p);
        }
    }
    let simp = simplify_left(&clean, d / 100.0);
    let mut g = SegGen {
        d,
        limit,
        pts: Vec::new(),
        s0: simp[0],
        s1: simp[0],
        s2: simp[1],
        o0: [simp[0], simp[0]],
        o1: [simp[0], simp[0]],
    };
    g.o1 = g.offset(simp[0], simp[1]);
    g.add(g.o1[0]);
    for &p in &simp[2..] {
        g.next(p);
    }
    g.add(g.o1[1]);
    g.pts
}

fn distance_to_line(p: Vec2, line: &[Vec2]) -> f64 {
    line.windows(2)
        .map(|w| point_to_segment(p, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

/// Splits a polyline where it meets `cutters` (pairs of segment endpoints) or itself (`cutters` None): the
/// pieces in order, each a polyline.
fn split(line: &[Vec2], cutters: Option<&[Vec2]>) -> Vec<Vec<Vec2>> {
    let m = line.len() - 1;
    let mut cuts: Vec<Vec<(f64, Vec2)>> = vec![Vec::new(); m];
    let seg_t = |a: Vec2, b: Vec2, x: Vec2| {
        let ab = b - a;
        let l2 = ab.dot(ab);
        if l2 == 0.0 {
            0.0
        } else {
            ((x - a).dot(ab) / l2).clamp(0.0, 1.0)
        }
    };
    match cutters {
        None => {
            for i in 0..m {
                for j in i + 2..m {
                    if let Some(x) =
                        segment_intersection(line[i], line[i + 1], line[j], line[j + 1])
                    {
                        cuts[i].push((seg_t(line[i], line[i + 1], x), x));
                        cuts[j].push((seg_t(line[j], line[j + 1], x), x));
                    }
                }
            }
        }
        Some(ring) => {
            let k = ring.len();
            for i in 0..m {
                for j in 0..k {
                    let (c0, c1) = (ring[j], ring[(j + 1) % k]);
                    if let Some(x) = segment_intersection(line[i], line[i + 1], c0, c1) {
                        cuts[i].push((seg_t(line[i], line[i + 1], x), x));
                    }
                }
            }
        }
    }
    // pieces: break at every cut point
    let mut pieces: Vec<Vec<Vec2>> = Vec::new();
    let mut cur = vec![line[0]];
    for i in 0..m {
        cuts[i].sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, x) in &cuts[i] {
            if cur.last() != Some(&x) {
                cur.push(x);
            }
            if cur.len() > 1 {
                pieces.push(std::mem::replace(&mut cur, vec![x]));
            }
        }
        if cur.last() != Some(&line[i + 1]) {
            cur.push(line[i + 1]);
        }
    }
    if cur.len() > 1 {
        pieces.push(cur);
    }
    pieces
}

/// Joins consecutive kept pieces whose ends meet into runs.
fn join(pieces: Vec<Vec<Vec2>>) -> Vec<Vec<Vec2>> {
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    for p in pieces {
        match out.last_mut() {
            Some(run) if run.last() == p.first() => run.extend_from_slice(&p[1..]),
            _ => out.push(p),
        }
    }
    out
}

fn midpoint(piece: &[Vec2]) -> Vec2 {
    // the middle of the piece's middle segment, away from the cut ends
    let k = (piece.len() - 1) / 2;
    (piece[k] + piece[k + 1]) * 0.5
}

/// `LineString(line).offset_curve(d, join_style="mitre", mitre_limit=limit)` for `d > 0` (the left side): its
/// sections, each in the line's direction.
#[must_use]
pub fn offset_curve(line: &[Vec2], d: f64, limit: f64) -> Vec<Vec<Vec2>> {
    let raw = raw_left_offset(line, d, limit);
    if raw.len() < 2 {
        return Vec::new();
    }
    if line.len() == 2 {
        return vec![raw];
    }
    let keep = d * (1.0 - 1.0e-9);
    let pieces: Vec<Vec<Vec2>> = split(&raw, None)
        .into_iter()
        .filter(|p| distance_to_line(midpoint(p), line) >= keep)
        .collect();
    join(pieces)
}

/// Even-odd point in polygon.
fn inside(p: Vec2, ring: &[Vec2]) -> bool {
    ftx_core::polyline::point_in_polygon(p, ring)
}

/// `Polygon(ring).intersection(line)`'s line parts, in the line's direction.
#[must_use]
pub fn clip_inside(line: &[Vec2], ring: &[Vec2]) -> Vec<Vec<Vec2>> {
    let pieces: Vec<Vec<Vec2>> = split(line, Some(ring))
        .into_iter()
        .filter(|p| inside(midpoint(p), ring))
        .collect();
    join(pieces)
}

/// The length of a polyline.
#[must_use]
pub fn length(line: &[Vec2]) -> f64 {
    line.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// `Polygon(ring).buffer(-clear).contains(LineString(line))`, decided by distance: every vertex inside the
/// polygon and the line at least `clear` from its boundary (shapely's rounded inner corners are chords, so a
/// line within a few hundredths of a millimetre of the limit may be judged differently).
#[must_use]
pub fn keeps_clear(line: &[Vec2], ring: &[Vec2], clear: f64) -> bool {
    if !line.iter().all(|&p| inside(p, ring)) {
        return false;
    }
    let k = ring.len();
    let seg_seg = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| {
        if segment_intersection(a, b, c, d).is_some() {
            return 0.0;
        }
        point_to_segment(a, c, d)
            .min(point_to_segment(b, c, d))
            .min(point_to_segment(c, a, b))
            .min(point_to_segment(d, a, b))
    };
    line.windows(2)
        .all(|w| (0..k).all(|j| seg_seg(w[0], w[1], ring[j], ring[(j + 1) % k]) > clear))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_corner_offsets_to_its_inner_corner() {
        // a right angle turning left, offset 1 to the left (inside): the curve turns at (9, 1)
        let line = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
        ];
        let secs = offset_curve(&line, 1.0, 3.0);
        assert_eq!(secs.len(), 1);
        assert_eq!(
            secs[0],
            vec![
                Vec2::new(0.0, 1.0),
                Vec2::new(9.0, 1.0),
                Vec2::new(9.0, 10.0)
            ]
        );
        // turning right, the mitre: (11, -1) ... (11, 10)
        let line = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, -10.0),
        ];
        let secs = offset_curve(&line, 1.0, 3.0);
        assert_eq!(
            secs[0],
            vec![
                Vec2::new(0.0, 1.0),
                Vec2::new(11.0, 1.0),
                Vec2::new(11.0, -10.0)
            ]
        );
    }

    #[test]
    fn a_dense_inside_corner_loses_its_loop() {
        // short segments into a sharp left corner: the raw offset loops, the curve does not
        let mut line: Vec<Vec2> = (0..=20)
            .map(|i| Vec2::new(f64::from(i) * 0.5, 0.0))
            .collect();
        line.extend((1..=20).map(|i| Vec2::new(10.0, f64::from(i) * 0.5)));
        let secs = offset_curve(&line, 2.0, 3.0);
        assert_eq!(secs.len(), 1);
        let c = &secs[0];
        assert!(
            c.iter().all(|p| distance_to_line(*p, &line) >= 2.0 - 1e-9),
            "{c:?}"
        );
        assert!(c.contains(&Vec2::new(8.0, 2.0)), "{c:?}");
    }

    #[test]
    fn clipping_keeps_the_inside_part() {
        let ring = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        let line = [
            Vec2::new(-5.0, 5.0),
            Vec2::new(5.0, 5.0),
            Vec2::new(15.0, 5.0),
        ];
        assert_eq!(
            clip_inside(&line, &ring),
            vec![vec![
                Vec2::new(0.0, 5.0),
                Vec2::new(5.0, 5.0),
                Vec2::new(10.0, 5.0)
            ]]
        );
        assert!(keeps_clear(
            &[Vec2::new(4.0, 4.0), Vec2::new(6.0, 6.0)],
            &ring,
            3.0
        ));
        assert!(!keeps_clear(
            &[Vec2::new(4.0, 4.0), Vec2::new(8.0, 6.0)],
            &ring,
            3.0
        ));
    }
}
