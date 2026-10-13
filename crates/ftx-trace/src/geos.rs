//! The two GEOS 3.14.1 operations the reference reaches through shapely, ported line for line from GEOS's
//! sources: `LineString.simplify(tol, preserve_topology=False)` (`DouglasPeuckerLineSimplifier`) and
//! `Polygon.hausdorff_distance` (`DiscreteHausdorffDistance` with `DistanceToPoint`).

use ftx_core::Vec2;

/// `Coordinate::distance`.
fn dist(a: Vec2, b: Vec2) -> f64 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    (dx * dx + dy * dy).sqrt()
}

/// `Coordinate::distanceSquared`.
fn dist2(a: Vec2, b: Vec2) -> f64 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    dx * dx + dy * dy
}

/// `Distance::pointToSegment`.
fn point_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    if a == b {
        return dist(p, a);
    }
    let r = ((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y))
        / ((b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y));
    if r <= 0.0 {
        return dist(p, a);
    }
    if r >= 1.0 {
        return dist(p, b);
    }
    let s = ((a.y - p.y) * (b.x - a.x) - (a.x - p.x) * (b.y - a.y))
        / ((b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y));
    s.abs() * ((b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y)).sqrt()
}

/// Douglas–Peucker on an open line (GEOS keeps both ends of a line string): the kept points in order.
#[must_use]
pub fn simplify(pts: &[Vec2], tol: f64) -> Vec<Vec2> {
    fn section(pts: &[Vec2], keep: &mut [bool], i: usize, j: usize, tol: f64) {
        if i + 1 == j {
            return;
        }
        let mut max_distance = -1.0;
        let mut max_index = i;
        for (k, &p) in pts.iter().enumerate().take(j).skip(i + 1) {
            let d = point_to_segment(p, pts[i], pts[j]);
            if d > max_distance {
                max_distance = d;
                max_index = k;
            }
        }
        if max_distance <= tol {
            keep[i + 1..j].iter_mut().for_each(|k| *k = false);
        } else {
            section(pts, keep, i, max_index, tol);
            section(pts, keep, max_index, j, tol);
        }
    }
    if pts.is_empty() {
        return Vec::new();
    }
    let mut keep = vec![true; pts.len()];
    section(pts, &mut keep, 0, pts.len() - 1, tol);
    pts.iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(p, _)| *p)
        .collect()
}

/// `LineSegment::closestPoint` of the segment `p0 → p1`.
fn closest_point(p0: Vec2, p1: Vec2, p: Vec2) -> Vec2 {
    // projectionFactor
    let factor = if p == p0 {
        0.0
    } else if p == p1 {
        1.0
    } else if p0 == p1 {
        0.0
    } else {
        let (dx, dy) = (p1.x - p0.x, p1.y - p0.y);
        let len2 = dx * dx + dy * dy;
        ((p.x - p0.x) * dx + (p.y - p0.y) * dy) / len2
    };
    if factor > 0.0 && factor < 1.0 {
        return Vec2::new(p0.x + factor * (p1.x - p0.x), p0.y + factor * (p1.y - p0.y));
    }
    if dist(p0, p) < dist(p1, p) { p0 } else { p1 }
}

/// Squared distance from `p` to a closed ring, as `DistanceToPoint::computeDistance(LineString)` walks it:
/// segment `i` has its ends swapped on alternate steps (`segPts[i % 2]`), which can change the last bit.
fn ring_dist2(ring: &[Vec2], p: Vec2) -> f64 {
    let mut seg = [ring[0], ring[0]];
    let mut best = f64::NAN;
    for (i, &q) in ring.iter().enumerate().skip(1) {
        seg[i % 2] = q;
        let c = closest_point(seg[0], seg[1], p);
        let d = dist2(c, p);
        if best.is_nan() || d < best {
            best = d;
        }
    }
    best
}

/// The discrete Hausdorff distance between two polygons given as open rings (shapely closes them): the
/// largest distance from a vertex of either to the other's boundary.
#[must_use]
pub fn hausdorff(a: &[Vec2], b: &[Vec2]) -> f64 {
    let close = |r: &[Vec2]| -> Vec<Vec2> {
        let mut v = r.to_vec();
        if v.first() != v.last() {
            v.push(v[0]);
        }
        v
    };
    let (ra, rb) = (close(a), close(b));
    let mut best = f64::NAN;
    for (from, to) in [(&ra, &rb), (&rb, &ra)] {
        for &p in from.iter() {
            let d = ring_dist2(to, p);
            if best.is_nan() || d > best {
                best = d;
            }
        }
    }
    best.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_keeps_the_corner_and_drops_the_wobble() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.01),
            Vec2::new(2.0, 0.0),
            Vec2::new(2.0, 2.0),
        ];
        assert_eq!(
            simplify(&pts, 0.1),
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(2.0, 0.0),
                Vec2::new(2.0, 2.0)
            ]
        );
    }

    #[test]
    fn hausdorff_of_offset_squares() {
        let sq = |o: f64| {
            vec![
                Vec2::new(o, 0.0),
                Vec2::new(o + 1.0, 0.0),
                Vec2::new(o + 1.0, 1.0),
                Vec2::new(o, 1.0),
            ]
        };
        assert!((hausdorff(&sq(0.0), &sq(0.25)) - 0.25).abs() < 1e-15);
    }
}
