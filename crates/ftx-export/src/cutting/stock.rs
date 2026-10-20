//! Finite irregular hides and sheet remnants, with authored defects and explicit cutting axes.
//!
//! Physical stock is identified individually, not multiplied or replaced by a bounding rectangle.
//! Placement is a bounded lattice heuristic, not an optimality or infeasibility proof. Accepted
//! paths use full polygon containment/clearance shared with roll nesting. No partial plan escapes.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec2, det, polyline::signed_area, receipt::sha256_hex};
use ftx_pattern::{
    Pattern,
    material_catalog::{ResolvedMaterial, Selection},
    materials::{Form, Source},
    textile_import::{catalog, piece_fabric, resolve_fabric},
};
use serde::{Deserialize, Serialize};

use super::{
    CutPlacement, bad, bbox, cut_shape,
    geometry::{Work, clear, enclosed, simple},
    grain_reversal, half_turn, place, positive, relocate,
};
use crate::{Ctx, ExportOptions, layout::rotation};

pub mod search;
/// Source-bearing finite-stock placement regions.
pub mod zones;

/// One individually inventoried hide, remnant or finite sheet, in its own measured mm frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stock {
    /// Unique physical stock identity. Each item is available exactly once.
    pub id: String,
    /// Must match the pattern's exact material snapshot AND evidence policy, not just a fiber name.
    pub material: Selection,
    /// Simple external polygon; either winding, no repeated closing vertex. Edge damage belongs here.
    pub outline_mm: Vec<Vec2>,
    /// Simple, nonoverlapping interior exclusions, strictly inside the external boundary.
    #[serde(default)]
    pub defects_mm: Vec<Vec<Vec2>>,
    /// Already-consumed exterior cut regions from earlier accepted plans. These are not defects:
    /// future cutters use the piece-to-piece gap rather than the stock/defect boundary margin.
    #[serde(default)]
    pub occupied_mm: Vec<Vec<Vec2>>,
    /// Stable receipt chain explaining the occupied regions. Required when `occupied_mm` is nonempty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occupied_basis: Option<String>,
    /// Optional source-bearing placement regions. Names do not infer mechanical or quality values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<zones::Zone>,
    /// Directed preferred cutting axis; for a hide this is authored, never inferred as a weave.
    pub grain_mm: [Vec2; 2],
    /// Evidence/rationale for actual stock geometry, exclusions, units and cutting axis.
    pub geometry_source: Source,
}

/// Explicit planning controls, not machine calibration or material measurements.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// Complete garment copies; the pattern's cut quantities are expanded exactly once.
    pub copies: usize,
    /// Minimum Euclidean distance between external cut paths, in mm.
    pub gap_mm: f64,
    /// Minimum distance to the stock boundary AND defects, in mm.
    pub boundary_margin_mm: f64,
    /// Lattice spacing in the stock's axis-aligned frame. Failure may require refinement.
    pub step_mm: f64,
    /// Total stock-validation/search operations across ALL stocks, in 1..20,000,000.
    pub work_limit: usize,
    /// Existing export allowance hierarchy is used when absent.
    #[serde(default)]
    pub seam_allowance_mm: Option<f64>,
    /// Exact material SHA-256 identities for which the job permits 180-degree grain reversal.
    /// Empty by default. This is an explicit nap/print-direction decision, not a fiber-name inference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grain_reversal_materials: Vec<String>,
    /// Source-piece ID -> required named region. Applies to every expanded instance and copy.
    /// Unmapped pieces remain unrestricted; constrained pieces are allocated first.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub piece_zones: BTreeMap<String, String>,
    /// Minimum Euclidean clearance to a required region boundary, independent of stock/defect margin.
    #[serde(default, skip_serializing_if = "zones::is_zero")]
    pub zone_margin_mm: f64,
    /// Opt-in reconsideration of earlier placements after a later piece cannot fit.
    /// Limits undone decisions to 1..=100,000; all trials still share `work_limit`.
    /// Omission preserves the original first-fit path and serialized request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backtrack_limit: Option<usize>,
    /// Continue past the first complete layout to minimize the declared stock-use objective.
    /// Requires `backtrack_limit`. A budget reached after a complete layout retains that incumbent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimize: Option<search::Objective>,
}

/// Finite stock allocation. Stock order is an explicit first-fit preference.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-finite-stock-request/1`.
    pub schema: String,
    /// Individually identified stock, in preferred consumption order.
    pub stocks: Vec<Stock>,
    /// Explicit search, allowance and spacing policy.
    pub options: Options,
}

/// Validated stock and complete allocations in a local +y cutting-axis frame.
#[derive(Clone, Debug, Serialize)]
pub struct Marker {
    /// Unique physical stock identity.
    pub id: String,
    /// Same material authority used by draping, sewing and purchasing.
    pub material: ResolvedMaterial,
    /// Rigid affine map from the authored stock frame to the emitted planning frame.
    pub source_to_marker: [[f64; 3]; 2],
    /// Full transformed stock boundary, not its bounding rectangle.
    pub outline_mm: Vec<Vec2>,
    /// Transformed exclusions. Internal cut holes are not reused to hide defects.
    pub defects_mm: Vec<Vec<Vec2>>,
    /// Prior accepted cuts transformed into the same planning frame.
    pub occupied_mm: Vec<Vec<Vec2>>,
    /// Original source-bearing regions transformed into this marker frame; not cutting paths.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<zones::Zone>,
    /// Whole polygon area including defects, in m²; not roll width times guessed length.
    pub gross_area_m2: f64,
    /// Polygon area less disjoint defects; margins and kerf are NOT subtracted.
    pub defect_free_area_m2: f64,
    /// Defect-free area less prior accepted external cut areas; clearances are not subtracted.
    pub available_area_m2: f64,
    /// Sum of allocated external cutter areas; interior holes remain occupied.
    pub external_cut_area_m2: f64,
    /// Allocated external area / whole stock area. Zero for unused stock.
    pub utilization: f64,
    /// Whole stock mass from admitted areal density; never inferred from hide thickness.
    pub stock_mass_kg: Option<f64>,
    /// Why optional mass is unknown; missing mass never becomes zero.
    pub mass_unavailable: Option<Refusal>,
    /// Deterministic instance/copy order, not machine traversal or sewing order.
    pub placements: Vec<CutPlacement>,
}

/// Atomic complete allocation; private fields prevent mutation of geometry underlying previews.
#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    schema: &'static str,
    pattern_sha256: String,
    request_sha256: String,
    request: Request,
    markers: Vec<Marker>,
    used_stock_count: usize,
    used_gross_area_m2: f64,
    external_cut_area_m2: f64,
    work_used: usize,
    candidates: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    backtracks_used: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    optimization: Option<search::Optimization>,
    optimality_claim: bool,
    hardware_execution_authorized: bool,
    limitations: Vec<&'static str>,
}

fn budget() -> Refusal {
    Refusal::new(
        RefusalCode::BudgetExceeded,
        "finite-stock work budget exhausted; no complete plan published",
    )
}
fn checked<T>(value: Option<T>) -> Result<T, Refusal> {
    value.ok_or_else(budget)
}
fn sheet(m: &ResolvedMaterial) -> Result<(), Refusal> {
    if matches!(m.material().form, Form::Fiber | Form::Yarn | Form::Fill) {
        return Err(bad(
            "finite stock requires a sheet or hide identity, not loose fiber/yarn/fill",
        ));
    }
    Ok(())
}
fn ring(p: &[Vec2], work: &mut Work) -> Result<f64, Refusal> {
    if !(3..=4096).contains(&p.len()) || p.iter().any(|v| !v.is_finite()) {
        return Err(bad("stock polygons need 3..4096 finite vertices"));
    }
    bbox(p)?;
    if !checked(simple(p, work))? {
        return Err(bad("stock polygon is not simple or retraces an edge"));
    }
    positive("stock polygon area", area_m2(p))
}
fn area_m2(p: &[Vec2]) -> f64 {
    // A scanner's arbitrary origin must not destroy small area differences in the shoelace sum.
    signed_area(&p.iter().map(|v| *v - p[0]).collect::<Vec<_>>()).abs() * 1e-6
}
fn marker(stock: &Stock, material: ResolvedMaterial, work: &mut Work) -> Result<Marker, Refusal> {
    sheet(&material)?;
    let d = stock.grain_mm[1] - stock.grain_mm[0];
    if stock.grain_mm.iter().any(|v| !v.is_finite())
        || !d.is_finite()
        || !d.norm2().is_finite()
        || d.norm2() == 0.0
    {
        return Err(bad("stock needs an explicit finite, nonzero cutting axis"));
    }
    let source = &stock.geometry_source;
    if source.reference.trim().is_empty()
        || source.accessed_on.trim().is_empty()
        || source.note.trim().is_empty()
    {
        return Err(bad(
            "stock geometry needs source, date and characterization note",
        ));
    }
    let r = rotation((90.0 - det::atan2(d.y, d.x).to_degrees() + 180.0).rem_euclid(360.0) - 180.0);
    let outline: Vec<_> = stock.outline_mm.iter().map(|p| r.apply(*p)).collect();
    let gross = ring(&outline, work)?;
    let mut defects: Vec<Vec<Vec2>> = Vec::new();
    let mut excluded = 0.0;
    for path in &stock.defects_mm {
        let path: Vec<_> = path.iter().map(|p| r.apply(*p)).collect();
        excluded += ring(&path, work)?;
        if !checked(enclosed(&path, &outline, 0.0, work))? {
            return Err(bad(
                "defects must be strictly inside stock; edge damage belongs in the stock outline",
            ));
        }
        for other in &defects {
            if !checked(clear(&path, bbox(&path)?, other, bbox(other)?, 0.0, work))? {
                return Err(bad("stock defects overlap or contain one another"));
            }
        }
        defects.push(path);
    }
    let usable = positive("defect-free stock area", gross - excluded)?;
    if !stock.occupied_mm.is_empty()
        && stock
            .occupied_basis
            .as_deref()
            .is_none_or(|basis| basis.trim().is_empty())
    {
        return Err(bad(
            "occupied finite-stock regions need a nonempty receipt basis",
        ));
    }
    let mut occupied: Vec<Vec<Vec2>> = Vec::new();
    let mut consumed = 0.0;
    for path in &stock.occupied_mm {
        let path: Vec<_> = path.iter().map(|p| r.apply(*p)).collect();
        consumed += ring(&path, work)?;
        if !checked(enclosed(&path, &outline, 0.0, work))? {
            return Err(bad(
                "occupied stock regions must lie inside the stock outline",
            ));
        }
        for defect in &defects {
            if !checked(clear(&path, bbox(&path)?, defect, bbox(defect)?, 0.0, work))? {
                return Err(bad("occupied stock regions overlap a declared defect"));
            }
        }
        for other in &occupied {
            if !checked(clear(&path, bbox(&path)?, other, bbox(other)?, 0.0, work))? {
                return Err(bad("occupied stock regions overlap one another"));
            }
        }
        occupied.push(path);
    }
    let available = positive("available finite-stock area", usable - consumed)?;
    let zones = zones::compile(stock, &r, &outline, work)?;
    let mass = material
        .material()
        .require("areal_density", "kg/m2", material.allows_estimates())
        .and_then(|rho| positive("areal density", rho))
        .and_then(|rho| positive("whole stock mass", rho * gross));
    Ok(Marker {
        id: stock.id.clone(),
        material,
        source_to_marker: [[r.a[0][0], r.a[0][1], 0.0], [r.a[1][0], r.a[1][1], 0.0]],
        outline_mm: outline,
        defects_mm: defects,
        occupied_mm: occupied,
        zones,
        gross_area_m2: gross,
        defect_free_area_m2: usable,
        available_area_m2: available,
        external_cut_area_m2: 0.0,
        utilization: 0.0,
        stock_mass_kg: mass.as_ref().ok().copied(),
        mass_unavailable: mass.err(),
        placements: Vec::new(),
    })
}

impl Request {
    /// Allocate every physical pattern instance to individually identified finite stock, atomically.
    /// Width, constitutive moduli and thermal storage are not needed to cut an identified hide.
    /// Frozen replay uses the same original pattern and emitted request; no files are modified.
    ///
    /// # Errors
    /// Invalid/ambiguous geometry, source/evidence mismatch, incomplete assignments, no candidate
    /// placement, or exceeded bounds: 128 stocks, 4096 instances, 32768 stock vertices, 20M work units.
    pub fn build(&self, pattern: &Pattern) -> Result<Plan, Refusal> {
        let o = &self.options;
        if self.schema != "ftx-finite-stock-request/1"
            || self.stocks.is_empty()
            || self.stocks.len() > 128
            || !(1..=4096).contains(&o.copies)
            || !(1..=20_000_000).contains(&o.work_limit)
        {
            return Err(bad(
                "invalid finite-stock schema, stock count, copies or work limit",
            ));
        }
        positive("finite-stock search step", o.step_mm)?;
        if o.backtrack_limit
            .is_some_and(|n| !(1..=100_000).contains(&n))
        {
            return Err(bad("backtrack_limit must be in 1..100000 when supplied"));
        }
        if o.optimize.is_some() && o.backtrack_limit.is_none() {
            return Err(bad(
                "finite-stock optimize requires an explicit backtrack_limit",
            ));
        }
        for v in [o.gap_mm, o.boundary_margin_mm, o.zone_margin_mm]
            .into_iter()
            .chain(o.seam_allowance_mm)
        {
            if !v.is_finite() || v < 0.0 {
                return Err(bad(
                    "finite-stock allowances must be finite and nonnegative",
                ));
            }
        }
        let vertices = self.stocks.iter().try_fold(0usize, |n, s| {
            s.defects_mm
                .iter()
                .chain(&s.occupied_mm)
                .chain(s.zones.iter().map(|z| &z.outline_mm))
                .try_fold(n.checked_add(s.outline_mm.len())?, |n, p| {
                    n.checked_add(p.len())
                })
        });
        if vertices.is_none_or(|n| n > 32768)
            || self.stocks.iter().any(|s| {
                s.defects_mm.len() > 256 || s.occupied_mm.len() > 4096 || s.zones.len() > 256
            })
            || o.piece_zones.len() > 4096
        {
            return Err(budget());
        }
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(bad(errors.join("; ")));
        }
        let raw = pattern.raw();
        let pieces = raw["pieces"]
            .as_array()
            .ok_or_else(|| bad("pattern needs pieces"))?;
        if pattern
            .instances
            .len()
            .checked_mul(o.copies)
            .is_none_or(|n| n == 0 || n > 4096)
            || pieces.len() > 4096
        {
            return Err(budget());
        }
        zones::validate_assignments(o, pieces)?;
        if raw["fabric"].get("layers").is_some() {
            return Err(bad(
                "implicit layers need explicit cut pieces; finite stock cannot invent them",
            ));
        }
        let ctx = Ctx::new(
            pattern,
            ExportOptions {
                sa: o.seam_allowance_mm,
                ..ExportOptions::default()
            },
        )?;
        if !ctx.sa.is_finite() || ctx.sa < 0.0 {
            return Err(bad("invalid export seam allowance"));
        }
        let mut work = Work::new(o.work_limit);
        let db = catalog()?;
        let mut ids = BTreeSet::new();
        let mut frozen = self.clone();
        let mut markers = Vec::new();
        for stock in &mut frozen.stocks {
            if stock.id.trim().is_empty() || !ids.insert(stock.id.clone()) {
                return Err(bad("physical stock IDs must be nonempty and unique"));
            }
            let material = db.resolve(&stock.material)?;
            stock.material = material.frozen_selection();
            markers.push(marker(stock, material, &mut work)?);
        }
        let mut materials = Vec::new();
        for i in 0..pieces.len() {
            let m = resolve_fabric(piece_fabric(pattern, i)?)?
                .ok_or_else(|| bad(format!("piece {i} needs an explicit material selection")))?;
            sheet(&m)?;
            if !markers.iter().any(|s| same_material(&s.material, &m)) {
                return Err(bad(format!(
                    "no stock matches piece {i} material {} and evidence policy",
                    m.material().id
                )));
            }
            materials.push(m);
        }
        for (i, piece) in pieces.iter().enumerate() {
            if let Some(zone) = piece["id"].as_str().and_then(|id| o.piece_zones.get(id))
                && !markers.iter().any(|stock| {
                    same_material(&stock.material, &materials[i])
                        && stock.zones.iter().any(|z| z.id == *zone)
                })
            {
                return Err(bad(format!(
                    "no stock matches piece {} required zone {zone} and material identity",
                    piece["id"]
                )));
            }
        }
        let reversible = grain_reversal(&o.grain_reversal_materials, &materials)?;
        let shapes = pattern
            .instances
            .iter()
            .map(|i| cut_shape(&ctx, i))
            .collect::<Result<Vec<_>, _>>()?;
        for shape in &shapes {
            for (path, closed) in &shape.geom.internal_cut {
                if !checked(super::geometry::path_inside(
                    path,
                    *closed,
                    &shape.ring,
                    &mut work,
                ))? {
                    return Err(bad(
                        "finite-stock internal cuts must lie strictly inside their external cut path",
                    ));
                }
            }
            for (point, diameter) in &shape.geom.drills {
                if !checked(super::geometry::disk_inside(
                    *point,
                    *diameter / 2.0,
                    &shape.ring,
                    &mut work,
                ))? {
                    return Err(bad(
                        "finite-stock drill discs must lie strictly inside their external cut path",
                    ));
                }
            }
        }
        let mut order: Vec<_> = (0..shapes.len())
            .flat_map(|i| (0..o.copies).map(move |c| (i, c)))
            .collect();
        order.sort_by(|&(a, ac), &(b, bc)| {
            o.piece_zones
                .contains_key(&shapes[b].geom.pid)
                .cmp(&o.piece_zones.contains_key(&shapes[a].geom.pid))
                .then_with(|| shapes[b].area.total_cmp(&shapes[a].area))
                .then_with(|| shapes[a].geom.iid.cmp(&shapes[b].geom.iid))
                .then(ac.cmp(&bc))
        });
        let mut optimization = None;
        let backtracks_used = if let Some(limit) = o.backtrack_limit {
            let tasks = order
                .iter()
                .map(|&(index, copy)| {
                    let shape = &shapes[index];
                    let material = &materials[pattern.instances[index].piece_index];
                    search::Task {
                        shape,
                        material,
                        copy,
                        zone: o.piece_zones.get(&shape.geom.pid).map(String::as_str),
                        reversible: reversible.contains(material.sha256()),
                    }
                })
                .collect::<Vec<_>>();
            let result = search::allocate(&tasks, &mut markers, o, limit, &mut work)?;
            optimization = result.optimization;
            Some(result.undone)
        } else {
            for (index, copy) in order {
                let s = &shapes[index];
                let m = &materials[pattern.instances[index].piece_index];
                let required_zone = o.piece_zones.get(&s.geom.pid).map(String::as_str);
                let cutter = place(s, copy, Vec2::ZERO)?;
                let turned = if reversible.contains(m.sha256()) {
                    checked(work.spend(cutter.cut_mm.len()))?;
                    Some(half_turn(&cutter).ok_or_else(|| bad("grain reversal overflows"))?)
                } else {
                    None
                };
                let mut placed = false;
                for marker in &mut markers {
                    if !same_material(&marker.material, m) {
                        continue;
                    }
                    let mut candidate =
                        locate(&cutter, s.area, marker, required_zone, o, &mut work)?;
                    if candidate.is_none()
                        && let Some(turned) = &turned
                    {
                        candidate = locate(turned, s.area, marker, required_zone, o, &mut work)?;
                    }
                    if let Some(p) = candidate {
                        marker.external_cut_area_m2 += s.area;
                        marker.placements.push(p);
                        placed = true;
                        break;
                    }
                }
                if !placed {
                    return Err(bad(format!(
                        "no checked finite-stock placement for {} copy {copy}; refine step_mm, reorder stock or supply more matching stock (not a proof of infeasibility)",
                        s.geom.iid
                    )));
                }
            }
            None
        };
        let mut used = 0;
        let mut gross = 0.0;
        let mut cut = 0.0;
        for marker in &mut markers {
            marker
                .placements
                .sort_by(|a, b| a.instance.cmp(&b.instance).then(a.copy.cmp(&b.copy)));
            marker.utilization = marker.external_cut_area_m2 / marker.gross_area_m2;
            if !marker.utilization.is_finite()
                || marker.utilization > 1.0 + 1e-10
                || marker.external_cut_area_m2 > marker.available_area_m2 * (1.0 + 1e-10)
            {
                return Err(bad("finite-stock area balance failed"));
            }
            if !marker.placements.is_empty() {
                used += 1;
                gross += marker.gross_area_m2;
                cut += marker.external_cut_area_m2;
            }
        }
        positive("used stock area", gross)?;
        positive("allocated cut area", cut)?;
        Ok(Plan {
            schema: "ftx-finite-stock-plan/1",
            pattern_sha256: hash(raw)?,
            request_sha256: hash(&frozen)?,
            request: frozen,
            markers,
            used_stock_count: used,
            used_gross_area_m2: gross,
            external_cut_area_m2: cut,
            work_used: work.used,
            candidates: work.candidates,
            backtracks_used,
            optimization,
            optimality_claim: false,
            hardware_execution_authorized: false,
            limitations: vec![
                "Finite single-ply stock; authored defects/axis and frozen material identity, not a hide scan or measured property acquisition.",
                if o.optimize.is_some() {
                    "Bounded stock-use minimization retains a complete incumbent; proof is conditional on the declared lattice and admitted orientations, not continuous-space optimality or purchase cost."
                } else if o.backtrack_limit.is_some() {
                    "Bounded depth-first reconsideration of stock, lattice and explicitly permitted half-turn choices; first complete checked layout, not a minimum-waste or globally optimal solution."
                } else if o.piece_zones.is_empty() {
                    "Largest-exterior-area-first pieces, stock input order, forward then explicitly permitted half-turn, bottom-left lattice; no reflection or crossgrain rotation. Not globally optimal."
                } else {
                    "Region-constrained pieces first, then largest exterior area, stock input order and bounded lattice. Grain permissions remain material-specific. No optimality claim."
                },
                if self.stocks.iter().all(|s| s.zones.is_empty()) {
                    "Full polygon checks on emitted geometry; no reuse of internal holes, no shrinkage, pattern-repeat matching, variable-quality zones or physical process qualification."
                } else {
                    "Named regions constrain complete cut outlines only; source declarations do not infer local material laws or certify quality. Defects and prior cuts remain excluded."
                },
                "Residual requests preserve prior exterior cuts as occupied geometry; they do not reconstruct arbitrary free-space polygons, price offcuts, or authorize hardware.",
            ],
        })
    }
}
fn hash(value: &impl Serialize) -> Result<String, Refusal> {
    Ok(sha256_hex(
        &serde_json::to_vec(value).map_err(|e| bad(e.to_string()))?,
    ))
}
fn same_material(a: &ResolvedMaterial, b: &ResolvedMaterial) -> bool {
    a.sha256() == b.sha256() && a.allows_estimates() == b.allows_estimates()
}
fn locate(
    shape: &CutPlacement,
    area: f64,
    stock: &Marker,
    required_zone: Option<&str>,
    o: &Options,
    work: &mut Work,
) -> Result<Option<CutPlacement>, Refusal> {
    locate_next(
        shape,
        area,
        stock,
        required_zone,
        o,
        work,
        &mut search::Cursor::default(),
    )
}
fn locate_next(
    shape: &CutPlacement,
    area: f64,
    stock: &Marker,
    required_zone: Option<&str>,
    o: &Options,
    work: &mut Work,
    cursor: &mut search::Cursor,
) -> Result<Option<CutPlacement>, Refusal> {
    let zone = match required_zone {
        Some(id) => match stock.zones.iter().find(|z| z.id == id) {
            Some(zone) => Some(zone),
            None => return Ok(None),
        },
        None => None,
    };
    let b = bbox(&stock.outline_mm)?;
    let w = shape.bounds_mm[2] - shape.bounds_mm[0];
    let h = shape.bounds_mm[3] - shape.bounds_mm[1];
    // Padding affects candidate construction only; no geometry or requested clearance is clamped.
    let guard = 128.0 * f64::EPSILON * (1.0 + b.iter().map(|v| v.abs()).fold(0.0, f64::max));
    let margin = o.boundary_margin_mm + guard;
    let mut lo = Vec2::new(b[0] + margin, b[1] + margin);
    let mut hi = Vec2::new(b[2] - margin - w, b[3] - margin - h);
    if let Some(zone) = zone {
        let z = bbox(&zone.outline_mm)?;
        let margin = o.zone_margin_mm + guard;
        lo.x = lo.x.max(z[0] + margin);
        lo.y = lo.y.max(z[1] + margin);
        hi.x = hi.x.min(z[2] - margin - w);
        hi.y = hi.y.min(z[3] - margin - h);
    }
    if hi.x < lo.x || hi.y < lo.y {
        return Ok(None);
    }
    let nx = axis_steps(lo.x, hi.x, o.step_mm, work.limit)?;
    let ny = axis_steps(lo.y, hi.y, o.step_mm, work.limit)?;
    let start = *cursor;
    for iy in start.y..=ny {
        let y = if iy == ny {
            hi.y
        } else {
            lo.y + iy as f64 * o.step_mm
        };
        for ix in (if iy == start.y { start.x } else { 0 })..=nx {
            *cursor = if ix == nx {
                search::Cursor { x: 0, y: iy + 1 }
            } else {
                search::Cursor { x: ix + 1, y: iy }
            };
            checked(work.tick())?;
            work.candidates += 1;
            checked(work.spend(shape.cut_mm.len()))?;
            let x = if ix == nx {
                hi.x
            } else {
                lo.x + ix as f64 * o.step_mm
            };
            let offset = Vec2::new(x - shape.bounds_mm[0], y - shape.bounds_mm[1]);
            let ring: Vec<_> = shape.cut_mm.iter().map(|p| *p + offset).collect();
            let rb = bbox(&ring)?;
            if !checked(enclosed(
                &ring,
                &stock.outline_mm,
                o.boundary_margin_mm,
                work,
            ))? {
                continue;
            }
            if let Some(zone) = zone
                && !checked(enclosed(&ring, &zone.outline_mm, o.zone_margin_mm, work))?
            {
                continue;
            }
            let mut free = true;
            for defect in &stock.defects_mm {
                if !checked(clear(
                    &ring,
                    rb,
                    defect,
                    bbox(defect)?,
                    o.boundary_margin_mm,
                    work,
                ))? {
                    free = false;
                    break;
                }
            }
            if !free {
                continue;
            }
            for occupied in &stock.occupied_mm {
                if !checked(clear(&ring, rb, occupied, bbox(occupied)?, o.gap_mm, work))? {
                    free = false;
                    break;
                }
            }
            if !free {
                continue;
            }
            for p in &stock.placements {
                if !checked(clear(&ring, rb, &p.cut_mm, p.bounds_mm, o.gap_mm, work))? {
                    free = false;
                    break;
                }
            }
            if free {
                let placed = relocate(shape, false, offset, ring, rb)
                    .ok_or_else(|| bad("finite-stock placement overflows"))?;
                let d = placed.grain_mm[1] - placed.grain_mm[0];
                if !d.is_finite()
                    || d.norm2() == 0.0
                    || !d.norm2().is_finite()
                    || (area_m2(&placed.cut_mm) - area).abs() > area * 1e-8
                    || !checked(simple(&placed.cut_mm, work))?
                {
                    return Err(bad(
                        "finite-stock translation lost representable cutter geometry",
                    ));
                }
                return Ok(Some(placed));
            }
        }
    }
    Ok(None)
}
fn axis_steps(lo: f64, hi: f64, step: f64, limit: usize) -> Result<usize, Refusal> {
    let count = ((hi - lo) / step).ceil();
    if !count.is_finite() || count > limit as f64 {
        return Err(budget());
    }
    Ok(count as usize)
}
impl Plan {
    /// Complete allocations; callers cannot mutate the geometry underlying a preview.
    #[must_use]
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }
    /// Portable, frozen stock request; apply to the SAME original pattern for replay.
    #[must_use]
    pub fn request(&self) -> &Request {
        &self.request
    }

    /// Produce a frozen request for the same physical inventory after consuming this complete plan.
    /// Existing stock boundaries/defects/material locks are preserved. Accepted exterior cutter
    /// paths become occupied regions in each stock's original authored frame, so a later plan cannot
    /// reuse the same physical area. This is a conservative inventory receipt, not a polygonized
    /// description of every disconnected offcut and not a reservation side effect.
    ///
    /// # Errors
    /// Non-rigid or non-finite stored stock transforms, malformed placement geometry, or serialization
    /// failure. The original plan and request are unchanged.
    pub fn residual_request(&self) -> Result<Request, Refusal> {
        let mut next = self.request.clone();
        if next.stocks.len() != self.markers.len() {
            return Err(bad("finite-stock receipt lost inventory correspondence"));
        }
        for (stock, marker) in next.stocks.iter_mut().zip(&self.markers) {
            let a = marker.source_to_marker;
            let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
            if !det.is_finite() || (det - 1.0).abs() > 1e-10 {
                return Err(bad(
                    "finite-stock receipt transform is not a proper rigid rotation",
                ));
            }
            let inverse = |p: Vec2| {
                let x = p.x - a[0][2];
                let y = p.y - a[1][2];
                Vec2::new(a[0][0] * x + a[1][0] * y, a[0][1] * x + a[1][1] * y)
            };
            for placement in &marker.placements {
                if placement.cut_mm.len() < 3 || placement.cut_mm.iter().any(|p| !p.is_finite()) {
                    return Err(bad(
                        "finite-stock receipt contains malformed placement geometry",
                    ));
                }
                stock
                    .occupied_mm
                    .push(placement.cut_mm.iter().copied().map(inverse).collect());
            }
            if !marker.placements.is_empty() {
                let receipt = format!("ftx-finite-stock-plan/1:{}", self.request_sha256);
                stock.occupied_basis = Some(match stock.occupied_basis.take() {
                    Some(previous) => format!("{previous} -> {receipt}"),
                    None => receipt,
                });
            }
        }
        Ok(next)
    }
}
