//! Prescribed, source-qualified normal pressure on independent moving thermal sheets.
//!
//! The existing pressure conditioner owns material acquisition and branch work. Pressure is an
//! authored boundary, never an inference from a contact force or guessed contact area. Each
//! instantaneous pressure traverse uses the accepted geometry's area; subsequent heat storage
//! still follows unchanged reference mass. Normal work is recorded separately, never heating.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec3};
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, normalize},
    material_compression::{self, stack},
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{hash, invalid};
use crate::{
    run::DrapeRun,
    thermal::{
        DryThermalModel,
        compression::Control,
        surface::{Patch, Surface, SurfaceState, io::retained_materials},
    },
};

/// Exact source acquisition and prescribed pressure history for one retained physical instance.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// Original selected mechanical material identity, before thermal pressure conditioning.
    pub material_sha256: String,
    /// Complete existing pressure acquisition. Its pressure is the known initial preload;
    /// its original normalized material must equal the retained source record exactly.
    pub preparation: material_compression::Input,
    /// Initial monotone acquired branch. Version 2 may explicitly replace it at a qualified join.
    pub direction: stack::Direction,
    /// Versions 1 and 2: one prescribed pressure, Pa, for each mechanical keyframe interval.
    /// Omit for version 3; even an empty or null legacy schedule is not a second control.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "pressures"
    )]
    pub pressure_pa: Option<Vec<f64>>,
    /// Version 3 only: one explicit pressure or clearance boundary per keyframe interval.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "controls"
    )]
    pub controls: Option<Vec<Control>>,
    /// Versions 2 and 3: exactly one entry per interval; null continues the active branch.
    /// A present change joins a single-layer unit-area source stack at the CURRENT state before
    /// this interval's pressure is applied. The whole field cannot be null or imply a recovery law.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "branch_changes"
    )]
    pub branch_changes: Option<Vec<Option<stack::BranchChange>>>,
}

fn pressures<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<f64>>, D::Error> {
    Vec::<f64>::deserialize(deserializer).map(Some)
}

fn controls<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<Control>>, D::Error> {
    Vec::<Control>::deserialize(deserializer).map(Some)
}

impl Binding {
    fn control(&self, interval: usize) -> Result<Control, Refusal> {
        match (&self.pressure_pa, &self.controls) {
            (Some(pressures), None) => pressures
                .get(interval)
                .map(|&pressure_pa| Control::Pressure { pressure_pa }),
            (None, Some(controls)) => controls.get(interval).copied(),
            _ => None,
        }
        .ok_or_else(|| invalid("compression needs exactly one control for this interval"))
    }
}

fn branch_changes<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<Option<stack::BranchChange>>>, D::Error> {
    Vec::<Option<stack::BranchChange>>::deserialize(deserializer).map(Some)
}

/// Optional dry pressure/heat closure on exact physical instance IDs; other sheets stay unchanged.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// `ftx-motion-surface-compression/1` for prescribed pressure on one monotone branch, /2
    /// for qualified reversals, or /3 for explicit pressure/clearance controls with optional joins.
    pub schema: String,
    /// Explicit application evidence for constant dry thermal coefficients, temperature/time
    /// range, prescribed pressure and films, and omission of normal-work conversion into heat.
    pub thermal_evidence: String,
    /// One to 256 exact retained instance bindings. A shared material may have different pressures
    /// on different instances. Unretained names and incomplete source matches refuse.
    pub bindings: BTreeMap<String, Binding>,
}

impl Request {
    pub(super) fn validate(&self, intervals: usize) -> Result<(), Refusal> {
        let cyclic = self.schema == "ftx-motion-surface-compression/2";
        let controlled = self.schema == "ftx-motion-surface-compression/3";
        if (!cyclic && !controlled && self.schema != "ftx-motion-surface-compression/1")
            || self.thermal_evidence.trim().is_empty()
            || self.thermal_evidence.len() > 4096
            || !(1..=256).contains(&self.bindings.len())
            || !(1..=16).contains(&intervals)
        {
            return Err(invalid(
                "prescribed compression needs schema /1, /2 or /3, explicit thermal evidence and 1..256 bindings",
            ));
        }
        let mut joined = false;
        for (id, binding) in &self.bindings {
            let invalid_schedule = if controlled {
                binding.pressure_pa.is_some()
                    || binding
                        .controls
                        .as_ref()
                        .is_none_or(|controls| controls.len() != intervals)
            } else {
                binding.controls.is_some()
                    || binding
                        .pressure_pa
                        .as_ref()
                        .is_none_or(|pressures| pressures.len() != intervals)
            };
            if id.is_empty()
                || id.len() > 512
                || binding.material_sha256.len() != 64
                || !binding
                    .material_sha256
                    .bytes()
                    .all(|c| c.is_ascii_hexdigit())
                || invalid_schedule
                || binding
                    .branch_changes
                    .as_ref()
                    .is_some_and(|changes| (!cyclic && !controlled) || changes.len() != intervals)
            {
                return Err(invalid(
                    "compression needs exact instance/material identities, one schema-qualified control per interval, explicit path-model estimate consent and constant thermal storage",
                ));
            }
            check_storage(&binding.preparation)?;
            let mut previous = Some(binding.preparation.pressure_pa);
            let mut direction = binding.direction;
            for interval in 0..intervals {
                if let Some(change) = binding
                    .branch_changes
                    .as_ref()
                    .and_then(|changes| changes[interval].as_ref())
                {
                    if change.entry.layers.len() != 1
                        || change.entry.layers[0].id != *id
                        || change.entry.reference_area_m2 != 1.
                        || change.entry.direction == direction
                    {
                        return Err(invalid(
                            "motion branch changes need one unchanged instance on a unit-area stack and the opposite direction",
                        ));
                    }
                    check_storage(&change.entry.layers[0].preparation)?;
                    direction = change.entry.direction;
                    joined = true;
                }
                match binding.control(interval)? {
                    Control::Pressure { pressure_pa } => {
                        if !pressure_pa.is_finite()
                            || pressure_pa < 0.
                            || previous.is_some_and(|previous| match direction {
                                stack::Direction::Loading => pressure_pa < previous,
                                stack::Direction::Unloading => pressure_pa > previous,
                            })
                        {
                            return Err(invalid(
                                "prescribed pressure must remain on its declared monotone source branch",
                            ));
                        }
                        previous = Some(pressure_pa);
                    }
                    Control::Gap { gap_m } => {
                        if !gap_m.is_finite() || gap_m <= 0. {
                            return Err(invalid(
                                "prescribed clearance must be positive and finite",
                            ));
                        }
                        // The shared inverse authority checks direction against the actual solved
                        // pressure at execution; an authored gap is never treated as a pressure.
                        previous = None;
                    }
                }
            }
        }
        if cyclic && !joined {
            return Err(invalid(
                "compression /2 needs at least one explicit acquired-branch transition",
            ));
        }
        Ok(())
    }
}

fn check_storage(preparation: &material_compression::Input) -> Result<(), Refusal> {
    if !preparation.allow_estimates || preparation.material.thermal_response.is_some() {
        return Err(invalid(
            "motion compression needs explicit path-model estimate consent and constant thermal storage",
        ));
    }
    for key in ["areal_density", "specific_heat"] {
        if !preparation.invariants.contains_key(key)
            || preparation
                .samples
                .iter()
                .any(|sample| sample.properties.contains_key(key))
        {
            return Err(invalid(format!(
                "pressure heat needs independently qualified invariant {key}; sampled storage is unsupported",
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
struct InstanceState {
    column: usize,
    material_sha256: String,
    conditioned_material_sha256: String,
    pressure_pa: f64,
    thickness_m: f64,
    reference_area_m2: f64,
    reference_mass_kg: f64,
    cumulative_normal_work_j: f64,
}

#[derive(Clone, Debug)]
struct Instance {
    binding: Binding,
    preparation: material_compression::Input,
    direction: stack::Direction,
    patches: Vec<usize>,
    state: InstanceState,
}

#[derive(Clone, Debug)]
pub(super) struct Compression {
    cyclic: bool,
    controlled: bool,
    source_model: Value,
    initial_model: Value,
    initial_instances: Value,
    instances: BTreeMap<String, Instance>,
    cells_per_layer: usize,
    last_interval: Option<usize>,
    model_sha256: String,
    updates: Vec<Value>,
    cumulative_normal_work_j: f64,
}

fn empty_catalog() -> Catalog {
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    }
}

fn quantity(material: &ResolvedMaterial, key: &str, unit: &str) -> Result<f64, Refusal> {
    material
        .material()
        .require(key, unit, material.allows_estimates())
}

fn positive(value: f64, label: &str) -> Result<f64, Refusal> {
    if !value.is_finite() || value <= 0. {
        return Err(invalid(format!(
            "prescribed compression needs positive finite {label}"
        )));
    }
    Ok(value)
}

impl Compression {
    pub(super) fn start(
        request: &Request,
        run: &DrapeRun,
        cells_per_layer: usize,
    ) -> Result<(Surface, Self), Refusal> {
        let faces = &run.scene.solver.model.membranes;
        if faces.len() != run.scene.triangle_owner.len() {
            return Err(invalid("compression topology differs from retained cloth"));
        }
        let (records, assignment) = retained_materials(
            &run.report["material"],
            &run.scene.mesh.instances,
            &run.scene.triangle_owner,
        )?;
        let patches = faces
            .iter()
            .zip(assignment)
            .map(|(face, column)| Patch {
                vertices: face.v,
                reference_area_m2: face.area,
                column,
            })
            .collect();
        Self::from_retained(
            request,
            &records,
            &run.scene.mesh.instances,
            &run.scene.triangle_owner,
            patches,
            run.scene.solver.x.len(),
            cells_per_layer,
            hash(&run.report["material"])?,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn from_retained(
        request: &Request,
        records: &[ResolvedMaterial],
        instance_names: &[String],
        owners: &[usize],
        mut patches: Vec<Patch>,
        vertex_count: usize,
        cells_per_layer: usize,
        material_report_sha256: String,
    ) -> Result<(Surface, Self), Refusal> {
        let retained = owners.iter().copied().collect::<BTreeSet<_>>();
        if owners.len() != patches.len()
            || retained.iter().any(|&owner| owner >= instance_names.len())
            || request
                .bindings
                .keys()
                .any(|id| !retained.iter().any(|&owner| instance_names[owner] == *id))
        {
            return Err(invalid(
                "compression binding names an unknown or unretained instance",
            ));
        }
        let mut source_columns = Vec::new();
        let mut column_instances = Vec::new();
        let mut columns = Vec::new();
        let mut instances = BTreeMap::new();
        for owner in retained {
            let id = &instance_names[owner];
            let owned: Vec<_> = owners
                .iter()
                .enumerate()
                .filter_map(|(index, &value)| (owner == value).then_some(index))
                .collect();
            let source_index = patches[owned[0]].column;
            let source = records
                .get(source_index)
                .ok_or_else(|| invalid("compression source material is missing"))?;
            if owned
                .iter()
                .any(|&index| patches[index].column != source_index)
            {
                return Err(invalid(
                    "one compression instance has conflicting source materials",
                ));
            }
            let column = columns.len();
            let mut material = source.clone();
            if let Some(binding) = request.bindings.get(id) {
                if binding.material_sha256 != source.sha256()
                    || hash(&normalize(&binding.preparation.material)?)? != hash(source.material())?
                {
                    return Err(invalid(
                        "compression acquisition must match the exact selected material and normalized source record",
                    ));
                }
                let conditioned = material_compression::condition(&binding.preparation)?;
                material = empty_catalog().resolve(&conditioned.selection)?;
                let area = positive(
                    owned
                        .iter()
                        .map(|&index| patches[index].reference_area_m2)
                        .sum(),
                    "reference area",
                )?;
                let mass = positive(quantity(&material, "areal_density", "kg/m2")?, "areal mass")?;
                if mass != quantity(source, "areal_density", "kg/m2")?
                    || quantity(&material, "specific_heat", "J/(kg*K)")?
                        != quantity(source, "specific_heat", "J/(kg*K)")?
                {
                    return Err(invalid(
                        "compression cannot change the selected reference mass or specific heat",
                    ));
                }
                instances.insert(
                    id.clone(),
                    Instance {
                        binding: binding.clone(),
                        preparation: binding.preparation.clone(),
                        direction: binding.direction,
                        patches: owned.clone(),
                        state: InstanceState {
                            column,
                            material_sha256: source.sha256().into(),
                            conditioned_material_sha256: material.sha256().into(),
                            pressure_pa: binding.preparation.pressure_pa,
                            thickness_m: positive(
                                quantity(&material, "thickness", "m")?,
                                "thickness",
                            )?,
                            reference_area_m2: area,
                            reference_mass_kg: positive(area * mass, "reference mass")?,
                            cumulative_normal_work_j: 0.,
                        },
                    },
                );
            }
            columns.push(DryThermalModel::new(
                vec![material],
                vec![],
                cells_per_layer,
            )?);
            source_columns.push(source.clone());
            column_instances.push(id.clone());
            for index in owned {
                patches[index].column = column;
            }
        }
        let model = Surface::new(columns, vertex_count, patches.clone())?;
        let initial_model = model.snapshot();
        let mut source_model = json!({"schema":"ftx-motion-compression-source/1",
            "material_report_sha256":material_report_sha256,
            "cells_per_layer":cells_per_layer,"vertex_count":vertex_count,
            "instances":instance_names,"triangle_instance":owners,
            "column_instances":column_instances,"columns":source_columns,"patches":patches});
        source_model["sha256"] = json!(hash(&source_model)?);
        let initial_instances = instance_snapshot(&instances);
        let compression = Self {
            cyclic: request.schema == "ftx-motion-surface-compression/2",
            controlled: request.schema == "ftx-motion-surface-compression/3",
            source_model,
            model_sha256: initial_model["sha256"]
                .as_str()
                .expect("surface identity")
                .into(),
            initial_model,
            initial_instances,
            instances,
            cells_per_layer,
            last_interval: None,
            updates: Vec::new(),
            cumulative_normal_work_j: 0.,
        };
        Ok((model, compression))
    }

    // The enclosing history runs this on a trial clone, committing pressure and heat together.
    pub(super) fn condition(
        &mut self,
        model: &Surface,
        state: &SurfaceState,
        positions: &[Vec3],
        interval: usize,
        sample_index: usize,
        time_s: f64,
    ) -> Result<Option<(Surface, SurfaceState)>, Refusal> {
        self.condition_with(
            model,
            positions,
            interval,
            sample_index,
            time_s,
            |model, replacements| model.recondition_columns(state, replacements),
        )
    }

    pub(super) fn condition_source(
        &mut self,
        model: &Surface,
        positions: &[Vec3],
        interval: usize,
        sample_index: usize,
        time_s: f64,
    ) -> Result<Option<Surface>, Refusal> {
        self.condition_with(
            model,
            positions,
            interval,
            sample_index,
            time_s,
            |model, replacements| Ok((model.reconditioned_columns(replacements)?, ())),
        )
        .map(|next| next.map(|(model, ())| model))
    }

    fn condition_with<T>(
        &mut self,
        model: &Surface,
        positions: &[Vec3],
        interval: usize,
        sample_index: usize,
        time_s: f64,
        recondition: impl FnOnce(
            &Surface,
            Vec<(usize, DryThermalModel)>,
        ) -> Result<(Surface, T), Refusal>,
    ) -> Result<Option<(Surface, T)>, Refusal> {
        if self.last_interval == Some(interval) {
            return Ok(None);
        }
        if interval != self.last_interval.map_or(0, |previous| previous + 1)
            || positions.len() != self.source_model["vertex_count"].as_u64().unwrap_or(0) as usize
            || positions.iter().any(|p| !p.is_finite())
        {
            return Err(invalid(
                "compression needs ordered intervals and complete finite geometry",
            ));
        }
        let mut replacements = Vec::new();
        let mut updates = BTreeMap::new();
        for (id, instance) in &mut self.instances {
            let control = instance.binding.control(interval)?;
            let previous_pressure = instance.state.pressure_pa;
            let mut preparation = instance.preparation.clone();
            preparation.pressure_pa = previous_pressure;
            let mut input = stack::Input {
                schema: stack::SCHEMA.into(),
                gap_m: instance.state.thickness_m,
                reference_area_m2: 1.,
                direction: instance.direction,
                layers: vec![stack::Layer {
                    id: id.clone(),
                    preparation,
                }],
                allow_estimates: instance.preparation.allow_estimates,
            };
            let branch_change = if let Some(change) = instance
                .binding
                .branch_changes
                .as_ref()
                .and_then(|changes| changes.get(interval))
                .and_then(Option::as_ref)
            {
                // Recompute the actual current branch at zero travel before asking the shared
                // source authority to admit a join. No authored thickness replaces retained state.
                let current = stack::at_pressure(&input, previous_pressure)?;
                let entry = stack::qualify_branch_change(&input, &current, change, true)?;
                check_storage(&change.entry.layers[0].preparation)?;
                input = change.entry.clone();
                instance.preparation = input.layers[0].preparation.clone();
                instance.direction = input.direction;
                Some(json!({"change":change,"entry_compression":entry}))
            } else {
                None
            };
            let compressed = match control {
                Control::Pressure { pressure_pa } => stack::at_pressure(&input, pressure_pa)?,
                Control::Gap { gap_m } => {
                    input.gap_m = gap_m;
                    stack::close(&input)?
                }
            };
            let pressure = compressed.pressure_pa;
            let layer = &compressed.layers[0];
            let material = empty_catalog().resolve(&layer.conditioned.selection)?;
            let mut area = 0.;
            for &index in &instance.patches {
                let [a, b, c] = model.patches()[index].vertices.map(|i| positions[i]);
                area += positive(0.5 * (b - a).cross(c - a).norm(), "current triangle area")?;
            }
            positive(area, "current instance area")?;
            let work = layer.external_work_j_m2 * area;
            let cumulative = instance.state.cumulative_normal_work_j + work;
            let total = self.cumulative_normal_work_j + work;
            if !work.is_finite()
                || !cumulative.is_finite()
                || !total.is_finite()
                || (layer.external_work_j_m2 != 0. && work == 0.)
            {
                return Err(invalid("compression normal work is not representable"));
            }
            instance.state.pressure_pa = pressure;
            instance.state.thickness_m = layer.thickness_m;
            instance.state.conditioned_material_sha256 = material.sha256().into();
            instance.state.cumulative_normal_work_j = cumulative;
            self.cumulative_normal_work_j = total;
            let mut update = serde_json::to_value(&instance.state)
                .map_err(|error| invalid(error.to_string()))?;
            update["previous_pressure_pa"] = json!(previous_pressure);
            update["work_area_m2"] = json!(area);
            update["normal_work_j"] = json!(work);
            update["compression_input_sha256"] = json!(compressed.input_sha256);
            if self.controlled {
                update["control"] = json!(control);
                update["closure_residual_m"] = json!(compressed.closure_residual_m);
            }
            if let Some(change) = branch_change {
                update["branch_change"] = change;
            }
            updates.insert(id.clone(), update);
            replacements.push((
                instance.state.column,
                DryThermalModel::new(vec![material], vec![], self.cells_per_layer)?,
            ));
        }
        let (next, temperature) = recondition(model, replacements)?;
        self.model_sha256 = next.snapshot()["sha256"]
            .as_str()
            .expect("surface identity")
            .into();
        self.updates.push(
            json!({"interval_index":interval,"sample_index":sample_index,
            "time_s":time_s,"positions_sha256":hash(&positions)?,
            "model_sha256":self.model_sha256,"instances":updates}),
        );
        self.last_interval = Some(interval);
        Ok(Some((next, temperature)))
    }

    pub(super) fn summary(&self) -> Value {
        json!({"model_sha256":self.model_sha256,"instances":instance_snapshot(&self.instances),
            "cumulative_normal_work_j":self.cumulative_normal_work_j})
    }

    pub(super) fn finish(&self) -> Value {
        json!({"schema":if self.controlled { "ftx-motion-surface-compression-result/3" }
        else if self.cyclic { "ftx-motion-surface-compression-result/2" }
        else { "ftx-motion-surface-compression-result/1" },
        "material_report_sha256":self.source_model["material_report_sha256"],
        "source_model":self.source_model,"initial_model":self.initial_model,
        "initial_instances":self.initial_instances,"updates":self.updates,
        "final_instances":instance_snapshot(&self.instances),
        "final_model_sha256":self.model_sha256,
        "cumulative_normal_work_j":self.cumulative_normal_work_j,"normal_work_to_heat_j":0.,
        "scope":if self.controlled {
            "Explicit uniform pressure or rigid-face clearance per retained instance on source-qualified dry branches. Clearance uses the shared inverse stack authority; every optional reversal preserves actual pressure, thickness, reference mass and temperature cells with zero entry work. Each instantaneous traverse uses current accepted area held fixed during that traverse; subsequent heat storage follows unchanged reference mass. No contact-derived pressure or gap, geometry feedback, inferred recovery-time or minor-loop law, wet law, work-to-heat conversion or empirical garment claim."
        } else if self.cyclic {
            "Prescribed uniform pressure per retained instance on explicitly joined source-qualified dry branches. Every reversal preserves actual pressure, thickness, reference mass and temperature cells with zero entry work. Each instantaneous traverse uses current accepted area held fixed during that traverse; subsequent heat storage follows unchanged reference mass. No inferred recovery-time or minor-loop law, contact pressure, geometry feedback, wet law, work-to-heat conversion or empirical garment claim."
        } else {
            "Prescribed uniform pressure per retained instance on its qualified monotone dry branch. Each instantaneous traverse uses current accepted area held fixed during that traverse; subsequent heat storage follows unchanged reference mass. No pressure inferred from contact, geometry feedback, branch reversal, wet law, work-to-heat conversion or empirical garment claim."
        }})
    }
}

fn instance_snapshot(instances: &BTreeMap<String, Instance>) -> Value {
    json!(
        instances
            .iter()
            .map(|(id, instance)| (id, &instance.state))
            .collect::<BTreeMap<_, _>>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::study::heat::{SurfaceHistory, SurfaceRequest};
    use ftx_pattern::material_catalog::Selection;

    fn acquisition() -> material_compression::Input {
        let mut input: material_compression::Input = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-compression.json"
        ))
        .unwrap();
        input.pressure_pa = 0.;
        input
    }

    fn selected(input: &material_compression::Input) -> ResolvedMaterial {
        empty_catalog()
            .resolve(&Selection {
                id: input.material.id.clone(),
                record: Some(input.material.clone()),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap()
    }

    fn fixture(pressures: Vec<f64>) -> SurfaceHistory {
        let preparation = acquisition();
        let record = selected(&preparation);
        let compression = Request {
            schema: "ftx-motion-surface-compression/1".into(),
            thermal_evidence: "Synthetic arithmetic fixture; constant dry coefficients over 0..40 C, prescribed pressures and films, no work conversion into heat. Not physical qualification.".into(),
            bindings: BTreeMap::from([("sheet".into(), Binding {
                material_sha256: record.sha256().into(),
                preparation,
                direction: stack::Direction::Loading,
                pressure_pa: Some(pressures),
                controls: None,
                branch_changes: None,
            })]),
        };
        history_from(compression)
    }

    fn history_from(compression: Request) -> SurfaceHistory {
        let binding = &compression.bindings["sheet"];
        let intervals = binding
            .pressure_pa
            .as_ref()
            .map_or_else(|| binding.controls.as_ref().unwrap().len(), Vec::len);
        let record = selected(&binding.preparation);
        let policy: SurfaceRequest = serde_json::from_value(json!({
            "schema":"ftx-motion-surface-heat/1","initial_temperature_c":0.,
            "cells_per_layer":1,"max_step_s":1.,"max_cell_steps":100,
            "boundaries":vec![json!({"inside_c":40.,"outside_c":0.,
                "inside_resistance_m2_k_w":0.,"outside_resistance_m2_k_w":0.});intervals],
            "prescribed_compression":compression,
        }))
        .unwrap();
        policy.validate(intervals).unwrap();
        let (model, compression) = Compression::from_retained(
            policy.prescribed_compression.as_ref().unwrap(),
            &[record],
            &["sheet".into()],
            &[0],
            vec![Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.25,
                column: 0,
            }],
            3,
            1,
            "a".repeat(64),
        )
        .unwrap();
        let mut history =
            SurfaceHistory::with_instances(policy, model, &["sheet".into()], &[0]).unwrap();
        history.compression = Some(compression);
        history
    }

    fn reversal(preparation: &material_compression::Input, unloading: bool) -> stack::BranchChange {
        let mut branch = preparation.clone();
        let (from, to, pressure, thickness, direction) = if unloading {
            (
                "loading",
                "unloading-after-500",
                500.,
                0.010,
                stack::Direction::Unloading,
            )
        } else {
            (
                "unloading-after-500",
                "loading",
                0.,
                0.020,
                stack::Direction::Loading,
            )
        };
        branch.pressure_pa = pressure;
        branch.loading_branch = to.into();
        if unloading {
            // Independently authored piecewise-linear unload path: (0 Pa,20 mm),
            // (250 Pa,12 mm), (500 Pa,10 mm). It rejoins the loading endpoints exactly.
            let mut middle = branch.samples[0].clone();
            middle.pressure_pa = 250.;
            middle.properties.get_mut("thickness").unwrap().value = 12.;
            middle.properties.get_mut("thickness").unwrap().bounds = None;
            middle
                .properties
                .get_mut("thermal_resistance")
                .unwrap()
                .value = 0.3;
            for quantity in middle.properties.values_mut() {
                quantity
                    .conditions
                    .insert("pressure_pa".into(), "250".into());
            }
            branch.samples.insert(1, middle);
        }
        for sample in &mut branch.samples {
            for quantity in sample.properties.values_mut() {
                quantity
                    .conditions
                    .insert("loading_branch".into(), to.into());
            }
        }
        for invariant in branch.invariants.values_mut() {
            invariant
                .conditions
                .insert("loading_branch".into(), to.into());
        }
        stack::BranchChange {
            entry: stack::Input {
                schema: stack::SCHEMA.into(),
                gap_m: thickness,
                reference_area_m2: 1.,
                direction,
                layers: vec![stack::Layer { id: "sheet".into(), preparation: branch }],
                allow_estimates: true,
            },
            transitions: BTreeMap::from([("sheet".into(), stack::TransitionEvidence {
                source: "synthetic".into(),
                evidence: ftx_pattern::materials::Evidence::Estimate,
                method: "Invented acquired complete cycle after a 500 Pa peak and a 0 Pa return; no measured textile or recovery claim".into(),
                conditions: BTreeMap::from([
                    ("specimen".into(), preparation.specimen.clone()),
                    ("conditioning".into(), preparation.conditioning.clone()),
                    ("from_loading_branch".into(), from.into()),
                    ("to_loading_branch".into(), to.into()),
                    ("pressure_pa".into(), pressure.to_string()),
                ]),
            })]),
        }
    }

    fn cyclic_request() -> Request {
        let mut request = fixture(vec![500.; 4])
            .request
            .prescribed_compression
            .unwrap();
        request.schema = "ftx-motion-surface-compression/2".into();
        let binding = request.bindings.get_mut("sheet").unwrap();
        binding.pressure_pa = Some(vec![500., 250., 0., 250.]);
        // This synthetic experiment's original observations explicitly apply on both branches.
        // A real branch-restricted observation cannot be transferred by renaming its source.
        for quantity in binding.preparation.material.properties.values_mut() {
            quantity.conditions.remove("loading_branch");
        }
        binding.material_sha256 = selected(&binding.preparation).sha256().into();
        binding.branch_changes = Some(vec![
            None,
            Some(reversal(&binding.preparation, true)),
            None,
            Some(reversal(&binding.preparation, false)),
        ]);
        request
    }

    fn physical_state(history: &SurfaceHistory) -> Value {
        json!({"model":history.model.snapshot(), "temperature":history.state.temperature_c(),
            "compression":format!("{:?}", history.compression),
            "time_s":history.time_s,"cell_steps":history.cell_steps,"thermal_steps":history.thermal_steps,
            "inside_j":history.inside_j,"outside_j":history.outside_j,"stored_j":history.stored_j,
            "net_j":history.net_j,"trace":history.trace})
    }

    fn geometry(area: f64) -> Vec<Vec3> {
        vec![
            Vec3::ZERO,
            Vec3::new(2. * area, 0., 0.),
            Vec3::new(0., 1., 0.),
        ]
    }

    fn close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-10 * expected.abs().max(1.),
            "{actual} differs from {expected}"
        );
    }

    // Independent one-cell backward-Euler balance: reference storage is 25 J/K; prescribed
    // face conductance is 2*A_current/R on each side, not 2*A_reference/R.
    fn expected_step(old: f64, area: f64, resistance: f64, seconds: f64) -> f64 {
        let capacity_rate = 25. / seconds;
        let conductance = 2. * area / resistance;
        (capacity_rate * old + conductance * 40.) / (capacity_rate + 2. * conductance)
    }

    fn controlled_request(controls: Vec<Control>) -> Request {
        let mut request = fixture(vec![0.; controls.len()])
            .request
            .prescribed_compression
            .unwrap();
        request.schema = "ftx-motion-surface-compression/3".into();
        let binding = request.bindings.get_mut("sheet").unwrap();
        binding.pressure_pa = None;
        binding.controls = Some(controls);
        request
    }

    #[test]
    fn clearance_and_pressure_controls_share_actual_heat_mass_and_branch_work() {
        let controls = vec![
            Control::Gap { gap_m: 0.015 },
            Control::Pressure { pressure_pa: 500. },
        ];
        let mut history = history_from(controlled_request(controls.clone()));
        let mut pressure = fixture(vec![250., 500.]);
        let mut expected = 0.;
        for (interval, (area, resistance)) in [(1., 0.375), (0.5, 0.25)].into_iter().enumerate() {
            let time = (interval + 1) as f64;
            history
                .advance(&geometry(area), time, interval, interval, None, None, None)
                .unwrap();
            pressure
                .advance(&geometry(area), time, interval, interval, None, None, None)
                .unwrap();
            expected = expected_step(expected, area, resistance, 1.);
            close(history.state.temperature_c()[0][0], expected);
            close(
                history.state.temperature_c()[0][0],
                pressure.state.temperature_c()[0][0],
            );
            close(history.stored_j, 25. * expected);
            close(history.balance_error_j(), 0.);
        }
        let result = history.finish("path", "initial", 2.).unwrap();
        let compression = &result["prescribed_compression"];
        assert_eq!(
            compression["schema"],
            "ftx-motion-surface-compression-result/3"
        );
        for (interval, expected_pressure) in [250., 500.].into_iter().enumerate() {
            let update = &compression["updates"][interval]["instances"]["sheet"];
            assert_eq!(update["control"], json!(controls[interval]));
            close(update["pressure_pa"].as_f64().unwrap(), expected_pressure);
            close(update["closure_residual_m"].as_f64().unwrap(), 0.);
        }
        close(
            compression["cumulative_normal_work_j"].as_f64().unwrap(),
            1.5625,
        );
        close(
            compression["final_instances"]["sheet"]["reference_mass_kg"]
                .as_f64()
                .unwrap(),
            0.025,
        );
        assert_eq!(compression["normal_work_to_heat_j"], 0.);
        assert!(
            result["request"]["prescribed_compression"]["bindings"]["sheet"]
                .get("pressure_pa")
                .is_none()
        );
    }

    #[test]
    fn mixed_controls_keep_qualified_reversals_and_roll_back_late_heat_failure() {
        let mut request = cyclic_request();
        request.schema = "ftx-motion-surface-compression/3".into();
        let binding = request.bindings.get_mut("sheet").unwrap();
        binding.pressure_pa = None;
        binding.controls = Some(vec![
            Control::Pressure { pressure_pa: 500. },
            Control::Gap { gap_m: 0.012 },
            Control::Pressure { pressure_pa: 0. },
            Control::Gap { gap_m: 0.015 },
        ]);
        let mut history = history_from(request);
        let mut pressure = history_from(cyclic_request());
        for interval in 0..4 {
            let time = (interval + 1) as f64;
            let area = [1., 0.5, 2., 0.25][interval];
            if interval == 1 {
                history.request.max_cell_steps = history.cell_steps;
                let accepted = physical_state(&history);
                assert!(
                    history
                        .advance(&geometry(area), time, interval, interval, None, None, None)
                        .is_err()
                );
                assert_eq!(physical_state(&history), accepted);
                history.request.max_cell_steps = 100;
            }
            history
                .advance(&geometry(area), time, interval, interval, None, None, None)
                .unwrap();
            pressure
                .advance(&geometry(area), time, interval, interval, None, None, None)
                .unwrap();
            close(
                history.state.temperature_c()[0][0],
                pressure.state.temperature_c()[0][0],
            );
            close(
                history
                    .compression
                    .as_ref()
                    .unwrap()
                    .cumulative_normal_work_j,
                pressure
                    .compression
                    .as_ref()
                    .unwrap()
                    .cumulative_normal_work_j,
            );
        }
        let result = history.finish("path", "initial", 4.).unwrap();
        for interval in [1, 3] {
            let update =
                &result["prescribed_compression"]["updates"][interval]["instances"]["sheet"];
            assert_eq!(
                update["branch_change"]["entry_compression"]["external_work_j"],
                0.
            );
            close(update["closure_residual_m"].as_f64().unwrap(), 0.);
        }
    }

    #[test]
    fn clearance_domain_and_direction_failures_preserve_the_last_accepted_state() {
        for control in [
            Control::Gap { gap_m: 0.025 },
            Control::Gap { gap_m: 0.018 },
            Control::Pressure { pressure_pa: 200. },
        ] {
            let mut history = history_from(controlled_request(vec![
                Control::Gap { gap_m: 0.015 },
                control,
            ]));
            history
                .advance(&geometry(1.), 1., 0, 0, None, None, None)
                .unwrap();
            let accepted = physical_state(&history);
            assert!(
                history
                    .advance(&geometry(0.5), 2., 1, 1, None, None, None)
                    .is_err()
            );
            assert_eq!(physical_state(&history), accepted);
        }
    }

    #[test]
    fn control_schedules_are_schema_specific_complete_and_never_null() {
        let source = controlled_request(vec![Control::Gap { gap_m: 0.015 }]);
        source.validate(1).unwrap();
        let raw = serde_json::to_value(&source).unwrap();
        assert_eq!(
            serde_json::to_value(serde_json::from_value::<Request>(raw.clone()).unwrap()).unwrap(),
            raw
        );
        for case in 0..13 {
            let mut invalid = raw.clone();
            match case {
                0 => invalid["bindings"]["sheet"]["pressure_pa"] = json!([]),
                1 => invalid["bindings"]["sheet"]["pressure_pa"] = Value::Null,
                2 => invalid["bindings"]["sheet"]["controls"] = Value::Null,
                3 => invalid["bindings"]["sheet"]["controls"] = json!([]),
                4 => invalid["bindings"]["sheet"]["controls"][0]["pressure_pa"] = json!(250.),
                5 => invalid["bindings"]["sheet"]["controls"][0]["gap_m"] = json!(0.),
                6 => invalid["bindings"]["sheet"]["controls"][0]["kind"] = json!("inferred"),
                7 => invalid["schema"] = json!("ftx-motion-surface-compression/1"),
                8 => invalid["schema"] = json!("ftx-motion-surface-compression/2"),
                9 => invalid["bindings"]["sheet"]["branch_changes"] = Value::Null,
                10 => invalid["bindings"]["sheet"]["branch_changes"] = json!([]),
                11 => {
                    invalid = serde_json::to_value(
                        fixture(vec![250.]).request.prescribed_compression.unwrap(),
                    )
                    .unwrap();
                    invalid["bindings"]["sheet"]["controls"] = Value::Null;
                }
                _ => {
                    invalid = serde_json::to_value(
                        fixture(vec![250.]).request.prescribed_compression.unwrap(),
                    )
                    .unwrap();
                    invalid["bindings"]["sheet"]["controls"] = json!([]);
                }
            }
            if let Ok(request) = serde_json::from_value::<Request>(invalid) {
                assert!(request.validate(1).is_err(), "case {case}");
            }
        }
    }

    #[test]
    fn acquired_loading_unloading_and_reloading_preserve_moving_heat_and_signed_work() {
        let mut history = history_from(cyclic_request());
        let mut expected_temperature = 0.;
        let mut expected_work = 0.;
        for (sample, (time, interval, area, seconds, resistance, work)) in [
            (1., 0, 1., 1., 0.25, 2.5),
            (1.5, 1, 0.5, 0.5, 0.3, -0.375),
            // A second accepted sample on the same branch must not replay the join or squeeze.
            (2., 1, 0.5, 0.5, 0.3, 0.),
            (3., 2, 2., 1., 0.5, -2.),
            (4., 3, 0.25, 1., 0.375, 0.15625),
        ]
        .into_iter()
        .enumerate()
        {
            history
                .advance(&geometry(area), time, interval, sample, None, None, None)
                .unwrap();
            expected_temperature = expected_step(expected_temperature, area, resistance, seconds);
            expected_work += work;
            close(history.state.temperature_c()[0][0], expected_temperature);
            close(history.stored_j, 25. * expected_temperature);
            close(history.balance_error_j(), 0.);
            close(
                history
                    .compression
                    .as_ref()
                    .unwrap()
                    .cumulative_normal_work_j,
                expected_work,
            );
        }
        close(expected_work, 0.28125);
        let result = history.finish("path", "initial", 4.).unwrap();
        let compression = &result["prescribed_compression"];
        assert_eq!(
            compression["schema"],
            "ftx-motion-surface-compression-result/2"
        );
        let updates = compression["updates"].as_array().unwrap();
        assert_eq!(updates.len(), 4);
        for index in [1, 3] {
            let update = &updates[index]["instances"]["sheet"];
            let entry = &update["branch_change"]["entry_compression"];
            assert_eq!(entry["external_work_j"], 0.);
            assert_eq!(
                entry["pressure_pa"],
                updates[index - 1]["instances"]["sheet"]["pressure_pa"]
            );
            assert_eq!(
                entry["layers"][0]["thickness_m"],
                updates[index - 1]["instances"]["sheet"]["thickness_m"]
            );
            assert_eq!(
                update["branch_change"]["change"],
                result["request"]["prescribed_compression"]["bindings"]["sheet"]["branch_changes"]
                    [index]
            );
        }
        assert!(
            updates[0]["instances"]["sheet"]
                .get("branch_change")
                .is_none()
        );
        assert!(
            updates[2]["instances"]["sheet"]
                .get("branch_change")
                .is_none()
        );
        assert_eq!(compression["normal_work_to_heat_j"], 0.);
        close(
            compression["final_instances"]["sheet"]["reference_mass_kg"]
                .as_f64()
                .unwrap(),
            0.025,
        );
        assert_eq!(compression["final_instances"]["sheet"]["pressure_pa"], 250.);
    }

    #[test]
    fn source_cycle_work_does_not_become_heat_at_uniform_temperature() {
        let mut history = history_from(cyclic_request());
        history.request.initial_temperature_c = 17.;
        history.state = history.model.initial(17.).unwrap();
        for film in &mut history.request.boundaries {
            film.inside_c = 17.;
            film.outside_c = 17.;
        }
        for interval in 0..4 {
            history
                .advance(
                    &geometry(0.25),
                    (interval + 1) as f64,
                    interval,
                    interval,
                    None,
                    None,
                    None,
                )
                .unwrap();
            close(history.state.temperature_c()[0][0], 17.);
            close(history.stored_j, 0.);
        }
        close(
            history
                .compression
                .as_ref()
                .unwrap()
                .cumulative_normal_work_j,
            0.34375,
        );
        close(history.inside_j, 0.);
        close(history.outside_j, 0.);
    }

    #[test]
    fn missing_source_or_discontinuous_cyclic_entry_cannot_mutate_accepted_state() {
        let source = cyclic_request();
        for case in 0..6 {
            let mut request = source.clone();
            let join = request
                .bindings
                .get_mut("sheet")
                .unwrap()
                .branch_changes
                .as_mut()
                .unwrap()[1]
                .as_mut()
                .unwrap();
            match case {
                0 => join.transitions.clear(),
                1 => join.transitions.get_mut("sheet").unwrap().source = "absent-source".into(),
                2 => {
                    join.transitions
                        .get_mut("sheet")
                        .unwrap()
                        .conditions
                        .insert("from_loading_branch".into(), "other-loading-history".into());
                }
                3 => join.entry.gap_m = 0.011,
                4 => {
                    join.entry.layers[0].preparation.samples[1]
                        .properties
                        .get_mut("thickness")
                        .unwrap()
                        .value = 13.
                }
                _ => {
                    join.entry.layers[0]
                        .preparation
                        .material
                        .sources
                        .get_mut("synthetic")
                        .unwrap()
                        .note
                        .push_str("Changed source revision");
                }
            }
            // Case 4 changes an interior acquired point legitimately; make the actual join
            // discontinuous too, while keeping the source curve monotone and within its bounds.
            if case == 4 {
                join.entry.layers[0].preparation.samples[2]
                    .properties
                    .get_mut("thickness")
                    .unwrap()
                    .value = 0.9;
            }
            let mut history = history_from(request);
            history
                .advance(&geometry(1.), 1., 0, 0, None, None, None)
                .unwrap();
            let accepted = physical_state(&history);
            assert!(
                history
                    .advance(&geometry(1.), 2., 1, 1, None, None, None)
                    .is_err(),
                "case {case}"
            );
            assert_eq!(physical_state(&history), accepted, "case {case}");
        }
    }

    #[test]
    fn late_thermal_failure_restores_active_branch_and_allows_identical_retry() {
        let mut history = history_from(cyclic_request());
        history
            .advance(&geometry(1.), 1., 0, 0, None, None, None)
            .unwrap();
        let mut control = history.clone();
        history.request.max_cell_steps = 1;
        let accepted = physical_state(&history);
        // Branch admission and pressure conditioning happen before this exhausted thermal solve.
        assert!(
            history
                .advance(&geometry(0.5), 2., 1, 1, None, None, None)
                .is_err()
        );
        assert_eq!(physical_state(&history), accepted);
        history.request.max_cell_steps = 100;
        history
            .advance(&geometry(0.5), 2., 1, 1, None, None, None)
            .unwrap();
        control
            .advance(&geometry(0.5), 2., 1, 1, None, None, None)
            .unwrap();
        assert_eq!(physical_state(&history), physical_state(&control));
        assert_eq!(history.compression.as_ref().unwrap().updates.len(), 2);
    }

    #[test]
    fn cyclic_schema_rejects_implicit_reversals_null_fields_and_unqualified_branch_storage() {
        let source = cyclic_request();
        source.validate(4).unwrap();
        let raw = serde_json::to_value(&source).unwrap();
        let mut malformed = raw.clone();
        malformed["bindings"]["sheet"]["branch_changes"] = Value::Null;
        assert!(serde_json::from_value::<Request>(malformed).is_err());
        for case in 0..7 {
            let mut request = source.clone();
            let binding = request.bindings.get_mut("sheet").unwrap();
            match case {
                0 => request.schema = "ftx-motion-surface-compression/1".into(),
                1 => binding.branch_changes = None,
                2 => binding.branch_changes = Some(vec![None; 4]),
                3 => {
                    binding.branch_changes.as_mut().unwrap().pop();
                }
                4 => {
                    binding.branch_changes.as_mut().unwrap()[1]
                        .as_mut()
                        .unwrap()
                        .entry
                        .layers[0]
                        .preparation
                        .invariants
                        .remove("specific_heat");
                }
                5 => {
                    binding.branch_changes.as_mut().unwrap()[1]
                        .as_mut()
                        .unwrap()
                        .entry
                        .reference_area_m2 = 0.25;
                }
                _ => {
                    binding.branch_changes.as_mut().unwrap()[1]
                        .as_mut()
                        .unwrap()
                        .entry
                        .layers[0]
                        .id = "other-sheet".into();
                }
            }
            assert!(request.validate(4).is_err(), "case {case}");
        }
        let legacy = fixture(vec![500.; 4])
            .request
            .prescribed_compression
            .unwrap();
        let before = serde_json::to_value(&legacy).unwrap();
        assert!(before["bindings"]["sheet"].get("branch_changes").is_none());
        assert_eq!(
            serde_json::to_value(serde_json::from_value::<Request>(before.clone()).unwrap())
                .unwrap(),
            before
        );
        let mut reserved = legacy;
        reserved.bindings.get_mut("sheet").unwrap().branch_changes = Some(vec![None; 4]);
        assert!(reserved.validate(4).is_err());
    }

    #[test]
    fn prescribed_compression_changes_moving_heat_without_creating_mass_or_heat() {
        let mut history = fixture(vec![250., 500.]);
        let initial = history.model.snapshot();
        history
            .advance(&geometry(1.), 0.5, 0, 1, None, None, None)
            .unwrap();
        let expected1 = expected_step(0., 1., 0.375, 0.5);
        close(history.state.temperature_c()[0][0], expected1);
        // A second accepted mechanical sample in the SAME interval must not replay the squeeze.
        history
            .advance(&geometry(1.), 1., 0, 2, None, None, None)
            .unwrap();
        let expected2 = expected_step(expected1, 1., 0.375, 0.5);
        close(history.state.temperature_c()[0][0], expected2);
        history
            .advance(&geometry(0.5), 2., 1, 3, None, None, None)
            .unwrap();
        let expected3 = expected_step(expected2, 0.5, 0.25, 1.);
        close(history.state.temperature_c()[0][0], expected3);
        close(history.stored_j, 25. * expected3);
        close(history.balance_error_j(), 0.);
        let result = history.finish("path", "initial", 2.).unwrap();
        let pressure = &result["prescribed_compression"];
        assert_eq!(pressure["initial_model"], initial);
        Surface::from_snapshot(&pressure["initial_model"]).unwrap();
        Surface::from_snapshot(&result["model"]).unwrap();
        assert_eq!(pressure["updates"].as_array().unwrap().len(), 2);
        close(
            pressure["updates"][0]["instances"]["sheet"]["normal_work_j"]
                .as_f64()
                .unwrap(),
            0.625,
        );
        close(
            pressure["updates"][1]["instances"]["sheet"]["normal_work_j"]
                .as_f64()
                .unwrap(),
            0.9375,
        );
        close(
            pressure["cumulative_normal_work_j"].as_f64().unwrap(),
            1.5625,
        );
        close(
            pressure["final_instances"]["sheet"]["reference_mass_kg"]
                .as_f64()
                .unwrap(),
            0.025,
        );
        assert_eq!(pressure["normal_work_to_heat_j"], 0.);
        assert_eq!(pressure["final_model_sha256"], result["model"]["sha256"]);
        assert_eq!(
            result["trace"][0]["prescribed_compression"]["model_sha256"],
            result["trace"][1]["prescribed_compression"]["model_sha256"]
        );
        assert_ne!(
            result["trace"][1]["prescribed_compression"]["model_sha256"],
            result["trace"][2]["prescribed_compression"]["model_sha256"]
        );
        assert!(
            expected3
                > expected_step(
                    expected_step(expected_step(0., 1., 0.5, 0.5), 1., 0.5, 0.5),
                    0.5,
                    0.5,
                    1.
                )
        );
    }

    #[test]
    fn different_instances_of_one_material_retain_independent_pressures_and_cells() {
        let mut history = fixture(vec![250.]);
        let mut request = history.request.prescribed_compression.clone().unwrap();
        let mut second = request.bindings["sheet"].clone();
        second.pressure_pa.as_mut().unwrap()[0] = 500.;
        request.bindings.insert("other".into(), second);
        let record = selected(&request.bindings["sheet"].preparation);
        let names = vec!["sheet".into(), "other".into()];
        let (model, compression) = Compression::from_retained(
            &request,
            &[record],
            &names,
            &[0, 1],
            vec![
                Patch {
                    vertices: [0, 1, 2],
                    reference_area_m2: 0.25,
                    column: 0,
                },
                Patch {
                    vertices: [3, 4, 5],
                    reference_area_m2: 0.25,
                    column: 0,
                },
            ],
            6,
            1,
            "a".repeat(64),
        )
        .unwrap();
        history.request.prescribed_compression = Some(request);
        history = SurfaceHistory::with_instances(history.request, model, &names, &[0, 1]).unwrap();
        history.compression = Some(compression);
        let positions = vec![
            Vec3::ZERO,
            Vec3::new(1., 0., 0.),
            Vec3::new(0., 1., 0.),
            Vec3::new(2., 0., 0.),
            Vec3::new(3., 0., 0.),
            Vec3::new(2., 1., 0.),
        ];
        history
            .advance(&positions, 1., 0, 1, None, None, None)
            .unwrap();
        close(
            history.state.temperature_c()[0][0],
            expected_step(0., 0.5, 0.375, 1.),
        );
        close(
            history.state.temperature_c()[1][0],
            expected_step(0., 0.5, 0.25, 1.),
        );
        assert_eq!(history.model.column_count(), 2);
        let receipt = history.compression.as_ref().unwrap().finish();
        assert_eq!(receipt["final_instances"]["sheet"]["pressure_pa"], 250.);
        assert_eq!(receipt["final_instances"]["other"]["pressure_pa"], 500.);
        assert_eq!(
            receipt["source_model"]["columns"][0]["sha256"],
            receipt["source_model"]["columns"][1]["sha256"]
        );
        assert_ne!(
            receipt["final_instances"]["sheet"]["conditioned_material_sha256"],
            receipt["final_instances"]["other"]["conditioned_material_sha256"]
        );
    }

    #[test]
    fn failed_heat_budget_rolls_back_conditioning_work_and_temperature_together() {
        let mut history = fixture(vec![250.]);
        history.request.max_cell_steps = 1;
        let old_model = history.model.snapshot();
        let old_compression = history.compression.as_ref().unwrap().finish();
        assert!(
            history
                .advance(&geometry(1.), 2., 0, 1, None, None, None)
                .is_err()
        );
        assert_eq!(history.model.snapshot(), old_model);
        assert_eq!(
            history.compression.as_ref().unwrap().finish(),
            old_compression
        );
        assert_eq!(history.state.temperature_c(), &[vec![0.]]);
        assert_eq!(history.cell_steps, 0);
        assert_eq!(history.time_s, 0.);
        assert!(history.trace.is_empty());
        history
            .advance(&geometry(1.), 0.5, 0, 1, None, None, None)
            .unwrap();
        close(
            history
                .compression
                .as_ref()
                .unwrap()
                .cumulative_normal_work_j,
            0.625,
        );
        assert_eq!(history.compression.as_ref().unwrap().updates.len(), 1);
    }

    #[test]
    fn pressure_domain_refusal_is_atomic_and_unqualified_storage_or_reversal_refuses() {
        let mut history = fixture(vec![250., 600.]);
        history
            .advance(&geometry(1.), 1., 0, 1, None, None, None)
            .unwrap();
        let model = history.model.snapshot();
        let compression = history.compression.as_ref().unwrap().finish();
        let temperatures = history.state.temperature_c().to_vec();
        assert!(
            history
                .advance(&geometry(1.), 2., 1, 2, None, None, None)
                .is_err()
        );
        assert_eq!(history.model.snapshot(), model);
        assert_eq!(history.compression.as_ref().unwrap().finish(), compression);
        assert_eq!(history.state.temperature_c(), temperatures);
        assert_eq!(history.time_s, 1.);
        let mut request = history.request.prescribed_compression.unwrap();
        request.bindings.get_mut("sheet").unwrap().pressure_pa = Some(vec![250., 200.]);
        assert!(request.validate(2).is_err());
        request.bindings.get_mut("sheet").unwrap().pressure_pa = Some(vec![250., 500.]);
        request
            .bindings
            .get_mut("sheet")
            .unwrap()
            .preparation
            .invariants
            .remove("specific_heat");
        assert!(request.validate(2).is_err());
    }

    #[test]
    fn source_mismatch_and_equal_capacity_mass_substitution_are_rejected() {
        let history = fixture(vec![250.]);
        let mut request = history.request.prescribed_compression.clone().unwrap();
        let record = selected(&request.bindings["sheet"].preparation);
        request
            .bindings
            .get_mut("sheet")
            .unwrap()
            .preparation
            .material
            .properties
            .get_mut("warp_stiffness")
            .unwrap()
            .value *= 2.;
        assert!(
            Compression::from_retained(
                &request,
                &[record],
                &["sheet".into()],
                &[0],
                vec![Patch {
                    vertices: [0, 1, 2],
                    reference_area_m2: 0.25,
                    column: 0
                }],
                3,
                1,
                "a".repeat(64)
            )
            .is_err()
        );

        let snapshot = history.model.snapshot();
        let original = &snapshot["columns"][0]["layers"][0];
        let mut material: ftx_pattern::materials::Material =
            serde_json::from_value(original["authored"].clone()).unwrap();
        material.properties.get_mut("areal_density").unwrap().value *= 2.;
        material.properties.get_mut("specific_heat").unwrap().value /= 2.;
        let replacement = empty_catalog()
            .resolve(&Selection {
                id: material.id.clone(),
                record: Some(material),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap();
        let model = DryThermalModel::new(vec![replacement], vec![], 1).unwrap();
        assert_eq!(
            snapshot["columns"][0]["cell_capacity_j_m2_k"],
            model.snapshot()["cell_capacity_j_m2_k"]
        );
        assert!(
            history
                .model
                .recondition_columns(&history.state, vec![(0, model)])
                .is_err()
        );
    }
    fn convection(air_c: f64, coefficient: f64) -> crate::thermal::steady::exterior::Exterior {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
        ))
        .unwrap();
        let mut exterior: crate::thermal::steady::exterior::Exterior =
            serde_json::from_value(fixture["exterior_exchange"].clone()).unwrap();
        exterior.air_c = air_c;
        exterior.convection_w_m2_k = coefficient;
        exterior.convection_conditions =
            "Synthetic independent moving sheet over the acquired dry pressure range".into();
        exterior.radiation = None;
        exterior
    }

    #[test]
    fn compressed_convection_preserves_reference_storage_and_current_area_exchange() {
        let mut history = fixture(vec![250., 500., 500.]);
        let environments = vec![convection(0., 4.), convection(10., 2.), convection(20., 0.)];
        for (film, exterior) in history.request.boundaries.iter_mut().zip(&environments) {
            film.inside_resistance_m2_k_w = 0.1;
            film.outside_c = exterior.air_c;
        }
        history.request.exterior_exchange = Some(environments.clone());
        history.request.validate(3).unwrap();
        let mut temperature = 0.;
        let mut outside_energy = 0.;
        let mut inside_energy = 0.;
        for (interval, (area, resistance)) in [(1., 0.375), (0.5, 0.25), (0.25, 0.25)]
            .into_iter()
            .enumerate()
        {
            let exterior = &environments[interval];
            // Independent one-cell RC answer. Capacity follows 0.25 m2 of source mass;
            // conductance uses accepted area and the conditioned half-cell resistance.
            let inside = area / (0.5 * resistance + 0.1);
            let outside = if exterior.convection_w_m2_k == 0. {
                0.
            } else {
                area / (0.5 * resistance + 1. / exterior.convection_w_m2_k)
            };
            temperature = (25. * temperature + inside * 40. + outside * exterior.air_c)
                / (25. + inside + outside);
            outside_energy += outside * (temperature - exterior.air_c);
            inside_energy += inside * (40. - temperature);
            history
                .advance(
                    &geometry(area),
                    (interval + 1) as f64,
                    interval,
                    interval,
                    None,
                    None,
                    None,
                )
                .unwrap();
            close(history.state.temperature_c()[0][0], temperature);
            close(history.stored_j, 25. * temperature);
            close(history.outside_j, outside_energy);
            close(history.convective_j, outside_energy);
            close(history.inside_j, inside_energy);
            close(history.radiative_j, 0.);
            close(history.balance_error_j(), 0.);
        }
        let report = history.finish("path", "initial", 3.).unwrap();
        close(
            report["prescribed_compression"]["cumulative_normal_work_j"]
                .as_f64()
                .unwrap(),
            1.5625,
        );
        assert_eq!(
            report["prescribed_compression"]["normal_work_to_heat_j"],
            0.
        );
        close(
            report["prescribed_compression"]["final_instances"]["sheet"]["reference_mass_kg"]
                .as_f64()
                .unwrap(),
            0.025,
        );
        assert_eq!(
            report["final"]["exterior_exchange"]["convective_heat_w"],
            0.
        );
        assert_eq!(report["exterior_exchange"]["radiative_energy_j"], 0.);
    }

    #[test]
    fn compressed_convection_refuses_unqualified_exchange_and_rolls_back_a_late_failure() {
        let mut history = fixture(vec![250., 500.]);
        for film in &mut history.request.boundaries {
            film.outside_c = 5.;
        }
        history.request.exterior_exchange = Some(vec![convection(5., 3.); 2]);
        history.request.validate(2).unwrap();
        history
            .advance(&geometry(1.), 1., 0, 0, None, None, None)
            .unwrap();
        let before = physical_state(&history);
        let convective = history.convective_j;
        let radiative = history.radiative_j;
        for fault in ["source", "consent", "domain"] {
            let mut rejected = history.clone();
            let exterior = &mut rejected.request.exterior_exchange.as_mut().unwrap()[1];
            match fault {
                "source" => exterior.convection_source.note.clear(),
                "consent" => exterior.allow_estimates = false,
                "domain" => exterior.surface_temperature_range_c = [-80., -70.],
                _ => unreachable!(),
            }
            assert!(
                rejected
                    .advance(&geometry(1.), 2., 1, 1, None, None, None)
                    .is_err(),
                "{fault}"
            );
            assert_eq!(physical_state(&rejected), before, "{fault}");
            assert_eq!(rejected.convective_j, convective, "{fault}");
            assert_eq!(rejected.radiative_j, radiative, "{fault}");
            rejected.request = history.request.clone();
            rejected
                .advance(&geometry(1.), 2., 1, 1, None, None, None)
                .unwrap();
            assert_eq!(rejected.compression.as_ref().unwrap().updates.len(), 2);
            close(
                rejected
                    .compression
                    .as_ref()
                    .unwrap()
                    .cumulative_normal_work_j,
                2.5,
            );
        }
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
        ))
        .unwrap();
        let radiation =
            serde_json::from_value(fixture["exterior_exchange"]["radiation"].clone()).unwrap();
        history.request.exterior_exchange.as_mut().unwrap()[0].radiation = Some(radiation);
        assert!(history.request.validate(2).is_err());
        history.request.exterior_exchange.as_mut().unwrap()[0].radiation = None;
        history.request.boundaries[0].outside_resistance_m2_k_w = 0.2;
        assert!(history.request.validate(2).is_err());
    }
}
