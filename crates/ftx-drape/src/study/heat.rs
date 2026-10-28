//! Thermal evolution on accepted mechanical samples, using the existing source-locked solvers.
//! The default is one-way. Explicit evaporation coupling updates mechanical liquid mass only
//! after a complete heat/water interval succeeds; temperature does not invent material laws.

use ftx_core::{Refusal, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{budget, hash, invalid, motion::Keyframe};
use crate::{
    run::DrapeRun,
    thermal::{
        Boundary,
        steady::{exterior::Exterior, request::Film},
        surface::{PatchHeat, Surface, SurfaceExteriorExchange, SurfaceState},
    },
};

pub mod coverage;
mod plastic;
pub mod surface_water;

/// Explicit closure for converting qualified material dissipation into sheet heat.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum DissipationHeatDistribution {
    /// Uniform energy per reference-thickness cell, released uniformly over each accepted interval.
    #[serde(rename = "uniform_reference_thickness")]
    UniformReferenceThickness,
}

/// Compatibility name for the original permanent-membrane heat option.
pub type PlasticHeatDistribution = DissipationHeatDistribution;

/// Explicit transient policy for independent cloth sheets during prescribed motion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRequest {
    /// Must be `ftx-motion-surface-heat/1`.
    pub schema: String,
    /// Uniform material temperature at the end of the initial mechanical drape (degrees Celsius).
    /// This is an authored initial condition, not a measured or equilibrated garment temperature.
    pub initial_temperature_c: f64,
    /// Through-thickness cells per retained material sheet, in 1..=64.
    pub cells_per_layer: usize,
    /// One spatially uniform reservoir/film pair per mechanical keyframe interval. Values apply
    /// after the preceding keyframe up to this keyframe; there is no implicit steady-film fallback.
    pub boundaries: Vec<Film>,
    /// Optional qualified replacement of each interval's outside film. Exactly one uniform
    /// environment per keyframe interval; each sheet supplies its own exposed-face emissivity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exterior_exchange: Option<Vec<Exterior>>,
    /// Opt-in irreversible membrane heating. Each active plastic material must independently
    /// qualify its conversion fraction and temperature domain; initial drape work is excluded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plastic_dissipation_heat: Option<PlasticHeatDistribution>,
    /// Opt-in Maxwell dashpot heating. Requires independently qualified conversion and temperature
    /// evidence on each relaxing material. Excludes initial drape work and numerical spring loss.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relaxation_dissipation_heat: Option<DissipationHeatDistribution>,
    /// Opt-in irreversible source-panel bending heat, with independently qualified conversion
    /// and temperature evidence. Initial drape work and recoverable curvature storage are excluded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bending_plastic_dissipation_heat: Option<DissipationHeatDistribution>,
    /// Optional source-qualified initial textile-owned free liquid, with conservative evaporation
    /// and latent temperature feedback. Independent of the prescribed body's wet-film consumer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_water: Option<surface_water::Request>,
    /// Optional source-qualified mass feedback into the next mechanical step. The original
    /// evaporation qualification requires water /1; independently qualified bidirectional exchange
    /// also admits supplied/condensed water. Initial liquid is added after the dry drape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mechanical_coupling: Option<crate::liquid_mass::Qualification>,
    /// Maximum thermal substep in seconds, in 1e-9..=1. Geometry stays at the accepted mechanical
    /// sample's right endpoint during these thermal substeps; no unseen cloth motion is invented.
    pub max_step_s: f64,
    /// Maximum temperature-cell advances for this whole history, in 1..=10000000. Also caps initial
    /// state size. Heated steps conservatively charge 129 cell-operation bound for bounded enthalpy
    /// inversion and transport. Each attempted history reserves against a 20000000 session budget.
    pub max_cell_steps: usize,
}

// Conservative reservations keep multi-alternative studies bounded, including failed histories.
// Separate units from the mechanical session's vertex/frame allocation budget.
#[derive(Default)]
pub(super) struct Budget {
    reserved_cell_steps: usize,
    reserved_geometry_operations: usize,
    reserved_vapor_geometry_operations: usize,
}

impl Budget {
    pub(in crate::study) fn reserve_vapor_geometry(&mut self, count: usize) -> Result<(), Refusal> {
        self.reserved_vapor_geometry_operations = self
            .reserved_vapor_geometry_operations
            .saturating_add(count);
        if self.reserved_vapor_geometry_operations > 40_000_000 {
            return Err(budget(
                "motion vapor exceeds 40000000 reserved session geometry operations",
            ));
        }
        Ok(())
    }

    fn reserve(&mut self, count: usize) -> Result<(), Refusal> {
        self.reserved_cell_steps = self.reserved_cell_steps.saturating_add(count);
        if self.reserved_cell_steps > 20_000_000 {
            return Err(budget(
                "motion heat exceeds 20000000 reserved session cell steps",
            ));
        }
        Ok(())
    }

    fn reserve_geometry(&mut self, count: usize) -> Result<(), Refusal> {
        self.reserved_geometry_operations = self.reserved_geometry_operations.saturating_add(count);
        if self.reserved_geometry_operations > 40_000_000 {
            return Err(budget(
                "motion heat exceeds 40000000 reserved session geometry operations",
            ));
        }
        Ok(())
    }
}

fn boundary(film: Film) -> Result<Boundary, Refusal> {
    if [film.inside_c, film.outside_c]
        .iter()
        .any(|t| !t.is_finite() || *t < -273.15)
        || [
            film.inside_resistance_m2_k_w,
            film.outside_resistance_m2_k_w,
        ]
        .iter()
        .any(|r| !r.is_finite() || *r < 0.)
    {
        return Err(invalid(
            "motion heat needs finite physical temperatures and nonnegative films",
        ));
    }
    Ok(Boundary {
        inside_c: film.inside_c,
        outside_c: film.outside_c,
        inside_resistance: film.inside_resistance_m2_k_w,
        outside_resistance: film.outside_resistance_m2_k_w,
    })
}

impl SurfaceRequest {
    pub(super) fn start(
        &self,
        run: &DrapeRun,
        keyframes: &[Keyframe],
        work: &mut Budget,
    ) -> Result<SurfaceHistory, Refusal> {
        self.validate(keyframes.len())?;
        work.reserve(self.max_cell_steps)?;
        let mut history =
            SurfaceHistory::new(self.clone(), run.thermal_surface(self.cells_per_layer)?)?;
        if self.plastic_dissipation_heat.is_some() {
            history.plastic = Some(plastic::Heating::new_kind(
                run,
                self.initial_temperature_c,
                plastic::Kind::Plastic,
            )?);
        }
        if self.relaxation_dissipation_heat.is_some() {
            history.relaxation = Some(plastic::Heating::new_kind(
                run,
                self.initial_temperature_c,
                plastic::Kind::Relaxation,
            )?);
        }
        if self.bending_plastic_dissipation_heat.is_some() {
            history.bending = Some(plastic::Heating::new_kind(
                run,
                self.initial_temperature_c,
                plastic::Kind::Bending,
            )?);
        }
        if let Some(request) = &self.surface_water {
            history.water = Some(surface_water::Water::new(
                request.clone(),
                &history.model,
                run,
                self.initial_temperature_c,
            )?);
        }
        Ok(history)
    }

    fn validate(&self, intervals: usize) -> Result<(), Refusal> {
        if self.schema != "ftx-motion-surface-heat/1"
            || !(1..=16).contains(&intervals)
            || self.boundaries.len() != intervals
            || self
                .exterior_exchange
                .as_ref()
                .is_some_and(|e| e.len() != intervals)
            || !(1..=64).contains(&self.cells_per_layer)
            || !self.initial_temperature_c.is_finite()
            || self.initial_temperature_c < -273.15
            || !self.max_step_s.is_finite()
            || !(1e-9..=1.).contains(&self.max_step_s)
            || !(1..=10_000_000).contains(&self.max_cell_steps)
        {
            return Err(invalid(
                "invalid motion surface heat schema, interval count, initial state or work policy",
            ));
        }
        for &film in &self.boundaries {
            boundary(film)?;
        }
        if let Some(exteriors) = &self.exterior_exchange {
            for (film, exterior) in self.boundaries.iter().zip(exteriors) {
                if film.outside_resistance_m2_k_w != 0. || film.outside_c != exterior.air_c {
                    return Err(invalid(
                        "motion exterior exchange requires zero outside film and outside_c equal to air_c in every interval",
                    ));
                }
            }
        }
        if let Some(water) = &self.surface_water {
            water.validate(intervals)?;
        }
        if let Some(coupling) = &self.mechanical_coupling {
            let exchange = coupling.schema == "ftx-coupled-liquid-mechanics/1";
            if exchange {
                coupling.validate_exchange()?;
            } else {
                coupling.validate()?;
            }
            let water = self.surface_water.as_ref().ok_or_else(|| {
                invalid(
                    "mechanical liquid coupling requires an explicit textile-owned water policy",
                )
            })?;
            if !exchange && water.schema != "ftx-textile-surface-water/1" {
                return Err(invalid(
                    "the evaporation-only mechanical qualification requires water /1; supply and condensation need an independent incoming/outgoing momentum qualification",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct SurfaceHistory {
    request: SurfaceRequest,
    model: Surface,
    state: SurfaceState,
    time_s: f64,
    cell_steps: usize,
    thermal_steps: usize,
    inside_j: f64,
    outside_j: f64,
    convective_j: f64,
    radiative_j: f64,
    stored_j: f64,
    net_j: f64,
    absolute_error_j: f64,
    trace: Vec<Value>,
    final_field: Option<SurfaceField>,
    plastic: Option<plastic::Heating>,
    relaxation: Option<plastic::Heating>,
    bending: Option<plastic::Heating>,
    water: Option<surface_water::Water>,
    mechanical_coupling: Option<Value>,
}

#[derive(Clone)]
struct SurfaceField {
    area_m2: f64,
    inside_heat_w: f64,
    outside_heat_w: f64,
    seconds: f64,
    stored_change_j: f64,
    patches: Vec<PatchHeat>,
    exterior_exchange: Option<SurfaceExteriorExchange>,
}

impl SurfaceHistory {
    fn new(request: SurfaceRequest, model: Surface) -> Result<Self, Refusal> {
        if model.cells() > request.max_cell_steps {
            return Err(budget(
                "motion heat initial cells exceed the whole-history budget",
            ));
        }
        let state = model.initial(request.initial_temperature_c)?;
        Ok(Self {
            request,
            model,
            state,
            time_s: 0.,
            cell_steps: 0,
            thermal_steps: 0,
            inside_j: 0.,
            outside_j: 0.,
            convective_j: 0.,
            radiative_j: 0.,
            stored_j: 0.,
            net_j: 0.,
            absolute_error_j: 0.,
            trace: Vec::new(),
            final_field: None,
            plastic: None,
            relaxation: None,
            bending: None,
            water: None,
            mechanical_coupling: None,
        })
    }

    pub(super) fn dissipation_capture(&self) -> crate::motion::DissipationCapture {
        crate::motion::DissipationCapture {
            plastic: self.plastic.is_some(),
            relaxation: self.relaxation.is_some(),
            bending: self.bending.is_some(),
        }
    }

    pub(super) fn bind_mechanics(&mut self, run: &mut DrapeRun) -> Result<Value, Refusal> {
        let qualification = self
            .request
            .mechanical_coupling
            .as_ref()
            .ok_or_else(|| invalid("missing evaporation mechanics qualification"))?;
        let policy = self
            .request
            .surface_water
            .as_ref()
            .ok_or_else(|| invalid("evaporation mechanics requires textile-owned water"))?;
        // Water checks this complete domain at every thermal substep, including event splits.
        // Requiring containment avoids admitting an out-of-domain intermediate temperature that
        // happens to return inside the mechanical domain by the next sampled endpoint.
        let temperatures = [
            policy.wet_vapor_pressure_pa[0][0],
            policy.wet_vapor_pressure_pa[policy.wet_vapor_pressure_pa.len() - 1][0],
        ];
        for (instance, water) in &policy.bindings {
            let bound = qualification.bindings.get(instance).ok_or_else(|| {
                invalid("evaporation mechanics must qualify every thermal material instance")
            })?;
            if bound.material_sha256 != water.material_sha256
                || bound.maximum_liquid_loading_kg_m2 < policy.maximum_liquid_loading_kg_m2
                || bound.temperature_range_c[0] > temperatures[0]
                || bound.temperature_range_c[1] < temperatures[1]
            {
                return Err(invalid(
                    "evaporation mechanics must qualify the complete thermal material, loading and temperature domain",
                ));
            }
        }
        if self.model.patches().len() != run.scene.solver.model.membranes.len()
            || self
                .model
                .patches()
                .iter()
                .zip(&run.scene.solver.model.membranes)
                .any(|(patch, element)| {
                    patch.vertices != element.v || patch.reference_area_m2 != element.area
                })
        {
            return Err(invalid(
                "evaporating mechanical and thermal histories must share the exact retained triangles",
            ));
        }
        let water = self
            .water
            .as_ref()
            .ok_or_else(|| invalid("missing admitted textile-water history"))?;
        let report = if qualification.schema == "ftx-coupled-liquid-mechanics/1" {
            crate::liquid_mass::bind_exchange(
                run,
                qualification,
                water.remaining_liquid_kg(),
                self.state.temperature_c(),
            )?
        } else {
            crate::liquid_mass::bind(
                run,
                qualification,
                water.remaining_liquid_kg(),
                self.state.temperature_c(),
            )?
        };
        let value = serde_json::to_value(report).map_err(|e| invalid(e.to_string()))?;
        self.mechanical_coupling = Some(value.clone());
        Ok(value)
    }

    pub(super) fn advance_coupled(
        &mut self,
        solver: &mut crate::Solver,
        sample: &crate::motion::MotionSample,
        time_s: f64,
        interval: usize,
        sample_index: usize,
    ) -> Result<(), Refusal> {
        let exchange = self
            .request
            .mechanical_coupling
            .as_ref()
            .is_some_and(|q| q.schema == "ftx-coupled-liquid-mechanics/1");
        if exchange {
            self.water
                .as_mut()
                .ok_or_else(|| invalid("missing coupled textile water"))?
                .begin_mechanical_interval();
        }
        self.advance(
            &sample.cloth,
            time_s,
            interval,
            sample_index,
            sample.membrane_plastic_dissipation_j.as_deref(),
            sample.membrane_relaxation_dissipation_j.as_deref(),
            sample.bending_plastic_dissipation_j.as_deref(),
        )?;
        let water = self
            .water
            .as_ref()
            .ok_or_else(|| invalid("missing coupled textile-water history"))?;
        let report = if exchange {
            water.accept_mechanical_exchange(self.state.temperature_c(), solver)?
        } else {
            solver.evaporate_liquid(water.remaining_liquid_kg(), self.state.temperature_c())?
        };
        self.trace
            .last_mut()
            .ok_or_else(|| invalid("missing accepted coupled heat sample"))?["mechanical_coupling"] =
            report.summary();
        self.mechanical_coupling =
            Some(serde_json::to_value(report).map_err(|e| invalid(e.to_string()))?);
        Ok(())
    }

    fn supplied_heat_j(&self) -> f64 {
        self.plastic
            .as_ref()
            .map_or(0., plastic::Heating::supplied_j)
            + self
                .relaxation
                .as_ref()
                .map_or(0., plastic::Heating::supplied_j)
            + self
                .bending
                .as_ref()
                .map_or(0., plastic::Heating::supplied_j)
    }

    fn balance_error_j(&self) -> f64 {
        self.stored_j
            + self
                .water
                .as_ref()
                .map_or(0., surface_water::Water::energy_correction_j)
            - self.net_j
            - self.supplied_heat_j()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn advance(
        &mut self,
        positions: &[Vec3],
        time_s: f64,
        interval: usize,
        sample_index: usize,
        plastic_dissipation_j: Option<&[f64]>,
        relaxation_dissipation_j: Option<&[f64]>,
        bending_dissipation_j: Option<&[f64]>,
    ) -> Result<(), Refusal> {
        // Temperature, liquid and every opted-in mechanical-source counter commit together only
        // after the complete accepted interval succeeds, including all event-driven substeps.
        if self.water.is_some() {
            let mut trial = self.clone();
            trial.advance_inner(
                positions,
                time_s,
                interval,
                sample_index,
                plastic_dissipation_j,
                relaxation_dissipation_j,
                bending_dissipation_j,
            )?;
            *self = trial;
            Ok(())
        } else {
            self.advance_inner(
                positions,
                time_s,
                interval,
                sample_index,
                plastic_dissipation_j,
                relaxation_dissipation_j,
                bending_dissipation_j,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn advance_inner(
        &mut self,
        positions: &[Vec3],
        time_s: f64,
        interval: usize,
        sample_index: usize,
        plastic_dissipation_j: Option<&[f64]>,
        relaxation_dissipation_j: Option<&[f64]>,
        bending_dissipation_j: Option<&[f64]>,
    ) -> Result<(), Refusal> {
        let seconds = time_s - self.time_s;
        if !time_s.is_finite() || seconds <= 0. || self.trace.len() >= 512 {
            return Err(invalid(
                "motion heat needs increasing accepted sample times and at most 512 samples",
            ));
        }
        let film = *self
            .request
            .boundaries
            .get(interval)
            .ok_or_else(|| invalid("motion heat has no boundary for this interval"))?;
        let boundaries = vec![boundary(film)?; self.model.column_count()];
        let exteriors = self
            .request
            .exterior_exchange
            .as_ref()
            .map(|e| {
                e.get(interval)
                    .map(|e| vec![e.clone(); self.model.column_count()])
                    .ok_or_else(|| {
                        invalid("motion heat has no exterior environment for this interval")
                    })
            })
            .transpose()?;
        let count = (seconds / self.request.max_step_s).ceil();
        let cell_cost = if self.plastic.is_some()
            || self.relaxation.is_some()
            || self.bending.is_some()
            || (self.water.is_some() && self.model.requires_enthalpy_inverse())
        {
            129
        } else {
            1
        };
        let available =
            (self.request.max_cell_steps - self.cell_steps) / (self.model.cells() * cell_cost);
        if !count.is_finite() || count < 1. || count > available as f64 {
            return Err(budget(
                "motion heat cell-step budget exhausted; no partial history published",
            ));
        }
        let count = count as usize;
        let maximum_dt = seconds / count as f64;
        let mut source = self
            .plastic
            .as_mut()
            .map(|heating| {
                heating.check_temperatures(self.state.temperature_c())?;
                heating.increment(plastic_dissipation_j, self.request.cells_per_layer)
            })
            .transpose()?;
        for (heating, work) in [
            (&mut self.relaxation, relaxation_dissipation_j),
            (&mut self.bending, bending_dissipation_j),
        ] {
            let Some(heating) = heating else {
                continue;
            };
            heating.check_temperatures(self.state.temperature_c())?;
            let values = heating.increment(work, self.request.cells_per_layer)?;
            if let Some(total) = &mut source {
                if total.len() != values.len()
                    || total.iter().zip(&values).any(|(a, b)| a.len() != b.len())
                {
                    return Err(invalid("dissipation heat source topologies differ"));
                }
                for (a, b) in total.iter_mut().flatten().zip(values.iter().flatten()) {
                    *a += b;
                    if !a.is_finite() {
                        return Err(invalid("combined dissipation heat overflows"));
                    }
                }
            } else {
                source = Some(values);
            }
        }
        if !self.supplied_heat_j().is_finite() {
            return Err(invalid("combined dissipation heat accumulation overflows"));
        }
        let mut substep = 0usize;
        let mut regular_interval = 0usize;
        let mut elapsed = 0.;
        let mut source_remaining = source.clone();
        while if self.water.is_some() {
            regular_interval < count
        } else {
            substep < count
        } {
            if substep >= available {
                return Err(budget(
                    "surface-water dryout substeps exceed the thermal cell-step budget; no partial history published",
                ));
            }
            // Anchor regular endpoints instead of accumulating dt: non-binary subdivisions must
            // not create an epsilon-sized extra step. Dryout only splits its own regular interval.
            let scheduled_end = if regular_interval + 1 == count {
                seconds
            } else {
                seconds * (regular_interval + 1) as f64 / count as f64
            };
            let proposed_dt = if self.water.is_some() {
                scheduled_end - elapsed
            } else {
                maximum_dt
            };
            let water_plan = self
                .water
                .as_ref()
                .map(|water| water.plan(&self.model, &self.state, positions, interval, proposed_dt))
                .transpose()?;
            let dt = water_plan.as_ref().map_or(proposed_dt, |plan| plan.seconds);
            let final_substep = dt == seconds - elapsed;
            if elapsed + dt <= elapsed {
                return Err(invalid(
                    "surface-water thermal substep loses positive physical time",
                ));
            }
            let heat = source.as_ref().map(|patches| {
                patches
                    .iter()
                    .enumerate()
                    .map(|(patch, cells)| {
                        cells
                            .iter()
                            .enumerate()
                            .map(|(cell, q)| {
                                if self.water.is_some() {
                                    let remaining = &mut source_remaining
                                        .as_mut()
                                        .expect("source remaining")[patch][cell];
                                    let part = if final_substep {
                                        *remaining
                                    } else {
                                        q * (dt / seconds)
                                    };
                                    *remaining -= part;
                                    return part;
                                }
                                let part = q / count as f64;
                                if substep + 1 == count {
                                    q - part * (count - 1) as f64
                                } else {
                                    part
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            });
            let next = if let Some(plan) = water_plan {
                if heat.as_ref().is_some_and(|patches| {
                    patches.iter().flatten().any(|q| !q.is_finite() || *q < 0.)
                }) {
                    return Err(invalid(
                        "surface-water substep loses a nonnegative mechanical heat contribution",
                    ));
                }
                let mut signed_heat = heat.unwrap_or_else(|| {
                    self.state
                        .temperature_c()
                        .iter()
                        .map(|cells| vec![0.; cells.len()])
                        .collect()
                });
                for (cells, liquid_heat) in signed_heat.iter_mut().zip(&plan.liquid_heat_j) {
                    cells[0] += liquid_heat;
                }
                let mut next = self.model.advance_with_liquid(
                    &self.state,
                    positions,
                    dt,
                    &boundaries,
                    exteriors.as_deref(),
                    &signed_heat,
                    &plan.capacity_j_k,
                )?;
                self.water.as_mut().expect("planned water").accept(
                    &self.state,
                    &mut next,
                    plan,
                    interval,
                )?;
                next
            } else {
                match (&exteriors, &heat) {
                    (Some(e), Some(q)) => self.model.advance_exterior_with_heat(
                        &self.state,
                        positions,
                        dt,
                        &boundaries,
                        e,
                        q,
                    )?,
                    (None, Some(q)) => {
                        self.model
                            .advance_with_heat(&self.state, positions, dt, &boundaries, q)?
                    }
                    (Some(e), None) => {
                        self.model
                            .advance_exterior(&self.state, positions, dt, &boundaries, e)?
                    }
                    (None, None) => self
                        .model
                        .advance(&self.state, positions, dt, &boundaries)?,
                }
            };
            for heating in [&self.plastic, &self.relaxation, &self.bending]
                .into_iter()
                .flatten()
            {
                heating.check_temperatures(next.state.temperature_c())?;
            }
            self.inside_j += dt * next.inside_heat_w;
            self.outside_j += dt * next.outside_heat_w;
            if let Some(e) = &next.exterior_exchange {
                self.convective_j += dt * e.convective_heat_w;
                self.radiative_j += dt * e.radiative_heat_w;
            }
            self.net_j += dt * (next.inside_heat_w - next.outside_heat_w);
            self.stored_j += next.stored_change_j;
            self.absolute_error_j += next.balance_error_j.abs();
            self.final_field = Some(SurfaceField {
                area_m2: next.area_m2,
                inside_heat_w: next.inside_heat_w,
                outside_heat_w: next.outside_heat_w,
                seconds: dt,
                stored_change_j: next.stored_change_j,
                patches: next.patches,
                exterior_exchange: next.exterior_exchange,
            });
            self.state = next.state;
            substep += 1;
            if self.water.is_some() && dt == proposed_dt {
                elapsed = scheduled_end;
                regular_interval += 1;
            } else {
                elapsed = if final_substep { seconds } else { elapsed + dt };
            }
        }
        if [
            self.inside_j,
            self.outside_j,
            self.convective_j,
            self.radiative_j,
            self.net_j,
            self.stored_j,
            self.absolute_error_j,
        ]
        .iter()
        .any(|x| !x.is_finite())
        {
            return Err(invalid("motion heat energy accumulation overflow"));
        }
        self.cell_steps += substep * self.model.cells() * cell_cost;
        self.thermal_steps += substep;
        self.time_s = time_s;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &t in self.state.temperature_c().iter().flatten() {
            lo = lo.min(t);
            hi = hi.max(t);
        }
        let field = self
            .final_field
            .as_ref()
            .ok_or_else(|| invalid("missing heat step"))?;
        let mut sample = json!({"sample_index":sample_index,"interval_index":interval,
            "time_s":time_s,"thermal_substeps":substep,"positions_sha256":hash(&positions)?,
            "temperature_range_c":[lo,hi],"inside_heat_w":field.inside_heat_w,
            "outside_heat_w":field.outside_heat_w,
            "inside_energy_j":self.inside_j,"outside_energy_j":self.outside_j,
            "net_boundary_energy_j":self.net_j,"stored_change_j":self.stored_j,
            "balance_error_j":self.balance_error_j()});
        if let Some(water) = &self.water {
            sample["surface_water"] =
                water.snapshot(self.state.temperature_c(), false, self.balance_error_j())?;
        }
        if let Some(heating) = &self.plastic {
            sample["plastic_dissipation_heat"] = heating.snapshot(false);
        }
        if let Some(heating) = &self.relaxation {
            sample["relaxation_dissipation_heat"] = heating.snapshot(false);
        }
        if let Some(heating) = &self.bending {
            sample["bending_plastic_dissipation_heat"] = heating.snapshot(false);
        }
        if let Some(e) = &field.exterior_exchange {
            sample["exterior_exchange"] = json!({"convective_heat_w":e.convective_heat_w,
                "radiative_heat_w":e.radiative_heat_w,"balance_residual_w":e.balance_residual_w,
                "convective_energy_j":self.convective_j,"radiative_energy_j":self.radiative_j});
        }
        self.trace.push(sample);
        Ok(())
    }

    pub(super) fn finish(
        self,
        path_hash: &str,
        initial_hash: &str,
        duration_s: f64,
    ) -> Result<Value, Refusal> {
        if self.time_s != duration_s || self.trace.is_empty() {
            return Err(invalid(
                "motion heat history does not cover the complete mechanical path",
            ));
        }
        let balance_error_j = self.balance_error_j();
        let water_receipt = self
            .water
            .as_ref()
            .map(|water| water.snapshot(self.state.temperature_c(), true, balance_error_j))
            .transpose()?;
        let f = self
            .final_field
            .ok_or_else(|| invalid("missing final heat field"))?;
        let mut final_field = json!({"temperature_c":self.state.temperature_c(),
            "area_m2":f.area_m2,"inside_heat_w":f.inside_heat_w,"outside_heat_w":f.outside_heat_w,
            "last_step_seconds":f.seconds,"last_step_stored_change_j":f.stored_change_j,
            "patches":f.patches.iter().map(|p| json!({"area_m2":p.area_m2,
                "inside_flux_w_m2":p.inside_flux_w_m2,"outside_flux_w_m2":p.outside_flux_w_m2,
                "last_step_stored_change_j":p.stored_change_j})).collect::<Vec<_>>()});
        if let Some(e) = &f.exterior_exchange {
            final_field["exterior_exchange"] = json!(e);
        }
        let mut result = json!({"schema":"ftx-motion-surface-heat-result/1","request":self.request,
            "model":self.model.snapshot(),"path_sha256":path_hash,
            "initial_positions_sha256":initial_hash,"duration_s":duration_s,
            "initial_temperature_c":self.request.initial_temperature_c,
            "cell_steps":self.cell_steps,"thermal_steps":self.thermal_steps,
            "inside_energy_j":self.inside_j,"outside_energy_j":self.outside_j,
            "net_boundary_energy_j":self.net_j,"stored_change_j":self.stored_j,
            "balance_error_j":balance_error_j,
            "absolute_step_balance_error_j":self.absolute_error_j,
            "trace":self.trace,"final":final_field,
            "uncertainty_propagated":false,"physical_validation_claim":false,"comfort_claim":false,
            "scope":"One-way independent-sheet backward Euler on every accepted mechanical sample's current geometry. Storage follows reference mass. Boundary conditions are uniform in space and piecewise constant by keyframe interval; thermal substeps do not interpolate geometry. No inferred layer overlap, body heat loss, thermomechanical feedback, mechanical heating, moisture, comfort or empirical garment claim."});
        if self.request.exterior_exchange.is_some() {
            result["exterior_exchange"] = json!({"convective_energy_j":self.convective_j,
                "radiative_energy_j":self.radiative_j});
        }
        if let Some(heating) = &self.plastic {
            result["plastic_dissipation_heat"] = heating.snapshot(true);
            result["scope"] = json!(
                "One-way independent-sheet backward Euler on accepted mechanical geometry with explicitly qualified irreversible membrane dissipation converted to heat. Storage and heat deposition follow reference mass. Work is distributed uniformly through reference thickness and over each accepted interval. No inferred overlap, body heat loss, temperature feedback to mechanics, bending/Maxwell/friction heating, moisture, comfort or empirical garment claim."
            );
        }
        if let Some(heating) = &self.relaxation {
            result["relaxation_dissipation_heat"] = heating.snapshot(true);
            result["scope"] = json!(
                "One-way independent-sheet backward Euler on accepted mechanical geometry with explicitly qualified Maxwell dashpot dissipation and, only when separately requested, irreversible membrane yield dissipation converted to heat. Storage and heat deposition follow reference mass. Work is distributed uniformly through reference thickness and over each accepted interval. No initial drape work, reversible spring or hardening storage, backward-Euler numerical loss, bending or friction heating. No inferred overlap, body heat loss, temperature feedback, moisture, comfort or empirical garment claim."
            );
        }
        if let Some(heating) = &self.bending {
            result["bending_plastic_dissipation_heat"] = heating.snapshot(true);
            result["scope"] = json!(
                "One-way independent-sheet heat on accepted mechanical geometry with source-qualified irreversible source-panel bending work, plus independently requested membrane yield and Maxwell dissipation. Each source triangle supplies heat once through reference thickness and over its accepted interval. Initial drape work, recoverable elastic/hardening storage, numerical loss, seam plasticity and contact friction are excluded. No temperature feedback, inferred overlap, body heat loss, moisture, comfort or empirical garment claim."
            );
        }
        if let Some(water) = water_receipt {
            let has_supply = water["supplied_liquid_kg"].is_number();
            result["surface_water"] = water;
            result["scope"] = json!(
                "Independent-sheet heat on accepted mechanical geometry with explicitly qualified initial textile-owned free liquid and latent cooling. Vapor rates and exported sensible enthalpy sample old inner-cell temperature; implicit dry heat uses remaining liquid capacity. Event splitting handles dryout. All liquid and dry-textile energy terms are retained. Separately requested qualified mechanical heat is added once. No condensation, replenishment, sorption, wet-property inference, body-water transfer, temperature feedback to mechanics, added mechanical water mass, comfort or empirical garment claim."
            );
            if has_supply {
                result["scope"] = json!(
                    "Independent-sheet heat on accepted mechanical geometry with source-qualified textile free liquid and separately sourced delivery rates and liquid temperatures. Reference area owns liquid and supply; current area conducts vapor. Latent removal and incoming sensible enthalpy enter the same conservative thermal balance, including independently requested mechanical heat. Exact net-depletion events handle dryout; dry supply can sustain evaporation or rewet the textile. Qualified loading-domain overflow refuses without drainage. No condensation, sorption, wet-property inference, body-water transfer, temperature feedback to mechanics, added mechanical water mass, comfort or empirical garment claim."
                );
            }
        }
        if let Some(coupling) = self.mechanical_coupling {
            let exchange = !coupling["exchange"].is_null();
            result["mechanical_coupling"] = coupling;
            result["scope"] = json!(
                "First-order accepted-step evaporation coupling: cloth and contact use interval-start liquid mass, then the qualified thermal/water solver uses accepted geometry and removes co-moving vapor for the next mechanical interval. Initial liquid is a post-drape co-moving condition, not a wet equilibrium. Frozen mechanical laws are source-qualified over the complete loading and temperature domain. Thermal latent/sensible enthalpy and mechanical exported momentum/bulk energy have separate ledgers. Mechanical step refinement remains necessary. No supply, condensation, drainage, inferred wet laws, body-water transfer or empirical garment/comfort claim."
            );
            result["surface_water"]["scope"] = json!(
                "The enclosing history's mechanical_coupling binds this exact remaining liquid to cloth inertia and gravity. These water fields retain only the independent conservative sensible/latent thermal ledger; exported bulk kinetic and gravitational energy appear in the mechanical ledger. No liquid slip, vapor thrust, supply, condensation or drainage is modeled."
            );
            if exchange {
                result["scope"] = json!(
                    "First-order accepted-step co-moving liquid exchange: each cloth/contact solve uses interval-start liquid mass, then the existing thermal/water solver uses accepted geometry and supplies, condenses or evaporates liquid for the next mechanical solve. Initial liquid is a post-dry-drape condition. Exact material/interface laws are qualified over the complete thermal/loading domain; all incoming and outgoing mean velocities equal local endpoint cloth velocity. Thermal sensible/latent energy stays separate from carried momentum and bulk kinetic/gravitational energy. Any failure rolls back the whole coupled motion. Mechanical and thermal refinement remain necessary. No relative recoil, drainage, unmeasured wet law, body transfer or empirical garment/comfort claim."
                );
                result["surface_water"]["scope"] = json!(
                    "The enclosing mechanical_coupling uses these exact reference-owned remaining, supplied, condensed and evaporated masses. The shared thermal authority retains their conservative sensible/latent ledger; the mechanical authority separately retains co-moving momentum and bulk energy. No water is counted twice and no relative flow velocity, drainage or wet constitutive change is inferred."
                );
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thermal::{DryThermalModel, surface::Patch};
    use ftx_pattern::{material_catalog::Selection, materials::Catalog};

    fn policy() -> SurfaceRequest {
        serde_json::from_value(json!({"schema":"ftx-motion-surface-heat/1",
            "initial_temperature_c":10.,"cells_per_layer":1,"max_step_s":0.5,
            "max_cell_steps":100,"boundaries":[{"inside_c":30.,"outside_c":0.,
                "inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2}]}))
        .unwrap()
    }
    fn model() -> Surface {
        model_with_emissivity(None)
    }

    fn model_with_emissivity(emissivity: Option<f64>) -> Surface {
        let mut selection = json!({"id":"test-heat","allow_estimates":true,
        "record":{"id":"test-heat","name":"Synthetic heat test","form":"woven",
            "sources":{"test":{"reference":"test:heat-history","accessed_on":"2026-10-17",
                "note":"Synthetic known-answer data, not textile characterization"}},
            "properties":{
                "areal_density":{"value":0.2,"unit":"kg/m2","evidence":"estimate","source":"test","method":"synthetic"},
                "specific_heat":{"value":1000.,"unit":"J/(kg*K)","evidence":"estimate","source":"test","method":"synthetic"},
                "thermal_resistance":{"value":0.4,"unit":"m2*K/W","evidence":"estimate","source":"test","method":"synthetic"}
            }}});
        if let Some(value) = emissivity {
            selection["record"]["properties"]["emissivity"] = json!({"value":value,
                "unit":"1","evidence":"estimate","source":"test","method":"Synthetic optical test",
                "conditions":{"face":"public","spectral_basis":"total_hemispherical",
                    "specimen":"synthetic-sheet","conditioning":"synthetic-dry",
                    "temperature_min_c":"-80","temperature_max_c":"80"}});
        }
        let selection: Selection = serde_json::from_value(selection).unwrap();
        let m = Catalog::builtin().unwrap().resolve(&selection).unwrap();
        Surface::new(
            vec![DryThermalModel::new(vec![m], vec![], 1).unwrap()],
            3,
            vec![Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            }],
        )
        .unwrap()
    }

    fn exterior_policy() -> SurfaceRequest {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
        ))
        .unwrap();
        let mut request = policy();
        request.boundaries[0].outside_c = 5.;
        request.boundaries[0].outside_resistance_m2_k_w = 0.;
        request.exterior_exchange = Some(vec![
            serde_json::from_value(fixture["exterior_exchange"].clone()).unwrap(),
        ]);
        request
    }

    #[test]
    fn moving_area_history_matches_independent_backward_euler_recurrence() {
        let mut history = SurfaceHistory::new(policy(), model()).unwrap();
        let mut temperature = 10.;
        for (i, area) in [0.5, 1., 0.125, 0.5].into_iter().enumerate() {
            let x = [
                Vec3::ZERO,
                Vec3::new(2. * area, 0., 0.),
                Vec3::new(0., 1., 0.),
            ];
            // Two half-second heat steps per accepted pose; capacity is ALWAYS 100 J/K,
            // irrespective of the current exchange area. R/2 plus the declared boundary films.
            for _ in 0..2 {
                temperature = (100. / 0.5 * temperature + area * 30. / 0.3)
                    / (100. / 0.5 + area * (1. / 0.3 + 1. / 0.4));
            }
            history
                .advance(&x, (i + 1) as f64, 0, i, None, None, None)
                .unwrap();
            assert!((history.state.temperature_c()[0][0] - temperature).abs() < 1e-12);
        }
        let h = history.finish("path", "initial", 4.).unwrap();
        assert!(
            (h["stored_change_j"].as_f64().unwrap() - 100. * (temperature - 10.)).abs() < 1e-10
        );
        assert!(h["balance_error_j"].as_f64().unwrap().abs() < 1e-10);
        assert_eq!(h["cell_steps"], 8);
        assert!(h["request"].get("exterior_exchange").is_none());
        assert!(h.get("exterior_exchange").is_none());
        assert!(h["trace"][0].get("exterior_exchange").is_none());
    }

    #[test]
    fn thermal_policy_and_aggregate_reservation_never_silently_clamp() {
        let good = policy();
        good.validate(1).unwrap();
        assert!(good.validate(2).is_err());
        for bad in [0., f64::NAN, f64::INFINITY, -1.] {
            let mut p = good.clone();
            p.max_step_s = bad;
            assert!(p.validate(1).is_err());
        }
        let mut p = good.clone();
        p.boundaries[0].outside_c = -300.;
        assert!(p.validate(1).is_err());
        p = good;
        p.max_cell_steps = 10_000_001;
        assert!(p.validate(1).is_err());
        let mut b = Budget::default();
        b.reserve(10_000_000).unwrap();
        b.reserve(10_000_000).unwrap();
        assert!(b.reserve(1).is_err());
        assert!(b.reserve(1).is_err());
    }

    #[test]
    fn exterior_motion_tracks_each_interval_and_material_change_without_heat_reset() {
        let mut request = exterior_policy();
        request.boundaries.push(Film {
            outside_c: 20.,
            ..request.boundaries[0]
        });
        let mut second = request.exterior_exchange.as_ref().unwrap()[0].clone();
        second.air_c = 20.;
        second.radiation.as_mut().unwrap().ambient_c = 25.;
        request.exterior_exchange.as_mut().unwrap().push(second);
        request.validate(2).unwrap();
        let model = model_with_emissivity(Some(0.8));
        let mut history = SurfaceHistory::new(request.clone(), model.clone()).unwrap();
        let mut old = model.initial(request.initial_temperature_c).unwrap();
        let (mut convective, mut radiative) = (0., 0.);
        let mut final_exchange = None;
        for (interval, area) in [0.5, 1.].into_iter().enumerate() {
            let x = [
                Vec3::ZERO,
                Vec3::new(2. * area, 0., 0.),
                Vec3::new(0., 1., 0.),
            ];
            let boundaries = [boundary(request.boundaries[interval]).unwrap()];
            let exteriors = [request.exterior_exchange.as_ref().unwrap()[interval].clone()];
            for _ in 0..2 {
                let next = model
                    .advance_exterior(&old, &x, 0.5, &boundaries, &exteriors)
                    .unwrap();
                let e = next.exterior_exchange.as_ref().unwrap();
                convective += 0.5 * e.convective_heat_w;
                radiative += 0.5 * e.radiative_heat_w;
                final_exchange = next.exterior_exchange;
                old = next.state;
            }
            history
                .advance(
                    &x,
                    (interval + 1) as f64,
                    interval,
                    interval,
                    None,
                    None,
                    None,
                )
                .unwrap();
            assert_eq!(history.state.temperature_c(), old.temperature_c());
            let trace = &history.trace[interval]["exterior_exchange"];
            assert_eq!(trace["convective_energy_j"], json!(convective));
            assert_eq!(trace["radiative_energy_j"], json!(radiative));
            assert_eq!(
                trace["radiative_heat_w"],
                json!(final_exchange.as_ref().unwrap().radiative_heat_w)
            );
        }
        let result = history.finish("path", "initial", 2.).unwrap();
        assert_eq!(result["request"], json!(request));
        assert_eq!(
            result["final"]["exterior_exchange"],
            json!(final_exchange.unwrap())
        );
        assert!(
            (result["outside_energy_j"].as_f64().unwrap() - convective - radiative).abs() < 1e-6
        );
        assert!(
            (result["stored_change_j"].as_f64().unwrap()
                - 100. * (old.temperature_c()[0][0] - 10.))
                .abs()
                < 1e-7
        );
        assert!(result["balance_error_j"].as_f64().unwrap().abs() < 1e-6);
        let mut changed = SurfaceHistory::new(request, model_with_emissivity(Some(0.05))).unwrap();
        for (interval, area) in [0.5, 1.].into_iter().enumerate() {
            let x = [
                Vec3::ZERO,
                Vec3::new(2. * area, 0., 0.),
                Vec3::new(0., 1., 0.),
            ];
            changed
                .advance(
                    &x,
                    (interval + 1) as f64,
                    interval,
                    interval,
                    None,
                    None,
                    None,
                )
                .unwrap();
        }
        let changed = changed.finish("path", "initial", 2.).unwrap();
        assert_eq!(result["request"], changed["request"]);
        assert_eq!(
            result["trace"][1]["positions_sha256"],
            changed["trace"][1]["positions_sha256"]
        );
        assert_ne!(result["model"]["sha256"], changed["model"]["sha256"]);
        assert_ne!(
            result["final"]["temperature_c"],
            changed["final"]["temperature_c"]
        );
    }

    #[test]
    fn exterior_motion_requires_every_interval_and_never_silently_uses_films() {
        let request = exterior_policy();
        request.validate(1).unwrap();
        let mut missing = request.clone();
        missing.exterior_exchange = Some(vec![]);
        assert!(missing.validate(1).is_err());
        missing = request.clone();
        missing.boundaries.push(missing.boundaries[0]);
        assert!(missing.validate(2).is_err());
        let mut bad = request.clone();
        bad.boundaries[0].outside_resistance_m2_k_w = 0.1;
        assert!(bad.validate(1).is_err());
        bad = request.clone();
        bad.boundaries[0].outside_c = 6.;
        assert!(bad.validate(1).is_err());
        let x = [Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)];
        let mut unqualified = SurfaceHistory::new(request, model()).unwrap();
        assert!(unqualified.advance(&x, 1., 0, 0, None, None, None).is_err());
        assert!(unqualified.finish("path", "initial", 1.).is_err());
    }
}
