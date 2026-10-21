//! Source-locked, conduction-only exchange over explicit disjoint body coverage.
//!
//! Reuses the transient coverage layout/allocation rules but never requires or invents storage.
//! Material transport uses current cloth area; films/interfaces use covered body area. The body is
//! a prescribed-temperature boundary, not a thermoregulating human. No overlap is inferred here.

use super::exterior::{Exterior, ExteriorState};
use super::{Boundary, SteadyConductionModel, SteadyState, surface::SteadySurface};
use crate::thermal::{
    invalid, nonnegative, positive,
    surface::{coverage::Layout, io::digest},
    temperature,
};
use ftx_core::{Refusal, Vec3};
use ftx_pattern::material_optics::Side;
use serde::Serialize;
use serde_json::{Value, json};

pub mod request;

/// Frozen single-layer source triangles and their explicit inside-to-outside coverage topology.
#[derive(Clone, Debug)]
pub struct SteadyCoverage {
    source: SteadySurface,
    layout: Layout,
    models: Vec<SteadyConductionModel>,
    covered: Vec<f64>,
    allocated: Vec<f64>,
}

/// One series path, with exact integrated Fourier face temperatures and no transient state.
#[derive(Clone, Debug, Serialize)]
pub struct RegionSolution {
    /// Stable layout region identity.
    pub id: String,
    /// Index into the declared body triangles.
    pub body_triangle: usize,
    /// Covered body area counted once, regardless of layer count, in m².
    pub body_area_m2: f64,
    /// Allocated current cloth area for each layer, in inside-to-outside order, in m².
    pub layer_area_m2: Vec<f64>,
    /// Flux and flux residual are per covered BODY area, not per layer's cloth area.
    pub conduction: SteadyState,
    /// Signed heat flow through the entire series path, in W.
    pub power_w: f64,
}

/// Covered and bare exchange at one prescribed body-boundary triangle.
#[derive(Clone, Debug, Serialize)]
pub struct BodySolution {
    /// Entire current body triangle area, in m².
    pub area_m2: f64,
    /// Covered area counted once, in m².
    pub covered_area_m2: f64,
    /// Heat through the uncovered fraction's declared films, in W.
    pub bare_power_w: f64,
    /// Sum of covered and bare power, positive inside-to-outside, in W.
    pub power_w: f64,
}

/// Atomic steady solution. This is not total wearer heat loss or a warmth/safety rating.
#[derive(Clone, Debug, Serialize)]
pub struct CoverageSolution {
    /// Covered paths in layout order.
    pub regions: Vec<RegionSolution>,
    /// Body fields in layout triangle order, including exposed fractions.
    pub body: Vec<BodySolution>,
    /// Total prescribed body boundary area, in m².
    pub body_area_m2: f64,
    /// Covered part of that area, counted once, in m².
    pub covered_area_m2: f64,
    /// Bare-path power already included in `power_w`, in W.
    pub bare_power_w: f64,
    /// Signed heat crossing both boundaries at steady state, in W.
    pub power_w: f64,
    /// Unallocated reference cloth area, explicitly excluded from this calculation, in m².
    pub unassigned_cloth_reference_area_m2: f64,
}

/// Covered textiles exchange with the environment only at each path's outermost physical layer.
/// Bare body paths retain separately declared films; textile optical properties never become skin.
#[derive(Clone, Debug, Serialize)]
pub struct ExteriorCoverageSolution {
    /// Body/region conduction and explicit bare-path powers in the original layout order.
    pub steady: CoverageSolution,
    /// Per-region outer face temperatures and fluxes per current outer cloth area.
    pub regions: Vec<ExteriorState>,
    /// Covered-cloth convection, W; excludes the separately reported bare-body films.
    pub convective_power_w: f64,
    /// Covered-cloth radiation, W; excludes bare body and interlayer exchange.
    pub radiative_power_w: f64,
    /// Sum of local covered-path through-power minus exterior exchange, W.
    pub balance_residual_w: f64,
}

fn product(a: f64, b: f64) -> Result<f64, Refusal> {
    let value = a * b;
    if !value.is_finite() || (a != 0.0 && b != 0.0 && value == 0.0) {
        return Err(invalid(
            "steady coverage area or power is not representable",
        ));
    }
    Ok(value)
}

impl SteadyCoverage {
    /// Bind existing material selections to the shared fractional coverage layout. Source triangles
    /// must each represent one physical layer; overlapping stacks may not double-count a sheet.
    /// `cells_per_layer` remains a validated layout compatibility field; no cells are allocated.
    ///
    /// # Errors
    /// Invalid allocation, physical layer order, interfaces, source topology or transport data.
    pub fn new(source: SteadySurface, layout: Layout) -> Result<Self, Refusal> {
        let (covered, allocated) = layout.allocations(source.patches.len())?;
        if source.columns.iter().any(|c| c.layers.len() != 1) {
            return Err(invalid(
                "steady coverage requires single-layer source triangles",
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
                SteadyConductionModel::new(layers, region.interface_resistance_m2_k_w.clone())
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            source,
            layout,
            models,
            covered,
            allocated,
        })
    }

    /// Complete frozen material/topology receipt; poses and boundary conditions are separate inputs.
    ///
    /// # Errors
    /// Serialization failure; every numeric observation was validated during resolution.
    pub fn snapshot(&self) -> Result<Value, Refusal> {
        let mut value = json!({"schema":"ftx-steady-thermal-coverage/1",
            "source":self.source.snapshot()?,"layout":self.layout,
            "geometry_policy":"transport per current cloth area; interfaces and films per covered body area",
            "bare_policy":"unallocated body fraction exchanges through declared films; no tissue storage",
            "unmapped_cloth_policy":"excluded and reported",
            "storage_required":false,"comfort_claim":false,"empirical_validation_claim":false});
        value["sha256"] = json!(digest(&value)?);
        Ok(value)
    }

    /// Replay without a catalog lookup, evidence relaxation or conversion to a storage-ready record.
    ///
    /// # Errors
    /// Modified, unsupported, incomplete or invalid snapshot, including unknown fields.
    pub fn from_snapshot(value: &Value) -> Result<Self, Refusal> {
        let source = SteadySurface::from_snapshot(&value["source"])?;
        let layout =
            serde_json::from_value(value["layout"].clone()).map_err(|e| invalid(e.to_string()))?;
        let assembly = Self::new(source, layout)?;
        if assembly.snapshot()? != *value {
            return Err(invalid("steady coverage snapshot mismatch"));
        }
        Ok(assembly)
    }

    /// Solve independent covered series paths and bare parallel paths on the supplied current pose.
    /// Each material's own characterized temperature domain and original Fourier equation is checked.
    /// Different layers may have different current areas; reference area never becomes thermal mass.
    ///
    /// # Errors
    /// Invalid poses/boundaries, zero bare-path film resistance, unrepresentable geometry/power,
    /// out-of-domain material faces or failed equations. No partial fields are returned.
    pub fn solve(
        &self,
        cloth: &[Vec3],
        body: &[Vec3],
        boundaries: &[Boundary],
    ) -> Result<CoverageSolution, Refusal> {
        Ok(self.solve_paths(cloth, body, boundaries, None)?.0)
    }

    /// Apply qualified exterior exchange to each covered series path's outermost material, using
    /// its allocated CURRENT cloth area. Environment entries are indexed by body triangle, not
    /// source material column. Pass an empty bare array only when the body is entirely covered.
    /// Otherwise every body triangle needs a separately declared bare-path film boundary whose
    /// inside temperature agrees with that same prescribed body reservoir.
    ///
    /// # Errors
    /// Invalid geometry/counts, missing bare conditions, incompatible reservoir temperatures,
    /// unqualified optical data or failed conduction/exterior equations. No partial results.
    pub fn solve_exterior(
        &self,
        cloth: &[Vec3],
        body: &[Vec3],
        boundaries: &[Boundary],
        exterior: &[Exterior],
        bare_boundaries: &[Boundary],
    ) -> Result<ExteriorCoverageSolution, Refusal> {
        let count = self.layout.body_triangles.len();
        if exterior.len() != count
            || (!bare_boundaries.is_empty() && bare_boundaries.len() != count)
            || (bare_boundaries.is_empty() && self.covered.iter().any(|&f| f < 1.0))
        {
            return Err(invalid(
                "coverage exterior needs one environment per body triangle and explicit bare films for uncovered area",
            ));
        }
        let (steady, regions) =
            self.solve_paths(cloth, body, boundaries, Some((exterior, bare_boundaries)))?;
        let mut convective_power_w = 0.0;
        let mut radiative_power_w = 0.0;
        let mut balance_residual_w = 0.0;
        for (r, e) in steady.regions.iter().zip(&regions) {
            let outer_area = *r.layer_area_m2.last().expect("nonempty region");
            let conv = product(outer_area, e.convective_flux_w_m2)?;
            let rad = product(outer_area, e.radiative_flux_w_m2)?;
            let error = (r.power_w - conv) - rad;
            if !error.is_finite()
                || error.abs() > 1e-9 * (r.power_w.abs().max(conv.abs()).max(rad.abs()))
            {
                return Err(invalid("coverage exterior power balance failed"));
            }
            convective_power_w += conv;
            radiative_power_w += rad;
            balance_residual_w += error;
        }
        if [convective_power_w, radiative_power_w, balance_residual_w]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(invalid("coverage exterior power accumulation overflow"));
        }
        Ok(ExteriorCoverageSolution {
            steady,
            regions,
            convective_power_w,
            radiative_power_w,
            balance_residual_w,
        })
    }

    fn solve_paths(
        &self,
        cloth: &[Vec3],
        body: &[Vec3],
        boundaries: &[Boundary],
        exterior: Option<(&[Exterior], &[Boundary])>,
    ) -> Result<(CoverageSolution, Vec<ExteriorState>), Refusal> {
        if body.len() != self.layout.body_vertex_count
            || body.iter().any(|p| !p.is_finite())
            || boundaries.len() != self.layout.body_triangles.len()
        {
            return Err(invalid(
                "steady coverage needs every finite body vertex and one boundary per body triangle",
            ));
        }
        let cloth_areas = self.source.areas(cloth)?;
        let mut fields = Vec::with_capacity(boundaries.len());
        for (i, vertices) in self.layout.body_triangles.iter().enumerate() {
            let [a, b, c] = vertices.map(|v| body[v]);
            let area = positive(
                "steady coverage body area",
                0.5 * (b - a).cross(c - a).norm(),
            )?;
            let b = boundaries[i];
            temperature(b.inside_c)?;
            temperature(b.outside_c)?;
            nonnegative("inside film resistance", b.inside_resistance)?;
            nonnegative("outside film resistance", b.outside_resistance)?;
            let bare = if self.covered[i] < 1.0 {
                let b = if let Some((_, bare)) = exterior {
                    let explicit = bare[i];
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
                let delta = b.inside_c - b.outside_c;
                let flux = delta / r;
                if !flux.is_finite() || (delta != 0.0 && flux == 0.0) {
                    return Err(invalid("steady bare-path flux is not representable"));
                }
                product(product(area, 1.0 - self.covered[i])?, flux)?
            } else {
                0.0
            };
            fields.push(BodySolution {
                area_m2: area,
                covered_area_m2: product(area, self.covered[i])?,
                bare_power_w: bare,
                power_w: bare,
            });
        }
        let mut regions = Vec::with_capacity(self.models.len());
        let mut exterior_states = Vec::new();
        for (region, model) in self.layout.regions.iter().zip(&self.models) {
            let area = product(fields[region.body_triangle].area_m2, region.body_fraction)?;
            let layer_area: Vec<_> = region
                .layers
                .iter()
                .map(|p| product(cloth_areas[p.cloth_triangle], p.fraction))
                .collect::<Result<_, _>>()?;
            let factors = layer_area.iter().map(|a| area / a).collect();
            let model = model.with_area_factors(factors)?;
            let b = boundaries[region.body_triangle];
            let conduction = if let Some((environments, _)) = exterior {
                let environment = &environments[region.body_triangle];
                let side = if environment
                    .radiation
                    .as_ref()
                    .is_some_and(|r| r.automatic_material_face)
                {
                    let outer = region.layers.last().expect("nonempty region");
                    crate::thermal::surface::coverage::material_side(
                        self.source.patches[outer.cloth_triangle].vertices,
                        cloth,
                        self.layout.body_triangles[region.body_triangle],
                        body,
                    )
                    .ok_or_else(|| {
                        invalid("automatic optical face needs an unambiguous cloth/body normal")
                    })?
                } else {
                    Side::Positive
                };
                let state = model.solve_exterior_on_side(b, environment, side)?;
                let conduction = state.conduction.clone();
                exterior_states.push(state);
                conduction
            } else {
                model.solve(b)?
            };
            let power = product(area, conduction.flux_w_m2)?;
            fields[region.body_triangle].power_w += power;
            regions.push(RegionSolution {
                id: region.id.clone(),
                body_triangle: region.body_triangle,
                body_area_m2: area,
                layer_area_m2: layer_area,
                conduction,
                power_w: power,
            });
        }
        let mut unassigned = 0.0;
        for (p, f) in self.source.patches.iter().zip(&self.allocated) {
            unassigned += product(p.reference_area_m2, 1.0 - f)?;
        }
        let result = CoverageSolution {
            regions,
            body_area_m2: fields.iter().map(|f| f.area_m2).sum(),
            covered_area_m2: fields.iter().map(|f| f.covered_area_m2).sum(),
            bare_power_w: fields.iter().map(|f| f.bare_power_w).sum(),
            power_w: fields.iter().map(|f| f.power_w).sum(),
            unassigned_cloth_reference_area_m2: unassigned,
            body: fields,
        };
        if [
            result.body_area_m2,
            result.covered_area_m2,
            result.bare_power_w,
            result.power_w,
            unassigned,
        ]
        .iter()
        .any(|n| !n.is_finite())
            || result.body.iter().any(|f| !f.power_w.is_finite())
        {
            return Err(invalid("steady coverage accumulation overflow"));
        }
        Ok((result, exterior_states))
    }
}
