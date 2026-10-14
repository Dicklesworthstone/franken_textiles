//! Material-following dry thermal columns on a deforming triangulated surface.
//!
//! Each triangle is an independent through-thickness column, possibly containing several explicitly
//! assigned, coextensive layers. Resistance per CURRENT area follows the characterized constant or
//! temperature law; sensible heat belongs to REFERENCE mass. Stretching cannot create stored heat.
//! This is a declared geometry closure, not a model of compressed loft, air-gap exchange,
//! overlapping garments, in-plane conduction, wind, moisture or human thermal comfort.

use std::collections::BTreeSet;

use ftx_core::{Refusal, Vec3, receipt::sha256_hex};
use serde_json::{Value, json};

use super::{Boundary, DryThermalModel, invalid, positive, temperature};

mod io;
pub(crate) use io::for_drape;

/// One retained material triangle. A seam weld does not merge either triangle's reference area.
#[derive(Clone, Copy, Debug)]
pub struct Patch {
    /// Vertex indices in the deformed surface, in the same frame and metres as the drape solver.
    pub vertices: [usize; 3],
    /// Material's undeformed area in m²; never substitute the deformed area here.
    pub reference_area_m2: f64,
    /// Index of the explicitly assigned through-thickness layer column.
    pub column: usize,
}

/// Immutable topology, reference geometry and source-locked layer columns.
#[derive(Clone, Debug)]
pub struct Surface {
    columns: Vec<DryThermalModel>,
    patches: Vec<Patch>,
    vertex_count: usize,
    sha256: String,
}

/// Material-following temperatures. The private identity prevents a state from another material or
/// mesh being silently reused after substitution. Position changes do not change this identity.
#[derive(Clone, Debug)]
pub struct SurfaceState {
    temperature_c: Vec<Vec<f64>>,
    sha256: String,
}

impl SurfaceState {
    /// Cell-centre temperatures, first by patch then from its inside to its outside.
    #[must_use]
    pub fn temperature_c(&self) -> &[Vec<f64>] {
        &self.temperature_c
    }
}

/// One patch's heat exchange. Positive boundary heat flows from inside toward outside.
#[derive(Clone, Debug)]
pub struct PatchHeat {
    /// Actual deformed triangle area in m², counted once, not once per layer or face side.
    pub area_m2: f64,
    /// Inside flux per current area in W/m².
    pub inside_flux_w_m2: f64,
    /// Outside flux per current area in W/m².
    pub outside_flux_w_m2: f64,
    /// Change of material sensible heat in J, not J/m².
    pub stored_change_j: f64,
}

/// A complete steady solution or backward-Euler step; a refusal publishes no partial result.
#[derive(Clone, Debug)]
pub struct SurfaceStep {
    /// New temperature state, suitable only for this same immutable surface/material identity.
    pub state: SurfaceState,
    /// Per-triangle field in the retained topology order, suitable for a heat-flow map.
    pub patches: Vec<PatchHeat>,
    /// Sum of actual current triangle areas in m²; not wearer skin coverage.
    pub area_m2: f64,
    /// Heat entering all columns in W, using their own areas and boundary conditions.
    pub inside_heat_w: f64,
    /// Heat leaving all columns in W.
    pub outside_heat_w: f64,
    /// Sum of reference-mass sensible heat changes in J.
    pub stored_change_j: f64,
    /// Stored change minus time-integrated net boundary heat in J; zero for a steady evaluation.
    pub balance_error_j: f64,
}

impl Surface {
    /// Prepare independent layer columns without guessing how nearby/overlapping sheets connect.
    /// A lining or fill sharing this triangle belongs in its column, not a duplicate triangle.
    ///
    /// # Errors
    /// Invalid areas, indices, repeated faces, unused columns, or excessive state allocation. Limits
    /// are 65,536 patches, 196,608 vertices and 1,000,000 cell temperatures; no silent truncation.
    pub fn new(
        columns: Vec<DryThermalModel>,
        vertex_count: usize,
        patches: Vec<Patch>,
    ) -> Result<Self, Refusal> {
        if patches.is_empty()
            || patches.len() > 65_536
            || columns.is_empty()
            || columns.len() > patches.len()
            || vertex_count == 0
            || vertex_count > 196_608
        {
            return Err(invalid("invalid thermal surface size"));
        }
        let mut faces = BTreeSet::new();
        let mut used = BTreeSet::new();
        let mut cells = 0usize;
        for p in &patches {
            positive("reference triangle area", p.reference_area_m2)?;
            let Some(column) = columns.get(p.column) else {
                return Err(invalid("thermal patch indexes a missing column"));
            };
            let mut v = p.vertices;
            v.sort_unstable();
            if v[2] >= vertex_count || v[0] == v[1] || v[1] == v[2] || !faces.insert(v) {
                return Err(invalid(
                    "thermal surface has an invalid or repeated triangle",
                ));
            }
            used.insert(p.column);
            cells = cells
                .checked_add(column.cells())
                .filter(|&n| n <= 1_000_000)
                .ok_or_else(|| invalid("thermal surface exceeds one million cells"))?;
        }
        if used.len() != columns.len() {
            return Err(invalid("thermal surface contains an unassigned column"));
        }
        let mut surface = Self {
            columns,
            patches,
            vertex_count,
            sha256: String::new(),
        };
        surface.sha256 =
            sha256_hex(&serde_json::to_vec(&surface.inputs()).map_err(|e| invalid(e.to_string()))?);
        Ok(surface)
    }

    fn inputs(&self) -> Value {
        let columns: Vec<_> = self.columns.iter().map(DryThermalModel::snapshot).collect();
        let patches: Vec<_> = self
            .patches
            .iter()
            .map(|p| {
                json!({
                    "vertices":p.vertices,"reference_area_m2":p.reference_area_m2,"column":p.column
                })
            })
            .collect();
        let geometry_policy = if self
            .columns
            .iter()
            .any(|c| c.temperature_domain().is_some())
        {
            "characterized resistance at material temperature per current area; integrated enthalpy follows reference mass"
        } else {
            "fixed resistance per current area; heat capacity follows reference mass"
        };
        json!({"schema":"ftx-thermal-surface/1", "columns":columns,
            "vertex_count":self.vertex_count,"patches":patches,
            "geometry_policy":geometry_policy,
            "coupling":"independent through-thickness columns; no inferred sheet overlaps or body coverage",
            "comfort_claim":false,"empirical_validation_claim":false})
    }

    /// Full immutable input receipt, including source observations, policy and state identity.
    #[must_use]
    pub fn snapshot(&self) -> Value {
        let mut input = self.inputs();
        input["sha256"] = json!(self.sha256);
        input
    }

    /// Initialize every material cell to the same specified temperature. Changing a material
    /// requires initialization of a new state; remapping stored heat is an explicit separate problem.
    ///
    /// # Errors
    /// Non-finite temperature, below absolute zero, or outside any column's characterized interval.
    pub fn initial(&self, temperature_c: f64) -> Result<SurfaceState, Refusal> {
        temperature(temperature_c)?;
        for column in &self.columns {
            column.check_temperature(temperature_c)?;
        }
        Ok(SurfaceState {
            temperature_c: self
                .patches
                .iter()
                .map(|p| vec![temperature_c; self.columns[p.column].cells()])
                .collect(),
            sha256: self.sha256.clone(),
        })
    }

    /// Steady heat flow and per-triangle cell temperatures at explicit column-specific boundaries.
    ///
    /// # Errors
    /// Invalid geometry, boundary count/conditions, non-finite heat or an incomplete thermal column.
    pub fn steady(
        &self,
        positions_m: &[Vec3],
        boundaries: &[Boundary],
    ) -> Result<SurfaceStep, Refusal> {
        self.evaluate(None, positions_m, 0.0, boundaries)
    }

    /// Advance material temperatures on the supplied CURRENT geometry. This is backward Euler on
    /// a frozen geometry step: no advection between triangles or mechanical work heating is implied.
    /// The caller's geometry and state remain unchanged on any failure.
    ///
    /// # Errors
    /// Wrong material/topology identity, invalid time, geometry, boundaries or coefficient overflow.
    pub fn advance(
        &self,
        old: &SurfaceState,
        positions_m: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
    ) -> Result<SurfaceStep, Refusal> {
        positive("surface thermal time step", seconds)?;
        if old.sha256 != self.sha256 || old.temperature_c.len() != self.patches.len() {
            return Err(invalid(
                "thermal state belongs to a different material or reference mesh",
            ));
        }
        self.evaluate(Some(old), positions_m, seconds, boundaries)
    }

    fn evaluate(
        &self,
        old: Option<&SurfaceState>,
        positions: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
    ) -> Result<SurfaceStep, Refusal> {
        if positions.len() != self.vertex_count
            || positions.iter().any(|v| !v.is_finite())
            || boundaries.len() != self.columns.len()
        {
            return Err(invalid(
                "thermal surface needs every vertex and one boundary per column",
            ));
        }
        let mut result = SurfaceStep {
            state: SurfaceState {
                temperature_c: Vec::with_capacity(self.patches.len()),
                sha256: self.sha256.clone(),
            },
            patches: Vec::with_capacity(self.patches.len()),
            area_m2: 0.0,
            inside_heat_w: 0.0,
            outside_heat_w: 0.0,
            stored_change_j: 0.0,
            balance_error_j: 0.0,
        };
        for (i, p) in self.patches.iter().enumerate() {
            let [a, b, c] = p.vertices.map(|v| positions[v]);
            let area = positive("deformed triangle area", 0.5 * (b - a).cross(c - a).norm())?;
            let ratio = positive("deformed/reference area ratio", area / p.reference_area_m2)?;
            let column = &self.columns[p.column];
            let boundary = boundaries[p.column];
            // Delta H_ref/(J*dt) = L(T) is exactly the column equation with dt' = J*dt.
            // R(T) remains per current area. Integrating Cp(T) against reference mass, NOT current
            // area, keeps material mass and stored heat invariant as a sheet stretches or changes pose.
            let state = match old {
                Some(old) => column.advance(
                    &old.temperature_c[i],
                    positive("geometry-scaled thermal time step", seconds * ratio)?,
                    boundary,
                )?,
                None => column.steady(boundary)?,
            };
            let stored = state.stored_change_j_m2 * p.reference_area_m2;
            result.area_m2 += area;
            result.inside_heat_w += area * state.inside_flux_w_m2;
            result.outside_heat_w += area * state.outside_flux_w_m2;
            result.stored_change_j += stored;
            result.patches.push(PatchHeat {
                area_m2: area,
                inside_flux_w_m2: state.inside_flux_w_m2,
                outside_flux_w_m2: state.outside_flux_w_m2,
                stored_change_j: stored,
            });
            result.state.temperature_c.push(state.temperature_c);
        }
        if old.is_some() {
            result.balance_error_j =
                result.stored_change_j - seconds * (result.inside_heat_w - result.outside_heat_w);
        }
        if [
            result.area_m2,
            result.inside_heat_w,
            result.outside_heat_w,
            result.stored_change_j,
            result.balance_error_j,
        ]
        .iter()
        .any(|x| !x.is_finite())
        {
            return Err(invalid("surface heat or stored energy overflow"));
        }
        Ok(result)
    }
}
