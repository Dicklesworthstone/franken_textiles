//! Continuous yarn-dyeing physical hazard verifier & deterministic machine emitter (`FTX-K16`).
//!
//! Enforces:
//! 1. Sequential Euler–Eytelwein Capstan tension cascade vs dry & wet-tenacity breaking limits;
//! 2. Thermoplastic `T_g` creep margin (`oven_temp_c <= yarn_tg_c - 15 C`);
//! 3. Dual-sided thermal fixation / scorch dwell window (`tau_fix <= L/v_max <= L/v_min <= tau_scorch`);
//! 4. Sacrificial startup/shutdown ramp leaders covering `applicator_to_feeder_path_m`;
//! 5. Regime III P-trimmed dancer-arm accumulator ODE bounds (`f_min*L_cap <= L_buf(t) <= f_max*L_cap`)
//!    across knit passes, reversals, transfers, racks, and operator pauses;
//! 6. Child/infant wash-off, ISO 105-C06, ISO 105-X12, and saliva fastness gates;
//! 7. Deterministic `.csv`, `.gcode`, and sealed `ftx-dye-cert/1` (`hardware_execution_authorized: false`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use ftx_core::receipt::{canonical_json_sha256, sha256_hex};
use ftx_core::{Refusal, RefusalCode, det};
use ftx_pattern::dye::{
    DYE_PROGRAM_SCHEMA, DyeMachineProfile, DyePlanUncertaintyReceipt, DyeSegment, DyeingRegime,
    OpticalMeasurementBasis, YarnDyeProgram, YarnDyeingQualification,
};
use ftx_pattern::knit::{CalibrationSwatchCaps, FourClaimsVerdict, WearerAgeClass};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

/// Schema tag for sealed yarn-dyeing machine certificates.
pub const DYE_CERT_SCHEMA: &str = "ftx-dye-cert/1";

/// Physical hazard verification telemetry for one [`YarnDyeProgram`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyeHazardReport {
    /// Peak predicted dry-zone tension along capstan guides, in Newtons.
    pub peak_dry_tension_n: f64,
    /// Peak predicted wet-zone tension along capstan guides, in Newtons.
    pub peak_wet_tension_n: f64,
    /// Minimum wet-tenacity safety factor (`wet_breaking_n / peak_wet_tension_n`).
    pub min_wet_safety_factor: f64,
    /// Minimum oven fixation dwell time across all segments, in seconds.
    pub min_oven_dwell_s: f64,
    /// Maximum oven scorch dwell time across all segments, in seconds.
    pub max_oven_dwell_s: f64,
    /// Minimum and maximum dancer-arm accumulator buffer fill `[min_m, max_m]` (Regime III only).
    pub accumulator_buffer_range_m: Option<[f64; 2]>,
    /// Ordered list of all passed physical hazard checks.
    pub checks_passed: Vec<String>,
}

#[derive(Serialize)]
struct DyeCertSealView<'a> {
    schema: &'a str,
    program_sha256: &'a str,
    machine_sha256: &'a str,
    qualification_sha256: &'a Option<String>,
    uncertainty_receipt_sha256: &'a Option<String>,
    wearer_age_class: WearerAgeClass,
    hardware_execution_authorized: bool,
    physical_dyeing_validated: bool,
    hazard_report: &'a DyeHazardReport,
    four_claims: &'a FourClaimsVerdict,
}

/// Sealed `ftx-dye-cert/1` continuous dyeing machine certificate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyeMachineCert {
    /// Must equal [`DYE_CERT_SCHEMA`].
    pub schema: String,
    /// Canonical SHA-256 of the verified [`YarnDyeProgram`].
    pub program_sha256: String,
    /// Canonical SHA-256 of the [`DyeMachineProfile`].
    pub machine_sha256: String,
    /// Optional SHA-256 of [`YarnDyeingQualification`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualification_sha256: Option<String>,
    /// Optional SHA-256 of sealed Seam-4 [`DyePlanUncertaintyReceipt`] (`FTX-K13`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncertainty_receipt_sha256: Option<String>,
    /// Required wearer age class.
    pub wearer_age_class: WearerAgeClass,
    /// ALWAYS `false`: software verification never authorizes unattended dye-rig execution.
    pub hardware_execution_authorized: bool,
    /// ALWAYS `false`: software verification never proves physical color/fastness without Exp-4 wash test.
    pub physical_dyeing_validated: bool,
    /// Verified physical hazard report.
    pub hazard_report: DyeHazardReport,
    /// Four-Claims separation record (`fit_claimed: false`).
    pub four_claims: FourClaimsVerdict,
    /// Sealed canonical SHA-256 over all fields above.
    pub cert_sha256: String,
}

impl DyeMachineCert {
    /// Verifies seal integrity and constitutional no-claim booleans on `ftx-dye-cert/1`.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] on wrong schema or [`RefusalCode::VerificationMismatch`]
    /// if tampered or if safety booleans were flipped to `true`.
    pub fn verify_integrity(&self) -> Result<(), Refusal> {
        if self.schema != DYE_CERT_SCHEMA {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("unexpected dye cert schema '{}'", self.schema),
            ));
        }
        if self.hardware_execution_authorized
            || self.physical_dyeing_validated
            || self.four_claims.fit_claimed
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "dye machine certificate violates constitutional no-claim invariants (hardware_execution_authorized, physical_dyeing_validated, and fit_claimed must be false)",
            ));
        }
        let view = DyeCertSealView {
            schema: &self.schema,
            program_sha256: &self.program_sha256,
            machine_sha256: &self.machine_sha256,
            qualification_sha256: &self.qualification_sha256,
            uncertainty_receipt_sha256: &self.uncertainty_receipt_sha256,
            wearer_age_class: self.wearer_age_class,
            hardware_execution_authorized: self.hardware_execution_authorized,
            physical_dyeing_validated: self.physical_dyeing_validated,
            hazard_report: &self.hazard_report,
            four_claims: &self.four_claims,
        };
        let expected = canonical_json_sha256(&view)?;
        if expected != self.cert_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "dye machine certificate cert_sha256 mismatch (tampered)",
            ));
        }
        Ok(())
    }
}

/// Complete exported yarn-dyeing controller bundle.
#[derive(Clone, Debug)]
pub struct DyeExportBundle {
    /// Deterministic tabular schedule CSV (`s_start_m,s_end_m,speed_m_s,fiducial,dose_0,...`).
    pub schedule_csv: String,
    /// Deterministic motion/dosing controller G-code (`G1` arc advance + `M` channel valve commands).
    pub controller_gcode: String,
    /// Sealed `ftx-dye-cert/1` certificate.
    pub cert: DyeMachineCert,
    /// Pretty JSON of `cert`.
    pub cert_json: String,
    /// SHA-256 of `controller_gcode`.
    pub gcode_sha256: String,
}

/// Complete `Exp-4` stepped bleed, shrinkage, tenacity, and colorfastness calibration swatch kit (`FTX-K16`).
#[derive(Clone, Debug)]
pub struct DyeBleedSwatchBundle {
    /// Hard-capped `CalibrationSwatchOnly` (`not_for_garment_wear: true`, `<= 40 m`) stepped dye program.
    pub program: YarnDyeProgram,
    /// Verified CSV, G-code, and sealed `ftx-dye-cert/1` bundle.
    pub export: DyeExportBundle,
    /// Blank `YarnDyeingQualification` template bound to exact `material_sha256` and `dye_machine_sha256`.
    pub qualification_template: YarnDyeingQualification,
    /// Deterministic JSON of `qualification_template`.
    pub qualification_template_json: String,
}

/// Verifies every physical hazard invariant of a [`YarnDyeProgram`] against [`DyeMachineProfile`]
/// and [`YarnDyeingQualification`], then emits deterministic `.csv`, `.gcode`, and `ftx-dye-cert/1`.
///
/// # Errors
/// Fails closed with [`RefusalCode::MaterialProcessHazard`], [`RefusalCode::UnqualifiedProcess`],
/// or [`RefusalCode::VerificationMismatch`] on any hazard or missing qualification.
pub fn verify_and_export_dye_program(
    program: &YarnDyeProgram,
    machine: &DyeMachineProfile,
    qualification: Option<&YarnDyeingQualification>,
    expected_material_sha256: Option<&str>,
) -> Result<DyeExportBundle, Refusal> {
    verify_and_export_dye_program_with_receipt(
        program,
        machine,
        qualification,
        expected_material_sha256,
        None,
    )
}

/// Verifies every physical hazard invariant of a [`YarnDyeProgram`] against [`DyeMachineProfile`],
/// [`YarnDyeingQualification`], and an optional sealed Seam-4 [`DyePlanUncertaintyReceipt`] (`FTX-K13`),
/// then emits deterministic `.csv`, `.gcode`, and `ftx-dye-cert/1` stamped with `uncertainty_receipt_sha256`.
///
/// # Errors
/// Fails closed with [`RefusalCode::MaterialProcessHazard`], [`RefusalCode::UnqualifiedProcess`],
/// or [`RefusalCode::VerificationMismatch`] on any hazard, missing qualification, or tampered receipt.
pub fn verify_and_export_dye_program_with_receipt(
    program: &YarnDyeProgram,
    machine: &DyeMachineProfile,
    qualification: Option<&YarnDyeingQualification>,
    expected_material_sha256: Option<&str>,
    uncertainty_receipt: Option<&DyePlanUncertaintyReceipt>,
) -> Result<DyeExportBundle, Refusal> {
    program.validate()?;
    machine.validate()?;
    let machine_sha256 = machine.canonical_sha256()?;
    if program.machine_sha256 != machine_sha256 {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "dye program machine_sha256 does not match target DyeMachineProfile",
        ));
    }

    // Qualification & wearer child/infant fastness gate
    let (qual_sha, dry_break_n, wet_break_n, yarn_tg_c) = if program.wearer_age_class
        == WearerAgeClass::CalibrationSwatchOnly
    {
        if let Some(q) = qualification {
            let mat_sha = expected_material_sha256.unwrap_or(&q.material_sha256);
            q.validate_for_wearer(
                WearerAgeClass::CalibrationSwatchOnly,
                mat_sha,
                &machine_sha256,
            )?;
            let dry = q.yarn_breaking_force_n;
            let wet = dry * q.wet_tenacity_retention_ratio;
            (Some(q.canonical_sha256()?), dry, wet, q.yarn_tg_c)
        } else {
            // Conservative fail-closed swatch baseline (explicitly stamped unvalidated)
            (None, 6.0, 4.0, None)
        }
    } else {
        let Some(q) = qualification else {
            return Err(Refusal::new(
                RefusalCode::UnqualifiedProcess,
                "wearable dyed garment requires a qualified YarnDyeingQualification (Exp-4)",
            )
            .repair("run an Exp-4 bleed/shrinkage/fastness coupon test and pass `--dye-qual`"));
        };
        let mat_sha = expected_material_sha256.unwrap_or(&q.material_sha256);
        q.validate_for_wearer(program.wearer_age_class, mat_sha, &machine_sha256)?;
        let dry = q.yarn_breaking_force_n;
        let wet = dry * q.wet_tenacity_retention_ratio;
        (Some(q.canonical_sha256()?), dry, wet, q.yarn_tg_c)
    };
    if let Some(bound_qual_sha) = &program.qualification_sha256
        && qual_sha.as_deref() != Some(bound_qual_sha.as_str())
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "dye program qualification_sha256 does not match supplied YarnDyeingQualification",
        ));
    }
    if program.name.contains(['\n', '\r']) || machine.id.contains(['\n', '\r']) {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "dye program name and machine id cannot contain line breaks",
        ));
    }
    for p in &machine.primaries {
        if p.id.contains(['\n', '\r', ',']) {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "dye primary id '{}' cannot contain commas or line breaks",
                    p.id
                ),
            ));
        }
    }

    if program.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        let last_end_m = program.segments.last().map_or(0.0, |s| s.s_end_m);
        let total_yarn_m = program.startup_leader_m + last_end_m + program.shutdown_trailer_m;
        if total_yarn_m > CalibrationSwatchCaps::MAX_YARN_M + 1e-9 {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "calibration dye swatch total yarn length {total_yarn_m:.2} m exceeds hard cap {:.1} m",
                    CalibrationSwatchCaps::MAX_YARN_M
                ),
            ));
        }
    }

    // 1. Sequential Euler-Eytelwein Capstan tension cascade T_{k+1} = T_k * exp(mu_k * theta_k)
    let mut tension = machine.entry_tension_n;
    let mut peak_dry = tension;
    let mut peak_wet = 0.0_f64;
    for pin in &machine.guide_capstan_pins {
        tension *= det::exp(pin.mu_friction * pin.wrap_angle_rad);
        if pin.wet_zone {
            peak_wet = peak_wet.max(tension);
        } else {
            peak_dry = peak_dry.max(tension);
        }
    }
    let min_wet_safety_factor = if peak_wet > 1e-12 {
        wet_break_n / peak_wet
    } else {
        f64::INFINITY
    };
    if peak_dry > 0.65 * dry_break_n {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "Capstan dry yarn tension {peak_dry:.2} N exceeds 65% of breaking force ({dry_break_n:.2} N)"
            ),
        ));
    }
    if peak_wet > 0.55 * wet_break_n {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "Capstan wet yarn tension {peak_wet:.2} N exceeds 55% of wet breaking force ({wet_break_n:.2} N)"
            ),
        ));
    }

    // 2. Thermoplastic T_g creep guard
    if let Some(tg) = yarn_tg_c
        && machine.oven_temp_c > tg - 15.0
    {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "oven temperature {:.1} C violates thermoplastic creep guard (yarn T_g = {tg:.1} C, max safe {:.1} C)",
                machine.oven_temp_c,
                tg - 15.0
            ),
        ));
    }

    // 3. Dual-sided thermal dwell & dose limits across every segment
    let req_fix_dwell = machine
        .primaries
        .iter()
        .map(|p| p.min_fixation_dwell_s)
        .fold(0.0_f64, f64::max);
    let mut min_dwell = f64::INFINITY;
    let mut max_dwell = 0.0_f64;
    let mut max_speed_seen = 0.0_f64;
    for (idx, seg) in program.segments.iter().enumerate() {
        if seg.target_speed_m_s < machine.min_line_speed_m_s
            || seg.target_speed_m_s > machine.max_line_speed_m_s
        {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "segment {idx} speed {:.4} m/s outside machine envelope [{:.4}, {:.4}] m/s",
                    seg.target_speed_m_s, machine.min_line_speed_m_s, machine.max_line_speed_m_s
                ),
            ));
        }
        max_speed_seen = max_speed_seen.max(seg.target_speed_m_s);
        let dwell = machine.oven_length_m / seg.target_speed_m_s;
        min_dwell = min_dwell.min(dwell);
        max_dwell = max_dwell.max(dwell);
        if dwell < req_fix_dwell {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "segment {idx} oven dwell {dwell:.2}s < required fixation {req_fix_dwell:.2}s"
                ),
            ));
        }
        if dwell > machine.scorch_max_dwell_s {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "segment {idx} oven dwell {dwell:.2}s > scorch cap {:.2}s",
                    machine.scorch_max_dwell_s
                ),
            ));
        }
        if seg.doses.len() != machine.primaries.len() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("segment {idx} dose count disagrees with machine primary count"),
            ));
        }
        for (p, &d) in seg.doses.iter().enumerate() {
            if d > machine.primaries[p].max_dose_per_m + 1e-9 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "segment {idx} primary '{}' dose {d:.4} exceeds max {:.4}",
                        machine.primaries[p].id, machine.primaries[p].max_dose_per_m
                    ),
                ));
            }
        }
    }

    // 4. Sacrificial startup/shutdown ramp leader check
    let ramp_len = (max_speed_seen * max_speed_seen) / (2.0 * machine.max_line_accel_m_s2);
    let min_leader = ramp_len.max(machine.applicator_to_feeder_path_m);
    if program.startup_leader_m + 1e-9 < min_leader {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "startup_leader_m {:.3} m is shorter than required ramp/path leader {:.3} m",
                program.startup_leader_m, min_leader
            ),
        ));
    }
    if program.shutdown_trailer_m + 1e-9 < ramp_len {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "shutdown_trailer_m {:.3} m is shorter than required deceleration ramp {:.3} m",
                program.shutdown_trailer_m, ramp_len
            ),
        ));
    }

    // 5. Regime II hardware sensor gate & Regime III P-trimmed dancer-arm accumulator ODE verification
    if program.regime == DyeingRegime::Regime2FiducialSynced && !machine.has_optical_fiducial_sensor
    {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "Regime II fiducial-synced dyeing refused: machine lacks optical fiducial sensor",
        ));
    }
    let accumulator_buffer_range_m = if program.regime == DyeingRegime::Regime3InlineDancer {
        let Some(acc) = &machine.accumulator else {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "Regime III inline dyeing refused: machine profile lacks dancer-arm accumulator",
            ));
        };
        let tl = program.regime3_intake_timeline.as_ref().ok_or_else(|| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                "Regime III requires regime3_intake_timeline",
            )
        })?;
        let lo_m = acc.min_fill_fraction * acc.capacity_m;
        let hi_m = acc.max_fill_fraction * acc.capacity_m;
        let mut buf = acc.initial_buffer_preload_m;
        let mut v_dye = 0.5 * (machine.min_line_speed_m_s + machine.max_line_speed_m_s);
        let mut min_buf = buf;
        let mut max_buf = buf;

        const MAX_TOTAL_ACCUMULATOR_STEPS: usize = 5_000_000;
        let mut total_steps: usize = 0;
        for (ev_idx, ev) in tl.events.iter().enumerate() {
            let raw_steps = (ev.duration_s / tl.dt_verify_s).ceil().max(1.0);
            if !raw_steps.is_finite() || raw_steps > 1_000_000.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "Regime III intake event #{ev_idx} requires too many verification substeps ({raw_steps:.0} > 1_000_000)"
                    ),
                ));
            }
            let steps = raw_steps as usize;
            total_steps = total_steps.saturating_add(steps);
            if total_steps > MAX_TOTAL_ACCUMULATOR_STEPS {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "Regime III intake timeline exceeds {MAX_TOTAL_ACCUMULATOR_STEPS} total verification substeps"
                    ),
                ));
            }
            let dt = ev.duration_s / (steps as f64);
            for _ in 0..steps {
                // Proportional trim controller steering dye line speed inside [v_min, v_max] with accel cap
                let v_cmd = (ev.yarn_intake_speed_m_s
                    + acc.dancer_kp_s_inv * (acc.target_buffer_m - buf))
                    .clamp(machine.min_line_speed_m_s, machine.max_line_speed_m_s);
                let max_dv = machine.max_line_accel_m_s2 * dt;
                v_dye += (v_cmd - v_dye).clamp(-max_dv, max_dv);
                v_dye = v_dye.clamp(machine.min_line_speed_m_s, machine.max_line_speed_m_s);
                buf += (v_dye - ev.yarn_intake_speed_m_s) * dt;
                min_buf = min_buf.min(buf);
                max_buf = max_buf.max(buf);
                if buf < lo_m || buf > hi_m {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!(
                            "Regime III dancer accumulator breached [{lo_m:.3}, {hi_m:.3}] m during {:?} event #{ev_idx} (buffer={buf:.3} m)",
                            ev.kind
                        ),
                    )
                    .repair("increase dancer capacity, shorten zero-feed pause/transfer dwells, or switch to offline Regime I/II/IV"));
                }
            }
        }
        Some([min_buf, max_buf])
    } else {
        None
    };

    let hazard_report = DyeHazardReport {
        peak_dry_tension_n: peak_dry,
        peak_wet_tension_n: peak_wet,
        min_wet_safety_factor: if min_wet_safety_factor.is_finite() {
            min_wet_safety_factor
        } else {
            999.0
        },
        min_oven_dwell_s: min_dwell,
        max_oven_dwell_s: max_dwell,
        accumulator_buffer_range_m,
        checks_passed: vec![
            "1_capstan_dry_and_wet_tenacity".to_string(),
            "2_thermoplastic_tg_creep_guard".to_string(),
            "3_dual_sided_fixation_and_scorch_dwell".to_string(),
            "4_sacrificial_ramp_and_path_leaders".to_string(),
            "5_regime_sensor_and_accumulator_bounds".to_string(),
            "6_wearer_age_and_fastness_gates".to_string(),
        ],
    };

    // Emit deterministic CSV schedule
    let mut csv = String::new();
    csv.push_str("s_start_m,s_end_m,target_speed_m_s,fiducial_mark");
    for p in &machine.primaries {
        let _ = write!(csv, ",dose_{}", p.id);
    }
    csv.push('\n');
    for seg in &program.segments {
        let _ = write!(
            csv,
            "{:.6},{:.6},{:.5},{}",
            seg.s_start_m,
            seg.s_end_m,
            seg.target_speed_m_s,
            u8::from(seg.fiducial_mark)
        );
        for &d in &seg.doses {
            let _ = write!(csv, ",{d:.5}");
        }
        csv.push('\n');
    }

    // Emit deterministic motion/dosing G-code
    let mut gcode = String::new();
    let _ = writeln!(
        gcode,
        "; FrankenTextiles ftx-dye-program/1 ({})",
        program.name
    );
    let _ = writeln!(
        gcode,
        "; machine: {} | regime: {:?}",
        machine.id, program.regime
    );
    gcode.push_str("; hardware_execution_authorized: false\n");
    gcode.push_str("G21 ; millimetres\nG90 ; absolute yarn arc-length E\n");
    let _ = writeln!(gcode, "M104 S{:.1} ; oven setpoint C", machine.oven_temp_c);
    let leader_mm = program.startup_leader_m * 1000.0;
    let first_seg = &program.segments[0];
    let init_feed_mm_min = first_seg.target_speed_m_s * 60_000.0;
    let _ = writeln!(
        gcode,
        "G1 E{leader_mm:.3} F{init_feed_mm_min:.1} ; sacrificial startup leader"
    );
    if first_seg.s_start_m > 1e-12 {
        let pre_offset_mm = leader_mm + first_seg.s_start_m * 1000.0;
        gcode.push_str("M400 ALL=0 ; undyed pre-offset before first dyed segment\n");
        let _ = writeln!(
            gcode,
            "G1 E{pre_offset_mm:.3} F{init_feed_mm_min:.1} ; advance to s_start_m"
        );
    }
    for seg in &program.segments {
        let e_mm = leader_mm + seg.s_end_m * 1000.0;
        let feed = seg.target_speed_m_s * 60_000.0;
        if seg.fiducial_mark {
            gcode.push_str("M401 ; emit optical fiducial mark\n");
        }
        let dose_tokens: Vec<String> = seg
            .doses
            .iter()
            .enumerate()
            .map(|(i, d)| format!("P{i}={d:.4}"))
            .collect();
        let _ = writeln!(gcode, "M400 {} ; set primary doses", dose_tokens.join(" "));
        let _ = writeln!(gcode, "G1 E{e_mm:.3} F{feed:.1}");
    }
    let last_end_m = program.segments.last().map_or(0.0, |s| s.s_end_m);
    let total_e_mm = leader_mm + last_end_m * 1000.0 + program.shutdown_trailer_m * 1000.0;
    gcode.push_str("M400 ALL=0 ; valves off\n");
    let _ = writeln!(
        gcode,
        "G1 E{total_e_mm:.3} F{init_feed_mm_min:.1} ; sacrificial shutdown trailer"
    );
    gcode.push_str("M104 S0 ; oven off\nM2 ; program end\n");

    verify_dye_export_readback(&csv, &gcode, program, machine)?;

    let program_sha256 = program.canonical_sha256()?;
    let mut hazard_report = hazard_report;
    let uncertainty_receipt_sha256 = if let Some(receipt) = uncertainty_receipt {
        receipt.verify_integrity()?;
        if receipt.program_sha256 != program_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "dye plan uncertainty receipt program_sha256 does not match YarnDyeProgram",
            ));
        }
        if receipt.machine_sha256 != machine_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "dye plan uncertainty receipt machine_sha256 does not match DyeMachineProfile",
            ));
        }
        if receipt.regime != program.regime {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "dye plan uncertainty receipt regime does not match YarnDyeProgram",
            ));
        }
        if let Some(rq) = &receipt.qualification_sha256
            && qual_sha.as_deref() != Some(rq.as_str())
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "dye plan uncertainty receipt qualification_sha256 does not match supplied YarnDyeingQualification",
            ));
        }
        if qual_sha.is_some() && receipt.qualification_sha256.is_none() {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "dye plan uncertainty receipt has no qualification_sha256 for the supplied YarnDyeingQualification",
            ));
        }
        hazard_report
            .checks_passed
            .push("7_uncertainty_receipt_verified".to_string());
        Some(receipt.payload_sha256.clone())
    } else {
        None
    };

    let gcode_sha256 = sha256_hex(gcode.as_bytes());
    let four_claims = FourClaimsVerdict {
        parity_basis: "ftx_dye_program_v1_verified".to_string(),
        physics_basis: if uncertainty_receipt_sha256.is_some() {
            "capstan_dwell_accumulator_km_transport_and_uncertainty_receipt".to_string()
        } else {
            "capstan_dwell_accumulator_and_km_transport".to_string()
        },
        fit_claimed: false,
        performance_basis: "unbenchmarked".to_string(),
    };
    let seal_view = DyeCertSealView {
        schema: DYE_CERT_SCHEMA,
        program_sha256: &program_sha256,
        machine_sha256: &machine_sha256,
        qualification_sha256: &qual_sha,
        uncertainty_receipt_sha256: &uncertainty_receipt_sha256,
        wearer_age_class: program.wearer_age_class,
        hardware_execution_authorized: false,
        physical_dyeing_validated: false,
        hazard_report: &hazard_report,
        four_claims: &four_claims,
    };
    let cert_sha256 = canonical_json_sha256(&seal_view)?;
    let cert = DyeMachineCert {
        schema: DYE_CERT_SCHEMA.to_string(),
        program_sha256,
        machine_sha256,
        qualification_sha256: qual_sha,
        uncertainty_receipt_sha256,
        wearer_age_class: program.wearer_age_class,
        hardware_execution_authorized: false,
        physical_dyeing_validated: false,
        hazard_report,
        four_claims,
        cert_sha256,
    };
    cert.verify_integrity()?;
    let cert_json = serde_json::to_string_pretty(&cert).map_err(|e| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("cannot serialize dye certificate: {e}"),
        )
    })?;

    Ok(DyeExportBundle {
        schedule_csv: csv,
        controller_gcode: gcode,
        cert,
        cert_json,
        gcode_sha256,
    })
}

/// Independent read-back verifier for emitted dye schedule CSV and controller G-code (`FTX-K16`).
///
/// Re-parses every CSV row and G-code command to guarantee:
/// - exact segment count, monotonic non-overlapping arc ranges, speed envelope, and dose bounds;
/// - monotonic non-decreasing `G1 E` yarn advance (never reversing wet yarn through the oven/capstan);
/// - matching oven setpoint (`M104`), explicit safety disclaimer comment, and clean `M2` termination.
///
/// # Errors
/// [`RefusalCode::VerificationMismatch`] if either file fails read-back checks.
pub fn verify_dye_export_readback(
    schedule_csv: &str,
    controller_gcode: &str,
    program: &YarnDyeProgram,
    machine: &DyeMachineProfile,
) -> Result<(), Refusal> {
    // 1. Parse and verify CSV schedule
    let mut csv_lines = schedule_csv.lines();
    let Some(header) = csv_lines.next() else {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "dye CSV read-back failed: empty schedule",
        ));
    };
    let mut expected_header = "s_start_m,s_end_m,target_speed_m_s,fiducial_mark".to_string();
    for p in &machine.primaries {
        let _ = write!(expected_header, ",dose_{}", p.id);
    }
    if header != expected_header {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "dye CSV read-back failed: header disagrees with machine primaries",
        ));
    }
    let expected_cols = 4 + machine.primaries.len();
    let mut parsed_rows = 0usize;
    let mut prev_end: Option<f64> = None;
    for (row_idx, line) in csv_lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() != expected_cols {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!(
                    "dye CSV row {row_idx} column count {} != expected {expected_cols}",
                    cols.len()
                ),
            ));
        }
        let s_start: f64 = cols[0].parse().map_err(|_| {
            Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("dye CSV row {row_idx} invalid s_start_m"),
            )
        })?;
        let s_end: f64 = cols[1].parse().map_err(|_| {
            Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("dye CSV row {row_idx} invalid s_end_m"),
            )
        })?;
        let speed: f64 = cols[2].parse().map_err(|_| {
            Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("dye CSV row {row_idx} invalid target_speed_m_s"),
            )
        })?;
        if !s_start.is_finite() || !s_end.is_finite() || s_end <= s_start {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("dye CSV row {row_idx} has invalid arc range"),
            ));
        }
        if let Some(pe) = prev_end
            && (s_start - pe).abs() > 1e-5
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!(
                    "dye CSV row {row_idx} start {s_start} disconnected from previous end {pe}"
                ),
            ));
        }
        prev_end = Some(s_end);
        if !speed.is_finite()
            || speed + 1e-5 < machine.min_line_speed_m_s
            || speed - 1e-5 > machine.max_line_speed_m_s
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("dye CSV row {row_idx} speed {speed} outside machine envelope"),
            ));
        }
        if !matches!(cols[3], "0" | "1") {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("dye CSV row {row_idx} invalid fiducial flag '{}'", cols[3]),
            ));
        }
        for (p_idx, tok) in cols[4..].iter().enumerate() {
            let dose: f64 = tok.parse().map_err(|_| {
                Refusal::new(
                    RefusalCode::VerificationMismatch,
                    format!("dye CSV row {row_idx} invalid dose token '{tok}'"),
                )
            })?;
            if !dose.is_finite()
                || dose < -1e-6
                || dose > machine.primaries[p_idx].max_dose_per_m + 1e-4
            {
                return Err(Refusal::new(
                    RefusalCode::VerificationMismatch,
                    format!("dye CSV row {row_idx} primary {p_idx} dose {dose} out of range"),
                ));
            }
        }
        parsed_rows += 1;
    }
    if parsed_rows != program.segments.len() {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            format!(
                "dye CSV row count {parsed_rows} != program segment count {}",
                program.segments.len()
            ),
        ));
    }

    // 2. Parse and verify G-code monotonicity and safety envelope
    let mut seen_no_hw = false;
    let mut seen_g21 = false;
    let mut seen_g90 = false;
    let mut seen_oven_set = false;
    let mut seen_oven_off = false;
    let mut seen_m2 = false;
    let mut prev_e_mm = -1.0_f64;
    let mut g1_count = 0usize;
    let expected_oven_set = format!("M104 S{:.1}", machine.oven_temp_c);

    for (line_idx, raw_line) in controller_gcode.lines().enumerate() {
        let line_no = line_idx + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen_m2 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("dye G-code line {line_no}: unexpected command after M2"),
            ));
        }
        if trimmed == "; hardware_execution_authorized: false" {
            seen_no_hw = true;
            continue;
        }
        if trimmed.starts_with(';') {
            continue;
        }
        let code = trimmed.split(';').next().unwrap_or("").trim();
        if code == "G21" {
            seen_g21 = true;
        } else if code == "G90" {
            seen_g90 = true;
        } else if code == expected_oven_set {
            seen_oven_set = true;
        } else if code == "M104 S0" {
            seen_oven_off = true;
        } else if code == "M2" {
            seen_m2 = true;
        } else if code.starts_with("G1 ") {
            let mut e_val: Option<f64> = None;
            let mut f_val: Option<f64> = None;
            for tok in code.split_whitespace().skip(1) {
                if let Some(rest) = tok.strip_prefix('E') {
                    e_val = rest.parse().ok();
                } else if let Some(rest) = tok.strip_prefix('F') {
                    f_val = rest.parse().ok();
                }
            }
            let (Some(e_mm), Some(feed)) = (e_val, f_val) else {
                return Err(Refusal::new(
                    RefusalCode::VerificationMismatch,
                    format!("dye G-code line {line_no}: malformed G1 move '{code}'"),
                ));
            };
            if !e_mm.is_finite()
                || !feed.is_finite()
                || feed <= 0.0
                || e_mm < 0.0
                || e_mm + 1e-6 < prev_e_mm
            {
                return Err(Refusal::new(
                    RefusalCode::VerificationMismatch,
                    format!(
                        "dye G-code line {line_no}: non-monotonic or invalid move E={e_mm} (prev {prev_e_mm}) F={feed}"
                    ),
                ));
            }
            prev_e_mm = e_mm;
            g1_count += 1;
        } else if code.starts_with("M400 ") || code == "M401" {
            // Valve dosing / optical fiducial command
        } else {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("dye G-code line {line_no}: unsupported command '{code}'"),
            ));
        }
    }
    let expected_g1 =
        2 + program.segments.len() + usize::from(program.segments[0].s_start_m > 1e-12);
    if !seen_no_hw
        || !seen_g21
        || !seen_g90
        || !seen_oven_set
        || !seen_oven_off
        || !seen_m2
        || g1_count != expected_g1
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "dye G-code read-back failed: missing mandatory setup/oven/M2 commands or G1 count mismatch",
        ));
    }
    Ok(())
}

/// Generates a hard-capped `Exp-4` stepped bleed, shrinkage, wet-tenacity, and fastness calibration
/// swatch kit (`CalibrationSwatchOnly`, `not_for_garment_wear: true`, total yarn `<= 40.0 m`)
/// alongside a blank [`YarnDyeingQualification`] template (`FTX-K16`).
///
/// # Errors
/// If `machine` is invalid or cannot admit a safe calibration speed inside its dual dwell window.
pub fn export_exp4_bleed_swatch_kit(
    machine: &DyeMachineProfile,
    yarn_id: &str,
    material_sha256: &str,
) -> Result<DyeBleedSwatchBundle, Refusal> {
    machine.validate()?;
    if yarn_id.trim().is_empty() || material_sha256.trim().is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "Exp-4 bleed swatch kit requires non-empty yarn_id and material_sha256",
        ));
    }
    let machine_sha256 = machine.canonical_sha256()?;
    // Choose the geometric-safe midpoint line speed inside BOTH machine speed bounds and oven dwell window.
    let req_fix = machine
        .primaries
        .iter()
        .map(|p| p.min_fixation_dwell_s)
        .fold(0.0_f64, f64::max);
    let v_lo = machine
        .min_line_speed_m_s
        .max(machine.oven_length_m / machine.scorch_max_dwell_s);
    let v_hi = if req_fix > 0.0 {
        machine
            .max_line_speed_m_s
            .min(machine.oven_length_m / req_fix)
    } else {
        machine.max_line_speed_m_s
    };
    if v_lo > v_hi + 1e-12 {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "dye machine '{}' has no feasible line speed satisfying both fixation and scorch dwell bounds ([{v_lo:.4}, {v_hi:.4}] m/s)",
                machine.id
            ),
        ));
    }
    let v_nom = 0.5 * (v_lo + v_hi);
    let ramp_len = (v_nom * v_nom) / (2.0 * machine.max_line_accel_m_s2);
    let startup_leader_m = ramp_len.max(machine.applicator_to_feeder_path_m) + 0.05;
    let shutdown_trailer_m = ramp_len + 0.10;

    // Emit sharp 0.5 m stepped pulses per primary (25%, 50%, 100% of max_dose) separated by 0.25 m undyed gaps
    // so the operator can directly measure capillary bleed sigma_wick, shrinkage, and K/S curves.
    let mut segments = Vec::new();
    let mut cursor = 0.0_f64;
    let n_p = machine.primaries.len();
    let levels = [0.25_f64, 0.50_f64, 1.0_f64];
    for p_idx in 0..n_p {
        for &lvl in &levels {
            let mut doses = vec![0.0; n_p];
            doses[p_idx] = machine.primaries[p_idx].max_dose_per_m * lvl;
            segments.push(DyeSegment {
                s_start_m: cursor,
                s_end_m: cursor + 0.40,
                target_speed_m_s: v_nom,
                doses,
                fiducial_mark: false,
            });
            cursor += 0.40;
            segments.push(DyeSegment {
                s_start_m: cursor,
                s_end_m: cursor + 0.20,
                target_speed_m_s: v_nom,
                doses: vec![0.0; n_p],
                fiducial_mark: false,
            });
            cursor += 0.20;
        }
    }

    let mut metadata = BTreeMap::new();
    metadata.insert("experiment".to_string(), "Exp-4".to_string());
    metadata.insert("wearable_certified".to_string(), "false".to_string());
    let program = YarnDyeProgram {
        schema: DYE_PROGRAM_SCHEMA.to_string(),
        name: format!("exp4-bleed-swatch-{yarn_id}"),
        run_id: 0,
        yarn_id: yarn_id.to_string(),
        wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
        regime: DyeingRegime::Regime1AStripes,
        machine_sha256: machine_sha256.clone(),
        qualification_sha256: None,
        startup_leader_m,
        shutdown_trailer_m,
        segments,
        regime3_intake_timeline: None,
        expected_delta_e00_p50: None,
        expected_delta_e00_p95: None,
        gamut_floor_delta_e00: None,
        unreproducible_energy_fraction: None,
        not_for_garment_wear: true,
        metadata,
    };
    let export = verify_and_export_dye_program(&program, machine, None, Some(material_sha256))?;

    let mut conditioning = BTreeMap::new();
    conditioning.insert(
        "protocol".to_string(),
        "dye Exp-4 stepped pulse strip -> fix -> soap wash-off -> dry flat 24h -> measure sigma_wick_mm, axial shrinkage %, wet tenacity, spectrophotometer K/S, and ISO 105 fastness".to_string(),
    );
    let qualification_template = YarnDyeingQualification {
        id: format!("exp4-{}-{yarn_id}", machine.id),
        material_sha256: material_sha256.to_string(),
        dye_machine_sha256: machine_sha256,
        yarn_lot_id: "FILL_IN_YARN_LOT_ID".to_string(),
        dye_lot_id: "FILL_IN_DYE_LOT_ID".to_string(),
        wick_dispersion_mm: 1.5,
        shrinkage_axial_pct: 2.0,
        wet_tenacity_retention_ratio: 0.90,
        yarn_breaking_force_n: 8.0,
        yarn_tg_c: None,
        optical_measurement_basis: OpticalMeasurementBasis::EstimateOnly,
        washing_off_validated: false,
        iso_105_c06_grade: None,
        iso_105_x12_wet_rub_grade: None,
        saliva_fastness_validated: false,
        evidence: Evidence::Estimate,
        source: Source {
            reference: "local:unmeasured_exp4_template".to_string(),
            accessed_on: "YYYY-MM-DD".to_string(),
            note: "Replace with physically measured Exp-4 coupon values before dyeing wearable garments".to_string(),
        },
        method: "Exp-4 stepped bleed/shrinkage/fastness swatch".to_string(),
        conditioning,
        allow_estimates: false,
    };
    let qualification_template_json = serde_json::to_string_pretty(&qualification_template)
        .map_err(|e| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                format!("cannot serialize Exp-4 dye qualification template: {e}"),
            )
        })?;
    Ok(DyeBleedSwatchBundle {
        program,
        export,
        qualification_template,
        qualification_template_json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_pattern::dye::{IntakeEvent, IntakeEventKind, Regime3IntakeTimeline};

    #[test]
    fn dye_verifier_catches_accumulator_overflow_and_emits_verified_bundle() {
        let machine = DyeMachineProfile::benchtop_piezo_cmyk();
        let mhash = machine.canonical_sha256().unwrap();
        let qual = YarnDyeingQualification {
            id: "exp4-cotton".to_string(),
            material_sha256: "mat-cotton".to_string(),
            dye_machine_sha256: mhash.clone(),
            yarn_lot_id: "lot-1".to_string(),
            dye_lot_id: "dye-1".to_string(),
            wick_dispersion_mm: 1.5,
            shrinkage_axial_pct: 2.0,
            wet_tenacity_retention_ratio: 0.95,
            yarn_breaking_force_n: 9.0,
            yarn_tg_c: None,
            optical_measurement_basis: OpticalMeasurementBasis::SpectrophotometerD65_10Deg,
            washing_off_validated: true,
            iso_105_c06_grade: Some(4.5),
            iso_105_x12_wet_rub_grade: Some(4.5),
            saliva_fastness_validated: true,
            evidence: Evidence::Measured,
            source: Source {
                reference: "local:exp4".to_string(),
                accessed_on: "2026-11-27".to_string(),
                note: "cotton coupon".to_string(),
            },
            method: "steam fix + soap wash".to_string(),
            conditioning: BTreeMap::new(),
            allow_estimates: false,
        };
        let mut prog = YarnDyeProgram {
            schema: DYE_PROGRAM_SCHEMA.to_string(),
            name: "inline-toddler-stripe".to_string(),
            run_id: 0,
            yarn_id: "cotton-120tex".to_string(),
            wearer_age_class: WearerAgeClass::InfantUnder3,
            regime: DyeingRegime::Regime3InlineDancer,
            machine_sha256: mhash,
            qualification_sha256: Some(qual.canonical_sha256().unwrap()),
            startup_leader_m: 1.4,
            shutdown_trailer_m: 0.3,
            segments: vec![
                DyeSegment {
                    s_start_m: 0.0,
                    s_end_m: 1.0,
                    target_speed_m_s: 0.08,
                    doses: vec![0.4, 0.1, 0.0, 0.0],
                    fiducial_mark: false,
                },
                DyeSegment {
                    s_start_m: 1.0,
                    s_end_m: 2.0,
                    target_speed_m_s: 0.08,
                    doses: vec![0.0, 0.5, 0.3, 0.0],
                    fiducial_mark: false,
                },
            ],
            regime3_intake_timeline: Some(Regime3IntakeTimeline {
                events: vec![
                    IntakeEvent {
                        duration_s: 8.0,
                        yarn_intake_speed_m_s: 0.08,
                        kind: IntakeEventKind::KnitPass,
                    },
                    IntakeEvent {
                        duration_s: 2.0,
                        yarn_intake_speed_m_s: 0.0,
                        kind: IntakeEventKind::ReversalDwell,
                    },
                ],
                dt_verify_s: 0.05,
            }),
            expected_delta_e00_p50: Some(1.6),
            expected_delta_e00_p95: Some(2.9),
            gamut_floor_delta_e00: Some(0.8),
            unreproducible_energy_fraction: Some(0.04),
            not_for_garment_wear: false,
            metadata: BTreeMap::new(),
        };

        let bundle =
            verify_and_export_dye_program(&prog, &machine, Some(&qual), Some("mat-cotton"))
                .unwrap();
        assert!(!bundle.cert.hardware_execution_authorized);
        assert!(!bundle.cert.physical_dyeing_validated);
        assert!(bundle.controller_gcode.contains("G1 E"));
        assert!(bundle.schedule_csv.contains("dose_cyan"));
        bundle.cert.verify_integrity().unwrap();

        // Tampering with cert fails verify_integrity
        let mut tampered_cert = bundle.cert.clone();
        tampered_cert.hardware_execution_authorized = true;
        assert_eq!(
            tampered_cert.verify_integrity().unwrap_err().code,
            RefusalCode::VerificationMismatch
        );

        // Tampering G-code to retract yarn backwards fails read-back
        let backward_gcode = bundle
            .controller_gcode
            .replacen("G1 E3700.000", "G1 E100.000", 1);
        assert_eq!(
            verify_dye_export_readback(&bundle.schedule_csv, &backward_gcode, &prog, &machine)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );

        // Non-zero s_start_m emits an explicit undyed pre-offset and stays strictly monotonic!
        let mut offset_prog = prog.clone();
        offset_prog.regime = DyeingRegime::Regime1AStripes;
        offset_prog.regime3_intake_timeline = None;
        offset_prog.segments = vec![DyeSegment {
            s_start_m: 0.5,
            s_end_m: 2.0,
            target_speed_m_s: 0.08,
            doses: vec![0.4, 0.1, 0.0, 0.0],
            fiducial_mark: false,
        }];
        offset_prog.qualification_sha256 = Some(qual.canonical_sha256().unwrap());
        let offset_bundle =
            verify_and_export_dye_program(&offset_prog, &machine, Some(&qual), Some("mat-cotton"))
                .unwrap();
        assert!(offset_bundle.controller_gcode.contains("G1 E1900.000"));
        assert!(offset_bundle.controller_gcode.contains("G1 E3700.000"));

        // A 40-second zero-feed pause overflows the 1.5m dancer accumulator (min line speed 0.04 m/s -> +1.6m!)
        prog.regime3_intake_timeline = Some(Regime3IntakeTimeline {
            events: vec![IntakeEvent {
                duration_s: 40.0,
                yarn_intake_speed_m_s: 0.0,
                kind: IntakeEventKind::OperatorPause,
            }],
            dt_verify_s: 0.1,
        });
        let err = verify_and_export_dye_program(&prog, &machine, Some(&qual), Some("mat-cotton"))
            .unwrap_err();
        assert_eq!(err.code, RefusalCode::MaterialProcessHazard);

        // Tiny dt_verify_s CPU DoS must refuse cleanly with OutOfRange
        let mut dos_prog = prog.clone();
        dos_prog.regime3_intake_timeline = Some(Regime3IntakeTimeline {
            events: vec![IntakeEvent {
                duration_s: 100.0,
                yarn_intake_speed_m_s: 0.08,
                kind: IntakeEventKind::KnitPass,
            }],
            dt_verify_s: 1e-9,
        });
        assert_eq!(
            verify_and_export_dye_program(&dos_prog, &machine, Some(&qual), Some("mat-cotton"))
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );

        // CalibrationSwatchOnly with NaN qualification must NOT bypass Capstan/creep gates!
        let mut swatch_prog = prog.clone();
        swatch_prog.wearer_age_class = WearerAgeClass::CalibrationSwatchOnly;
        swatch_prog.not_for_garment_wear = true;
        swatch_prog.qualification_sha256 = None;
        swatch_prog.regime = DyeingRegime::Regime1AStripes;
        swatch_prog.regime3_intake_timeline = None;
        let mut nan_qual = qual.clone();
        nan_qual.yarn_breaking_force_n = f64::NAN;
        assert_eq!(
            verify_and_export_dye_program(&swatch_prog, &machine, Some(&nan_qual), None)
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );

        // CalibrationSwatchOnly exceeding 40m yarn cap refuses fail-closed
        let mut huge_swatch = swatch_prog.clone();
        huge_swatch.segments = vec![DyeSegment {
            s_start_m: 0.0,
            s_end_m: 45.0,
            target_speed_m_s: 0.08,
            doses: vec![0.2, 0.0, 0.0, 0.0],
            fiducial_mark: false,
        }];
        assert_eq!(
            verify_and_export_dye_program(&huge_swatch, &machine, None, None)
                .unwrap_err()
                .code,
            RefusalCode::MaterialProcessHazard
        );

        // Bound qualification_sha256 mismatch and G-code newline injection fail closed
        let mut bad_qsha = swatch_prog.clone();
        bad_qsha.qualification_sha256 = Some("0".repeat(64));
        assert_eq!(
            verify_and_export_dye_program(&bad_qsha, &machine, None, None)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );

        let mut bad_name = swatch_prog;
        bad_name.name = "stripe\nG1 E99999".to_string();
        assert_eq!(
            verify_and_export_dye_program(&bad_name, &machine, None, None)
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );

        // Exp-4 bleed swatch kit exports cleanly with not_for_garment_wear = true and <= 40m yarn
        let kit = export_exp4_bleed_swatch_kit(&machine, "cotton-120tex", "mat-cotton").unwrap();
        assert!(kit.program.not_for_garment_wear);
        assert_eq!(
            kit.program.wearer_age_class,
            WearerAgeClass::CalibrationSwatchOnly
        );
        assert!(!kit.qualification_template.washing_off_validated);
        assert_eq!(kit.qualification_template.evidence, Evidence::Estimate);
        kit.export.cert.verify_integrity().unwrap();
    }

    #[test]
    fn dye_export_verifies_uncertainty_receipt_linkage_and_rejects_tamper_or_mismatch() {
        use ftx_pattern::dye::{
            DressedAppearanceSummary, DyeDriftLedger, DyeFeatureFloorSummary,
            DyePlanUncertaintyInput,
        };

        let machine = DyeMachineProfile::benchtop_piezo_cmyk();
        let mhash = machine.canonical_sha256().unwrap();
        let qual = YarnDyeingQualification {
            id: "exp4-receipt-link".to_string(),
            material_sha256: "mat-cotton".to_string(),
            dye_machine_sha256: mhash.clone(),
            yarn_lot_id: "lot-1".to_string(),
            dye_lot_id: "dye-1".to_string(),
            wick_dispersion_mm: 1.2,
            shrinkage_axial_pct: 2.0,
            wet_tenacity_retention_ratio: 0.95,
            yarn_breaking_force_n: 9.0,
            yarn_tg_c: None,
            optical_measurement_basis: OpticalMeasurementBasis::CalibratedChartScanner,
            washing_off_validated: true,
            iso_105_c06_grade: Some(4.5),
            iso_105_x12_wet_rub_grade: Some(4.5),
            saliva_fastness_validated: true,
            evidence: Evidence::Measured,
            source: Source {
                reference: "local:exp4".to_string(),
                accessed_on: "2026-11-27".to_string(),
                note: "cotton coupon".to_string(),
            },
            method: "steam fix + soap wash".to_string(),
            conditioning: BTreeMap::new(),
            allow_estimates: false,
        };
        let qhash = qual.canonical_sha256().unwrap();
        let prog = YarnDyeProgram {
            schema: DYE_PROGRAM_SCHEMA.to_string(),
            name: "receipt-linked-stripe".to_string(),
            run_id: 0,
            yarn_id: "cotton-120tex".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            regime: DyeingRegime::Regime1AStripes,
            machine_sha256: mhash.clone(),
            qualification_sha256: Some(qhash.clone()),
            startup_leader_m: 1.4,
            shutdown_trailer_m: 0.3,
            segments: vec![DyeSegment {
                s_start_m: 0.0,
                s_end_m: 1.0,
                target_speed_m_s: 0.08,
                doses: vec![0.4, 0.1, 0.0, 0.0],
                fiducial_mark: false,
            }],
            regime3_intake_timeline: None,
            expected_delta_e00_p50: Some(1.4),
            expected_delta_e00_p95: Some(2.6),
            gamut_floor_delta_e00: Some(0.6),
            unreproducible_energy_fraction: Some(0.05),
            not_for_garment_wear: false,
            metadata: BTreeMap::new(),
        };
        let phash = prog.canonical_sha256().unwrap();
        let make_receipt =
            |p_sha: String, m_sha: String, reg: DyeingRegime, q_sha: Option<String>| {
                DyePlanUncertaintyReceipt::seal(DyePlanUncertaintyInput {
                    program_sha256: p_sha,
                    machine_sha256: m_sha,
                    qualification_sha256: q_sha,
                    regime: reg,
                    optical_measurement_basis: OpticalMeasurementBasis::CalibratedChartScanner,
                    drift_ledger: DyeDriftLedger {
                        topology_multiplier: 1.0,
                        drift_epsilon: 0.002,
                        drift_sigma_eta: 0.015,
                        max_stitch_phase_shift: 0.5,
                        coherent_course_horizon: 10,
                        coherent_horizon_circular_stitches: 180,
                        coherent_horizon_flatbed_stitches: 60,
                    },
                    feature_floor: DyeFeatureFloorSummary {
                        sigma_wick_mm: 1.2,
                        sigma_wick_stitches: 0.18,
                        min_reproducible_feature_mm: 2.7,
                        chromatographic_halo_mm: 0.3,
                        unreproducible_energy_fraction: 0.05,
                    },
                    dressed_appearance: DressedAppearanceSummary {
                        delta_e00_mean: 1.5,
                        delta_e00_p50: 1.4,
                        delta_e00_p95: 2.6,
                        gamut_floor_delta_e00: 0.6,
                        aspect_ratio_stretch_bound: 1.05,
                        differential_shrinkage_stretch_perturbation_flagged: false,
                        pore_grin_through_alert_zones: Vec::new(),
                    },
                    evidence: Evidence::Measured,
                    gating_flags: Vec::new(),
                })
                .unwrap()
            };

        let receipt = make_receipt(
            phash.clone(),
            mhash.clone(),
            DyeingRegime::Regime1AStripes,
            Some(qhash.clone()),
        );
        let bundle = verify_and_export_dye_program_with_receipt(
            &prog,
            &machine,
            Some(&qual),
            Some("mat-cotton"),
            Some(&receipt),
        )
        .unwrap();
        assert_eq!(
            bundle.cert.uncertainty_receipt_sha256.as_deref(),
            Some(receipt.payload_sha256.as_str())
        );
        assert!(
            bundle
                .cert
                .hazard_report
                .checks_passed
                .iter()
                .any(|c| c == "7_uncertainty_receipt_verified")
        );
        bundle.cert.verify_integrity().unwrap();

        // Tampered receipt refuses
        let mut tampered = receipt.clone();
        tampered.dressed_appearance.delta_e00_p95 = 0.01;
        assert_eq!(
            verify_and_export_dye_program_with_receipt(
                &prog,
                &machine,
                Some(&qual),
                Some("mat-cotton"),
                Some(&tampered),
            )
            .unwrap_err()
            .code,
            RefusalCode::VerificationMismatch
        );

        // Mismatched program_sha256, machine_sha256, regime, or qualification_sha256 all refuse
        for bad_receipt in [
            make_receipt(
                "0".repeat(64),
                mhash.clone(),
                DyeingRegime::Regime1AStripes,
                Some(qhash.clone()),
            ),
            make_receipt(
                phash.clone(),
                "0".repeat(64),
                DyeingRegime::Regime1AStripes,
                Some(qhash.clone()),
            ),
            make_receipt(
                phash.clone(),
                mhash.clone(),
                DyeingRegime::Regime1BCircularPooling,
                Some(qhash),
            ),
            make_receipt(
                phash.clone(),
                mhash.clone(),
                DyeingRegime::Regime1AStripes,
                Some("0".repeat(64)),
            ),
        ] {
            assert_eq!(
                verify_and_export_dye_program_with_receipt(
                    &prog,
                    &machine,
                    Some(&qual),
                    Some("mat-cotton"),
                    Some(&bad_receipt),
                )
                .unwrap_err()
                .code,
                RefusalCode::VerificationMismatch
            );
        }

        // Unqualified receipt (qualification_sha256 = None, color_fidelity_claimed = false)
        // must still refuse when paired with a supplied YarnDyeingQualification.
        let unqualified_receipt = make_receipt(phash, mhash, DyeingRegime::Regime1AStripes, None);
        assert!(!unqualified_receipt.four_claims.color_fidelity_claimed);
        assert_eq!(
            verify_and_export_dye_program_with_receipt(
                &prog,
                &machine,
                Some(&qual),
                Some("mat-cotton"),
                Some(&unqualified_receipt),
            )
            .unwrap_err()
            .code,
            RefusalCode::VerificationMismatch
        );
    }
}
