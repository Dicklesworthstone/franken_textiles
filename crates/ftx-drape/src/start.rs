//! The kinematic start state (spec 04 §3–4): each piece wrapped isometrically around its body part, the
//! seams closed outside the body (pre-sew), then position-based relaxation toward the flat rest lengths
//! with the sewing groups held together and the cloth pushed out of the body.
//!
//! Port of the reference's `placed_positions`, `gather_weights`, `presew` and `relax` with their
//! arithmetic order (numpy's `add.at` accumulation order, pairwise means), generic over the body: any
//! [`NearestTree`] (the reference's dress form in the parity tests, the Anny skin for real drapes) and
//! any part frames (see [`WrapPlacement`]).

use ftx_core::np::{pairwise_sum, percentile};
use ftx_core::{Vec2, Vec3};

use crate::garment::GarmentMesh;
use crate::nearest::NearestTree;

/// mm → m.
pub const MM: f64 = 0.001;
/// Pre-sew clearance above the body (m).
pub const PRESEW_CLEARANCE: f64 = 0.008;
/// Push-out clearance during relaxation (m; the reference's literal 0.0048, which is not the double
/// `0.008 × 0.6` the pre-sew pushes out to).
pub const RELAX_CLEARANCE: f64 = 0.0048;
/// Harmonic spreading iterations of the pre-sew.
pub const PRESEW_ITERS: usize = 400;
/// Relaxation block length.
pub const RELAX_ITERS: usize = 200;
/// Stop relaxing when the p95 of |strain| over plain edges is at most this.
pub const RELAX_TARGET_P95: f64 = 0.12;
/// Cap on the total relaxation iterations.
pub const RELAX_MAX_ITERS: usize = 1600;
/// How far from a gathered seam the cloth may draw up in its own plane (mm).
pub const GATHER_ZONE_MM: f64 = 50.0;

/// A body-part frame (spec 04 §3.2), in mm: `h` runs along `up`, θ = 0 faces `front`, +90° faces
/// `lat = up × front`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartFrame {
    /// Origin (mm).
    pub origin: Vec3,
    /// Proximal direction.
    pub up: Vec3,
    /// θ = 0.
    pub front: Vec3,
    /// θ = 90°.
    pub lat: Vec3,
}

/// Everything the wrap needs for one instance: the frame, the anchor height `h0 = landmark_h − y0`, the
/// wrap radius, the anchor column and angle, and the mirroring rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WrapPlacement {
    /// The part frame (always the wearer's-right one for limbs).
    pub frame: PartFrame,
    /// Height of piece y = 0 along `up` (mm).
    pub h0: f64,
    /// Wrap radius (mm).
    pub radius: f64,
    /// Angle of the anchor column (degrees).
    pub theta_deg: f64,
    /// Piece x of the anchor column (mm, after `sim_scale`).
    pub x0: f64,
    /// The instance is a mirrored (`.L`) copy: mirror its 2D x, wrap, then mirror the 3D x.
    pub mirrored: bool,
    /// Clamp the drafted-frame 3D x to ≤ −1 mm before mirroring (side R/L instances on the leg).
    pub own_side: bool,
}

/// The isometric wrap of one instance's 2D points (mm) into metres (spec 04 §3.3–3.4).
#[must_use]
pub fn wrap_points(points: &[Vec2], p: &WrapPlacement) -> Vec<Vec3> {
    let f = &p.frame;
    let theta0 = p.theta_deg.to_radians();
    points
        .iter()
        .map(|q| {
            let x = if p.mirrored { -q.x } else { q.x };
            let h = p.h0 + q.y;
            let th = theta0 + (x - p.x0) / p.radius;
            let (s, c) = (ftx_core::det::sin(th), ftx_core::det::cos(th));
            let r = f.lat * s + f.front * c;
            let mut v = f.origin + f.up * h + r * p.radius;
            if p.own_side {
                v.x = v.x.min(-1.0);
            }
            if p.mirrored {
                v.x = -v.x;
            }
            v * MM
        })
        .collect()
}

/// Unique triangle edges `(min, max)`, sorted.
#[must_use]
pub fn face_edges(faces: &[[usize; 3]]) -> Vec<(usize, usize)> {
    let mut e: Vec<(usize, usize)> = faces
        .iter()
        .flat_map(|f| (0..3).map(move |k| (f[k].min(f[(k + 1) % 3]), f[k].max(f[(k + 1) % 3]))))
        .collect();
    e.sort_unstable();
    e.dedup();
    e
}

fn norm3(d: Vec3) -> f64 {
    (d.x * d.x + d.y * d.y + d.z * d.z).sqrt()
}

fn norm2(d: Vec2) -> f64 {
    (d.x * d.x + d.y * d.y).sqrt()
}

/// Strain `L₃/L₂ − 1` of each edge (rest lengths from the 2D mesh in mm).
#[must_use]
pub fn edge_strain(x: &[Vec3], rest2d: &[Vec2], edges: &[(usize, usize)]) -> Vec<f64> {
    edges
        .iter()
        .map(|&(a, b)| {
            let l3 = norm3(x[a] - x[b]);
            let l2 = norm2(rest2d[a] - rest2d[b]) * MM;
            l3 / l2.max(1e-9) - 1.0
        })
        .collect()
}

/// Per-vertex compression weight (spec 04 §5.4): 0 on the long side of a gathered seam (|ratio − 1| >
/// 0.15), rising to 1 at [`GATHER_ZONE_MM`] from it, 1 elsewhere.
#[must_use]
pub fn gather_weights(mesh: &GarmentMesh) -> Vec<f64> {
    let r2 = &mesh.verts2d;
    let mut w = vec![1.0_f64; r2.len()];
    for s in &mesh.seams {
        if (s.ratio - 1.0).abs() <= 0.15 {
            continue;
        }
        let length = |side: &[usize]| {
            let seg: Vec<f64> = side
                .windows(2)
                .map(|p| norm2(r2[p[1]] - r2[p[0]]))
                .collect();
            pairwise_sum(&seg)
        };
        let (la, lb) = (length(&s.side_a), length(&s.side_b));
        let long = if la >= lb { &s.side_a } else { &s.side_b };
        let Some(&first) = long.first() else { continue };
        let (lo, hi) = mesh.inst_range[mesh.owner[first]];
        for i in lo..hi {
            let d = long
                .iter()
                .map(|&j| norm2(r2[i] - r2[j]))
                .fold(f64::INFINITY, f64::min);
            w[i] = w[i].min((d / GATHER_ZONE_MM).clamp(0.0, 1.0));
        }
    }
    w
}

/// Sewing groups: connected components of the sewing pairs, each sorted, ordered by smallest member.
#[must_use]
pub fn sewing_groups(n: usize, pairs: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for &(a, b) in pairs {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[ra] = rb;
        }
    }
    let mut members: Vec<usize> = pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
    members.sort_unstable();
    members.dedup();
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for v in members {
        let r = find(&mut parent, v);
        groups.entry(r).or_default().push(v);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by_key(|g| g[0]);
    out
}

/// numpy's `P[members].mean(axis=0)`: per coordinate, a pairwise sum over the members divided by n.
fn mean(x: &[Vec3], members: &[usize]) -> Vec3 {
    let n = members.len() as f64;
    let col = |f: fn(&Vec3) -> f64| {
        pairwise_sum(&members.iter().map(|&i| f(&x[i])).collect::<Vec<_>>()) / n
    };
    Vec3::new(col(|v| v.x), col(|v| v.y), col(|v| v.z))
}

/// Vertex degrees on the edge graph.
fn degrees(n: usize, edges: &[(usize, usize)]) -> Vec<f64> {
    let mut deg = vec![0.0; n];
    for &(a, _) in edges {
        deg[a] += 1.0;
    }
    for &(_, b) in edges {
        deg[b] += 1.0;
    }
    deg
}

/// Pre-sew (spec 04 §4.2): every sewing group moves to its mean (pushed to `clearance` outside the body
/// when closer), the rest of each piece follows by harmonic spreading, then everything is pushed out to
/// `0.6·clearance`. Returns the pre-sewn positions and the groups.
#[must_use]
pub fn presew(
    mesh: &GarmentMesh,
    wrapped: &[Vec3],
    body: &NearestTree,
    clearance: f64,
    iters: usize,
) -> (Vec<Vec3>, Vec<Vec<usize>>) {
    let n = wrapped.len();
    let mut disp = vec![Vec3::ZERO; n];
    let mut fixed = vec![false; n];
    let groups = sewing_groups(n, &mesh.pairs);
    for g in &groups {
        let mut m = mean(wrapped, g);
        let near = body.nearest(m);
        if (m - near.point).dot(near.normal) < clearance {
            m = near.point + near.normal * clearance;
        }
        for &v in g {
            disp[v] = m - wrapped[v];
            fixed[v] = true;
        }
    }
    let edges = face_edges(&mesh.faces);
    let deg = degrees(n, &edges);
    let free: Vec<bool> = (0..n).map(|i| !fixed[i] && deg[i] > 0.0).collect();
    for _ in 0..iters {
        let mut acc = vec![Vec3::ZERO; n];
        for &(a, b) in &edges {
            acc[a] += disp[b];
        }
        for &(a, b) in &edges {
            acc[b] += disp[a];
        }
        for i in 0..n {
            if free[i] {
                disp[i] = acc[i] / deg[i];
            }
        }
    }
    let mut q: Vec<Vec3> = wrapped.iter().zip(&disp).map(|(p, d)| *p + *d).collect();
    body.push_out(&mut q, clearance * 0.6);
    (q, groups)
}

/// One relaxation block (spec 04 §4.3): `iters` Jacobi edge-length projections toward the flat rest
/// lengths (shortening a gathered edge costs only its weight), the sewing groups set to their means after
/// each, push-out to `clearance` every 25th iteration and at the last.
#[must_use]
pub fn relax(
    mesh: &GarmentMesh,
    start: &[Vec3],
    groups: &[Vec<usize>],
    body: &NearestTree,
    iters: usize,
    clearance: f64,
    weights: &[f64],
) -> Vec<Vec3> {
    let edges = face_edges(&mesh.faces);
    let l0: Vec<f64> = edges
        .iter()
        .map(|&(a, b)| norm2(mesh.verts2d[a] - mesh.verts2d[b]) * MM)
        .collect();
    let w_e: Vec<f64> = edges
        .iter()
        .map(|&(a, b)| 0.5 * (weights[a] + weights[b]))
        .collect();
    let n = start.len();
    let deg: Vec<f64> = degrees(n, &edges).into_iter().map(|d| d.max(1.0)).collect();
    let mut q = start.to_vec();
    for it in 0..iters {
        let corr: Vec<Vec3> = edges
            .iter()
            .enumerate()
            .map(|(k, &(a, b))| {
                let d = q[b] - q[a];
                let l = norm3(d);
                let s = (if l < l0[k] { w_e[k] } else { 1.0 }) * (l - l0[k]) / l.max(1e-9);
                d * s * 0.5
            })
            .collect();
        let mut acc = vec![Vec3::ZERO; n];
        for (k, &(a, _)) in edges.iter().enumerate() {
            acc[a] += corr[k];
        }
        for (k, &(_, b)) in edges.iter().enumerate() {
            acc[b] -= corr[k]; // numpy adds −corr: the same rounding
        }
        for i in 0..n {
            q[i] += acc[i] * 0.8 / deg[i];
        }
        for g in groups {
            let m = mean(&q, g);
            for &v in g {
                q[v] = m;
            }
        }
        if it % 25 == 24 || it + 1 == iters {
            body.push_out(&mut q, clearance);
        }
    }
    q
}

/// The relaxed start state.
#[derive(Clone, Debug)]
pub struct StartState {
    /// Wrapped positions (m).
    pub wrapped: Vec<Vec3>,
    /// Pre-sewn positions (m).
    pub presewn: Vec<Vec3>,
    /// Sewing groups.
    pub groups: Vec<Vec<usize>>,
    /// Relaxed positions (m): the drape's start.
    pub relaxed: Vec<Vec3>,
    /// Relaxation iterations run.
    pub relax_iterations: usize,
    /// p95 of |strain| over plain edges after each block.
    pub block_p95: Vec<f64>,
    /// Gather weights used.
    pub gather_weights: Vec<f64>,
}

/// Pre-sew then relax in blocks until the plain-edge p95 strain is at most [`RELAX_TARGET_P95`] or
/// [`RELAX_MAX_ITERS`] have run (spec 04 §4).
#[must_use]
pub fn start_state(mesh: &GarmentMesh, wrapped: Vec<Vec3>, body: &NearestTree) -> StartState {
    let weights = gather_weights(mesh);
    let (presewn, groups) = presew(mesh, &wrapped, body, PRESEW_CLEARANCE, PRESEW_ITERS);
    let edges = face_edges(&mesh.faces);
    let plain: Vec<(usize, usize)> = edges
        .into_iter()
        .filter(|&(a, b)| weights[a] >= 1.0 && weights[b] >= 1.0)
        .collect();
    let (mut q, mut done, mut blocks) = (presewn.clone(), 0, Vec::new());
    while done < RELAX_MAX_ITERS {
        q = relax(
            mesh,
            &q,
            &groups,
            body,
            RELAX_ITERS,
            RELAX_CLEARANCE,
            &weights,
        );
        done += RELAX_ITERS;
        let strain: Vec<f64> = edge_strain(&q, &mesh.verts2d, &plain)
            .into_iter()
            .map(f64::abs)
            .collect();
        let p95 = percentile(&strain, 95.0);
        blocks.push(p95);
        if p95 <= RELAX_TARGET_P95 {
            break;
        }
    }
    StartState {
        wrapped,
        presewn,
        groups,
        relaxed: q,
        relax_iterations: done,
        block_p95: blocks,
        gather_weights: weights,
    }
}
