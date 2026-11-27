//! Verified CMU Knitout v2 (`.k`) + `ftx-machine-cert/1` + `Exp-3` swatch kit exporter (`FTX-K06`).

use std::collections::BTreeMap;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    CompiledKnitBundle, KnitDoc, KnitMachineCert, KnitSafetyContext, KnitSwatchQualification,
    KnitoutProgram, MachineBackendKind, MachineProfile, VBedVmReport, compile_knit_doc,
    draft_calibration_swatch, parse_knitout, verify_knitout_program,
};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

/// Schema tag for Tier-B vendor handoff manifests (`FTX-K06`).
pub const VENDOR_HANDOFF_SCHEMA: &str = "ftx-vendor-handoff/1";

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

/// Manifest accompanying a Tier-B vendor handoff package (`ShimaSeikiHandoff` / `StollHandoff`).
///
/// Direct proprietary binary emission (`.dat`, `SINTRAL`) is strictly refused; this manifest
/// records every opaque vendor table that must be populated inside vendor software before
/// hardware execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VendorHandoffManifest {
    /// Must equal [`VENDOR_HANDOFF_SCHEMA`].
    pub schema: String,
    /// Target proprietary vendor family (`"shima_seiki"` or `"stoll"`).
    pub target_vendor: String,
    /// Proprietary binary format that was explicitly withheld (e.g. `".dat"` or `"SINTRAL"`).
    pub withheld_binary_format: String,
    /// Target [`MachineProfile::id`].
    pub machine_id: String,
    /// Canonical SHA-256 of the target [`MachineProfile`].
    pub machine_sha256: String,
    /// SHA-256 of the verified CMU Knitout v2 handoff `.k` program.
    pub knitout_sha256: String,
    /// Sealed `ftx-machine-cert/1` digest.
    pub cert_sha256: String,
    /// Always `false`: Knitout v2 handoff requires vendor compilation/import before running.
    pub machine_executable: bool,
    /// Always `false`: software verification never authorizes unattended hardware execution.
    pub hardware_execution_authorized: bool,
    /// Explicit opaque vendor tables not encoded in open Knitout v2.
    pub unresolved_vendor_tables: Vec<String>,
    /// Required operator workflow inside vendor software before knitting.
    pub operator_handoff_steps: Vec<String>,
}

/// Complete Tier-B vendor handoff package (`FTX-K06`): verified `.k` + manifest + sealed cert.
#[derive(Clone, Debug)]
pub struct TierBVendorHandoff {
    /// Verified CMU Knitout v2 bundle (`.k` + `ftx-machine-cert/1`).
    pub knitout: KnitoutExportBundle,
    /// Explicit Tier-B vendor handoff manifest (`machine_executable: false`).
    pub manifest: VendorHandoffManifest,
    /// Pretty deterministic JSON of `manifest`.
    pub manifest_json: String,
    /// SHA-256 of `manifest_json`.
    pub manifest_sha256: String,
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

/// Builds a Tier-B proprietary vendor handoff package (`ShimaSeikiHandoff` / `StollHandoff`):
/// verified CMU Knitout v2 (`.k`) + sealed `ftx-machine-cert/1` + `vendor_handoff_manifest.json`
/// with `machine_executable: false` and explicit unresolved proprietary vendor tables (`FTX-K06`).
///
/// Call [`refuse_proprietary_binary_target`] when a caller requests direct `.dat` / `SINTRAL`
/// binary emission; use this function only to produce the explicit Knitout v2 vendor handoff IR.
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] if `profile` is not a Tier-B opaque proprietary profile, or
/// [`RefusalCode::VerificationMismatch`] if the bundle certificate does not match `profile`.
pub fn export_tier_b_vendor_handoff(
    bundle: &CompiledKnitBundle,
    profile: &MachineProfile,
    withheld_binary_format: &str,
) -> Result<TierBVendorHandoff, Refusal> {
    profile.validate()?;
    if !profile.backend_kind.is_opaque_proprietary() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            format!(
                "Tier-B vendor handoff requires a proprietary Shima/Stoll profile (got {:?})",
                profile.backend_kind
            ),
        ));
    }
    if withheld_binary_format.trim().is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "withheld_binary_format must be non-empty",
        ));
    }
    let machine_sha256 = profile.canonical_sha256()?;
    if bundle.cert.machine_sha256 != machine_sha256 {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Tier-B vendor handoff certificate machine_sha256 does not match target MachineProfile",
        ));
    }
    let knitout = export_knitout_v2(bundle)?;
    let (target_vendor, unresolved_vendor_tables, operator_handoff_steps) = match profile.backend_kind
    {
        MachineBackendKind::ShimaSeikiHandoff => (
            "shima_seiki".to_string(),
            vec![
                "shima_dat_stitch_value_lut (np/cam dial to physical loop length)".to_string(),
                "shima_sub_roller_and_takedown_table".to_string(),
                "shima_presser_foot_and_sinker_timing_table".to_string(),
                "shima_carriage_collision_and_yarn_carrier_kickback_controller".to_string(),
            ],
            vec![
                "Import the verified `.k` Knitout v2 file into Shima Seiki SDS-ONE APEX or official knitout-to-dat converter.".to_string(),
                "Assign yarn-lot stitch value tables, takedown weights, and carriage speed profile inside vendor software.".to_string(),
                "Run vendor knitting simulation and physical calibration swatch before unattended knitting.".to_string(),
            ],
        ),
        MachineBackendKind::StollHandoff => (
            "stoll".to_string(),
            vec![
                "stoll_sintral_np_stitch_length_table".to_string(),
                "stoll_wm_takedown_and_comb_control_table".to_string(),
                "stoll_holding_down_jack_and_presser_table".to_string(),
                "stoll_carriage_stroke_and_carrier_parking_controller".to_string(),
            ],
            vec![
                "Import the verified `.k` Knitout v2 file into Stoll M1plus / Create Plus.".to_string(),
                "Assign NP stitch values, WM takedown profile, and yarn carrier home positions inside vendor software.".to_string(),
                "Run vendor technical check and physical calibration swatch before unattended knitting.".to_string(),
            ],
        ),
        _ => unreachable!("checked is_opaque_proprietary above"),
    };
    let manifest = VendorHandoffManifest {
        schema: VENDOR_HANDOFF_SCHEMA.to_string(),
        target_vendor,
        withheld_binary_format: withheld_binary_format.to_string(),
        machine_id: profile.id.clone(),
        machine_sha256,
        knitout_sha256: knitout.knitout_sha256.clone(),
        cert_sha256: knitout.cert.cert_sha256.clone(),
        machine_executable: false,
        hardware_execution_authorized: false,
        unresolved_vendor_tables,
        operator_handoff_steps,
    };
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("cannot serialize Tier-B vendor handoff manifest: {e}"),
        )
    })?;
    let manifest_sha256 = sha256_hex(manifest_json.as_bytes());
    Ok(TierBVendorHandoff {
        knitout,
        manifest,
        manifest_json,
        manifest_sha256,
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

/// Checks whether a requested backend kind is Tier-B proprietary.
#[must_use]
pub const fn is_proprietary_handoff(kind: MachineBackendKind) -> bool {
    kind.is_opaque_proprietary()
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

    #[test]
    fn tier_b_vendor_handoff_emits_non_executable_manifest_and_refuses_tier_a() {
        let shima = MachineProfile::shima_seiki_12e_handoff();
        let err = refuse_proprietary_binary_target(&shima, ".dat").unwrap_err();
        assert_eq!(err.code, RefusalCode::OpaqueProprietaryTarget);

        let doc = draft_calibration_swatch(&shima, "merino-120tex", 18, 6, &[5.0, 6.0]).unwrap();
        let compiled = compile_knit_doc(&doc, &shima, None, Some("mat-sha"), None).unwrap();
        let handoff = export_tier_b_vendor_handoff(&compiled, &shima, ".dat").unwrap();
        assert!(!handoff.manifest.machine_executable);
        assert!(!handoff.manifest.hardware_execution_authorized);
        assert_eq!(handoff.manifest.target_vendor, "shima_seiki");
        assert!(!handoff.manifest.unresolved_vendor_tables.is_empty());
        assert_eq!(
            handoff.manifest.knitout_sha256,
            handoff.knitout.knitout_sha256
        );

        // Tier-A Kniterate profile is rejected for Tier-B vendor handoff.
        let kniterate = MachineProfile::kniterate_7e();
        assert_eq!(
            export_tier_b_vendor_handoff(&compiled, &kniterate, ".dat")
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );
    }
}
