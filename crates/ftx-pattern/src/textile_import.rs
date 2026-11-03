//! Compatibility import of the initial `ftx-textiles/1` observations into the shared material model.
//! All selections, evidence policy, normalization and physics then use `materials`/`material_catalog`.
//! The original record is retained verbatim as JSON in the imported structure, not silently replaced.

use std::collections::BTreeMap;

use ftx_core::textile as legacy;
use ftx_core::{Refusal, RefusalCode};
use serde_json::Value;

use crate::Pattern;
use crate::material_catalog::{ResolvedMaterial, Selection};
use crate::materials::{
    CATALOG_SCHEMA, Catalog, Component, Evidence, Form, Material, Quantity, Source,
};

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
            let has_selector =
                object.contains_key("material") || object.contains_key("material_id");
            if !has_selector
                || object
                    .keys()
                    .any(|k| k != "material" && k != "material_id" && k != "role")
            {
                return Err(invalid(
                    "piece.fabric requires a complete material OR material_id selection, not partial coefficients/layers",
                ));
            }
            if let Some(role) = object.get("role") {
                match role.as_str() {
                    Some("shell" | "lining" | "laminate") => {}
                    Some("fill" | "loose_fill") => {
                        return Err(invalid(
                            "loose fill cannot be assigned as a load-bearing shell piece",
                        ));
                    }
                    _ => {
                        return Err(invalid(
                            "piece.fabric.role must be \"shell\", \"lining\", or \"laminate\"",
                        ));
                    }
                }
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
        membrane_relaxation: None,
        thermal_response: None,
        optical_faces: None,
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
/// Embedded records are self-contained: they use the canonical resolver without loading unrelated
/// bundled records. Sources, overrides, units, evidence policy and snapshot locks are still checked.
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
    let available = if selection.record.is_some() {
        Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: Vec::new(),
        }
    } else {
        catalog()?
    };
    available.resolve(&selection).map(Some)
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

    #[test]
    fn embedded_records_preserve_canonical_overrides_units_policy_and_frozen_replay() {
        let mut c = catalog().unwrap();
        let record: Material = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-nonlinear.json"
        ))
        .unwrap();
        assert!(c.get(&record.id).is_err());
        let mut selected = Selection {
            id: record.id.clone(),
            record: Some(record),
            allow_estimates: true,
            ..Selection::default()
        };
        selected.sources.insert(
            "replacement".into(),
            Source {
                reference: "test:frozen-record-replacement".into(),
                accessed_on: "2026-11-03".into(),
                note: "Synthetic replacement, not a textile measurement".into(),
            },
        );
        selected.overrides.insert(
            "thickness".into(),
            Quantity {
                value: 1.25,
                unit: "mm".into(),
                source: "replacement".into(),
                evidence: Evidence::Estimate,
                method: "Synthetic frozen-replay regression".into(),
                conditions: BTreeMap::from([("scope".into(), "software test".into())]),
                bounds: Some([1.0, 1.5]),
            },
        );
        let expected = c.resolve(&selected).unwrap();
        let actual = resolve_fabric(&json!({"material":selected}))
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(actual.mechanics().unwrap().thickness_m, 0.00125);
        assert_eq!(actual.authored().properties["thickness"].unit, "mm");
        assert_eq!(
            actual.material().properties["thickness"].bounds,
            Some([0.001, 0.0015])
        );
        let frozen = actual.frozen_selection();
        let replay = resolve_fabric(&json!({"material":frozen}))
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(&replay).unwrap()
        );

        selected.allow_estimates = false;
        let unadmitted = resolve_fabric(&json!({"material":selected}))
            .unwrap()
            .unwrap();
        assert!(!unadmitted.allows_estimates());
        assert!(unadmitted.mechanics().is_err());
        assert_ne!(unadmitted.sha256(), actual.sha256());
        // Catalog callers still validate their complete supplied collection, including when
        // the selected record is embedded. Only the pattern's self-contained ingress skips it.
        c.schema = "invalid".into();
        assert!(c.resolve(&frozen).is_err());
    }

    #[test]
    fn malformed_embedded_records_refuse_without_falling_back_to_the_named_catalog_entry() {
        let c = catalog().unwrap();
        let named = c
            .resolve(&Selection {
                id: "fs_il019_natural".into(),
                ..Selection::default()
            })
            .unwrap()
            .frozen_selection();
        for mutation in 0..6 {
            let mut selected = named.clone();
            selected.expected_sha256 = None;
            let record = selected.record.as_mut().unwrap();
            match mutation {
                0 => record.id = "different-specimen".into(),
                1 => record.properties.get_mut("areal_density").unwrap().unit = "Pa".into(),
                2 => selected.expected_sha256 = Some("0".repeat(64)),
                3 => {
                    let key = record.properties["areal_density"].source.clone();
                    let mut source = record.sources[&key].clone();
                    source.reference = "test:rebound-source".into();
                    selected.sources.insert(key, source);
                }
                4 => {
                    record.properties.get_mut("areal_density").unwrap().source = "missing".into();
                }
                _ => record.name.clear(),
            }
            let expected = c.resolve(&selected).unwrap_err();
            let actual = resolve_fabric(&json!({"material":selected})).unwrap_err();
            assert_eq!(actual, expected, "mutation {mutation}");
        }
        assert!(resolve_fabric(&json!({"material_id":"unknown-specimen"})).is_err());
    }
}
