//! Deterministic V-bed knitting machine virtual machine & 7-check safety verifier (`FTX-K03`).
//!
//! Enforces discrete V-bed kinematics, carrier lifecycle & spacing, hook-stack & tuck limits,
//! slider-bed bans, quarter-pitch racking alignment, calibration-swatch hard caps, and child/infant
//! safety gates (float length, EN 14682 drawstring ban, infant cut-tail & eyelet aperture caps).

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::ir::{Bed, CarrierId, CourseDir, KnitDoc, KnitoutOp, KnitoutProgram, Needle};
use super::profile::{CalibrationSwatchCaps, MachineProfile, WearerAgeClass};

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
    courses_knitted: u32,
}

#[derive(Clone, Debug)]
struct CarrierState {
    active: bool,
    hook_holding: bool,
    last_needle: Option<i32>,
    last_dir: Option<CourseDir>,
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

    if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        if let Some(sf) = ctx.speed_fraction
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
        if let Some(tf) = ctx.takedown_fraction
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
            ));
        }
        carriers.insert(
            cid.0.clone(),
            CarrierState {
                active: false,
                hook_holding: false,
                last_needle: None,
                last_dir: None,
                last_knit_needle: None,
            },
        );
    }

    let mut hooks: BTreeMap<(Bed, i32), HookState> = BTreeMap::new();
    let mut rack_quarter_pitches: i32 = 0;
    let mut max_abs_rack: i32 = 0;
    let mut max_loops_on_hook: u8 = 0;
    let mut max_tuck_depth: u8 = 0;
    let mut max_float_mm: f64 = 0.0;
    let mut total_transfers: usize = 0;
    let mut active_min_needle: Option<i32> = None;
    let mut active_max_needle: Option<i32> = None;
    let mut prev_pass_dir: Option<CourseDir> = None;
    let mut estimated_courses: u32 = 0;

    let check_bed_and_bounds =
        |needle: Needle, allow_slider: bool, step: usize| -> Result<(), Refusal> {
            if needle.bed.is_slider() {
                if !profile.has_sliders {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: slider needle {} used on sliderless machine '{}'",
                            needle.knitout_token(),
                            profile.id
                        ),
                    ));
                }
                if !allow_slider {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: loop formation forbidden on slider bed {}",
                            needle.knitout_token()
                        ),
                    ));
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
                ));
            }
            if needle.index < 0 || needle.index >= profile.needle_count as i32 {
                return Err(Refusal::new(
                    RefusalCode::MachineStateInfeasible,
                    format!(
                        "op #{step}: needle {} outside bed range 0..{}",
                        needle.knitout_token(),
                        profile.needle_count
                    ),
                ));
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
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                    })?;
                    if st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: carrier '{}' brought in twice", c.0),
                        ));
                    }
                    st.active = true;
                }
            }
            KnitoutOp::Out { carriers: cs } => {
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                    })?;
                    if !st.active || st.hook_holding {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!(
                                "op #{step}: carrier '{}' cannot execute `out` in current state",
                                c.0
                            ),
                        ));
                    }
                    st.active = false;
                    st.last_needle = None;
                    st.last_knit_needle = None;
                }
            }
            KnitoutOp::InHook { carriers: cs } => {
                if !profile.has_yarn_inserting_hooks {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: `inhook` forbidden on machine '{}' (uses direct grippers `in`/`out`, no yarn-inserting hooks)",
                            profile.id
                        ),
                    ));
                }
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                    })?;
                    if st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: carrier '{}' already active on `inhook`", c.0),
                        ));
                    }
                    st.active = true;
                    st.hook_holding = true;
                }
            }
            KnitoutOp::ReleaseHook { carriers: cs } => {
                if !profile.has_yarn_inserting_hooks {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: `releasehook` forbidden on machine '{}'",
                            profile.id
                        ),
                    ));
                }
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                    })?;
                    if !st.hook_holding {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!(
                                "op #{step}: `releasehook` without active `inhook` on '{}'",
                                c.0
                            ),
                        ));
                    }
                    st.hook_holding = false;
                }
            }
            KnitoutOp::OutHook { carriers: cs } => {
                if !profile.has_yarn_inserting_hooks {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: `outhook` forbidden on machine '{}'",
                            profile.id
                        ),
                    ));
                }
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                    })?;
                    if !st.active || st.hook_holding {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: invalid `outhook` on carrier '{}'", c.0),
                        ));
                    }
                    st.active = false;
                    st.last_needle = None;
                    st.last_knit_needle = None;
                }
            }
            KnitoutOp::Rack { quarter_pitches } => {
                if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
                    && !CalibrationSwatchCaps::ALLOW_RACK
                    && *quarter_pitches != 0
                {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!("op #{step}: racking forbidden in CalibrationSwatchOnly mode"),
                    ));
                }
                if quarter_pitches.abs() > profile.max_racking_quarter_pitches {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: rack {} quarter-pitches exceeds machine limit ±{}",
                            quarter_pitches, profile.max_racking_quarter_pitches
                        ),
                    ));
                }
                rack_quarter_pitches = *quarter_pitches;
                max_abs_rack = max_abs_rack.max(quarter_pitches.abs());
            }
            KnitoutOp::Knit {
                dir,
                needle,
                carriers: cs,
            } => {
                check_bed_and_bounds(*needle, false, step)?;
                if cs.is_empty() {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!("op #{step}: `knit` requires at least one active carrier"),
                    ));
                }
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
                    })?;
                    if !st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: carrier '{}' knitted before `in`/`inhook`", c.0),
                        ));
                    }
                    if let Some(prev_k) = st.last_knit_needle
                        && st.last_dir == Some(*dir)
                    {
                        let span = (needle.index - prev_k).abs().saturating_sub(1);
                        let float_mm = f64::from(span) * effective_wale_pitch_mm;
                        max_float_mm = max_float_mm.max(float_mm);
                    }
                    st.last_needle = Some(needle.index);
                    st.last_knit_needle = Some(needle.index);
                    st.last_dir = Some(*dir);
                }
                let h = hooks.entry((needle.bed, needle.index)).or_default();
                h.loops = 1;
                h.tuck_depth = 0;
                h.courses_knitted += 1;
                max_loops_on_hook = max_loops_on_hook.max(h.loops);
            }
            KnitoutOp::Tuck {
                dir,
                needle,
                carriers: cs,
            } => {
                check_bed_and_bounds(*needle, false, step)?;
                if cs.is_empty() {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!("op #{step}: `tuck` requires at least one active carrier"),
                    ));
                }
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
                    })?;
                    if !st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: carrier '{}' tucked before `in`/`inhook`", c.0),
                        ));
                    }
                    st.last_needle = Some(needle.index);
                    st.last_knit_needle = Some(needle.index);
                    st.last_dir = Some(*dir);
                }
                let h = hooks.entry((needle.bed, needle.index)).or_default();
                h.loops = h.loops.saturating_add(1);
                h.tuck_depth = h.tuck_depth.saturating_add(1);
                max_loops_on_hook = max_loops_on_hook.max(h.loops);
                max_tuck_depth = max_tuck_depth.max(h.tuck_depth);
                if h.loops > profile.max_loops_per_hook {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: needle {} loop stack {} exceeds max_loops_per_hook {}",
                            needle.knitout_token(),
                            h.loops,
                            profile.max_loops_per_hook
                        ),
                    ));
                }
                if h.tuck_depth > profile.max_tuck_stack {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: needle {} tuck depth {} exceeds max_tuck_stack {}",
                            needle.knitout_token(),
                            h.tuck_depth,
                            profile.max_tuck_stack
                        ),
                    ));
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
                    ));
                }
                check_bed_and_bounds(*from, false, step)?;
                check_bed_and_bounds(*to, true, step)?;
                validate_bed_transfer_alignment(*from, *to, rack_quarter_pitches, step)?;
                if cs.is_empty() {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!("op #{step}: `split` requires >= 1 active carrier"),
                    ));
                }
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                    })?;
                    if !st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: inactive carrier '{}' in `split`", c.0),
                        ));
                    }
                    st.last_needle = Some(from.index);
                    st.last_knit_needle = Some(from.index);
                    st.last_dir = Some(*dir);
                }
                let src_loops = hooks.get(&(from.bed, from.index)).map_or(0, |h| h.loops);
                let dst = hooks.entry((to.bed, to.index)).or_default();
                dst.loops = dst.loops.saturating_add(src_loops);
                max_loops_on_hook = max_loops_on_hook.max(dst.loops);
                if dst.loops > profile.max_loops_per_hook {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        format!(
                            "op #{step}: `split` destination {} stack {} exceeds max_loops_per_hook {}",
                            to.knitout_token(),
                            dst.loops,
                            profile.max_loops_per_hook
                        ),
                    ));
                }
                let src = hooks.entry((from.bed, from.index)).or_default();
                src.loops = 1;
                src.tuck_depth = 0;
                src.courses_knitted += 1;
                total_transfers += 1;
            }
            KnitoutOp::Xfer { from, to } => {
                if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
                    && !CalibrationSwatchCaps::ALLOW_TRANSFERS
                {
                    return Err(Refusal::new(
                        RefusalCode::MaterialProcessHazard,
                        format!("op #{step}: `xfer` forbidden in CalibrationSwatchOnly mode"),
                    ));
                }
                check_bed_and_bounds(*from, true, step)?;
                check_bed_and_bounds(*to, true, step)?;
                validate_bed_transfer_alignment(*from, *to, rack_quarter_pitches, step)?;
                let src_loops = hooks.get(&(from.bed, from.index)).map_or(0, |h| h.loops);
                if let Some(src) = hooks.get_mut(&(from.bed, from.index)) {
                    src.loops = 0;
                    src.tuck_depth = 0;
                }
                if src_loops > 0 {
                    let dst = hooks.entry((to.bed, to.index)).or_default();
                    dst.loops = dst.loops.saturating_add(src_loops);
                    max_loops_on_hook = max_loops_on_hook.max(dst.loops);
                    if dst.loops > profile.max_loops_per_hook {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!(
                                "op #{step}: `xfer` destination {} stack {} exceeds max_loops_per_hook {}",
                                to.knitout_token(),
                                dst.loops,
                                profile.max_loops_per_hook
                            ),
                        ));
                    }
                }
                total_transfers += 1;
            }
            KnitoutOp::Miss {
                dir,
                needle,
                carriers: cs,
            } => {
                check_bed_and_bounds(*needle, false, step)?;
                for c in cs {
                    let st = carriers.get_mut(&c.0).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: undeclared carrier '{}'", c.0),
                        )
                    })?;
                    if !st.active {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            format!("op #{step}: inactive carrier '{}' in `miss`", c.0),
                        ));
                    }
                    st.last_needle = Some(needle.index);
                    st.last_dir = Some(*dir);
                }
            }
            KnitoutOp::Amiss { needle } => {
                check_bed_and_bounds(*needle, false, step)?;
            }
            KnitoutOp::Drop { needle } => {
                check_bed_and_bounds(*needle, true, step)?;
                if let Some(h) = hooks.get_mut(&(needle.bed, needle.index)) {
                    h.loops = 0;
                    h.tuck_depth = 0;
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
                    ));
                }
            }
            KnitoutOp::Stitch { .. }
            | KnitoutOp::Pause { .. }
            | KnitoutOp::Comment { .. }
            | KnitoutOp::AddRollerAdvance { .. }
            | KnitoutOp::SpeedNumber { .. }
            | KnitoutOp::PresserMode { .. } => {}
        }
    }

    // Check active carriers cleanly parked at end
    for (cid, st) in &carriers {
        if st.active || st.hook_holding {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!("carrier '{cid}' was not parked (`out`/`outhook`) before end of program"),
            ));
        }
    }

    // Check CalibrationSwatchOnly needle span & course caps
    let active_needle_range = match (active_min_needle, active_max_needle) {
        (Some(lo), Some(hi)) => Some([lo, hi]),
        _ => None,
    };
    if ctx.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        if let Some([lo, hi]) = active_needle_range {
            let span = (hi - lo + 1) as u32;
            if span > CalibrationSwatchCaps::MAX_NEEDLES {
                return Err(Refusal::new(
                    RefusalCode::MaterialProcessHazard,
                    format!(
                        "calibration swatch needle span {span} exceeds hard cap {}",
                        CalibrationSwatchCaps::MAX_NEEDLES
                    ),
                ));
            }
        }
        if estimated_courses > CalibrationSwatchCaps::MAX_COURSES {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "calibration swatch course count {estimated_courses} exceeds hard cap {}",
                    CalibrationSwatchCaps::MAX_COURSES
                ),
            ));
        }
    }

    // Check 5: Roller-imbalance bound across knitted needles
    let mut min_c = u32::MAX;
    let mut max_c = 0u32;
    for h in hooks.values() {
        if h.courses_knitted > 0 {
            min_c = min_c.min(h.courses_knitted);
            max_c = max_c.max(h.courses_knitted);
        }
    }
    let max_roller_imbalance = max_c.saturating_sub(min_c);
    if max_roller_imbalance > profile.max_roller_imbalance_courses {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "short-row roller imbalance ({max_roller_imbalance} courses) exceeds machine cap {}",
                profile.max_roller_imbalance_courses
            ),
        ));
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
        ));
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
        ));
    }
    let rack_int = rack_quarter_pitches / 4;
    // Standard Knitout convention: back needle `b_j` aligns with front needle `f_{j + rack}`
    let aligned = if from_front {
        from.index == to.index + rack_int
    } else {
        to.index == from.index + rack_int
    };
    if !aligned {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "op #{step}: misaligned transfer {} -> {} at rack {rack_int}",
                from.knitout_token(),
                to.knitout_token()
            ),
        ));
    }
    Ok(())
}
