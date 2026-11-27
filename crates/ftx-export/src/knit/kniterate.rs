//! Kniterate KCode (`.kc`) export boundary.
//!
//! KCode is a pass-oriented format, not a line-for-line renaming of Knitout operations.
//! The earlier HEADER/GRIPPER_IN/KNIT text was not that format and cannot be certified
//! by parsing it back with another local parser. Until a real pass scheduler and
//! independent upstream fixtures are implemented, keep verified Knitout available
//! and refuse both direct KCode emission and purported KCode read-back verification.
//!
//! Primary format/backend source: https://github.com/textiles-lab/knitout-backend-kniterate
//! (README and test-files, inspected 2026-11-27).

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{CompiledKnitBundle, KnitMachineCert, MachineBackendKind, MachineProfile};

/// Reserved result type for a future independently verified KCode backend.
///
/// No current exporter constructs this type. Use the verified Knitout bundle for review
/// and separately qualified backend conversion.
#[derive(Clone, Debug)]
pub struct KniterateExportBundle {
    /// Actual pass-oriented KCode text, once an implementation is qualified.
    pub kc_text: String,
    /// SHA-256 of the exact KCode bytes.
    pub kc_sha256: String,
    /// Software verification certificate; never hardware authorization.
    pub cert: KnitMachineCert,
}

fn validate_target(bundle: &CompiledKnitBundle, profile: &MachineProfile) -> Result<(), Refusal> {
    profile.validate()?;
    bundle.cert.verify_integrity()?;
    if profile.backend_kind != MachineBackendKind::KniterateKc {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "Kniterate KCode export requires a KniterateKc machine profile",
        ));
    }
    if profile.needle_count != 252
        || profile.has_yarn_inserting_hooks
        || profile.has_sliders
        || profile.min_carrier_spacing < 2
    {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "Kniterate 7E requires 252 needles, carrier spacing >= 2, grippers and no sliders",
        ));
    }
    if bundle.cert.machine_sha256 != profile.canonical_sha256()? {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "KCode target machine differs from the compiled certificate",
        ));
    }
    if bundle.cert.program_sha256 != bundle.program.canonical_text_sha256()? {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Knitout program differs from the compiled certificate",
        ));
    }
    Ok(())
}

fn unavailable_backend() -> Refusal {
    Refusal::new(
        RefusalCode::UnqualifiedProcess,
        "Kniterate pass-oriented KCode (.kc) generation and read-back are not implemented; a renamed Knitout instruction stream is not a machine file",
    )
    .repair("keep the verified .k Knitout file for review; no .kc artifact has been generated")
    .repair("use the textiles-lab/knitout-backend-kniterate converter with its machine settings and operator checks; conversion does not authorize hardware execution")
}

/// Refuses direct KCode emission until the actual pass format is implemented and checked.
///
/// Verified `.k` output remains available through `export_knitout_v2`.
///
/// # Errors
/// `UnqualifiedProcess` for the unavailable backend. Invalid target profiles, altered
/// programs and mismatched certificates retain their more specific refusal codes.
pub fn export_kniterate_kc(
    bundle: &CompiledKnitBundle,
    profile: &MachineProfile,
) -> Result<KniterateExportBundle, Refusal> {
    validate_target(bundle, profile)?;
    Err(unavailable_backend())
}

/// Refuses to certify arbitrary text as KCode without an implemented independent parser.
///
/// # Errors
/// `UnqualifiedProcess` for the unavailable parser, or the exact target/identity refusal.
pub fn verify_kniterate_kc_roundtrip(
    _kc_text: &str,
    bundle: &CompiledKnitBundle,
    profile: &MachineProfile,
) -> Result<(), Refusal> {
    validate_target(bundle, profile)?;
    Err(unavailable_backend())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knit::export_knitout_v2;
    use ftx_pattern::knit::{KnitoutOp, compile_knit_doc, draft_calibration_swatch};

    #[test]
    fn kniterate_keeps_verified_knitout_without_fabricating_kcode() {
        let profile = MachineProfile::kniterate_7e();
        let doc = draft_calibration_swatch(&profile, "synthetic-yarn", 12, 4, &[6.0]).unwrap();
        let compiled = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
        let knitout = export_knitout_v2(&compiled).unwrap();
        assert!(knitout.knitout_text.starts_with(";!knitout-2\n"));
        assert!(!knitout.cert.hardware_execution_authorized);

        let refusal = export_kniterate_kc(&compiled, &profile).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::UnqualifiedProcess);
        assert!(!refusal.repairs.is_empty());
        for text in [
            "HEADER MACHINE=KNITERATE_7E NEEDLES=252 CARRIER_SPACING=2\nEND\n",
            knitout.knitout_text.as_str(),
            "",
        ] {
            assert_eq!(
                verify_kniterate_kc_roundtrip(text, &compiled, &profile)
                    .unwrap_err()
                    .code,
                RefusalCode::UnqualifiedProcess
            );
        }
    }

    #[test]
    fn kniterate_identity_failures_do_not_become_optional_backend_refusals() {
        let profile = MachineProfile::kniterate_7e();
        let doc = draft_calibration_swatch(&profile, "synthetic-yarn", 12, 4, &[6.0]).unwrap();
        let compiled = compile_knit_doc(&doc, &profile, None, None, None).unwrap();

        let mut changed_profile = profile.clone();
        changed_profile.child_float_cap_mm += 1.0;
        assert_eq!(
            export_kniterate_kc(&compiled, &changed_profile)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );

        let mut changed_program = compiled.clone();
        changed_program
            .program
            .ops
            .push(KnitoutOp::Rack { quarter_pitches: 0 });
        assert_eq!(
            export_kniterate_kc(&changed_program, &profile)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
        assert_eq!(
            export_kniterate_kc(&compiled, &MachineProfile::brother_kh930_ayab())
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
    }
}
