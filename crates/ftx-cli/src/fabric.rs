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
                pattern
                    .as_object_mut()
                    .ok_or_else(|| bad_input("pattern must be an object"))?
                    .insert("fabric".into(), fabric);
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

    pub(super) fn save(&self, dir: &Path) -> Result<(), Refusal> {
        workspace::write_new(&dir.join("fabric.source.json"), &self.bytes)?;
        workspace::write_json(&dir.join("fabric_override.json"), &self.receipt)
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
    let file =
        File::open(path).map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
    let metadata = file
        .metadata()
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
    if !metadata.is_file() {
        return Err(bad_input("material input requires a regular JSON file"));
    }
    if metadata.len() > limit {
        return Err(budget());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
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
