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

use crate::bvh::QueryBvh;
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
/// Default iteration stride for opt-in separation impulses (divides [`RELAX_PUSH_OUT_ITERS`]).
pub const DEFAULT_SELF_REPULSION_PERIOD: usize = 5;
/// Default complete-call work bound for cloth–cloth and body-feature start separation.
pub const DEFAULT_SELF_REPULSION_CANDIDATE_CHECKS: usize = 32_000_000;

const MAX_SELF_REPULSION_CLEARANCE_M: f64 = 0.1;
const MAX_SELF_REPULSION_CANDIDATE_CHECKS: usize = 128_000_000;

/// Opt-in bounded cloth–cloth and body-feature separation impulses during [`relax_with_repulsion`]
/// and [`start_state_with_repulsion`]. Body features use the caller's body clearance.
///
/// Leaving this `None` preserves the reference's bit-identical relaxation trajectory.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelfRepulsionOptions {
    /// Minimum target 3D separation between non-adjacent cloth layers (m, in `(0, 0.1]`).
    pub min_separation_m: f64,
    /// Jacobi PBD separation stiffness in `(0, 1]`.
    pub stiffness: f64,
    /// Apply cloth–cloth and body-feature impulses every `period` iterations (`1..=RELAX_PUSH_OUT_ITERS`).
    pub period: usize,
    /// Broad-phase candidates examined over a complete relaxation call (`1..=128000000`).
    /// All primary and recovery blocks of an ordinary scene share one allowance. Standalone
    /// [`relax_with_repulsion`] calls each have their own. Exhaustion returns no partial start.
    #[serde(skip_serializing_if = "default_repulsion_candidate_checks")]
    pub max_candidate_checks: usize,
}

impl Default for SelfRepulsionOptions {
    fn default() -> Self {
        Self {
            min_separation_m: DEFAULT_SELF_REPULSION_CLEARANCE_M,
            stiffness: DEFAULT_SELF_REPULSION_STIFFNESS,
            period: DEFAULT_SELF_REPULSION_PERIOD,
            max_candidate_checks: DEFAULT_SELF_REPULSION_CANDIDATE_CHECKS,
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
        if !(1..=MAX_SELF_REPULSION_CANDIDATE_CHECKS).contains(&self.max_candidate_checks) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "self-repulsion max_candidate_checks must be in 1..={MAX_SELF_REPULSION_CANDIDATE_CHECKS}"
                ),
            ));
        }
        Ok(())
    }
}

fn default_repulsion_candidate_checks(value: &usize) -> bool {
    *value == DEFAULT_SELF_REPULSION_CANDIDATE_CHECKS
}

struct RepulsionWork {
    remaining: usize,
    limit: usize,
}

impl RepulsionWork {
    fn new(options: SelfRepulsionOptions) -> Self {
        Self {
            remaining: options.max_candidate_checks,
            limit: options.max_candidate_checks,
        }
    }

    fn spend(&mut self, count: usize) -> Result<(), Refusal> {
        self.remaining = self.remaining.checked_sub(count).ok_or_else(|| {
            Refusal::new(
                RefusalCode::BudgetExceeded,
                format!("self-repelling start exceeded {} candidate checks", self.limit),
            ).repair("Use a coarser mesh, separate the starting layers, or increase max_candidate_checks within its supported range.")
        })?;
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

/// Unit normal from a face cross product without squaring underflow/overflow or silently
/// turning a collapsed cross product into a zero-vector impulse.
fn unit_normal(cross: Vec3, what: &str) -> Result<Option<Vec3>, Refusal> {
    if !cross.is_finite() {
        return Err(Refusal::new(RefusalCode::NonFinite, what));
    }
    let len = norm3(cross);
    if len > 0.0 && len.is_finite() {
        return Ok(Some(cross / len));
    }
    let scale = cross.x.abs().max(cross.y.abs()).max(cross.z.abs());
    if scale == 0.0 {
        return Ok(None);
    }
    let scaled = cross / scale;
    let scaled_len = norm3(scaled);
    if !scaled_len.is_finite() || scaled_len == 0.0 {
        return Err(Refusal::new(RefusalCode::NonFinite, what));
    }
    Ok(Some(scaled / scaled_len))
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
        .expect("legacy relaxation has no fallible self-repulsion work")
}

/// As [`relax`], with optional deterministic cloth–cloth and two-way body-feature separation
/// ([`SelfRepulsionOptions`]) before vertex push-out. Interior face crossings participate even when
/// every cloth vertex is outside. This bounded heuristic still requires exact scene admission.
///
/// When `repulsion` is `None`, the result is bit-identical to [`relax`].
///
/// # Errors
/// Invalid repulsion settings or body geometry, non-finite separation geometry, negative body
/// clearance, or exhausted candidate work.
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
    StartRelaxation::new(repulsion)?.relax(mesh, start, groups, body, iters, clearance, weights)
}

/// One scene's immutable welded/body feature queries and monotone candidate allowance. Primary
/// relaxation and all fallback replays use the same mesh, sewing groups and rigid body. Explicit
/// checkpoint admission performs no relaxation and cannot mint another allowance.
pub(crate) struct StartRelaxation {
    options: Option<SelfRepulsionOptions>,
    prepared: Option<PreparedRepulsion>,
}

struct PreparedRepulsion {
    cloth: RepulsionTopology,
    body: BodyRepulsion,
    work: RepulsionWork,
}

impl StartRelaxation {
    pub(crate) fn new(options: Option<SelfRepulsionOptions>) -> Result<Self, Refusal> {
        if let Some(options) = options {
            options.validate()?;
        }
        Ok(Self {
            options,
            prepared: None,
        })
    }

    pub(crate) fn options(&self) -> Option<SelfRepulsionOptions> {
        self.options
    }

    #[cfg(test)]
    pub(crate) fn candidate_checks(&self) -> usize {
        self.prepared
            .as_ref()
            .map_or(0, |prepared| prepared.work.limit - prepared.work.remaining)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn relax(
        &mut self,
        mesh: &GarmentMesh,
        start: &[Vec3],
        groups: &[Vec<usize>],
        body: &NearestTree,
        iters: usize,
        clearance: f64,
        weights: &[f64],
    ) -> Result<Vec<Vec3>, Refusal> {
        if self.options.is_some() && start.iter().any(|p| !p.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "non-finite self-repulsion start",
            ));
        }
        let prepared = if let Some(options) = self.options {
            if self.prepared.is_none() {
                self.prepared = Some(PreparedRepulsion {
                    cloth: RepulsionTopology::build(mesh, groups),
                    body: BodyRepulsion::new(body)?,
                    work: RepulsionWork::new(options),
                });
            }
            Some((options, self.prepared.as_mut().expect("prepared repulsion")))
        } else {
            None
        };
        relax_inner(
            mesh, start, groups, body, iters, clearance, weights, prepared,
        )
    }
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
}

impl RepulsionTopology {
    fn build(mesh: &GarmentMesh, groups: &[Vec<usize>]) -> Self {
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

struct RepulsionHit {
    dist: f64,
    tri: usize,
    w: [f64; 3],
    dir: Vec3,
}

/// Applies one deterministic Jacobi vertex–triangle/edge–edge cloth separation sweep over
/// welded vertices, keeping sewing-group members identical, respecting the body surface as a rigid
/// unilateral floor (`clearance`), and bounding every displacement.
#[allow(clippy::too_many_arguments)]
fn apply_self_repulsion(
    q: &mut [Vec3],
    verts2d: &[Vec2],
    mesh_owner: &[usize],
    topo: &RepulsionTopology,
    body: &NearestTree,
    clearance: f64,
    opts: SelfRepulsionOptions,
    work: &mut RepulsionWork,
) -> Result<(), Refusal> {
    if topo.tris.is_empty() || topo.n_weld < 4 {
        return Ok(());
    }
    let d_min = opts.min_separation_m;
    let rest_margin_mm = (1.5 * d_min) / MM;

    let p: Vec<Vec3> = topo.members.iter().map(|m| q[m[0]]).collect();
    if p.iter().any(|v| !v.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "non-finite self-repulsion positions",
        ));
    }
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
    let triangle_boxes: Vec<_> = topo
        .tris
        .iter()
        .map(|t| crate::contact::aabb(&t.map(|v| p[v])))
        .collect();
    let triangle_query = QueryBvh::build(&triangle_boxes);
    let mut candidates = Vec::new();

    let mut acc = vec![Vec3::ZERO; topo.n_weld];
    let mut w_sum = vec![0.0_f64; topo.n_weld];
    for v in 0..topo.n_weld {
        let pv = p[v];
        triangle_query.query((pv - pad, pv + pad), &mut candidates);
        work.spend(candidates.len())?;

        // Collect all active (dist < d_min) triangle interactions for vertex `v`, then keep only
        // the closest opposing surface hit (plus at most one hit from the opposite side, for a
        // middle leaf in a multi-layer pleat). This prevents adjacent triangles of the same
        // opposing sheet from diluting the true penetration depth.
        let mut hits: Vec<RepulsionHit> = Vec::new();
        let mut check_tri = |ti: usize| -> Result<(), Refusal> {
            if topo.excluded(v, ti, verts2d, mesh_owner, rest_margin_mm) {
                return Ok(());
            }
            let t = topo.tris[ti];
            let (a, b, c) = (p[t[0]], p[t[1]], p[t[2]]);
            let (w, _) = point_triangle_closest(pv, a, b, c);
            let closest = a * w[0] + b * w[1] + c * w[2];
            let diff = pv - closest;
            let dist = norm3(diff);
            if !dist.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "non-finite self-repulsion vertex–triangle distance",
                ));
            }
            if dist >= d_min {
                return Ok(());
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
            Ok(())
        };

        for &ti in &candidates {
            check_tri(ti)?;
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

    // Visit each unordered pair once in the same sorted order as the former materialized list.
    // A query retains at most O(E) leaf candidates, even when every layer overlaps. There is no
    // O(E²) pair vector or Cartesian-product fallback for long edges.
    let edge_boxes: Vec<_> = topo
        .edges
        .iter()
        .map(|e| crate::contact::aabb(&e.map(|v| p[v])))
        .collect();
    let edge_query = QueryBvh::build(&edge_boxes);
    for (ea, &(lo, hi)) in edge_boxes.iter().enumerate() {
        edge_query.query((lo - pad, hi + pad), &mut candidates);
        work.spend(candidates.len())?;
        for &eb in &candidates {
            if eb <= ea {
                continue;
            }
            if topo.excluded_edge(ea, eb, rest_margin_mm) {
                continue;
            }
            let (a_ids, b_ids) = (topo.edges[ea], topo.edges[eb]);
            let (a0, a1, b0, b1) = (p[a_ids[0]], p[a_ids[1]], p[b_ids[0]], p[b_ids[1]]);
            let ((s, t), _) = edge_edge_closest(a0, a1, b0, b1);
            let qa = a0 * (1.0 - s) + a1 * s;
            let qb = b0 * (1.0 - t) + b1 * t;
            let diff = qb - qa;
            let dist = norm3(diff);
            if !dist.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "non-finite self-repulsion edge–edge distance",
                ));
            }
            if dist >= d_min {
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
    }

    // A segment can pass through the interior of another panel without an endpoint or either
    // boundary edge inside d_min. Resolve proper piercings before proximity on the affected
    // vertices: unsigned proximity alone pushes an already pierced endpoint the wrong way.
    let mut crossing_acc = vec![Vec3::ZERO; topo.n_weld];
    let mut crossing_weight = vec![0.0; topo.n_weld];
    for (ei, &bounds) in edge_boxes.iter().enumerate() {
        let edge = topo.edges[ei];
        triangle_query.query(bounds, &mut candidates);
        work.spend(candidates.len())?;
        for &ti in &candidates {
            if !crate::bvh::overlaps(bounds, triangle_boxes[ti])
                || edge
                    .iter()
                    .any(|&v| topo.excluded(v, ti, verts2d, mesh_owner, rest_margin_mm))
            {
                continue;
            }
            let face = topo.tris[ti];
            let triangle = face.map(|v| p[v]);
            let segment = edge.map(|v| p[v]);
            let cross = (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]);
            let raw_signed = segment.map(|v| (v - triangle[0]).dot(cross));
            if !cross.is_finite() || !raw_signed[0].is_finite() || !raw_signed[1].is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "non-finite self-repulsion crossing witness",
                ));
            }
            if ftx_core::intersection::segment_triangle(segment[0], segment[1], triangle)
                != Some(ftx_core::intersection::IntersectionKind::Crossing)
            {
                continue;
            }
            // Keep the farther endpoint on its current side and move the shallower endpoint
            // across the face. On an exact tie the ordered welded edge breaks symmetry. The
            // direction is invariant to triangle winding and independent of garment part names.
            let Some(normal) = unit_normal(cross, "non-finite self-repulsion crossing normal")?
            else {
                continue;
            };
            let signed = segment.map(|v| (v - triangle[0]).dot(normal));
            if !signed[0].is_finite() || !signed[1].is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "non-finite self-repulsion crossing clearance",
                ));
            }
            let tip = usize::from(signed[1].abs() < signed[0].abs());
            let v = edge[tip];
            let dir = if signed[1 - tip] >= 0.0 {
                normal
            } else {
                -normal
            };
            // Linearize the signed normal clearance at the tip with the opposing face lagged.
            // Nearest face weights stay nonnegative even if the projected tip is outside it.
            let (shape, _) = point_triangle_closest(p[v], triangle[0], triangle[1], triangle[2]);
            let tip_free = body_floor[v].is_none_or(|n| dir.dot(n) >= -1e-6);
            let face_shape = std::array::from_fn::<_, 3, _>(|k| {
                if body_floor[face[k]].is_some_and(|n| dir.dot(n) > 1e-6) {
                    0.0
                } else {
                    shape[k]
                }
            });
            let denom =
                f64::from(u8::from(tip_free)) + face_shape.iter().map(|w| w * w).sum::<f64>();
            if denom <= 1e-12 {
                continue;
            }
            let delta = dir * (opts.stiffness * (d_min + signed[tip].abs()) / denom);
            if tip_free {
                crossing_acc[v] += delta;
                crossing_weight[v] += 1.0;
            }
            for k in 0..3 {
                crossing_acc[face[k]] -= delta * face_shape[k];
                crossing_weight[face[k]] += face_shape[k];
            }
        }
    }

    let max_step = 0.5 * d_min;
    for w in 0..topo.n_weld {
        if crossing_weight[w] > 0.0 {
            acc[w] = crossing_acc[w];
            w_sum[w] = crossing_weight[w];
        }
        if w_sum[w] <= 0.0 {
            continue;
        }
        let mut delta = acc[w] / w_sum[w].max(1.0);
        let len = norm3(delta);
        if !len.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "non-finite self-repulsion correction",
            ));
        }
        if len == 0.0 {
            continue;
        }
        if len > max_step {
            delta = delta * (max_step / len);
        }
        for &mv in &topo.members[w] {
            q[mv] += delta;
        }
    }
    if q.iter().any(|p| !p.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "non-finite repelling start",
        ));
    }
    Ok(())
}

/// Immutable body feature queries for the opt-in start heuristic. The final contact audit remains
/// authoritative; this only lets relaxation react before a ridge has passed through a cloth face.
struct BodyRepulsion {
    edges: Vec<[usize; 2]>,
    triangle_query: QueryBvh,
    edge_query: QueryBvh,
}

impl BodyRepulsion {
    fn new(body: &NearestTree) -> Result<Self, Refusal> {
        let (vertices, triangles) = body.surface();
        ftx_core::intersection::validate_mesh(vertices, triangles)?;
        let edges: Vec<_> = face_edges(triangles)
            .into_iter()
            .map(|(a, b)| [a, b])
            .collect();
        let triangle_boxes: Vec<_> = triangles
            .iter()
            .map(|t| crate::contact::aabb(&t.map(|v| vertices[v])))
            .collect();
        let edge_boxes: Vec<_> = edges
            .iter()
            .map(|e| crate::contact::aabb(&e.map(|v| vertices[v])))
            .collect();
        Ok(Self {
            edges,
            triangle_query: QueryBvh::build(&triangle_boxes),
            edge_query: QueryBvh::build(&edge_boxes),
        })
    }
}

struct EdgeFaceWitness {
    edge_weights: [f64; 2],
    triangle_weights: [f64; 3],
    edge_point: Vec3,
    triangle_point: Vec3,
    distance: f64,
}

/// Endpoints and edge pairs cover separated features, but not a segment piercing a face's interior.
/// The exact predicate admits that missing case before its floating-point witness is reconstructed.
fn edge_face_witness(edge: [Vec3; 2], triangle: [Vec3; 3]) -> Result<EdgeFaceWitness, Refusal> {
    let at = |edge_weights: [f64; 2], triangle_weights: [f64; 3]| {
        let edge_point = edge[0] * edge_weights[0] + edge[1] * edge_weights[1];
        let triangle_point = triangle[0] * triangle_weights[0]
            + triangle[1] * triangle_weights[1]
            + triangle[2] * triangle_weights[2];
        EdgeFaceWitness {
            edge_weights,
            triangle_weights,
            edge_point,
            triangle_point,
            distance: norm3(edge_point - triangle_point),
        }
    };
    let normal = (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]);
    let a = (edge[0] - triangle[0]).dot(normal);
    let b = (edge[1] - triangle[0]).dot(normal);
    if !normal.is_finite() || !a.is_finite() || !b.is_finite() || !(a - b).is_finite() {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "non-finite start body-feature witness",
        ));
    }
    if a != b && ftx_core::intersection::segment_triangle(edge[0], edge[1], triangle).is_some() {
        // Exact intersection establishes that the mathematical parameter lies in [0, 1].
        // Only the rounded geometric witness is bounded here, never a supplied physical value.
        let s = (a / (a - b)).clamp(0.0, 1.0);
        let point = edge[0] * (1.0 - s) + edge[1] * s;
        let (weights, _) = point_triangle_closest(point, triangle[0], triangle[1], triangle[2]);
        let hit = at([1.0 - s, s], weights);
        if hit.distance.is_finite() {
            return Ok(hit);
        }
    }
    let (weights, _) = point_triangle_closest(edge[0], triangle[0], triangle[1], triangle[2]);
    let mut best = at([1.0, 0.0], weights);
    let (weights, _) = point_triangle_closest(edge[1], triangle[0], triangle[1], triangle[2]);
    let candidate = at([0.0, 1.0], weights);
    if candidate.distance < best.distance {
        best = candidate;
    }
    for i in 0..3 {
        let j = (i + 1) % 3;
        let ((s, t), _) = edge_edge_closest(edge[0], edge[1], triangle[i], triangle[j]);
        let mut weights = [0.0; 3];
        weights[i] = 1.0 - t;
        weights[j] = t;
        let candidate = at([1.0 - s, s], weights);
        if candidate.distance < best.distance {
            best = candidate;
        }
    }
    if !best.distance.is_finite() {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "non-finite start body-feature distance",
        ));
    }
    Ok(best)
}

/// One bounded Jacobi sweep over body/cloth features in both directions. Correction directions come
/// from body outward normals, never an arbitrary cloth normal. Symmetric or infeasible starts may
/// remain unresolved and must still pass ordinary exact scene admission.
fn apply_body_repulsion(
    q: &mut [Vec3],
    topo: &RepulsionTopology,
    body: &NearestTree,
    body_topo: &BodyRepulsion,
    clearance: f64,
    opts: SelfRepulsionOptions,
    work: &mut RepulsionWork,
) -> Result<(), Refusal> {
    let (vertices, triangles) = body.surface();
    let p: Vec<_> = topo.members.iter().map(|members| q[members[0]]).collect();
    if p.iter().any(|v| !v.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "non-finite body-repulsion positions",
        ));
    }
    Refusal::finite("start body clearance", clearance)?;
    if clearance < 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "start body clearance must be nonnegative",
        ));
    }
    if clearance == 0.0 {
        return Ok(());
    }
    let pad = Vec3::new(clearance, clearance, clearance);
    let mut acc = vec![Vec3::ZERO; topo.n_weld];
    let mut weight = vec![0.0_f64; topo.n_weld];
    let mut candidates = Vec::new();
    let mut add = |ids: &[usize], weights: &[f64], delta: Vec3, normal: Vec3, distance: f64| {
        if distance >= clearance {
            return;
        }
        let gap = delta.dot(normal);
        let denominator: f64 = weights.iter().map(|w| w * w).sum();
        let correction = normal * (opts.stiffness * (clearance - gap) / denominator);
        for (&id, &w) in ids.iter().zip(weights) {
            acc[id] += correction * w;
            weight[id] += w;
        }
    };
    for edge in &topo.edges {
        let segment = edge.map(|v| p[v]);
        let (lo, hi) = crate::contact::aabb(&segment);
        let bounds = (lo - pad, hi + pad);
        body_topo.triangle_query.query(bounds, &mut candidates);
        work.spend(candidates.len())?;
        for &index in &candidates {
            let triangle = triangles[index].map(|v| vertices[v]);
            if !crate::bvh::overlaps(bounds, crate::contact::aabb(&triangle)) {
                continue;
            }
            let hit = edge_face_witness(segment, triangle)?;
            if hit.distance >= clearance {
                continue;
            }
            let cross = (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]);
            let Some(normal) = unit_normal(cross, "non-finite start body triangle normal")? else {
                continue;
            };
            add(
                edge,
                &hit.edge_weights,
                hit.edge_point - hit.triangle_point,
                normal,
                hit.distance,
            );
        }
    }
    for triangle in &topo.tris {
        let face = triangle.map(|v| p[v]);
        let (lo, hi) = crate::contact::aabb(&face);
        let bounds = (lo - pad, hi + pad);
        body_topo.edge_query.query(bounds, &mut candidates);
        work.spend(candidates.len())?;
        for &index in &candidates {
            let segment = body_topo.edges[index].map(|v| vertices[v]);
            if !crate::bvh::overlaps(bounds, crate::contact::aabb(&segment)) {
                continue;
            }
            let hit = edge_face_witness(segment, face)?;
            if hit.distance >= clearance {
                continue;
            }
            let normal = body.nearest(hit.edge_point).normal;
            let nlen = norm3(normal);
            if !normal.is_finite() || !nlen.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "non-finite start body nearest normal",
                ));
            }
            if nlen == 0.0 {
                continue;
            }
            add(
                triangle,
                &hit.triangle_weights,
                hit.triangle_point - hit.edge_point,
                normal,
                hit.distance,
            );
        }
    }
    // Nothing is published before both complete candidate streams fit the shared allowance.
    let max_step = 0.5 * clearance;
    let mut updated = p.clone();
    for w in 0..topo.n_weld {
        if weight[w] == 0.0 {
            continue;
        }
        let mut delta = acc[w] / weight[w].max(1.0);
        let length = norm3(delta);
        if !length.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "non-finite body-repelling start correction",
            ));
        }
        if length > max_step {
            delta = delta * (max_step / length);
        }
        updated[w] = p[w] + delta;
        if !updated[w].is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "non-finite body-repelling start position",
            ));
        }
    }
    for (w, members) in topo.members.iter().enumerate() {
        if weight[w] > 0.0 {
            for &member in members {
                q[member] = updated[w];
            }
        }
    }
    Ok(())
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
    mut repulsion: Option<(SelfRepulsionOptions, &mut PreparedRepulsion)>,
) -> Result<Vec<Vec3>, Refusal> {
    if repulsion.is_some() && start.iter().any(|p| !p.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "non-finite self-repulsion start",
        ));
    }
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
        if let Some((opts, prepared)) = repulsion.as_mut()
            && (it % opts.period == opts.period - 1 || it + 1 == iters)
        {
            apply_self_repulsion(
                &mut q,
                &mesh.verts2d,
                &mesh.owner,
                &prepared.cloth,
                body,
                clearance,
                *opts,
                &mut prepared.work,
            )?;
            apply_body_repulsion(
                &mut q,
                &prepared.cloth,
                body,
                &prepared.body,
                clearance,
                *opts,
                &mut prepared.work,
            )?;
        }
        if it % RELAX_PUSH_OUT_ITERS == RELAX_PUSH_OUT_ITERS - 1 || it + 1 == iters {
            body.push_out(&mut q, clearance);
        }
    }
    Ok(q)
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
    StartRelaxation::new(None)
        .expect("legacy relaxation options are valid")
        .start(mesh, wrapped, body)
        .expect("legacy start has no fallible self-repulsion work")
}

/// As [`start_state`], with optional bounded cloth–cloth and body-feature separation during
/// relaxation ([`SelfRepulsionOptions`]). When `repulsion` is `None`, the result is bit-identical to
/// [`start_state`].
///
/// # Errors
/// Invalid settings, non-finite separation geometry, or exhaustion of the allowance shared by all
/// blocks. No partially relaxed start is returned on refusal.
pub fn start_state_with_repulsion(
    mesh: &GarmentMesh,
    wrapped: Vec<Vec3>,
    body: &NearestTree,
    repulsion: Option<SelfRepulsionOptions>,
) -> Result<StartState, Refusal> {
    StartRelaxation::new(repulsion)?.start(mesh, wrapped, body)
}

impl StartRelaxation {
    pub(crate) fn start(
        &mut self,
        mesh: &GarmentMesh,
        wrapped: Vec<Vec3>,
        body: &NearestTree,
    ) -> Result<StartState, Refusal> {
        if self.options.is_some() && wrapped.iter().any(|p| !p.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "non-finite self-repulsion wrap",
            ));
        }
        let weights = gather_weights(mesh);
        let (presewn, groups) = presew(mesh, &wrapped, body, PRESEW_CLEARANCE, PRESEW_ITERS);
        let edges = face_edges(&mesh.faces);
        let plain: Vec<(usize, usize)> = edges
            .into_iter()
            .filter(|&(a, b)| weights[a] >= 1.0 && weights[b] >= 1.0)
            .collect();
        let (mut q, mut done, mut blocks) = (presewn.clone(), 0, Vec::new());
        while done < RELAX_MAX_ITERS {
            q = self.relax(
                mesh,
                &q,
                &groups,
                body,
                RELAX_ITERS,
                RELAX_CLEARANCE,
                &weights,
            )?;
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
        Ok(StartState {
            wrapped,
            presewn,
            groups,
            relaxed: q,
            relax_iterations: done,
            block_p95: blocks,
            gather_weights: weights,
        })
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
    fn self_repulsion_clears_panel_piercing_in_public_relaxation() {
        use crate::contact::{Body, Contact, ContactParams};

        let body = flat_body();
        // A tilted sleeve tip starts 10 mm through a bodice triangle. The other corners
        // are 25 mm clear, and both crossing edges pass well inside the bodice boundary.
        // The second copy of the tip is a welded seam member.
        let depth = 25.0_f64.hypot(35.0);
        for mirror in [1.0, -1.0] {
            for face in [[0, 1, 2], [0, 2, 1]] {
                let mesh = GarmentMesh {
                    instances: vec!["bodice".into(), "sleeve".into()],
                    verts2d: vec![
                        Vec2::new(-120.0, -100.0),
                        Vec2::new(120.0, -100.0),
                        Vec2::new(0.0, 160.0),
                        Vec2::new(0.0, 0.0),
                        Vec2::new(-15.0, -depth),
                        Vec2::new(15.0, -depth),
                        Vec2::new(0.0, 0.0),
                    ],
                    owner: vec![0, 0, 0, 1, 1, 1, 1],
                    faces: vec![face, [3, 4, 5]],
                    inst_range: vec![(0, 3), (3, 7)],
                    pairs: vec![(3, 6)],
                    seams: vec![],
                    corner: Default::default(),
                    index: Default::default(),
                    creases: vec![],
                };
                let z = 0.100;
                let tip = Vec3::new(0.0, 0.0, z - mirror * 0.010);
                let start = vec![
                    Vec3::new(-0.120, -0.100, z),
                    Vec3::new(0.120, -0.100, z),
                    Vec3::new(0.0, 0.160, z),
                    tip,
                    Vec3::new(-0.015, -0.025, z + mirror * 0.025),
                    Vec3::new(0.015, -0.025, z + mirror * 0.025),
                    tip,
                ];
                let params = ContactParams {
                    dhat: 0.001,
                    thickness: 0.0,
                    kappa: 1.0,
                    mu: 0.0,
                    eps_v: 0.001,
                };
                let (vertices, faces) = body.surface();
                let audit = Contact::new(
                    Body::new(vertices.to_vec(), faces.to_vec(), 0.01).unwrap(),
                    params,
                    mesh.faces.clone(),
                    |_, _| 0.001,
                );
                let groups = vec![vec![3, 6]];
                let weights = vec![1.0; start.len()];
                let unrepelled = relax(
                    &mesh,
                    &start,
                    &groups,
                    &body,
                    RELAX_ITERS,
                    RELAX_CLEARANCE,
                    &weights,
                );
                assert_eq!(
                    audit
                        .clone()
                        .with_self_contact(&unrepelled[..6], params)
                        .unwrap_err()
                        .code,
                    RefusalCode::IntersectingStart
                );
                let repaired = relax_with_repulsion(
                    &mesh,
                    &start,
                    &groups,
                    &body,
                    RELAX_ITERS,
                    RELAX_CLEARANCE,
                    &weights,
                    Some(SelfRepulsionOptions::default()),
                )
                .unwrap();
                assert_eq!(repaired[3], repaired[6]);
                assert!((repaired[3].z - tip.z) * mirror > 0.0);
                assert!((0..3).any(|v| (repaired[v].z - z) * mirror < 0.0));
                audit
                    .with_self_contact(&repaired[..6], params)
                    .expect("both layers must pass production self-contact admission");
            }
        }
    }

    #[test]
    fn self_repulsion_rescales_tiny_crossing_normal_and_refuses_overflowing_crossing() {
        assert_eq!(unit_normal(Vec3::ZERO, "zero").unwrap(), None);
        assert_eq!(
            unit_normal(Vec3::new(0.0, 0.0, 1e-200), "tiny").unwrap(),
            Some(Vec3::new(0.0, 0.0, 1.0))
        );
        assert_eq!(
            unit_normal(Vec3::new(0.0, 0.0, 1e200), "huge").unwrap(),
            Some(Vec3::new(0.0, 0.0, 1.0))
        );
        assert_eq!(
            unit_normal(Vec3::new(0.0, 0.0, f64::INFINITY), "inf")
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );

        let body = flat_body();
        let mesh = GarmentMesh {
            instances: vec!["bodice".into(), "sleeve".into()],
            verts2d: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(100.0, 0.0),
                Vec2::new(0.0, 100.0),
                Vec2::new(0.0, 0.0),
                Vec2::new(40.0, 0.0),
                Vec2::new(0.0, 40.0),
            ],
            owner: vec![0, 0, 0, 1, 1, 1],
            faces: vec![[0, 1, 2], [3, 4, 5]],
            inst_range: vec![(0, 3), (3, 6)],
            pairs: vec![],
            seams: vec![],
            corner: Default::default(),
            index: Default::default(),
            creases: vec![],
        };
        let topology = RepulsionTopology::build(&mesh, &[]);
        let opts = SelfRepulsionOptions::default();

        // Side 1e-100 m => cross product (0, 0, 1e-200): exact segment_triangle sees a Crossing,
        // while cross.norm() underflows to 0.0 in f64. Rescaling must still push the shallow tip
        // in +z across the face instead of overwriting proximity repulsion with a zero impulse.
        let z = 0.100;
        let tiny = 1e-100;
        let mut q = vec![
            Vec3::new(0.0, 0.0, z),
            Vec3::new(tiny, 0.0, z),
            Vec3::new(0.0, tiny, z),
            Vec3::new(0.25 * tiny, 0.25 * tiny, z - 0.002),
            Vec3::new(0.25 * tiny, 0.25 * tiny, z + 0.010),
            Vec3::new(0.050, 0.050, z + 0.010),
        ];
        let cross = (q[1] - q[0]).cross(q[2] - q[0]);
        assert_eq!(cross.norm(), 0.0);
        assert_eq!(
            ftx_core::intersection::segment_triangle(q[3], q[4], [q[0], q[1], q[2]]),
            Some(ftx_core::intersection::IntersectionKind::Crossing)
        );
        let tip_before = q[3].z;
        apply_self_repulsion(
            &mut q,
            &mesh.verts2d,
            &mesh.owner,
            &topology,
            &body,
            RELAX_CLEARANCE,
            opts,
            &mut RepulsionWork::new(opts),
        )
        .unwrap();
        assert!(
            q[3].z > tip_before + 1e-4,
            "tiny-area crossing must push shallow tip in +z, got before={tip_before} after={}",
            q[3].z
        );

        // Huge finite coordinates must fail closed as NonFinite instead of panicking inside
        // exact predicates.
        let mut huge = vec![
            Vec3::new(0.0, 0.0, 0.1),
            Vec3::new(1e110, 0.0, 0.1),
            Vec3::new(0.0, 1e110, 0.1),
            Vec3::new(0.25e110, 0.25e110, 0.09),
            Vec3::new(0.25e110, 0.25e110, 1e100),
            Vec3::new(0.50e110, 0.25e110, 1e100),
        ];
        let err = apply_self_repulsion(
            &mut huge,
            &mesh.verts2d,
            &mesh.owner,
            &topology,
            &body,
            RELAX_CLEARANCE,
            opts,
            &mut RepulsionWork::new(opts),
        )
        .unwrap_err();
        assert_eq!(err.code, RefusalCode::NonFinite);
    }

    #[test]
    fn body_feature_witness_sees_face_piercing_and_edge_proximity() {
        let triangle = [
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        // Neither endpoint nor either triangle edge is near this interior crossing.
        let crossing = edge_face_witness(
            [Vec3::new(0.0, 0.0, -0.1), Vec3::new(0.0, 0.0, 0.3)],
            triangle,
        )
        .unwrap();
        assert!(crossing.distance < 1e-14);
        assert!((crossing.edge_weights[1] - 0.25).abs() < 1e-14);
        for (actual, expected) in crossing.triangle_weights.into_iter().zip([0.25, 0.25, 0.5]) {
            assert!((actual - expected).abs() < 1e-14);
        }
        let nearby = edge_face_witness(
            [Vec3::new(-2.0, 0.0, 0.002), Vec3::new(2.0, 0.0, 0.002)],
            triangle,
        )
        .unwrap();
        assert!((nearby.distance - 0.002).abs() < 1e-14);
        assert!(nearby.edge_weights.iter().all(|&w| w > 0.0));
        assert!(nearby.triangle_weights.contains(&0.0));
    }

    #[test]
    fn body_repulsion_clears_a_ridge_through_one_welded_cloth_face() {
        use crate::contact::{Body, Contact, ContactParams, Intersection};

        let vertices = vec![
            Vec3::new(-0.02, -0.02, -0.02),
            Vec3::new(0.02, -0.02, -0.02),
            Vec3::new(0.0, 0.02, -0.02),
            Vec3::new(0.0, 0.0, 0.02),
        ];
        let faces = vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]];
        let body = NearestTree::new(vertices.clone(), faces.clone()).unwrap();
        let body_topo = BodyRepulsion::new(&body).unwrap();
        // Three independent cloth vertices, plus a seam duplicate of the first.
        let topo = RepulsionTopology {
            n_weld: 3,
            members: vec![vec![0, 3], vec![1], vec![2]],
            adj: vec![vec![1, 2], vec![0, 2], vec![0, 1]],
            tris: vec![[0, 1, 2]],
            tri_owner: vec![0],
            tri_rest2d: vec![[Vec2::new(0.0, 0.0); 3]],
            edges: vec![[0, 1], [0, 2], [1, 2]],
            edge_rest2d: vec![vec![]; 3],
            vert_owners: vec![vec![0]; 3],
        };
        let a = Vec3::new(-0.05, -0.05, 0.0);
        let initial = vec![
            a,
            Vec3::new(0.05, -0.05, 0.0),
            Vec3::new(0.0, 0.075, 0.0),
            a,
        ];
        assert!(
            initial
                .iter()
                .all(|&p| body.signed_distance(p) > RELAX_CLEARANCE)
        );
        let mut vertex_only = initial.clone();
        body.push_out(&mut vertex_only, RELAX_CLEARANCE);
        assert_eq!(vertex_only, initial);
        for &p in &vertices {
            let (w, _) = point_triangle_closest(p, initial[0], initial[1], initial[2]);
            let closest = initial[0] * w[0] + initial[1] * w[1] + initial[2] * w[2];
            assert!((p - closest).norm() > RELAX_CLEARANCE);
        }
        let audit = Contact::new(
            Body::new(vertices, faces, 0.01).unwrap(),
            ContactParams {
                dhat: 0.001,
                thickness: 0.0,
                kappa: 1.0,
                mu: 0.0,
                eps_v: 0.001,
            },
            vec![[0, 1, 2]],
            |_, _| 0.001,
        );
        assert!(matches!(
            audit.first_intersection(&initial),
            Some(Intersection::BodyEdge { .. })
        ));
        let opts = SelfRepulsionOptions::default();
        let repair = || {
            let mut q = initial.clone();
            let mut work = RepulsionWork::new(opts);
            for _ in 0..200 {
                apply_body_repulsion(
                    &mut q,
                    &topo,
                    &body,
                    &body_topo,
                    RELAX_CLEARANCE,
                    opts,
                    &mut work,
                )
                .unwrap();
                body.push_out(&mut q, RELAX_CLEARANCE);
                assert_eq!(q[0], q[3], "seam members must remain welded");
                if audit.first_intersection(&q).is_none() {
                    break;
                }
            }
            assert!(
                audit.first_intersection(&q).is_none(),
                "the exact body audit must admit the repaired face"
            );
            q
        };
        assert_eq!(repair(), repair());

        let mut q = initial.clone();
        let mut work = RepulsionWork::new(SelfRepulsionOptions {
            max_candidate_checks: 1,
            ..opts
        });
        let refused = apply_body_repulsion(
            &mut q,
            &topo,
            &body,
            &body_topo,
            RELAX_CLEARANCE,
            opts,
            &mut work,
        )
        .unwrap_err();
        assert_eq!(refused.code, RefusalCode::BudgetExceeded);
        assert_eq!(q, initial, "a failed sweep must not publish corrections");

        let mut work = RepulsionWork::new(opts);
        q[0].x = f64::NAN;
        assert_eq!(
            apply_body_repulsion(
                &mut q,
                &topo,
                &body,
                &body_topo,
                RELAX_CLEARANCE,
                opts,
                &mut work,
            )
            .unwrap_err()
            .code,
            RefusalCode::NonFinite
        );
        assert_eq!(q[1..], initial[1..]);
        for clearance in [-1.0, f64::NAN] {
            let mut q = initial.clone();
            assert!(
                apply_body_repulsion(&mut q, &topo, &body, &body_topo, clearance, opts, &mut work,)
                    .is_err()
            );
            assert_eq!(q, initial);
        }
        let mut q = initial.clone();
        apply_body_repulsion(&mut q, &topo, &body, &body_topo, 0.0, opts, &mut work).unwrap();
        assert_eq!(q, initial);
    }

    #[test]
    fn body_repulsion_refuses_degenerate_body_features() {
        let body = NearestTree::new(
            vec![
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 0.0),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap();
        assert!(matches!(
            BodyRepulsion::new(&body),
            Err(Refusal {
                code: RefusalCode::DegenerateGeometry,
                ..
            })
        ));
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
        let mut shared = start.clone();
        let mut workspace = StartRelaxation::new(Some(opts)).unwrap();
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
            shared = workspace
                .relax(
                    &mesh,
                    &shared,
                    &[],
                    &body,
                    RELAX_PUSH_OUT_ITERS,
                    RELAX_CLEARANCE,
                    &weights,
                )
                .unwrap();
            assert_eq!(
                shared, chunked,
                "cached queries must preserve every checkpoint"
            );
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
        assert!(wire.get("max_candidate_checks").is_none());
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
            (
                SelfRepulsionOptions {
                    max_candidate_checks: 0,
                    ..opts
                },
                RefusalCode::OutOfRange,
            ),
            (
                SelfRepulsionOptions {
                    max_candidate_checks: MAX_SELF_REPULSION_CANDIDATE_CHECKS + 1,
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
    fn self_repulsion_work_bound_covers_all_sweeps_without_publishing_partial_state() {
        let pat = Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [panel_piece("inner", 30.0, 30.0), panel_piece("outer", 30.0, 30.0)],
            "seams": []
        }));
        let mesh = build_garment(&pat, 15.0).unwrap();
        let weights = gather_weights(&mesh);
        let body = flat_body();
        let start: Vec<_> = mesh
            .verts2d
            .iter()
            .enumerate()
            .map(|(i, p)| Vec3::new(p.x * MM, p.y * MM, 0.1 + mesh.owner[i] as f64 * 0.0005))
            .collect();
        let before = start.clone();
        let mut opts = SelfRepulsionOptions {
            period: 1,
            ..SelfRepulsionOptions::default()
        };
        let mut work = StartRelaxation::new(Some(opts)).unwrap();
        let first = work
            .relax(&mesh, &start, &[], &body, 1, RELAX_CLEARANCE, &weights)
            .unwrap();
        let spent = opts.max_candidate_checks - work.prepared.as_ref().unwrap().work.remaining;
        assert!(spent > 0);
        opts.max_candidate_checks = spent;
        let wire = serde_json::to_value(opts).unwrap();
        assert_eq!(wire["max_candidate_checks"], spent);
        assert_eq!(
            serde_json::from_value::<SelfRepulsionOptions>(wire).unwrap(),
            opts
        );
        assert_eq!(
            relax_with_repulsion(
                &mesh,
                &start,
                &[],
                &body,
                1,
                RELAX_CLEARANCE,
                &weights,
                Some(opts)
            )
            .unwrap(),
            first
        );
        for iterations in [2, RELAX_ITERS] {
            let refusal = relax_with_repulsion(
                &mesh,
                &start,
                &[],
                &body,
                iterations,
                RELAX_CLEARANCE,
                &weights,
                Some(opts),
            )
            .unwrap_err();
            assert_eq!(refusal.code, RefusalCode::BudgetExceeded);
            assert!(refusal.message.contains(&spent.to_string()));
            assert_eq!(start, before);
        }
        assert_eq!(
            start_state_with_repulsion(&mesh, start.clone(), &body, Some(opts))
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
        assert_eq!(start, before);
    }

    #[test]
    fn self_repulsion_streams_long_overlapping_features_and_refuses_nonfinite_starts() {
        let pat = Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [panel_piece("inner", 30.0, 30.0), panel_piece("outer", 30.0, 30.0)],
            "seams": []
        }));
        let mesh = build_garment(&pat, 30.0).unwrap();
        let topology = RepulsionTopology::build(&mesh, &[]);
        let opts = SelfRepulsionOptions::default();
        let body = flat_body();
        // Long current edges used to enter the all-edges Cartesian-product fallback. The model's
        // flat rest coordinates remain unchanged: this is a geometric broad-phase experiment.
        let start: Vec<_> = mesh
            .verts2d
            .iter()
            .enumerate()
            .map(|(i, p)| Vec3::new(p.x, p.y, 0.1 + mesh.owner[i] as f64 * 0.0005))
            .collect();
        let mut output = start.clone();
        let mut work = RepulsionWork::new(opts);
        apply_self_repulsion(
            &mut output,
            &mesh.verts2d,
            &mesh.owner,
            &topology,
            &body,
            RELAX_CLEARANCE,
            opts,
            &mut work,
        )
        .unwrap();
        let (lo0, hi0) = mesh.inst_range[0];
        let (lo1, hi1) = mesh.inst_range[1];
        let mean_z = |range: std::ops::Range<usize>| {
            let n = range.len();
            range.map(|i| output[i].z).sum::<f64>() / n as f64
        };
        assert!(mean_z(lo1..hi1) - mean_z(lo0..hi0) > 0.0005);
        let mut repeat = start.clone();
        apply_self_repulsion(
            &mut repeat,
            &mesh.verts2d,
            &mesh.owner,
            &topology,
            &body,
            RELAX_CLEARANCE,
            opts,
            &mut RepulsionWork::new(opts),
        )
        .unwrap();
        assert_eq!(output, repeat);
        let mut invalid = start;
        invalid[0].x = f64::NAN;
        let refusal = relax_with_repulsion(
            &mesh,
            &invalid,
            &[],
            &body,
            1,
            RELAX_CLEARANCE,
            &gather_weights(&mesh),
            Some(opts),
        )
        .unwrap_err();
        assert_eq!(refusal.code, RefusalCode::NonFinite);
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
            ..SelfRepulsionOptions::default()
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
