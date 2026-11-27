//! Verified CMU Knitout v2 (`.k`) + `ftx-machine-cert/1` + `Exp-3` swatch kit exporter (`FTX-K06`).

use std::collections::BTreeMap;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    CompiledKnitBundle, KnitDoc, KnitMachineCert, KnitSafetyContext, KnitSwatchQualification,
    KnitoutProgram, MachineProfile, VBedVmReport, compile_knit_doc, draft_calibration_swatch,
    parse_knitout, verify_knitout_program,
};
use ftx_pattern::materials::{Evidence, Source};

pub use super::vendor_handoff::{
    TierBProprietaryTarget, TierBVendorHandoff, VENDOR_HANDOFF_SCHEMA, VendorHandoffManifest,
    export_tier_b_vendor_handoff, is_proprietary_handoff, refuse_proprietary_binary_target,
    verify_tier_b_handoff_bundle,
};

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

/// Exports a verified CMU Knitout v2 (`.k`) + sealed `ftx-machine-cert/1` bundle from a [`CompiledKnitBundle`].
///
/// # Errors
/// If Knitout serialization or certificate integrity verification fails.
pub fn export_knitout_v2(bundle: &CompiledKnitBundle) -> Result<KnitoutExportBundle, Refusal> {
    bundle.cert.verify_integrity()?;
    let knitout_text = bundle.program.to_knitout_text()?;
    // Independent read-back verification: re-parse the emitted Knitout v2 stream and assert
    // exact AST and canonical text round-trip equality before sealing the export bundle.
    let reparsed = parse_knitout(&knitout_text)?;
    if reparsed != bundle.program || reparsed.to_knitout_text()? != knitout_text {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Knitout v2 read-back verification disagreed with compiled program AST",
        ));
    }
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

/// Independently parses a `;!knitout-2` text stream, verifies canonical round-trip fidelity, and
/// executes the `FTX-K03` Virtual V-Bed VM safety verifier (`FTX-K06` read-back gate).
///
/// # Errors
/// [`RefusalCode::VerificationMismatch`] if canonical re-serialization disagrees, or any parse /
/// `verify_knitout_program` refusal (`InvalidDocument`, `MachineStateInfeasible`,
/// `MaterialProcessHazard`).
pub fn verify_knitout_v2_readback(
    knitout_text: &str,
    profile: &MachineProfile,
    ctx: &KnitSafetyContext<'_>,
) -> Result<(KnitoutProgram, VBedVmReport), Refusal> {
    let reparsed = parse_knitout(knitout_text)?;
    let canonical = reparsed.to_knitout_text()?;
    let normalized_in = knitout_text.replace("\r\n", "\n");
    if canonical != normalized_in
        && canonical.trim_end_matches('\n') != normalized_in.trim_end_matches('\n')
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Knitout v2 read-back canonical re-serialization disagreed with input text",
        ));
    }
    let vm_report = verify_knitout_program(&reparsed, profile, ctx)?;
    Ok((reparsed, vm_report))
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
    let first_cam_setting = cam_settings.first().copied().ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "calibration swatch kit requires at least one cam setting",
        )
    })?;
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
        cam_stitch_setting: first_cam_setting,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_pattern::knit::WearerAgeClass;

    #[test]
    fn knitout_readback_verifies_exported_stream_and_refuses_tampered_crash() {
        let kniterate = MachineProfile::kniterate_7e();
        let kit = export_calibration_swatch_kit(
            &kniterate,
            "wool-200tex",
            "mat-sha-readback",
            16,
            6,
            &[6.0, 8.0],
        )
        .unwrap();
        let ctx = KnitSafetyContext {
            wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
            blocked_wale_pitch_mm: 100.0 / kit.doc.gauge.wales_per_100mm,
            source_doc: Some(&kit.doc),
            speed_fraction: Some(0.50),
            takedown_fraction: Some(0.60),
        };
        let (reparsed, report) =
            verify_knitout_v2_readback(&kit.knitout.knitout_text, &kniterate, &ctx).unwrap();
        assert_eq!(report.program_sha256, kit.knitout.knitout_sha256);
        assert!(!reparsed.ops.is_empty());

        // Tampering an exported Kniterate stream to use motorized inhook fails VM read-back.
        let tampered = kit.knitout.knitout_text.replacen("in 1", "inhook 1", 1);
        let err = verify_knitout_v2_readback(&tampered, &kniterate, &ctx).unwrap_err();
        assert_eq!(err.code, RefusalCode::MachineStateInfeasible);
    }
}
