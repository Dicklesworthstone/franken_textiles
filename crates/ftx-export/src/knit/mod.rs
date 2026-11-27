//! Knitting machine & human hand-knit exporters (`FTX-K06`, `FTX-K07`):
//! - Verified CMU Knitout v2 (`.k`) + sealed `ftx-machine-cert/1` + `Exp-3` calibration swatch kits;
//! - Native/WASM pass-oriented Kniterate `.kc` with independent raster/control read-back;
//! - Brother KH-930 / KH-270 AYAB 1-bit indexed PNG + synchronized manual-carriage checklist;
//! - Human hand-knit pure-vector CYC SVG & PDF charts with dedicated acceptance checks;
//! - Fail-closed Tier-B proprietary refusal (`OpaqueProprietaryTarget`) for Shima/Stoll binaries.

pub mod ayab;
pub mod hand_knit;
pub mod kniterate;
pub mod knitout;
pub mod vendor_handoff;

pub use ayab::{
    AyabExportBundle, AyabMode, AyabSessionChecklist, export_brother_ayab, verify_ayab_1bit_png,
};
pub use hand_knit::{
    HandKnitExportBundle, check_hand_knit_pdf, check_hand_knit_svg, export_hand_knit_document,
};
pub use kniterate::{
    KCodeReadback, KCodeTakedownReport, KCodeVerification, KniterateExportBundle,
    export_kniterate_kc, read_kniterate_kc, verify_kniterate_kc_roundtrip,
    verify_kniterate_takedown,
};
pub use knitout::{
    KnitoutExportBundle, SwatchKitBundle, export_calibration_swatch_kit, export_knitout_v2,
    verify_knitout_v2_readback,
};
pub use vendor_handoff::{
    TierBProprietaryTarget, TierBVendorHandoff, VENDOR_HANDOFF_SCHEMA, VendorHandoffManifest,
    export_tier_b_vendor_handoff, is_proprietary_handoff, refuse_proprietary_binary_target,
    verify_tier_b_handoff_bundle,
};

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    CompiledKnitBundle, DrapeUncertaintyReceipt, KnitDoc, KnitMachineCert, KnitSwatchQualification,
    MachineBackendKind, MachineProfile, SeamedPulloverDraft, compile_knit_doc,
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
    /// Exact KCode byte/source binding when the Kniterate target was representable.
    pub kcode_verification: Option<KCodeVerification>,
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
    let mut kcode_verification = None;
    match profile.backend_kind {
        MachineBackendKind::KniterateKc => match export_kniterate_kc(&compiled, profile) {
            Ok(kc) => {
                files.insert(
                    "kcode_verification.json".into(),
                    json_bytes(&kc.verification)?,
                );
                kcode_verification = Some(kc.verification);
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
        kcode_verification,
    })
}

/// All four independent pullover programs plus checked seam accounting and assembly instructions.
#[derive(Clone, Debug)]
pub struct SeamedPulloverExportBundle {
    /// Ordered, relative artifact paths. Each piece owns its own subdirectory and certificate.
    pub files: BTreeMap<String, Vec<u8>>,
    /// Certificates indexed by the stable piece roles.
    pub piece_certificates: BTreeMap<String, KnitMachineCert>,
    /// Unavailable target conversions, retained independently for every affected piece.
    pub backend_export_refusals: BTreeMap<String, Refusal>,
}

/// Prepare a complete four-piece pullover without exposing partially validated output.
/// Every piece passes the ordinary document compiler, qualification, chart and target checks.
/// Optional receipts are keyed by `front`, `back`, `left_sleeve`, or `right_sleeve`; a receipt
/// for one piece cannot qualify another. Manual joining never becomes whole-garment machine code.
///
/// # Errors
/// Invalid assembly/seam accounting, unknown receipt roles, or any per-piece export refusal.
pub fn export_seamed_pullover(
    draft: &SeamedPulloverDraft,
    profile: &MachineProfile,
    qualification: Option<&KnitSwatchQualification>,
    expected_material_sha256: Option<&str>,
    drape_receipts: &BTreeMap<String, DrapeUncertaintyReceipt>,
    created: &str,
) -> Result<SeamedPulloverExportBundle, Refusal> {
    draft.validate()?;
    for role in drape_receipts.keys() {
        if !draft.pieces.iter().any(|piece| piece.role.as_str() == role) {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("pullover drape receipt names unknown piece {role}"),
            ));
        }
    }
    let mut files = BTreeMap::new();
    let mut piece_certificates = BTreeMap::new();
    let mut backend_export_refusals = BTreeMap::new();
    let mut piece_manifest = Vec::new();
    for piece in &draft.pieces {
        let role = piece.role.as_str();
        let bundle = export_knit_document(
            &piece.doc,
            profile,
            qualification,
            expected_material_sha256,
            drape_receipts.get(role),
            created,
        )?;
        piece_manifest.push(serde_json::json!({
            "role":role,
            "pattern_sha256":piece.doc.canonical_sha256()?,
            "certificate_sha256":bundle.compiled.cert.cert_sha256,
            "directory":role,
        }));
        piece_certificates.insert(role.to_owned(), bundle.compiled.cert);
        if let Some(refusal) = bundle.backend_export_refusal {
            backend_export_refusals.insert(role.to_owned(), refusal);
        }
        for (name, bytes) in bundle.files {
            files.insert(format!("{role}/{name}"), bytes);
        }
    }
    files.insert("garment.knit.json".into(), json_bytes(draft)?);
    files.insert(
        "assembly.json".into(),
        json_bytes(&serde_json::json!({
            "schema":"ftx-knit-pullover-assembly/1",
            "pieces":piece_manifest,
            "seams":draft.seams,
            "openings":draft.openings,
            "residuals_mm":draft.residuals_mm,
            "instructions":draft.instructions,
            "assembly":"four independently knitted pieces, joined manually",
            "hardware_execution_authorized":false,
            "physical_knitting_validated":false,
            "fit_claimed":false,
        }))?,
    );
    let mut instructions = String::from(
        "Four-piece seamed pullover\n\nKnit each piece from its own checked program or chart.\n\n",
    );
    for (i, instruction) in draft.instructions.iter().enumerate() {
        instructions.push_str(&format!("{}. {instruction}\n", i + 1));
    }
    instructions.push_str(
        "\nUse assembly.json for exact stitch/row groups and reported seam-length residuals.\nEach piece's instructions.txt retains its held neckline loops and manual finishing.\nA checked digital assembly does not establish physical garment fit or authorize machine execution.\n",
    );
    files.insert(
        "assembly_instructions.txt".into(),
        instructions.into_bytes(),
    );
    Ok(SeamedPulloverExportBundle {
        files,
        piece_certificates,
        backend_export_refusals,
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
            (MachineProfile::kniterate_7e(), "program.kc"),
            (MachineProfile::brother_kh930_ayab(), "pattern_ayab.png"),
            (MachineProfile::hand_knit(4.0), "chart.pdf"),
            (
                MachineProfile::shima_seiki_12e_handoff(),
                "vendor_handoff_manifest.json",
            ),
        ] {
            let mut cam = (profile.cam_setting_range[0] + profile.cam_setting_range[1]) * 0.5;
            if profile.backend_kind == MachineBackendKind::KniterateKc {
                cam = cam.floor();
            }
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

#[cfg(test)]
mod pullover_tests {
    use super::*;
    use ftx_pattern::knit::{
        CarrierId, GaugeSpec, SeamedPulloverSpec, WearerAgeClass, draft_seamed_pullover,
    };

    fn garment() -> (SeamedPulloverDraft, MachineProfile) {
        let profile = MachineProfile::hand_knit(4.0);
        let gauge = GaugeSpec {
            wales_per_100mm: 20.0,
            courses_per_100mm: 25.0,
            needle_pitch_mm: profile.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let spec = SeamedPulloverSpec {
            name: "synthetic-export-pullover".into(),
            wearer_age_class: WearerAgeClass::Child3To14,
            chest_girth_mm: 600.0,
            hem_girth_mm: 560.0,
            body_length_mm: 320.0,
            armhole_depth_mm: 96.0,
            sleeve_length_mm: 240.0,
            cuff_girth_mm: 120.0,
            neck_width_mm: 100.0,
            front_neck_depth_mm: 0.0,
            back_neck_depth_mm: 0.0,
            body_rib_courses: 0,
            cuff_rib_courses: 0,
            cam_setting: 0.5 * (profile.cam_setting_range[0] + profile.cam_setting_range[1]),
            yarn_id: "synthetic-test-yarn".into(),
            carrier_id: CarrierId("1".into()),
            max_seam_length_mismatch_mm: 1.0,
            has_hood_or_neck_drawstring: false,
        };
        (
            draft_seamed_pullover(&spec, &gauge, &profile).unwrap(),
            profile,
        )
    }

    #[test]
    fn pullover_bundle_keeps_all_four_verified_documents_and_manual_assembly() {
        let (draft, profile) = garment();
        let receipts = BTreeMap::new();
        let bundle =
            export_seamed_pullover(&draft, &profile, None, None, &receipts, "2026-11-27T12:00")
                .unwrap();
        assert_eq!(bundle.piece_certificates.len(), 4);
        assert!(bundle.backend_export_refusals.is_empty());
        let saved: SeamedPulloverDraft =
            serde_json::from_slice(&bundle.files["garment.knit.json"]).unwrap();
        saved.validate().unwrap();
        assert_eq!(saved, draft);
        let assembly: serde_json::Value =
            serde_json::from_slice(&bundle.files["assembly.json"]).unwrap();
        assert_eq!(assembly["seams"].as_array().unwrap().len(), 10);
        assert_eq!(assembly["openings"].as_array().unwrap().len(), 4);
        assert_eq!(assembly["hardware_execution_authorized"], false);
        assert_eq!(assembly["fit_claimed"], false);
        for piece in &draft.pieces {
            let role = piece.role.as_str();
            let standalone =
                export_knit_document(&piece.doc, &profile, None, None, None, "2026-11-27T12:00")
                    .unwrap();
            for (name, bytes) in standalone.files {
                assert_eq!(bundle.files[&format!("{role}/{name}")], bytes);
            }
            bundle.piece_certificates[role].verify_integrity().unwrap();
        }
        let mut stale = draft;
        stale.seams[0].groups[0].a_indices[1] += 1;
        assert_eq!(
            export_seamed_pullover(&stale, &profile, None, None, &receipts, "2026-11-27T12:00",)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
    }
}
