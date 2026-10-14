//! Connect retained cloth elements and frozen material receipts, not catalogue lookups or viewer faces.
use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, Vec3, receipt::sha256_hex};
use ftx_pattern::material_catalog::{ResolvedMaterial, Selection};
use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog, Material};
use serde_json::{Value, json};

use super::{DryThermalModel, Patch, Surface, invalid};
use crate::{run::DrapeRun, scene::Scene};

fn usize_field(v: &Value, key: &str) -> Result<usize, Refusal> {
    v[key]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| invalid(format!("thermal receipt needs integer {key}")))
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], Refusal> {
    v[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid(format!("thermal receipt needs array {key}")))
}
fn digest(v: &Value) -> Result<String, Refusal> {
    Ok(sha256_hex(
        &serde_json::to_vec(v).map_err(|e| invalid(e.to_string()))?,
    ))
}

// The authored observations, normalized observations, policy AND hash must agree. No late catalogue
// refresh, relaxed estimate policy or substitution of a similarly named fabric can enter this path.
fn restore_material(v: &Value) -> Result<ResolvedMaterial, Refusal> {
    let authored: Material = serde_json::from_value(v["authored"].clone())
        .map_err(|e| invalid(format!("thermal material receipt: {e}")))?;
    let policy = v["allow_estimates"]
        .as_bool()
        .ok_or_else(|| invalid("missing material estimate policy"))?;
    let hash = v["sha256"]
        .as_str()
        .ok_or_else(|| invalid("missing material hash"))?;
    let empty = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let material = empty.resolve(&Selection {
        id: authored.id.clone(),
        record: Some(authored),
        allow_estimates: policy,
        expected_sha256: Some(hash.into()),
        ..Selection::default()
    })?;
    if serde_json::to_value(&material).map_err(|e| invalid(e.to_string()))? != *v {
        return Err(invalid(
            "material receipt differs from its frozen canonical inputs",
        ));
    }
    Ok(material)
}

impl Surface {
    /// Number of distinct columns; boundary arrays use this order, also recorded in the snapshot.
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Total temperature cells across retained triangles, for bounded integration schedules.
    #[must_use]
    pub fn cells(&self) -> usize {
        self.patches
            .iter()
            .map(|p| self.columns[p.column].cells())
            .sum()
    }

    /// Restore the exact immutable model without looking up any material by name.
    ///
    /// # Errors
    /// Missing or changed provenance, hash, policy, topology, discretization or derived coefficients.
    pub fn from_snapshot(v: &Value) -> Result<Self, Refusal> {
        let raw_patches = array(v, "patches")?;
        let raw_columns = array(v, "columns")?;
        let vertex_count = usize_field(v, "vertex_count")?;
        if raw_patches.is_empty()
            || raw_patches.len() > 65_536
            || raw_columns.is_empty()
            || raw_columns.len() > raw_patches.len()
            || vertex_count == 0
            || vertex_count > 196_608
        {
            return Err(invalid("thermal snapshot exceeds its topology budget"));
        }
        let mut patches = Vec::with_capacity(raw_patches.len());
        let mut cells = 0usize;
        let mut used = BTreeSet::new();
        for p in raw_patches {
            let column = usize_field(p, "column")?;
            used.insert(column);
            let raw = raw_columns
                .get(column)
                .ok_or_else(|| invalid("missing thermal column"))?;
            let count =
                usize_field(raw, "cells_per_layer")?.checked_mul(array(raw, "layers")?.len());
            cells = count
                .filter(|&n| n > 0 && n <= 4096)
                .and_then(|n| cells.checked_add(n))
                .filter(|&n| n <= 1_000_000)
                .ok_or_else(|| invalid("thermal snapshot exceeds cell budget"))?;
            patches.push(Patch {
                vertices: serde_json::from_value(p["vertices"].clone())
                    .map_err(|e| invalid(e.to_string()))?,
                reference_area_m2: p["reference_area_m2"]
                    .as_f64()
                    .ok_or_else(|| invalid("missing reference area"))?,
                column,
            });
        }
        if used.len() != raw_columns.len() {
            return Err(invalid("thermal snapshot contains unused columns"));
        }
        let mut columns = Vec::with_capacity(raw_columns.len());
        for raw in raw_columns {
            let layers = array(raw, "layers")?
                .iter()
                .map(restore_material)
                .collect::<Result<Vec<_>, _>>()?;
            let interfaces = serde_json::from_value(raw["interface_resistance_m2_k_w"].clone())
                .map_err(|e| invalid(e.to_string()))?;
            columns.push(DryThermalModel::new(
                layers,
                interfaces,
                usize_field(raw, "cells_per_layer")?,
            )?);
        }
        let surface = Self::new(columns, vertex_count, patches)?;
        if surface.snapshot() != *v {
            return Err(invalid(
                "thermal snapshot changed or contains unsupported fields",
            ));
        }
        Ok(surface)
    }

    /// Read the geometry/material exchange attached to a characterized drape report. Each sheet is
    /// evaluated independently: nearby lining/shell faces are NOT guessed to form a series stack.
    ///
    /// # Errors
    /// Thermal data unavailable, changed frame digest, invalid geometry or inconsistent model receipt.
    pub fn from_drape_report(report: &Value) -> Result<(Self, Vec<Vec3>), Refusal> {
        let frame = &report["thermal_surface"];
        if frame["available"] != true || frame["schema"] != "ftx-drape-thermal-surface/1" {
            return Err(invalid(format!(
                "drape thermal surface unavailable: {}",
                frame["refusal"]
            )));
        }
        let hash = frame["sha256"]
            .as_str()
            .ok_or_else(|| invalid("missing thermal frame hash"))?;
        let mut inputs = frame.clone();
        inputs
            .as_object_mut()
            .ok_or_else(|| invalid("thermal frame must be an object"))?
            .remove("sha256");
        if digest(&inputs)? != hash {
            return Err(invalid("thermal frame hash mismatch"));
        }
        if frame["material_report_sha256"] != digest(&report["material"])? {
            return Err(invalid(
                "thermal exchange belongs to a different material report",
            ));
        }
        let model = Self::from_snapshot(&frame["model"])?;
        let x: Vec<Vec3> = serde_json::from_value(frame["positions_m"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        model.validate_positions(&x)?;
        if array(frame, "triangle_instance")?.len() != model.patches.len() {
            return Err(invalid(
                "thermal frame triangle owners do not match topology",
            ));
        }
        let instances = array(frame, "instances")?;
        for owner in array(frame, "triangle_instance")? {
            if owner
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .is_none_or(|n| n >= instances.len())
            {
                return Err(invalid("thermal frame has an invalid instance owner"));
            }
        }
        Ok((model, x))
    }

    fn validate_positions(&self, x: &[Vec3]) -> Result<(), Refusal> {
        if x.len() != self.vertex_count || x.iter().any(|p| !p.is_finite()) {
            return Err(invalid("thermal frame needs every finite vertex in metres"));
        }
        for p in &self.patches {
            let [a, b, c] = p.vertices.map(|i| x[i]);
            super::positive("thermal triangle area", 0.5 * (b - a).cross(c - a).norm())?;
        }
        Ok(())
    }

    fn from_scene(
        scene: &Scene,
        material: &Value,
        cells_per_layer: usize,
    ) -> Result<Self, Refusal> {
        let faces = &scene.solver.model.membranes;
        if faces.len() != scene.triangle_owner.len()
            || faces.len() > 65_536
            || cells_per_layer == 0
            || cells_per_layer > 4096
            || faces
                .len()
                .checked_mul(cells_per_layer)
                .is_none_or(|n| n > 1_000_000)
        {
            return Err(invalid(
                "thermal topology differs from retained cloth elements or exceeds budget",
            ));
        }
        let mut records = BTreeMap::new();
        let instance_hashes: Vec<String> = match material["basis"].as_str() {
            Some("characterized_record") => {
                let hash = material["record_sha256"]
                    .as_str()
                    .ok_or_else(|| invalid("missing material identity"))?;
                records.insert(hash.to_string(), &material["record"]);
                vec![hash.into(); scene.mesh.instances.len()]
            }
            Some("per_piece_characterization") => {
                let receipts = array(material, "instances")?;
                if receipts.len() != scene.mesh.instances.len() {
                    return Err(invalid("missing instance material receipts"));
                }
                let mut hashes = Vec::with_capacity(receipts.len());
                for (r, id) in receipts.iter().zip(&scene.mesh.instances) {
                    if r["instance"] != *id {
                        return Err(invalid("instance material order differs from cloth"));
                    }
                    let hash = r["record_sha256"]
                        .as_str()
                        .ok_or_else(|| invalid("missing instance material identity"))?;
                    records.insert(hash.to_string(), &material["records"][hash]["record"]);
                    hashes.push(hash.into());
                }
                hashes
            }
            _ => {
                return Err(invalid(
                    "thermal coupling needs characterized material records, not legacy defaults",
                ));
            }
        };
        let mut used = BTreeSet::new();
        for &owner in &scene.triangle_owner {
            used.insert(
                instance_hashes
                    .get(owner)
                    .ok_or_else(|| invalid("invalid retained triangle owner"))?
                    .clone(),
            );
        }
        let mut ids = BTreeMap::new();
        let mut columns = Vec::new();
        for hash in used {
            let record = restore_material(records[&hash])?;
            if record.sha256() != hash {
                return Err(invalid("instance refers to a different material receipt"));
            }
            ids.insert(hash, columns.len());
            columns.push(DryThermalModel::new(vec![record], vec![], cells_per_layer)?);
        }
        let patches = faces
            .iter()
            .zip(&scene.triangle_owner)
            .map(|(e, &owner)| Patch {
                vertices: e.v,
                reference_area_m2: e.area,
                column: ids[&instance_hashes[owner]],
            })
            .collect();
        let surface = Self::new(columns, scene.solver.x.len(), patches)?;
        surface.validate_positions(&scene.solver.x)?;
        Ok(surface)
    }
}

impl DrapeRun {
    /// Compile thermal columns from the same frozen materials and retained elements as this run.
    /// No new lookup, guessed thermal properties or inference of overlapping garment layers occurs.
    ///
    /// # Errors
    /// Incomplete thermal data, inconsistent scene/receipts, or unsupported topology/discretization.
    pub fn thermal_surface(&self, cells_per_layer: usize) -> Result<Surface, Refusal> {
        Surface::from_scene(&self.scene, &self.report["material"], cells_per_layer)
    }
}

pub(crate) fn for_drape(scene: &Scene, material: &Value) -> Value {
    let outcome = (|| {
        let surface = Surface::from_scene(scene, material, 4)?;
        let mut frame = json!({"schema":"ftx-drape-thermal-surface/1","available":true,
            "model":surface.snapshot(),"positions_m":scene.solver.x,
            "material_report_sha256":digest(material)?,
            "triangle_instance":scene.triangle_owner,"instances":scene.mesh.instances,
            "scope":"independent sheet exchange; not inferred body coverage or whole-wearer heat loss"});
        frame["sha256"] = json!(digest(&frame)?);
        Ok::<_, Refusal>(frame)
    })();
    outcome.unwrap_or_else(|error| json!({"available":false,"refusal":error}))
}
