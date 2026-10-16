//! Shared full-polygon clearance for roll compaction and finite hide/remnant stock.
//! Search can be approximate; acceptance checks every edge of the emitted paths.

use ftx_core::{
    Vec2,
    exact::{Sign, orient2d},
};

pub(super) struct Work {
    pub used: usize,
    pub limit: usize,
    pub exhausted: bool,
    pub candidates: usize,
}
impl Work {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            used: 0,
            limit,
            exhausted: false,
            candidates: 0,
        }
    }
    pub(super) fn tick(&mut self) -> Option<()> {
        self.spend(1)
    }
    pub(super) fn spend(&mut self, count: usize) -> Option<()> {
        if count > self.limit - self.used {
            self.used = self.limit;
            self.exhausted = true;
            return None;
        }
        self.used += count;
        Some(())
    }
}
fn orient(a: Vec2, b: Vec2, c: Vec2) -> Sign {
    orient2d([a.x, a.y], [b.x, b.y], [c.x, c.y])
}
fn between(p: Vec2, a: Vec2, b: Vec2) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}
fn intersects(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let (u, v, w, z) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if (u == Sign::Zero && between(c, a, b))
        || (v == Sign::Zero && between(d, a, b))
        || (w == Sign::Zero && between(a, c, d))
        || (z == Sign::Zero && between(b, c, d))
    {
        return true;
    }
    u != Sign::Zero && v != Sign::Zero && w != Sign::Zero && z != Sign::Zero && u != v && w != z
}
fn point_distance(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let edge = b - a;
    let length2 = edge.norm2();
    if length2 == 0.0 {
        return (p - a).norm();
    }
    if !length2.is_finite() {
        return f64::NAN;
    }
    let t = ((p - a).dot(edge) / length2).clamp(0.0, 1.0);
    (p - (a + edge * t)).norm()
}
fn edges(p: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    p.iter()
        .copied()
        .zip(p.iter().copied().cycle().skip(1))
        .take(p.len())
}
fn inside(p: Vec2, ring: &[Vec2], work: &mut Work) -> Option<bool> {
    let mut winding = 0_i32;
    for (a, b) in edges(ring) {
        work.tick()?;
        let side = orient(a, b, p);
        if side == Sign::Zero && between(p, a, b) {
            return Some(true);
        }
        if a.y <= p.y && b.y > p.y && side == Sign::Positive {
            winding += 1;
        }
        if a.y > p.y && b.y <= p.y && side == Sign::Negative {
            winding -= 1;
        }
    }
    Some(winding != 0)
}
fn separated_boxes(a: [f64; 4], b: [f64; 4], gap: f64) -> bool {
    a[2] + gap <= b[0] || b[2] + gap <= a[0] || a[3] + gap <= b[1] || b[3] + gap <= a[1]
}
fn boundaries_clear(a: &[Vec2], b: &[Vec2], gap: f64, work: &mut Work) -> Option<bool> {
    for (a0, a1) in edges(a) {
        for (b0, b1) in edges(b) {
            work.tick()?;
            if intersects(a0, a1, b0, b1) {
                return Some(false);
            }
            if gap > 0.0 {
                for d in [
                    point_distance(a0, b0, b1),
                    point_distance(a1, b0, b1),
                    point_distance(b0, a0, a1),
                    point_distance(b1, a0, a1),
                ] {
                    if !d.is_finite() || d < gap {
                        return Some(false);
                    }
                }
            }
        }
    }
    Some(true)
}
// Preserve the roll kernel's conservative overlapping-box boundary-contact semantics, including gap=0.
pub(super) fn clear(
    a: &[Vec2],
    ab: [f64; 4],
    b: &[Vec2],
    bb: [f64; 4],
    gap: f64,
    work: &mut Work,
) -> Option<bool> {
    work.tick()?;
    if separated_boxes(ab, bb, gap) {
        return Some(true);
    }
    if !boundaries_clear(a, b, gap, work)? {
        return Some(false);
    }
    Some(!inside(a[0], b, work)? && !inside(b[0], a, work)?)
}
// No edge may cross a concavity or touch the stock boundary. Vertices-inside alone is insufficient.
// Zero requested margin still forbids contact; search anchors include a numerical guard, not a clamp.
pub(super) fn enclosed(
    inner: &[Vec2],
    outer: &[Vec2],
    margin: f64,
    work: &mut Work,
) -> Option<bool> {
    if !inside(inner[0], outer, work)? {
        return Some(false);
    }
    boundaries_clear(inner, outer, margin, work)
}
// Budgeted simplicity check for authored stock. Consecutive collinear edges may continue, not retrace.
pub(super) fn simple(ring: &[Vec2], work: &mut Work) -> Option<bool> {
    for (i, (a, b)) in edges(ring).enumerate() {
        work.tick()?;
        if a == b {
            return Some(false);
        }
        let c = ring[(i + 2) % ring.len()];
        if orient(a, b, c) == Sign::Zero && (b - a).dot(c - b) <= 0.0 {
            return Some(false);
        }
        for (j, (c, d)) in edges(ring).enumerate().skip(i + 1) {
            work.tick()?;
            if j == i + 1 || (i == 0 && j + 1 == ring.len()) {
                continue;
            }
            if intersects(a, b, c, d) {
                return Some(false);
            }
        }
    }
    Some(true)
}

pub(super) fn path_inside(
    path: &[Vec2],
    closed: bool,
    outer: &[Vec2],
    work: &mut Work,
) -> Option<bool> {
    if path.len() < 2 || path.iter().any(|p| !p.is_finite()) {
        return Some(false);
    }
    if !inside(path[0], outer, work)? {
        return Some(false);
    }
    let n = if closed { path.len() } else { path.len() - 1 };
    for (a, b) in edges(path).take(n) {
        for (c, d) in edges(outer) {
            work.tick()?;
            if intersects(a, b, c, d) {
                return Some(false);
            }
        }
    }
    Some(true)
}

pub(super) fn disk_inside(p: Vec2, radius: f64, outer: &[Vec2], work: &mut Work) -> Option<bool> {
    if !p.is_finite() || !radius.is_finite() || radius <= 0.0 || !inside(p, outer, work)? {
        return Some(false);
    }
    for (a, b) in edges(outer) {
        work.tick()?;
        let d = point_distance(p, a, b);
        if !d.is_finite() || d <= radius {
            return Some(false);
        }
    }
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn poly(p: &[[f64; 2]]) -> Vec<Vec2> {
        p.iter().map(|p| Vec2::new(p[0], p[1])).collect()
    }
    #[test]
    fn concave_stock_does_not_admit_edges_that_leave_stock_between_inside_vertices() {
        let outer = poly(&[
            [0., 0.],
            [100., 0.],
            [100., 100.],
            [70., 100.],
            [70., 40.],
            [30., 40.],
            [30., 100.],
            [0., 100.],
        ]);
        let inner = poly(&[[10., 60.], [90., 60.], [90., 80.], [10., 80.]]);
        let mut w = Work::new(10000);
        for p in &inner {
            assert_eq!(inside(*p, &outer, &mut w), Some(true));
        }
        assert_eq!(enclosed(&inner, &outer, 0., &mut w), Some(false));
        assert_eq!(path_inside(&inner[..2], false, &outer, &mut w), Some(false));
        assert_eq!(enclosed(&inner, &outer, 0., &mut Work::new(1)), None);
    }
    #[test]
    fn stock_simple_and_containment_checks_preserve_euclidean_margin_and_defect_discs() {
        let tri = poly(&[[0., 0.], [10., 0.], [0., 10.]]);
        let inner = poly(&[[1., 1.], [4., 1.], [4., 4.], [1., 4.]]);
        let mut w = Work::new(10000);
        assert_eq!(simple(&tri, &mut w), Some(true));
        assert_eq!(enclosed(&inner, &tri, 0.99, &mut w), Some(true));
        assert_eq!(enclosed(&inner, &tri, 1.01, &mut w), Some(false));
        assert_eq!(
            disk_inside(Vec2::new(4., 4.), 1.42, &tri, &mut w),
            Some(false)
        );
        assert_eq!(
            disk_inside(Vec2::new(4., 4.), 1.40, &tri, &mut w),
            Some(true)
        );
        let retraced = poly(&[[0., 0.], [10., 0.], [5., 0.], [10., 10.], [0., 10.]]);
        assert_eq!(simple(&retraced, &mut w), Some(false));
    }
}
