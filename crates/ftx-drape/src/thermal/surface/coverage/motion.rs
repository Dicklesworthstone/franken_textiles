//! Material-coordinate transfer between geometric coverage partitions, with no elapsed heat step.
//!
//! Piecewise-constant donor enthalpy is integrated over polygon AND thickness-cell intersections.
//! Average temperature is not conservative when Cp varies with temperature. This first-order remap
//! mixes unresolved gradients; conservation and boundedness are not an accuracy/garment claim.
//! Uncovered cloth retains one canonical through-thickness profile per source triangle, so material
//! can leave/re-enter body coverage without a temperature reset. Physical heat exchange on those
//! exposed faces is opt-in and requires explicit boundary conditions; no ambient state is inferred.

use ftx_core::{
    Refusal, Vec2, Vec3,
    exact::{Sign, orient2d},
    receipt::sha256_hex,
};
use ftx_pattern::material_optics::Side;
use ftx_pattern::material_thermal::ThermalLaw;
use serde::Serialize;
use serde_json::{Value, json};

use super::{
    Boundary, Coverage, CoverageState, CoverageStep, ExteriorConditions, Surface, invalid,
    material_side,
};
use crate::thermal::{
    DryThermalModel,
    exterior::ExteriorFlux,
    steady::exterior::Exterior,
    surface::projection::{self, Projection, Request},
};
use projection::clip::{Work, area, bounds_overlap, intersection};

// The supplied model has reference-area-scaled capacities, so both arguments and returned
// requested/realized energies are joules. Inversion and source-domain checks stay in one kernel.
fn preload(
    model: &DryThermalModel,
    old: &[f64],
    heat: &[f64],
) -> Result<(Vec<f64>, f64, f64), Refusal> {
    let Some((heated, requested)) = model.prepare_heat(old, heat)? else {
        return Ok((old.to_vec(), 0., 0.));
    };
    let realized = old
        .iter()
        .zip(&heated)
        .enumerate()
        .map(|(cell, (&before, &after))| model.cell_heat(cell, before, after))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum::<f64>();
    if !realized.is_finite() {
        return Err(invalid("coverage internal enthalpy overflows"));
    }
    Ok((heated, requested, realized))
}

/// Canonical inside/outside faces of uncovered cloth, in retained source-triangle order.
/// Optional environments replace the outside films only on currently exposed fractions.
/// Automatic optical mode selects the material's source-bound positive face. The other face retains
/// its explicit inside film; no nearest-body rule chooses an ambient side for detached cloth.
#[derive(Clone, Copy, Debug)]
pub struct ExposedConditions<'a> {
    /// Exactly one boundary per retained source cloth triangle; no implicit broadcast.
    pub boundaries: &'a [Boundary],
    /// Exactly one qualified environment per source triangle when present. Hidden fractions do
    /// not acquire an exterior surface or require an unrelated lining's optical observations.
    pub environments: Option<&'a [Exterior]>,
}

/// Exterior exchange on one active uncovered source-triangle fraction.
#[derive(Clone, Debug, Serialize)]
pub struct ExposedTriangleExteriorExchange {
    /// Retained source triangle index, independent of current body-region order.
    pub cloth_triangle: usize,
    /// Current area of this triangle's uncovered fraction, in m².
    pub current_area_m2: f64,
    /// Signed fluxes per current exposed cloth area and the solved canonical outside face.
    pub flux: ExteriorFlux,
    /// Signed convection leaving this exposed fraction, W.
    pub convective_heat_w: f64,
    /// Signed radiation leaving this exposed fraction, W.
    pub radiative_heat_w: f64,
    /// Conductive outward power minus convection and radiation, W.
    pub balance_residual_w: f64,
}

/// Only uncovered-cloth exterior terms; covered regions remain in [CoverageStep].
#[derive(Clone, Debug, Serialize)]
pub struct ExposedExteriorExchange {
    /// Active uncovered fractions in ascending retained source-triangle order.
    pub triangles: Vec<ExposedTriangleExteriorExchange>,
    /// Signed convection over the active uncovered material, W.
    pub convective_heat_w: f64,
    /// Signed radiation over the active uncovered material, W.
    pub radiative_heat_w: f64,
    /// Sum of local exterior equation residuals in W; separate from stored-energy error in J.
    pub balance_residual_w: f64,
}

/// One derived geometric partition and its frozen poses. No mutable projection enters the remapper.
#[derive(Clone, Debug)]
pub struct Frame {
    coverage: Coverage,
    exposed_models: Vec<crate::thermal::DryThermalModel>,
    projection: Projection,
    request: Request,
    cloth: Vec<Vec3>,
    body: Vec<Vec3>,
    // Temperature cell order in a coverage column is body-outward. This flag connects it to the
    // persistent oriented cloth triangle, so approaching the other side reverses the cell index.
    reverse: Vec<Vec<bool>>,
    sha256: String,
}

/// Temperatures bound to both a coverage assembly AND the geometry that located its subpolygons.
#[derive(Clone, Debug)]
pub struct State {
    frame_sha256: String,
    inner: CoverageState,
    // Canonical source-column inside-to-outside order. Empty only when a patch is fully covered.
    exposed: Vec<Vec<f64>>,
}
impl State {
    /// Region, then inside-to-outside thermal-cell order in the owning frame.
    #[must_use]
    pub fn temperature_c(&self) -> &[Vec<f64>] {
        self.inner.temperature_c()
    }

    /// Uncovered-cloth temperatures in retained source-triangle order. Empty rows are fully covered.
    #[must_use]
    pub fn exposed_temperature_c(&self) -> &[Vec<f64>] {
        &self.exposed
    }
}

/// A physical heat step, separate from an instantaneous repartition/remap.
#[derive(Clone, Debug)]
pub struct Step {
    frame_sha256: String,
    heat: CoverageStep,
    exposed: Vec<Vec<f64>>,
    exposed_inside_heat_w: f64,
    exposed_outside_heat_w: f64,
    exposed_stored_change_j: f64,
    exposed_balance_error_j: f64,
    exposed_exterior_exchange: Option<ExposedExteriorExchange>,
    internal_heat: Option<InternalHeat>,
}

/// A prescribed source apportioned over the complete retained material domain, in joules.
/// Covered and exposed fractions are disjoint; bare body receives no internal source.
#[derive(Clone, Debug, Serialize)]
pub struct InternalHeat {
    /// Exact sum of requested source-triangle energies, independent of current area or coverage.
    pub supplied_heat_j: f64,
    /// Sum allocated to material portions in covered series columns.
    pub covered_heat_j: f64,
    /// Sum allocated to the remaining, optionally adiabatic exposed cloth.
    pub exposed_heat_j: f64,
    /// Covered plus exposed allocation minus supplied energy; retained in the overall residual.
    pub allocation_residual_j: f64,
}
impl Step {
    /// Present after a nonzero source-bearing step; omitted steps retain the original output.
    #[must_use]
    pub fn internal_heat(&self) -> Option<&InternalHeat> {
        self.internal_heat.as_ref()
    }

    /// Requested internal energy over all source material, excluding boundary exchange, J.
    #[must_use]
    pub fn supplied_heat_j(&self) -> f64 {
        self.internal_heat
            .as_ref()
            .map_or(0., |q| q.supplied_heat_j)
    }
    /// Existing coverage fields, including boundary heat and reference-mass storage change.
    #[must_use]
    pub fn heat(&self) -> &CoverageStep {
        &self.heat
    }
    /// Retain the temperature state for the next physical step or geometric remap.
    #[must_use]
    pub fn into_state(self) -> State {
        State {
            frame_sha256: self.frame_sha256,
            inner: self.heat.state,
            exposed: self.exposed,
        }
    }

    /// Heat entering uncovered cloth faces only, W. Covered/body paths are in [Self::heat].
    #[must_use]
    pub fn exposed_inside_heat_w(&self) -> f64 {
        self.exposed_inside_heat_w
    }

    /// Updated uncovered-cloth temperature profiles in source-triangle order.
    #[must_use]
    pub fn exposed_temperature_c(&self) -> &[Vec<f64>] {
        &self.exposed
    }

    /// Heat leaving uncovered cloth faces only, W.
    #[must_use]
    pub fn exposed_outside_heat_w(&self) -> f64 {
        self.exposed_outside_heat_w
    }

    /// Sensible-heat change of uncovered reference material only, J.
    #[must_use]
    pub fn exposed_stored_change_j(&self) -> f64 {
        self.exposed_stored_change_j
    }

    /// Heat entering all modeled textile storage paths, W. Bare-body heat is retained in the
    /// covered report's boundary totals but cancels from [Self::net_heat_w].
    #[must_use]
    pub fn inside_heat_w(&self) -> f64 {
        self.heat.inside_heat_w + self.exposed_inside_heat_w
    }

    /// Heat leaving all modeled textile storage paths, W.
    #[must_use]
    pub fn outside_heat_w(&self) -> f64 {
        self.heat.outside_heat_w + self.exposed_outside_heat_w
    }

    /// Net heat entering covered plus uncovered textile storage, W.
    #[must_use]
    pub fn net_heat_w(&self) -> f64 {
        self.heat.net_heat_w + self.exposed_inside_heat_w - self.exposed_outside_heat_w
    }

    /// Exposed storage minus its integrated boundary heat, J.
    #[must_use]
    pub fn exposed_balance_error_j(&self) -> f64 {
        self.exposed_balance_error_j
    }

    /// Per-active-source-triangle exterior fields, absent for adiabatic or film-only exposed cloth.
    #[must_use]
    pub fn exposed_exterior_exchange(&self) -> Option<&ExposedExteriorExchange> {
        self.exposed_exterior_exchange.as_ref()
    }

    fn exterior_totals(&self) -> Option<[f64; 3]> {
        let covered = self.heat.exterior_exchange.as_ref();
        let exposed = self.exposed_exterior_exchange.as_ref();
        if covered.is_none() && exposed.is_none() {
            return None;
        }
        Some([
            covered.map_or(0., |e| e.convective_heat_w)
                + exposed.map_or(0., |e| e.convective_heat_w),
            covered.map_or(0., |e| e.radiative_heat_w) + exposed.map_or(0., |e| e.radiative_heat_w),
            covered.map_or(0., |e| e.balance_residual_w)
                + exposed.map_or(0., |e| e.balance_residual_w),
        ])
    }

    /// Signed convection over the covered and exposed textile paths that opted into exterior
    /// exchange, W. Bare-body films and textile paths using ordinary films are not included.
    #[must_use]
    pub fn exterior_convective_heat_w(&self) -> Option<f64> {
        self.exterior_totals().map(|values| values[0])
    }

    /// Signed radiation over all opted-in textile exterior paths, W; absent when none requested.
    #[must_use]
    pub fn exterior_radiative_heat_w(&self) -> Option<f64> {
        self.exterior_totals().map(|values| values[1])
    }

    /// Sum of opted-in exterior equation residuals, W; separate from physical storage error in J.
    #[must_use]
    pub fn exterior_balance_residual_w(&self) -> Option<f64> {
        self.exterior_totals().map(|values| values[2])
    }

    /// Covered plus uncovered material sensible-heat change, J.
    #[must_use]
    pub fn stored_change_j(&self) -> f64 {
        self.heat.stored_change_j + self.exposed_stored_change_j
    }

    /// Covered plus uncovered material energy-balance residual, J.
    #[must_use]
    pub fn balance_error_j(&self) -> f64 {
        self.heat.balance_error_j
            + self.exposed_balance_error_j
            + self
                .internal_heat
                .as_ref()
                .map_or(0., |q| q.allocation_residual_j)
    }
}

/// Numerical accounting for an instantaneous remap; this is not heat supplied by the environment.
#[derive(Clone, Debug, Serialize)]
pub struct Balance {
    /// Source frame identity.
    pub from_sha256: String,
    /// Destination frame identity.
    pub to_sha256: String,
    /// Conserved complete material domain, in reference m², counted once per physical cloth triangle.
    pub reference_area_m2: f64,
    /// Largest summed donor/destination allocation residual, in reference m².
    pub allocation_residual_m2: f64,
    /// Summed absolute reference-area changes when convexifying roundoff-scale inward corners.
    /// Original projection receipts are retained unchanged; genuine nonconvexity is refused.
    pub polygon_roundoff_m2: f64,
    /// One enthalpy zero per source triangle (minimum donor temperature), or None when unassigned.
    pub reference_temperature_c: Vec<Option<f64>>,
    /// Sensible heat above those explicitly retained zeroes, before the remap, J.
    pub before_j: f64,
    /// Sensible heat above the SAME zeroes, after the remap, J.
    pub after_j: f64,
    /// Signed after-minus-before; never added to physical boundary heat.
    pub residual_j: f64,
    /// Sum of absolute per-cloth-triangle energy residuals; opposite errors cannot cancel.
    pub absolute_residual_j: f64,
    /// Bounded polygon, thickness-cell and thermal-law evaluation work.
    pub operations: usize,
}

/// New mapped state and its conservative-transfer accounting. A failure returns neither.
#[derive(Clone, Debug)]
pub struct Transfer {
    /// State belonging to the destination frame, ready for [`Frame::advance`].
    pub state: State,
    /// Independent remap energy and material-area balance.
    pub balance: Balance,
}

impl Frame {
    /// Derive a coverage partition from actual geometry using the existing projection closure.
    /// Material topology/reference areas remain immutable; body triangulation may change per frame.
    ///
    /// # Errors
    /// Every existing projection/coverage refusal, including ambiguity and geometry/work budgets.
    pub fn new(
        source: Surface,
        cloth: Vec<Vec3>,
        body: Vec<Vec3>,
        body_triangles: &[[usize; 3]],
        request: Request,
    ) -> Result<Self, Refusal> {
        let projection = projection::project(&source, &cloth, &body, body_triangles, &request)?;
        let mut reverse = Vec::new();
        for region in &projection.layout.regions {
            reverse.push(
                region
                    .layers
                    .iter()
                    .map(|p| {
                        material_side(
                            source.patches[p.cloth_triangle].vertices,
                            &cloth,
                            body_triangles[region.body_triangle],
                            &body,
                        )
                        .map(|side| side == Side::Negative)
                        .ok_or_else(|| invalid("unresolved material-normal orientation in remap"))
                    })
                    .collect::<Result<Vec<_>, Refusal>>()?,
            );
        }
        let coverage = Coverage::new(source, projection.layout.clone())?;
        let exposed_models = coverage
            .source
            .patches
            .iter()
            .map(|patch| {
                let column = &coverage.source.columns[patch.column];
                crate::thermal::DryThermalModel::new(
                    vec![column.layers[0].clone()],
                    vec![],
                    coverage.layout.cells_per_layer,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let sha256 = sha256_hex(
            &serde_json::to_vec(&(
                "ftx-thermal-motion-frame/1",
                &coverage.sha256,
                &projection.input_sha256,
            ))
            .map_err(|e| invalid(e.to_string()))?,
        );
        Ok(Self {
            coverage,
            exposed_models,
            projection,
            request,
            cloth,
            body,
            reverse,
            sha256,
        })
    }

    /// Immutable derived geometry, allocations, numerical resolution and input identity.
    #[must_use]
    pub fn projection(&self) -> &Projection {
        &self.projection
    }

    /// Frame identity, including source observations, geometry and request.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    fn coverage_orientation(&self) -> Vec<Vec<Option<Side>>> {
        self.reverse
            .iter()
            .map(|row| {
                row.iter()
                    .map(|&reverse| {
                        Some(if reverse {
                            Side::Negative
                        } else {
                            Side::Positive
                        })
                    })
                    .collect()
            })
            .collect()
    }

    /// Full frozen input receipt. Source geometry validity remains the projection's caller contract.
    #[must_use]
    pub fn snapshot(&self) -> Value {
        json!({"schema":"ftx-thermal-motion-frame/1", "sha256":self.sha256,
            "source":self.coverage.source.snapshot(), "cloth_positions_m":self.cloth,
            "body_positions_m":self.body,"request":self.request,"projection":self.projection,
            "remap":"first-order material-polygon and thickness-cell enthalpy intersections",
            "domain":"same covered spatial cloth portions; no cloth remeshing or material substitution"})
    }

    /// Restore geometry, material observations and the derived spatial partition without a catalog
    /// lookup. Recompute correspondence rather than trusting saved polygons or a verification flag.
    ///
    /// # Errors
    /// Changed/unsupported snapshot content or any source, projection or coverage refusal.
    pub fn from_snapshot(value: &Value) -> Result<Self, Refusal> {
        if value["schema"] != "ftx-thermal-motion-frame/1" {
            return Err(invalid("unknown thermal motion frame schema"));
        }
        for (array, limit) in [
            (&value["cloth_positions_m"], 196_608),
            (&value["body_positions_m"], 196_608),
            (&value["projection"]["layout"]["body_triangles"], 65_536),
        ] {
            if array
                .as_array()
                .is_none_or(|a| a.is_empty() || a.len() > limit)
            {
                return Err(invalid("thermal motion snapshot exceeds geometry budget"));
            }
        }
        let source = Surface::from_snapshot(&value["source"])?;
        let decode = |e| invalid(format!("thermal motion frame: {e}"));
        let cloth = serde_json::from_value(value["cloth_positions_m"].clone()).map_err(decode)?;
        let body = serde_json::from_value(value["body_positions_m"].clone()).map_err(decode)?;
        let triangles: Vec<[usize; 3]> =
            serde_json::from_value(value["projection"]["layout"]["body_triangles"].clone())
                .map_err(decode)?;
        let request = serde_json::from_value(value["request"].clone()).map_err(decode)?;
        let frame = Self::new(source, cloth, body, &triangles, request)?;
        if frame.snapshot() != *value {
            return Err(invalid(
                "thermal motion frame differs from recomputed geometry/material inputs",
            ));
        }
        Ok(frame)
    }

    /// Serialize a resumable material-temperature state with its complete immutable frame. The
    /// digest detects content changes; it is not authentication or proof of a simulation's history.
    /// No time elapses and no temperatures are recomputed, mixed or initialized by saving.
    ///
    /// # Errors
    /// State from another frame, invalid temperature cells, or serialization failure.
    pub fn checkpoint(&self, state: &State) -> Result<Value, Refusal> {
        self.check(state)?;
        let mut value = json!({"schema":"ftx-thermal-motion-state/1",
            "frame":self.snapshot(),"temperature_c":state.temperature_c(),
            "exposed_temperature_c":state.exposed_temperature_c()});
        let hash = sha256_hex(&serde_json::to_vec(&value).map_err(|e| invalid(e.to_string()))?);
        value["sha256"] = json!(hash);
        Ok(value)
    }

    /// Restore a saved state and rebuild its exact frame, including source curves and geometric
    /// correspondence. A material edit cannot reuse these temperatures without an explicit model.
    ///
    /// # Errors
    /// Unsupported fields/schema, tampered digest/geometry, invalid cell shape or temperature domain.
    pub fn from_checkpoint(value: &Value) -> Result<(Self, State), Refusal> {
        if value["schema"] != "ftx-thermal-motion-state/1" {
            return Err(invalid("unknown thermal motion state schema"));
        }
        let frame = Self::from_snapshot(&value["frame"])?;
        let rows = value["temperature_c"]
            .as_array()
            .ok_or_else(|| invalid("thermal motion state needs temperature rows"))?;
        if rows.len() != frame.coverage.models.len()
            || rows
                .iter()
                .zip(&frame.coverage.models)
                .any(|(row, model)| row.as_array().is_none_or(|r| r.len() != model.cells()))
        {
            return Err(invalid(
                "thermal motion state has incorrect cell dimensions",
            ));
        }
        let legacy = value.get("exposed_temperature_c").is_none();
        let exposed: Vec<Vec<f64>> = if legacy {
            if frame.has_exposed_cloth() {
                return Err(invalid(
                    "legacy thermal motion checkpoint has no uncovered-cloth temperature state",
                ));
            }
            vec![Vec::new(); frame.coverage.source.patches.len()]
        } else {
            serde_json::from_value(value["exposed_temperature_c"].clone())
                .map_err(|e| invalid(format!("exposed thermal motion temperatures: {e}")))?
        };
        let state = State {
            frame_sha256: frame.sha256.clone(),
            inner: CoverageState {
                sha256: frame.coverage.sha256.clone(),
                temperatures: serde_json::from_value(value["temperature_c"].clone())
                    .map_err(|e| invalid(format!("thermal motion temperatures: {e}")))?,
                orientation: Some(frame.coverage_orientation()),
            },
            exposed,
        };
        let expected = if legacy {
            let mut expected = json!({"schema":"ftx-thermal-motion-state/1",
                "frame":frame.snapshot(),"temperature_c":state.temperature_c()});
            expected["sha256"] = json!(sha256_hex(
                &serde_json::to_vec(&expected).map_err(|e| invalid(e.to_string()))?
            ));
            expected
        } else {
            frame.checkpoint(&state)?
        };
        if expected != *value {
            return Err(invalid("thermal motion state digest or content mismatch"));
        }
        Ok((frame, state))
    }

    /// Total temperature cells for budgeting, including currently uncovered cloth state.
    #[must_use]
    pub fn cells(&self) -> usize {
        self.coverage.cells()
            + self
                .exposed_fractions()
                .into_iter()
                .filter(|f| *f > 0.)
                .count()
                * self.coverage.layout.cells_per_layer
    }

    /// Required boundary-array length.
    #[must_use]
    pub fn body_triangle_count(&self) -> usize {
        self.coverage.body_triangle_count()
    }

    /// Number of retained source cloth triangles; exposed-boundary arrays follow this order.
    #[must_use]
    pub fn cloth_triangle_count(&self) -> usize {
        self.coverage.source.patches.len()
    }

    /// Whether this frame contains any cloth outside mapped body coverage.
    #[must_use]
    pub fn has_exposed_cloth(&self) -> bool {
        self.exposed_fractions().into_iter().any(|f| f > 0.)
    }

    /// Start a new simulation at an explicitly supplied temperature; never called implicitly on remap.
    ///
    /// # Errors
    /// Invalid or uncharacterized temperature.
    pub fn initial(&self, t: f64) -> Result<State, Refusal> {
        let exposed = self
            .exposed_fractions()
            .into_iter()
            .zip(&self.exposed_models)
            .map(|(fraction, model)| {
                if fraction > 0. {
                    model.check_temperature(t)?;
                    Ok(vec![t; model.cells()])
                } else {
                    Ok(Vec::new())
                }
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        Ok(State {
            frame_sha256: self.sha256.clone(),
            inner: self.coverage.initial(t)?,
            exposed,
        })
    }

    fn check(&self, state: &State) -> Result<(), Refusal> {
        if state.frame_sha256 != self.sha256 || state.inner.sha256 != self.coverage.sha256 {
            return Err(invalid(
                "thermal state belongs to another mapped geometry; remap explicitly",
            ));
        }
        if state.inner.temperatures.len() != self.coverage.models.len() {
            return Err(invalid("thermal motion state has incorrect region count"));
        }
        for (row, model) in state.inner.temperatures.iter().zip(&self.coverage.models) {
            if row.len() != model.cells() {
                return Err(invalid("thermal motion state has incorrect cell count"));
            }
            for &t in row {
                model.check_temperature(t)?;
            }
        }
        let fractions = self.exposed_fractions();
        if state.exposed.len() != fractions.len() {
            return Err(invalid(
                "thermal motion state has incorrect exposed-triangle count",
            ));
        }
        for (((row, fraction), model), patch) in state
            .exposed
            .iter()
            .zip(fractions)
            .zip(&self.exposed_models)
            .zip(&self.coverage.source.patches)
        {
            let expected = if fraction > 0. { model.cells() } else { 0 };
            if row.len() != expected {
                return Err(invalid(format!(
                    "exposed thermal state for cloth triangle {} has incorrect cell count",
                    patch.vertices[0]
                )));
            }
            for &t in row {
                model.check_temperature(t)?;
            }
        }
        Ok(())
    }

    /// Solve steady temperatures on this geometry, for initialization/verification when appropriate.
    ///
    /// # Errors
    /// Invalid boundaries or failed existing thermal solve.
    pub fn steady(&self, boundaries: &[Boundary]) -> Result<Step, Refusal> {
        if self.has_exposed_cloth() {
            return Err(invalid(
                "steady thermal motion with exposed cloth requires explicit exposed-face boundaries",
            ));
        }
        Ok(Step {
            frame_sha256: self.sha256.clone(),
            heat: self.coverage.steady(&self.cloth, &self.body, boundaries)?,
            exposed: vec![Vec::new(); self.coverage.source.patches.len()],
            exposed_inside_heat_w: 0.,
            exposed_outside_heat_w: 0.,
            exposed_stored_change_j: 0.,
            exposed_balance_error_j: 0.,
            exposed_exterior_exchange: None,
            internal_heat: None,
        })
    }

    /// Advance on the frozen geometry for a declared duration. Remapping itself advances no time.
    ///
    /// # Errors
    /// State from a different geometric frame, invalid time/boundaries or failed thermal solve.
    pub fn advance(
        &self,
        state: &State,
        seconds: f64,
        boundaries: &[Boundary],
    ) -> Result<Step, Refusal> {
        self.advance_with_exchange(state, seconds, boundaries, None, None)
    }

    /// Advance covered paths plus uncovered cloth faces under explicitly supplied face conditions.
    /// Exposed-boundary order is retained source-cloth triangle order. Fully covered triangles
    /// contribute no exposed heat. Omitting these conditions via [Self::advance] keeps uncovered
    /// cloth adiabatic rather than inventing an ambient reservoir.
    ///
    /// # Errors
    /// Any covered solve error, wrong boundary count, invalid cloth geometry or exposed solve error.
    pub fn advance_with_exposed(
        &self,
        state: &State,
        seconds: f64,
        boundaries: &[Boundary],
        exposed_boundaries: &[Boundary],
    ) -> Result<Step, Refusal> {
        self.advance_with_exchange(
            state,
            seconds,
            boundaries,
            None,
            Some(ExposedConditions {
                boundaries: exposed_boundaries,
                environments: None,
            }),
        )
    }

    /// Advance stored heat through covered regions and optional canonical exposed-cloth faces.
    /// Covered environments use body-triangle order; exposed fields use source-triangle order.
    /// Bare body exchange remains an explicitly prescribed film path, with no inferred skin optics.
    /// Weather is a step input, not part of material-temperature identity: valid next-interval
    /// conditions may change without remapping or resetting the old state.
    ///
    /// Automatic mode selects each region's actual outer material side through its source-bearing
    /// face mapping, using the same orientation as the depth remapper. For uncovered cloth it selects
    /// the canonical source positive face. Explicit mode keeps the caller's face/property choice.
    ///
    /// # Errors
    /// Wrong state, array counts or time; invalid active geometry/material/boundary observations;
    /// incompatible exterior films or optical qualifications; failed solve or accumulated overflow.
    /// All inputs remain unchanged and no partial field is returned on failure.
    pub fn advance_with_exchange(
        &self,
        state: &State,
        seconds: f64,
        boundaries: &[Boundary],
        covered_exterior: Option<ExteriorConditions<'_>>,
        exposed_conditions: Option<ExposedConditions<'_>>,
    ) -> Result<Step, Refusal> {
        self.check(state)?;
        if exposed_conditions.is_some_and(|e| {
            e.boundaries.len() != self.cloth_triangle_count()
                || e.environments
                    .is_some_and(|v| v.len() != self.cloth_triangle_count())
        }) {
            return Err(invalid(
                "exposed thermal motion needs one face boundary and, when enabled, one exterior environment per source cloth triangle",
            ));
        }
        let heat = match covered_exterior {
            Some(e) => self.coverage.advance_exterior(
                &state.inner,
                &self.cloth,
                &self.body,
                seconds,
                boundaries,
                e,
            )?,
            None => {
                self.coverage
                    .advance(&state.inner, &self.cloth, &self.body, seconds, boundaries)?
            }
        };
        let Some(conditions) = exposed_conditions else {
            return Ok(Step {
                frame_sha256: self.sha256.clone(),
                heat,
                exposed: state.exposed.clone(),
                exposed_inside_heat_w: 0.,
                exposed_outside_heat_w: 0.,
                exposed_stored_change_j: 0.,
                exposed_balance_error_j: 0.,
                exposed_exterior_exchange: None,
                internal_heat: None,
            });
        };
        let fractions = self.exposed_fractions();
        let mut exposed = Vec::with_capacity(fractions.len());
        let mut exchange = conditions.environments.map(|_| ExposedExteriorExchange {
            triangles: Vec::new(),
            convective_heat_w: 0.,
            radiative_heat_w: 0.,
            balance_residual_w: 0.,
        });
        let mut inside = 0.;
        let mut outside = 0.;
        let mut stored = 0.;
        let mut error = 0.;
        for (i, (((fraction, model), old), boundary)) in fractions
            .into_iter()
            .zip(&self.exposed_models)
            .zip(&state.exposed)
            .zip(conditions.boundaries)
            .enumerate()
        {
            if fraction <= 0. {
                exposed.push(Vec::new());
                continue;
            }
            let patch = &self.coverage.source.patches[i];
            let [a, b, c] = patch.vertices.map(|v| self.cloth[v]);
            let current_area = 0.5 * (b - a).cross(c - a).norm();
            if !current_area.is_finite() || current_area <= 0. {
                return Err(invalid("exposed cloth triangle has invalid current area"));
            }
            let ratio = current_area / patch.reference_area_m2;
            if !ratio.is_finite() || ratio <= 0. {
                return Err(invalid(
                    "exposed cloth current/reference area ratio is invalid",
                ));
            }
            let (next, exterior) = match conditions.environments {
                Some(e) => {
                    let next = model.advance_exterior(old, seconds * ratio, *boundary, &e[i])?;
                    (next.state, Some(next.exchange))
                }
                None => (model.advance(old, seconds * ratio, *boundary)?, None),
            };
            let current = current_area * fraction;
            let reference = patch.reference_area_m2 * fraction;
            inside += current * next.inside_flux_w_m2;
            outside += current * next.outside_flux_w_m2;
            stored += reference * next.stored_change_j_m2;
            error += reference * next.balance_error_j_m2;
            if let Some(flux) = exterior {
                let convective = current * flux.convective_flux_w_m2;
                let radiative = current * flux.radiative_flux_w_m2;
                let residual = current * flux.balance_residual_w_m2;
                if [convective, radiative, residual]
                    .iter()
                    .any(|v| !v.is_finite())
                {
                    return Err(invalid("exposed exterior area-scaled heat overflow"));
                }
                let output = exchange.as_mut().expect("exterior branch enabled");
                output.convective_heat_w += convective;
                output.radiative_heat_w += radiative;
                output.balance_residual_w += residual;
                output.triangles.push(ExposedTriangleExteriorExchange {
                    cloth_triangle: i,
                    current_area_m2: current,
                    flux,
                    convective_heat_w: convective,
                    radiative_heat_w: radiative,
                    balance_residual_w: residual,
                });
            }
            exposed.push(next.temperature_c);
        }
        if [inside, outside, stored, error]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(invalid("exposed cloth heat accumulation overflow"));
        }
        let step = Step {
            frame_sha256: self.sha256.clone(),
            heat,
            exposed,
            exposed_inside_heat_w: inside,
            exposed_outside_heat_w: outside,
            exposed_stored_change_j: stored,
            exposed_balance_error_j: error,
            exposed_exterior_exchange: exchange,
            internal_heat: None,
        };
        if step
            .exterior_totals()
            .is_some_and(|values| values.iter().any(|v| !v.is_finite()))
            || [
                step.inside_heat_w(),
                step.outside_heat_w(),
                step.net_heat_w(),
                step.stored_change_j(),
                step.balance_error_j(),
            ]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(invalid(
                "combined covered/exposed heat accumulation overflow",
            ));
        }
        Ok(step)
    }

    /// Advance boundary exchange with one nonnegative internal energy per retained source triangle.
    /// Joules are uniform over that triangle's reference area and thickness, then apportioned over
    /// its disjoint covered and exposed portions. Even adiabatic exposed cloth receives its share.
    /// Enthalpy preloading uses the same bounded kernel as independent sheet heating and supplies
    /// the backward-Euler source term; it is not a new elapsed step or a remap. Source qualification
    /// and its physical origin belong to the caller. The source never heats the bare-body path.
    ///
    /// # Errors
    /// Invalid state/time/boundaries, wrong source count, negative/nonfinite energy, characterized
    /// temperature-domain violation, failed enthalpy inversion or inconsistent material allocation.
    /// Failure changes neither the supplied state nor any retained mechanical/thermal history.
    pub fn advance_with_exchange_and_heat(
        &self,
        state: &State,
        seconds: f64,
        boundaries: &[Boundary],
        covered_exterior: Option<ExteriorConditions<'_>>,
        exposed_conditions: Option<ExposedConditions<'_>>,
        source_heat_j: &[f64],
    ) -> Result<Step, Refusal> {
        self.check(state)?;
        if source_heat_j.len() != self.cloth_triangle_count()
            || source_heat_j.iter().any(|q| !q.is_finite() || *q < 0.)
            || !seconds.is_finite()
            || seconds <= 0.
        {
            return Err(invalid(
                "coverage heat needs positive time and one finite nonnegative energy per source triangle",
            ));
        }
        let supplied: f64 = source_heat_j.iter().sum();
        if !supplied.is_finite() {
            return Err(invalid("coverage source heat overflows"));
        }
        if supplied == 0. {
            return self.advance_with_exchange(
                state,
                seconds,
                boundaries,
                covered_exterior,
                exposed_conditions,
            );
        }
        let mut heated = state.clone();
        let cells = self.coverage.layout.cells_per_layer;
        let mut covered = Vec::with_capacity(self.coverage.models.len());
        for (i, region) in self.coverage.layout.regions.iter().enumerate() {
            // This temporary model measures capacities in J/K rather than J/(m² K): each layer
            // owns its allocated REFERENCE area, independent of current body or cloth geometry.
            let capacities: Vec<_> = region
                .layers
                .iter()
                .map(|p| {
                    self.coverage.source.patches[p.cloth_triangle].reference_area_m2 * p.fraction
                })
                .collect();
            let model = self.coverage.models[i]
                .with_area_factors(&vec![1.; capacities.len()], &capacities)?;
            let energy: Vec<_> = region
                .layers
                .iter()
                .flat_map(|p| {
                    vec![source_heat_j[p.cloth_triangle] * p.fraction / cells as f64; cells]
                })
                .collect();
            let (next, requested, realized) =
                preload(&model, &state.inner.temperatures[i], &energy)?;
            heated.inner.temperatures[i] = next;
            covered.push((requested, realized));
        }
        let mut exposed_requested = 0.;
        let mut exposed_realized = 0.;
        for (i, fraction) in self.exposed_fractions().into_iter().enumerate() {
            if fraction <= 0. {
                continue;
            }
            let reference = self.coverage.source.patches[i].reference_area_m2 * fraction;
            let model = self.exposed_models[i].with_area_factors(&[1.], &[reference])?;
            let energy = vec![source_heat_j[i] * fraction / model.cells() as f64; model.cells()];
            let (next, requested, realized) = preload(&model, &state.exposed[i], &energy)?;
            heated.exposed[i] = next;
            exposed_requested += requested;
            exposed_realized += realized;
        }
        let covered_requested: f64 = covered.iter().map(|q| q.0).sum();
        let allocation_error = covered_requested + exposed_requested - supplied;
        if [
            covered_requested,
            exposed_requested,
            exposed_realized,
            allocation_error,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || allocation_error.abs() > 1e-9 * (1. + supplied)
        {
            return Err(invalid(
                "coverage heat does not conserve the complete source material",
            ));
        }
        let mut step = self.advance_with_exchange(
            &heated,
            seconds,
            boundaries,
            covered_exterior,
            exposed_conditions,
        )?;
        // The ordinary solver measures storage from the temporary preloaded state. Restore the
        // physical old-to-new change and retain preload roundoff against the requested energy.
        for (region, (requested, realized)) in step.heat.regions.iter_mut().zip(covered) {
            region.stored_change_j += realized;
            step.heat.stored_change_j += realized;
            step.heat.balance_error_j += realized - requested;
        }
        step.exposed_stored_change_j += exposed_realized;
        step.exposed_balance_error_j += exposed_realized - exposed_requested;
        step.internal_heat = Some(InternalHeat {
            supplied_heat_j: supplied,
            covered_heat_j: covered_requested,
            exposed_heat_j: exposed_requested,
            allocation_residual_j: allocation_error,
        });
        if !step.stored_change_j().is_finite() || !step.balance_error_j().is_finite() {
            return Err(invalid("coverage heat accounting overflows"));
        }
        Ok(step)
    }

    /// Every covered/exposed temperature grouped by its unchanged source triangle. This allows
    /// source-qualified conversions to check all partitions, including hidden inner material.
    pub(crate) fn source_temperatures(&self, state: &State) -> Result<Vec<Vec<f64>>, Refusal> {
        self.check(state)?;
        let mut values = state.exposed.clone();
        let cells = self.coverage.layout.cells_per_layer;
        for (region, temperatures) in self
            .coverage
            .layout
            .regions
            .iter()
            .zip(&state.inner.temperatures)
        {
            for (layer, portion) in region.layers.iter().enumerate() {
                values[portion.cloth_triangle]
                    .extend_from_slice(&temperatures[layer * cells..(layer + 1) * cells]);
            }
        }
        Ok(values)
    }

    fn exposed_fractions(&self) -> Vec<f64> {
        self.coverage
            .allocated
            .iter()
            .map(|allocated| (1. - allocated).max(0.))
            .collect()
    }

    /// Conservatively transfer old temperatures into this frame using material-coordinate overlap.
    /// Body mesh/pose, region ordering and through-thickness discretization may change. No source
    /// material, cloth topology, physical-layer identity or covered material domain may change.
    ///
    /// # Errors
    /// Changed source/physical identity, entering/leaving cloth, unmatched spatial partitions,
    /// work-budget exhaustion, out-of-domain temperatures, or failed local/global energy balance.
    pub fn remap(&self, old: &Self, state: &State) -> Result<Transfer, Refusal> {
        old.check(state)?;
        if self.coverage.source.sha256 != old.coverage.source.sha256
            || self.request.cloth_layers != old.request.cloth_layers
        {
            return Err(invalid(
                "remap requires unchanged source material, cloth topology and layer identities",
            ));
        }
        let source = &self.coverage.source;
        let laws = source
            .columns
            .iter()
            .map(|column| {
                let m = &column.layers[0];
                ThermalLaw::from_material(m.material(), m.allows_estimates())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let costs: Vec<_> = source
            .columns
            .iter()
            .map(|column| {
                column.layers[0]
                    .material()
                    .thermal_response
                    .as_ref()
                    .and_then(|r| r.specific_heat.as_ref())
                    .map_or(1, |s| s.points.len().max(1))
            })
            .collect();
        let mut donors = vec![Vec::new(); source.patches.len()];
        let mut zeros = vec![None::<f64>; source.patches.len()];
        let n = old.coverage.layout.cells_per_layer;
        let m = self.coverage.layout.cells_per_layer;
        for (r, region) in old.coverage.layout.regions.iter().enumerate() {
            for (l, p) in region.layers.iter().enumerate() {
                donors[p.cloth_triangle].push((r, l));
                for &t in &state.inner.temperatures[r][l * n..(l + 1) * n] {
                    laws[source.patches[p.cloth_triangle].column].check_temperature(t)?;
                    zeros[p.cloth_triangle] = Some(zeros[p.cloth_triangle].map_or(t, |v| v.min(t)));
                }
            }
        }
        for (patch, row) in state.exposed.iter().enumerate() {
            for &t in row {
                laws[source.patches[patch].column].check_temperature(t)?;
                zeros[patch] = Some(zeros[patch].map_or(t, |v| v.min(t)));
            }
        }
        let old_exposed = old.exposed_fractions();
        let new_exposed = self.exposed_fractions();
        let mut work = Work::default();
        let (old_polygons, old_roundoff) = old.material_polygons(&mut work)?;
        let (polygons, new_roundoff) = self.material_polygons(&mut work)?;
        let mut used: Vec<Vec<f64>> = old
            .coverage
            .layout
            .regions
            .iter()
            .map(|r| vec![0.; r.layers.len()])
            .collect();
        let mut exposed_used = vec![0.; source.patches.len()];
        let mut output = Vec::new();
        let mut destination_error = 0.;
        for (r, region) in self.coverage.layout.regions.iter().enumerate() {
            let mut values = Vec::new();
            for (l, p) in region.layers.iter().enumerate() {
                let polygon = &polygons[r][l];
                let mut intersections = Vec::new();
                let mut received = 0.;
                for &(s, k) in &donors[p.cloth_triangle] {
                    work.add(1)?;
                    let other = &old_polygons[s][k];
                    if !bounds_overlap(polygon, other) {
                        continue;
                    }
                    let overlap = 2. * area(&intersection(polygon, other, &mut work)?).max(0.);
                    if overlap == 0. {
                        continue;
                    }
                    let f = old.coverage.layout.regions[s].layers[k].fraction * overlap
                        / (2. * area(other));
                    if !f.is_finite() || f <= 0. {
                        return Err(invalid("remap intersection fraction overflow"));
                    }
                    used[s][k] += f;
                    received += f;
                    intersections.push((s, k, f));
                }
                let mut from_exposed = p.fraction - received;
                let tol = 5e-10 * p.fraction + 64. * f64::EPSILON;
                if from_exposed < 0. && from_exposed.abs() <= tol {
                    from_exposed = 0.;
                }
                // Fully covered old material has no exposed donor. Independent inverse
                // projections/clipping can leave a positive overlap deficit within the same
                // allocation tolerance already admitted below. Keep the actual covered weights
                // and received area: destination/donor allocation and enthalpy residuals must
                // still report this roundoff. A missing or materially incomplete stencil fails.
                if old_exposed[p.cloth_triangle] == 0.
                    && received > 0.
                    && from_exposed > 0.
                    && from_exposed <= tol
                {
                    from_exposed = 0.;
                }
                if from_exposed > 0. {
                    if old_exposed[p.cloth_triangle] <= 0.
                        || state.exposed[p.cloth_triangle].is_empty()
                    {
                        return Err(invalid(
                            "newly covered cloth has no retained exposed thermal history",
                        ));
                    }
                    exposed_used[p.cloth_triangle] += from_exposed;
                    if exposed_used[p.cloth_triangle] > old_exposed[p.cloth_triangle] + tol {
                        return Err(invalid(
                            "coverage consumes more exposed material than the source triangle owns",
                        ));
                    }
                    received += from_exposed;
                }
                check_fraction(received, p.fraction)?;
                destination_error += (received - p.fraction).abs()
                    * source.patches[p.cloth_triangle].reference_area_m2;
                let law = &laws[source.patches[p.cloth_triangle].column];
                for j in 0..m {
                    let mut inputs = Vec::new();
                    let [a, b] = depth(j, m, self.reverse[r][l]);
                    for &(s, k, f) in &intersections {
                        add_depth_inputs(
                            &mut inputs,
                            f,
                            [a, b],
                            old.reverse[s][k],
                            &state.inner.temperatures[s][k * n..(k + 1) * n],
                            &mut work,
                        )?;
                    }
                    if from_exposed > 0. {
                        add_depth_inputs(
                            &mut inputs,
                            from_exposed,
                            [a, b],
                            false,
                            &state.exposed[p.cloth_triangle],
                            &mut work,
                        )?;
                    }
                    values.push(mix(
                        law,
                        &inputs,
                        costs[source.patches[p.cloth_triangle].column],
                        &mut work,
                    )?);
                }
            }
            output.push(values);
        }

        // The uncovered state is the material complement of destination coverage. It receives the
        // unused portions of every old covered polygon plus old exposed material not consumed above.
        let mut exposed = Vec::with_capacity(source.patches.len());
        let mut donor_error = 0.;
        for patch in 0..source.patches.len() {
            let expected = new_exposed[patch];
            if expected <= 0. {
                exposed.push(Vec::new());
                continue;
            }
            let old_remaining = old_exposed[patch] - exposed_used[patch];
            let tol = 5e-10 * old_exposed[patch].max(1e-30) + 64. * f64::EPSILON;
            if old_remaining < -tol {
                return Err(invalid(
                    "exposed material allocation underflow during remap",
                ));
            }
            let old_remaining = old_remaining.max(0.);
            let mut covered_remaining = Vec::new();
            let mut received = old_remaining;
            for &(s, k) in &donors[patch] {
                let fraction = old.coverage.layout.regions[s].layers[k].fraction;
                let remaining = fraction - used[s][k];
                let local_tol = 5e-10 * fraction + 64. * f64::EPSILON;
                if remaining < -local_tol {
                    return Err(invalid(
                        "covered material allocation underflow during remap",
                    ));
                }
                let remaining = remaining.max(0.);
                received += remaining;
                covered_remaining.push((s, k, remaining));
            }
            check_fraction(received, expected)?;
            destination_error +=
                (received - expected).abs() * source.patches[patch].reference_area_m2;
            donor_error += ((old_remaining + exposed_used[patch]) - old_exposed[patch]).abs()
                * source.patches[patch].reference_area_m2;
            let law = &laws[source.patches[patch].column];
            let mut row = Vec::with_capacity(m);
            for j in 0..m {
                let a = j as f64 / m as f64;
                let b = (j + 1) as f64 / m as f64;
                let mut inputs = Vec::new();
                if old_remaining > 0. {
                    add_depth_inputs(
                        &mut inputs,
                        old_remaining,
                        [a, b],
                        false,
                        &state.exposed[patch],
                        &mut work,
                    )?;
                }
                for &(s, k, remaining) in &covered_remaining {
                    if remaining <= 0. {
                        continue;
                    }
                    add_depth_inputs(
                        &mut inputs,
                        remaining,
                        [a, b],
                        old.reverse[s][k],
                        &state.inner.temperatures[s][k * n..(k + 1) * n],
                        &mut work,
                    )?;
                }
                row.push(mix(
                    law,
                    &inputs,
                    costs[source.patches[patch].column],
                    &mut work,
                )?);
            }
            exposed.push(row);
        }
        // Fully covered destination patches still require every old covered/exposed donor to be
        // consumed by covered recipients. This catches spatial overlap mistakes that scalar totals hide.
        for patch in 0..source.patches.len() {
            if new_exposed[patch] > 0. {
                continue;
            }
            check_fraction(exposed_used[patch], old_exposed[patch])?;
            for &(s, k) in &donors[patch] {
                let fraction = old.coverage.layout.regions[s].layers[k].fraction;
                check_fraction(used[s][k], fraction)?;
                donor_error +=
                    (used[s][k] - fraction).abs() * source.patches[patch].reference_area_m2;
            }
        }
        let next = State {
            frame_sha256: self.sha256.clone(),
            inner: CoverageState {
                sha256: self.coverage.sha256.clone(),
                temperatures: output,
                orientation: Some(self.coverage_orientation()),
            },
            exposed,
        };
        let before = old.energy(state, &laws, &zeros, &costs, &mut work)?;
        let after = self.energy(&next, &laws, &zeros, &costs, &mut work)?;
        let mut absolute_residual = 0.;
        for (&a, &b) in before.iter().zip(&after) {
            let error = (b - a).abs();
            if !error.is_finite() || error > 2e-9 * (1. + a.abs() + b.abs()) {
                return Err(invalid("material-local remap enthalpy balance failed"));
            }
            absolute_residual += error;
        }
        let before_j = before.iter().sum::<f64>();
        let after_j = after.iter().sum::<f64>();
        let reference_area_m2 = source
            .patches
            .iter()
            .map(|p| p.reference_area_m2)
            .sum::<f64>();
        let balance = Balance {
            from_sha256: old.sha256.clone(),
            to_sha256: self.sha256.clone(),
            reference_area_m2,
            allocation_residual_m2: donor_error.max(destination_error),
            polygon_roundoff_m2: old_roundoff + new_roundoff,
            reference_temperature_c: zeros,
            before_j,
            after_j,
            residual_j: after_j - before_j,
            absolute_residual_j: absolute_residual,
            operations: work.0,
        };
        if [
            reference_area_m2,
            donor_error,
            destination_error,
            before_j,
            after_j,
            balance.residual_j,
            balance.polygon_roundoff_m2,
            absolute_residual,
        ]
        .iter()
        .any(|x| !x.is_finite())
        {
            return Err(invalid("remap balance accumulation overflow"));
        }
        Ok(Transfer {
            state: next,
            balance,
        })
    }

    fn material_polygons(&self, work: &mut Work) -> Result<(MaterialPolygons, f64), Refusal> {
        let mut roundoff = 0.;
        let polygons = self
            .projection
            .footprints
            .iter()
            .map(|fp| {
                fp.hits
                    .iter()
                    .map(|hit| {
                        let (polygon, delta) = convex_roundoff(&hit.cloth_polygon, work)?;
                        roundoff += 2.
                            * delta
                            * self.coverage.source.patches[hit.cloth_triangle].reference_area_m2;
                        Ok(polygon)
                    })
                    .collect::<Result<Vec<_>, Refusal>>()
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        Ok((polygons, roundoff))
    }

    fn energy(
        &self,
        state: &State,
        laws: &[ThermalLaw],
        zeros: &[Option<f64>],
        costs: &[usize],
        work: &mut Work,
    ) -> Result<Vec<f64>, Refusal> {
        let source = &self.coverage.source;
        let n = self.coverage.layout.cells_per_layer;
        let mut energy = vec![0.; source.patches.len()];
        for (r, region) in self.coverage.layout.regions.iter().enumerate() {
            for (l, p) in region.layers.iter().enumerate() {
                let patch = &source.patches[p.cloth_triangle];
                let zero = zeros[p.cloth_triangle]
                    .ok_or_else(|| invalid("material triangle has no thermal history"))?;
                let scale = patch.reference_area_m2 * p.fraction / n as f64;
                for &t in &state.inner.temperatures[r][l * n..(l + 1) * n] {
                    work.add(costs[patch.column])?;
                    energy[p.cloth_triangle] +=
                        scale * laws[patch.column].sensible_change(zero, t)?;
                }
            }
        }
        for (patch_index, (&fraction, row)) in self
            .exposed_fractions()
            .iter()
            .zip(&state.exposed)
            .enumerate()
        {
            if fraction <= 0. {
                continue;
            }
            let patch = &source.patches[patch_index];
            let zero = zeros[patch_index]
                .ok_or_else(|| invalid("exposed material triangle has no thermal history"))?;
            let scale = patch.reference_area_m2 * fraction / n as f64;
            for &t in row {
                work.add(costs[patch.column])?;
                energy[patch_index] += scale * laws[patch.column].sensible_change(zero, t)?;
            }
        }
        Ok(energy)
    }
}

type MaterialPolygons = Vec<Vec<Vec<Vec2>>>;

// Projection polygons are convex by construction, but inverse barycentric arithmetic can leave
// almost-duplicate corners with an inward turn. Treating that tiny edge as an infinite clip line
// can erase a finite donor wedge. An exact-sign hull removes only numerically insignificant dents;
// it does not round coordinates, weaken the material-domain test or repair genuine nonconvexity.
fn convex_roundoff(p: &[Vec2], work: &mut Work) -> Result<(Vec<Vec2>, f64), Refusal> {
    if !(3..=256).contains(&p.len())
        || p.iter()
            .any(|v| !v.is_finite() || v.x.abs() > 2. || v.y.abs() > 2.)
    {
        return Err(invalid(
            "invalid material-coordinate polygon for heat remapping",
        ));
    }
    work.add(p.len() * 10)?;
    let mut sorted = p.to_vec();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    sorted.dedup();
    let mut hull: Vec<Vec2> = Vec::new();
    let mut append = |points: &mut dyn Iterator<Item = &Vec2>| {
        let start = hull.len();
        for &c in points {
            while hull.len() >= start + 2 {
                let a = hull[hull.len() - 2];
                let b = hull[hull.len() - 1];
                if orient2d([a.x, a.y], [b.x, b.y], [c.x, c.y]) == Sign::Positive {
                    break;
                }
                hull.pop();
            }
            hull.push(c);
        }
        hull.pop();
    };
    append(&mut sorted.iter());
    append(&mut sorted.iter().rev());
    let original = area(p);
    let corrected = area(&hull);
    let delta = (corrected - original).abs();
    if !original.is_finite()
        || !corrected.is_finite()
        || original <= 0.
        || corrected <= 0.
        || delta > 128. * f64::EPSILON * (1. + original.abs())
    {
        return Err(invalid(
            "material polygon is nonconvex beyond floating-point roundoff",
        ));
    }
    Ok((hull, delta))
}

fn check_fraction(received: f64, expected: f64) -> Result<(), Refusal> {
    // Explicit numerical allowance for the projection's bounded allocation-roundoff corrections.
    // Never normalize away a genuinely entering/leaving portion or a spatial gap.
    if !received.is_finite() || (received - expected).abs() > 5e-10 * expected + 64. * f64::EPSILON
    {
        return Err(invalid(format!(
            "material allocation changed unexpectedly (received {received:.17e}, expected {expected:.17e})",
        )));
    }
    if expected == 0. {
        return Ok(());
    }
    if received <= 0. {
        return Err(invalid("destination has no thermal donor"));
    }
    Ok(())
}

fn add_depth_inputs(
    inputs: &mut Vec<(f64, f64)>,
    fraction: f64,
    interval: [f64; 2],
    reverse: bool,
    temperatures: &[f64],
    work: &mut Work,
) -> Result<(), Refusal> {
    let n = temperatures.len();
    if n == 0 {
        return Err(invalid("thermal donor has no depth cells"));
    }
    let [a, b] = interval;
    let start = (a * n as f64).floor() as usize;
    let end = ((b * n as f64).ceil() as usize).min(n);
    for (canonical, _) in temperatures.iter().enumerate().take(end).skip(start) {
        work.add(1)?;
        let overlap = b.min((canonical + 1) as f64 / n as f64) - a.max(canonical as f64 / n as f64);
        if overlap <= 0. {
            continue;
        }
        let i = if reverse {
            n - 1 - canonical
        } else {
            canonical
        };
        inputs.push((fraction * overlap, temperatures[i]));
    }
    Ok(())
}

fn depth(i: usize, n: usize, reverse: bool) -> [f64; 2] {
    let i = if reverse { n - 1 - i } else { i };
    [i as f64 / n as f64, (i + 1) as f64 / n as f64]
}

fn mix(
    law: &ThermalLaw,
    inputs: &[(f64, f64)],
    knots: usize,
    work: &mut Work,
) -> Result<f64, Refusal> {
    if inputs.is_empty() {
        return Err(invalid("empty enthalpy transfer stencil"));
    }
    work.add(inputs.len())?;
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut weight = 0.;
    for &(w, t) in inputs {
        if !w.is_finite() || w <= 0. {
            return Err(invalid("invalid enthalpy transfer weight"));
        }
        law.check_temperature(t)?;
        lo = lo.min(t);
        hi = hi.max(t);
        weight += w;
    }
    if !weight.is_finite() || weight <= 0. {
        return Err(invalid("enthalpy transfer weight overflow"));
    }
    if lo == hi {
        return Ok(lo);
    }
    // Curve integration visits its knots. Bound this work too, not just the number of stencil
    // entries, so a long characterization table cannot evade the remap budget.
    work.add(
        knots
            .checked_mul(inputs.len() + 132)
            .ok_or_else(|| invalid("remap work overflow"))?,
    )?;
    let zero = lo;
    let target = inputs
        .iter()
        .map(|&(w, t)| Ok((w / weight) * law.sensible_change(zero, t)?))
        .collect::<Result<Vec<_>, Refusal>>()?
        .iter()
        .sum::<f64>();
    if !target.is_finite() {
        return Err(invalid("enthalpy average overflow"));
    }
    // Positive Cp gives a strictly monotone inverse. Bracket by donor temperatures, not by an
    // extrapolated material domain; 128 bounded bisections preserve the donor maximum principle.
    for _ in 0..128 {
        let t = lo / 2. + hi / 2.;
        if t == lo || t == hi {
            break;
        }
        if law.sensible_change(zero, t)? < target {
            lo = t;
        } else {
            hi = t;
        }
    }
    let error_lo = (law.sensible_change(zero, lo)? - target).abs();
    let error_hi = (law.sensible_change(zero, hi)? - target).abs();
    let t = if error_lo <= error_hi { lo } else { hi };
    let tolerance = 256. * f64::EPSILON * law.capacity_at(t)? * (1. + zero.abs() + t.abs())
        + 1e-12 * (1. + target.abs());
    if !tolerance.is_finite() || error_lo.min(error_hi) > tolerance {
        return Err(invalid("enthalpy inverse did not converge"));
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covered_roundoff_allowance_cannot_supply_a_missing_spatial_donor() {
        use crate::thermal::{DryThermalModel, surface::Patch, surface::projection::Settings};
        use ftx_pattern::{material_catalog::Selection, materials::Catalog};

        let fixture: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/materials/synthetic-variable-thermal.json"
        )))
        .unwrap();
        let selection: Selection = serde_json::from_value(fixture["layers"][0].clone()).unwrap();
        let model = DryThermalModel::new(
            vec![Catalog::builtin().unwrap().resolve(&selection).unwrap()],
            vec![],
            1,
        )
        .unwrap();
        let source = Surface::new(
            vec![model],
            3,
            vec![Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            }],
        )
        .unwrap();
        let body = vec![Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)];
        let old = Frame::new(
            source,
            body.iter().map(|&p| p + Vec3::new(0., 0., 0.04)).collect(),
            body,
            &[[0, 1, 2]],
            Request {
                schema: "ftx-thermal-projection/1".into(),
                cloth_layers: vec!["shell".into()],
                settings: Settings {
                    max_distance_m: 0.2,
                    min_normal_cosine: 0.2,
                    separation_tolerance_m: 1e-8,
                    cells_per_layer: 1,
                    interfaces: vec![],
                },
            },
        )
        .unwrap();
        let state = old.initial(20.).unwrap();
        for (offset, fraction) in [(0.25, 1.), (1., 1e-16)] {
            // Deliberately damage the private spatial stencil. Public frames derive these
            // footprints from geometry; scalar allocation tolerances must still never turn
            // a finite uncovered wedge or an entirely empty microscopic stencil into heat.
            let mut next = old.clone();
            for p in &mut next.projection.footprints[0].hits[0].cloth_polygon {
                p.x += offset;
            }
            next.coverage.layout.regions[0].layers[0].fraction = fraction;
            let refusal = next.remap(&old, &state).unwrap_err();
            assert_eq!(
                refusal.message,
                "newly covered cloth has no retained exposed thermal history"
            );
        }
        assert_eq!(state.temperature_c(), &[vec![20.]]);
        assert!(state.exposed_temperature_c()[0].is_empty());
    }

    #[test]
    fn microscopic_inward_edge_cannot_delete_a_finite_remap_donor() {
        // Retained from the sewn-band CLI regression: inverse barycentric rounding puts two
        // nearly coincident corners in an inward turn. Treating that microscopic edge as an
        // infinite clipping plane removes a donor about 5.14e-8 of the material triangle.
        let target = [
            Vec2::new(0.141_542_294_724_737_98, 0.000_965_775_692_924_402_1),
            Vec2::new(4.273_511_510_613_281e-14, 5.463_578_223_132_216e-16),
            Vec2::new(0.141_381_797_130_168_27, -5.922_597_980_646_374e-16),
        ];
        let donor = [
            Vec2::new(0.002_742_812_421_032_216, 2.776_217_803_427_988e-16),
            Vec2::new(0.002_744_394_779_058_933_5, 1.872_563_797_723_176_4e-5),
            Vec2::new(1.333_450_065_677_077e-15, 2.659_877_951_251_099e-16),
            Vec2::new(1.365_249_613_424_237_6e-15, 2.902_447_992_725_097_4e-16),
        ];
        let mut work = Work::default();
        let broken = intersection(&target, &donor, &mut work).unwrap();
        assert!(area(&broken).abs() < 1e-12);
        let (target, _) = convex_roundoff(&target, &mut work).unwrap();
        let (fixed, correction) = convex_roundoff(&donor, &mut work).unwrap();
        let overlap = intersection(&target, &fixed, &mut work).unwrap();
        assert!(correction < 1e-18);
        // Independent rational-coordinate half-plane intersection, starting from these exact
        // f64 inputs, gives this doubled area. The tiny boundary truncation is real, not zero.
        assert!((2. * area(&overlap) - 5.136_091_243_263_29e-8).abs() < 1e-20);
        assert!((area(&fixed) - area(&donor)).abs() < 1e-18);
    }

    #[test]
    fn convex_roundoff_preserves_convex_domain_despite_duplicates_and_cycle_origin() {
        let original = [
            Vec2::new(0., 0.),
            Vec2::new(0.5, 0.),
            Vec2::new(0.5, 0.),
            Vec2::new(1., 0.),
            Vec2::new(0., 1.),
            Vec2::new(0., 0.),
        ];
        for start in 0..original.len() {
            let mut p = original.to_vec();
            p.rotate_left(start);
            let (hull, correction) = convex_roundoff(&p, &mut Work::default()).unwrap();
            assert_eq!(hull.len(), 3);
            assert_eq!(area(&hull), 0.5);
            assert_eq!(correction, 0.);
        }
    }

    #[test]
    fn roundoff_admission_is_not_a_repair_for_nonconvex_or_invalid_material_geometry() {
        let concave = [
            Vec2::new(0., 0.),
            Vec2::new(1., 0.),
            Vec2::new(0.25, 0.25),
            Vec2::new(0., 1.),
        ];
        assert!(convex_roundoff(&concave, &mut Work::default()).is_err());
        let triangle = [Vec2::new(0., 0.), Vec2::new(1., 0.), Vec2::new(0., 1.)];
        let mut clockwise = triangle;
        clockwise.reverse();
        assert!(convex_roundoff(&clockwise, &mut Work::default()).is_err());
        for v in [f64::NAN, f64::INFINITY, 3.] {
            let mut p = triangle;
            p[0].x = v;
            assert!(convex_roundoff(&p, &mut Work::default()).is_err());
        }
        assert!(convex_roundoff(&[Vec2::ZERO; 3], &mut Work::default()).is_err());
    }
}
