//! Nearest point on a triangle soup: an axis-aligned bounding-box tree searched branch-and-bound, giving
//! the closest point, that triangle's unit normal and the signed distance `(p − q)·n` the start state
//! uses to keep cloth outside the body (spec 04 §4.1).
//!
//! Ties: when the nearest feature is an edge or vertex shared by several triangles (equal distances to
//! 1e-12 relative), the triangle whose normal best aligns with `p − q` wins, so the signed distance is
//! the true distance outside a convex crease. (Blender's BVH, which the reference uses, keeps whichever
//! tied triangle its traversal meets first.)

use ftx_core::{Refusal, RefusalCode, Vec3};

use crate::distance::point_triangle_closest;

#[derive(Clone, Debug)]
struct Node {
    lo: Vec3,
    hi: Vec3,
    /// Leaf: `order[start..end]`; inner: children `left`, `right`.
    start: usize,
    end: usize,
    left: usize,
    right: usize,
}

/// A static triangle soup with an AABB tree for nearest-point queries.
#[derive(Clone, Debug)]
pub struct NearestTree {
    vertices: Vec<Vec3>,
    triangles: Vec<[usize; 3]>,
    order: Vec<usize>,
    nodes: Vec<Node>,
}

/// The nearest point of the soup to a query point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nearest {
    /// Closest point on the surface.
    pub point: Vec3,
    /// Unit normal of the triangle it lies on.
    pub normal: Vec3,
    /// That triangle's index.
    pub triangle: usize,
    /// Distance to the query point.
    pub distance: f64,
}

const LEAF: usize = 4;

fn bounds(points: impl Iterator<Item = Vec3>) -> (Vec3, Vec3) {
    points.fold(
        (
            Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(lo, hi), p| {
            (
                Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)),
                Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)),
            )
        },
    )
}

fn box_distance2(p: Vec3, lo: Vec3, hi: Vec3) -> f64 {
    let d = |v: f64, a: f64, b: f64| {
        if v < a {
            a - v
        } else if v > b {
            v - b
        } else {
            0.0
        }
    };
    let (dx, dy, dz) = (d(p.x, lo.x, hi.x), d(p.y, lo.y, hi.y), d(p.z, lo.z, hi.z));
    dx * dx + dy * dy + dz * dz
}

impl NearestTree {
    /// Builds the tree (median splits on the longest axis of the triangle centroids).
    ///
    /// # Errors
    /// [`RefusalCode::DegenerateGeometry`] for no triangles or an index out of range;
    /// [`RefusalCode::NonFinite`] for a non-finite vertex.
    pub fn new(vertices: Vec<Vec3>, triangles: Vec<[usize; 3]>) -> Result<Self, Refusal> {
        if triangles.is_empty() || triangles.iter().flatten().any(|&i| i >= vertices.len()) {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "the surface is empty or indexes a missing vertex",
            ));
        }
        if vertices.iter().any(|v| !v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the surface has a non-finite vertex",
            ));
        }
        let centroid: Vec<Vec3> = triangles
            .iter()
            .map(|t| (vertices[t[0]] + vertices[t[1]] + vertices[t[2]]) / 3.0)
            .collect();
        let mut tree = Self {
            order: (0..triangles.len()).collect(),
            vertices,
            triangles,
            nodes: Vec::new(),
        };
        tree.build(0, tree.triangles.len(), &centroid);
        Ok(tree)
    }

    fn build(&mut self, start: usize, end: usize, centroid: &[Vec3]) -> usize {
        let (lo, hi) = bounds(
            self.order[start..end]
                .iter()
                .flat_map(|&t| self.triangles[t].map(|v| self.vertices[v])),
        );
        let id = self.nodes.len();
        self.nodes.push(Node {
            lo,
            hi,
            start,
            end,
            left: 0,
            right: 0,
        });
        if end - start > LEAF {
            let (clo, chi) = bounds(self.order[start..end].iter().map(|&t| centroid[t]));
            let ext = chi - clo;
            let axis = if ext.x >= ext.y && ext.x >= ext.z {
                0
            } else if ext.y >= ext.z {
                1
            } else {
                2
            };
            let key = |t: usize| {
                let c = centroid[t];
                [c.x, c.y, c.z][axis]
            };
            self.order[start..end].sort_by(|&a, &b| key(a).total_cmp(&key(b)).then(a.cmp(&b)));
            let mid = start + (end - start) / 2;
            let left = self.build(start, mid, centroid);
            let right = self.build(mid, end, centroid);
            self.nodes[id].left = left;
            self.nodes[id].right = right;
        }
        id
    }

    /// The nearest point of the surface to `p`.
    #[must_use]
    pub fn nearest(&self, p: Vec3) -> Nearest {
        const TIE: f64 = 1e-12;
        // (distance², triangle, point, normal, signed distance)
        let mut best = (
            f64::INFINITY,
            0usize,
            Vec3::ZERO,
            Vec3::ZERO,
            f64::NEG_INFINITY,
        );
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            let n = &self.nodes[id];
            if box_distance2(p, n.lo, n.hi) > best.0 * (1.0 + TIE) {
                continue;
            }
            if n.end - n.start <= LEAF {
                for &t in &self.order[n.start..n.end] {
                    let [a, b, c] = self.triangles[t].map(|v| self.vertices[v]);
                    let (w, _) = point_triangle_closest(p, a, b, c);
                    let q = a * w[0] + b * w[1] + c * w[2];
                    let d2 = (p - q).norm2();
                    if d2 > best.0 * (1.0 + TIE) {
                        continue;
                    }
                    let normal = (b - a).cross(c - a).normalized();
                    let sd = (p - q).dot(normal);
                    if d2 < best.0 * (1.0 - TIE) || sd > best.4 {
                        best = (d2.min(best.0), t, q, normal, sd);
                    }
                }
                continue;
            }
            let (l, r) = (n.left, n.right);
            let (dl, dr) = (
                box_distance2(p, self.nodes[l].lo, self.nodes[l].hi),
                box_distance2(p, self.nodes[r].lo, self.nodes[r].hi),
            );
            // Push the farther child first so the nearer is searched first.
            if dl <= dr {
                stack.push(r);
                stack.push(l);
            } else {
                stack.push(l);
                stack.push(r);
            }
        }
        Nearest {
            point: best.2,
            normal: best.3,
            triangle: best.1,
            distance: (p - best.2).norm(),
        }
    }

    /// Signed distance `(p − q)·n` to the nearest triangle (positive outside an outward-wound surface).
    #[must_use]
    pub fn signed_distance(&self, p: Vec3) -> f64 {
        let n = self.nearest(p);
        (p - n.point).dot(n.normal)
    }

    /// Moves every point closer than `clearance` (signed) back out to `q + n·clearance`, in order.
    pub fn push_out(&self, points: &mut [Vec3], clearance: f64) {
        for p in points.iter_mut() {
            let n = self.nearest(*p);
            if (*p - n.point).dot(n.normal) < clearance {
                *p = n.point + n.normal * clearance;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against brute force on a closed box surface: same distance for scattered points, signed
    /// distance positive outside and negative inside.
    #[test]
    fn nearest_matches_brute_force() {
        let v: Vec<Vec3> = (0..8)
            .map(|i| {
                Vec3::new(
                    f64::from(i & 1) * 0.3,
                    f64::from((i >> 1) & 1) * 0.2,
                    f64::from((i >> 2) & 1) * 0.5,
                )
            })
            .collect();
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let tris: Vec<[usize; 3]> = quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect();
        let tree = NearestTree::new(v.clone(), tris.clone()).unwrap();
        let mut seed = 12_345u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..500 {
            let p = Vec3::new(rnd() - 0.3, rnd() * 0.8 - 0.3, rnd() * 1.1 - 0.3);
            let brute = tris
                .iter()
                .map(|t| {
                    let (w, _) = point_triangle_closest(p, v[t[0]], v[t[1]], v[t[2]]);
                    (p - (v[t[0]] * w[0] + v[t[1]] * w[1] + v[t[2]] * w[2])).norm()
                })
                .fold(f64::INFINITY, f64::min);
            let n = tree.nearest(p);
            assert!((n.distance - brute).abs() < 1e-15, "{p:?}");
        }
        assert!(tree.signed_distance(Vec3::new(0.15, 0.1, 0.6)) > 0.0);
        assert!(tree.signed_distance(Vec3::new(0.15, 0.1, 0.25)) < 0.0);
    }
}
