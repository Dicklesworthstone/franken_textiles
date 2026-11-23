//! The garment mesh (spec 04 §2): every simulated piece instance meshed flat in one vertex numbering,
//! with matched seam samples so that sewing pairs are vertex pairs.
//!
//! Port of the reference's `build_garment`. Selection, seam sampling, boundary samples, the hex interior
//! and the seam records are bit-exact with the reference. Triangulation is `spade`'s constrained
//! Delaunay on the same points and constraint loop where the reference calls Blender's
//! `delaunay_2d_cdt`: both are Delaunay, so they agree except where points are cocircular (the hex
//! interior has many such quadruples) and either diagonal is correct. Exactly coincident points are
//! merged (the reference merges within 1e-6 mm).

use std::collections::BTreeMap;

use ftx_core::curve::EDGE_DENSE;
use ftx_core::det;
use ftx_core::np::{arange, interp, linspace, percentile};
use ftx_core::polyline::{at_fractions, cumulative, point_in_polygon, polyline_length};
use ftx_core::py;
use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_pattern::{Instance, Pattern, PyError, Seam, truthy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

/// Default target turning angle per curved boundary segment (radians, ≈ 6.9°) when
/// curvature-adaptive boundary refinement is enabled.
pub const DEFAULT_MAX_TURNING_ANGLE_RAD: f64 = 0.12;
/// Default spacing refinement divisor along gathered seams (`|ratio − 1| > GATHER_RATIO_MARGIN`)
/// when adaptive refinement is enabled.
pub const DEFAULT_GATHER_REFINEMENT: f64 = 2.0;
/// Maximum permitted interior Lloyd CVT relaxation passes.
pub const MAX_CVT_ITERATIONS: usize = 32;
/// Maximum permitted coarse-to-fine spacing ratio `spacing / min_spacing_mm`.
pub const MAX_ADAPTIVE_RATIO: f64 = 8.0;

const MIN_TURNING_ANGLE_RAD: f64 = 0.05;
const SIZING_GRADING_BETA: f64 = 0.15;
const CVT_OMEGA: f64 = 0.8;
const CVT_STOP_REL: f64 = 1e-3;

fn is_zero_usize(v: &usize) -> bool {
    *v == 0
}

fn is_default_turning_angle(v: &f64) -> bool {
    v.to_bits() == DEFAULT_MAX_TURNING_ANGLE_RAD.to_bits()
}

fn is_default_gather_refinement(v: &f64) -> bool {
    v.to_bits() == DEFAULT_GATHER_REFINEMENT.to_bits()
}

/// Opt-in curvature/gather-adaptive meshing and bounded interior Lloyd CVT smoothing for
/// [`build_garment_with_options`].
///
/// The default (`min_spacing_mm: None`, `cvt_iterations: 0`) disables both features so
/// [`build_garment`] remains bit-identical to the reference mesh generator.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MeshOptions {
    /// Minimum local spacing (mm) when adaptive curvature/gather refinement is enabled.
    /// `None` keeps uniform `spacing` on every boundary and in the interior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_spacing_mm: Option<f64>,
    /// Bounded interior Lloyd centroidal-Voronoi-tessellation (CVT) relaxation passes
    /// (`0` disables smoothing; at most [`MAX_CVT_ITERATIONS`]).
    #[serde(skip_serializing_if = "is_zero_usize")]
    pub cvt_iterations: usize,
    /// Target turning angle per curved boundary segment (radians) when `min_spacing_mm` is set.
    #[serde(skip_serializing_if = "is_default_turning_angle")]
    pub max_turning_angle_rad: f64,
    /// Local spacing refinement factor along gathered seams (`>= 1.0`, clamped by `min_spacing_mm`)
    /// when `min_spacing_mm` is set.
    #[serde(skip_serializing_if = "is_default_gather_refinement")]
    pub gather_refinement: f64,
}

impl Default for MeshOptions {
    fn default() -> Self {
        Self {
            min_spacing_mm: None,
            cvt_iterations: 0,
            max_turning_angle_rad: DEFAULT_MAX_TURNING_ANGLE_RAD,
            gather_refinement: DEFAULT_GATHER_REFINEMENT,
        }
    }
}

impl MeshOptions {
    /// Convenience preset enabling curvature/gather-adaptive refinement down to `min_spacing_mm`
    /// and 6 passes of bounded interior Lloyd CVT smoothing.
    #[must_use]
    pub fn adaptive(min_spacing_mm: f64) -> Self {
        Self {
            min_spacing_mm: Some(min_spacing_mm),
            cvt_iterations: 6,
            ..Self::default()
        }
    }

    /// Whether these options match the bit-identical legacy default.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.min_spacing_mm.is_none()
            && self.cvt_iterations == 0
            && is_default_turning_angle(&self.max_turning_angle_rad)
            && is_default_gather_refinement(&self.gather_refinement)
    }

    /// Validates meshing options against the nominal `spacing` (mm).
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`], [`RefusalCode::OutOfRange`], or [`RefusalCode::BudgetExceeded`]
    /// on non-finite, out-of-range, or silently-ignored adaptive tuning parameters.
    pub fn validate(&self, spacing: f64) -> Result<(), Refusal> {
        Refusal::finite("mesh spacing", spacing)?;
        if spacing <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("mesh spacing {spacing} mm must be positive"),
            ));
        }
        Refusal::finite("mesh max_turning_angle_rad", self.max_turning_angle_rad)?;
        Refusal::finite("mesh gather_refinement", self.gather_refinement)?;
        if self.cvt_iterations > MAX_CVT_ITERATIONS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                format!(
                    "cvt_iterations {} exceeds maximum {MAX_CVT_ITERATIONS}",
                    self.cvt_iterations
                ),
            ));
        }
        if !(MIN_TURNING_ANGLE_RAD..=std::f64::consts::PI).contains(&self.max_turning_angle_rad) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "max_turning_angle_rad {} must be in [{MIN_TURNING_ANGLE_RAD}, pi]",
                    self.max_turning_angle_rad
                ),
            ));
        }
        if !(1.0..=MAX_ADAPTIVE_RATIO).contains(&self.gather_refinement) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "gather_refinement {} must be in [1.0, {MAX_ADAPTIVE_RATIO}]",
                    self.gather_refinement
                ),
            ));
        }
        if let Some(min_s) = self.min_spacing_mm {
            Refusal::finite("mesh min_spacing_mm", min_s)?;
            if min_s <= 0.0 || min_s > spacing {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "min_spacing_mm {min_s} must be positive and at most spacing {spacing} mm"
                    ),
                )
                .repair("Choose min_spacing_mm in (0, spacing] or omit it for uniform spacing."));
            }
            if spacing / min_s > MAX_ADAPTIVE_RATIO {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    format!(
                        "adaptive spacing ratio {spacing}/{min_s} exceeds maximum {MAX_ADAPTIVE_RATIO}"
                    ),
                ));
            }
        } else if !is_default_turning_angle(&self.max_turning_angle_rad)
            || !is_default_gather_refinement(&self.gather_refinement)
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "max_turning_angle_rad and gather_refinement require min_spacing_mm to be set",
            )
            .repair("Set min_spacing_mm when customizing curvature or gather refinement."));
        }
        Ok(())
    }
}

/// Deterministic triangle angle quality summary for a 2D [`GarmentMesh`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MeshQuality {
    /// Total triangles inspected.
    pub triangle_count: usize,
    /// Smallest interior angle across all triangles (degrees).
    pub min_angle_deg: f64,
    /// 5th percentile of per-triangle minimum angles (degrees).
    pub p05_angle_deg: f64,
    /// Largest interior angle across all triangles (degrees).
    pub max_angle_deg: f64,
    /// Count of obtuse triangles (maximum interior angle > 90°).
    pub obtuse_triangles: usize,
    /// Count of sliver triangles (minimum interior angle < 20°).
    pub sliver_triangles: usize,
}

/// Computes deterministic triangle interior-angle quality metrics on `mesh` using `ftx_core::det`.
#[must_use]
pub fn mesh_quality(mesh: &GarmentMesh) -> MeshQuality {
    if mesh.faces.is_empty() {
        return MeshQuality {
            triangle_count: 0,
            min_angle_deg: 0.0,
            p05_angle_deg: 0.0,
            max_angle_deg: 0.0,
            obtuse_triangles: 0,
            sliver_triangles: 0,
        };
    }
    let rad_to_deg = 180.0 / std::f64::consts::PI;
    let angle_deg = |a: Vec2, b: Vec2, c: Vec2| -> f64 {
        let u = b - a;
        let v = c - a;
        let dot = u.x * v.x + u.y * v.y;
        let cross = (u.x * v.y - u.y * v.x).abs();
        det::atan2(cross, dot) * rad_to_deg
    };
    let mut tri_mins = Vec::with_capacity(mesh.faces.len());
    let mut min_angle_deg = f64::INFINITY;
    let mut max_angle_deg = 0.0_f64;
    let mut obtuse_triangles = 0usize;
    let mut sliver_triangles = 0usize;
    for &[i, j, k] in &mesh.faces {
        let (a, b, c) = (mesh.verts2d[i], mesh.verts2d[j], mesh.verts2d[k]);
        let ang = [angle_deg(a, b, c), angle_deg(b, c, a), angle_deg(c, a, b)];
        let t_min = ang[0].min(ang[1]).min(ang[2]);
        let t_max = ang[0].max(ang[1]).max(ang[2]);
        tri_mins.push(t_min);
        min_angle_deg = min_angle_deg.min(t_min);
        max_angle_deg = max_angle_deg.max(t_max);
        if t_max > 90.0 {
            obtuse_triangles += 1;
        }
        if t_min < 20.0 {
            sliver_triangles += 1;
        }
    }
    MeshQuality {
        triangle_count: mesh.faces.len(),
        min_angle_deg,
        p05_angle_deg: percentile(&tri_mins, 5.0),
        max_angle_deg,
        obtuse_triangles,
        sliver_triangles,
    }
}

/// One seam's sewing record (spec 04 §2.6).
#[derive(Clone, Debug, PartialEq)]
pub struct SeamRecord {
    /// Seam id.
    pub id: String,
    /// Sewing pairs `(side a, side b)`, without self-pairs.
    pub pairs: Vec<(usize, usize)>,
    /// Side a's global vertex path.
    pub side_a: Vec<usize>,
    /// Side b's path in its own loop order.
    pub side_b: Vec<usize>,
    /// `seam.ratio` (default 1).
    pub ratio: f64,
    /// `seam.gather_relief_mm` and `seam.gather_compression` when the seam declares either (spec 04 §5.4,
    /// native extension): the depth of the band of pleats below a gathered seam and the cloth's compression
    /// factor inside it. `None` is the reference's gather zone.
    pub gather_relief: Option<GatherRelief>,
}

/// A gathered seam's own band of pleats (spec 04 §5.4, native extension).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GatherRelief {
    /// Distance from the seam's long side over which the cloth keeps the floor factor, before the
    /// reference's [`crate::start::GATHER_ZONE_MM`] ramp back to the cloth's own factor begins. Millimetres.
    pub relief_mm: f64,
    /// Compression factor at the floor, in (0, 1]. The reference's is [`crate::start::GATHER_COMPRESSION`].
    pub compression: f64,
}

/// The garment mesh: 2D rest positions (mm, after `sim_scale`), CCW triangles and the sewing records.
#[derive(Clone, Debug)]
pub struct GarmentMesh {
    /// Simulated instance ids in construction order.
    pub instances: Vec<String>,
    /// 2D vertices (mm).
    pub verts2d: Vec<Vec2>,
    /// Triangles, counter-clockwise in 2D.
    pub faces: Vec<[usize; 3]>,
    /// Owning instance (index into `instances`) of each vertex.
    pub owner: Vec<usize>,
    /// Half-open global vertex range of each instance.
    pub inst_range: Vec<(usize, usize)>,
    /// All sewing pairs, seam by seam.
    pub pairs: Vec<(usize, usize)>,
    /// Seam records in pattern order.
    pub seams: Vec<SeamRecord>,
    /// `(instance, vertex)` → global index of the first sample of the edge starting there.
    pub corner: BTreeMap<(String, String), usize>,
    /// `(instance, edge)` → global indices along the edge, both ends included.
    pub index: BTreeMap<(String, String), Vec<usize>>,
}

fn doc_error(e: &PyError) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, e.message().to_string())
}

/// Admit the production drape's declared piece and seam scope in source order.
/// Every intended simulated instance needs a placement object with a string `part`; only an explicit
/// `sim: false` excludes a physical piece. Frame/landmark validity remains the scene's authority.
/// This does not change [`build_garment`]'s reference-compatible subset meshing primitive.
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] for invalid pattern structure/references, unresolved placements,
/// or no active instances. Missing placements never silently remove attachment seams.
pub fn simulation_scope(pattern: &Pattern) -> Result<Value, Refusal> {
    let invalid = |message| Refusal::new(RefusalCode::InvalidDocument, message);
    let errors = pattern.structure_errors();
    if !errors.is_empty() {
        return Err(invalid(format!(
            "cannot determine simulation scope: {}",
            errors.join("; ")
        )));
    }
    let mut simulated_instances = Vec::new();
    let mut excluded_instances = Vec::new();
    let mut excluded = BTreeMap::new();
    let mut missing = Vec::new();
    for (order, instance) in pattern.instances.iter().enumerate() {
        let piece = &pattern.raw()["pieces"][instance.piece_index];
        if piece["sim"] == false {
            excluded.insert(instance.id.as_str(), order);
            excluded_instances.push(json!({"instance":instance.id,"piece":instance.piece,
                "reason":"sim_false"}));
        } else if piece["placement"].as_object().is_some_and(|placement| {
            !placement.is_empty()
                && placement
                    .get("part")
                    .and_then(Value::as_str)
                    .is_some_and(|part| !part.trim().is_empty())
        }) {
            simulated_instances.push(instance.id.as_str());
        } else {
            missing.push(instance.id.as_str());
        }
    }
    let mut simulated_seams = Vec::new();
    let mut excluded_seams = Vec::new();
    let mut affected = Vec::new();
    for (index, seam) in pattern.seams().iter().enumerate() {
        let reference = |key| {
            seam.side_ref(key)
                .ok_or_else(|| invalid(format!("seam {}: missing side {key}", seam.id_text())))
        };
        let (a_ref, b_ref) = (reference("a")?, reference("b")?);
        let (a, _) = pattern.side(a_ref).map_err(|e| doc_error(&e))?;
        let (b, _) = pattern.side(b_ref).map_err(|e| doc_error(&e))?;
        let id = seam.0.get("id").cloned().unwrap_or(Value::Null);
        if missing.contains(&a.id.as_str()) || missing.contains(&b.id.as_str()) {
            affected.push(format!("#{index} {} ({a_ref} -> {b_ref})", seam.id_text()));
        }
        let mut omitted: Vec<_> = [a.id.as_str(), b.id.as_str()]
            .into_iter()
            .filter(|id| excluded.contains_key(id))
            .collect();
        omitted.sort_by_key(|id| excluded[id]);
        omitted.dedup();
        if omitted.is_empty() {
            simulated_seams.push(json!({"index":index,"id":id}));
        } else {
            excluded_seams.push(json!({"index":index,"id":id,"a":a_ref,"b":b_ref,
                "excluded_instances":omitted}));
        }
    }
    if !missing.is_empty() {
        return Err(invalid(format!(
            "unresolved simulation placements for instances [{}]; affected seams: {}",
            missing.join(", "),
            if affected.is_empty() {
                "none (unconnected instances still require a declared scope)".into()
            } else {
                affected.join("; ")
            }
        ))
        .repair("Supply explicit placement objects and body-part anchors for every intended simulated piece.")
        .repair("Set sim:false only for pieces deliberately excluded from this drape; their attachment seams will be reported as excluded."));
    }
    if simulated_instances.is_empty() {
        return Err(invalid("nothing to drape: no active physical instances in the declared simulation scope".into())
            .repair("Choose at least one piece to simulate and supply its placement; retain sim:false on deliberate exclusions."));
    }
    Ok(
        json!({"basis":if excluded_instances.is_empty() {"complete_pattern"} else {"declared_subset"},
        "simulated_instances":simulated_instances,"excluded_instances":excluded_instances,
        "simulated_seams":simulated_seams,"excluded_seams":excluded_seams}),
    )
}

/// `np.linalg.norm` of a 2-vector, as numpy evaluates it (√(x² + y²), not `hypot`).
fn norm2(d: Vec2) -> f64 {
    (d.x * d.x + d.y * d.y).sqrt()
}

/// Splits a seam path's samples (loop order) into per-edge lists at the samples nearest the internal
/// junction vertices (the reference's `_assign_path`).
fn assign_path(
    inst: &Instance,
    eids: &[String],
    pts: &[Vec2],
    edge_pts: &mut BTreeMap<(String, String), Vec<Vec2>>,
) -> Result<(), Refusal> {
    if pts.len() < 2 {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            format!(
                "{}: seam path has {} samples (need at least 2)",
                inst.id,
                pts.len()
            ),
        ));
    }
    let mut cuts = vec![0];
    for e in &eids[..eids.len().saturating_sub(1)] {
        let to = &inst.edge(e).map_err(|e| doc_error(&e))?.to;
        let v = inst
            .verts
            .get(to)
            .ok_or_else(|| Refusal::new(RefusalCode::InvalidDocument, format!("no vertex {to}")))?;
        let (mut best, mut best_d) = (0, f64::INFINITY);
        for (k, p) in pts.iter().enumerate() {
            let d = norm2(*p - v);
            if d < best_d {
                (best, best_d) = (k, d);
            }
        }
        cuts.push(best);
    }
    cuts.push(pts.len() - 1);
    for (k, e) in eids.iter().enumerate() {
        let (a, b) = (cuts[k], cuts[k + 1]);
        if a > b {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!(
                    "{}/{e}: non-monotonic seam junction cuts ({a} > {b})",
                    inst.id
                ),
            ));
        }
        edge_pts.insert((inst.id.clone(), e.clone()), pts[a..=b].to_vec());
    }
    Ok(())
}

fn path_indices(
    index: &BTreeMap<(String, String), Vec<usize>>,
    iid: &str,
    eids: &[String],
) -> Vec<usize> {
    let mut out = Vec::new();
    for (k, e) in eids.iter().enumerate() {
        let idx = &index[&(iid.to_string(), e.clone())];
        out.extend_from_slice(if k == 0 {
            idx
        } else {
            idx.get(1..).unwrap_or(&[])
        });
    }
    out
}

/// The reference's hex interior: rows `0.866·s` apart from `lo + s/2`, odd rows shifted `s/2`, kept
/// inside the loop (even-odd) and more than `0.55·s` from every boundary sample.
fn interior(boundary: &[Vec2], s: f64) -> Vec<Vec2> {
    let (mut lo, mut hi) = (boundary[0], boundary[0]);
    for p in boundary {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let gx = arange(lo.x + s / 2.0, hi.x, s);
    let gy = arange(lo.y + s / 2.0, hi.y, s * 0.866);
    let mut out = Vec::new();
    for (j, &y) in gy.iter().enumerate() {
        for &x in &gx {
            let p = Vec2::new(if j % 2 == 1 { x + s / 2.0 } else { x }, y);
            if !point_in_polygon(p, boundary) {
                continue;
            }
            let d = boundary
                .iter()
                .map(|b| norm2(p - *b))
                .fold(f64::INFINITY, f64::min);
            if d > 0.55 * s {
                out.push(p);
            }
        }
    }
    out
}

/// Signed triangle area (positive for CCW).
fn tri_area(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    0.5 * ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x))
}

/// Shortest Euclidean distance from point `p` to closed segment `[a, b]`.
fn dist_point_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let len2 = ab.x * ab.x + ab.y * ab.y;
    if len2 <= 0.0 {
        return norm2(p - a);
    }
    let t = (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2).clamp(0.0, 1.0);
    norm2(Vec2::new(p.x - (a.x + t * ab.x), p.y - (a.y + t * ab.y)))
}

/// Unsigned discrete turning angle (radians, in `[0, π]`) at each vertex of an open polyline.
fn turning_angles(pts: &[Vec2]) -> Vec<f64> {
    let n = pts.len();
    let mut turn = vec![0.0; n];
    if n < 3 {
        return turn;
    }
    for k in 1..(n - 1) {
        let v0 = pts[k] - pts[k - 1];
        let v1 = pts[k + 1] - pts[k];
        if norm2(v0) > 1e-12 && norm2(v1) > 1e-12 {
            let dot = v0.x * v1.x + v0.y * v1.y;
            let cross = (v0.x * v1.y - v0.y * v1.x).abs();
            turn[k] = det::atan2(cross, dot);
        }
    }
    turn
}

/// Cumulative arclength fractions `u ∈ [0, 1]` and cumulative adaptive sample-step monitor `M(u)`
/// combining base spacing, local boundary curvature (`turn / max_turning_angle_rad`), and the
/// minimum spacing floor `min_spacing`.
fn adaptive_density_curve(
    dense: &[Vec2],
    base_spacing: f64,
    min_spacing: f64,
    max_turning_angle_rad: f64,
) -> (Vec<f64>, Vec<f64>) {
    let s = cumulative(dense);
    let total = *s.last().unwrap_or(&0.0);
    if dense.len() < 2 || total <= 0.0 {
        return (vec![0.0, 1.0], vec![0.0, 1.0]);
    }
    let u: Vec<f64> = s.iter().map(|&v| (v / total).clamp(0.0, 1.0)).collect();
    let turn = turning_angles(dense);
    let rho_base = 1.0 / base_spacing.max(min_spacing);
    let rho_max = 1.0 / min_spacing;
    let mut m = Vec::with_capacity(dense.len());
    m.push(0.0);
    for j in 0..(dense.len() - 1) {
        let ds = (s[j + 1] - s[j]).max(0.0);
        let turn_seg = 0.5 * (turn[j] + turn[j + 1]);
        let dm = (ds * rho_base)
            .max(turn_seg / max_turning_angle_rad)
            .min(ds * rho_max);
        m.push(m[j] + dm);
    }
    (u, m)
}

/// Samples an open edge densely and redistributes boundary points according to curvature density.
fn adaptive_edge_samples(
    dense: &[Vec2],
    spacing: f64,
    min_spacing: f64,
    max_turning_angle_rad: f64,
    label: &str,
) -> Result<Vec<Vec2>, Refusal> {
    let (u, m) = adaptive_density_curve(dense, spacing, min_spacing, max_turning_angle_rad);
    let m_tot = *m.last().unwrap_or(&0.0);
    let steps = m_tot.ceil();
    if !steps.is_finite() || steps > 1_000_000.0 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("{label}: adaptive spacing produces too many boundary samples"),
        ));
    }
    let n = (steps as usize).max(1);
    let mut fracs = Vec::with_capacity(n + 1);
    fracs.push(0.0);
    if m_tot <= 0.0 {
        for k in 1..n {
            fracs.push(k as f64 / n as f64);
        }
    } else {
        for k in 1..n {
            let target = m_tot * (k as f64 / n as f64);
            fracs.push(interp(target, &m, &u).clamp(0.0, 1.0));
        }
    }
    fracs.push(1.0);
    Ok(at_fractions(dense, &fracs))
}

fn side_dense_polyline(inst: &Instance, eids: &[String]) -> Result<Vec<Vec2>, Refusal> {
    let mut out = Vec::new();
    for (k, eid) in eids.iter().enumerate() {
        let pts = inst
            .edge_dense(eid, EDGE_DENSE)
            .map_err(|e| doc_error(&e))?;
        if k == 0 {
            out.extend_from_slice(&pts);
        } else if pts.len() > 1 {
            out.extend_from_slice(&pts[1..]);
        }
    }
    Ok(out)
}

/// Matched curvature- and gather-adaptive seam samples preserving exact notch and edge-junction
/// alignment between `side_a` and `side_b`.
#[allow(clippy::too_many_arguments)]
fn adaptive_seam_pairs(
    seam: &Seam,
    pr: &ftx_pattern::SeamPairs,
    ia: &Instance,
    ea: &[String],
    ib: &Instance,
    eb: &[String],
    rev: bool,
    ratio: f64,
    spacing: f64,
    options: MeshOptions,
) -> Result<(Vec<Vec2>, Vec<Vec2>), Refusal> {
    let min_s = options.min_spacing_mm.unwrap_or(spacing);
    let a_dense = side_dense_polyline(ia, ea)?;
    let mut b_dense = side_dense_polyline(ib, eb)?;
    if rev {
        b_dense.reverse();
    }
    let gathered = (ratio - 1.0).abs() > crate::start::GATHER_RATIO_MARGIN;
    let base_s = if gathered {
        (spacing / options.gather_refinement).max(min_s)
    } else {
        spacing
    };
    let (ua, ma) = adaptive_density_curve(&a_dense, base_s, min_s, options.max_turning_angle_rad);
    let (ub, mb) = adaptive_density_curve(&b_dense, base_s, min_s, options.max_turning_angle_rad);
    let (ka, kb) = if !pr.notches_a.is_empty() && pr.notches_a.len() == pr.notches_b.len() {
        let wrap = |v: &[f64]| [&[0.0][..], v, &[1.0][..]].concat();
        (wrap(&pr.notches_a), wrap(&pr.notches_b))
    } else {
        (vec![0.0, 1.0], vec![0.0, 1.0])
    };
    let to_b = |f: f64| interp(f, &ka, &kb);
    let to_a = |f: f64| interp(f, &kb, &ka);
    let mut brk: Vec<f64> = ka
        .iter()
        .chain(&pr.a_junctions)
        .copied()
        .chain(pr.b_junctions.iter().map(|&f| to_a(f)))
        .map(|f| py::round(f, 9))
        .collect();
    brk.sort_by(f64::total_cmp);
    brk.dedup_by(|x, y| x == y);

    let (mut fa, mut fb) = (vec![0.0], vec![0.0]);
    for w in brk.windows(2) {
        let (u0, u1) = (w[0], w[1]);
        if u1 <= u0 {
            continue;
        }
        let (v0, v1) = (to_b(u0), to_b(u1));
        let mut us = Vec::with_capacity(ua.len() + ub.len() + 2);
        us.push(u0);
        us.push(u1);
        for &u in &ua {
            if u > u0 && u < u1 {
                us.push(u);
            }
        }
        for &v in &ub {
            if v > v0 && v < v1 {
                us.push(to_a(v).clamp(u0, u1));
            }
        }
        us.sort_by(f64::total_cmp);
        us.dedup_by(|x, y| (*y - *x).abs() <= 1e-12);
        if let Some(first) = us.first_mut() {
            *first = u0;
        }
        if let Some(last) = us.last_mut() {
            *last = u1;
        }
        let mut m_pair = Vec::with_capacity(us.len());
        m_pair.push(0.0);
        for k in 0..(us.len() - 1) {
            let dma = (interp(us[k + 1], &ua, &ma) - interp(us[k], &ua, &ma)).max(0.0);
            let dmb = (interp(to_b(us[k + 1]), &ub, &mb) - interp(to_b(us[k]), &ub, &mb)).max(0.0);
            m_pair.push(m_pair[k] + dma.max(dmb));
        }
        let m_tot = *m_pair.last().unwrap_or(&0.0);
        let steps = m_tot.ceil();
        if !steps.is_finite() || steps > 1_000_000.0 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                format!(
                    "seam {}: adaptive spacing produces too many samples",
                    seam.id_text()
                ),
            ));
        }
        let n = (steps as usize).max(1);
        for k in 1..=n {
            let u = if k == n {
                u1
            } else if m_tot <= 0.0 {
                u0 + (u1 - u0) * (k as f64 / n as f64)
            } else {
                interp(m_tot * (k as f64 / n as f64), &m_pair, &us).clamp(u0, u1)
            };
            fa.push(u);
            fb.push(to_b(u));
        }
    }
    Ok((at_fractions(&a_dense, &fa), at_fractions(&b_dense, &fb)))
}

/// 1-Lipschitz graded sizing field `h(p)` induced by the sampled boundary loop.
struct SizingField {
    max_spacing: f64,
    min_spacing: f64,
    sources: Vec<(Vec2, f64)>,
}

impl SizingField {
    fn from_boundary(boundary: &[Vec2], spacing: f64, min_spacing: f64) -> Self {
        let nb = boundary.len();
        let mut sources = Vec::new();
        if min_spacing < spacing && nb >= 2 {
            for i in 0..nb {
                let prev = boundary[(i + nb - 1) % nb];
                let curr = boundary[i];
                let next = boundary[(i + 1) % nb];
                let local_h =
                    (0.5 * (norm2(curr - prev) + norm2(next - curr))).clamp(min_spacing, spacing);
                if local_h < spacing * 0.9999 {
                    sources.push((curr, local_h));
                }
            }
        }
        Self {
            max_spacing: spacing,
            min_spacing,
            sources,
        }
    }

    fn eval(&self, p: Vec2) -> f64 {
        let mut h = self.max_spacing;
        for &(s, hs) in &self.sources {
            let cand = hs + SIZING_GRADING_BETA * norm2(p - s);
            if cand < h {
                h = cand;
            }
        }
        h.max(self.min_spacing)
    }
}

/// Deterministic flat 2D uniform cell index with exact brute-force fallback on extreme bounds.
struct SpatialGrid2D {
    lo: Vec2,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<usize>>,
    all: Vec<usize>,
    dense: bool,
}

impl SpatialGrid2D {
    fn new(lo: Vec2, hi: Vec2, cell: f64) -> Self {
        let dim = |span: f64| -> Option<usize> {
            let n = (span / cell).ceil();
            if (0.0..=511.0).contains(&n) {
                Some((n as usize).max(1) + 1)
            } else {
                None
            }
        };
        if let (Some(nx), Some(ny)) = (dim(hi.x - lo.x), dim(hi.y - lo.y))
            && nx.saturating_mul(ny) <= 262_144
        {
            return Self {
                lo,
                cell,
                nx,
                ny,
                cells: vec![Vec::new(); nx * ny],
                all: Vec::new(),
                dense: true,
            };
        }
        Self {
            lo,
            cell,
            nx: 0,
            ny: 0,
            cells: Vec::new(),
            all: Vec::new(),
            dense: false,
        }
    }

    fn coord(&self, p: Vec2) -> (usize, usize) {
        let cx = (((p.x - self.lo.x) / self.cell).floor() as isize).clamp(0, self.nx as isize - 1)
            as usize;
        let cy = (((p.y - self.lo.y) / self.cell).floor() as isize).clamp(0, self.ny as isize - 1)
            as usize;
        (cx, cy)
    }

    fn insert_point(&mut self, p: Vec2, id: usize) {
        if self.dense {
            let (cx, cy) = self.coord(p);
            self.cells[cy * self.nx + cx].push(id);
        } else {
            self.all.push(id);
        }
    }

    fn insert_segment(&mut self, a: Vec2, b: Vec2, id: usize) {
        if self.dense {
            let lo = Vec2::new(a.x.min(b.x), a.y.min(b.y));
            let hi = Vec2::new(a.x.max(b.x), a.y.max(b.y));
            let (x0, y0) = self.coord(lo);
            let (x1, y1) = self.coord(hi);
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    self.cells[cy * self.nx + cx].push(id);
                }
            }
        } else {
            self.all.push(id);
        }
    }

    fn any_nearby(&self, p: Vec2, mut pred: impl FnMut(usize) -> bool) -> bool {
        if !self.dense {
            return self.all.iter().copied().any(pred);
        }
        let (cx, cy) = self.coord(p);
        let x0 = cx.saturating_sub(1);
        let x1 = (cx + 1).min(self.nx - 1);
        let y0 = cy.saturating_sub(1);
        let y1 = (cy + 1).min(self.ny - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &id in &self.cells[y * self.nx + x] {
                    if pred(id) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Deterministic sizing-field-graded interior seeding with exact boundary-segment clearance.
fn adaptive_interior(
    boundary: &[Vec2],
    sizing: &SizingField,
    inst_id: &str,
) -> Result<Vec<Vec2>, Refusal> {
    let (mut lo, mut hi) = (boundary[0], boundary[0]);
    for &p in boundary {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let s_seed = if sizing.sources.is_empty() {
        sizing.max_spacing
    } else {
        sizing.min_spacing
    };
    let gx = arange(lo.x + s_seed / 2.0, hi.x, s_seed);
    let gy = arange(lo.y + s_seed / 2.0, hi.y, s_seed * 0.866);
    if gx.len().saturating_mul(gy.len()) > 2_000_000 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("{inst_id}: adaptive interior lattice exceeds 2,000,000 candidate sites"),
        ));
    }
    let nb = boundary.len();
    let mut seg_grid = SpatialGrid2D::new(lo, hi, sizing.max_spacing);
    for i in 0..nb {
        seg_grid.insert_segment(boundary[i], boundary[(i + 1) % nb], i);
    }
    let mut candidates: Vec<(f64, usize, usize, Vec2)> = Vec::new();
    for (j, &y) in gy.iter().enumerate() {
        for (i, &x) in gx.iter().enumerate() {
            let p = Vec2::new(if j % 2 == 1 { x + s_seed / 2.0 } else { x }, y);
            if !point_in_polygon(p, boundary) {
                continue;
            }
            let hp = sizing.eval(p);
            let boundary_clear = 0.55 * hp;
            let too_close_to_boundary = seg_grid.any_nearby(p, |seg_idx| {
                dist_point_segment(p, boundary[seg_idx], boundary[(seg_idx + 1) % nb])
                    <= boundary_clear
            });
            if too_close_to_boundary {
                continue;
            }
            candidates.push((hp, j, i, p));
        }
    }
    if sizing.sources.is_empty() {
        return Ok(candidates.into_iter().map(|(_, _, _, p)| p).collect());
    }
    // Accept finest sizing sites first (deterministic tie-break by lattice row/col) so coarse
    // interior points never shadow out refined triangles along tight curves or gathered seams.
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut pt_grid = SpatialGrid2D::new(lo, hi, sizing.max_spacing);
    let mut accepted: Vec<(Vec2, f64)> = Vec::new();
    for (hp, _, _, p) in candidates {
        let too_close_to_interior = pt_grid.any_nearby(p, |id| {
            let (q, hq) = accepted[id];
            norm2(p - q) < 0.85 * 0.5 * (hp + hq)
        });
        if too_close_to_interior {
            continue;
        }
        let id = accepted.len();
        accepted.push((p, hp));
        pt_grid.insert_point(p, id);
    }
    Ok(accepted.into_iter().map(|(p, _)| p).collect())
}

/// Clamped Voronoi center of a CCW triangle: exact circumcenter when acute, or the midpoint of the
/// hypotenuse when obtuse (guaranteed to lie on/inside the triangle).
fn triangle_voronoi_center(a: Vec2, b: Vec2, c: Vec2) -> Vec2 {
    let ab = b - a;
    let ac = c - a;
    if ab.x * ac.x + ab.y * ac.y <= 0.0 {
        return (b + c) * 0.5;
    }
    let ba = a - b;
    let bc = c - b;
    if ba.x * bc.x + ba.y * bc.y <= 0.0 {
        return (a + c) * 0.5;
    }
    let ca = a - c;
    let cb = b - c;
    if ca.x * cb.x + ca.y * cb.y <= 0.0 {
        return (a + b) * 0.5;
    }
    let d = 2.0 * (ab.x * ac.y - ab.y * ac.x);
    if !d.is_finite() || d.abs() <= 1e-18 {
        return (a + b + c) / 3.0;
    }
    let ab2 = ab.x * ab.x + ab.y * ab.y;
    let ac2 = ac.x * ac.x + ac.y * ac.y;
    let cc = a + Vec2::new((ab2 * ac.y - ac2 * ab.y) / d, (ac2 * ab.x - ab2 * ac.x) / d);
    if cc.x.is_finite() && cc.y.is_finite() {
        cc
    } else {
        (a + b + c) / 3.0
    }
}

/// Bounded density-weighted interior Lloyd CVT smoothing (`ρ(x) = h(x)⁻²`, fixed boundary vertices,
/// star-validity backtracking, and constrained Delaunay re-triangulation after each pass).
fn smooth_interior_cvt(
    allp: &mut [Vec2],
    nb: usize,
    mut faces: Vec<[usize; 3]>,
    sizing: &SizingField,
    iterations: usize,
    inst_id: &str,
) -> Result<Vec<[usize; 3]>, Refusal> {
    let n_int = allp.len().saturating_sub(nb);
    if iterations == 0 || n_int == 0 {
        return Ok(faces);
    }
    let stop_tol = CVT_STOP_REL * sizing.min_spacing;
    for _ in 0..iterations {
        let mut star: Vec<Vec<[usize; 3]>> = vec![Vec::new(); n_int];
        for &[i, j, k] in &faces {
            if i >= nb {
                star[i - nb].push([i, j, k]);
            }
            if j >= nb {
                star[j - nb].push([j, k, i]);
            }
            if k >= nb {
                star[k - nb].push([k, i, j]);
            }
        }
        let mut max_move = 0.0_f64;
        for v in nb..allp.len() {
            let inc = &star[v - nb];
            if inc.len() < 3 {
                continue;
            }
            let pv = allp[v];
            let mut num = Vec2::ZERO;
            let mut den = 0.0_f64;
            for &[_, u, w] in inc {
                if u == v || w == v || u == w {
                    continue;
                }
                let (pu, pw) = (allp[u], allp[w]);
                let ct = triangle_voronoi_center(pv, pu, pw);
                let m_vu = (pv + pu) * 0.5;
                let m_vw = (pv + pw) * 0.5;
                for (a, g) in [
                    (tri_area(pv, m_vu, ct).max(0.0), (pv + m_vu + ct) / 3.0),
                    (tri_area(pv, ct, m_vw).max(0.0), (pv + ct + m_vw) / 3.0),
                ] {
                    if a > 0.0 {
                        let h = sizing.eval(g);
                        let wt = a / (h * h);
                        num += g * wt;
                        den += wt;
                    }
                }
            }
            if den <= 0.0 || !num.x.is_finite() || !num.y.is_finite() {
                continue;
            }
            let target = num / den;
            let delta = (target - pv) * CVT_OMEGA;
            for alpha in [1.0, 0.5, 0.25, 0.125, 0.0625] {
                let cand = pv + delta * alpha;
                if !(cand.x.is_finite() && cand.y.is_finite()) {
                    continue;
                }
                let star_ok = inc.iter().all(|&[_, u, w]| {
                    let old_a = tri_area(pv, allp[u], allp[w]);
                    let new_a = tri_area(cand, allp[u], allp[w]);
                    new_a > 0.1 * old_a && new_a > 1e-12
                });
                if star_ok && point_in_polygon(cand, &allp[..nb]) {
                    allp[v] = cand;
                    max_move = max_move.max(norm2(cand - pv));
                    break;
                }
            }
        }
        faces = triangulate(allp, nb, inst_id)?;
        if max_move < stop_tol {
            break;
        }
    }
    Ok(faces)
}

/// Opt-in garment construction mechanics multipliers for folded hem bands and sewn seam allowances
/// (FTX-27d).
///
/// Multipliers are dimensionless factors applied on top of each triangle's single-ply areal mass
/// and each hinge's base bending rigidity. Default values (`1.0×` everywhere, `0.0 mm` band
/// widths) are a bit-identical no-op preserving legacy digests and omitted from serialized reports
/// when default.
///
/// **No-Claim**: generic reference multipliers (e.g. `2×–3×` folded hem mass, `3×–8×` hem bending,
/// `2×–4×` seam-allowance bending) are uncalibrated structural approximations, not measured
/// laminate or stitch-class properties and do not establish garment fit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConstructionMechanicsOptions {
    /// Width of the folded hem band measured inward from open (non-seam) boundary edges, in mm
    /// (default `0.0`; e.g. `15.0–25.0` mm for a double-folded hem).
    pub hem_width_mm: f64,
    /// Areal mass multiplier applied to triangles in the folded hem band (`>= 1.0`, default `1.0`).
    pub hem_mass_multiplier: f64,
    /// Bending rigidity multiplier applied to hinges in the folded hem band (`>= 1.0`, default
    /// `1.0`).
    pub hem_bending_multiplier: f64,
    /// Width of the pressed seam-allowance band measured inward from sewn seam edges, in mm
    /// (default `0.0`: only the welded seam hinge line itself is stiffened unless `seam_width_mm > 0`).
    pub seam_width_mm: f64,
    /// Areal mass multiplier applied to triangles in the seam-allowance band (`>= 1.0`, default
    /// `1.0`).
    pub seam_mass_multiplier: f64,
    /// Bending rigidity multiplier applied to welded seam hinges and interior hinges inside
    /// `seam_width_mm` of a sewn seam (`>= 1.0`, default `1.0`).
    pub seam_bending_multiplier: f64,
}

impl Default for ConstructionMechanicsOptions {
    fn default() -> Self {
        Self {
            hem_width_mm: 0.0,
            hem_mass_multiplier: 1.0,
            hem_bending_multiplier: 1.0,
            seam_width_mm: 0.0,
            seam_mass_multiplier: 1.0,
            seam_bending_multiplier: 1.0,
        }
    }
}

impl ConstructionMechanicsOptions {
    /// Uncalibrated reference garment construction defaults (20 mm folded hem at 2.5× mass / 5×
    /// bending, 10 mm seam allowance at 2× mass / 3× bending). Structural approximation only, not
    /// measured laminate constants.
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            hem_width_mm: 20.0,
            hem_mass_multiplier: 2.5,
            hem_bending_multiplier: 5.0,
            seam_width_mm: 10.0,
            seam_mass_multiplier: 2.0,
            seam_bending_multiplier: 3.0,
        }
    }

    /// True when all fields equal the legacy no-op default.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// True when any mass or bending multiplier exceeds `1.0`.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.hem_mass_multiplier > 1.0
            || self.hem_bending_multiplier > 1.0
            || self.seam_mass_multiplier > 1.0
            || self.seam_bending_multiplier > 1.0
    }

    /// Validates all widths and multipliers fail-closed.
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] for any non-finite value; [`RefusalCode::OutOfRange`] for
    /// out-of-range widths/multipliers or band multipliers > 1 paired with zero band width.
    pub fn validate(&self) -> Result<(), Refusal> {
        for (name, value) in [
            ("hem band width", self.hem_width_mm),
            ("hem mass multiplier", self.hem_mass_multiplier),
            ("hem bending multiplier", self.hem_bending_multiplier),
            ("seam allowance width", self.seam_width_mm),
            ("seam mass multiplier", self.seam_mass_multiplier),
            ("seam bending multiplier", self.seam_bending_multiplier),
        ] {
            Refusal::finite(name, value)?;
        }
        if !(0.0..=200.0).contains(&self.hem_width_mm) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "hem_width_mm {} must be within [0, 200] mm",
                    self.hem_width_mm
                ),
            ));
        }
        if !(0.0..=100.0).contains(&self.seam_width_mm) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "seam_width_mm {} must be within [0, 100] mm",
                    self.seam_width_mm
                ),
            ));
        }
        if !(1.0..=10.0).contains(&self.hem_mass_multiplier)
            || !(1.0..=10.0).contains(&self.seam_mass_multiplier)
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "hem and seam mass multipliers must be within [1, 10]",
            ));
        }
        if !(1.0..=32.0).contains(&self.hem_bending_multiplier)
            || !(1.0..=32.0).contains(&self.seam_bending_multiplier)
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "hem and seam bending multipliers must be within [1, 32]",
            ));
        }
        if (self.hem_mass_multiplier > 1.0 || self.hem_bending_multiplier > 1.0)
            && self.hem_width_mm <= 0.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "hem mass/bending multipliers > 1 require hem_width_mm > 0",
            ));
        }
        if self.hem_width_mm > 0.0
            && self.hem_mass_multiplier == 1.0
            && self.hem_bending_multiplier == 1.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "hem_width_mm > 0 requires hem_mass_multiplier > 1 or hem_bending_multiplier > 1",
            ));
        }
        if self.seam_mass_multiplier > 1.0 && self.seam_width_mm <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "seam_mass_multiplier > 1 requires seam_width_mm > 0",
            ));
        }
        if self.seam_width_mm > 0.0
            && self.seam_mass_multiplier == 1.0
            && self.seam_bending_multiplier == 1.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "seam_width_mm > 0 requires seam_mass_multiplier > 1 or seam_bending_multiplier > 1",
            ));
        }
        Ok(())
    }
}

/// Deterministic per-triangle and per-hinge construction multipliers derived from flat rest
/// geometry, piece ownership, and welded seam topology.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstructionField {
    /// Areal mass multiplier per retained triangle (`>= 1.0`).
    pub triangle_mass_multiplier: Vec<f64>,
    /// Band bending multiplier per retained triangle (`>= 1.0`).
    pub triangle_bending_multiplier: Vec<f64>,
    /// Effective bending rigidity multiplier per solver hinge (`>= 1.0`).
    pub hinge_bending_multiplier: Vec<f64>,
    /// Number of retained triangles classified inside the open hem band.
    pub hem_triangles: usize,
    /// Number of retained triangles classified inside the seam-allowance band.
    pub seam_band_triangles: usize,
    /// Number of welded seam hinges (`common_source_verts < 2`).
    pub seam_hinges: usize,
    /// Number of open (non-seam) boundary edges identified across all pieces.
    pub hem_edges: usize,
}

fn point_seg_dist2(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let ab2 = ab.x * ab.x + ab.y * ab.y;
    let t = if ab2 > 0.0 {
        ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / ab2
    } else {
        0.0
    }
    .clamp(0.0, 1.0);
    let q = a + ab * t;
    let d = p - q;
    d.x * d.x + d.y * d.y
}

fn build_segment_grid(segs: &[(Vec2, Vec2)], width_m: f64) -> SpatialGrid2D {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &(a, b) in segs {
        lo.x = lo.x.min(a.x).min(b.x);
        lo.y = lo.y.min(a.y).min(b.y);
        hi.x = hi.x.max(a.x).max(b.x);
        hi.y = hi.y.max(a.y).max(b.y);
    }
    let pad = (width_m + 1e-12) * 1.01;
    let mut grid = SpatialGrid2D::new(
        Vec2::new(lo.x - pad, lo.y - pad),
        Vec2::new(hi.x + pad, hi.y + pad),
        pad.max(1e-6),
    );
    for (id, &(a, b)) in segs.iter().enumerate() {
        grid.insert_segment(a, b, id);
    }
    grid
}

/// Classifies open (non-seam) hem boundaries, welded seam hinges, and their inward construction
/// bands in each piece's flat rest coordinates (metres).
///
/// # Errors
/// [`RefusalCode::NonFinite`] or [`RefusalCode::OutOfRange`] on invalid `options`;
/// [`RefusalCode::DegenerateGeometry`] on mismatched array lengths, out-of-range indices, or
/// non-manifold welded edges shared by more than two retained triangles.
pub fn compute_construction_field(
    source_rest_m: &[Vec2],
    source_triangles: &[[usize; 3]],
    triangle_owner: &[usize],
    weld: &[usize],
    hinge_support: &[[(usize, f64); 2]],
    options: ConstructionMechanicsOptions,
) -> Result<ConstructionField, Refusal> {
    options.validate()?;
    let n_tris = source_triangles.len();
    if triangle_owner.len() != n_tris || source_rest_m.len() != weld.len() {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "construction field requires matching triangle owners and source/weld vertex maps",
        ));
    }
    if source_rest_m
        .iter()
        .any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "construction field requires finite flat rest coordinates",
        ));
    }
    type ConstructionEdgeUses = BTreeMap<(usize, usize), Vec<(usize, [usize; 2])>>;
    let mut edge_uses: ConstructionEdgeUses = BTreeMap::new();
    for (t, tri) in source_triangles.iter().enumerate() {
        if tri.iter().any(|&v| v >= source_rest_m.len()) {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("construction source triangle {t} indexes a missing vertex"),
            ));
        }
        let w = tri.map(|v| weld[v]);
        if w[0] == w[1] || w[1] == w[2] || w[0] == w[2] {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("construction triangle {t} is collapsed after welding"),
            ));
        }
        for k in 0..3 {
            let (sa, sb) = (tri[k], tri[(k + 1) % 3]);
            let (wa, wb) = (w[k], w[(k + 1) % 3]);
            edge_uses
                .entry((wa.min(wb), wa.max(wb)))
                .or_default()
                .push((t, [sa, sb]));
        }
    }

    let mut hem_segments: BTreeMap<usize, Vec<(Vec2, Vec2)>> = BTreeMap::new();
    let mut seam_segments: BTreeMap<usize, Vec<(Vec2, Vec2)>> = BTreeMap::new();
    let mut hem_edges = 0usize;
    for (key, uses) in &edge_uses {
        match uses.as_slice() {
            [(t, [sa, sb])] => {
                hem_edges += 1;
                hem_segments
                    .entry(triangle_owner[*t])
                    .or_default()
                    .push((source_rest_m[*sa], source_rest_m[*sb]));
            }
            [(t1, [a1, b1]), (t2, [a2, b2])] => {
                let common = source_triangles[*t1]
                    .iter()
                    .filter(|v| source_triangles[*t2].contains(v))
                    .count();
                if common < 2 {
                    seam_segments
                        .entry(triangle_owner[*t1])
                        .or_default()
                        .push((source_rest_m[*a1], source_rest_m[*b1]));
                    seam_segments
                        .entry(triangle_owner[*t2])
                        .or_default()
                        .push((source_rest_m[*a2], source_rest_m[*b2]));
                }
            }
            _ => {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    format!("construction edge {key:?} is shared by more than two triangles"),
                ));
            }
        }
    }

    let hem_w_m = options.hem_width_mm * 1e-3;
    let seam_w_m = options.seam_width_mm * 1e-3;
    let hem_w_tol2 = (hem_w_m + 1e-12) * (hem_w_m + 1e-12);
    let seam_w_tol2 = (seam_w_m + 1e-12) * (seam_w_m + 1e-12);

    let hem_grids: BTreeMap<usize, SpatialGrid2D> = if hem_w_m > 0.0 {
        hem_segments
            .iter()
            .map(|(&owner, segs)| (owner, build_segment_grid(segs, hem_w_m)))
            .collect()
    } else {
        BTreeMap::new()
    };
    let seam_grids: BTreeMap<usize, SpatialGrid2D> = if seam_w_m > 0.0 {
        seam_segments
            .iter()
            .map(|(&owner, segs)| (owner, build_segment_grid(segs, seam_w_m)))
            .collect()
    } else {
        BTreeMap::new()
    };

    let mut triangle_mass_multiplier = Vec::with_capacity(n_tris);
    let mut triangle_bending_multiplier = Vec::with_capacity(n_tris);
    let mut hem_triangles = 0usize;
    let mut seam_band_triangles = 0usize;

    for (t, tri) in source_triangles.iter().enumerate() {
        let owner = triangle_owner[t];
        let c = (source_rest_m[tri[0]] + source_rest_m[tri[1]] + source_rest_m[tri[2]]) / 3.0;
        let in_hem = hem_w_m > 0.0
            && hem_grids.get(&owner).is_some_and(|grid| {
                let segs = &hem_segments[&owner];
                grid.any_nearby(c, |id| {
                    let (a, b) = segs[id];
                    point_seg_dist2(c, a, b) <= hem_w_tol2
                })
            });
        let in_seam = seam_w_m > 0.0
            && seam_grids.get(&owner).is_some_and(|grid| {
                let segs = &seam_segments[&owner];
                grid.any_nearby(c, |id| {
                    let (a, b) = segs[id];
                    point_seg_dist2(c, a, b) <= seam_w_tol2
                })
            });
        let mut m_mult = 1.0_f64;
        let mut b_mult = 1.0_f64;
        if in_hem {
            hem_triangles += 1;
            m_mult = m_mult.max(options.hem_mass_multiplier);
            b_mult = b_mult.max(options.hem_bending_multiplier);
        }
        if in_seam {
            seam_band_triangles += 1;
            m_mult = m_mult.max(options.seam_mass_multiplier);
            b_mult = b_mult.max(options.seam_bending_multiplier);
        }
        triangle_mass_multiplier.push(m_mult);
        triangle_bending_multiplier.push(b_mult);
    }

    let mut hinge_bending_multiplier = Vec::with_capacity(hinge_support.len());
    let mut seam_hinges = 0usize;
    for &[(t1, wa), (t2, wb)] in hinge_support {
        if t1 >= n_tris
            || t2 >= n_tris
            || !wa.is_finite()
            || !wb.is_finite()
            || wa < 0.0
            || wb < 0.0
            || wa + wb <= 0.0
        {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "invalid hinge support in construction field",
            ));
        }
        let common = source_triangles[t1]
            .iter()
            .filter(|v| source_triangles[t2].contains(v))
            .count();
        let is_seam_hinge = common < 2;
        let (ba, bb) = (
            triangle_bending_multiplier[t1],
            triangle_bending_multiplier[t2],
        );
        let mut hm = if ba == bb {
            ba
        } else {
            let scale = wa.max(wb);
            let (u, v) = (wa / scale, wb / scale);
            ba * (u / (u + v)) + bb * (v / (u + v))
        };
        if is_seam_hinge {
            seam_hinges += 1;
            hm = hm.max(options.seam_bending_multiplier);
        }
        hinge_bending_multiplier.push(hm);
    }

    Ok(ConstructionField {
        triangle_mass_multiplier,
        triangle_bending_multiplier,
        hinge_bending_multiplier,
        hem_triangles,
        seam_band_triangles,
        seam_hinges,
        hem_edges,
    })
}

/// Constrained Delaunay of `points` with the first `nb` as the closed boundary loop; the triangles
/// inside the loop, counter-clockwise, as indices into `points`.
fn triangulate(points: &[Vec2], nb: usize, inst: &str) -> Result<Vec<[usize; 3]>, Refusal> {
    let bad = |m: String| Refusal::new(RefusalCode::DegenerateGeometry, m);
    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> =
        ConstrainedDelaunayTriangulation::new();
    let mut handles = Vec::with_capacity(points.len());
    // First input index of each spade vertex (coincident points share one).
    let mut first: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, p) in points.iter().enumerate() {
        let h = cdt
            .insert(Point2::new(p.x, p.y))
            .map_err(|e| bad(format!("{inst}: cannot triangulate point {i}: {e:?}")))?;
        first.entry(h.index()).or_insert(i);
        handles.push(h);
    }
    for k in 0..nb {
        let (a, b) = (handles[k], handles[(k + 1) % nb]);
        if a == b {
            continue;
        }
        if !cdt.can_add_constraint(a, b) {
            return Err(bad(format!(
                "{inst}: the outline crosses itself near boundary sample {k}"
            )));
        }
        cdt.add_constraint(a, b);
    }
    // Inside = an odd number of constraint edges crossed on the way in from the outer face (a flood
    // fill over face adjacency, as the reference keeps the faces of the constraint loop's region; a
    // centroid test would keep slivers along straight edges whose centroid rounds onto the line).
    let mut adjacent: Vec<Vec<(usize, usize)>> = vec![Vec::new(); cdt.num_all_faces()];
    for e in cdt.undirected_edges() {
        let d = e.as_directed();
        let (f1, f2) = (d.face().fix().index(), d.rev().face().fix().index());
        let crossing = usize::from(cdt.is_constraint_edge(e.fix()));
        adjacent[f1].push((f2, crossing));
        adjacent[f2].push((f1, crossing));
    }
    let mut depth: Vec<Option<usize>> = vec![None; adjacent.len()];
    let outer = cdt.outer_face().fix().index();
    depth[outer] = Some(0);
    let mut queue = std::collections::VecDeque::from([outer]);
    while let Some(fi) = queue.pop_front() {
        let d = depth[fi].unwrap_or(0);
        for &(n, crossing) in &adjacent[fi] {
            if depth[n].is_none() {
                depth[n] = Some(d + crossing);
                queue.push_back(n);
            }
        }
    }
    let mut faces = Vec::new();
    for f in cdt.inner_faces() {
        if depth[f.fix().index()].unwrap_or(0).is_multiple_of(2) {
            continue;
        }
        let v = f.vertices().map(|h| first[&h.fix().index()]);
        let (a, b, c) = (points[v[0]], points[v[1]], points[v[2]]);
        let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        faces.push(if cross < 0.0 { [v[2], v[1], v[0]] } else { v });
    }
    Ok(faces)
}

fn number(v: &Value, what: &str) -> Result<f64, Refusal> {
    v.as_f64().ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("{what} must be a number, not {v}"),
        )
    })
}

/// Builds the garment mesh of `pattern` at `spacing` mm (spec 04 §2; the reference uses 12 mm).
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] for a broken reference (unknown edge, malformed seam, a non-numeric
/// ratio or `sim_scale`); [`RefusalCode::DegenerateGeometry`] for implausible seam sides, an outline
/// that crosses itself, or seam sides with different sample counts; [`RefusalCode::OutOfRange`] for a
/// non-positive spacing.
pub fn build_garment(pattern: &Pattern, spacing: f64) -> Result<GarmentMesh, Refusal> {
    build_garment_with_options(pattern, spacing, MeshOptions::default())
}

/// Builds the garment mesh of `pattern` at nominal `spacing` mm with optional curvature/gather
/// refinement and bounded interior Lloyd CVT smoothing ([`MeshOptions`]).
///
/// When `options` is [`MeshOptions::default()`], the output is bit-identical to [`build_garment`].
///
/// # Errors
/// As [`build_garment`], plus [`MeshOptions::validate`] and [`RefusalCode::BudgetExceeded`] if
/// adaptive refinement exceeds sample or lattice bounds.
#[allow(clippy::too_many_lines)] // one pass, in the reference's order
pub fn build_garment_with_options(
    pattern: &Pattern,
    spacing: f64,
    options: MeshOptions,
) -> Result<GarmentMesh, Refusal> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("mesh spacing {spacing} mm must be positive"),
        ));
    }
    if !options.is_default() {
        options.validate(spacing)?;
    }
    if !pattern.build_errors.is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            format!(
                "cannot mesh malformed pattern: {}",
                pattern.build_errors.join("; ")
            ),
        ));
    }
    let raw = pattern.raw();
    let piece_def = |inst: &Instance| &raw["pieces"][inst.piece_index];
    let insts: Vec<&Instance> = pattern
        .instances
        .iter()
        .filter(|i| {
            let p = piece_def(i);
            p.get("sim") != Some(&Value::Bool(false)) && p.get("placement").is_some_and(truthy)
        })
        .collect();
    let ids: Vec<&str> = insts.iter().map(|i| i.id.as_str()).collect();
    let on_sim =
        |r: Option<&str>| r.is_some_and(|r| ids.contains(&r.split('/').next().unwrap_or(r)));
    let mut edge_pts: BTreeMap<(String, String), Vec<Vec2>> = BTreeMap::new();
    let mut seam_info = Vec::new();
    for s in pattern.seams() {
        if !(on_sim(s.side_ref("a")) && on_sim(s.side_ref("b"))) {
            continue;
        }
        let pr = pattern
            .seam_pairs(s, spacing)
            .map_err(|e| Refusal::new(RefusalCode::DegenerateGeometry, e.message().to_string()))?;
        let (ra, rb) = (s.side_ref("a").unwrap_or(""), s.side_ref("b").unwrap_or(""));
        let (ia, ea) = pattern.side(ra).map_err(|e| doc_error(&e))?;
        let (ib, eb) = pattern.side(rb).map_err(|e| doc_error(&e))?;
        let rev = s.reverse();
        let ratio = match s.0.get("ratio") {
            None => 1.0,
            Some(v) => number(v, &format!("seam {} ratio", s.id_text()))?,
        };
        let (a_pts, b_pts) = if options.min_spacing_mm.is_some() {
            adaptive_seam_pairs(s, &pr, ia, &ea, ib, &eb, rev, ratio, spacing, options)?
        } else {
            (pr.a, pr.b)
        };
        let b_loop: Vec<Vec2> = if rev {
            b_pts.iter().rev().copied().collect()
        } else {
            b_pts
        };
        assign_path(ia, &ea, &a_pts, &mut edge_pts)?;
        assign_path(ib, &eb, &b_loop, &mut edge_pts)?;
        let declared = |key: &str| {
            s.0.get(key)
                .map(|v| number(v, &format!("seam {} {key}", s.id_text())))
                .transpose()
        };
        let gather_relief = match (
            declared("gather_relief_mm")?,
            declared("gather_compression")?,
        ) {
            (None, None) => None,
            (relief, compression) => {
                let relief = GatherRelief {
                    relief_mm: relief.unwrap_or(0.0),
                    compression: compression.unwrap_or(crate::start::GATHER_COMPRESSION),
                };
                let valid = relief.relief_mm.is_finite()
                    && relief.relief_mm >= 0.0
                    && relief.compression > 0.0
                    && relief.compression <= 1.0;
                if !valid {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!(
                            "seam {}: gather_relief_mm must be a length of 0 or more and gather_compression a factor in (0, 1] (got {} and {})",
                            s.id_text(),
                            relief.relief_mm,
                            relief.compression
                        ),
                    )
                    .repair("Omit both for the reference's gather zone: 3 % compression at the seam, back to the cloth's own within 50 mm."));
                }
                // The solver gathers only a seam whose ratio is clear of 1; on any other seam the band
                // would be ignored, so it is refused instead.
                if (ratio - 1.0).abs() <= crate::start::GATHER_RATIO_MARGIN {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!(
                            "seam {}: a band of pleats is declared, but ratio {ratio} is within {} of 1, which the solver does not gather",
                            s.id_text(),
                            crate::start::GATHER_RATIO_MARGIN
                        ),
                    )
                    .repair("Remove gather_relief_mm and gather_compression from this seam, or give it the ratio of its gathered length to its sewn length."));
                }
                Some(relief)
            }
        };
        seam_info.push((
            s.id_text(),
            ia.id.clone(),
            ea,
            ib.id.clone(),
            eb,
            rev,
            ratio,
            gather_relief,
        ));
    }

    let mut mesh = GarmentMesh {
        instances: ids.iter().map(|s| (*s).to_string()).collect(),
        verts2d: Vec::new(),
        faces: Vec::new(),
        owner: Vec::new(),
        inst_range: Vec::new(),
        pairs: Vec::new(),
        seams: Vec::new(),
        corner: BTreeMap::new(),
        index: BTreeMap::new(),
    };
    for (k, inst) in insts.iter().enumerate() {
        let base = mesh.verts2d.len();
        let mut loop_pts: Vec<Vec2> = Vec::new();
        let mut loop_edge_idx = Vec::new();
        for e in &inst.edges {
            let key = (inst.id.clone(), e.id.clone());
            let pts = if let Some(p) = edge_pts.get(&key) {
                p.clone()
            } else {
                let dense = inst
                    .edge_dense(&e.id, EDGE_DENSE)
                    .map_err(|e| doc_error(&e))?;
                if let Some(min_s) = options.min_spacing_mm {
                    adaptive_edge_samples(
                        &dense,
                        spacing,
                        min_s,
                        options.max_turning_angle_rad,
                        &format!("{}/{}", inst.id, e.id),
                    )?
                } else {
                    let n = ((polyline_length(&dense) / spacing).ceil() as usize).max(1);
                    at_fractions(&dense, &linspace(0.0, 1.0, n + 1))
                }
            };
            let start = loop_pts.len();
            loop_pts.extend_from_slice(&pts[..pts.len().saturating_sub(1)]);
            loop_edge_idx.push((e.id.clone(), e.from.clone(), start, pts.len()));
        }
        let nb = loop_pts.len();
        if nb < 3 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("{}: the outline has {nb} boundary samples", inst.id),
            ));
        }
        for (eid, from, start, n) in &loop_edge_idx {
            mesh.index.insert(
                (inst.id.clone(), eid.clone()),
                (0..*n).map(|j| base + (start + j) % nb).collect(),
            );
            mesh.corner
                .insert((inst.id.clone(), from.clone()), base + start);
        }
        let mut allp = loop_pts.clone();
        let faces = if options.is_default() {
            allp.extend(interior(&loop_pts, spacing));
            triangulate(&allp, nb, &inst.id)?
        } else {
            let min_s = options.min_spacing_mm.unwrap_or(spacing);
            let sizing = SizingField::from_boundary(&loop_pts, spacing, min_s);
            if options.min_spacing_mm.is_some() {
                allp.extend(adaptive_interior(&loop_pts, &sizing, &inst.id)?);
            } else {
                allp.extend(interior(&loop_pts, spacing));
            }
            let faces = triangulate(&allp, nb, &inst.id)?;
            smooth_interior_cvt(
                &mut allp,
                nb,
                faces,
                &sizing,
                options.cvt_iterations,
                &inst.id,
            )?
        };
        for f in faces {
            mesh.faces.push(f.map(|j| base + j));
        }
        mesh.owner.extend(std::iter::repeat_n(k, allp.len()));
        mesh.verts2d.extend(allp);
        mesh.inst_range.push((base, mesh.verts2d.len()));
    }

    let pos = |id: &str| ids.iter().position(|x| *x == id).unwrap_or(0);
    for (sid, ia, ea, ib, eb, rev, ratio, gather_relief) in seam_info {
        let pa = path_indices(&mesh.index, &ia, &ea);
        let mut pb = path_indices(&mesh.index, &ib, &eb);
        if rev {
            pb.reverse();
        }
        if pa.len() != pb.len() {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("seam {sid}: {} vs {} samples", pa.len(), pb.len()),
            ));
        }
        let sp: Vec<(usize, usize)> = pa
            .iter()
            .zip(&pb)
            .filter(|(a, b)| a != b)
            .map(|(a, b)| (*a, *b))
            .collect();
        mesh.pairs.extend_from_slice(&sp);
        if rev {
            pb.reverse();
        }
        mesh.seams.push(SeamRecord {
            id: sid,
            pairs: sp,
            side_a: pa,
            side_b: pb,
            ratio,
            gather_relief,
        });
    }
    for inst in &insts {
        if let Some(sc) = piece_def(inst).get("sim_scale").filter(|v| truthy(v)) {
            let pair = sc.as_array().filter(|a| a.len() == 2).ok_or_else(|| {
                Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!("{}: sim_scale must be [sx, sy]", inst.id),
                )
            })?;
            let (sx, sy) = (
                number(&pair[0], "sim_scale[0]")?,
                number(&pair[1], "sim_scale[1]")?,
            );
            if !(sx.is_finite() && sx > 0.0 && sy.is_finite() && sy > 0.0) {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "{}: sim_scale [{sx}, {sy}] must be finite and positive",
                        inst.id
                    ),
                ));
            }
            let (lo, hi) = mesh.inst_range[pos(&inst.id)];
            for p in &mut mesh.verts2d[lo..hi] {
                *p = Vec2::new(p.x * sx, p.y * sy);
            }
        }
    }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rect(id: &str, w: f64, h: f64) -> Value {
        json!({
            "id": id, "name": id,
            "vertices": {"a": [0, 0], "b": [w, 0], "c": [w, h], "d": [0, h]},
            "edges": [
                {"id": "bottom", "from": "a", "to": "b"},
                {"id": "right", "from": "b", "to": "c", "curve": {"type": "quad", "c": [[0.5, 0.12]]}},
                {"id": "top", "from": "c", "to": "d"},
                {"id": "left", "from": "d", "to": "a"}
            ],
            "notches": [], "cut": {"qty": 1}, "grain": [[10, 10], [10, 40]],
            "placement": {"part": "torso"}
        })
    }

    fn imported_sleeve() -> Value {
        let mut notes: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/import/synthetic-sleeve.annotations.json"
        ))
        .unwrap();
        for (panel, angle) in [("outer", 0.), ("under", 180.)] {
            notes["panels"][panel]["placement"] = json!({"part":"arm","side":"R",
                "y0_landmark":"shoulder_tip","y0":200.,"x0":0.,"theta_deg":angle,
                "radius_mm":100. / std::f64::consts::PI});
            notes["panels"][panel]["sim"] = json!(true);
        }
        let imported = ftx_pattern::garmentcode_import::import_garmentcode_json(
            include_str!("../../../tests/fixtures/import/synthetic-sleeve.garmentcode.json"),
            Some(&serde_json::to_string(&notes).unwrap()),
        )
        .unwrap();
        assert_eq!(imported.report["status"], "REVIEW_REQUIRED");
        imported.pattern
    }

    #[test]
    fn production_scope_retains_the_complete_imported_sleeve() {
        let pattern = Pattern::new(imported_sleeve());
        let scope = simulation_scope(&pattern).unwrap();
        assert_eq!(
            scope,
            json!({"basis":"complete_pattern",
            "simulated_instances":["p0000","p0001"],"excluded_instances":[],
            "simulated_seams":[{"index":0,"id":"s0000"},{"index":1,"id":"s0001"}],
            "excluded_seams":[]})
        );
        let mesh = build_garment(&pattern, 12.).unwrap();
        assert_eq!(mesh.instances, ["p0000", "p0001"]);
        assert_eq!(mesh.seams.len(), 2);
        assert!(mesh.seams.iter().all(|seam| !seam.pairs.is_empty()));
        assert_eq!(simulation_scope(&pattern).unwrap(), scope);
    }

    #[test]
    fn unresolved_scope_names_the_missing_panel_and_both_attachment_seams() {
        let mut raw = imported_sleeve();
        raw["pieces"][1]
            .as_object_mut()
            .unwrap()
            .remove("placement");
        let pattern = Pattern::new(raw);
        let refusal = simulation_scope(&pattern).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::InvalidDocument);
        for expected in ["p0001", "s0000", "s0001", "p0000/e0001", "p0001/e0002"] {
            assert!(refusal.message.contains(expected), "{expected}");
        }
        assert!(
            refusal
                .repairs
                .iter()
                .any(|repair| repair.contains("placement"))
        );
        assert!(
            refusal
                .repairs
                .iter()
                .any(|repair| repair.contains("sim:false"))
        );
        // The lower-level reference primitive still deliberately meshes subsets.
        let mesh = build_garment(&pattern, 12.).unwrap();
        assert_eq!(mesh.instances, ["p0000"]);
        assert!(mesh.seams.is_empty() && mesh.pairs.is_empty());
    }

    #[test]
    fn disconnected_or_unusable_placements_cannot_disappear() {
        for placement in [
            Value::Null,
            json!({}),
            json!({"x0":0.}),
            json!({"part":17}),
            json!({"part":""}),
            json!({"part":"  "}),
        ] {
            let mut raw = imported_sleeve();
            let mut pocket = rect("pocket", 50., 40.);
            pocket["placement"] = placement;
            raw["pieces"].as_array_mut().unwrap().push(pocket);
            let refusal = simulation_scope(&Pattern::new(raw)).unwrap_err();
            assert_eq!(refusal.code, RefusalCode::InvalidDocument);
        }
        let mut raw = imported_sleeve();
        let mut pocket = rect("pocket", 50., 40.);
        pocket.as_object_mut().unwrap().remove("placement");
        raw["pieces"].as_array_mut().unwrap().push(pocket);
        let refusal = simulation_scope(&Pattern::new(raw)).unwrap_err();
        assert!(refusal.message.contains("pocket"));
        assert!(refusal.message.contains("affected seams: none"));
    }

    #[test]
    fn deliberate_exclusions_retain_exact_instance_and_seam_scope() {
        let mut raw = imported_sleeve();
        raw["pieces"][1]["sim"] = json!(false);
        raw["pieces"][1]
            .as_object_mut()
            .unwrap()
            .remove("placement");
        let mut lining = rect("lining", 50., 40.);
        lining["sim"] = json!(false);
        lining["cut"]["qty"] = json!(2);
        raw["pieces"].as_array_mut().unwrap().push(lining);
        let pattern = Pattern::new(raw);
        assert_eq!(
            simulation_scope(&pattern).unwrap(),
            json!({
            "basis":"declared_subset","simulated_instances":["p0000"],
            "excluded_instances":[
                {"instance":"p0001","piece":"p0001","reason":"sim_false"},
                {"instance":"lining.R","piece":"lining","reason":"sim_false"},
                {"instance":"lining.L","piece":"lining","reason":"sim_false"}],
            "simulated_seams":[],
            "excluded_seams":[
                {"index":0,"id":"s0000","a":"p0000/e0001","b":"p0001/e0000","excluded_instances":["p0001"]},
                {"index":1,"id":"s0001","a":"p0000/e0003","b":"p0001/e0002","excluded_instances":["p0001"]}]})
        );
        let mesh = build_garment(&pattern, 12.).unwrap();
        assert_eq!(mesh.instances, ["p0000"]);
        assert!(mesh.seams.is_empty());
    }

    #[test]
    fn invalid_hidden_seams_and_empty_active_scope_refuse() {
        let mut raw = imported_sleeve();
        raw["pieces"][1]["sim"] = json!(false);
        for reference in [json!("p0001/missing"), json!("unknown/e0000"), Value::Null] {
            let mut broken = raw.clone();
            broken["seams"][0]["b"] = reference;
            let refusal = simulation_scope(&Pattern::new(broken)).unwrap_err();
            assert_eq!(refusal.code, RefusalCode::InvalidDocument);
            assert!(refusal.message.contains("seam s0000"));
        }
        raw["pieces"][0]["sim"] = json!(false);
        let refusal = simulation_scope(&Pattern::new(raw)).unwrap_err();
        assert!(refusal.message.contains("nothing to drape"));
        assert!(!refusal.repairs.is_empty());
    }

    /// A skirt top gathered 1.3 to 1 into a band. A seam that declares nothing has no relief field, so the
    /// scene keeps the reference's weights. A declared 100 mm band holds weight 0 and its own floor for
    /// 100 mm, is half way back 25 mm later and is plain cloth by the hem.
    #[test]
    fn a_gathered_seam_may_declare_its_band_of_pleats() {
        let pattern = |seam: Value| {
            Pattern::new(json!({
                "schema": "fabric-pattern/1", "units": "mm",
                "pieces": [rect("skirt", 260.0, 300.0), rect("band", 200.0, 60.0)],
                "seams": [seam]
            }))
        };
        let gathered = json!({"id": "g", "a": "skirt/top", "b": "band/bottom", "ratio": 1.3});
        let plain = build_garment(&pattern(gathered.clone()), 20.0).unwrap();
        assert_eq!(plain.seams[0].gather_relief, None);
        assert!(crate::start::gather_relief(&plain).is_none());

        let mut seam = gathered.clone();
        seam["gather_relief_mm"] = json!(100.0);
        seam["gather_compression"] = json!(0.002);
        let mesh = build_garment(&pattern(seam), 20.0).unwrap();
        assert_eq!(
            mesh.seams[0].gather_relief,
            Some(GatherRelief {
                relief_mm: 100.0,
                compression: 0.002
            })
        );
        // The start state's weights are the reference's whether or not a band is declared.
        assert_eq!(
            crate::start::gather_weights(&mesh),
            crate::start::gather_weights(&plain)
        );
        let (weights, floors) = crate::start::gather_relief(&mesh).unwrap();
        let (lo, hi) = mesh.inst_range[0];
        // Distance is to the nearest sample of the seam's long side (the skirt top), as in the reference.
        let seam_samples = &mesh.seams[0].side_a;
        let mut seen = [false; 3];
        for v in lo..hi {
            let d = seam_samples
                .iter()
                .map(|&j| {
                    let step = mesh.verts2d[v] - mesh.verts2d[j];
                    det::hypot(step.x, step.y)
                })
                .fold(f64::INFINITY, f64::min);
            let want = ((d - 100.0) / 50.0).clamp(0.0, 1.0);
            assert!((weights[v] - want).abs() < 1e-9, "weight at {d} mm");
            assert_eq!(
                floors[v],
                if d < 150.0 {
                    0.002
                } else {
                    crate::start::GATHER_COMPRESSION
                }
            );
            seen[0] |= d > 1.0 && d <= 100.0;
            seen[1] |= d > 100.0 && d < 150.0;
            seen[2] |= d >= 150.0;
        }
        assert_eq!(
            seen, [true; 3],
            "the mesh samples the band, the ramp and the plain cloth"
        );
        let (band_lo, band_hi) = mesh.inst_range[1];
        assert!(weights[band_lo..band_hi].iter().all(|&w| w == 1.0));

        for (key, bad) in [
            ("gather_relief_mm", json!(-1.0)),
            ("gather_compression", json!(0.0)),
            ("gather_compression", json!(1.5)),
        ] {
            let mut seam = gathered.clone();
            seam[key] = bad;
            let refusal = build_garment(&pattern(seam), 20.0).unwrap_err();
            assert_eq!(refusal.code, RefusalCode::OutOfRange, "{key}");
        }
        // A band on a seam the solver does not gather would do nothing, so it refuses and names the ratio.
        for ratio in [json!(1.1), Value::Null] {
            let mut seam = gathered.clone();
            seam["gather_relief_mm"] = json!(100.0);
            if ratio.is_null() {
                seam.as_object_mut().unwrap().remove("ratio");
            } else {
                seam["ratio"] = ratio;
            }
            let refusal = build_garment(&pattern(seam), 20.0).unwrap_err();
            assert_eq!(refusal.code, RefusalCode::OutOfRange);
            assert!(
                refusal.message.contains("does not gather"),
                "{}",
                refusal.message
            );
        }
        let mut seam = gathered.clone();
        seam["gather_relief_mm"] = json!("deep");
        let refusal = build_garment(&pattern(seam), 20.0).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::InvalidDocument);
    }

    /// Two rectangles sewn right edge (curved) to left edge: matched samples, CCW triangles covering
    /// both pieces, and a pinned digest so native and wasm agree.
    #[test]
    fn two_sewn_rectangles() {
        let doc = json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [rect("A", 100.0, 50.0), rect("B", 80.0, 50.0)],
            "seams": [{"id": "s1", "a": "A/right", "b": "B/left"}]
        });
        let g = build_garment(&Pattern::new(doc), 12.0).unwrap();
        assert_eq!(g.instances, ["A", "B"]);
        assert_eq!(g.seams.len(), 1);
        let s = &g.seams[0];
        assert_eq!(s.side_a.len(), s.side_b.len());
        assert_eq!(s.pairs.len(), s.side_a.len());
        for &(a, b) in &s.pairs {
            assert_eq!(g.owner[a], 0);
            assert_eq!(g.owner[b], 1);
        }
        let area = |f: &[usize; 3]| {
            let (a, b, c) = (g.verts2d[f[0]], g.verts2d[f[1]], g.verts2d[f[2]]);
            0.5 * ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x))
        };
        assert!(g.faces.iter().all(|f| area(f) > 0.0));
        let bytes: Vec<u8> = g
            .verts2d
            .iter()
            .flat_map(|p| [p.x, p.y])
            .flat_map(f64::to_le_bytes)
            .chain(
                g.faces
                    .iter()
                    .flatten()
                    .flat_map(|&i| (i as u64).to_le_bytes()),
            )
            .collect();
        let digest = ftx_core::receipt::sha256_hex(&bytes);
        println!(
            "{} vertices, {} triangles, digest {digest}",
            g.verts2d.len(),
            g.faces.len()
        );
        assert_eq!(
            digest,
            "180a58857b4049576200c36884c5420c96e3b3830fb7461f511fda5b368e78cf"
        );
    }

    #[test]
    fn sim_scale_must_be_two_finite_positive_numbers() {
        for (bad_scale, expected) in [
            (json!([0.0, 1.0]), RefusalCode::OutOfRange),
            (json!([-1.0, 1.0]), RefusalCode::OutOfRange),
            (json!([1.0, -0.5]), RefusalCode::OutOfRange),
            (json!([1.0, 1.0, 2.0]), RefusalCode::InvalidDocument),
            (json!(["x", 1.0]), RefusalCode::InvalidDocument),
            (json!(true), RefusalCode::InvalidDocument),
        ] {
            let mut piece = rect("A", 100.0, 50.0);
            piece["placement"] = json!({"part": "torso"});
            piece["sim_scale"] = bad_scale.clone();
            let doc = json!({
                "schema": "fabric-pattern/1", "units": "mm",
                "pieces": [piece], "seams": []
            });
            let err = build_garment(&Pattern::new(doc), 12.0).unwrap_err();
            assert_eq!(err.code, expected, "sim_scale={bad_scale}: {err:?}");
        }
        let mut ok_piece = rect("A", 100.0, 50.0);
        ok_piece["placement"] = json!({"part": "torso"});
        ok_piece["sim_scale"] = json!([0.8, 0.5]);
        let g = build_garment(
            &Pattern::new(json!({
                "schema": "fabric-pattern/1", "units": "mm",
                "pieces": [ok_piece], "seams": []
            })),
            12.0,
        )
        .unwrap();
        assert!(!g.faces.is_empty());
        assert!(g.faces.iter().all(|f| {
            let (a, b, c) = (g.verts2d[f[0]], g.verts2d[f[1]], g.verts2d[f[2]]);
            (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x) > 0.0
        }));
    }

    #[test]
    fn assign_path_and_path_indices_handle_empty_and_non_monotonic_cuts_safely() {
        let doc = json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [rect("A", 100.0, 50.0)], "seams": []
        });
        let pat = Pattern::new(doc);
        let inst = pat.instance("A").unwrap();
        let mut edge_pts = BTreeMap::new();
        let err = assign_path(inst, &["bottom".into()], &[], &mut edge_pts).unwrap_err();
        assert_eq!(err.code, RefusalCode::DegenerateGeometry);

        // Non-monotonic junction cuts (second junction matches index 0 while first matches index 2).
        let reversed_pts = vec![
            Vec2::new(100.0, 50.0),
            Vec2::new(100.0, 25.0),
            Vec2::new(100.0, 0.0),
            Vec2::new(0.0, 0.0),
        ];
        let err = assign_path(
            inst,
            &["bottom".into(), "right".into(), "top".into()],
            &reversed_pts,
            &mut edge_pts,
        )
        .unwrap_err();
        assert_eq!(err.code, RefusalCode::DegenerateGeometry);

        // Defensive path_indices guard never panics on an empty second edge entry.
        let mut idx = BTreeMap::new();
        idx.insert(("A".into(), "bottom".into()), vec![0, 1]);
        idx.insert(("A".into(), "right".into()), Vec::new());
        assert_eq!(
            path_indices(&idx, "A", &["bottom".into(), "right".into()]),
            vec![0, 1]
        );
    }

    fn mesh_digest(g: &GarmentMesh) -> String {
        let bytes: Vec<u8> = g
            .verts2d
            .iter()
            .flat_map(|p| [p.x, p.y])
            .flat_map(f64::to_le_bytes)
            .chain(
                g.faces
                    .iter()
                    .flatten()
                    .flat_map(|&i| (i as u64).to_le_bytes()),
            )
            .chain(
                g.pairs
                    .iter()
                    .flat_map(|&(a, b)| [(a as u64).to_le_bytes(), (b as u64).to_le_bytes()])
                    .flatten(),
            )
            .collect();
        ftx_core::receipt::sha256_hex(&bytes)
    }

    fn armhole_bodice_pattern() -> Pattern {
        // Synthetic bodice panel with a tight concave armscye curve and curved neckline — the
        // geometry class where uniform 12 mm hex placement creates obtuse boundary slivers.
        let bodice = json!({
            "id": "bodice", "name": "bodice",
            "vertices": {
                "hem_cf": [0.0, 0.0],
                "hem_side": [140.0, 0.0],
                "arm_low": [140.0, 125.0],
                "shoulder_tip": [105.0, 220.0],
                "neck_side": [52.0, 232.0],
                "neck_cf": [0.0, 185.0]
            },
            "edges": [
                {"id": "hem", "from": "hem_cf", "to": "hem_side"},
                {"id": "side", "from": "hem_side", "to": "arm_low"},
                {"id": "armscye", "from": "arm_low", "to": "shoulder_tip",
                 "curve": {"type": "cubic", "c": [[0.20, 0.52], [0.68, 0.44]]}},
                {"id": "shoulder", "from": "shoulder_tip", "to": "neck_side"},
                {"id": "neckline", "from": "neck_side", "to": "neck_cf",
                 "curve": {"type": "quad", "c": [[0.45, 0.36]]}},
                {"id": "cf", "from": "neck_cf", "to": "hem_cf"}
            ],
            "notches": [], "cut": {"qty": 1}, "grain": [[20, 20], [20, 120]],
            "placement": {"part": "torso"}
        });
        Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [bodice], "seams": []
        }))
    }

    #[test]
    fn mesh_options_default_is_bit_identical_and_invalid_options_refuse() {
        let doc = json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [rect("A", 100.0, 50.0), rect("B", 80.0, 50.0)],
            "seams": [{"id": "s1", "a": "A/right", "b": "B/left"}]
        });
        let pat = Pattern::new(doc);
        let legacy = build_garment(&pat, 12.0).unwrap();
        let opt_default = build_garment_with_options(&pat, 12.0, MeshOptions::default()).unwrap();
        assert_eq!(mesh_digest(&legacy), mesh_digest(&opt_default));
        assert!(MeshOptions::default().is_default());
        assert_eq!(
            serde_json::to_value(MeshOptions::default()).unwrap(),
            json!({})
        );

        // Adaptive preset round-trips cleanly through JSON.
        let adaptive = MeshOptions::adaptive(6.0);
        assert!(!adaptive.is_default());
        let wire = serde_json::to_value(adaptive).unwrap();
        assert_eq!(wire, json!({"min_spacing_mm": 6.0, "cvt_iterations": 6}));
        assert_eq!(
            serde_json::from_value::<MeshOptions>(wire).unwrap(),
            adaptive
        );

        // Unknown fields refuse.
        assert!(serde_json::from_value::<MeshOptions>(json!({"bogus": true})).is_err());

        // Fail-closed validation checks.
        for (bad, code) in [
            (
                MeshOptions {
                    min_spacing_mm: Some(15.0),
                    ..MeshOptions::default()
                },
                RefusalCode::OutOfRange,
            ),
            (
                MeshOptions {
                    min_spacing_mm: Some(0.0),
                    ..MeshOptions::default()
                },
                RefusalCode::OutOfRange,
            ),
            (
                MeshOptions {
                    min_spacing_mm: Some(f64::NAN),
                    ..MeshOptions::default()
                },
                RefusalCode::NonFinite,
            ),
            (
                MeshOptions {
                    min_spacing_mm: Some(1.0),
                    ..MeshOptions::default()
                },
                RefusalCode::BudgetExceeded,
            ),
            (
                MeshOptions {
                    cvt_iterations: MAX_CVT_ITERATIONS + 1,
                    ..MeshOptions::default()
                },
                RefusalCode::BudgetExceeded,
            ),
            (
                MeshOptions {
                    max_turning_angle_rad: 0.15,
                    ..MeshOptions::default()
                },
                RefusalCode::OutOfRange,
            ),
            (
                MeshOptions {
                    gather_refinement: 2.5,
                    ..MeshOptions::default()
                },
                RefusalCode::OutOfRange,
            ),
            (
                MeshOptions {
                    min_spacing_mm: Some(6.0),
                    max_turning_angle_rad: 0.01,
                    ..MeshOptions::default()
                },
                RefusalCode::OutOfRange,
            ),
            (
                MeshOptions {
                    min_spacing_mm: Some(6.0),
                    gather_refinement: 0.5,
                    ..MeshOptions::default()
                },
                RefusalCode::OutOfRange,
            ),
        ] {
            let err = build_garment_with_options(&pat, 12.0, bad).unwrap_err();
            assert_eq!(err.code, code, "options={bad:?} err={err:?}");
        }
    }

    #[test]
    fn adaptive_curvature_and_cvt_improve_minimum_angle_and_reduce_slivers_deterministically() {
        let pat = armhole_bodice_pattern();
        let uniform = build_garment(&pat, 12.0).unwrap();
        let cvt_only = build_garment_with_options(
            &pat,
            12.0,
            MeshOptions {
                cvt_iterations: 8,
                ..MeshOptions::default()
            },
        )
        .unwrap();
        let adaptive = build_garment_with_options(&pat, 12.0, MeshOptions::adaptive(5.0)).unwrap();
        let adaptive_repeat =
            build_garment_with_options(&pat, 12.0, MeshOptions::adaptive(5.0)).unwrap();

        // Exact determinism across repeated runs.
        assert_eq!(mesh_digest(&adaptive), mesh_digest(&adaptive_repeat));

        let q_uniform = mesh_quality(&uniform);
        let q_cvt = mesh_quality(&cvt_only);
        let q_adaptive = mesh_quality(&adaptive);

        // Every triangle stays strictly CCW and total area is conserved within polygonal curve
        // chord refinement tolerance.
        let total_area = |g: &GarmentMesh| -> f64 {
            g.faces
                .iter()
                .map(|&[i, j, k]| {
                    let a = tri_area(g.verts2d[i], g.verts2d[j], g.verts2d[k]);
                    assert!(a > 0.0, "non-CCW or degenerate triangle [{i},{j},{k}]");
                    a
                })
                .sum()
        };
        let a_uni = total_area(&uniform);
        let a_cvt = total_area(&cvt_only);
        let a_ada = total_area(&adaptive);
        assert!(
            (a_uni - a_cvt).abs() < 1e-9,
            "CVT-only preserves exact boundary polygon area: {a_uni} vs {a_cvt}"
        );
        assert!(
            (a_uni - a_ada).abs() / a_uni < 0.01,
            "adaptive boundary refinement preserves piece area within 1%: {a_uni} vs {a_ada}"
        );

        // Curvature refinement adds samples on the curved armscye/neckline, not the straight hem.
        assert!(
            adaptive.index[&("bodice".into(), "armscye".into())].len()
                > uniform.index[&("bodice".into(), "armscye".into())].len(),
            "curved armscye must be refined"
        );
        assert_eq!(
            adaptive.index[&("bodice".into(), "hem".into())].len(),
            uniform.index[&("bodice".into(), "hem".into())].len(),
            "straight ungathered hem keeps nominal spacing"
        );

        // Both CVT-only and adaptive+CVT must materially improve triangle quality (minimum angle,
        // 5th-percentile angle, and worst obtuse angle) over the uniform hex mesh.
        assert!(
            q_cvt.min_angle_deg > q_uniform.min_angle_deg + 3.0,
            "CVT smoothing must raise min angle: uniform={q_uniform:?} cvt={q_cvt:?}"
        );
        assert!(
            q_adaptive.min_angle_deg > q_uniform.min_angle_deg + 8.0
                && q_adaptive.p05_angle_deg > q_uniform.p05_angle_deg + 2.0
                && q_adaptive.max_angle_deg < q_uniform.max_angle_deg - 10.0,
            "adaptive+CVT must improve min/p05/max angles: uniform={q_uniform:?} adaptive={q_adaptive:?}"
        );
        assert!(
            q_adaptive.sliver_triangles <= q_uniform.sliver_triangles,
            "adaptive+CVT must not increase sliver triangles: uniform={q_uniform:?} adaptive={q_adaptive:?}"
        );
    }

    #[test]
    fn adaptive_meshing_refines_gathered_seams_with_matched_pairs_and_graded_interior() {
        let pat = Pattern::new(json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [rect("skirt", 260.0, 220.0), rect("band", 200.0, 50.0)],
            "seams": [{"id": "waist_gather", "a": "skirt/top", "b": "band/bottom", "ratio": 1.3}]
        }));
        let uniform = build_garment(&pat, 12.0).unwrap();
        let adaptive = build_garment_with_options(
            &pat,
            12.0,
            MeshOptions {
                min_spacing_mm: Some(6.0),
                cvt_iterations: 6,
                gather_refinement: 2.0,
                ..MeshOptions::default()
            },
        )
        .unwrap();

        // Seam matching invariant stays exact and gathers get ~2x finer boundary resolution.
        let su = &uniform.seams[0];
        let sa = &adaptive.seams[0];
        assert_eq!(sa.side_a.len(), sa.side_b.len());
        assert_eq!(sa.pairs.len(), sa.side_a.len());
        assert!(
            sa.side_a.len() >= su.side_a.len() + (su.side_a.len() / 2),
            "gathered seam should be refined: uniform={} adaptive={}",
            su.side_a.len(),
            sa.side_a.len()
        );
        // Straight ungathered hem of the skirt keeps nominal 12 mm spacing.
        assert_eq!(
            adaptive.index[&("skirt".into(), "bottom".into())].len(),
            uniform.index[&("skirt".into(), "bottom".into())].len()
        );

        // Graded interior: mean triangle area near the gathered top (y > 190 mm) is significantly
        // smaller than near the ungathered bottom hem (y < 40 mm).
        let (lo, hi) = adaptive.inst_range[0];
        let mut near_gather = Vec::new();
        let mut near_hem = Vec::new();
        for &[i, j, k] in &adaptive.faces {
            if !(lo..hi).contains(&i) {
                continue;
            }
            let (a, b, c) = (
                adaptive.verts2d[i],
                adaptive.verts2d[j],
                adaptive.verts2d[k],
            );
            let area = tri_area(a, b, c);
            assert!(area > 0.0);
            let cy = (a.y + b.y + c.y) / 3.0;
            if cy > 190.0 {
                near_gather.push(area);
            } else if cy < 40.0 {
                near_hem.push(area);
            }
        }
        assert!(!near_gather.is_empty() && !near_hem.is_empty());
        let mean = |xs: &[f64]| xs.iter().sum::<f64>() / xs.len() as f64;
        assert!(
            mean(&near_gather) < 0.6 * mean(&near_hem),
            "interior near gather seam must be finer than near hem: gather={} hem={}",
            mean(&near_gather),
            mean(&near_hem)
        );
        // Mesh quality remains strong with zero sliver triangles (< 20 deg) and high p05 angle.
        let q = mesh_quality(&adaptive);
        assert!(
            q.sliver_triangles == 0 && q.min_angle_deg >= 20.0 && q.p05_angle_deg >= 35.0,
            "gathered adaptive+CVT mesh should keep healthy angles with zero slivers: {q:?}"
        );
    }
}
