//! Prescribed, source-qualified normal pressure on independent moving thermal sheets.
//!
//! The existing pressure conditioner owns material acquisition and branch work. Pressure is an
//! authored boundary, never an inference from a contact force or guessed contact area. Each
//! instantaneous pressure traverse uses the accepted geometry's area; subsequent heat storage
//! still follows unchanged reference mass. Normal work is recorded separately, never heating.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec2, Vec3};
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, normalize},
    material_compression::{
        self,
        regions::{MAX_REGION_EDGE_TESTS, PreparedSelector, Selector, triangle_centroid_mm},
        stack,
    },
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
        steady::exterior::Exterior,
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
    /// Omit for versions 3 and 4; even an empty or null legacy schedule is not a second control.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "pressures"
    )]
    pub pressure_pa: Option<Vec<f64>>,
    /// Versions 3 and 4: one explicit pressure or clearance boundary per keyframe interval.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "controls"
    )]
    pub controls: Option<Vec<Control>>,
    /// Versions 2..4: exactly one entry per interval; null continues the active branch.
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

/// A fixed flat-material region with its own source-qualified prescribed history.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionalBinding {
    /// Physical retained instance; stack-layer and acquired branch identities use this name.
    pub instance: String,
    /// Whole-triangle membership in mirrored, simulation-scaled flat millimetres.
    pub selector: Selector,
    /// The existing explicit pressure/clearance controls and optional acquired branch joins.
    pub binding: Binding,
}

/// Optional dry pressure/heat closure on physical instances or fixed material regions.
#[derive(Clone, Debug, Serialize)]
pub struct Request {
    /// `ftx-motion-surface-compression/1` for prescribed pressure on one monotone branch, /2
    /// for qualified reversals, /3 for explicit controls, or /4 for named flat-material regions.
    pub schema: String,
    /// Explicit application evidence for constant dry thermal coefficients, temperature/time
    /// range, prescribed pressure and films, and omission of normal-work conversion into heat.
    pub thermal_evidence: String,
    /// One to 256 exact retained instance bindings. A shared material may have different pressures
    /// on different instances. Versions 1..3 only; omitted completely for version 4.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, Binding>,
    /// Version 4 only: one to 256 named regions. Unselected triangles retain their original material.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<BTreeMap<String, RegionalBinding>>,
}

impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        fn bindings<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<BTreeMap<String, Binding>>, D::Error> {
            BTreeMap::deserialize(deserializer).map(Some)
        }
        fn regions<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<BTreeMap<String, RegionalBinding>>, D::Error> {
            BTreeMap::deserialize(deserializer).map(Some)
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            schema: String,
            thermal_evidence: String,
            #[serde(default, deserialize_with = "bindings")]
            bindings: Option<BTreeMap<String, Binding>>,
            #[serde(default, deserialize_with = "regions")]
            regions: Option<BTreeMap<String, RegionalBinding>>,
        }
        let fields = Fields::deserialize(deserializer)?;
        let regional = fields.schema == "ftx-motion-surface-compression/4";
        if (regional && (fields.bindings.is_some() || fields.regions.is_none()))
            || (!regional && (fields.bindings.is_none() || fields.regions.is_some()))
        {
            return Err(serde::de::Error::custom(
                "compression /4 requires regions without bindings; versions 1..3 require bindings without regions",
            ));
        }
        Ok(Self {
            schema: fields.schema,
            thermal_evidence: fields.thermal_evidence,
            bindings: fields.bindings.unwrap_or_default(),
            regions: fields.regions,
        })
    }
}

impl Request {
    pub(super) fn validate(&self, intervals: usize) -> Result<(), Refusal> {
        let cyclic = self.schema == "ftx-motion-surface-compression/2";
        let regional = self.schema == "ftx-motion-surface-compression/4";
        let controlled = regional || self.schema == "ftx-motion-surface-compression/3";
        let count = if regional {
            self.regions.as_ref().map_or(0, BTreeMap::len)
        } else {
            self.bindings.len()
        };
        if (!cyclic && !controlled && self.schema != "ftx-motion-surface-compression/1")
            || self.thermal_evidence.trim().is_empty()
            || self.thermal_evidence.len() > 4096
            || !(1..=256).contains(&count)
            || !(1..=16).contains(&intervals)
            || (regional && !self.bindings.is_empty())
            || (!regional && self.regions.is_some())
        {
            return Err(invalid(
                "prescribed compression needs schema /1..4, explicit thermal evidence and 1..256 schema-qualified bindings or regions",
            ));
        }
        let mut bindings = Vec::with_capacity(count);
        if let Some(regions) = &self.regions {
            for (name, region) in regions {
                if name.trim().is_empty() || name.len() > 512 {
                    return Err(invalid("compression region needs a nonempty bounded name"));
                }
                PreparedSelector::new(&region.selector)?;
                bindings.push((&region.instance, &region.binding));
            }
        } else {
            bindings.extend(self.bindings.iter());
        }
        let mut joined = false;
        for (id, binding) in bindings {
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

pub(super) fn validate_exterior(exterior: &Exterior) -> Result<(), Refusal> {
    exterior.validate_convection()?;
    if let Some(radiation) = &exterior.radiation {
        let invalid_selection = if radiation.automatic_material_face {
            !radiation.emissivity_property.is_empty() || !radiation.exposed_face.is_empty()
        } else {
            radiation.emissivity_property.trim().is_empty()
                || radiation.exposed_face.trim().is_empty()
        };
        if invalid_selection
            || !radiation.allow_opaque_gray_approximation
            || !radiation.ambient_c.is_finite()
            || radiation.ambient_c < -273.15
        {
            return Err(invalid(
                "compressed radiation needs either a qualified source-side mapping or an explicit face and emissivity, physical enclosure temperature and opaque-gray consent",
            ));
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
    owner: String,
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
    regional: bool,
    source_model: Value,
    initial_model: Value,
    initial_instances: Value,
    instances: BTreeMap<String, Instance>,
    remainders: Value,
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
        let patches: Vec<_> = faces
            .iter()
            .zip(assignment)
            .map(|(face, column)| Patch {
                vertices: face.v,
                reference_area_m2: face.area,
                column,
            })
            .collect();
        if request.schema == "ftx-motion-surface-compression/4" {
            let mut rest_triangles_mm = Vec::with_capacity(faces.len());
            for face in &run.scene.mesh.faces {
                let welded = face.map(|vertex| run.scene.weld[vertex]);
                if welded[0] == welded[1] || welded[1] == welded[2] || welded[0] == welded[2] {
                    continue;
                }
                let index = rest_triangles_mm.len();
                if faces.get(index).is_none_or(|retained| retained.v != welded)
                    || run.scene.triangle_owner.get(index) != Some(&run.scene.mesh.owner[face[0]])
                {
                    return Err(invalid(
                        "regional compression flat source differs from retained cloth",
                    ));
                }
                rest_triangles_mm.push(face.map(|vertex| run.scene.mesh.verts2d[vertex]));
            }
            return Self::from_regions(
                request,
                &records,
                &run.scene.mesh.instances,
                &run.scene.triangle_owner,
                patches,
                rest_triangles_mm,
                run.scene.solver.x.len(),
                cells_per_layer,
                hash(&run.report["material"])?,
            );
        }
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
        if request.regions.is_some()
            || owners.len() != patches.len()
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
                        owner: id.clone(),
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
        let initial_instances = instance_snapshot(&instances, false);
        let compression = Self {
            cyclic: request.schema == "ftx-motion-surface-compression/2",
            controlled: request.schema == "ftx-motion-surface-compression/3",
            regional: false,
            source_model,
            model_sha256: initial_model["sha256"]
                .as_str()
                .expect("surface identity")
                .into(),
            initial_model,
            initial_instances,
            instances,
            remainders: Value::Null,
            cells_per_layer,
            last_interval: None,
            updates: Vec::new(),
            cumulative_normal_work_j: 0.,
        };
        Ok((model, compression))
    }

    #[allow(clippy::too_many_arguments)]
    fn from_regions(
        request: &Request,
        records: &[ResolvedMaterial],
        instance_names: &[String],
        owners: &[usize],
        mut patches: Vec<Patch>,
        rest_triangles_mm: Vec<[Vec2; 3]>,
        vertex_count: usize,
        cells_per_layer: usize,
        material_report_sha256: String,
    ) -> Result<(Surface, Self), Refusal> {
        let regions = request
            .regions
            .as_ref()
            .ok_or_else(|| invalid("regional compression needs named regions"))?;
        let retained = owners.iter().copied().collect::<BTreeSet<_>>();
        if request.schema != "ftx-motion-surface-compression/4"
            || !request.bindings.is_empty()
            || !(1..=256).contains(&regions.len())
            || patches.is_empty()
            || patches.len() > 65_536
            || owners.len() != patches.len()
            || rest_triangles_mm.len() != patches.len()
            || retained.iter().any(|&owner| owner >= instance_names.len())
            || regions.values().any(|region| {
                !retained
                    .iter()
                    .any(|&owner| instance_names[owner] == region.instance)
            })
        {
            return Err(invalid(
                "regional compression needs exact retained instances and complete flat source triangles",
            ));
        }
        for (triangle, patch) in rest_triangles_mm.iter().zip(&patches) {
            triangle_centroid_mm(*triangle)?;
            let [a, b, c] = *triangle;
            let flat_area = positive(0.5 * (b - a).cross(c - a).abs() * 1e-6, "flat source area")?;
            positive(patch.reference_area_m2, "reference source area")?;
            if (flat_area - patch.reference_area_m2).abs()
                > 1e-10 * flat_area.max(patch.reference_area_m2)
            {
                return Err(invalid(
                    "regional compression flat area differs from retained reference area",
                ));
            }
        }
        let mut owned_by_instance: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (index, &owner) in owners.iter().enumerate() {
            owned_by_instance.entry(owner).or_default().push(index);
        }
        let mut edge_tests = 0_usize;
        let mut prepared = Vec::with_capacity(regions.len());
        for (name, region) in regions {
            let selector = PreparedSelector::new(&region.selector)?;
            let count = owned_by_instance
                .iter()
                .find(|(owner, _)| instance_names[**owner] == region.instance)
                .map_or(0, |(_, owned)| owned.len());
            edge_tests = edge_tests
                .checked_add(
                    count
                        .checked_mul(region.selector.polygon_mm.len())
                        .ok_or_else(|| {
                            invalid("regional compression selector work exceeds its bound")
                        })?,
                )
                .ok_or_else(|| invalid("regional compression selector work exceeds its bound"))?;
            if edge_tests > MAX_REGION_EDGE_TESTS {
                return Err(invalid(
                    "regional compression selector work exceeds its bound",
                ));
            }
            prepared.push((name, region, selector));
        }
        let mut assigned = vec![false; patches.len()];
        let mut source_columns = Vec::new();
        let mut column_instances = Vec::new();
        let mut column_regions: Vec<Option<String>> = Vec::new();
        let mut columns = Vec::new();
        let mut instances = BTreeMap::new();
        let mut remainders = BTreeMap::new();
        for (owner, owned) in owned_by_instance {
            let id = &instance_names[owner];
            let source_index = patches[owned[0]].column;
            let source = records
                .get(source_index)
                .ok_or_else(|| invalid("regional compression source material is missing"))?;
            if owned
                .iter()
                .any(|&index| patches[index].column != source_index)
            {
                return Err(invalid(
                    "one regional compression instance has conflicting source materials",
                ));
            }
            for (name, region, selector) in prepared
                .iter()
                .filter(|(_, region, _)| region.instance == *id)
            {
                let mut selected = Vec::new();
                for &index in &owned {
                    if selector.contains_triangle_centroid(rest_triangles_mm[index])? {
                        if assigned[index] {
                            return Err(invalid(
                                "compression regions select the same retained triangle",
                            ));
                        }
                        assigned[index] = true;
                        selected.push(index);
                    }
                }
                if selected.is_empty() {
                    return Err(invalid("compression region selects no retained triangle"));
                }
                let binding = &region.binding;
                if binding.material_sha256 != source.sha256()
                    || hash(&normalize(&binding.preparation.material)?)? != hash(source.material())?
                {
                    return Err(invalid(
                        "regional compression acquisition must match the exact selected material and normalized source record",
                    ));
                }
                let conditioned = material_compression::condition(&binding.preparation)?;
                let material = empty_catalog().resolve(&conditioned.selection)?;
                let area = positive(
                    selected
                        .iter()
                        .map(|&index| patches[index].reference_area_m2)
                        .sum(),
                    "region reference area",
                )?;
                let mass = positive(quantity(&material, "areal_density", "kg/m2")?, "areal mass")?;
                if mass != quantity(source, "areal_density", "kg/m2")?
                    || quantity(&material, "specific_heat", "J/(kg*K)")?
                        != quantity(source, "specific_heat", "J/(kg*K)")?
                {
                    return Err(invalid(
                        "regional compression cannot change selected reference mass or specific heat",
                    ));
                }
                let column = columns.len();
                let state = InstanceState {
                    column,
                    material_sha256: source.sha256().into(),
                    conditioned_material_sha256: material.sha256().into(),
                    pressure_pa: binding.preparation.pressure_pa,
                    thickness_m: positive(quantity(&material, "thickness", "m")?, "thickness")?,
                    reference_area_m2: area,
                    reference_mass_kg: positive(area * mass, "reference mass")?,
                    cumulative_normal_work_j: 0.,
                };
                columns.push(DryThermalModel::new(
                    vec![material],
                    vec![],
                    cells_per_layer,
                )?);
                source_columns.push(source.clone());
                column_instances.push(id.clone());
                column_regions.push(Some((*name).clone()));
                for &index in &selected {
                    patches[index].column = column;
                }
                instances.insert(
                    (*name).clone(),
                    Instance {
                        owner: id.clone(),
                        binding: binding.clone(),
                        preparation: binding.preparation.clone(),
                        direction: binding.direction,
                        patches: selected,
                        state,
                    },
                );
            }
            let remainder: Vec<_> = owned
                .into_iter()
                .filter(|&index| !assigned[index])
                .collect();
            if !remainder.is_empty() {
                let column = columns.len();
                let area = positive(
                    remainder
                        .iter()
                        .map(|&index| patches[index].reference_area_m2)
                        .sum(),
                    "remainder reference area",
                )?;
                let mass = positive(
                    quantity(source, "areal_density", "kg/m2")?,
                    "remainder areal mass",
                )?;
                // The untouched original record must supply its own thermal properties. A region's
                // acquired law cannot silently qualify transport on the unselected material.
                columns.push(DryThermalModel::new(
                    vec![source.clone()],
                    vec![],
                    cells_per_layer,
                )?);
                source_columns.push(source.clone());
                column_instances.push(id.clone());
                column_regions.push(None);
                for &index in &remainder {
                    patches[index].column = column;
                    assigned[index] = true;
                }
                remainders.insert(
                    id.clone(),
                    json!({"column":column,"source_triangles":remainder,
                    "material_sha256":source.sha256(),"reference_area_m2":area,
                    "reference_mass_kg":positive(area * mass, "remainder reference mass")?}),
                );
            }
        }
        if assigned.iter().any(|selected| !selected) {
            return Err(invalid(
                "regional compression did not partition every retained triangle",
            ));
        }
        let model = Surface::new(columns, vertex_count, patches.clone())?;
        let initial_model = model.snapshot();
        let mut source_model = json!({"schema":"ftx-motion-compression-source/2",
            "material_report_sha256":material_report_sha256,
            "cells_per_layer":cells_per_layer,"vertex_count":vertex_count,
            "instances":instance_names,"triangle_instance":owners,
            "column_instances":column_instances,"column_regions":column_regions,
            "columns":source_columns,"patches":patches,"rest_triangles_mm":rest_triangles_mm});
        source_model["sha256"] = json!(hash(&source_model)?);
        let compression = Self {
            cyclic: false,
            controlled: true,
            regional: true,
            source_model,
            model_sha256: initial_model["sha256"]
                .as_str()
                .expect("surface identity")
                .into(),
            initial_model,
            initial_instances: instance_snapshot(&instances, true),
            instances,
            remainders: json!(remainders),
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
                    id: instance.owner.clone(),
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
            let mut update = state_snapshot(instance, self.regional);
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
        let mut update = json!({"interval_index":interval,"sample_index":sample_index,
            "time_s":time_s,"positions_sha256":hash(&positions)?,
            "model_sha256":self.model_sha256});
        update[if self.regional {
            "regions"
        } else {
            "instances"
        }] = json!(updates);
        self.updates.push(update);
        self.last_interval = Some(interval);
        Ok(Some((next, temperature)))
    }

    pub(super) fn summary(&self) -> Value {
        let mut summary = json!({"model_sha256":self.model_sha256,
            "cumulative_normal_work_j":self.cumulative_normal_work_j});
        summary[if self.regional {
            "regions"
        } else {
            "instances"
        }] = instance_snapshot(&self.instances, self.regional);
        summary
    }

    pub(super) fn finish(&self) -> Value {
        let mut result = json!({"schema":if self.regional { "ftx-motion-surface-compression-result/4" }
        else if self.controlled { "ftx-motion-surface-compression-result/3" }
        else if self.cyclic { "ftx-motion-surface-compression-result/2" }
        else { "ftx-motion-surface-compression-result/1" },
        "material_report_sha256":self.source_model["material_report_sha256"],
        "source_model":self.source_model,"initial_model":self.initial_model,
        "updates":self.updates,
        "final_model_sha256":self.model_sha256,
        "cumulative_normal_work_j":self.cumulative_normal_work_j,"normal_work_to_heat_j":0.,
        "scope":if self.regional {
            "Explicit pressure or rigid-face clearance on fixed whole-triangle material regions selected by convex flat-rest-mm polygons. Region histories follow the same retained cloth triangles; unselected triangles retain independently qualified original material. Every optional acquired reversal preserves actual pressure, thickness, reference mass and temperature cells with zero entry work. Normal work uses current accepted selected-triangle area while thermal storage follows unchanged reference mass. No geometric clipping, pressure inferred from force or contact, geometry feedback, inferred recovery-time or minor-loop law, wet law, work-to-heat conversion or empirical garment claim."
        } else if self.controlled {
            "Explicit uniform pressure or rigid-face clearance per retained instance on source-qualified dry branches. Clearance uses the shared inverse stack authority; every optional reversal preserves actual pressure, thickness, reference mass and temperature cells with zero entry work. Each instantaneous traverse uses current accepted area held fixed during that traverse; subsequent heat storage follows unchanged reference mass. No contact-derived pressure or gap, geometry feedback, inferred recovery-time or minor-loop law, wet law, work-to-heat conversion or empirical garment claim."
        } else if self.cyclic {
            "Prescribed uniform pressure per retained instance on explicitly joined source-qualified dry branches. Every reversal preserves actual pressure, thickness, reference mass and temperature cells with zero entry work. Each instantaneous traverse uses current accepted area held fixed during that traverse; subsequent heat storage follows unchanged reference mass. No inferred recovery-time or minor-loop law, contact pressure, geometry feedback, wet law, work-to-heat conversion or empirical garment claim."
        } else {
            "Prescribed uniform pressure per retained instance on its qualified monotone dry branch. Each instantaneous traverse uses current accepted area held fixed during that traverse; subsequent heat storage follows unchanged reference mass. No pressure inferred from contact, geometry feedback, branch reversal, wet law, work-to-heat conversion or empirical garment claim."
        }});
        result[if self.regional {
            "initial_regions"
        } else {
            "initial_instances"
        }] = self.initial_instances.clone();
        result[if self.regional {
            "final_regions"
        } else {
            "final_instances"
        }] = instance_snapshot(&self.instances, self.regional);
        if self.regional {
            result["remainders"] = self.remainders.clone();
        }
        result
    }
}

fn state_snapshot(instance: &Instance, regional: bool) -> Value {
    let mut state = json!(instance.state);
    if regional {
        state["instance"] = json!(instance.owner);
        state["source_triangles"] = json!(instance.patches);
    }
    state
}

fn instance_snapshot(instances: &BTreeMap<String, Instance>, regional: bool) -> Value {
    json!(
        instances
            .iter()
            .map(|(id, instance)| (id, state_snapshot(instance, regional)))
            .collect::<BTreeMap<_, _>>()
    )
}

#[cfg(test)]
pub(super) mod tests {
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

    pub(in crate::study::heat) fn optical_acquisition() -> material_compression::Input {
        let mut input = acquisition();
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
        ))
        .unwrap();
        let observed = fixture["layers"][0]["record"]["properties"]["emissivity"].clone();
        let mut optical: ftx_pattern::materials::Quantity =
            serde_json::from_value(observed).unwrap();
        optical.conditions.extend([
            ("specimen".into(), input.specimen.clone()),
            ("conditioning".into(), input.conditioning.clone()),
            ("loading_branch".into(), input.loading_branch.clone()),
        ]);
        let mut invariant = input.invariants["specific_heat"].clone();
        invariant.method =
            "Synthetic pressure-independent optical coefficient; no measured textile claim".into();
        invariant.conditions = optical.conditions.clone();
        input.invariants.insert("emissivity".into(), invariant);
        input
            .material
            .properties
            .insert("emissivity".into(), optical);
        input
    }

    pub(in crate::study::heat) fn mapped_optical_acquisition() -> material_compression::Input {
        let mut input = optical_acquisition();
        let mut reverse = input.material.properties["emissivity"].clone();
        reverse.value = 0.2;
        reverse.bounds = None;
        reverse.conditions.insert("face".into(), "reverse".into());
        let mut invariant = input.invariants["emissivity"].clone();
        invariant.conditions = reverse.conditions.clone();
        input
            .invariants
            .insert("emissivity_reverse".into(), invariant);
        input
            .material
            .properties
            .insert("emissivity_reverse".into(), reverse);
        input.material.optical_faces = Some(
            serde_json::from_value(json!({
                "positive":{"face":"public","emissivity_property":"emissivity"},
                "negative":{"face":"reverse","emissivity_property":"emissivity_reverse"},
                "source":"synthetic","evidence":"estimate",
                "method":"Synthetic original orientation; no physical face qualification",
                "specimen":input.specimen,"conditioning":input.conditioning
            }))
            .unwrap(),
        );
        input.invariants.insert(
            "optical_faces".into(),
            input.invariants["specific_heat"].clone(),
        );
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
            regions: None,
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

    fn regional_request() -> Request {
        let mut request = controlled_request(vec![
            Control::Pressure { pressure_pa: 250. },
            Control::Pressure { pressure_pa: 500. },
        ]);
        let pressure = request.bindings.remove("sheet").unwrap();
        let mut clearance = pressure.clone();
        clearance.controls = Some(vec![
            Control::Gap { gap_m: 0.018 },
            Control::Gap { gap_m: 0.015 },
        ]);
        let selector = |left: f64| Selector {
            kind: "whole_triangle_centroid".into(),
            polygon_mm: [
                [left - 100., -100.],
                [left + 1100., -100.],
                [left + 1100., 600.],
                [left - 100., 600.],
            ]
            .map(Vec2::from)
            .to_vec(),
        };
        request.schema = "ftx-motion-surface-compression/4".into();
        request.regions = Some(BTreeMap::from([
            (
                "pressure-zone".into(),
                RegionalBinding {
                    instance: "sheet".into(),
                    selector: selector(0.),
                    binding: pressure,
                },
            ),
            (
                "clearance-zone".into(),
                RegionalBinding {
                    instance: "sheet".into(),
                    selector: selector(2000.),
                    binding: clearance,
                },
            ),
        ]));
        request
    }

    fn regional_source(request: &Request) -> Result<(Surface, Compression), Refusal> {
        let preparation = &request
            .regions
            .as_ref()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .binding
            .preparation;
        let triangles = [0., 2000., 4000.].map(|left| {
            [
                Vec2::new(left, 0.),
                Vec2::new(left + 1000., 0.),
                Vec2::new(left, 500.),
            ]
        });
        Compression::from_regions(
            request,
            &[selected(preparation)],
            &["sheet".into()],
            &[0, 0, 0],
            (0..3)
                .map(|index| Patch {
                    vertices: [3 * index, 3 * index + 1, 3 * index + 2],
                    reference_area_m2: 0.25,
                    column: 0,
                })
                .collect(),
            triangles.to_vec(),
            9,
            1,
            "a".repeat(64),
        )
    }

    fn regional_history(request: Request) -> SurfaceHistory {
        request.validate(2).unwrap();
        let (model, compression) = regional_source(&request).unwrap();
        let policy: SurfaceRequest = serde_json::from_value(json!({
            "schema":"ftx-motion-surface-heat/1","initial_temperature_c":0.,
            "cells_per_layer":1,"max_step_s":1.,"max_cell_steps":100,
            "boundaries":vec![json!({"inside_c":40.,"outside_c":0.,
                "inside_resistance_m2_k_w":0.,"outside_resistance_m2_k_w":0.});2],
            "prescribed_compression":request,
        }))
        .unwrap();
        policy.validate(2).unwrap();
        let mut history =
            SurfaceHistory::with_instances(policy, model, &["sheet".into()], &[0, 0, 0]).unwrap();
        history.compression = Some(compression);
        history
    }

    fn regional_geometry(areas: [f64; 3]) -> Vec<Vec3> {
        areas
            .into_iter()
            .enumerate()
            .flat_map(|(index, area)| {
                let origin = Vec3::new(10. * index as f64, 0., 0.);
                [
                    origin,
                    origin + Vec3::new(2. * area, 0., 0.),
                    origin + Vec3::new(0., 1., 0.),
                ]
            })
            .collect()
    }

    #[test]
    fn regional_pressure_and_clearance_preserve_remainder_mass_and_independent_heat() {
        let request = regional_request();
        let raw = serde_json::to_value(&request).unwrap();
        assert!(raw.get("bindings").is_none());
        assert_eq!(
            serde_json::to_value(serde_json::from_value::<Request>(raw.clone()).unwrap()).unwrap(),
            raw
        );
        let mut history = regional_history(request);
        let source_hash =
            history.compression.as_ref().unwrap().source_model["columns"][0]["sha256"].clone();
        let mut temperatures = [0.; 3];
        for (interval, (areas, resistances)) in [
            ([1., 0.5, 0.25], [0.375, 0.45, 0.5]),
            ([0.5, 1., 0.125], [0.25, 0.375, 0.5]),
        ]
        .into_iter()
        .enumerate()
        {
            history
                .advance(
                    &regional_geometry(areas),
                    (interval + 1) as f64,
                    interval,
                    interval,
                    None,
                    None,
                    None,
                )
                .unwrap();
            for patch in 0..3 {
                temperatures[patch] =
                    expected_step(temperatures[patch], areas[patch], resistances[patch], 1.);
                close(history.state.temperature_c()[patch][0], temperatures[patch]);
            }
            close(history.stored_j, 25. * temperatures.iter().sum::<f64>());
            close(history.balance_error_j(), 0.);
        }
        let result = history.finish("regional", "initial", 2.).unwrap();
        let report = &result["prescribed_compression"];
        assert_eq!(report["schema"], "ftx-motion-surface-compression-result/4");
        assert_eq!(
            report["source_model"]["schema"],
            "ftx-motion-compression-source/2"
        );
        assert_eq!(
            report["source_model"]["column_instances"],
            json!(["sheet", "sheet", "sheet"])
        );
        assert_eq!(
            report["source_model"]["column_regions"],
            json!(["clearance-zone", "pressure-zone", null])
        );
        assert_eq!(
            report["final_regions"]["pressure-zone"]["source_triangles"],
            json!([0])
        );
        assert_eq!(
            report["final_regions"]["clearance-zone"]["source_triangles"],
            json!([1])
        );
        assert_eq!(
            report["remainders"]["sheet"]["source_triangles"],
            json!([2])
        );
        for name in ["pressure-zone", "clearance-zone"] {
            assert_eq!(report["final_regions"][name]["instance"], "sheet");
            close(
                report["final_regions"][name]["reference_mass_kg"]
                    .as_f64()
                    .unwrap(),
                0.025,
            );
        }
        assert_eq!(
            report["remainders"]["sheet"]["material_sha256"],
            source_hash
        );
        close(
            report["remainders"]["sheet"]["reference_mass_kg"]
                .as_f64()
                .unwrap(),
            0.025,
        );
        close(
            report["final_regions"]["pressure-zone"]["cumulative_normal_work_j"]
                .as_f64()
                .unwrap(),
            1.5625,
        );
        close(
            report["final_regions"]["clearance-zone"]["cumulative_normal_work_j"]
                .as_f64()
                .unwrap(),
            0.575,
        );
        assert_eq!(report["normal_work_to_heat_j"], 0.);
        assert!(report.get("initial_instances").is_none());
        assert!(report["updates"][0].get("instances").is_none());
        assert!(
            result["trace"][0]["prescribed_compression"]
                .get("regions")
                .is_some()
        );
        Surface::from_snapshot(&result["model"]).unwrap();
    }

    #[test]
    fn regional_partition_refuses_overlap_empty_stale_and_implicit_remainder_transport() {
        let original = regional_request();
        for case in 0..6 {
            let mut request = original.clone();
            let regions = request.regions.as_mut().unwrap();
            match case {
                0 => {
                    regions.get_mut("pressure-zone").unwrap().selector =
                        regions["clearance-zone"].selector.clone()
                }
                1 => {
                    for point in &mut regions
                        .get_mut("pressure-zone")
                        .unwrap()
                        .selector
                        .polygon_mm
                    {
                        point.x += 20_000.;
                    }
                }
                2 => regions.get_mut("pressure-zone").unwrap().instance = "unknown".into(),
                3 => {
                    regions
                        .get_mut("pressure-zone")
                        .unwrap()
                        .binding
                        .material_sha256 = "0".repeat(64)
                }
                4 => {
                    regions
                        .get_mut("pressure-zone")
                        .unwrap()
                        .binding
                        .preparation
                        .material
                        .name
                        .push_str(" stale");
                }
                _ => {
                    for region in regions.values_mut() {
                        region
                            .binding
                            .preparation
                            .material
                            .properties
                            .remove("thermal_resistance");
                        region
                            .binding
                            .preparation
                            .material
                            .properties
                            .remove("thermal_conductivity");
                        region.binding.material_sha256 =
                            selected(&region.binding.preparation).sha256().into();
                    }
                }
            }
            assert!(regional_source(&request).is_err(), "case {case}");
        }
        let raw = serde_json::to_value(original).unwrap();
        for value in [Value::Null, json!({})] {
            let mut mixed = raw.clone();
            mixed["bindings"] = value;
            assert!(serde_json::from_value::<Request>(mixed).is_err());
        }
        let mut legacy = raw;
        legacy["schema"] = json!("ftx-motion-surface-compression/3");
        assert!(serde_json::from_value::<Request>(legacy).is_err());
    }

    #[test]
    fn regional_branch_join_keeps_physical_owner_and_fully_selected_material_needs_no_remainder() {
        let mut request = regional_request();
        let mut region = request
            .regions
            .take()
            .unwrap()
            .remove("pressure-zone")
            .unwrap();
        for quantity in region.binding.preparation.material.properties.values_mut() {
            quantity.conditions.remove("loading_branch");
        }
        region
            .binding
            .preparation
            .material
            .properties
            .remove("thermal_resistance");
        region
            .binding
            .preparation
            .material
            .properties
            .remove("thermal_conductivity");
        region.binding.material_sha256 = selected(&region.binding.preparation).sha256().into();
        region.selector.polygon_mm = [[-100., -100.], [6000., -100.], [6000., 600.], [-100., 600.]]
            .map(Vec2::from)
            .to_vec();
        region.binding.controls = Some(vec![
            Control::Pressure { pressure_pa: 500. },
            Control::Gap { gap_m: 0.012 },
        ]);
        region.binding.branch_changes = Some(vec![
            None,
            Some(reversal(&region.binding.preparation, true)),
        ]);
        request.regions = Some(BTreeMap::from([("named-zone".into(), region)]));
        let mut history = regional_history(request);
        for interval in 0..2 {
            history
                .advance(
                    &regional_geometry([0.25; 3]),
                    (interval + 1) as f64,
                    interval,
                    interval,
                    None,
                    None,
                    None,
                )
                .unwrap();
        }
        let result = history.finish("regional-cycle", "initial", 2.).unwrap();
        let report = &result["prescribed_compression"];
        assert_eq!(report["remainders"], json!({}));
        assert_eq!(
            report["final_regions"]["named-zone"]["source_triangles"],
            json!([0, 1, 2])
        );
        assert_eq!(report["final_regions"]["named-zone"]["pressure_pa"], 250.);
        assert_eq!(
            report["updates"][1]["regions"]["named-zone"]["branch_change"]["change"]["entry"]["layers"]
                [0]["id"],
            "sheet"
        );
        close(report["cumulative_normal_work_j"].as_f64().unwrap(), 1.3125);
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

    fn radiating_fixture(pressures: Vec<f64>) -> SurfaceHistory {
        let mut policy = fixture(pressures).request.prescribed_compression.unwrap();
        let binding = policy.bindings.get_mut("sheet").unwrap();
        binding.preparation = optical_acquisition();
        binding.material_sha256 = selected(&binding.preparation).sha256().into();
        history_from(policy)
    }

    fn radiation(air_c: f64, coefficient: f64, enclosure_c: f64) -> Exterior {
        let mut exterior = convection(air_c, coefficient);
        exterior.radiation = Some(crate::thermal::steady::exterior::AmbientRadiation {
            ambient_c: enclosure_c,
            emissivity_property: "emissivity".into(),
            exposed_face: "public".into(),
            automatic_material_face: false,
            allow_opaque_gray_approximation: true,
        });
        exterior
    }

    fn automatic_radiation() -> Exterior {
        let mut exterior = radiation(0., 3., -10.);
        let optical = exterior.radiation.as_mut().unwrap();
        optical.automatic_material_face = true;
        optical.emissivity_property.clear();
        optical.exposed_face.clear();
        exterior
    }

    #[test]
    fn automatic_compressed_radiation_matches_explicit_through_qualified_branch_changes() {
        let mut policy = cyclic_request();
        let binding = policy.bindings.get_mut("sheet").unwrap();
        binding.preparation = mapped_optical_acquisition();
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
        let mut automatic = history_from(policy);
        let mut explicit = automatic.clone();
        automatic.request.exterior_exchange = Some(vec![automatic_radiation(); 4]);
        explicit.request.exterior_exchange = Some(vec![radiation(0., 3., -10.); 4]);
        automatic.request.validate(4).unwrap();
        explicit.request.validate(4).unwrap();
        for interval in 0..4 {
            let area = 1. / (interval + 1) as f64;
            for history in [&mut automatic, &mut explicit] {
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
            }
            assert_eq!(
                automatic.state.temperature_c(),
                explicit.state.temperature_c()
            );
            assert_eq!(automatic.stored_j, explicit.stored_j);
            assert_eq!(automatic.inside_j, explicit.inside_j);
            assert_eq!(automatic.radiative_j, explicit.radiative_j);
            assert_eq!(automatic.outside_j, explicit.outside_j);
            assert_eq!(
                automatic.compression.as_ref().unwrap().finish(),
                explicit.compression.as_ref().unwrap().finish()
            );
            assert!(automatic.balance_error_j().abs() < 1e-8);
        }
        let report = automatic.finish("path", "initial", 4.).unwrap();
        let face = &report["final"]["exterior_exchange"]["patches"][0]["material_face"];
        assert_eq!(face["side"], "positive");
        assert_eq!(face["face"], "public");
        assert_eq!(face["emissivity_property"], "emissivity");
        close(
            report["prescribed_compression"]["final_instances"]["sheet"]["reference_mass_kg"]
                .as_f64()
                .unwrap(),
            0.025,
        );
        assert_eq!(
            report["prescribed_compression"]["normal_work_to_heat_j"],
            0.
        );
    }

    #[test]
    fn automatic_compressed_radiation_requires_mapping_and_rolls_back_its_failed_qualification() {
        let mut policy = fixture(vec![250., 500.])
            .request
            .prescribed_compression
            .unwrap();
        let binding = policy.bindings.get_mut("sheet").unwrap();
        binding.preparation = mapped_optical_acquisition();
        binding.material_sha256 = selected(&binding.preparation).sha256().into();
        let mut history = history_from(policy);
        history.request.exterior_exchange = Some(vec![automatic_radiation(); 2]);
        history.request.validate(2).unwrap();
        history
            .advance(&geometry(1.), 1., 0, 0, None, None, None)
            .unwrap();
        let before = history.clone().finish("path", "initial", 1.).unwrap();
        for fault in ["qualifier", "domain", "mapping", "property"] {
            let mut rejected = history.clone();
            let preparation = &mut rejected
                .compression
                .as_mut()
                .unwrap()
                .instances
                .get_mut("sheet")
                .unwrap()
                .preparation;
            match fault {
                "qualifier" => {
                    preparation.invariants.remove("optical_faces");
                    preparation.invariants.remove("emissivity_reverse");
                }
                "domain" => {
                    preparation
                        .invariants
                        .get_mut("optical_faces")
                        .unwrap()
                        .pressure_range_pa = [0., 250.];
                }
                "mapping" => preparation.material.optical_faces = None,
                "property" => {
                    preparation.invariants.remove("emissivity_reverse");
                }
                _ => unreachable!(),
            }
            assert!(
                rejected
                    .advance(&geometry(1.), 2., 1, 1, None, None, None)
                    .is_err(),
                "{fault}"
            );
            rejected
                .compression
                .as_mut()
                .unwrap()
                .instances
                .get_mut("sheet")
                .unwrap()
                .preparation = history.compression.as_ref().unwrap().instances["sheet"]
                .preparation
                .clone();
            assert_eq!(
                rejected.clone().finish("path", "initial", 1.).unwrap(),
                before,
                "{fault}"
            );
            rejected
                .advance(&geometry(1.), 2., 1, 1, None, None, None)
                .unwrap();
        }
        let unqualified = radiating_fixture(vec![250.]);
        assert!(
            unqualified
                .model
                .validate_exterior_sources(&automatic_radiation())
                .is_err()
        );
    }

    #[test]
    fn compressed_radiation_matches_independent_nonlinear_rc_balance() {
        // The exterior solver bounds its original equation using the separate signed fluxes.
        // Their cancellation must not shrink this oracle's energy comparison below that budget.
        let answer = |actual: f64, expected: f64| {
            assert!(
                (actual - expected).abs() < 1e-8,
                "{actual} differs from {expected}"
            );
        };
        let mut history = radiating_fixture(vec![250., 500., 500.]);
        let environments = vec![
            radiation(0., 4., -10.),
            radiation(10., 0., 60.),
            radiation(20., 2., -10.),
        ];
        for (film, exterior) in history.request.boundaries.iter_mut().zip(&environments) {
            film.inside_resistance_m2_k_w = 0.1;
            film.outside_c = exterior.air_c;
        }
        history.request.exterior_exchange = Some(environments.clone());
        history.request.validate(3).unwrap();
        let mut temperature = 0.;
        let (mut inside_energy, mut convective_energy, mut radiative_energy) = (0., 0., 0.);
        for (interval, (area, resistance)) in [(1., 0.375), (0.5, 0.25), (0.25, 0.25)]
            .into_iter()
            .enumerate()
        {
            let exterior = &environments[interval];
            let enclosure = exterior.radiation.as_ref().unwrap().ambient_c;
            let inside = area / (0.5 * resistance + 0.1);
            let outside = area / (0.5 * resistance);
            // Independently eliminate the one reference-mass cell, then solve its exterior
            // face balance using the unfactored fourth-power law. Capacity is 25 J/K and dt=1 s.
            let state = |face: f64| {
                let cell =
                    (25. * temperature + inside * 40. + outside * face) / (25. + inside + outside);
                let convection = area * exterior.convection_w_m2_k * (face - exterior.air_c);
                let radiation = area
                    * 0.8
                    * 5.670_374_419e-8
                    * ((face + 273.15).powi(4) - (enclosure + 273.15).powi(4));
                (cell, convection, radiation)
            };
            let (mut lo, mut hi) = (-80., 80.);
            for _ in 0..96 {
                let face = (lo + hi) * 0.5;
                let (cell, convection, radiation) = state(face);
                if convection + radiation > outside * (cell - face) {
                    hi = face;
                } else {
                    lo = face;
                }
            }
            let (expected, convective, radiative) = state((lo + hi) * 0.5);
            temperature = expected;
            inside_energy += inside * (40. - temperature);
            convective_energy += convective;
            radiative_energy += radiative;
            assert_eq!(radiative < 0., interval == 1);
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
            answer(history.state.temperature_c()[0][0], temperature);
            answer(history.stored_j, 25. * temperature);
            answer(history.inside_j, inside_energy);
            answer(history.convective_j, convective_energy);
            answer(history.radiative_j, radiative_energy);
            answer(history.outside_j, convective_energy + radiative_energy);
            answer(history.balance_error_j(), 0.);
        }
        let report = history.finish("path", "initial", 3.).unwrap();
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
        answer(
            report["exterior_exchange"]["radiative_energy_j"]
                .as_f64()
                .unwrap(),
            radiative_energy,
        );
    }

    #[test]
    fn compressed_radiation_requires_retained_optics_and_rolls_back_late_source_failures() {
        let mut history = radiating_fixture(vec![250., 500.]);
        history.request.exterior_exchange = Some(vec![radiation(0., 3., -10.); 2]);
        history.request.validate(2).unwrap();
        history
            .advance(&geometry(1.), 1., 0, 0, None, None, None)
            .unwrap();
        let before = history.clone().finish("path", "initial", 1.).unwrap();
        for fault in [
            "missing",
            "face",
            "spectral",
            "temperature",
            "invariance",
            "consent",
        ] {
            let mut rejected = history.clone();
            if fault == "consent" {
                rejected.request.exterior_exchange.as_mut().unwrap()[1]
                    .radiation
                    .as_mut()
                    .unwrap()
                    .allow_opaque_gray_approximation = false;
            } else {
                let preparation = &mut rejected
                    .compression
                    .as_mut()
                    .unwrap()
                    .instances
                    .get_mut("sheet")
                    .unwrap()
                    .preparation;
                match fault {
                    "missing" => {
                        preparation.invariants.remove("emissivity");
                    }
                    "invariance" => {
                        preparation
                            .invariants
                            .get_mut("emissivity")
                            .unwrap()
                            .pressure_range_pa = [0., 250.];
                    }
                    _ => {
                        let (key, value) = match fault {
                            "face" => ("face", "reverse"),
                            "spectral" => ("spectral_basis", "visible_only"),
                            "temperature" => ("temperature_min_c", "0"),
                            _ => unreachable!(),
                        };
                        preparation
                            .material
                            .properties
                            .get_mut("emissivity")
                            .unwrap()
                            .conditions
                            .insert(key.into(), value.into());
                        preparation
                            .invariants
                            .get_mut("emissivity")
                            .unwrap()
                            .conditions
                            .insert(key.into(), value.into());
                    }
                }
            }
            assert!(
                rejected
                    .advance(&geometry(1.), 2., 1, 1, None, None, None)
                    .is_err(),
                "{fault}"
            );
            // Only the authored fault differs; none of its pressure, state or energy trial commits.
            rejected.request = history.request.clone();
            rejected
                .compression
                .as_mut()
                .unwrap()
                .instances
                .get_mut("sheet")
                .unwrap()
                .preparation = history.compression.as_ref().unwrap().instances["sheet"]
                .preparation
                .clone();
            assert_eq!(
                rejected.clone().finish("path", "initial", 1.).unwrap(),
                before,
                "{fault}"
            );
            rejected
                .advance(&geometry(1.), 2., 1, 1, None, None, None)
                .unwrap();
        }
        let bare = fixture(vec![250., 500.]);
        assert!(
            bare.model
                .validate_exterior_sources(&radiation(0., 3., -10.))
                .is_err()
        );
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
        history.request.validate(2).unwrap();
        assert!(
            history
                .model
                .validate_exterior_sources(&history.request.exterior_exchange.as_ref().unwrap()[0])
                .is_err()
        );
        history.request.exterior_exchange.as_mut().unwrap()[0]
            .radiation
            .as_mut()
            .unwrap()
            .automatic_material_face = true;
        assert!(history.request.validate(2).is_err());
        history.request.exterior_exchange.as_mut().unwrap()[0].radiation = None;
        history.request.boundaries[0].outside_resistance_m2_k_w = 0.2;
        assert!(history.request.validate(2).is_err());
    }
}
