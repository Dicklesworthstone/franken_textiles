//! One source-to-program path for native and portable annular-shell drafting.

use ftx_core::receipt::sha256_hex;
use ftx_core::shaping::EaseShellMesh;
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    AutoKnitConfig, AutoKnitReport, DrapeUncertaintyReceipt, GaugeSpec, KnitSwatchQualification,
    MachineProfile, draft_autoknit_from_shell,
};

use super::{KnitDocumentExportBundle, export_knit_document};

/// Maximum retained source-shell JSON size, shared by native and portable callers.
pub const MAX_SHAPING_SOURCE_BYTES: usize = 16 * 1024 * 1024;
/// Maximum complete artifact payload before ZIP/base64 encoding.
pub const MAX_SHAPING_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;

/// Explicit geometry, garment choices and relaxed gauge for one shaped tube.
pub struct ShapedKnitSource<'a> {
    /// Original shell text, retained unchanged in the export.
    pub shell_json: &'a str,
    /// Explicit wearer, yarn, carrier and numerical work choices.
    pub config: &'a AutoKnitConfig,
    /// Independently supplied relaxed gauge.
    pub gauge: &'a GaugeSpec,
}

/// Shaped document, numerical residuals and the ordinary verified export artifacts.
pub struct ShapedKnitExportBundle {
    /// Actual geometric approximation and quantization results; no fit claim.
    pub report: AutoKnitReport,
    /// Exact input-text digest, distinct from the report's canonical shell digest.
    pub source_file_sha256: String,
    /// Shared target-specific programs, certificates, charts and retained inputs.
    pub export: KnitDocumentExportBundle,
}

/// Draft and compile an admitted annular shell through the existing machine export authority.
/// Complete artifacts are prepared before a caller creates files or publishes a download.
///
/// # Errors
/// Input/output budgets, invalid or unsupported shell topology, drafting, qualification,
/// machine, receipt and backend verification refusals.
pub fn export_shaped_knit(
    source: ShapedKnitSource<'_>,
    profile: &MachineProfile,
    qualification: Option<&KnitSwatchQualification>,
    expected_material_sha256: Option<&str>,
    drape_receipt: Option<&DrapeUncertaintyReceipt>,
    created: &str,
) -> Result<ShapedKnitExportBundle, Refusal> {
    if source.shell_json.len() > MAX_SHAPING_SOURCE_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "shaping source exceeds 16 MiB",
        ));
    }
    let shell: EaseShellMesh = serde_json::from_str(source.shell_json).map_err(|error| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("shaping shell: {error}"),
        )
    })?;
    let report = draft_autoknit_from_shell(&shell, source.gauge, profile, source.config)?;
    let source_file_sha256 = sha256_hex(source.shell_json.as_bytes());
    let mut export = export_knit_document(
        &report.doc,
        profile,
        qualification,
        expected_material_sha256,
        drape_receipt,
        created,
    )?;
    export.files.insert(
        "source_shell.json".into(),
        source.shell_json.as_bytes().to_vec(),
    );
    export.files.insert(
        "shaping_config.json".into(),
        super::json_bytes(source.config)?,
    );
    export
        .files
        .insert("gauge.json".into(), super::json_bytes(source.gauge)?);
    export.files.insert(
        "shape_report.json".into(),
        super::json_bytes(&serde_json::json!({
            "source_file_sha256":source_file_sha256,
            "shape":report,
            "physical_knitting_validated":false,
            "hardware_execution_authorized":false,
            "fit_claimed":false,
        }))?,
    );
    let bytes = export.files.values().try_fold(0usize, |sum, file| {
        sum.checked_add(file.len())
            .filter(|n| *n <= MAX_SHAPING_ARTIFACT_BYTES)
    });
    if bytes.is_none() {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "shaped knit artifacts exceed 64 MiB",
        ));
    }
    Ok(ShapedKnitExportBundle {
        report,
        source_file_sha256,
        export,
    })
}
