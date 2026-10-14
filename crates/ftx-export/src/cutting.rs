//! Grain-constrained single-ply roll markers from the same instance/cutter geometry as exports.
//! Optional bounded polygon compaction improves a complete shelf marker without changing cut shapes.
//! Source-piece selections are shared with draping; purchasing needs width, not a fictitious modulus.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, Vec2, det, polyline::signed_area, receipt::sha256_hex};
use ftx_pattern::textile_import::{piece_fabric, resolve_fabric};
use ftx_pattern::{Pattern, material_catalog::ResolvedMaterial, materials::Form};
use serde::{Deserialize, Serialize};

use crate::{Affine, Ctx, ExportOptions, Geom, Source, cutter::cutter_ring, layout::rotation};

pub mod nesting;
pub mod preview;
pub mod procurement;

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
    /// Unused length at both ends of each marker, in mm.
    pub end_margin_mm: f64,
    /// Optional export seam allowance override; otherwise the existing export hierarchy is used.
    #[serde(default)]
    pub seam_allowance_mm: Option<f64>,
    /// Optional shape-aware translation search; omission retains the original shelf layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nesting: Option<nesting::NestingOptions>,
}

/// One physical cut piece in roll coordinates (mm, y along the roll, all directed grainlines +y).
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
    /// Existing internal cut paths and closure flags, transformed with the outer path.
    pub internal_cut_mm: Vec<(Vec<Vec2>, bool)>,
    /// Drill centres and diameters, in mm. Not a machine execution instruction.
    pub drills_mm: Vec<(Vec2, f64)>,
    /// Bounding box of the external path, [xmin, ymin, xmax, ymax], in mm.
    pub bounds_mm: [f64; 4],
    /// Rotation and translation from the existing expanded instance's frame.
    pub transform: [[f64; 3]; 2],
}

/// A separate roll marker for each exact material snapshot, never pooled merely by display name.
#[derive(Clone, Debug, Serialize)]
pub struct RollMarker {
    /// Full, source-preserving material snapshot shared with the physics and quote consumers.
    pub material: ResolvedMaterial,
    /// Nominal sourced stock width, in metres, before the two declared edge margins.
    pub width_m: f64,
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
    /// `ftx-roll-cut-plan/1`.
    pub schema: &'static str,
    /// Hash of the source pattern document, including material declarations and geometry.
    pub pattern_sha256: String,
    /// Caller-selected allowances and garment quantity.
    pub options: CutOptions,
    /// Hash-ordered material-specific rolls.
    pub rolls: Vec<RollMarker>,
    /// Fixed planning assumptions and unsupported process effects.
    pub limitations: Vec<&'static str>,
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
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
struct Shelf {
    y: f64,
    height: f64,
    right: f64,
}

impl CutPlan {
    /// Derive real cut-instance requirements and a grain-constrained marker per selected material.
    /// Mirror pairs and on-fold unfolding use `Ctx::geom`, exactly as the cutter export does.
    /// `sim:false`, absent body placements and simulation-only scaling never remove/change cut parts.
    /// Every grain points up the roll: no automatic flipping against nap or crossgrain substitution.
    ///
    /// # Errors
    /// Invalid/unscaled patterns, unsupported sheet forms, missing width/grain, partial assignments,
    /// oversized pieces, arithmetic overflow, or more than 4096 expanded cut pieces.
    pub fn build(pattern: &Pattern, options: &CutOptions) -> Result<Self, Refusal> {
        options.validate()?;
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(bad(errors.join("; ")));
        }
        let raw = pattern.raw();
        let pieces = raw["pieces"]
            .as_array()
            .ok_or_else(|| bad("pattern needs pieces"))?;
        let count = pattern.instances.len().checked_mul(options.copies);
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
        let mut materials = Vec::with_capacity(pieces.len());
        let mut groups: BTreeMap<String, (ResolvedMaterial, Vec<Shape>)> = BTreeMap::new();
        for i in 0..pieces.len() {
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
            materials.push(record);
        }
        for instance in &pattern.instances {
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
            let angle =
                (90.0 - det::atan2(d.y, d.x).to_degrees() + 180.0).rem_euclid(360.0) - 180.0;
            let oriented = geom.transform(&rotation(angle));
            let ring = cutter_ring(&oriented)?;
            let bounds = bbox(&ring)?;
            positive("cut polygon area", signed_area(&ring).abs())?;
            if !crate::outline::ring_is_simple(&ring) {
                return Err(bad("self-intersecting cutter outline"));
            }
            let area = signed_area(&ring).abs() * 1e-6;
            positive("cut area in square metres", area)?;
            let record = &materials[instance.piece_index];
            groups
                .entry(record.sha256().to_owned())
                .or_insert_with(|| (record.clone(), Vec::new()))
                .1
                .push(Shape {
                    geom: oriented,
                    ring,
                    bounds,
                    angle,
                    area,
                });
        }
        let mut rolls = Vec::with_capacity(groups.len());
        for (_, (material, shapes)) in groups {
            rolls.push(pack(material, &shapes, options)?);
        }
        let pattern_sha256 = sha256_hex(&serde_json::to_vec(raw).map_err(|e| bad(e.to_string()))?);
        Ok(Self {
            schema: "ftx-roll-cut-plan/1",
            pattern_sha256,
            options: options.clone(),
            rolls,
            limitations: vec![
                "Single-ply rectangular roll stock; on-fold pieces are physically unfolded.",
                "One-way explicit grain; shelf baseline with optional bounded polygon compaction, never a global optimality claim.",
                "No defect maps, pattern-repeat matching, shrinkage, finish/machine qualification or automatic layer generation.",
                "External cut area includes interior holes; offcuts are neither reused nor claimed recoverable.",
                "Nominal source width/density and caller margins; source uncertainty is retained, not propagated into a fit or safety guarantee.",
                "Planning geometry only: no device execution, sewing order, tool compensation or process-safety authorization.",
            ],
        })
    }
}

fn pack(
    material: ResolvedMaterial,
    shapes: &[Shape],
    options: &CutOptions,
) -> Result<RollMarker, Refusal> {
    let width_m = positive(
        "stock width",
        material
            .material()
            .require("width", "m", material.allows_estimates())?,
    )?;
    let width_mm = positive("stock width in mm", width_m * 1000.0)?;
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
    let mut shelves: Vec<Shelf> = Vec::new();
    let mut placements = Vec::with_capacity(order.len());
    let mut total_area = 0.0;
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
        let shelf = shelves
            .iter()
            .position(|s| h <= s.height && s.right + options.gap_mm + w <= usable);
        let (x, y) = match shelf {
            Some(k) => {
                let s = &mut shelves[k];
                let x = s.right + options.gap_mm;
                s.right = x + w;
                (x, s.y)
            }
            None => {
                let y = shelves
                    .last()
                    .map_or(0.0, |s| s.y + s.height + options.gap_mm);
                shelves.push(Shelf {
                    y,
                    height: h,
                    right: w,
                });
                (0.0, y)
            }
        };
        let offset = Vec2::new(
            options.edge_margin_mm + x - shape.bounds[0],
            options.end_margin_mm + y - shape.bounds[1],
        );
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
        placements.push(CutPlacement {
            piece: shape.geom.pid.clone(),
            instance: shape.geom.iid.clone(),
            copy,
            cut_mm: ring,
            grain_mm: [a, b],
            internal_cut_mm: translated.internal_cut,
            drills_mm: translated.drills,
            bounds_mm: bounds,
            transform: [
                [r.a[0][0], r.a[0][1], offset.x],
                [r.a[1][0], r.a[1][1], offset.y],
            ],
        });
        total_area += shape.area;
    }
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
            )
        })
        .transpose()?;
    let length_m = nesting.as_ref().map_or(baseline_length_m, |r| r.length_m);
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
