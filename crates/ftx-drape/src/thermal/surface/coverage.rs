//! Explicit fractional body coverage and series-connected garment layers.
//!
//! Mapping fractions declare disjoint areal partitions; this module does not discover geometric
//! overlaps, infer air gaps, or prove that a hand-authored partition matches a real garment. Each
//! retained cloth triangle has a finite allocation budget: splitting a sheet never duplicates mass.
//! The supplied body mesh is a prescribed-temperature boundary, not a thermoregulating human.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec3, receipt::sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Boundary, DryThermalModel, Surface, invalid, positive};
use crate::thermal::{nonnegative, temperature};

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
        if source.columns.iter().any(|c| c.layers.len() != 1) {
            return Err(invalid(
                "coverage requires independent single-layer source triangles",
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
        let mut allocated = vec![0.0; source.patches.len()];
        let mut cloth_layer = BTreeMap::new();
        let mut order: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut models = Vec::new();
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
            let mut layers = Vec::new();
            let mut seen = BTreeSet::new();
            for portion in &region.layers {
                fraction(portion.fraction)?;
                if portion.layer.trim().is_empty() || !seen.insert(portion.layer.as_str()) {
                    return Err(invalid(
                        "physical layer is missing or repeated within a coverage region",
                    ));
                }
                let patch = source
                    .patches
                    .get(portion.cloth_triangle)
                    .ok_or_else(|| invalid("coverage refers to an absent cloth triangle"))?;
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
                layers.push(source.columns[patch.column].layers[0].clone());
            }
            for pair in region.layers.windows(2) {
                order
                    .entry(pair[0].layer.as_str())
                    .or_default()
                    .insert(pair[1].layer.as_str());
            }
            models.push(DryThermalModel::new(
                layers,
                region.interface_resistance_m2_k_w.clone(),
                layout.cells_per_layer,
            )?);
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
        self.evaluate(None, cloth, body, 0.0, boundaries)
    }

    /// Advance source-following heat storage without replacing reference masses as the poses change.
    ///
    /// # Errors
    /// State from another material/topology, invalid time, geometry or boundary, or failed solve.
    pub fn advance(
        &self,
        old: &CoverageState,
        cloth: &[Vec3],
        body: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
    ) -> Result<CoverageStep, Refusal> {
        positive("coverage time step", seconds)?;
        if old.sha256 != self.sha256 || old.temperatures.len() != self.models.len() {
            return Err(invalid(
                "coverage state belongs to a different material or layout",
            ));
        }
        self.evaluate(Some(old), cloth, body, seconds, boundaries)
    }

    fn evaluate(
        &self,
        old: Option<&CoverageState>,
        cloth: &[Vec3],
        body: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
    ) -> Result<CoverageStep, Refusal> {
        positions(cloth, self.source.vertex_count)?;
        positions(body, self.layout.body_vertex_count)?;
        if boundaries.len() != self.layout.body_triangles.len() {
            return Err(invalid("coverage needs one boundary per body triangle"));
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
        let mut result = CoverageStep {
            state: CoverageState {
                sha256: self.sha256.clone(),
                temperatures: Vec::new(),
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
            let state = match old {
                None => model.steady(b)?,
                Some(old) => model.advance(&old.temperatures[i], seconds, b)?,
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
