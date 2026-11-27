//! Immutable whole-fabric and per-piece overrides shared by native garment workflows.
//! The source file is read once, validated and frozen before any workspace/output publication.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_drape::material::Fabric;
use ftx_pattern::{Pattern, material_binding::Changes, textile_import::resolve_fabric};
use serde_json::{Value, json};

use crate::{Args, bad_input, workspace};

pub(super) const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

/// Serialize an effective overridden pattern and enforce the shared 64 MiB replay snapshot budget.
pub(super) fn snapshot_bytes(pattern: &Value, context: &str) -> Result<Vec<u8>, Refusal> {
    snapshot_bytes_with_limit(pattern, context, MAX_SNAPSHOT_BYTES)
}

fn snapshot_bytes_with_limit(
    pattern: &Value,
    context: &str,
    limit: usize,
) -> Result<Vec<u8>, Refusal> {
    let bytes = serde_json::to_vec(pattern).map_err(|e| bad_input(e.to_string()))?;
    if bytes.len() > limit {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("{context} exceeds the 64 MiB replay budget"),
        ));
    }
    Ok(bytes)
}

enum Selection {
    Fabric(Value),
    Bindings(Box<Changes>),
}

pub(super) struct Input {
    selection: Selection,
    frozen: Override,
}

impl Input {
    pub(super) fn apply(mut self, pattern: &mut Value) -> Result<Override, Refusal> {
        match self.selection {
            Selection::Fabric(fabric) => {
                let object = pattern
                    .as_object_mut()
                    .ok_or_else(|| bad_input("pattern must be an object"))?;
                object.insert("fabric".into(), fabric);
                // Whole-fabric replacement must not leave stale per-piece fabric overrides merged
                // with a second physical law.
                let mut cleared = Vec::new();
                if let Some(pieces) = object.get_mut("pieces").and_then(Value::as_array_mut) {
                    for (idx, piece) in pieces.iter_mut().enumerate() {
                        if let Some(piece_obj) = piece.as_object_mut()
                            && let Some(old) = piece_obj.remove("fabric")
                            && !old.is_null()
                        {
                            let id = piece_obj
                                .get("id")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .unwrap_or_else(|| format!("piece[{idx}]"));
                            cleared.push(id);
                        }
                    }
                }
                if !cleared.is_empty() {
                    self.frozen.receipt["cleared_piece_fabric_overrides"] = json!(cleared);
                }
            }
            Selection::Bindings(changes) => {
                let mut result = changes.apply(&Pattern::new(pattern.clone()))?;
                // A later round may infer placement. Keep the hash of this binding stage.
                let receipt = &mut self.frozen.receipt;
                receipt["source_pattern_sha256"] = result["source_pattern_sha256"].take();
                receipt["bound_pattern_sha256"] = result["pattern_sha256"].take();
                receipt["pieces"] = result["pieces"].take();
                if let Some(bands) = result.get_mut("frozen_elastic_casings") {
                    receipt["frozen_elastic_casings"] = bands.take();
                }
                *pattern = result["pattern"].take();
            }
        }
        Ok(self.frozen)
    }
}

pub(super) struct Override {
    bytes: Vec<u8>,
    receipt: Value,
}

impl Override {
    pub(super) fn receipt(&self) -> &Value {
        &self.receipt
    }

    pub(super) fn entries(&self) -> Result<[(&'static str, Vec<u8>); 2], Refusal> {
        Ok([
            ("fabric.source.json", self.bytes.clone()),
            (
                "fabric_override.json",
                workspace::json_bytes(&self.receipt)?,
            ),
        ])
    }

    pub(super) fn save(&self, dir: &Path) -> Result<(), Refusal> {
        for (name, bytes) in self.entries()? {
            workspace::write_new(&dir.join(name), &bytes)?;
        }
        Ok(())
    }
}

pub(super) fn load(args: &Args) -> Result<Option<Input>, Refusal> {
    let (path, limit, bindings) = match (
        args.options.get("fabric"),
        args.options.get("material-bindings"),
    ) {
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => return Err(bad_input("choose --fabric or --material-bindings")),
        (Some(path), None) => (path, 1024 * 1024, false),
        (None, Some(path)) => (path, 64 * 1024 * 1024, true),
    };
    let budget = || {
        Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "material input exceeds the {} MiB input budget",
                limit / (1024 * 1024)
            ),
        )
    };
    let io = |e| Refusal::new(RefusalCode::Io, format!("{path}: {e}"));
    let meta = std::fs::metadata(path).map_err(io)?;
    if !meta.is_file() {
        return Err(bad_input("material input requires a regular JSON file"));
    }
    if meta.len() > limit {
        return Err(budget());
    }
    let file = File::open(path).map_err(io)?;
    let metadata = file.metadata().map_err(io)?;
    if !metadata.is_file() {
        return Err(bad_input("material input requires a regular JSON file"));
    }
    if metadata.len() > limit {
        return Err(budget());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 > limit {
        return Err(budget());
    }
    if bindings {
        let changes: Changes = serde_json::from_slice(&bytes)
            .map_err(|e| bad_input(format!("material bindings: {e}")))?;
        let receipt = json!({
            "source": path, "source_sha256": sha256_hex(&bytes),
            "operation": "apply_material_bindings", "basis": "source_bearing_snapshot"
        });
        return Ok(Some(Input {
            selection: Selection::Bindings(Box::new(changes)),
            frozen: Override { bytes, receipt },
        }));
    }
    let mut fabric: Value =
        serde_json::from_slice(&bytes).map_err(|e| bad_input(format!("fabric override: {e}")))?;
    let object = fabric
        .as_object()
        .ok_or_else(|| bad_input("--fabric needs a fabric object, not a pattern or catalog"))?;
    for key in object.keys() {
        if ![
            "material",
            "material_id",
            "areal_g_m2",
            "tensile_N_m",
            "bending_Nm",
            "id",
            "name",
            "kind",
            "width_mm",
            "sim_note",
            "notes",
        ]
        .contains(&key.as_str())
        {
            return Err(bad_input(format!(
                "unknown fabric override field '{key}'; use fabric.material for source-bearing directional properties"
            )));
        }
    }
    if let Some(width) = object.get("width_mm")
        && width
            .as_f64()
            .is_none_or(|value| !value.is_finite() || value <= 0.0)
    {
        return Err(bad_input("fabric.width_mm must be finite and positive"));
    }
    for key in ["id", "name", "kind", "sim_note"] {
        if object.get(key).is_some_and(|value| !value.is_string()) {
            return Err(bad_input(format!("fabric.{key} must be a string")));
        }
    }

    let (basis, material_sha256) = if let Some(record) = resolve_fabric(&fabric)? {
        // Freeze source identity here. The requested drape/export consumer admits only its own
        // required capabilities; a flat draft does not need a complete cloth/contact law.
        let frozen = serde_json::to_value(record.frozen_selection())
            .map_err(|e| bad_input(e.to_string()))?;
        let object = fabric
            .as_object_mut()
            .expect("fabric was checked as object");
        object.remove("material_id");
        object
            .entry("name")
            .or_insert_with(|| json!(record.authored().name));
        object.insert("material".into(), frozen);
        ("source_bearing_snapshot", Some(record.sha256().to_string()))
    } else {
        Fabric::of(&Pattern::new(json!({"fabric":fabric.clone()})))?;
        ("legacy_uncertain", None)
    };

    let receipt = json!({
        "source": path,
        "source_sha256": sha256_hex(&bytes),
        "basis": basis,
        "material_sha256": material_sha256,
        "effective_fabric": fabric,
        "operation": "replace_whole_fabric",
        "note": "No geometry, fit contract or measurement evidence is changed by a fabric substitution."
    });
    Ok(Some(Input {
        selection: Selection::Fabric(fabric),
        frozen: Override { bytes, receipt },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_entries_match_source_bytes_and_pretty_receipt() {
        let source = br#"{"areal_g_m2":180,"tensile_N_m":900,"bending_Nm":1e-5}"#.to_vec();
        let receipt = json!({
            "source": "custom_fabric.json",
            "source_sha256": sha256_hex(&source),
            "operation": "replace_whole_fabric",
            "basis": "legacy_uncertain",
        });
        let ov = Override {
            bytes: source.clone(),
            receipt: receipt.clone(),
        };
        let [src_entry, receipt_entry] = ov.entries().expect("entries serialize");
        assert_eq!(src_entry.0, "fabric.source.json");
        assert_eq!(src_entry.1, source);
        assert_eq!(receipt_entry.0, "fabric_override.json");
        assert!(receipt_entry.1.ends_with(b"\n"));
        let parsed: Value = serde_json::from_slice(&receipt_entry.1).expect("valid receipt JSON");
        assert_eq!(parsed, receipt);
    }

    #[test]
    fn whole_fabric_override_clears_stale_piece_overrides_and_records_them() {
        let replacement = json!({"areal_g_m2": 220, "tensile_N_m": 1200, "bending_Nm": 2e-5});
        let make_input = || Input {
            selection: Selection::Fabric(replacement.clone()),
            frozen: Override {
                bytes: serde_json::to_vec(&replacement).unwrap(),
                receipt: json!({
                    "source": "wool.json",
                    "operation": "replace_whole_fabric",
                    "basis": "legacy_uncertain",
                    "effective_fabric": replacement,
                }),
            },
        };

        // Pattern with per-piece overrides: stale piece fabrics are stripped and audited.
        let mut bound = json!({
            "schema": "fabric-pattern/1",
            "fabric": {"name": "old_shell", "interfaces": []},
            "pieces": [
                {"id": "front"},
                {"id": "sleeve", "fabric": {"name": "old_sleeve"}},
                {"id": "cuff", "fabric": null}
            ]
        });
        let ov = make_input().apply(&mut bound).unwrap();
        assert_eq!(bound["fabric"], replacement);
        assert!(bound["pieces"][0].get("fabric").is_none());
        assert!(bound["pieces"][1].get("fabric").is_none());
        assert!(bound["pieces"][2].get("fabric").is_none());
        assert_eq!(
            ov.receipt()["cleared_piece_fabric_overrides"],
            json!(["sleeve"])
        );

        // Uniform pattern without piece overrides: receipt gains no extra field.
        let mut uniform = json!({
            "schema": "fabric-pattern/1",
            "fabric": {"name": "old_shell"},
            "pieces": [{"id": "front"}]
        });
        let ov_uniform = make_input().apply(&mut uniform).unwrap();
        assert_eq!(uniform["fabric"], replacement);
        assert!(
            ov_uniform
                .receipt()
                .get("cleared_piece_fabric_overrides")
                .is_none()
        );
    }

    #[test]
    fn snapshot_bytes_enforces_exact_byte_budget() {
        let doc = json!({"schema": "fabric-pattern/1", "pieces": []});
        let exact = serde_json::to_vec(&doc).unwrap();
        assert_eq!(
            snapshot_bytes_with_limit(&doc, "effective test pattern", exact.len()).unwrap(),
            exact
        );
        let err =
            snapshot_bytes_with_limit(&doc, "effective test pattern", exact.len() - 1).unwrap_err();
        assert_eq!(err.code, RefusalCode::BudgetExceeded);
        assert!(err.message.contains("effective test pattern"));
    }
}
