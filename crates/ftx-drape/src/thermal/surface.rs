//! Material-following dry thermal columns on a deforming triangulated surface.
//!
//! Each triangle is an independent through-thickness column, possibly containing several explicitly
//! assigned, coextensive layers. Resistance per CURRENT area follows the characterized constant or
//! temperature law; sensible heat belongs to REFERENCE mass. Stretching cannot create stored heat.
//! This is a declared geometry closure, not a model of compressed loft, air-gap exchange,
//! overlapping garments, in-plane conduction, wind, moisture or human thermal comfort.

use std::collections::BTreeSet;

use ftx_core::{Refusal, Vec3, receipt::sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Boundary, DryThermalModel, invalid, positive, temperature};
use super::{exterior::ExteriorFlux, steady::exterior::Exterior};

pub mod coverage;
pub(crate) mod io;
pub mod projection;
pub(crate) use io::for_drape;

/// One retained material triangle. A seam weld does not merge either triangle's reference area.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    /// Vertex indices in the deformed surface, in the same frame and metres as the drape solver.
    /// Their ordered normal is the canonical positive/outside material side. Reordering indices
    /// changes source identity; ordinary mirrored pattern instances already preserve CCW order.
    pub vertices: [usize; 3],
    /// Material's undeformed area in m²; never substitute the deformed area here.
    pub reference_area_m2: f64,
    /// Index of the explicitly assigned through-thickness layer column.
    pub column: usize,
}

// Both storage-bearing and conduction-only surfaces use the same retained topology contract.
pub(crate) fn validate_topology(
    columns: usize,
    vertex_count: usize,
    patches: &[Patch],
) -> Result<(), Refusal> {
    if patches.is_empty()
        || patches.len() > 65_536
        || columns == 0
        || columns > patches.len()
        || vertex_count == 0
        || vertex_count > 196_608
    {
        return Err(invalid("invalid thermal surface size"));
    }
    let mut faces = BTreeSet::new();
    let mut used = BTreeSet::new();
    for p in patches {
        positive("reference triangle area", p.reference_area_m2)?;
        if p.column >= columns {
            return Err(invalid("thermal patch indexes a missing column"));
        }
        let mut v = p.vertices;
        v.sort_unstable();
        if v[2] >= vertex_count || v[0] == v[1] || v[1] == v[2] || !faces.insert(v) {
            return Err(invalid(
                "thermal surface has an invalid or repeated triangle",
            ));
        }
        used.insert(p.column);
    }
    if used.len() != columns {
        return Err(invalid("thermal surface contains an unassigned column"));
    }
    Ok(())
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
    /// Requested internal cell energies summed in J; zero for the original source-free APIs.
    pub supplied_heat_j: f64,
    /// Stored change minus net boundary energy and supplied internal heat in J; zero at steady state.
    pub balance_error_j: f64,
    /// Present only when qualified convection/radiation replaces the outside film. The full
    /// per-patch field is suitable for the final display, not unbounded duplication in every trace.
    pub exterior_exchange: Option<SurfaceExteriorExchange>,
}

/// Material-following exterior exchange on the actual current surface geometry.
#[derive(Clone, Debug, Serialize)]
pub struct SurfaceExteriorExchange {
    /// Face temperature and signed flux decomposition, in the same order as the surface patches.
    pub patches: Vec<ExteriorFlux>,
    /// Integrated convection to air over current exposed cloth area, W.
    pub convective_heat_w: f64,
    /// Integrated material-qualified radiation to the enclosure, W.
    pub radiative_heat_w: f64,
    /// Outgoing conductive heat minus convective and radiative heat, W.
    pub balance_residual_w: f64,
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
        validate_topology(columns.len(), vertex_count, &patches)?;
        let mut cells = 0usize;
        for p in &patches {
            cells = cells
                .checked_add(columns[p.column].cells())
                .filter(|&n| n <= 1_000_000)
                .ok_or_else(|| invalid("thermal surface exceeds one million cells"))?;
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
        self.evaluate(None, positions_m, 0.0, boundaries, None, None)
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
        self.evaluate(Some(old), positions_m, seconds, boundaries, None, None)
    }

    /// Advance with one explicitly supplied energy in J per retained patch/cell. Energies belong
    /// to reference material mass: a current-area change never scales or duplicates a heat source.
    /// The source's physical origin and through-thickness distribution belong to the caller.
    ///
    /// # Errors
    /// As [`Self::advance`], plus mismatched cell arrays, negative/nonfinite energy, unavailable
    /// adiabatic enthalpy inside the material domain, or a failed source-inclusive energy balance.
    pub fn advance_with_heat(
        &self,
        old: &SurfaceState,
        positions_m: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
        heat_j: &[Vec<f64>],
    ) -> Result<SurfaceStep, Refusal> {
        positive("surface thermal time step", seconds)?;
        if old.sha256 != self.sha256 || old.temperature_c.len() != self.patches.len() {
            return Err(invalid(
                "thermal state belongs to a different material or reference mesh",
            ));
        }
        self.evaluate(
            Some(old),
            positions_m,
            seconds,
            boundaries,
            None,
            Some(heat_j),
        )
    }

    /// Steady limit with an explicit qualified exterior environment per material column.
    /// Only the outermost material in a column supplies the exposed face's optical properties.
    /// Independent columns do not infer shielding by other nearby sheets.
    ///
    /// # Errors
    /// Wrong geometry or environment count, conflicting films, missing qualification or solve failure.
    pub fn steady_exterior(
        &self,
        positions_m: &[Vec3],
        boundaries: &[Boundary],
        exteriors: &[Exterior],
    ) -> Result<SurfaceStep, Refusal> {
        self.evaluate(None, positions_m, 0.0, boundaries, Some(exteriors), None)
    }

    /// Advance with source-qualified exterior exchange on CURRENT triangle areas while keeping
    /// sensible heat on REFERENCE material mass. Changing emissivity changes cooling without
    /// changing the mechanical source or introducing any reservoir-temperature reset.
    ///
    /// # Errors
    /// A state from another material/reference mesh, invalid time/geometry, wrong environment count,
    /// conflicting outside films, unqualified optical or boundary evidence, or any solver refusal.
    pub fn advance_exterior(
        &self,
        old: &SurfaceState,
        positions_m: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
        exteriors: &[Exterior],
    ) -> Result<SurfaceStep, Refusal> {
        positive("surface thermal time step", seconds)?;
        if old.sha256 != self.sha256 || old.temperature_c.len() != self.patches.len() {
            return Err(invalid(
                "thermal state belongs to a different material or reference mesh",
            ));
        }
        self.evaluate(
            Some(old),
            positions_m,
            seconds,
            boundaries,
            Some(exteriors),
            None,
        )
    }

    /// Advance with internal cell energies in J and qualified exterior convection/radiation.
    /// Reference mass and source energy stay fixed while exposure uses current patch geometry.
    ///
    /// # Errors
    /// As [`Self::advance_with_heat`] and [`Self::advance_exterior`].
    pub fn advance_exterior_with_heat(
        &self,
        old: &SurfaceState,
        positions_m: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
        exteriors: &[Exterior],
        heat_j: &[Vec<f64>],
    ) -> Result<SurfaceStep, Refusal> {
        positive("surface thermal time step", seconds)?;
        if old.sha256 != self.sha256 || old.temperature_c.len() != self.patches.len() {
            return Err(invalid(
                "thermal state belongs to a different material or reference mesh",
            ));
        }
        self.evaluate(
            Some(old),
            positions_m,
            seconds,
            boundaries,
            Some(exteriors),
            Some(heat_j),
        )
    }

    fn evaluate(
        &self,
        old: Option<&SurfaceState>,
        positions: &[Vec3],
        seconds: f64,
        boundaries: &[Boundary],
        exteriors: Option<&[Exterior]>,
        heat_j: Option<&[Vec<f64>]>,
    ) -> Result<SurfaceStep, Refusal> {
        if positions.len() != self.vertex_count
            || positions.iter().any(|v| !v.is_finite())
            || boundaries.len() != self.columns.len()
            || exteriors.is_some_and(|e| e.len() != self.columns.len())
        {
            return Err(invalid(
                "thermal surface needs every vertex and one boundary/environment per column",
            ));
        }
        if heat_j.is_some_and(|heat| {
            heat.len() != self.patches.len()
                || heat.iter().zip(&self.patches).any(|(cells, patch)| {
                    cells.len() != self.columns[patch.column].cells()
                        || cells.iter().any(|q| !q.is_finite() || *q < 0.0)
                })
        }) {
            return Err(invalid(
                "surface internal heat needs one finite nonnegative energy per patch cell",
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
            supplied_heat_j: 0.0,
            balance_error_j: 0.0,
            exterior_exchange: exteriors.map(|_| SurfaceExteriorExchange {
                patches: Vec::with_capacity(self.patches.len()),
                convective_heat_w: 0.0,
                radiative_heat_w: 0.0,
                balance_residual_w: 0.0,
            }),
        };
        for (i, p) in self.patches.iter().enumerate() {
            let [a, b, c] = p.vertices.map(|v| positions[v]);
            let area = positive("deformed triangle area", 0.5 * (b - a).cross(c - a).norm())?;
            let ratio = positive("deformed/reference area ratio", area / p.reference_area_m2)?;
            let column = &self.columns[p.column];
            let boundary = boundaries[p.column];
            let heat = heat_j.map(|source| {
                result.supplied_heat_j += source[i].iter().sum::<f64>();
                source[i]
                    .iter()
                    .map(|energy| energy / p.reference_area_m2)
                    .collect::<Vec<_>>()
            });
            // Delta H_ref/(J*dt) = L(T) is exactly the column equation with dt' = J*dt.
            // R(T) remains per current area. Integrating Cp(T) against reference mass, NOT current
            // area, keeps material mass and stored heat invariant as a sheet stretches or changes pose.
            let state = if let Some(exteriors) = exteriors {
                let step = match (old, heat.as_deref()) {
                    (Some(old), Some(heat)) => column.advance_exterior_with_heat(
                        &old.temperature_c[i],
                        positive("geometry-scaled thermal time step", seconds * ratio)?,
                        boundary,
                        &exteriors[p.column],
                        heat,
                    )?,
                    (Some(old), None) => column.advance_exterior(
                        &old.temperature_c[i],
                        positive("geometry-scaled thermal time step", seconds * ratio)?,
                        boundary,
                        &exteriors[p.column],
                    )?,
                    (None, _) => column.steady_exterior(boundary, &exteriors[p.column])?,
                };
                let total = result
                    .exterior_exchange
                    .as_mut()
                    .expect("requested exchange");
                total.convective_heat_w += area * step.exchange.convective_flux_w_m2;
                total.radiative_heat_w += area * step.exchange.radiative_flux_w_m2;
                total.balance_residual_w += area * step.exchange.balance_residual_w_m2;
                total.patches.push(step.exchange);
                step.state
            } else {
                match (old, heat.as_deref()) {
                    (Some(old), Some(heat)) => column.advance_with_heat(
                        &old.temperature_c[i],
                        positive("geometry-scaled thermal time step", seconds * ratio)?,
                        boundary,
                        heat,
                    )?,
                    (Some(old), None) => column.advance(
                        &old.temperature_c[i],
                        positive("geometry-scaled thermal time step", seconds * ratio)?,
                        boundary,
                    )?,
                    (None, _) => column.steady(boundary)?,
                }
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
            result.balance_error_j = result.stored_change_j
                - seconds * (result.inside_heat_w - result.outside_heat_w)
                - result.supplied_heat_j;
        }
        if [
            result.area_m2,
            result.inside_heat_w,
            result.outside_heat_w,
            result.stored_change_j,
            result.supplied_heat_j,
            result.balance_error_j,
        ]
        .iter()
        .any(|x| !x.is_finite())
            || result.exterior_exchange.as_ref().is_some_and(|e| {
                [
                    e.convective_heat_w,
                    e.radiative_heat_w,
                    e.balance_residual_w,
                ]
                .iter()
                .any(|v| !v.is_finite())
            })
        {
            return Err(invalid("surface heat or stored energy overflow"));
        }
        Ok(result)
    }
}
