//! Point–triangle and edge–edge distances for contact, with exact derivatives (plan §11.5).
//!
//! A pair is first classified in plain f64 by its closest features (vertex, edge or face; Ericson's
//! Voronoi regions and clamped segment parameters), then the squared distance of that feature pair is
//! evaluated on [`Jet`]s over the pair's 12 coordinates: point–point `|p − q|²`, point–edge
//! `|(e₀ − p) × (e₁ − p)|² / |e₁ − e₀|²`, point–plane `((p − t₀)·n)² / |n|²`, line–line
//! `((a₀ − b₀)·n)² / |n|²`. These are the ipc-toolkit distance types; differentiating them with jets
//! replaces hand-derived Hessians.

use ftx_core::Vec3;

use crate::ad::{Dual, JVec, Jet, Scalar};

/// Which features of a pair are closest. Points are numbered as in the pair: for point–triangle,
/// 0 is the point and 1, 2, 3 the triangle; for edge–edge, 0–1 edge a and 2–3 edge b.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closest {
    /// Two points.
    PointPoint(usize, usize),
    /// A point and the edge (i, j).
    PointEdge(usize, usize, usize),
    /// The point 0 and the plane of 1, 2, 3.
    PointPlane,
    /// The lines through 0–1 and 2–3.
    LineLine,
}

/// Closest point on triangle (a, b, c) to p as barycentric weights, and its region.
#[must_use]
pub fn point_triangle_closest(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> ([f64; 3], Closest) {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return ([1.0, 0.0, 0.0], Closest::PointPoint(0, 1));
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return ([0.0, 1.0, 0.0], Closest::PointPoint(0, 2));
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 && d1 > d3 {
        let v = d1 / (d1 - d3);
        return ([1.0 - v, v, 0.0], Closest::PointEdge(0, 1, 2));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return ([0.0, 0.0, 1.0], Closest::PointPoint(0, 3));
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 && d2 > d6 {
        let w = d2 / (d2 - d6);
        return ([1.0 - w, 0.0, w], Closest::PointEdge(0, 1, 3));
    }
    let va = d3 * d6 - d5 * d4;
    let bc_denom = (d4 - d3) + (d5 - d6);
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 && bc_denom > 0.0 {
        let w = (d4 - d3) / bc_denom;
        return ([0.0, 1.0 - w, w], Closest::PointEdge(0, 2, 3));
    }
    let area_sum = va + vb + vc;
    if area_sum > 0.0 && area_sum.is_finite() {
        let denom = 1.0 / area_sum;
        let (v, w) = (vb * denom, vc * denom);
        return ([1.0 - v - w, v, w], Closest::PointPlane);
    }
    // Degenerate (collinear or collapsed) triangle: choose the closest valid edge or vertex
    // instead of dividing by zero. Non-degenerate triangles never reach this branch.
    let mut best_d2 = (p - a).norm2();
    let mut best = ([1.0, 0.0, 0.0], Closest::PointPoint(0, 1));
    for (idx, vtx, w) in [(2usize, b, [0.0, 1.0, 0.0]), (3usize, c, [0.0, 0.0, 1.0])] {
        let d2 = (p - vtx).norm2();
        if d2 < best_d2 {
            best_d2 = d2;
            best = (w, Closest::PointPoint(0, idx));
        }
    }
    for (i, j, vi, vj, make_w) in [
        (
            1usize,
            2usize,
            a,
            b,
            (|t: f64| [1.0 - t, t, 0.0]) as fn(f64) -> [f64; 3],
        ),
        (1, 3, a, c, |t| [1.0 - t, 0.0, t]),
        (2, 3, b, c, |t| [0.0, 1.0 - t, t]),
    ] {
        let e = vj - vi;
        let e2 = e.norm2();
        if e2 > 0.0 && e2.is_finite() {
            let t = ((p - vi).dot(e) / e2).clamp(0.0, 1.0);
            if t > 0.0 && t < 1.0 {
                let q = vi + e * t;
                let d2 = (p - q).norm2();
                if d2 < best_d2 {
                    best_d2 = d2;
                    best = (make_w(t), Closest::PointEdge(0, i, j));
                }
            }
        }
    }
    best
}

/// Closest parameters (s on a₀a₁, t on b₀b₁) of two segments and the feature pair (Ericson 5.1.9,
/// parallel segments handled by clamping). Near-parallel but nonparallel edges retain their
/// interior closest points: replacing them with an endpoint overestimates separation and can
/// let additive CCD step through an edge crossing.
#[must_use]
pub fn edge_edge_closest(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> ((f64, f64), Closest) {
    let (d1, d2, r) = (a1 - a0, b1 - b0, a0 - b0);
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    let (c, b) = (d1.dot(r), d1.dot(d2));
    // Degenerate (zero-length) segments fall back to exact point-point or point-edge
    // classification instead of dividing by zero (Ericson 5.1.9).
    if a <= 0.0 && e <= 0.0 {
        return ((0.0, 0.0), Closest::PointPoint(0, 2));
    }
    if a <= 0.0 {
        let t = (f / e).clamp(0.0, 1.0);
        let kind = if t == 0.0 {
            Closest::PointPoint(0, 2)
        } else if t == 1.0 {
            Closest::PointPoint(0, 3)
        } else {
            Closest::PointEdge(0, 2, 3)
        };
        return ((0.0, t), kind);
    }
    if e <= 0.0 {
        let s = (-c / a).clamp(0.0, 1.0);
        let kind = if s == 0.0 {
            Closest::PointPoint(0, 2)
        } else if s == 1.0 {
            Closest::PointPoint(1, 2)
        } else {
            Closest::PointEdge(2, 0, 1)
        };
        return ((s, 0.0), kind);
    }
    let denom = a * e - b * b;
    let (mut s, mut t) = if denom <= 1e-12 * a * e {
        // The Gram determinant subtracts nearly equal numbers. Cross-product ratios avoid
        // that cancellation; the threshold selects arithmetic, not a parallel geometry model.
        let n = d1.cross(d2);
        let n2 = n.norm2();
        if n2 > 0.0 && n2.is_finite() {
            let s_raw = d2.cross(r).dot(n) / n2;
            if s_raw.is_finite() {
                let s = s_raw.clamp(0.0, 1.0);
                let t = if s > 0.0 && s < 1.0 {
                    let t_raw = d1.cross(r).dot(n) / n2;
                    if t_raw.is_finite() {
                        t_raw
                    } else {
                        (b * s + f) / e
                    }
                } else {
                    (b * s + f) / e
                };
                (s, t)
            } else {
                (0.0, f / e)
            }
        } else {
            // A vanishing cross product retains the existing parallel boundary convention.
            (0.0, f / e)
        }
    } else {
        let s = ((b * f - c * e) / denom).clamp(0.0, 1.0);
        (s, (b * s + f) / e)
    };
    if t < 0.0 {
        t = 0.0;
        s = (-c / a).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((b - c) / a).clamp(0.0, 1.0);
    }
    let (s_end, t_end) = (s == 0.0 || s == 1.0, t == 0.0 || t == 1.0);
    let pa = if s == 0.0 { 0 } else { 1 };
    let pb = if t == 0.0 { 2 } else { 3 };
    let kind = match (s_end, t_end) {
        (true, true) => Closest::PointPoint(pa, pb),
        (true, false) => Closest::PointEdge(pa, 2, 3),
        (false, true) => Closest::PointEdge(pb, 0, 1),
        (false, false) => Closest::LineLine,
    };
    ((s, t), kind)
}

/// Squared distance of the classified feature pair over the 12 coordinates of `pts` (points marked
/// `fixed` are constants), at the order of `T`: `f64` (value), [`Dual`] (gradient) or [`Jet`]
/// (gradient and Hessian).
#[must_use]
pub fn squared_distance_as<T: Scalar>(pts: [Vec3; 4], fixed: [bool; 4], kind: Closest) -> T {
    let p: [JVec<T>; 4] = std::array::from_fn(|k| JVec::point(pts[k], k, fixed[k]));
    match kind {
        Closest::PointPoint(i, j) => {
            let d = p[i].sub(&p[j]);
            d.dot(&d)
        }
        Closest::PointEdge(i, a, b) => {
            let c = p[a].sub(&p[i]).cross(&p[b].sub(&p[i]));
            let e = p[b].sub(&p[a]);
            c.dot(&c) / e.dot(&e)
        }
        Closest::PointPlane => {
            let n = p[2].sub(&p[1]).cross(&p[3].sub(&p[1]));
            let s = p[0].sub(&p[1]).dot(&n);
            s * s / n.dot(&n)
        }
        Closest::LineLine => {
            let n = p[1].sub(&p[0]).cross(&p[3].sub(&p[2]));
            let s = p[0].sub(&p[2]).dot(&n);
            s * s / n.dot(&n)
        }
    }
}

/// Squared distance with gradient and Hessian.
#[must_use]
pub fn squared_distance(pts: [Vec3; 4], fixed: [bool; 4], kind: Closest) -> Jet<12> {
    squared_distance_as(pts, fixed, kind)
}

/// Squared distance with gradient.
#[must_use]
pub fn squared_distance_grad(pts: [Vec3; 4], fixed: [bool; 4], kind: Closest) -> Dual<12> {
    squared_distance_as(pts, fixed, kind)
}

/// The same squared distance in plain f64 (for broad filtering, energy-only evaluations and CCD).
#[must_use]
pub fn squared_distance_f64(p: [Vec3; 4], kind: Closest) -> f64 {
    squared_distance_as(p, [true; 4], kind)
}

/// `|e_a × e_b|²` for the edge–edge mollifier, at the order of `T`.
#[must_use]
pub fn edge_cross_squared_as<T: Scalar>(pts: [Vec3; 4], fixed: [bool; 4]) -> T {
    let p: [JVec<T>; 4] = std::array::from_fn(|k| JVec::point(pts[k], k, fixed[k]));
    let c = p[1].sub(&p[0]).cross(&p[3].sub(&p[2]));
    c.dot(&c)
}

/// `|e_a × e_b|²` with gradient and Hessian.
#[must_use]
pub fn edge_cross_squared(pts: [Vec3; 4], fixed: [bool; 4]) -> Jet<12> {
    edge_cross_squared_as(pts, fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute_point_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
        let n = 400;
        let mut best = f64::INFINITY;
        for i in 0..=n {
            for j in 0..=n - i {
                let (u, v) = (i as f64 / n as f64, j as f64 / n as f64);
                let q = a * (1.0 - u - v) + b * u + c * v;
                best = best.min((p - q).norm2());
            }
        }
        best
    }

    fn lcg(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((*seed >> 11) as f64) / ((1u64 << 53) as f64) - 0.5
    }

    fn rnd(seed: &mut u64) -> Vec3 {
        Vec3::new(lcg(seed), lcg(seed), lcg(seed))
    }

    /// Every classified squared distance equals the true minimum (brute force), and its jet's
    /// gradient and Hessian match central differences.
    #[test]
    fn point_triangle_distance_regions_and_derivatives() {
        let mut seed = 1;
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..300 {
            let pts = [
                rnd(&mut seed) * 2.0,
                rnd(&mut seed),
                rnd(&mut seed),
                rnd(&mut seed),
            ];
            let (w, kind) = point_triangle_closest(pts[0], pts[1], pts[2], pts[3]);
            seen.insert(format!("{kind:?}"));
            let d = squared_distance(pts, [false; 4], kind);
            // The returned point lies on the triangle, the jet measures the distance to it, and no
            // sampled triangle point is closer.
            assert!(
                w.iter().all(|&b| (-1e-12..=1.0 + 1e-12).contains(&b))
                    && (w.iter().sum::<f64>() - 1.0).abs() < 1e-12
            );
            let q = pts[1] * w[0] + pts[2] * w[1] + pts[3] * w[2];
            assert!((d.v - (pts[0] - q).norm2()).abs() < 1e-12, "{kind:?}");
            let brute = brute_point_triangle(pts[0], pts[1], pts[2], pts[3]);
            assert!(d.v <= brute + 1e-12, "{kind:?}: {} vs {brute}", d.v);
            check_derivatives(pts, kind, |p| squared_distance(p, [false; 4], kind));
        }
        assert_eq!(seen.len(), 7, "all seven regions exercised: {seen:?}");
    }

    #[test]
    fn edge_edge_distance_types_and_derivatives() {
        let mut seed = 2;
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..300 {
            let pts = [
                rnd(&mut seed),
                rnd(&mut seed),
                rnd(&mut seed),
                rnd(&mut seed),
            ];
            let ((s, t), kind) = edge_edge_closest(pts[0], pts[1], pts[2], pts[3]);
            seen.insert(format!("{kind:?}"));
            let d = squared_distance(pts, [false; 4], kind);
            let qa = pts[0] * (1.0 - s) + pts[1] * s;
            let qb = pts[2] * (1.0 - t) + pts[3] * t;
            assert!((d.v - (qa - qb).norm2()).abs() < 1e-12, "{kind:?}");
            // Brute force over both segments.
            let mut best = f64::INFINITY;
            for i in 0..=200 {
                for j in 0..=200 {
                    let (u, v) = (f64::from(i) / 200.0, f64::from(j) / 200.0);
                    best = best.min(
                        ((pts[0] * (1.0 - u) + pts[1] * u) - (pts[2] * (1.0 - v) + pts[3] * v))
                            .norm2(),
                    );
                }
            }
            assert!(d.v <= best + 1e-12, "{kind:?}: {} > brute {best}", d.v);
            check_derivatives(pts, kind, |p| squared_distance(p, [false; 4], kind));
        }
        assert!(seen.len() >= 4, "{seen:?}");
    }

    fn edge_orders(p: [Vec3; 4]) -> [[Vec3; 4]; 8] {
        std::array::from_fn(|mask| {
            let mut q = p;
            if mask & 1 != 0 {
                q.swap(0, 1);
            }
            if mask & 2 != 0 {
                q.swap(2, 3);
            }
            if mask & 4 != 0 {
                q.rotate_left(2);
            }
            q
        })
    }

    #[test]
    fn near_parallel_edges_keep_the_independently_known_interior_minimum() {
        for scale in [1e-3, 1.0, 1e3] {
            for slope in [1e-7, 1e-9] {
                let z = scale * slope * 0.1;
                let p = [
                    Vec3::new(-scale, 0.0, 0.0),
                    Vec3::new(scale, 0.0, 0.0),
                    Vec3::new(-scale, -scale * slope, z),
                    Vec3::new(scale, scale * slope, z),
                ];
                // Both segment midpoints have x=y=0; their separation is exactly the z gap.
                // Exercise both windings, both segment orders, and all three coordinate axes.
                for axis in 0..3 {
                    let p = p.map(|v| Vec3::new(v[axis], v[(axis + 1) % 3], v[(axis + 2) % 3]));
                    for q in edge_orders(p) {
                        let ((s, t), kind) = edge_edge_closest(q[0], q[1], q[2], q[3]);
                        assert_eq!(kind, Closest::LineLine);
                        assert!((s - 0.5).abs() < 1e-12 && (t - 0.5).abs() < 1e-12);
                        let d2 = squared_distance_f64(q, kind);
                        assert!((d2 - z * z).abs() <= 1e-12 * z * z, "{d2} vs {}", z * z);
                    }
                }
                let jet = squared_distance(p, [false; 4], Closest::LineLine);
                assert!(
                    jet.g
                        .iter()
                        .chain(jet.h.iter().flatten())
                        .all(|v| v.is_finite())
                );
                for (i, expected) in [-z, -z, z, z].into_iter().enumerate() {
                    assert!((jet.g[3 * i + 2] - expected).abs() <= 1e-12 * z);
                }
            }
        }
    }

    #[test]
    fn near_parallel_edges_still_clamp_to_the_correct_segment_boundary() {
        let (eps, z) = (1e-7, 1e-8);
        let a = [Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)];
        for (b, expected) in [
            (
                [Vec3::new(2.0, eps, z), Vec3::new(3.0, 2.0 * eps, z)],
                1.0 + eps * eps + z * z,
            ),
            (
                [Vec3::new(-1.0, -eps, z), Vec3::new(0.5, -0.25 * eps, z)],
                0.0625 * eps * eps + z * z,
            ),
            (
                [Vec3::new(0.25, eps, z), Vec3::new(1.25, 2.0 * eps, z)],
                eps * eps + z * z,
            ),
        ] {
            for q in edge_orders([a[0], a[1], b[0], b[1]]) {
                let ((s, t), kind) = edge_edge_closest(q[0], q[1], q[2], q[3]);
                assert_ne!(kind, Closest::LineLine);
                assert!((0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t));
                let distance = squared_distance_f64(q, kind);
                assert!((distance - expected).abs() <= 1e-12 * expected);
            }
        }
        // A genuinely parallel overlap retains the previous endpoint/edge convention.
        assert_eq!(
            edge_edge_closest(
                Vec3::ZERO,
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.25),
                Vec3::new(3.0, 0.0, 0.25),
            ),
            ((0.5, 0.0), Closest::PointEdge(2, 0, 1))
        );
    }

    #[test]
    fn actual_edge_ccd_stops_near_parallel_crossings_between_clear_endpoints() {
        use crate::contact::{Pair, accd};

        for scale in [1e-3, 1.0, 1e3] {
            let (eps, z) = (scale * 1e-7, scale * 1e-8);
            let p = [
                Vec3::new(-scale, 0.0, 0.0),
                Vec3::new(scale, 0.0, 0.0),
                Vec3::new(-scale, -eps, z),
                Vec3::new(scale, eps, z),
            ];
            let movement = [
                Vec3::ZERO,
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, -2.0 * z),
                Vec3::new(0.0, 0.0, -2.0 * z),
            ];
            for (q, dp) in edge_orders(p).into_iter().zip(edge_orders(movement)) {
                for pair in [
                    Pair::EdgeEdge { ce: 0, be: 0 },
                    Pair::SelfEdgeEdge { a: 0, b: 1 },
                ] {
                    for xi in [0.0, 0.2 * z] {
                        // At both endpoints the segments are separated; their midpoints meet
                        // at t=0.5 (or touch the thickness offset earlier).
                        let impact = (z - xi) / (2.0 * z);
                        let fraction = accd(pair, q, dp, xi, 1.0);
                        assert!(
                            fraction > 0.0 && fraction < impact,
                            "CCD must stop before the known first contact: {fraction} vs {impact}"
                        );
                        assert!(z - 2.0 * z * fraction > xi);
                        assert_eq!(accd(pair, q, dp, xi, 0.2), 0.2);
                    }
                    let away = dp.map(|v| -v);
                    assert_eq!(accd(pair, q, away, 0.0, 1.0), 1.0);
                    let common = [Vec3::new(0.1, 0.2, 0.3); 4];
                    assert_eq!(accd(pair, q, common, 0.0, 1.0), 1.0);
                }
            }
        }
    }

    fn check_derivatives(pts: [Vec3; 4], kind: Closest, f: impl Fn([Vec3; 4]) -> Jet<12>) {
        let h = 1e-6;
        let j = f(pts);
        let d2: crate::ad::Dual2<12> = squared_distance_as(pts, [false; 4], kind);
        let d2_grad = d2.grad();
        let d2_hess = d2.hess();
        let scale = j.g.iter().fold(1e-9f64, |m, g| m.max(g.abs()));
        let hscale = j.h.iter().flatten().fold(1e-9f64, |m, g| m.max(g.abs()));
        assert!(
            (d2.value() - j.v).abs() <= 1e-12 * j.v.abs().max(1.0),
            "{kind:?} Dual2 value: {} vs {}",
            d2.value(),
            j.v
        );
        for i in 0..12 {
            assert!(
                (d2_grad[i] - j.g[i]).abs() <= 1e-12 * scale,
                "{kind:?} Dual2 grad[{i}]: {} vs {}",
                d2_grad[i],
                j.g[i]
            );
            let bump = |d: f64| {
                let mut q = pts;
                let c = &mut q[i / 3];
                match i % 3 {
                    0 => c.x += d,
                    1 => c.y += d,
                    _ => c.z += d,
                }
                q
            };
            let (fp, fm) = (f(bump(h)), f(bump(-h)));
            let fd = (fp.v - fm.v) / (2.0 * h);
            assert!(
                (fd - j.g[i]).abs() < 1e-6 * scale,
                "{kind:?} grad[{i}]: {fd} vs {}",
                j.g[i]
            );
            for k in 0..12 {
                assert!(
                    (d2_hess[k][i] - j.h[k][i]).abs() <= 1e-12 * hscale,
                    "{kind:?} Dual2 hess[{k}][{i}]: {} vs {}",
                    d2_hess[k][i],
                    j.h[k][i]
                );
                let fdh = (fp.g[k] - fm.g[k]) / (2.0 * h);
                assert!(
                    (fdh - j.h[k][i]).abs() < 1e-5 * hscale,
                    "{kind:?} hess[{k}][{i}]: {fdh} vs {}",
                    j.h[k][i]
                );
            }
        }
    }

    #[test]
    fn degenerate_and_collinear_triangles_return_finite_exact_closest_points() {
        let p = Vec3::new(0.5, 0.3, 0.0);
        let cases = [
            // Collapsed edge a == b != c (previously divided 0/0 in edge ab check)
            (
                Vec3::ZERO,
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                0.09,
                Vec3::new(0.5, 0.0, 0.0),
            ),
            // Collapsed edge a == c != b
            (
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::ZERO,
                0.09,
                Vec3::new(0.5, 0.0, 0.0),
            ),
            // Collapsed edge b == c != a
            (
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::ZERO,
                Vec3::ZERO,
                0.09,
                Vec3::new(0.5, 0.0, 0.0),
            ),
            // Fully collapsed point triangle a == b == c
            (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, 0.34, Vec3::ZERO),
            // Collinear triangle a, b, c with p projecting onto interior
            (
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 0.0),
                0.09,
                Vec3::new(0.5, 0.0, 0.0),
            ),
        ];
        for (a, b, c, expected_d2, expected_q) in cases {
            let (w, kind) = point_triangle_closest(p, a, b, c);
            assert!(w.iter().all(|v| v.is_finite()), "{w:?} {kind:?}");
            assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-12, "{w:?}");
            let q = a * w[0] + b * w[1] + c * w[2];
            assert!(
                (q - expected_q).norm() < 1e-12,
                "closest point {q:?} vs {expected_q:?} ({kind:?})"
            );
            let d2 = squared_distance_f64([p, a, b, c], kind);
            assert!(
                d2.is_finite() && (d2 - expected_d2).abs() < 1e-12,
                "d2={d2} vs {expected_d2} ({kind:?})"
            );
        }
    }

    #[test]
    fn degenerate_edges_return_finite_exact_closest_points() {
        let cases = [
            // Collapsed second edge projecting onto interior of first edge.
            (
                [
                    Vec3::ZERO,
                    Vec3::new(2.0, 0.0, 0.0),
                    Vec3::new(0.5, 0.3, 0.0),
                    Vec3::new(0.5, 0.3, 0.0),
                ],
                0.09,
                Closest::PointEdge(2, 0, 1),
            ),
            // Collapsed second edge projecting past endpoint of first edge.
            (
                [
                    Vec3::ZERO,
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(2.0, 3.0, 0.0),
                    Vec3::new(2.0, 3.0, 0.0),
                ],
                10.0,
                Closest::PointPoint(1, 2),
            ),
            // Collapsed first edge projecting onto interior of second edge.
            (
                [
                    Vec3::new(0.5, 0.4, 0.0),
                    Vec3::new(0.5, 0.4, 0.0),
                    Vec3::ZERO,
                    Vec3::new(2.0, 0.0, 0.0),
                ],
                0.16,
                Closest::PointEdge(0, 2, 3),
            ),
            // Collapsed first edge projecting before start of second edge.
            (
                [
                    Vec3::new(-1.0, 2.0, 0.0),
                    Vec3::new(-1.0, 2.0, 0.0),
                    Vec3::ZERO,
                    Vec3::new(2.0, 0.0, 0.0),
                ],
                5.0,
                Closest::PointPoint(0, 2),
            ),
            // Both edges collapsed to points.
            (
                [
                    Vec3::new(1.0, 2.0, 3.0),
                    Vec3::new(1.0, 2.0, 3.0),
                    Vec3::new(4.0, 6.0, 3.0),
                    Vec3::new(4.0, 6.0, 3.0),
                ],
                25.0,
                Closest::PointPoint(0, 2),
            ),
        ];
        for (pts, expected_d2, expected_kind) in cases {
            let ((s, t), kind) = edge_edge_closest(pts[0], pts[1], pts[2], pts[3]);
            assert!(
                s.is_finite()
                    && t.is_finite()
                    && (0.0..=1.0).contains(&s)
                    && (0.0..=1.0).contains(&t),
                "non-finite or out-of-range params ({s}, {t}) for {pts:?}"
            );
            assert_eq!(kind, expected_kind);
            let qa = pts[0] * (1.0 - s) + pts[1] * s;
            let qb = pts[2] * (1.0 - t) + pts[3] * t;
            assert!(((qa - qb).norm2() - expected_d2).abs() < 1e-12);
            let jet = squared_distance(pts, [false; 4], kind);
            assert!(
                jet.v.is_finite()
                    && (jet.v - expected_d2).abs() < 1e-12
                    && jet.g.iter().all(|v| v.is_finite())
                    && jet.h.iter().flatten().all(|v| v.is_finite()),
                "non-finite jet {jet:?} for {kind:?}"
            );
        }
    }
}
