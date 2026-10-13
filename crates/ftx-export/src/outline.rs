//! Edge flattening and the seam-allowance cut outline (`docs/spec/03` §3–4).

use ftx_core::curve::{CurveKind, curve_points, rel_to_abs};
use ftx_core::polyline::signed_area;
use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_pattern::{InstEdge, Instance, Piece, Side};

use crate::union::union_exterior;

/// The default seam allowance (mm) when neither the edge, the piece nor the pattern gives one.
pub const DEFAULT_SA: f64 = 10.0;

/// How a cut outline was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Direct offset of each edge with mitred corners.
    Mitre,
    /// Union of single-sided edge buffers, corner wedges and the sew polygon.
    Union,
}

impl Method {
    /// `"mitre"` or `"union"`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mitre => "mitre",
            Self::Union => "union",
        }
    }
}

/// One edge of a loop: its id, its flattened points and its seam allowance.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeLoop {
    /// Edge id (as in the instance).
    pub id: String,
    /// Flattened points, endpoints included.
    pub pts: Vec<Vec2>,
    /// Seam allowance (mm).
    pub sa: f64,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// The piece as drafted: id = piece id, side C, not mirrored, the drafted vertices and edges.
#[must_use]
pub fn drafted_instance(piece: &Piece, piece_index: usize) -> Instance {
    Instance {
        id: piece.id.clone(),
        piece: piece.id.clone(),
        piece_index,
        side: Side::C,
        mirrored: false,
        verts: piece.vertices.clone(),
        edges: piece
            .edges
            .iter()
            .enumerate()
            .map(|(i, e)| InstEdge {
                id: e.id.clone(),
                from: e.from.clone(),
                to: e.to.clone(),
                curve: e.curve.clone(),
                source: i,
            })
            .collect(),
    }
}

/// `edge.sa` → `piece.sa` → `sa_default`.
#[must_use]
pub fn edge_sa(piece: &Piece, source: usize, sa_default: f64) -> f64 {
    piece
        .edges
        .get(source)
        .and_then(|e| e.sa)
        .or(piece.sa)
        .unwrap_or(sa_default)
}

/// Bézier sample count: `clip(ceil(control polygon length / 1 mm), 24, 800)`; 2 for lines and polylines.
fn dense_n(p0: Vec2, p1: Vec2, e: &InstEdge) -> Result<usize, Refusal> {
    let Some(curve) = &e.curve else {
        return Ok(2);
    };
    let kind = curve.kind().map_err(|x| invalid(x.to_string()))?;
    if matches!(kind, CurveKind::Line | CurveKind::Poly) {
        return Ok(2);
    }
    let mut ctrl = vec![p0];
    ctrl.extend(curve.controls().iter().map(|&th| rel_to_abs(p0, p1, th)));
    ctrl.push(p1);
    let est: f64 = ctrl
        .windows(2)
        .map(|w| {
            let d = w[1] - w[0];
            (d.x * d.x + d.y * d.y).sqrt()
        })
        .sum();
    Ok(((est / 1.0).ceil() as usize).clamp(24, 800))
}

/// Consecutive points closer than 1e-7 dropped; if all collapse, the first and last kept.
fn dedupe(pts: &[Vec2]) -> Vec<Vec2> {
    let mut keep = vec![pts[0]];
    for &p in &pts[1..] {
        let d = p - keep[keep.len() - 1];
        if (d.x * d.x + d.y * d.y).sqrt() > 1e-7 {
            keep.push(p);
        }
    }
    if keep.len() == 1 {
        keep.push(pts[pts.len() - 1]);
    }
    keep
}

/// The flattened points of an instance edge.
///
/// # Errors
/// A missing vertex or a malformed curve.
pub fn edge_points(inst: &Instance, e: &InstEdge) -> Result<Vec<Vec2>, Refusal> {
    let get = |v: &str| {
        inst.verts
            .get(v)
            .ok_or_else(|| invalid(format!("{}: no vertex {v}", inst.id)))
    };
    let (p0, p1) = (get(&e.from)?, get(&e.to)?);
    let n = dense_n(p0, p1, e)?;
    let pts = curve_points(p0, p1, e.curve.as_ref(), n).map_err(|x| invalid(x.to_string()))?;
    Ok(dedupe(&pts))
}

/// The edge loop of an instance: per edge its flattened points and seam allowance (`fold`, the drafted
/// fold edge of a piece shown as a half, gets 0).
///
/// # Errors
/// As [`edge_points`].
pub fn edge_loops(
    inst: &Instance,
    piece: &Piece,
    fold: Option<&str>,
    sa_default: f64,
) -> Result<Vec<EdgeLoop>, Refusal> {
    inst.edges
        .iter()
        .map(|e| {
            let sa = if fold == Some(e.id.as_str()) {
                0.0
            } else {
                edge_sa(piece, e.source, sa_default)
            };
            Ok(EdgeLoop {
                id: e.id.clone(),
                pts: edge_points(inst, e)?,
                sa,
            })
        })
        .collect()
}

/// The sew ring: each edge's points without its last (an open counter-clockwise ring).
#[must_use]
pub fn sew_ring(loops: &[EdgeLoop]) -> Vec<Vec2> {
    loops
        .iter()
        .flat_map(|l| l.pts[..l.pts.len() - 1].iter().copied())
        .collect()
}

/// Index in the sew ring where each edge starts.
#[must_use]
pub fn sew_breaks(loops: &[EdgeLoop]) -> Vec<usize> {
    let mut out = Vec::with_capacity(loops.len());
    let mut k = 0;
    for l in loops {
        out.push(k);
        k += l.pts.len() - 1;
    }
    out
}

fn norm(v: Vec2) -> f64 {
    (v.x * v.x + v.y * v.y).sqrt()
}

fn unit(v: Vec2) -> Vec2 {
    let n = norm(v);
    if n > 0.0 {
        Vec2::new(v.x / n, v.y / n)
    } else {
        v
    }
}

/// Per segment: unit tangent and outward normal (right of travel).
pub(crate) fn seg_frames(pts: &[Vec2]) -> Vec<(Vec2, Vec2)> {
    pts.windows(2)
        .map(|w| {
            let d = w[1] - w[0];
            let n = norm(d);
            let t = Vec2::new(d.x / n, d.y / n);
            (t, Vec2::new(t.y, -t.x))
        })
        .collect()
}

/// `np.linalg.solve` of `[[a, b], [c, d]] x = r` as LAPACK does it: partial pivoting on the first column,
/// the multiplier as a product with the pivot's reciprocal.
fn solve2(m: [[f64; 2]; 2], r: Vec2) -> Option<(f64, f64)> {
    let (mut m, mut r) = (m, [r.x, r.y]);
    if m[1][0].abs() > m[0][0].abs() {
        m.swap(0, 1);
        r.swap(0, 1);
    }
    if m[0][0] == 0.0 {
        return None;
    }
    let l = m[1][0] * (1.0 / m[0][0]);
    let u11 = m[1][1] - l * m[0][1];
    if u11 == 0.0 {
        return None;
    }
    let y1 = r[1] - l * r[0];
    let x1 = y1 / u11;
    let x0 = (r[0] - x1 * m[0][1]) / m[0][0];
    Some((x0, x1))
}

/// Outer corner points between two offset edges at a convex corner: the mitre point, or two points
/// clipping it square to the bisector at `limit × max(sa)` (`None`: straight, tangent or reflex).
#[allow(clippy::too_many_arguments)]
pub(crate) fn corner(
    v: Vec2,
    t_in: Vec2,
    n_in: Vec2,
    sa_i: f64,
    t_out: Vec2,
    n_out: Vec2,
    sa_j: f64,
    limit: f64,
) -> Option<Vec<Vec2>> {
    let cr = t_in.x * t_out.y - t_in.y * t_out.x;
    if cr <= 1e-9 {
        return None;
    }
    let a1 = v + n_in * sa_i;
    let a2 = v + n_out * sa_j;
    let (s, _) = solve2([[t_in.x, -t_out.x], [t_in.y, -t_out.y]], a2 - a1)?;
    let mp = a1 + t_in * s;
    let big = sa_i.max(sa_j);
    if big <= 0.0 {
        return None;
    }
    let lim = limit * big;
    if norm(mp - v) <= lim {
        return Some(vec![mp]);
    }
    let b = unit(mp - v);
    Some(
        [(a1, t_in), (a2, t_out)]
            .into_iter()
            .map(|(a, t)| {
                let tb = t.x * b.x + t.y * b.y;
                let ab = a - v;
                let k = if tb.abs() > 1e-12 {
                    (lim - (ab.x * b.x + ab.y * b.y)) / tb
                } else {
                    0.0
                };
                a + t * k
            })
            .collect(),
    )
}

fn direct_offset(loops: &[EdgeLoop], limit: f64) -> Option<Vec<Vec2>> {
    let mut ring = Vec::new();
    let n = loops.len();
    for (i, l) in loops.iter().enumerate() {
        let fr = seg_frames(&l.pts);
        let (sa, pts) = (l.sa, &l.pts);
        let mut off = pts.clone();
        off[0] = pts[0] + fr[0].1 * sa;
        let last = pts.len() - 1;
        off[last] = pts[last] + fr[fr.len() - 1].1 * sa;
        for k in 1..last {
            let (na, nb) = (fr[k - 1].1, fr[k].1);
            let den = 1.0 + (na.x * nb.x + na.y * nb.y);
            if den < 0.2 {
                return None;
            }
            let s = na + nb;
            off[k] = pts[k] + Vec2::new(s.x / den, s.y / den) * sa;
        }
        ring.extend(off);
        let next = &loops[(i + 1) % n];
        let fr2 = seg_frames(&next.pts);
        let (t, nrm) = fr[fr.len() - 1];
        if let Some(c) = corner(pts[last], t, nrm, sa, fr2[0].0, fr2[0].1, next.sa, limit) {
            ring.extend(c);
        }
    }
    Some(ring)
}

/// Whether segments `p1p2` and `q1q2` share any point (touching counts).
fn segments_touch(p1: Vec2, p2: Vec2, q1: Vec2, q2: Vec2) -> bool {
    let orient = |a: Vec2, b: Vec2, c: Vec2| {
        let v = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if v > 0.0 {
            1
        } else if v < 0.0 {
            -1
        } else {
            0
        }
    };
    let on = |a: Vec2, b: Vec2, c: Vec2| {
        c.x >= a.x.min(b.x) && c.x <= a.x.max(b.x) && c.y >= a.y.min(b.y) && c.y <= a.y.max(b.y)
    };
    let (d1, d2) = (orient(q1, q2, p1), orient(q1, q2, p2));
    let (d3, d4) = (orient(p1, p2, q1), orient(p1, p2, q2));
    if d1 * d2 < 0 && d3 * d4 < 0 {
        return true;
    }
    (d1 == 0 && on(q1, q2, p1))
        || (d2 == 0 && on(q1, q2, p2))
        || (d3 == 0 && on(p1, p2, q1))
        || (d4 == 0 && on(p1, p2, q2))
}

/// Whether the ring is a valid simple polygon shell (repeated consecutive points allowed): at least three
/// distinct points, no two non-adjacent segments touching, no adjacent segments folding back.
#[must_use]
pub fn ring_is_simple(ring: &[Vec2]) -> bool {
    let mut pts: Vec<Vec2> = Vec::with_capacity(ring.len());
    for &p in ring {
        if pts.last() != Some(&p) {
            pts.push(p);
        }
    }
    while pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    let n = pts.len();
    if n < 3 {
        return false;
    }
    // sweep segments in order of their least x
    let seg = |i: usize| (pts[i], pts[(i + 1) % n]);
    let mut order: Vec<usize> = (0..n).collect();
    let lo = |i: usize| {
        let (a, b) = seg(i);
        a.x.min(b.x)
    };
    order.sort_by(|&a, &b| lo(a).total_cmp(&lo(b)));
    for (k, &i) in order.iter().enumerate() {
        let (a, b) = seg(i);
        let (xmax, ylo, yhi) = (a.x.max(b.x), a.y.min(b.y), a.y.max(b.y));
        for &j in &order[k + 1..] {
            if lo(j) > xmax {
                break;
            }
            let (c, d) = seg(j);
            if c.y.max(d.y) < ylo || c.y.min(d.y) > yhi {
                continue;
            }
            let adjacent = (i + 1) % n == j || (j + 1) % n == i;
            if adjacent {
                // they share one end; invalid if the other end of either lies on the other segment
                let (far_i, far_j) = if (i + 1) % n == j { (a, d) } else { (b, c) };
                if segments_touch(a, b, far_j, far_j) || segments_touch(c, d, far_i, far_i) {
                    return false;
                }
            } else if segments_touch(a, b, c, d) {
                return false;
            }
        }
    }
    true
}

/// Duplicate (≤ 1e-4, cyclic) and straight-through collinear vertices dropped.
#[must_use]
pub fn clean_ring(ring: &[Vec2]) -> Vec<Vec2> {
    let n = ring.len();
    let mut pts: Vec<Vec2> = (0..n)
        .filter(|&i| norm(ring[i] - ring[(i + n - 1) % n]) > 1e-4)
        .map(|i| ring[i])
        .collect();
    let mut changed = true;
    while changed && pts.len() > 3 {
        changed = false;
        let n = pts.len();
        let mut keep = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (ab, bc) = (b - a, c - b);
            let cr = ab.x * bc.y - ab.y * bc.x;
            if cr.abs() <= 1e-9 * 1f64.max(norm(ab) * norm(bc)) && ab.x * bc.x + ab.y * bc.y > 0.0 {
                changed = true;
                continue;
            }
            keep.push(b);
        }
        pts = keep;
    }
    pts
}

/// The cut outline (open counter-clockwise ring, mm) of an instance and how it was made: the direct offset
/// when it is a valid polygon at least as large as the sew ring, else the union of single-sided edge
/// buffers, corner wedges and the sew polygon.
///
/// # Errors
/// As [`edge_loops`]; a loop whose union has no area.
pub fn cut_outline(
    inst: &Instance,
    piece: &Piece,
    fold: Option<&str>,
    sa_default: f64,
    mitre_limit: f64,
) -> Result<(Vec<Vec2>, Method), Refusal> {
    let loops = edge_loops(inst, piece, fold, sa_default)?;
    let sew_area = signed_area(&sew_ring(&loops));
    let direct = direct_offset(&loops, mitre_limit)
        .filter(|r| r.len() >= 3 && ring_is_simple(r) && signed_area(r) >= sew_area - 1e-6);
    let (ring, method) = match direct {
        Some(r) => (r, Method::Mitre),
        None => (union_offset(&loops, mitre_limit)?, Method::Union),
    };
    let mut ring = clean_ring(&ring);
    if signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    Ok((ring, method))
}

/// Mitre limit of the single-sided edge buffers (GEOS `mitre_limit = 10`).
const BUFFER_MITRE_LIMIT: f64 = 10.0;

/// The right-side single-sided buffer of a polyline at distance `sa`, as GEOS builds it: the polyline and,
/// back along it, its offset, joined at a joint turning left by the mitre (limited at 10 × `sa`) and at one
/// turning right by the crossing of the two offset segments, or where they do not cross (a curve tighter than
/// `sa`) by a closing segment through the midpoints towards the vertex. The ring may overlap itself; the
/// union counts its area by depth, as GEOS does.
fn single_sided(pts: &[Vec2], sa: f64) -> Vec<Vec2> {
    let fr = seg_frames(pts);
    let m = pts.len() - 1;
    let off = |k: usize, end: usize| pts[end] + fr[k].1 * sa;
    let mut offset = vec![off(0, 0)];
    for k in 1..m {
        let ((t_in, n_in), (t_out, n_out)) = (fr[k - 1], fr[k]);
        let (o0a, o0b, o1a, o1b) = (off(k - 1, k - 1), off(k - 1, k), off(k, k), off(k, k + 1));
        if cross(t_in, t_out) > 0.0 {
            match corner(pts[k], t_in, n_in, sa, t_out, n_out, sa, BUFFER_MITRE_LIMIT) {
                Some(c) => offset.extend(c),
                None => offset.extend([o0b, o1a]),
            }
        } else if let Some(x) = segment_crossing(o0a, o0b, o1a, o1b) {
            offset.push(x);
        } else {
            let mid = |q: Vec2| Vec2::new(0.5 * (q.x + pts[k].x), 0.5 * (q.y + pts[k].y));
            offset.extend([o0b, mid(o0b), mid(o1a), o1a]);
        }
    }
    offset.push(off(m - 1, m));
    let mut ring = pts.to_vec();
    ring.extend(offset.iter().rev());
    ring
}

fn cross(u: Vec2, v: Vec2) -> f64 {
    u.x * v.y - u.y * v.x
}

/// Where segments `ab` and `cd` cross (touching included), if they do.
fn segment_crossing(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<Vec2> {
    let (r, s) = (b - a, d - c);
    let den = cross(r, s);
    if den == 0.0 {
        return None;
    }
    let t = cross(c - a, s) / den;
    let u = cross(c - a, r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| a + r * t)
}

fn union_offset(loops: &[EdgeLoop], limit: f64) -> Result<Vec<Vec2>, Refusal> {
    let mut polys = vec![sew_ring(loops)];
    let n = loops.len();
    for (i, l) in loops.iter().enumerate() {
        if l.sa > 0.0 {
            polys.push(single_sided(&l.pts, l.sa));
        }
        let next = &loops[(i + 1) % n];
        let fr = seg_frames(&l.pts);
        let fr2 = seg_frames(&next.pts);
        let v = l.pts[l.pts.len() - 1];
        let (t, nrm) = fr[fr.len() - 1];
        if let Some(c) = corner(v, t, nrm, l.sa, fr2[0].0, fr2[0].1, next.sa, limit) {
            let mut w = vec![v, v + nrm * l.sa];
            w.extend(c);
            w.push(v + fr2[0].1 * next.sa);
            polys.push(w);
        }
    }
    union_exterior(&polys).ok_or_else(|| invalid("the seam allowance outline has no area"))
}
