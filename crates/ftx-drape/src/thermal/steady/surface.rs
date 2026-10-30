//! Conduction-only heat fields on retained garment triangles, independent of storage readiness.
//!
//! Uses the same source receipts, triangle ownership and geometry as mechanical draping. A column
//! contains explicitly coextensive layers; nearby sheets do not imply a stack or body coverage.

use super::exterior::{Exterior, ExteriorState};
use super::{Boundary, PowerRange, SteadyConductionModel, SteadyState, invalid, positive};
use crate::{
    run::DrapeRun,
    scene::Scene,
    thermal::surface::{
        Patch,
        io::{digest, retained_materials},
        validate_topology,
    },
};
use ftx_core::{Refusal, Vec3};
use serde::Serialize;
use serde_json::{Value, json};

/// Immutable independent columns with no invented mass, heat capacity, cells or transient state.
#[derive(Clone, Debug)]
pub struct SteadySurface {
    pub(super) columns: Vec<SteadyConductionModel>,
    pub(super) patches: Vec<Patch>,
    pub(super) vertex_count: usize,
}

/// One triangle's signed heat flow; face temperatures are stored once in its assigned column.
#[derive(Clone, Debug, Serialize)]
pub struct PatchFlux {
    /// Column index in the surface snapshot and solution.
    pub column: usize,
    /// Current conducting triangle area in m², not body coverage.
    pub area_m2: f64,
    /// Heat flow in W, positive from the inside reservoir toward the outside.
    pub power_w: f64,
    /// Complete conditional material ranges on this exact current patch area.
    #[serde(flatten)]
    pub ranges: PowerRange,
}

/// Complete steady field, with no partial result on a failed column or geometric overflow.
#[derive(Clone, Debug, Serialize)]
pub struct SurfaceSolution {
    /// Exact integrated face temperatures, flux and residuals per distinct column.
    pub columns: Vec<SteadyState>,
    /// Patch order is exactly the retained solver triangle order.
    pub patches: Vec<PatchFlux>,
    /// Sum of current cloth triangle areas, not wearer skin area.
    pub area_m2: f64,
    /// Signed sum of patch powers, not an inferred whole-wearer heat loss.
    pub power_w: f64,
    /// Signed interval sum over every retained current patch; no partial bounded subtotal.
    #[serde(flatten)]
    pub ranges: PowerRange,
}

/// Exposed-face exchange over the same retained cloth areas; not a body-coverage calculation.
#[derive(Clone, Debug, Serialize)]
pub struct ExteriorSolution {
    /// Original conduction field and patch powers in unchanged retained order.
    pub steady: SurfaceSolution,
    /// Exterior temperature and signed flux contributions per material column.
    pub columns: Vec<ExteriorState>,
    /// Signed convection integrated over current cloth area, W.
    pub convective_power_w: f64,
    /// Signed ambient radiation integrated over current cloth area, W.
    pub radiative_power_w: f64,
}

impl SteadySurface {
    /// Derive disjoint body coverage from this source's current cloth geometry. The same clipping,
    /// layer-order and finite-thickness gap kernels serve transient surfaces; missing heat capacity
    /// does not disable geometric correspondence or create a fictitious transient-ready material.
    /// Normals follow body winding. A non-self-intersecting outward boundary is a caller precondition.
    ///
    /// # Errors
    /// Invalid geometry or controls, stacked source columns, ambiguous/crossing layers, missing
    /// interfaces or gap thickness, spatially reused cloth, or exceeded numerical/allocation budgets.
    pub fn project_coverage(
        &self,
        cloth: &[Vec3],
        body: &[Vec3],
        faces: &[[usize; 3]],
        request: &crate::thermal::surface::projection::Request,
    ) -> Result<crate::thermal::surface::projection::Projection, Refusal> {
        use crate::thermal::surface::projection::{ProjectionSource, project_source};
        let input = ProjectionSource {
            vertex_count: self.vertex_count,
            patches: &self.patches,
            layers: self.columns.iter().map(|c| c.layers.as_slice()).collect(),
            snapshot: self.snapshot()?,
        };
        let projection = project_source(&input, cloth, body, faces, request)?;
        super::coverage::SteadyCoverage::new(self.clone(), projection.layout.clone())?;
        Ok(projection)
    }

    /// Bind source-locked columns to the same topology contract as storage-bearing surfaces.
    /// Reference areas remain in the receipt; steady exchange uses only current areas. Neither
    /// stretch-dependent thickness nor pressure/compression-dependent resistance is inferred.
    ///
    /// # Errors
    /// Invalid/repeated triangles, unused columns, invalid areas or exceeded allocation budgets.
    pub fn new(
        columns: Vec<SteadyConductionModel>,
        vertex_count: usize,
        patches: Vec<Patch>,
    ) -> Result<Self, Refusal> {
        validate_topology(columns.len(), vertex_count, &patches)?;
        let mut layers = 0usize;
        for p in &patches {
            layers = layers
                .checked_add(columns[p.column].layers.len())
                .filter(|&n| n <= 1_000_000)
                .ok_or_else(|| invalid("steady surface exceeds one million patch layers"))?;
        }
        Ok(Self {
            columns,
            patches,
            vertex_count,
        })
    }

    /// Boundary arrays use this deterministic column order, retained in the snapshot.
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Source-complete, content-locked model; a digest checks consistency, not authenticity.
    ///
    /// # Errors
    /// A serialization failure; all numerical inputs were validated at construction.
    pub fn snapshot(&self) -> Result<Value, Refusal> {
        let mut value = json!({"schema":"ftx-steady-thermal-surface/1",
            "columns":self.columns.iter().map(SteadyConductionModel::snapshot).collect::<Vec<_>>(),
            "vertex_count":self.vertex_count,"patches":self.patches,
            "geometry_policy":"characterized fixed thickness/conditioning; resistance per current cloth area",
            "coupling":"independent columns; no inferred overlaps or body coverage",
            "storage_required":false,"comfort_claim":false,"empirical_validation_claim":false});
        value["sha256"] = json!(digest(&value)?);
        Ok(value)
    }

    /// Restore exact material/topology inputs without consulting a catalog or relaxing evidence.
    ///
    /// # Errors
    /// Unsupported fields/schema, modified observations, invalid geometry or allocation budgets.
    pub fn from_snapshot(value: &Value) -> Result<Self, Refusal> {
        let raw = value["columns"]
            .as_array()
            .ok_or_else(|| invalid("missing steady columns"))?;
        let patches = value["patches"]
            .as_array()
            .ok_or_else(|| invalid("missing steady patches"))?;
        let vertices = value["vertex_count"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| invalid("invalid steady vertex count"))?;
        if patches.is_empty()
            || patches.len() > 65_536
            || raw.is_empty()
            || raw.len() > patches.len()
            || vertices == 0
            || vertices > 196_608
        {
            return Err(invalid("steady snapshot exceeds topology budget"));
        }
        let patches: Vec<Patch> =
            serde_json::from_value(value["patches"].clone()).map_err(|e| invalid(e.to_string()))?;
        let columns = raw
            .iter()
            .map(SteadyConductionModel::from_snapshot)
            .collect::<Result<_, _>>()?;
        let surface = Self::new(columns, vertices, patches)?;
        if surface.snapshot()? != *value {
            return Err(invalid(
                "steady surface snapshot changed or has unsupported fields",
            ));
        }
        Ok(surface)
    }

    pub(super) fn areas(&self, positions: &[Vec3]) -> Result<Vec<f64>, Refusal> {
        if positions.len() != self.vertex_count || positions.iter().any(|p| !p.is_finite()) {
            return Err(invalid(
                "steady surface needs every finite vertex in metres",
            ));
        }
        self.patches
            .iter()
            .map(|p| {
                let [a, b, c] = p.vertices.map(|i| positions[i]);
                positive(
                    "current steady triangle area",
                    0.5 * (b - a).cross(c - a).norm(),
                )
            })
            .collect()
    }

    /// Solve each distinct column once, then integrate its flux over current retained triangles.
    /// Local material domains, film jumps, reversed heat flow and residuals use the exact existing
    /// Fourier solver; an absent heat capacity cannot disable this independent capability.
    ///
    /// # Errors
    /// Wrong boundary/vertex counts, invalid or collapsed triangles, domain/evidence refusals,
    /// failed equations, or nonrepresentable patch/total power. The surface is never mutated.
    pub fn solve(
        &self,
        positions: &[Vec3],
        boundaries: &[Boundary],
    ) -> Result<SurfaceSolution, Refusal> {
        if boundaries.len() != self.columns.len() {
            return Err(invalid("steady surface needs one boundary per column"));
        }
        let areas = self.areas(positions)?;
        let columns: Vec<_> = self
            .columns
            .iter()
            .zip(boundaries)
            .map(|(c, &b)| c.solve(b))
            .collect::<Result<_, _>>()?;
        self.integrate(areas, columns)
    }

    /// Couple each column to its declared environment and selected outermost textile face.
    /// Both input arrays must contain exactly one entry per frozen column. Convection/radiation
    /// powers use the same current triangle areas as the through-conduction powers.
    ///
    /// # Errors
    /// Invalid geometry/counts, missing or denied optical data, unsupported temperatures or fluxes.
    pub fn solve_exterior(
        &self,
        positions: &[Vec3],
        boundaries: &[Boundary],
        exterior: &[Exterior],
    ) -> Result<ExteriorSolution, Refusal> {
        if boundaries.len() != self.columns.len() || exterior.len() != self.columns.len() {
            return Err(invalid(
                "steady exterior needs one boundary and environment per column",
            ));
        }
        let areas = self.areas(positions)?;
        let columns: Vec<_> = self
            .columns
            .iter()
            .zip(boundaries)
            .zip(exterior)
            .map(|((c, &b), e)| c.solve_exterior(b, e))
            .collect::<Result<_, _>>()?;
        let steady = self.integrate(
            areas,
            columns.iter().map(|c| c.conduction.clone()).collect(),
        )?;
        let mut convective_power_w = 0.0;
        let mut radiative_power_w = 0.0;
        for p in &steady.patches {
            let c = &columns[p.column];
            let conv = p.area_m2 * c.convective_flux_w_m2;
            let rad = p.area_m2 * c.radiative_flux_w_m2;
            if !conv.is_finite()
                || !rad.is_finite()
                || (conv == 0.0 && c.convective_flux_w_m2 != 0.0)
                || (rad == 0.0 && c.radiative_flux_w_m2 != 0.0)
            {
                return Err(invalid("exterior patch power overflow or underflow"));
            }
            convective_power_w += conv;
            radiative_power_w += rad;
        }
        if !convective_power_w.is_finite() || !radiative_power_w.is_finite() {
            return Err(invalid("exterior surface power overflow"));
        }
        Ok(ExteriorSolution {
            steady,
            columns,
            convective_power_w,
            radiative_power_w,
        })
    }

    fn integrate(
        &self,
        areas: Vec<f64>,
        columns: Vec<SteadyState>,
    ) -> Result<SurfaceSolution, Refusal> {
        let mut patches = Vec::with_capacity(self.patches.len());
        let mut area_m2 = 0.0;
        let mut power_w = 0.0;
        let mut ranges = PowerRange::exact(0.);
        for (patch, area) in self.patches.iter().zip(areas) {
            let flux = columns[patch.column].flux_w_m2;
            let power = area * flux;
            if !power.is_finite() || (flux != 0.0 && power == 0.0) {
                return Err(invalid("steady patch power overflow or underflow"));
            }
            area_m2 += area;
            power_w += power;
            let patch_ranges = columns[patch.column].power_range(area);
            ranges.add(&patch_ranges);
            patches.push(PatchFlux {
                column: patch.column,
                area_m2: area,
                power_w: power,
                ranges: patch_ranges,
            });
        }
        if !area_m2.is_finite() || !power_w.is_finite() {
            return Err(invalid("steady surface area or power overflow"));
        }
        Ok(SurfaceSolution {
            columns,
            patches,
            area_m2,
            power_w,
            ranges,
        })
    }

    fn from_scene(scene: &Scene, material: &Value) -> Result<Self, Refusal> {
        let faces = &scene.solver.model.membranes;
        if faces.len() != scene.triangle_owner.len() {
            return Err(invalid(
                "steady topology differs from retained cloth elements",
            ));
        }
        let (records, assignment) =
            retained_materials(material, &scene.mesh.instances, &scene.triangle_owner)?;
        let columns = records
            .into_iter()
            .map(|r| SteadyConductionModel::new(vec![r], vec![]))
            .collect::<Result<_, _>>()?;
        let patches = faces
            .iter()
            .zip(assignment)
            .map(|(e, column)| Patch {
                vertices: e.v,
                reference_area_m2: e.area,
                column,
            })
            .collect();
        let surface = Self::new(columns, scene.solver.x.len(), patches)?;
        surface.areas(&scene.solver.x)?;
        Ok(surface)
    }

    /// Restore the independent steady exchange on a drape, verifying frame and material linkage.
    /// This does not accept a transient snapshot by changing its schema or inventing missing cells.
    ///
    /// # Errors
    /// Unavailable or changed frame, invalid owners, mismatched material/column assignment or mesh.
    pub fn from_drape_report(report: &Value) -> Result<(Self, Vec<Vec3>), Refusal> {
        let frame = &report["steady_thermal_surface"];
        if frame["available"] != true || frame["schema"] != "ftx-drape-steady-thermal-surface/1" {
            return Err(invalid(format!(
                "drape steady surface unavailable: {}",
                frame["refusal"]
            )));
        }
        let mut input = frame.clone();
        input
            .as_object_mut()
            .ok_or_else(|| invalid("steady frame must be an object"))?
            .remove("sha256");
        if frame["sha256"] != digest(&input)?
            || frame["material_report_sha256"] != digest(&report["material"])?
        {
            return Err(invalid("steady frame or material report hash mismatch"));
        }
        let surface = Self::from_snapshot(&frame["model"])?;
        let positions: Vec<Vec3> = serde_json::from_value(frame["positions_m"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        surface.areas(&positions)?;
        let instances: Vec<String> = serde_json::from_value(frame["instances"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        let owners: Vec<usize> = serde_json::from_value(frame["triangle_instance"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        let (records, assignment) = retained_materials(&report["material"], &instances, &owners)?;
        if assignment.len() != surface.patches.len()
            || records.len() != surface.columns.len()
            || assignment
                .iter()
                .zip(&surface.patches)
                .any(|(&id, p)| id != p.column)
            || records
                .iter()
                .zip(&surface.columns)
                .any(|(r, c)| c.snapshot()["layers"] != json!([r.frozen_selection()]))
        {
            return Err(invalid(
                "steady columns differ from retained instance material assignments",
            ));
        }
        Ok((surface, positions))
    }
}

impl DrapeRun {
    /// Compile conduction-only columns from this run's retained cloth and frozen materials.
    ///
    /// # Errors
    /// Missing transport, denied estimates, inconsistent scene/receipts or invalid topology.
    pub fn steady_thermal_surface(&self) -> Result<SteadySurface, Refusal> {
        SteadySurface::from_scene(&self.scene, &self.report["material"])
    }
}

pub(crate) fn for_drape(scene: &Scene, material: &Value) -> Value {
    let outcome = (|| {
        let surface = SteadySurface::from_scene(scene, material)?;
        let mut frame = json!({"schema":"ftx-drape-steady-thermal-surface/1","available":true,
            "model":surface.snapshot()?,"positions_m":scene.solver.x,
            "material_report_sha256":digest(material)?,
            "triangle_instance":scene.triangle_owner,"instances":scene.mesh.instances,
            "scope":"independent sheet conduction; not inferred body coverage or whole-wearer heat loss"});
        frame["sha256"] = json!(digest(&frame)?);
        Ok::<_, Refusal>(frame)
    })();
    outcome.unwrap_or_else(|error| json!({"available":false,"refusal":error}))
}
