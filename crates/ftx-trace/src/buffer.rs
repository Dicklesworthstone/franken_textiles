//! shapely's `Polygon.buffer` (GEOS 3.14.1) for the trace's polygons: the raw ring curves ported line for
//! line (`BufferCurveSetBuilder` with its erosion and inversion checks → `OffsetCurveBuilder::
//! computeRingBufferCurve`: `BufferInputLineSimplifier`, `OffsetSegmentGenerator` with mitre,
//! limited-mitre, bevel and round joins, `LineIntersector`, the double-double `CGAlgorithmsDD::
//! intersection` and `orientationIndex`, `Angle::sinCosSnap`), then GEOS's noding and polygon building
//! ([`crate::buffer_graph`]). Also shapely's `LineString.distance` / `project(normalized=True)` for points.

use ftx_core::{Vec2, det};

pub use crate::buffer_graph::Polygon;
use crate::buffer_graph::{self, Curve};

/// Join style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Join {
    /// Round, with this many segments per quadrant (shapely's default 16).
    Round(u32),
    /// Mitre, with this limit (× the distance).
    Mitre(f64),
}

/// GEOS `Orientation::index` (`CGAlgorithmsDD::orientationIndex`): Ozaki's filter, then the determinant in
/// double-double, which is not exact (a tiny determinant can come out 0, so nearly collinear points count as
/// collinear exactly where GEOS's do) and never overflows a certified range. 1 left, −1 right, 0 collinear.
pub(crate) fn orientation(p1: Vec2, p2: Vec2, q: Vec2) -> i32 {
    if !q.is_finite() {
        return 0; // GEOS throws; a non-finite point never reaches here (inputs are filtered)
    }
    let detleft = (p1.x - q.x) * (p2.y - q.y);
    let detright = (p1.y - q.y) * (p2.x - q.x);
    let det = detleft - detright;
    let error = (detleft + detright).abs() * 3.330_669_062_177_372_4e-16;
    if det.abs() >= error {
        return i32::from(det > 0.0) - i32::from(det < 0.0);
    }
    let dx1 = Dd(p2.x, 0.0).add(-p1.x, 0.0);
    let dy1 = Dd(p2.y, 0.0).add(-p1.y, 0.0);
    let dx2 = Dd(q.x, 0.0).add(-p2.x, 0.0);
    let dy2 = Dd(q.y, 0.0).add(-p2.y, 0.0);
    let d = dx1.mul(dy2).sub(dy1.mul(dx2));
    // `DD::operator<` / `>` against zero: the high part, then the low part
    if d.0 < 0.0 || (d.0 == 0.0 && d.1 < 0.0) {
        -1
    } else if d.0 > 0.0 || (d.0 == 0.0 && d.1 > 0.0) {
        1
    } else {
        0
    }
}

/// GEOS `Coordinate::distance`.
pub(crate) fn dist(a: Vec2, b: Vec2) -> f64 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    (dx * dx + dy * dy).sqrt()
}

/// A FIXED GEOS `PrecisionModel` (`PrecisionModel(scale)`, `setScale`): nearly integral scales and grid
/// sizes snap to the integer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fixed {
    scale: f64,
    grid: f64,
}

impl Fixed {
    pub(crate) fn new(scale: f64) -> Self {
        let snap = |v: f64| {
            let r = v.round();
            if (v - r).abs() < 1e-5 { r } else { v }
        };
        if scale < 1.0 {
            Self {
                scale,
                grid: snap(1.0 / scale),
            }
        } else {
            let scale = snap(scale);
            Self {
                scale,
                grid: 1.0 / scale,
            }
        }
    }

    /// `PrecisionModel::makePrecise`.
    fn precise(self, v: f64) -> f64 {
        if self.grid > 1.0 {
            buffer_graph::java_round(v / self.grid) * self.grid
        } else {
            buffer_graph::java_round(v * self.scale) / self.scale
        }
    }
}

/// GEOS `Distance::pointToSegment`.
pub(crate) fn point_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
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

/// GEOS `Distance::pointToLinePerpendicular`.
fn point_to_line_perpendicular(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let len2 = (b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y);
    let s = ((a.y - p.y) * (b.x - a.x) - (a.x - p.x) * (b.y - a.y)) / len2;
    s.abs() * len2.sqrt()
}

/// GEOS's double-double (`math::DD`), the operations `CGAlgorithmsDD::intersection` uses.
#[derive(Clone, Copy)]
struct Dd(f64, f64);

impl Dd {
    const SPLIT: f64 = 134_217_729.0;

    fn add(self, yhi: f64, ylo: f64) -> Self {
        let (hi, lo) = (self.0, self.1);
        let s0 = hi + yhi;
        let t0 = lo + ylo;
        let mut e = s0 - hi;
        let f = t0 - lo;
        let mut s = s0 - e;
        let mut t = t0 - f;
        s = (yhi - e) + (hi - s);
        t = (ylo - f) + (lo - t);
        e = s + t0;
        let h0 = s0 + e;
        let h = e + (s0 - h0);
        e = t + h;
        let zhi = h0 + e;
        Dd(zhi, e + (h0 - zhi))
    }

    fn sub(self, o: Self) -> Self {
        self.add(-o.0, -o.1)
    }

    fn add_dd(self, o: Self) -> Self {
        self.add(o.0, o.1)
    }

    fn mul(self, o: Self) -> Self {
        let (hi, lo, yhi, ylo) = (self.0, self.1, o.0, o.1);
        let mut c0 = Self::SPLIT * hi;
        let mut hx = c0 - hi;
        let mut c = Self::SPLIT * yhi;
        hx = c0 - hx;
        let tx = hi - hx;
        let mut hy = c - yhi;
        c0 = hi * yhi;
        hy = c - hy;
        let ty = yhi - hy;
        c = ((((hx * hy - c0) + hx * ty) + tx * hy) + tx * ty) + (hi * ylo + lo * yhi);
        let zhi = c0 + c;
        hx = c0 - zhi;
        Dd(zhi, c + hx)
    }

    fn div(self, o: Self) -> Self {
        let (hi, lo, yhi, ylo) = (self.0, self.1, o.0, o.1);
        let c0 = hi / yhi;
        let c = Self::SPLIT * c0;
        let mut hc = c - c0;
        let mut u = Self::SPLIT * yhi;
        hc = c - hc;
        let tc = c0 - hc;
        let mut hy = u - yhi;
        let uu = c0 * yhi;
        hy = u - hy;
        let ty = yhi - hy;
        u = (((hc * hy - uu) + hc * ty) + tc * hy) + tc * ty;
        let cc = ((((hi - uu) - u) + lo) - c0 * ylo) / yhi;
        let u2 = c0 + cc;
        Dd(u2, (c0 - u2) + cc)
    }

    fn value(self) -> f64 {
        self.0 + self.1
    }
}

/// ln(x) correctly rounded (but for hard cases of ~2⁻⁵⁰ odds): the atanh series in double-double. macOS's
/// `log`, which GEOS's envelope digits use, is correctly rounded beside the powers of ten where those
/// digits turn (2,267 of 2,268 neighbours checked against mpmath); `det::ln` is up to 3 ulp off there.
fn ln_correctly_rounded(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return x;
    }
    // x = m 2^e with m in [√½, √2)
    let (x, bias) = if x < f64::MIN_POSITIVE {
        (x * 18_014_398_509_481_984.0, -54) // 2⁵⁴: subnormals normalized
    } else {
        (x, 0)
    };
    let bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i32 - 1023 + bias;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > std::f64::consts::SQRT_2 {
        m /= 2.0;
        e += 1;
    }
    // ln m = 2 atanh s, s = (m − 1)/(m + 1), |s| ≤ 0.1716
    let s = Dd(m - 1.0, 0.0).div(Dd(m, 0.0).add(1.0, 0.0));
    let s2 = s.mul(s);
    let (mut term, mut sum) = (s, s);
    for k in 1..=24 {
        term = term.mul(s2);
        sum = sum.add_dd(term.div(Dd(f64::from(2 * k + 1), 0.0)));
    }
    let ln2 = Dd(
        f64::from_bits(0x3FE6_2E42_FEFA_39EF),
        f64::from_bits(0x3C7A_BC9E_3B39_803F),
    );
    Dd(f64::from(e), 0.0)
        .mul(ln2)
        .add_dd(sum.add_dd(sum))
        .value()
}

/// `CGAlgorithmsDD::intersection` of the lines `p1 p2` and `q1 q2` (`None` when parallel).
fn intersection_dd(p1: Vec2, p2: Vec2, q1: Vec2, q2: Vec2) -> Option<Vec2> {
    let d = |v: f64| Dd(v, 0.0);
    let px = d(p1.y).sub(d(p2.y));
    let py = d(p2.x).sub(d(p1.x));
    let pw = d(p1.x).mul(d(p2.y)).sub(d(p2.x).mul(d(p1.y)));
    let qx = d(q1.y).sub(d(q2.y));
    let qy = d(q2.x).sub(d(q1.x));
    let qw = d(q1.x).mul(d(q2.y)).sub(d(q2.x).mul(d(q1.y)));
    let x = py.mul(qw).sub(qy.mul(pw));
    let y = qx.mul(pw).sub(px.mul(qw));
    let w = px.mul(qy).sub(qx.mul(py));
    let (xi, yi) = (x.div(w).value(), y.div(w).value());
    (xi.is_finite() && yi.is_finite()).then(|| Vec2::new(xi, yi))
}

fn env_contains(a: Vec2, b: Vec2, p: Vec2) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

fn nearest_endpoint(p1: Vec2, p2: Vec2, q1: Vec2, q2: Vec2) -> Vec2 {
    let mut best = (p1, point_to_segment(p1, q1, q2));
    for (p, d) in [
        (p2, point_to_segment(p2, q1, q2)),
        (q1, point_to_segment(q1, p1, p2)),
        (q2, point_to_segment(q2, p1, p2)),
    ] {
        if d < best.1 {
            best = (p, d);
        }
    }
    best.0
}

/// `LineIntersector::computeIntersection` of two segments: the number of intersections and the first.
fn segment_intersection(p1: Vec2, p2: Vec2, q1: Vec2, q2: Vec2) -> (u8, Vec2) {
    let (n, pts) = segment_intersections(p1, p2, q1, q2);
    (n, pts[0])
}

/// `LineIntersector::computeIntersection` of two segments: the number of intersections (0, 1, or 2 for a
/// collinear overlap) and the intersection points.
pub(crate) fn segment_intersections(p1: Vec2, p2: Vec2, q1: Vec2, q2: Vec2) -> (u8, [Vec2; 2]) {
    let envelopes = p1.x.min(p2.x) <= q1.x.max(q2.x)
        && q1.x.min(q2.x) <= p1.x.max(p2.x)
        && p1.y.min(p2.y) <= q1.y.max(q2.y)
        && q1.y.min(q2.y) <= p1.y.max(p2.y);
    if !envelopes {
        return (0, [p1, p1]);
    }
    let (pq1, pq2) = (orientation(p1, p2, q1), orientation(p1, p2, q2));
    if (pq1 > 0 && pq2 > 0) || (pq1 < 0 && pq2 < 0) {
        return (0, [p1, p1]);
    }
    let (qp1, qp2) = (orientation(q1, q2, p1), orientation(q1, q2, p2));
    if (qp1 > 0 && qp2 > 0) || (qp1 < 0 && qp2 < 0) {
        return (0, [p1, p1]);
    }
    if pq1 == 0 && pq2 == 0 && qp1 == 0 && qp2 == 0 {
        let (q1p, q2p, p1q, p2q) = (
            env_contains(p1, p2, q1),
            env_contains(p1, p2, q2),
            env_contains(q1, q2, p1),
            env_contains(q1, q2, p2),
        );
        return match (q1p, q2p, p1q, p2q) {
            (true, true, _, _) => (2, [q1, q2]),
            (_, _, true, true) => (2, [p1, p2]),
            (true, _, true, _) => (if q1 == p1 && !q2p && !p2q { 1 } else { 2 }, [q1, p1]),
            (true, _, _, true) => (if q1 == p2 && !q2p && !p1q { 1 } else { 2 }, [q1, p2]),
            (_, true, true, _) => (if q2 == p1 && !q1p && !p2q { 1 } else { 2 }, [q2, p1]),
            (_, true, _, true) => (if q2 == p2 && !q1p && !p1q { 1 } else { 2 }, [q2, p2]),
            _ => (0, [p1, p1]),
        };
    }
    if pq1 == 0 || pq2 == 0 || qp1 == 0 || qp2 == 0 {
        let p = if p1 == q1 || p1 == q2 {
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
        };
        return (1, [p, p]);
    }
    let mut x = intersection_dd(p1, p2, q1, q2).unwrap_or_else(|| nearest_endpoint(p1, p2, q1, q2));
    if !(env_contains(p1, p2, x) && env_contains(q1, q2, x)) {
        x = nearest_endpoint(p1, p2, q1, q2);
    }
    (1, [x, x])
}

/// `Intersection::intersectionLineSegment`: the line `l1 l2` with the segment `s1 s2`.
fn intersection_line_segment(l1: Vec2, l2: Vec2, s1: Vec2, s2: Vec2) -> Option<Vec2> {
    let o1 = orientation(l1, l2, s1);
    if o1 == 0 {
        return Some(s1);
    }
    let o2 = orientation(l1, l2, s2);
    if o2 == 0 {
        return Some(s2);
    }
    if (o1 > 0 && o2 > 0) || (o1 < 0 && o2 < 0) {
        return None;
    }
    if let Some(x) = intersection_dd(l1, l2, s1, s2) {
        return Some(x);
    }
    Some(
        if point_to_line_perpendicular(s1, l1, l2) < point_to_line_perpendicular(s2, l1, l2) {
            s1
        } else {
            s2
        },
    )
}

fn sin_cos_snap(a: f64) -> (f64, f64) {
    let (mut s, mut c) = det::sin_cos(a);
    if s.abs() < 5e-16 {
        s = 0.0;
    }
    if c.abs() < 5e-16 {
        c = 0.0;
    }
    (s, c)
}

const PI: f64 = std::f64::consts::PI;

fn normalize(mut a: f64) -> f64 {
    while a > PI {
        a -= 2.0 * PI;
    }
    while a <= -PI {
        a += 2.0 * PI;
    }
    a
}

fn angle(p0: Vec2, p1: Vec2) -> f64 {
    det::atan2(p1.y - p0.y, p1.x - p0.x)
}

/// `BufferInputLineSimplifier::simplify(pts, tol)` (negative `tol`: the right side).
fn simplify_input(pts: &[Vec2], tol: f64) -> Vec<Vec2> {
    let n = pts.len();
    let want = if tol < 0.0 { -1 } else { 1 };
    let tol = tol.abs();
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
        if orientation(p0, p1, p2) != want || point_to_segment(p1, p0, p2) >= tol {
            return false;
        }
        // isShallowSampled(p0, p1, i0, i2): GEOS passes the middle vertex as the far end
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
    let mut out: Vec<Vec2> = Vec::with_capacity(n);
    for (p, d) in pts.iter().zip(&deleted) {
        if !*d && out.last() != Some(p) {
            out.push(*p);
        }
    }
    out
}

/// `OffsetSegmentGenerator` (one side, a ring).
struct SegGen {
    d: f64,
    join: Join,
    left: bool,
    fillet_quantum: f64,
    closing_factor: f64,
    pts: Vec<Vec2>,
    s0: Vec2,
    s1: Vec2,
    s2: Vec2,
    off0: (Vec2, Vec2),
    off1: (Vec2, Vec2),
    /// The working precision model (none: floating).
    pm: Option<Fixed>,
}

impl SegGen {
    fn new(d: f64, join: Join, left: bool, pm: Option<Fixed>) -> Self {
        // shapely's quad_segs (16 unless given): round joins, and a point's circle under any join
        let quad = match join {
            Join::Round(q) => q.max(1),
            Join::Mitre(_) => 16,
        };
        let closing_factor = if matches!(join, Join::Round(q) if q >= 8) {
            80.0
        } else {
            1.0
        };
        Self {
            d,
            join,
            left,
            fillet_quantum: (PI / 2.0) / f64::from(quad),
            closing_factor,
            pts: Vec::new(),
            s0: Vec2::new(0.0, 0.0),
            s1: Vec2::new(0.0, 0.0),
            s2: Vec2::new(0.0, 0.0),
            off0: (Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0)),
            off1: (Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0)),
            pm,
        }
    }

    /// `OffsetSegmentString::addPt`: made precise, then dropped if it nearly repeats the last point.
    fn add(&mut self, p: Vec2) {
        let p = self
            .pm
            .map_or(p, |pm| Vec2::new(pm.precise(p.x), pm.precise(p.y)));
        if let Some(&last) = self.pts.last()
            && dist(p, last) < self.d * 1.0e-4
        {
            return;
        }
        self.pts.push(p);
    }

    fn offset(&self, a: Vec2, b: Vec2) -> (Vec2, Vec2) {
        let sign = if self.left { 1.0 } else { -1.0 };
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        let ux = sign * self.d * dx / len;
        let uy = sign * self.d * dy / len;
        (Vec2::new(a.x - uy, a.y + ux), Vec2::new(b.x - uy, b.y + ux))
    }

    fn init(&mut self, s1: Vec2, s2: Vec2) {
        self.s1 = s1;
        self.s2 = s2;
        self.off1 = self.offset(s1, s2);
    }

    fn fillet_angles(&mut self, p: Vec2, start: f64, end: f64, clockwise: bool) {
        let factor = if clockwise { -1.0 } else { 1.0 };
        let total = (start - end).abs();
        let n = (total / self.fillet_quantum + 0.5) as i64;
        if n < 1 {
            return;
        }
        let inc = total / n as f64;
        for i in 0..n {
            let (s, c) = sin_cos_snap(start + factor * i as f64 * inc);
            self.add(Vec2::new(p.x + self.d * c, p.y + self.d * s));
        }
    }

    fn fillet(&mut self, p: Vec2, p0: Vec2, p1: Vec2, clockwise: bool) {
        let mut start = det::atan2(p0.y - p.y, p0.x - p.x);
        let end = det::atan2(p1.y - p.y, p1.x - p.x);
        if clockwise {
            if start <= end {
                start += 2.0 * PI;
            }
        } else if start >= end {
            start -= 2.0 * PI;
        }
        self.add(p0);
        self.fillet_angles(p, start, end, clockwise);
        self.add(p1);
    }

    fn next(&mut self, p: Vec2, add_start: bool) {
        if self.s2 == p {
            return;
        }
        (self.s0, self.s1, self.s2) = (self.s1, self.s2, p);
        self.off0 = self.offset(self.s0, self.s1);
        self.off1 = self.offset(self.s1, self.s2);
        let o = orientation(self.s0, self.s1, self.s2);
        let outside = (o == -1 && self.left) || (o == 1 && !self.left);
        if o == 0 {
            let (n, _) = segment_intersection(self.s0, self.s1, self.s1, self.s2);
            if n >= 2 {
                match self.join {
                    Join::Mitre(_) => {
                        if add_start {
                            self.add(self.off0.1);
                        }
                        self.add(self.off1.0);
                    }
                    Join::Round(_) => {
                        let (s1, a, b) = (self.s1, self.off0.1, self.off1.0);
                        self.fillet(s1, a, b, true);
                    }
                }
            }
        } else if outside {
            self.outside_turn(o, add_start);
        } else {
            self.inside_turn();
        }
    }

    fn outside_turn(&mut self, o: i32, add_start: bool) {
        let (o0, o1) = (self.off0, self.off1);
        if dist(o0.1, o1.0) < self.d * 1.0e-3 {
            self.add(o0.1);
            return;
        }
        match self.join {
            Join::Mitre(limit) => self.mitre(limit),
            Join::Round(_) => {
                if add_start {
                    self.add(o0.1);
                }
                let s1 = self.s1;
                self.fillet(s1, o0.1, o1.0, o == -1);
                self.add(o1.0);
            }
        }
    }

    fn inside_turn(&mut self) {
        let (o0, o1, s1) = (self.off0, self.off1, self.s1);
        let (n, x) = segment_intersection(o0.0, o0.1, o1.0, o1.1);
        if n > 0 {
            self.add(x);
            return;
        }
        if dist(o0.1, o1.0) < self.d * 1.0e-3 {
            self.add(o0.1);
        } else {
            let f = self.closing_factor;
            self.add(o0.1);
            self.add(Vec2::new(
                (f * o0.1.x + s1.x) / (f + 1.0),
                (f * o0.1.y + s1.y) / (f + 1.0),
            ));
            self.add(Vec2::new(
                (f * o1.0.x + s1.x) / (f + 1.0),
                (f * o1.0.y + s1.y) / (f + 1.0),
            ));
            self.add(o1.0);
        }
    }

    fn mitre(&mut self, limit: f64) {
        let (o0, o1, corner) = (self.off0, self.off1, self.s1);
        let limit_d = limit * self.d;
        if let Some(x) = intersection_dd(o0.0, o0.1, o1.0, o1.1)
            && dist(x, corner) <= limit_d
        {
            self.add(x);
            return;
        }
        if point_to_segment(corner, o0.1, o1.0) >= limit_d {
            self.add(o0.1);
            self.add(o1.0);
            return;
        }
        // addLimitedMitreJoin
        let ang_interior = {
            let a1 = angle(corner, self.s0);
            let a2 = angle(corner, self.s2);
            let del = a2 - a1;
            if del <= -PI {
                del + 2.0 * PI
            } else if del > PI {
                del - 2.0 * PI
            } else {
                del
            }
        };
        let dir0 = angle(corner, self.s0);
        let bis = normalize(dir0 + ang_interior / 2.0);
        let bis_out = normalize(bis + PI);
        let project = |p: Vec2, d: f64, dir: f64| {
            let (s, c) = sin_cos_snap(dir);
            Vec2::new(p.x + d * c, p.y + d * s)
        };
        let mid = project(corner, limit_d, bis_out);
        let dir_bevel = normalize(bis_out + PI / 2.0);
        let b0 = project(mid, self.d, dir_bevel);
        let b1 = project(mid, self.d, dir_bevel + PI);
        match (
            intersection_line_segment(o0.0, o0.1, b0, b1),
            intersection_line_segment(o1.0, o1.1, b0, b1),
        ) {
            (Some(i0), Some(i1)) => {
                self.add(i0);
                self.add(i1);
            }
            _ => {
                self.add(o0.1);
                self.add(o1.0);
            }
        }
    }
}

/// GEOS `Area::ofRingSigned` (positive for clockwise) of a closed ring.
pub(crate) fn ring_signed_area(c: &[Vec2]) -> f64 {
    let n = c.len();
    if n < 3 {
        return 0.0;
    }
    let x0 = c[0].x;
    let mut p1 = c[0];
    let mut p2 = Vec2::new(c[1].x - x0, c[1].y);
    let mut sum = 0.0;
    for &q in &c[2..] {
        let p0y = p1.y;
        p1 = p2;
        p2 = Vec2::new(q.x - x0, q.y);
        sum += p1.x * (p0y - p2.y);
    }
    sum / 2.0
}

/// GEOS's raw ring offset curve (closed) of a polygon's shell at `distance` (negative: inward).
fn raw_ring_curve(shell: &[Vec2], distance: f64, join: Join, pm: Option<Fixed>) -> Vec<Vec2> {
    // the shell as a closed sequence without repeated or non-finite points
    let mut coords: Vec<Vec2> = Vec::with_capacity(shell.len() + 1);
    for &p in shell.iter().chain(shell.first()) {
        if p.is_finite() && coords.last() != Some(&p) {
            coords.push(p);
        }
    }
    if let Some(&f) = coords.first()
        && coords.last() != Some(&f)
    {
        coords.push(f);
    }
    let d = distance.abs();
    let mut left = distance >= 0.0;
    if coords.len() >= 4 && ring_signed_area(&coords) < 0.0 {
        left = !left; // counter-clockwise ring: the other side
    }
    let tol = d / 100.0;
    let simp = simplify_input(&coords, if left { tol } else { -tol });
    // fewer than two distinct points has no ring curve (GEOS would buffer it as a line or a point)
    if simp.len() < 3 {
        return Vec::new();
    }
    let n = simp.len() - 1;
    let mut g = SegGen::new(d, join, left, pm);
    g.init(simp[n - 1], simp[0]);
    for (i, &p) in simp.iter().enumerate().skip(1) {
        g.next(p, i != 1);
    }
    let mut pts = g.pts;
    if pts.first() != pts.last()
        && let Some(&f) = pts.first()
    {
        pts.push(f);
    }
    pts
}

fn closed(ring: &[Vec2]) -> Vec<Vec2> {
    let mut c = ring.to_vec();
    if c.first() != c.last()
        && let Some(&f) = c.first()
    {
        c.push(f);
    }
    c
}

/// `BufferCurveSetBuilder::isTriangleErodedCompletely`: the in-centre is nearer an edge than the distance.
fn triangle_eroded(t: &[Vec2], distance: f64) -> bool {
    let (p0, p1, p2) = (t[0], t[1], t[2]);
    let (l0, l1, l2) = (dist(p1, p2), dist(p0, p2), dist(p0, p1));
    let c = l0 + l1 + l2;
    let centre = Vec2::new(
        (l0 * p0.x + l1 * p1.x + l2 * p2.x) / c,
        (l0 * p0.y + l1 * p1.y + l2 * p2.y) / c,
    );
    point_to_segment(centre, p0, p1) < distance.abs()
}

/// `BufferCurveSetBuilder::isRingFullyEroded` (the ring closed, as given).
fn ring_fully_eroded(ring: &[Vec2], hole: bool, distance: f64) -> bool {
    if ring.len() < 4 {
        return true;
    }
    if ring.len() == 4 {
        return triangle_eroded(ring, distance);
    }
    if (hole && distance > 0.0) || (!hole && distance < 0.0) {
        let (lo_x, hi_x, lo_y, hi_y) = ring.iter().fold(
            (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ),
            |e, p| (e.0.min(p.x), e.1.max(p.x), e.2.min(p.y), e.3.max(p.y)),
        );
        return 2.0 * distance.abs() > (hi_x - lo_x).min(hi_y - lo_y);
    }
    false
}

/// `BufferCurveSetBuilder::isRingCurveInverted`: a small ring's curve with no point at the distance.
fn ring_curve_inverted(ring: &[Vec2], distance: f64, curve: &[Vec2]) -> bool {
    if distance == 0.0 || ring.len() <= 3 || ring.len() >= 9 || curve.len() > 4 * ring.len() {
        return false;
    }
    let tol = 0.99 * distance.abs();
    let far = |p: Vec2| {
        ring.windows(2)
            .map(|w| point_to_segment(p, w[0], w[1]))
            .fold(dist(p, ring[0]), f64::min)
            > tol
    };
    !(0..curve.len()).any(|i| {
        let (v, w) = (curve[i], curve[(i + 1) % curve.len()]);
        far(v) || far(Vec2::new((v.x + w.x) / 2.0, (v.y + w.y) / 2.0))
    })
}

/// `BufferCurveSetBuilder::addPolygonRingSide`: a ring's raw curve and its depth delta. A clockwise
/// shell has the polygon on its right (delta −1), a hole on its left; counter-clockwise rings swap.
fn ring_curve(
    ring: &[Vec2],
    distance: f64,
    hole: bool,
    join: Join,
    pm: Option<Fixed>,
) -> Option<Curve> {
    // GEOS 3.14's `LinearRing::MINIMUM_VALID_SIZE` is 3
    if distance == 0.0 && ring.len() < 3 {
        return None;
    }
    let ccw = ring.len() >= 3 && ring_signed_area(ring) < 0.0;
    let delta = if hole == ccw { -1 } else { 1 };
    let pts = if distance == 0.0 {
        ring.to_vec()
    } else if ring.len() == 1 {
        // `getRingCurve` → `getLineCurve` → `computePointCurve`: a round cap is the point's circle
        // (`createCircle`, clockwise from due east), whichever side and sign
        let mut g = SegGen::new(distance.abs(), join, true, pm);
        let p = ring[0];
        g.add(Vec2::new(p.x + distance.abs(), p.y));
        g.fillet_angles(p, 0.0, 2.0 * PI, true);
        let mut pts = g.pts;
        if let Some(&f) = pts.first()
            && pts.last() != Some(&f)
        {
            pts.push(f); // `closeRing`
        }
        pts
    } else if ring.len() == 2 {
        return None; // an unclosed two-point ring cannot come from a closed shell or hole
    } else {
        // a hole is offset to the other side
        raw_ring_curve(ring, if hole { -distance } else { distance }, join, pm)
    };
    (pts.len() >= 2 && !ring_curve_inverted(ring, distance, &pts)).then_some(Curve { pts, delta })
}

/// shapely `MultiPolygon(polygons).buffer(distance, join_style=…)` (GEOS 3.14.1 `BufferOp`; shells and
/// holes of any orientation): the result polygons, rings closed, in GEOS's order with GEOS's starts.
/// `distance` 0 is the classic clean-up: spikes and zero-width necks fall away, a pinched shape splits.
/// Where the topology fails at full precision, GEOS's retry (`BufferOp::bufferReducedPrecision`): the curves
/// made precise at 12 significant digits of the buffer's envelope, then 11, down to 6, and snap-rounded.
///
/// # Errors
/// A non-finite shell, or a topology GEOS refuses at every precision.
pub fn buffer(polygons: &[Polygon], distance: f64, join: Join) -> Result<Vec<Polygon>, String> {
    if !distance.is_finite() {
        return Err("buffer: distance must be a finite value".into());
    }
    let full = raw_curves(polygons, distance, join, None)?;
    if full.is_empty() {
        return Ok(Vec::new());
    }
    let mut err = match buffer_graph::polygons(&full) {
        Ok(result) => return Ok(result),
        Err(e) => e,
    };
    for digits in (6..=12).rev() {
        let pm = Fixed::new(precision_scale_factor(polygons, distance, digits));
        let curves = raw_curves(polygons, distance, join, Some(pm))?;
        if curves.is_empty() {
            return Ok(Vec::new());
        }
        match buffer_graph::polygons_snap_rounded(&curves, pm.scale) {
            Ok(result) => return Ok(result),
            Err(e) => err = e,
        }
    }
    Err(err)
}

/// `BufferOp::precisionScaleFactor`: the power of ten that keeps `digits` significant digits over the
/// input's envelope grown by twice a positive distance.
fn precision_scale_factor(polygons: &[Polygon], distance: f64, digits: i32) -> f64 {
    let env_max = polygons
        .iter()
        .flat_map(|p| p.shell.iter().chain(p.holes.iter().flatten()))
        .filter(|p| p.is_finite())
        .fold(0.0_f64, |m, p| m.max(p.x.abs()).max(p.y.abs()));
    let buf_env_max = env_max + 2.0 * if distance > 0.0 { distance } else { 0.0 };
    // the smallest power of ten above the envelope (`std::log(10.0)` is LN_10); `std::pow(10.0, n)` is
    // exact or correctly rounded
    let env_digits = (ln_correctly_rounded(buf_env_max) / std::f64::consts::LN_10 + 1.0) as i32;
    format!("1e{}", digits.saturating_sub(env_digits))
        .parse()
        .unwrap_or(1.0)
}

/// `BufferCurveSetBuilder::getCurves`: every ring's raw curve and depth delta, the points made precise by
/// the working precision model.
fn raw_curves(
    polygons: &[Polygon],
    distance: f64,
    join: Join,
    pm: Option<Fixed>,
) -> Result<Vec<Curve>, String> {
    let mut curves = Vec::new();
    for poly in polygons {
        if poly.shell.is_empty() {
            continue; // an empty geometry adds no curve
        }
        let shell = closed(&poly.shell);
        if distance < 0.0 && ring_fully_eroded(&shell, false, distance) {
            continue;
        }
        let coords = without_repeats(&shell);
        if coords.is_empty() {
            return Err("buffer: a shell with no finite points".into());
        }
        if distance <= 0.0 && coords.len() < 3 {
            continue;
        }
        curves.extend(ring_curve(&coords, distance, false, join, pm));
        for hole in &poly.holes {
            let hole = closed(hole);
            if distance > 0.0 && ring_fully_eroded(&hole, true, distance) {
                continue;
            }
            curves.extend(ring_curve(
                &without_repeats(&hole),
                distance,
                true,
                join,
                pm,
            ));
        }
    }
    Ok(curves)
}

/// `RepeatedPointRemover::removeRepeatedAndInvalidPoints` (`RepeatedInvalidPointFilter`, tolerance 0): a
/// non-finite point, or one whose squared distance to the last kept point is not positive (equal, or so
/// close the square underflows), is dropped.
fn without_repeats(ring: &[Vec2]) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(ring.len());
    for &p in ring {
        if !p.is_finite() {
            continue;
        }
        if let Some(&q) = out.last() {
            let (dx, dy) = (p.x - q.x, p.y - q.y);
            if p == q || dx * dx + dy * dy <= 0.0 {
                continue;
            }
        }
        out.push(p);
    }
    out
}

/// shapely `LineString(line).distance(Point(p))`.
#[must_use]
pub fn line_distance(line: &[Vec2], p: Vec2) -> f64 {
    line.windows(2)
        .map(|w| point_to_segment(p, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

/// shapely `LineString(line).project(Point(p), normalized=True)` (GEOS `LengthIndexedLine::project`).
#[must_use]
pub fn line_project_normalized(line: &[Vec2], p: Vec2) -> f64 {
    let (mut best, mut measure, mut start) = (f64::INFINITY, 0.0, 0.0);
    let mut total = 0.0;
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = dist(a, b);
        let d = point_to_segment(p, a, b);
        if d < best {
            best = d;
            let f = if p == a {
                0.0
            } else if p == b {
                1.0
            } else if a == b {
                0.0
            } else {
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                ((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy)
            };
            measure = if f <= 0.0 {
                start
            } else if f <= 1.0 {
                start + f * len
            } else {
                start + len
            };
        }
        start += len;
        total += len;
    }
    if total == 0.0 { 0.0 } else { measure / total }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The retry's scale: GEOS's envelope digits `(int)(log(x)/log(10) + 1)` as macOS's libm gives them at
    /// 10^k and three ulps either side (k = −8 … 14), and `ln` bits against mpmath at 200 bits.
    #[test]
    fn retry_scale_digits_match_geos_beside_powers_of_ten() {
        const DIGITS: [[i32; 7]; 23] = [
            [-7, -7, -7, -7, -6, -6, -6],
            [-6, -6, -6, -6, -6, -6, -6],
            [-5, -5, -4, -4, -4, -4, -4],
            [-4, -4, -4, -4, -4, -4, -4],
            [-3, -3, -3, -2, -2, -2, -2],
            [-2, -2, -1, -1, -1, -1, -1],
            [-1, -1, -1, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0],
            [0, 0, 1, 1, 1, 1, 1],
            [1, 1, 1, 2, 2, 2, 2],
            [2, 2, 2, 3, 3, 3, 3],
            [3, 3, 3, 3, 3, 4, 4],
            [4, 4, 4, 5, 5, 5, 5],
            [6, 6, 6, 6, 6, 6, 6],
            [6, 6, 6, 6, 6, 6, 6],
            [8, 8, 8, 8, 8, 8, 8],
            [8, 8, 8, 9, 9, 9, 9],
            [9, 9, 9, 9, 9, 9, 9],
            [11, 11, 11, 11, 11, 11, 11],
            [11, 11, 12, 12, 12, 12, 12],
            [12, 12, 12, 12, 12, 12, 12],
            [13, 13, 13, 13, 13, 13, 13],
            [15, 15, 15, 15, 15, 15, 15],
        ];
        for (row, k) in DIGITS.iter().zip(-8..=14) {
            let p: f64 = format!("1e{k}").parse().unwrap();
            for (&want, j) in row.iter().zip(-3_i64..=3) {
                let x = f64::from_bits(p.to_bits().wrapping_add_signed(j));
                let got = (ln_correctly_rounded(x) / std::f64::consts::LN_10 + 1.0) as i32;
                assert_eq!(got, want, "10^{k} {j:+} ulp");
            }
        }
        const LN: [(u64, u64); 30] = [
            (0x4092_9752_b1a6_e261, 0x401c_5386_215b_48b3),
            (0x40a5_424d_267a_52bb, 0x401f_a29f_1e71_30cb),
            (0x409c_e720_e785_cadc, 0x401e_175e_f35a_827f),
            (0x40a7_9735_53a4_f2bf, 0x4020_0698_578a_4c30),
            (0x40a8_7135_e513_7dbc, 0x4020_18c0_5bdc_b76f),
            (0x4074_7a75_502b_dcf5, 0x4017_2af6_cbd0_d266),
            (0x4050_7663_8cfc_be39, 0x4010_bfdf_b355_17ef),
            (0x40b0_5b58_d756_6ef2, 0x4020_adfd_3be4_0859),
            (0x4094_431c_2338_119e, 0x401c_aba9_3119_8c37),
            (0x4092_4ea6_5c6b_277e, 0x401c_43c4_689a_9f5e),
            (0x40b3_7239_668d_e494, 0x4021_0691_75d8_d34a),
            (0x40a2_5ea5_4aa0_49db, 0x401f_0d09_f924_1877),
            (0x4023_ffff_ffff_ffff, 0x4002_6bb1_bbb5_5515),
            (0x4024_0000_0000_0000, 0x4002_6bb1_bbb5_5516),
            (0x4024_0000_0000_0001, 0x4002_6bb1_bbb5_5516),
            (0x4058_ffff_ffff_ffff, 0x4012_6bb1_bbb5_5515),
            (0x4059_0000_0000_0000, 0x4012_6bb1_bbb5_5516),
            (0x4059_0000_0000_0001, 0x4012_6bb1_bbb5_5516),
            (0x408f_3fff_ffff_ffff, 0x401b_a18a_998f_ffa0),
            (0x408f_4000_0000_0000, 0x401b_a18a_998f_ffa0),
            (0x408f_4000_0000_0001, 0x401b_a18a_998f_ffa0),
            (0x40c3_87ff_ffff_ffff, 0x4022_6bb1_bbb5_5515),
            (0x40c3_8800_0000_0000, 0x4022_6bb1_bbb5_5516),
            (0x40c3_8800_0000_0001, 0x4022_6bb1_bbb5_5516),
            (0x0000_0000_0000_0001, 0xc087_4385_446d_71c3),
            (0x0010_0000_0000_0000, 0xc086_232b_dd7a_bcd2),
            (0x3ff0_0000_0000_0000, 0x0000_0000_0000_0000),
            (0x4000_0000_0000_0000, 0x3fe6_2e42_fefa_39ef),
            (0x3fe0_0000_0000_0000, 0xbfe6_2e42_fefa_39ef),
            (0x7fef_ffff_ffff_ffff, 0x4086_2e42_fefa_39ef),
        ];
        for (x, want) in LN {
            let got = ln_correctly_rounded(f64::from_bits(x));
            assert_eq!(got.to_bits(), want, "ln({:e})", f64::from_bits(x));
        }
    }

    /// A single point or a doubled segment (a one-pixel blob traced with no opening) never panics.
    #[test]
    fn degenerate_rings_do_not_panic() {
        let (a, b) = (Vec2::new(3.0, 4.0), Vec2::new(5.0, 4.0));
        for ring in [vec![a], vec![a, a], vec![a, b, a], vec![a, b, a, b, a]] {
            for d in [-1.0, 0.0, 0.5, 2.0] {
                for join in [Join::Mitre(4.0), Join::Round(16)] {
                    assert!(buffer(&[Polygon::new(ring.clone())], d, join).is_ok());
                }
            }
        }
    }

    /// GEOS refuses a non-finite distance; an empty shell adds nothing.
    #[test]
    fn non_finite_distances_refuse_and_empty_shells_vanish() {
        let sq = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ];
        for d in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(buffer(&[Polygon::new(sq.clone())], d, Join::Mitre(4.0)).is_err());
        }
        assert_eq!(
            buffer(&[Polygon::new(Vec::new())], 1.0, Join::Round(16)),
            Ok(Vec::new())
        );
    }
}
