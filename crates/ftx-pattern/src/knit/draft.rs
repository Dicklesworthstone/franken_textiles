//! Pathway 1 measured knitting drafters (`FTX-K05`):
//! - Hard-capped stepped calibration swatch generator (`CalibrationSwatchOnly`),
//! - Measured fully-fashioned flat panel drafter (ribbed hem, waist/armhole decreases, short-row neck),
//! - Seamless top-down raglan pullover tube drafter.
//!
//! All drafters report exact per-section mm quantization residuals honestly.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::ir::{
    Bed, CamStitchSetting, CarrierId, CourseDir, GaugeSpec, KNIT_SCHEMA, KnitDoc, LoopKind,
    ParentRef, ShortRowTurn, StitchNode, YarnRun,
};
use super::profile::{CalibrationSwatchCaps, MachineProfile, WearerAgeClass};

/// Hard upper bound on drafted courses to prevent unbounded allocation on extreme dimensions.
const MAX_DRAFT_COURSES: u32 = 5_000;

/// Parameters for drafting a measured fully-fashioned flat knitted panel (front/back bodice or sleeve).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParametricPanelSpec {
    /// Panel name (e.g. `"toddler_sweater_front"`).
    pub name: String,
    /// Required wearer age class (non-defaulting).
    pub wearer_age_class: WearerAgeClass,
    /// Target finished hem width (including wearing ease), in mm.
    pub hem_width_mm: f64,
    /// Target finished chest/upper width (including wearing ease), in mm.
    pub chest_width_mm: f64,
    /// Target total panel length from hem to shoulder, in mm.
    pub length_mm: f64,
    /// Armhole height from underarm bind-off to shoulder, in mm (`0.0` for simple rectangle).
    pub armhole_depth_mm: f64,
    /// Armhole total width reduction across both sides combined, in mm (`>= 0`).
    pub armhole_inset_total_mm: f64,
    /// Neck opening width at top, in mm (`0.0` for closed straight top).
    pub neck_width_mm: f64,
    /// Neck drop below shoulder line, in mm (`0.0` for straight top).
    pub neck_depth_mm: f64,
    /// Number of 1x1 rib courses at the bottom hem (`0` for plain stockinette).
    pub rib_hem_courses: u32,
    /// Dimensionless stitch cam setting for main jersey body.
    pub cam_setting: f64,
    /// Catalog yarn identifier.
    pub yarn_id: String,
    /// Active yarn carrier identifier.
    pub carrier_id: CarrierId,
    /// True if panel design includes a hood/neck drawstring (refused for child garments).
    #[serde(default)]
    pub has_hood_or_neck_drawstring: bool,
}

/// Parameters for a top-down seamless raglan pullover tube.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaglanPulloverSpec {
    /// Garment name.
    pub name: String,
    /// Required wearer age class (non-defaulting).
    pub wearer_age_class: WearerAgeClass,
    /// Neck opening circumference (finished, including ease), in mm.
    pub neck_girth_mm: f64,
    /// Finished chest circumference (including wearing ease), in mm.
    pub chest_girth_mm: f64,
    /// Raglan yoke vertical depth (collar to underarm), in mm.
    pub yoke_depth_mm: f64,
    /// Torso length below underarm to hem, in mm.
    pub body_length_below_arm_mm: f64,
    /// Collar rib course count.
    pub collar_rib_courses: u32,
    /// Bottom hem rib course count.
    pub hem_rib_courses: u32,
    /// Dimensionless cam stitch setting.
    pub cam_setting: f64,
    /// Catalog yarn identifier.
    pub yarn_id: String,
    /// Active yarn carrier identifier.
    pub carrier_id: CarrierId,
    /// True if hood/neck drawstring is present.
    #[serde(default)]
    pub has_hood_or_neck_drawstring: bool,
}

/// Drafts a hard-capped stepped calibration swatch (`WearerAgeClass::CalibrationSwatchOnly`)
/// stepping across `cam_settings` zones so the operator can wash, block, and measure real gauge & drift.
///
/// # Errors
/// [`RefusalCode::OutOfRange`] or [`RefusalCode::MaterialProcessHazard`] if caps are exceeded.
pub fn draft_calibration_swatch(
    profile: &MachineProfile,
    yarn_id: &str,
    needles: u32,
    courses_per_zone: u32,
    cam_settings: &[f64],
) -> Result<KnitDoc, Refusal> {
    profile.validate()?;
    if yarn_id.trim().is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "calibration swatch requires a non-empty yarn_id",
        ));
    }
    if cam_settings.is_empty() || courses_per_zone == 0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "calibration swatch requires at least one cam_setting zone and courses_per_zone >= 1",
        ));
    }
    let total_courses = courses_per_zone
        .checked_mul(u32::try_from(cam_settings.len()).unwrap_or(u32::MAX))
        .and_then(|c| c.checked_add(2)) // cast-on + bind-off
        .ok_or_else(|| Refusal::new(RefusalCode::OutOfRange, "swatch course count overflow"))?;
    if !(4..=CalibrationSwatchCaps::MAX_NEEDLES).contains(&needles)
        || needles > profile.needle_count
    {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "calibration swatch width {needles} must be in 4..={}",
                CalibrationSwatchCaps::MAX_NEEDLES.min(profile.needle_count)
            ),
        ));
    }
    if total_courses > CalibrationSwatchCaps::MAX_COURSES {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "calibration swatch total courses {total_courses} exceeds hard cap {}",
                CalibrationSwatchCaps::MAX_COURSES
            ),
        ));
    }
    for &s in cam_settings {
        Refusal::finite("cam_setting", s)?;
        if s < profile.cam_setting_range[0] || s > profile.cam_setting_range[1] {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "swatch cam setting {s} outside machine profile range {:?}",
                    profile.cam_setting_range
                ),
            ));
        }
    }

    let carrier = profile.carrier_ids[0].clone();
    let est_loop_mm = 2.6 * profile.needle_pitch_mm;
    let total_yarn_m = f64::from(needles) * f64::from(total_courses) * est_loop_mm / 1000.0;
    if total_yarn_m > CalibrationSwatchCaps::MAX_YARN_M {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            format!(
                "calibration swatch yarn estimate {total_yarn_m:.2} m exceeds hard cap {} m",
                CalibrationSwatchCaps::MAX_YARN_M
            ),
        ));
    }

    let mut stitches = Vec::with_capacity((needles as usize) * (total_courses as usize));
    let mut prev_row: Vec<u32> = Vec::with_capacity(needles as usize);
    let mut next_id = 0u32;

    // Course 0: Cast-on
    for w in 0..needles {
        stitches.push(StitchNode {
            id: next_id,
            course: 0,
            wale: w as i32,
            bed: Bed::Front,
            kind: LoopKind::CastOn,
            parents: vec![],
            carrier_ids: vec![carrier.clone()],
            yarn_id: yarn_id.to_string(),
            cam_stitch_setting: CamStitchSetting {
                setting: cam_settings[0],
                machine_family: profile.id.clone(),
            },
            yarn_length_mm: Some(est_loop_mm),
        });
        prev_row.push(next_id);
        next_id += 1;
    }

    // Stepped cam-setting stockinette zones
    let mut course_idx = 1u32;
    for &setting in cam_settings {
        for _ in 0..courses_per_zone {
            let mut cur_row = Vec::with_capacity(needles as usize);
            for w in 0..needles {
                let pid = prev_row[w as usize];
                stitches.push(StitchNode {
                    id: next_id,
                    course: course_idx,
                    wale: w as i32,
                    bed: Bed::Front,
                    kind: LoopKind::Knit,
                    parents: vec![ParentRef {
                        stitch_id: pid,
                        stack_pos: 0,
                    }],
                    carrier_ids: vec![carrier.clone()],
                    yarn_id: yarn_id.to_string(),
                    cam_stitch_setting: CamStitchSetting {
                        setting,
                        machine_family: profile.id.clone(),
                    },
                    yarn_length_mm: Some(est_loop_mm),
                });
                cur_row.push(next_id);
                next_id += 1;
            }
            prev_row = cur_row;
            course_idx += 1;
        }
    }

    // Final bind-off course
    for w in 0..needles {
        let pid = prev_row[w as usize];
        stitches.push(StitchNode {
            id: next_id,
            course: course_idx,
            wale: w as i32,
            bed: Bed::Front,
            kind: LoopKind::BindOff,
            parents: vec![ParentRef {
                stitch_id: pid,
                stack_pos: 0,
            }],
            carrier_ids: vec![carrier.clone()],
            yarn_id: yarn_id.to_string(),
            cam_stitch_setting: CamStitchSetting {
                setting: *cam_settings.last().unwrap(),
                machine_family: profile.id.clone(),
            },
            yarn_length_mm: Some(est_loop_mm),
        });
        next_id += 1;
    }

    let mut metadata = BTreeMap::new();
    metadata.insert("calibration_swatch_only".to_string(), "true".to_string());
    metadata.insert(
        "max_speed_fraction".to_string(),
        format!("{:.2}", CalibrationSwatchCaps::MAX_SPEED_FRACTION),
    );
    metadata.insert(
        "max_takedown_fraction".to_string(),
        format!("{:.2}", CalibrationSwatchCaps::MAX_TAKEDOWN_FRACTION),
    );
    metadata.insert(
        "cam_zones".to_string(),
        cam_settings
            .iter()
            .map(|s| format!("{s:.2}"))
            .collect::<Vec<_>>()
            .join(","),
    );

    let doc = KnitDoc {
        schema: KNIT_SCHEMA.to_string(),
        name: format!("calibration_swatch_{}", profile.id),
        wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
        gauge: GaugeSpec {
            wales_per_100mm: 100.0 / profile.needle_pitch_mm,
            courses_per_100mm: 140.0 / profile.needle_pitch_mm,
            needle_pitch_mm: profile.needle_pitch_mm,
            yarn_tex: None,
            thickness_mm: None,
            relaxed_after_wash: false,
        },
        stitches,
        short_row_turns: vec![],
        cable_groups: vec![],
        yarn_runs: vec![YarnRun {
            run_id: 0,
            carrier_id: carrier,
            yarn_id: yarn_id.to_string(),
            start_course: 0,
            end_course: course_idx,
            total_yarn_length_m: total_yarn_m,
            cut_tail_mm: Some(15.0),
        }],
        has_hood_or_neck_drawstring: false,
        metadata,
    };
    doc.validate()?;
    Ok(doc)
}

/// Drafts a measured fully-fashioned flat panel (hem rib, side taper, armhole decreases, and
/// anchored short-row neckline) with exact mm stitch/course quantization residuals.
///
/// # Errors
/// [`RefusalCode::OutOfRange`], [`RefusalCode::NonFinite`], or [`RefusalCode::MaterialProcessHazard`].
pub fn draft_shaped_panel(
    spec: &ParametricPanelSpec,
    gauge: &GaugeSpec,
    profile: &MachineProfile,
) -> Result<KnitDoc, Refusal> {
    gauge.validate()?;
    profile.validate()?;
    if spec.name.trim().is_empty() || spec.yarn_id.trim().is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "panel spec requires non-empty name and yarn_id",
        ));
    }
    CarrierId::new(spec.carrier_id.as_str())?;
    if !profile.carrier_ids.contains(&spec.carrier_id) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "panel carrier '{}' is not available on machine '{}'",
                spec.carrier_id.as_str(),
                profile.id
            ),
        ));
    }
    for (name, val) in [
        ("hem_width_mm", spec.hem_width_mm),
        ("chest_width_mm", spec.chest_width_mm),
        ("length_mm", spec.length_mm),
        ("armhole_depth_mm", spec.armhole_depth_mm),
        ("armhole_inset_total_mm", spec.armhole_inset_total_mm),
        ("neck_width_mm", spec.neck_width_mm),
        ("neck_depth_mm", spec.neck_depth_mm),
        ("cam_setting", spec.cam_setting),
    ] {
        Refusal::finite(name, val)?;
    }
    if spec.hem_width_mm <= 0.0 || spec.chest_width_mm <= 0.0 || spec.length_mm <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "panel widths and length must be positive",
        ));
    }
    if spec.armhole_inset_total_mm < 0.0 || spec.neck_width_mm < 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "armhole_inset_total_mm and neck_width_mm cannot be negative",
        ));
    }
    if spec.cam_setting < profile.cam_setting_range[0]
        || spec.cam_setting > profile.cam_setting_range[1]
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "panel cam setting {} outside machine profile range {:?}",
                spec.cam_setting, profile.cam_setting_range
            ),
        ));
    }
    if spec.armhole_depth_mm < 0.0
        || spec.armhole_depth_mm >= spec.length_mm
        || spec.neck_depth_mm < 0.0
        || spec.neck_depth_mm > spec.armhole_depth_mm.max(spec.length_mm * 0.4)
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "armhole_depth_mm or neck_depth_mm is inconsistent with panel length",
        ));
    }

    let wale_pitch = gauge.blocked_wale_pitch_mm();
    let course_height = gauge.blocked_course_height_mm();

    let max_needles_f64 = f64::from(profile.needle_count);
    for (label, mm) in [
        ("hem_width_mm", spec.hem_width_mm),
        ("chest_width_mm", spec.chest_width_mm),
    ] {
        let est = (mm / wale_pitch).round();
        if !est.is_finite() || est > max_needles_f64 + 2.0 {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!(
                    "panel {label} ({mm:.1} mm, ~{est:.0} needles) exceeds machine '{}' limit {}",
                    profile.id, profile.needle_count
                ),
            ));
        }
    }

    // Round stitch counts to even integers for symmetric shaping around center 0
    let round_even = |mm: f64| -> i32 {
        let raw = (mm / wale_pitch).round().max(4.0) as i32;
        if raw % 2 == 0 { raw } else { raw + 1 }
    };
    let hem_wales = round_even(spec.hem_width_mm);
    let chest_wales = round_even(spec.chest_width_mm);
    let max_wales = hem_wales.max(chest_wales) as u32;
    if max_wales > profile.needle_count {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "panel requires {max_wales} needles but machine '{}' has only {}",
                profile.id, profile.needle_count
            ),
        ));
    }

    let total_courses_f64 = (spec.length_mm / course_height).round();
    if !total_courses_f64.is_finite() || total_courses_f64 > f64::from(MAX_DRAFT_COURSES) {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "panel length {:.1} mm ({total_courses_f64:.0} courses) exceeds course cap {MAX_DRAFT_COURSES}",
                spec.length_mm
            ),
        ));
    }
    let total_courses = total_courses_f64.max(6.0) as u32;
    if spec.rib_hem_courses.saturating_add(2) > total_courses {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "rib_hem_courses ({}) leaves insufficient body courses for panel length ({total_courses} courses)",
                spec.rib_hem_courses
            ),
        ));
    }
    let armhole_courses = (spec.armhole_depth_mm / course_height).round() as u32;
    let armhole_start = total_courses
        .saturating_sub(armhole_courses)
        .max(spec.rib_hem_courses + 2);
    if spec.armhole_inset_total_mm >= spec.chest_width_mm {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "armhole_inset_total_mm must be strictly smaller than chest_width_mm",
        ));
    }
    let armhole_inset_per_side = ((spec.armhole_inset_total_mm * 0.5) / wale_pitch)
        .round()
        .max(0.0) as i32;
    let min_shoulder_half = chest_wales / 2 - armhole_inset_per_side;
    if min_shoulder_half < 2 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "armhole_inset_total_mm ({:.1} mm) leaves fewer than 4 shoulder stitches at chest width {:.1} mm",
                spec.armhole_inset_total_mm, spec.chest_width_mm
            ),
        ));
    }
    let neck_courses = (spec.neck_depth_mm / course_height).round() as u32;
    let neck_start = total_courses.saturating_sub(neck_courses);
    let neck_half = if neck_courses > 0 {
        let nh = ((spec.neck_width_mm * 0.5) / wale_pitch).round() as i32;
        if spec.neck_width_mm <= 0.0 || nh < 1 || nh >= min_shoulder_half {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "neck_width_mm ({:.1} mm, half-width {nh} st) must fit strictly inside remaining shoulder half-width ({min_shoulder_half} st)",
                    spec.neck_width_mm
                ),
            ));
        }
        nh
    } else {
        0
    };

    let est_loop_mm = 2.6 * wale_pitch;
    let mut stitches = Vec::new();
    let mut short_row_turns = Vec::new();
    let mut prev_map: BTreeMap<i32, u32> = BTreeMap::new();
    let mut next_id = 0u32;

    let half_wales_at = |c: u32| -> i32 {
        if c <= spec.rib_hem_courses {
            hem_wales / 2
        } else if c < armhole_start {
            let span = (armhole_start - spec.rib_hem_courses).max(1) as f64;
            let t = f64::from(c - spec.rib_hem_courses) / span;
            let w = f64::from(hem_wales) + t * f64::from(chest_wales - hem_wales);
            (w * 0.5).round().max(2.0) as i32
        } else {
            let span = (total_courses - armhole_start).max(1) as f64;
            let t = (f64::from(c - armhole_start) / span).min(1.0);
            let dec = (t * f64::from(armhole_inset_per_side)).round() as i32;
            chest_wales / 2 - dec
        }
    };

    for c in 0..=total_courses {
        let half = half_wales_at(c);
        let left = -half;
        let right = half - 1;
        let mut cur_map = BTreeMap::new();
        let is_rib = c > 0 && c <= spec.rib_hem_courses && profile.has_back_bed;

        // If inside neck shaping region, emit anchored short-row turnaround on shoulders
        if neck_courses > 0 && c >= neck_start && c < total_courses {
            short_row_turns.push(ShortRowTurn {
                course: c,
                turn_wale: neck_half,
                dir_after: if c.is_multiple_of(2) {
                    CourseDir::Minus
                } else {
                    CourseDir::Plus
                },
                anchored: true,
            });
        }

        for w in left..=right {
            let bed = if is_rib && (w.rem_euclid(2) == 1) {
                Bed::Back
            } else {
                Bed::Front
            };
            let mut parents = Vec::new();
            if c > 0 {
                if let Some(&pid) = prev_map.get(&w) {
                    parents.push(ParentRef {
                        stitch_id: pid,
                        stack_pos: 0,
                    });
                } else if let Some(&pid) = prev_map.get(&(w + 1)).or_else(|| prev_map.get(&(w - 1)))
                {
                    parents.push(ParentRef {
                        stitch_id: pid,
                        stack_pos: 0,
                    });
                }
                // Merge outer edge stitch cleanly on symmetrical decrease courses
                if w == left
                    && let Some(&outer) = prev_map.get(&(w - 1))
                {
                    parents.push(ParentRef {
                        stitch_id: outer,
                        stack_pos: 1,
                    });
                } else if w == right
                    && let Some(&outer) = prev_map.get(&(w + 1))
                {
                    parents.push(ParentRef {
                        stitch_id: outer,
                        stack_pos: 1,
                    });
                }
            }
            let kind = if c == 0 {
                LoopKind::CastOn
            } else if c == total_courses {
                LoopKind::BindOff
            } else if parents.len() >= 2 {
                LoopKind::Decrease
            } else if bed == Bed::Back {
                LoopKind::Purl
            } else {
                LoopKind::Knit
            };
            stitches.push(StitchNode {
                id: next_id,
                course: c,
                wale: w,
                bed,
                kind,
                parents,
                carrier_ids: vec![spec.carrier_id.clone()],
                yarn_id: spec.yarn_id.clone(),
                cam_stitch_setting: CamStitchSetting {
                    setting: spec.cam_setting,
                    machine_family: profile.id.clone(),
                },
                yarn_length_mm: Some(est_loop_mm),
            });
            cur_map.insert(w, next_id);
            next_id += 1;
        }
        prev_map = cur_map;
    }

    let total_yarn_m = stitches.len() as f64 * est_loop_mm / 1000.0;
    let hem_residual_mm = f64::from(hem_wales) * wale_pitch - spec.hem_width_mm;
    let chest_residual_mm = f64::from(chest_wales) * wale_pitch - spec.chest_width_mm;
    let length_residual_mm = f64::from(total_courses) * course_height - spec.length_mm;

    let mut metadata = BTreeMap::new();
    metadata.insert(
        "pathway".to_string(),
        "measured_fully_fashioned_panel".to_string(),
    );
    metadata.insert(
        "hem_width_residual_mm".to_string(),
        format!("{hem_residual_mm:+.2}"),
    );
    metadata.insert(
        "chest_width_residual_mm".to_string(),
        format!("{chest_residual_mm:+.2}"),
    );
    metadata.insert(
        "length_residual_mm".to_string(),
        format!("{length_residual_mm:+.2}"),
    );

    let doc = KnitDoc {
        schema: KNIT_SCHEMA.to_string(),
        name: spec.name.clone(),
        wearer_age_class: spec.wearer_age_class,
        gauge: gauge.clone(),
        stitches,
        short_row_turns,
        cable_groups: vec![],
        yarn_runs: vec![YarnRun {
            run_id: 0,
            carrier_id: spec.carrier_id.clone(),
            yarn_id: spec.yarn_id.clone(),
            start_course: 0,
            end_course: total_courses,
            total_yarn_length_m: total_yarn_m,
            cut_tail_mm: Some(15.0),
        }],
        has_hood_or_neck_drawstring: spec.has_hood_or_neck_drawstring,
        metadata,
    };
    doc.validate()?;
    Ok(doc)
}

/// Drafts a top-down seamless raglan pullover body tube (front + back bed circular knitting).
///
/// # Errors
/// [`RefusalCode::OutOfRange`] or [`RefusalCode::MachineStateInfeasible`].
pub fn draft_seamless_raglan_tube(
    spec: &RaglanPulloverSpec,
    gauge: &GaugeSpec,
    profile: &MachineProfile,
) -> Result<KnitDoc, Refusal> {
    gauge.validate()?;
    profile.validate()?;
    if spec.name.trim().is_empty() || spec.yarn_id.trim().is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "raglan spec requires non-empty name and yarn_id",
        ));
    }
    CarrierId::new(spec.carrier_id.as_str())?;
    if !profile.carrier_ids.contains(&spec.carrier_id) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "raglan carrier '{}' is not available on machine '{}'",
                spec.carrier_id.as_str(),
                profile.id
            ),
        ));
    }
    if !profile.has_back_bed {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "seamless tubular raglan requires a back bed, which '{}' lacks",
                profile.id
            ),
        ));
    }
    for (name, val) in [
        ("neck_girth_mm", spec.neck_girth_mm),
        ("chest_girth_mm", spec.chest_girth_mm),
        ("yoke_depth_mm", spec.yoke_depth_mm),
        ("body_length_below_arm_mm", spec.body_length_below_arm_mm),
        ("cam_setting", spec.cam_setting),
    ] {
        Refusal::finite(name, val)?;
    }
    if spec.neck_girth_mm <= 0.0
        || spec.chest_girth_mm < spec.neck_girth_mm
        || spec.yoke_depth_mm <= 0.0
        || spec.body_length_below_arm_mm <= 0.0
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "invalid raglan girths or lengths",
        ));
    }
    if spec.cam_setting < profile.cam_setting_range[0]
        || spec.cam_setting > profile.cam_setting_range[1]
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "raglan cam setting {} outside machine profile range {:?}",
                spec.cam_setting, profile.cam_setting_range
            ),
        ));
    }

    let wale_pitch = gauge.blocked_wale_pitch_mm();
    let course_height = gauge.blocked_course_height_mm();
    let max_tube_st_f64 = 2.0 * f64::from(profile.needle_count);
    for (label, girth) in [
        ("neck_girth_mm", spec.neck_girth_mm),
        ("chest_girth_mm", spec.chest_girth_mm),
    ] {
        let est = (girth / wale_pitch).round();
        if !est.is_finite() || est > max_tube_st_f64 + 4.0 {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                format!(
                    "raglan {label} ({girth:.1} mm, ~{est:.0} stitches) exceeds tubular capacity of machine '{}' ({} needles/bed)",
                    profile.id, profile.needle_count
                ),
            ));
        }
    }
    let round_mult4 = |girth: f64| -> i32 {
        let s = (girth / wale_pitch).round().max(8.0) as i32;
        ((s + 3) / 4) * 4
    };
    let neck_st = round_mult4(spec.neck_girth_mm);
    let chest_st = round_mult4(spec.chest_girth_mm);
    let max_per_bed = (chest_st / 2) as u32;
    if max_per_bed > profile.needle_count {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "tubular chest requires {max_per_bed} needles per bed, exceeding {}",
                profile.needle_count
            ),
        ));
    }

    let yoke_f64 = (spec.yoke_depth_mm / course_height).round();
    let body_f64 = (spec.body_length_below_arm_mm / course_height).round();
    if !yoke_f64.is_finite()
        || !body_f64.is_finite()
        || yoke_f64 > f64::from(MAX_DRAFT_COURSES)
        || body_f64 > f64::from(MAX_DRAFT_COURSES)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("raglan vertical dimensions exceed course cap {MAX_DRAFT_COURSES}"),
        ));
    }
    let yoke_courses = yoke_f64.max(4.0) as u32;
    let body_courses = body_f64.max(4.0) as u32;
    let total_courses = spec
        .collar_rib_courses
        .checked_add(yoke_courses)
        .and_then(|c| c.checked_add(body_courses))
        .and_then(|c| c.checked_add(spec.hem_rib_courses))
        .ok_or_else(|| Refusal::new(RefusalCode::OutOfRange, "raglan course count overflow"))?;
    if total_courses > MAX_DRAFT_COURSES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("raglan total courses {total_courses} exceeds course cap {MAX_DRAFT_COURSES}"),
        ));
    }
    let est_loop_mm = 2.6 * wale_pitch;

    let mut stitches = Vec::new();
    let mut prev_front: BTreeMap<i32, u32> = BTreeMap::new();
    let mut prev_back: BTreeMap<i32, u32> = BTreeMap::new();
    let mut next_id = 0u32;

    for c in 0..=total_courses {
        let per_bed = if c <= spec.collar_rib_courses {
            neck_st / 2
        } else if c <= spec.collar_rib_courses + yoke_courses {
            let t = f64::from(c - spec.collar_rib_courses) / f64::from(yoke_courses);
            let cur = f64::from(neck_st) + t * f64::from(chest_st - neck_st);
            ((cur * 0.5).round() as i32).max(4)
        } else {
            chest_st / 2
        };
        let half = per_bed / 2;
        let left = -half;
        let right = left + per_bed - 1;
        let mut cur_front = BTreeMap::new();
        let mut cur_back = BTreeMap::new();

        for (bed, prev_bed, cur_bed) in [
            (Bed::Front, &prev_front, &mut cur_front),
            (Bed::Back, &prev_back, &mut cur_back),
        ] {
            for w in left..=right {
                let mut parents = Vec::new();
                if c > 0
                    && let Some(&pid) = prev_bed
                        .get(&w)
                        .or_else(|| prev_bed.get(&(w - 1)))
                        .or_else(|| prev_bed.get(&(w + 1)))
                {
                    parents.push(ParentRef {
                        stitch_id: pid,
                        stack_pos: 0,
                    });
                }
                let kind = if c == 0 {
                    LoopKind::CastOn
                } else if c == total_courses {
                    LoopKind::BindOff
                } else if c > spec.collar_rib_courses
                    && c <= spec.collar_rib_courses + yoke_courses
                    && !prev_bed.contains_key(&w)
                {
                    LoopKind::Increase
                } else if bed == Bed::Back {
                    LoopKind::Purl
                } else {
                    LoopKind::Knit
                };
                stitches.push(StitchNode {
                    id: next_id,
                    course: c,
                    wale: w,
                    bed,
                    kind,
                    parents,
                    carrier_ids: vec![spec.carrier_id.clone()],
                    yarn_id: spec.yarn_id.clone(),
                    cam_stitch_setting: CamStitchSetting {
                        setting: spec.cam_setting,
                        machine_family: profile.id.clone(),
                    },
                    yarn_length_mm: Some(est_loop_mm),
                });
                cur_bed.insert(w, next_id);
                next_id += 1;
            }
        }
        prev_front = cur_front;
        prev_back = cur_back;
    }

    let total_yarn_m = stitches.len() as f64 * est_loop_mm / 1000.0;
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "pathway".to_string(),
        "top_down_seamless_raglan".to_string(),
    );
    metadata.insert(
        "neck_girth_residual_mm".to_string(),
        format!(
            "{:+.2}",
            f64::from(neck_st) * wale_pitch - spec.neck_girth_mm
        ),
    );
    metadata.insert(
        "chest_girth_residual_mm".to_string(),
        format!(
            "{:+.2}",
            f64::from(chest_st) * wale_pitch - spec.chest_girth_mm
        ),
    );

    let doc = KnitDoc {
        schema: KNIT_SCHEMA.to_string(),
        name: spec.name.clone(),
        wearer_age_class: spec.wearer_age_class,
        gauge: gauge.clone(),
        stitches,
        short_row_turns: vec![],
        cable_groups: vec![],
        yarn_runs: vec![YarnRun {
            run_id: 0,
            carrier_id: spec.carrier_id.clone(),
            yarn_id: spec.yarn_id.clone(),
            start_course: 0,
            end_course: total_courses,
            total_yarn_length_m: total_yarn_m,
            cut_tail_mm: Some(15.0),
        }],
        has_hood_or_neck_drawstring: spec.has_hood_or_neck_drawstring,
        metadata,
    };
    doc.validate()?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_swatch_enforces_hard_caps() {
        let prof = MachineProfile::kniterate_7e();
        let doc =
            draft_calibration_swatch(&prof, "wool-200tex", 40, 20, &[6.0, 8.0, 10.0]).unwrap();
        assert_eq!(doc.wearer_age_class, WearerAgeClass::CalibrationSwatchOnly);
        assert_eq!(doc.stitches.len(), 40 * (60 + 2));

        // Exceeding 80 needles is refused fail-closed
        let err = draft_calibration_swatch(&prof, "wool-200tex", 90, 20, &[8.0]).unwrap_err();
        assert_eq!(err.code, RefusalCode::MaterialProcessHazard);
    }

    #[test]
    fn shaped_panel_and_raglan_draft_with_residuals() {
        let prof = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: prof.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.2),
            relaxed_after_wash: true,
        };
        let panel = draft_shaped_panel(
            &ParametricPanelSpec {
                name: "toddler_front".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                hem_width_mm: 280.0,
                chest_width_mm: 300.0,
                length_mm: 360.0,
                armhole_depth_mm: 120.0,
                armhole_inset_total_mm: 40.0,
                neck_width_mm: 110.0,
                neck_depth_mm: 40.0,
                rib_hem_courses: 8,
                cam_setting: 8.0,
                yarn_id: "merino-180tex".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &prof,
        )
        .unwrap();
        assert!(panel.metadata.contains_key("chest_width_residual_mm"));
        assert!(!panel.short_row_turns.is_empty());

        let raglan = draft_seamless_raglan_tube(
            &RaglanPulloverSpec {
                name: "toddler_raglan".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                neck_girth_mm: 280.0,
                chest_girth_mm: 580.0,
                yoke_depth_mm: 130.0,
                body_length_below_arm_mm: 200.0,
                collar_rib_courses: 6,
                hem_rib_courses: 8,
                cam_setting: 8.0,
                yarn_id: "merino-180tex".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &prof,
        )
        .unwrap();
        assert!(raglan.metadata.contains_key("chest_girth_residual_mm"));

        // Non-finite raglan inputs and out-of-range panel inputs fail closed instead of clamping.
        let nan_raglan = draft_seamless_raglan_tube(
            &RaglanPulloverSpec {
                name: "bad_raglan".to_string(),
                wearer_age_class: WearerAgeClass::Adult,
                neck_girth_mm: f64::NAN,
                chest_girth_mm: 580.0,
                yoke_depth_mm: 130.0,
                body_length_below_arm_mm: 200.0,
                collar_rib_courses: 6,
                hem_rib_courses: 8,
                cam_setting: 8.0,
                yarn_id: "merino-180tex".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &prof,
        )
        .unwrap_err();
        assert_eq!(nan_raglan.code, RefusalCode::NonFinite);

        let neg_inset = draft_shaped_panel(
            &ParametricPanelSpec {
                name: "bad_panel".to_string(),
                wearer_age_class: WearerAgeClass::Adult,
                hem_width_mm: 280.0,
                chest_width_mm: 300.0,
                length_mm: 360.0,
                armhole_depth_mm: 120.0,
                armhole_inset_total_mm: -10.0,
                neck_width_mm: 0.0,
                neck_depth_mm: 0.0,
                rib_hem_courses: 8,
                cam_setting: 8.0,
                yarn_id: "merino-180tex".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &prof,
        )
        .unwrap_err();
        assert_eq!(neg_inset.code, RefusalCode::OutOfRange);

        assert_eq!(
            draft_calibration_swatch(&prof, "wool-200tex", 40, 0, &[8.0])
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );
    }

    #[test]
    fn drafters_refuse_impossible_shaping_overflow_and_invalid_ids() {
        let prof = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: prof.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.2),
            relaxed_after_wash: true,
        };
        assert_eq!(
            draft_calibration_swatch(&prof, "   ", 24, 4, &[8.0])
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );

        let base_panel = ParametricPanelSpec {
            name: "front".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            hem_width_mm: 280.0,
            chest_width_mm: 300.0,
            length_mm: 360.0,
            armhole_depth_mm: 120.0,
            armhole_inset_total_mm: 40.0,
            neck_width_mm: 110.0,
            neck_depth_mm: 40.0,
            rib_hem_courses: 8,
            cam_setting: 8.0,
            yarn_id: "merino-180tex".to_string(),
            carrier_id: CarrierId("1".to_string()),
            has_hood_or_neck_drawstring: false,
        };

        // Impossible armhole inset >= chest_width must refuse OutOfRange (no silent clamp).
        let mut bad_inset = base_panel.clone();
        bad_inset.armhole_inset_total_mm = 300.0;
        assert_eq!(
            draft_shaped_panel(&bad_inset, &gauge, &prof)
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );

        // Neck opening wider than remaining shoulder span must refuse OutOfRange (no silent clamp).
        let mut bad_neck = base_panel.clone();
        bad_neck.neck_width_mm = 280.0;
        assert_eq!(
            draft_shaped_panel(&bad_neck, &gauge, &prof)
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );

        // Extreme panel length must refuse BudgetExceeded (no OOM).
        let mut huge_len = base_panel.clone();
        huge_len.length_mm = 1.0e8;
        assert_eq!(
            draft_shaped_panel(&huge_len, &gauge, &prof)
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );

        // Extreme width must refuse MachineStateInfeasible without i32 overflow.
        let mut huge_w = base_panel.clone();
        huge_w.chest_width_mm = 1.0e15;
        assert_eq!(
            draft_shaped_panel(&huge_w, &gauge, &prof).unwrap_err().code,
            RefusalCode::MachineStateInfeasible
        );

        // Unknown carrier must refuse MachineStateInfeasible.
        let mut bad_car = base_panel;
        bad_car.carrier_id = CarrierId("99".to_string());
        assert_eq!(
            draft_shaped_panel(&bad_car, &gauge, &prof)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );

        // Extreme raglan girth & height refuse cleanly without overflow or OOM.
        let base_raglan = RaglanPulloverSpec {
            name: "raglan".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            neck_girth_mm: 280.0,
            chest_girth_mm: 580.0,
            yoke_depth_mm: 130.0,
            body_length_below_arm_mm: 200.0,
            collar_rib_courses: 6,
            hem_rib_courses: 8,
            cam_setting: 8.0,
            yarn_id: "merino-180tex".to_string(),
            carrier_id: CarrierId("1".to_string()),
            has_hood_or_neck_drawstring: false,
        };
        let mut huge_girth = base_raglan.clone();
        huge_girth.chest_girth_mm = 1.0e15;
        assert_eq!(
            draft_seamless_raglan_tube(&huge_girth, &gauge, &prof)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
        let mut huge_yoke = base_raglan;
        huge_yoke.yoke_depth_mm = 1.0e8;
        assert_eq!(
            draft_seamless_raglan_tube(&huge_yoke, &gauge, &prof)
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
    }
}
