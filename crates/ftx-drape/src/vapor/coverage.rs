//! Isothermal vapor series paths over geometry-derived, disjoint body coverage.
//!
//! Projection shares the dry solver's clipping/order/allocation authority but requires no dry
//! transport or heat-storage properties. Each layer uses its own allocated current cloth area;
//! boundary films and explicit vapor interfaces use covered body area. Uncovered body fractions
//! and unallocated cloth are reported separately and contribute no invented transfer.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec3};
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, Selection},
    material_vapor,
    materials::Catalog,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    scene::Scene,
    thermal::surface::{
        Patch,
        io::{digest, retained_materials},
        projection::{self, ProjectionSource},
        validate_topology,
    },
};

/// Portable geometry and source request; no live catalog or body model is needed for replay.
pub const SCHEMA: &str = "ftx-vapor-coverage/1";
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_REGIONS: usize = 4096;
const MAX_LAYER_OCCURRENCES: usize = 16_384;

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn budget(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}
fn positive(value: f64, name: &str) -> Result<f64, Refusal> {
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid(format!("{name} must be positive and finite")));
    }
    Ok(value)
}
fn add(total: &mut f64, value: f64) -> Result<(), Refusal> {
    let sum = *total + value;
    if !value.is_finite() || value < 0.0 || !sum.is_finite() || (value > 0.0 && sum <= *total) {
        return Err(invalid(
            "vapor coverage accumulation overflows or loses a positive term",
        ));
    }
    *total = sum;
    Ok(())
}
fn area(vertices: [usize; 3], positions: &[Vec3]) -> Result<f64, Refusal> {
    let [a, b, c] = vertices.map(|i| positions[i]);
    positive(
        0.5 * (b - a).cross(c - a).norm(),
        "vapor coverage triangle area",
    )
}
fn empty_catalog() -> Catalog {
    Catalog {
        schema: "ftx-textile-catalog/1".into(),
        materials: vec![],
    }
}
fn bounded(value: &impl Serialize) -> Result<(), Refusal> {
    if serde_json::to_vec_pretty(value)
        .map_err(|e| invalid(e.to_string()))?
        .len()
        >= MAX_BYTES
    {
        return Err(budget("vapor coverage exceeds 64 MiB replay budget"));
    }
    Ok(())
}

/// Retained solver geometry and exact material assignments. It can also describe a declared
/// synthetic surface; its digest checks consistency, never physical measurement or authenticity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Must be `ftx-vapor-coverage-source/1`.
    pub schema: String,
    /// Current cloth vertices in the same metre frame as the supplied body boundary.
    pub positions_m: Vec<Vec3>,
    /// Retained element topology, reference areas and material-column indices.
    pub patches: Vec<Patch>,
    /// Embedded, digest-locked canonical material selections; no live catalog lookup is allowed.
    pub materials: Vec<Selection>,
    /// Retained drape instance identities in their original order.
    pub instances: Vec<String>,
    /// One instance index per retained patch.
    pub triangle_instance: Vec<usize>,
    /// Identity of the drape material report from which these selections were extracted.
    /// Consumers comparing with a drape must also use [`Self::matches_material`].
    pub material_report_sha256: String,
}

impl Source {
    /// Freeze actual retained material assignments and current geometry, without requiring Ret,
    /// dry resistance, heat capacity or body measurements.
    ///
    /// # Errors
    /// Invalid retained topology, inconsistent material receipts or exceeded source budgets.
    pub fn from_scene(scene: &Scene, material: &Value) -> Result<Self, Refusal> {
        let faces = &scene.solver.model.membranes;
        if faces.len() != scene.triangle_owner.len() {
            return Err(invalid(
                "vapor source triangle ownership differs from the retained cloth",
            ));
        }
        let (records, assignment) =
            retained_materials(material, &scene.mesh.instances, &scene.triangle_owner)?;
        let source = Self {
            schema: "ftx-vapor-coverage-source/1".into(),
            positions_m: scene.solver.x.clone(),
            patches: faces
                .iter()
                .zip(assignment)
                .map(|(face, column)| Patch {
                    vertices: face.v,
                    reference_area_m2: face.area,
                    column,
                })
                .collect(),
            materials: records
                .iter()
                .map(ResolvedMaterial::frozen_selection)
                .collect(),
            instances: scene.mesh.instances.clone(),
            triangle_instance: scene.triangle_owner.clone(),
            material_report_sha256: digest(material)?,
        };
        source.matches_material(material)?;
        bounded(&source)?;
        Ok(source)
    }

    /// Verify material identity AND every selection/patch assignment against the original drape.
    /// A caller-provided digest alone cannot establish that the embedded selections match it.
    ///
    /// # Errors
    /// Invalid source, changed report identity, changed selection or changed retained assignment.
    pub fn matches_material(&self, material: &Value) -> Result<(), Refusal> {
        self.resolve()?;
        if digest(material)? != self.material_report_sha256 {
            return Err(invalid(
                "vapor source belongs to a different material report",
            ));
        }
        let (records, assignment) =
            retained_materials(material, &self.instances, &self.triangle_instance)?;
        let selections: Vec<_> = records
            .iter()
            .map(ResolvedMaterial::frozen_selection)
            .collect();
        if serde_json::to_value(selections).map_err(|e| invalid(e.to_string()))?
            != serde_json::to_value(&self.materials).map_err(|e| invalid(e.to_string()))?
            || assignment
                .iter()
                .zip(&self.patches)
                .any(|(&column, p)| column != p.column)
        {
            return Err(invalid(
                "vapor source selections differ from retained material assignments",
            ));
        }
        Ok(())
    }

    fn resolve(&self) -> Result<Vec<ResolvedMaterial>, Refusal> {
        validate_topology(self.materials.len(), self.positions_m.len(), &self.patches)?;
        if self.schema != "ftx-vapor-coverage-source/1"
            || self.positions_m.iter().any(|v| !v.is_finite())
            || self.instances.is_empty()
            || self.instances.len() > 65_536
            || self
                .instances
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > 4096)
            || self.instances.iter().collect::<BTreeSet<_>>().len() != self.instances.len()
            || self.triangle_instance.len() != self.patches.len()
            || self
                .triangle_instance
                .iter()
                .any(|&i| i >= self.instances.len())
            || self.material_report_sha256.len() != 64
            || !self
                .material_report_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            || self
                .materials
                .iter()
                .any(|s| s.record.is_none() || s.expected_sha256.is_none())
        {
            return Err(invalid(
                "invalid vapor source geometry, owners or frozen material identities",
            ));
        }
        let mut instance_material = BTreeMap::new();
        for (patch, &owner) in self.patches.iter().zip(&self.triangle_instance) {
            area(patch.vertices, &self.positions_m)?;
            if let Some(old) = instance_material.insert(owner, patch.column)
                && old != patch.column
            {
                return Err(invalid(
                    "one retained instance cannot change material between triangles",
                ));
            }
        }
        self.materials
            .iter()
            .map(|s| empty_catalog().resolve(s))
            .collect()
    }
}

/// An explicit vapor interface per covered BODY area, independent of any dry heat interface.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    /// Inner physical layer identity.
    pub inner: String,
    /// Outer physical layer identity.
    pub outer: String,
    /// Finite nonnegative vapor resistance in m²·Pa/W; zero must be explicitly declared.
    pub resistance_m2_pa_w: f64,
    /// Source or explicit modeling assumption; proximity never supplies this coefficient.
    pub basis: String,
}

/// Geometric closure and explicitly supplied vapor interfaces; no heat coefficients or cells.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Positive outward normal search horizon in metres.
    pub max_distance_m: f64,
    /// Minimum absolute cloth/body normal cosine in (0,1].
    pub min_normal_cosine: f64,
    /// Positive distinguishable mid-surface separation below the search horizon, in metres.
    pub separation_tolerance_m: f64,
    /// Exactly defined directed interfaces for every encountered adjacent pair.
    pub interfaces: Vec<Interface>,
}

/// Body geometry and physical layer identities; overlap is recomputed on every evaluation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    /// Body boundary in the same metre frame as the retained cloth.
    pub body_positions_m: Vec<Vec3>,
    /// Outward-wound body triangles; global watertightness is not inferred.
    pub body_triangles: Vec<[usize; 3]>,
    /// Every retained instance exactly once, mapped to its physical layer name.
    pub instance_layers: BTreeMap<String, String>,
    /// Declared local-normal projection closure and vapor interfaces.
    pub settings: Settings,
}

/// Prescribed isothermal vapor reservoirs and boundary-area films; no saturation or sweat law.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    /// Nonnegative prescribed inner partial pressure in Pa, no lower than the outer pressure.
    pub inside_vapor_pressure_pa: f64,
    /// Nonnegative prescribed outer partial pressure in Pa.
    pub outside_vapor_pressure_pa: f64,
    /// Positive inner-film resistance in m²·Pa/W of covered body area.
    pub inside_film_resistance_m2_pa_w: f64,
    /// Positive outer-film resistance in m²·Pa/W of covered body area.
    pub outside_film_resistance_m2_pa_w: f64,
    /// Positive explicit latent heat in J/kg, solely for potential mass-flow conversion.
    pub latent_heat_j_kg: f64,
}

/// Uniform or body-triangle-indexed conditions and exact source-observation filters.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conditions {
    /// One uniform boundary, or exactly one per body triangle with a bound body mesh identity.
    pub boundaries: Vec<Boundary>,
    /// Required for nonuniform boundaries; populated on every frozen replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_mesh_sha256: Option<String>,
    /// Exact subset required of each consumed Ret observation's reported conditions.
    #[serde(default)]
    pub required_conditions: BTreeMap<String, String>,
    /// Optional exact observation method required of each consumed Ret observation.
    #[serde(default)]
    pub required_method: Option<String>,
    /// Explicitly admit differing complete observation protocols across consumed layer occurrences.
    #[serde(default)]
    pub allow_mixed_protocols: bool,
}

/// Complete portable material-following vapor coverage experiment.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// Frozen material selections, retained ownership and current cloth geometry.
    pub source: Source,
    /// Current body geometry, physical layers and vapor interfaces.
    pub map: Map,
    /// Prescribed boundaries and source qualification policy.
    pub conditions: Conditions,
}

impl Request {
    /// Recompute actual disjoint overlap and evaluate each ordered path through the canonical Ret
    /// consumer. Uncovered body fractions remain unknown and excluded from covered subtotals.
    ///
    /// # Errors
    /// Invalid/frozen-source mismatch, stale nonuniform boundaries, ambiguous coverage, missing
    /// vapor observations/interfaces, unadmitted evidence/protocols or exhausted replay budgets.
    pub fn evaluate(&self) -> Result<Value, Refusal> {
        if self.schema != SCHEMA
            || self.conditions.boundaries.is_empty()
            || self.conditions.boundaries.len() > 65_536
            || self.map.instance_layers.len() > 65_536
            || self.map.settings.interfaces.len() > 4096
            || self.conditions.required_conditions.len() > 64
        {
            return Err(invalid("invalid vapor coverage schema or input budget"));
        }
        let text_ok = |s: &str| !s.trim().is_empty() && s.len() <= 4096;
        if self
            .conditions
            .required_method
            .as_deref()
            .is_some_and(|s| !text_ok(s))
            || self
                .conditions
                .required_conditions
                .iter()
                .any(|(k, v)| !text_ok(k) || !text_ok(v))
        {
            return Err(invalid("invalid vapor source filter text"));
        }
        bounded(self)?;
        let records = self.source.resolve()?;
        let mut layer_names = BTreeSet::new();
        if self.map.instance_layers.len() != self.source.instances.len() {
            return Err(invalid(
                "vapor coverage needs exactly one physical layer per retained instance",
            ));
        }
        for instance in &self.source.instances {
            let name = self
                .map
                .instance_layers
                .get(instance)
                .filter(|s| !s.trim().is_empty() && s.len() <= 4096)
                .ok_or_else(|| invalid("vapor coverage has a missing physical layer assignment"))?;
            layer_names.insert(name.as_str());
        }
        let mut interfaces = BTreeMap::new();
        for interface in &self.map.settings.interfaces {
            if interface.inner == interface.outer
                || !layer_names.contains(interface.inner.as_str())
                || !layer_names.contains(interface.outer.as_str())
                || interface.basis.trim().is_empty()
                || interface.basis.len() > 4096
                || !interface.resistance_m2_pa_w.is_finite()
                || interface.resistance_m2_pa_w < 0.0
                || interfaces
                    .insert(
                        (interface.inner.as_str(), interface.outer.as_str()),
                        interface,
                    )
                    .is_some()
            {
                return Err(invalid("invalid, duplicate or unsupported vapor interface"));
            }
        }
        let body_sha = digest(
            &json!({"positions_m":self.map.body_positions_m,"triangles":self.map.body_triangles}),
        )?;
        if self
            .conditions
            .body_mesh_sha256
            .as_ref()
            .is_some_and(|s| *s != body_sha)
            || (self.conditions.boundaries.len() != 1 && self.conditions.body_mesh_sha256.is_none())
            || (self.conditions.boundaries.len() != 1
                && self.conditions.boundaries.len() != self.map.body_triangles.len())
        {
            return Err(invalid(
                "nonuniform vapor boundaries require the exact body_mesh_sha256 and one boundary per body triangle",
            ));
        }
        // Validate every boundary even if its body triangle ends up uncovered. This checks no
        // material capability and does not assign a fictitious bare-skin resistance.
        for b in &self.conditions.boundaries {
            if !b.inside_vapor_pressure_pa.is_finite()
                || !b.outside_vapor_pressure_pa.is_finite()
                || b.outside_vapor_pressure_pa < 0.0
                || b.inside_vapor_pressure_pa < b.outside_vapor_pressure_pa
            {
                return Err(invalid(
                    "vapor boundaries need finite inside >= outside >= 0 Pa",
                ));
            }
            positive(b.inside_film_resistance_m2_pa_w, "inside vapor film")?;
            positive(b.outside_film_resistance_m2_pa_w, "outside vapor film")?;
            positive(b.latent_heat_j_kg, "latent heat")?;
        }
        let source_snapshot =
            serde_json::to_value(&self.source).map_err(|e| invalid(e.to_string()))?;
        let geometry = ProjectionSource {
            vertex_count: self.source.positions_m.len(),
            patches: &self.source.patches,
            layers: records.iter().map(std::slice::from_ref).collect(),
            snapshot: source_snapshot.clone(),
        };
        let projection = projection::project_geometry(
            &geometry,
            &self.source.positions_m,
            &self.map.body_positions_m,
            &self.map.body_triangles,
            &projection::Request {
                schema: "ftx-thermal-projection/1".into(),
                cloth_layers: self
                    .source
                    .triangle_instance
                    .iter()
                    .map(|&owner| self.map.instance_layers[&self.source.instances[owner]].clone())
                    .collect(),
                settings: projection::Settings {
                    max_distance_m: self.map.settings.max_distance_m,
                    min_normal_cosine: self.map.settings.min_normal_cosine,
                    separation_tolerance_m: self.map.settings.separation_tolerance_m,
                    cells_per_layer: 1,
                    interfaces: vec![],
                },
            },
        )?;
        let (covered, allocated) = projection.layout.allocations(self.source.patches.len())?;
        let occurrences: usize = projection
            .layout
            .regions
            .iter()
            .map(|r| r.layers.len())
            .sum();
        if projection.layout.regions.len() > MAX_REGIONS || occurrences > MAX_LAYER_OCCURRENCES {
            return Err(budget(
                "vapor coverage exceeds 4096 regions or 16384 layer occurrences",
            ));
        }
        let selection_bytes: Vec<_> = self
            .source
            .materials
            .iter()
            .map(|s| {
                serde_json::to_vec(s)
                    .map(|v| v.len())
                    .map_err(|e| invalid(e.to_string()))
            })
            .collect::<Result<_, _>>()?;
        let mut expanded = projection.layout.regions.len().saturating_mul(12_288);
        for r in &projection.layout.regions {
            for l in &r.layers {
                expanded = expanded.saturating_add(
                    selection_bytes[self.source.patches[l.cloth_triangle].column].saturating_mul(8),
                );
            }
        }
        if expanded > MAX_BYTES {
            return Err(budget(
                "vapor source expansion exceeds the replay budget; reduce the boundary mesh",
            ));
        }
        let body_areas = self
            .map
            .body_triangles
            .iter()
            .map(|&f| area(f, &self.map.body_positions_m))
            .collect::<Result<Vec<_>, _>>()?;
        let cloth_areas = self
            .source
            .patches
            .iter()
            .map(|p| area(p.vertices, &self.source.positions_m))
            .collect::<Result<Vec<_>, _>>()?;
        let mut regions = Vec::new();
        let mut heat = 0.0;
        let mut mass = 0.0;
        let mut first_protocol = None;
        let mut mixed_protocols = false;
        for r in &projection.layout.regions {
            let body_area = positive(
                body_areas[r.body_triangle] * r.body_fraction,
                "covered body area",
            )?;
            let b = self.conditions.boundaries[if self.conditions.boundaries.len() == 1 {
                0
            } else {
                r.body_triangle
            }];
            let layer_areas: Vec<_> = r
                .layers
                .iter()
                .map(|p| {
                    positive(
                        cloth_areas[p.cloth_triangle] * p.fraction,
                        "allocated cloth area",
                    )
                })
                .collect::<Result<_, _>>()?;
            let interface_values = r
                .layers
                .windows(2)
                .map(|pair| {
                    interfaces
                        .get(&(pair[0].layer.as_str(), pair[1].layer.as_str()))
                        .map(|i| i.resistance_m2_pa_w)
                        .ok_or_else(|| {
                            invalid(format!(
                                "missing explicit vapor interface {} -> {}",
                                pair[0].layer, pair[1].layer
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let vapor = material_vapor::Request {
                schema: material_vapor::SCHEMA.into(),
                layers: r
                    .layers
                    .iter()
                    .zip(&layer_areas)
                    .map(|(p, &a)| material_vapor::Layer {
                        key: p.layer.clone(),
                        selection: self.source.materials
                            [self.source.patches[p.cloth_triangle].column]
                            .clone(),
                        area_m2: Some(a),
                    })
                    .collect(),
                area_m2: body_area,
                inside_vapor_pressure_pa: b.inside_vapor_pressure_pa,
                outside_vapor_pressure_pa: b.outside_vapor_pressure_pa,
                inside_film_resistance_m2_pa_w: b.inside_film_resistance_m2_pa_w,
                outside_film_resistance_m2_pa_w: b.outside_film_resistance_m2_pa_w,
                latent_heat_j_kg: b.latent_heat_j_kg,
                required_conditions: self.conditions.required_conditions.clone(),
                required_method: self.conditions.required_method.clone(),
                allow_mixed_protocols: self.conditions.allow_mixed_protocols,
                interface_resistance_m2_pa_w: interface_values,
            }
            .evaluate(&empty_catalog())?;
            for layer in &vapor.layers {
                let protocol = (
                    layer.observation.method.clone(),
                    layer.observation.conditions.clone(),
                );
                if let Some(first) = &first_protocol {
                    mixed_protocols |= first != &protocol;
                } else {
                    first_protocol = Some(protocol);
                }
            }
            if mixed_protocols && !self.conditions.allow_mixed_protocols {
                return Err(invalid(
                    "vapor coverage mixes reported methods/conditions across regions; explicit allow_mixed_protocols is required",
                ));
            }
            add(&mut heat, vapor.potential_heat_flow_w)?;
            add(&mut mass, vapor.potential_mass_flow_kg_s)?;
            regions.push(
                json!({"id":r.id,"body_triangle":r.body_triangle,"body_area_m2":body_area,
                "layer_area_m2":layer_areas,"vapor":vapor}),
            );
        }
        let mut body_area = 0.0;
        let mut covered_area = 0.0;
        let mut uncovered_area = 0.0;
        let mut body = Vec::new();
        for (&a, &fraction) in body_areas.iter().zip(&covered) {
            let covered = a * fraction;
            let uncovered = a * (1.0 - fraction);
            add(&mut body_area, a)?;
            add(&mut covered_area, covered)?;
            add(&mut uncovered_area, uncovered)?;
            body.push(json!({"area_m2":a,"covered_area_m2":covered,"uncovered_area_m2":uncovered}));
        }
        let mut unassigned = 0.0;
        for (p, &fraction) in self.source.patches.iter().zip(&allocated) {
            add(&mut unassigned, p.reference_area_m2 * (1.0 - fraction))?;
        }
        let mut replay = self.clone();
        replay.conditions.body_mesh_sha256 = Some(body_sha.clone());
        let request = serde_json::to_value(&replay).map_err(|e| invalid(e.to_string()))?;
        let geometry_regions: Vec<_> = projection.layout.regions.iter().map(|r|
            json!({"id":r.id,"body_triangle":r.body_triangle,"body_fraction":r.body_fraction,"layers":r.layers})
        ).collect();
        let result = json!({
            "schema":"ftx-vapor-coverage-report/1", "request_sha256":digest(&request)?,
            "replay_request_json":serde_json::to_string(&replay).map_err(|e| invalid(e.to_string()))?,
            "request":request,"source_sha256":digest(&source_snapshot)?,"body_mesh_sha256":body_sha,
            "mixed_protocols":mixed_protocols,
            "projection":{"schema":"ftx-vapor-coverage-projection/1","regions":geometry_regions,
                "footprints":projection.footprints,"input_sha256":projection.input_sha256,
                "candidate_pairs":projection.candidate_pairs,"geometry_operations":projection.geometry_operations,
                "partition_roundoff_m2":projection.partition_roundoff_m2,
                "unresolved_footprint_area_m2":projection.unresolved_footprint_area_m2,
                "body_roundoff_adjustment_m2":projection.body_roundoff_adjustment_m2,
                "cloth_roundoff_adjustment_m2":projection.cloth_roundoff_adjustment_m2},
            "coverage":{"regions":regions,"body":body,"body_area_m2":body_area,
                "covered_area_m2":covered_area,"uncovered_area_m2":uncovered_area,
                "unassigned_cloth_reference_area_m2":unassigned,
                "covered_potential_heat_flow_w":heat,"covered_potential_mass_flow_kg_s":mass},
            "storage_required":false,"dry_thermal_properties_required":false,
            "physical_validation_claim":false,"comfort_claim":false,"uncertainty_propagated":false,
            "notes":[
                "Disjoint covered body regions are counted once. Each layer Ret uses its allocated current cloth area; boundary films and explicitly declared vapor interfaces use covered body area. Strain dependence of Ret is not characterized by this geometric rescaling.",
                "Covered subtotals exclude uncovered body fractions and unallocated cloth. Their vapor transfer is unknown, not zero; these are not total wearer heat loss or comfort predictions.",
                "The local-normal projection, isothermal reservoirs and fixed source resistances are explicit modeling assumptions. No air-gap vapor coefficient, sweat supply, actual evaporation, condensation, moisture storage or latent coupling into dry heat is inferred."
            ]
        });
        bounded(&result)?;
        Ok(result)
    }
}
