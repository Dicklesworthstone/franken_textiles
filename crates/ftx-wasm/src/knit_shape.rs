//! Portable annular-shell drafting and the same verified files as `ftx knit shape`.

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_export::knit::{ShapedKnitSource, export_shaped_knit};
use ftx_pattern::knit::{
    AutoKnitConfig, DrapeUncertaintyReceipt, GaugeSpec, KnitSwatchQualification,
};
use serde::Deserialize;
use serde_json::json;

const MAX_REQUEST_BYTES: usize = 24 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 96 * 1024 * 1024;

/// Local source and explicit shaping settings. Sealed evidence remains raw JSON text.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitShapeRequest {
    /// Built-in target machine ID.
    pub machine: String,
    /// Complete original `EaseShellMesh` text, such as `ftx knit shell` output.
    pub shell_json: String,
    /// Explicit wearer, yarn, carrier and numerical work choices.
    pub config: AutoKnitConfig,
    /// Relaxed gauge; never inferred from the mesh or the machine needle pitch.
    pub gauge: GaugeSpec,
    /// Optional measured swatch evidence, unchanged across the JavaScript boundary.
    pub qualification_json: Option<String>,
    /// Independently selected source material identity.
    pub material_sha256: Option<String>,
    /// Optional exact sealed drape receipt for the resulting document.
    pub drape_receipt_json: Option<String>,
    /// Deterministic artifact creation date (`YYYY-MM-DDTHH:MM`).
    #[serde(default = "crate::knit::default_created")]
    pub created: String,
}

/// Draft an admitted annular shell, verify it and return a bounded ZIP of the complete output.
///
/// # Errors
/// Input/output budgets, invalid or unsupported shell topology, numerical, machine,
/// qualification and receipt refusals are shared with the native exporter.
pub fn knit_shape_json(request_json: &str) -> Result<String, Refusal> {
    if request_json.len() > MAX_REQUEST_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "knit shaping request exceeds 24 MiB",
        ));
    }
    let req: KnitShapeRequest = serde_json::from_str(request_json).map_err(invalid_json)?;
    let qualification: Option<KnitSwatchQualification> = req
        .qualification_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(invalid_json)?;
    let receipt: Option<DrapeUncertaintyReceipt> = req
        .drape_receipt_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(invalid_json)?;
    let profile = crate::knit::resolve_knit_machine(&req.machine)?;
    let bundle = export_shaped_knit(
        ShapedKnitSource {
            shell_json: &req.shell_json,
            config: &req.config,
            gauge: &req.gauge,
        },
        &profile,
        qualification.as_ref(),
        req.material_sha256.as_deref(),
        receipt.as_ref(),
        &req.created,
    )?;
    let files: Vec<_> = bundle.export.files.into_iter().collect();
    let manifest: Vec<_> = files
        .iter()
        .map(|(name, bytes)| {
            json!({
                "name":name, "bytes":bytes.len(), "sha256":sha256_hex(bytes),
            })
        })
        .collect();
    let archive = crate::zip::zip(&files, &req.created);
    let output = serde_json::to_string(&json!({
        "machine":profile.id,
        "source_file_sha256":bundle.source_file_sha256,
        "pattern_sha256":bundle.report.doc.canonical_sha256()?,
        "shape":bundle.report,
        "files":manifest,
        "archive_base64":crate::knit::encode_base64(&archive),
        "archive_sha256":sha256_hex(&archive),
        "cert":bundle.export.compiled.cert,
        "vm_report":bundle.export.compiled.vm_report,
        "backend_export_refusal":bundle.export.backend_export_refusal,
        "kcode_verification":bundle.export.kcode_verification,
        "machine_executable":false,
        "hardware_execution_authorized":false,
        "physical_knitting_validated":false,
        "fit_claimed":false,
    }))
    .map_err(invalid_json)?;
    if output.len() > MAX_RESPONSE_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "knit shaping response exceeds 96 MiB",
        ));
    }
    Ok(output)
}

fn invalid_json(error: serde_json::Error) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, error.to_string())
}

/// Browser entry with structured refusals and the standard knitting result envelope.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = knit_shape_json)]
pub fn web_knit_shape_json(request_json: &str) -> String {
    match knit_shape_json(request_json) {
        Ok(result) => format!(r#"{{"ok":true,"result":{result}}}"#),
        Err(refusal) => json!({"ok":false,"refusal":refusal}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_shape_preserves_native_zip_and_requires_machine_qualification() {
        let shell = include_str!("../../../tests/fixtures/knit/synthetic-annular-shell.json");
        let mut request = json!({
            "machine":"hand-knit", "shell_json":shell,
            "config":{
                "name":"synthetic-portable-annulus", "wearer_age_class":"adult",
                "yarn_id":"synthetic-yarn", "carrier_id":"1", "cam_setting":13.0,
                "max_idt_flips":1024, "has_hood_or_neck_drawstring":false
            },
            "gauge":{
                "wales_per_100mm":20.0, "courses_per_100mm":25.0, "needle_pitch_mm":4.0,
                "yarn_tex":180.0, "thickness_mm":2.0, "relaxed_after_wash":true
            },
            "created":"2026-11-27T12:00"
        });
        let result: serde_json::Value =
            serde_json::from_str(&knit_shape_json(&request.to_string()).unwrap()).unwrap();
        let req: KnitShapeRequest = serde_json::from_value(request.clone()).unwrap();
        let expected = export_shaped_knit(
            ShapedKnitSource {
                shell_json: &req.shell_json,
                config: &req.config,
                gauge: &req.gauge,
            },
            &crate::knit::resolve_knit_machine(&req.machine).unwrap(),
            None,
            None,
            None,
            &req.created,
        )
        .unwrap();
        let files: Vec<_> = expected.export.files.into_iter().collect();
        assert_eq!(
            result["archive_base64"],
            crate::knit::encode_base64(&crate::zip::zip(&files, &req.created))
        );
        assert_eq!(result["source_file_sha256"], sha256_hex(shell.as_bytes()));
        assert_eq!(result["shape"]["course_count"], 20);
        assert!(
            result["shape"].get("doc").is_none(),
            "the graph has its own exact artifact"
        );
        for (name, bytes) in files {
            let entry = result["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["name"] == name)
                .unwrap();
            assert_eq!(entry["sha256"], sha256_hex(&bytes));
        }
        assert_eq!(result["physical_knitting_validated"], false);
        assert_eq!(result["hardware_execution_authorized"], false);
        assert_eq!(result["fit_claimed"], false);
        request["machine"] = json!("kniterate-7e");
        request["gauge"]["needle_pitch_mm"] = json!(25.4 / 7.0);
        request["config"]["cam_setting"] = json!(7.0);
        assert_eq!(
            knit_shape_json(&request.to_string()).unwrap_err().code,
            RefusalCode::UnqualifiedProcess
        );
    }
}
