//! Fitting Anny to what is known of the wearer (plan §9.4).
//!
//! - [`fit_stature`] ("from age/sex/height"): the age and gender parameters are set directly, the height
//!   parameter is bisected until the rest body's height equals the stature, and the other phenotype
//!   parameters stay at the population middle (0.5).
//! - [`fit_tape`] ("from tape measurements"): Levenberg–Marquardt over the height and weight parameters and
//!   the `measure-*` local changes that move one measurement each ([`FREE`]), minimising the residuals
//!   between the wearer's measurements and what a tape reads on the posed body ([`tape_readings`]), with
//!   weak priors that keep unmeasured directions at the population middle. The residuals are reported,
//!   never hidden: a measurement Anny cannot reach within its parameter ranges stays a residual.
//!
//! The tape is read the way the measurement catalogue (spec 02) defines it, on the standing drape pose:
//!
//! | Measurement | Reading |
//! |---|---|
//! | `stature` | the rest body's height (Anny's own definition, as [`fit_stature`]) |
//! | `chest` | girth [`CHEST_BELOW_ARMPIT_MM`] below the armpit landmark, where the arms have left the section |
//! | `waist` | girth at the waist landmark (the mean height of Anny's waist loop) |
//! | `hip` | the largest girth from 20 % of the way up from the crotch to the waist, in 2 mm steps ("the fullest part of the seat") |
//! | `waist_height`, `crotch_height`, `knee_height` | the landmarks above the floor (the knee joint for the knee crease) |
//! | `thigh` | the largest leg girth 5–40 mm below the crotch |
//! | `knee` | leg girth at the knee joint |
//! | `calf` | the largest leg girth from 30 % to 90 % of the way up the shin |
//! | `ankle` | the smallest leg girth from 10 mm above the ankle joint to a quarter up the shin |
//! | `upper_arm` | arm girth midway between the shoulder point and the elbow (0.55 of the arm length down) |
//! | `wrist` | arm girth at the wrist |
//! | `arm_length` | shoulder point to wrist along the straight arm ([`BodyFrames::arm_length`]) |
//! | `back_neck_to_waist` | C7 down the back's median line to the waist level, along the surface |
//!
//! A torso girth is [`crate::tape::girth`] around the torso axis; a limb girth is taken perpendicular to
//! the right leg's or arm's axis (its part frame), around it.

use ftx_core::{Refusal, RefusalCode, Vec3, linalg::lstsq};
use serde_json::Value;

use crate::drape::{Pose, pose_deltas};
use crate::landmarks::BodyFrames;
use crate::model::{AnnyModel, BodyParams, Phenotype, PosedBody, age_parameter_for_years};
use crate::tape::{Slicer, in_frame};

/// The wearer as the measurement file states them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wearer {
    /// Age (years).
    pub years: f64,
    /// Anny gender: 0 male, 1 female, 0.5 unknown.
    pub gender: f64,
    /// Stature (mm).
    pub stature_mm: f64,
    /// Body weight (kg), when known: [`fit_tape`] fits Anny's mass to it.
    pub weight_kg: Option<f64>,
}

impl Wearer {
    /// Anny's gender parameter for a sex code as the measurement files write it (`M`/`B` male, `F`/`G`
    /// female, case-insensitive on the first letter); unknown or absent → 0.5.
    #[must_use]
    pub fn gender_of(sex: Option<&str>) -> f64 {
        match sex
            .and_then(|s| s.chars().next())
            .map(|c| c.to_ascii_uppercase())
        {
            Some('M' | 'B') => 0.0,
            Some('F' | 'G') => 1.0,
            _ => 0.5,
        }
    }

    /// The wearer of a measurement file (`fabric-measurements/1`): age from `wearer.age_months`, stature
    /// from `wearer.stature_mm` or else the fused stature, sex from `wearer.sex`, weight from
    /// `wearer.weight_kg` or else `wearer.bmi` × stature².
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] without an age or a stature, or with a weight or BMI that is not
    /// a positive number.
    pub fn from_measurements(meas: &Value) -> Result<Self, Refusal> {
        let bad = |m: &str| Refusal::new(RefusalCode::InvalidDocument, m);
        let w = &meas["wearer"];
        let months = w["age_months"].as_f64().ok_or_else(|| {
            bad("the measurement file has no wearer.age_months")
                .repair("add the wearer's age in months")
        })?;
        let stature = w["stature_mm"]
            .as_f64()
            .or_else(|| meas["fused"]["stature"]["value_mm"].as_f64())
            .ok_or_else(|| {
                bad("the measurement file has no stature (wearer.stature_mm or a fused stature)")
                    .repair("measure the stature, or fuse a stature prior")
            })?;
        let positive = |key: &str| -> Result<Option<f64>, Refusal> {
            match &w[key] {
                Value::Null => Ok(None),
                v => match v.as_f64() {
                    Some(x) if x.is_finite() && x > 0.0 => Ok(Some(x)),
                    _ => Err(bad(&format!("wearer.{key} must be a positive number"))),
                },
            }
        };
        let weight_kg = match positive("weight_kg")? {
            Some(kg) => Some(kg),
            None => positive("bmi")?.map(|bmi| bmi * (stature / 1000.0).powi(2)),
        };
        Ok(Self {
            years: months / 12.0,
            gender: Self::gender_of(w["sex"].as_str()),
            stature_mm: stature,
            weight_kg,
        })
    }
}

/// The tape targets of a measurement file: each [`TAPE_FIELDS`] name with a fused value, whatever its
/// provenance (the drape tests a pattern against the measurements it was drafted to, as the reference's
/// dress form does), the stature from `wearer.stature_mm` when given.
#[must_use]
pub fn tape_targets(meas: &Value) -> Vec<TapeTarget> {
    TAPE_FIELDS
        .iter()
        .filter_map(|&name| {
            let fused = meas["fused"][name]["value_mm"].as_f64();
            let value = if name == "stature" {
                meas["wearer"]["stature_mm"].as_f64().or(fused)
            } else {
                fused
            }?;
            Some(TapeTarget {
                name: name.to_string(),
                value_mm: value,
            })
        })
        .collect()
}

/// The body parameters for `wearer` and the stature reached (mm, within 0.01 mm).
///
/// # Errors
/// [`RefusalCode::OutOfRange`] for a non-positive or non-finite age or stature, or a stature Anny cannot
/// reach at that age with the other parameters at 0.5; refusals of the model.
pub fn fit_stature(model: &AnnyModel, wearer: &Wearer) -> Result<(BodyParams, f64), Refusal> {
    let Wearer {
        years,
        gender,
        stature_mm,
        ..
    } = *wearer;
    if !(years.is_finite() && years >= 0.0 && stature_mm.is_finite() && stature_mm > 0.0) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "age {years} y and stature {stature_mm} mm: the age must be finite and not negative, \
                 the stature finite and positive"
            ),
        ));
    }
    let base = Phenotype {
        gender,
        age: age_parameter_for_years(years),
        ..Phenotype::default()
    };
    let height_at = |h: f64| -> Result<f64, Refusal> {
        let p = model.params(Phenotype { height: h, ..base });
        Ok(model.anthropometry(&model.rest(&p)?).height * 1000.0)
    };
    let (shortest, tallest) = (height_at(0.0)?, height_at(1.0)?);
    if !(shortest..=tallest).contains(&stature_mm) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "a stature of {stature_mm:.0} mm is outside what the body model reaches at {years:.1} y \
                 ({shortest:.0}–{tallest:.0} mm with the other parameters at the population middle)"
            ),
        )
        .repair("check the stature and the age in the measurement file"));
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..50 {
        let mid = 0.5 * (lo + hi);
        if height_at(mid)? < stature_mm {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let h = 0.5 * (lo + hi);
    Ok((model.params(Phenotype { height: h, ..base }), height_at(h)?))
}

/// The measurements the tape fit reads, in the measurement files' names; [`tape_readings`] returns them
/// in this order (mm).
pub const TAPE_FIELDS: [&str; 21] = [
    "stature",
    "chest",
    "waist",
    "hip",
    "waist_height",
    "crotch_height",
    "thigh",
    "knee",
    "calf",
    "ankle",
    "upper_arm",
    "wrist",
    "arm_length",
    "knee_height",
    "back_neck_to_waist",
    "neck_base",
    "head",
    "shoulder_length",
    "across_back",
    "total_crotch_length",
    "outseam",
];

/// Number of tape readings.
pub const N_TAPE: usize = TAPE_FIELDS.len();

/// Whether a [`TAPE_FIELDS`] measurement is a girth (else a length or height).
#[must_use]
pub fn is_girth(name: &str) -> bool {
    matches!(
        name,
        "chest"
            | "waist"
            | "hip"
            | "thigh"
            | "knee"
            | "calf"
            | "ankle"
            | "upper_arm"
            | "wrist"
            | "neck_base"
            | "head"
    )
}

/// How far below the armpit landmark the chest is read (mm).
pub const CHEST_BELOW_ARMPIT_MM: f64 = 10.0;
/// Search step for the fullest or narrowest girth over a range of levels (mm).
const SEARCH_STEP_MM: f64 = 2.0;
/// Height slab of the slicers' triangle buckets (mm).
const SLAB_MM: f64 = 5.0;

/// The fit's tolerance (1σ, mm) for a girth and for a length: the accuracy gate G1b asks of it.
pub const FIT_SIGMA_GIRTH_MM: f64 = 3.0;
/// See [`FIT_SIGMA_GIRTH_MM`].
pub const FIT_SIGMA_LENGTH_MM: f64 = 2.0;

/// The fit's 1σ for a [`TAPE_FIELDS`] measurement (mm). Four lengths whose reading on a body rests on an
/// approximation of where a measurer puts the tape (the shoulder point taken above the shoulder joint, a tape
/// hanging from the hip, a snug tape through the crotch as a hull) pull more gently: 5 mm, and 8 mm for the
/// outseam (the catalogue's tape σ for them is 10–15 mm). Pulled to 2 mm they distort the body where the
/// definition, not the body, is uncertain (the 92 cm chart's outseam and shoulder length cannot both be met
/// with its crotch height).
#[must_use]
pub fn fit_sigma(name: &str) -> f64 {
    match name {
        "shoulder_length" | "across_back" | "total_crotch_length" => 5.0,
        "outseam" => 8.0,
        _ if is_girth(name) => FIT_SIGMA_GIRTH_MM,
        _ => FIT_SIGMA_LENGTH_MM,
    }
}

/// The free parameters of [`fit_tape`]: [`N_PHENOTYPE`] phenotype parameters (Anny's shape space,
/// learned from scans), then the `measure-*` local changes (authored blend shapes, one measurement each).
pub const FREE: [&str; 23] = [
    "height",
    "weight",
    "muscle",
    "proportions",
    "measure-bust-circ-incr",
    "measure-waist-circ-incr",
    "measure-hips-circ-incr",
    "measure-upperleg-height-incr",
    "measure-waisttohip-dist-incr",
    "measure-thigh-circ-incr",
    "measure-knee-circ-incr",
    "measure-calf-circ-incr",
    "measure-ankle-circ-incr",
    "measure-upperarm-circ-incr",
    "measure-wrist-circ-incr",
    "measure-upperarm-length-incr",
    "measure-lowerarm-length-incr",
    "measure-lowerleg-height-incr",
    "measure-napetowaist-dist-incr",
    "measure-neck-circ-incr",
    "measure-shoulder-dist-incr",
    // the head girth: both scales free, so the prior spreads a change evenly (a uniform scale)
    "head-scale-horiz-incr",
    "head-scale-depth-incr",
];
const N_FREE: usize = FREE.len();
/// How many of [`FREE`] are phenotype parameters.
pub const N_PHENOTYPE: usize = 4;
/// Phenotype parameters range over [0, 1]; local changes over [−1, 1], Anny's modelled range.
const LOCAL_LIMIT: f64 = 1.0;
/// The fit's tolerance for the weight: 1 % of it (a scale reads that well), at least 0.1 kg.
const WEIGHT_SIGMA_FRACTION: f64 = 0.01;
/// See [`WEIGHT_SIGMA_FRACTION`].
const WEIGHT_SIGMA_MIN_KG: f64 = 0.1;
/// Prior 1σ of a phenotype parameter about the population middle (0.5) and of a local change about 0:
/// the scanned shape space explains a body before the authored blend shapes adjust it.
const PRIOR_SIGMA_PHENOTYPE: f64 = 0.5;
/// See [`PRIOR_SIGMA_PHENOTYPE`].
const PRIOR_SIGMA_LOCAL: f64 = 0.35;
/// Central-difference step in the parameters.
const FD_STEP: f64 = 1e-3;
/// Levenberg–Marquardt iteration cap.
const MAX_ITERATIONS: usize = 40;
/// Stop once an accepted step lowers the cost by less than this fraction.
const REL_GAIN_STOP: f64 = 1e-5;

/// The fullest (`max`) or narrowest girth `g(h)` over `lo..=hi` in [`SEARCH_STEP_MM`] steps.
fn extreme(lo: f64, hi: f64, max: bool, g: impl Fn(f64) -> Option<f64>) -> Option<f64> {
    let mut best: Option<f64> = None;
    let mut h = lo;
    while h <= hi {
        let v = g(h)?;
        best = Some(best.map_or(v, |b| if max { b.max(v) } else { b.min(v) }));
        h += SEARCH_STEP_MM;
    }
    best
}

/// Maximum nappy size parameter `s` for [`crate::drape::nappy_local_changes`] (`buttocks-volume-incr` += `s`,
/// `hip-scale-depth-incr` += `0.5 * s`), corresponding to twice Anny's unit local-change scale (`s = 2.0`
/// adds ~226 mm to the 2.5 y toddler seat girth).
pub const MAX_NAPPY_SIZE: f64 = 2.0;

/// The seat girth on `posed` (mm): the fullest torso girth from 20 % of the way up from the crotch to
/// the waist, in [`SEARCH_STEP_MM`] steps (what [`tape_readings`] reads for `hip`, and what a nappy adds to).
#[must_use]
pub fn seat_girth(faces: &[[usize; 3]], posed: &PosedBody, frames: &BodyFrames) -> Option<f64> {
    let (waist_h, crotch_h) = (
        frames.landmarks.get("waist").copied()?,
        frames.landmarks.get("crotch").copied()?,
    );
    let mm: Vec<Vec3> = posed.vertices.iter().map(|v| *v * 1000.0).collect();
    let axis = [frames.torso.origin.x, frames.torso.origin.y];
    let torso = Slicer::new(&mm, faces, SLAB_MM);
    let g = |h: f64| torso.girth(frames.floor_z + h, axis);
    extreme(crotch_h + 0.2 * (waist_h - crotch_h), waist_h, true, g)
}

/// Calibrates the nappy size `s` (`0.0..=MAX_NAPPY_SIZE`) on `params` so the posed body's seat girth
/// ([`seat_girth`]) matches a taped `seat_over_nappy_mm` target (mm, within 0.01 mm), returning
/// `(nappy_params, s, achieved_seat_mm)`.
///
/// # Errors
/// [`RefusalCode::OutOfRange`] when `seat_over_nappy_mm` is non-positive, non-finite, below the bare body's
/// seat girth (`s = 0`), or above the seat girth reached at [`MAX_NAPPY_SIZE`];
/// [`RefusalCode::DegenerateGeometry`] when the posed body lacks a seat section; or refusals of the model.
pub fn calibrate_nappy(
    model: &AnnyModel,
    params: &BodyParams,
    pose: &Pose,
    seat_over_nappy_mm: f64,
) -> Result<(BodyParams, f64, f64), Refusal> {
    if !(seat_over_nappy_mm.is_finite() && seat_over_nappy_mm > 0.0) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("seat_over_nappy {seat_over_nappy_mm} mm must be finite and positive"),
        ));
    }
    let faces: Vec<[usize; 3]> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();
    let seat_at = |s: f64| -> Result<(BodyParams, f64), Refusal> {
        let p = crate::drape::nappy_local_changes(model, params, s)?;
        let rest = model.rest(&p)?;
        let posed = model.pose(&rest, &pose_deltas(model, &rest, pose, 1.0)?)?;
        let frames = BodyFrames::new(model, &posed)?;
        let girth = seat_girth(&faces, &posed, &frames).ok_or_else(|| {
            Refusal::new(
                RefusalCode::DegenerateGeometry,
                "the posed body lacks a torso section in the seat range",
            )
        })?;
        Ok((p, girth))
    };
    let (_, bare) = seat_at(0.0)?;
    let (_, fullest) = seat_at(MAX_NAPPY_SIZE)?;
    if !(bare..=fullest).contains(&seat_over_nappy_mm) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "seat_over_nappy {seat_over_nappy_mm:.1} mm is outside what a nappy reaches on this body \
                 ({bare:.1}–{fullest:.1} mm for s in 0..={MAX_NAPPY_SIZE})"
            ),
        )
        .repair("check that seat_over_nappy is at least the bare seat (hip) girth and within nappy range"));
    }
    let (mut lo, mut hi) = (0.0, MAX_NAPPY_SIZE);
    for _ in 0..50 {
        let mid = 0.5 * (lo + hi);
        if seat_at(mid)?.1 < seat_over_nappy_mm {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let s = 0.5 * (lo + hi);
    let (p, achieved) = seat_at(s)?;
    Ok((p, s, achieved))
}

/// What a tape reads on `posed` (mm, in [`TAPE_FIELDS`] order), or `None` when a section around an
/// axis is missing (a body far outside the model's range).
#[must_use]
pub fn tape_readings(
    faces: &[[usize; 3]],
    rest_height_m: f64,
    posed: &PosedBody,
    frames: &BodyFrames,
) -> Option<[f64; N_TAPE]> {
    let lm = |n: &str| frames.landmarks.get(n).copied();
    let mm: Vec<Vec3> = posed.vertices.iter().map(|v| *v * 1000.0).collect();
    // torso: horizontal sections around the torso axis
    let axis = [frames.torso.origin.x, frames.torso.origin.y];
    let torso = Slicer::new(&mm, faces, SLAB_MM);
    let g = |h: f64| torso.girth(frames.floor_z + h, axis);
    let (waist_h, crotch_h, armpit_h) = (lm("waist")?, lm("crotch")?, lm("chest")?);
    let hip = extreme(crotch_h + 0.2 * (waist_h - crotch_h), waist_h, true, g)?;
    // limbs: sections perpendicular to the leg and arm axes, around them
    let frame = |f: &crate::landmarks::Frame| in_frame(&mm, f.origin, f.lat, f.front, f.up);
    let (leg, arm) = (frame(&frames.leg_r), frame(&frames.arm_r));
    let (legs, arms) = (
        Slicer::new(&leg, faces, SLAB_MM),
        Slicer::new(&arm, faces, SLAB_MM),
    );
    // the part axes run parallel to the limbs, not always through them (near the ankle the leg
    // axis passes beside it): the nearest loop when none contains the axis
    let gl = |h: f64| legs.girth_near(h, [0.0, 0.0]);
    let ga = |h: f64| arms.girth_near(h, [0.0, 0.0]);
    let lh = |n: &str| frames.landmark_h("leg_R", n);
    let (crotch_l, knee_l, ankle_l) = (lh("crotch")?, lh("knee")?, lh("ankle")?);
    let shin = knee_l - ankle_l;
    // thigh: fullest just below the crotch; calf: fullest between 30 % and 90 % up the shin;
    // ankle: narrowest from 10 mm above the joint to a quarter up the shin
    let thigh = extreme(crotch_l - 40.0, crotch_l - 5.0, true, gl)?;
    let calf = extreme(ankle_l + 0.3 * shin, knee_l - 0.1 * shin, true, gl)?;
    let ankle = extreme(ankle_l + 10.0, ankle_l + 0.25 * shin, false, gl)?;
    let tip = frames.tip_h;
    // upper arm midway between the shoulder point and the elbow (0.55 of the arm down); the wrist
    let upper_arm = ga(tip - 0.275 * frames.arm_length)?;
    let wrist = ga(tip - frames.arm_length)?;
    // C7 down the spine to the waist, along the back's median line
    let c7_h = lm("c7")?;
    let mut back = 0.0;
    let mut prev: Option<(f64, f64)> = None;
    let mut h = waist_h;
    loop {
        let y = torso.back_point(frames.floor_z + h, axis[0])?;
        if let Some((ph, py)) = prev {
            back += ftx_core::det::hypot(h - ph, y - py);
        }
        prev = Some((h, y));
        if h >= c7_h {
            break;
        }
        h = (h + SEARCH_STEP_MM).min(c7_h);
    }
    let (neck_base, snp) = crate::tape::neck_base(&mm, faces, frames)?;
    let head = crate::tape::head_girth(&torso, frames)?;
    let (shoulder_length, across_back) =
        crate::tape::shoulder_and_back(&mm, faces, &torso, frames, snp)?;
    let total_crotch_length = crate::tape::crotch_length(&mm, faces, frames)?;
    let outseam = crate::tape::outseam(&torso, frames)?;
    Some([
        rest_height_m * 1000.0,
        g(armpit_h - CHEST_BELOW_ARMPIT_MM)?,
        g(waist_h)?,
        hip,
        waist_h,
        crotch_h,
        thigh,
        gl(knee_l)?,
        calf,
        ankle,
        upper_arm,
        wrist,
        frames.arm_length,
        lm("knee")?,
        back,
        neck_base,
        head,
        shoulder_length,
        across_back,
        total_crotch_length,
        outseam,
    ])
}

/// One measurement to fit: a name of [`TAPE_FIELDS`] and its value (mm).
#[derive(Clone, Debug, PartialEq)]
pub struct TapeTarget {
    /// The measurement.
    pub name: String,
    /// Its value (mm).
    pub value_mm: f64,
}

/// One measurement after the fit.
#[derive(Clone, Debug, PartialEq)]
pub struct TapeResidual {
    /// The measurement.
    pub name: String,
    /// What the wearer measures (mm).
    pub target_mm: f64,
    /// What the tape reads on the fitted body (mm).
    pub body_mm: f64,
}

/// A body fitted to tape measurements.
#[derive(Clone, Debug)]
pub struct TapeFit {
    /// The body parameters.
    pub params: BodyParams,
    /// Every tape reading on the fitted body (mm, [`TAPE_FIELDS`] order), measured or not.
    pub readings: [f64; N_TAPE],
    /// The fitted body's mass (kg; Anny's 980 kg/m³ × volume), fitted when the wearer's weight is known.
    pub mass_kg: f64,
    /// The fitted measurements, target against body.
    pub residuals: Vec<TapeResidual>,
    /// The free parameters' values ([`FREE`] order).
    pub free: [f64; N_FREE],
    /// Levenberg–Marquardt iterations taken.
    pub iterations: usize,
}

/// Fits the body of `wearer` (age and gender set; its stature fitted unless `targets` has one, its
/// weight when known) to `targets` on `pose`.
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] for a target that is not a [`TAPE_FIELDS`] name or not finite and
/// positive, or a model without the [`FREE`] local changes; refusals of [`fit_stature`] (the start) and of
/// the model; [`RefusalCode::DegenerateGeometry`] when the start body lacks a section a reading needs.
pub fn fit_tape(
    model: &AnnyModel,
    wearer: &Wearer,
    targets: &[TapeTarget],
    pose: &Pose,
) -> Result<TapeFit, Refusal> {
    let bad = |m: String| Refusal::new(RefusalCode::InvalidDocument, m);
    let mut rows: Vec<(usize, f64, f64)> = Vec::new(); // (field, value, σ)
    for t in targets {
        let k = TAPE_FIELDS
            .iter()
            .position(|f| *f == t.name)
            .ok_or_else(|| {
                bad(format!(
                    "no tape reading for {}: the fit reads {}",
                    t.name,
                    TAPE_FIELDS.join(", ")
                ))
            })?;
        if !(t.value_mm.is_finite() && t.value_mm > 0.0) {
            return Err(bad(format!(
                "{} = {} mm must be finite and positive",
                t.name, t.value_mm
            )));
        }
        rows.push((k, t.value_mm, fit_sigma(&t.name)));
    }
    // the wearer's stature is a target unless `targets` gives one
    if !rows.iter().any(|r| r.0 == 0) {
        rows.insert(0, (0, wearer.stature_mm, FIT_SIGMA_LENGTH_MM));
    }
    // the weight: a row on the mass, read in the extra slot after the tape readings
    if let Some(kg) = wearer.weight_kg {
        if !(kg.is_finite() && kg > 0.0) {
            return Err(bad(format!("weight {kg} kg must be finite and positive")));
        }
        rows.push((
            N_TAPE,
            kg,
            (WEIGHT_SIGMA_FRACTION * kg).max(WEIGHT_SIGMA_MIN_KG),
        ));
    }
    let lc: Vec<usize> = FREE[N_PHENOTYPE..]
        .iter()
        .map(|n| {
            model
                .local_change_index(n)
                .ok_or_else(|| bad(format!("the body model has no local change {n}")))
        })
        .collect::<Result<_, _>>()?;
    let stature = rows
        .iter()
        .find(|r| r.0 == 0)
        .map_or(wearer.stature_mm, |r| r.1);
    let (start, _) = fit_stature(
        model,
        &Wearer {
            stature_mm: stature,
            ..*wearer
        },
    )?;
    let faces: Vec<[usize; 3]> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();
    let base = start.phenotype;
    let params_of = |th: &[f64; N_FREE]| {
        let mut p = model.params(Phenotype {
            height: th[0],
            weight: th[1],
            muscle: th[2],
            proportions: th[3],
            ..base
        });
        for (j, &i) in lc.iter().enumerate() {
            p.local_changes[i] = th[N_PHENOTYPE + j];
        }
        p
    };
    // the tape readings, then the mass (kg)
    let read = |th: &[f64; N_FREE]| -> Result<Option<[f64; N_TAPE + 1]>, Refusal> {
        let rest = model.rest(&params_of(th))?;
        let anthro = model.anthropometry(&rest);
        let posed = model.pose(&rest, &pose_deltas(model, &rest, pose, 1.0)?)?;
        let frames = BodyFrames::new(model, &posed)?;
        Ok(tape_readings(&faces, anthro.height, &posed, &frames)
            .map(|t| std::array::from_fn(|k| if k < N_TAPE { t[k] } else { anthro.mass })))
    };
    let pheno = |j: usize| j < N_PHENOTYPE;
    let prior = |j: usize| if pheno(j) { 0.5 } else { 0.0 };
    let tau = |j: usize| {
        if pheno(j) {
            PRIOR_SIGMA_PHENOTYPE
        } else {
            PRIOR_SIGMA_LOCAL
        }
    };
    let clamp = |th: [f64; N_FREE]| -> [f64; N_FREE] {
        std::array::from_fn(|j| {
            if pheno(j) {
                th[j].clamp(0.0, 1.0)
            } else {
                th[j].clamp(-LOCAL_LIMIT, LOCAL_LIMIT)
            }
        })
    };
    // residual vector: the measurements, then the priors
    let resid = |th: &[f64; N_FREE], r: &[f64; N_TAPE + 1]| -> Vec<f64> {
        rows.iter()
            .map(|&(k, v, s)| (r[k] - v) / s)
            .chain((0..N_FREE).map(|j| (th[j] - prior(j)) / tau(j)))
            .collect()
    };
    let cost = |r: &[f64]| r.iter().map(|x| x * x).sum::<f64>();
    let mut th: [f64; N_FREE] =
        std::array::from_fn(|j| if j == 0 { base.height } else { prior(j) });
    let mut readings = read(&th)?.ok_or_else(|| {
        Refusal::new(
            RefusalCode::DegenerateGeometry,
            "the start body lacks a section one of the tape readings needs (a body far outside the \
             model's range?)",
        )
    })?;
    let mut r = resid(&th, &readings);
    let mut c = cost(&r);
    let mut lambda: f64 = 1e-3;
    let mut iterations = 0;
    for _ in 0..MAX_ITERATIONS {
        iterations += 1;
        // Jacobian of the measurement rows by central differences (each reading is continuous in the
        // parameters, kinked where a landmark's extreme vertex changes)
        let mut jac = vec![[0.0; N_FREE]; rows.len()];
        for j in 0..N_FREE {
            let (mut up, mut dn) = (th, th);
            up[j] += FD_STEP;
            dn[j] -= FD_STEP;
            let (up, dn) = (clamp(up), clamp(dn));
            // a probe whose body lacks a section falls back to the one-sided difference from the current
            // point; only when both fail does the column stay zero (the parameter rests this iteration)
            let (hi, lo, xh, xl) = match (read(&up)?, read(&dn)?) {
                (Some(ru), Some(rd)) => (ru, rd, up[j], dn[j]),
                (Some(ru), None) => (ru, readings, up[j], th[j]),
                (None, Some(rd)) => (readings, rd, th[j], dn[j]),
                (None, None) => continue,
            };
            if xh == xl {
                continue;
            }
            for (i, &(k, _, s)) in rows.iter().enumerate() {
                jac[i][j] = (hi[k] - lo[k]) / ((xh - xl) * s);
            }
        }
        let mut improved = false;
        while lambda < 1e10 {
            // [J; I/τ; √λ I] Δ = [−r_meas; −r_prior; 0]
            let mut a: Vec<[f64; N_FREE]> = jac.clone();
            a.extend((0..N_FREE).map(|j| {
                let mut row = [0.0; N_FREE];
                row[j] = 1.0 / tau(j);
                row
            }));
            a.extend((0..N_FREE).map(|j| {
                let mut row = [0.0; N_FREE];
                row[j] = lambda.sqrt();
                row
            }));
            let mut b: Vec<f64> = r.iter().map(|x| -x).collect();
            b.extend(std::iter::repeat_n(0.0, N_FREE));
            let step = lstsq(&a, &b);
            let trial = clamp(std::array::from_fn(|j| th[j] + step[j]));
            if let Some(rt) = read(&trial)? {
                let res = resid(&trial, &rt);
                let ct = cost(&res);
                if ct < c {
                    let moved = (0..N_FREE)
                        .map(|j| (trial[j] - th[j]).abs())
                        .fold(0.0, f64::max);
                    let gain = c - ct;
                    (th, readings, r, c) = (trial, rt, res, ct);
                    lambda = (lambda / 3.0).max(1e-9);
                    improved = moved > 1e-7 && gain > REL_GAIN_STOP * (c + gain);
                    break;
                }
            }
            lambda *= 4.0;
        }
        if !improved {
            break;
        }
    }
    let residuals = rows
        .iter()
        .filter(|&&(k, _, _)| k < N_TAPE)
        .map(|&(k, v, _)| TapeResidual {
            name: TAPE_FIELDS[k].to_string(),
            target_mm: v,
            body_mm: readings[k],
        })
        .collect();
    Ok(TapeFit {
        params: params_of(&th),
        readings: std::array::from_fn(|k| readings[k]),
        mass_kg: readings[N_TAPE],
        residuals,
        free: th,
        iterations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sex_codes() {
        assert_eq!(Wearer::gender_of(Some("boy")), 0.0);
        assert_eq!(Wearer::gender_of(Some("M")), 0.0);
        assert_eq!(Wearer::gender_of(Some("girl")), 1.0);
        assert_eq!(Wearer::gender_of(Some("f")), 1.0);
        assert_eq!(Wearer::gender_of(Some("x")), 0.5);
        assert_eq!(Wearer::gender_of(None), 0.5);
    }

    #[test]
    fn wearer_from_a_measurement_file() {
        let m =
            serde_json::json!({"wearer": {"age_months": 30.0, "stature_mm": 920.0, "sex": "girl"}});
        let w = Wearer::from_measurements(&m).unwrap();
        assert_eq!((w.years, w.gender, w.stature_mm), (2.5, 1.0, 920.0));
        assert_eq!(w.weight_kg, None);
        let fused = serde_json::json!({"wearer": {"age_months": 24.0}, "fused": {"stature": {"value_mm": 870.0}}});
        assert_eq!(Wearer::from_measurements(&fused).unwrap().stature_mm, 870.0);
        assert!(Wearer::from_measurements(&serde_json::json!({"wearer": {}})).is_err());
        // weight, else BMI × stature²; a weight wins over a BMI; nonsense is refused
        let with = |extra: serde_json::Value| {
            let mut m = serde_json::json!({"wearer": {"age_months": 30.0, "stature_mm": 1000.0}});
            m["wearer"]
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            Wearer::from_measurements(&m)
        };
        let kg = |extra| with(extra).unwrap().weight_kg;
        assert_eq!(kg(serde_json::json!({"weight_kg": 13.5})), Some(13.5));
        assert_eq!(kg(serde_json::json!({"bmi": 15.5})), Some(15.5));
        assert_eq!(
            kg(serde_json::json!({"weight_kg": 13.5, "bmi": 99.0})),
            Some(13.5)
        );
        assert!(with(serde_json::json!({"weight_kg": -2.0})).is_err());
        assert!(with(serde_json::json!({"bmi": "high"})).is_err());
    }

    #[test]
    fn targets_from_a_measurement_file() {
        let m = serde_json::json!({
            "wearer": {"age_months": 30.0, "stature_mm": 921.0},
            "fused": {"stature": {"value_mm": 920.0}, "waist": {"value_mm": 530.0},
                      "thigh": {"value_mm": 290.0}, "hip": {"value_deg": 3.0},
                      "head": {"value_mm": 480.0}, "belly_depth": {"value_mm": 140.0}},
        });
        let t = tape_targets(&m);
        let got: Vec<(&str, f64)> = t.iter().map(|t| (t.name.as_str(), t.value_mm)).collect();
        // in TAPE_FIELDS order; no value_mm (hip) or not a tape reading (belly_depth): left out
        assert_eq!(
            got,
            [
                ("stature", 921.0),
                ("waist", 530.0),
                ("thigh", 290.0),
                ("head", 480.0)
            ]
        );
    }

    #[test]
    fn seat_girth_reads_the_fullest_section_in_the_seat_band() {
        // A vertical square prism from z = 0 to z = 0.2 m with side 0.10 m (perimeter 400 mm),
        // widened at z = 0.10 m to side 0.12 m (perimeter 480 mm).
        let ring = |z: f64, half: f64| {
            [
                Vec3::new(-half, -half, z),
                Vec3::new(half, -half, z),
                Vec3::new(half, half, z),
                Vec3::new(-half, half, z),
            ]
        };
        let mut vertices = Vec::new();
        vertices.extend(ring(0.0, 0.05));
        vertices.extend(ring(0.10, 0.06));
        vertices.extend(ring(0.20, 0.05));
        let mut faces = Vec::new();
        for base in [0usize, 4] {
            for k in 0..4 {
                let n = (k + 1) % 4;
                faces.push([base + k, base + n, base + 4 + n]);
                faces.push([base + k, base + 4 + n, base + 4 + k]);
            }
        }
        let posed = PosedBody {
            vertices,
            bone_poses: Vec::new(),
            transforms: Vec::new(),
        };
        let dummy = crate::landmarks::Frame {
            origin: Vec3::ZERO,
            lat: Vec3::new(1.0, 0.0, 0.0),
            front: Vec3::new(0.0, -1.0, 0.0),
            up: Vec3::new(0.0, 0.0, 1.0),
        };
        let mut landmarks = std::collections::BTreeMap::new();
        landmarks.insert("crotch".to_string(), 20.0);
        landmarks.insert("waist".to_string(), 180.0);
        let frames = BodyFrames {
            floor_z: 0.0,
            landmarks,
            torso: dummy,
            arm_r: dummy,
            leg_r: dummy,
            tip_h: 0.0,
            upper_arm_radius: 0.0,
            arm_length: 0.0,
            neck: dummy,
            head_bone: Vec3::ZERO,
            shoulder_joint_r: Vec3::ZERO,
            torso_points: Vec::new(),
            leg_points: Vec::new(),
        };
        let girth = seat_girth(&faces, &posed, &frames).expect("seat girth");
        assert!((girth - 480.0).abs() < 1e-6, "got {girth}");
    }
}
