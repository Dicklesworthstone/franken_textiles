//! Piecewise-isometric, 180-degree flat folds in an explicitly placed operation-local assembly.
//! A region is the intersection of the real source cut polygon and a convex source-space mask.
//! This representation retains disconnected concave fragments without inventing connecting cloth.
//! Folds reverse the moving stack order. No finite-radius fold, compression, collision-free folding
//! trajectory, presser-foot access or machine calibration is inferred from this geometric model.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{
    Refusal, RefusalCode, Vec2,
    polyline::{point_in_polygon, signed_area},
};
use ftx_pattern::{Pattern, SeamPairs, material_catalog::Selection};
use serde::{Deserialize, Serialize};

use super::{bad, positive, sheet};
use crate::{
    Ctx,
    outline::{cut_outline, ring_is_simple, segments_touch},
};

const EPS: f64 = 1e-8;
const MAX_REGIONS: usize = 4096;
const MAX_WORK: usize = 50_000_000;
const MAX_LAYER_STATIONS: usize = 250_000;

/// One material instance placed in a shared, uncalibrated millimetre frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedPiece {
    /// Exact manufacturing instance ID, not the source piece name of a mirror pair.
    pub instance: String,
    /// Location of the instance's original origin.
    pub origin_mm: Vec2,
    /// Unit direction of the original +x axis; no scaling or implicit normalization.
    pub x_axis: Vec2,
    /// Turn the public face down by reflecting the local y axis before placement.
    #[serde(default)]
    pub face_down: bool,
}

/// Half-plane relative to the directed fold line in current assembly coordinates.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovingSide {
    /// Positive cross product with the line's direction.
    Left,
    /// Negative cross product with the line's direction.
    Right,
}

/// Placement of the reflected moving stack relative to stationary cloth.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoldStack {
    /// Moving stack lands above the stationary stack, in reversed layer order.
    Over,
    /// Moving stack lands below the stationary stack, in reversed layer order.
    Under,
}

/// A fully flattened straight crease, applied to every currently placed layer on one side.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlatFold {
    /// Distinct endpoints of an infinite fold line in current assembly millimetres.
    pub line_mm: [Vec2; 2],
    /// Which half-plane moves; unaffected regions keep their existing transforms.
    pub moving_side: MovingSide,
    /// Above/below order after the 180-degree fold, not an accessibility assertion.
    pub stack: FoldStack,
}

/// Explicit operation-local assembly. Supply instead of legacy `plies` on a sewing pass.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlatAssembly {
    /// All pieces participating in this declared assembly, initially bottom to top.
    /// Other pattern instances are outside this explicitly scoped calculation.
    pub pieces: Vec<PlacedPiece>,
    /// Ordered flat folds; later crease coordinates refer to the already folded assembly.
    pub folds: Vec<FlatFold>,
    /// Largest admitted distance between paired side targets after their respective folds (mm).
    /// Side A drives the needle; the discrepancy is reported, not averaged or fitted away.
    pub registration_tolerance_mm: f64,
    /// Required needle/chord distance from each intersected ply's cut edge and crease (mm).
    pub minimum_boundary_clearance_mm: f64,
}

/// Source-to-assembly isometry; z is assigned separately from source-qualified layer thickness.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct LayerFrame {
    /// Original origin in assembly millimetres.
    pub origin_mm: Vec2,
    /// Mapped original +x axis.
    pub x_axis: Vec2,
    /// Mapped original +y axis, including face flips.
    pub y_axis: Vec2,
}

impl LayerFrame {
    fn map(self, p: Vec2) -> Vec2 {
        self.origin_mm + self.x_axis * p.x + self.y_axis * p.y
    }
    fn inverse(self, p: Vec2) -> Vec2 {
        let p = p - self.origin_mm;
        Vec2::new(p.dot(self.x_axis), p.dot(self.y_axis))
    }
    fn reflect(self, origin: Vec2, normal: Vec2) -> Self {
        let vector = |v: Vec2| v - normal * (2.0 * v.dot(normal));
        Self {
            origin_mm: origin + vector(self.origin_mm - origin),
            x_axis: vector(self.x_axis),
            y_axis: vector(self.y_axis),
        }
    }
}

/// One actual material preimage of the shared needle line. A folded instance can occur repeatedly.
#[derive(Clone, Debug, Serialize)]
pub struct NeedleLayer {
    /// Exact source instance.
    pub instance: String,
    /// Deterministic final region identity, unique even for repeated plies of one instance.
    pub region: usize,
    /// Frozen selection key in the enclosing plan's material map.
    pub material_sha256: String,
    /// Whether this region's public face points toward increasing assembly z.
    pub public_face_up: bool,
    /// Source-qualified uncompressed thickness, taking a reported upper bound when supplied.
    pub thickness_mm: f64,
    /// Whether thickness used the source's reported upper bound; no new statistical meaning.
    pub uses_thickness_upper_bound: bool,
    /// Bottom/top of the uncompressed local stack; its bottom is z=0, not a calibrated bed height.
    pub z_bounds_mm: [f64; 2],
    /// Actual needle preimages in this layer's original source coordinates, by canonical station.
    pub source_stations_mm: Vec<Vec2>,
    /// Source-to-assembly transform for this region.
    pub frame: LayerFrame,
    /// Convex source mask, intersected with the ORIGINAL cut polygon, never a replacement for it.
    pub source_mask_mm: Vec<Vec2>,
}

/// Geometric needle/material correspondence, not a physically validated folded garment.
#[derive(Clone, Debug, Serialize)]
pub struct AssemblyReceipt {
    /// Explicit numerical model identifier.
    pub model: &'static str,
    /// Frame scope; these coordinates are not a robot or needle-controller registration.
    pub frame: &'static str,
    /// Assembly needle positions by canonical station, driven by side A.
    pub needle_mm: Vec<Vec2>,
    /// Material preimages in bottom-to-top order; constant throughout this operation.
    pub layers: Vec<NeedleLayer>,
    /// Indices of the intended [A, B] seam layers within `layers`; they must be distinct.
    pub side_layers: [usize; 2],
    /// Maximum paired-target separation before any numerical acceptance tolerance (mm).
    pub maximum_registration_error_mm: f64,
    /// Smallest clearance over all intersected source layers and complete stitch chords (mm).
    pub minimum_boundary_clearance_mm: f64,
    /// Total derived uncompressed thickness, not nominal ply counts or presser-foot compression.
    pub stack_mm: f64,
}

struct Sheet {
    instance: String,
    cut: Vec<Vec2>,
    sha: String,
    thickness: f64,
    upper: bool,
}

#[derive(Clone)]
struct Region {
    sheet: usize,
    mask: Vec<Vec2>,
    frame: LayerFrame,
}

fn charge(work: &mut usize, n: usize) -> Result<(), Refusal> {
    *work = work.saturating_add(n);
    if *work > MAX_WORK {
        Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "flat-fold geometry budget exceeded",
        ))
    } else {
        Ok(())
    }
}

fn bounded(p: Vec2) -> bool {
    p.is_finite() && p.x.abs().max(p.y.abs()) <= 1_000_000.0
}

fn point_distance(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    if d.norm2() == 0.0 {
        return p.dist(a);
    }
    p.dist(a + d * ((p - a).dot(d) / d.norm2()).clamp(0.0, 1.0))
}

fn segment_distance(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> f64 {
    if segments_touch(a, b, c, d) {
        0.0
    } else {
        point_distance(a, c, d)
            .min(point_distance(b, c, d))
            .min(point_distance(c, a, b))
            .min(point_distance(d, a, b))
    }
}

fn edges(ring: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    ring.iter()
        .copied()
        .zip(ring.iter().copied().cycle().skip(1))
        .take(ring.len())
}

fn contains(p: Vec2, ring: &[Vec2]) -> bool {
    point_in_polygon(p, ring) || edges(ring).any(|(a, b)| point_distance(p, a, b) <= EPS)
}

fn clip(mask: &[Vec2], n: Vec2, c: f64) -> Vec<Vec2> {
    let mut out = Vec::new();
    for (a, b) in edges(mask) {
        let da = n.dot(a) + c;
        let db = n.dot(b) + c;
        if da >= 0.0 {
            out.push(a);
        }
        if (da < 0.0 && db > 0.0) || (da > 0.0 && db < 0.0) {
            out.push(a + (b - a) * (da / (da - db)));
        }
    }
    out.dedup();
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

// A segment may pass through a disconnected concave fragment with neither endpoint in that ply.
// Clip it to the convex mask, then test the remaining segment against the actual source polygon.
fn intersects_region(a: Vec2, b: Vec2, mask: &[Vec2], cut: &[Vec2]) -> bool {
    let mut lo: f64 = 0.0;
    let mut hi: f64 = 1.0;
    for (u, v) in edges(mask) {
        let n = (v - u).perp();
        let da = n.dot(a - u);
        let db = n.dot(b - u);
        if da < 0.0 && db < 0.0 {
            return false;
        }
        if da < 0.0 {
            lo = lo.max(da / (da - db));
        } else if db < 0.0 {
            hi = hi.min(da / (da - db));
        }
        if lo > hi {
            return false;
        }
    }
    let (a, b) = (a + (b - a) * lo, a + (b - a) * hi);
    contains(a, cut) || contains(b, cut) || edges(cut).any(|(u, v)| segments_touch(a, b, u, v))
}

impl FlatAssembly {
    pub(super) fn compile(
        &self,
        pattern: &Pattern,
        ctx: &Ctx<'_>,
        sides: [&str; 2],
        stations: &SeamPairs,
        materials: &mut BTreeMap<String, Selection>,
        work: &mut usize,
    ) -> Result<AssemblyReceipt, Refusal> {
        if self.pieces.is_empty() || self.pieces.len() > 128 || self.folds.len() > 16 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "flat-fold assembly needs 1..128 pieces and at most 16 folds",
            ));
        }
        if !self.registration_tolerance_mm.is_finite()
            || self.registration_tolerance_mm < 0.0
            || self.registration_tolerance_mm > 1000.0
            || !positive(self.minimum_boundary_clearance_mm)
            || self.minimum_boundary_clearance_mm <= EPS
        {
            return Err(bad(
                "invalid flat-fold registration or boundary-clearance policy",
            ));
        }
        let (sheets, mut regions) = self.place(pattern, ctx, materials, work)?;
        for fold in &self.folds {
            let [a, b] = fold.line_mm;
            if !bounded(a) || !bounded(b) || a.dist(b) <= EPS {
                return Err(bad("flat fold needs two distinct bounded line points"));
            }
            let mut normal = (b - a).perp() / a.dist(b);
            if matches!(fold.moving_side, MovingSide::Right) {
                normal = -normal;
            }
            let mut stationary = Vec::new();
            let mut moving = Vec::new();
            for region in regions {
                charge(work, region.mask.len())?;
                // Pull the current assembly half-plane back to original source material coordinates.
                let n = Vec2::new(
                    normal.dot(region.frame.x_axis),
                    normal.dot(region.frame.y_axis),
                );
                let c = normal.dot(region.frame.origin_mm - a);
                for (sign, target) in [(-1.0, &mut stationary), (1.0, &mut moving)] {
                    let mask = clip(&region.mask, n * sign, c * sign);
                    if mask.len() < 3 || signed_area(&mask) == 0.0 {
                        continue;
                    }
                    if signed_area(&mask) < 0.0 || mask.iter().any(|p| !bounded(*p)) {
                        return Err(bad("invalid clipped flat-fold region"));
                    }
                    target.push(Region {
                        sheet: region.sheet,
                        mask,
                        frame: if sign > 0.0 {
                            region.frame.reflect(a, normal)
                        } else {
                            region.frame
                        },
                    });
                }
                if stationary.len() + moving.len() > MAX_REGIONS {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "too many flat-fold regions",
                    ));
                }
            }
            moving.reverse();
            regions = match fold.stack {
                FoldStack::Over => {
                    stationary.extend(moving);
                    stationary
                }
                FoldStack::Under => {
                    moving.extend(stationary);
                    moving
                }
            };
        }
        self.correspondence(pattern, sides, stations, &sheets, &regions, work)
    }

    fn place(
        &self,
        pattern: &Pattern,
        ctx: &Ctx<'_>,
        materials: &mut BTreeMap<String, Selection>,
        work: &mut usize,
    ) -> Result<(Vec<Sheet>, Vec<Region>), Refusal> {
        let mut sheets = Vec::new();
        let mut regions = Vec::new();
        let mut seen = BTreeSet::new();
        for p in &self.pieces {
            if !seen.insert(&p.instance)
                || !bounded(p.origin_mm)
                || !p.x_axis.is_finite()
                || (p.x_axis.norm2() - 1.0).abs() > 1e-12
            {
                return Err(bad(
                    "duplicate assembly instance or invalid rigid placement (unit x_axis required)",
                ));
            }
            let instance = pattern
                .instances
                .iter()
                .find(|i| i.id == p.instance)
                .ok_or_else(|| bad(format!("unknown assembly instance {}", p.instance)))?;
            let piece =
                &pattern.doc.as_ref().expect("validated pattern").pieces[instance.piece_index];
            if instance.edges.len() > 128
                || instance
                    .edges
                    .iter()
                    .any(|e| e.curve.as_ref().is_some_and(|c| c.controls().len() > 800))
            {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "assembly outline budget exceeded",
                ));
            }
            if piece
                .internal
                .as_ref()
                .is_some_and(|m| m.iter().any(|m| m.kind.as_deref() == Some("cut")))
                || piece.drills.as_ref().is_some_and(|d| !d.is_empty())
            {
                return Err(bad(
                    "flat-fold assembly needs explicit internal cut/drill obstacles",
                ));
            }
            // Bound outline construction BEFORE its sampled offset/union work, not only its output.
            let estimate: usize = instance
                .edges
                .iter()
                .map(|e| if e.curve.is_some() { 802 } else { 2 })
                .sum();
            charge(work, estimate.saturating_mul(estimate))?;
            if estimate > 8192 {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "assembly outline sample budget exceeded",
                ));
            }
            let (cut, _) = cut_outline(instance, piece, None, ctx.sa, ctx.opts.mitre_limit)?;
            charge(work, cut.len().saturating_mul(cut.len()))?;
            if cut.len() < 3
                || cut.len() > 8192
                || cut.iter().any(|q| !bounded(*q))
                || !ring_is_simple(&cut)
            {
                return Err(bad(
                    "flat-fold assembly requires a bounded simple cut polygon",
                ));
            }
            let (selected, thickness, upper) = sheet(pattern, instance.piece_index)?;
            let sha = selected.sha256().to_string();
            materials
                .entry(sha.clone())
                .or_insert_with(|| selected.frozen_selection());
            let mut lo = cut[0];
            let mut hi = lo;
            for q in &cut {
                lo.x = lo.x.min(q.x);
                lo.y = lo.y.min(q.y);
                hi.x = hi.x.max(q.x);
                hi.y = hi.y.max(q.y);
            }
            regions.push(Region {
                sheet: sheets.len(),
                mask: vec![lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)],
                frame: LayerFrame {
                    origin_mm: p.origin_mm,
                    x_axis: p.x_axis,
                    y_axis: p.x_axis.perp() * if p.face_down { -1.0 } else { 1.0 },
                },
            });
            sheets.push(Sheet {
                instance: p.instance.clone(),
                cut,
                sha,
                thickness,
                upper,
            });
        }
        Ok((sheets, regions))
    }

    fn correspondence(
        &self,
        pattern: &Pattern,
        sides: [&str; 2],
        stations: &SeamPairs,
        sheets: &[Sheet],
        regions: &[Region],
        work: &mut usize,
    ) -> Result<AssemblyReceipt, Refusal> {
        let a_instance = pattern
            .side(sides[0])
            .map_err(|e| bad(e.to_string()))?
            .0
            .id
            .as_str();
        let b_instance = pattern
            .side(sides[1])
            .map_err(|e| bad(e.to_string()))?
            .0
            .id
            .as_str();
        let inputs = [&stations.a, &stations.b];
        let source_region = |instance: &str, p: Vec2, work: &mut usize| -> Result<usize, Refusal> {
            let mut found = None;
            for (index, r) in regions.iter().enumerate() {
                let s = &sheets[r.sheet];
                if s.instance != instance {
                    continue;
                }
                charge(work, r.mask.len() + s.cut.len())?;
                if contains(p, &r.mask) && contains(p, &s.cut) {
                    if found.is_some() {
                        return Err(bad("sewing target lies on an ambiguous fold crease"));
                    }
                    found = Some(index);
                }
            }
            found.ok_or_else(|| {
                bad(format!(
                    "sewing target missing from declared assembly: {instance}"
                ))
            })
        };
        let anchor_regions = [
            source_region(a_instance, inputs[0][0], work)?,
            source_region(b_instance, inputs[1][0], work)?,
        ];
        if anchor_regions[0] == anchor_regions[1] {
            return Err(bad(
                "assembly seam sides must identify two distinct material plies",
            ));
        }
        let mut needle = Vec::with_capacity(inputs[0].len());
        let mut error: f64 = 0.0;
        for k in 0..inputs[0].len() {
            for (side, instance) in [a_instance, b_instance].iter().enumerate() {
                if source_region(instance, inputs[side][k], work)? != anchor_regions[side] {
                    return Err(bad(
                        "sewing path changes fold region; split into constant-layer operations",
                    ));
                }
            }
            let a = regions[anchor_regions[0]].frame.map(inputs[0][k]);
            let b = regions[anchor_regions[1]].frame.map(inputs[1][k]);
            if !bounded(a) || !bounded(b) {
                return Err(bad("unbounded assembled sewing coordinate"));
            }
            error = error.max(a.dist(b));
            needle.push(a);
        }
        if error > self.registration_tolerance_mm {
            return Err(bad(format!(
                "assembled sewing targets do not coincide: {error} mm registration error"
            )));
        }
        let mut layers = Vec::new();
        let mut stack: f64 = 0.0;
        let mut minimum: f64 = f64::INFINITY;
        for (index, r) in regions.iter().enumerate() {
            let s = &sheets[r.sheet];
            charge(
                work,
                needle.len().saturating_mul(r.mask.len() + s.cut.len()),
            )?;
            let first = r.frame.inverse(needle[0]);
            let present = contains(first, &r.mask) && contains(first, &s.cut);
            let mut source = Vec::new();
            if present {
                if layers.len().saturating_add(1).saturating_mul(needle.len()) > MAX_LAYER_STATIONS
                {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "too many material needle preimages",
                    ));
                }
                source.push(first);
            }
            for w in needle.windows(2) {
                let (a, b) = (r.frame.inverse(w[0]), r.frame.inverse(w[1]));
                if present {
                    if !contains(b, &r.mask) || !contains(b, &s.cut) {
                        return Err(bad(
                            "needle path leaves a material ply; split the sewing operation",
                        ));
                    }
                    for ring in [&r.mask, &s.cut] {
                        for (u, v) in edges(ring) {
                            minimum = minimum.min(segment_distance(a, b, u, v));
                        }
                    }
                    source.push(b);
                } else if intersects_region(a, b, &r.mask, &s.cut) {
                    return Err(bad(
                        "needle chord encounters an extra material ply between stations",
                    ));
                }
            }
            if present {
                let z0 = stack;
                stack += s.thickness;
                layers.push(NeedleLayer {
                    instance: s.instance.clone(),
                    region: index,
                    material_sha256: s.sha.clone(),
                    public_face_up: r.frame.x_axis.cross(r.frame.y_axis) > 0.0,
                    thickness_mm: s.thickness,
                    uses_thickness_upper_bound: s.upper,
                    z_bounds_mm: [z0, stack],
                    source_stations_mm: source,
                    frame: r.frame,
                    source_mask_mm: r.mask.clone(),
                });
            }
        }
        if !positive(stack)
            || !minimum.is_finite()
            || minimum + EPS < self.minimum_boundary_clearance_mm
        {
            return Err(bad(
                "assembled needle path lacks cut-edge or fold-crease clearance",
            ));
        }
        let mut side_layers = [0; 2];
        for side in 0..2 {
            side_layers[side] = layers
                .iter()
                .position(|l| l.region == anchor_regions[side])
                .ok_or_else(|| bad("intended seam ply is not under the assembled needle"))?;
        }
        Ok(AssemblyReceipt {
            model: "flat_fold/1",
            frame: "operation_local_mm",
            needle_mm: needle,
            layers,
            side_layers,
            maximum_registration_error_mm: error,
            minimum_boundary_clearance_mm: minimum,
            stack_mm: stack,
        })
    }
}
