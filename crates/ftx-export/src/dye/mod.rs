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

use std::fmt::Write as _;

use ftx_core::receipt::{canonical_json_sha256, sha256_hex};
use ftx_core::{Refusal, RefusalCode, det};
use ftx_pattern::dye::{
    DyeMachineProfile, DyeingRegime, YarnDyeProgram, YarnDyeingQualification,
};
use ftx_pattern::knit::{FourClaimsVerdict, WearerAgeClass};
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
    let (qual_sha, dry_break_n, wet_break_n, yarn_tg_c) =
        if program.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
            if let Some(q) = qualification {
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
    if let Some(tg) = yarn_tg_c {
        if machine.oven_temp_c > tg - 15.0 {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "oven temperature {:.1} C violates thermoplastic creep guard (yarn T_g = {tg:.1} C, max safe {:.1} C)",
                    machine.oven_temp_c,
                    tg - 15.0
                ),
            ));
        }
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
                format!("segment {idx} oven dwell {dwell:.2}s < required fixation {req_fix_dwell:.2}s"),
            ));
        }
        if dwell > machine.scorch_max_dwell_s {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!("segment {idx} oven dwell {dwell:.2}s > scorch cap {:.2}s", machine.scorch_max_dwell_s),
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
    if program.regime == DyeingRegime::Regime2FiducialSynced && !machine.has_optical_fiducial_sensor {
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

        for (ev_idx, ev) in tl.events.iter().enumerate() {
            let steps = (ev.duration_s / tl.dt_verify_s).ceil().max(1.0) as usize;
            let dt = ev.duration_s / (steps as f64);
            for _ in 0..steps {
                // Proportional trim controller steering dye line speed inside [v_min, v_max] with accel cap
                let v_cmd = (ev.yarn_intake_speed_m_s + acc.dancer_kp_s_inv * (acc.target_buffer_m - buf))
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
    let _ = writeln!(gcode, "; FrankenTextiles ftx-dye-program/1 ({})", program.name);
    let _ = writeln!(gcode, "; machine: {} | regime: {:?}", machine.id, program.regime);
    gcode.push_str("; hardware_execution_authorized: false\n");
    gcode.push_str("G21 ; millimetres\nG90 ; absolute yarn arc-length E\n");
    let _ = writeln!(gcode, "M104 S{:.1} ; oven setpoint C", machine.oven_temp_c);
    let leader_mm = program.startup_leader_m * 1000.0;
    let init_feed_mm_min = program.segments[0].target_speed_m_s * 60_000.0;
    let _ = writeln!(
        gcode,
        "G1 E{leader_mm:.3} F{init_feed_mm_min:.1} ; sacrificial startup leader"
    );
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
    let total_e_mm =
        leader_mm + program.active_yarn_length_m() * 1000.0 + program.shutdown_trailer_m * 1000.0;
    gcode.push_str("M400 ALL=0 ; valves off\n");
    let _ = writeln!(
        gcode,
        "G1 E{total_e_mm:.3} F{init_feed_mm_min:.1} ; sacrificial shutdown trailer"
    );
    gcode.push_str("M104 S0 ; oven off\nM2 ; program end\n");

    let program_sha256 = program.canonical_sha256()?;
    let gcode_sha256 = sha256_hex(gcode.as_bytes());
    let four_claims = FourClaimsVerdict {
        parity_basis: "ftx_dye_program_v1_verified".to_string(),
        physics_basis: "capstan_dwell_accumulator_and_km_transport".to_string(),
        fit_claimed: false,
        performance_basis: "unbenchmarked".to_string(),
    };
    let seal_view = DyeCertSealView {
        schema: DYE_CERT_SCHEMA,
        program_sha256: &program_sha256,
        machine_sha256: &machine_sha256,
        qualification_sha256: &qual_sha,
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
        wearer_age_class: program.wearer_age_class,
        hardware_execution_authorized: false,
        physical_dyeing_validated: false,
        hazard_report,
        four_claims,
        cert_sha256,
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use ftx_pattern::dye::{
        DYE_PROGRAM_SCHEMA, DyeSegment, IntakeEvent, IntakeEventKind, OpticalMeasurementBasis,
        Regime3IntakeTimeline,
    };
    use ftx_pattern::materials::{Evidence, Source};

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
            verify_and_export_dye_program(&prog, &machine, Some(&qual), Some("mat-cotton")).unwrap();
        assert!(!bundle.cert.hardware_execution_authorized);
        assert!(!bundle.cert.physical_dyeing_validated);
        assert!(bundle.controller_gcode.contains("G1 E"));
        assert!(bundle.schedule_csv.contains("dose_cyan"));

        // A 40-second zero-feed pause overflows the 1.5m dancer accumulator (min line speed 0.04 m/s -> +1.6m!)
        prog.regime3_intake_timeline = Some(Regime3IntakeTimeline {
            events: vec![IntakeEvent {
                duration_s: 40.0,
                yarn_intake_speed_m_s: 0.0,
                kind: IntakeEventKind::OperatorPause,
            }],
            dt_verify_s: 0.1,
        });
        let err =
            verify_and_export_dye_program(&prog, &machine, Some(&qual), Some("mat-cotton"))
                .unwrap_err();
        assert_eq!(err.code, RefusalCode::MaterialProcessHazard);
    }
}
