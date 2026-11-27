//! Kniterate 7E `.kc` dialect emitter (`FTX-K07`, open `knitout-backend-kniterate` specification).
//!
//! Strictly enforces Kniterate 7E hardware invariants: `252` needles, gripper carriers (`in`/`out`,
//! NO `inhook`/`releasehook`/`outhook`), `carrier_spacing >= 2`, racking within `±4` integer pitches,
//! and dial stitch settings in `0..=15`.

use std::fmt::Write as _;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    CompiledKnitBundle, KnitMachineCert, KnitoutOp, MachineBackendKind, MachineProfile,
    WearerAgeClass, format_quarter_pitches,
};

/// Verified Kniterate `.kc` export bundle.
#[derive(Clone, Debug)]
pub struct KniterateExportBundle {
    /// Deterministic Kniterate `.kc` command text (`knitout-backend-kniterate` dialect).
    pub kc_text: String,
    /// SHA-256 of `kc_text`.
    pub kc_sha256: String,
    /// Sealed `ftx-machine-cert/1` (`hardware_execution_authorized: false`).
    pub cert: KnitMachineCert,
}

/// Translates a VM-verified [`CompiledKnitBundle`] into open-dialect Kniterate 7E `.kc` text.
///
/// # Errors
/// [`RefusalCode::MachineStateInfeasible`] if profile is not Kniterate 7E or contains unsupported
/// slider/hook/fractional-rack operations.
pub fn export_kniterate_kc(
    bundle: &CompiledKnitBundle,
    profile: &MachineProfile,
) -> Result<KniterateExportBundle, Refusal> {
    bundle.cert.verify_integrity()?;
    if profile.backend_kind != MachineBackendKind::KniterateKc {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "Kniterate `.kc` emitter requires a KniterateKc machine profile (got {:?})",
                profile.backend_kind
            ),
        ));
    }
    if profile.needle_count != 252 || profile.has_yarn_inserting_hooks || profile.has_sliders {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "Kniterate 7E profile must have 252 needles, no yarn-inserting hooks, and no sliders",
        ));
    }

    let mut out = String::new();
    out.push_str("; FrankenTextiles Kniterate 7E (.kc open knitout-backend-kniterate dialect)\n");
    let _ = writeln!(out, "; program_sha256: {}", bundle.cert.program_sha256);
    let _ = writeln!(out, "; cert_sha256: {}", bundle.cert.cert_sha256);
    out.push_str("; hardware_execution_authorized: false\n");
    out.push_str("; physical_knitting_validated: false\n");
    let _ = writeln!(
        out,
        "HEADER MACHINE=KNITERATE_7E NEEDLES=252 CARRIER_SPACING={}",
        profile.min_carrier_spacing.max(2)
    );

    let default_speed = if bundle.cert.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        150
    } else {
        300
    };
    let default_roller = if bundle.cert.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        120
    } else {
        200
    };
    let _ = writeln!(out, "SPEED {default_speed}");
    let _ = writeln!(out, "ROLLER {default_roller}");

    for (idx, op) in bundle.program.ops.iter().enumerate() {
        match op {
            KnitoutOp::InHook { .. }
            | KnitoutOp::ReleaseHook { .. }
            | KnitoutOp::OutHook { .. } => {
                return Err(Refusal::new(
                    RefusalCode::MachineStateInfeasible,
                    format!(
                        "op #{idx}: Kniterate does not support motorized yarn-inserting hooks (`inhook`/`releasehook`/`outhook`)"
                    ),
                ));
            }
            KnitoutOp::In { carriers } => {
                for c in carriers {
                    let _ = writeln!(out, "GRIPPER_IN {}", c.as_str());
                }
            }
            KnitoutOp::Out { carriers } => {
                for c in carriers {
                    let _ = writeln!(out, "GRIPPER_OUT {}", c.as_str());
                }
            }
            KnitoutOp::Stitch { leading, trailing } => {
                // Convert tenth-dial Knitout stitch numbers to Kniterate dial `0..=15`
                let dial = (f64::from(*leading) / 10.0).clamp(0.0, 15.0);
                let _ = writeln!(out, "STITCH {dial:.1} ; raw={leading}/{trailing}");
            }
            KnitoutOp::Rack { quarter_pitches } => {
                if quarter_pitches % 4 != 0 || quarter_pitches.abs() > 16 {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{idx}: Kniterate `.kc` supports integer racking in -4..=4 pitches (got {})",
                            format_quarter_pitches(*quarter_pitches)
                        ),
                    ));
                }
                let _ = writeln!(out, "RACK {}", quarter_pitches / 4);
            }
            KnitoutOp::Knit {
                dir,
                needle,
                carriers,
            } => {
                let cs = carriers
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = writeln!(
                    out,
                    "KNIT {} {} {}",
                    dir.knitout_symbol(),
                    needle.knitout_token(),
                    cs
                );
            }
            KnitoutOp::Tuck {
                dir,
                needle,
                carriers,
            } => {
                let cs = carriers
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = writeln!(
                    out,
                    "TUCK {} {} {}",
                    dir.knitout_symbol(),
                    needle.knitout_token(),
                    cs
                );
            }
            KnitoutOp::Split {
                dir,
                from,
                to,
                carriers,
            } => {
                let cs = carriers
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = writeln!(
                    out,
                    "SPLIT {} {} {} {}",
                    dir.knitout_symbol(),
                    from.knitout_token(),
                    to.knitout_token(),
                    cs
                );
            }
            KnitoutOp::Xfer { from, to } => {
                let _ = writeln!(out, "XFER {} {}", from.knitout_token(), to.knitout_token());
            }
            KnitoutOp::Miss {
                dir,
                needle,
                carriers,
            } => {
                let cs = carriers
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = writeln!(
                    out,
                    "MISS {} {} {}",
                    dir.knitout_symbol(),
                    needle.knitout_token(),
                    cs
                );
            }
            KnitoutOp::Amiss { needle } => {
                let _ = writeln!(out, "AMISS {}", needle.knitout_token());
            }
            KnitoutOp::Drop { needle } => {
                let _ = writeln!(out, "DROP {}", needle.knitout_token());
            }
            KnitoutOp::Pause { note } => {
                let _ = writeln!(
                    out,
                    "PAUSE ; {}",
                    note.as_deref().unwrap_or("operator check")
                );
            }
            KnitoutOp::Comment { text } => {
                let _ = writeln!(out, "; {text}");
            }
            KnitoutOp::AddRollerAdvance { units } => {
                let _ = writeln!(out, "ROLLER_ADD {units}");
            }
            KnitoutOp::SpeedNumber { speed } => {
                let capped =
                    if bundle.cert.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
                        (*speed).min(150)
                    } else {
                        *speed
                    };
                let _ = writeln!(out, "SPEED {capped}");
            }
            KnitoutOp::PresserMode { mode } => {
                let _ = writeln!(out, "; presser-mode {mode}");
            }
            KnitoutOp::CarrierSpacing { needles } => {
                let _ = writeln!(out, "CARRIER_SPACING {}", (*needles).max(2));
            }
        }
    }
    out.push_str("END\n");

    let kc_sha256 = sha256_hex(out.as_bytes());
    Ok(KniterateExportBundle {
        kc_text: out,
        kc_sha256,
        cert: bundle.cert.clone(),
    })
}
