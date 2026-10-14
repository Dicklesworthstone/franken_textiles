//! Compatibility import of the initial `ftx-textiles/1` observations into the shared material model.
//! All selections, evidence policy, normalization and physics then use `materials`/`material_catalog`.
//! The original record is retained verbatim as JSON in the imported structure, not silently replaced.

use std::collections::BTreeMap;

use ftx_core::textile as legacy;
use ftx_core::{Refusal, RefusalCode};
use serde_json::Value;

use crate::Pattern;
use crate::material_catalog::{ResolvedMaterial, Selection};
use crate::materials::{Catalog, Component, Evidence, Form, Material, Quantity, Source};

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// The source-piece selection shared by simulation, cutting and purchasing. An absent piece
/// selection inherits the global fabric; a present one must be complete. Informational cutting
/// labels do not select physics or supplier goods. This does not require mechanical properties.
///
/// # Errors
/// Missing piece, ambiguous labels/selectors or partial per-piece overrides/layer declarations.
pub fn piece_fabric(pattern: &Pattern, index: usize) -> Result<&Value, Refusal> {
    let raw = pattern.raw();
    let piece = raw["pieces"]
        .as_array()
        .and_then(|p| p.get(index))
        .ok_or_else(|| invalid(format!("no source piece {index}")))?;
    if piece.get("material").is_some_and(|m| !m.is_string()) || piece.get("material_id").is_some() {
        return Err(invalid(
            "piece.material is a cutting label; use piece.fabric.material",
        ));
    }
    match piece.get("fabric") {
        None => Ok(&raw["fabric"]),
        Some(f) => {
            let object = f
                .as_object()
                .ok_or_else(|| invalid("piece.fabric must be an object"))?;
            if object.is_empty() || object.keys().any(|k| k != "material" && k != "material_id") {
                return Err(invalid(
                    "piece.fabric requires a complete material OR material_id selection, not partial coefficients/layers",
                ));
            }
            Ok(f)
        }
    }
}

/// Imports an initial catalog record without promoting assumed/estimated observations to evidence.
///
/// # Errors
/// Invalid source records, reserved import metadata, conflicting property aliases or conversion error.
pub fn import(record: &legacy::Textile) -> Result<Material, Refusal> {
    record.validate()?;
    let mut structure = record.construction.clone();
    for (key, value) in [
        ("ftx_import_schema", legacy::CATALOG_SCHEMA.to_string()),
        (
            "ftx_import_original",
            serde_json::to_string(record).map_err(|e| invalid(e.to_string()))?,
        ),
    ] {
        if structure.insert(key.into(), value).is_some() {
            return Err(invalid("construction uses reserved ftx_import metadata"));
        }
    }
    let mut properties = BTreeMap::new();
    for (key, p) in &record.properties {
        let name = match key.as_str() {
            "areal_mass" => "areal_density",
            "warp_count" => "thread_density_warp",
            "weft_count" => "thread_density_weft",
            other => other,
        };
        // Canonical normalization understands 1/in. lb/yd2 is an ingress-only legacy unit; retain
        // the original observation above and explicitly record its exact SI conversion here.
        let (value, unit, bounds, conversion) = if name == "areal_density" && p.unit == "lb/yd2" {
            let scale = 0.453_592_37 / (0.9144 * 0.9144);
            (
                p.value * scale,
                "kg/m2",
                p.range.map(|b| b.map(|x| x * scale)),
                "; converted lb/yd2 using 0.45359237 kg/lb and 0.9144 m/yd",
            )
        } else {
            let unit = match p.unit.as_str() {
                "threads/in" => "1/in",
                other => other,
            };
            (p.value, unit, p.range, "")
        };
        let q = Quantity {
            value,
            unit: unit.into(),
            source: p.source.clone(),
            evidence: match p.evidence {
                legacy::Evidence::Manufacturer => Evidence::Published,
                legacy::Evidence::Measured => Evidence::Measured,
                legacy::Evidence::Estimated | legacy::Evidence::Assumed => Evidence::Estimate,
            },
            method: format!("Imported {:?} observation{conversion}", p.evidence),
            conditions: BTreeMap::from([("original_conditions".into(), p.conditions.clone())]),
            bounds,
        };
        if properties.insert(name.into(), q).is_some() {
            return Err(invalid(format!("conflicting imported property {name}")));
        }
    }
    let out = Material {
        id: record.id.clone(),
        name: record.name.clone(),
        form: match record.form {
            legacy::Form::Fiber => Form::Fiber,
            legacy::Form::Yarn => Form::Yarn,
            legacy::Form::Woven => Form::Woven,
            legacy::Form::Knit => Form::Knit,
            legacy::Form::Nonwoven => Form::Nonwoven,
            legacy::Form::Leather => Form::Hide,
            legacy::Form::Fill => Form::Fill,
            legacy::Form::Laminate => Form::Laminate,
        },
        composition: record
            .composition
            .iter()
            .filter(|(_, fraction)| **fraction > 0.0)
            .map(|(fiber, fraction)| Component {
                fiber: fiber.clone(),
                fraction: *fraction,
                source: record.identity_source.clone(),
            })
            .collect(),
        composition_complete: !record.composition.is_empty(),
        structure,
        properties,
        membrane_response: None,
        sources: record
            .sources
            .iter()
            .map(|(key, s)| {
                (
                    key.clone(),
                    Source {
                        reference: s.locator.clone(),
                        accessed_on: s.accessed.clone(),
                        note: s.title.clone(),
                    },
                )
            })
            .collect(),
    };
    out.validate()?;
    Ok(out)
}

/// One discovery view of both source tranches, using the existing canonical catalog and evaluator.
/// Product/variant IDs are retained; similar names never cause implicit deduplication/substitution.
///
/// # Errors
/// Invalid bundled observations or conflicting identifiers between source tranches.
pub fn catalog() -> Result<Catalog, Refusal> {
    let mut catalog = Catalog::builtin()?;
    for record in legacy::Catalog::builtin()?.materials {
        catalog.materials.push(import(&record)?);
    }
    catalog.validate()?;
    catalog.materials.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(catalog)
}

/// Resolves one pattern's shared `fabric.material` selection, or the `material_id` convenience form.
/// The latter cannot consent to estimates; use the full selection to choose its evidence policy.
/// Legacy physical values cannot coexist with a selected material.
///
/// # Errors
/// Malformed or conflicting selectors, unknown IDs, invalid evidence or failed snapshot locks.
pub fn resolve_fabric(fabric: &Value) -> Result<Option<ResolvedMaterial>, Refusal> {
    let id = fabric.get("material_id");
    let selection = fabric.get("material");
    if id.is_none() && selection.is_none() {
        return Ok(None);
    }
    if id.is_some() && selection.is_some() {
        return Err(invalid("choose material_id OR material, not both"));
    }
    for key in ["areal_g_m2", "tensile_N_m", "bending_Nm"] {
        if fabric.get(key).is_some() {
            return Err(invalid(format!(
                "{key} conflicts with the selected material"
            )));
        }
    }
    let selection = match selection {
        Some(value) => serde_json::from_value::<Selection>(value.clone())
            .map_err(|e| invalid(format!("fabric.material selection: {e}")))?,
        None => Selection {
            id: id
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("material_id must be a string"))?
                .into(),
            ..Selection::default()
        },
    };
    catalog()?.resolve(&selection).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn both_source_tranches_use_the_same_units_and_evidence_policy() {
        let c = catalog().unwrap();
        assert!(c.get("fs/il019-dusty-lotus-softened").is_ok());
        for record in legacy::Catalog::builtin().unwrap().materials {
            let m = c
                .resolve(&Selection {
                    id: record.id.clone(),
                    ..Selection::default()
                })
                .unwrap();
            assert!(m.mechanics().is_err());
            let original: legacy::Textile =
                serde_json::from_str(&m.authored().structure["ftx_import_original"]).unwrap();
            assert_eq!(original, record);
        }
        let f = c
            .resolve(&Selection {
                id: "felt_f51_3_32".into(),
                ..Selection::default()
            })
            .unwrap();
        let expected = 1.3 * 0.453_592_37 / (0.9144 * 0.9144);
        assert!(
            (f.material()
                .require("areal_density", "kg/m2", false)
                .unwrap()
                - expected)
                .abs()
                < 1e-15
        );
        let l = c
            .resolve(&Selection {
                id: "tandy_cow_garment_9114_71".into(),
                ..Selection::default()
            })
            .unwrap();
        assert!(l.material().require("thickness", "m", false).is_err());
        assert_eq!(
            l.material().require("thickness", "m", true).unwrap(),
            0.0007
        );
    }

    #[test]
    fn import_refuses_alias_collisions_and_preserves_assumed_evidence() {
        let mut m = legacy::Catalog::builtin()
            .unwrap()
            .get("fs_il019_natural")
            .unwrap()
            .clone();
        m.properties.get_mut("areal_mass").unwrap().evidence = legacy::Evidence::Assumed;
        assert_eq!(
            import(&m).unwrap().properties["areal_density"].evidence,
            Evidence::Estimate
        );
        m.properties
            .insert("areal_density".into(), m.properties["areal_mass"].clone());
        assert!(import(&m).is_err());
    }

    #[test]
    fn selectors_reuse_canonical_locks_and_reject_ambiguity() {
        let m = resolve_fabric(&json!({"material_id":"fs_il019_natural"}))
            .unwrap()
            .unwrap();
        let frozen = m.frozen_selection();
        assert_eq!(
            resolve_fabric(&json!({"material":frozen}))
                .unwrap()
                .unwrap()
                .sha256(),
            m.sha256()
        );
        let mut broken = json!({"material":frozen});
        broken["material"]["record"]["name"] = json!("Changed");
        assert!(resolve_fabric(&broken).is_err());
        assert!(
            resolve_fabric(&json!({"material_id":"fs_il019_natural", "bending_Nm":1})).is_err()
        );
        assert!(resolve_fabric(&json!({"material":{}, "material_id":"fs_il019_natural"})).is_err());
    }
}
