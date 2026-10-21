//! Mass of the declared cut assembly, independently of roll waste or simulation omissions.
//!
//! Every physical pattern instance uses the export cutter geometry and its own frozen material.
//! Additional thread, closures, trim and fill require an explicit source-bearing mass inventory.
//! This is a pre-finishing inventory calculation, not a weighed finished garment prediction.

use std::collections::BTreeSet;

use ftx_core::{Refusal, RefusalCode, Vec2, polyline::signed_area, receipt::sha256_hex};
use ftx_pattern::{
    Pattern,
    material_catalog::ResolvedMaterial,
    material_mass::Attachments,
    materials::{Form, Quantity},
    textile_import::{piece_fabric, resolve_fabric},
};
use serde::{Deserialize, Serialize};

use crate::{
    Ctx, ExportOptions, Geom,
    cutter::cutter_ring,
    cutting::geometry::{Work, clear_paths, enclosed, simple},
};

pub use ftx_pattern::material_mass::Extra;

/// Assembly inventory and the existing export allowance policy, for ONE complete pattern expansion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-assembly-mass/1`.
    pub schema: String,
    /// Explicit author declaration that extras covers everything beyond the pattern cut pieces.
    /// False retains useful sheet/group subtotals, but never publishes a complete assembly total.
    pub inventory_complete: bool,
    /// Explicit inventory, including unknown groups. An empty list does not imply completeness.
    pub extras: Vec<Extra>,
    /// Same edge/piece/export allowance hierarchy as cutting; no simulation scaling is used.
    #[serde(default)]
    pub seam_allowance_mm: Option<f64>,
}

/// A mass contribution whose missing value and missing reported range remain distinct.
#[derive(Clone, Debug, Serialize)]
pub struct Mass {
    /// Admitted nominal mass in kg, or unknown. Never a partial value pretending to be a total.
    pub kg: Option<f64>,
    /// Scaled/summed reported input endpoints when ALL contributing observations report them.
    /// Fixed authored geometry/inventory; not a confidence interval or measurement-error estimate.
    pub reported_bounds_kg: Option<[f64; 2]>,
    /// Why the nominal mass is unavailable. Absent bounds alone do not block nominal mass.
    pub unavailable: Option<Refusal>,
}

/// One expanded physical cut piece, including mirror/fold handling from the real exporter.
#[derive(Clone, Debug, Serialize)]
pub struct PieceMass {
    /// Authored source-piece ID.
    pub piece: String,
    /// Physical instance ID; quantities are expanded exactly once.
    pub instance: String,
    /// Source-complete resolved sheet record, when resolution succeeds.
    pub material: Option<ResolvedMaterial>,
    /// Exterior cutter area including allowances/notch tabs, m², before removing closed cutouts.
    pub external_cut_area_m2: f64,
    /// Total strictly contained, disjoint closed cutout area, m².
    pub removed_cutout_area_m2: f64,
    /// Retained sheet area, m². Simulation-only deformation, omission and scaling have no effect.
    pub retained_area_m2: f64,
    /// Mass computed only from admitted source areal density and retained area.
    pub mass: Mass,
}

/// A complete-or-unknown assembly calculation with explicit physical scope.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-assembly-mass-report/1`.
    pub schema: &'static str,
    /// Exact original pattern identity, not a draped/trimmed simulation mesh.
    pub pattern_sha256: String,
    /// Exact inventory and allowance policy identity.
    pub request_sha256: String,
    /// Original sourced inventory; quantities/sources are never edited to make totals agree.
    pub request: Request,
    /// Pattern-owned attachment groups merged once with the explicit request inventory. Exact
    /// repeated group observations share one contribution; conflicting same-ID observations refuse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attached_inventory: Option<Attachments>,
    /// Every expanded physical cut instance, in the pattern's deterministic order.
    pub pieces: Vec<PieceMass>,
    /// Mass of every sheet piece, or unknown if even one contribution is unavailable.
    pub sheet_mass: Mass,
    /// Independent additional group outcomes, in authored inventory order.
    pub extras: Vec<(String, Mass)>,
    /// Complete only under the declared inventory and when every contribution is admitted.
    pub assembly_mass: Mass,
    /// Budgeted polygon-validation work across the entire assembly.
    pub geometry_work: usize,
    /// Always false: this does not weigh or predict an unspecified finished garment.
    pub physical_validation_claim: bool,
    /// Accounting boundaries that distinguish this from stock or simulated-cloth mass.
    pub limitations: Vec<&'static str>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn budget() -> Refusal {
    Refusal::new(
        RefusalCode::BudgetExceeded,
        "assembly mass geometry/inventory budget exceeded",
    )
}
fn positive(x: f64) -> Result<f64, Refusal> {
    if x.is_finite() && x > 0.0 {
        Ok(x)
    } else {
        Err(invalid(
            "mass and retained area must be finite and positive",
        ))
    }
}
fn hash<T: Serialize>(x: &T) -> Result<String, Refusal> {
    Ok(sha256_hex(
        &serde_json::to_vec(x).map_err(|e| invalid(e.to_string()))?,
    ))
}

impl Mass {
    fn unknown(reason: impl Into<String>) -> Self {
        Self::from_result(Err(invalid(reason)))
    }
    fn from_result(result: Result<(f64, Option<[f64; 2]>), Refusal>) -> Self {
        match result {
            Ok((kg, reported_bounds_kg)) => Self {
                kg: Some(kg),
                reported_bounds_kg,
                unavailable: None,
            },
            Err(refusal) => Self {
                kg: None,
                reported_bounds_kg: None,
                unavailable: Some(refusal),
            },
        }
    }
    fn sum<'a>(items: impl Iterator<Item = &'a Self>) -> Self {
        let mut kg = 0.0;
        let mut bounds = Some([0.0, 0.0]);
        for mass in items {
            let Some(value) = mass.kg else {
                return Self::unknown(
                    "one or more declared mass contributions are unavailable; no partial total",
                );
            };
            kg += value;
            bounds = bounds
                .zip(mass.reported_bounds_kg)
                .map(|(a, b)| [a[0] + b[0], a[1] + b[1]]);
        }
        if !kg.is_finite() || bounds.is_some_and(|b| b.iter().any(|v| !v.is_finite())) {
            return Self::unknown("assembly mass sum overflow");
        }
        Self::from_result(Ok((kg, bounds)))
    }
}

fn scale(q: &Quantity, multiplier: f64) -> Result<(f64, Option<[f64; 2]>), Refusal> {
    positive(q.value)?;
    let kg = positive(q.value * multiplier)?;
    let bounds = if let Some([lo, hi]) = q.bounds {
        if !lo.is_finite() || !hi.is_finite() || lo < 0.0 || lo > q.value || hi < q.value {
            return Err(invalid("mass observation has invalid reported bounds"));
        }
        let b = [lo * multiplier, hi * multiplier];
        if b.iter().any(|v| !v.is_finite()) {
            return Err(invalid("mass bounds overflow"));
        }
        Some(b)
    } else {
        None
    };
    Ok((kg, bounds))
}

impl Request {
    /// Validate inventory shape before geometry work. Missing/unsupported observations stay as
    /// individual unknown contributions so the report still exposes the other physical pieces.
    ///
    /// # Errors
    /// Wrong schema, duplicate/empty inventory IDs, excessive inventory or invalid allowance.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-assembly-mass/1" {
            return Err(invalid("expected ftx-assembly-mass/1"));
        }
        if self.extras.len() > 1024 {
            return Err(budget());
        }
        let mut ids = BTreeSet::new();
        if self
            .extras
            .iter()
            .any(|x| x.id.trim().is_empty() || !ids.insert(&x.id))
            || self
                .seam_allowance_mm
                .is_some_and(|x| !x.is_finite() || x < 0.0)
        {
            return Err(invalid(
                "mass inventory needs unique nonempty IDs and a finite nonnegative allowance",
            ));
        }
        Ok(())
    }

    // Use the same source merge for accounting and study admission. Order, exact-observation
    // deduplication and same-ID conflict refusals must not depend on the caller.
    fn merged_inventory<'a>(
        &'a self,
        attached: Option<&'a Attachments>,
    ) -> Result<Vec<&'a Extra>, Refusal> {
        let mut inventory: Vec<&Extra> = self.extras.iter().collect();
        if let Some(attached) = attached {
            for group in &attached.groups {
                if let Some(previous) = inventory.iter().find(|x| x.id == group.extra.id) {
                    if !previous.same_observation(&group.extra)? {
                        return Err(invalid(format!(
                            "attached mass group {} conflicts with the assembly inventory observation",
                            group.extra.id
                        )));
                    }
                } else {
                    inventory.push(&group.extra);
                }
            }
        }
        if inventory.len() > 1024 {
            return Err(budget());
        }
        Ok(inventory)
    }

    // Material variants retain the source attachment inventory. Refusals here still belong to
    // individual mass outcomes; a study uses the count only when the merged inventory is valid.
    pub(crate) fn merged_inventory_count(&self, pattern: &Pattern) -> Result<usize, Refusal> {
        self.validate()?;
        let attached = Attachments::from_pattern(pattern)?;
        Ok(self.merged_inventory(attached.as_ref())?.len())
    }

    /// Compute actual retained cut-sheet mass and sourced extras for one declared assembly.
    /// Requires no roll width, grain direction, mechanics, body or thermal coefficients.
    ///
    /// # Errors
    /// Malformed/unscaled geometry, implicit layers, ambiguous/overlapping cutouts or work overflow.
    /// Missing source mass is reported per contribution and never yields a complete total.
    pub fn evaluate(&self, pattern: &Pattern) -> Result<Report, Refusal> {
        self.evaluate_with_work(pattern, &mut Work::new(20_000_000))
    }

    // Studies share an aggregate geometry budget across successful AND refused alternatives.
    pub(crate) fn evaluate_with_work(
        &self,
        pattern: &Pattern,
        work: &mut Work,
    ) -> Result<Report, Refusal> {
        let initial_work = work.used;
        self.validate()?;
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(invalid(errors.join("; ")));
        }
        let attached_inventory =
            Attachments::from_pattern(pattern)?.filter(|a| !a.groups.is_empty());
        let inventory = self.merged_inventory(attached_inventory.as_ref())?;
        if pattern.instances.is_empty() || pattern.instances.len() > 4096 {
            return Err(budget());
        }
        if pattern.raw()["units"] != "mm" {
            return Err(invalid(
                "assembly mass needs explicit millimetre pattern units",
            ));
        }
        let ctx = Ctx::new(
            pattern,
            ExportOptions {
                sa: self.seam_allowance_mm,
                ..ExportOptions::default()
            },
        )?;
        if !ctx.sa.is_finite() || ctx.sa < 0.0 {
            return Err(invalid("invalid export seam allowance"));
        }
        let source_pieces = pattern.raw()["pieces"]
            .as_array()
            .ok_or_else(|| invalid("pattern needs pieces"))?;
        let mut materials = Vec::new();
        for (i, piece) in source_pieces.iter().enumerate() {
            let fabric = piece_fabric(pattern, i)?;
            if fabric.get("layers").is_some() || pattern.raw()["fabric"].get("layers").is_some() {
                return Err(invalid(
                    "mass needs each physical layer as explicit cut pieces; implicit layers cannot be counted",
                ));
            }
            // Bound the exporter input before its sampled curves/internal labels are allocated.
            if piece["edges"].as_array().is_some_and(|e| e.len() > 128)
                || ["internal", "decor", "drills", "notches"]
                    .iter()
                    .any(|k| piece[k].as_array().is_some_and(|a| a.len() > 256))
                || piece["internal"].as_array().is_some_and(|a| {
                    a.iter()
                        .any(|v| v["pts"].as_array().is_some_and(|p| p.len() > 4096))
                })
            {
                return Err(budget());
            }
            materials.push(resolve_fabric(fabric).and_then(|m| {
                m.ok_or_else(|| invalid("sheet mass needs an explicit material selection"))
            }));
            // Export annotations are permissive; a mass consumer must not silently drop malformed
            // cut points and then report a complete retained area for a different hole.
            if let Some(items) = piece.get("internal") {
                let items = items
                    .as_array()
                    .ok_or_else(|| invalid("internal paths must be an array"))?;
                for item in items.iter().filter(|v| v["kind"] == "cut") {
                    if !item["pts"].as_array().is_some_and(|points| {
                        points.len() >= 2
                            && points.iter().all(|p| {
                                p.as_array().is_some_and(|xy| {
                                    xy.len() == 2
                                        && xy.iter().all(|x| x.as_f64().is_some_and(f64::is_finite))
                                })
                            })
                    }) || item.get("closed").is_some_and(|v| !v.is_boolean())
                    {
                        return Err(invalid(
                            "mass cutouts require complete finite 2D paths and boolean closed flags",
                        ));
                    }
                }
            }
        }
        let mut pieces = Vec::with_capacity(pattern.instances.len());
        for instance in &pattern.instances {
            let geom = ctx.geom(crate::Source::Instance(instance), 6.0)?;
            let (external, cutouts) = areas(&geom, work)?;
            let retained = positive(external - cutouts)?;
            let material = &materials[instance.piece_index];
            let mass = Mass::from_result(material.as_ref().map_err(Clone::clone).and_then(|m| {
                if matches!(m.material().form, Form::Fiber | Form::Yarn) {
                    return Err(invalid(
                        "pattern area requires a sheet/batt/hide, not raw fiber or yarn",
                    ));
                }
                m.material()
                    .require("areal_density", "kg/m2", m.allows_estimates())?;
                scale(&m.material().properties["areal_density"], retained)
            }));
            pieces.push(PieceMass {
                piece: instance.piece.clone(),
                instance: instance.id.clone(),
                material: material.as_ref().ok().cloned(),
                external_cut_area_m2: external,
                removed_cutout_area_m2: cutouts,
                retained_area_m2: retained,
                mass,
            });
        }
        let sheet_mass = Mass::sum(pieces.iter().map(|p| &p.mass));
        let extras: Vec<_> = inventory
            .iter()
            .map(|x| (x.id.clone(), Mass::from_result(x.evaluate())))
            .collect();
        let assembly_mass = if self.inventory_complete {
            Mass::sum(std::iter::once(&sheet_mass).chain(extras.iter().map(|(_, m)| m)))
        } else {
            Mass::unknown("extra-component inventory is not declared complete")
        };
        Ok(Report {
            schema: "ftx-assembly-mass-report/1",
            pattern_sha256: hash(pattern.raw())?,
            request_sha256: hash(self)?,
            request: self.clone(),
            attached_inventory,
            pieces,
            sheet_mass,
            extras,
            assembly_mass,
            geometry_work: work.used - initial_work,
            physical_validation_claim: false,
            limitations: vec![
                "One authored assembly: all expanded cut pieces, including sim:false and unplaced pieces, plus explicitly inventoried retained extras. Purchased waste and simulation scaling are excluded.",
                "Pre-finishing cut geometry retains seam/hem allowances and outward notch tabs. Closed internal cut polygons are removed. Open slits and drill marks remove no area; kerf, seam grading, laundering, coating and other unmodelled finishing changes are not inferred.",
                "Completeness is conditional on the author's explicit extra-component inventory, not automatic discovery of thread, decoration, closures or hidden layers. A laminate density must describe the complete selected laminate; separately represented layers must not count it again.",
                "Reported endpoints are propagated only through fixed authored area and additive mass accounting. They are not statistical confidence, geometry/quantity uncertainty, a weighed garment or empirical fit validation.",
            ],
        })
    }
}

fn ring_area(ring: &[Vec2], work: &mut Work) -> Result<f64, Refusal> {
    if !(3..=4096).contains(&ring.len()) || ring.iter().any(|p| !p.is_finite()) {
        return Err(invalid("mass polygons need 3..4096 finite vertices"));
    }
    if !simple(ring, work).ok_or_else(budget)? {
        return Err(invalid("mass polygon is not simple"));
    }
    // Translation-invariant shoelace evaluation avoids cancellation at a scanner's large origin.
    positive(signed_area(&ring.iter().map(|p| *p - ring[0]).collect::<Vec<_>>()).abs() * 1e-6)
}

fn areas(geom: &Geom, work: &mut Work) -> Result<(f64, f64), Refusal> {
    let outer = cutter_ring(geom)?;
    let external = ring_area(&outer, work)?;
    let mut holes: Vec<Vec<Vec2>> = Vec::new();
    let mut removed = 0.0;
    for (path, closed) in &geom.internal_cut {
        if !closed {
            continue;
        } // A slit has no authored area; never invent kerf.
        let mut ring = path.clone();
        if ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        let area = ring_area(&ring, work)?;
        if !enclosed(&ring, &outer, 0.0, work).ok_or_else(budget)? {
            return Err(invalid(
                "closed cutout must be strictly inside its physical cut piece",
            ));
        }
        for other in &holes {
            if !clear_paths(&ring, other, 0.0, work).ok_or_else(budget)? {
                return Err(invalid(
                    "closed cutouts overlap, nest or touch; mass cannot subtract them twice",
                ));
            }
        }
        removed += area;
        holes.push(ring);
    }
    Ok((external, removed))
}
