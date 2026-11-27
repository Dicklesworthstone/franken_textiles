//! Knitting machine & human hand-knit exporters (`FTX-K06`, `FTX-K07`):
//! - Verified CMU Knitout v2 (`.k`) + sealed `ftx-machine-cert/1` + `Exp-3` calibration swatch kits;
//! - Explicit Kniterate `.kc` refusal until an actual pass-oriented KCode backend is implemented;
//! - Brother KH-930 / KH-270 AYAB 1-bit indexed PNG + synchronized manual-carriage checklist;
//! - Human hand-knit pure-vector CYC SVG & PDF charts with dedicated acceptance checks;
//! - Fail-closed Tier-B proprietary refusal (`OpaqueProprietaryTarget`) for Shima/Stoll binaries.

pub mod ayab;
pub mod hand_knit;
pub mod kniterate;
pub mod knitout;

pub use ayab::{
    AyabExportBundle, AyabMode, AyabSessionChecklist, export_brother_ayab, verify_ayab_1bit_png,
};
pub use hand_knit::{
    HandKnitExportBundle, check_hand_knit_pdf, check_hand_knit_svg, export_hand_knit_document,
};
pub use kniterate::{KniterateExportBundle, export_kniterate_kc, verify_kniterate_kc_roundtrip};
pub use knitout::{
    KnitoutExportBundle, SwatchKitBundle, TierBVendorHandoff, VENDOR_HANDOFF_SCHEMA,
    VendorHandoffManifest, export_calibration_swatch_kit, export_knitout_v2,
    export_tier_b_vendor_handoff, refuse_proprietary_binary_target, verify_knitout_v2_readback,
};

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    CompiledKnitBundle, DrapeUncertaintyReceipt, KnitDoc, KnitSwatchQualification,
    MachineBackendKind, MachineProfile, compile_knit_doc,
};

/// A complete, verified export of one saved knit document. Native and browser clients write
/// these same bytes; all validation finishes before either client exposes an artifact.
#[derive(Clone, Debug)]
pub struct KnitDocumentExportBundle {
    /// Compiled program, discrete verification report and sealed certificate.
    pub compiled: CompiledKnitBundle,
    /// Deterministically ordered artifact names and exact contents.
    pub files: BTreeMap<String, Vec<u8>>,
    /// A target conversion that remains unavailable; verified review artifacts still exist.
    pub backend_export_refusal: Option<Refusal>,
}

/// Compiles a saved knit document and prepares its target-specific files and printable charts.
/// An optional drape receipt must be intact and refer to this exact document and selected material.
/// Its gating flags remain blocking; a machine certificate never upgrades a drape or fit claim.
///
/// # Errors
/// Propagates document, qualification, topology, machine and read-back refusals. Returns
/// [`RefusalCode::VerificationMismatch`] for a stale/tampered receipt or failed chart check,
/// and [`RefusalCode::UnqualifiedProcess`] for a drape receipt with unresolved gating flags.
pub fn export_knit_document(
    doc: &KnitDoc,
    profile: &MachineProfile,
    qualification: Option<&KnitSwatchQualification>,
    expected_material_sha256: Option<&str>,
    drape_receipt: Option<&DrapeUncertaintyReceipt>,
    created: &str,
) -> Result<KnitDocumentExportBundle, Refusal> {
    if let Some(receipt) = drape_receipt {
        receipt.verify_seal()?;
        let material_sha256 = expected_material_sha256
            .or_else(|| qualification.map(|q| q.material_sha256.as_str()))
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::UnqualifiedProcess,
                    "a drape receipt requires the selected material digest or swatch qualification",
                )
                .repair("provide the material digest used to qualify and drape this knit document")
            })?;
        if receipt.pattern_sha256 != doc.canonical_sha256()?
            || receipt.material_sha256 != material_sha256
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "drape receipt does not describe the selected knit document and material",
            )
            .repair("recompute the drape receipt after changing the document or material"));
        }
        if !receipt.gating_flags.is_empty() {
            return Err(Refusal::new(
                RefusalCode::UnqualifiedProcess,
                format!(
                    "drape receipt has unresolved gating flags: {}",
                    receipt.gating_flags.join(", ")
                ),
            )
            .repair(
                "resolve the reported drape conditions before exporting this machine program",
            ));
        }
    }
    let compiled = compile_knit_doc(
        doc,
        profile,
        qualification,
        expected_material_sha256,
        drape_receipt.map(|receipt| receipt.payload_sha256.clone()),
    )?;
    let knitout = export_knitout_v2(&compiled)?;
    let hand = export_hand_knit_document(doc, created)?;
    let mut checks = check_hand_knit_svg(&hand.svg_text, "chart.svg")
        .into_iter()
        .chain(check_hand_knit_pdf(&hand.pdf_bytes, "chart.pdf"));
    if checks.any(|check| !check.ok) {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "knit chart failed its export acceptance checks",
        ));
    }

    let mut files = BTreeMap::from([
        ("pattern.knit.json".into(), json_bytes(doc)?),
        ("machine.json".into(), json_bytes(profile)?),
        ("program.k".into(), knitout.knitout_text.into_bytes()),
        ("cert.json".into(), knitout.cert_json.into_bytes()),
        ("chart.svg".into(), hand.svg_text.into_bytes()),
        ("chart.pdf".into(), hand.pdf_bytes),
        (
            "instructions.txt".into(),
            hand.instructions_text.into_bytes(),
        ),
    ]);
    if let Some(qualification) = qualification {
        files.insert("qualification.json".into(), json_bytes(qualification)?);
    }
    if let Some(receipt) = drape_receipt {
        files.insert("drape_receipt.json".into(), json_bytes(receipt)?);
    }
    let mut backend_export_refusal = None;
    match profile.backend_kind {
        MachineBackendKind::KniterateKc => match export_kniterate_kc(&compiled, profile) {
            Ok(kc) => {
                files.insert("program.kc".into(), kc.kc_text.into_bytes());
            }
            Err(refusal) if refusal.code == RefusalCode::UnqualifiedProcess => {
                files.insert("backend_export_refusal.json".into(), json_bytes(&refusal)?);
                backend_export_refusal = Some(refusal);
            }
            Err(refusal) => return Err(refusal),
        },
        MachineBackendKind::BrotherAyab => {
            let ayab = export_brother_ayab(doc, profile, &compiled.cert)?;
            files.insert("pattern_ayab.png".into(), ayab.png_bytes);
            files.insert(
                "ayab_session_checklist.json".into(),
                ayab.checklist_json.into_bytes(),
            );
        }
        MachineBackendKind::ShimaSeikiHandoff | MachineBackendKind::StollHandoff => {
            let format = if profile.backend_kind == MachineBackendKind::ShimaSeikiHandoff {
                ".dat"
            } else {
                "SINTRAL"
            };
            let handoff = export_tier_b_vendor_handoff(&compiled, profile, format)?;
            files.insert(
                "vendor_handoff_manifest.json".into(),
                handoff.manifest_json.into_bytes(),
            );
        }
        MachineBackendKind::HandKnitChart | MachineBackendKind::KnitoutV2 => {}
    }
    Ok(KnitDocumentExportBundle {
        compiled,
        files,
        backend_export_refusal,
    })
}

fn json_bytes(value: &impl serde::Serialize) -> Result<Vec<u8>, Refusal> {
    serde_json::to_vec_pretty(value)
        .map_err(|error| Refusal::new(RefusalCode::InvalidDocument, error.to_string()))
}

#[cfg(test)]
mod document_tests {
    use super::*;
    use ftx_core::receipt::sha256_hex;
    use ftx_pattern::knit::{KnitMachineCert, draft_calibration_swatch};
    use ftx_pattern::materials::Evidence;

    #[test]
    fn saved_document_exports_all_target_artifacts_deterministically() {
        for (profile, target) in [
            (
                MachineProfile::kniterate_7e(),
                "backend_export_refusal.json",
            ),
            (MachineProfile::brother_kh930_ayab(), "pattern_ayab.png"),
            (MachineProfile::hand_knit(4.0), "chart.pdf"),
            (
                MachineProfile::shima_seiki_12e_handoff(),
                "vendor_handoff_manifest.json",
            ),
        ] {
            let cam = (profile.cam_setting_range[0] + profile.cam_setting_range[1]) * 0.5;
            let doc = draft_calibration_swatch(&profile, "synthetic-yarn", 8, 4, &[cam]).unwrap();
            let export = || {
                export_knit_document(&doc, &profile, None, None, None, "2026-11-27T12:00").unwrap()
            };
            let bundle = export();
            assert_eq!(bundle.files, export().files);
            assert!(bundle.files.contains_key(target), "{}", profile.id);
            let saved: KnitDoc =
                serde_json::from_slice(&bundle.files["pattern.knit.json"]).unwrap();
            assert_eq!(saved, doc);
            let cert: KnitMachineCert = serde_json::from_slice(&bundle.files["cert.json"]).unwrap();
            cert.verify_integrity().unwrap();
            assert_eq!(cert.program_sha256, sha256_hex(&bundle.files["program.k"]));
            assert_eq!(cert.machine_sha256, profile.canonical_sha256().unwrap());
            assert!(!cert.hardware_execution_authorized);
            assert!(!cert.physical_knitting_validated);
            assert!(!cert.four_claims.fit_claimed);
        }
    }

    #[test]
    fn exports_bind_and_validate_the_complete_drape_receipt() {
        let profile = MachineProfile::kniterate_7e();
        let doc = draft_calibration_swatch(&profile, "synthetic-yarn", 8, 4, &[7.0]).unwrap();
        let receipt = DrapeUncertaintyReceipt::seal(
            doc.canonical_sha256().unwrap(),
            "synthetic-material".into(),
            1.05,
            0.2,
            0.0,
            0.0,
            vec![],
            Evidence::Estimate,
        )
        .unwrap();
        let export = |receipt: &DrapeUncertaintyReceipt, material: &str| {
            export_knit_document(
                &doc,
                &profile,
                None,
                Some(material),
                Some(receipt),
                "2026-11-27T12:00",
            )
        };
        let bundle = export(&receipt, "synthetic-material").unwrap();
        assert_eq!(
            bundle.compiled.cert.drape_receipt_sha256,
            Some(receipt.payload_sha256.clone())
        );
        assert_eq!(
            serde_json::from_slice::<DrapeUncertaintyReceipt>(&bundle.files["drape_receipt.json"])
                .unwrap(),
            receipt
        );
        assert_eq!(
            export(&receipt, "other-material").unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
        let mut tampered = receipt.clone();
        tampered.max_strain_ratio = 1.5;
        assert_eq!(
            export(&tampered, "synthetic-material").unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
        let stale = DrapeUncertaintyReceipt::seal(
            "other-pattern".into(),
            "synthetic-material".into(),
            1.05,
            0.2,
            0.0,
            0.0,
            vec![],
            Evidence::Estimate,
        )
        .unwrap();
        assert_eq!(
            export(&stale, "synthetic-material").unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
        let gated = DrapeUncertaintyReceipt::seal(
            doc.canonical_sha256().unwrap(),
            "synthetic-material".into(),
            1.5,
            0.0,
            0.0,
            0.0,
            vec!["KnitJamming".into()],
            Evidence::Estimate,
        )
        .unwrap();
        assert_eq!(
            export(&gated, "synthetic-material").unwrap_err().code,
            RefusalCode::UnqualifiedProcess
        );
    }
}
