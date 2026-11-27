//! Pathway 1 measured knitting drafters (`FTX-K05`):
//! - Hard-capped stepped calibration swatch generator (`CalibrationSwatchOnly`),
//! - Measured flat panel drafter (ribbed hem, side taper, armholes, separate shoulders and held neck),
//! - Top-down shaped torso tube drafter with half-gauge tubular collar and hem rib.
//!
//! All drafters report exact per-section mm quantization residuals honestly.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::ir::{
    Bed, CableGroup, CamStitchSetting, CarrierId, CourseDir, GaugeSpec, KNIT_SCHEMA, KnitDoc,
    KnitPass, LoopKind, ParentRef, StitchNode, YarnRun,
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
    /// Width of the held center neckline, in mm (`0.0` for a closed straight top).
    pub neck_width_mm: f64,
    /// Neck drop below the shoulder line, in mm (`0.0` for a closed straight top).
    pub neck_depth_mm: f64,
    /// Number of 1x1 rib courses at the bottom hem (`0` for plain stockinette).
    pub rib_hem_courses: u32,
    /// Dimensionless stitch cam setting for main jersey body.
    pub cam_setting: f64,
    /// Catalog yarn identifier.
    pub yarn_id: String,
    /// Primary yarn carrier. An open neckline uses a second available carrier for the other
    /// shoulder, recorded in the drafted yarn runs and construction instructions.
    pub carrier_id: CarrierId,
    /// True if panel design includes a hood/neck drawstring (refused for child garments).
    #[serde(default)]
    pub has_hood_or_neck_drawstring: bool,
}

/// Parameters for a top-down shaped torso tube. Sleeve branches are not drafted.
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
    /// Collar 1x1 rib course count. Ribbed tubes use interleaved half-gauge needles.
    pub collar_rib_courses: u32,
    /// Bottom hem 1x1 rib course count, before the terminal bind-off pass.
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

/// A symmetric bottom-up pullover built from three independently fed tubes, joined
/// on both beds before the yoke narrows to the neck. Measurements include ease.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchedPulloverSpec {
    /// Neck, torso, yoke, rib, yarn, and body carrier settings.
    pub torso: RaglanPulloverSpec,
    /// Finished circumference of each wrist cuff, in mm.
    pub sleeve_cuff_girth_mm: f64,
    /// Finished circumference of each upper arm at the underarm, in mm.
    pub sleeve_upper_arm_girth_mm: f64,
    /// Sleeve length below the underarm, excluding cuff rib, in mm.
    pub sleeve_length_mm: f64,
    /// 1x1 rib courses at each wrist before sleeve shaping.
    pub cuff_rib_courses: u32,
    /// Separate yarn supply for the left sleeve, continued through the joined yoke.
    pub left_sleeve_carrier: CarrierId,
    /// Separate yarn supply for the right sleeve, removed after the underarm join.
    pub right_sleeve_carrier: CarrierId,
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

    // Keep the final course on the needles for an explicit manual bind-off. An automatic
    // chain bind-off needs transfers, which unqualified calibration swatches forbid.
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
                setting: *cam_settings.last().unwrap(),
                machine_family: profile.id.clone(),
            },
            yarn_length_mm: Some(est_loop_mm),
        });
        next_id += 1;
    }

    let mut metadata = BTreeMap::new();
    metadata.insert("calibration_swatch_only".to_string(), "true".to_string());
    metadata.insert("completion".to_string(), "manual_bind_off".to_string());
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
        passes: Vec::new(),
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

/// Drafts a measured flat panel with hem rib, side taper, armhole decreases, and explicit
/// chain bind-off. An open neckline holds its center loops while two separate yarn carriers
/// form the shoulder sections and their independent bind-off chains. The held neck loops
/// require manual finishing; no anchored short-row construction is claimed.
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
    // Finish width shaping before the terminal chain, so every bind-off starts with its own loop.
    let last_body_course = total_courses - 1;
    let armhole_start = last_body_course
        .saturating_sub(armhole_courses)
        .max(spec.rib_hem_courses + 1);
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
    if (spec.neck_width_mm > 0.0) != (spec.neck_depth_mm > 0.0) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "an open panel neckline requires both positive neck_width_mm and neck_depth_mm",
        ));
    }
    let neck_courses = (spec.neck_depth_mm / course_height).round() as u32;
    // The real terminal bind-off is a formed course. Stop the center neck_courses below
    // it, so the two shoulders receive exactly the requested quantized additional height.
    let neck_start = total_courses.saturating_sub(neck_courses) + 1;
    if spec.neck_depth_mm > 0.0 && (neck_courses == 0 || neck_start <= spec.rib_hem_courses + 1) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "neck drop must span at least one course and leave body fabric above the hem rib",
        ));
    }
    let neck_half = if neck_courses > 0 {
        let half = ((spec.neck_width_mm * 0.5) / wale_pitch).round() as i32;
        if half < 1 || half >= min_shoulder_half {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "neck opening must fit strictly inside the remaining shoulder span",
            ));
        }
        half
    } else {
        0
    };
    let shoulder_carrier = if neck_courses > 0 {
        Some(
            profile
                .carrier_ids
                .iter()
                .find(|carrier| **carrier != spec.carrier_id)
                .cloned()
                .ok_or_else(|| {
                    Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        "separate shoulders require a second yarn carrier to avoid a float across the neckline",
                    )
                    .repair("use a profile with two carriers or draft a closed straight top")
                })?,
        )
    } else {
        None
    };
    let primary_on_left = neck_start.is_multiple_of(2);
    if spec.rib_hem_courses > 0 && !profile.has_back_bed {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "a ribbed panel requires independently controlled front and back beds",
        )
        .repair("use a double-bed machine or request zero rib_hem_courses"));
    }
    if armhole_inset_per_side > 0 && (armhole_courses == 0 || armhole_start >= last_body_course) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "panel has no knitted courses available for the requested armhole decrease",
        )
        .repair("increase the panel or armhole depth, or reduce the rib course count"));
    }

    let est_loop_mm = 2.6 * wale_pitch;
    let mut stitches = Vec::new();
    let mut held_neck_stitches = Vec::new();
    let mut prev_map: BTreeMap<i32, u32> = BTreeMap::new();
    let mut next_id = 0u32;

    let half_wales_at = |c: u32| -> i32 {
        let c = c.min(last_body_course);
        if c <= spec.rib_hem_courses {
            hem_wales / 2
        } else if c < armhole_start {
            let span = (armhole_start - spec.rib_hem_courses).max(1) as f64;
            let t = f64::from(c - spec.rib_hem_courses) / span;
            let w = f64::from(hem_wales) + t * f64::from(chest_wales - hem_wales);
            (w * 0.5).round().max(2.0) as i32
        } else {
            let span = (last_body_course - armhole_start).max(1) as f64;
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
        let is_rib = c > 0 && c <= spec.rib_hem_courses;
        let separate_shoulders = neck_courses > 0 && c >= neck_start;
        if separate_shoulders && half <= neck_half {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "neck opening leaves no shoulder stitches at the requested shaping course",
            ));
        }
        if neck_courses > 0 && c == neck_start {
            held_neck_stitches.extend(prev_map.range(-neck_half..neck_half).map(|(_, &id)| id));
        }
        let wales = if separate_shoulders {
            // Both shoulders begin at their outer edge and then alternate independently.
            let mut left_wales: Vec<i32> = (left..-neck_half).collect();
            let mut right_wales: Vec<i32> = (neck_half..=right).collect();
            if (c - neck_start).is_multiple_of(2) {
                right_wales.reverse();
            } else {
                left_wales.reverse();
            }
            left_wales.extend(right_wales);
            left_wales
        } else {
            let mut wales: Vec<i32> = (left..=right).collect();
            if !c.is_multiple_of(2) {
                wales.reverse();
            }
            wales
        };
        let mut bind_chains = BTreeMap::new();

        for w in wales {
            let carrier = if separate_shoulders && ((w < 0) != primary_on_left) {
                shoulder_carrier
                    .as_ref()
                    .expect("open neck has second carrier")
            } else {
                &spec.carrier_id
            };
            let bed = if is_rib && (w.rem_euclid(2) == 1) {
                Bed::Back
            } else {
                Bed::Front
            };
            let mut parents = Vec::new();
            if let Some(&pid) = prev_map.get(&w) {
                append_parent(&mut parents, pid, profile)?;
            }
            // Every disappearing edge loop belongs to the retained edge's decrease. Never
            // lose a loop when a coarse course removes more than one needle on either side.
            if w == left {
                for (_, &pid) in prev_map.range(..left) {
                    append_parent(&mut parents, pid, profile)?;
                }
            } else if w == right {
                for (&old_wale, &pid) in prev_map.iter().rev() {
                    if old_wale <= right {
                        break;
                    }
                    append_parent(&mut parents, pid, profile)?;
                }
            }
            let kind = if c == total_courses {
                if let Some(&pid) = bind_chains.get(carrier) {
                    append_parent(&mut parents, pid, profile)?;
                }
                bind_chains.insert(carrier.clone(), next_id);
                LoopKind::BindOff
            } else if parents.is_empty() {
                // Edge widening casts on a new loop; it must not consume a neighbor's loop
                // already used by that neighbor in this course.
                LoopKind::CastOn
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
                carrier_ids: vec![carrier.clone()],
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
    for (key, value) in [
        ("hem_stitch_count", hem_wales.to_string()),
        ("chest_stitch_count", chest_wales.to_string()),
        ("formed_course_count", total_courses.to_string()),
        ("source_stitch_count", stitches.len().to_string()),
        (
            "shoulder_stitch_count_at_top",
            (2 * (min_shoulder_half - neck_half)).to_string(),
        ),
    ] {
        metadata.insert(key.to_string(), value);
    }
    for (key, residual) in [
        (
            "armhole_inset_total_residual_mm",
            f64::from(2 * armhole_inset_per_side) * wale_pitch - spec.armhole_inset_total_mm,
        ),
        (
            "neck_width_residual_mm",
            f64::from(2 * neck_half) * wale_pitch - spec.neck_width_mm,
        ),
        (
            "neck_depth_residual_mm",
            f64::from(neck_courses) * course_height - spec.neck_depth_mm,
        ),
    ] {
        metadata.insert(key.to_string(), format!("{residual:+.2}"));
    }
    let primary_count = stitches
        .iter()
        .filter(|stitch| stitch.carrier_ids[0] == spec.carrier_id)
        .count();
    let mut yarn_runs = vec![YarnRun {
        run_id: 0,
        carrier_id: spec.carrier_id.clone(),
        yarn_id: spec.yarn_id.clone(),
        start_course: 0,
        end_course: total_courses,
        total_yarn_length_m: primary_count as f64 * est_loop_mm / 1000.0,
        cut_tail_mm: Some(15.0),
    }];
    if let Some(second) = shoulder_carrier {
        let (left_carrier, right_carrier) = if primary_on_left {
            (&spec.carrier_id, &second)
        } else {
            (&second, &spec.carrier_id)
        };
        metadata.insert("shoulder_start_course".to_string(), neck_start.to_string());
        metadata.insert("left_shoulder_carrier".to_string(), left_carrier.0.clone());
        metadata.insert(
            "right_shoulder_carrier".to_string(),
            right_carrier.0.clone(),
        );
        metadata.insert(
            format!("shoulder_initial_direction_{}", left_carrier.as_str()),
            "plus".to_string(),
        );
        metadata.insert(
            format!("shoulder_initial_direction_{}", right_carrier.as_str()),
            "minus".to_string(),
        );
        let held_ids = held_neck_stitches
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        metadata.insert("held_neck_stitch_ids".to_string(), held_ids.clone());
        metadata.insert(
            "neckline_construction".to_string(),
            "center_loops_held_separate_shoulder_carriers".to_string(),
        );
        metadata.insert(
            "completion".to_string(),
            "manual_neckline_finishing".to_string(),
        );
        metadata.insert(
            "manual_finishing".to_string(),
            format!(
                "Neckline stitch IDs [{held_ids}] remain on hold after chart course {}. Work the left/right shoulders with carriers {}/{} using separate supplies of the stated yarn. The shoulders have independent bind-off chains; secure their final loops and finish the held neckline manually. No anchored short-row construction is claimed.",
                neck_start,
                left_carrier.as_str(),
                right_carrier.as_str(),
            ),
        );
        yarn_runs.push(YarnRun {
            run_id: 1,
            carrier_id: second,
            yarn_id: spec.yarn_id.clone(),
            start_course: neck_start,
            end_course: total_courses,
            total_yarn_length_m: (stitches.len() - primary_count) as f64 * est_loop_mm / 1000.0,
            cut_tail_mm: Some(15.0),
        });
    }

    let doc = KnitDoc {
        schema: KNIT_SCHEMA.to_string(),
        name: spec.name.clone(),
        wearer_age_class: spec.wearer_age_class,
        gauge: gauge.clone(),
        stitches,
        passes: Vec::new(),
        short_row_turns: vec![],
        cable_groups: vec![],
        yarn_runs,
        has_hood_or_neck_drawstring: spec.has_hood_or_neck_drawstring,
        metadata,
    };
    doc.validate()?;
    Ok(doc)
}

/// Drafts a top-down shaped body tube with distinct front/back loops and chain bind-off.
/// This tube does not include sleeve openings or a branched raglan garment.
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

    let half_gauge = spec.collar_rib_courses != 0 || spec.hem_rib_courses != 0;

    let wale_pitch = gauge.blocked_wale_pitch_mm();
    let course_height = gauge.blocked_course_height_mm();
    let max_tube_st_f64 = f64::from(profile.needle_count) * if half_gauge { 1.0 } else { 2.0 };
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
    let required_span = max_per_bed * if half_gauge { 2 } else { 1 };
    if required_span > profile.needle_count {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "tubular chest requires a {required_span}-needle span (half-gauge: {half_gauge}), exceeding {}",
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
    if spec.hem_rib_courses > body_courses {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "hem rib must fit inside the requested underarm-to-hem body length",
        ));
    }
    let total_courses = spec
        .collar_rib_courses
        .checked_add(yoke_courses)
        .and_then(|c| c.checked_add(body_courses))
        .ok_or_else(|| Refusal::new(RefusalCode::OutOfRange, "raglan course count overflow"))?;
    if total_courses > MAX_DRAFT_COURSES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("raglan total courses {total_courses} exceeds course cap {MAX_DRAFT_COURSES}"),
        ));
    }
    let est_loop_mm = 2.6 * wale_pitch;

    let mut stitches = Vec::new();
    let mut passes = Vec::new();
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

        let rib_course = c < total_courses
            && ((spec.collar_rib_courses > 0 && c <= spec.collar_rib_courses)
                || c >= total_courses - spec.hem_rib_courses);
        let mut bind_chain = None;
        for (home_bed, prev_bed, cur_bed) in [
            (Bed::Front, &prev_front, &mut cur_front),
            (Bed::Back, &prev_back, &mut cur_back),
        ] {
            let mut wales: Vec<i32> = (left..=right).collect();
            let direction = if home_bed == Bed::Front {
                CourseDir::Plus
            } else {
                wales.reverse();
                CourseDir::Minus
            };
            let mut pass_ids = Vec::new();
            for w in wales {
                // The two logical tube sides occupy disjoint needle indices. Moving
                // alternate loops to the opposite bed forms rib without exchanging
                // the sides or changing their opposite yarn traversal directions.
                let wale = if half_gauge {
                    2 * w + i32::from(home_bed == Bed::Back)
                } else {
                    w
                };
                let bed = if half_gauge && rib_course && w.rem_euclid(2) != 0 {
                    home_bed.opposite()
                } else {
                    home_bed
                };
                let mut parents = Vec::new();
                if let Some(&pid) = prev_bed.get(&w) {
                    append_parent(&mut parents, pid, profile)?;
                }
                let kind = if c == total_courses {
                    if let Some(pid) = bind_chain {
                        append_parent(&mut parents, pid, profile)?;
                    }
                    bind_chain = Some(next_id);
                    LoopKind::BindOff
                } else if parents.is_empty() {
                    LoopKind::CastOn
                } else if bed == Bed::Back {
                    LoopKind::Purl
                } else {
                    LoopKind::Knit
                };
                stitches.push(StitchNode {
                    id: next_id,
                    course: c,
                    wale,
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
                pass_ids.push(next_id);
                cur_bed.insert(w, next_id);
                next_id += 1;
            }
            if half_gauge {
                passes.push(KnitPass {
                    direction,
                    stitch_ids: pass_ids,
                });
            }
        }
        prev_front = cur_front;
        prev_back = cur_back;
    }

    let total_yarn_m = stitches.len() as f64 * est_loop_mm / 1000.0;
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "pathway".to_string(),
        "top_down_shaped_torso_tube".to_string(),
    );
    if half_gauge {
        metadata.insert(
            "tube_structure".to_string(),
            "half_gauge_1x1_rib".to_string(),
        );
        metadata.insert(
            "collar_rib_courses".to_string(),
            spec.collar_rib_courses.to_string(),
        );
        metadata.insert(
            "hem_rib_courses".to_string(),
            spec.hem_rib_courses.to_string(),
        );
        metadata.insert(
            "physical_needle_layout".to_string(),
            "front side: even indices; back side: odd indices; rib alternates beds".to_string(),
        );
    }
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
        passes,
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

struct HalfGaugeRounds<'a> {
    torso: &'a RaglanPulloverSpec,
    profile: &'a MachineProfile,
    loop_length_mm: f64,
    stitches: Vec<StitchNode>,
    passes: Vec<KnitPass>,
}

impl HalfGaugeRounds<'_> {
    fn push(
        &mut self,
        course: u32,
        carrier: &CarrierId,
        parents: &BTreeMap<(Bed, i32), Vec<u32>>,
        rib: bool,
        bind_off: bool,
    ) -> Result<BTreeMap<(Bed, i32), u32>, Refusal> {
        let mut current = BTreeMap::new();
        let mut bind_chain = None;
        for (home, direction) in [(Bed::Front, CourseDir::Plus), (Bed::Back, CourseDir::Minus)] {
            let mut positions: Vec<_> = parents
                .iter()
                .filter(|((bed, _), _)| *bed == home)
                .collect();
            if direction == CourseDir::Minus {
                positions.reverse();
            }
            let mut stitch_ids = Vec::new();
            for (&(_, wale), old) in positions {
                let id = u32::try_from(self.stitches.len()).map_err(|_| {
                    Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "branched pullover exceeds the stitch identity range",
                    )
                })?;
                let mut parent_refs = Vec::new();
                for &parent in old {
                    append_parent(&mut parent_refs, parent, self.profile)?;
                }
                let bed = if rib && wale.rem_euclid(2) != 0 {
                    home.opposite()
                } else {
                    home
                };
                let kind = if bind_off {
                    if old.is_empty() {
                        return Err(Refusal::new(
                            RefusalCode::MachineStateInfeasible,
                            "terminal tubular bind-off cannot create an unparented loop",
                        ));
                    }
                    if let Some(parent) = bind_chain {
                        append_parent(&mut parent_refs, parent, self.profile)?;
                    }
                    bind_chain = Some(id);
                    LoopKind::BindOff
                } else if old.is_empty() {
                    LoopKind::CastOn
                } else if old.len() > 1 {
                    LoopKind::Decrease
                } else if bed == Bed::Back {
                    LoopKind::Purl
                } else {
                    LoopKind::Knit
                };
                self.stitches.push(StitchNode {
                    id,
                    course,
                    wale: 2 * wale + i32::from(home == Bed::Back),
                    bed,
                    kind,
                    parents: parent_refs,
                    carrier_ids: vec![carrier.clone()],
                    yarn_id: self.torso.yarn_id.clone(),
                    cam_stitch_setting: CamStitchSetting {
                        setting: self.torso.cam_setting,
                        machine_family: self.profile.id.clone(),
                    },
                    yarn_length_mm: Some(self.loop_length_mm),
                });
                stitch_ids.push(id);
                current.insert((home, wale), id);
            }
            self.passes.push(KnitPass {
                direction,
                stitch_ids,
            });
        }
        Ok(current)
    }
}

/// Drafts actual cuff, torso, and sleeve branches and joins their edge loops on both
/// beds. Sleeve and torso lengths start at different courses so their underarms meet
/// together. The left sleeve's continuous yarn forms the narrowed yoke and collar.
///
/// # Errors
/// Refuses insufficient carriers, half-gauge bed width, extreme course counts, or
/// a shaping step that would exceed the machine hook-stack capacity.
pub fn draft_branched_raglan_pullover(
    spec: &BranchedPulloverSpec,
    gauge: &GaugeSpec,
    profile: &MachineProfile,
) -> Result<KnitDoc, Refusal> {
    gauge.validate()?;
    profile.validate()?;
    let torso = &spec.torso;
    if torso.name.trim().is_empty() || torso.yarn_id.trim().is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "branched pullover needs a name and yarn",
        ));
    }
    if !profile.has_back_bed || profile.needle_count > i32::MAX as u32 {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "branched pullover requires two addressable main needle beds",
        ));
    }
    let carriers = [
        &spec.left_sleeve_carrier,
        &torso.carrier_id,
        &spec.right_sleeve_carrier,
    ];
    for (index, carrier) in carriers.iter().enumerate() {
        CarrierId::new(carrier.as_str())?;
        if !profile.carrier_ids.contains(carrier) || carriers[..index].contains(carrier) {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "branched pullover requires three distinct available yarn carriers",
            ));
        }
    }
    for (name, value) in [
        ("neck_girth_mm", torso.neck_girth_mm),
        ("chest_girth_mm", torso.chest_girth_mm),
        ("yoke_depth_mm", torso.yoke_depth_mm),
        ("body_length_below_arm_mm", torso.body_length_below_arm_mm),
        ("sleeve_cuff_girth_mm", spec.sleeve_cuff_girth_mm),
        ("sleeve_upper_arm_girth_mm", spec.sleeve_upper_arm_girth_mm),
        ("sleeve_length_mm", spec.sleeve_length_mm),
    ] {
        Refusal::finite(name, value)?;
        if value <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("{name} must be positive"),
            ));
        }
    }
    Refusal::finite("cam_setting", torso.cam_setting)?;
    if torso.chest_girth_mm < torso.neck_girth_mm
        || spec.sleeve_upper_arm_girth_mm < spec.sleeve_cuff_girth_mm
        || torso.cam_setting < profile.cam_setting_range[0]
        || torso.cam_setting > profile.cam_setting_range[1]
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "branched pullover needs neck <= chest, cuff <= upper arm, and a supported cam setting",
        ));
    }
    let pitch = gauge.blocked_wale_pitch_mm();
    let height = gauge.blocked_course_height_mm();
    let stitch_count = |mm: f64| -> Result<i32, Refusal> {
        let estimate = (mm / pitch).round();
        if estimate < 8.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "each branch circumference must provide at least eight stitches at the selected gauge",
            ));
        }
        if !estimate.is_finite() || estimate > f64::from(profile.needle_count) {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "branch circumference exceeds the half-gauge bed capacity",
            ));
        }
        let rounded = (estimate as u32).div_ceil(4) * 4;
        if rounded > profile.needle_count || rounded > i32::MAX as u32 {
            return Err(Refusal::new(
                RefusalCode::MachineStateInfeasible,
                "rounded branch circumference exceeds the half-gauge bed capacity",
            ));
        }
        Ok(rounded as i32)
    };
    let course_count = |mm: f64| -> Result<u32, Refusal> {
        let estimate = (mm / height).round();
        if estimate < 4.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "each branched garment length must provide at least four courses at the selected gauge",
            ));
        }
        if !estimate.is_finite() || estimate > f64::from(MAX_DRAFT_COURSES) {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "branched pullover dimension exceeds the course allocation cap",
            ));
        }
        Ok(estimate as u32)
    };
    let neck_st = stitch_count(torso.neck_girth_mm)?;
    let chest_st = stitch_count(torso.chest_girth_mm)?;
    let cuff_st = stitch_count(spec.sleeve_cuff_girth_mm)?;
    let arm_st = stitch_count(spec.sleeve_upper_arm_girth_mm)?;
    let required_span = i64::from(chest_st) + 2 * i64::from(arm_st);
    if required_span > i64::from(profile.needle_count) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!(
                "three half-gauge branches need {required_span} needle positions, but '{}' has {}",
                profile.id, profile.needle_count,
            ),
        ));
    }
    let body_courses = course_count(torso.body_length_below_arm_mm)?;
    let sleeve_courses = course_count(spec.sleeve_length_mm)?;
    let yoke_courses = course_count(torso.yoke_depth_mm)?;
    let overflow = || {
        Refusal::new(
            RefusalCode::BudgetExceeded,
            "branched pullover exceeds the course allocation cap",
        )
    };
    if torso.hem_rib_courses > body_courses {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "hem rib must fit inside the requested underarm-to-hem body length",
        ));
    }
    let body_rounds = body_courses;
    let sleeve_rounds = sleeve_courses
        .checked_add(spec.cuff_rib_courses)
        .ok_or_else(overflow)?;
    let lower_rounds = body_rounds.max(sleeve_rounds);
    let join_course = lower_rounds.checked_add(1).ok_or_else(overflow)?;
    let end_course = join_course
        .checked_add(yoke_courses)
        .and_then(|value| value.checked_add(torso.collar_rib_courses))
        .ok_or_else(overflow)?;
    if end_course > MAX_DRAFT_COURSES {
        return Err(overflow());
    }
    let arm_width = arm_st / 2;
    let body_width = chest_st / 2;
    let full_width = body_width + 2 * arm_width;
    struct Branch<'a> {
        carrier: &'a CarrierId,
        left: i32,
        width: i32,
        initial_width: i32,
        start: u32,
        rib: u32,
        growth: u32,
    }
    let branches = [
        Branch {
            carrier: &spec.left_sleeve_carrier,
            left: 0,
            width: arm_width,
            initial_width: cuff_st / 2,
            start: lower_rounds - sleeve_rounds,
            rib: spec.cuff_rib_courses,
            growth: sleeve_courses,
        },
        Branch {
            carrier: &torso.carrier_id,
            left: arm_width,
            width: body_width,
            initial_width: body_width,
            start: lower_rounds - body_rounds,
            rib: torso.hem_rib_courses,
            growth: body_courses,
        },
        Branch {
            carrier: &spec.right_sleeve_carrier,
            left: arm_width + body_width,
            width: arm_width,
            initial_width: cuff_st / 2,
            start: lower_rounds - sleeve_rounds,
            rib: spec.cuff_rib_courses,
            growth: sleeve_courses,
        },
    ];
    let mut builder = HalfGaugeRounds {
        torso,
        profile,
        loop_length_mm: 2.6 * pitch,
        stitches: Vec::new(),
        passes: Vec::new(),
    };
    let mut previous: [BTreeMap<(Bed, i32), u32>; 3] = std::array::from_fn(|_| BTreeMap::new());
    for course in 0..=lower_rounds {
        for (index, branch) in branches.iter().enumerate() {
            if course < branch.start {
                continue;
            }
            let local = course - branch.start;
            let t = f64::from(local.saturating_sub(branch.rib)) / f64::from(branch.growth);
            let width = branch.initial_width
                + (f64::from(branch.width - branch.initial_width) * t).round() as i32;
            let left = branch.left + (branch.width - width) / 2;
            let mut parents = BTreeMap::new();
            for bed in [Bed::Front, Bed::Back] {
                for wale in left..left + width {
                    parents.insert(
                        (bed, wale),
                        previous[index]
                            .get(&(bed, wale))
                            .copied()
                            .into_iter()
                            .collect(),
                    );
                }
            }
            previous[index] = builder.push(
                course,
                branch.carrier,
                &parents,
                branch.rib > 0 && local <= branch.rib,
                false,
            )?;
        }
    }
    // Both torso edge loops stay at their own hooks. Each adjacent sleeve moves
    // inward by one logical wale, making two real underarm decreases on each bed.
    // Processing torso parents first defines the bottom-to-top decrease order.
    let mut joined = BTreeMap::<(Bed, i32), Vec<u32>>::new();
    for index in [1, 0, 2] {
        for (&(bed, wale), &id) in &previous[index] {
            let target = match index {
                0 => wale + 1,
                2 => wale - 1,
                _ => wale,
            };
            joined.entry((bed, target)).or_default().push(id);
        }
    }
    let mut current = builder.push(
        join_course,
        &spec.left_sleeve_carrier,
        &joined,
        false,
        false,
    )?;
    let joined_width = full_width - 2;
    let neck_width = neck_st / 2;
    for step in 1..yoke_courses {
        let reduction = (f64::from(joined_width - neck_width) * f64::from(step)
            / f64::from(yoke_courses - 1))
        .round() as i32;
        let width = joined_width - reduction;
        let left = 1 + reduction / 2;
        let right = left + width - 1;
        let mut parents = BTreeMap::new();
        for bed in [Bed::Front, Bed::Back] {
            for wale in left..=right {
                let own = current
                    .get(&(bed, wale))
                    .copied()
                    .into_iter()
                    .collect::<Vec<_>>();
                parents.insert((bed, wale), own);
            }
            for (&(old_bed, wale), &id) in &current {
                if old_bed == bed && wale < left {
                    parents.get_mut(&(bed, left)).expect("left target").push(id);
                }
            }
            for (&(old_bed, wale), &id) in current.iter().rev() {
                if old_bed == bed && wale > right {
                    parents
                        .get_mut(&(bed, right))
                        .expect("right target")
                        .push(id);
                }
            }
        }
        current = builder.push(
            join_course + step,
            &spec.left_sleeve_carrier,
            &parents,
            false,
            false,
        )?;
    }
    for collar in 0..torso.collar_rib_courses {
        let parents = current
            .iter()
            .map(|(address, id)| (*address, vec![*id]))
            .collect();
        current = builder.push(
            join_course + yoke_courses + collar,
            &spec.left_sleeve_carrier,
            &parents,
            true,
            false,
        )?;
    }
    let terminal = current
        .iter()
        .map(|(address, id)| (*address, vec![*id]))
        .collect();
    builder.push(
        end_course,
        &spec.left_sleeve_carrier,
        &terminal,
        false,
        true,
    )?;
    let mut yarn_runs = Vec::new();
    for (index, branch) in branches.iter().enumerate() {
        let yarn_m = builder
            .stitches
            .iter()
            .filter(|node| node.carrier_ids[0] == *branch.carrier)
            .filter_map(|node| node.yarn_length_mm)
            .sum::<f64>()
            / 1000.0;
        yarn_runs.push(YarnRun {
            run_id: index as u32,
            carrier_id: branch.carrier.clone(),
            yarn_id: torso.yarn_id.clone(),
            start_course: branch.start,
            end_course: if index == 0 { end_course } else { lower_rounds },
            total_yarn_length_m: yarn_m,
            cut_tail_mm: Some(15.0),
        });
    }
    let mut metadata = BTreeMap::new();
    for (key, value) in [
        ("pathway", "bottom_up_three_tube_raglan".to_string()),
        (
            "boundary_rings",
            "waist_hem,neck,left_wrist_cuff,right_wrist_cuff".to_string(),
        ),
        ("tube_structure", "interleaved_half_gauge".to_string()),
        ("join_course", join_course.to_string()),
        ("upper_yoke_carrier", spec.left_sleeve_carrier.0.clone()),
        ("left_sleeve_carrier", spec.left_sleeve_carrier.0.clone()),
        ("right_sleeve_carrier", spec.right_sleeve_carrier.0.clone()),
        ("body_start_course", branches[1].start.to_string()),
        ("sleeve_start_course", branches[0].start.to_string()),
        ("source_stitch_count", builder.stitches.len().to_string()),
        ("neck_stitch_count", neck_st.to_string()),
        ("chest_stitch_count", chest_st.to_string()),
        ("cuff_stitch_count", cuff_st.to_string()),
        ("upper_arm_stitch_count", arm_st.to_string()),
        ("body_course_count", body_courses.to_string()),
        ("sleeve_plain_course_count", sleeve_courses.to_string()),
        ("yoke_course_count", yoke_courses.to_string()),
        ("hem_rib_courses", torso.hem_rib_courses.to_string()),
        ("cuff_rib_courses", spec.cuff_rib_courses.to_string()),
        ("collar_rib_courses", torso.collar_rib_courses.to_string()),
        (
            "body_total_length_mm",
            format!("{:.2}", f64::from(body_rounds) * height),
        ),
        (
            "sleeve_total_length_mm",
            format!("{:.2}", f64::from(sleeve_rounds) * height),
        ),
    ] {
        metadata.insert(key.to_string(), value);
    }
    for (key, actual, requested) in [
        (
            "neck_girth_residual_mm",
            f64::from(neck_st) * pitch,
            torso.neck_girth_mm,
        ),
        (
            "chest_girth_residual_mm",
            f64::from(chest_st) * pitch,
            torso.chest_girth_mm,
        ),
        (
            "sleeve_cuff_girth_residual_mm",
            f64::from(cuff_st) * pitch,
            spec.sleeve_cuff_girth_mm,
        ),
        (
            "sleeve_upper_arm_girth_residual_mm",
            f64::from(arm_st) * pitch,
            spec.sleeve_upper_arm_girth_mm,
        ),
        (
            "sleeve_length_residual_mm",
            f64::from(sleeve_courses) * height,
            spec.sleeve_length_mm,
        ),
        (
            "body_length_residual_mm",
            f64::from(body_courses) * height,
            torso.body_length_below_arm_mm,
        ),
        (
            "yoke_depth_residual_mm",
            f64::from(yoke_courses) * height,
            torso.yoke_depth_mm,
        ),
    ] {
        metadata.insert(key.to_string(), format!("{:+.2}", actual - requested));
    }
    let doc = KnitDoc {
        schema: KNIT_SCHEMA.to_string(),
        name: torso.name.clone(),
        wearer_age_class: torso.wearer_age_class,
        gauge: gauge.clone(),
        stitches: builder.stitches,
        passes: builder.passes,
        short_row_turns: Vec::new(),
        cable_groups: Vec::new(),
        yarn_runs,
        has_hood_or_neck_drawstring: torso.has_hood_or_neck_drawstring,
        metadata,
    };
    doc.validate()?;
    Ok(doc)
}

/// Drafts an open-neck panel with tuck-anchored short-row wedges on both shoulders.
/// Outer shoulder edges retain the requested panel height; inner stitches remain
/// held for progressively shorter intervals, then all shoulder loops are rejoined
/// before the two independent bind-off chains. Center neck loops still need finishing.
///
/// # Errors
/// Refuses insufficient neck depth/shoulder width or shaping beyond the hook capacity.
pub fn draft_short_row_neckline_panel(
    spec: &ParametricPanelSpec,
    gauge: &GaugeSpec,
    profile: &MachineProfile,
    shoulder_short_row_pairs: u32,
) -> Result<KnitDoc, Refusal> {
    let original = draft_shaped_panel(spec, gauge, profile)?;
    if shoulder_short_row_pairs == 0 {
        return Ok(original);
    }
    let invalid = || {
        Refusal::new(
            RefusalCode::OutOfRange,
            "anchored neckline short rows require an open neck, sufficient shoulder width, and 2*pairs+1 forming courses",
        )
    };
    let start: u32 = original
        .metadata
        .get("shoulder_start_course")
        .and_then(|value| value.parse().ok())
        .ok_or_else(invalid)?;
    let end: u32 = original.metadata["formed_course_count"]
        .parse()
        .map_err(|_| invalid())?;
    let required = shoulder_short_row_pairs
        .checked_mul(2)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(invalid)?;
    if end.saturating_sub(start) < required {
        return Err(invalid());
    }
    let left_carrier = CarrierId::new(original.metadata["left_shoulder_carrier"].clone())?;
    let right_carrier = CarrierId::new(original.metadata["right_shoulder_carrier"].clone())?;
    let mut by_course: BTreeMap<u32, Vec<&StitchNode>> = BTreeMap::new();
    for node in &original.stitches {
        by_course.entry(node.course).or_default().push(node);
    }
    let left_inner = by_course[&start]
        .iter()
        .filter(|node| node.carrier_ids[0] == left_carrier)
        .map(|node| node.wale)
        .max()
        .ok_or_else(invalid)?;
    let right_inner = by_course[&start]
        .iter()
        .filter(|node| node.carrier_ids[0] == right_carrier)
        .map(|node| node.wale)
        .min()
        .ok_or_else(invalid)?;
    let mut live: BTreeMap<i32, Vec<u32>> = by_course[&(start - 1)]
        .iter()
        .filter(|node| node.wale <= left_inner || node.wale >= right_inner)
        .map(|node| (node.wale, vec![node.id]))
        .collect();
    let mut result = original.clone();
    result.stitches.retain(|node| node.course < start);
    result.passes.clear();
    let mut next_id = result
        .stitches
        .iter()
        .map(|node| node.id)
        .max()
        .unwrap_or(0)
        + 1;
    for (&course, nodes) in by_course.range(..start) {
        let direction = if course.is_multiple_of(2) {
            CourseDir::Plus
        } else {
            CourseDir::Minus
        };
        let mut sorted = nodes.clone();
        sorted.sort_by_key(|node| node.wale);
        if direction == CourseDir::Minus {
            sorted.reverse();
        }
        result.passes.push(KnitPass {
            direction,
            stitch_ids: sorted.iter().map(|node| node.id).collect(),
        });
    }
    let mut last_direction = result.passes.last().map(|pass| pass.direction);
    for course in start..=end {
        let row = course - start;
        let partial = row < shoulder_short_row_pairs * 2;
        let outgoing = row.is_multiple_of(2);
        let mut sides = [
            (
                if outgoing {
                    CourseDir::Plus
                } else {
                    CourseDir::Minus
                },
                &left_carrier,
                true,
            ),
            (
                if outgoing {
                    CourseDir::Minus
                } else {
                    CourseDir::Plus
                },
                &right_carrier,
                false,
            ),
        ];
        sides.sort_by_key(|(direction, _, _)| last_direction == Some(*direction));
        for (direction, carrier, is_left) in sides {
            let mut templates: Vec<_> = by_course[&course]
                .iter()
                .copied()
                .filter(|node| node.carrier_ids[0] == *carrier)
                .collect();
            templates.sort_by_key(|node| node.wale);
            let min = templates.first().ok_or_else(invalid)?.wale;
            let max = templates.last().ok_or_else(invalid)?.wale;
            let outer = if is_left { min } else { max };
            let mut removed = Vec::new();
            let old_wales: Vec<_> = live
                .keys()
                .copied()
                .filter(|wale| {
                    let on_side = if is_left {
                        *wale <= left_inner
                    } else {
                        *wale >= right_inner
                    };
                    on_side && (*wale < min || *wale > max)
                })
                .collect();
            for wale in old_wales {
                removed.extend(live.remove(&wale).expect("enumerated live hook"));
            }
            if !is_left {
                removed.reverse();
            }
            let anchor = if partial {
                let held_inner = (shoulder_short_row_pairs - 1 - row / 2) as i32;
                if is_left {
                    max - held_inner
                } else {
                    min + held_inner
                }
            } else {
                outer
            };
            if partial && (anchor <= min || anchor >= max) {
                // An endpoint anchor is allowed at the inner edge, but at least
                // one ordinary stitch must remain toward the outer edge.
                if (is_left && anchor <= min) || (!is_left && anchor >= max) {
                    return Err(invalid());
                }
            }
            if partial {
                templates.retain(|node| {
                    if is_left {
                        node.wale < anchor || (outgoing && node.wale == anchor)
                    } else {
                        node.wale > anchor || (outgoing && node.wale == anchor)
                    }
                });
            }
            if templates.is_empty() {
                return Err(invalid());
            }
            if direction == CourseDir::Minus {
                templates.reverse();
            }
            let mut ids = Vec::new();
            let mut bind_chain = None;
            for template in templates {
                let mut node = template.clone();
                node.id = next_id;
                node.parents.clear();
                let old = live.remove(&node.wale).unwrap_or_default();
                for &id in &old {
                    append_parent(&mut node.parents, id, profile)?;
                }
                if node.wale == outer {
                    for &id in &removed {
                        append_parent(&mut node.parents, id, profile)?;
                    }
                }
                let is_anchor = partial && outgoing && node.wale == anchor;
                node.kind = if course == end {
                    if let Some(id) = bind_chain {
                        append_parent(&mut node.parents, id, profile)?;
                    }
                    bind_chain = Some(next_id);
                    LoopKind::BindOff
                } else if is_anchor {
                    if old.is_empty() {
                        return Err(invalid());
                    }
                    LoopKind::Tuck
                } else if node.parents.is_empty() {
                    LoopKind::CastOn
                } else if node.parents.len() > 1 {
                    LoopKind::Decrease
                } else {
                    LoopKind::Knit
                };
                node.bed = Bed::Front;
                let stack = if is_anchor {
                    let mut stack = old;
                    stack.push(next_id);
                    stack
                } else {
                    vec![next_id]
                };
                live.insert(node.wale, stack);
                ids.push(next_id);
                result.stitches.push(node);
                next_id += 1;
            }
            if partial && outgoing {
                result.short_row_turns.push(super::ir::ShortRowTurn {
                    course,
                    turn_wale: anchor,
                    dir_after: direction.reversed(),
                    anchored: true,
                });
            }
            result.passes.push(KnitPass {
                direction,
                stitch_ids: ids,
            });
            last_direction = Some(direction);
        }
    }
    for run in &mut result.yarn_runs {
        run.total_yarn_length_m = result
            .stitches
            .iter()
            .filter(|node| node.carrier_ids.contains(&run.carrier_id))
            .filter_map(|node| node.yarn_length_mm)
            .sum::<f64>()
            / 1000.0;
    }
    result.metadata.insert(
        "source_stitch_count".to_string(),
        result.stitches.len().to_string(),
    );
    result.metadata.insert(
        "shoulder_short_row_pairs".to_string(),
        shoulder_short_row_pairs.to_string(),
    );
    result.metadata.insert(
        "neckline_construction".to_string(),
        "held_center_tuck_anchored_shoulders".to_string(),
    );
    result.metadata.insert("manual_finishing".to_string(),
        "Each short-row turn tucks into a held shoulder loop and is knitted off before shoulder bind-off. Secure both terminal chains and finish the separately held center neckline loops manually.".to_string());
    result.validate()?;
    Ok(result)
}

/// Adds explicit front-bed cable crossings to ordinary single-parent stockinette
/// rows. The stitch identities at each destination remain stable for later courses;
/// their incoming parents are exchanged, preserving the complete garment graph.
///
/// # Errors
/// Refuses overlapping/irregular blocks or a crossing through shaping, tucks, or rib.
pub fn apply_cable_crossings(doc: &KnitDoc, groups: &[CableGroup]) -> Result<KnitDoc, Refusal> {
    doc.validate()?;
    let mut result = doc.clone();
    for cable in groups {
        cable.validate()?;
        let positions: BTreeMap<_, _> = result
            .stitches
            .iter()
            .enumerate()
            .filter(|(_, node)| node.course == cable.course && node.bed == Bed::Front)
            .map(|(index, node)| (node.wale, index))
            .collect();
        for (&left, &right) in cable.left_wales.iter().zip(&cable.right_wales) {
            let index = |wale: i32| -> Result<usize, Refusal> {
                positions.get(&wale).copied().ok_or_else(|| Refusal::new(
                    RefusalCode::MachineStateInfeasible,
                    "cable drafting requires an existing front-bed stitch at every crossing wale",
                ))
            };
            let left_index = index(left)?;
            let right_index = index(right)?;
            for position in [left_index, right_index] {
                let node = &result.stitches[position];
                if node.kind != LoopKind::Knit || node.parents.len() != 1 {
                    return Err(Refusal::new(
                        RefusalCode::MachineStateInfeasible,
                        "cable drafting requires ordinary single-parent front-bed stockinette stitches",
                    ));
                }
            }
            let left_parent = result.stitches[left_index].parents.clone();
            let right_parent = result.stitches[right_index].parents.clone();
            result.stitches[left_index].parents = right_parent;
            result.stitches[right_index].parents = left_parent;
            result.stitches[left_index].kind = LoopKind::Cable;
            result.stitches[right_index].kind = LoopKind::Cable;
        }
        result.cable_groups.push(cable.clone());
    }
    result.validate()?;
    Ok(result)
}

fn append_parent(
    parents: &mut Vec<ParentRef>,
    stitch_id: u32,
    profile: &MachineProfile,
) -> Result<(), Refusal> {
    if parents.len() >= usize::from(profile.max_loops_per_hook) {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            "drafted decrease or bind-off exceeds the machine hook-stack capacity",
        )
        .repair("spread the width change over more courses or reduce the armhole inset"));
    }
    parents.push(ParentRef {
        stitch_id,
        stack_pos: parents.len() as u8,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn assert_consumed_once(doc: &KnitDoc) {
        let mut uses: BTreeMap<u32, usize> = doc.stitches.iter().map(|st| (st.id, 0)).collect();
        for st in &doc.stitches {
            for (index, parent) in st.parents.iter().enumerate() {
                assert_eq!(usize::from(parent.stack_pos), index);
                *uses.get_mut(&parent.stitch_id).unwrap() += 1;
            }
            if st.kind == LoopKind::CastOn {
                assert!(st.parents.is_empty());
            }
        }
        let final_id = doc.stitches.last().unwrap().id;
        for st in &doc.stitches {
            assert_eq!(
                uses[&st.id],
                usize::from(st.id != final_id),
                "loop {}",
                st.id
            );
        }
        assert_eq!(doc.stitches.last().unwrap().kind, LoopKind::BindOff);
    }

    #[test]
    fn open_neck_has_real_separate_shoulders_exact_parents_and_independent_bind_offs() {
        let profile = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: profile.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.2),
            relaxed_after_wash: true,
        };
        let spec = ParametricPanelSpec {
            name: "toddler_front_with_neck".to_string(),
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
        };
        let doc = draft_shaped_panel(&spec, &gauge, &profile).unwrap();
        let neck_start: u32 = doc.metadata["shoulder_start_course"].parse().unwrap();
        let formed: u32 = doc.metadata["formed_course_count"].parse().unwrap();
        let held: BTreeSet<u32> = doc.metadata["held_neck_stitch_ids"]
            .split(',')
            .map(|id| id.parse().unwrap())
            .collect();
        assert_eq!(held.len(), 26);
        assert_eq!(formed, 115);
        assert_eq!(neck_start, 103);
        assert!(doc.short_row_turns.is_empty());
        assert_eq!(doc.wearer_age_class, spec.wearer_age_class);
        assert_eq!(doc.yarn_runs.len(), 2);
        assert_ne!(
            doc.metadata["left_shoulder_carrier"],
            doc.metadata["right_shoulder_carrier"]
        );
        let by_id: BTreeMap<_, _> = doc
            .stitches
            .iter()
            .map(|stitch| (stitch.id, stitch))
            .collect();
        let mut uses = BTreeMap::<u32, usize>::new();
        let mut terminal_chains = BTreeMap::<CarrierId, u32>::new();
        for stitch in &doc.stitches {
            for (position, parent) in stitch.parents.iter().enumerate() {
                assert_eq!(usize::from(parent.stack_pos), position);
                *uses.entry(parent.stitch_id).or_default() += 1;
            }
            if stitch.parents.is_empty() {
                assert_eq!(stitch.kind, LoopKind::CastOn);
            }
            if held.contains(&stitch.id) {
                assert_eq!(stitch.course, neck_start - 1);
                assert!((-13..13).contains(&stitch.wale));
            }
            if stitch.course >= neck_start {
                assert!(!(-13..13).contains(&stitch.wale));
                let key = if stitch.wale < 0 {
                    "left_shoulder_carrier"
                } else {
                    "right_shoulder_carrier"
                };
                assert_eq!(stitch.carrier_ids[0].as_str(), doc.metadata[key]);
            } else {
                assert_eq!(stitch.carrier_ids, vec![spec.carrier_id.clone()]);
            }
            if stitch.kind == LoopKind::BindOff {
                assert_eq!(stitch.course, formed);
                let previous = terminal_chains.insert(stitch.carrier_ids[0].clone(), stitch.id);
                assert_eq!(stitch.parents.len(), if previous.is_some() { 2 } else { 1 });
                assert_eq!(by_id[&stitch.parents[0].stitch_id].wale, stitch.wale);
                assert_eq!(by_id[&stitch.parents[0].stitch_id].course, formed - 1);
                if let Some(id) = previous {
                    assert_eq!(stitch.parents[1].stitch_id, id);
                }
            }
        }
        assert_eq!(terminal_chains.len(), 2);
        for stitch in &doc.stitches {
            let remains =
                held.contains(&stitch.id) || terminal_chains.values().any(|id| *id == stitch.id);
            assert_eq!(
                uses.get(&stitch.id).copied().unwrap_or(0),
                usize::from(!remains),
                "stitch {}",
                stitch.id
            );
        }
        assert!(
            doc.stitches
                .iter()
                .any(|stitch| stitch.course > 0 && stitch.kind == LoopKind::CastOn)
        );
        assert!(
            doc.stitches
                .iter()
                .any(|stitch| stitch.kind == LoopKind::Decrease)
        );
        let actual_drop = f64::from(formed - (neck_start - 1)) * gauge.blocked_course_height_mm();
        let depth_residual: f64 = doc.metadata["neck_depth_residual_mm"].parse().unwrap();
        let width_residual: f64 = doc.metadata["neck_width_residual_mm"].parse().unwrap();
        assert!((actual_drop - spec.neck_depth_mm - depth_residual).abs() <= 0.0051);
        assert!(
            (held.len() as f64 * gauge.blocked_wale_pitch_mm()
                - spec.neck_width_mm
                - width_residual)
                .abs()
                <= 0.0051
        );
        assert_eq!(
            doc.metadata["source_stitch_count"],
            doc.stitches.len().to_string()
        );
        assert!(doc.metadata["manual_finishing"].contains("remain on hold"));
        let yarn_total: f64 = doc
            .yarn_runs
            .iter()
            .map(|run| run.total_yarn_length_m)
            .sum();
        let expected_yarn =
            doc.stitches.len() as f64 * 2.6 * gauge.blocked_wale_pitch_mm() / 1000.0;
        assert!((yarn_total - expected_yarn).abs() < 1.0e-9);

        let mut one_carrier = profile.clone();
        one_carrier.carrier_ids.truncate(1);
        assert_eq!(
            draft_shaped_panel(&spec, &gauge, &one_carrier)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
        let mut inconsistent = spec.clone();
        inconsistent.neck_depth_mm = 0.0;
        assert_eq!(
            draft_shaped_panel(&inconsistent, &gauge, &profile)
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );
        inconsistent.neck_depth_mm = 0.1;
        assert_eq!(
            draft_shaped_panel(&inconsistent, &gauge, &profile)
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );
    }

    #[test]
    fn coarse_taper_retains_every_removed_edge_loop_and_declared_dimensions() {
        let prof = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 20.0,
            courses_per_100mm: 10.0,
            needle_pitch_mm: prof.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let spec = ParametricPanelSpec {
            name: "coarse_taper".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            hem_width_mm: 200.0,
            chest_width_mm: 100.0,
            length_mm: 80.0,
            armhole_depth_mm: 0.0,
            armhole_inset_total_mm: 0.0,
            neck_width_mm: 0.0,
            neck_depth_mm: 0.0,
            rib_hem_courses: 0,
            cam_setting: 8.0,
            yarn_id: "synthetic-yarn".to_string(),
            carrier_id: CarrierId("1".to_string()),
            has_hood_or_neck_drawstring: false,
        };
        let doc = draft_shaped_panel(&spec, &gauge, &prof).unwrap();
        assert_consumed_once(&doc);
        assert_eq!(doc.stitches.iter().filter(|st| st.course == 0).count(), 40);
        assert_eq!(doc.stitches.iter().filter(|st| st.course == 8).count(), 20);
        assert!(
            doc.stitches
                .iter()
                .any(|st| st.kind == LoopKind::Decrease && st.parents.len() >= 3)
        );
        let mut tight_hook = prof;
        tight_hook.max_loops_per_hook = 2;
        assert_eq!(
            draft_shaped_panel(&spec, &gauge, &tight_hook)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
    }

    #[test]
    fn calibration_swatch_enforces_hard_caps() {
        let prof = MachineProfile::kniterate_7e();
        let doc =
            draft_calibration_swatch(&prof, "wool-200tex", 40, 20, &[6.0, 8.0, 10.0]).unwrap();
        assert_eq!(doc.wearer_age_class, WearerAgeClass::CalibrationSwatchOnly);
        assert_eq!(doc.stitches.len(), 40 * (60 + 2));
        assert!(doc.stitches.iter().all(|st| st.kind != LoopKind::BindOff));
        assert_eq!(doc.metadata["completion"], "manual_bind_off");

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
        .unwrap();
        assert!(panel.metadata.contains_key("chest_width_residual_mm"));
        assert!(panel.short_row_turns.is_empty());
        assert_consumed_once(&panel);

        let raglan = draft_seamless_raglan_tube(
            &RaglanPulloverSpec {
                name: "toddler_torso_tube".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                neck_girth_mm: 280.0,
                chest_girth_mm: 580.0,
                yoke_depth_mm: 130.0,
                body_length_below_arm_mm: 200.0,
                collar_rib_courses: 0,
                hem_rib_courses: 0,
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
        assert_consumed_once(&raglan);
        let last_course = raglan.stitches.last().unwrap().course;
        let terminal: Vec<_> = raglan
            .stitches
            .iter()
            .filter(|st| st.course == last_course)
            .collect();
        assert_eq!(terminal.first().unwrap().bed, Bed::Front);
        assert_eq!(terminal.last().unwrap().bed, Bed::Back);
        for pair in terminal.windows(2) {
            assert_eq!(pair[1].parents.last().unwrap().stitch_id, pair[0].id);
        }

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
            neck_width_mm: 0.0,
            neck_depth_mm: 0.0,
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

        // A requested neckline must not silently become a rectangular straight top.
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
            name: "torso_tube".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            neck_girth_mm: 280.0,
            chest_girth_mm: 580.0,
            yoke_depth_mm: 130.0,
            body_length_below_arm_mm: 200.0,
            collar_rib_courses: 0,
            hem_rib_courses: 0,
            cam_setting: 8.0,
            yarn_id: "merino-180tex".to_string(),
            carrier_id: CarrierId("1".to_string()),
            has_hood_or_neck_drawstring: false,
        };
        let mut ribbed = base_raglan.clone();
        ribbed.collar_rib_courses = 2;
        ribbed.hem_rib_courses = 3;
        let rib_doc = draft_seamless_raglan_tube(&ribbed, &gauge, &prof).unwrap();
        assert_eq!(rib_doc.metadata["tube_structure"], "half_gauge_1x1_rib");
        assert_consumed_once(&rib_doc);
        assert!(!rib_doc.passes.is_empty());
        let mut too_narrow = prof.clone();
        too_narrow.needle_count = 128;
        assert!(draft_seamless_raglan_tube(&base_raglan, &gauge, &too_narrow).is_ok());
        assert_eq!(
            draft_seamless_raglan_tube(&ribbed, &gauge, &too_narrow)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );

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
