//! Tier-B proprietary vendor handoff bundle & manifest verification (`FTX-K06`).
//!
//! Direct proprietary binary emission (`.dat`, `SINTRAL`) is refused fail-closed with
//! [`RefusalCode::OpaqueProprietaryTarget`]. Callers instead receive a verified CMU Knitout v2
//! (`.k`) handoff IR paired with a canonically sealed `vendor_handoff_manifest.json` listing all
//! unresolved proprietary vendor tables and enforcing `machine_executable: false`,
//! `hardware_execution_authorized: false`, `physical_knitting_validated: false`, and
//! `fit_claimed: false`.

use ftx_core::receipt::{canonical_json_sha256, sha256_hex};
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    CompiledKnitBundle, KnitSafetyContext, MachineBackendKind, MachineProfile, VBedVmReport,
};
use serde::{Deserialize, Serialize};

use super::knitout::{KnitoutExportBundle, export_knitout_v2, verify_knitout_v2_readback};

/// Schema tag for Tier-B vendor handoff manifests (`FTX-K06`).
pub const VENDOR_HANDOFF_SCHEMA: &str = "ftx-vendor-handoff/1";

/// Supported Tier-B opaque proprietary machine targets for verified Knitout v2 handoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TierBProprietaryTarget {
    /// Shima Seiki `.dat` proprietary binary target (requires SDS-ONE APEX / official converter).
    ShimaSeikiDat,
    /// Stoll `SINTRAL` proprietary program target (requires M1plus / Create Plus).
    StollSintral,
}

impl TierBProprietaryTarget {
    /// Canonical withheld proprietary binary format identifier.
    #[must_use]
    pub const fn withheld_format(self) -> &'static str {
        match self {
            Self::ShimaSeikiDat => ".dat",
            Self::StollSintral => "SINTRAL",
        }
    }

    /// Canonical vendor family string stored in [`VendorHandoffManifest::target_vendor`].
    #[must_use]
    pub const fn vendor_key(self) -> &'static str {
        match self {
            Self::ShimaSeikiDat => "shima_seiki",
            Self::StollSintral => "stoll",
        }
    }

    /// Resolves a Tier-B [`MachineProfile`] into its [`TierBProprietaryTarget`].
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] if `profile` is not a Tier-B opaque proprietary profile.
    pub fn from_profile(profile: &MachineProfile) -> Result<Self, Refusal> {
        match profile.backend_kind {
            MachineBackendKind::ShimaSeikiHandoff => Ok(Self::ShimaSeikiDat),
            MachineBackendKind::StollHandoff => Ok(Self::StollSintral),
            other => Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "Tier-B vendor handoff requires a proprietary Shima/Stoll profile (got {other:?})"
                ),
            )),
        }
    }
}

#[derive(Serialize)]
struct VendorHandoffSealView<'a> {
    schema: &'a str,
    target_vendor: &'a str,
    withheld_binary_format: &'a str,
    machine_id: &'a str,
    machine_sha256: &'a str,
    knitout_sha256: &'a str,
    cert_sha256: &'a str,
    machine_executable: bool,
    hardware_execution_authorized: bool,
    physical_knitting_validated: bool,
    fit_claimed: bool,
    unresolved_vendor_tables: &'a [String],
    operator_handoff_steps: &'a [String],
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
    /// Always `false`: software handoff never proves physical knitability without a machine test.
    pub physical_knitting_validated: bool,
    /// Always `false`: real-body fit is established only by a physical muslin or dressed wearer.
    pub fit_claimed: bool,
    /// Explicit opaque vendor tables not encoded in open Knitout v2.
    pub unresolved_vendor_tables: Vec<String>,
    /// Required operator workflow inside vendor software before knitting.
    pub operator_handoff_steps: Vec<String>,
    /// Sealed canonical SHA-256 over all fields above.
    pub manifest_sha256: String,
}

impl VendorHandoffManifest {
    /// Seals a new [`VendorHandoffManifest`], enforcing Four-Claims false booleans, explicit
    /// unresolved vendor table coverage, and canonical SHA-256 digest sealing.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] on missing identifiers/steps or serialization failure, or
    /// [`RefusalCode::VerificationMismatch`] if required unresolved vendor table categories are absent.
    #[allow(clippy::too_many_arguments)]
    pub fn seal(
        target_vendor: String,
        withheld_binary_format: String,
        machine_id: String,
        machine_sha256: String,
        knitout_sha256: String,
        cert_sha256: String,
        unresolved_vendor_tables: Vec<String>,
        operator_handoff_steps: Vec<String>,
    ) -> Result<Self, Refusal> {
        Self::validate_fields(
            VENDOR_HANDOFF_SCHEMA,
            &target_vendor,
            &withheld_binary_format,
            &machine_id,
            &machine_sha256,
            &knitout_sha256,
            &cert_sha256,
            false,
            false,
            false,
            false,
            &unresolved_vendor_tables,
            &operator_handoff_steps,
        )?;
        let view = VendorHandoffSealView {
            schema: VENDOR_HANDOFF_SCHEMA,
            target_vendor: &target_vendor,
            withheld_binary_format: &withheld_binary_format,
            machine_id: &machine_id,
            machine_sha256: &machine_sha256,
            knitout_sha256: &knitout_sha256,
            cert_sha256: &cert_sha256,
            machine_executable: false,
            hardware_execution_authorized: false,
            physical_knitting_validated: false,
            fit_claimed: false,
            unresolved_vendor_tables: &unresolved_vendor_tables,
            operator_handoff_steps: &operator_handoff_steps,
        };
        let manifest_sha256 = canonical_json_sha256(&view)?;
        Ok(Self {
            schema: VENDOR_HANDOFF_SCHEMA.to_string(),
            target_vendor,
            withheld_binary_format,
            machine_id,
            machine_sha256,
            knitout_sha256,
            cert_sha256,
            machine_executable: false,
            hardware_execution_authorized: false,
            physical_knitting_validated: false,
            fit_claimed: false,
            unresolved_vendor_tables,
            operator_handoff_steps,
            manifest_sha256,
        })
    }

    /// Verifies schema, vendor target, Four-Claims false booleans, explicit unresolved vendor
    /// table coverage, and canonical SHA-256 seal integrity.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] on malformed schema/vendor metadata, or
    /// [`RefusalCode::VerificationMismatch`] on tampered honesty booleans, missing vendor tables,
    /// or `manifest_sha256` mismatch.
    pub fn verify_integrity(&self) -> Result<(), Refusal> {
        Self::validate_fields(
            &self.schema,
            &self.target_vendor,
            &self.withheld_binary_format,
            &self.machine_id,
            &self.machine_sha256,
            &self.knitout_sha256,
            &self.cert_sha256,
            self.machine_executable,
            self.hardware_execution_authorized,
            self.physical_knitting_validated,
            self.fit_claimed,
            &self.unresolved_vendor_tables,
            &self.operator_handoff_steps,
        )?;
        let view = VendorHandoffSealView {
            schema: &self.schema,
            target_vendor: &self.target_vendor,
            withheld_binary_format: &self.withheld_binary_format,
            machine_id: &self.machine_id,
            machine_sha256: &self.machine_sha256,
            knitout_sha256: &self.knitout_sha256,
            cert_sha256: &self.cert_sha256,
            machine_executable: self.machine_executable,
            hardware_execution_authorized: self.hardware_execution_authorized,
            physical_knitting_validated: self.physical_knitting_validated,
            fit_claimed: self.fit_claimed,
            unresolved_vendor_tables: &self.unresolved_vendor_tables,
            operator_handoff_steps: &self.operator_handoff_steps,
        };
        let expected = canonical_json_sha256(&view)?;
        if expected != self.manifest_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "Tier-B vendor handoff manifest_sha256 mismatch (tampered or unsealed)",
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_fields(
        schema: &str,
        target_vendor: &str,
        withheld_binary_format: &str,
        machine_id: &str,
        machine_sha256: &str,
        knitout_sha256: &str,
        cert_sha256: &str,
        machine_executable: bool,
        hardware_execution_authorized: bool,
        physical_knitting_validated: bool,
        fit_claimed: bool,
        unresolved_vendor_tables: &[String],
        operator_handoff_steps: &[String],
    ) -> Result<(), Refusal> {
        if schema != VENDOR_HANDOFF_SCHEMA {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("unexpected vendor handoff manifest schema '{schema}'"),
            ));
        }
        if !matches!(target_vendor, "shima_seiki" | "stoll") {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "Tier-B vendor handoff target_vendor must be 'shima_seiki' or 'stoll' (got '{target_vendor}')"
                ),
            ));
        }
        if withheld_binary_format.trim().is_empty()
            || machine_id.trim().is_empty()
            || machine_sha256.trim().is_empty()
            || knitout_sha256.trim().is_empty()
            || cert_sha256.trim().is_empty()
            || operator_handoff_steps.is_empty()
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "Tier-B vendor handoff manifest requires non-empty format, machine/program/cert digests, and operator steps",
            ));
        }
        if machine_executable
            || hardware_execution_authorized
            || physical_knitting_validated
            || fit_claimed
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "Tier-B vendor handoff manifest violates Four-Claims honesty invariants (machine_executable, hardware_execution_authorized, physical_knitting_validated, and fit_claimed must all be false)",
            ));
        }
        let joined_tables = unresolved_vendor_tables.join("\n").to_ascii_lowercase();
        let covers_stitch_values = joined_tables.contains("stitch");
        let covers_sinker_presser_cams = joined_tables.contains("sinker")
            && (joined_tables.contains("presser") || joined_tables.contains("cam"));
        let covers_takedown_curves = joined_tables.contains("takedown");
        let covers_carriage_carrier =
            joined_tables.contains("carriage") && joined_tables.contains("carrier");
        if unresolved_vendor_tables.is_empty()
            || !covers_stitch_values
            || !covers_sinker_presser_cams
            || !covers_takedown_curves
            || !covers_carriage_carrier
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "Tier-B vendor handoff manifest unresolved_vendor_tables must explicitly cover stitch values, sinker/presser cams, takedown curves, and carriage/carrier controllers",
            ));
        }
        Ok(())
    }
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
    /// Canonical SHA-256 of `manifest` (equal to `manifest.manifest_sha256`).
    pub manifest_sha256: String,
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
        .repair("Export verified Knitout v2 + vendor handoff manifest for licensed Shima SDS-ONE APEX / Stoll M1plus compilation.")
        .repair("Or select an open Tier-A machine profile (Kniterate 7E .kc, Brother AYAB, or hand-knit chart)."));
    }
    Ok(())
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
    let target = TierBProprietaryTarget::from_profile(profile)?;
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
    let (unresolved_vendor_tables, operator_handoff_steps) = match target {
        TierBProprietaryTarget::ShimaSeikiDat => (
            vec![
                "stitch_values: shima_dat_stitch_value_lut (np/cam dial to physical loop length)"
                    .to_string(),
                "takedown_curves: shima_sub_roller_and_takedown_curve_table".to_string(),
                "sinker_presser_cams: shima_presser_foot_and_sinker_cam_timing_table".to_string(),
                "carriage_carrier_controllers: shima_carriage_collision_and_yarn_carrier_kickback_controller".to_string(),
            ],
            vec![
                "Import the verified `.k` Knitout v2 file into Shima Seiki SDS-ONE APEX or official knitout-to-dat converter.".to_string(),
                "Assign yarn-lot stitch value tables, takedown curves, sinker/presser cams, and carriage speed profile inside vendor software.".to_string(),
                "Run vendor knitting simulation and physical calibration swatch before unattended knitting.".to_string(),
            ],
        ),
        TierBProprietaryTarget::StollSintral => (
            vec![
                "stitch_values: stoll_sintral_np_stitch_length_value_table".to_string(),
                "takedown_curves: stoll_wm_takedown_curve_and_comb_control_table".to_string(),
                "sinker_presser_cams: stoll_holding_down_jack_sinker_and_presser_cam_table"
                    .to_string(),
                "carriage_carrier_controllers: stoll_carriage_stroke_and_carrier_parking_controller".to_string(),
            ],
            vec![
                "Import the verified `.k` Knitout v2 file into Stoll M1plus / Create Plus.".to_string(),
                "Assign NP stitch values, WM takedown curves, sinker/presser cams, and yarn carrier home positions inside vendor software.".to_string(),
                "Run vendor technical check and physical calibration swatch before unattended knitting.".to_string(),
            ],
        ),
    };
    let manifest = VendorHandoffManifest::seal(
        target.vendor_key().to_string(),
        withheld_binary_format.to_string(),
        profile.id.clone(),
        machine_sha256,
        knitout.knitout_sha256.clone(),
        knitout.cert.cert_sha256.clone(),
        unresolved_vendor_tables,
        operator_handoff_steps,
    )?;
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("cannot serialize Tier-B vendor handoff manifest: {e}"),
        )
    })?;
    let manifest_sha256 = manifest.manifest_sha256.clone();
    Ok(TierBVendorHandoff {
        knitout,
        manifest,
        manifest_json,
        manifest_sha256,
    })
}

/// Independently verifies a [`TierBVendorHandoff`] bundle:
/// 1. Manifest integrity (`schema`, vendor, Four-Claims false booleans, explicit table coverage, `manifest_sha256`);
/// 2. Serialized `manifest_json` round-trip equivalence;
/// 3. Sealed `ftx-machine-cert/1` integrity and cross-digest linkage (`machine_sha256`, `knitout_sha256`, `cert_sha256`);
/// 4. Independent Knitout v2 read-back parse & `FTX-K03` Virtual V-Bed VM verification.
///
/// # Errors
/// [`RefusalCode::InvalidDocument`], [`RefusalCode::VerificationMismatch`], or VM safety refusals.
pub fn verify_tier_b_handoff_bundle(
    handoff: &TierBVendorHandoff,
    profile: &MachineProfile,
    ctx: &KnitSafetyContext<'_>,
) -> Result<VBedVmReport, Refusal> {
    profile.validate()?;
    let target = TierBProprietaryTarget::from_profile(profile)?;
    handoff.manifest.verify_integrity()?;
    handoff.knitout.cert.verify_integrity()?;
    let reparsed_manifest: VendorHandoffManifest = serde_json::from_str(&handoff.manifest_json)
        .map_err(|e| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                format!("cannot parse serialized Tier-B vendor handoff manifest: {e}"),
            )
        })?;
    reparsed_manifest.verify_integrity()?;
    if reparsed_manifest != handoff.manifest {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Tier-B vendor handoff manifest_json disagrees with manifest struct",
        ));
    }
    if handoff.manifest_sha256 != handoff.manifest.manifest_sha256 {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Tier-B vendor handoff manifest_sha256 disagrees with sealed manifest digest",
        ));
    }
    if handoff.manifest.target_vendor != target.vendor_key()
        || handoff.manifest.machine_id != profile.id
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Tier-B vendor handoff manifest target_vendor or machine_id disagrees with MachineProfile",
        ));
    }
    let expected_machine_sha = profile.canonical_sha256()?;
    let actual_knitout_sha = sha256_hex(handoff.knitout.knitout_text.as_bytes());
    if handoff.manifest.machine_sha256 != expected_machine_sha
        || handoff.knitout.cert.machine_sha256 != expected_machine_sha
        || handoff.manifest.knitout_sha256 != handoff.knitout.knitout_sha256
        || actual_knitout_sha != handoff.knitout.knitout_sha256
        || handoff.knitout.cert.program_sha256 != handoff.knitout.knitout_sha256
        || handoff.manifest.cert_sha256 != handoff.knitout.cert.cert_sha256
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Tier-B vendor handoff cross-artifact SHA-256 digests disagree",
        ));
    }
    let (_reparsed, vm_report) =
        verify_knitout_v2_readback(&handoff.knitout.knitout_text, profile, ctx)?;
    Ok(vm_report)
}

/// Checks whether a requested backend kind is Tier-B proprietary.
#[must_use]
pub const fn is_proprietary_handoff(kind: MachineBackendKind) -> bool {
    kind.is_opaque_proprietary()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_pattern::knit::{KnitDoc, WearerAgeClass, compile_knit_doc, draft_calibration_swatch};

    fn compile_tier_b_swatch_for_test(
        profile: &MachineProfile,
    ) -> Result<(KnitDoc, CompiledKnitBundle), Refusal> {
        let doc = draft_calibration_swatch(profile, "merino-120tex", 18, 6, &[5.0, 6.0])?;
        let compiled = compile_knit_doc(&doc, profile, None, Some("mat-sha"), None)?;
        Ok((doc, compiled))
    }

    #[test]
    fn tier_b_vendor_handoff_refuses_direct_binary_and_verifies_shima_and_stoll_bundles() {
        for (profile, target, expected_fmt) in [
            (
                MachineProfile::shima_seiki_12e_handoff(),
                TierBProprietaryTarget::ShimaSeikiDat,
                ".dat",
            ),
            (
                MachineProfile::stoll_12e_handoff(),
                TierBProprietaryTarget::StollSintral,
                "SINTRAL",
            ),
        ] {
            assert_eq!(target.withheld_format(), expected_fmt);
            let err = refuse_proprietary_binary_target(&profile, expected_fmt).unwrap_err();
            assert_eq!(err.code, RefusalCode::OpaqueProprietaryTarget);
            assert_eq!(
                err.repairs.first().map(String::as_str),
                Some(
                    "Export verified Knitout v2 + vendor handoff manifest for licensed Shima SDS-ONE APEX / Stoll M1plus compilation."
                )
            );
            assert!(err.repairs.len() >= 2);

            let (doc, compiled) = compile_tier_b_swatch_for_test(&profile).unwrap();
            let handoff = export_tier_b_vendor_handoff(&compiled, &profile, expected_fmt).unwrap();
            assert_eq!(handoff.manifest.schema, VENDOR_HANDOFF_SCHEMA);
            assert_eq!(handoff.manifest.target_vendor, target.vendor_key());
            assert_eq!(handoff.manifest.withheld_binary_format, expected_fmt);
            assert!(!handoff.manifest.machine_executable);
            assert!(!handoff.manifest.hardware_execution_authorized);
            assert!(!handoff.manifest.physical_knitting_validated);
            assert!(!handoff.manifest.fit_claimed);
            assert_eq!(handoff.manifest_sha256, handoff.manifest.manifest_sha256);

            let ctx = KnitSafetyContext {
                wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
                blocked_wale_pitch_mm: 100.0 / doc.gauge.wales_per_100mm,
                source_doc: Some(&doc),
                speed_fraction: Some(0.50),
                takedown_fraction: Some(0.60),
            };
            let report = verify_tier_b_handoff_bundle(&handoff, &profile, &ctx).unwrap();
            assert_eq!(report.program_sha256, handoff.knitout.knitout_sha256);
        }

        // Tier-A Kniterate profile is rejected for Tier-B vendor handoff.
        let shima = MachineProfile::shima_seiki_12e_handoff();
        let (_doc, compiled) = compile_tier_b_swatch_for_test(&shima).unwrap();
        let kniterate = MachineProfile::kniterate_7e();
        assert_eq!(
            export_tier_b_vendor_handoff(&compiled, &kniterate, ".dat")
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );
    }

    #[test]
    fn tier_b_manifest_rejects_tamper_unknown_fields_and_flipped_honesty_booleans() {
        let stoll = MachineProfile::stoll_12e_handoff();
        let (doc, compiled) = compile_tier_b_swatch_for_test(&stoll).unwrap();
        let handoff = export_tier_b_vendor_handoff(&compiled, &stoll, "SINTRAL").unwrap();
        handoff.manifest.verify_integrity().unwrap();

        // Unknown fields rejected by #[serde(deny_unknown_fields)]
        let mut val = serde_json::to_value(&handoff.manifest).unwrap();
        val.as_object_mut()
            .unwrap()
            .insert("extra".to_string(), serde_json::json!(1));
        assert!(serde_json::from_value::<VendorHandoffManifest>(val).is_err());

        // Flipping any of the 4 honesty booleans refuses with VerificationMismatch
        for mutator in [
            (|m: &mut VendorHandoffManifest| m.machine_executable = true)
                as fn(&mut VendorHandoffManifest),
            |m| m.hardware_execution_authorized = true,
            |m| m.physical_knitting_validated = true,
            |m| m.fit_claimed = true,
            |m| m.unresolved_vendor_tables.clear(),
            |m| m.knitout_sha256 = "0".repeat(64),
        ] {
            let mut tampered = handoff.manifest.clone();
            mutator(&mut tampered);
            assert_eq!(
                tampered.verify_integrity().unwrap_err().code,
                RefusalCode::VerificationMismatch
            );
        }

        // Bundle verification catches tampered manifest_json even if struct wasn't modified
        let mut bad_bundle = handoff;
        bad_bundle.manifest_json = bad_bundle.manifest_json.replace("SINTRAL", ".dat");
        let ctx = KnitSafetyContext {
            wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
            blocked_wale_pitch_mm: 100.0 / doc.gauge.wales_per_100mm,
            source_doc: Some(&doc),
            speed_fraction: Some(0.50),
            takedown_fraction: Some(0.60),
        };
        assert_eq!(
            verify_tier_b_handoff_bundle(&bad_bundle, &stoll, &ctx)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
    }
}
