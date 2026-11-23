//! The kinematic start state (spec 04 §3–4): each piece wrapped isometrically around its body part, the
//! seams closed outside the body (pre-sew), then position-based relaxation toward the flat rest lengths
//! with the sewing groups held together and the cloth pushed out of the body.
//!
//! Port of the reference's `placed_positions`, `gather_weights`, `presew` and `relax` with their
//! arithmetic order (numpy's `add.at` accumulation order, pairwise means), generic over the body: any
//! [`NearestTree`] (the reference's dress form in the parity tests, the Anny skin for real drapes) and
//! any part frames (see [`WrapPlacement`]).

use ftx_core::np::{pairwise_sum, percentile};
use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::distance::{edge_edge_closest, point_triangle_closest};
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
/// Push-out period within a relaxation block, also a boundary for exact start-state replay.
pub const RELAX_PUSH_OUT_ITERS: usize = 25;
/// Stop relaxing when the p95 of |strain| over plain edges is at most this.
pub const RELAX_TARGET_P95: f64 = 0.12;
/// Cap on the total relaxation iterations.
pub const RELAX_MAX_ITERS: usize = 1600;
/// A seam is gathered when its `ratio` differs from 1 by more than this (spec 04 §5.4).
pub const GATHER_RATIO_MARGIN: f64 = 0.15;
/// How far from a gathered seam the cloth may draw up in its own plane (mm).
pub const GATHER_ZONE_MM: f64 = 50.0;
/// Compression stiffness at a gathered seam, as a fraction of the tensile stiffness (the reference's
/// `GATHER_COMPRESSION`); it rises linearly with the gather weight to the cloth's own factor.
pub const GATHER_COMPRESSION: f64 = 0.03;
/// Default minimum cloth–cloth separation distance for opt-in self-repelling start relaxation (m).
pub const DEFAULT_SELF_REPULSION_CLEARANCE_M: f64 = 0.004;
/// Default Jacobi separation stiffness in `(0, 1]` for opt-in self-repelling start relaxation.
pub const DEFAULT_SELF_REPULSION_STIFFNESS: f64 = 0.5;
/// Default iteration stride for opt-in cloth–cloth separation impulses (divides [`RELAX_PUSH_OUT_ITERS`]).
pub const DEFAULT_SELF_REPULSION_PERIOD: usize = 5;

const MAX_SELF_REPULSION_CLEARANCE_M: f64 = 0.1;

/// Opt-in spatial-hash cloth–cloth separation impulses during [`relax_with_repulsion`] and
/// [`start_state_with_repulsion`].
///
/// Leaving this `None` preserves the reference's bit-identical relaxation trajectory.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelfRepulsionOptions {
    /// Minimum target 3D separation between non-adjacent cloth layers (m, in `(0, 0.1]`).
    pub min_separation_m: f64,
    /// Jacobi PBD separation stiffness in `(0, 1]`.
    pub stiffness: f64,
    /// Apply cloth–cloth separation impulses every `period` iterations (`1..=RELAX_PUSH_OUT_ITERS`).
    pub period: usize,
}

impl Default for SelfRepulsionOptions {
    fn default() -> Self {
        Self {
            min_separation_m: DEFAULT_SELF_REPULSION_CLEARANCE_M,
            stiffness: DEFAULT_SELF_REPULSION_STIFFNESS,
            period: DEFAULT_SELF_REPULSION_PERIOD,
        }
    }
}

impl SelfRepulsionOptions {
    /// Validates self-repulsion parameters fail-closed.
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] or [`RefusalCode::OutOfRange`] on non-finite or out-of-range
    /// clearance, stiffness, or iteration stride.
    pub fn validate(&self) -> Result<(), Refusal> {
        Refusal::finite("self-repulsion min_separation_m", self.min_separation_m)?;
        Refusal::finite("self-repulsion stiffness", self.stiffness)?;
        if !(self.min_separation_m > 0.0 && self.min_separation_m <= MAX_SELF_REPULSION_CLEARANCE_M)
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "self-repulsion min_separation_m {} must be in (0, {MAX_SELF_REPULSION_CLEARANCE_M}] m",
                    self.min_separation_m
                ),
            ));
        }
        if !(self.stiffness > 0.0 && self.stiffness <= 1.0) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "self-repulsion stiffness {} must be in (0, 1]",
                    self.stiffness
                ),
            ));
        }
        if !(1..=RELAX_PUSH_OUT_ITERS).contains(&self.period) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "self-repulsion period {} must be in 1..={RELAX_PUSH_OUT_ITERS}",
                    self.period
                ),
            ));
        }
        Ok(())
    }
}

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
    /// The part frame (wearer's right for legacy mirror pairs, selected side for physical panels).
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
    /// Keep a leg on its own side before any legacy mirroring.
    pub own_side: bool,
    /// When `own_side`, clamp x to ≥ +1 mm for an explicit physical left leg; otherwise ≤ −1 mm.
    pub own_side_left: bool,
}

/// The isometric wrap of one instance's 2D points (mm) into metres (spec 04 §3.3–3.4).
#[must_use]
pub fn wrap_points(points: &[Vec2], p: &WrapPlacement) -> Vec<Vec3> {
    wrap_drafted(&drafted(points, p), p)
        .into_iter()
        .map(|mut v| {
            if p.own_side {
                v.x = if p.own_side_left {
                    v.x.max(1.0)
                } else {
                    v.x.min(-1.0)
                };
            }
            if p.mirrored {
                v.x = -v.x;
            }
            v * MM
        })
        .collect()
}

/// The instance's points in the drafted (wearer's-right) frame: x mirrored for a mirrored instance.
pub(crate) fn drafted(points: &[Vec2], p: &WrapPlacement) -> Vec<Vec2> {
    points
        .iter()
        .map(|q| Vec2::new(if p.mirrored { -q.x } else { q.x }, q.y))
        .collect()
}

/// The reference's `body.wrap` of drafted-frame points, in mm (no own-side rule, no mirroring).
pub(crate) fn wrap_drafted(points: &[Vec2], p: &WrapPlacement) -> Vec<Vec3> {
    let f = &p.frame;
    let theta0 = p.theta_deg.to_radians();
    points
        .iter()
        .map(|q| {
            let h = p.h0 + q.y;
            let th = theta0 + (q.x - p.x0) / p.radius;
            let (s, c) = (ftx_core::det::sin(th), ftx_core::det::cos(th));
            let r = f.lat * s + f.front * c;
            f.origin + f.up * h + r * p.radius
        })
        .collect()
}

/// The isometric roll between the flat piece (k = 0, a sheet tangent to the wrap at the anchor column)
/// and its wrap (k = 1), in metres, with the own-side rule and mirroring at k ≥ 1 as the wrap (spec 04
/// §3.6; the animation's roll-up).
#[must_use]
pub fn roll_points(points: &[Vec2], p: &WrapPlacement, k: f64) -> Vec<Vec3> {
    if k >= 1.0 {
        return wrap_points(points, p);
    }
    let pts = drafted(points, p);
    let f = &p.frame;
    let kk = k.max(1e-6) / p.radius;
    let column: Vec<Vec2> = pts.iter().map(|q| Vec2::new(p.x0, q.y)).collect();
    let flat = wrap_drafted(&column, p);
    let (s0, c0) = ftx_core::det::sin_cos(p.theta_deg.to_radians());
    let tangent = f.lat * c0 - f.front * s0;
    let normal_in = -(f.lat * s0 + f.front * c0);
    pts.iter()
        .zip(flat)
        .map(|(q, base)| {
            let (s, c) = ftx_core::det::sin_cos(kk * (q.x - p.x0));
            let mut v = base + tangent * (s / kk) + normal_in * ((1.0 - c) / kk);
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
    gather_field(mesh, false).0
}

/// The cloth model's compression weight and floor per vertex when a gathered seam declares its own band of
/// pleats (`gather_relief_mm`, `gather_compression`; spec 04 §5.4, native extension), `None` when none does.
/// Within `gather_relief_mm` of such a seam's long side the weight stays 0, then rises to 1 over
/// [`GATHER_ZONE_MM`]; the floor is the seam's `gather_compression` wherever that seam sets the weight and
/// [`GATHER_COMPRESSION`] elsewhere. The start state keeps the reference's [`gather_weights`].
#[must_use]
pub fn gather_relief(mesh: &GarmentMesh) -> Option<(Vec<f64>, Vec<f64>)> {
    mesh.seams
        .iter()
        .any(|s| s.gather_relief.is_some())
        .then(|| gather_field(mesh, true))
}

/// The instances that hang from a gathered seam with a declared band of pleats: the owner of each such
/// seam's long side, sorted, without repeats. Empty when no seam declares a band.
#[must_use]
pub fn pleat_band_instances(mesh: &GarmentMesh) -> Vec<usize> {
    let mut instances: Vec<usize> = mesh
        .seams
        .iter()
        .filter(|s| s.gather_relief.is_some() && (s.ratio - 1.0).abs() > GATHER_RATIO_MARGIN)
        .filter_map(|s| long_side(mesh, s).first().map(|&v| mesh.owner[v]))
        .collect();
    instances.sort_unstable();
    instances.dedup();
    instances
}

/// The longer of a seam's two sides on the flat pattern, side a on a tie: the side the reference gathers.
fn long_side<'a>(mesh: &GarmentMesh, seam: &'a crate::garment::SeamRecord) -> &'a [usize] {
    let r2 = &mesh.verts2d;
    let length = |side: &[usize]| {
        let seg: Vec<f64> = side
            .windows(2)
            .map(|p| norm2(r2[p[1]] - r2[p[0]]))
            .collect();
        pairwise_sum(&seg)
    };
    let (la, lb) = (length(&seam.side_a), length(&seam.side_b));
    if la >= lb { &seam.side_a } else { &seam.side_b }
}

/// Weight and floor per vertex. With `declared` false every seam uses the reference's zone and floor.
fn gather_field(mesh: &GarmentMesh, declared: bool) -> (Vec<f64>, Vec<f64>) {
    let r2 = &mesh.verts2d;
    let mut w = vec![1.0_f64; r2.len()];
    let mut floor = vec![GATHER_COMPRESSION; r2.len()];
    for s in &mesh.seams {
        if (s.ratio - 1.0).abs() <= GATHER_RATIO_MARGIN {
            continue;
        }
        let (relief, low) = match s.gather_relief.filter(|_| declared) {
            Some(band) => (band.relief_mm, band.compression),
            None => (0.0, GATHER_COMPRESSION),
        };
        let long = long_side(mesh, s);
        let Some(&first) = long.first() else { continue };
        let (lo, hi) = mesh.inst_range[mesh.owner[first]];
        for i in lo..hi {
            let d = long
                .iter()
                .map(|&j| norm2(r2[i] - r2[j]))
                .fold(f64::INFINITY, f64::min);
            // With no relief this is the reference's `d / 50 mm` exactly (`d − 0` is `d`).
            let here = ((d - relief) / GATHER_ZONE_MM).clamp(0.0, 1.0);
            if here < w[i] {
                (w[i], floor[i]) = (here, low);
            }
        }
    }
    (w, floor)
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
    relax_inner(mesh, start, groups, body, iters, clearance, weights, None)
}

/// As [`relax`], with optional deterministic spatial-hash cloth–cloth separation impulses
/// ([`SelfRepulsionOptions`]) applied before body push-out to prevent layered and pleated starts
/// from self-intersecting.
///
/// When `repulsion` is `None`, the result is bit-identical to [`relax`].
///
/// # Errors
/// [`SelfRepulsionOptions::validate`] refusals for invalid repulsion settings.
#[allow(clippy::too_many_arguments)]
pub fn relax_with_repulsion(
    mesh: &GarmentMesh,
    start: &[Vec3],
    groups: &[Vec<usize>],
    body: &NearestTree,
    iters: usize,
    clearance: f64,
    weights: &[f64],
    repulsion: Option<SelfRepulsionOptions>,
) -> Result<Vec<Vec3>, Refusal> {
    if let Some(opts) = repulsion {
        opts.validate()?;
    }
    Ok(relax_inner(
        mesh, start, groups, body, iters, clearance, weights, repulsion,
    ))
}

/// Precomputed welded cloth topology used for deterministic vertex–triangle and edge–edge
/// repulsion during start-state relaxation.
struct RepulsionTopology {
    n_weld: usize,
    members: Vec<Vec<usize>>,
    adj: Vec<Vec<usize>>,
    tris: Vec<[usize; 3]>,
    tri_owner: Vec<usize>,
    tri_rest2d: Vec<[Vec2; 3]>,
    edges: Vec<[usize; 2]>,
    edge_rest2d: Vec<Vec<(usize, [Vec2; 2])>>,
    vert_owners: Vec<Vec<usize>>,
    cell_size: f64,
}

impl RepulsionTopology {
    fn build(mesh: &GarmentMesh, groups: &[Vec<usize>], min_sep: f64) -> Self {
        let n = mesh.verts2d.len();
        let mut weld = vec![usize::MAX; n];
        let mut members: Vec<Vec<usize>> = Vec::new();
        for g in groups {
            let id = members.len();
            let mut m = Vec::with_capacity(g.len());
            for &v in g {
                if v < n && weld[v] == usize::MAX {
                    weld[v] = id;
                    m.push(v);
                }
            }
            if !m.is_empty() {
                members.push(m);
            }
        }
        for (v, w) in weld.iter_mut().enumerate() {
            if *w == usize::MAX {
                *w = members.len();
                members.push(vec![v]);
            }
        }
        let n_weld = members.len();
        let mut adj = vec![Vec::new(); n_weld];
        let mut tris = Vec::with_capacity(mesh.faces.len());
        let mut tri_owner = Vec::with_capacity(mesh.faces.len());
        let mut tri_rest2d = Vec::with_capacity(mesh.faces.len());
        let mut edge_map: std::collections::BTreeMap<[usize; 2], Vec<(usize, [Vec2; 2])>> =
            std::collections::BTreeMap::new();
        let mut edge_sum = 0.0_f64;
        let mut edge_count = 0usize;
        for f in &mesh.faces {
            let t = f.map(|v| weld[v]);
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                continue;
            }
            let owner = mesh.owner[f[0]];
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let (ra, rb) = (mesh.verts2d[f[k]], mesh.verts2d[f[(k + 1) % 3]]);
                adj[a].push(b);
                adj[b].push(a);
                let (key, rest_pair) = if a < b {
                    ([a, b], [ra, rb])
                } else {
                    ([b, a], [rb, ra])
                };
                edge_map.entry(key).or_default().push((owner, rest_pair));
                edge_sum += norm2(rb - ra) * MM;
                edge_count += 1;
            }
            tris.push(t);
            tri_owner.push(owner);
            tri_rest2d.push(f.map(|v| mesh.verts2d[v]));
        }
        let mut edges = Vec::with_capacity(edge_map.len());
        let mut edge_rest2d = Vec::with_capacity(edge_map.len());
        for (key, uses) in edge_map {
            edges.push(key);
            edge_rest2d.push(uses);
        }
        for nbrs in &mut adj {
            nbrs.sort_unstable();
            nbrs.dedup();
        }
        let mut vert_owners = vec![Vec::new(); n_weld];
        for (w, ms) in members.iter().enumerate() {
            let o = &mut vert_owners[w];
            for &v in ms {
                o.push(mesh.owner[v]);
            }
            o.sort_unstable();
            o.dedup();
        }
        let mean_edge = if edge_count > 0 {
            (edge_sum / edge_count as f64).clamp(min_sep, 0.1)
        } else {
            min_sep
        };
        Self {
            n_weld,
            members,
            adj,
            tris,
            tri_owner,
            tri_rest2d,
            edges,
            edge_rest2d,
            vert_owners,
            cell_size: mean_edge.max(min_sep),
        }
    }

    fn excluded_edge(&self, ea: usize, eb: usize, rest_margin_mm: f64) -> bool {
        let (a, b) = (self.edges[ea], self.edges[eb]);
        if a[0] == b[0] || a[0] == b[1] || a[1] == b[0] || a[1] == b[1] {
            return true;
        }
        if a.iter()
            .any(|&u| b.iter().any(|w| self.adj[u].binary_search(w).is_ok()))
        {
            return true;
        }
        for &(oa, ra) in &self.edge_rest2d[ea] {
            for &(ob, rb) in &self.edge_rest2d[eb] {
                if oa == ob
                    && ra
                        .iter()
                        .any(|&pa| rb.iter().any(|&pb| norm2(pa - pb) < rest_margin_mm))
                {
                    return true;
                }
            }
        }
        false
    }

    fn excluded(
        &self,
        v: usize,
        tri_idx: usize,
        verts2d: &[Vec2],
        mesh_owner: &[usize],
        rest_margin_mm: f64,
    ) -> bool {
        let t = self.tris[tri_idx];
        if t.contains(&v) {
            return true;
        }
        let nbrs = &self.adj[v];
        if t.iter().any(|tv| nbrs.binary_search(tv).is_ok()) {
            return true;
        }
        let owner = self.tri_owner[tri_idx];
        if self.vert_owners[v].binary_search(&owner).is_ok() {
            let tri_r = self.tri_rest2d[tri_idx];
            for &mv in &self.members[v] {
                if mesh_owner[mv] != owner {
                    continue;
                }
                let pv = verts2d[mv];
                if tri_r.iter().any(|&q| norm2(pv - q) < rest_margin_mm) {
                    return true;
                }
            }
        }
        false
    }
}

fn cell_key_3d(p: Vec3, cell: f64) -> Option<[i64; 3]> {
    let mut k = [0i64; 3];
    for (dst, coord) in k.iter_mut().zip([p.x, p.y, p.z]) {
        let f = (coord / cell).floor();
        if !(f.is_finite() && (-1e15..=1e15).contains(&f)) {
            return None;
        }
        *dst = f as i64;
    }
    Some(k)
}

struct RepulsionHit {
    dist: f64,
    tri: usize,
    w: [f64; 3],
    dir: Vec3,
}

/// Applies one deterministic Jacobi spatial-hash vertex–triangle cloth–cloth separation sweep over
/// welded vertices, keeping sewing-group members identical, respecting the body surface as a rigid
/// unilateral floor (`clearance`), and bounding every displacement.
fn apply_self_repulsion(
    q: &mut [Vec3],
    verts2d: &[Vec2],
    mesh_owner: &[usize],
    topo: &RepulsionTopology,
    body: &NearestTree,
    clearance: f64,
    opts: SelfRepulsionOptions,
) {
    if topo.tris.is_empty() || topo.n_weld < 4 {
        return;
    }
    let d_min = opts.min_separation_m;
    let cell = topo.cell_size;
    let rest_margin_mm = (1.5 * d_min) / MM;

    let p: Vec<Vec3> = topo.members.iter().map(|m| q[m[0]]).collect();
    let body_floor: Vec<Option<Vec3>> = p
        .iter()
        .map(|&pv| {
            if !pv.is_finite() {
                return None;
            }
            let near = body.nearest(pv);
            let sd = (pv - near.point).dot(near.normal);
            (sd <= clearance + 1e-4).then_some(near.normal)
        })
        .collect();
    let pad = Vec3::new(d_min, d_min, d_min);
    let mut entries: Vec<([i64; 3], usize)> = Vec::with_capacity(topo.tris.len() * 4);
    let mut oversized: Vec<usize> = Vec::new();
    for (ti, &t) in topo.tris.iter().enumerate() {
        let (a, b, c) = (p[t[0]], p[t[1]], p[t[2]]);
        if !(a.is_finite() && b.is_finite() && c.is_finite()) {
            continue;
        }
        let lo = Vec3::new(
            a.x.min(b.x).min(c.x),
            a.y.min(b.y).min(c.y),
            a.z.min(b.z).min(c.z),
        ) - pad;
        let hi = Vec3::new(
            a.x.max(b.x).max(c.x),
            a.y.max(b.y).max(c.y),
            a.z.max(b.z).max(c.z),
        ) + pad;
        let (Some(k0), Some(k1)) = (cell_key_3d(lo, cell), cell_key_3d(hi, cell)) else {
            oversized.push(ti);
            continue;
        };
        let dx = k1[0].saturating_sub(k0[0]).saturating_add(1);
        let dy = k1[1].saturating_sub(k0[1]).saturating_add(1);
        let dz = k1[2].saturating_sub(k0[2]).saturating_add(1);
        if dx <= 0 || dy <= 0 || dz <= 0 || dx > 16 || dy > 16 || dz > 16 || dx * dy * dz > 64 {
            oversized.push(ti);
            continue;
        }
        for cx in k0[0]..=k1[0] {
            for cy in k0[1]..=k1[1] {
                for cz in k0[2]..=k1[2] {
                    entries.push(([cx, cy, cz], ti));
                }
            }
        }
    }
    entries.sort_unstable();
    let mut cells: Vec<([i64; 3], usize, usize)> = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        let key = entries[i].0;
        let start = i;
        while i < entries.len() && entries[i].0 == key {
            i += 1;
        }
        cells.push((key, start, i));
    }

    let mut acc = vec![Vec3::ZERO; topo.n_weld];
    let mut w_sum = vec![0.0_f64; topo.n_weld];
    for v in 0..topo.n_weld {
        let pv = p[v];
        if !pv.is_finite() {
            continue;
        }
        let cell_slice: &[([i64; 3], usize)] = cell_key_3d(pv, cell)
            .and_then(|key| cells.binary_search_by_key(&key, |c| c.0).ok())
            .map_or(&[], |idx| &entries[cells[idx].1..cells[idx].2]);

        // Collect all active (dist < d_min) triangle interactions for vertex `v`, then keep only
        // the closest opposing surface hit (plus at most one hit from the opposite side, for a
        // middle leaf in a multi-layer pleat). This prevents adjacent triangles of the same
        // opposing sheet from diluting the true penetration depth.
        let mut hits: Vec<RepulsionHit> = Vec::new();
        let mut check_tri = |ti: usize| {
            if topo.excluded(v, ti, verts2d, mesh_owner, rest_margin_mm) {
                return;
            }
            let t = topo.tris[ti];
            let (a, b, c) = (p[t[0]], p[t[1]], p[t[2]]);
            let (w, _) = point_triangle_closest(pv, a, b, c);
            let closest = a * w[0] + b * w[1] + c * w[2];
            let diff = pv - closest;
            let dist = norm3(diff);
            if !dist.is_finite() || dist >= d_min {
                return;
            }
            let dir = if dist > 1e-12 {
                diff / dist
            } else {
                let tri_n = (b - a).cross(c - a);
                let nlen = norm3(tri_n);
                let bn = body.nearest(pv).normal;
                let base = if nlen > 1e-12 {
                    tri_n / nlen
                } else {
                    let blen = norm3(bn);
                    if blen > 1e-12 {
                        bn / blen
                    } else {
                        Vec3::new(0.0, 0.0, 1.0)
                    }
                };
                // Align with the body's outward normal, then break strictly-coplanar symmetry
                // antisymmetrically by piece owner (or vertex index within the same piece) so
                // opposite (v, T) / (u, T') interactions reinforce instead of cancelling.
                let outward = if base.dot(bn) < 0.0 { -base } else { base };
                let v_owner = topo.vert_owners[v][0];
                let t_owner = topo.tri_owner[ti];
                let push_outward = if v_owner != t_owner {
                    v_owner > t_owner
                } else {
                    v > t[0]
                };
                if push_outward { outward } else { -outward }
            };
            hits.push(RepulsionHit {
                dist,
                tri: ti,
                w,
                dir,
            });
        };

        for &(_, ti) in cell_slice {
            check_tri(ti);
        }
        for &ti in &oversized {
            check_tri(ti);
        }
        if hits.is_empty() {
            continue;
        }
        hits.sort_by(|a, b| a.dist.total_cmp(&b.dist).then(a.tri.cmp(&b.tri)));
        let first = &hits[0];
        let opposite = hits[1..].iter().find(|h| h.dir.dot(first.dir) < -0.25);
        for hit in std::iter::once(first).chain(opposite) {
            let t = topo.tris[hit.tri];
            let pen = d_min - hit.dist;
            let mut inv_v = if body_floor[v].is_some_and(|bn| hit.dir.dot(bn) < -1e-6) {
                0.0
            } else {
                1.0
            };
            let mut inv_t = std::array::from_fn(|k| {
                if body_floor[t[k]].is_some_and(|bn| hit.dir.dot(bn) > 1e-6) {
                    0.0
                } else {
                    1.0
                }
            });
            let mut denom = inv_v
                + inv_t[0] * hit.w[0] * hit.w[0]
                + inv_t[1] * hit.w[1] * hit.w[1]
                + inv_t[2] * hit.w[2] * hit.w[2];
            if denom <= 1e-12 {
                inv_v = 1.0;
                inv_t = [1.0; 3];
                denom = 1.0 + hit.w[0] * hit.w[0] + hit.w[1] * hit.w[1] + hit.w[2] * hit.w[2];
            }
            let scale = opts.stiffness * pen / denom;
            if inv_v > 0.0 {
                acc[v] += hit.dir * (scale * inv_v);
                w_sum[v] += 1.0;
            }
            for k in 0..3 {
                if hit.w[k] > 0.0 && inv_t[k] > 0.0 {
                    acc[t[k]] -= hit.dir * (scale * inv_t[k] * hit.w[k]);
                    w_sum[t[k]] += hit.w[k];
                }
            }
        }
    }

    // Edge–edge spatial-hash repulsion catches overhanging border edges that pass within `d_min`
    // of an opposing cloth edge even when neither endpoint is close to an opposing triangle.
    let half_pad = Vec3::new(0.5 * d_min, 0.5 * d_min, 0.5 * d_min);
    let mut edge_entries: Vec<([i64; 3], usize)> = Vec::with_capacity(topo.edges.len() * 2);
    let mut oversized_edges: Vec<usize> = Vec::new();
    for (ei, &e) in topo.edges.iter().enumerate() {
        let (a, b) = (p[e[0]], p[e[1]]);
        if !(a.is_finite() && b.is_finite()) {
            continue;
        }
        let lo = Vec3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)) - half_pad;
        let hi = Vec3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z)) + half_pad;
        let (Some(k0), Some(k1)) = (cell_key_3d(lo, cell), cell_key_3d(hi, cell)) else {
            oversized_edges.push(ei);
            continue;
        };
        let dx = k1[0].saturating_sub(k0[0]).saturating_add(1);
        let dy = k1[1].saturating_sub(k0[1]).saturating_add(1);
        let dz = k1[2].saturating_sub(k0[2]).saturating_add(1);
        if dx <= 0 || dy <= 0 || dz <= 0 || dx > 16 || dy > 16 || dz > 16 || dx * dy * dz > 64 {
            oversized_edges.push(ei);
            continue;
        }
        for cx in k0[0]..=k1[0] {
            for cy in k0[1]..=k1[1] {
                for cz in k0[2]..=k1[2] {
                    edge_entries.push(([cx, cy, cz], ei));
                }
            }
        }
    }
    edge_entries.sort_unstable();
    let mut edge_pairs: Vec<(usize, usize)> = Vec::new();
    let mut j = 0;
    while j < edge_entries.len() {
        let key = edge_entries[j].0;
        let start = j;
        while j < edge_entries.len() && edge_entries[j].0 == key {
            j += 1;
        }
        let bucket = &edge_entries[start..j];
        for a_idx in 0..bucket.len() {
            let ea = bucket[a_idx].1;
            for &(_, eb) in &bucket[(a_idx + 1)..] {
                if ea != eb {
                    edge_pairs.push((ea.min(eb), ea.max(eb)));
                }
            }
        }
    }
    for &ea in &oversized_edges {
        for eb in 0..topo.edges.len() {
            if ea != eb {
                edge_pairs.push((ea.min(eb), ea.max(eb)));
            }
        }
    }
    edge_pairs.sort_unstable();
    edge_pairs.dedup();
    for (ea, eb) in edge_pairs {
        if topo.excluded_edge(ea, eb, rest_margin_mm) {
            continue;
        }
        let (a_ids, b_ids) = (topo.edges[ea], topo.edges[eb]);
        let (a0, a1, b0, b1) = (p[a_ids[0]], p[a_ids[1]], p[b_ids[0]], p[b_ids[1]]);
        if !(a0.is_finite() && a1.is_finite() && b0.is_finite() && b1.is_finite()) {
            continue;
        }
        let ((s, t), _) = edge_edge_closest(a0, a1, b0, b1);
        let qa = a0 * (1.0 - s) + a1 * s;
        let qb = b0 * (1.0 - t) + b1 * t;
        let diff = qb - qa;
        let dist = norm3(diff);
        if !dist.is_finite() || dist >= d_min {
            continue;
        }
        let dir = if dist > 1e-12 {
            diff / dist
        } else {
            let cross = (a1 - a0).cross(b1 - b0);
            let clen = norm3(cross);
            let bn = body.nearest(qa).normal;
            let base = if clen > 1e-12 {
                cross / clen
            } else {
                let blen = norm3(bn);
                if blen > 1e-12 {
                    bn / blen
                } else {
                    Vec3::new(0.0, 0.0, 1.0)
                }
            };
            let outward = if base.dot(bn) < 0.0 { -base } else { base };
            let oa = topo.edge_rest2d[ea][0].0;
            let ob = topo.edge_rest2d[eb][0].0;
            let push_b_outward = if ob != oa { ob > oa } else { eb > ea };
            if push_b_outward { outward } else { -outward }
        };
        // `dir` points from edge `a` toward edge `b`, so `b` moves along `+dir` and `a` along `-dir`.
        let wa = [1.0 - s, s];
        let wb = [1.0 - t, t];
        let mut inv_a = std::array::from_fn(|k| {
            if body_floor[a_ids[k]].is_some_and(|bn| dir.dot(bn) > 1e-6) {
                0.0
            } else {
                1.0
            }
        });
        let mut inv_b = std::array::from_fn(|k| {
            if body_floor[b_ids[k]].is_some_and(|bn| dir.dot(bn) < -1e-6) {
                0.0
            } else {
                1.0
            }
        });
        let mut denom = inv_a[0] * wa[0] * wa[0]
            + inv_a[1] * wa[1] * wa[1]
            + inv_b[0] * wb[0] * wb[0]
            + inv_b[1] * wb[1] * wb[1];
        if denom <= 1e-12 {
            inv_a = [1.0; 2];
            inv_b = [1.0; 2];
            denom = wa[0] * wa[0] + wa[1] * wa[1] + wb[0] * wb[0] + wb[1] * wb[1];
        }
        let scale = opts.stiffness * (d_min - dist) / denom;
        for k in 0..2 {
            if wa[k] > 0.0 && inv_a[k] > 0.0 {
                acc[a_ids[k]] -= dir * (scale * inv_a[k] * wa[k]);
                w_sum[a_ids[k]] += wa[k];
            }
            if wb[k] > 0.0 && inv_b[k] > 0.0 {
                acc[b_ids[k]] += dir * (scale * inv_b[k] * wb[k]);
                w_sum[b_ids[k]] += wb[k];
            }
        }
    }

    let max_step = 0.5 * d_min;
    for w in 0..topo.n_weld {
        if w_sum[w] <= 0.0 {
            continue;
        }
        let mut delta = acc[w] / w_sum[w].max(1.0);
        let len = norm3(delta);
        if !len.is_finite() || len == 0.0 {
            continue;
        }
        if len > max_step {
            delta = delta * (max_step / len);
        }
        for &mv in &topo.members[w] {
            q[mv] += delta;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn relax_inner(
    mesh: &GarmentMesh,
    start: &[Vec3],
    groups: &[Vec<usize>],
    body: &NearestTree,
    iters: usize,
    clearance: f64,
    weights: &[f64],
    repulsion: Option<SelfRepulsionOptions>,
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
    let topo = repulsion.map(|opts| RepulsionTopology::build(mesh, groups, opts.min_separation_m));
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
        if let (Some(opts), Some(topo)) = (repulsion, topo.as_ref())
            && (it % opts.period == opts.period - 1 || it + 1 == iters)
        {
            apply_self_repulsion(
                &mut q,
                &mesh.verts2d,
                &mesh.owner,
                topo,
                body,
                clearance,
                opts,
            );
        }
        if it % RELAX_PUSH_OUT_ITERS == RELAX_PUSH_OUT_ITERS - 1 || it + 1 == iters {
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
    start_state_inner(mesh, wrapped, body, None)
}

/// As [`start_state`], with optional spatial-hash cloth–cloth separation impulses during relaxation
/// ([`SelfRepulsionOptions`]). When `repulsion` is `None`, the result is bit-identical to
/// [`start_state`].
///
/// # Errors
/// [`SelfRepulsionOptions::validate`] refusals for invalid repulsion settings.
pub fn start_state_with_repulsion(
    mesh: &GarmentMesh,
    wrapped: Vec<Vec3>,
    body: &NearestTree,
    repulsion: Option<SelfRepulsionOptions>,
) -> Result<StartState, Refusal> {
    if let Some(opts) = repulsion {
        opts.validate()?;
    }
    Ok(start_state_inner(mesh, wrapped, body, repulsion))
}

fn start_state_inner(
    mesh: &GarmentMesh,
    wrapped: Vec<Vec3>,
    body: &NearestTree,
    repulsion: Option<SelfRepulsionOptions>,
) -> StartState {
    let weights = gather_weights(mesh);
    let (presewn, groups) = presew(mesh, &wrapped, body, PRESEW_CLEARANCE, PRESEW_ITERS);
    let edges = face_edges(&mesh.faces);
    let plain: Vec<(usize, usize)> = edges
        .into_iter()
        .filter(|&(a, b)| weights[a] >= 1.0 && weights[b] >= 1.0)
        .collect();
    let (mut q, mut done, mut blocks) = (presewn.clone(), 0, Vec::new());
    while done < RELAX_MAX_ITERS {
        q = relax_inner(
            mesh,
            &q,
            &groups,
            body,
            RELAX_ITERS,
            RELAX_CLEARANCE,
            &weights,
            repulsion,
        );
        done += RELAX_ITERS;
        let strain: Vec<f64> = edge_strain(&q, &mesh.verts2d, &plain)
            .into_iter()
            .map(f64::abs)
            .collect();
        let p95 = if strain.is_empty() {
            0.0
        } else {
            percentile(&strain, 95.0)
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::garment::build_garment;
    use ftx_pattern::Pattern;
    use serde_json::json;

    #[test]
    fn start_state_finishes_cleanly_when_every_edge_is_inside_gather_zone() {
        // Two 30 mm tall pieces joined by a 1.5x gathered seam: every vertex is within 50 mm
        // (GATHER_ZONE_MM) of the gathered seam, leaving no plain edges on the long piece.
        // Putting both pieces inside the gather zone exercises empty `plain` without NaN p95.
        let piece = |id: &str, w: f64| {
            json!({
                "id": id, "cut": {"qty": 1}, "grain": [[0, 0], [0, 20]],
                "vertices": {"a": [0, 0], "b": [w, 0], "c": [w, 25], "d": [0, 25]},
                "edges": [
                    {"id": "bottom", "from": "a", "to": "b"},
                    {"id": "right", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"},
                    {"id": "left", "from": "d", "to": "a"}
                ],
                "placement": {"part": "torso"}
            })
        };
        let pat = Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [piece("ruffle", 45.0)],
            "seams": [{"id": "self_gather", "a": "ruffle/top", "b": "ruffle/bottom", "ratio": 1.5}]
        }));
        let mesh = build_garment(&pat, 15.0).unwrap();
        let weights = gather_weights(&mesh);
        assert!(weights.iter().all(|&w| w < 1.0));
        let wrapped: Vec<Vec3> = mesh
            .verts2d
            .iter()
            .map(|p| Vec3::new(p.x * MM, p.y * MM, 0.2))
            .collect();
        let body = NearestTree::new(
            vec![
                Vec3::new(-1.0, -1.0, 0.0),
                Vec3::new(2.0, -1.0, 0.0),
                Vec3::new(-1.0, 2.0, 0.0),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap();
        let st = start_state(&mesh, wrapped, &body);
        assert_eq!(st.relax_iterations, RELAX_ITERS);
        assert_eq!(st.block_p95, vec![0.0]);
        assert!(st.relaxed.iter().all(|p| p.is_finite()));
    }

    fn flat_body() -> NearestTree {
        NearestTree::new(
            vec![
                Vec3::new(-1.0, -1.0, 0.0),
                Vec3::new(2.0, -1.0, 0.0),
                Vec3::new(-1.0, 2.0, 0.0),
                Vec3::new(2.0, 2.0, 0.0),
            ],
            vec![[0, 1, 2], [1, 3, 2]],
        )
        .unwrap()
    }

    fn panel_piece(id: &str, w: f64, h: f64) -> serde_json::Value {
        json!({
            "id": id, "cut": {"qty": 1}, "grain": [[0, 0], [0, 20]],
            "vertices": {"a": [0, 0], "b": [w, 0], "c": [w, h], "d": [0, h]},
            "edges": [
                {"id": "bottom", "from": "a", "to": "b"},
                {"id": "right", "from": "b", "to": "c"},
                {"id": "top", "from": "c", "to": "d"},
                {"id": "left", "from": "d", "to": "a"}
            ],
            "placement": {"part": "torso"}
        })
    }

    #[test]
    fn self_repulsion_validation_legacy_parity_and_checkpoint_replay() {
        let pat = Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [panel_piece("inner", 60.0, 60.0), panel_piece("outer", 60.0, 60.0)],
            "seams": []
        }));
        let mesh = build_garment(&pat, 15.0).unwrap();
        let weights = gather_weights(&mesh);
        let body = flat_body();
        let mut start = vec![Vec3::ZERO; mesh.verts2d.len()];
        let (lo0, hi0) = mesh.inst_range[0];
        let (lo1, hi1) = mesh.inst_range[1];
        for i in lo0..hi0 {
            let p = mesh.verts2d[i];
            start[i] = Vec3::new(p.x * MM, p.y * MM, RELAX_CLEARANCE);
        }
        for i in lo1..hi1 {
            let p = mesh.verts2d[i];
            start[i] = Vec3::new(p.x * MM + 0.004, p.y * MM + 0.003, RELAX_CLEARANCE + 0.0006);
        }

        // None option is bit-identical to legacy relax and start_state.
        let legacy = relax(
            &mesh,
            &start,
            &[],
            &body,
            RELAX_ITERS,
            RELAX_CLEARANCE,
            &weights,
        );
        let none_rep = relax_with_repulsion(
            &mesh,
            &start,
            &[],
            &body,
            RELAX_ITERS,
            RELAX_CLEARANCE,
            &weights,
            None,
        )
        .unwrap();
        assert_eq!(legacy, none_rep);

        let st_legacy = start_state(&mesh, start.clone(), &body);
        let st_none = start_state_with_repulsion(&mesh, start.clone(), &body, None).unwrap();
        assert_eq!(st_legacy.relaxed, st_none.relaxed);
        assert_eq!(st_legacy.block_p95, st_none.block_p95);

        // Push-out checkpoint replay is bit-identical when period divides RELAX_PUSH_OUT_ITERS.
        let opts = SelfRepulsionOptions::default();
        let whole = relax_with_repulsion(
            &mesh,
            &start,
            &[],
            &body,
            RELAX_ITERS,
            RELAX_CLEARANCE,
            &weights,
            Some(opts),
        )
        .unwrap();
        let mut chunked = start.clone();
        for _ in 0..(RELAX_ITERS / RELAX_PUSH_OUT_ITERS) {
            chunked = relax_with_repulsion(
                &mesh,
                &chunked,
                &[],
                &body,
                RELAX_PUSH_OUT_ITERS,
                RELAX_CLEARANCE,
                &weights,
                Some(opts),
            )
            .unwrap();
        }
        for (a, b) in whole.iter().zip(&chunked) {
            assert_eq!(
                [a.x.to_bits(), a.y.to_bits(), a.z.to_bits()],
                [b.x.to_bits(), b.y.to_bits(), b.z.to_bits()],
                "push-out checkpoint replay must stay bit-identical"
            );
        }

        // Serde round-trip and fail-closed validation.
        let wire = serde_json::to_value(opts).unwrap();
        assert_eq!(
            serde_json::from_value::<SelfRepulsionOptions>(wire).unwrap(),
            opts
        );
        assert!(serde_json::from_value::<SelfRepulsionOptions>(json!({"bogus": 1})).is_err());
        for (bad, code) in [
            (
                SelfRepulsionOptions {
                    min_separation_m: 0.0,
                    ..opts
                },
                RefusalCode::OutOfRange,
            ),
            (
                SelfRepulsionOptions {
                    min_separation_m: 0.25,
                    ..opts
                },
                RefusalCode::OutOfRange,
            ),
            (
                SelfRepulsionOptions {
                    min_separation_m: f64::NAN,
                    ..opts
                },
                RefusalCode::NonFinite,
            ),
            (
                SelfRepulsionOptions {
                    stiffness: 0.0,
                    ..opts
                },
                RefusalCode::OutOfRange,
            ),
            (
                SelfRepulsionOptions {
                    stiffness: 1.5,
                    ..opts
                },
                RefusalCode::OutOfRange,
            ),
            (
                SelfRepulsionOptions { period: 0, ..opts },
                RefusalCode::OutOfRange,
            ),
            (
                SelfRepulsionOptions {
                    period: RELAX_PUSH_OUT_ITERS + 1,
                    ..opts
                },
                RefusalCode::OutOfRange,
            ),
        ] {
            let err = relax_with_repulsion(
                &mesh,
                &start,
                &[],
                &body,
                10,
                RELAX_CLEARANCE,
                &weights,
                Some(bad),
            )
            .unwrap_err();
            assert_eq!(err.code, code, "bad={bad:?} err={err:?}");
        }
    }

    #[test]
    fn self_repelling_relax_separates_staggered_layers_and_passes_self_contact_audit() {
        let pat = Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [panel_piece("lining", 60.0, 60.0), panel_piece("shell", 60.0, 60.0)],
            "seams": []
        }));
        let mesh = build_garment(&pat, 15.0).unwrap();
        let weights = gather_weights(&mesh);
        let body = flat_body();
        // Place two staggered layers tightly over the body (only 0.4 mm apart, well below the
        // 1.5 mm self-contact thickness) so legacy relaxation leaves them overlapping/touching.
        let mut start = vec![Vec3::ZERO; mesh.verts2d.len()];
        let (lo0, hi0) = mesh.inst_range[0];
        let (lo1, hi1) = mesh.inst_range[1];
        for i in lo0..hi0 {
            let p = mesh.verts2d[i];
            start[i] = Vec3::new(p.x * MM, p.y * MM, RELAX_CLEARANCE - 0.001);
        }
        for i in lo1..hi1 {
            let p = mesh.verts2d[i];
            start[i] = Vec3::new(p.x * MM + 0.005, p.y * MM + 0.004, RELAX_CLEARANCE - 0.0006);
        }

        let legacy = relax(
            &mesh,
            &start,
            &[],
            &body,
            RELAX_ITERS,
            RELAX_CLEARANCE,
            &weights,
        );
        let repelled = relax_with_repulsion(
            &mesh,
            &start,
            &[],
            &body,
            RELAX_ITERS,
            RELAX_CLEARANCE,
            &weights,
            Some(SelfRepulsionOptions::default()),
        )
        .unwrap();

        // Body-outside projection invariant holds for every vertex.
        assert!(
            repelled.iter().all(|p| p.z >= RELAX_CLEARANCE - 1e-12),
            "repulsion must never push cloth inside body clearance"
        );

        // Plain-edge strain stays converged.
        let plain = face_edges(&mesh.faces);
        let strain: Vec<f64> = edge_strain(&repelled, &mesh.verts2d, &plain)
            .into_iter()
            .map(f64::abs)
            .collect();
        let p95 = percentile(&strain, 95.0);
        assert!(
            p95 <= RELAX_TARGET_P95,
            "repelled layers must remain relaxed (p95={p95})"
        );

        // Verify with the production self-contact audit: legacy fails IntersectingStart while
        // self-repelling relaxation passes with clear layer separation!
        let self_params = crate::contact::ContactParams {
            dhat: 1e-3,
            thickness: 1.5e-3,
            kappa: 100.0,
            mu: 0.3,
            eps_v: 1e-3,
        };
        let make_contact = |x: &[Vec3]| {
            crate::contact::Contact::new(
                crate::contact::Body::empty(),
                self_params,
                mesh.faces.clone(),
                |a, b| norm2(mesh.verts2d[a] - mesh.verts2d[b]) * MM,
            )
            .with_self_contact(x, self_params)
        };
        assert_eq!(
            make_contact(&legacy).unwrap_err().code,
            RefusalCode::IntersectingStart,
            "legacy relax collapses close layers onto the same body clearance plane"
        );
        let audited = make_contact(&repelled)
            .expect("self-repelling relax must produce a self-contact-clear layered start");
        let gap = audited.min_self_gap(&repelled, 0.02).unwrap();
        assert!(
            gap > 0.001,
            "repelled layers must keep positive self-contact gap beyond thickness (gap={gap} m)"
        );
    }

    #[test]
    fn self_repelling_relax_separates_single_piece_pleat_fold_deterministically() {
        // One continuous strip folded back onto itself in an S-pleat over the flat body.
        let pat = Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [panel_piece("pleat", 120.0, 40.0)],
            "seams": []
        }));
        let mesh = build_garment(&pat, 15.0).unwrap();
        let weights = gather_weights(&mesh);
        let body = flat_body();
        // Fold x ∈ [0, 40] -> [0, 0.04], x ∈ [40, 80] folds back to [0.04, 0.0] just 0.5 mm above,
        // x ∈ [80, 120] folds forward to [0.0, 0.04] just 1.0 mm above.
        let mut start = Vec::with_capacity(mesh.verts2d.len());
        for p in &mesh.verts2d {
            let (x3, z3) = if p.x <= 40.0 {
                (p.x * MM, RELAX_CLEARANCE)
            } else if p.x <= 80.0 {
                ((80.0 - p.x) * MM, RELAX_CLEARANCE + 0.0005)
            } else {
                ((p.x - 80.0) * MM, RELAX_CLEARANCE + 0.001)
            };
            start.push(Vec3::new(x3, p.y * MM, z3));
        }
        let opts = SelfRepulsionOptions {
            min_separation_m: 0.0035,
            stiffness: 0.6,
            period: 5,
        };
        let rep1 = relax_with_repulsion(
            &mesh,
            &start,
            &[],
            &body,
            RELAX_ITERS,
            RELAX_CLEARANCE,
            &weights,
            Some(opts),
        )
        .unwrap();
        let rep2 = relax_with_repulsion(
            &mesh,
            &start,
            &[],
            &body,
            RELAX_ITERS,
            RELAX_CLEARANCE,
            &weights,
            Some(opts),
        )
        .unwrap();
        assert_eq!(rep1, rep2, "self-repulsion must be bit-deterministic");
        assert!(rep1.iter().all(|p| p.z >= RELAX_CLEARANCE - 1e-12));

        // Mean height of the upper pleat leaf (flat x > 95 mm) must rise clearly above the lower
        // leaf (flat x < 25 mm) instead of remaining compressed within 1 mm.
        let mean_z = |pred: &dyn Fn(f64) -> bool| -> f64 {
            let zs: Vec<f64> = mesh
                .verts2d
                .iter()
                .zip(&rep1)
                .filter(|(r, _)| pred(r.x))
                .map(|(_, p)| p.z)
                .collect();
            zs.iter().sum::<f64>() / zs.len() as f64
        };
        let z_low = mean_z(&|x| x < 25.0);
        let z_high = mean_z(&|x| x > 95.0);
        assert!(
            z_high - z_low > 0.003,
            "pleat leaves must separate vertically: z_low={z_low} z_high={z_high}"
        );
    }

    #[test]
    fn strictly_coplanar_layers_separate_cleanly_without_symmetric_cancellation() {
        let pat = Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [panel_piece("lining", 60.0, 60.0), panel_piece("shell", 60.0, 60.0)],
            "seams": []
        }));
        let mesh = build_garment(&pat, 15.0).unwrap();
        let weights = gather_weights(&mesh);
        let body = flat_body();
        let mut start = vec![Vec3::ZERO; mesh.verts2d.len()];
        let (lo0, hi0) = mesh.inst_range[0];
        let (lo1, hi1) = mesh.inst_range[1];
        for i in lo0..hi0 {
            let p = mesh.verts2d[i];
            start[i] = Vec3::new(p.x * MM, p.y * MM, RELAX_CLEARANCE);
        }
        for i in lo1..hi1 {
            let p = mesh.verts2d[i];
            start[i] = Vec3::new(p.x * MM + 0.005, p.y * MM + 0.004, RELAX_CLEARANCE);
        }
        let repelled = relax_with_repulsion(
            &mesh,
            &start,
            &[],
            &body,
            RELAX_ITERS,
            RELAX_CLEARANCE,
            &weights,
            Some(SelfRepulsionOptions::default()),
        )
        .unwrap();
        assert!(repelled.iter().all(|p| p.z >= RELAX_CLEARANCE - 1e-12));
        let mean_z = |lo: usize, hi: usize| {
            repelled[lo..hi].iter().map(|p| p.z).sum::<f64>() / (hi - lo) as f64
        };
        let z0 = mean_z(lo0, hi0);
        let z1 = mean_z(lo1, hi1);
        assert!(
            z1 - z0 > 0.003,
            "strictly coplanar shell must separate above lining: z0={z0} z1={z1}"
        );
    }
}
