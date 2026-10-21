//! Explicit fractional body coverage and series-connected garment layers.
//!
//! Mapping fractions declare disjoint areal partitions; this module does not discover geometric
//! overlaps, infer air gaps, or prove that a hand-authored partition matches a real garment. Each
//! retained cloth triangle has a finite allocation budget: splitting a sheet never duplicates mass.
//! The supplied body mesh is a prescribed-temperature boundary, not a thermoregulating human.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec3, receipt::sha256_hex};
use ftx_pattern::material_optics::Side;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Boundary, DryThermalModel, Surface, invalid, positive};
use crate::thermal::{
    exterior::ExteriorFlux, nonnegative, steady::exterior::Exterior, temperature,
};

pub mod motion;

/// A disjoint portion of one retained cloth triangle, assigned to a named physical layer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Portion {
    /// Physical layer identity, e.g. shell or lining. Multiple pieces may belong to one layer.
    pub layer: String,
    /// Index in the source thermal surface's retained triangle order, never the viewer mesh order.
    pub cloth_triangle: usize,
    /// Fraction of both the reference and current triangle area, in (0, 1].
    pub fraction: f64,
}

/// A body-area partition with textile layers ordered from inside to outside.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    /// Stable region identity, retained in the field report.
    pub id: String,
    /// Index into [`Layout::body_triangles`].
    pub body_triangle: usize,
    /// Disjoint fraction of this body's triangle area, in (0, 1].
    pub body_fraction: f64,
    /// Series-connected material portions, in declared inside-to-outside order.
    pub layers: Vec<Portion>,
    /// Exactly one explicit interface resistance per adjacent pair, in m² K/W of covered BODY area.
    /// No gap, convection, or radiation coefficient is inferred from proximity or a material label.
    pub interface_resistance_m2_k_w: Vec<f64>,
}

/// Explicit topology/area allocation; source material snapshots are supplied separately.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// Must be `ftx-thermal-coverage-layout/1`.
    pub schema: String,
    /// Source or rationale for the declared disjoint partitions, layer identities and interfaces.
    pub mapping_basis: String,
    /// Number of vertices required in each body pose, in metres.
    pub body_vertex_count: usize,
    /// Body boundary triangles. Duplicated faces (including reversed copies) are refused.
    pub body_triangles: Vec<[usize; 3]>,
    /// Disjoint covered fractions. The remainder of each body triangle is an explicit bare path.
    pub regions: Vec<Region>,
    /// Through-thickness cells for every layer in every region.
    pub cells_per_layer: usize,
}

/// A complete source-locked coverage assembly. Geometry may move; topology and reference mass may not.
#[derive(Clone, Debug)]
pub struct Coverage {
    source: Surface,
    layout: Layout,
    models: Vec<DryThermalModel>,
    covered: Vec<f64>,
    allocated: Vec<f64>,
    sha256: String,
}

/// Material temperatures segmented by declared region, not duplicated over the source sheet.
#[derive(Clone, Debug)]
pub struct CoverageState {
    sha256: String,
    temperatures: Vec<Vec<f64>>,
    // A fresh uniform state has no geometric orientation yet. Solved/remapped states bind each
    // layer's body-outward cell order to the material normal; a pose cannot silently reverse it.
    orientation: Option<Vec<Vec<Option<Side>>>>,
}

impl CoverageState {
    /// Region order, then inside-to-outside thermal cell order.
    #[must_use]
    pub fn temperature_c(&self) -> &[Vec<f64>] {
        &self.temperatures
    }
}

/// Heat-flow results for one covered region. Positive heat flows from the body side outwards.
#[derive(Clone, Debug, Serialize)]
pub struct RegionHeat {
    /// Layout region identity.
    pub id: String,
    /// Owning body triangle.
    pub body_triangle: usize,
    /// Current covered body area, m². This is counted once, irrespective of layer count.
    pub body_area_m2: f64,
    /// Heat entering the innermost textile, W.
    pub inside_heat_w: f64,
    /// Heat leaving the outermost textile, W.
    pub outside_heat_w: f64,
    /// Integrated sensible heat change of ALL allocated reference masses, J.
    pub stored_change_j: f64,
}

/// Covered and bare contributions to each prescribed body-boundary triangle.
#[derive(Clone, Debug, Serialize)]
pub struct BodyHeat {
    /// Entire triangle's current area, m².
    pub area_m2: f64,
    /// Covered part of this triangle, counted once, m².
    pub covered_area_m2: f64,
    /// Signed heat through the uncovered fraction and its declared films, W.
    pub bare_heat_w: f64,
    /// Covered inner heat plus bare heat, W.
    pub inside_heat_w: f64,
    /// Covered outer heat plus bare heat, W.
    pub outside_heat_w: f64,
}

/// Explicit exterior policies in body-triangle order. Bare skin has its own declared film path;
/// an outer textile's emissivity is never applied to the uncovered body fraction.
#[derive(Clone, Copy, Debug)]
pub struct ExteriorConditions<'a> {
    /// One environment per body triangle, qualified against each active region's outermost sheet.
    /// Automatic material-face mode resolves each active outer sheet separately from its signed
    /// cloth/body normal. Explicit mode retains the caller's qualified physical face selection.
    pub environments: &'a [Exterior],
    /// One film boundary per body triangle, or empty only when every body triangle is fully covered.
    /// Active bare paths must share the covered path's prescribed inside/body temperature.
    pub bare_boundaries: &'a [Boundary],
}

/// Exterior face and signed powers for one series-connected covered region.
#[derive(Clone, Debug, Serialize)]
pub struct RegionExteriorExchange {
    /// Layout region identity; order matches [`CoverageStep::regions`].
    pub id: String,
    /// Owning body triangle, which also indexes the prescribed environment.
    pub body_triangle: usize,
    /// Retained source cloth triangle carrying the exposed material observation.
    pub outer_cloth_triangle: usize,
    /// Outermost physical layer identity; inner layers contribute storage and conduction only.
    pub outer_layer: String,
    /// Allocated CURRENT area of the outer cloth portion, m², distinct from covered body area.
    pub outer_area_m2: f64,
    /// Face temperature and signed fluxes per current outer cloth area.
    pub flux: ExteriorFlux,
    /// Signed convection from this outer cloth portion, W.
    pub convective_heat_w: f64,
    /// Signed radiation from this outer cloth portion, W.
    pub radiative_heat_w: f64,
    /// Covered outgoing conduction minus exterior exchange, W.
    pub balance_residual_w: f64,
}

/// Textile exterior exchange only. Bare-body film heat remains separately reported in CoverageStep.
#[derive(Clone, Debug, Serialize)]
pub struct CoverageExteriorExchange {
    /// One active outer face for every covered region, in the original region order.
    pub regions: Vec<RegionExteriorExchange>,
    /// Sum of signed covered-textile convection, W.
    pub convective_heat_w: f64,
    /// Sum of signed covered-textile radiation, W.
    pub radiative_heat_w: f64,
    /// Sum of covered exterior face-equation residuals, W; distinct from stored-heat residuals.
    pub balance_residual_w: f64,
}

/// Atomic steady/transient result. Unmapped cloth is explicitly excluded, not quietly counted twice.
#[derive(Clone, Debug)]
pub struct CoverageStep {
    /// New state for this same assembly identity.
    pub state: CoverageState,
    /// Covered regions in layout order.
    pub regions: Vec<RegionHeat>,
    /// Per-body-triangle heat fields, including exposed fractions.
    pub body: Vec<BodyHeat>,
    /// Total current body-boundary area, m²; not total garment area or an inferred wearer's skin area.
    pub body_area_m2: f64,
    /// Current covered area, m², independent of textile layer count.
    pub covered_area_m2: f64,
    /// Bare-path heat, W, included in both boundary totals.
    pub bare_heat_w: f64,
    /// Heat entering covered and bare paths, W.
    pub inside_heat_w: f64,
    /// Heat leaving covered and bare paths, W.
    pub outside_heat_w: f64,
    /// Stored sensible heat change based on allocated reference mass, J.
    pub stored_change_j: f64,
    /// Stored heat minus the net integrated boundary heat, J.
    pub balance_error_j: f64,
    /// Net heat entering storage, W, summed before adding cancelling bare-path heat.
    pub net_heat_w: f64,
    /// Unallocated reference cloth area. Its heat is NOT part of this result.
    pub unassigned_cloth_reference_area_m2: f64,
    /// Present only when exterior exchange was explicitly requested for the covered textile paths.
    pub exterior_exchange: Option<CoverageExteriorExchange>,
}

fn fraction(f: f64) -> Result<(), Refusal> {
    positive("coverage fraction", f)?;
    if f > 1.0 {
        return Err(invalid("coverage fraction exceeds one"));
    }
    Ok(())
}

fn area(vertices: [usize; 3], positions: &[Vec3]) -> Result<f64, Refusal> {
    let [a, b, c] = vertices.map(|i| positions[i]);
    positive("coverage triangle area", 0.5 * (b - a).cross(c - a).norm())
}

fn positions(x: &[Vec3], count: usize) -> Result<(), Refusal> {
    if x.len() != count || x.iter().any(|v| !v.is_finite()) {
        return Err(invalid("coverage pose needs every finite vertex in metres"));
    }
    Ok(())
}

// Shared with motion's depth remapper and the transport-only coverage consumer. A mirrored pattern
// is meshed CCW again: its retained vertex order, not its .L name, defines the positive material side.
pub(in crate::thermal) fn material_side(
    cloth_vertices: [usize; 3],
    cloth: &[Vec3],
    body_vertices: [usize; 3],
    body: &[Vec3],
) -> Option<Side> {
    let [a, b, c] = cloth_vertices.map(|i| cloth[i]);
    let cloth_normal = (b - a).cross(c - a);
    let [a, b, c] = body_vertices.map(|i| body[i]);
    let body_normal = (b - a).cross(c - a);
    let cosine = (cloth_normal / cloth_normal.norm()).dot(body_normal / body_normal.norm());
    if !cosine.is_finite() || cosine.abs() <= 1e-12 {
        None
    } else if cosine < 0.0 {
        Some(Side::Negative)
    } else {
        Some(Side::Positive)
    }
}

impl Layout {
    // Shared geometric/allocation authority for both storage-bearing and conduction-only coverage.
    // The legacy cells_per_layer budget remains a layout limit; steady transport allocates no cells.
    pub(crate) fn allocations(&self, cloth_count: usize) -> Result<(Vec<f64>, Vec<f64>), Refusal> {
        let layout = self;
        if layout.schema != "ftx-thermal-coverage-layout/1"
            || layout.mapping_basis.trim().is_empty()
            || layout.body_vertex_count == 0
            || layout.body_vertex_count > 196_608
            || layout.body_triangles.is_empty()
            || layout.body_triangles.len() > 65_536
            || layout.regions.len() > 65_536
            || !(1..=4096).contains(&layout.cells_per_layer)
        {
            return Err(invalid(
                "invalid coverage schema, mapping basis or topology budget",
            ));
        }
        let mut faces = BTreeSet::new();
        for &face in &layout.body_triangles {
            let mut f = face;
            f.sort_unstable();
            if f[2] >= layout.body_vertex_count || f[0] == f[1] || f[1] == f[2] || !faces.insert(f)
            {
                return Err(invalid("invalid or duplicated coverage body triangle"));
            }
        }
        let mut covered = vec![0.0; layout.body_triangles.len()];
        let mut allocated = vec![0.0; cloth_count];
        let mut cloth_layer = BTreeMap::new();
        let mut order: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut cells = 0usize;
        for region in &layout.regions {
            if region.id.trim().is_empty()
                || !ids.insert(&region.id)
                || region.body_triangle >= covered.len()
                || region.layers.is_empty()
                || region.layers.len() > 64
            {
                return Err(invalid(
                    "invalid coverage region identity, body triangle or layer count",
                ));
            }
            fraction(region.body_fraction)?;
            covered[region.body_triangle] += region.body_fraction;
            if covered[region.body_triangle] > 1.0 {
                return Err(invalid("coverage allocates a body triangle more than once"));
            }
            cells = region
                .layers
                .len()
                .checked_mul(layout.cells_per_layer)
                .filter(|&n| n <= 4096)
                .and_then(|n| cells.checked_add(n))
                .filter(|&n| n <= 1_000_000)
                .ok_or_else(|| invalid("coverage exceeds thermal cell budget"))?;
            if region.interface_resistance_m2_k_w.len() != region.layers.len() - 1 {
                return Err(invalid(
                    "coverage needs one resistance per adjacent layer pair",
                ));
            }
            for &r in &region.interface_resistance_m2_k_w {
                nonnegative("coverage interface resistance", r)?;
            }
            let mut seen = BTreeSet::new();
            for portion in &region.layers {
                fraction(portion.fraction)?;
                if portion.layer.trim().is_empty() || !seen.insert(portion.layer.as_str()) {
                    return Err(invalid(
                        "physical layer is missing or repeated within a coverage region",
                    ));
                }
                if portion.cloth_triangle >= cloth_count {
                    return Err(invalid("coverage refers to an absent cloth triangle"));
                }
                if let Some(old) =
                    cloth_layer.insert(portion.cloth_triangle, portion.layer.as_str())
                    && old != portion.layer
                {
                    return Err(invalid(
                        "one cloth triangle cannot belong to different physical layers",
                    ));
                }
                allocated[portion.cloth_triangle] += portion.fraction;
                if allocated[portion.cloth_triangle] > 1.0 {
                    return Err(invalid("coverage duplicates reference cloth area/mass"));
                }
                order.entry(portion.layer.as_str()).or_default();
            }
            for pair in region.layers.windows(2) {
                order
                    .entry(pair[0].layer.as_str())
                    .or_default()
                    .insert(pair[1].layer.as_str());
            }
        }
        // Kahn traversal detects conflicting ordering across regions without recursion or a dense graph.
        let mut indegree: BTreeMap<&str, usize> = order.keys().map(|&k| (k, 0)).collect();
        for edges in order.values() {
            for &next in edges {
                *indegree.get_mut(next).unwrap() += 1;
            }
        }
        let mut ready: BTreeSet<_> = indegree
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(&k, _)| k)
            .collect();
        let mut visited = 0;
        while let Some(node) = ready.pop_first() {
            visited += 1;
            for &next in &order[node] {
                let count = indegree.get_mut(next).unwrap();
                *count -= 1;
                if *count == 0 {
                    ready.insert(next);
                }
            }
        }
        if visited != order.len() {
            return Err(invalid("coverage physical-layer order is cyclic"));
        }
        Ok((covered, allocated))
    }
}

impl Coverage {
    /// Assemble explicit covered-body regions from an existing frozen independent-sheet surface.
    /// One source triangle must represent one textile layer; already-stacked columns are refused.
    /// Area allocations may split one sheet over many regions and combine many sheets over one body
    /// triangle. Both allocation budgets and the global physical-layer ordering are checked.
    ///
    /// # Errors
    /// Invalid topology, fractions, repeated identities, oversubscribed material/body area, cyclic
    /// layer order, missing material coefficients, or more than 65,536 regions / one million cells.
    pub fn new(source: Surface, layout: Layout) -> Result<Self, Refusal> {
        let (covered, allocated) = layout.allocations(source.patches.len())?;
        if source.columns.iter().any(|c| c.layers.len() != 1) {
            return Err(invalid(
                "coverage requires independent single-layer source triangles",
            ));
        }
        let models = layout
            .regions
            .iter()
            .map(|region| {
                let layers = region
                    .layers
                    .iter()
                    .map(|portion| {
                        let patch = &source.patches[portion.cloth_triangle];
                        source.columns[patch.column].layers[0].clone()
                    })
                    .collect();
                DryThermalModel::new(
                    layers,
                    region.interface_resistance_m2_k_w.clone(),
                    layout.cells_per_layer,
                )
            })
            .collect::<Result<_, _>>()?;
        let mut assembly = Self {
            source,
            layout,
            models,
            covered,
            allocated,
            sha256: String::new(),
        };
        assembly.sha256 = sha256_hex(
            &serde_json::to_vec(&assembly.inputs()).map_err(|e| invalid(e.to_string()))?,
        );
        Ok(assembly)
    }

    fn inputs(&self) -> Value {
        json!({"schema":"ftx-thermal-coverage/1", "source":self.source.snapshot(), "layout":self.layout,
            "scope":"declared disjoint fractional coverage; independent series columns; no inferred geometric overlaps",
            "geometry_policy":"layer resistance uses current cloth area; enthalpy uses allocated reference mass; films/interfaces use covered body area",
            "bare_policy":"unallocated body fraction exchanges through declared films; no tissue storage",
            "unmapped_cloth_policy":"excluded and reported", "comfort_claim":false,"empirical_validation_claim":false})
    }

    /// Full material/topology receipt. Current poses are separate so pose updates preserve the state.
    #[must_use]
    pub fn snapshot(&self) -> Value {
        let mut v = self.inputs();
        v["sha256"] = json!(self.sha256);
        v
    }

    /// Restore frozen observations and topology without a catalog lookup or coefficient substitution.
    ///
    /// # Errors
    /// Altered, incomplete or unsupported snapshot, or an invalid assembly.
    pub fn from_snapshot(value: &Value) -> Result<Self, Refusal> {
        let source = Surface::from_snapshot(&value["source"])?;
        let layout =
            serde_json::from_value(value["layout"].clone()).map_err(|e| invalid(e.to_string()))?;
        let assembly = Self::new(source, layout)?;
        if assembly.snapshot() != *value {
            return Err(invalid("coverage snapshot mismatch"));
        }
        Ok(assembly)
    }

    /// Total temperature cells in covered regions, excluding massless bare paths.
    #[must_use]
    pub fn cells(&self) -> usize {
        self.models.iter().map(DryThermalModel::cells).sum()
    }

    /// Number of body triangles; boundary arrays follow this order.
    #[must_use]
    pub fn body_triangle_count(&self) -> usize {
        self.layout.body_triangles.len()
    }

    /// Initialize allocated material to a common temperature, subject to every material's domain.
    ///
    /// # Errors
    /// Invalid or out-of-domain temperature.
    pub fn initial(&self, t: f64) -> Result<CoverageState, Refusal> {
        temperature(t)?;
        for model in &self.models {
            model.check_temperature(t)?;
        }
        Ok(CoverageState {
            sha256: self.sha256.clone(),
            temperatures: self.models.iter().map(|m| vec![t; m.cells()]).collect(),
            orientation: None,
        })
    }

    /// Calculate coupled steady heat fields on declared body coverage and a supplied cloth pose.
    ///
    /// # Errors
    /// Incomplete/degenerate geometry, invalid boundaries, unsupported temperature or solve failure.
    pub fn steady(
        &self,
        cloth: &[Vec3],
        body: &[Vec3],
        boundaries: &[Boundary],
    ) -> Result<CoverageStep, Refusal> {
        self.evaluate(None, cloth, body, 0.0, boundaries, None)
    }

    /// Advance source-following heat storage without replacing reference masses as the poses change.
    ///
    /// # Errors
    /// State from another material/topology, reversed material orientation without motion remapping,
    /// invalid time, geometry or boundary, or failed solve.
    pub fn advance(
        &self,
        old: &CoverageState,
        cloth: &[Vec3],
        body: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
    ) -> Result<CoverageStep, Refusal> {
        self.evaluate(Some(old), cloth, body, seconds, boundaries, None)
    }

    /// Steady temperatures with qualified convection/radiation on each region's outermost material.
    /// Body films and interfaces use covered body area; exterior exchange uses allocated current
    /// OUTER cloth area. The covered outside film must be zero and outside_c must equal air_c.
    ///
    /// # Errors
    /// Existing coverage/thermal refusals, conflicting outside films, unqualified exterior data,
    /// missing explicit bare-body conditions or failed exterior surface balance.
    pub fn steady_exterior(
        &self,
        cloth: &[Vec3],
        body: &[Vec3],
        boundaries: &[Boundary],
        exterior: ExteriorConditions<'_>,
    ) -> Result<CoverageStep, Refusal> {
        self.evaluate(None, cloth, body, 0.0, boundaries, Some(exterior))
    }

    /// Advance layered heat storage with source-qualified exterior exchange and explicit bare films.
    /// A new interval may prescribe a different environment without resetting material temperatures.
    /// Material/topology changes still require a new state; geometry repartition uses motion::Frame.
    ///
    /// # Errors
    /// The same state/time/geometry refusals as advance, plus exterior evidence and balance refusals.
    pub fn advance_exterior(
        &self,
        old: &CoverageState,
        cloth: &[Vec3],
        body: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
        exterior: ExteriorConditions<'_>,
    ) -> Result<CoverageStep, Refusal> {
        self.evaluate(Some(old), cloth, body, seconds, boundaries, Some(exterior))
    }

    fn evaluate(
        &self,
        old: Option<&CoverageState>,
        cloth: &[Vec3],
        body: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
        exterior: Option<ExteriorConditions<'_>>,
    ) -> Result<CoverageStep, Refusal> {
        if let Some(old) = old {
            positive("coverage time step", seconds)?;
            if old.sha256 != self.sha256 || old.temperatures.len() != self.models.len() {
                return Err(invalid(
                    "coverage state belongs to a different material or layout",
                ));
            }
        }
        positions(cloth, self.source.vertex_count)?;
        positions(body, self.layout.body_vertex_count)?;
        if boundaries.len() != self.layout.body_triangles.len() {
            return Err(invalid("coverage needs one boundary per body triangle"));
        }
        if let Some(e) = exterior {
            let count = self.layout.body_triangles.len();
            if e.environments.len() != count
                || (!e.bare_boundaries.is_empty() && e.bare_boundaries.len() != count)
                || (e.bare_boundaries.is_empty() && self.covered.iter().any(|&f| f < 1.0))
            {
                return Err(invalid(
                    "coverage exterior needs one environment per body triangle and explicit bare films for uncovered area",
                ));
            }
        }
        let cloth_areas = self
            .source
            .patches
            .iter()
            .map(|p| area(p.vertices, cloth))
            .collect::<Result<Vec<_>, _>>()?;
        let mut fields = Vec::new();
        for (i, &face) in self.layout.body_triangles.iter().enumerate() {
            let a = area(face, body)?;
            let b = boundaries[i];
            temperature(b.inside_c)?;
            temperature(b.outside_c)?;
            nonnegative("inside film resistance", b.inside_resistance)?;
            nonnegative("outside film resistance", b.outside_resistance)?;
            let bare = if self.covered[i] < 1.0 {
                let b = if let Some(e) = exterior {
                    let explicit = e.bare_boundaries[i];
                    temperature(explicit.inside_c)?;
                    temperature(explicit.outside_c)?;
                    nonnegative("bare inside film resistance", explicit.inside_resistance)?;
                    nonnegative("bare outside film resistance", explicit.outside_resistance)?;
                    if explicit.inside_c != b.inside_c {
                        return Err(invalid(
                            "covered and bare paths must share the prescribed body temperature",
                        ));
                    }
                    explicit
                } else {
                    b
                };
                let r = positive(
                    "bare-body path needs a positive declared film resistance",
                    b.inside_resistance + b.outside_resistance,
                )?;
                a * (1.0 - self.covered[i]) * ((b.inside_c - b.outside_c) / r)
            } else {
                0.0
            };
            fields.push(BodyHeat {
                area_m2: a,
                covered_area_m2: a * self.covered[i],
                bare_heat_w: bare,
                inside_heat_w: bare,
                outside_heat_w: bare,
            });
        }
        let orientation: Vec<Vec<_>> = self
            .layout
            .regions
            .iter()
            .map(|region| {
                region
                    .layers
                    .iter()
                    .map(|portion| {
                        material_side(
                            self.source.patches[portion.cloth_triangle].vertices,
                            cloth,
                            self.layout.body_triangles[region.body_triangle],
                            body,
                        )
                    })
                    .collect()
            })
            .collect();
        if old
            .and_then(|s| s.orientation.as_ref())
            .is_some_and(|previous| previous != &orientation)
        {
            return Err(invalid(
                "coverage material orientation changed; use thermal motion remap to preserve through-thickness temperatures",
            ));
        }
        let mut result = CoverageStep {
            state: CoverageState {
                sha256: self.sha256.clone(),
                temperatures: Vec::new(),
                orientation: Some(orientation),
            },
            regions: Vec::new(),
            body: fields,
            body_area_m2: 0.0,
            covered_area_m2: 0.0,
            bare_heat_w: 0.0,
            inside_heat_w: 0.0,
            outside_heat_w: 0.0,
            stored_change_j: 0.0,
            balance_error_j: 0.0,
            net_heat_w: 0.0,
            unassigned_cloth_reference_area_m2: self
                .source
                .patches
                .iter()
                .zip(&self.allocated)
                .map(|(p, f)| p.reference_area_m2 * (1.0 - f))
                .sum(),
            exterior_exchange: exterior.map(|_| CoverageExteriorExchange {
                regions: Vec::with_capacity(self.layout.regions.len()),
                convective_heat_w: 0.0,
                radiative_heat_w: 0.0,
                balance_residual_w: 0.0,
            }),
        };
        for (i, region) in self.layout.regions.iter().enumerate() {
            let a = positive(
                "covered body area",
                result.body[region.body_triangle].area_m2 * region.body_fraction,
            )?;
            let mut resistance = Vec::new();
            let mut capacity = Vec::new();
            for p in &region.layers {
                let current = positive(
                    "allocated current cloth area",
                    cloth_areas[p.cloth_triangle] * p.fraction,
                )?;
                let reference = positive(
                    "allocated reference cloth area",
                    self.source.patches[p.cloth_triangle].reference_area_m2 * p.fraction,
                )?;
                resistance.push(a / current);
                capacity.push(reference / a);
            }
            let model = self.models[i].with_area_factors(&resistance, &capacity)?;
            let b = boundaries[region.body_triangle];
            let state = if let Some(e) = exterior {
                let outer = region.layers.last().expect("nonempty region");
                let environment = &e.environments[region.body_triangle];
                let side = if environment
                    .radiation
                    .as_ref()
                    .is_some_and(|r| r.automatic_material_face)
                {
                    result
                        .state
                        .orientation
                        .as_ref()
                        .expect("bound orientation")[i]
                        .last()
                        .copied()
                        .flatten()
                        .ok_or_else(|| {
                            invalid("automatic optical face needs an unambiguous cloth/body normal")
                        })?
                } else {
                    Side::Positive
                };
                let outer_area = positive(
                    "allocated outer cloth area",
                    cloth_areas[outer.cloth_triangle] * outer.fraction,
                )?;
                let next = model.exterior_step(
                    old.map(|o| o.temperatures[i].as_slice()),
                    seconds,
                    b,
                    environment,
                    outer_area / a,
                    side,
                )?;
                let convective = outer_area * next.exchange.convective_flux_w_m2;
                let radiative = outer_area * next.exchange.radiative_flux_w_m2;
                let error = (a * next.state.outside_flux_w_m2 - convective) - radiative;
                let tolerance = 1e-9
                    * (outer_area
                        + (a * next.state.outside_flux_w_m2).abs()
                        + convective.abs()
                        + radiative.abs());
                if [convective, radiative, error, tolerance]
                    .iter()
                    .any(|v| !v.is_finite())
                    || error.abs() > tolerance
                {
                    return Err(invalid("coverage exterior power balance failed"));
                }
                let exchange = result
                    .exterior_exchange
                    .as_mut()
                    .expect("exterior requested");
                exchange.convective_heat_w += convective;
                exchange.radiative_heat_w += radiative;
                exchange.balance_residual_w += error;
                exchange.regions.push(RegionExteriorExchange {
                    id: region.id.clone(),
                    body_triangle: region.body_triangle,
                    outer_cloth_triangle: outer.cloth_triangle,
                    outer_layer: outer.layer.clone(),
                    outer_area_m2: outer_area,
                    flux: next.exchange,
                    convective_heat_w: convective,
                    radiative_heat_w: radiative,
                    balance_residual_w: error,
                });
                next.state
            } else {
                match old {
                    None => model.steady(b)?,
                    Some(old) => model.advance(&old.temperatures[i], seconds, b)?,
                }
            };
            let inside = a * state.inside_flux_w_m2;
            let outside = a * state.outside_flux_w_m2;
            let stored = a * state.stored_change_j_m2;
            if [inside, outside, stored].iter().any(|v| !v.is_finite()) {
                return Err(invalid("coverage region heat overflow"));
            }
            result.body[region.body_triangle].inside_heat_w += inside;
            result.body[region.body_triangle].outside_heat_w += outside;
            result.stored_change_j += stored;
            result.net_heat_w += inside - outside;
            result.regions.push(RegionHeat {
                id: region.id.clone(),
                body_triangle: region.body_triangle,
                body_area_m2: a,
                inside_heat_w: inside,
                outside_heat_w: outside,
                stored_change_j: stored,
            });
            result.state.temperatures.push(state.temperature_c);
        }
        for field in &result.body {
            result.body_area_m2 += field.area_m2;
            result.covered_area_m2 += field.covered_area_m2;
            result.bare_heat_w += field.bare_heat_w;
            result.inside_heat_w += field.inside_heat_w;
            result.outside_heat_w += field.outside_heat_w;
        }
        if old.is_some() {
            result.balance_error_j = result.stored_change_j - seconds * result.net_heat_w;
        }
        if let Some(e) = &result.exterior_exchange
            && [
                e.convective_heat_w,
                e.radiative_heat_w,
                e.balance_residual_w,
            ]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(invalid("coverage exterior heat accumulation overflow"));
        }
        if [
            result.body_area_m2,
            result.covered_area_m2,
            result.bare_heat_w,
            result.inside_heat_w,
            result.outside_heat_w,
            result.stored_change_j,
            result.net_heat_w,
            result.balance_error_j,
            result.unassigned_cloth_reference_area_m2,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err(invalid("coverage heat or area accumulation overflow"));
        }
        Ok(result)
    }
}
