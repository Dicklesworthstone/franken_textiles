//! Conditional vapor-transfer potential on the actual retained cloth, independent of dry heat.
//!
//! Each material column reuses the canonical source-qualified Ret stack consumer. Columns are
//! independent sheets: overlapping lining/shell areas are not a body-coverage or series model.
//! Current cloth area integrates potential flux; no water supply or latent-energy feedback is added.

use std::collections::BTreeMap;

use ftx_core::{Refusal, Vec3};
use ftx_pattern::{
    material_catalog::ResolvedMaterial,
    material_vapor,
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    invalid, positive,
    surface::{
        Patch,
        io::{digest, retained_materials},
        validate_topology,
    },
};
use crate::scene::Scene;

/// Prescribed vapor reservoirs and films for one independent sheet column.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    /// Inner vapor partial pressure in Pa; no saturation or relative-humidity conversion is inferred.
    pub inside_vapor_pressure_pa: f64,
    /// Outer vapor partial pressure in Pa, no greater than the inner pressure.
    pub outside_vapor_pressure_pa: f64,
    /// Positive inner film resistance in m² Pa/W, separate from measured material Ret.
    pub inside_film_resistance_m2_pa_w: f64,
    /// Positive outer film resistance in m² Pa/W, separate from measured material Ret.
    pub outside_film_resistance_m2_pa_w: f64,
    /// Positive caller-supplied latent heat in J/kg, only for converting potential heat to mass.
    pub latent_heat_j_kg: f64,
}

/// Portable source policy and boundary conditions on the retained, independently exposed sheets.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conditions {
    /// Must be `ftx-vapor-surface-conditions/1`.
    pub schema: String,
    /// One entry broadcasts uniformly; otherwise exactly one per retained material column.
    pub boundaries: Vec<Boundary>,
    /// Required for nonuniform boundaries so substitution cannot silently reorder their meaning.
    /// Optional with a uniform boundary; exact frozen replay always binds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_sha256: Option<String>,
    /// Exact subset required of every canonical Ret observation's source conditions.
    #[serde(default)]
    pub required_conditions: BTreeMap<String, String>,
    /// Optional exact Ret observation method required of every material column.
    #[serde(default)]
    pub required_method: Option<String>,
    /// Explicitly admit differences in complete reported methods/conditions between columns.
    /// This does not relax source filters, independent estimate consent or content locks.
    #[serde(default)]
    pub allow_mixed_protocols: bool,
}

struct Surface {
    records: Vec<ResolvedMaterial>,
    patches: Vec<Patch>,
    positions: Vec<Vec3>,
    instances: Vec<String>,
    owners: Vec<usize>,
}

impl Surface {
    fn validate(&self) -> Result<Vec<f64>, Refusal> {
        validate_topology(self.records.len(), self.positions.len(), &self.patches)?;
        if self.patches.len() != self.owners.len()
            || self.positions.iter().any(|p| !p.is_finite())
            || self.owners.iter().any(|&i| i >= self.instances.len())
        {
            return Err(invalid(
                "invalid retained vapor surface positions or ownership",
            ));
        }
        self.patches
            .iter()
            .map(|p| {
                let [a, b, c] = p.vertices.map(|i| self.positions[i]);
                positive(
                    "current vapor triangle area",
                    0.5 * (b - a).cross(c - a).norm(),
                )
            })
            .collect()
    }

    fn model(&self) -> Result<Value, Refusal> {
        let mut model = json!({"schema":"ftx-vapor-surface/1",
            "columns":self.records.iter().map(ResolvedMaterial::frozen_selection).collect::<Vec<_>>(),
            "vertex_count":self.positions.len(),"patches":self.patches,
            "geometry_policy":"fixed source Ret per current cloth area; independent sheets"});
        model["sha256"] = json!(digest(&model)?);
        Ok(model)
    }

    fn frame(&self, material: &Value) -> Result<Value, Refusal> {
        self.validate()?;
        let mut frame = json!({"schema":"ftx-drape-vapor-surface/1","available":true,
            "model":self.model()?,"positions_m":self.positions,
            "material_report_sha256":digest(material)?,
            "triangle_instance":self.owners,"instances":self.instances,
            "scope":"retained independent sheet geometry; transport requires explicit vapor conditions; no inferred body coverage"});
        frame["sha256"] = json!(digest(&frame)?);
        Ok(frame)
    }

    fn restore(report: &Value) -> Result<Self, Refusal> {
        let frame = &report["vapor_surface"];
        if frame["available"] != true || frame["schema"] != "ftx-drape-vapor-surface/1" {
            return Err(invalid(format!(
                "drape vapor surface unavailable: {}",
                frame["refusal"]
            )));
        }
        let instances: Vec<String> = serde_json::from_value(frame["instances"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        let owners: Vec<usize> = serde_json::from_value(frame["triangle_instance"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        let (records, assignment) = retained_materials(&report["material"], &instances, &owners)?;
        let patches: Vec<Patch> = serde_json::from_value(frame["model"]["patches"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        if assignment.len() != patches.len()
            || assignment
                .iter()
                .zip(&patches)
                .any(|(&column, p)| column != p.column)
        {
            return Err(invalid(
                "vapor patch material differs from retained instance assignment",
            ));
        }
        let surface = Self {
            records,
            patches,
            positions: serde_json::from_value(frame["positions_m"].clone())
                .map_err(|e| invalid(e.to_string()))?,
            instances,
            owners,
        };
        // Rebuild the entire frame from canonical materials and validated geometry. This also
        // rejects stale hashes, unknown fields and a source-policy change disguised as the same ID.
        if surface.frame(&report["material"])? != *frame {
            return Err(invalid("vapor frame or canonical material receipt changed"));
        }
        Ok(surface)
    }
}

fn bounded(value: &impl Serialize) -> Result<(), Refusal> {
    if serde_json::to_vec_pretty(value)
        .map_err(|e| invalid(e.to_string()))?
        .len()
        >= 64 * 1024 * 1024
    {
        return Err(invalid("vapor surface exceeds 64 MiB replay budget"));
    }
    Ok(())
}

fn scaled(value: f64, area: f64) -> Result<f64, Refusal> {
    let result = value * area;
    if !result.is_finite() || (value > 0.0 && result <= 0.0) {
        return Err(invalid(
            "vapor patch flux arithmetic overflows or underflows",
        ));
    }
    Ok(result)
}

impl Conditions {
    /// Resolve measured Ret on source-locked retained cloth and integrate each column over its
    /// current triangle areas. Missing dry conductivity, heat capacity or density is irrelevant.
    /// Source methods/conditions are checked by the same consumer as `thermal-vapor`.
    ///
    /// # Errors
    /// Invalid conditions, unavailable/stale source geometry, denied estimates, missing Ret,
    /// incompatible source policies, nonrepresentable arithmetic or exceeded replay budgets.
    pub fn evaluate(&self, report: &Value) -> Result<Value, Refusal> {
        if self.schema != "ftx-vapor-surface-conditions/1"
            || self.boundaries.is_empty()
            || self.boundaries.len() > 65_536
        {
            return Err(invalid("invalid vapor surface schema or boundary count"));
        }
        let retained =
            json!({"material":report["material"],"vapor_surface":report["vapor_surface"]});
        bounded(&retained)?;
        let surface = Surface::restore(&retained)?;
        let frame = &retained["vapor_surface"];
        let model_sha256 = frame["model"]["sha256"]
            .as_str()
            .ok_or_else(|| invalid("missing vapor model identity"))?;
        if self
            .model_sha256
            .as_deref()
            .is_some_and(|s| s != model_sha256)
            || (self.boundaries.len() != 1 && self.model_sha256.is_none())
            || (self.boundaries.len() != 1 && self.boundaries.len() != surface.records.len())
        {
            return Err(invalid(
                "vapor boundaries need one uniform entry or one per column with the exact model_sha256",
            ));
        }
        let areas = surface.validate()?;
        let empty = Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: vec![],
        };
        let mut columns: Vec<material_vapor::Report> = Vec::with_capacity(surface.records.len());
        let mut mixed = false;
        for (i, record) in surface.records.iter().enumerate() {
            let b = &self.boundaries[if self.boundaries.len() == 1 { 0 } else { i }];
            let column = material_vapor::Request {
                schema: material_vapor::SCHEMA.into(),
                layers: vec![material_vapor::Layer {
                    key: "sheet".into(),
                    selection: record.frozen_selection(),
                    area_m2: None,
                }],
                area_m2: 1.0,
                inside_vapor_pressure_pa: b.inside_vapor_pressure_pa,
                outside_vapor_pressure_pa: b.outside_vapor_pressure_pa,
                inside_film_resistance_m2_pa_w: b.inside_film_resistance_m2_pa_w,
                outside_film_resistance_m2_pa_w: b.outside_film_resistance_m2_pa_w,
                interface_resistance_m2_pa_w: vec![],
                latent_heat_j_kg: b.latent_heat_j_kg,
                required_conditions: self.required_conditions.clone(),
                required_method: self.required_method.clone(),
                allow_mixed_protocols: self.allow_mixed_protocols,
            }
            .evaluate(&empty)?;
            if let Some(first) = columns.first() {
                mixed |= first.layers[0].observation.method != column.layers[0].observation.method
                    || first.layers[0].observation.conditions
                        != column.layers[0].observation.conditions;
            }
            if mixed && !self.allow_mixed_protocols {
                return Err(invalid(
                    "vapor surface mixes reported methods/conditions; explicit allow_mixed_protocols is required",
                ));
            }
            columns.push(column);
        }
        let mut pieces: Vec<Value> = surface.instances.iter().map(|id| json!({
            "instance":id,"area_m2":0.0,"potential_heat_flow_w":0.0,"potential_mass_flow_kg_s":0.0
        })).collect();
        let mut patches = Vec::with_capacity(surface.patches.len());
        let mut area_m2 = 0.0;
        let mut heat_w = 0.0;
        let mut mass_kg_s = 0.0;
        for ((patch, area), &owner) in surface.patches.iter().zip(areas).zip(&surface.owners) {
            let column = &columns[patch.column];
            let heat = scaled(column.potential_heat_flux_w_m2, area)?;
            let mass = scaled(column.potential_mass_flux_kg_m2_s, area)?;
            area_m2 += area;
            heat_w += heat;
            mass_kg_s += mass;
            for (key, value) in [
                ("area_m2", area),
                ("potential_heat_flow_w", heat),
                ("potential_mass_flow_kg_s", mass),
            ] {
                let sum = pieces[owner][key]
                    .as_f64()
                    .expect("initialized finite total")
                    + value;
                if !sum.is_finite() {
                    return Err(invalid("vapor piece total overflows"));
                }
                pieces[owner][key] = json!(sum);
            }
            patches.push(
                json!({"column":patch.column,"instance":owner,"area_m2":area,
                "potential_heat_flux_w_m2":column.potential_heat_flux_w_m2,
                "potential_mass_flux_kg_m2_s":column.potential_mass_flux_kg_m2_s,
                "potential_heat_flow_w":heat,"potential_mass_flow_kg_s":mass}),
            );
        }
        if !area_m2.is_finite() || !heat_w.is_finite() || !mass_kg_s.is_finite() {
            return Err(invalid("vapor surface totals overflow"));
        }
        let mut conditions = self.clone();
        conditions.model_sha256 = Some(model_sha256.into());
        let request = json!({"report":retained,"conditions":conditions});
        let result = json!({"schema":"ftx-vapor-surface-result/1",
            "drape_vapor_frame_sha256":frame["sha256"],"model_sha256":model_sha256,
            "request_sha256":digest(&request)?,"replay_request_json":serde_json::to_string(&request).map_err(|e| invalid(e.to_string()))?,
            "request":request,"positions_m":surface.positions,"patch_topology":surface.patches,
            "triangle_instance":surface.owners,"instances":surface.instances,
            "columns":columns,"patches":patches,"pieces":pieces,
            "area_m2":area_m2,"potential_heat_flow_w":heat_w,"potential_mass_flow_kg_s":mass_kg_s,
            "mixed_protocols":mixed,"dry_transport_required":false,"storage_required":false,
            "physical_validation_claim":false,"comfort_claim":false,"uncertainty_propagated":false,
            "effective_model":"constant_source_resistance_independent_current_area_sheets/1",
            "scope":"independent sheet transfer potentials, not total garment evaporation or wearer heat loss",
            "notes":["Each canonical column report is evaluated at one square metre; only patch, piece and surface totals use actual current cloth areas.",
                "Overlapping sheets remain independent; no body coverage, interlayer series resistance, exposure, air-gap or bare-skin path is inferred.",
                "Source Ret is held constant at its recorded characterization. Area change does not identify stretch-dependent permeability or a new humidity/temperature domain.",
                "Water availability, sweating, moisture storage, condensation and latent-energy feedback into dry heat are not modeled. Source bounds and variability are retained, not propagated."]});
        bounded(&result)?;
        Ok(result)
    }
}

/// Retain source-qualified material ownership and actual cloth geometry without requiring dry heat
/// or Ret readiness. The latter is evaluated only when the caller supplies explicit vapor conditions.
pub(crate) fn for_drape(scene: &Scene, material: &Value) -> Value {
    let result = (|| {
        let (records, assignment) =
            retained_materials(material, &scene.mesh.instances, &scene.triangle_owner)?;
        if assignment.len() != scene.solver.model.membranes.len() {
            return Err(invalid(
                "vapor topology differs from retained cloth elements",
            ));
        }
        Surface {
            records,
            patches: scene
                .solver
                .model
                .membranes
                .iter()
                .zip(assignment)
                .map(|(m, column)| Patch {
                    vertices: m.v,
                    reference_area_m2: m.area,
                    column,
                })
                .collect(),
            positions: scene.solver.x.clone(),
            instances: scene.mesh.instances.clone(),
            owners: scene.triangle_owner.clone(),
        }
        .frame(material)
    })();
    result.unwrap_or_else(|refusal| json!({"available":false,"refusal":refusal}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_pattern::material_catalog::Selection;

    fn record(id: &str, ret: f64, method: &str) -> ResolvedMaterial {
        let selection: Selection = serde_json::from_value(json!({"id":id,"allow_estimates":true,
            "record":{"id":id,"name":id,"form":"knit",
                "properties":{"vapor_resistance":{"value":ret,"unit":"m2*Pa/W",
                    "evidence":"estimate","source":"test","method":method,
                    "conditions":{"temperature_c":"35"}}},
                "sources":{"test":{"reference":"test:vapor-surface","accessed_on":"2026-10-22",
                    "note":"Synthetic resistance only; no density, mechanics or dry heat"}}}}))
        .unwrap();
        Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: vec![],
        }
        .resolve(&selection)
        .unwrap()
    }

    fn report(records: &[ResolvedMaterial]) -> Value {
        let instances: Vec<String> = (0..records.len()).map(|i| format!("piece-{i}")).collect();
        let mut material =
            json!({"basis":"per_piece_characterization","records":{},"instances":[]});
        for (r, instance) in records.iter().zip(&instances) {
            material["records"][r.sha256()] = json!({"record":r});
            material["instances"]
                .as_array_mut()
                .unwrap()
                .push(json!({"instance":instance,"record_sha256":r.sha256()}));
        }
        let owners: Vec<usize> = (0..records.len()).collect();
        let (records, assignment) = retained_materials(&material, &instances, &owners).unwrap();
        let mut positions = Vec::new();
        let mut patches = Vec::new();
        for (i, &column) in assignment.iter().enumerate() {
            let x = 2.0 * i as f64;
            positions.extend([
                Vec3::new(x, 0., 0.),
                Vec3::new(x + 1., 0., 0.),
                Vec3::new(x, 1., 0.),
            ]);
            patches.push(Patch {
                vertices: [3 * i, 3 * i + 1, 3 * i + 2],
                reference_area_m2: 0.5,
                column,
            });
        }
        let surface = Surface {
            records,
            patches,
            positions,
            instances,
            owners,
        };
        json!({"vapor_surface":surface.frame(&material).unwrap(),"material":material})
    }

    fn conditions() -> Conditions {
        Conditions {
            schema: "ftx-vapor-surface-conditions/1".into(),
            boundaries: vec![Boundary {
                inside_vapor_pressure_pa: 2000.,
                outside_vapor_pressure_pa: 1000.,
                inside_film_resistance_m2_pa_w: 1.,
                outside_film_resistance_m2_pa_w: 2.,
                latent_heat_j_kg: 2_500_000.,
            }],
            model_sha256: None,
            required_conditions: BTreeMap::new(),
            required_method: None,
            allow_mixed_protocols: false,
        }
    }

    fn close(a: f64, b: f64) {
        assert!(
            (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1e-10),
            "{a} != {b}"
        );
    }

    #[test]
    fn analytic_current_area_flux_and_frozen_replay_need_no_other_material_properties() {
        let source = report(&[record("vapor-only", 7., "synthetic Ret")]);
        let c = conditions();
        let value = c.evaluate(&source).unwrap();
        close(value["area_m2"].as_f64().unwrap(), 0.5);
        close(value["potential_heat_flow_w"].as_f64().unwrap(), 50.);
        close(
            value["potential_mass_flow_kg_s"].as_f64().unwrap(),
            50. / 2_500_000.,
        );
        assert_eq!(value["pieces"][0]["potential_heat_flow_w"], 50.);
        assert_eq!(value["columns"][0]["layers"][0]["pressure_drop_pa"], 700.);
        assert_eq!(value["columns"][0]["inside_film_pressure_drop_pa"], 100.);
        assert_eq!(value["columns"][0]["outside_film_pressure_drop_pa"], 200.);
        assert_eq!(value["dry_transport_required"], false);
        let request: Value =
            serde_json::from_str(value["replay_request_json"].as_str().unwrap()).unwrap();
        let replay: Conditions = serde_json::from_value(request["conditions"].clone()).unwrap();
        assert_eq!(replay.evaluate(&request["report"]).unwrap(), value);

        let mut surface = Surface::restore(&source).unwrap();
        surface.positions[1].x *= 2.;
        let moved = json!({"material":source["material"],"vapor_surface":surface.frame(&source["material"]).unwrap()});
        let stretched = c.evaluate(&moved).unwrap();
        assert_eq!(stretched["model_sha256"], value["model_sha256"]);
        assert_ne!(stretched["request_sha256"], value["request_sha256"]);
        assert_eq!(stretched["columns"], value["columns"]);
        close(stretched["potential_heat_flow_w"].as_f64().unwrap(), 100.);
        close(
            stretched["patch_topology"][0]["reference_area_m2"]
                .as_f64()
                .unwrap(),
            0.5,
        );
    }

    #[test]
    fn independent_sheets_sum_once_and_material_substitution_changes_potential() {
        let source = report(&[
            record("thin", 7., "synthetic Ret"),
            record("dense", 17., "synthetic Ret"),
        ]);
        let value = conditions().evaluate(&source).unwrap();
        close(value["area_m2"].as_f64().unwrap(), 1.);
        close(value["potential_heat_flow_w"].as_f64().unwrap(), 75.);
        close(
            value["pieces"][0]["potential_heat_flow_w"]
                .as_f64()
                .unwrap(),
            50.,
        );
        close(
            value["pieces"][1]["potential_heat_flow_w"]
                .as_f64()
                .unwrap(),
            25.,
        );
        let changed = report(&[
            record("thin", 17., "synthetic Ret"),
            record("dense", 17., "synthetic Ret"),
        ]);
        let replacement = conditions().evaluate(&changed).unwrap();
        assert_ne!(value["model_sha256"], replacement["model_sha256"]);
        close(replacement["potential_heat_flow_w"].as_f64().unwrap(), 50.);
        assert_eq!(replacement["columns"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn refuse_stale_material_geometry_source_policy_and_nonuniform_boundaries() {
        let source = report(&[
            record("a", 7., "synthetic Ret"),
            record("b", 17., "synthetic Ret"),
        ]);
        let mut c = conditions();
        let mut stale = source.clone();
        stale["vapor_surface"]["positions_m"][1][0] = json!(2.);
        assert!(c.evaluate(&stale).is_err());
        let mut stale = source.clone();
        // Columns follow canonical hash order, not the order of the input record slice.
        let column = stale["vapor_surface"]["model"]["patches"][0]["column"]
            .as_u64()
            .unwrap();
        stale["vapor_surface"]["model"]["patches"][0]["column"] =
            json!(if column == 0 { 1 } else { 0 });
        assert_ne!(stale["vapor_surface"], source["vapor_surface"]);
        assert_eq!(
            c.evaluate(&stale).unwrap_err().message,
            "vapor patch material differs from retained instance assignment"
        );
        let mut stale = source.clone();
        stale["vapor_surface"]["surprise"] = json!(true);
        assert!(c.evaluate(&stale).is_err());
        let mut stale = source.clone();
        let hash = stale["material"]["records"]
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        stale["material"]["records"][&hash]["record"]["allow_estimates"] = json!(false);
        assert!(c.evaluate(&stale).is_err());
        c.boundaries.push(c.boundaries[0].clone());
        assert!(c.evaluate(&source).is_err());
        c.model_sha256 = source["vapor_surface"]["model"]["sha256"]
            .as_str()
            .map(str::to_string);
        assert!(c.evaluate(&source).is_ok());
        c.model_sha256 = Some("0".repeat(64));
        assert!(c.evaluate(&source).is_err());
        c = conditions();
        c.required_conditions
            .insert("temperature_c".into(), "20".into());
        assert!(c.evaluate(&source).is_err());
        c = conditions();
        c.required_method = Some("a different protocol".into());
        assert!(c.evaluate(&source).is_err());
        let mixed = report(&[record("a", 7., "first"), record("b", 17., "second")]);
        assert!(conditions().evaluate(&mixed).is_err());
        c = conditions();
        c.allow_mixed_protocols = true;
        assert_eq!(c.evaluate(&mixed).unwrap()["mixed_protocols"], true);
    }

    #[test]
    fn zero_drive_is_zero_but_invalid_boundary_or_missing_ret_refuses() {
        let source = report(&[record("a", 7., "synthetic Ret")]);
        let mut c = conditions();
        c.boundaries[0].inside_vapor_pressure_pa = 1000.;
        let value = c.evaluate(&source).unwrap();
        assert_eq!(value["potential_heat_flow_w"], 0.);
        assert_eq!(value["potential_mass_flow_kg_s"], 0.);
        c.boundaries[0].inside_vapor_pressure_pa = 999.;
        assert!(c.evaluate(&source).is_err());
        c = conditions();
        c.boundaries[0].latent_heat_j_kg = 0.;
        assert!(c.evaluate(&source).is_err());
        c = conditions();
        c.boundaries[0].outside_film_resistance_m2_pa_w = 0.;
        assert!(c.evaluate(&source).is_err());
        let mut selection = record("missing", 7., "synthetic Ret").frozen_selection();
        selection.record.as_mut().unwrap().properties.clear();
        selection.expected_sha256 = None;
        let empty = Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: vec![],
        };
        let missing = report(&[empty.resolve(&selection).unwrap()]);
        assert!(Surface::restore(&missing).is_ok());
        assert!(
            conditions()
                .evaluate(&missing)
                .unwrap_err()
                .message
                .contains("missing textile property vapor_resistance")
        );
        let mut denied = record("denied", 7., "synthetic Ret").frozen_selection();
        let original_hash = denied.expected_sha256.clone().unwrap();
        denied.allow_estimates = false;
        // Consent participates in the frozen identity; an old lock cannot authorize a new policy.
        assert!(
            empty
                .resolve(&denied)
                .unwrap_err()
                .message
                .contains("expected_sha256")
        );
        denied.expected_sha256 = None;
        let denied = empty.resolve(&denied).unwrap();
        assert_ne!(denied.sha256(), original_hash);
        let denied = report(&[denied]);
        assert!(Surface::restore(&denied).is_ok());
        assert!(
            conditions()
                .evaluate(&denied)
                .unwrap_err()
                .message
                .contains("vapor_resistance is an estimate; explicit opt-in required")
        );
    }

    #[test]
    fn measured_sportswear_ret_is_consumed_without_fabric_mechanics_or_dry_storage() {
        let catalog = Catalog::builtin().unwrap();
        let cotton = catalog
            .resolve(&Selection {
                id: "zagreb/sportswear-2022/y1-sp".into(),
                ..Selection::default()
            })
            .unwrap();
        let polyamide = catalog
            .resolve(&Selection {
                id: "zagreb/sportswear-2022/y5-sp".into(),
                ..Selection::default()
            })
            .unwrap();
        let a = conditions().evaluate(&report(&[cotton])).unwrap();
        let b = conditions().evaluate(&report(&[polyamide])).unwrap();
        close(
            a["potential_heat_flow_w"].as_f64().unwrap(),
            500. / (3.7448 + 3.),
        );
        close(
            b["potential_heat_flow_w"].as_f64().unwrap(),
            500. / (0.6112 + 3.),
        );
        assert_eq!(
            a["columns"][0]["layers"][0]["observation"]["evidence"],
            "measured"
        );
        assert_eq!(
            a["columns"][0]["layers"][0]["observation"]["conditions"]["air_temperature_c"],
            "35"
        );
        assert_eq!(a["physical_validation_claim"], false);
    }

    #[test]
    fn frozen_constituent_receipts_do_not_become_vapor_sheet_characterization() {
        use ftx_pattern::materials::Form;
        let empty = Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: vec![],
        };
        for form in [Form::Fiber, Form::Yarn] {
            let mut selection = record("constituent", 7., "synthetic Ret").frozen_selection();
            selection.record.as_mut().unwrap().form = form;
            selection.expected_sha256 = None;
            let source = report(&[empty.resolve(&selection).unwrap()]);
            // A structurally valid canonical source/geometry frame is not transport admission.
            assert!(Surface::restore(&source).is_ok());
            let refusal = conditions().evaluate(&source).unwrap_err();
            assert!(refusal.message.contains("fiber/yarn"));
        }
    }
}
