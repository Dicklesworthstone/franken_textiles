//! Shared full-polygon clearance for roll compaction and finite hide/remnant stock.
//! Search can be approximate; acceptance checks every edge of the emitted paths.

use ftx_core::{
    Vec2,
    exact::{Sign, circle_diameter_margin, orient2d},
};

pub(crate) struct Work {
    pub used: usize,
    pub limit: usize,
    pub exhausted: bool,
    pub candidates: usize,
}
impl Work {
    pub(crate) fn new(limit: usize) -> Self {
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
pub(crate) fn clear(
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
    clear_paths(a, b, gap, work)
}
// The same exact boundary/containment acceptance without a caller-supplied broad-phase box.
pub(crate) fn clear_paths(a: &[Vec2], b: &[Vec2], gap: f64, work: &mut Work) -> Option<bool> {
    if !boundaries_clear(a, b, gap, work)? {
        return Some(false);
    }
    Some(!inside(a[0], b, work)? && !inside(b[0], a, work)?)
}
// Certify strict separation without a rounded projected point or square root. Outward-rounded
// intervals enclose every operation on the authored f64 coordinates; an inconclusive comparison
// refuses clearance rather than adding an arbitrary physical tolerance. Endpoint distances use
// the core exact circle predicate. This stricter comparison belongs only to the tool sweep.
fn sweep_point_clear(p: Vec2, a: Vec2, b: Vec2, radius: f64) -> bool {
    type Bounds = [f64; 2];
    fn sub(a: Bounds, b: Bounds) -> Bounds {
        [(a[0] - b[1]).next_down(), (a[1] - b[0]).next_up()]
    }
    fn add(a: Bounds, b: Bounds) -> Bounds {
        [(a[0] + b[0]).next_down(), (a[1] + b[1]).next_up()]
    }
    fn mul(a: Bounds, b: Bounds) -> Bounds {
        let products = [a[0] * b[0], a[0] * b[1], a[1] * b[0], a[1] * b[1]];
        if products.iter().any(|v| !v.is_finite()) {
            return [f64::NEG_INFINITY, f64::INFINITY];
        }
        [
            products
                .into_iter()
                .fold(f64::INFINITY, f64::min)
                .next_down(),
            products
                .into_iter()
                .fold(f64::NEG_INFINITY, f64::max)
                .next_up(),
        ]
    }
    fn square(a: Bounds) -> Bounds {
        if a.iter().any(|v| !v.is_finite()) {
            return [0.0, f64::INFINITY];
        }
        let nearest = if a[0] > 0.0 {
            a[0]
        } else if a[1] < 0.0 {
            a[1]
        } else {
            0.0
        };
        let farthest = a[0].abs().max(a[1].abs());
        [
            (nearest * nearest).next_down().max(0.0),
            (farthest * farthest).next_up(),
        ]
    }
    let difference = |u: Vec2, v: Vec2| [sub([u.x; 2], [v.x; 2]), sub([u.y; 2], [v.y; 2])];
    let dot = |u: [Bounds; 2], v: [Bounds; 2]| add(mul(u[0], v[0]), mul(u[1], v[1]));
    let edge = difference(b, a);
    let delta = difference(p, a);
    let endpoint = if a == b || dot(delta, edge)[1] <= 0.0 {
        Some(a)
    } else if dot(difference(p, b), edge)[0] >= 0.0 {
        Some(b)
    } else {
        None
    };
    if let Some(endpoint) = endpoint {
        return circle_diameter_margin([p.x, p.y], [endpoint.x, endpoint.y], radius * 0.5)
            .is_some_and(|(sign, _)| sign == Sign::Negative);
    }
    // The infinite line also gives a conservative lower bound if the projection's interval
    // straddles an endpoint. Only a proved exterior projection uses an endpoint distance.
    let cross2 = square(sub(mul(delta[0], edge[1]), mul(delta[1], edge[0])));
    let length2 = add(square(edge[0]), square(edge[1]));
    let limit2 = mul(square([radius; 2]), length2);
    cross2[0].is_finite() && limit2[1].is_finite() && cross2[0] > limit2[1]
}
// Full circular tool sweep against a filled fixture, including stationary heads. Touching the
// envelope is not clearance. Checking endpoints alone would miss crossings between safe endpoints.
pub(super) fn swept_disk_clear(
    a: Vec2,
    b: Vec2,
    radius: f64,
    fixture: &[Vec2],
    work: &mut Work,
) -> Option<bool> {
    if inside(a, fixture, work)? || inside(b, fixture, work)? {
        return Some(false);
    }
    for (c, d) in edges(fixture) {
        work.tick()?;
        if intersects(a, b, c, d) {
            return Some(false);
        }
        for (p, u, v) in [(a, c, d), (b, c, d), (c, a, b), (d, a, b)] {
            if !sweep_point_clear(p, u, v, radius) {
                return Some(false);
            }
        }
    }
    Some(true)
}
// No edge may cross a concavity or touch the stock boundary. Vertices-inside alone is insufficient.
// Zero requested margin still forbids contact; search anchors include a numerical guard, not a clamp.
pub(crate) fn enclosed(
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
pub(crate) fn simple(ring: &[Vec2], work: &mut Work) -> Option<bool> {
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
    fn swept_disk_refuses_exact_oblique_tangency_after_scaling_and_translation() {
        // |(42,56)| = 70 and cross((42,56),(23,39)) = 350: the closest corner is
        // exactly 5 mm from the segment, with an interior projection. Rounded projection
        // previously returned 5.000000000000003 and incorrectly admitted this fixture.
        let fixture = poly(&[[22., 39.], [23., 39.], [23., 40.], [22., 40.]]);
        for (scale, offset) in [
            (1.0 / 1024.0, Vec2::new(0., 0.)),
            (1.0, Vec2::new(1.0 / 1024.0, -1.0 / 2048.0)),
            (1.0, Vec2::new(524288., -524288.)),
            (1024.0, Vec2::new(-262144., 262144.)),
        ] {
            let transform = |p: Vec2| p * scale + offset;
            let obstacle: Vec<_> = fixture.iter().copied().map(transform).collect();
            let a = transform(Vec2::new(0., 0.));
            let b = transform(Vec2::new(42., 56.));
            for (start, end) in [(a, b), (b, a)] {
                for (radius, clear) in [
                    (5.0, false),
                    (5.0 - 1.0 / 16777216.0, true),
                    (5.0 + 1.0 / 16777216.0, false),
                ] {
                    assert_eq!(
                        swept_disk_clear(
                            start,
                            end,
                            radius * scale,
                            &obstacle,
                            &mut Work::new(1000)
                        ),
                        Some(clear),
                        "scale={scale}, offset={offset:?}, radius={radius}"
                    );
                }
            }
        }
    }
    #[test]
    fn swept_disk_endpoint_and_stationary_clearance_use_the_finite_segment() {
        for (a, b, fixture) in [
            (
                Vec2::new(0., 0.),
                Vec2::new(0., 0.),
                poly(&[[3., 4.], [4., 4.], [4., 5.], [3., 5.]]),
            ),
            (
                Vec2::new(0., 0.),
                Vec2::new(4., 0.),
                poly(&[[7., 4.], [8., 4.], [8., 5.], [7., 5.]]),
            ),
        ] {
            // The 3-4-5 corner is nearest the stationary head or the segment's end.
            // In the latter case the infinite line is only 4 mm away and is insufficient.
            for (start, end) in [(a, b), (b, a)] {
                assert_eq!(
                    swept_disk_clear(start, end, 5.0, &fixture, &mut Work::new(1000)),
                    Some(false)
                );
                assert_eq!(
                    swept_disk_clear(
                        start,
                        end,
                        5.0 - 1.0 / 16777216.0,
                        &fixture,
                        &mut Work::new(1000)
                    ),
                    Some(true)
                );
            }
        }
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
