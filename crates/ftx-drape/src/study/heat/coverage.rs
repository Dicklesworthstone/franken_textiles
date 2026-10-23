//! Transient heat on actual moving body/layer overlap. Repartitioning transfers source-material
//! enthalpy without elapsed time; only subsequent physical steps exchange heat with reservoirs.
use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    Budget, DissipationHeatDistribution, PlasticHeatDistribution, boundary, budget, hash, invalid,
    plastic,
};
use crate::{
    motion::DissipationCapture,
    run::DrapeRun,
    thermal::{
        steady::{exterior::Exterior, request::Film},
        surface::{
            Surface,
            coverage::{
                ExteriorConditions,
                motion::{ExposedConditions, Frame, State, Step},
            },
            projection,
        },
    },
};

/// One explicit, source-locked body domain followed through every accepted mechanical sample.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-motion-coverage-heat/1`.
    pub schema: String,
    /// Unique collision-body triangle IDs in this exact scenario, not a saved different pose.
    pub body_triangle_ids: Vec<usize>,
    /// Physical layer name for every retained drape instance, with no missing or extra assignments.
    pub instance_layers: BTreeMap<String, String>,
    /// Existing geometric closure, thickness discretization and sourced/declared interfaces.
    pub settings: projection::Settings,
    /// Uniform cloth temperature at motion time zero (degrees Celsius), not a fitted temperature.
    pub initial_temperature_c: f64,
    /// One spatially uniform pair of body-side/exterior films for each keyframe interval.
    pub boundaries: Vec<Film>,
    /// Optional exact interval schedule; null retains that interval's outside film. Each active
    /// environment replaces the outside film on the outermost covered material face only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exterior_exchange: Option<Vec<Option<Exterior>>>,
    /// Explicit bare-body films for each exterior interval, independently of the covered shell.
    /// Null is required in film-only intervals; skin emissivity is never inferred from cloth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bare_boundaries: Option<Vec<Option<Film>>>,
    /// One explicit choice per interval: null is adiabatic exposed cloth; a film pair applies to
    /// all currently exposed source sheets in their original material orientation. No ambient guess.
    pub exposed_boundaries: Vec<Option<Film>>,
    /// Optional exact interval schedule for uncovered source cloth, independent of covered
    /// exterior exchange. Every active entry also requires that interval's exposed boundaries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposed_exterior_exchange: Option<Vec<Option<Exterior>>>,
    /// Convert qualified accepted irreversible membrane work through the same source-material
    /// fractions as independent sheets. Deposition follows covered AND exposed reference material;
    /// initial drape work is excluded and bare body receives no internal energy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plastic_dissipation_heat: Option<PlasticHeatDistribution>,
    /// Convert separately qualified Maxwell dashpot work over the same covered/exposed material.
    /// Initial drape work, reversible storage and backward-Euler numerical loss are excluded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relaxation_dissipation_heat: Option<DissipationHeatDistribution>,
    /// Convert independently qualified source-panel bending work over covered and exposed
    /// reference material. Initial drape work, elastic/hardening storage and seam work are excluded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bending_plastic_dissipation_heat: Option<DissipationHeatDistribution>,
    /// Maximum physical thermal substep, seconds, independent of mechanical adaptive steps.
    pub max_step_s: f64,
    /// Maximum cell plus body-face advances, including all substeps, in 1..=10000000. Heating
    /// charges 129 operations per temperature cell for bounded enthalpy inversion and transport.
    pub max_cell_steps: usize,
    /// Cumulative projection/remap operations, in 1..=20000000; checked after each bounded kernel.
    pub max_geometry_operations: usize,
}

impl Request {
    pub(in crate::study) fn start(
        &self,
        run: &DrapeRun,
        body: (&[Vec3], &[[usize; 3]]),
        intervals: usize,
        work: &mut Budget,
    ) -> Result<History, Refusal> {
        if self.schema != "ftx-motion-coverage-heat/1"
            || !(1..=16).contains(&intervals)
            || self.boundaries.len() != intervals
            || self.exposed_boundaries.len() != intervals
            || self
                .exterior_exchange
                .as_ref()
                .is_some_and(|v| v.len() != intervals)
            || self
                .bare_boundaries
                .as_ref()
                .is_some_and(|v| v.len() != intervals)
            || self
                .exposed_exterior_exchange
                .as_ref()
                .is_some_and(|v| v.len() != intervals)
            || !self.initial_temperature_c.is_finite()
            || self.initial_temperature_c < -273.15
            || !self.max_step_s.is_finite()
            || !(1e-9..=1.).contains(&self.max_step_s)
            || !(1..=64).contains(&self.settings.cells_per_layer)
            || !(1..=10_000_000).contains(&self.max_cell_steps)
            || !(1..=20_000_000).contains(&self.max_geometry_operations)
        {
            return Err(invalid(
                "invalid motion coverage heat schema, schedule, initial state or budgets",
            ));
        }
        for &film in self
            .boundaries
            .iter()
            .chain(self.exposed_boundaries.iter().flatten())
            .chain(self.bare_boundaries.iter().flatten().flatten())
        {
            boundary(film)?;
        }
        for i in 0..intervals {
            let exterior = self.exterior_exchange.as_ref().and_then(|v| v[i].as_ref());
            let bare = self.bare_boundaries.as_ref().and_then(|v| v[i]);
            if exterior.is_some() != bare.is_some() {
                return Err(invalid(
                    "each covered exterior interval requires explicit bare-body films; film-only intervals must leave bare_boundaries null",
                ));
            }
            if let Some(e) = exterior {
                exterior_boundary(self.boundaries[i], e)?;
            }
            if let Some(e) = self
                .exposed_exterior_exchange
                .as_ref()
                .and_then(|v| v[i].as_ref())
            {
                let film = self.exposed_boundaries[i].ok_or_else(|| {
                    invalid("exposed exterior exchange requires explicit exposed boundaries")
                })?;
                exterior_boundary(film, e)?;
            }
        }
        let mut seen = BTreeSet::new();
        if self.body_triangle_ids.is_empty()
            || self.body_triangle_ids.len() > 4096
            || self
                .body_triangle_ids
                .iter()
                .any(|&i| i >= body.1.len() || !seen.insert(i))
        {
            return Err(invalid(
                "motion coverage needs 1..4096 unique current collision-body triangle IDs",
            ));
        }
        if self.instance_layers.len() != run.scene.mesh.instances.len()
            || run.scene.mesh.instances.iter().any(|id| {
                self.instance_layers
                    .get(id)
                    .is_none_or(|s| s.trim().is_empty())
            })
        {
            return Err(invalid(
                "motion coverage requires exact retained instance/layer assignments",
            ));
        }
        // Even a later compile/remap failure consumes this conservative session reservation.
        work.reserve(self.max_cell_steps)?;
        work.reserve_geometry(self.max_geometry_operations)?;
        let source = run.thermal_surface(self.settings.cells_per_layer)?;
        let triangles: Vec<_> = self.body_triangle_ids.iter().map(|&i| body.1[i]).collect();
        let projection = projection::Request {
            schema: "ftx-thermal-projection/1".into(),
            cloth_layers: run
                .scene
                .triangle_owner
                .iter()
                .map(|&owner| self.instance_layers[&run.scene.mesh.instances[owner]].clone())
                .collect(),
            settings: self.settings.clone(),
        };
        let frame = Frame::new(
            source.clone(),
            run.scene.solver.x.clone(),
            body.0.to_vec(),
            &triangles,
            projection.clone(),
        )?;
        let geometry_operations = frame.projection().geometry_operations;
        if geometry_operations > self.max_geometry_operations || frame.cells() > self.max_cell_steps
        {
            return Err(budget(
                "motion coverage initial geometry/cells exceed history budget",
            ));
        }
        let initial_frame_sha256 = frame.sha256().to_owned();
        let state = frame.initial(self.initial_temperature_c)?;
        let plastic = self
            .plastic_dissipation_heat
            .map(|_| plastic::Heating::new(run, self.initial_temperature_c))
            .transpose()?;
        let relaxation = self
            .relaxation_dissipation_heat
            .map(|_| {
                plastic::Heating::new_kind(
                    run,
                    self.initial_temperature_c,
                    plastic::Kind::Relaxation,
                )
            })
            .transpose()?;
        let bending = self
            .bending_plastic_dissipation_heat
            .map(|_| {
                plastic::Heating::new_kind(run, self.initial_temperature_c, plastic::Kind::Bending)
            })
            .transpose()?;
        Ok(History {
            request: self.clone(),
            source,
            triangles,
            projection,
            frame,
            state,
            initial_frame_sha256,
            geometry_operations,
            time_s: 0.,
            cell_steps: 0,
            thermal_steps: 0,
            inside_j: 0.,
            outside_j: 0.,
            covered_convective_j: 0.,
            covered_radiative_j: 0.,
            exposed_convective_j: 0.,
            exposed_radiative_j: 0.,
            net_j: 0.,
            stored_j: 0.,
            bare_j: 0.,
            signed_remap_j: 0.,
            absolute_remap_j: 0.,
            absolute_step_error_j: 0.,
            trace: Vec::new(),
            final_field: Value::Null,
            plastic,
            relaxation,
            bending,
        })
    }
}

fn exterior_boundary(film: Film, exterior: &Exterior) -> Result<(), Refusal> {
    if film.outside_resistance_m2_k_w != 0. || film.outside_c != exterior.air_c {
        return Err(invalid(
            "motion coverage exterior exchange requires zero outside film and outside_c equal to air_c in every active interval",
        ));
    }
    Ok(())
}

pub(in crate::study) struct History {
    request: Request,
    source: Surface,
    triangles: Vec<[usize; 3]>,
    projection: projection::Request,
    frame: Frame,
    state: State,
    initial_frame_sha256: String,
    geometry_operations: usize,
    time_s: f64,
    cell_steps: usize,
    thermal_steps: usize,
    inside_j: f64,
    outside_j: f64,
    covered_convective_j: f64,
    covered_radiative_j: f64,
    exposed_convective_j: f64,
    exposed_radiative_j: f64,
    net_j: f64,
    stored_j: f64,
    bare_j: f64,
    signed_remap_j: f64,
    absolute_remap_j: f64,
    absolute_step_error_j: f64,
    trace: Vec<Value>,
    final_field: Value,
    plastic: Option<plastic::Heating>,
    relaxation: Option<plastic::Heating>,
    bending: Option<plastic::Heating>,
}

impl History {
    pub(in crate::study) fn dissipation_capture(&self) -> DissipationCapture {
        DissipationCapture {
            plastic: self.plastic.is_some(),
            relaxation: self.relaxation.is_some(),
            bending: self.bending.is_some(),
        }
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

    fn geometry(&mut self, n: usize) -> Result<(), Refusal> {
        self.geometry_operations = self.geometry_operations.saturating_add(n);
        if self.geometry_operations > self.request.max_geometry_operations {
            return Err(budget(
                "motion coverage geometry/remap budget exhausted; no partial heat published",
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::study) fn advance(
        &mut self,
        cloth: &[Vec3],
        body: &[Vec3],
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
                "motion coverage needs increasing sample times and at most 512 samples",
            ));
        }
        let film = *self
            .request
            .boundaries
            .get(interval)
            .ok_or_else(|| invalid("missing motion coverage interval"))?;
        let exposed = *self
            .request
            .exposed_boundaries
            .get(interval)
            .ok_or_else(|| invalid("missing exposed-cloth interval"))?;
        let exterior = self
            .request
            .exterior_exchange
            .as_ref()
            .and_then(|v| v[interval].clone());
        let bare = self
            .request
            .bare_boundaries
            .as_ref()
            .and_then(|v| v[interval]);
        let exposed_exterior = self
            .request
            .exposed_exterior_exchange
            .as_ref()
            .and_then(|v| v[interval].clone());
        let next = Frame::new(
            self.source.clone(),
            cloth.to_vec(),
            body.to_vec(),
            &self.triangles,
            self.projection.clone(),
        )?;
        self.geometry(next.projection().geometry_operations)?;
        let heated = self.plastic.is_some() || self.relaxation.is_some() || self.bending.is_some();
        let cell_cost = if heated { 129 } else { 1 };
        let unit = next
            .cells()
            .checked_mul(cell_cost)
            .ok_or_else(|| budget("motion coverage heated cell overflow"))?
            .checked_add(next.body_triangle_count())
            .ok_or_else(|| budget("motion coverage cell overflow"))?;
        let count = (seconds / self.request.max_step_s).ceil();
        if !count.is_finite()
            || count < 1.
            || count > ((self.request.max_cell_steps - self.cell_steps) / unit) as f64
        {
            return Err(budget(
                "motion coverage cell-step budget exhausted; no partial heat published",
            ));
        }
        let count = count as usize;
        let dt = seconds / count as f64;
        let remap = if next.sha256() == self.frame.sha256() {
            None
        } else {
            let transfer = next.remap(&self.frame, &self.state)?;
            self.geometry(transfer.balance.operations)?;
            self.signed_remap_j += transfer.balance.residual_j;
            self.absolute_remap_j += transfer.balance.absolute_residual_j;
            self.state = transfer.state;
            Some(transfer.balance)
        };
        self.frame = next;
        let source = if heated {
            let temperatures = self.frame.source_temperatures(&self.state)?;
            let mut total = vec![0.0; self.frame.cloth_triangle_count()];
            for (heating, work) in [
                (&mut self.plastic, plastic_dissipation_j),
                (&mut self.relaxation, relaxation_dissipation_j),
                (&mut self.bending, bending_dissipation_j),
            ] {
                if let Some(heating) = heating {
                    heating.check_temperatures(&temperatures)?;
                    // Channels retain separate source/work receipts. Sum whole-triangle joules
                    // once; the core owns covered/exposed allocation and material depth distribution.
                    let values = heating.increment(work, 1)?;
                    if values.len() != total.len() {
                        return Err(invalid("coverage dissipation heat source topology changed"));
                    }
                    for (sum, row) in total.iter_mut().zip(values) {
                        *sum += row[0];
                        if !sum.is_finite() {
                            return Err(invalid("combined coverage dissipation heat overflows"));
                        }
                    }
                }
            }
            Some(total)
        } else {
            None
        };
        let boundaries = vec![boundary(film)?; self.frame.body_triangle_count()];
        let environments = exterior.map(|e| vec![e; self.frame.body_triangle_count()]);
        let bare_boundaries = bare
            .map(|b| boundary(b).map(|b| vec![b; self.frame.body_triangle_count()]))
            .transpose()?;
        let covered_exterior = environments.as_deref().map(|e| ExteriorConditions {
            environments: e,
            bare_boundaries: bare_boundaries.as_deref().expect("validated bare schedule"),
        });
        // Declared exterior policies persist at zero active area: the core returns an empty,
        // zero-power field without qualifying hidden optics. Film-only omission is unchanged.
        let exposed = exposed
            .filter(|_| self.frame.has_exposed_cloth() || exposed_exterior.is_some())
            .map(|film| boundary(film).map(|b| vec![b; self.frame.cloth_triangle_count()]))
            .transpose()?;
        let exposed_environments =
            exposed_exterior.map(|e| vec![e; self.frame.cloth_triangle_count()]);
        let exposed_conditions = exposed.as_deref().map(|b| ExposedConditions {
            boundaries: b,
            environments: exposed_environments.as_deref(),
        });
        // Move each state into the next step; only the last field is retained/formatted for output.
        let mut first = Some(self.state.clone());
        let mut last: Option<Step> = None;
        for substep in 0..count {
            let current = first
                .take()
                .or_else(|| last.take().map(Step::into_state))
                .ok_or_else(|| invalid("missing transient coverage state"))?;
            let step = if let Some(source) = &source {
                let heat: Vec<_> = source
                    .iter()
                    .map(|&q| {
                        let part = q / count as f64;
                        if substep + 1 == count {
                            q - part * (count - 1) as f64
                        } else {
                            part
                        }
                    })
                    .collect();
                self.frame.advance_with_exchange_and_heat(
                    &current,
                    dt,
                    &boundaries,
                    covered_exterior,
                    exposed_conditions,
                    &heat,
                )?
            } else {
                self.frame.advance_with_exchange(
                    &current,
                    dt,
                    &boundaries,
                    covered_exterior,
                    exposed_conditions,
                )?
            };
            if heated {
                let temperatures = self.frame.source_temperatures(&step.clone().into_state())?;
                for heating in [&self.plastic, &self.relaxation, &self.bending]
                    .into_iter()
                    .flatten()
                {
                    heating.check_temperatures(&temperatures)?;
                }
            }
            self.inside_j += dt * step.inside_heat_w();
            self.outside_j += dt * step.outside_heat_w();
            if let Some(e) = &step.heat().exterior_exchange {
                self.covered_convective_j += dt * e.convective_heat_w;
                self.covered_radiative_j += dt * e.radiative_heat_w;
            }
            if let Some(e) = step.exposed_exterior_exchange() {
                self.exposed_convective_j += dt * e.convective_heat_w;
                self.exposed_radiative_j += dt * e.radiative_heat_w;
            }
            self.net_j += dt * step.net_heat_w();
            self.stored_j += step.stored_change_j();
            self.bare_j += dt * step.heat().bare_heat_w;
            self.absolute_step_error_j += step.balance_error_j().abs();
            last = Some(step);
        }
        let last = last.ok_or_else(|| invalid("missing final coverage heat field"))?;
        let heat = last.heat();
        self.final_field = json!({"body_area_m2":heat.body_area_m2,"covered_area_m2":heat.covered_area_m2,
            "bare_heat_w":heat.bare_heat_w,"body_inside_heat_w":heat.inside_heat_w,"body_outside_heat_w":heat.outside_heat_w,
            "exposed_inside_heat_w":last.exposed_inside_heat_w(),"exposed_outside_heat_w":last.exposed_outside_heat_w(),
            "body":heat.body,"regions":heat.regions});
        if let Some(e) = &heat.exterior_exchange {
            self.final_field["exterior_exchange"] = json!(e);
        }
        if let Some(e) = last.exposed_exterior_exchange() {
            self.final_field["exposed_exterior_exchange"] = json!(e);
            self.final_field["exposed_temperature_c"] = json!(last.exposed_temperature_c());
        }
        if heated {
            self.final_field["supplied_heat_j"] = json!(last.supplied_heat_j());
            self.final_field["internal_heat"] = json!(last.internal_heat());
        }
        self.state = last.into_state();
        if [
            self.inside_j,
            self.outside_j,
            self.covered_convective_j,
            self.covered_radiative_j,
            self.exposed_convective_j,
            self.exposed_radiative_j,
            self.net_j,
            self.stored_j,
            self.bare_j,
            self.signed_remap_j,
            self.absolute_remap_j,
            self.absolute_step_error_j,
            self.stored_j - self.net_j - self.supplied_heat_j(),
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err(invalid("motion coverage heat accumulation overflow"));
        }
        self.cell_steps += unit * count;
        self.thermal_steps += count;
        self.time_s = time_s;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &t in self
            .state
            .temperature_c()
            .iter()
            .chain(self.state.exposed_temperature_c())
            .flatten()
        {
            lo = lo.min(t);
            hi = hi.max(t);
        }
        let mut sample = json!({"sample_index":sample_index,"interval_index":interval,"time_s":time_s,
            "frame_sha256":self.frame.sha256(),"positions_sha256":hash(&cloth)?,"body_positions_sha256":hash(&body)?,
            "thermal_substeps":count,"temperature_range_c":[lo,hi],"remap":remap,
            "covered_area_m2":self.final_field["covered_area_m2"],
            "bare_energy_j":self.bare_j,"net_boundary_energy_j":self.net_j,"physical_stored_change_j":self.stored_j,
            "signed_remap_residual_j":self.signed_remap_j,"balance_error_j":self.stored_j-self.net_j-self.supplied_heat_j()});
        if heated {
            sample["inside_energy_j"] = json!(self.inside_j);
            sample["outside_energy_j"] = json!(self.outside_j);
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
        for (field, convective, radiative) in [
            (
                "exterior_exchange",
                self.covered_convective_j,
                self.covered_radiative_j,
            ),
            (
                "exposed_exterior_exchange",
                self.exposed_convective_j,
                self.exposed_radiative_j,
            ),
        ] {
            if let Some(e) = self.final_field.get(field) {
                sample[field] = json!({"convective_heat_w":e["convective_heat_w"],
                    "radiative_heat_w":e["radiative_heat_w"],"balance_residual_w":e["balance_residual_w"],
                    "convective_energy_j":convective,"radiative_energy_j":radiative});
            }
        }
        self.trace.push(sample);
        Ok(())
    }

    pub(in crate::study) fn finish(
        self,
        path_hash: &str,
        initial_hash: &str,
        duration_s: f64,
    ) -> Result<Value, Refusal> {
        if self.time_s != duration_s || self.trace.is_empty() {
            return Err(invalid(
                "coverage heat does not cover the complete mechanical path",
            ));
        }
        let supplied_heat_j = self.supplied_heat_j();
        let mut result = json!({"schema":"ftx-motion-coverage-heat-result/1","request":self.request,
            "path_sha256":path_hash,"initial_positions_sha256":initial_hash,
            "initial_frame_sha256":self.initial_frame_sha256,"duration_s":duration_s,
            "cell_steps":self.cell_steps,"thermal_steps":self.thermal_steps,"geometry_operations":self.geometry_operations,
            "inside_energy_j":self.inside_j,"outside_energy_j":self.outside_j,"net_boundary_energy_j":self.net_j,
            "physical_stored_change_j":self.stored_j,"bare_energy_j":self.bare_j,
            "signed_remap_residual_j":self.signed_remap_j,"absolute_remap_residual_j":self.absolute_remap_j,
            "balance_error_j":self.stored_j-self.net_j-supplied_heat_j,"absolute_step_balance_error_j":self.absolute_step_error_j,
            "trace":self.trace,"final_field":self.final_field,"checkpoint":self.frame.checkpoint(&self.state)?,
            "uncertainty_propagated":false,"physical_validation_claim":false,"comfort_claim":false,
            "scope":"One-way transient heat on the explicit collision-body domain with first-order source-material enthalpy remap at each accepted cloth/body sample. Remap has no elapsed time; heat substeps use the sample's right-endpoint geometry. Reference mass and exposed-sheet storage persist. Bare parallel paths store no energy. No anatomical coverage certification, human thermoregulation, mechanical heating, moisture, two-way coupling or empirical warmth claim."});
        if let Some(heating) = &self.plastic {
            result["plastic_dissipation_heat"] = heating.snapshot(true);
            result["scope"] = json!(
                "One-way transient heat on the explicit collision-body domain with qualified accepted irreversible membrane work deposited over covered and exposed source material. Reference mass, stored enthalpy and source energy follow material through remapping; remap residuals are separate from physical heat. Bare body receives no internal energy. No temperature feedback, friction/Maxwell/bending heating, moisture, thermoregulation or empirical garment claim."
            );
        }
        if let Some(heating) = &self.relaxation {
            result["relaxation_dissipation_heat"] = heating.snapshot(true);
            result["scope"] = json!(
                "One-way transient heat on the explicit collision-body domain with independently qualified accepted Maxwell dashpot dissipation and, only when separately requested, irreversible membrane yield dissipation. Source heat follows covered and exposed reference material and is summed once in the physical balance; remap residuals remain separate and bare body receives no internal energy. Initial drape work, reversible storage, backward-Euler numerical loss, bending and friction heating are excluded. No temperature feedback, moisture, thermoregulation or empirical garment claim."
            );
        }
        if let Some(heating) = &self.bending {
            result["bending_plastic_dissipation_heat"] = heating.snapshot(true);
            result["scope"] = json!(
                "One-way transient heat on the explicit body domain with source-qualified irreversible source-panel bending work, plus independently requested membrane yield and Maxwell dissipation. Source heat follows covered and exposed reference material and is summed once; remap residuals stay separate and bare body receives no internal energy. Initial drape work, recoverable elastic/hardening storage, numerical loss, seam plasticity and contact friction are excluded. No temperature feedback, moisture, thermoregulation or empirical garment claim."
            );
        }
        for (field, schedule, convective, radiative) in [
            (
                "exterior_exchange",
                &self.request.exterior_exchange,
                self.covered_convective_j,
                self.covered_radiative_j,
            ),
            (
                "exposed_exterior_exchange",
                &self.request.exposed_exterior_exchange,
                self.exposed_convective_j,
                self.exposed_radiative_j,
            ),
        ] {
            if schedule
                .as_ref()
                .is_some_and(|v| v.iter().any(Option::is_some))
            {
                result[field] =
                    json!({"convective_energy_j":convective,"radiative_energy_j":radiative});
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
