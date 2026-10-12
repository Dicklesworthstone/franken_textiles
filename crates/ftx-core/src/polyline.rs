//! Polylines and polygons (`docs/spec/01` §5): lengths, arclength fractions, end tangents, signed
//! area, point-in-polygon and the width of a horizontal line inside a polygon.
//!
//! Evaluation orders follow the reference's numpy calls where numpy's order is fixed: segment
//! lengths are `sqrt(dx² + dy²)`, totals are numpy's pairwise sum, running sums are sequential.

use crate::linalg::lstsq;
use crate::np;
use crate::vec::Vec2;

/// Which end of a polyline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// The first point; tangents point along the direction of travel.
    Start,
    /// The last point; tangents still point along the direction of travel.
    End,
}

fn segment_lengths(pts: &[Vec2]) -> Vec<f64> {
    pts.windows(2).map(|w| (w[1] - w[0]).norm()).collect()
}

/// Total length, `Σ ‖P[i+1] − P[i]‖` (numpy pairwise sum). One point or none → 0.
#[must_use]
pub fn polyline_length(pts: &[Vec2]) -> f64 {
    np::pairwise_sum(&segment_lengths(pts))
}

/// Arclength at each point: `[0, s₁, …]`, same length as `pts` (empty for no points).
#[must_use]
pub fn cumulative(pts: &[Vec2]) -> Vec<f64> {
    if pts.is_empty() {
        return Vec::new();
    }
    let mut s = Vec::with_capacity(pts.len());
    s.push(0.0);
    s.extend(np::cumsum(&segment_lengths(pts)));
    s
}

/// Points at arclength fractions along the polyline. Fractions clamp to [0, 1]; at a repeated
/// arclength (a zero-length segment) the last of the repeated points wins; a zero-length polyline
/// returns its last point.
///
/// # Panics
/// If `pts` is empty.
#[must_use]
pub fn at_fractions(pts: &[Vec2], fractions: &[f64]) -> Vec<Vec2> {
    assert!(!pts.is_empty(), "at_fractions: empty polyline");
    let s = cumulative(pts);
    let total = s[s.len() - 1];
    let xs: Vec<f64> = pts.iter().map(|p| p.x).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.y).collect();
    fractions
        .iter()
        .map(|&f| {
            let u = f.clamp(0.0, 1.0) * total;
            Vec2::new(np::interp(u, &s, &xs), np::interp(u, &s, &ys))
        })
        .collect()
}

/// Unit tangent (direction of travel) at one end, from a least-squares quadratic fit of `x(s)` and
/// `y(s)` over the samples with arclength in `[min(2, 0.1·L), min(14, 0.5·L)]` mm, extrapolated back
/// to the vertex. With fewer than 4 samples in that window the first `max(4, r)` samples are used
/// (`r` = samples within the upper bound); with fewer than 3 samples the first segment's direction.
/// A zero tangent is returned unscaled; fewer than 2 points give zero.
#[must_use]
pub fn tangent_at(pts: &[Vec2], end: End) -> Vec2 {
    if pts.len() < 2 {
        return Vec2::ZERO;
    }
    let p: Vec<Vec2> = match end {
        End::Start => pts.to_vec(),
        End::End => pts.iter().rev().copied().collect(),
    };
    let mut d = p[1] - p[0];
    if p.len() >= 3 {
        let s = cumulative(&p);
        let total = s[s.len() - 1];
        let (lo, hi) = (2.0f64.min(0.1 * total), 14.0f64.min(0.5 * total));
        let mut idx: Vec<usize> = (0..p.len()).filter(|&i| s[i] >= lo && s[i] <= hi).collect();
        if idx.len() < 4 {
            let r = s.partition_point(|&v| v <= hi);
            idx = (0..p.len().min(r.max(4))).collect();
        }
        if idx.len() >= 3 {
            let a: Vec<[f64; 3]> = idx.iter().map(|&i| [1.0, s[i], s[i] * s[i]]).collect();
            let bx: Vec<f64> = idx.iter().map(|&i| p[i].x).collect();
            let by: Vec<f64> = idx.iter().map(|&i| p[i].y).collect();
            d = Vec2::new(lstsq(&a, &bx)[1], lstsq(&a, &by)[1]);
        }
    }
    if end == End::End {
        d = -d;
    }
    let n = d.norm();
    if n == 0.0 { d } else { d / n }
}

/// Shoelace area of the closed ring (last point joins the first); counter-clockwise is positive.
#[must_use]
pub fn signed_area(poly: &[Vec2]) -> f64 {
    let n = poly.len();
    let (mut a, mut b) = (0.0, 0.0);
    for i in 0..n {
        let q = poly[(i + 1) % n];
        a += poly[i].x * q.y;
        b += poly[i].y * q.x;
    }
    0.5 * (a - b)
}

/// Even-odd point-in-polygon test (ring wraps): an edge crosses when exactly one end is above the
/// point's y, at `x_c = x₀ + (y − y₀)(x₁ − x₀)/(y₁ − y₀)`; crossings with `x < x_c` (strict) count.
#[must_use]
pub fn point_in_polygon(pt: Vec2, poly: &[Vec2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > pt.y) != (b.y > pt.y) {
            let xc = a.x + (pt.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if pt.x < xc {
                inside = !inside;
            }
        }
    }
    inside
}

/// Total length of the line `Y = y` inside the closed polygon: crossings of edges with exactly one
/// end above `y` (half-open: a vertex on the line counts as below, horizontal edges never count),
/// sorted, summed in pairs; an odd trailing crossing is ignored.
#[must_use]
pub fn horizontal_width(poly: &[Vec2], y: f64) -> f64 {
    let n = poly.len();
    let mut xs = Vec::new();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > y) != (b.y > y) {
            xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.as_chunks::<2>()
        .0
        .iter()
        .map(|[a, b]| b - a)
        .fold(0.0, |acc, w| acc + w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::{Curve, CurveKind, EDGE_DENSE, curve_points};

    fn pts(c: &[(f64, f64)]) -> Vec<Vec2> {
        c.iter().map(|&(x, y)| Vec2::new(x, y)).collect()
    }

    fn close(a: Vec2, b: (f64, f64), tol: f64) -> bool {
        (a.x - b.0).abs() <= tol && (a.y - b.1).abs() <= tol
    }

    /// Spec 01 §14.2 F4.
    #[test]
    fn f4_at_fractions() {
        let p = pts(&[(0.0, 0.0), (10.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        let got = at_fractions(&p, &[0.0, 0.25, 0.5, 0.75, 1.0, 1.5, -1.0]);
        assert_eq!(
            got,
            pts(&[
                (0.0, 0.0),
                (5.0, 0.0),
                (10.0, 0.0),
                (10.0, 5.0),
                (10.0, 10.0),
                (10.0, 10.0),
                (0.0, 0.0)
            ])
        );
        let p = pts(&[(3.0, 4.0), (3.0, 4.0)]);
        assert_eq!(
            at_fractions(&p, &[0.0, 0.5, 1.0]),
            pts(&[(3.0, 4.0), (3.0, 4.0), (3.0, 4.0)])
        );
        assert_eq!(cumulative(&p), vec![0.0, 0.0]);
        assert_eq!(polyline_length(&p[..1]), 0.0);
    }

    /// Spec 01 §14.2 F5.
    #[test]
    fn f5_tangent_at() {
        let p = pts(&[(0.0, 0.0), (1.0, 0.0)]);
        assert_eq!(tangent_at(&p, End::Start), Vec2::new(1.0, 0.0));
        let e = tangent_at(&p, End::End);
        assert_eq!((e.x, e.y.to_bits()), (1.0, (-0.0f64).to_bits()));

        let quad = Curve::new(CurveKind::Quad, vec![[0.5, 0.5]]);
        let q = curve_points(
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 0.0),
            Some(&quad),
            EDGE_DENSE,
        )
        .unwrap();
        assert!(close(
            tangent_at(&q, End::Start),
            (0.70379761, 0.71040054),
            5e-9
        ));
        assert!(close(
            tangent_at(&q, End::End),
            (0.70379761, -0.71040054),
            5e-9
        ));

        let p = pts(&[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        assert!(close(
            tangent_at(&p, End::Start),
            (0.95799406, -0.28678803),
            5e-9
        ));
        assert!(close(
            tangent_at(&p, End::End),
            (-0.27056336, 0.96270217),
            5e-9
        ));

        let p = pts(&[(0.0, 0.0), (1.0, 0.0), (2.0, 1.0)]);
        assert!(close(
            tangent_at(&p, End::Start),
            (0.96753822, -0.25272473),
            5e-9
        ));
        assert!(close(
            tangent_at(&p, End::End),
            (0.43096441, 0.90236893),
            5e-9
        ));
    }

    /// Spec 01 §14.2 F6.
    #[test]
    fn f6_area_width_inside() {
        let sq = pts(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        assert_eq!(signed_area(&sq), 100.0);
        let rev: Vec<Vec2> = sq.iter().rev().copied().collect();
        assert_eq!(signed_area(&rev), -100.0);
        assert_eq!(signed_area(&pts(&[(0.0, 0.0), (1.0, 0.0)])), 0.0);
        let widths: Vec<f64> = [5.0, 0.0, 10.0, -1.0]
            .iter()
            .map(|&y| horizontal_width(&sq, y))
            .collect();
        assert_eq!(widths, vec![10.0, 10.0, 0.0, 0.0]);
        let u = pts(&[
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 30.0),
            (20.0, 30.0),
            (20.0, 10.0),
            (10.0, 10.0),
            (10.0, 30.0),
            (0.0, 30.0),
        ]);
        assert_eq!(
            (horizontal_width(&u, 20.0), horizontal_width(&u, 10.0)),
            (20.0, 20.0)
        );
        let tri = pts(&[(0.0, 0.0), (10.0, 0.0), (5.0, 10.0)]);
        assert_eq!(
            (horizontal_width(&tri, 0.0), horizontal_width(&tri, 5.0)),
            (10.0, 5.0)
        );
        let inside: Vec<bool> = [
            (5.0, 5.0),
            (15.0, 5.0),
            (10.0, 0.0),
            (0.0, 5.0),
            (10.0, 10.0),
        ]
        .iter()
        .map(|&(x, y)| point_in_polygon(Vec2::new(x, y), &sq))
        .collect();
        assert_eq!(inside, vec![true, false, false, true, false]);
    }
}
