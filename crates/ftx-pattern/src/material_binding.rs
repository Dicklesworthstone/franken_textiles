//! Atomic source-piece and explicit interface substitutions. No geometry, role or friction is guessed.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::material_catalog::Selection;
use crate::material_friction::Interface;
use crate::{Pattern, textile_import};

/// Explicit edits against the original pattern; omitted pieces retain their existing selections.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Changes {
    /// Must be `ftx-material-bindings/1`.
    pub schema: String,
    /// Replace the pattern default, without replacing explicitly assigned pieces.
    #[serde(default)]
    pub default: Option<Selection>,
    /// Exact source-piece IDs (not instance IDs or cutting labels) to replace.
    #[serde(default)]
    pub pieces: BTreeMap<String, Selection>,
    /// Explicit complete replacement for fabric.interfaces. Omitted/null preserves the original;
    /// an empty list clears it. Entries use the existing Interface contract but retain their raw
    /// number representation, order and omitted defaults for exact source replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interfaces: Option<Vec<Value>>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn hash(value: &Value) -> Result<String, Refusal> {
    Ok(sha256_hex(
        &serde_json::to_vec(value).map_err(|e| invalid(e.to_string()))?,
    ))
}

fn put(fabric: &mut Value, selection: &Selection) -> Result<(), Refusal> {
    let object = fabric
        .as_object_mut()
        .ok_or_else(|| invalid("fabric must be an object"))?;
    // Only the explicitly replaced physical coefficients/selectors change. Keep interface evidence,
    // role and all other authored metadata; downstream consumers must still validate their scope.
    for key in ["areal_g_m2", "tensile_N_m", "bending_Nm", "material_id"] {
        object.remove(key);
    }
    object.insert(
        "material".into(),
        serde_json::to_value(selection).map_err(|e| invalid(e.to_string()))?,
    );
    Ok(())
}

fn validate_interfaces(pairs: &[Value], assigned: &BTreeSet<String>) -> Result<(), Refusal> {
    let mut seen = BTreeSet::new();
    for raw in pairs {
        let interface = Interface::parse(raw)?;
        interface.validate_observation()?;
        let mut materials = interface.materials;
        for hash in &materials {
            if !assigned.contains(hash) {
                return Err(invalid(format!(
                    "interface names unassigned or stale material snapshot {hash}"
                )));
            }
        }
        materials.sort();
        if materials[0] == materials[1] || !seen.insert(materials) {
            return Err(invalid(
                "duplicate or same-sheet interface; diagonals belong to the sheet record",
            ));
        }
    }
    Ok(())
}

impl Changes {
    /// Apply named changes and freeze EVERY effective source selection, including untouched pieces.
    /// Missing mechanical data does not prevent choosing a material for a supported other consumer.
    /// A uniform change never erases a pre-existing lining/cuff selection. Pair evidence is preserved
    /// unless its complete replacement is supplied explicitly; hashes and coefficients are not inferred.
    ///
    /// # Errors
    /// Invalid schema/pattern, ambiguous or unknown piece IDs, invalid units/evidence/digest locks,
    /// missing effective selections, stale/duplicate/malformed supplied interfaces or exceeded
    /// source-piece/interface budgets. Estimate admission and pair coverage remain consumer checks.
    /// Inputs are never mutated.
    pub fn apply(&self, pattern: &Pattern) -> Result<Value, Refusal> {
        if self.schema != "ftx-material-bindings/1"
            || self.pieces.len() > 4096
            // Existing contact authority supports at most 64 distinct materials / 2016 pairs.
            || self.interfaces.as_ref().is_some_and(|v| v.len() > 2016)
        {
            return Err(invalid(
                "unsupported material binding schema or piece/interface budget",
            ));
        }
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(invalid(errors.join("; ")));
        }
        let source = pattern.raw();
        let pieces = source["pieces"]
            .as_array()
            .ok_or_else(|| invalid("pattern needs source pieces"))?;
        if pieces.is_empty() || pieces.len() > 4096 {
            return Err(invalid("material binding needs 1..4096 source pieces"));
        }
        let mut ids = BTreeSet::new();
        for p in pieces {
            let id = p["id"]
                .as_str()
                .ok_or_else(|| invalid("piece ID must be text"))?;
            if !ids.insert(id) {
                return Err(invalid("ambiguous repeated source piece ID"));
            }
        }
        for id in self.pieces.keys() {
            if !ids.contains(id.as_str()) {
                return Err(invalid(format!("unknown source piece {id}")));
            }
        }
        let catalog = textile_import::catalog()?;
        let mut raw = source.clone();
        if let Some(selection) = &self.default {
            let frozen = catalog.resolve(selection)?.frozen_selection();
            let fabric = raw
                .as_object_mut()
                .unwrap()
                .entry("fabric")
                .or_insert(json!({}));
            put(fabric, &frozen)?;
        }
        for (id, selection) in &self.pieces {
            let frozen = catalog.resolve(selection)?.frozen_selection();
            let piece = raw["pieces"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|p| p["id"] == *id)
                .unwrap();
            let fabric = piece
                .as_object_mut()
                .unwrap()
                .entry("fabric")
                .or_insert(json!({}));
            put(fabric, &frozen)?;
        }
        let default = textile_import::resolve_fabric(&raw["fabric"])?
            .ok_or_else(|| invalid("choose a source-bearing default material"))?;
        put(&mut raw["fabric"], &default.frozen_selection())?;
        let candidate = Pattern::new(raw.clone());
        let mut assignments = Vec::new();
        let mut material_hashes = BTreeSet::new();
        for (i, original) in pieces.iter().enumerate() {
            let selected =
                textile_import::resolve_fabric(textile_import::piece_fabric(&candidate, i)?)?
                    .ok_or_else(|| invalid("every effective piece needs a material selection"))?;
            if raw["pieces"][i].get("fabric").is_some() {
                put(
                    &mut raw["pieces"][i]["fabric"],
                    &selected.frozen_selection(),
                )?;
            }
            assignments.push(json!({"piece_id":original["id"], "material_id":selected.material().id,
                "material_sha256":selected.sha256(), "inherited_default":raw["pieces"][i].get("fabric").is_none()}));
            material_hashes.insert(selected.sha256().to_owned());
        }
        if let Some(interfaces) = &self.interfaces {
            validate_interfaces(interfaces, &material_hashes)?;
            raw["fabric"]["interfaces"] = Value::Array(interfaces.clone());
        }
        // Retain Rust's numerical JSON representation through JavaScript. Parsing/stringifying a
        // geometry coordinate such as -0.0 or 1.0 in JS can change a source-bound report's identity.
        let pattern_json = serde_json::to_string(&raw).map_err(|e| invalid(e.to_string()))?;
        Ok(
            json!({"schema":"ftx-bound-material-pattern/1", "source_pattern_sha256":hash(source)?,
            "pattern_sha256":hash(&raw)?, "pattern":raw, "pattern_json":pattern_json,"pieces":assignments,
            "geometry_changed":false, "physical_validation_claim":false,
            "note":if self.interfaces.is_some() {
                "Only fabric selections and the explicitly supplied complete interface table changed. Geometry and roles were not rewritten; each consumer must validate its own inputs."
            } else {
                "Only fabric selections changed. Geometry, roles and pair-friction evidence were not rewritten; each consumer must validate its own inputs."
            }}),
        )
    }
}
