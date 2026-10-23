//! Grain-constrained single-ply roll markers from the same instance/cutter geometry as exports.
//! Optional bounded polygon compaction improves a complete shelf marker without changing cut shapes.
//! Source-piece selections are shared with draping; purchasing needs width, not a fictitious modulus.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec2, det, polyline::signed_area, receipt::sha256_hex};
use ftx_pattern::textile_import::{piece_fabric, resolve_fabric};
use ftx_pattern::{Pattern, material_catalog::ResolvedMaterial, materials::Form};
use serde::{Deserialize, Serialize};

use crate::{Affine, Ctx, ExportOptions, Geom, Source, cutter::cutter_ring, layout::rotation};

pub(crate) mod geometry;
pub mod job;
pub mod nesting;
pub mod preview;
pub mod procurement;
pub mod stock;

/// Source-width admission for placement, separate from nominal purchase-unit conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidthBasis {
    /// Preserve ordinary planning at the source's central width.
    #[default]
    Nominal,
    /// Plan inside the source-reported lower width; missing bounds refuse rather than fall back.
    ReportedLowerBound,
}

impl WidthBasis {
    fn is_nominal(&self) -> bool {
        *self == Self::Nominal
    }
}

/// Declared roll-planning policy. Values are allowances, not calibrated machine parameters.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutOptions {
    /// Complete garments to cut; the pattern's own quantities are expanded exactly once.
    pub copies: usize,
    /// Minimum cut-outline separation, in mm (bounding-box separation in shelf-only mode).
    pub gap_mm: f64,
    /// Unused strip on each selvage, in mm, subtracted from the material's sourced width.
    pub edge_margin_mm: f64,
    /// Width used for layout before edge margins; nominal purchased width remains unchanged.
    #[serde(default, skip_serializing_if = "WidthBasis::is_nominal")]
    pub width_basis: WidthBasis,
    /// Unused length at both ends of each marker, in mm.
    pub end_margin_mm: f64,
    /// Optional export seam allowance override; otherwise the existing export hierarchy is used.
    #[serde(default)]
    pub seam_allowance_mm: Option<f64>,
    /// Explicit maximum length of each complete marker, including both end margins, in metres.
    /// Whole physical instances are assigned to separate markers when necessary; none are split.
    /// This job limit does not establish a supplier's available continuous stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_marker_length_m: Option<f64>,
    /// Optional shape-aware placement search; omission retains the original one-way shelf layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nesting: Option<nesting::NestingOptions>,
}

/// One physical cut piece in marker coordinates (mm, y along the stock's authored cutting axis).
#[derive(Clone, Debug, Serialize)]
pub struct CutPlacement {
    /// Source piece identity, not an inferred material name.
    pub piece: String,
    /// Expanded physical instance, including the existing R/L or unfolded convention.
    pub instance: String,
    /// Zero-based garment copy index.
    pub copy: usize,
    /// External cut path including seam allowances and outward notch tabs.
    pub cut_mm: Vec<Vec2>,
    /// Explicit transformed material axis; no generated default grain is accepted.
    pub grain_mm: [Vec2; 2],
    /// True only after an explicitly permitted half-turn: directed grain then points toward -y.
    #[serde(skip_serializing_if = "is_false")]
    pub grain_reversed: bool,
    /// Existing internal cut paths and closure flags, transformed with the outer path.
    pub internal_cut_mm: Vec<(Vec<Vec2>, bool)>,
    /// Drill centres and diameters, in mm. Not a machine execution instruction.
    pub drills_mm: Vec<(Vec2, f64)>,
    /// Bounding box of the external path, [xmin, ymin, xmax, ymax], in mm.
    pub bounds_mm: [f64; 4],
    /// Rotation and translation from the existing expanded instance's frame.
    pub transform: [[f64; 3]; 2],
}

/// Conditional source-width envelope actually used by the complete placement authority.
#[derive(Clone, Debug, Serialize)]
pub struct LayoutWidth {
    /// Full planning width before edge margins, equal to the admitted lower source bound.
    pub width_m: f64,
    /// Original normalized source range, not a confidence interval or a measured guarantee.
    pub reported_bounds_m: [f64; 2],
    /// Fixed-input interpretation and the unchanged scope of purchasing and gross material metrics.
    pub notes: [&'static str; 2],
}

/// One continuous marker of an exact material snapshot, never pooled merely by display name.
#[derive(Clone, Debug, Serialize)]
pub struct RollMarker {
    /// Full, source-preserving material snapshot shared with the physics and quote consumers.
    pub material: ResolvedMaterial,
    /// Nominal sourced stock width, in metres, before the two declared edge margins.
    pub width_m: f64,
    /// Explicit source-lower-bound layout; absent for nominal planning, including legacy requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_width: Option<LayoutWidth>,
    /// Required roll length, including outline gaps and both end margins, in metres.
    pub length_m: f64,
    /// Gross purchased stock area width × length, NOT the sum of pattern-piece areas.
    pub stock_area_m2: f64,
    /// Sum of external cutter polygon areas; interior holes are not subtracted or reused.
    pub external_cut_area_m2: f64,
    /// Outer-piece area / gross stock area; a property of this layout, not an optimal yield claim.
    pub utilization: f64,
    /// Stock mass when a permitted source-bearing areal density is available.
    pub stock_mass_kg: Option<f64>,
    /// Why optional stock mass is unavailable. Missing density never means zero mass.
    pub mass_unavailable: Option<Refusal>,
    /// Placements in deterministic instance/copy order (not sewing or machine traversal order).
    pub placements: Vec<CutPlacement>,
    /// Bounded search outcome against this marker's complete shelf baseline, when requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nesting: Option<nesting::NestingReport>,
}

/// A reproducible manufacturing geometry/BOM result. It does not authorize a cutter or sewing robot.
#[derive(Clone, Debug, Serialize)]
pub struct CutPlan {
    /// `/1` for one unbounded marker per material, `/2` for an explicit marker-length limit.
    pub schema: &'static str,
    /// Hash of the source pattern document, including material declarations and geometry.
    pub pattern_sha256: String,
    /// Explicit source-piece scope of a subplan in a complete mixed-stock job.
    /// Omitted for the ordinary all-piece planner; the pattern hash always names the whole source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_piece_ids: Option<Vec<String>>,
    /// Caller-selected allowances and garment quantity.
    pub options: CutOptions,
    /// Hash-ordered material-specific markers; one material may occupy several bounded markers.
    pub rolls: Vec<RollMarker>,
    /// Fixed planning assumptions and unsupported process effects.
    pub limitations: Vec<&'static str>,
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn is_false(value: &bool) -> bool {
    !*value
}

/// A permission applies to the exact snapshot actually cut, never a fiber/product label.
fn grain_reversal<'a>(
    requested: &'a [String],
    materials: &[ResolvedMaterial],
) -> Result<BTreeSet<&'a str>, Refusal> {
    if requested.len() > 4096 {
        return Err(bad("too many grain-reversal material permissions"));
    }
    let used: BTreeSet<_> = materials.iter().map(ResolvedMaterial::sha256).collect();
    let allowed: BTreeSet<_> = requested.iter().map(String::as_str).collect();
    if allowed.len() != requested.len() || allowed.iter().any(|sha| !used.contains(sha)) {
        return Err(bad(
            "grain_reversal_materials needs unique exact SHA-256 identities of materials cut in this job; re-evaluate permissions after material changes",
        ));
    }
    Ok(allowed)
}
fn positive(name: &str, x: f64) -> Result<f64, Refusal> {
    if !x.is_finite() || x <= 0.0 {
        return Err(bad(format!("{name} must be finite and positive")));
    }
    Ok(x)
}
fn bbox(ring: &[Vec2]) -> Result<[f64; 4], Refusal> {
    if ring.len() < 3 || ring.iter().any(|p| !p.is_finite()) {
        return Err(bad("cut path must contain at least three finite points"));
    }
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in ring {
        b[0] = b[0].min(p.x);
        b[1] = b[1].min(p.y);
        b[2] = b[2].max(p.x);
        b[3] = b[3].max(p.y);
    }
    positive("cut width", b[2] - b[0])?;
    positive("cut height", b[3] - b[1])?;
    Ok(b)
}

impl CutOptions {
    fn validate(&self) -> Result<(), Refusal> {
        if let Some(nesting) = &self.nesting {
            nesting.validate()?;
        }
        if !(1..=4096).contains(&self.copies) {
            return Err(bad("copies must be in 1..4096"));
        }
        for x in [self.gap_mm, self.edge_margin_mm, self.end_margin_mm]
            .into_iter()
            .chain(self.seam_allowance_mm)
        {
            if !x.is_finite() || x < 0.0 {
                return Err(bad("cut allowances must be finite and nonnegative"));
            }
        }
        if let Some(limit) = self.maximum_marker_length_m {
            positive("maximum marker length", limit)?;
            if 2.0 * self.end_margin_mm * 0.001 >= limit {
                return Err(bad("maximum marker length must exceed both end margins"));
            }
        }
        Ok(())
    }
}

struct Shape {
    geom: Geom,
    ring: Vec<Vec2>,
    bounds: [f64; 4],
    angle: f64,
    area: f64,
}

// Shared unfolded/mirrored cutter geometry, not a second approximation for finite stock.
fn cut_shape(ctx: &Ctx<'_>, instance: &ftx_pattern::Instance) -> Result<Shape, Refusal> {
    let geom = ctx.geom(Source::Instance(instance), 6.0)?;
    let (a, b) = geom
        .grain
        .ok_or_else(|| bad(format!("{} needs explicit grain", instance.id)))?;
    let d = b - a;
    if geom.grain_default || !d.is_finite() || !d.norm2().is_finite() || d.norm2() == 0.0 {
        return Err(bad(format!(
            "{} needs explicit nonzero grain; generated export grain is not a cutting direction",
            instance.id
        )));
    }
    let angle = (90.0 - det::atan2(d.y, d.x).to_degrees() + 180.0).rem_euclid(360.0) - 180.0;
    let oriented = geom.transform(&rotation(angle));
    let ring = cutter_ring(&oriented)?;
    let bounds = bbox(&ring)?;
    positive("cut polygon area", signed_area(&ring).abs())?;
    if !crate::outline::ring_is_simple(&ring) {
        return Err(bad("self-intersecting cutter outline"));
    }
    let area = positive("cut area in square metres", signed_area(&ring).abs() * 1e-6)?;
    Ok(Shape {
        geom: oriented,
        ring,
        bounds,
        angle,
        area,
    })
}

fn place(shape: &Shape, copy: usize, offset: Vec2) -> Result<CutPlacement, Refusal> {
    if !offset.is_finite() {
        return Err(bad("cut placement arithmetic overflows"));
    }
    let translated = shape.geom.transform(&Affine {
        a: [[1., 0.], [0., 1.]],
        b: offset,
    });
    let ring: Vec<_> = shape.ring.iter().map(|&p| p + offset).collect();
    let bounds = bbox(&ring)?;
    let r = rotation(shape.angle);
    let (a, b) = translated.grain.ok_or_else(|| bad("cut grain was lost"))?;
    Ok(CutPlacement {
        piece: shape.geom.pid.clone(),
        instance: shape.geom.iid.clone(),
        copy,
        cut_mm: ring,
        grain_mm: [a, b],
        grain_reversed: false,
        internal_cut_mm: translated.internal_cut,
        drills_mm: translated.drills,
        bounds_mm: bounds,
        transform: [
            [r.a[0][0], r.a[0][1], offset.x],
            [r.a[1][0], r.a[1][1], offset.y],
        ],
    })
}

/// Transform every manufacturing coordinate together; the caller checks this exact exterior ring.
fn relocate(
    source: &CutPlacement,
    reverse: bool,
    delta: Vec2,
    ring: Vec<Vec2>,
    bounds: [f64; 4],
) -> Option<CutPlacement> {
    let mut p = source.clone();
    let map = |v: Vec2| if reverse { -v + delta } else { v + delta };
    p.cut_mm = ring;
    p.bounds_mm = bounds;
    p.grain_reversed ^= reverse;
    for v in &mut p.grain_mm {
        *v = map(*v);
    }
    for (path, _) in &mut p.internal_cut_mm {
        for v in path {
            *v = map(*v);
        }
    }
    for (v, _) in &mut p.drills_mm {
        *v = map(*v);
    }
    if reverse {
        for row in &mut p.transform {
            for v in row {
                *v = -*v;
            }
        }
    }
    p.transform[0][2] += delta.x;
    p.transform[1][2] += delta.y;
    if p.cut_mm
        .iter()
        .chain(&p.grain_mm)
        .chain(p.internal_cut_mm.iter().flat_map(|(p, _)| p))
        .chain(p.drills_mm.iter().map(|(p, _)| p))
        .any(|v| !v.is_finite())
        || p.transform.iter().flatten().any(|v| !v.is_finite())
        || p.bounds_mm.iter().any(|v| !v.is_finite())
    {
        return None;
    }
    Some(p)
}

fn half_turn(p: &CutPlacement) -> Option<CutPlacement> {
    let [x, y, x1, y1] = p.bounds_mm;
    relocate(
        p,
        true,
        Vec2::ZERO,
        p.cut_mm.iter().map(|v| -*v).collect(),
        [-x1, -y1, -x, -y],
    )
}
struct Shelf {
    y: f64,
    height: f64,
    right: f64,
}

#[derive(Default)]
struct MarkerParts {
    shelves: Vec<Shelf>,
    placements: Vec<CutPlacement>,
    total_area: f64,
}

impl CutPlan {
    /// Derive real cut-instance requirements and grain-constrained markers per selected material.
    /// Mirror pairs and on-fold unfolding use `Ctx::geom`, exactly as the cutter export does.
    /// `sim:false`, absent body placements and simulation-only scaling never remove/change cut parts.
    /// Grain points up the roll unless nesting explicitly permits its exact material's half-turn.
    ///
    /// # Errors
    /// Invalid/unscaled patterns, unsupported sheet forms, missing width/grain, partial assignments,
    /// oversized pieces, arithmetic overflow, or more than 4096 expanded cut pieces.
    pub fn build(pattern: &Pattern, options: &CutOptions) -> Result<Self, Refusal> {
        Self::build_scoped(pattern, options, None)
    }

    pub(super) fn build_scoped(
        pattern: &Pattern,
        options: &CutOptions,
        scope: Option<&BTreeSet<usize>>,
    ) -> Result<Self, Refusal> {
        options.validate()?;
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(bad(errors.join("; ")));
        }
        let raw = pattern.raw();
        let pieces = raw["pieces"]
            .as_array()
            .ok_or_else(|| bad("pattern needs pieces"))?;
        let indices = piece_scope(pattern, scope)?;
        let instances: Vec<_> = pattern
            .instances
            .iter()
            .filter(|i| indices.contains(&i.piece_index))
            .collect();
        let count = instances.len().checked_mul(options.copies);
        if count.is_none_or(|n| n == 0 || n > 4096) || pieces.len() > 4096 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "cut plan needs 1..4096 expanded instances",
            ));
        }
        if raw["fabric"].get("layers").is_some() {
            return Err(bad(
                "implicit layers need explicit cut pieces; a marker cannot invent them",
            ));
        }
        let ctx = Ctx::new(
            pattern,
            ExportOptions {
                sa: options.seam_allowance_mm,
                ..ExportOptions::default()
            },
        )?;
        if !ctx.sa.is_finite() || ctx.sa < 0.0 {
            return Err(bad("invalid export seam allowance"));
        }
        let mut materials = BTreeMap::new();
        let mut groups: BTreeMap<String, (ResolvedMaterial, Vec<Shape>)> = BTreeMap::new();
        for &i in &indices {
            let record = resolve_fabric(piece_fabric(pattern, i)?)?.ok_or_else(|| {
                bad(format!(
                    "piece {i} needs an explicit material selection, not a cutting label"
                ))
            })?;
            if matches!(
                record.material().form,
                Form::Fiber | Form::Yarn | Form::Fill | Form::Hide
            ) {
                return Err(bad(
                    "roll markers require woven/knit/nonwoven/laminate sheet stock; hides and loose fill need different stock geometry",
                ));
            }
            positive(
                "stock width",
                record
                    .material()
                    .require("width", "m", record.allows_estimates())?,
            )?;
            materials.insert(i, record);
        }
        if let Some(search) = &options.nesting {
            grain_reversal(
                &search.grain_reversal_materials,
                &materials.values().cloned().collect::<Vec<_>>(),
            )?;
        }
        for instance in instances {
            let shape = cut_shape(&ctx, instance)?;
            let record = &materials[&instance.piece_index];
            groups
                .entry(record.sha256().to_owned())
                .or_insert_with(|| (record.clone(), Vec::new()))
                .1
                .push(shape);
        }
        let mut rolls = Vec::with_capacity(groups.len());
        for (_, (material, shapes)) in groups {
            rolls.extend(pack(material, &shapes, options)?);
        }
        let pattern_sha256 = sha256_hex(&serde_json::to_vec(raw).map_err(|e| bad(e.to_string()))?);
        Ok(Self {
            schema: if options.maximum_marker_length_m.is_some() {
                "ftx-roll-cut-plan/2"
            } else {
                "ftx-roll-cut-plan/1"
            },
            pattern_sha256,
            source_piece_ids: scope.map(|_| scope_names(pattern, &indices)),
            options: options.clone(),
            rolls,
            limitations: {
                let mut limits = vec![
                    "Single-ply rectangular roll stock; on-fold pieces are physically unfolded.",
                    "One-way explicit grain unless its material snapshot permits a half-turn; shelf baseline with optional bounded polygon compaction, never a global optimality claim.",
                    "No defect maps, pattern-repeat matching, shrinkage, finish/machine qualification or automatic layer generation.",
                    "External cut area includes interior holes; offcuts are neither reused nor claimed recoverable.",
                    "Nominal source width/density and caller margins; source uncertainty is retained, not propagated into a fit or safety guarantee.",
                    "Planning geometry only: no device execution, sewing order, tool compensation or process-safety authorization.",
                ];
                if options.width_basis == WidthBasis::ReportedLowerBound {
                    limits[4] = "Placement uses the admitted source-reported lower width with fixed geometry and margins. Gross area, mass and invoice conversions retain nominal source width; no area/mass/cost uncertainty is propagated.";
                }
                if options.maximum_marker_length_m.is_some() {
                    limits.push("Whole cut instances are assigned by bounded first-fit shelves; marker count and total length are heuristic, not globally optimal. Compaction stays within each assigned marker.");
                    limits.push("The requested marker-length limit is a planning constraint, not observed continuous stock or a supplier fulfillment guarantee.");
                }
                limits
            },
        })
    }
}

fn piece_scope(
    pattern: &Pattern,
    scope: Option<&BTreeSet<usize>>,
) -> Result<BTreeSet<usize>, Refusal> {
    let count = pattern.raw()["pieces"]
        .as_array()
        .ok_or_else(|| bad("pattern needs source pieces"))?
        .len();
    let indices = scope.cloned().unwrap_or_else(|| (0..count).collect());
    if indices.is_empty() || indices.iter().any(|&i| i >= count) {
        return Err(bad(
            "cutting scope needs existing nonempty source-piece indices",
        ));
    }
    Ok(indices)
}

fn scope_names(pattern: &Pattern, indices: &BTreeSet<usize>) -> Vec<String> {
    indices
        .iter()
        .map(|&i| {
            pattern.raw()["pieces"][i]["id"]
                .as_str()
                .unwrap_or_default()
                .into()
        })
        .collect()
}

fn pack(
    material: ResolvedMaterial,
    shapes: &[Shape],
    options: &CutOptions,
) -> Result<Vec<RollMarker>, Refusal> {
    let width_m = positive(
        "stock width",
        material
            .material()
            .require("width", "m", material.allows_estimates())?,
    )?;
    let layout_width = if options.width_basis == WidthBasis::ReportedLowerBound {
        let bounds = material.material().properties["width"]
            .bounds
            .ok_or_else(|| {
                bad(format!(
                    "material {} needs explicit source-reported width bounds for reported_lower_bound planning",
                    material.material().id
                ))
            })?;
        if bounds.iter().any(|x| !x.is_finite() || *x <= 0.0)
            || bounds[0] > width_m
            || bounds[1] < width_m
        {
            return Err(bad(
                "reported width bounds must be finite, positive and contain nominal width",
            ));
        }
        Some(LayoutWidth {
            width_m: bounds[0],
            reported_bounds_m: bounds,
            notes: [
                "Placement uses the source-reported lower width with fixed cutter geometry and declared margins. The source range is not a confidence interval, stock guarantee or physical validation.",
                "Gross stock area, stock mass and invoice-unit conversion retain nominal source width and the newly planned length. No area, mass or cost uncertainty interval is inferred.",
            ],
        })
    } else {
        None
    };
    let width_mm = positive(
        "layout width in mm",
        layout_width.as_ref().map_or(width_m, |w| w.width_m) * 1000.0,
    )?;
    let usable = positive(
        "usable width after both edge margins",
        width_mm - 2.0 * options.edge_margin_mm,
    )?;
    let mut order: Vec<_> = shapes
        .iter()
        .enumerate()
        .flat_map(|(i, _)| (0..options.copies).map(move |copy| (i, copy)))
        .collect();
    order.sort_by(|&(a, ac), &(b, bc)| {
        let (a, b) = (&shapes[a], &shapes[b]);
        (b.bounds[3] - b.bounds[1])
            .total_cmp(&(a.bounds[3] - a.bounds[1]))
            .then_with(|| (b.bounds[2] - b.bounds[0]).total_cmp(&(a.bounds[2] - a.bounds[0])))
            .then_with(|| a.geom.iid.cmp(&b.geom.iid))
            .then(ac.cmp(&bc))
    });
    // At most 4096 physical instances bound the first-fit search and the number of markers.
    // Without a length limit every new shelf enters the first marker, preserving the legacy order.
    let mut markers: Vec<MarkerParts> = Vec::new();
    for (i, copy) in order {
        let shape = &shapes[i];
        let w = shape.bounds[2] - shape.bounds[0];
        let h = shape.bounds[3] - shape.bounds[1];
        if w > usable {
            return Err(bad(format!(
                "{}: cutter width {w} mm exceeds material {} usable width {usable} mm; crossgrain rotation is not permitted",
                shape.geom.iid,
                material.material().id
            )));
        }
        let fits_length = |y: f64| {
            options
                .maximum_marker_length_m
                .is_none_or(|limit| (y + h + 2.0 * options.end_margin_mm) * 0.001 <= limit)
        };
        if !fits_length(0.0) {
            return Err(bad(format!(
                "{}: whole cutter height {h} mm plus both end margins exceeds the maximum marker length; cutting a piece into segments is not permitted",
                shape.geom.iid
            )));
        }
        let mut slot = None;
        for (index, marker) in markers.iter_mut().enumerate() {
            if let Some(shelf) = marker
                .shelves
                .iter_mut()
                .find(|s| h <= s.height && s.right + options.gap_mm + w <= usable)
            {
                let x = shelf.right + options.gap_mm;
                shelf.right = x + w;
                slot = Some((index, x, shelf.y));
                break;
            }
            let y = marker
                .shelves
                .last()
                .map_or(0.0, |s| s.y + s.height + options.gap_mm);
            if fits_length(y) {
                marker.shelves.push(Shelf {
                    y,
                    height: h,
                    right: w,
                });
                slot = Some((index, 0.0, y));
                break;
            }
        }
        let (index, x, y) = slot.unwrap_or_else(|| {
            let index = markers.len();
            markers.push(MarkerParts {
                shelves: vec![Shelf {
                    y: 0.0,
                    height: h,
                    right: w,
                }],
                ..MarkerParts::default()
            });
            (index, 0.0, 0.0)
        });
        let offset = Vec2::new(
            options.edge_margin_mm + x - shape.bounds[0],
            options.end_margin_mm + y - shape.bounds[1],
        );
        markers[index].placements.push(place(shape, copy, offset)?);
        markers[index].total_area += shape.area;
    }
    markers
        .into_iter()
        .map(|parts| {
            finish_marker(
                material.clone(),
                parts,
                options,
                width_m,
                width_mm,
                layout_width.clone(),
            )
        })
        .collect()
}

fn finish_marker(
    material: ResolvedMaterial,
    parts: MarkerParts,
    options: &CutOptions,
    width_m: f64,
    width_mm: f64,
    layout_width: Option<LayoutWidth>,
) -> Result<RollMarker, Refusal> {
    let MarkerParts {
        shelves,
        mut placements,
        total_area,
    } = parts;
    placements.sort_by(|a, b| a.instance.cmp(&b.instance).then(a.copy.cmp(&b.copy)));
    let baseline_length_m = positive(
        "marker length",
        (shelves.last().ok_or_else(|| bad("empty marker"))?.y
            + shelves.last().unwrap().height
            + 2.0 * options.end_margin_mm)
            * 0.001,
    )?;
    let nesting = options
        .nesting
        .as_ref()
        .map(|search| {
            nesting::compact(
                &mut placements,
                width_mm,
                options,
                search,
                baseline_length_m,
                search
                    .grain_reversal_materials
                    .iter()
                    .any(|sha| sha == material.sha256()),
            )
        })
        .transpose()?;
    let length_m = nesting.as_ref().map_or(baseline_length_m, |r| r.length_m);
    if options
        .maximum_marker_length_m
        .is_some_and(|limit| length_m > limit)
    {
        return Err(bad("computed marker exceeds its maximum length"));
    }
    let stock_area_m2 = positive("gross stock area", width_m * length_m)?;
    let utilization = positive("marker utilization", total_area / stock_area_m2)?;
    if !total_area.is_finite() || utilization > 1.0 + 1e-10 {
        return Err(bad("invalid marker area balance"));
    }
    let mass = material
        .material()
        .require("areal_density", "kg/m2", material.allows_estimates())
        .and_then(|rho| positive("areal density", rho))
        .and_then(|rho| positive("gross stock mass", rho * stock_area_m2));
    Ok(RollMarker {
        material,
        width_m,
        layout_width,
        length_m,
        stock_area_m2,
        external_cut_area_m2: total_area,
        utilization,
        stock_mass_kg: mass.as_ref().ok().copied(),
        mass_unavailable: mass.err(),
        placements,
        nesting,
    })
}
