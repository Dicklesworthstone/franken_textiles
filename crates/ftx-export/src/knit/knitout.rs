//! Verified CMU Knitout v2 (`.k`) + `ftx-machine-cert/1` + `Exp-3` swatch kit exporter (`FTX-K06`).

use std::collections::BTreeMap;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    CompiledKnitBundle, KnitDoc, KnitMachineCert, KnitSwatchQualification, MachineBackendKind,
    MachineProfile, compile_knit_doc, draft_calibration_swatch,
};
use ftx_pattern::materials::{Evidence, Source};

/// Emitted Knitout v2 artifact bundle.
#[derive(Clone, Debug)]
pub struct KnitoutExportBundle {
    /// Canonical UTF-8 `.k` text (`;!knitout-2`).
    pub knitout_text: String,
    /// Sealed `ftx-machine-cert/1` JSON.
    pub cert: KnitMachineCert,
    /// Pretty deterministic JSON string of `cert`.
    pub cert_json: String,
    /// SHA-256 of `knitout_text`.
    pub knitout_sha256: String,
}

/// Complete `Exp-3` stepped calibration swatch kit (ready to knit, wash, measure, and record).
#[derive(Clone, Debug)]
pub struct SwatchKitBundle {
    /// Drafted `CalibrationSwatchOnly` [`KnitDoc`].
    pub doc: KnitDoc,
    /// Verified Knitout v2 export bundle.
    pub knitout: KnitoutExportBundle,
    /// Blank `KnitSwatchQualification` template pre-populated with exact `machine_sha256` and `material_sha256`.
    pub qualification_template: KnitSwatchQualification,
    /// Deterministic JSON of `qualification_template`.
    pub qualification_template_json: String,
}

/// Refuses direct binary emission for Tier-B proprietary targets (`ShimaSeikiHandoff`, `StollHandoff`)
/// with [`RefusalCode::OpaqueProprietaryTarget`] and ranked repair pointing to Knitout v2 handoff.
///
/// # Errors
/// Always returns `Err(Refusal)` when `profile.backend_kind.is_opaque_proprietary()`.
pub fn refuse_proprietary_binary_target(
    profile: &MachineProfile,
    requested_format: &str,
) -> Result<(), Refusal> {
    if profile.backend_kind.is_opaque_proprietary() {
        return Err(Refusal::new(
            RefusalCode::OpaqueProprietaryTarget,
            format!(
                "direct proprietary binary emission ('{requested_format}') for '{}' is refused: vendor instruction sets and needle-bed collision controllers are opaque",
                profile.name
            ),
        )
        .repair("export verified CMU Knitout v2 (`.k`) handoff IR and import into vendor SDS-ONE APEX / M1plus software")
        .repair("or select an open Tier-A machine profile (Kniterate 7E `.kc`, Brother AYAB, or hand-knit chart)"));
    }
    Ok(())
}

/// Exports a verified CMU Knitout v2 (`.k`) + sealed `ftx-machine-cert/1` bundle from a [`CompiledKnitBundle`].
///
/// # Errors
/// If Knitout serialization or certificate integrity verification fails.
pub fn export_knitout_v2(bundle: &CompiledKnitBundle) -> Result<KnitoutExportBundle, Refusal> {
    bundle.cert.verify_integrity()?;
    let knitout_text = bundle.program.to_knitout_text()?;
    let knitout_sha256 = sha256_hex(knitout_text.as_bytes());
    if knitout_sha256 != bundle.cert.program_sha256 {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "compiled Knitout text digest disagrees with sealed certificate program_sha256",
        ));
    }
    let cert_json = serde_json::to_string_pretty(&bundle.cert).map_err(|e| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("cannot serialize knit certificate: {e}"),
        )
    })?;
    Ok(KnitoutExportBundle {
        knitout_text,
        cert: bundle.cert.clone(),
        cert_json,
        knitout_sha256,
    })
}

/// Generates a complete `Exp-3` stepped calibration & drift/barré swatch kit for any machine profile.
///
/// # Errors
/// If swatch geometry exceeds `CalibrationSwatchCaps` or profile validation fails.
pub fn export_calibration_swatch_kit(
    profile: &MachineProfile,
    yarn_id: &str,
    material_sha256: &str,
    needles: u32,
    courses_per_zone: u32,
    cam_settings: &[f64],
) -> Result<SwatchKitBundle, Refusal> {
    let doc = draft_calibration_swatch(profile, yarn_id, needles, courses_per_zone, cam_settings)?;
    let compiled = compile_knit_doc(&doc, profile, None, Some(material_sha256), None)?;
    let knitout = export_knitout_v2(&compiled)?;
    let machine_sha256 = profile.canonical_sha256()?;

    let mut conditioning = BTreeMap::new();
    conditioning.insert(
        "protocol".to_string(),
        "knit -> wash per yarn care -> dry flat 24h -> measure 100mm wales/courses & Exp-3 stripe drift".to_string(),
    );
    let qualification_template = KnitSwatchQualification {
        id: format!("swatch-{}-{}", profile.id, yarn_id),
        material_sha256: material_sha256.to_string(),
        machine_sha256,
        yarn_lot_id: "FILL_IN_YARN_LOT_ID".to_string(),
        was_washed_and_blocked: false, // operator must set true ONLY after washing & blocking!
        wales_per_100mm: doc.gauge.wales_per_100mm,
        courses_per_100mm: doc.gauge.courses_per_100mm,
        thickness_mm: 2.0,
        cam_stitch_setting: cam_settings[0],
        wick_dispersion_mm: None,
        shrinkage_axial_pct: None,
        yarn_intake_drift_eps: None,
        yarn_intake_drift_sigma_eta: None,
        evidence: Evidence::Estimate, // operator switches to Measured after physical measurement
        source: Source {
            reference: "local:unmeasured_template".to_string(),
            accessed_on: "YYYY-MM-DD".to_string(),
            note: "Replace with measured washed swatch values before production knitting"
                .to_string(),
        },
        method: "Exp-3 stepped cam calibration swatch".to_string(),
        conditioning,
        allow_estimates: false,
    };
    let qualification_template_json = serde_json::to_string_pretty(&qualification_template)
        .map_err(|e| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                format!("cannot serialize swatch template: {e}"),
            )
        })?;
    Ok(SwatchKitBundle {
        doc,
        knitout,
        qualification_template,
        qualification_template_json,
    })
}

/// Checks whether a requested backend kind is Tier-B proprietary.
#[must_use]
pub const fn is_proprietary_handoff(kind: MachineBackendKind) -> bool {
    kind.is_opaque_proprietary()
}
