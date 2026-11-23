//! Contact between the cloth and a static body (plan §11.2, §11.5): the IPC log barrier on the
//! thickness-offset gap, the edge–edge mollifier, deterministic broad phases (a bounded body grid with
//! BVH fallback, a BVH for self-contact), additive CCD (Li et al. 2021) to cap each Newton step, and
//! lagged smoothed Coulomb friction.
//!
//! Pairs: a cloth vertex against a body triangle, a body vertex against a cloth triangle, and a cloth
//! edge against a body edge. Optional self-contact adds non-incident cloth vertex–triangle and
//! edge–edge pairs; all four vertices move and the solver must rebuild its symbolic analysis when
//! their couplings change. Energies are evaluated on [`Jet`]s for exact gradients and Hessians.

use std::collections::BTreeMap;

use ftx_core::intersection::{indexed_triangle_pair, segment_triangle, validate_mesh};
use ftx_core::{Refusal, RefusalCode, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ad::Jet;
use crate::bvh::{Bvh, QueryBvh};
use crate::distance::{
    Closest, edge_cross_squared, edge_edge_closest, point_triangle_closest, squared_distance,
    squared_distance_f64, squared_distance_grad,
};
use crate::winding::{DEFAULT_BETA, WindingOctree, winding_exact};

mod materials;
mod motion;
pub use materials::SurfaceMaterial;
pub use motion::MotionBound;

/// Contact and friction parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactParams {
    /// Barrier activation distance on the gap (m): pairs closer than this repel.
    pub dhat: f64,
    /// Offset ξ between the cloth mid-surface and the body (half the cloth thickness, m). The barrier acts
    /// on `distance − ξ`, so clearance is measured to the cloth's real surface.
    pub thickness: f64,
    /// Barrier stiffness κ (J/m² = N/m); multiplies `−(g − ĝ)² ln(g/ĝ)` in m².
    pub kappa: f64,
    /// Coulomb friction coefficient (0 = frictionless).
    pub mu: f64,
    /// Friction smoothing: slip speeds below this (m/s) are treated as sticking.
    pub eps_v: f64,
}

impl ContactParams {
    /// Checks contact parameters before a solve. For self-contact, `thickness` is the sum of the two
    /// cloth half-thicknesses, not the cloth–body offset.
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] for non-finite values; [`RefusalCode::OutOfRange`] unless distances,
    /// stiffness and smoothing are positive, and thickness and friction are non-negative.
    pub fn validate(&self) -> Result<(), Refusal> {
        for (name, value) in [
            ("contact activation distance", self.dhat),
            ("contact thickness", self.thickness),
            ("contact stiffness", self.kappa),
            ("contact friction", self.mu),
            ("friction smoothing speed", self.eps_v),
        ] {
            Refusal::finite(name, value)?;
        }
        if self.dhat <= 0.0
            || self.kappa <= 0.0
            || self.eps_v <= 0.0
            || self.thickness < 0.0
            || self.mu < 0.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "contact requires positive activation distance, stiffness and smoothing; \
                 thickness and friction must be non-negative",
            ));
        }
        Ok(())
    }
}

/// Opt-in Li et al. (2020) physically scaled IPC barrier stiffness policy (FTX-27e).
///
/// Scales the initial barrier stiffness `kappa` (N/m) from the cloth's mean vertex mass, gravity
/// and membrane stiffness so both ultra-light fabrics (e.g. 40 g/m² silk) and heavy fabrics (e.g.
/// 500 g/m² wool) settle at consistent mid-band contact clearance instead of hovering or
/// compressing deep into the barrier. Between steps, when the minimum active gap drops below
/// `min_gap_fraction * dhat`, `kappa` scales up by `growth_factor` within `[kappa_min, kappa_max]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdaptiveBarrierPolicy {
    /// Target resting clearance as a fraction of `dhat`, in `(0, 1)` (default `0.5`).
    pub target_gap_fraction: f64,
    /// Minimum active gap fraction of `dhat` before between-step `kappa` escalation triggers, in
    /// `(0, target_gap_fraction]` (default `0.05`).
    pub min_gap_fraction: f64,
    /// Multiplicative growth factor applied between steps when `min_gap < min_gap_fraction * dhat`,
    /// in `[1.0, 16.0]` (default `2.0`).
    pub growth_factor: f64,
    /// Dimensionless fraction of characteristic membrane stiffness `E_t` providing a positive
    /// stiffness floor during zero-gravity phases, in `[0.0, 1.0]` (default `1e-5`).
    pub membrane_floor_scale: f64,
    /// Lower bound on adapted barrier stiffness `kappa` (N/m, default `1e-3`).
    pub kappa_min: f64,
    /// Upper bound on adapted barrier stiffness `kappa` (N/m, default `1e5`).
    pub kappa_max: f64,
}

impl Default for AdaptiveBarrierPolicy {
    fn default() -> Self {
        Self {
            target_gap_fraction: 0.5,
            min_gap_fraction: 0.05,
            growth_factor: 2.0,
            membrane_floor_scale: 1e-5,
            kappa_min: 1e-3,
            kappa_max: 1e5,
        }
    }
}

impl AdaptiveBarrierPolicy {
    /// Validates every parameter of the adaptive barrier policy.
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] for non-finite values; [`RefusalCode::OutOfRange`] for invalid
    /// fractions, growth factor, or stiffness bounds.
    pub fn validate(&self) -> Result<(), Refusal> {
        for (name, value) in [
            (
                "adaptive barrier target gap fraction",
                self.target_gap_fraction,
            ),
            (
                "adaptive barrier minimum gap fraction",
                self.min_gap_fraction,
            ),
            ("adaptive barrier growth factor", self.growth_factor),
            (
                "adaptive barrier membrane floor scale",
                self.membrane_floor_scale,
            ),
            ("adaptive barrier minimum stiffness", self.kappa_min),
            ("adaptive barrier maximum stiffness", self.kappa_max),
        ] {
            Refusal::finite(name, value)?;
        }
        if !(self.target_gap_fraction > 0.0 && self.target_gap_fraction < 1.0)
            || !(self.min_gap_fraction > 0.0 && self.min_gap_fraction <= self.target_gap_fraction)
            || !(1.0..=16.0).contains(&self.growth_factor)
            || !(0.0..=1.0).contains(&self.membrane_floor_scale)
            || self.kappa_min <= 0.0
            || self.kappa_max < self.kappa_min
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "adaptive barrier requires 0 < min_gap_fraction <= target_gap_fraction < 1, \
                 growth_factor in [1, 16], membrane_floor_scale in [0, 1], and 0 < kappa_min <= kappa_max",
            ));
        }
        Ok(())
    }

    /// Dimensionless barrier reaction factor `phi(alpha) = 2(1 - alpha) ln(1/alpha) + (1 - alpha)^2 / alpha`
    /// for `alpha in (0, 1)`, such that `-db/dg(alpha * dhat) = kappa * dhat * phi(alpha)`.
    /// Returns `NaN` outside `(0, 1)`.
    #[must_use]
    pub fn barrier_force_factor(alpha: f64) -> f64 {
        if !alpha.is_finite() || !(alpha > 0.0 && alpha < 1.0) {
            return f64::NAN;
        }
        let one_minus = 1.0 - alpha;
        2.0 * one_minus * ftx_core::det::ln(1.0 / alpha) + (one_minus * one_minus) / alpha
    }

    /// Computes the physically scaled initial barrier stiffness `kappa` (N/m) for a cloth with the
    /// given mean vertex mass (kg), characteristic membrane stiffness (N/m), reference gravity
    /// (m/s²), and barrier activation distance `dhat_m` (m).
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] or [`RefusalCode::OutOfRange`] on invalid policy or inputs.
    pub fn initial_kappa(
        &self,
        mean_vertex_mass_kg: f64,
        membrane_stiffness_n_m: f64,
        gravity_m_s2: f64,
        dhat_m: f64,
    ) -> Result<f64, Refusal> {
        self.validate()?;
        for (name, value) in [
            ("mean vertex mass", mean_vertex_mass_kg),
            ("membrane stiffness", membrane_stiffness_n_m),
            ("gravity", gravity_m_s2),
            ("barrier activation distance", dhat_m),
        ] {
            Refusal::finite(name, value)?;
        }
        if mean_vertex_mass_kg < 0.0
            || membrane_stiffness_n_m < 0.0
            || gravity_m_s2 < 0.0
            || dhat_m <= 0.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "adaptive barrier inputs require non-negative mass, stiffness, gravity and positive dhat",
            ));
        }
        let phi = Self::barrier_force_factor(self.target_gap_fraction);
        let k_grav = (mean_vertex_mass_kg * gravity_m_s2) / (dhat_m * phi);
        let k_mem = self.membrane_floor_scale * membrane_stiffness_n_m;
        let raw = k_grav.max(k_mem);
        Refusal::finite("adaptive initial barrier stiffness", raw)?;
        Ok(raw.clamp(self.kappa_min, self.kappa_max))
    }

    /// Between-step stiffness update: if `min_gap` fell below `min_gap_fraction * dhat_m`, scales
    /// `current_kappa` up by `growth_factor` within `[kappa_min, kappa_max]`.
    #[must_use]
    pub fn adapt_kappa(&self, current_kappa: f64, min_gap: Option<f64>, dhat_m: f64) -> f64 {
        if !current_kappa.is_finite() || !dhat_m.is_finite() || dhat_m <= 0.0 {
            return self.kappa_min;
        }
        let clamped = current_kappa.clamp(self.kappa_min, self.kappa_max);
        match min_gap {
            Some(g) if g.is_finite() && g > 0.0 && g < self.min_gap_fraction * dhat_m => {
                (clamped * self.growth_factor).min(self.kappa_max)
            }
            _ => clamped,
        }
    }
}

/// Opt-in smooth static/kinetic stick-slip Coulomb friction (FTX-27e).
///
/// Extends IPC's smoothed Coulomb dissipation potential with a C²-continuous Stribeck transition
/// from static friction `mu_s` (peak resisting traction at `eps_v`) down to kinetic friction
/// `mu_k <= mu_s` over characteristic slip velocity `transition_v_m_s`. When `mu_s == mu_k`, this
/// reduces identically to standard IPC Coulomb friction.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StickSlipFriction {
    /// Static friction coefficient `mu_s >= mu_k >= 0` (dimensionless).
    pub mu_s: f64,
    /// Kinetic (sliding) friction coefficient `0 <= mu_k <= mu_s` (dimensionless).
    pub mu_k: f64,
    /// Characteristic slip velocity over which friction transitions from `mu_s` to `mu_k` (m/s, > 0).
    pub transition_v_m_s: f64,
}

impl StickSlipFriction {
    /// Constructs and validates a stick-slip friction law.
    ///
    /// # Errors
    /// See [`StickSlipFriction::validate`].
    pub fn new(mu_s: f64, mu_k: f64, transition_v_m_s: f64) -> Result<Self, Refusal> {
        let law = Self {
            mu_s,
            mu_k,
            transition_v_m_s,
        };
        law.validate()?;
        Ok(law)
    }

    /// Validates static/kinetic friction parameters.
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] for non-finite values; [`RefusalCode::OutOfRange`] unless
    /// `0 <= mu_k <= mu_s` and `transition_v_m_s > 0`.
    pub fn validate(&self) -> Result<(), Refusal> {
        for (name, value) in [
            ("static friction coefficient", self.mu_s),
            ("kinetic friction coefficient", self.mu_k),
            ("stick-slip transition speed", self.transition_v_m_s),
        ] {
            Refusal::finite(name, value)?;
        }
        if self.mu_k < 0.0 || self.mu_s < self.mu_k || self.transition_v_m_s <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "stick-slip friction requires 0 <= mu_k <= mu_s and positive transition_v_m_s",
            ));
        }
        Ok(())
    }

    /// Effective friction traction ratio `tau(v)` at tangential slip speed `slip_speed_m_s >= 0`
    /// with stick regularization speed `eps_v > 0`. Returns `NaN` on non-finite or non-positive
    /// scale inputs.
    #[must_use]
    pub fn effective_mu(&self, slip_speed_m_s: f64, eps_v: f64) -> f64 {
        if !slip_speed_m_s.is_finite()
            || !eps_v.is_finite()
            || eps_v <= 0.0
            || !self.mu_s.is_finite()
            || !self.mu_k.is_finite()
            || !self.transition_v_m_s.is_finite()
            || self.transition_v_m_s <= 0.0
        {
            return f64::NAN;
        }
        let v = slip_speed_m_s.max(0.0);
        if v < eps_v {
            let r = v / eps_v;
            self.mu_s * (-r * r + 2.0 * r)
        } else {
            let u = (v - eps_v) / self.transition_v_m_s;
            let exp_neg_u = if u > 700.0 {
                0.0
            } else {
                ftx_core::det::exp(-u)
            };
            self.mu_k + (self.mu_s - self.mu_k) * (1.0 + u) * exp_neg_u
        }
    }
}

/// Radial Stribeck stick-slip dissipation potential and derivatives (divided by normal load `lambda`).
#[derive(Clone, Copy, Debug)]
struct StickSlipRadial {
    d_val: f64,
    tau_over_y: f64,
    #[cfg(test)]
    radial_eig_exact: f64,
    /// Exact `(radial_eig_psd - tau_over_y)` computed without cancellation near `y -> 0`.
    radial_psd_minus_tau_over_y: f64,
}

/// Evaluates the radial Stribeck stick-slip potential at displacement norm `y >= 0`.
///
/// For `y < eps`: `tau(y) = mu_s * (-y^2/eps^2 + 2y/eps)`, strictly convex (`radial_eig >= 0`).
/// For `y >= eps`: `tau(y) = mu_k + (mu_s - mu_k) * (1 + u) * exp(-u)` with `u = (y - eps) / w`,
/// C¹-continuous with `tau(eps) = mu_s` and `tau'(eps) = 0`. In the Stribeck softening regime
/// (`mu_s > mu_k`), `radial_eig_exact <= 0`; clamping to `0.0` gives the exact closed-form PSD
/// projection of the 2x2 tangential Hessian.
fn stick_slip_radial(y: f64, eps: f64, ss: StickSlipFriction, h: f64) -> StickSlipRadial {
    if y < eps {
        let d_val = ss.mu_s * (-y * y * y / (3.0 * eps * eps) + y * y / eps + eps / 3.0);
        let tau_over_y = ss.mu_s * (-y / (eps * eps) + 2.0 / eps);
        #[cfg(test)]
        let radial_eig_exact = ss.mu_s * (-2.0 * y / (eps * eps) + 2.0 / eps);
        StickSlipRadial {
            d_val,
            tau_over_y,
            #[cfg(test)]
            radial_eig_exact,
            radial_psd_minus_tau_over_y: -ss.mu_s * y / (eps * eps),
        }
    } else {
        let w = ss.transition_v_m_s * h;
        let s = y - eps;
        let u = s / w;
        let dm = ss.mu_s - ss.mu_k;
        let exp_neg_u = if u > 700.0 {
            0.0
        } else {
            ftx_core::det::exp(-u)
        };
        let u_exp = if u > 700.0 { 0.0 } else { u * exp_neg_u };
        let d_val = ss.mu_s * eps + ss.mu_k * s + dm * w * (2.0 - (2.0 * exp_neg_u + u_exp));
        let tau = ss.mu_k + dm * (exp_neg_u + u_exp);
        let tau_over_y = tau / y;
        let radial_eig_exact = -dm * u_exp / w;
        let radial_eig_psd = radial_eig_exact.max(0.0);
        StickSlipRadial {
            d_val,
            tau_over_y,
            #[cfg(test)]
            radial_eig_exact,
            radial_psd_minus_tau_over_y: radial_eig_psd - tau_over_y,
        }
    }
}

/// A static body: a triangle mesh (metres), closed (a whole body) or open (a test surface).
#[derive(Clone, Debug)]
pub struct Body {
    /// Vertex positions.
    pub vertices: Vec<Vec3>,
    /// Triangles (outward winding).
    pub triangles: Vec<[usize; 3]>,
    /// Unique edges.
    pub edges: Vec<[usize; 2]>,
    /// Every edge has exactly two triangles, so inside and outside are defined.
    pub closed: bool,
    tri_grid: Grid,
    vert_grid: Grid,
    edge_grid: Grid,
    tri_box: Vec<(Vec3, Vec3)>,
    edge_box: Vec<(Vec3, Vec3)>,
    winding_tree: WindingOctree,
}

/// Whether two axis-aligned boxes overlap (touching counts).
fn overlaps(a: (Vec3, Vec3), lo: Vec3, hi: Vec3) -> bool {
    !(a.1.x < lo.x || a.0.x > hi.x || a.1.y < lo.y || a.0.y > hi.y || a.1.z < lo.z || a.0.z > hi.z)
}

fn cell_range(lo: Vec3, hi: Vec3, cell: f64) -> ([i64; 3], [i64; 3]) {
    let f = |v: f64| (v / cell).floor() as i64;
    ([f(lo.x), f(lo.y), f(lo.z)], [f(hi.x), f(hi.y), f(hi.z)])
}

pub(crate) fn aabb(points: &[Vec3]) -> (Vec3, Vec3) {
    points.iter().fold(
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

/// A swept primitive contains both endpoints of every vertex trajectory, not just its start shape.
fn swept_box(ids: &[usize], x: &[Vec3], p: Option<&[Vec3]>) -> (Vec3, Vec3) {
    let mut pts: Vec<Vec3> = ids.iter().map(|&i| x[i]).collect();
    if let Some(p) = p {
        pts.extend(ids.iter().map(|&i| x[i] + p[i]));
    }
    aabb(&pts)
}

/// Each box grown by half of `pad` on every side, so two boxes overlap iff they come within `pad`.
fn padded(boxes: &[(Vec3, Vec3)], pad: f64) -> Vec<(Vec3, Vec3)> {
    let d = Vec3::new(pad * 0.5, pad * 0.5, pad * 0.5);
    boxes.iter().map(|&(lo, hi)| (lo - d, hi + d)).collect()
}

/// Every `(i, j)` whose boxes `a[i]` and `b[j]` come within `pad`, sorted (a BVH per side; both sides can
/// move). Unlike a swept uniform grid, this does not enumerate a potentially enormous number of cells for a
/// long Newton displacement. Every overlapping pair is retained; no truncation or heuristic neighbor limit.
fn box_pairs(a: &[(Vec3, Vec3)], b: &[(Vec3, Vec3)], pad: f64) -> Vec<(usize, usize)> {
    let (a, b) = (padded(a, pad), padded(b, pad));
    Bvh::build(&a).pairs(&Bvh::build(&b))
}

/// Every `(i, j)`, `i < j`, whose boxes come within `pad`, sorted.
fn box_self_pairs(a: &[(Vec3, Vec3)], pad: f64) -> Vec<(usize, usize)> {
    Bvh::build(&padded(a, pad)).self_pairs()
}

/// A dense broad-phase grid over the cells the items' boxes touch, in CSR form: cell `c` holds
/// `ids[start[c]..start[c + 1]]`, in item order. A query returns the sorted, deduplicated ids of the cells
/// a box touches (cells outside the grid hold nothing). A frankensim-derived BVH replaces the grid
/// when its integer range or bounded storage cannot represent the mesh economically. The fallback
/// is exhaustive, not a truncated query or a coarsening of the collision geometry.
#[derive(Clone, Debug)]
struct Grid {
    cell: f64,
    lo: [i64; 3],
    dims: [usize; 3],
    start: Vec<usize>,
    ids: Vec<usize>,
    fallback: Option<QueryBvh>,
}

impl Grid {
    fn build(boxes: &[(Vec3, Vec3)], cell: f64) -> Self {
        Self::dense(boxes, cell).unwrap_or_else(|| Self {
            cell: 1.0,
            lo: [0; 3],
            dims: [0; 3],
            start: Vec::new(),
            ids: Vec::new(),
            fallback: Some(QueryBvh::build(boxes)),
        })
    }

    // A moving body's first update converts a dense grid to the existing owning BVH. Later
    // updates reuse its immutable primitive ordering instead of rediscovering a spatial partition.
    fn refitted(&self, boxes: &[(Vec3, Vec3)]) -> Result<Self, Refusal> {
        let tree = match &self.fallback {
            Some(tree) => tree.refitted(boxes)?,
            None => QueryBvh::build(boxes),
        };
        Ok(Self {
            cell: 1.0,
            lo: [0; 3],
            dims: [0; 3],
            start: Vec::new(),
            ids: Vec::new(),
            fallback: Some(tree),
        })
    }

    fn dense(boxes: &[(Vec3, Vec3)], cell: f64) -> Option<Self> {
        // These cap only the choice of accelerator, never the geometry or returned candidates.
        const MAX_CELLS: usize = 1 << 20;
        const MAX_ENTRIES: usize = 8 << 20;
        if !cell.is_finite() || cell <= 0.0 {
            return None;
        }
        // A saturating float-to-integer cast can alias remote coordinates. Fall back before casting
        // either endpoint outside i64's range (the upper endpoint rounds to 2^63 as f64).
        for &(lo, hi) in boxes {
            for k in 0..3 {
                let (a, b) = ((lo[k] / cell).floor(), (hi[k] / cell).floor());
                if !(i64::MIN as f64 <= a && a <= b && b < i64::MAX as f64) {
                    return None;
                }
            }
        }
        let ranges: Vec<([i64; 3], [i64; 3])> = boxes
            .iter()
            .map(|&(lo, hi)| cell_range(lo, hi, cell))
            .collect();
        let (mut lo, mut hi) = match ranges.first() {
            Some(&(a, b)) => (a, b),
            None => ([0; 3], [-1; 3]), // no cells
        };
        for (a, b) in &ranges {
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
        }
        let mut dims = [0; 3];
        for k in 0..3 {
            dims[k] = usize::try_from(hi[k].checked_sub(lo[k])?.checked_add(1)?).ok()?;
        }
        let cells = dims.iter().try_fold(1usize, |n, &d| n.checked_mul(d))?;
        if cells > MAX_CELLS {
            return None;
        }
        let mut entries = 0usize;
        for &(a, b) in &ranges {
            let mut volume = 1usize;
            for k in 0..3 {
                let d = usize::try_from(b[k].checked_sub(a[k])?.checked_add(1)?).ok()?;
                volume = volume.checked_mul(d)?;
            }
            entries = entries.checked_add(volume)?;
            if entries > MAX_ENTRIES {
                return None;
            }
        }
        let mut grid = Self {
            cell,
            lo,
            dims,
            start: vec![0; cells + 1],
            ids: Vec::new(),
            fallback: None,
        };
        let each = |r: &([i64; 3], [i64; 3]), f: &mut dyn FnMut(usize)| {
            for i in r.0[0]..=r.1[0] {
                for j in r.0[1]..=r.1[1] {
                    for k in r.0[2]..=r.1[2] {
                        f(grid_index(lo, dims, [i, j, k]));
                    }
                }
            }
        };
        for r in &ranges {
            each(r, &mut |c| grid.start[c + 1] += 1);
        }
        for c in 0..grid.start.len() - 1 {
            grid.start[c + 1] += grid.start[c];
        }
        let mut fill = grid.start.clone();
        grid.ids = vec![0; *grid.start.last().unwrap_or(&0)];
        for (id, r) in ranges.iter().enumerate() {
            each(r, &mut |c| {
                grid.ids[fill[c]] = id;
                fill[c] += 1;
            });
        }
        Some(grid)
    }

    fn query(&self, lo: Vec3, hi: Vec3, out: &mut Vec<usize>) {
        if let Some(bvh) = &self.fallback {
            bvh.query((lo, hi), out);
            return;
        }
        out.clear();
        let (a, b) = cell_range(lo, hi, self.cell);
        let a: [i64; 3] = std::array::from_fn(|k| a[k].max(self.lo[k]));
        let b: [i64; 3] = std::array::from_fn(|k| b[k].min(self.lo[k] + self.dims[k] as i64 - 1));
        for i in a[0]..=b[0] {
            for j in a[1]..=b[1] {
                for k in a[2]..=b[2] {
                    let c = grid_index(self.lo, self.dims, [i, j, k]);
                    out.extend_from_slice(&self.ids[self.start[c]..self.start[c + 1]]);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

fn grid_index(lo: [i64; 3], dims: [usize; 3], c: [i64; 3]) -> usize {
    let r: [usize; 3] = std::array::from_fn(|k| (c[k] - lo[k]) as usize);
    (r[0] * dims[1] + r[1]) * dims[2] + r[2]
}

/// Unique edges of a triangle list, sorted.
#[must_use]
pub fn edges_of(triangles: &[[usize; 3]]) -> Vec<[usize; 2]> {
    let mut e: Vec<[usize; 2]> = triangles
        .iter()
        .flat_map(|t| {
            (0..3).map(move |k| {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                [a.min(b), a.max(b)]
            })
        })
        .collect();
    e.sort_unstable();
    e.dedup();
    e
}

impl Body {
    /// No body collider, for a cloth self-contact-only simulation. This has no interior; it does not
    /// relax [`Body::new`]'s checks on supplied meshes.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            vertices: Vec::new(),
            triangles: Vec::new(),
            edges: Vec::new(),
            closed: false,
            tri_grid: Grid::build(&[], 1.0),
            vert_grid: Grid::build(&[], 1.0),
            edge_grid: Grid::build(&[], 1.0),
            tri_box: Vec::new(),
            edge_box: Vec::new(),
            winding_tree: WindingOctree::empty(),
        }
    }

    /// Builds the body and its broad-phase grids (cell size about the mean edge length, at least `min_cell`).
    ///
    /// # Errors
    /// [`RefusalCode::DegenerateGeometry`] for an empty mesh, an out-of-range index or a zero-area face;
    /// [`RefusalCode::NonFinite`] for a non-finite vertex or minimum cell size;
    /// [`RefusalCode::OutOfRange`] for a non-positive minimum cell size.
    pub fn new(
        vertices: Vec<Vec3>,
        triangles: Vec<[usize; 3]>,
        min_cell: f64,
    ) -> Result<Self, Refusal> {
        if triangles.is_empty() || triangles.iter().flatten().any(|&i| i >= vertices.len()) {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "the body mesh is empty or indexes a missing vertex",
            ));
        }
        // Admit before grids can hide NaNs or distance derivatives encounter a collapsed face.
        validate_mesh(&vertices, &triangles)?;
        Refusal::finite("body grid minimum cell size", min_cell)?;
        if min_cell <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "body grid minimum cell size must be positive",
            ));
        }
        let edges = edges_of(&triangles);
        let mut uses: BTreeMap<[usize; 2], usize> = BTreeMap::new();
        for t in &triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *uses.entry([a.min(b), a.max(b)]).or_default() += 1;
            }
        }
        let closed = uses.values().all(|&n| n == 2);
        let mean = edges
            .iter()
            .map(|[a, b]| (vertices[*a] - vertices[*b]).norm())
            .sum::<f64>()
            / edges.len() as f64;
        let cell = mean.max(min_cell);
        let tri_box: Vec<(Vec3, Vec3)> = triangles
            .iter()
            .map(|t| aabb(&t.map(|v| vertices[v])))
            .collect();
        let edge_box: Vec<(Vec3, Vec3)> = edges
            .iter()
            .map(|e| aabb(&e.map(|v| vertices[v])))
            .collect();
        let vert_box: Vec<(Vec3, Vec3)> = vertices.iter().map(|&v| (v, v)).collect();
        let winding_tree = if closed {
            WindingOctree::build(&vertices, &triangles, DEFAULT_BETA)
        } else {
            WindingOctree::empty()
        };
        Ok(Self {
            tri_grid: Grid::build(&tri_box, cell),
            vert_grid: Grid::build(&vert_box, cell),
            edge_grid: Grid::build(&edge_box, cell),
            vertices,
            triangles,
            edges,
            closed,
            tri_box,
            edge_box,
            winding_tree,
        })
    }

    /// Refit an already-admitted fixed-topology body for a prescribed-motion trial. Its triangles,
    /// unique edges and closure are retained; surface/trajectory admission remains the caller's
    /// responsibility. Geometry and all three query indices commit together, only after validation.
    pub(crate) fn refit_vertices(&mut self, vertices: Vec<Vec3>) -> Result<(), Refusal> {
        if vertices.len() != self.vertices.len()
            || self.edges.iter().flatten().any(|&i| i >= vertices.len())
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "body refit requires unchanged vertex and edge topology",
            ));
        }
        validate_mesh(&vertices, &self.triangles)?;
        let tri_box: Vec<_> = self
            .triangles
            .iter()
            .map(|t| aabb(&t.map(|i| vertices[i])))
            .collect();
        let edge_box: Vec<_> = self
            .edges
            .iter()
            .map(|e| aabb(&e.map(|i| vertices[i])))
            .collect();
        let vert_box: Vec<_> = vertices.iter().map(|&p| (p, p)).collect();
        let tri_grid = self.tri_grid.refitted(&tri_box)?;
        let edge_grid = self.edge_grid.refitted(&edge_box)?;
        let vert_grid = self.vert_grid.refitted(&vert_box)?;
        self.vertices = vertices;
        self.tri_box = tri_box;
        self.edge_box = edge_box;
        self.tri_grid = tri_grid;
        self.edge_grid = edge_grid;
        self.vert_grid = vert_grid;
        if self.closed {
            self.winding_tree.refit(&self.vertices, &self.triangles);
        }
        Ok(())
    }

    /// Exact winding number of the surface about `p` (Van Oosterom–Strackee solid angles): about 1
    /// inside a closed, outward-wound body and 0 outside.
    #[must_use]
    pub fn winding_number(&self, p: Vec3) -> f64 {
        winding_exact(&self.vertices, &self.triangles, p)
    }

    /// O(log F) dipole-accelerated winding number for closed bodies (exact for open meshes).
    #[must_use]
    pub fn winding_number_approx(&self, p: Vec3) -> f64 {
        if self.closed {
            self.winding_tree
                .winding(&self.vertices, &self.triangles, p)
        } else {
            winding_exact(&self.vertices, &self.triangles, p)
        }
    }

    /// Exact-guarded inside classification (`|w| > 0.5`) using the O(log F) dipole octree with
    /// exact solid-angle fallback near `|w| = 0.5`.
    #[must_use]
    pub fn is_inside(&self, p: Vec3) -> bool {
        if self.closed {
            self.winding_tree
                .is_inside(&self.vertices, &self.triangles, p)
        } else {
            winding_exact(&self.vertices, &self.triangles, p).abs() > 0.5
        }
    }
}

/// Why a cloth state is not intersection-free with the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intersection {
    /// Cloth edge `ce` passes through body triangle `bt`.
    ClothEdge {
        /// Cloth edge.
        ce: usize,
        /// Body triangle.
        bt: usize,
    },
    /// Body edge `be` passes through cloth triangle `ct`.
    BodyEdge {
        /// Cloth triangle.
        ct: usize,
        /// Body edge.
        be: usize,
    },
    /// The cloth piece holding vertex `cv` lies wholly inside the (closed) body.
    Inside {
        /// The piece's lowest-numbered vertex.
        cv: usize,
    },
}

/// Union–find root with path halving.
fn find(root: &mut [usize], mut i: usize) -> usize {
    while root[i] != i {
        root[i] = root[root[i]];
        i = root[i];
    }
    i
}

/// Exact closed-segment/triangle contact, including coplanar containment and boundary contact.
/// The kernel is lifted from frankensim fs-topo in `ftx_core::intersection`.
fn segment_meets_triangle(p: Vec3, q: Vec3, t: [Vec3; 3]) -> bool {
    segment_triangle(p, q, t).is_some()
}

/// A contact candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Pair {
    /// Cloth vertex against body triangle.
    VertexTri {
        /// Cloth vertex.
        cv: usize,
        /// Body triangle.
        bt: usize,
    },
    /// Body vertex against cloth triangle.
    TriVertex {
        /// Cloth triangle.
        ct: usize,
        /// Body vertex.
        bv: usize,
    },
    /// Cloth edge against body edge.
    EdgeEdge {
        /// Cloth edge.
        ce: usize,
        /// Body edge.
        be: usize,
    },
    /// Non-incident cloth vertex against cloth triangle.
    SelfVertexTri {
        /// Cloth vertex.
        cv: usize,
        /// Cloth triangle.
        ct: usize,
    },
    /// Two non-incident cloth edges, in increasing edge-index order.
    SelfEdgeEdge {
        /// First cloth edge.
        a: usize,
        /// Second cloth edge.
        b: usize,
    },
}

impl Pair {
    fn is_self_contact(self) -> bool {
        matches!(self, Self::SelfVertexTri { .. } | Self::SelfEdgeEdge { .. })
    }
}

/// The cloth side of contact: its triangles and edges (with rest lengths for the mollifier).
#[derive(Clone, Debug, PartialEq)]
pub struct ClothTopology {
    /// Triangles.
    pub triangles: Vec<[usize; 3]>,
    /// Unique edges.
    pub edges: Vec<[usize; 2]>,
    /// Rest length of each edge (m).
    pub edge_rest: Vec<f64>,
    /// Each vertex's neighbours along cloth edges, sorted.
    pub neighbours: Vec<Vec<usize>>,
}

impl ClothTopology {
    /// Whether two cloth vertices are joined by a cloth edge.
    fn joined(&self, v: usize, w: usize) -> bool {
        self.neighbours
            .get(v)
            .is_some_and(|n| n.binary_search(&w).is_ok())
    }

    /// Whether a self-contact pair's features are one ring apart (joined by a cloth edge).
    fn one_ring(&self, pair: Pair) -> bool {
        match pair {
            Pair::SelfVertexTri { cv, ct } => {
                self.triangles[ct].iter().any(|&w| self.joined(cv, w))
            }
            Pair::SelfEdgeEdge { a, b } => {
                let (ea, eb) = (self.edges[a], self.edges[b]);
                ea.iter().any(|&i| eb.iter().any(|&j| self.joined(i, j)))
            }
            _ => false,
        }
    }
}

/// A pair's four points: positions, which are fixed (body), and the cloth vertex behind each free one.
struct Stencil {
    pts: [Vec3; 4],
    fixed: [bool; 4],
    cloth: [Option<usize>; 4],
    mollifier_eps: Option<f64>,
}

fn stencil(pair: Pair, x: &[Vec3], body: &Body, cloth: &ClothTopology) -> Stencil {
    match pair {
        Pair::VertexTri { cv, bt } => {
            let t = body.triangles[bt];
            Stencil {
                pts: [
                    x[cv],
                    body.vertices[t[0]],
                    body.vertices[t[1]],
                    body.vertices[t[2]],
                ],
                fixed: [false, true, true, true],
                cloth: [Some(cv), None, None, None],
                mollifier_eps: None,
            }
        }
        Pair::TriVertex { ct, bv } => {
            let t = cloth.triangles[ct];
            Stencil {
                pts: [body.vertices[bv], x[t[0]], x[t[1]], x[t[2]]],
                fixed: [true, false, false, false],
                cloth: [None, Some(t[0]), Some(t[1]), Some(t[2])],
                mollifier_eps: None,
            }
        }
        Pair::EdgeEdge { ce, be } => {
            let (c, b) = (cloth.edges[ce], body.edges[be]);
            let lb = (body.vertices[b[0]] - body.vertices[b[1]]).norm2();
            let lc = cloth.edge_rest[ce] * cloth.edge_rest[ce];
            Stencil {
                pts: [x[c[0]], x[c[1]], body.vertices[b[0]], body.vertices[b[1]]],
                fixed: [false, false, true, true],
                cloth: [Some(c[0]), Some(c[1]), None, None],
                mollifier_eps: Some(1e-3 * lc * lb),
            }
        }
        Pair::SelfVertexTri { cv, ct } => {
            let t = cloth.triangles[ct];
            let ids = [cv, t[0], t[1], t[2]];
            Stencil {
                pts: ids.map(|i| x[i]),
                fixed: [false; 4],
                cloth: ids.map(Some),
                mollifier_eps: None,
            }
        }
        Pair::SelfEdgeEdge { a, b } => {
            let (ea, eb) = (cloth.edges[a], cloth.edges[b]);
            let ids = [ea[0], ea[1], eb[0], eb[1]];
            let (la, lb) = (cloth.edge_rest[a], cloth.edge_rest[b]);
            Stencil {
                pts: ids.map(|i| x[i]),
                fixed: [false; 4],
                cloth: ids.map(Some),
                mollifier_eps: Some(1e-3 * la * la * lb * lb),
            }
        }
    }
}

fn classify(pair: Pair, pts: &[Vec3; 4]) -> (Closest, [f64; 4]) {
    match pair {
        Pair::VertexTri { .. } | Pair::TriVertex { .. } | Pair::SelfVertexTri { .. } => {
            let (w, k) = point_triangle_closest(pts[0], pts[1], pts[2], pts[3]);
            (k, [1.0, -w[0], -w[1], -w[2]])
        }
        Pair::EdgeEdge { .. } | Pair::SelfEdgeEdge { .. } => {
            let ((s, t), k) = edge_edge_closest(pts[0], pts[1], pts[2], pts[3]);
            (k, [1.0 - s, s, -(1.0 - t), -t])
        }
    }
}

/// Barrier `−κ (g − ĝ)² ln(g/ĝ)` on a gap jet (0 < g < ĝ).
fn barrier(g: Jet<12>, p: &ContactParams) -> Jet<12> {
    let r = g - Jet::constant(p.dhat);
    // det-ok: Jet::ln, which takes its value from det::ln
    let l = (g * (1.0 / p.dhat)).ln();
    r * r * l * (-p.kappa)
}

/// One pair's energy: inactive (gap ≥ ĝ), penetrating (gap ≤ 0, the cloth surface touching or inside
/// the body: infinite energy), with derivatives, or as a bare value.
enum PairEnergy {
    Inactive,
    Penetrating,
    Active(Box<Jet<12>>),
    Value(f64),
}

/// The pair's gap (distance − ξ) in plain f64.
fn pair_gap(st: &Stencil, kind: Closest, p: &ContactParams) -> f64 {
    squared_distance_f64(st.pts, kind).sqrt() - p.thickness
}

/// The pair's energy value only (no derivatives): the cheap path for line searches.
fn pair_energy_value(st: &Stencil, kind: Closest, p: &ContactParams) -> PairEnergy {
    let gap = pair_gap(st, kind, p);
    if gap >= p.dhat {
        return PairEnergy::Inactive;
    }
    if gap <= 0.0 {
        return PairEnergy::Penetrating;
    }
    let r = gap - p.dhat;
    let mut e = -p.kappa * r * r * ftx_core::det::ln(gap / p.dhat);
    if let Some(eps) = st.mollifier_eps {
        let (a, b) = (st.pts[1] - st.pts[0], st.pts[3] - st.pts[2]);
        let c = a.cross(b).norm2();
        if c < eps {
            e *= (2.0 - c / eps) * c / eps;
        }
    }
    PairEnergy::Value(e)
}

fn pair_energy(st: &Stencil, kind: Closest, p: &ContactParams) -> PairEnergy {
    let gap = pair_gap(st, kind, p);
    if gap >= p.dhat {
        return PairEnergy::Inactive;
    }
    if gap <= 0.0 {
        return PairEnergy::Penetrating;
    }
    let d2 = squared_distance(st.pts, st.fixed, kind);
    let g = d2.sqrt() - Jet::constant(p.thickness);
    let mut e = barrier(g, p);
    if let Some(eps) = st.mollifier_eps {
        let c = edge_cross_squared(st.pts, st.fixed);
        if c.v < eps {
            let u = c * (1.0 / eps);
            let m = (Jet::constant(2.0) - u) * u;
            e = e * m;
        }
    }
    PairEnergy::Active(Box::new(e))
}

/// The contact state of a solver: the body, parameters, cloth topology and the friction lag.
#[derive(Clone, Debug)]
pub struct Contact {
    /// The body.
    pub body: Body,
    /// Parameters.
    pub params: ContactParams,
    /// Cloth triangles and edges.
    pub cloth: ClothTopology,
    self_params: Option<ContactParams>,
    adaptive_barrier: Option<AdaptiveBarrierPolicy>,
    stick_slip: Option<StickSlipFriction>,
    self_stick_slip: Option<StickSlipFriction>,
    surface_materials: Option<materials::MaterialField>,
    /// Start creases: one-ring cloth pairs already inside the thickness when self-contact was
    /// enabled, with their own offset (half their start distance), sorted by pair.
    creases: Vec<(Pair, f64)>,
    lag: Vec<FrictionPair>,
    lag_x: Vec<Vec3>,
    lag_h: f64,
}

// A liquid-domain qualification freezes physical interfaces and contact policy, not body motion
// or evolving Coulomb lag. Keep the private material field in this module's authority.
#[derive(Clone, Debug)]
pub(crate) struct FrozenPolicy {
    params: ContactParams,
    cloth: ClothTopology,
    self_params: Option<ContactParams>,
    adaptive_barrier: Option<AdaptiveBarrierPolicy>,
    stick_slip: Option<StickSlipFriction>,
    self_stick_slip: Option<StickSlipFriction>,
    surface_materials: Option<materials::MaterialField>,
    creases: Vec<(Pair, f64)>,
}

impl FrozenPolicy {
    pub(crate) fn matches(&self, contact: &Contact) -> bool {
        self.params == contact.params
            && self.cloth == contact.cloth
            && self.self_params == contact.self_params
            && self.adaptive_barrier == contact.adaptive_barrier
            && self.stick_slip == contact.stick_slip
            && self.self_stick_slip == contact.self_stick_slip
            && self.surface_materials == contact.surface_materials
            && self.creases == contact.creases
    }
}

/// One domain at an accepted endpoint. Counts are energy primitive pairs, not physical patches.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ContactDomainSample {
    /// Pairs with 0 < thickness-offset gap < their barrier activation distance.
    pub active_pairs: usize,
    /// Minimum among those active pairs, metres; absent means no active pair, never zero gap.
    pub minimum_active_gap_m: Option<f64>,
    /// The primitive attaining that gap, indexing the unchanged contact topology.
    pub minimum_active_gap_pair: Option<Pair>,
    /// That primitive's effective offset (m), including material/crease policy.
    pub minimum_active_gap_thickness_m: Option<f64>,
    /// Pairs in the frozen Coulomb lag state, which can differ from endpoint activation.
    pub friction_lag_pairs: usize,
}

/// Body and self-contact remain independent; disabled domains serialize as unavailable.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ContactSample {
    /// Static/prescribed body contact, absent for an empty collider.
    pub body: Option<ContactDomainSample>,
    /// Cloth–cloth contact, absent when disabled.
    pub self_contact: Option<ContactDomainSample>,
}

/// A frozen friction pair: normal force, tangent basis and contact weights from the lag state.
#[derive(Clone, Copy, Debug)]
struct FrictionPair {
    pair: Pair,
    beta: [f64; 4],
    tangent: [Vec3; 2],
    lambda: f64,
    // Fixed for this lag solve, using the SAME material weights at both body endpoints.
    body_slip: Option<Vec3>,
}

/// Contribution of contact to the solver: energy (∞ if penetrating), and per-pair local gradients and
/// Hessians over cloth vertices.
pub(crate) struct ContactTerms {
    pub(crate) energy: f64,
    pub(crate) blocks: Vec<Block>,
}

/// A local gradient and Hessian over the cloth coordinates of one pair: `(vertex, component)` per row.
pub(crate) struct Block {
    pub(crate) dofs: Vec<(usize, usize)>,
    pub(crate) grad: Vec<f64>,
    pub(crate) hess: Vec<f64>,
    /// Already positive semidefinite (no projection needed).
    pub(crate) psd: bool,
}

/// Restricts a 12-coordinate pair block to the cloth's coordinates (the body is static).
fn compact(
    cloth: [Option<usize>; 4],
    grad: &[f64; 12],
    hess: impl Fn(usize, usize) -> f64,
    psd: bool,
) -> Block {
    let rows: Vec<usize> = (0..12).filter(|&a| cloth[a / 3].is_some()).collect();
    Block {
        dofs: rows
            .iter()
            .filter_map(|&a| cloth[a / 3].map(|v| (v, a % 3)))
            .collect(),
        grad: rows.iter().map(|&a| grad[a]).collect(),
        hess: rows
            .iter()
            .flat_map(|&a| rows.iter().map(move |&b| (a, b)))
            .map(|(a, b)| hess(a, b))
            .collect(),
        psd,
    }
}

/// Barrier value and its first two derivatives in the gap: b, b', b'' (0 < g < ĝ).
fn barrier_f64(g: f64, p: &ContactParams) -> (f64, f64, f64) {
    let r = g - p.dhat;
    let l = ftx_core::det::ln(g / p.dhat);
    (
        -p.kappa * r * r * l,
        -p.kappa * (2.0 * r * l + r * r / g),
        -p.kappa * (2.0 * l + 4.0 * r / g - r * r / (g * g)),
    )
}

fn tangent_basis(n: Vec3) -> [Vec3; 2] {
    let a = if n.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let t1 = n.cross(a).normalized();
    [t1, n.cross(t1)]
}

impl Contact {
    pub(crate) fn frozen_policy(&self) -> FrozenPolicy {
        FrozenPolicy {
            params: self.params,
            cloth: self.cloth.clone(),
            self_params: self.self_params,
            adaptive_barrier: self.adaptive_barrier,
            stick_slip: self.stick_slip,
            self_stick_slip: self.self_stick_slip,
            surface_materials: self.surface_materials.clone(),
            creases: self.creases.clone(),
        }
    }

    /// Endpoint diagnostics for solver-accepted geometry with this contact topology.
    /// The minimum is sampled within barrier activation, not a continuous-time clearance bound.
    #[must_use]
    pub fn accepted_sample(&self, x: &[Vec3]) -> ContactSample {
        let mut sample = ContactSample {
            body: (!self.body.triangles.is_empty()).then(ContactDomainSample::default),
            self_contact: self.self_params.map(|_| ContactDomainSample::default()),
        };
        for pair in self.candidates(x, None, self.activation_range()) {
            let st = stencil(pair, x, &self.body, &self.cloth);
            let (kind, _) = classify(pair, &st.pts);
            let p = self.pair_params(pair);
            let gap = pair_gap(&st, kind, &p);
            if gap > 0. && gap < p.dhat {
                let domain = if pair.is_self_contact() {
                    &mut sample.self_contact
                } else {
                    &mut sample.body
                };
                if let Some(domain) = domain {
                    domain.active_pairs += 1;
                    if domain.minimum_active_gap_m.is_none_or(|g| gap < g) {
                        domain.minimum_active_gap_m = Some(gap);
                        domain.minimum_active_gap_pair = Some(pair);
                        domain.minimum_active_gap_thickness_m = Some(p.thickness);
                    }
                }
            }
        }
        for lag in &self.lag {
            let domain = if lag.pair.is_self_contact() {
                &mut sample.self_contact
            } else {
                &mut sample.body
            };
            if let Some(domain) = domain {
                domain.friction_lag_pairs += 1;
            }
        }
        sample
    }

    /// Final barrier reactions on indexed cloth vertices, in N, excluding lagged friction.
    /// Uses the same exact pair energy gradient (including edge mollifiers) as assembly.
    ///
    /// # Errors
    /// Invalid geometry, penetrating gaps or non-finite reactions. No physical pressure area is inferred.
    pub fn reaction_map(&self, x: &[Vec3]) -> Result<Value, Refusal> {
        self.reaction_map_bounded(x, usize::MAX).map(|(map, _)| map)
    }

    // Bound expensive derivative evaluations for optional sampled consumers. Candidate generation
    // remains the ordinary production broad phase; no pair can be dropped to meet this allowance.
    pub(crate) fn reaction_map_bounded(
        &self,
        x: &[Vec3],
        max_pairs: usize,
    ) -> Result<(Value, usize), Refusal> {
        self.check_cloth(x)?;
        let candidates = self.candidates(x, None, self.activation_range());
        let pair_count = candidates.len();
        if pair_count > max_pairs {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "contact-reaction pair-evaluation budget exhausted; no partial map published",
            ));
        }
        let mut body = vec![Vec3::ZERO; x.len()];
        let mut own = vec![Vec3::ZERO; x.len()];
        let mut policies = Vec::new();
        let mut barrier_energy_j = 0.;
        for pair in candidates {
            let st = stencil(pair, x, &self.body, &self.cloth);
            let (kind, _) = classify(pair, &st.pts);
            let p = self.pair_params(pair);
            match pair_energy(&st, kind, &p) {
                PairEnergy::Inactive => {}
                PairEnergy::Penetrating => {
                    return Err(Refusal::new(
                        RefusalCode::IntersectingStart,
                        "reaction map requires positive contact gaps",
                    ));
                }
                PairEnergy::Active(energy) => {
                    barrier_energy_j += energy.v;
                    let forces = if pair.is_self_contact() {
                        &mut own
                    } else {
                        &mut body
                    };
                    for (i, vertex) in st.cloth.iter().enumerate() {
                        if let Some(vertex) = vertex {
                            forces[*vertex] -= Vec3::new(
                                energy.g[3 * i],
                                energy.g[3 * i + 1],
                                energy.g[3 * i + 2],
                            );
                        }
                    }
                    policies.push((pair, p));
                }
                PairEnergy::Value(_) => unreachable!("pair_energy always returns derivatives"),
            }
        }
        if !barrier_energy_j.is_finite() || body.iter().chain(&own).any(|f| !f.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "contact reactions are non-finite",
            ));
        }
        let digest = |v: &Value| {
            ftx_core::receipt::sha256_hex(
                &serde_json::to_vec(v).expect("finite contact map identity"),
            )
        };
        let policy_value = if self.adaptive_barrier.is_none()
            && self.stick_slip.is_none()
            && self.self_stick_slip.is_none()
        {
            json!([self.params, self.self_params, self.creases, policies])
        } else {
            json!([
                self.params,
                self.self_params,
                self.creases,
                policies,
                self.adaptive_barrier,
                self.stick_slip,
                self.self_stick_slip
            ])
        };
        Ok((
            json!({"schema":"ftx-contact-reactions/1",
            "barrier_energy_j":barrier_energy_j,
            "vertex_basis":"welded solver vertices; forces follow positions_m",
            "positions_m":x,"triangles":self.cloth.triangles,
            "cloth_state_sha256":digest(&json!([x,self.cloth.triangles,self.cloth.edges,self.cloth.edge_rest])),
            "body_state_sha256":digest(&json!([self.body.vertices,self.body.triangles])),
            "active_policy_sha256":digest(&policy_value),
            "body_force_on_cloth_n":if self.body.triangles.is_empty() {None} else {Some(body)},
            "self_force_on_cloth_n":self.self_params.map(|_| own),
            "pressure_pa":null,
            "pressure_basis":"unavailable: no justified physical contact-area allocation",
            "basis":"negative gradient of the accepted endpoint barrier energy including edge mollifiers; each assembled primitive once; excludes Coulomb friction and measured pressure/comfort/fit"}),
            pair_count,
        ))
    }

    /// A contact state for a cloth with these triangles and rest positions (2D material coordinates
    /// lifted with z = 0 are fine: only edge lengths are used).
    #[must_use]
    pub fn new(
        body: Body,
        params: ContactParams,
        triangles: Vec<[usize; 3]>,
        edge_rest: impl Fn(usize, usize) -> f64,
    ) -> Self {
        let edges = edges_of(&triangles);
        let rest = edges.iter().map(|&[a, b]| edge_rest(a, b)).collect();
        let n = triangles.iter().flatten().max().map_or(0, |&m| m + 1);
        let mut neighbours = vec![Vec::new(); n];
        for &[a, b] in &edges {
            neighbours[a].push(b);
            neighbours[b].push(a);
        }
        neighbours.iter_mut().for_each(|v| v.sort_unstable());
        Self {
            body,
            params,
            cloth: ClothTopology {
                triangles,
                edges,
                edge_rest: rest,
                neighbours,
            },
            self_params: None,
            adaptive_barrier: None,
            stick_slip: None,
            self_stick_slip: None,
            surface_materials: None,
            lag: Vec::new(),
            lag_x: Vec::new(),
            lag_h: 1.0,
            creases: Vec::new(),
        }
    }

    /// Enables cloth self-contact after auditing the start, with independent friction and full
    /// cloth–cloth thickness. Incident features (including welded seam vertices) are excluded. A start
    /// crease, a pair joined by a cloth edge that already sits inside the thickness (where a welded
    /// seam meets at a sharp angle, or a gathered seam creases a band), keeps its barrier at half its
    /// start distance instead: it cannot cross, and no motion is needed to satisfy it.
    ///
    /// # Errors
    /// Invalid parameters or geometry, non-finite positions, or [`RefusalCode::IntersectingStart`]
    /// for crossing or touching layers. An intersecting start is refused, never pushed apart silently.
    pub fn with_self_contact(self, x: &[Vec3], params: ContactParams) -> Result<Self, Refusal> {
        self.with_self_contact_observed(x, params, &mut None)
    }

    pub(crate) fn with_self_contact_observed(
        mut self,
        x: &[Vec3],
        params: ContactParams,
        crossing: &mut Option<[usize; 2]>,
    ) -> Result<Self, Refusal> {
        *crossing = None;
        params.validate()?;
        let params = self.material_self_params(params)?;
        self.self_params = Some(params);
        self.creases.clear();
        // the creases' search reads every triangle, so the mesh is checked first
        self.check_cloth(x)?;
        self.creases = self.find_start_creases(x, &params);
        self.audit_self_contact_observed(x, crossing)?;
        Ok(self)
    }

    /// Finite positions and non-degenerate triangles with valid vertex indices.
    fn check_cloth(&self, x: &[Vec3]) -> Result<(), Refusal> {
        if let Some(f) = &self.surface_materials {
            f.validate_topology(&self.cloth, x.len())?;
        }
        validate_mesh(x, &self.cloth.triangles)
    }

    /// The start creases' count and smallest offset (m).
    #[must_use]
    pub fn start_creases(&self) -> (usize, Option<f64>) {
        let min = self.creases.iter().map(|c| c.1).reduce(f64::min);
        (self.creases.len(), min)
    }

    fn find_start_creases(&self, x: &[Vec3], p: &ContactParams) -> Vec<(Pair, f64)> {
        let mut out: Vec<(Pair, f64)> = self
            .self_candidates(x, None, p.thickness + p.dhat)
            .into_iter()
            .filter(|&pair| self.cloth.one_ring(pair))
            .filter_map(|pair| {
                let st = stencil(pair, x, &self.body, &self.cloth);
                let (kind, _) = classify(pair, &st.pts);
                let d = squared_distance_f64(st.pts, kind).sqrt();
                (d <= self.pair_params(pair).thickness && d > 0.0).then_some((pair, 0.5 * d))
            })
            .collect();
        out.sort_unstable_by_key(|c| c.0);
        out
    }

    /// The independently configured cloth–cloth parameters, or `None` when self-contact is disabled.
    #[must_use]
    pub fn self_contact_params(&self) -> Option<ContactParams> {
        self.self_params
    }

    /// The active adaptive barrier policy, if enabled.
    #[must_use]
    pub fn adaptive_barrier(&self) -> Option<AdaptiveBarrierPolicy> {
        self.adaptive_barrier
    }

    /// The active body stick-slip friction law, if enabled.
    #[must_use]
    pub fn stick_slip(&self) -> Option<StickSlipFriction> {
        self.stick_slip
    }

    /// The active cloth–cloth stick-slip friction law, if enabled.
    #[must_use]
    pub fn self_stick_slip(&self) -> Option<StickSlipFriction> {
        self.self_stick_slip
    }

    /// Configures opt-in Li et al. (2020) adaptive barrier stiffness scaling and sets the initial
    /// barrier `kappa` (for body contact and, if already enabled, self-contact) from the cloth's
    /// vertex mass, membrane stiffness, and reference gravity. Call after [`Contact::with_self_contact`]
    /// so cloth–cloth `kappa` receives the same initial scaling. Returns the resulting body `kappa` (N/m).
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] or [`RefusalCode::OutOfRange`] on invalid policy or cloth scales.
    pub fn set_adaptive_barrier(
        &mut self,
        policy: AdaptiveBarrierPolicy,
        mean_vertex_mass_kg: f64,
        membrane_stiffness_n_m: f64,
        gravity_m_s2: f64,
    ) -> Result<f64, Refusal> {
        let k0 = policy.initial_kappa(
            mean_vertex_mass_kg,
            membrane_stiffness_n_m,
            gravity_m_s2,
            self.params.dhat,
        )?;
        let k0_self = self
            .self_params
            .map(|sp| {
                policy.initial_kappa(
                    mean_vertex_mass_kg,
                    membrane_stiffness_n_m,
                    gravity_m_s2,
                    sp.dhat,
                )
            })
            .transpose()?;
        self.params.kappa = k0;
        if let (Some(sp), Some(ks)) = (self.self_params.as_mut(), k0_self) {
            sp.kappa = ks;
        }
        self.adaptive_barrier = Some(policy);
        Ok(k0)
    }

    /// Builder form of [`Contact::set_adaptive_barrier`].
    ///
    /// # Errors
    /// As [`Contact::set_adaptive_barrier`].
    pub fn with_adaptive_barrier(
        mut self,
        policy: AdaptiveBarrierPolicy,
        mean_vertex_mass_kg: f64,
        membrane_stiffness_n_m: f64,
        gravity_m_s2: f64,
    ) -> Result<Self, Refusal> {
        self.set_adaptive_barrier(
            policy,
            mean_vertex_mass_kg,
            membrane_stiffness_n_m,
            gravity_m_s2,
        )?;
        Ok(self)
    }

    /// Updates barrier `kappa` between time steps when adaptive barrier scaling is enabled and the
    /// minimum active contact gap fell below `min_gap_fraction * dhat`. No-op when disabled.
    /// Returns the active body barrier stiffness `kappa` (N/m).
    pub fn adapt_barrier_stiffness(&mut self, x: &[Vec3]) -> f64 {
        let Some(policy) = self.adaptive_barrier else {
            return self.params.kappa;
        };
        let min_body = self.min_gap(x, self.params.dhat);
        self.params.kappa = policy.adapt_kappa(self.params.kappa, min_body, self.params.dhat);
        if let Some(sp) = self.self_params {
            let min_self = self.min_self_gap(x, sp.dhat);
            let next_self = policy.adapt_kappa(sp.kappa, min_self, sp.dhat);
            if let Some(sp_mut) = self.self_params.as_mut() {
                sp_mut.kappa = next_self;
            }
        }
        self.params.kappa
    }

    /// Configures opt-in smooth static/kinetic stick-slip friction for cloth–body contact.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] if per-triangle surface materials are bound (stick-slip is
    /// a uniform interface law and will not silently override spatially assigned friction);
    /// otherwise as [`StickSlipFriction::validate`].
    pub fn set_stick_slip(&mut self, friction: StickSlipFriction) -> Result<(), Refusal> {
        friction.validate()?;
        if self.surface_materials.is_some() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "stick-slip friction is a uniform interface law and cannot override per-triangle surface materials",
            ));
        }
        self.stick_slip = Some(friction);
        Ok(())
    }

    /// Builder form of [`Contact::set_stick_slip`].
    ///
    /// # Errors
    /// See [`Contact::set_stick_slip`].
    pub fn with_stick_slip(mut self, friction: StickSlipFriction) -> Result<Self, Refusal> {
        self.set_stick_slip(friction)?;
        Ok(self)
    }

    /// Configures opt-in smooth static/kinetic stick-slip friction for cloth–cloth self-contact.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] if self-contact is not enabled or per-triangle surface
    /// materials are bound; otherwise as [`StickSlipFriction::validate`].
    pub fn set_self_stick_slip(&mut self, friction: StickSlipFriction) -> Result<(), Refusal> {
        friction.validate()?;
        if self.self_params.is_none() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "cloth-cloth stick-slip friction requires self-contact to be enabled",
            ));
        }
        if self.surface_materials.is_some() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "cloth-cloth stick-slip friction cannot override per-triangle surface materials",
            ));
        }
        self.self_stick_slip = Some(friction);
        Ok(())
    }

    /// Builder form of [`Contact::set_self_stick_slip`].
    ///
    /// # Errors
    /// As [`Contact::set_self_stick_slip`].
    pub fn with_self_stick_slip(mut self, friction: StickSlipFriction) -> Result<Self, Refusal> {
        self.set_self_stick_slip(friction)?;
        Ok(self)
    }

    pub(crate) fn restore_kappas(&mut self, body_kappa: f64, self_kappa: Option<f64>) {
        self.params.kappa = body_kappa;
        if let (Some(sp), Some(ks)) = (self.self_params.as_mut(), self_kappa) {
            sp.kappa = ks;
        }
    }

    fn pair_stick_slip(&self, pair: Pair) -> Option<StickSlipFriction> {
        if pair.is_self_contact() {
            self.self_stick_slip
        } else {
            self.stick_slip
        }
    }

    /// A pair's parameters: the body's, or the cloth–cloth ones with a start crease's own offset.
    fn pair_params(&self, pair: Pair) -> ContactParams {
        if !pair.is_self_contact() {
            return self
                .surface_materials
                .as_ref()
                .map_or(self.params, |f| f.params(pair, self.params));
        }
        let mut p = self.self_params.unwrap_or(self.params);
        if let Some(f) = &self.surface_materials {
            p = f.params(pair, p);
        }
        if let Ok(i) = self.creases.binary_search_by(|(q, _)| q.cmp(&pair)) {
            p.thickness = self.creases[i].1;
        }
        p
    }

    fn activation_range(&self) -> f64 {
        let body = self.params.dhat + self.params.thickness;
        self.self_params
            .map_or(body, |p| body.max(p.dhat + p.thickness))
    }

    pub(crate) fn has_friction(&self) -> bool {
        let body_mu = self.stick_slip.map_or(self.params.mu, |s| s.mu_s);
        let self_mu = self
            .self_params
            .map_or(0.0, |p| self.self_stick_slip.map_or(p.mu, |s| s.mu_s));
        body_mu > 0.0 || self_mu > 0.0
    }

    /// Checks a self-contact-enabled state: finite, non-degenerate triangles, positive thickness
    /// gaps, and no triangle intersections beyond shared indexed vertices/edges. Exact coplanar
    /// contacts and duplicate faces are refused; legal seam adjacency is not an intersection.
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`], [`RefusalCode::DegenerateGeometry`] or
    /// [`RefusalCode::IntersectingStart`]. With self-contact disabled there is nothing to audit.
    pub fn audit_self_contact(&self, x: &[Vec3]) -> Result<(), Refusal> {
        self.audit_self_contact_observed(x, &mut None)
    }

    fn self_pair_triangles(&self, pair: Pair) -> Option<[usize; 2]> {
        match pair {
            Pair::SelfVertexTri { cv, ct } => {
                let ta = self
                    .cloth
                    .triangles
                    .iter()
                    .enumerate()
                    .find_map(|(ti, t)| (ti != ct && t.contains(&cv)).then_some(ti))?;
                Some([ta, ct])
            }
            Pair::SelfEdgeEdge { a, b } => {
                let [a0, a1] = *self.cloth.edges.get(a)?;
                let [b0, b1] = *self.cloth.edges.get(b)?;
                let ta = self
                    .cloth
                    .triangles
                    .iter()
                    .position(|t| t.contains(&a0) && t.contains(&a1))?;
                let tb = self
                    .cloth
                    .triangles
                    .iter()
                    .enumerate()
                    .find_map(|(ti, t)| {
                        (ti != ta && t.contains(&b0) && t.contains(&b1)).then_some(ti)
                    })
                    .or_else(|| {
                        self.cloth
                            .triangles
                            .iter()
                            .position(|t| t.contains(&b0) && t.contains(&b1))
                    })?;
                Some([ta, tb])
            }
            _ => None,
        }
    }

    fn audit_self_contact_observed(
        &self,
        x: &[Vec3],
        crossing: &mut Option<[usize; 2]>,
    ) -> Result<(), Refusal> {
        if let Some(f) = &self.surface_materials {
            f.validate_topology(&self.cloth, x.len())?;
        }
        let Some(p) = self.self_params else {
            return Ok(());
        };
        self.check_cloth(x)?;
        let boxes = |ids: &[usize]| swept_box(ids, x, None);
        let tris: Vec<_> = self.cloth.triangles.iter().map(|t| boxes(t)).collect();
        for (a, b) in box_self_pairs(&tris, 0.0) {
            if indexed_triangle_pair(x, self.cloth.triangles[a], self.cloth.triangles[b]).is_some()
            {
                *crossing = Some([a, b]);
                return Err(Refusal::new(
                    RefusalCode::IntersectingStart,
                    format!(
                        "cloth triangles {a} and {b} intersect beyond their shared mesh feature"
                    ),
                )
                .repair("separate the overlapping cloth layers before enabling self-contact"));
            }
        }
        // Disjoint mid-surfaces must still satisfy their thickness offsets. Keep the original
        // force-pair exclusions and per-crease offsets; exact geometry does not replace clearance.
        for pair in self.self_candidates(x, None, p.thickness + p.dhat) {
            let st = stencil(pair, x, &self.body, &self.cloth);
            let (kind, _) = classify(pair, &st.pts);
            let gap = pair_gap(&st, kind, &self.pair_params(pair));
            if !gap.is_finite() || gap <= 0.0 {
                *crossing = self.self_pair_triangles(pair);
                return Err(Refusal::new(
                    RefusalCode::IntersectingStart,
                    format!("cloth layers touch or overlap at {pair:?} (gap {gap} m)"),
                )
                .repair("separate the layers by more than their combined half-thicknesses"));
            }
        }
        Ok(())
    }

    fn self_candidates(&self, x: &[Vec3], p: Option<&[Vec3]>, pad: f64) -> Vec<Pair> {
        let verts: Vec<_> = (0..x.len()).map(|i| swept_box(&[i], x, p)).collect();
        let tris: Vec<_> = self
            .cloth
            .triangles
            .iter()
            .map(|t| swept_box(t, x, p))
            .collect();
        let edges: Vec<_> = self
            .cloth
            .edges
            .iter()
            .map(|e| swept_box(e, x, p))
            .collect();
        let mut pairs = Vec::new();
        for (cv, ct) in box_pairs(&verts, &tris, pad) {
            if !self.cloth.triangles[ct].contains(&cv) {
                pairs.push(Pair::SelfVertexTri { cv, ct });
            }
        }
        for (a, b) in box_self_pairs(&edges, pad) {
            if !self.cloth.edges[a]
                .iter()
                .any(|i| self.cloth.edges[b].contains(i))
            {
                pairs.push(Pair::SelfEdgeEdge { a, b });
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        pairs
    }

    /// Candidate pairs whose primitives' boxes (from `x` to `x + p`, if given) come within `pad` of each other.
    pub(crate) fn candidates(&self, x: &[Vec3], p: Option<&[Vec3]>, pad: f64) -> Vec<Pair> {
        let mut out = self.body_candidates(x, p, pad);
        if self.self_params.is_some() {
            out.extend(self.self_candidates(x, p, pad));
        }
        out
    }

    fn body_candidates(&self, x: &[Vec3], p: Option<&[Vec3]>, pad: f64) -> Vec<Pair> {
        let b = &self.body;
        let mut out = Vec::new();
        if b.triangles.is_empty() {
            return out;
        }
        let mut buf = Vec::new();
        let span = |ids: &[usize]| -> (Vec3, Vec3) {
            let mut pts: Vec<Vec3> = ids.iter().map(|&i| x[i]).collect();
            if let Some(p) = p {
                pts.extend(ids.iter().map(|&i| x[i] + p[i]));
            }
            let (lo, hi) = aabb(&pts);
            (lo - Vec3::new(pad, pad, pad), hi + Vec3::new(pad, pad, pad))
        };
        // Grid cells give a superset; the exact box test keeps the pairs whose boxes come within `pad`.
        for cv in 0..x.len() {
            let (lo, hi) = span(&[cv]);
            b.tri_grid.query(lo, hi, &mut buf);
            out.extend(
                buf.iter()
                    .filter(|&&bt| overlaps(b.tri_box[bt], lo, hi))
                    .map(|&bt| Pair::VertexTri { cv, bt }),
            );
        }
        for (ct, t) in self.cloth.triangles.iter().enumerate() {
            let (lo, hi) = span(t);
            b.vert_grid.query(lo, hi, &mut buf);
            out.extend(
                buf.iter()
                    .filter(|&&bv| overlaps((b.vertices[bv], b.vertices[bv]), lo, hi))
                    .map(|&bv| Pair::TriVertex { ct, bv }),
            );
        }
        for (ce, e) in self.cloth.edges.iter().enumerate() {
            let (lo, hi) = span(e);
            b.edge_grid.query(lo, hi, &mut buf);
            out.extend(
                buf.iter()
                    .filter(|&&be| overlaps(b.edge_box[be], lo, hi))
                    .map(|&be| Pair::EdgeEdge { ce, be }),
            );
        }
        out
    }

    /// Smallest cloth–body gap (distance − ξ) within `range` (m), or `None` if none. Self-contact
    /// does not affect body-clearance measurements or the body's thickness ramp.
    #[must_use]
    pub fn min_gap(&self, x: &[Vec3], range: f64) -> Option<f64> {
        self.closest_pair(x, range).map(|(g, _, _)| g)
    }

    /// Smallest cloth–cloth gap within `range` (m), or `None` when self-contact is disabled or no
    /// pair's boxes are nearby. This uses the cloth–cloth thickness (a start crease's own offset),
    /// independent of the body.
    #[must_use]
    pub fn min_self_gap(&self, x: &[Vec3], range: f64) -> Option<f64> {
        let p = self.self_params?;
        self.self_candidates(x, None, range + p.thickness)
            .into_iter()
            .map(|pair| {
                let st = stencil(pair, x, &self.body, &self.cloth);
                let (kind, _) = classify(pair, &st.pts);
                pair_gap(&st, kind, &self.pair_params(pair))
            })
            .reduce(f64::min)
    }

    /// The pair with the smallest gap within `range` (m): its gap, the pair and the cloth point of it
    /// (the cloth vertex, or the first cloth vertex of the cloth triangle or edge).
    #[must_use]
    pub fn closest_pair(&self, x: &[Vec3], range: f64) -> Option<(f64, Pair, Vec3)> {
        self.body_candidates(x, None, range + self.params.thickness)
            .into_iter()
            .map(|pair| {
                let st = stencil(pair, x, &self.body, &self.cloth);
                let (kind, _) = classify(pair, &st.pts);
                let at = st
                    .cloth
                    .iter()
                    .flatten()
                    .next()
                    .map_or(Vec3::ZERO, |&v| x[v]);
                (pair_gap(&st, kind, &self.pair_params(pair)), pair, at)
            })
            .reduce(|a, b| if b.0 < a.0 { b } else { a })
    }

    /// Up to `limit` crossings of the cloth at `x` with the body (exact predicates), in a fixed order: each
    /// cloth edge through a body triangle (the first such triangle), then each cloth triangle pierced by
    /// a body edge (the first such edge).
    ///
    /// # Panics
    /// On a non-finite position.
    #[must_use]
    pub fn crossings(&self, x: &[Vec3], limit: usize) -> Vec<Intersection> {
        let b = &self.body;
        let mut out = Vec::new();
        let mut buf = Vec::new();
        for (ce, e) in self.cloth.edges.iter().enumerate() {
            if out.len() >= limit {
                return out;
            }
            let (lo, hi) = aabb(&e.map(|i| x[i]));
            b.tri_grid.query(lo, hi, &mut buf);
            if let Some(&bt) = buf.iter().find(|&&bt| {
                segment_meets_triangle(x[e[0]], x[e[1]], b.triangles[bt].map(|i| b.vertices[i]))
            }) {
                out.push(Intersection::ClothEdge { ce, bt });
            }
        }
        for (ct, t) in self.cloth.triangles.iter().enumerate() {
            if out.len() >= limit {
                return out;
            }
            let tri = t.map(|i| x[i]);
            let (lo, hi) = aabb(&tri);
            b.edge_grid.query(lo, hi, &mut buf);
            if let Some(&be) = buf.iter().find(|&&be| {
                let [p, q] = b.edges[be].map(|i| b.vertices[i]);
                segment_meets_triangle(p, q, tri)
            }) {
                out.push(Intersection::BodyEdge { ct, be });
            }
        }
        out
    }

    /// The first way the cloth at `x` crosses the body, or `None` when it is intersection-free: an edge
    /// of either mesh through a triangle of the other (exact predicates), or (for a closed body) a
    /// cloth piece wholly inside it. Near-contact without crossing is [`Contact::min_gap`]'s business.
    ///
    /// # Panics
    /// On a non-finite position (check first, as [`crate::Solver::with_contact`] does).
    #[must_use]
    pub fn first_intersection(&self, x: &[Vec3]) -> Option<Intersection> {
        if let Some(&i) = self.crossings(x, 1).first() {
            return Some(i);
        }
        let b = &self.body;
        if b.closed {
            // Nothing crosses, so each piece is wholly inside or outside: test one vertex per piece.
            let mut root: Vec<usize> = (0..x.len()).collect();
            for t in &self.cloth.triangles {
                for k in 1..3 {
                    let (ra, rb) = (find(&mut root, t[0]), find(&mut root, t[k]));
                    root[ra.max(rb)] = ra.min(rb);
                }
            }
            let used: std::collections::BTreeSet<usize> =
                self.cloth.triangles.iter().flatten().copied().collect();
            for cv in used {
                if find(&mut root, cv) == cv && b.is_inside(x[cv]) {
                    return Some(Intersection::Inside { cv });
                }
            }
        }
        None
    }

    /// Barrier (and, during a time step, friction) energy and its derivative blocks at `x`.
    pub(crate) fn terms(&self, x: &[Vec3], with_derivatives: bool, friction: bool) -> ContactTerms {
        let mut terms = ContactTerms {
            energy: 0.0,
            blocks: Vec::new(),
        };
        for pair in self.candidates(x, None, self.activation_range()) {
            let p = &self.pair_params(pair);
            let st = stencil(pair, x, &self.body, &self.cloth);
            let (kind, _) = classify(pair, &st.pts);
            if !with_derivatives {
                match pair_energy_value(&st, kind, p) {
                    PairEnergy::Inactive => {}
                    PairEnergy::Penetrating => {
                        terms.energy = f64::INFINITY;
                        return terms;
                    }
                    PairEnergy::Value(e) => terms.energy += e,
                    PairEnergy::Active(e) => terms.energy += e.v,
                }
                continue;
            }
            let gap = pair_gap(&st, kind, p);
            if gap >= p.dhat {
                continue;
            }
            if gap <= 0.0 {
                terms.energy = f64::INFINITY;
                return terms;
            }
            let mollified = st.mollifier_eps.is_some_and(|eps| {
                let (a, b) = (st.pts[1] - st.pts[0], st.pts[3] - st.pts[2]);
                a.cross(b).norm2() < eps
            });
            if mollified {
                // Near-parallel edges: the full Hessian of mollifier × barrier, projected later.
                if let PairEnergy::Active(e) = pair_energy(&st, kind, p) {
                    terms.energy += e.v;
                    terms
                        .blocks
                        .push(compact(st.cloth, &e.g, |a, b| e.h[a][b], false));
                }
                continue;
            }
            // Gauss–Newton: keep b''·∇g∇gᵀ, drop the indefinite b'·∇²g (exact gradient, PSD Hessian;
            // exact for a point against a plane).
            let d2 = squared_distance_grad(st.pts, st.fixed, kind);
            let d = d2.v.sqrt();
            let dg: [f64; 12] = std::array::from_fn(|i| d2.g[i] / (2.0 * d));
            let (b, b1, b2) = barrier_f64(d - p.thickness, p);
            terms.energy += b;
            let grad: [f64; 12] = std::array::from_fn(|i| b1 * dg[i]);
            terms
                .blocks
                .push(compact(st.cloth, &grad, |i, j| b2 * dg[i] * dg[j], true));
        }
        if friction && self.has_friction() {
            for f in &self.lag {
                let (e, g, h) = self.friction_local(f, x);
                terms.energy += e;
                if with_derivatives {
                    let st = stencil(f.pair, x, &self.body, &self.cloth);
                    terms
                        .blocks
                        .push(compact(st.cloth, &g, |a, b| h[a * 12 + b], true));
                }
            }
        }
        terms
    }

    /// Freezes the friction pairs (normal forces, tangent bases, contact weights) at `x` for a step of
    /// size `h` starting at `x_start` (IPC lagged friction).
    pub(crate) fn update_lag(&mut self, x_start: &[Vec3], x: &[Vec3], h: f64) {
        self.update_lag_motion(x_start, x, h, None);
    }

    pub(crate) fn update_lag_motion(
        &mut self,
        x_start: &[Vec3],
        x: &[Vec3],
        h: f64,
        body_start: Option<&[Vec3]>,
    ) {
        self.lag.clear();
        self.lag_x = x_start.to_vec();
        self.lag_h = h;
        if !self.has_friction() {
            return;
        }
        // Normal forces, tangents and contact weights from `x` (the latest iterate); slip is measured
        // from the start of the step.
        for pair in self.candidates(x, None, self.activation_range()) {
            let p = self.pair_params(pair);
            let active_mu = self.pair_stick_slip(pair).map_or(p.mu, |s| s.mu_s);
            if active_mu <= 0.0 {
                continue;
            }
            let st = stencil(pair, x, &self.body, &self.cloth);
            let (kind, beta) = classify(pair, &st.pts);
            let d2 = squared_distance_f64(st.pts, kind);
            let d = d2.sqrt();
            let gap = d - p.thickness;
            if !(gap > 0.0 && gap < p.dhat) {
                continue;
            }
            // Normal force λ = −∂b/∂gap = κ·[2r·ln(g/ĝ) + r²/g], r = g − ĝ < 0 (positive inside the
            // barrier's range; the mollifier is left out of the friction magnitude, as in IPC).
            let r = gap - p.dhat;
            let lambda = p.kappa * (2.0 * r * ftx_core::det::ln(gap / p.dhat) + r * r / gap);
            if lambda.is_nan() || lambda <= 0.0 {
                continue;
            }
            let ca: Vec3 = (0..4)
                .filter(|&i| beta[i] > 0.0)
                .map(|i| st.pts[i] * beta[i])
                .fold(Vec3::ZERO, |a, b| a + b);
            let cb: Vec3 = (0..4)
                .filter(|&i| beta[i] < 0.0)
                .map(|i| st.pts[i] * -beta[i])
                .fold(Vec3::ZERO, |a, b| a + b);
            let n = (ca - cb).normalized();
            self.lag.push(FrictionPair {
                pair,
                beta,
                tangent: tangent_basis(n),
                lambda,
                body_slip: body_start.filter(|_| !pair.is_self_contact()).map(|start| {
                    let d = motion::body_delta(pair, start, &self.body.vertices, &self.body);
                    (0..4)
                        .map(|i| d[i] * beta[i])
                        .fold(Vec3::ZERO, |a, b| a + b)
                }),
            });
        }
    }

    /// Smoothed Coulomb friction `μλ f₀(|u|)`, u the tangential relative displacement since the lag state.
    fn friction_local(&self, f: &FrictionPair, x: &[Vec3]) -> (f64, [f64; 12], Box<[f64; 144]>) {
        let st_now = stencil(f.pair, x, &self.body, &self.cloth);
        let st_lag = stencil(f.pair, &self.lag_x, &self.body, &self.cloth);
        let mut rel: Vec3 = (0..4)
            .map(|i| (st_now.pts[i] - st_lag.pts[i]) * f.beta[i])
            .fold(Vec3::ZERO, |a, b| a + b);
        if let Some(slip) = f.body_slip {
            rel += slip;
        }
        let u = [f.tangent[0].dot(rel), f.tangent[1].dot(rel)];
        let y = (u[0] * u[0] + u[1] * u[1]).sqrt();
        let params = self.pair_params(f.pair);
        let eps = params.eps_v * self.lag_h;
        let (energy, gu, hu) = if let Some(ss) = self.pair_stick_slip(f.pair) {
            let rad = stick_slip_radial(y, eps, ss, self.lag_h);
            let gu = [
                f.lambda * rad.tau_over_y * u[0],
                f.lambda * rad.tau_over_y * u[1],
            ];
            let mut hu = [
                [f.lambda * rad.tau_over_y, 0.0],
                [0.0, f.lambda * rad.tau_over_y],
            ];
            if y > 0.0 {
                // Use unit direction u / y rather than dividing by y^2 so subnormal |u| ~ 1e-160
                // never overflows to inf * 0 = NaN.
                let dir = [u[0] / y, u[1] / y];
                let diff = f.lambda * rad.radial_psd_minus_tau_over_y;
                for a in 0..2 {
                    for b in 0..2 {
                        hu[a][b] += diff * dir[a] * dir[b];
                    }
                }
            }
            (f.lambda * rad.d_val, gu, hu)
        } else {
            let c = params.mu * f.lambda;
            let (f0, f1_over_y, hess_coef) = if y < eps {
                (
                    -y * y * y / (3.0 * eps * eps) + y * y / eps + eps / 3.0,
                    -y / (eps * eps) + 2.0 / eps,
                    -1.0 / (eps * eps * y.max(1e-300)),
                )
            } else {
                (y, 1.0 / y, -1.0 / (y * y * y))
            };
            // ∂D/∂u = c·f₁(y)/y·u; ∂²D/∂u² = c·[(f₁/y)·I + hess_coef·u uᵀ] (PSD for y ≤ ε and beyond).
            let gu = [c * f1_over_y * u[0], c * f1_over_y * u[1]];
            let mut hu = [[c * f1_over_y, 0.0], [0.0, c * f1_over_y]];
            if y > 0.0 {
                for a in 0..2 {
                    for b in 0..2 {
                        hu[a][b] += c * hess_coef * u[a] * u[b];
                    }
                }
            }
            (c * f0, gu, hu)
        };
        // u = A·x_local with A (2 × 12) = Tᵀ [β₀I β₁I β₂I β₃I].
        let a: [[f64; 12]; 2] =
            std::array::from_fn(|r| std::array::from_fn(|j| f.beta[j / 3] * f.tangent[r][j % 3]));
        let mut g = [0.0; 12];
        let mut h = Box::new([0.0; 144]);
        for i in 0..12 {
            g[i] = a[0][i] * gu[0] + a[1][i] * gu[1];
            for j in 0..12 {
                let mut s = 0.0;
                for r in 0..2 {
                    for q in 0..2 {
                        s += a[r][i] * hu[r][q] * a[q][j];
                    }
                }
                h[i * 12 + j] = s;
            }
        }
        (energy, g, h)
    }

    /// The largest fraction α ∈ [0, 1] of the step `p` that keeps every pair's gap positive (additive
    /// CCD: no gap closes below a tenth of its current value).
    pub(crate) fn step_bound(&self, x: &[Vec3], p: &[Vec3]) -> f64 {
        let mut alpha: f64 = 1.0;
        let thickness = self.self_params.map_or(self.params.thickness, |s| {
            self.params.thickness.max(s.thickness)
        });
        for pair in self.candidates(x, Some(p), thickness) {
            let st = stencil(pair, x, &self.body, &self.cloth);
            let dp: [Vec3; 4] = std::array::from_fn(|i| st.cloth[i].map_or(Vec3::ZERO, |v| p[v]));
            let bound = accd(pair, st.pts, dp, self.pair_params(pair).thickness, alpha);
            if !bound.is_finite() || bound <= 0.0 {
                return 0.0;
            }
            alpha = alpha.min(bound);
        }
        alpha
    }
}

/// Additive CCD (Li et al. 2021, Algorithm 1, s = 0.1): a time t ∈ [0, t_max] before which the pair's
/// gap stays above 10 % of its start value along the linear motion `pts + t·dp`, or `t_max` if it does.
/// Returns `0.0` when the starting gap is non-positive or when inputs/motion are invalid.
#[must_use]
pub fn accd(pair: Pair, pts: [Vec3; 4], dp: [Vec3; 4], xi: f64, t_max: f64) -> f64 {
    if !(xi.is_finite() && xi >= 0.0 && t_max.is_finite() && t_max >= 0.0)
        || pts.iter().chain(&dp).any(|v| !v.is_finite())
    {
        return 0.0;
    }
    let s = 0.1; // C-IPC's choice: a step may close a gap to 10 % of its start value
    let mean = dp.iter().fold(Vec3::ZERO, |a, b| a + *b) / 4.0;
    let rel: [Vec3; 4] = dp.map(|d| d - mean);
    if rel.iter().any(|v| !v.is_finite()) {
        return 0.0;
    }
    let (ga, gb): (&[Vec3], &[Vec3]) = match pair {
        Pair::VertexTri { .. } | Pair::TriVertex { .. } | Pair::SelfVertexTri { .. } => {
            (&rel[..1], &rel[1..])
        }
        Pair::EdgeEdge { .. } | Pair::SelfEdgeEdge { .. } => (&rel[..2], &rel[2..]),
    };
    let lp = ga.iter().map(|v| v.norm()).fold(0.0, f64::max)
        + gb.iter().map(|v| v.norm()).fold(0.0, f64::max);
    if !lp.is_finite() {
        return 0.0;
    }
    let dist2 = |q: &[Vec3; 4]| -> f64 {
        let (kind, _) = classify(pair, q);
        squared_distance_f64(*q, kind)
    };
    let gap_of = |d2: f64| -> f64 {
        if !(d2.is_finite() && d2 >= 0.0) {
            return f64::NAN;
        }
        let denom = d2.sqrt() + xi;
        if denom == 0.0 {
            0.0
        } else {
            (d2 - xi * xi) / denom
        }
    };
    let mut q = pts;
    let mut d2 = dist2(&q);
    let gap0 = gap_of(d2);
    if !(gap0.is_finite() && gap0 > 0.0) {
        return 0.0;
    }
    if lp == 0.0 || t_max == 0.0 {
        return t_max;
    }
    let target = s * gap0;
    let mut t = 0.0;
    let mut tl = (1.0 - s) * gap0 / lp;
    for _ in 0..10_000 {
        if !(tl.is_finite() && tl > 0.0) {
            return t;
        }
        for (qi, ri) in q.iter_mut().zip(&rel) {
            *qi += *ri * tl;
        }
        d2 = dist2(&q);
        let gap = gap_of(d2);
        if !gap.is_finite() || gap <= 0.0 {
            return t;
        }
        if t > 0.0 && gap < target {
            return t;
        }
        t += tl;
        if t > t_max {
            return t_max;
        }
        tl = 0.9 * gap / lp;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::Vec2;

    #[test]
    fn grid_falls_back_before_dense_storage_or_integer_ranges_explode() {
        let one = (Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
        assert!(Grid::dense(&[one], 0.25).is_some());
        assert!(Grid::build(&[], 1.0).fallback.is_none());
        // Cell-volume cap, then entry cap (each of nine boxes occupies 101^3 cells).
        assert!(Grid::build(&[one], 1e-3).fallback.is_some());
        assert!(Grid::build(&[one; 9], 0.01).fallback.is_some());
        // Float-to-integer aliasing, and a span whose subtraction overflows i64.
        for (a, b) in [(1e30, 1e30), (-9e18, 9e18)] {
            let boxes = [(Vec3::new(a, 0.0, 0.0), Vec3::new(b, 1.0, 1.0))];
            let index = Grid::build(&boxes, 1.0);
            assert!(index.fallback.is_some());
            let mut out = vec![999];
            index.query(boxes[0].0, boxes[0].1, &mut out);
            assert_eq!(out, [0]);
        }
    }

    #[test]
    fn dense_and_fallback_queries_match_after_the_same_exact_box_filter() {
        let boxes: Vec<_> = (0..200)
            .map(|i| {
                let lo = Vec3::new((i % 11) as f64, (i % 17) as f64, (i % 7) as f64);
                (lo, lo + Vec3::new(0.4, 0.7, 0.2))
            })
            .collect();
        let dense = Grid::build(&boxes, 1.0);
        let fallback = Grid::build(&boxes, 1e-6);
        assert!(dense.fallback.is_none());
        assert!(fallback.fallback.is_some());
        let mut out = Vec::new();
        for (lo, hi) in boxes.iter().copied().chain([
            (Vec3::ZERO, Vec3::ZERO),
            (Vec3::new(-10.0, -10.0, -10.0), Vec3::new(30.0, 30.0, 30.0)),
            (Vec3::new(50.0, 50.0, 50.0), Vec3::new(60.0, 60.0, 60.0)),
        ]) {
            let expected: Vec<_> = (0..boxes.len())
                .filter(|&i| overlaps(boxes[i], lo, hi))
                .collect();
            for index in [&dense, &fallback] {
                index.query(lo, hi, &mut out);
                assert!(out.windows(2).all(|w| w[0] < w[1]));
                out.retain(|&i| overlaps(boxes[i], lo, hi));
                assert_eq!(out, expected);
            }
        }
    }

    fn params() -> ContactParams {
        ContactParams {
            dhat: 1e-3,
            thickness: 5e-4,
            kappa: 10.0,
            mu: 0.3,
            eps_v: 1e-3,
        }
    }

    #[test]
    fn body_refit_matches_rebuild_and_commits_geometry_with_all_query_indices() {
        let patch = [
            Vec3::ZERO,
            Vec3::new(0.04, 0.0, 0.0),
            Vec3::new(0.0, 0.04, 0.0),
        ];
        let positions = |phase: usize| {
            (0..20)
                .flat_map(|i| {
                    let location = (7 * i + 3 * phase) % 20;
                    let origin = Vec3::new(
                        (location % 5) as f64 * 0.25 + phase as f64 * 5.0,
                        (location / 5) as f64 * 0.25,
                        (location % 3) as f64 * 0.01,
                    );
                    patch.map(|p| p + origin)
                })
                .collect::<Vec<_>>()
        };
        let triangles: Vec<_> = (0..20).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect();
        let mut body = Body::new(positions(0), triangles.clone(), 0.01).unwrap();
        let original_edges = body.edges.clone();
        for phase in 1..=3 {
            // First convert the dense grids; then move primitives between the cached BVH leaves.
            body.refit_vertices(positions(phase)).unwrap();
            assert_eq!(body.triangles, triangles);
            assert_eq!(body.edges, original_edges);
            let rebuilt = Body::new(body.vertices.clone(), triangles.clone(), 0.01).unwrap();
            assert_eq!(body.closed, rebuilt.closed);
            let c = Contact::new(body.clone(), params(), vec![[0, 1, 2]], |_, _| 0.04);
            let reference = Contact::new(rebuilt, params(), vec![[0, 1, 2]], |_, _| 0.04);
            for &t in &triangles {
                let x = t.map(|i| body.vertices[i] + Vec3::new(0.0, 0.0, 0.002));
                let movement = [Vec3::new(0.3, 0.1, 0.0); 3];
                for sweep in [None, Some(movement.as_slice())] {
                    assert_eq!(
                        c.candidates(&x, sweep, 0.01),
                        reference.candidates(&x, sweep, 0.01)
                    );
                }
                assert_eq!(c.closest_pair(&x, 0.01), reference.closest_pair(&x, 0.01));
                let touching = t.map(|i| body.vertices[i]);
                assert_eq!(
                    c.crossings(&touching, 10),
                    reference.crossings(&touching, 10)
                );
                let target: Vec<_> = body
                    .vertices
                    .iter()
                    .map(|p| *p + Vec3::new(0.0, 0.0, 0.004))
                    .collect();
                let swept = c.motion_bound(&x, &x, &target, 10_000).unwrap();
                assert!(swept.fraction < 0.375);
                assert_eq!(
                    swept,
                    reference.motion_bound(&x, &x, &target, 10_000).unwrap()
                );
            }

            let before = format!("{body:?}");
            let mut nonfinite = body.vertices.clone();
            nonfinite[0].z = f64::NAN;
            let mut collapsed = body.vertices.clone();
            collapsed[0] = collapsed[1];
            for invalid in [body.vertices[..59].to_vec(), nonfinite, collapsed] {
                assert!(body.refit_vertices(invalid).is_err());
                assert_eq!(format!("{body:?}"), before);
            }
        }
    }

    #[test]
    fn endpoint_map_matches_an_independent_plane_barrier_and_keeps_domains_separate() {
        let p = params();
        let gap = 0.0004;
        let x = vec![
            Vec3::new(-0.01, -0.01, p.thickness + gap),
            Vec3::new(0.01, -0.01, p.thickness + gap),
            Vec3::new(0., 0.01, p.thickness + gap),
        ];
        let body = Body::new(
            vec![
                Vec3::new(-1., -1., 0.),
                Vec3::new(1., -1., 0.),
                Vec3::new(0., 1., 0.),
            ],
            vec![[0, 1, 2]],
            0.1,
        )
        .unwrap();
        let mut contact = Contact::new(body, p, vec![[0, 1, 2]], |a, b| (x[a] - x[b]).norm());
        contact.update_lag(&x, &x, 0.01);
        let before = format!("{contact:?}");
        let sample = contact.accepted_sample(&x);
        let domain = sample.body.unwrap();
        assert_eq!(domain.active_pairs, 3);
        assert_eq!(domain.friction_lag_pairs, 3);
        assert!((domain.minimum_active_gap_m.unwrap() - gap).abs() < 1e-15);
        assert_eq!(domain.minimum_active_gap_thickness_m, Some(p.thickness));
        assert!(sample.self_contact.is_none());
        let map = contact.reaction_map(&x).unwrap();
        let (bounded, pairs) = contact.reaction_map_bounded(&x, usize::MAX).unwrap();
        assert_eq!(bounded, map);
        assert!(pairs >= 3);
        assert_eq!(contact.reaction_map_bounded(&x, pairs).unwrap().0, map);
        assert!(contact.reaction_map_bounded(&x, pairs - 1).is_err());
        let r = gap - p.dhat;
        // Independent derivative of -kappa*(g-dhat)^2*ln(g/dhat), in N.
        let expected = p.kappa * (2. * r * (gap / p.dhat).ln() + r * r / gap);
        let forces: Vec<Vec3> =
            serde_json::from_value(map["body_force_on_cloth_n"].clone()).unwrap();
        for force in forces {
            assert!(force.x.abs() < 1e-12 && force.y.abs() < 1e-12);
            assert!((force.z - expected).abs() < 1e-12);
        }
        assert!(map["self_force_on_cloth_n"].is_null());
        assert!(map["pressure_pa"].is_null());
        assert_eq!(format!("{contact:?}"), before);
        let away: Vec<_> = x.iter().map(|p| *p + Vec3::new(0., 0., 1.)).collect();
        let quiet = contact.accepted_sample(&away).body.unwrap();
        assert_eq!(quiet.active_pairs, 0);
        assert!(quiet.minimum_active_gap_m.is_none());
        assert_ne!(
            map["cloth_state_sha256"],
            contact.reaction_map(&away).unwrap()["cloth_state_sha256"]
        );
        let mut invalid = x;
        invalid[0].z = f64::NAN;
        assert!(contact.reaction_map(&invalid).is_err());
    }

    #[test]
    fn self_reaction_map_balances_both_layers_without_a_body_reaction() {
        let x = layers(0.0012);
        let contact = self_layers(&x).with_self_contact(&x, params()).unwrap();
        let sample = contact.accepted_sample(&x);
        assert!(sample.body.is_none());
        assert!(sample.self_contact.unwrap().active_pairs > 0);
        let map = contact.reaction_map(&x).unwrap();
        assert!(map["body_force_on_cloth_n"].is_null());
        let forces: Vec<Vec3> =
            serde_json::from_value(map["self_force_on_cloth_n"].clone()).unwrap();
        assert!(forces.iter().copied().fold(Vec3::ZERO, |a, b| a + b).norm() < 1e-12);
        assert!(forces[..3].iter().map(|f| f.z).sum::<f64>() < 0.);
        assert!(forces[3..].iter().map(|f| f.z).sum::<f64>() > 0.);
    }

    /// KAT 1, barrier: zero with zero slope at the activation gap, infinite at contact, and the pair
    /// energies' gradients and Hessians equal both nested `Dual2<12>` (to machine precision) and central
    /// differences for every pair type, including the mollified edge–edge case.
    #[test]
    fn barrier_and_pair_calculus() {
        use crate::ad::{Dual, Dual2, Scalar, gradcheck};
        use crate::distance::{edge_cross_squared_as, squared_distance_as};

        let p = params();
        let at = |g: f64| barrier(Jet::<12>::var(g, 0), &p);
        let at_dual2 = |g: f64| -> Dual2<12> {
            let g = Dual2::<12>::var(g, 0);
            let r = g - Dual2::constant(p.dhat);
            r * r * (g * (1.0 / p.dhat)).ln() * (-p.kappa)
        };
        let edge = at(p.dhat * (1.0 - 1e-9));
        assert!(
            edge.v.abs() < 1e-20 && edge.g[0].abs() < 1e-9,
            "C¹ at ĝ: {} {}",
            edge.v,
            edge.g[0]
        );
        assert!(at(1e-9).g[0] < -1e-3, "repels near contact");
        // Barrier derivatives against nested Dual2<12>, gradcheck, and differences of the value.
        for g in [1e-5, 1e-4, 5e-4, 9e-4] {
            let (b, d2, h) = (at(g), at_dual2(g), 1e-9);
            assert!(
                (d2.value() - b.v).abs() <= 1e-12 * b.v.abs().max(1.0)
                    && (d2.grad()[0] - b.g[0]).abs() <= 1e-12 * b.g[0].abs()
                    && (d2.hess()[0][0] - b.h[0][0]).abs() <= 1e-12 * b.h[0][0].abs(),
                "Dual2 barrier at {g}"
            );
            let gc = gradcheck(
                |[u]: [Dual<1>; 1]| {
                    let gv = u * g;
                    let r = gv - Dual::constant(p.dhat);
                    r * r * (gv * (1.0 / p.dhat)).ln() * (-p.kappa)
                },
                [1.0],
                5e-6,
            );
            assert!(gc.pass, "barrier gradcheck at {g}: {gc:?}");
            let fd = (at(g + h).v - at(g - h).v) / (2.0 * h);
            let fd2 = (at(g + h).g[0] - at(g - h).g[0]) / (2.0 * h);
            assert!((fd - b.g[0]).abs() < 1e-6 * b.g[0].abs(), "b' at {g}");
            assert!(
                (fd2 - b.h[0][0]).abs() < 1e-5 * b.h[0][0].abs(),
                "b'' at {g}"
            );
        }
        // A body "tent" whose apex pokes up under a two-triangle cloth patch about 1.1 mm above the
        // base, with one cloth edge nearly parallel to a body edge (mollifier active).
        let body = Body::new(
            vec![
                Vec3::new(-0.01, -0.01, 0.0),
                Vec3::new(0.01, -0.01, 0.0),
                Vec3::new(0.0, 0.012, 0.0),
                Vec3::new(0.0005, 0.0003, 0.0002),
            ],
            vec![[0, 1, 3], [1, 2, 3], [2, 0, 3]],
            1e-3,
        )
        .unwrap();
        let cloth_x = vec![
            Vec3::new(0.001, 0.0005, 0.0012),
            Vec3::new(-0.004, 0.003, 0.0011),
            Vec3::new(0.004, -0.002, 0.0013),
            Vec3::new(-0.006, -0.00636, 0.0011), // edge 3–0 within ~0.1° of body edge 0–3
        ];
        let tris = vec![[0, 1, 2], [3, 0, 1]];
        let contact = Contact::new(body, p, tris, |a, b| (cloth_x[a] - cloth_x[b]).norm());
        let mut seen = std::collections::BTreeSet::new();
        let mut mollified = 0;
        for pair in contact.candidates(&cloth_x, None, p.dhat + p.thickness) {
            let energy = |x: &[Vec3]| -> Option<Jet<12>> {
                let st = stencil(pair, x, &contact.body, &contact.cloth);
                let (kind, _) = classify(pair, &st.pts);
                match pair_energy(&st, kind, &p) {
                    PairEnergy::Active(e) => Some(*e),
                    _ => None,
                }
            };
            let Some(e) = energy(&cloth_x) else { continue };
            let st = stencil(pair, &cloth_x, &contact.body, &contact.cloth);
            let (kind, _) = classify(pair, &st.pts);
            seen.insert(match pair {
                Pair::VertexTri { .. } => 0,
                Pair::TriVertex { .. } => 1,
                Pair::EdgeEdge { .. } => 2,
                _ => unreachable!("self-contact is disabled in this body-contact test"),
            });
            let gscale = e.g.iter().fold(1e-12f64, |m, g| m.max(g.abs()));
            let hscale = e.h.iter().flatten().fold(1e-12f64, |m, g| m.max(g.abs()));

            // Independent nested Dual2<12> oracle over the pair's 12 coordinates.
            let d2: Dual2<12> = squared_distance_as(st.pts, st.fixed, kind);
            let g_d2 = d2.sqrt() - Dual2::constant(p.thickness);
            let r_d2 = g_d2 - Dual2::constant(p.dhat);
            let mut e_d2 = r_d2 * r_d2 * (g_d2 * (1.0 / p.dhat)).ln() * (-p.kappa);
            if let Some(eps) = st.mollifier_eps {
                let c_d2: Dual2<12> = edge_cross_squared_as(st.pts, st.fixed);
                if c_d2.value() < eps {
                    let u = c_d2 * (1.0 / eps);
                    e_d2 = e_d2 * ((Dual2::constant(2.0) - u) * u);
                }
            }
            let d2_grad = e_d2.grad();
            let d2_hess = e_d2.hess();
            assert!(
                (e_d2.value() - e.v).abs() <= 1e-12 * e.v.abs().max(1.0),
                "{pair:?} Dual2 value: {} vs {}",
                e_d2.value(),
                e.v
            );
            for i in 0..12 {
                assert!(
                    (d2_grad[i] - e.g[i]).abs() <= 1e-12 * gscale,
                    "{pair:?} Dual2 grad[{i}]: {} vs {}",
                    d2_grad[i],
                    e.g[i]
                );
                for j in 0..12 {
                    assert!(
                        (d2_hess[i][j] - e.h[i][j]).abs() <= 1e-12 * hscale,
                        "{pair:?} Dual2 hess[{i}][{j}]: {} vs {}",
                        d2_hess[i][j],
                        e.h[i][j]
                    );
                }
            }

            if st
                .mollifier_eps
                .is_some_and(|eps| edge_cross_squared(st.pts, st.fixed).v < eps)
            {
                mollified += 1;
            } else {
                // The solver's path (first-order dual distance, closed-form barrier) gives the jet's
                // value and gradient to 1e-8 relative.
                let d2 = squared_distance_grad(st.pts, st.fixed, kind);
                let (b, b1, _) = barrier_f64(d2.v.sqrt() - p.thickness, &p);
                assert!((b - e.v).abs() <= 1e-8 * e.v.abs(), "{pair:?} value");
                for i in 0..12 {
                    let gi = b1 * d2.g[i] / (2.0 * d2.v.sqrt());
                    assert!(
                        (gi - e.g[i]).abs() <= 1e-8 * gscale,
                        "{pair:?} solver grad[{i}]: {gi} vs {}",
                        e.g[i]
                    );
                }
            }
            let h = 1e-9;
            for (k, cv) in st.cloth.iter().enumerate() {
                let Some(v) = *cv else { continue };
                for c in 0..3 {
                    let bump = |d: f64| {
                        let mut x = cloth_x.clone();
                        match c {
                            0 => x[v].x += d,
                            1 => x[v].y += d,
                            _ => x[v].z += d,
                        }
                        x
                    };
                    let (Some(ep), Some(em)) = (energy(&bump(h)), energy(&bump(-h))) else {
                        continue;
                    };
                    let i = 3 * k + c;
                    let fd = (ep.v - em.v) / (2.0 * h);
                    assert!(
                        (fd - e.g[i]).abs() < 1e-5 * gscale,
                        "{pair:?} grad[{i}]: {fd} vs {}",
                        e.g[i]
                    );
                    for j in 0..12 {
                        let fdh = (ep.g[j] - em.g[j]) / (2.0 * h);
                        assert!(
                            (fdh - e.h[j][i]).abs() < 1e-4 * hscale,
                            "{pair:?} hess[{j}][{i}]: {fdh} vs {}",
                            e.h[j][i]
                        );
                    }
                }
            }
        }
        assert_eq!(seen.len(), 3, "every pair type active");
        assert!(mollified > 0, "a mollified edge pair was checked");
    }

    /// The start audit finds each way a cloth can cross a closed body, passes a clear one, and
    /// `with_contact` refuses a crossing start whose gaps are all positive.
    #[test]
    fn start_intersections_are_found() {
        let s = 0.1;
        let tetra = || {
            Body::new(
                vec![
                    Vec3::ZERO,
                    Vec3::new(s, 0.0, 0.0),
                    Vec3::new(0.0, s, 0.0),
                    Vec3::new(0.0, 0.0, s),
                ],
                vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
                0.01,
            )
            .unwrap()
        };
        assert!(tetra().closed);
        assert!((tetra().winding_number(Vec3::new(0.01, 0.01, 0.01)) - 1.0).abs() < 1e-12);
        assert!(tetra().winding_number(Vec3::new(0.2, 0.2, 0.2)).abs() < 1e-12);
        let audit = |x: Vec<Vec3>| {
            let c = Contact::new(tetra(), params(), vec![[0, 1, 2]], |a, b| {
                (x[a] - x[b]).norm()
            });
            c.first_intersection(&x)
        };
        // Clear, above the apex.
        let clear = vec![
            Vec3::new(0.0, 0.0, 0.2),
            Vec3::new(s, 0.0, 0.2),
            Vec3::new(0.0, s, 0.2),
        ];
        assert_eq!(audit(clear), None);
        // A cloth edge through the bottom face.
        let edge_through = vec![
            Vec3::new(0.02, 0.02, -0.05),
            Vec3::new(0.02, 0.02, 0.05),
            Vec3::new(0.5, 0.5, -0.05),
        ];
        assert!(matches!(
            audit(edge_through),
            Some(Intersection::ClothEdge { .. })
        ));
        // The apex poking through a large cloth triangle at half height: no cloth edge crosses, and
        // every cloth vertex is far from the body.
        let pierced = vec![
            Vec3::new(-1.0, -1.0, 0.05),
            Vec3::new(1.0, -1.0, 0.05),
            Vec3::new(0.0, 1.0, 0.05),
        ];
        assert!(matches!(
            audit(pierced.clone()),
            Some(Intersection::BodyEdge { ct: 0, .. })
        ));
        // Wholly inside.
        let inside = vec![
            Vec3::new(0.01, 0.01, 0.01),
            Vec3::new(0.02, 0.01, 0.01),
            Vec3::new(0.01, 0.02, 0.01),
        ];
        assert_eq!(audit(inside), Some(Intersection::Inside { cv: 0 }));
        // The solver refuses the pierced start although every gap is positive.
        let rest: Vec<Vec2> = pierced.iter().map(|p| Vec2::new(p.x, p.y)).collect();
        let model = crate::ClothModel::new(
            rest,
            &[[0, 1, 2]],
            crate::Membrane::orthotropic(1000.0, 1000.0, 0.0, 100.0, 0.05),
            1e-5,
            0.2,
        )
        .unwrap();
        let solver = crate::Solver::new(model, pierced, &[[false; 3]; 3], Vec3::ZERO);
        let err = solver.with_contact(tetra(), params()).unwrap_err();
        assert_eq!(err.code, RefusalCode::IntersectingStart);
    }

    /// Additive CCD never lets a moving point pass a triangle, and returns the full step when the
    /// motion stays clear.
    #[test]
    fn accd_stops_before_contact() {
        let tri = [
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let pair = Pair::VertexTri { cv: 0, bt: 0 };
        let pts = [Vec3::new(0.0, 0.0, 0.01), tri[0], tri[1], tri[2]];
        // Moving straight through the plane: impact when the gap (z − ξ) reaches zero at t = 0.9.
        let xi = 1e-3;
        let t = accd(
            pair,
            pts,
            [
                Vec3::new(0.0, 0.0, -0.02),
                Vec3::ZERO,
                Vec3::ZERO,
                Vec3::ZERO,
            ],
            xi,
            1.0,
        );
        assert!(t > 0.0 && t < 0.45, "t = {t}");
        let z = 0.01 - 0.02 * t;
        assert!(z - xi > 0.0, "still clear at the returned time");
        // Sideways motion keeps the gap: full step.
        let t = accd(
            pair,
            pts,
            [Vec3::new(0.3, 0.0, 0.0), Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
            xi,
            1.0,
        );
        assert_eq!(t, 1.0);
    }

    #[test]
    fn accd_fails_closed_on_touching_penetrating_and_non_finite_inputs() {
        let tri = [
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let pair = Pair::VertexTri { cv: 0, bt: 0 };
        let down = [
            Vec3::new(0.0, 0.0, -0.02),
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::ZERO,
        ];
        // Exact touching at xi = 0 must return 0.0, never 0.0 / 0.0 = NaN.
        let touching = [Vec3::ZERO, tri[0], tri[1], tri[2]];
        assert_eq!(accd(pair, touching, down, 0.0, 1.0), 0.0);
        // Already inside thickness offset must return 0.0, never walk backwards to a negative bound.
        let penetrating = [Vec3::new(0.0, 0.0, 0.0005), tri[0], tri[1], tri[2]];
        assert_eq!(accd(pair, penetrating, down, 0.001, 1.0), 0.0);
        // Overflowing displacement norm or non-finite inputs must return 0.0 immediately.
        let clear = [Vec3::new(0.0, 0.0, 0.01), tri[0], tri[1], tri[2]];
        let huge = [
            Vec3::new(0.0, 0.0, -1e308),
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::ZERO,
        ];
        assert_eq!(accd(pair, clear, huge, 1e-3, 1.0), 0.0);
        assert_eq!(accd(pair, clear, down, -1e-3, 1.0), 0.0);
        assert_eq!(accd(pair, clear, down, 1e-3, f64::NAN), 0.0);
    }

    fn layers(z: f64) -> Vec<Vec3> {
        [0.0, z]
            .into_iter()
            .flat_map(|z| {
                [
                    Vec3::new(-0.01, -0.01, z),
                    Vec3::new(0.01, -0.01, z),
                    Vec3::new(0.0, 0.01, z),
                ]
            })
            .collect()
    }

    fn self_layers(x: &[Vec3]) -> Contact {
        Contact::new(
            Body::empty(),
            params(),
            vec![[0, 1, 2], [3, 4, 5]],
            |a, b| (x[a] - x[b]).norm(),
        )
    }

    /// A crease: two triangles joined along an edge and folded nearly flat, the folded vertex closer
    /// than the thickness to its neighbour triangle. It is not two layers: it keeps a barrier at half
    /// its start distance, which bounds a step pressing it flat; a fold that starts apart keeps the
    /// full thickness, and a separate triangle as close is still refused.
    #[test]
    fn a_crease_is_not_two_layers() {
        let folded = |z: f64| {
            vec![
                Vec3::new(-0.01, 0.0, 0.0),
                Vec3::new(0.0, -0.01, 0.0),
                Vec3::new(0.0, 0.01, 0.0),
                Vec3::new(-0.008, 0.0, z),
            ]
        };
        let crease = folded(0.0003);
        let tris = vec![[0, 1, 2], [1, 3, 2]];
        let c = Contact::new(Body::empty(), params(), tris.clone(), |a, b| {
            (crease[a] - crease[b]).norm()
        });
        // inside the thickness at the start: a crease, with its own offset, half the start distance
        let creased = c.clone().with_self_contact(&crease, params()).unwrap();
        let (n, offset) = creased.start_creases();
        assert!(n >= 1);
        let offset = offset.unwrap();
        assert!(offset > 0.0 && offset <= 0.5 * 0.0003 + 1e-12, "{offset}");
        let gap = creased.min_self_gap(&crease, params().dhat).unwrap();
        assert!(gap > 0.0, "{gap}");
        // the crease cannot close further than its offset: pressed flat, the step is bounded
        let flatten: Vec<Vec3> = (0..4)
            .map(|i| {
                if i == 3 {
                    Vec3::new(0.0, 0.0, -0.0006)
                } else {
                    Vec3::ZERO
                }
            })
            .collect();
        let alpha = creased.step_bound(&crease, &flatten);
        assert!(alpha < 1.0, "{alpha}");
        // a fold that starts apart keeps the full thickness
        let open = folded(0.002);
        assert_eq!(
            c.with_self_contact(&open, params())
                .unwrap()
                .start_creases()
                .0,
            0
        );
        let mut x = crease.clone();
        x.extend([
            Vec3::new(-0.008, -0.002, 0.0003),
            Vec3::new(-0.002, -0.002, 0.0003),
            Vec3::new(-0.005, 0.003, 0.0003),
        ]);
        let mut separate = tris;
        separate.push([4, 5, 6]);
        let c = Contact::new(Body::empty(), params(), separate, |a, b| {
            (x[a] - x[b]).norm()
        });
        let err = c.with_self_contact(&x, params()).unwrap_err();
        assert_eq!(err.code, RefusalCode::IntersectingStart);
    }

    #[test]
    fn self_contact_refuses_a_bad_triangle_before_searching_creases() {
        let x = layers(0.002);
        let c = Contact::new(
            Body::empty(),
            params(),
            vec![[0, 1, 2], [3, 4, 99]],
            |_, _| 0.01,
        );
        let err = c.with_self_contact(&x, params()).unwrap_err();
        assert_eq!(err.code, RefusalCode::DegenerateGeometry);
    }

    #[test]
    fn self_sweep_matches_brute_force_and_excludes_incident_features() {
        let mut x = layers(0.002);
        x.extend(
            layers(0.004)
                .iter()
                .map(|p| *p + Vec3::new(0.03, 0.01, 0.02)),
        );
        let c = Contact::new(
            Body::empty(),
            params(),
            vec![[0, 1, 2], [3, 4, 5], [6, 7, 8], [9, 10, 11]],
            |a, b| (x[a] - x[b]).norm(),
        );
        let p: Vec<_> = x
            .iter()
            .enumerate()
            .map(|(i, _)| {
                Vec3::new(
                    0.01 * (i % 3) as f64,
                    0.0,
                    if i % 2 == 0 { 0.03 } else { -0.03 },
                )
            })
            .collect();
        for motion in [None, Some(p.as_slice())] {
            let pad = 0.001;
            let d = Vec3::new(pad, pad, pad);
            let mut expected = Vec::new();
            for cv in 0..x.len() {
                let (lo, hi) = swept_box(&[cv], &x, motion);
                for (ct, t) in c.cloth.triangles.iter().enumerate() {
                    if !t.contains(&cv) && overlaps(swept_box(t, &x, motion), lo - d, hi + d) {
                        expected.push(Pair::SelfVertexTri { cv, ct });
                    }
                }
            }
            for (a, ea) in c.cloth.edges.iter().enumerate() {
                let (lo, hi) = swept_box(ea, &x, motion);
                for (b, eb) in c.cloth.edges.iter().enumerate().skip(a + 1) {
                    if !ea.iter().any(|i| eb.contains(i))
                        && overlaps(swept_box(eb, &x, motion), lo - d, hi + d)
                    {
                        expected.push(Pair::SelfEdgeEdge { a, b });
                    }
                }
            }
            expected.sort_unstable();
            let actual = c.self_candidates(&x, motion, pad);
            assert_eq!(actual, expected);
            assert!(actual.windows(2).all(|w| w[0] < w[1]));
        }
        // A welded, flat two-triangle patch is not in collision with itself.
        let x = vec![
            Vec3::ZERO,
            Vec3::new(0.01, 0.0, 0.0),
            Vec3::new(0.0, 0.01, 0.0),
            Vec3::new(0.01, 0.01, 0.0),
        ];
        Contact::new(
            Body::empty(),
            params(),
            vec![[0, 1, 2], [1, 3, 2]],
            |a, b| (x[a] - x[b]).norm(),
        )
        .with_self_contact(&x, params())
        .unwrap();
    }

    #[test]
    fn self_audit_refuses_crossings_coplanar_overlap_and_bad_parameters() {
        let x = layers(0.0);
        assert_eq!(
            self_layers(&x)
                .with_self_contact(&x, params())
                .unwrap_err()
                .code,
            RefusalCode::IntersectingStart
        );
        // Large triangles can cross even while all vertex–triangle and edge–edge gaps are positive.
        let x = vec![
            Vec3::new(-0.02, -0.02, 0.0),
            Vec3::new(0.02, -0.02, 0.0),
            Vec3::new(0.0, 0.02, 0.0),
            Vec3::new(0.0, -0.01, -0.02),
            Vec3::new(0.0, -0.01, 0.02),
            Vec3::new(0.0, 0.03, 0.02),
        ];
        let mut observed = None;
        let err = self_layers(&x)
            .with_self_contact_observed(&x, params(), &mut observed)
            .unwrap_err();
        assert_eq!(err.code, RefusalCode::IntersectingStart);
        assert!(err.message.contains("triangles 0 and 1"), "{err}");
        assert_eq!(observed, Some([0, 1]));
        // Disjoint mid-surfaces closer than their combined thickness also report an incident
        // solver triangle pair so diagnostics can name and mark the touching pieces.
        let thin_gap = layers(0.5 * params().thickness);
        let mut touch_observed = None;
        let err = self_layers(&thin_gap)
            .with_self_contact_observed(&thin_gap, params(), &mut touch_observed)
            .unwrap_err();
        assert_eq!(err.code, RefusalCode::IntersectingStart);
        assert!(err.message.contains("touch or overlap"), "{err}");
        let [ta, tb] = touch_observed.expect("touch/overlap start refusal must report a pair");
        assert_eq!([ta.min(tb), ta.max(tb)], [0, 1]);
        let x = layers(0.002);
        for bad in [
            ContactParams {
                mu: -1.0,
                ..params()
            },
            ContactParams {
                dhat: 0.0,
                ..params()
            },
            ContactParams {
                eps_v: 0.0,
                ..params()
            },
        ] {
            assert_eq!(
                self_layers(&x).with_self_contact(&x, bad).unwrap_err().code,
                RefusalCode::OutOfRange
            );
        }
        assert_eq!(
            self_layers(&x)
                .with_self_contact(
                    &x,
                    ContactParams {
                        thickness: f64::NAN,
                        ..params()
                    }
                )
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );
    }

    #[test]
    fn self_ccd_accounts_for_motion_of_both_layers() {
        let x = layers(0.01);
        let c = self_layers(&x).with_self_contact(&x, params()).unwrap();
        let p: Vec<_> = (0..6)
            .map(|i| Vec3::new(0.0, 0.0, if i < 3 { 0.02 } else { -0.02 }))
            .collect();
        let t = c.step_bound(&x, &p);
        assert!(t > 0.0 && t < (0.01 - params().thickness) / 0.04, "{t}");
        let trial: Vec<_> = x.iter().zip(&p).map(|(x, p)| *x + *p * t).collect();
        c.audit_self_contact(&trial).unwrap();
        assert_eq!(
            c.step_bound(&x, &vec![Vec3::new(1.0, 2.0, 3.0); x.len()]),
            1.0
        );
    }

    #[test]
    fn self_barrier_and_friction_move_both_sides() {
        let x = layers(0.001);
        let mut c = self_layers(&x).with_self_contact(&x, params()).unwrap();
        let pair = Pair::SelfVertexTri { cv: 3, ct: 0 };
        let st = stencil(pair, &x, &c.body, &c.cloth);
        assert_eq!(st.fixed, [false; 4]);
        let (kind, _) = classify(pair, &st.pts);
        let PairEnergy::Active(e) = pair_energy(&st, kind, &params()) else {
            panic!("the pair must be active");
        };
        for axis in 0..3 {
            let force: f64 = (0..4).map(|v| e.g[3 * v + axis]).sum();
            assert!(force.abs() < 1e-12, "internal forces must balance");
        }
        // Difference the energy on all four moving vertices, not just the point.
        for (v, id) in st.cloth.iter().enumerate() {
            let id = id.unwrap();
            let h = 1e-9;
            let energy = |delta: f64| {
                let mut y = x.clone();
                y[id].z += delta;
                let s = stencil(pair, &y, &c.body, &c.cloth);
                let (k, _) = classify(pair, &s.pts);
                match pair_energy_value(&s, k, &params()) {
                    PairEnergy::Value(v) => v,
                    _ => panic!("perturbation left the barrier"),
                }
            };
            let fd = (energy(h) - energy(-h)) / (2.0 * h);
            assert!((fd - e.g[3 * v + 2]).abs() < 1e-7);
        }
        // Body friction can be disabled while cloth–cloth friction remains enabled.
        c.params.mu = 0.0;
        c.update_lag(&x, &x, 0.01);
        assert!(!c.lag.is_empty());
        let rigid: Vec<_> = x
            .iter()
            .map(|p| *p + Vec3::new(0.001, 0.002, 0.0))
            .collect();
        for f in &c.lag {
            let (e0, _, _) = c.friction_local(f, &x);
            let (e1, g, _) = c.friction_local(f, &rigid);
            assert!(
                (e0 - e1).abs() < 1e-12,
                "rigid translation cannot cause friction"
            );
            for axis in 0..3 {
                assert!((0..4).map(|v| g[3 * v + axis]).sum::<f64>().abs() < 1e-12);
            }
        }
    }

    #[test]
    fn stick_slip_potential_gradient_and_hessian_calculus() {
        let ss = StickSlipFriction::new(0.6, 0.3, 0.02).unwrap();
        let eps_v = 1e-3;
        let h = 0.01;
        let eps = eps_v * h;
        let w = ss.transition_v_m_s * h;

        // C⁰, C¹, C² continuity across the stick/slip boundary y = eps.
        let delta = 1e-12;
        let lo = stick_slip_radial(eps - delta, eps, ss, h);
        let at = stick_slip_radial(eps, eps, ss, h);
        let hi = stick_slip_radial(eps + delta, eps, ss, h);
        assert!((lo.d_val - at.d_val).abs() < 1e-12 && (hi.d_val - at.d_val).abs() < 1e-12);
        let tau_lo = lo.tau_over_y * (eps - delta);
        let tau_at = at.tau_over_y * eps;
        let tau_hi = hi.tau_over_y * (eps + delta);
        assert!((tau_at - ss.mu_s).abs() < 1e-12);
        assert!((tau_lo - tau_at).abs() < 1e-9 && (tau_hi - tau_at).abs() < 1e-9);
        assert!(at.radial_eig_exact.abs() < 1e-15);
        assert!(lo.radial_eig_exact.abs() < 1e-1 && hi.radial_eig_exact.abs() < 1e-3);

        // Exact 2D FD gradient & exact Hessian vs analytic, plus strict PSD projection.
        let eval_2d = |u: [f64; 2]| {
            let y = (u[0] * u[0] + u[1] * u[1]).sqrt();
            let rad = stick_slip_radial(y, eps, ss, h);
            let g = [rad.tau_over_y * u[0], rad.tau_over_y * u[1]];
            let mut h_exact = [[rad.tau_over_y, 0.0], [0.0, rad.tau_over_y]];
            let mut h_psd = [[rad.tau_over_y, 0.0], [0.0, rad.tau_over_y]];
            if y > 0.0 {
                let dir = [u[0] / y, u[1] / y];
                let d_exact = rad.radial_eig_exact - rad.tau_over_y;
                let d_psd = rad.radial_psd_minus_tau_over_y;
                for a in 0..2 {
                    for b in 0..2 {
                        h_exact[a][b] += d_exact * dir[a] * dir[b];
                        h_psd[a][b] += d_psd * dir[a] * dir[b];
                    }
                }
            }
            (rad.d_val, g, h_exact, h_psd)
        };
        for &(ux, uy) in &[
            (0.3 * eps, -0.25 * eps),
            (0.8 * eps, 0.5 * eps),
            (eps + 0.7 * w, 0.4 * w),
            (eps + 6.0 * w, -3.0 * w),
        ] {
            let (_, g, h_exact, h_psd) = eval_2d([ux, uy]);
            let step = 1e-9;
            for k in 0..2 {
                let mut up = [ux, uy];
                let mut um = [ux, uy];
                up[k] += step;
                um[k] -= step;
                let (dp, gp, _, _) = eval_2d(up);
                let (dm, gm, _, _) = eval_2d(um);
                let fd_g = (dp - dm) / (2.0 * step);
                assert!(
                    (fd_g - g[k]).abs() < 1e-6 * g[k].abs().max(1e-6),
                    "grad[{k}] at ({ux}, {uy}): fd={fd_g} vs {}",
                    g[k]
                );
                for j in 0..2 {
                    let fd_h = (gp[j] - gm[j]) / (2.0 * step);
                    assert!(
                        (fd_h - h_exact[j][k]).abs() < 1e-5 * h_exact[j][k].abs().max(1.0),
                        "hess[{j}][{k}] at ({ux}, {uy}): fd={fd_h} vs {}",
                        h_exact[j][k]
                    );
                }
            }
            let trace = h_psd[0][0] + h_psd[1][1];
            let det = h_psd[0][0] * h_psd[1][1] - h_psd[0][1] * h_psd[1][0];
            assert!(trace >= 0.0 && det >= -1e-12 * trace * trace.max(1.0));
        }

        // Exact reduction to legacy Coulomb when mu_s == mu_k.
        let plane = Body::new(
            vec![
                Vec3::new(-1.0, -1.0, 0.0),
                Vec3::new(1.0, -1.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![[0, 1, 2]],
            0.1,
        )
        .unwrap();
        let x0 = vec![
            Vec3::new(-0.01, -0.01, 9e-4),
            Vec3::new(0.01, -0.01, 9e-4),
            Vec3::new(0.0, 0.01, 9e-4),
        ];
        let x1: Vec<Vec3> = x0
            .iter()
            .map(|p| *p + Vec3::new(5e-4, -2e-4, 0.0))
            .collect();
        let mut legacy = Contact::new(plane.clone(), params(), vec![[0, 1, 2]], |a, b| {
            (x0[a] - x0[b]).norm()
        });
        let mut equal_ss = legacy
            .clone()
            .with_stick_slip(StickSlipFriction::new(params().mu, params().mu, 0.02).unwrap())
            .unwrap();
        legacy.update_lag(&x0, &x0, h);
        equal_ss.update_lag(&x0, &x0, h);
        for (fl, fs) in legacy.lag.iter().zip(&equal_ss.lag) {
            let (el, gl, hl) = legacy.friction_local(fl, &x1);
            let (es, gs, hs) = equal_ss.friction_local(fs, &x1);
            assert!((el - es).abs() < 1e-15);
            for i in 0..12 {
                assert!((gl[i] - gs[i]).abs() < 1e-15);
            }
            for i in 0..144 {
                assert!((hl[i] - hs[i]).abs() < 1e-12);
            }
        }
        // Tiny subnormal-square slip (|u| = 1e-160 with one axis zero) must stay strictly finite.
        let x_tiny: Vec<Vec3> = x0
            .iter()
            .map(|p| *p + Vec3::new(1e-160, 0.0, 0.0))
            .collect();
        for fs in &equal_ss.lag {
            let (e0, g0, h0) = equal_ss.friction_local(fs, &x0);
            let (et, gt, ht) = equal_ss.friction_local(fs, &x_tiny);
            assert!(e0.is_finite() && et.is_finite());
            assert!(g0.iter().chain(&gt).all(|v| v.is_finite()));
            assert!(h0.iter().chain(ht.iter()).all(|v| v.is_finite()));
        }
    }

    /// KAT: Inclined-plane stick-slip transition (FTX-27e).
    /// A cloth patch with static friction `mu_s = 0.6` and kinetic friction `mu_k = 0.3`:
    /// 1. At `tan(theta) = 0.45` (`mu_k < tan(theta) < mu_s`): sticks on the slope, whereas a
    ///    single-`mu = mu_k` cloth slides downslope.
    /// 2. At `tan(theta) = 0.75` (`tan(theta) > mu_s`): breaks free and accelerates past a
    ///    constant-`mu = mu_s` cloth as dynamic friction decays smoothly toward `mu_k`.
    #[test]
    fn inclined_plane_stick_slip_kat() {
        let plane = || {
            Body::new(
                vec![
                    Vec3::new(-1.0, -1.0, 0.0),
                    Vec3::new(1.0, -1.0, 0.0),
                    Vec3::new(1.0, 1.0, 0.0),
                    Vec3::new(-1.0, 1.0, 0.0),
                ],
                vec![[0, 1, 2], [0, 2, 3]],
                0.2,
            )
            .unwrap()
        };
        let run_slope = |tan_theta: f64, mu: f64, ss: Option<StickSlipFriction>, steps: usize| {
            let s = 0.04;
            let rest = vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(s, 0.0),
                Vec2::new(s, s),
                Vec2::new(0.0, s),
            ];
            let tris = [[0, 1, 2], [0, 2, 3]];
            let model = crate::ClothModel::new(
                rest.clone(),
                &tris,
                crate::Membrane::orthotropic(2000.0, 2000.0, 0.0, 400.0, 1.0),
                1e-4,
                0.2,
            )
            .unwrap();
            let mean_m = model.mass.iter().sum::<f64>() / model.mass.len() as f64;
            let g_mag = 9.81;
            let cos_t = 1.0 / (1.0 + tan_theta * tan_theta).sqrt();
            let sin_t = tan_theta * cos_t;
            let z0 = 0.001; // thickness 5e-4 + 0.5 * dhat 5e-4
            let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, z0)).collect();
            let mut contact_params = ContactParams {
                dhat: 1e-3,
                thickness: 5e-4,
                kappa: 1.0,
                mu,
                eps_v: 1e-3,
            };
            let policy = AdaptiveBarrierPolicy::default();
            contact_params.kappa = policy
                .initial_kappa(mean_m, 2000.0, g_mag * cos_t, contact_params.dhat)
                .unwrap();
            let mut solver = crate::Solver::new(
                model,
                x0,
                &[[false; 3]; 4],
                Vec3::new(0.0, 0.0, -g_mag * cos_t),
            )
            .with_contact(plane(), contact_params)
            .unwrap();
            if let Some(law) = ss {
                solver
                    .contact
                    .as_mut()
                    .unwrap()
                    .set_stick_slip(law)
                    .unwrap();
            }
            solver.friction_rounds = 2;
            solver.tol = 1e-6;
            // Settle vertically onto the plane so normal load equals M * g * cos(theta).
            for _ in 0..8 {
                let rep = solver.step(0.01).unwrap();
                assert!(rep.converged);
            }
            let x_settled = solver.x.iter().map(|p| p.x).sum::<f64>() / 4.0;
            solver.v.fill(Vec3::ZERO);
            solver.gravity = Vec3::new(g_mag * sin_t, 0.0, -g_mag * cos_t);
            for _ in 0..steps {
                let rep = solver.step(0.01).unwrap();
                assert!(rep.converged);
            }
            let x_end = solver.x.iter().map(|p| p.x).sum::<f64>() / 4.0;
            x_end - x_settled
        };

        let law = StickSlipFriction::new(0.6, 0.3, 0.02).unwrap();
        // Sub-threshold slope (tan = 0.45 in (0.3, 0.6)): stick-slip holds, pure kinetic slides.
        let slip_kinetic = run_slope(0.45, 0.3, None, 12);
        let slip_held = run_slope(0.45, 0.3, Some(law), 12);
        assert!(
            slip_kinetic > 2e-3,
            "pure kinetic friction must slide at tan=0.45 > mu_k=0.3, got {slip_kinetic}"
        );
        assert!(
            slip_held < 1.5e-4 && slip_held < 0.05 * slip_kinetic,
            "stick-slip must hold below mu_s=0.6: held={slip_held}, kinetic={slip_kinetic}"
        );

        // Super-threshold slope (tan = 0.75 > mu_s = 0.6): breaks free and outruns constant mu_s.
        let slip_static_const = run_slope(0.75, 0.6, None, 16);
        let slip_breakaway = run_slope(0.75, 0.6, Some(law), 16);
        let slip_kinetic_const = run_slope(0.75, 0.3, None, 16);
        assert!(
            slip_breakaway > 1.3 * slip_static_const,
            "stick-slip breakaway must accelerate past constant mu_s: breakaway={slip_breakaway}, static={slip_static_const}"
        );
        assert!(
            slip_breakaway < slip_kinetic_const,
            "stick-slip must remain bounded below pure mu_k: breakaway={slip_breakaway}, kinetic={slip_kinetic_const}"
        );
    }

    /// KAT: Light (40 g/m² silk) vs heavy (500 g/m² wool) adaptive IPC barrier clearance (FTX-27e).
    /// Legacy fixed `kappa = 100 N/m` leaves 40 g/m² silk hovering near `dhat`; `AdaptiveBarrierPolicy`
    /// scales `kappa` physically with fabric mass so both silk and heavy wool settle in the same
    /// mid-band clearance window, and escalates `kappa` under extreme overload compression.
    #[test]
    fn light_and_heavy_fabric_adaptive_barrier_clearance_kat() {
        let plane = || {
            Body::new(
                vec![
                    Vec3::new(-1.0, -1.0, 0.0),
                    Vec3::new(1.0, -1.0, 0.0),
                    Vec3::new(1.0, 1.0, 0.0),
                    Vec3::new(-1.0, 1.0, 0.0),
                ],
                vec![[0, 1, 2], [0, 2, 3]],
                0.2,
            )
            .unwrap()
        };
        let settle_sheet = |areal_kg_m2: f64,
                            tensile_n_m: f64,
                            adaptive: Option<AdaptiveBarrierPolicy>| {
            let s = 0.04;
            let rest = vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(s, 0.0),
                Vec2::new(s, s),
                Vec2::new(0.0, s),
            ];
            let tris = [[0, 1, 2], [0, 2, 3]];
            let model = crate::ClothModel::new(
                rest.clone(),
                &tris,
                crate::Membrane::orthotropic(tensile_n_m, tensile_n_m, 0.0, 0.2 * tensile_n_m, 1.0),
                1e-4,
                areal_kg_m2,
            )
            .unwrap();
            let mean_m = model.mass.iter().sum::<f64>() / model.mass.len() as f64;
            let dhat = 1e-3;
            let thickness = 5e-4;
            let x0: Vec<Vec3> = rest
                .iter()
                .map(|p| Vec3::new(p.x, p.y, thickness + 0.6 * dhat))
                .collect();
            let contact_params = ContactParams {
                dhat,
                thickness,
                kappa: 100.0,
                mu: 0.0,
                eps_v: 1e-3,
            };
            let mut solver =
                crate::Solver::new(model, x0, &[[false; 3]; 4], Vec3::new(0.0, 0.0, -9.81))
                    .with_contact(plane(), contact_params)
                    .unwrap();
            if let Some(policy) = adaptive {
                solver
                    .contact
                    .as_mut()
                    .unwrap()
                    .set_adaptive_barrier(policy, mean_m, tensile_n_m, 9.81)
                    .unwrap();
            }
            solver.tol = 1e-7;
            for _ in 0..20 {
                let rep = solver.step(0.02).unwrap();
                assert!(rep.converged);
                if let Some(c) = solver.contact.as_mut() {
                    c.adapt_barrier_stiffness(&solver.x);
                }
            }
            let gap = solver
                .contact
                .as_ref()
                .unwrap()
                .min_gap(&solver.x, dhat)
                .unwrap();
            let kappa = solver.contact.as_ref().unwrap().params.kappa;
            (solver, gap / dhat, kappa)
        };

        // Fixed kappa = 100 N/m causes 40 g/m² silk to hover right at the outer edge of dhat.
        let (_, silk_fixed_frac, _) = settle_sheet(0.04, 400.0, None);
        assert!(
            silk_fixed_frac > 0.90,
            "legacy fixed kappa=100 causes silk hover, got gap/dhat={silk_fixed_frac}"
        );

        // Adaptive scaling brings both 40 g/m² silk and 500 g/m² wool into the same mid-band window.
        let policy = AdaptiveBarrierPolicy::default();
        let (mut silk_solver, silk_frac, silk_kappa) = settle_sheet(0.04, 400.0, Some(policy));
        let (_, wool_frac, wool_kappa) = settle_sheet(0.50, 4000.0, Some(policy));
        assert!(
            ((wool_kappa / silk_kappa) - (0.50 / 0.04)).abs() < 0.25,
            "kappa must scale with areal density: wool={wool_kappa}, silk={silk_kappa}"
        );
        assert!(
            (0.35..0.65).contains(&silk_frac) && (0.35..0.65).contains(&wool_frac),
            "both fabrics must settle mid-band: silk={silk_frac}, wool={wool_frac}"
        );
        assert!(
            (silk_frac - wool_frac).abs() < 0.08,
            "light and heavy fabrics must achieve consistent clearance: silk={silk_frac}, wool={wool_frac}"
        );

        // Extreme downward overload compresses gap below min_gap_fraction and escalates kappa.
        silk_solver.gravity = Vec3::new(0.0, 0.0, -9.81 * 50.0);
        for _ in 0..6 {
            let rep = silk_solver.step(0.01).unwrap();
            assert!(rep.converged);
        }
        let squeezed = silk_solver
            .contact
            .as_ref()
            .unwrap()
            .min_gap(&silk_solver.x, 1e-3)
            .unwrap();
        assert!(
            squeezed < policy.min_gap_fraction * 1e-3,
            "overload must compress below trigger: squeezed={squeezed}"
        );
        let escalated = silk_solver
            .contact
            .as_mut()
            .unwrap()
            .adapt_barrier_stiffness(&silk_solver.x);
        assert!((escalated - silk_kappa * policy.growth_factor).abs() < 1e-12);
        for _ in 0..8 {
            let rep = silk_solver.step(0.01).unwrap();
            assert!(rep.converged);
        }
        let recovered = silk_solver
            .contact
            .as_ref()
            .unwrap()
            .min_gap(&silk_solver.x, 1e-3)
            .unwrap();
        assert!(
            recovered > squeezed,
            "escalated kappa must restore clearance: before={squeezed}, after={recovered}"
        );
    }

    #[test]
    fn adaptive_barrier_and_stick_slip_validation_and_serde() {
        let policy = AdaptiveBarrierPolicy::default();
        policy.validate().unwrap();
        let json_p = serde_json::to_value(policy).unwrap();
        assert_eq!(
            serde_json::from_value::<AdaptiveBarrierPolicy>(json_p).unwrap(),
            policy
        );
        assert!(
            serde_json::from_value::<AdaptiveBarrierPolicy>(json!({"unknown_key": 1.0})).is_err()
        );
        for bad in [
            AdaptiveBarrierPolicy {
                target_gap_fraction: 0.0,
                ..policy
            },
            AdaptiveBarrierPolicy {
                target_gap_fraction: 1.0,
                ..policy
            },
            AdaptiveBarrierPolicy {
                min_gap_fraction: 0.6,
                ..policy
            },
            AdaptiveBarrierPolicy {
                growth_factor: 0.5,
                ..policy
            },
            AdaptiveBarrierPolicy {
                kappa_min: 10.0,
                kappa_max: 5.0,
                ..policy
            },
            AdaptiveBarrierPolicy {
                membrane_floor_scale: f64::NAN,
                ..policy
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }

        let ss = StickSlipFriction::new(0.5, 0.3, 0.02).unwrap();
        let json_s = serde_json::to_value(ss).unwrap();
        assert_eq!(
            serde_json::from_value::<StickSlipFriction>(json_s).unwrap(),
            ss
        );
        assert!(
            serde_json::from_value::<StickSlipFriction>(json!({
                "mu_s": 0.5, "mu_k": 0.3, "transition_v_m_s": 0.02, "extra": 1
            }))
            .is_err()
        );
        assert_eq!(
            StickSlipFriction::new(0.2, 0.4, 0.02).unwrap_err().code,
            RefusalCode::OutOfRange
        );
        assert_eq!(
            StickSlipFriction::new(0.4, -0.1, 0.02).unwrap_err().code,
            RefusalCode::OutOfRange
        );
        assert_eq!(
            StickSlipFriction::new(0.4, 0.2, 0.0).unwrap_err().code,
            RefusalCode::OutOfRange
        );
        assert_eq!(
            StickSlipFriction::new(f64::NAN, 0.2, 0.02)
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );
        // Domain guards for pure evaluators return NaN outside valid domain.
        assert!(AdaptiveBarrierPolicy::barrier_force_factor(0.0).is_nan());
        assert!(AdaptiveBarrierPolicy::barrier_force_factor(1.0).is_nan());
        assert!(AdaptiveBarrierPolicy::barrier_force_factor(f64::NAN).is_nan());
        assert!(ss.effective_mu(f64::NAN, 1e-3).is_nan());
        assert!(ss.effective_mu(0.01, 0.0).is_nan());

        // Self stick-slip refuses if self-contact is not enabled.
        let x = layers(0.002);
        assert_eq!(
            self_layers(&x).with_self_stick_slip(ss).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        // Surface materials and uniform stick-slip refuse mutual override in either order.
        let mats = [SurfaceMaterial {
            half_thickness_m: 1e-4,
            friction_body: 0.3,
        }];
        let with_mat = self_layers(&x)
            .with_surface_materials(&x, &[0, 0], &mats, None)
            .unwrap();
        assert_eq!(
            with_mat.with_stick_slip(ss).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        let with_ss = self_layers(&x).with_stick_slip(ss).unwrap();
        assert_eq!(
            with_ss
                .with_surface_materials(&x, &[0, 0], &mats, None)
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );

        // Collider replacement must preserve explicit new policies instead of overwriting with None,
        // keep surface-material vs stick-slip mutual exclusivity, and not leak self_stick_slip onto
        // contacts without self-contact.
        let prev_plain = self_layers(&x);
        let upgraded = self_layers(&x)
            .with_adaptive_barrier(policy, 1e-4, 1000.0, 9.81)
            .unwrap()
            .with_stick_slip(ss)
            .unwrap()
            .retain_surface_materials(Some(&prev_plain), &x)
            .unwrap();
        assert_eq!(upgraded.adaptive_barrier(), Some(policy));
        assert_eq!(upgraded.stick_slip(), Some(ss));

        // Fresh collider inherits prior policies when unset, but does not leak self_stick_slip
        // if the replacement contact has no self-contact.
        let prev_full = self_layers(&x)
            .with_self_contact(&x, params())
            .unwrap()
            .with_adaptive_barrier(policy, 1e-4, 1000.0, 9.81)
            .unwrap()
            .with_stick_slip(ss)
            .unwrap()
            .with_self_stick_slip(ss)
            .unwrap();
        let inherited_body_only = self_layers(&x)
            .retain_surface_materials(Some(&prev_full), &x)
            .unwrap();
        assert_eq!(inherited_body_only.adaptive_barrier(), Some(policy));
        assert_eq!(inherited_body_only.stick_slip(), Some(ss));
        assert_eq!(inherited_body_only.self_stick_slip(), None);

        // Replacing a surface-material contact with stick-slip refuses instead of combining both.
        let prev_mat = self_layers(&x)
            .with_surface_materials(&x, &[0, 0], &mats, None)
            .unwrap();
        assert_eq!(
            self_layers(&x)
                .with_stick_slip(ss)
                .unwrap()
                .retain_surface_materials(Some(&prev_mat), &x)
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );

        // Replacing a stick-slip contact with explicit surface materials drops uniform stick-slip.
        let replaced_with_mat = self_layers(&x)
            .with_surface_materials(&x, &[0, 0], &mats, None)
            .unwrap()
            .retain_surface_materials(Some(&prev_full), &x)
            .unwrap();
        assert_eq!(replaced_with_mat.stick_slip(), None);
        assert_eq!(replaced_with_mat.self_stick_slip(), None);
    }
}
