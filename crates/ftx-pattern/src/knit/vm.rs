//! Deterministic V-bed knitting machine virtual machine & 7-check safety verifier (`FTX-K03`).
//!
//! Enforces discrete V-bed kinematics, carrier lifecycle & spacing, hook-stack & tuck limits,
//! slider-bed bans, quarter-pitch racking alignment, calibration-swatch hard caps, and child/infant
//! safety gates (float length, EN 14682 drawstring ban, infant cut-tail & eyelet aperture caps).

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::ir::{Bed, CarrierId, CourseDir, KnitDoc, KnitoutOp, KnitoutProgram, Needle};
use super::profile::{CalibrationSwatchCaps, MachineBackendKind, MachineProfile, WearerAgeClass};
use crate::dye::{IntakeEvent, IntakeEventKind, Regime3IntakeTimeline};

/// Structured telemetry emitted by a passing V-bed VM verification run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VBedVmReport {
    /// SHA-256 of the verified Knitout program text.
    pub program_sha256: String,
    /// SHA-256 of the target [`MachineProfile`].
    pub machine_sha256: String,
    /// Ordered list of all passed safety check identifiers.
    pub checks_passed: Vec<String>,
    /// Number of Knitout operations verified.
    pub op_count: usize,
    /// Active needle index span `(min_needle, max_needle)` if any loops were formed.
    pub active_needle_range: Option<[i32; 2]>,
    /// Peak simultaneous loops on any single needle hook.
    pub max_loops_on_hook: u8,
    /// Peak consecutive tuck depth on any single needle hook.
    pub max_tuck_depth: u8,
    /// Maximum interior carrier float length measured over `max(needle_pitch_mm, blocked_wale_pitch_mm)`, in mm.
    pub max_float_mm: f64,
    /// Maximum course-count imbalance across active needles (short-row roller balance).
    pub max_roller_imbalance: u32,
    /// Total loop transfer (`xfer` / `split`) operations executed.
    pub total_transfers: usize,
    /// Peak absolute racking in quarter-pitches.
    pub max_abs_rack_quarter_pitches: i32,
    /// Estimated course passes executed.
    pub estimated_courses: u32,
}

#[derive(Clone, Debug, Default)]
struct HookState {
    loops: u8,
    tuck_depth: u8,
    last_active_course: u32,
}

#[derive(Clone, Debug)]
struct CarrierState {
    active: bool,
    hook_holding: bool,
    last_needle: Option<i32>,
    last_knit_needle: Option<i32>,
}

/// Optional garment-level metadata checked alongside the raw Knitout stream (for float pitch,
/// yarn tex, cut tails, unanchored short rows, and drawstring bans).
#[derive(Clone, Debug)]
pub struct KnitSafetyContext<'a> {
    /// Required wearer age class (no default).
    pub wearer_age_class: WearerAgeClass,
    /// Blocked wale pitch in mm (`100.0 / wales_per_100mm`), used in `max(needle_pitch_mm, blocked_wale_pitch_mm)`.
    pub blocked_wale_pitch_mm: f64,
    /// Optional source [`KnitDoc`] for garment-level checks (tex, cam range, cut tails, drawstrings).
    pub source_doc: Option<&'a KnitDoc>,
    /// Optional maximum declared carriage speed fraction (`0..=1`), checked for `CalibrationSwatchOnly`.
    pub speed_fraction: Option<f64>,
    /// Optional maximum declared takedown fraction (`0..=1`), checked for `CalibrationSwatchOnly`.
    pub takedown_fraction: Option<f64>,
}

/// Verifies a [`KnitoutProgram`] against a [`MachineProfile`] and [`KnitSafetyContext`] using all 7 checks.
///
/// # Errors
/// [`RefusalCode::MachineStateInfeasible`] for discrete machine violations or
/// [`RefusalCode::MaterialProcessHazard`] for physical/child-safety/calibration-cap hazards.
pub fn verify_knitout_program(
    program: &KnitoutProgram,
    profile: &MachineProfile,
    ctx: &KnitSafetyContext<'_>,
) -> Result<VBedVmReport, Refusal> {
    profile.validate()?;
    Refusal::finite("blocked_wale_pitch_mm", ctx.blocked_wale_pitch_mm)?;
    if ctx.blocked_wale_pitch_mm <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "blocked_wale_pitch_mm must be positive",
        ));
    }
    let effective_wale_pitch_mm = profile.needle_pitch_mm.max(ctx.blocked_wale_pitch_mm);

    // Check 7 (pre-flight document & child/infant static hazards)
    if let Some(doc) = ctx.source_doc {
        doc.validate()?;
        if doc.wearer_age_class != ctx.wearer_age_class {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "source KnitDoc wearer_age_class disagrees with verification context",
            ));
        }
        if ctx.wearer_age_class.is_child() && doc.has_hood_or_neck_drawstring {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                "EN 14682 child-safety refusal: hood or neck drawstrings are forbidden on child/infant knitwear",
            )
            .repair("remove hood/neck drawstrings and replace with ribbed shaping or concealed snaps"));
        }
        if let Some(tex) = doc.gauge.yarn_tex
            && (tex < profile.recommended_tex_range[0] || tex > profile.recommended_tex_range[1])
        {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "yarn linear density {tex:.1} tex lies outside machine '{}' recommended range [{:.1}, {:.1}] tex",
                    profile.id,
                    profile.recommended_tex_range[0],
                    profile.recommended_tex_range[1]
                ),
            )
            .repair("select a finer/heavier gauge machine profile or compatible yarn tex"));
        }
        for st in &doc.stitches {
            let s = st.cam_stitch_setting.setting;
            if s < profile.cam_setting_range[0] || s > profile.cam_setting_range[1] {
                return Err(Refusal::new(
                    RefusalCode::MaterialProcessHazard,
                    format!(
                        "stitch {} cam setting {s} outside machine '{}' range {:?}",
                        st.id, profile.id, profile.cam_setting_range
                    ),
                ));
            }
        }
        if ctx.wearer_age_class.is_infant() {
            for run in &doc.yarn_runs {
                if let Some(tail_mm) = run.cut_tail_mm
                    && tail_mm > profile.infant_tail_cap_mm
                {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!(
                            "infant-safety refusal: yarn run {} cut tail {tail_mm:.1} mm exceeds {:.1} mm cap (finger entanglement hazard)",
                            run.run_id, profile.infant_tail_cap_mm
                        ),
                    )
                    .repair("weave in or trim interior cut tails to <= 20 mm"));
                }
            }
            for sr in &doc.short_row_turns {
                if !sr.anchored {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!(
                            "infant-safety refusal: unanchored short-row turn at course {} wale {} risks an eyelet > {:.1} mm",
                            sr.course, sr.turn_wale, profile.infant_eyelet_cap_mm
                        ),
                    )
                    .repair("enable anchored wrap/tuck/German short-row turns"));
                }
            }
        }
        if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
            let total_yarn_m: f64 = doc.yarn_runs.iter().map(|r| r.total_yarn_length_m).sum();
            if total_yarn_m > CalibrationSwatchCaps::MAX_YARN_M {
                return Err(Refusal::new(
                    RefusalCode::MaterialProcessHazard,
                    format!(
                        "calibration swatch yarn length {total_yarn_m:.2} m exceeds hard cap {} m",
                        CalibrationSwatchCaps::MAX_YARN_M
                    ),
                ));
            }
        }
    }

    if let Some(sf) = ctx.speed_fraction {
        Refusal::finite("speed_fraction", sf)?;
        if !(0.0..=1.0).contains(&sf) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("speed_fraction {sf} must be within [0.0, 1.0]"),
            ));
        }
        if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
            && sf > CalibrationSwatchCaps::MAX_SPEED_FRACTION
        {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "calibration swatch speed fraction {sf:.2} exceeds hard cap {:.2}",
                    CalibrationSwatchCaps::MAX_SPEED_FRACTION
                ),
            ));
        }
    }
    if let Some(tf) = ctx.takedown_fraction {
        Refusal::finite("takedown_fraction", tf)?;
        if !(0.0..=1.0).contains(&tf) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("takedown_fraction {tf} must be within [0.0, 1.0]"),
            ));
        }
        if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
            && tf > CalibrationSwatchCaps::MAX_TAKEDOWN_FRACTION
        {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "calibration swatch takedown fraction {tf:.2} exceeds hard cap {:.2}",
                    CalibrationSwatchCaps::MAX_TAKEDOWN_FRACTION
                ),
            ));
        }
    }

    if program.header.width == 0 || program.header.width > profile.needle_count {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "Knitout header width {} is outside machine '{}' bed range 1..={}",
                program.header.width, profile.id, profile.needle_count
            ),
        )
        .repair("set Knitout `;;Width:` within the target machine needle_count"));
    }

    // Initialize known carriers from MachineProfile + header validation
    let valid_carriers: BTreeSet<&str> =
        profile.carrier_ids.iter().map(CarrierId::as_str).collect();
    let mut carriers: BTreeMap<String, CarrierState> = BTreeMap::new();
    for cid in &program.header.carriers {
        if !valid_carriers.contains(cid.as_str()) {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!(
                    "Knitout header declares carrier '{}' unavailable on machine '{}'",
                    cid.as_str(),
                    profile.id
                ),
            )
            .repair("declare only carriers listed in MachineProfile.carrier_ids"));
        }
        carriers.insert(
            cid.0.clone(),
            CarrierState {
                active: false,
                hook_holding: false,
                last_needle: None,
                last_knit_needle: None,
            },
        );
    }

    let mut hooks: BTreeMap<(Bed, i32), HookState> = BTreeMap::new();
    let mut rack_quarter_pitches: i32 = 0;
    let mut active_bridge_racks: BTreeSet<i32> = BTreeSet::new();
    let mut max_abs_rack: i32 = 0;
    let mut max_loops_on_hook: u8 = 0;
    let mut max_tuck_depth: u8 = 0;
    let mut max_float_mm: f64 = 0.0;
    let mut max_roller_imbalance: u32 = 0;
    let mut total_transfers: usize = 0;
    let mut active_min_needle: Option<i32> = None;
    let mut active_max_needle: Option<i32> = None;
    let mut prev_pass_dir: Option<CourseDir> = None;
    let mut estimated_courses: u32 = 0;
    let mut ayab_transfer_acknowledged = false;

    let both_beds_live = |h: &BTreeMap<(Bed, i32), HookState>| -> bool {
        let has_front = h
            .iter()
            .any(|((bed, _), st)| matches!(bed, Bed::Front | Bed::FrontSlider) && st.loops > 0);
        let has_back = h
            .iter()
            .any(|((bed, _), st)| matches!(bed, Bed::Back | Bed::BackSlider) && st.loops > 0);
        has_front && has_back
    };

    let check_unique_carriers =
        |cs: &[CarrierId], op_name: &str, step: usize| -> Result<(), Refusal> {
            if cs.is_empty() {
                return Err(Refusal::new(
                    RefusalCode::MachineStateInfeasible,
                    format!("op #{step}: `{op_name}` requires at least one carrier"),
                )
                .repair("specify at least one declared active carrier"));
            }
            let mut seen = BTreeSet::new();
            for c in cs {
                if !seen.insert(c.as_str()) {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!("op #{step}: `{op_name}` lists duplicate carrier '{}'", c.0),
                    )
                    .repair("remove duplicate carrier entries from the Knitout carrier set"));
                }
            }
            Ok(())
        };

    let check_bed_and_bounds = |needle: Needle,
                                allow_slider: bool,
                                step: usize|
     -> Result<(), Refusal> {
        if needle.bed.is_slider() {
            if !profile.has_sliders {
                return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: slider needle {} used on sliderless machine '{}'",
                            needle.knitout_token(),
                            profile.id
                        ),
                    )
                    .repair("switch to half-gauge empty-needle parking or select a slider-bed machine profile"));
            }
            if !allow_slider {
                return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: loop formation forbidden on slider bed {}",
                            needle.knitout_token()
                        ),
                    )
                    .repair("transfer loops back to main needles (`f`/`b`) before knitting, tucking, or splitting"));
            }
        }
        if matches!(needle.bed, Bed::Back | Bed::BackSlider) && !profile.has_back_bed {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!(
                    "op #{step}: back-bed needle {} used on single-bed machine '{}'",
                    needle.knitout_token(),
                    profile.id
                ),
            )
            .repair("use front-bed stitches only or switch to a V-bed / ribber profile"));
        }
        if needle.index < 0 || i64::from(needle.index) >= i64::from(profile.needle_count) {
            return Err(Refusal::new(
                    RefusalCode::MachineStateInfeasible,
                    format!(
                        "op #{step}: needle {} outside bed range 0..{}",
                        needle.knitout_token(),
                        profile.needle_count
                    ),
                )
                .repair("reduce pattern stitch width, adjust centering offset, or choose a wider needle bed"));
        }
        Ok(())
    };

    let record_needle_span = |idx: i32, lo: &mut Option<i32>, hi: &mut Option<i32>| {
        *lo = Some(lo.map_or(idx, |m| m.min(idx)));
        *hi = Some(hi.map_or(idx, |m| m.max(idx)));
    };

    for (step, op) in program.ops.iter().enumerate() {
        match op {
            KnitoutOp::In { carriers: cs } => {
                ayab_transfer_acknowledged = false;
                check_unique_carriers(cs, "in", step)?;
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                        .repair(
                            "declare the carrier in `;;Carriers:` and MachineProfile.carrier_ids",
                        )
                    })?;
                    if st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: carrier '{}' brought in twice", c.0),
                        )
                        .repair("park the carrier with `out` before bringing it in again"));
                    }
                    st.active = true;
                }
            }
            KnitoutOp::Out { carriers: cs } => {
                ayab_transfer_acknowledged = false;
                check_unique_carriers(cs, "out", step)?;
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                        .repair(
                            "declare the carrier in `;;Carriers:` and MachineProfile.carrier_ids",
                        )
                    })?;
                    if !st.active || st.hook_holding {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!(
                                "op #{step}: carrier '{}' cannot execute `out` in current state",
                                c.0
                            ),
                        )
                        .repair("ensure the carrier is active and its yarn-inserting hook is released before `out`"));
                    }
                    st.active = false;
                    st.last_needle = None;
                    st.last_knit_needle = None;
                }
            }
            KnitoutOp::InHook { carriers: cs } => {
                ayab_transfer_acknowledged = false;
                if !profile.has_yarn_inserting_hooks {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: `inhook` forbidden on machine '{}' (uses direct grippers `in`/`out`, no yarn-inserting hooks)",
                            profile.id
                        ),
                    )
                    .repair("replace `inhook`/`releasehook`/`outhook` with gripper `in`/`out` for this profile"));
                }
                check_unique_carriers(cs, "inhook", step)?;
                if cs.len() > 1 || carriers.values().any(|s| s.hook_holding) {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: single yarn-inserting hook on '{}' cannot hold multiple carriers concurrently",
                            profile.id
                        ),
                    )
                    .repair("call `releasehook` on the currently held carrier before executing `inhook` on another carrier"));
                }
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                        .repair(
                            "declare the carrier in `;;Carriers:` and MachineProfile.carrier_ids",
                        )
                    })?;
                    if st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: carrier '{}' already active on `inhook`", c.0),
                        )
                        .repair("park the carrier with `outhook` before re-inserting it"));
                    }
                    st.active = true;
                    st.hook_holding = true;
                }
            }
            KnitoutOp::ReleaseHook { carriers: cs } => {
                ayab_transfer_acknowledged = false;
                if !profile.has_yarn_inserting_hooks {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: `releasehook` forbidden on machine '{}'",
                            profile.id
                        ),
                    )
                    .repair("remove `releasehook` on gripper-only machines"));
                }
                check_unique_carriers(cs, "releasehook", step)?;
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                        .repair(
                            "declare the carrier in `;;Carriers:` and MachineProfile.carrier_ids",
                        )
                    })?;
                    if !st.hook_holding {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!(
                                "op #{step}: `releasehook` without active `inhook` on '{}'",
                                c.0
                            ),
                        )
                        .repair("emit `inhook` before `releasehook` for each carrier"));
                    }
                    st.hook_holding = false;
                }
            }
            KnitoutOp::OutHook { carriers: cs } => {
                ayab_transfer_acknowledged = false;
                if !profile.has_yarn_inserting_hooks {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: `outhook` forbidden on machine '{}'",
                            profile.id
                        ),
                    )
                    .repair("replace `outhook` with gripper `out` for this profile"));
                }
                check_unique_carriers(cs, "outhook", step)?;
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                        .repair(
                            "declare the carrier in `;;Carriers:` and MachineProfile.carrier_ids",
                        )
                    })?;
                    if !st.active || st.hook_holding {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: invalid `outhook` on carrier '{}'", c.0),
                        )
                        .repair("call `releasehook` after cast-on before parking with `outhook`"));
                    }
                    st.active = false;
                    st.last_needle = None;
                    st.last_knit_needle = None;
                }
            }
            KnitoutOp::Rack { quarter_pitches } => {
                ayab_transfer_acknowledged = false;
                if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
                    && !CalibrationSwatchCaps::ALLOW_RACK
                    && *quarter_pitches != 0
                {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!("op #{step}: racking forbidden in CalibrationSwatchOnly mode"),
                    )
                    .repair(
                        "remove non-zero `rack` operations from uncalibrated swatch programs",
                    ));
                }
                if profile.backend_kind == MachineBackendKind::BrotherAyab
                    && !profile.automated_carriage_control
                    && *quarter_pitches != 0
                {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: motorized `rack` ({quarter_pitches} quarter-pitches) is unsupported on manual-carriage Brother AYAB '{}'",
                            profile.id
                        ),
                    )
                    .repair("remove motorized `rack` ops and record manual KR-850 racking handle changes as operator pauses"));
                }
                if (profile.requires_whole_pitch_racking
                    || profile.backend_kind == MachineBackendKind::KniterateKc)
                    && quarter_pitches % 4 != 0
                {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: machine '{}' requires whole-pitch racking (got {quarter_pitches} quarter-pitches)",
                            profile.id
                        ),
                    )
                    .repair("use integer-pitch racking (multiples of 4 quarter-pitches) on this machine"));
                }
                let abs_qp = quarter_pitches.unsigned_abs();
                if abs_qp > profile.max_racking_quarter_pitches as u32 {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: rack {} quarter-pitches exceeds machine limit ±{}",
                            quarter_pitches, profile.max_racking_quarter_pitches
                        ),
                    )
                    .repair("split wide shifts into multi-step transfers within the machine racking cap"));
                }
                if both_beds_live(&hooks) {
                    let delta_qp = active_bridge_racks
                        .iter()
                        .map(|&r0| quarter_pitches.abs_diff(r0))
                        .max()
                        .unwrap_or(abs_qp);
                    if delta_qp > 0 {
                        let dx = (f64::from(delta_qp) * profile.needle_pitch_mm) / 4.0;
                        let span = (profile.bed_gap_mm * profile.bed_gap_mm + dx * dx).sqrt();
                        // Conservative inter-bed unstretched yarn length (bed gap + ~2.2 pitches
                        // of draw-in slack from adjacent rib/transfer stitches).
                        let l0 = profile.bed_gap_mm + 2.2 * profile.needle_pitch_mm;
                        let strain = ((span - l0) / l0).max(0.0);
                        let max_allow = profile.racking_yarn_breaking_elongation / 2.0;
                        if strain > max_allow {
                            return Err(Refusal::new(
                                RefusalCode::MaterialProcessHazard,
                                format!(
                                    "op #{step}: cross-bed racking ({quarter_pitches} quarter-pitches, span {span:.2} mm) induces strain {:.2}% exceeding SF >= 2.0 limit {:.2}%",
                                    strain * 100.0,
                                    max_allow * 100.0
                                ),
                            )
                            .repair("split wide transfers into multi-step smaller racks, increase stitch cam slack, or select a higher-elongation yarn"));
                        }
                    }
                }
                rack_quarter_pitches = *quarter_pitches;
                max_abs_rack = max_abs_rack.max(abs_qp as i32);
            }
            KnitoutOp::Knit {
                dir,
                needle,
                carriers: cs,
            } => {
                ayab_transfer_acknowledged = false;
                check_bed_and_bounds(*needle, false, step)?;
                check_unique_carriers(cs, "knit", step)?;
                if prev_pass_dir != Some(*dir) {
                    estimated_courses += 1;
                    prev_pass_dir = Some(*dir);
                }
                record_needle_span(needle.index, &mut active_min_needle, &mut active_max_needle);
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                        .repair(
                            "declare the carrier in `;;Carriers:` and MachineProfile.carrier_ids",
                        )
                    })?;
                    if !st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: carrier '{}' knitted before `in`/`inhook`", c.0),
                        )
                        .repair(
                            "bring the carrier into action with `in` or `inhook` before knitting",
                        ));
                    }
                    if let Some(prev_k) = st.last_knit_needle {
                        let span = (i64::from(needle.index) - i64::from(prev_k))
                            .unsigned_abs()
                            .saturating_sub(1);
                        let float_mm = (span as f64) * effective_wale_pitch_mm;
                        max_float_mm = max_float_mm.max(float_mm);
                    }
                    st.last_needle = Some(needle.index);
                    st.last_knit_needle = Some(needle.index);
                }
                let h = hooks.entry((needle.bed, needle.index)).or_default();
                if h.loops > 0 && h.last_active_course > 0 {
                    max_roller_imbalance = max_roller_imbalance
                        .max(estimated_courses.saturating_sub(h.last_active_course + 1));
                }
                h.loops = 1;
                h.tuck_depth = 0;
                h.last_active_course = estimated_courses;
                max_loops_on_hook = max_loops_on_hook.max(h.loops);
                if both_beds_live(&hooks) {
                    active_bridge_racks.insert(rack_quarter_pitches);
                }
            }
            KnitoutOp::Tuck {
                dir,
                needle,
                carriers: cs,
            } => {
                ayab_transfer_acknowledged = false;
                check_bed_and_bounds(*needle, false, step)?;
                check_unique_carriers(cs, "tuck", step)?;
                if prev_pass_dir != Some(*dir) {
                    estimated_courses += 1;
                    prev_pass_dir = Some(*dir);
                }
                record_needle_span(needle.index, &mut active_min_needle, &mut active_max_needle);
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                        .repair(
                            "declare the carrier in `;;Carriers:` and MachineProfile.carrier_ids",
                        )
                    })?;
                    if !st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: carrier '{}' tucked before `in`/`inhook`", c.0),
                        )
                        .repair(
                            "bring the carrier into action with `in` or `inhook` before tucking",
                        ));
                    }
                    if let Some(prev_k) = st.last_knit_needle {
                        let span = (i64::from(needle.index) - i64::from(prev_k))
                            .unsigned_abs()
                            .saturating_sub(1);
                        let float_mm = (span as f64) * effective_wale_pitch_mm;
                        max_float_mm = max_float_mm.max(float_mm);
                    }
                    st.last_needle = Some(needle.index);
                    st.last_knit_needle = Some(needle.index);
                }
                let h = hooks.entry((needle.bed, needle.index)).or_default();
                if h.loops > 0 && h.last_active_course > 0 {
                    max_roller_imbalance = max_roller_imbalance
                        .max(estimated_courses.saturating_sub(h.last_active_course + 1));
                }
                let loops = u16::from(h.loops) + 1;
                let tuck_depth = u16::from(h.tuck_depth) + 1;
                if loops > u16::from(profile.max_loops_per_hook) {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: needle {} loop stack {loops} exceeds max_loops_per_hook {}",
                            needle.knitout_token(),
                            profile.max_loops_per_hook
                        ),
                    )
                    .repair("knit off accumulated loops on this needle before adding another tuck"));
                }
                if tuck_depth > u16::from(profile.max_tuck_stack) {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: needle {} tuck depth {tuck_depth} exceeds max_tuck_stack {}",
                            needle.knitout_token(),
                            profile.max_tuck_stack
                        ),
                    )
                    .repair("insert a knit pass to clear consecutive tucks on this hook"));
                }
                if h.loops == 0 {
                    h.last_active_course = estimated_courses;
                }
                h.loops = loops as u8;
                h.tuck_depth = tuck_depth as u8;
                max_loops_on_hook = max_loops_on_hook.max(h.loops);
                max_tuck_depth = max_tuck_depth.max(h.tuck_depth);
                if both_beds_live(&hooks) {
                    active_bridge_racks.insert(rack_quarter_pitches);
                }
            }
            KnitoutOp::Split {
                dir,
                from,
                to,
                carriers: cs,
            } => {
                if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
                    && !CalibrationSwatchCaps::ALLOW_TRANSFERS
                {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!("op #{step}: `split` forbidden in CalibrationSwatchOnly mode"),
                    )
                    .repair("remove `split` transfers from uncalibrated swatch programs"));
                }
                if profile.backend_kind == MachineBackendKind::BrotherAyab
                    && !ayab_transfer_acknowledged
                {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: unflagged `split` on manual-carriage Brother AYAB '{}' requires a preceding operator `pause`",
                            profile.id
                        ),
                    )
                    .repair("insert an explicit operator `pause ; hand-transfer ...` before any transfer/split on Brother AYAB"));
                }
                check_bed_and_bounds(*from, false, step)?;
                check_bed_and_bounds(*to, true, step)?;
                validate_bed_transfer_alignment(*from, *to, rack_quarter_pitches, step)?;
                check_unique_carriers(cs, "split", step)?;
                if prev_pass_dir != Some(*dir) {
                    estimated_courses += 1;
                    prev_pass_dir = Some(*dir);
                }
                record_needle_span(from.index, &mut active_min_needle, &mut active_max_needle);
                record_needle_span(to.index, &mut active_min_needle, &mut active_max_needle);
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                        .repair(
                            "declare the carrier in `;;Carriers:` and MachineProfile.carrier_ids",
                        )
                    })?;
                    if !st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: inactive carrier '{}' in `split`", c.0),
                        )
                        .repair(
                            "bring the carrier into action with `in` or `inhook` before `split`",
                        ));
                    }
                    if let Some(prev_k) = st.last_knit_needle {
                        let span = (i64::from(from.index) - i64::from(prev_k))
                            .unsigned_abs()
                            .saturating_sub(1);
                        let float_mm = (span as f64) * effective_wale_pitch_mm;
                        max_float_mm = max_float_mm.max(float_mm);
                    }
                    st.last_needle = Some(from.index);
                    st.last_knit_needle = Some(from.index);
                }
                let source = hooks
                    .get(&(from.bed, from.index))
                    .cloned()
                    .unwrap_or_default();
                if source.loops > 0 && source.last_active_course > 0 {
                    max_roller_imbalance = max_roller_imbalance
                        .max(estimated_courses.saturating_sub(source.last_active_course + 1));
                }
                if source.loops > 0 {
                    let dst = hooks.entry((to.bed, to.index)).or_default();
                    merge_transferred_hooks(dst, &source, *to, profile, step)?;
                    max_loops_on_hook = max_loops_on_hook.max(dst.loops);
                    max_tuck_depth = max_tuck_depth.max(dst.tuck_depth);
                }
                let src = hooks.entry((from.bed, from.index)).or_default();
                src.loops = 1;
                src.tuck_depth = 0;
                src.last_active_course = estimated_courses;
                total_transfers += 1;
                if both_beds_live(&hooks) {
                    active_bridge_racks.insert(rack_quarter_pitches);
                }
            }
            KnitoutOp::Xfer { from, to } => {
                if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
                    && !CalibrationSwatchCaps::ALLOW_TRANSFERS
                {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!("op #{step}: `xfer` forbidden in CalibrationSwatchOnly mode"),
                    )
                    .repair("remove `xfer` operations from uncalibrated swatch programs"));
                }
                if profile.backend_kind == MachineBackendKind::BrotherAyab
                    && !ayab_transfer_acknowledged
                {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: unflagged `xfer` on manual-carriage Brother AYAB '{}' requires a preceding operator `pause`",
                            profile.id
                        ),
                    )
                    .repair("insert an explicit operator `pause ; hand-transfer ...` before any transfer on Brother AYAB"));
                }
                check_bed_and_bounds(*from, true, step)?;
                check_bed_and_bounds(*to, true, step)?;
                validate_bed_transfer_alignment(*from, *to, rack_quarter_pitches, step)?;
                let source = hooks
                    .get(&(from.bed, from.index))
                    .cloned()
                    .unwrap_or_default();
                if source.loops > 0 {
                    let dst = hooks.entry((to.bed, to.index)).or_default();
                    merge_transferred_hooks(dst, &source, *to, profile, step)?;
                    max_loops_on_hook = max_loops_on_hook.max(dst.loops);
                    max_tuck_depth = max_tuck_depth.max(dst.tuck_depth);
                }
                if let Some(src) = hooks.get_mut(&(from.bed, from.index)) {
                    *src = HookState::default();
                }
                total_transfers += 1;
                if !both_beds_live(&hooks) {
                    active_bridge_racks.clear();
                } else if active_bridge_racks.is_empty() {
                    active_bridge_racks.insert(rack_quarter_pitches);
                }
            }
            KnitoutOp::Miss {
                needle,
                carriers: cs,
                ..
            } => {
                ayab_transfer_acknowledged = false;
                check_bed_and_bounds(*needle, false, step)?;
                check_unique_carriers(cs, "miss", step)?;
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                        .repair(
                            "declare the carrier in `;;Carriers:` and MachineProfile.carrier_ids",
                        )
                    })?;
                    if !st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: inactive carrier '{}' in `miss`", c.0),
                        )
                        .repair(
                            "bring the carrier into action with `in` or `inhook` before `miss`",
                        ));
                    }
                    st.last_needle = Some(needle.index);
                }
            }
            KnitoutOp::Amiss { needle } => {
                ayab_transfer_acknowledged = false;
                check_bed_and_bounds(*needle, false, step)?;
            }
            KnitoutOp::Drop { needle } => {
                check_bed_and_bounds(*needle, true, step)?;
                if let Some(h) = hooks.get_mut(&(needle.bed, needle.index)) {
                    h.loops = 0;
                    h.tuck_depth = 0;
                    h.last_active_course = 0;
                }
                if !both_beds_live(&hooks) {
                    active_bridge_racks.clear();
                }
            }
            KnitoutOp::CarrierSpacing { needles } => {
                if *needles < profile.min_carrier_spacing {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: carrier spacing {needles} below machine minimum {}",
                            profile.min_carrier_spacing
                        ),
                    )
                    .repair(
                        "set `x-carrier-spacing` to at least MachineProfile.min_carrier_spacing",
                    ));
                }
            }
            KnitoutOp::Stitch { leading, trailing } => {
                let lo = profile.cam_setting_range[0] - 1e-6;
                let hi = profile.cam_setting_range[1] + 1e-6;
                let lead_cam = f64::from(*leading) / 10.0;
                let trail_cam = f64::from(*trailing) / 10.0;
                if lead_cam < lo || lead_cam > hi || trail_cam < lo || trail_cam > hi {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!(
                            "op #{step}: stitch cam setting {lead_cam:.1}/{trail_cam:.1} outside machine '{}' cam_setting_range {:?}",
                            profile.id, profile.cam_setting_range
                        ),
                    )
                    .repair("adjust cam stitch numbers to fall inside MachineProfile.cam_setting_range"));
                }
            }
            KnitoutOp::SpeedNumber { speed } => {
                if !profile.automated_carriage_control {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: motorized `x-speed-number` is unsupported on manual-carriage machine '{}'",
                            profile.id
                        ),
                    )
                    .repair("remove `x-speed-number` directives when targeting a manual-carriage machine"));
                }
                if *speed == 0 {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!("op #{step}: `x-speed-number` must be positive"),
                    )
                    .repair("set a positive carriage speed number"));
                }
                if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
                    && profile.backend_kind == MachineBackendKind::KniterateKc
                    && *speed > 150
                {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!(
                            "op #{step}: CalibrationSwatchOnly speed {speed} exceeds Kniterate swatch cap 150"
                        ),
                    )
                    .repair("cap `x-speed-number` to <= 150 on uncalibrated Kniterate swatches"));
                }
            }
            KnitoutOp::Pause { .. } => {
                if !profile.supports_pause {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!("op #{step}: machine '{}' does not support operator `pause`", profile.id),
                    )
                    .repair("remove `pause` operations or select a machine profile supporting operator pauses"));
                }
                ayab_transfer_acknowledged = true;
            }
            KnitoutOp::AddRollerAdvance { .. } => {
                if !profile.automated_carriage_control {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: motorized `x-add-roller-advance` is unsupported on manual-carriage machine '{}'",
                            profile.id
                        ),
                    )
                    .repair("remove roller-advance commands and manage takedown weights manually on this machine"));
                }
            }
            KnitoutOp::PresserMode { .. } => {
                if !profile.supports_presser || !profile.automated_carriage_control {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: `x-presser-mode` is unsupported on machine '{}'",
                            profile.id
                        ),
                    )
                    .repair("remove `x-presser-mode` directives or select a machine profile equipped with presser-foot control"));
                }
            }
            KnitoutOp::Comment { .. } => {}
        }
    }

    // Check active carriers cleanly parked at end
    for (cid, st) in &carriers {
        if st.active || st.hook_holding {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!("carrier '{cid}' was not parked (`out`/`outhook`) before end of program"),
            )
            .repair("park every active carrier with `out` or `outhook` before EOF"));
        }
    }

    // Check CalibrationSwatchOnly needle span & course caps
    let active_needle_range = match (active_min_needle, active_max_needle) {
        (Some(lo), Some(hi)) => Some([lo, hi]),
        _ => None,
    };
    if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        if let Some([lo, hi]) = active_needle_range {
            let span = (i64::from(hi) - i64::from(lo) + 1) as u64;
            if span > u64::from(CalibrationSwatchCaps::MAX_NEEDLES) {
                return Err(Refusal::new(
                    RefusalCode::MaterialProcessHazard,
                    format!(
                        "calibration swatch needle span {span} exceeds hard cap {}",
                        CalibrationSwatchCaps::MAX_NEEDLES
                    ),
                )
                .repair("reduce calibration swatch width to <= 80 needles"));
            }
        }
        if estimated_courses > CalibrationSwatchCaps::MAX_COURSES {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "calibration swatch course count {estimated_courses} exceeds hard cap {}",
                    CalibrationSwatchCaps::MAX_COURSES
                ),
            )
            .repair("reduce calibration swatch length to <= 200 courses"));
        }
    }

    // Check 5: Roller-imbalance bound across needles still holding live loops (catches genuine
    // short-row pileup without falsely flagging empty transferred rib/armhole hooks!)
    for h in hooks.values() {
        if h.loops > 0 && h.last_active_course > 0 {
            max_roller_imbalance =
                max_roller_imbalance.max(estimated_courses.saturating_sub(h.last_active_course));
        }
    }
    if max_roller_imbalance > profile.max_roller_imbalance_courses {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "short-row roller imbalance ({max_roller_imbalance} courses) exceeds machine cap {}",
                profile.max_roller_imbalance_courses
            ),
        )
        .repair("distribute short rows symmetrically across opposing edges or reduce held-loop depth"));
    }

    // Check 6: Child float length gate (measured over max(needle_pitch_mm, blocked_wale_pitch_mm))
    if ctx.wearer_age_class.is_child() && max_float_mm > profile.child_float_cap_mm {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "child-safety refusal: interior float {max_float_mm:.1} mm exceeds {:.1} mm finger-snag cap",
                profile.child_float_cap_mm
            ),
        )
        .repair("add birdseye/ladderback tacking every <= 3 needles or switch to solid/dyed yarn"));
    }

    Ok(VBedVmReport {
        program_sha256: program.canonical_text_sha256()?,
        machine_sha256: profile.canonical_sha256()?,
        checks_passed: vec![
            "1_carrier_lifecycle_and_hooks".to_string(),
            "2_needle_bed_bounds_and_slider_ban".to_string(),
            "3_hook_stack_and_tuck_caps".to_string(),
            "4_racking_and_transfer_alignment".to_string(),
            "5_roller_balance_and_float_bounds".to_string(),
            "6_wearer_age_and_child_safety".to_string(),
            "7_material_cam_and_swatch_caps".to_string(),
        ],
        op_count: program.ops.len(),
        active_needle_range,
        max_loops_on_hook,
        max_tuck_depth,
        max_float_mm,
        max_roller_imbalance,
        total_transfers,
        max_abs_rack_quarter_pitches: max_abs_rack,
        estimated_courses,
    })
}

// Moving held loops does not knit them off. Preserve the oldest held course and all
// pending tucks even when a stack visits an empty hook or joins a newer stack.
fn merge_transferred_hooks(
    destination: &mut HookState,
    source: &HookState,
    needle: Needle,
    profile: &MachineProfile,
    step: usize,
) -> Result<(), Refusal> {
    let loops = u16::from(destination.loops) + u16::from(source.loops);
    let tuck_depth = u16::from(destination.tuck_depth) + u16::from(source.tuck_depth);
    if loops > u16::from(profile.max_loops_per_hook) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "op #{step}: transfer destination {} stack {loops} exceeds max_loops_per_hook {}",
                needle.knitout_token(),
                profile.max_loops_per_hook
            ),
        )
        .repair("knit off held loops before stacking more loops on this needle"));
    }
    if tuck_depth > u16::from(profile.max_tuck_stack) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "op #{step}: transfer destination {} tuck depth {tuck_depth} exceeds max_tuck_stack {}",
                needle.knitout_token(),
                profile.max_tuck_stack
            ),
        )
        .repair("knit off the held tucks before merging these stacks"));
    }
    destination.last_active_course = if destination.loops == 0 {
        source.last_active_course
    } else {
        destination
            .last_active_course
            .min(source.last_active_course)
    };
    destination.loops = loops as u8;
    destination.tuck_depth = tuck_depth as u8;
    Ok(())
}

fn validate_bed_transfer_alignment(
    from: Needle,
    to: Needle,
    rack_quarter_pitches: i32,
    step: usize,
) -> Result<(), Refusal> {
    let from_front = matches!(from.bed, Bed::Front | Bed::FrontSlider);
    let to_front = matches!(to.bed, Bed::Front | Bed::FrontSlider);
    if from_front == to_front {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "op #{step}: transfer {} -> {} must cross between front and back beds",
                from.knitout_token(),
                to.knitout_token()
            ),
        )
        .repair("transfer across to an empty opposite-bed needle, rack, and transfer back"));
    }
    if rack_quarter_pitches % 4 != 0 {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "op #{step}: transfer {} -> {} requires integer-pitch rack (current {} quarter-pitches)",
                from.knitout_token(),
                to.knitout_token(),
                rack_quarter_pitches
            ),
        )
        .repair("set `rack` to an integer needle pitch before executing `xfer` or `split`"));
    }
    let rack_int = i64::from(rack_quarter_pitches / 4);
    // Standard Knitout convention: back needle `b_j` aligns with front needle `f_{j + rack}`
    let aligned = if from_front {
        i64::from(from.index) == i64::from(to.index) + rack_int
    } else {
        i64::from(to.index) == i64::from(from.index) + rack_int
    };
    if !aligned {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "op #{step}: misaligned transfer {} -> {} at rack {rack_int}",
                from.knitout_token(),
                to.knitout_token()
            ),
        )
        .repair("align front and back needle indices with `rack` before transferring"));
    }
    Ok(())
}

/// Physical parameters for geometric cross-bed racking-strain verification (`FTX-K03` mode #2):
/// $\ell_{\text{span}} = \sqrt{g^2 + (r \cdot p / 4)^2}$ compared against unstretched bridge
/// length $\ell_0 = g + \text{available\_loop\_slack\_mm}$ and yarn breaking elongation with
/// safety factor $\text{SF} \ge 2.0$.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackingStrainSpec {
    /// Physical needle-bed clearance gap `g`, in mm (`> 0`).
    pub bed_gap_mm: f64,
    /// Unstretched inter-bed yarn slack drawn from adjacent stitches beyond `bed_gap_mm`, in mm (`>= 0`).
    pub available_loop_slack_mm: f64,
    /// Measured yarn breaking elongation fraction (e.g. `0.029` for 2.90% wool, `0.052` for 5.20% cotton).
    /// `None` denotes `UNCERTAIN` and refuses fail-closed whenever cross-bed racking occurs!
    pub yarn_breaking_elongation: Option<f64>,
    /// Required tensile elongation safety factor (`>= 2.0`).
    pub safety_factor: f64,
}

impl RackingStrainSpec {
    /// Builds a default envelope from `profile.bed_gap_mm` with `SF = 2.0`.
    #[must_use]
    pub fn for_profile(
        profile: &MachineProfile,
        available_loop_slack_mm: f64,
        yarn_breaking_elongation: Option<f64>,
    ) -> Self {
        Self {
            bed_gap_mm: profile.bed_gap_mm,
            available_loop_slack_mm,
            yarn_breaking_elongation,
            safety_factor: 2.0,
        }
    }
}

/// Verified cross-bed racking-strain telemetry (`FTX-K03`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackingStrainReport {
    /// Maximum geometric inter-bed span $\sqrt{g^2 + (r \cdot p / 4)^2}$ observed during cross-bed racking, in mm.
    pub max_racking_span_mm: f64,
    /// Peak geometric elongation fraction `(l_span - l0) / l0` (`>= 0`).
    pub max_racking_strain: f64,
    /// Allowable elongation ceiling `eps_break / safety_factor` (`None` if no cross-bed racking occurred and elongation was omitted).
    pub allowable_strain: Option<f64>,
    /// Number of non-zero cross-bed racking events evaluated while both beds held live loops.
    pub checked_racking_events: usize,
}

/// Checks diagonal cross-bed racking yarn-span breakage ($\sqrt{g^2 + (r \cdot p / 4)^2}$) across
/// every `Rack` step while both front and back beds hold live loops (`FTX-K03` mode #2).
///
/// # Errors
/// [`RefusalCode::MaterialProcessHazard`] if cross-bed racking occurs with `UNCERTAIN`
/// (`yarn_breaking_elongation: None`) or exceeds `eps_break / safety_factor`; [`RefusalCode::OutOfRange`]
/// or [`RefusalCode::NonFinite`] for invalid specification values.
pub fn verify_racking_strain(
    program: &KnitoutProgram,
    profile: &MachineProfile,
    spec: &RackingStrainSpec,
) -> Result<RackingStrainReport, Refusal> {
    profile.validate()?;
    Refusal::finite("bed_gap_mm", spec.bed_gap_mm)?;
    Refusal::finite("available_loop_slack_mm", spec.available_loop_slack_mm)?;
    Refusal::finite("safety_factor", spec.safety_factor)?;
    if spec.bed_gap_mm <= 0.0 || spec.available_loop_slack_mm < 0.0 || spec.safety_factor < 2.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "racking strain verification requires bed_gap_mm > 0, available_loop_slack_mm >= 0, and safety_factor >= 2.0",
        )
        .repair("provide positive bed_gap_mm, non-negative loop slack, and safety_factor >= 2.0"));
    }
    if let Some(eps) = spec.yarn_breaking_elongation {
        Refusal::finite("yarn_breaking_elongation", eps)?;
        if !(eps > 0.0 && eps <= 5.0) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("yarn_breaking_elongation {eps} must lie in (0, 5.0]"),
            )
            .repair("provide measured yarn breaking elongation fraction (e.g. 0.029 for 2.9%)"));
        }
    }

    let l0 = spec.bed_gap_mm + spec.available_loop_slack_mm;
    let allowable = spec
        .yarn_breaking_elongation
        .map(|eps| eps / spec.safety_factor);
    let mut front_live: BTreeSet<(Bed, i32)> = BTreeSet::new();
    let mut back_live: BTreeSet<(Bed, i32)> = BTreeSet::new();
    let mut current_rack: i32 = 0;
    let mut active_bridge_racks: BTreeSet<i32> = BTreeSet::new();
    let mut max_span = spec.bed_gap_mm;
    let mut max_strain = 0.0_f64;
    let mut checked = 0usize;

    let set_live =
        |n: Needle, occupied: bool, f: &mut BTreeSet<(Bed, i32)>, b: &mut BTreeSet<(Bed, i32)>| {
            let target = if matches!(n.bed, Bed::Front | Bed::FrontSlider) {
                f
            } else {
                b
            };
            if occupied {
                target.insert((n.bed, n.index));
            } else {
                target.remove(&(n.bed, n.index));
            }
        };

    for (step, op) in program.ops.iter().enumerate() {
        match op {
            KnitoutOp::Knit { needle, .. } | KnitoutOp::Tuck { needle, .. } => {
                set_live(*needle, true, &mut front_live, &mut back_live);
                if !front_live.is_empty() && !back_live.is_empty() {
                    active_bridge_racks.insert(current_rack);
                }
            }
            KnitoutOp::Split { from, to, .. } => {
                let had_src = if matches!(from.bed, Bed::Front | Bed::FrontSlider) {
                    front_live.contains(&(from.bed, from.index))
                } else {
                    back_live.contains(&(from.bed, from.index))
                };
                if had_src {
                    set_live(*to, true, &mut front_live, &mut back_live);
                }
                set_live(*from, true, &mut front_live, &mut back_live);
                if !front_live.is_empty() && !back_live.is_empty() {
                    active_bridge_racks.insert(current_rack);
                }
            }
            KnitoutOp::Xfer { from, to } => {
                let had_src = if matches!(from.bed, Bed::Front | Bed::FrontSlider) {
                    front_live.remove(&(from.bed, from.index))
                } else {
                    back_live.remove(&(from.bed, from.index))
                };
                if had_src {
                    set_live(*to, true, &mut front_live, &mut back_live);
                }
                if front_live.is_empty() || back_live.is_empty() {
                    active_bridge_racks.clear();
                } else if active_bridge_racks.is_empty() {
                    // Single-bed fabric just bridged onto the opposite bed at `current_rack`.
                    active_bridge_racks.insert(current_rack);
                }
            }
            KnitoutOp::Drop { needle } => {
                set_live(*needle, false, &mut front_live, &mut back_live);
                if front_live.is_empty() || back_live.is_empty() {
                    active_bridge_racks.clear();
                }
            }
            KnitoutOp::Rack { quarter_pitches } => {
                if !front_live.is_empty() && !back_live.is_empty() {
                    let delta_qp = active_bridge_racks
                        .iter()
                        .map(|&r0| quarter_pitches.abs_diff(r0))
                        .max()
                        .unwrap_or_else(|| quarter_pitches.unsigned_abs());
                    if delta_qp > 0 {
                        let Some(max_allow) = allowable else {
                            return Err(Refusal::new(
                                RefusalCode::MaterialProcessHazard,
                                format!(
                                    "op #{step}: cross-bed racking ({quarter_pitches} quarter-pitches, delta {delta_qp}) refused because yarn_breaking_elongation is UNCERTAIN"
                                ),
                            )
                            .repair("measure or supply source-qualified yarn breaking elongation before cross-bed racking")
                            .repair("or eliminate cross-bed racking while both beds hold live loops"));
                        };
                        let dx = (f64::from(delta_qp) * profile.needle_pitch_mm) / 4.0;
                        let span = (spec.bed_gap_mm * spec.bed_gap_mm + dx * dx).sqrt();
                        let strain = ((span - l0) / l0).max(0.0);
                        checked += 1;
                        max_span = max_span.max(span);
                        max_strain = max_strain.max(strain);
                        if strain > max_allow {
                            return Err(Refusal::new(
                                RefusalCode::MaterialProcessHazard,
                                format!(
                                    "op #{step}: cross-bed racking ({quarter_pitches} quarter-pitches, delta {delta_qp}, span {span:.2} mm) induces strain {:.2}% exceeding allowable {:.2}% (SF >= {:.1})",
                                    strain * 100.0,
                                    max_allow * 100.0,
                                    spec.safety_factor
                                ),
                            )
                            .repair("split the transfer into smaller racking steps, increase stitch cam loop slack, or choose a higher-elongation yarn"));
                        }
                    }
                }
                current_rack = *quarter_pitches;
            }
            _ => {}
        }
    }

    Ok(RackingStrainReport {
        max_racking_span_mm: max_span,
        max_racking_strain: max_strain,
        allowable_strain: allowable,
        checked_racking_events: checked,
    })
}

/// Runs both [`verify_knitout_program`] and [`verify_racking_strain`] atomically (`FTX-K03`).
///
/// # Errors
/// Returns any discrete VM, child-safety, or racking-strain refusal.
pub fn verify_knitout_program_with_racking_strain(
    program: &KnitoutProgram,
    profile: &MachineProfile,
    ctx: &KnitSafetyContext<'_>,
    strain_spec: &RackingStrainSpec,
) -> Result<(VBedVmReport, RackingStrainReport), Refusal> {
    let vm_report = verify_knitout_program(program, profile, ctx)?;
    let strain_report = verify_racking_strain(program, profile, strain_spec)?;
    Ok((vm_report, strain_report))
}

/// Synthesizes a deterministic Regime III inline yarn-dyeing intake timeline
/// ([`Regime3IntakeTimeline`]) directly from a Knitout v2 program and [`MachineProfile`] timing
/// kinematics (`FTX-K03` -> `FTX-K16` bridge).
///
/// Consecutive same-direction loop-forming operations (`Knit`/`Tuck`/`Split`) form [`IntakeEventKind::KnitPass`]
/// intervals preserving exact yarn mass (`duration_s * yarn_intake_speed_m_s == n_loops * loop_length_mm * 0.001`),
/// while carriage reversals, transfer passes, racks, and operator pauses emit zero-feed dwell events
/// for dancer-arm accumulator ODE verification.
///
/// # Errors
/// [`RefusalCode::OutOfRange`], [`RefusalCode::NonFinite`], or [`RefusalCode::DegenerateGeometry`].
pub fn synthesize_regime3_intake_timeline(
    program: &KnitoutProgram,
    profile: &MachineProfile,
    loop_length_mm: f64,
    dt_verify_s: f64,
) -> Result<Regime3IntakeTimeline, Refusal> {
    profile.validate()?;
    let kin = profile.timing;
    kin.validate()?;
    Refusal::finite("loop_length_mm", loop_length_mm)?;
    Refusal::finite("dt_verify_s", dt_verify_s)?;
    if loop_length_mm <= 0.0 || dt_verify_s <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "Regime III intake timeline synthesis requires loop_length_mm > 0 and dt_verify_s > 0",
        )
        .repair("pass calibrated positive loop_length_mm and dt_verify_s"));
    }

    let mut events: Vec<IntakeEvent> = Vec::new();
    let mut pass_dir: Option<CourseDir> = None;
    let mut pass_min: i32 = i32::MAX;
    let mut pass_max: i32 = i32::MIN;
    let mut pass_loops: u64 = 0;
    let mut last_completed_dir: Option<CourseDir> = None;
    let mut in_xfer_batch = false;

    let flush_pass = |events: &mut Vec<IntakeEvent>,
                      pass_dir: &mut Option<CourseDir>,
                      pass_min: &mut i32,
                      pass_max: &mut i32,
                      pass_loops: &mut u64,
                      last_completed_dir: &mut Option<CourseDir>| {
        if let Some(dir) = pass_dir.take()
            && *pass_loops > 0
        {
            if let Some(prev) = *last_completed_dir
                && prev != dir
                && kin.reversal_dwell_s > 0.0
            {
                events.push(IntakeEvent {
                    duration_s: kin.reversal_dwell_s,
                    yarn_intake_speed_m_s: 0.0,
                    kind: IntakeEventKind::ReversalDwell,
                });
            }
            let travel_needles =
                ((i64::from(*pass_max) - i64::from(*pass_min)).unsigned_abs() + 1).max(1) as f64;
            let travel_m = travel_needles * profile.needle_pitch_mm * 0.001;
            let duration_s = (travel_m / kin.carriage_speed_m_s).max(1e-4);
            let yarn_m = (*pass_loops as f64) * loop_length_mm * 0.001;
            events.push(IntakeEvent {
                duration_s,
                yarn_intake_speed_m_s: yarn_m / duration_s,
                kind: IntakeEventKind::KnitPass,
            });
            *last_completed_dir = Some(dir);
            *pass_min = i32::MAX;
            *pass_max = i32::MIN;
            *pass_loops = 0;
        }
    };

    for op in &program.ops {
        match op {
            KnitoutOp::Knit { dir, needle, .. } | KnitoutOp::Tuck { dir, needle, .. } => {
                in_xfer_batch = false;
                if pass_dir.is_some() && pass_dir != Some(*dir) {
                    flush_pass(
                        &mut events,
                        &mut pass_dir,
                        &mut pass_min,
                        &mut pass_max,
                        &mut pass_loops,
                        &mut last_completed_dir,
                    );
                }
                pass_dir = Some(*dir);
                pass_min = pass_min.min(needle.index);
                pass_max = pass_max.max(needle.index);
                pass_loops += 1;
            }
            KnitoutOp::Split { dir, from, .. } => {
                in_xfer_batch = false;
                if pass_dir.is_some() && pass_dir != Some(*dir) {
                    flush_pass(
                        &mut events,
                        &mut pass_dir,
                        &mut pass_min,
                        &mut pass_max,
                        &mut pass_loops,
                        &mut last_completed_dir,
                    );
                }
                pass_dir = Some(*dir);
                pass_min = pass_min.min(from.index);
                pass_max = pass_max.max(from.index);
                pass_loops += 1;
            }
            KnitoutOp::Xfer { .. } => {
                flush_pass(
                    &mut events,
                    &mut pass_dir,
                    &mut pass_min,
                    &mut pass_max,
                    &mut pass_loops,
                    &mut last_completed_dir,
                );
                if !in_xfer_batch {
                    events.push(IntakeEvent {
                        duration_s: kin.xfer_cycle_s,
                        yarn_intake_speed_m_s: 0.0,
                        kind: IntakeEventKind::TransferPause,
                    });
                    in_xfer_batch = true;
                }
            }
            KnitoutOp::Rack { .. } => {
                in_xfer_batch = false;
                flush_pass(
                    &mut events,
                    &mut pass_dir,
                    &mut pass_min,
                    &mut pass_max,
                    &mut pass_loops,
                    &mut last_completed_dir,
                );
                events.push(IntakeEvent {
                    duration_s: kin.rack_cycle_s,
                    yarn_intake_speed_m_s: 0.0,
                    kind: IntakeEventKind::RackPause,
                });
            }
            KnitoutOp::Pause { .. } => {
                in_xfer_batch = false;
                flush_pass(
                    &mut events,
                    &mut pass_dir,
                    &mut pass_min,
                    &mut pass_max,
                    &mut pass_loops,
                    &mut last_completed_dir,
                );
                if kin.pause_budget_s > 0.0 {
                    events.push(IntakeEvent {
                        duration_s: kin.pause_budget_s,
                        yarn_intake_speed_m_s: 0.0,
                        kind: IntakeEventKind::OperatorPause,
                    });
                }
            }
            KnitoutOp::Comment { .. } => {}
            _ => {
                in_xfer_batch = false;
            }
        }
    }
    flush_pass(
        &mut events,
        &mut pass_dir,
        &mut pass_min,
        &mut pass_max,
        &mut pass_loops,
        &mut last_completed_dir,
    );

    if events.is_empty() {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "cannot synthesize Regime III intake timeline from an empty Knitout program",
        )
        .repair(
            "compile a non-empty knitted garment or swatch before synthesizing an intake timeline",
        ));
    }

    Ok(Regime3IntakeTimeline {
        events,
        dt_verify_s,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knit::ir::{KNITOUT_SCHEMA, KnitoutHeader};

    fn minimal_program(profile: &MachineProfile, ops: Vec<KnitoutOp>) -> KnitoutProgram {
        KnitoutProgram {
            schema: KNITOUT_SCHEMA.to_string(),
            header: KnitoutHeader {
                machine: profile.id.clone(),
                gauge: format!("{}", profile.gauge_e),
                width: profile.needle_count,
                position: "Center".to_string(),
                carriers: vec![CarrierId("1".to_string())],
                extra: BTreeMap::new(),
            },
            ops,
        }
    }

    fn test_context(profile: &MachineProfile, age: WearerAgeClass) -> KnitSafetyContext<'_> {
        KnitSafetyContext {
            wearer_age_class: age,
            blocked_wale_pitch_mm: profile.needle_pitch_mm,
            source_doc: None,
            speed_fraction: None,
            takedown_fraction: None,
        }
    }

    fn yarn_in() -> KnitoutOp {
        KnitoutOp::In {
            carriers: vec![CarrierId("1".to_string())],
        }
    }

    fn yarn_out() -> KnitoutOp {
        KnitoutOp::Out {
            carriers: vec![CarrierId("1".to_string())],
        }
    }

    fn knit_at(dir: CourseDir, bed: Bed, index: i32) -> KnitoutOp {
        KnitoutOp::Knit {
            dir,
            needle: Needle::new(bed, index),
            carriers: vec![CarrierId("1".to_string())],
        }
    }

    fn tuck_at(dir: CourseDir, bed: Bed, index: i32) -> KnitoutOp {
        KnitoutOp::Tuck {
            dir,
            needle: Needle::new(bed, index),
            carriers: vec![CarrierId("1".to_string())],
        }
    }

    #[test]
    fn vm_measures_floats_across_reversals_and_misses() {
        let profile = MachineProfile::kniterate_7e();
        let ctx = test_context(&profile, WearerAgeClass::Child3To14);
        for with_miss in [false, true] {
            let mut ops = vec![yarn_in(), knit_at(CourseDir::Plus, Bed::Front, 0)];
            if with_miss {
                ops.push(KnitoutOp::Miss {
                    dir: CourseDir::Minus,
                    needle: Needle::new(Bed::Front, 8),
                    carriers: vec![CarrierId("1".to_string())],
                });
            }
            ops.push(knit_at(CourseDir::Minus, Bed::Front, 8));
            ops.push(yarn_out());
            let err = verify_knitout_program(&minimal_program(&profile, ops), &profile, &ctx)
                .unwrap_err();
            assert_eq!(err.code, RefusalCode::MaterialProcessHazard);
            assert!(err.message.contains("interior float"));
        }

        let no_span = minimal_program(
            &profile,
            vec![
                yarn_in(),
                knit_at(CourseDir::Plus, Bed::Front, 0),
                knit_at(CourseDir::Minus, Bed::Back, 0),
                yarn_out(),
            ],
        );
        assert_eq!(
            verify_knitout_program(&no_span, &profile, &ctx)
                .unwrap()
                .max_float_mm,
            0.0
        );
    }

    #[test]
    fn vm_preserves_tuck_depth_through_transfer_and_split() {
        let mut profile = MachineProfile::kniterate_7e();
        profile.max_loops_per_hook = 4;
        profile.max_tuck_stack = 1;
        let ctx = test_context(&profile, WearerAgeClass::Adult);
        for transfer in [
            KnitoutOp::Xfer {
                from: Needle::new(Bed::Front, 10),
                to: Needle::new(Bed::Back, 10),
            },
            KnitoutOp::Split {
                dir: CourseDir::Plus,
                from: Needle::new(Bed::Front, 10),
                to: Needle::new(Bed::Back, 10),
                carriers: vec![CarrierId("1".to_string())],
            },
        ] {
            let ops = vec![
                yarn_in(),
                tuck_at(CourseDir::Plus, Bed::Front, 10),
                transfer,
                tuck_at(CourseDir::Minus, Bed::Back, 10),
                yarn_out(),
            ];
            let err = verify_knitout_program(&minimal_program(&profile, ops), &profile, &ctx)
                .unwrap_err();
            assert_eq!(err.code, RefusalCode::MachineStateInfeasible);
            assert!(err.message.contains("tuck depth"));
        }
    }

    #[test]
    fn vm_does_not_refresh_held_loops_when_merging_into_a_newer_stack() {
        let mut profile = MachineProfile::kniterate_7e();
        profile.max_roller_imbalance_courses = 1;
        let ctx = test_context(&profile, WearerAgeClass::Adult);
        let ops = vec![
            yarn_in(),
            knit_at(CourseDir::Plus, Bed::Front, 10),
            knit_at(CourseDir::Minus, Bed::Front, 11),
            knit_at(CourseDir::Plus, Bed::Front, 11),
            KnitoutOp::Xfer {
                from: Needle::new(Bed::Front, 10),
                to: Needle::new(Bed::Back, 10),
            },
            KnitoutOp::Rack { quarter_pitches: 4 },
            KnitoutOp::Xfer {
                from: Needle::new(Bed::Back, 10),
                to: Needle::new(Bed::Front, 11),
            },
            yarn_out(),
        ];
        let err =
            verify_knitout_program(&minimal_program(&profile, ops), &profile, &ctx).unwrap_err();
        assert_eq!(err.code, RefusalCode::MaterialProcessHazard);
        assert!(err.message.contains("roller imbalance"));
    }

    #[test]
    fn vm_transfers_preserve_valid_stacks_and_refuse_count_overflow() {
        let profile = MachineProfile::kniterate_7e();
        let ctx = test_context(&profile, WearerAgeClass::Adult);
        let ops = vec![
            yarn_in(),
            tuck_at(CourseDir::Plus, Bed::Front, 10),
            KnitoutOp::Xfer {
                from: Needle::new(Bed::Front, 10),
                to: Needle::new(Bed::Back, 10),
            },
            tuck_at(CourseDir::Minus, Bed::Back, 10),
            knit_at(CourseDir::Plus, Bed::Back, 10),
            yarn_out(),
        ];
        let report =
            verify_knitout_program(&minimal_program(&profile, ops), &profile, &ctx).unwrap();
        assert_eq!(report.max_loops_on_hook, 2);
        assert_eq!(report.max_tuck_depth, 2);
        assert_eq!(report.total_transfers, 1);

        let mut wide_profile = profile;
        wide_profile.max_loops_per_hook = u8::MAX;
        wide_profile.max_tuck_stack = u8::MAX;
        let source = HookState {
            loops: 1,
            tuck_depth: 0,
            last_active_course: 1,
        };
        let mut full_hook = HookState {
            loops: u8::MAX,
            tuck_depth: 0,
            last_active_course: 2,
        };
        assert_eq!(
            merge_transferred_hooks(
                &mut full_hook,
                &source,
                Needle::new(Bed::Back, 10),
                &wide_profile,
                0,
            )
            .unwrap_err()
            .code,
            RefusalCode::MachineStateInfeasible
        );
        assert_eq!(full_hook.loops, u8::MAX);
        assert_eq!(full_hook.last_active_course, 2);

        let mut too_many_tucks = vec![yarn_in()];
        too_many_tucks.extend((0..256).map(|_| tuck_at(CourseDir::Plus, Bed::Front, 10)));
        too_many_tucks.push(yarn_out());
        let ctx = test_context(&wide_profile, WearerAgeClass::Adult);
        assert_eq!(
            verify_knitout_program(
                &minimal_program(&wide_profile, too_many_tucks),
                &wide_profile,
                &ctx,
            )
            .unwrap_err()
            .code,
            RefusalCode::MachineStateInfeasible
        );
    }

    #[test]
    fn vm_rejects_nan_speed_takedown_i32_min_rack_and_empty_carrier_ops() {
        let prof = MachineProfile::kniterate_7e();
        let ok_prog = minimal_program(
            &prof,
            vec![
                KnitoutOp::In {
                    carriers: vec![CarrierId("1".to_string())],
                },
                KnitoutOp::Knit {
                    dir: CourseDir::Plus,
                    needle: Needle::new(Bed::Front, 10),
                    carriers: vec![CarrierId("1".to_string())],
                },
                KnitoutOp::Out {
                    carriers: vec![CarrierId("1".to_string())],
                },
            ],
        );

        let nan_speed_ctx = KnitSafetyContext {
            wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
            blocked_wale_pitch_mm: prof.needle_pitch_mm,
            source_doc: None,
            speed_fraction: Some(f64::NAN),
            takedown_fraction: Some(0.5),
        };
        assert_eq!(
            verify_knitout_program(&ok_prog, &prof, &nan_speed_ctx)
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );

        let neg_takedown_ctx = KnitSafetyContext {
            wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
            blocked_wale_pitch_mm: prof.needle_pitch_mm,
            source_doc: None,
            speed_fraction: Some(0.4),
            takedown_fraction: Some(-0.1),
        };
        assert_eq!(
            verify_knitout_program(&ok_prog, &prof, &neg_takedown_ctx)
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );

        let min_rack = minimal_program(
            &prof,
            vec![KnitoutOp::Rack {
                quarter_pitches: i32::MIN,
            }],
        );
        let adult_ctx = KnitSafetyContext {
            wearer_age_class: WearerAgeClass::Adult,
            blocked_wale_pitch_mm: prof.needle_pitch_mm,
            source_doc: None,
            speed_fraction: None,
            takedown_fraction: None,
        };
        assert_eq!(
            verify_knitout_program(&min_rack, &prof, &adult_ctx)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );

        let empty_in = minimal_program(&prof, vec![KnitoutOp::In { carriers: vec![] }]);
        assert_eq!(
            verify_knitout_program(&empty_in, &prof, &adult_ctx)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );

        let mut wide_hdr = ok_prog;
        wide_hdr.header.width = prof.needle_count + 1;
        assert_eq!(
            verify_knitout_program(&wide_hdr, &prof, &adult_ctx)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
    }

    #[test]
    fn vm_enforces_child_float_cap_across_tuck_and_split() {
        let prof = MachineProfile::kniterate_7e();
        let child_ctx = KnitSafetyContext {
            wearer_age_class: WearerAgeClass::InfantUnder3,
            blocked_wale_pitch_mm: 4.0,
            source_doc: None,
            speed_fraction: None,
            takedown_fraction: None,
        };
        // Knit at f10 then tuck at f16 -> 5 skipped needles * 4 mm = 20 mm > 12 mm cap!
        let long_tuck_float = minimal_program(
            &prof,
            vec![
                KnitoutOp::In {
                    carriers: vec![CarrierId("1".to_string())],
                },
                KnitoutOp::Knit {
                    dir: CourseDir::Plus,
                    needle: Needle::new(Bed::Front, 10),
                    carriers: vec![CarrierId("1".to_string())],
                },
                KnitoutOp::Tuck {
                    dir: CourseDir::Plus,
                    needle: Needle::new(Bed::Front, 16),
                    carriers: vec![CarrierId("1".to_string())],
                },
                KnitoutOp::Out {
                    carriers: vec![CarrierId("1".to_string())],
                },
            ],
        );
        assert_eq!(
            verify_knitout_program(&long_tuck_float, &prof, &child_ctx)
                .unwrap_err()
                .code,
            RefusalCode::MaterialProcessHazard
        );

        // Same long float into a Split must also trigger child float cap!
        let long_split_float = minimal_program(
            &prof,
            vec![
                KnitoutOp::In {
                    carriers: vec![CarrierId("1".to_string())],
                },
                KnitoutOp::Knit {
                    dir: CourseDir::Plus,
                    needle: Needle::new(Bed::Front, 10),
                    carriers: vec![CarrierId("1".to_string())],
                },
                KnitoutOp::Split {
                    dir: CourseDir::Plus,
                    from: Needle::new(Bed::Front, 16),
                    to: Needle::new(Bed::Back, 16),
                    carriers: vec![CarrierId("1".to_string())],
                },
                KnitoutOp::Out {
                    carriers: vec![CarrierId("1".to_string())],
                },
            ],
        );
        assert_eq!(
            verify_knitout_program(&long_split_float, &prof, &child_ctx)
                .unwrap_err()
                .code,
            RefusalCode::MaterialProcessHazard
        );
    }

    #[test]
    fn vm_roller_imbalance_ignores_transferred_empty_hooks_but_refuses_live_short_row_pileup() {
        let prof = MachineProfile::kniterate_7e();
        let adult_ctx = KnitSafetyContext {
            wearer_age_class: WearerAgeClass::Adult,
            blocked_wale_pitch_mm: prof.needle_pitch_mm,
            source_doc: None,
            speed_fraction: None,
            takedown_fraction: None,
        };
        let c1 = vec![CarrierId("1".to_string())];

        // 1. Rib hem on b10 transferred to f10 followed by 20 front-bed courses -> zero live lag!
        let mut rib_ops = vec![
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Back, 10),
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Front, 11),
                carriers: c1.clone(),
            },
            KnitoutOp::Xfer {
                from: Needle::new(Bed::Back, 10),
                to: Needle::new(Bed::Front, 10),
            },
        ];
        for i in 0usize..20 {
            let dir = if i.is_multiple_of(2) {
                CourseDir::Minus
            } else {
                CourseDir::Plus
            };
            let order = if dir == CourseDir::Plus {
                [10, 11]
            } else {
                [11, 10]
            };
            for n in order {
                rib_ops.push(KnitoutOp::Knit {
                    dir,
                    needle: Needle::new(Bed::Front, n),
                    carriers: c1.clone(),
                });
            }
        }
        rib_ops.push(KnitoutOp::Out {
            carriers: c1.clone(),
        });
        let rib_report =
            verify_knitout_program(&minimal_program(&prof, rib_ops), &prof, &adult_ctx).unwrap();
        assert_eq!(rib_report.max_roller_imbalance, 0);

        // 2. Live loop held on f10 while f11 knits 12 consecutive short-row passes -> exceeds cap 8!
        let mut pileup_ops = vec![
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Front, 10),
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Front, 11),
                carriers: c1.clone(),
            },
        ];
        for i in 0usize..12 {
            let dir = if i.is_multiple_of(2) {
                CourseDir::Minus
            } else {
                CourseDir::Plus
            };
            pileup_ops.push(KnitoutOp::Knit {
                dir,
                needle: Needle::new(Bed::Front, 11),
                carriers: c1.clone(),
            });
        }
        pileup_ops.push(KnitoutOp::Out { carriers: c1 });
        assert_eq!(
            verify_knitout_program(&minimal_program(&prof, pileup_ops), &prof, &adult_ctx)
                .unwrap_err()
                .code,
            RefusalCode::MaterialProcessHazard
        );
    }
}
