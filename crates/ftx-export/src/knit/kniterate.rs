//! Kniterate 7E `.kc` dialect emitter (`FTX-K07`, open `knitout-backend-kniterate` specification).
//!
//! Strictly enforces Kniterate 7E hardware invariants: `252` needles, gripper carriers (`in`/`out`,
//! NO `inhook`/`releasehook`/`outhook`), `carrier_spacing >= 2`, racking within `±4` integer pitches,
//! and dial stitch settings in `0..=15`.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::knit::{
    CarrierId, CompiledKnitBundle, KnitMachineCert, KnitoutOp, MachineBackendKind, MachineProfile,
    Needle, WearerAgeClass, format_quarter_pitches,
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

fn validate_kniterate_needle(needle: Needle, op_idx: usize) -> Result<(), Refusal> {
    if needle.bed.is_slider() {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "op #{op_idx}: Kniterate 7E has no transfer sliders (got {})",
                needle.knitout_token()
            ),
        ));
    }
    if !(0..252).contains(&needle.index) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "op #{op_idx}: Kniterate 7E needle {} is outside valid bed range 0..252",
                needle.knitout_token()
            ),
        ));
    }
    Ok(())
}

fn format_kniterate_carriers(
    carriers: &[CarrierId],
    allowed: &BTreeSet<&str>,
    op_idx: usize,
) -> Result<String, Refusal> {
    if carriers.is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            format!("op #{op_idx}: Kniterate carrier list cannot be empty"),
        ));
    }
    let mut out = String::new();
    for (i, c) in carriers.iter().enumerate() {
        let s = c.as_str();
        if !allowed.contains(s) {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!("op #{op_idx}: carrier '{s}' is not declared on Kniterate 7E profile"),
            ));
        }
        if i > 0 {
            out.push(',');
        }
        out.push_str(s);
    }
    Ok(out)
}

fn reject_control_breaks(label: &str, text: &str, op_idx: usize) -> Result<(), Refusal> {
    if text.contains(['\n', '\r']) {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            format!("op #{op_idx}: Kniterate {label} cannot contain line breaks"),
        ));
    }
    Ok(())
}

/// Translates a VM-verified [`CompiledKnitBundle`] into open-dialect Kniterate 7E `.kc` text
/// and runs independent read-back verification ([`verify_kniterate_kc_roundtrip`]) before returning.
///
/// # Errors
/// [`RefusalCode::MachineStateInfeasible`] if profile is not Kniterate 7E or contains unsupported
/// slider/hook/fractional-rack/out-of-bounds operations; [`RefusalCode::VerificationMismatch`] if
/// read-back verification disagrees.
pub fn export_kniterate_kc(
    bundle: &CompiledKnitBundle,
    profile: &MachineProfile,
) -> Result<KniterateExportBundle, Refusal> {
    profile.validate()?;
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
    if profile.needle_count != 252
        || profile.has_yarn_inserting_hooks
        || profile.has_sliders
        || profile.min_carrier_spacing < 2
    {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "Kniterate 7E profile must have 252 needles, min_carrier_spacing >= 2, no yarn-inserting hooks, and no sliders",
        ));
    }
    let machine_sha256 = profile.canonical_sha256()?;
    if bundle.cert.machine_sha256 != machine_sha256 {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Kniterate `.kc` certificate machine_sha256 does not match target MachineProfile",
        ));
    }
    let prog_sha256 = bundle.program.canonical_text_sha256()?;
    if bundle.cert.program_sha256 != prog_sha256 {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Kniterate `.kc` program text digest does not match sealed cert.program_sha256",
        ));
    }
    let allowed_carriers: BTreeSet<&str> =
        profile.carrier_ids.iter().map(CarrierId::as_str).collect();

    let mut out = String::new();
    out.push_str("; FrankenTextiles Kniterate 7E (.kc open knitout-backend-kniterate dialect)\n");
    let _ = writeln!(out, "; program_sha256: {}", bundle.cert.program_sha256);
    let _ = writeln!(out, "; cert_sha256: {}", bundle.cert.cert_sha256);
    out.push_str("; hardware_execution_authorized: false\n");
    out.push_str("; physical_knitting_validated: false\n");
    out.push_str("; kniterate_official_firmware_validated: false\n");
    let _ = writeln!(
        out,
        "HEADER MACHINE=KNITERATE_7E NEEDLES=252 CARRIER_SPACING={}",
        profile.min_carrier_spacing
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
                let _ = format_kniterate_carriers(carriers, &allowed_carriers, idx)?;
                for c in carriers {
                    let _ = writeln!(out, "GRIPPER_IN {}", c.as_str());
                }
            }
            KnitoutOp::Out { carriers } => {
                let _ = format_kniterate_carriers(carriers, &allowed_carriers, idx)?;
                for c in carriers {
                    let _ = writeln!(out, "GRIPPER_OUT {}", c.as_str());
                }
            }
            KnitoutOp::Stitch { leading, trailing } => {
                // Convert tenth-dial Knitout stitch numbers to Kniterate dial `0..=15` (refuse out-of-range, never clamp)
                if !(0..=150).contains(leading) || !(0..=150).contains(trailing) {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!(
                            "op #{idx}: Kniterate stitch dial {leading}/{trailing} outside valid tenth-dial range 0..=150 (0.0..=15.0)"
                        ),
                    ));
                }
                let dial = f64::from(*leading) / 10.0;
                let _ = writeln!(out, "STITCH {dial:.1} ; raw={leading}/{trailing}");
            }
            KnitoutOp::Rack { quarter_pitches } => {
                if quarter_pitches % 4 != 0 || quarter_pitches.unsigned_abs() > 16 {
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
                validate_kniterate_needle(*needle, idx)?;
                let cs = format_kniterate_carriers(carriers, &allowed_carriers, idx)?;
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
                validate_kniterate_needle(*needle, idx)?;
                let cs = format_kniterate_carriers(carriers, &allowed_carriers, idx)?;
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
                validate_kniterate_needle(*from, idx)?;
                validate_kniterate_needle(*to, idx)?;
                let cs = format_kniterate_carriers(carriers, &allowed_carriers, idx)?;
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
                validate_kniterate_needle(*from, idx)?;
                validate_kniterate_needle(*to, idx)?;
                let _ = writeln!(out, "XFER {} {}", from.knitout_token(), to.knitout_token());
            }
            KnitoutOp::Miss {
                dir,
                needle,
                carriers,
            } => {
                validate_kniterate_needle(*needle, idx)?;
                let cs = format_kniterate_carriers(carriers, &allowed_carriers, idx)?;
                let _ = writeln!(
                    out,
                    "MISS {} {} {}",
                    dir.knitout_symbol(),
                    needle.knitout_token(),
                    cs
                );
            }
            KnitoutOp::Amiss { needle } => {
                validate_kniterate_needle(*needle, idx)?;
                let _ = writeln!(out, "AMISS {}", needle.knitout_token());
            }
            KnitoutOp::Drop { needle } => {
                validate_kniterate_needle(*needle, idx)?;
                let _ = writeln!(out, "DROP {}", needle.knitout_token());
            }
            KnitoutOp::Pause { note } => {
                let text = note.as_deref().unwrap_or("operator check");
                reject_control_breaks("pause note", text, idx)?;
                let _ = writeln!(out, "PAUSE ; {text}");
            }
            KnitoutOp::Comment { text } => {
                reject_control_breaks("comment", text, idx)?;
                let _ = writeln!(out, "; {text}");
            }
            KnitoutOp::AddRollerAdvance { units } => {
                let _ = writeln!(out, "ROLLER_ADD {units}");
            }
            KnitoutOp::SpeedNumber { speed } => {
                if *speed == 0 {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!("op #{idx}: Kniterate carriage speed must be positive"),
                    ));
                }
                if bundle.cert.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
                    && *speed > 150
                {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!(
                            "op #{idx}: CalibrationSwatchOnly speed {speed} exceeds Kniterate swatch cap 150"
                        ),
                    ));
                }
                let _ = writeln!(out, "SPEED {speed}");
            }
            KnitoutOp::PresserMode { mode } => {
                reject_control_breaks("presser-mode", mode, idx)?;
                let _ = writeln!(out, "; presser-mode {mode}");
            }
            KnitoutOp::CarrierSpacing { needles } => {
                if *needles < 2 {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{idx}: Kniterate carrier spacing {needles} is below required minimum 2"
                        ),
                    ));
                }
                let _ = writeln!(out, "CARRIER_SPACING {needles}");
            }
        }
    }
    out.push_str("END\n");

    verify_kniterate_kc_roundtrip(&out, bundle, profile)?;
    let kc_sha256 = sha256_hex(out.as_bytes());
    Ok(KniterateExportBundle {
        kc_text: out,
        kc_sha256,
        cert: bundle.cert.clone(),
    })
}

fn parse_kc_needle(tok: &str, line_no: usize) -> Result<i32, Refusal> {
    if tok.starts_with("fs") || tok.starts_with("bs") {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            format!("line {line_no}: Kniterate `.kc` read-back rejected slider needle '{tok}'"),
        ));
    }
    let idx_str = tok
        .strip_prefix('f')
        .or_else(|| tok.strip_prefix('b'))
        .ok_or_else(|| {
            Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("line {line_no}: invalid Kniterate needle token '{tok}'"),
            )
        })?;
    let idx: i32 = idx_str.parse().map_err(|_| {
        Refusal::new(
            RefusalCode::VerificationMismatch,
            format!("line {line_no}: invalid Kniterate needle index '{tok}'"),
        )
    })?;
    if !(0..252).contains(&idx) {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            format!("line {line_no}: Kniterate needle '{tok}' outside 0..252"),
        ));
    }
    Ok(idx)
}

fn verify_kc_carriers(tok: &str, allowed: &BTreeSet<&str>, line_no: usize) -> Result<(), Refusal> {
    if tok.is_empty() {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            format!("line {line_no}: empty carrier token in `.kc`"),
        ));
    }
    for c in tok.split(',') {
        if !allowed.contains(c) {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("line {line_no}: undeclared Kniterate carrier '{c}' in `.kc`"),
            ));
        }
    }
    Ok(())
}

/// Independent `.kc` read-back parser & hardware-envelope verifier (`FTX-K07`).
///
/// Parses every line of the emitted Kniterate 7E `.kc` text, checks headers, forbidden hook/slider
/// opcodes, needle bounds `0..252`, carrier membership, racking `±4`, stitch dial `0.0..=15.0`,
/// carrier spacing `>= 2`, speed bounds, exact command count parity against `bundle.program.ops`,
/// and clean `END` termination.
///
/// # Errors
/// [`RefusalCode::VerificationMismatch`] if any `.kc` invariant or round-trip parity check fails.
pub fn verify_kniterate_kc_roundtrip(
    kc_text: &str,
    bundle: &CompiledKnitBundle,
    profile: &MachineProfile,
) -> Result<(), Refusal> {
    let allowed: BTreeSet<&str> = profile.carrier_ids.iter().map(CarrierId::as_str).collect();
    let expected_prog_comment = format!("; program_sha256: {}", bundle.cert.program_sha256);
    let expected_cert_comment = format!("; cert_sha256: {}", bundle.cert.cert_sha256);
    let expected_header = format!(
        "HEADER MACHINE=KNITERATE_7E NEEDLES=252 CARRIER_SPACING={}",
        profile.min_carrier_spacing
    );

    let mut seen_prog = false;
    let mut seen_cert = false;
    let mut seen_no_hw = false;
    let mut seen_no_phys = false;
    let mut seen_no_fw = false;
    let mut seen_header = false;
    let mut seen_end = false;
    let mut command_count = 0usize;

    for (idx, raw_line) in kc_text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if seen_end {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("line {line_no}: unexpected content after `.kc` END terminator"),
            ));
        }
        if line == expected_prog_comment {
            seen_prog = true;
            continue;
        }
        if line == expected_cert_comment {
            seen_cert = true;
            continue;
        }
        if line == "; hardware_execution_authorized: false" {
            seen_no_hw = true;
            continue;
        }
        if line == "; physical_knitting_validated: false" {
            seen_no_phys = true;
            continue;
        }
        if line == "; kniterate_official_firmware_validated: false" {
            seen_no_fw = true;
            continue;
        }
        if line.starts_with(';') {
            continue;
        }
        let code = line.split(';').next().unwrap_or("").trim();
        if code == expected_header {
            if seen_header {
                return Err(Refusal::new(
                    RefusalCode::VerificationMismatch,
                    format!("line {line_no}: duplicate `.kc` HEADER directive"),
                ));
            }
            seen_header = true;
            continue;
        }
        if !seen_header {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("line {line_no}: `.kc` command appeared before HEADER"),
            ));
        }
        if code == "END" {
            seen_end = true;
            continue;
        }
        let parts: Vec<&str> = code.split_whitespace().collect();
        let Some(&cmd) = parts.first() else {
            continue;
        };
        command_count += 1;
        match cmd {
            "SPEED" => {
                if parts.len() != 2 {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed SPEED directive"),
                    ));
                }
                let spd: u32 = parts[1].parse().map_err(|_| {
                    Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: invalid SPEED value '{}'", parts[1]),
                    )
                })?;
                if spd == 0
                    || (bundle.cert.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
                        && spd > 150)
                {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: invalid Kniterate SPEED {spd}"),
                    ));
                }
            }
            "ROLLER" | "ROLLER_ADD" => {
                if parts.len() != 2 || parts[1].parse::<i32>().is_err() {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed {cmd} directive"),
                    ));
                }
            }
            "GRIPPER_IN" | "GRIPPER_OUT" => {
                if parts.len() != 2 {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed {cmd} directive"),
                    ));
                }
                verify_kc_carriers(parts[1], &allowed, line_no)?;
            }
            "STITCH" => {
                if parts.len() != 2 {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed STITCH directive"),
                    ));
                }
                let dial: f64 = parts[1].parse().map_err(|_| {
                    Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: invalid STITCH dial '{}'", parts[1]),
                    )
                })?;
                if !dial.is_finite() || !(0.0..=15.0).contains(&dial) {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: Kniterate STITCH dial {dial} outside 0..=15"),
                    ));
                }
            }
            "RACK" => {
                if parts.len() != 2 {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed RACK directive"),
                    ));
                }
                let rack: i32 = parts[1].parse().map_err(|_| {
                    Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: invalid RACK value '{}'", parts[1]),
                    )
                })?;
                if !(-4..=4).contains(&rack) {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: Kniterate RACK {rack} outside -4..=4"),
                    ));
                }
            }
            "KNIT" | "TUCK" | "MISS" => {
                if parts.len() != 4 || !matches!(parts[1], "+" | "-") {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed {cmd} directive"),
                    ));
                }
                parse_kc_needle(parts[2], line_no)?;
                verify_kc_carriers(parts[3], &allowed, line_no)?;
            }
            "SPLIT" => {
                if parts.len() != 5 || !matches!(parts[1], "+" | "-") {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed SPLIT directive"),
                    ));
                }
                parse_kc_needle(parts[2], line_no)?;
                parse_kc_needle(parts[3], line_no)?;
                verify_kc_carriers(parts[4], &allowed, line_no)?;
            }
            "XFER" => {
                if parts.len() != 3 {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed XFER directive"),
                    ));
                }
                parse_kc_needle(parts[1], line_no)?;
                parse_kc_needle(parts[2], line_no)?;
            }
            "AMISS" | "DROP" => {
                if parts.len() != 2 {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed {cmd} directive"),
                    ));
                }
                parse_kc_needle(parts[1], line_no)?;
            }
            "PAUSE" => {
                if parts.len() != 1 {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed PAUSE directive"),
                    ));
                }
            }
            "CARRIER_SPACING" => {
                if parts.len() != 2 {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: malformed CARRIER_SPACING directive"),
                    ));
                }
                let spacing: u32 = parts[1].parse().map_err(|_| {
                    Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: invalid CARRIER_SPACING '{}'", parts[1]),
                    )
                })?;
                if spacing < 2 {
                    return Err(Refusal::new(
                        RefusalCode::VerificationMismatch,
                        format!("line {line_no}: CARRIER_SPACING {spacing} < 2"),
                    ));
                }
            }
            other => {
                return Err(Refusal::new(
                    RefusalCode::VerificationMismatch,
                    format!("line {line_no}: unsupported Kniterate `.kc` command '{other}'"),
                ));
            }
        }
    }

    if !seen_prog
        || !seen_cert
        || !seen_no_hw
        || !seen_no_phys
        || !seen_no_fw
        || !seen_header
        || !seen_end
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "Kniterate `.kc` read-back missing mandatory header, safety disclaimers, or END terminator",
        ));
    }

    // Account for default SPEED + ROLLER preamble plus every emitted command from bundle.program.ops.
    let expected_cmds: usize = 2 + bundle
        .program
        .ops
        .iter()
        .map(|op| match op {
            KnitoutOp::In { carriers } | KnitoutOp::Out { carriers } => carriers.len(),
            KnitoutOp::Comment { .. } | KnitoutOp::PresserMode { .. } => 0,
            _ => 1,
        })
        .sum::<usize>();
    if command_count != expected_cmds {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            format!(
                "Kniterate `.kc` read-back command count {command_count} != expected {expected_cmds}"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_pattern::knit::{compile_knit_doc, draft_calibration_swatch};

    #[test]
    fn kniterate_kc_roundtrip_passes_and_rejects_tampered_commands() {
        let kniterate = MachineProfile::kniterate_7e();
        let doc = draft_calibration_swatch(&kniterate, "wool-200tex", 16, 6, &[6.0, 8.0]).unwrap();
        let compiled = compile_knit_doc(&doc, &kniterate, None, None, None).unwrap();
        let kc = export_kniterate_kc(&compiled, &kniterate).unwrap();
        assert!(
            kc.kc_text
                .contains("; kniterate_official_firmware_validated: false")
        );
        verify_kniterate_kc_roundtrip(&kc.kc_text, &compiled, &kniterate).unwrap();

        // Tampered slider needle fails read-back.
        let slider_kc = kc.kc_text.replacen("KNIT + f", "KNIT + fs", 1);
        assert_eq!(
            verify_kniterate_kc_roundtrip(&slider_kc, &compiled, &kniterate)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );

        // Tampered stitch dial > 15 fails read-back.
        let bad_dial = kc.kc_text.replacen("STITCH 6.0", "STITCH 19.5", 1);
        assert_eq!(
            verify_kniterate_kc_roundtrip(&bad_dial, &compiled, &kniterate)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );

        // Missing END fails read-back.
        let truncated = kc.kc_text.trim_end_matches("END\n");
        assert_eq!(
            verify_kniterate_kc_roundtrip(truncated, &compiled, &kniterate)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );

        // Unauthorized inhook command injected fails read-back.
        let injected = kc.kc_text.replacen("GRIPPER_IN 1", "INHOOK 1", 1);
        assert_eq!(
            verify_kniterate_kc_roundtrip(&injected, &compiled, &kniterate)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
    }
}
