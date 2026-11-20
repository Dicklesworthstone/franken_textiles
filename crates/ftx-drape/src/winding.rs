//! Dipole-accelerated generalized winding numbers (Barill et al., 2018).
//!
//! Provenance: adapted from frankensim `crates/fs-rep-mesh/src/winding.rs`
//! (`frankensim-source-manifest.json` `source_root`
//! `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`; MIT + OpenAI/Anthropic
//! rider). Adaptations for FrankenTextiles:
//! - uses [`ftx_core::Vec3`] and [`ftx_core::det::atan2`] (no platform libm, deterministic across
//!   native and `wasm32-unknown-unknown`);
//! - avoids `f64::mul_add` in hot loops to prevent software-FMA slowdown in browser WebAssembly;
//! - adds bottom-up [`WindingOctree::refit`] (triangle-inequality bounding radii) so moving bodies
//!   reuse the immutable octree topology without rebuilding;
//! - adds [`WindingOctree::is_inside`], which uses the O(log F) dipole octree at `beta = 2.5` and
//!   falls back to exact Van Oosterom–Strackee summation whenever `(|w| - 0.5).abs() < 0.05`,
//!   guaranteeing exact inside/outside parity with [`winding_exact`].

use std::f64::consts::PI;

use ftx_core::{Vec3, det};

/// Default accuracy knob β: nodes farther than `β * radius` use the dipole term.
pub const DEFAULT_BETA: f64 = 2.5;

/// Half-width around `|w| = 0.5` that triggers exact solid-angle fallback in
/// [`WindingOctree::is_inside`].
const EXACT_FALLBACK_BAND: f64 = 0.05;

/// Exact solid angle of triangle `[a, b, c]` seen from `p`, divided by `4π`
/// (Van Oosterom–Strackee). Positive when `p` sees the triangle from its inward side (so a closed
/// outward-wound shell has winding `+1` inside and `0` outside).
#[must_use]
pub fn triangle_winding(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let va = a - p;
    let vb = b - p;
    let vc = c - p;
    let (la, lb, lc) = (va.norm(), vb.norm(), vc.norm());
    let num = va.dot(vb.cross(vc));
    let den = la * lb * lc + va.dot(vb) * lc + va.dot(vc) * lb + vb.dot(vc) * la;
    (2.0 * det::atan2(num, den)) / (4.0 * PI)
}

/// Exact generalized winding number of a triangle mesh at `p` (O(F)).
#[must_use]
pub fn winding_exact(vertices: &[Vec3], triangles: &[[usize; 3]], p: Vec3) -> f64 {
    let mut sum = 0.0;
    for t in triangles {
        sum += triangle_winding(p, vertices[t[0]], vertices[t[1]], vertices[t[2]]);
    }
    sum
}

#[derive(Clone, Debug)]
struct DipoleNode {
    /// Aggregate area-weighted outward normal `Σ ½(b−a)×(c−a)`.
    moment: Vec3,
    /// Total area weight of triangles in this subtree.
    weight: f64,
    /// Area-weighted centroid.
    centroid: Vec3,
    /// Conservative bounding radius around `centroid` containing every corner in this subtree.
    radius: f64,
    /// Owned triangle indices (populated at leaves only).
    tris: Vec<usize>,
    /// Child octants (empty at leaves).
    children: Vec<DipoleNode>,
}

/// Barill et al. dipole octree for O(log F) winding-number queries on large triangle meshes.
#[derive(Clone, Debug)]
pub struct WindingOctree {
    root: Option<DipoleNode>,
    beta: f64,
}

fn tri_centroid_area_normal(vertices: &[Vec3], t: [usize; 3]) -> (Vec3, f64, Vec3) {
    let (a, b, c) = (vertices[t[0]], vertices[t[1]], vertices[t[2]]);
    let n2 = (b - a).cross(c - a);
    let area = 0.5 * n2.norm();
    let centroid = Vec3::new(
        (a.x + b.x + c.x) / 3.0,
        (a.y + b.y + c.y) / 3.0,
        (a.z + b.z + c.z) / 3.0,
    );
    (centroid, area, n2 * 0.5)
}

impl WindingOctree {
    /// Empty octree (returns `0.0` everywhere).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            root: None,
            beta: DEFAULT_BETA,
        }
    }

    /// Build a dipole octree over `triangles` with accuracy knob `beta >= 1.0`.
    #[must_use]
    pub fn build(vertices: &[Vec3], triangles: &[[usize; 3]], beta: f64) -> Self {
        let beta = if beta.is_finite() {
            beta.max(1.0)
        } else {
            DEFAULT_BETA
        };
        if triangles.is_empty() {
            return Self { root: None, beta };
        }
        let tris: Vec<usize> = (0..triangles.len()).collect();
        let root = Self::build_node(vertices, triangles, tris, 0);
        Self {
            root: Some(root),
            beta,
        }
    }

    fn build_node(
        vertices: &[Vec3],
        triangles: &[[usize; 3]],
        tris: Vec<usize>,
        depth: u32,
    ) -> DipoleNode {
        let mut moment = Vec3::ZERO;
        let mut weight = 0.0;
        let mut csum = Vec3::ZERO;
        for &t in &tris {
            let (c, area, n) = tri_centroid_area_normal(vertices, triangles[t]);
            moment += n;
            let w = area.max(1e-300);
            weight += w;
            csum += c * w;
        }
        let centroid = csum * (1.0 / weight);
        let mut radius = 0.0_f64;
        for &t in &tris {
            for &vi in &triangles[t] {
                radius = radius.max((vertices[vi] - centroid).norm());
            }
        }
        if tris.len() <= 16 || depth >= 20 {
            return DipoleNode {
                moment,
                weight,
                centroid,
                radius,
                tris,
                children: Vec::new(),
            };
        }
        let mut buckets: [Vec<usize>; 8] = Default::default();
        for &t in &tris {
            let (c, _, _) = tri_centroid_area_normal(vertices, triangles[t]);
            let idx = usize::from(c.x >= centroid.x)
                | (usize::from(c.y >= centroid.y) << 1)
                | (usize::from(c.z >= centroid.z) << 2);
            buckets[idx].push(t);
        }
        if buckets.iter().filter(|b| !b.is_empty()).count() <= 1 {
            return DipoleNode {
                moment,
                weight,
                centroid,
                radius,
                tris,
                children: Vec::new(),
            };
        }
        let children = buckets
            .into_iter()
            .filter(|b| !b.is_empty())
            .map(|b| Self::build_node(vertices, triangles, b, depth + 1))
            .collect();
        DipoleNode {
            moment,
            weight,
            centroid,
            radius,
            tris: Vec::new(),
            children,
        }
    }

    /// Recompute dipole moments, centroids, and conservative bounding radii bottom-up for moved
    /// vertices with unchanged triangle topology (zero heap allocations).
    pub fn refit(&mut self, vertices: &[Vec3], triangles: &[[usize; 3]]) {
        if let Some(root) = &mut self.root {
            Self::refit_node(root, vertices, triangles);
        }
    }

    fn refit_node(node: &mut DipoleNode, vertices: &[Vec3], triangles: &[[usize; 3]]) {
        if node.children.is_empty() {
            let mut moment = Vec3::ZERO;
            let mut weight = 0.0;
            let mut csum = Vec3::ZERO;
            for &t in &node.tris {
                let (c, area, n) = tri_centroid_area_normal(vertices, triangles[t]);
                moment += n;
                let w = area.max(1e-300);
                weight += w;
                csum += c * w;
            }
            let centroid = csum * (1.0 / weight);
            let mut radius = 0.0_f64;
            for &t in &node.tris {
                for &vi in &triangles[t] {
                    radius = radius.max((vertices[vi] - centroid).norm());
                }
            }
            node.moment = moment;
            node.weight = weight;
            node.centroid = centroid;
            node.radius = radius;
            return;
        }
        let mut moment = Vec3::ZERO;
        let mut weight = 0.0;
        let mut csum = Vec3::ZERO;
        for ch in &mut node.children {
            Self::refit_node(ch, vertices, triangles);
            moment += ch.moment;
            weight += ch.weight;
            csum += ch.centroid * ch.weight;
        }
        let centroid = csum * (1.0 / weight);
        // Triangle inequality guarantees every corner in every child lies within `radius`.
        let mut radius = 0.0_f64;
        for ch in &node.children {
            radius = radius.max((ch.centroid - centroid).norm() + ch.radius);
        }
        node.moment = moment;
        node.weight = weight;
        node.centroid = centroid;
        node.radius = radius;
    }

    fn eval_node(
        &self,
        vertices: &[Vec3],
        triangles: &[[usize; 3]],
        node: &DipoleNode,
        p: Vec3,
    ) -> f64 {
        let d = node.centroid - p;
        let dist = d.norm();
        if dist > self.beta * node.radius && dist > 0.0 {
            let inv_d = 1.0 / dist;
            return (d * inv_d).dot(node.moment) / (4.0 * PI * dist * dist);
        }
        if node.children.is_empty() {
            let mut sum = 0.0;
            for &t in &node.tris {
                let tri = triangles[t];
                sum += triangle_winding(p, vertices[tri[0]], vertices[tri[1]], vertices[tri[2]]);
            }
            return sum;
        }
        let mut sum = 0.0;
        for ch in &node.children {
            sum += self.eval_node(vertices, triangles, ch, p);
        }
        sum
    }

    /// Approximate generalized winding number at `p` in O(log F).
    #[must_use]
    pub fn winding(&self, vertices: &[Vec3], triangles: &[[usize; 3]], p: Vec3) -> f64 {
        match &self.root {
            Some(root) => self.eval_node(vertices, triangles, root, p),
            None => 0.0,
        }
    }

    /// Exact-guarded inside classification (`|w| > 0.5`): uses the O(log F) dipole octree and falls
    /// back to exact Van Oosterom–Strackee summation whenever the approximation lands near `0.5` or
    /// is non-finite.
    #[must_use]
    pub fn is_inside(&self, vertices: &[Vec3], triangles: &[[usize; 3]], p: Vec3) -> bool {
        let w = self.winding(vertices, triangles, p);
        if !w.is_finite() || (w.abs() - 0.5).abs() < EXACT_FALLBACK_BAND {
            winding_exact(vertices, triangles, p).abs() > 0.5
        } else {
            w.abs() > 0.5
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Closed icosphere built by recursive midpoint subdivision of an octahedron (8 * 4^level faces).
    fn octasphere(level: usize, radius: f64, center: Vec3) -> (Vec<Vec3>, Vec<[usize; 3]>) {
        let mut verts = vec![
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
        ];
        let mut tris = vec![
            [0, 2, 4],
            [2, 1, 4],
            [1, 3, 4],
            [3, 0, 4],
            [2, 0, 5],
            [1, 2, 5],
            [3, 1, 5],
            [0, 3, 5],
        ];
        for _ in 0..level {
            let mut next = Vec::with_capacity(tris.len() * 4);
            let mut mid_map = std::collections::BTreeMap::new();
            let mut midpoint = |a: usize, b: usize, verts: &mut Vec<Vec3>| {
                let key = (a.min(b), a.max(b));
                *mid_map.entry(key).or_insert_with(|| {
                    let p = (verts[a] + verts[b]) * 0.5;
                    let n = p.norm();
                    verts.push(p * (1.0 / n));
                    verts.len() - 1
                })
            };
            for [a, b, c] in tris {
                let ab = midpoint(a, b, &mut verts);
                let bc = midpoint(b, c, &mut verts);
                let ca = midpoint(c, a, &mut verts);
                next.push([a, ab, ca]);
                next.push([ab, b, bc]);
                next.push([ca, bc, c]);
                next.push([ab, bc, ca]);
            }
            tris = next;
        }
        for v in &mut verts {
            *v = *v * radius + center;
        }
        (verts, tris)
    }

    #[test]
    fn dipole_octree_matches_exact_winding_on_closed_sphere_and_after_refit() {
        // 512 triangles: deep enough to exercise multi-level octree dipole aggregation.
        let (verts, tris) = octasphere(3, 0.25, Vec3::new(0.1, -0.2, 0.8));
        assert_eq!(tris.len(), 512);
        let mut octree = WindingOctree::build(&verts, &tris, DEFAULT_BETA);
        let probes = [
            (Vec3::new(0.1, -0.2, 0.8), true),    // center
            (Vec3::new(0.30, -0.2, 0.8), true),   // near inner boundary
            (Vec3::new(0.34, -0.2, 0.8), true),   // very close inside
            (Vec3::new(0.36, -0.2, 0.8), false),  // very close outside
            (Vec3::new(0.8, 0.4, 1.5), false),    // far outside
            (Vec3::new(-1.0, -1.0, -1.0), false), // distant outside
        ];
        for (p, expected_inside) in probes {
            let exact = winding_exact(&verts, &tris, p);
            let approx = octree.winding(&verts, &tris, p);
            assert!(
                (approx - exact).abs() < 2e-2,
                "probe {p:?}: exact={exact}, approx={approx}"
            );
            assert_eq!(octree.is_inside(&verts, &tris, p), expected_inside);
            assert_eq!(exact.abs() > 0.5, expected_inside);
        }

        // Rigid translation + anisotropic stretch via zero-allocation refit.
        let moved: Vec<Vec3> = verts
            .iter()
            .map(|&v| Vec3::new(v.x * 1.2 + 0.5, v.y * 0.8 - 0.3, v.z + 0.4))
            .collect();
        octree.refit(&moved, &tris);
        let moved_center = Vec3::new(0.1 * 1.2 + 0.5, -0.2 * 0.8 - 0.3, 0.8 + 0.4);
        assert!(octree.is_inside(&moved, &tris, moved_center));
        assert!(!octree.is_inside(&moved, &tris, Vec3::new(0.1, -0.2, 0.8)));
        for p in [
            moved_center,
            moved_center + Vec3::new(0.28, 0.0, 0.0),
            moved_center + Vec3::new(0.35, 0.0, 0.0),
            Vec3::new(2.0, 2.0, 2.0),
        ] {
            let exact = winding_exact(&moved, &tris, p);
            let approx = octree.winding(&moved, &tris, p);
            assert!(
                (approx - exact).abs() < 2e-2,
                "refitted probe {p:?}: exact={exact}, approx={approx}"
            );
            assert_eq!(octree.is_inside(&moved, &tris, p), exact.abs() > 0.5);
        }
    }

    #[test]
    fn empty_octree_and_determinism() {
        let empty = WindingOctree::empty();
        assert_eq!(empty.winding(&[], &[], Vec3::ZERO), 0.0);
        assert!(!empty.is_inside(&[], &[], Vec3::ZERO));

        let (verts, tris) = octasphere(2, 0.3, Vec3::ZERO);
        let a = WindingOctree::build(&verts, &tris, DEFAULT_BETA);
        let b = WindingOctree::build(&verts, &tris, DEFAULT_BETA);
        let p = Vec3::new(0.12, -0.08, 0.05);
        assert_eq!(
            a.winding(&verts, &tris, p).to_bits(),
            b.winding(&verts, &tris, p).to_bits()
        );
    }
}
