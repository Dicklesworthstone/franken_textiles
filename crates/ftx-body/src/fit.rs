//! Fitting Anny to what is known of the wearer (plan §9.4).
//!
//! - [`fit_stature`] ("from age/sex/height"): the age and gender parameters are set directly, the height
//!   parameter is bisected until the rest body's height equals the stature, and the other phenotype
//!   parameters stay at the population middle (0.5).
//! - [`fit_tape`] ("from tape measurements"): Levenberg–Marquardt over the height and weight parameters and
//!   the `measure-*` local changes that move one measurement each ([`FREE`]), minimising the residuals
//!   between the wearer's measurements and what a tape reads on the posed body ([`partial_tape_readings`]), with
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
/// dress form does), and its reported uncertainty. The stature from `wearer.stature_mm` takes precedence
/// when given; it is separate evidence and never inherits a fused stature's uncertainty.
/// Missing uncertainty retains the fitter's documented per-functional tolerance.
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] if a selected fused target declares a `sigma_mm` that is not finite
/// and positive. An absent sigma is supported; an explicit null or malformed sigma is not absence.
pub fn tape_targets(meas: &Value) -> Result<Vec<TapeTarget>, Refusal> {
    TAPE_FIELDS
        .iter()
        .filter_map(|&name| {
            let fused = meas["fused"][name]["value_mm"].as_f64();
            let override_value = if name == "stature" {
                meas["wearer"]["stature_mm"].as_f64()
            } else {
                None
            };
            let value = override_value.or(fused)?;
            Some((|| {
                let sigma_mm = if override_value.is_some() {
                    None
                } else {
                    meas["fused"][name]
                        .get("sigma_mm")
                        .map(|v| {
                            v.as_f64()
                                .filter(|s| s.is_finite() && *s > 0.0)
                                .ok_or_else(|| {
                                    Refusal::new(
                                        RefusalCode::InvalidDocument,
                                        format!(
                                            "fused {name}.sigma_mm must be finite and positive"
                                        ),
                                    )
                                })
                        })
                        .transpose()?
                };
                Ok(TapeTarget {
                    name: name.to_string(),
                    value_mm: value,
                    sigma_mm,
                })
            })())
        })
        .collect()
}

/// The body parameters for `wearer` and the stature reached (mm, within 0.01 mm).
///
/// # Errors
/// [`RefusalCode::OutOfRange`] for a non-positive or non-finite age or stature, or a stature Anny cannot
/// reach at that age with the other parameters at 0.5; refusals of the model.
pub fn fit_stature(model: &AnnyModel, wearer: &Wearer) -> Result<(BodyParams, f64), Refusal> {
    stature_start(model, wearer, false)
}

// The full tape fit can change body proportions and local leg/torso lengths. The range of the
// height parameter with all those controls at their priors is only an initialization range, not
// the full model's reachable stature. Endpoint seeds never change the target sent to the solve.
fn stature_start(
    model: &AnnyModel,
    wearer: &Wearer,
    allow_endpoint_seed: bool,
) -> Result<(BodyParams, f64), Refusal> {
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
    if !shortest.is_finite() || !tallest.is_finite() || shortest <= 0.0 || tallest < shortest {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "the body model's stature initialization range must be finite, positive and ordered",
        ));
    }
    if !(shortest..=tallest).contains(&stature_mm) {
        if allow_endpoint_seed {
            let h = if stature_mm < shortest { 0.0 } else { 1.0 };
            return Ok((model.params(Phenotype { height: h, ..base }), height_at(h)?));
        }
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

/// The fit's residual-scale floor (mm) for a girth, from the accuracy gate G1b.
pub const FIT_SIGMA_GIRTH_MM: f64 = 3.0;
/// See [`FIT_SIGMA_GIRTH_MM`].
pub const FIT_SIGMA_LENGTH_MM: f64 = 2.0;

/// The fit's residual-scale floor for a [`TAPE_FIELDS`] measurement (mm). Four lengths whose reading
/// on a body rests on an approximation of where a measurer puts the tape (the shoulder point taken
/// above the shoulder joint, a tape
/// hanging from the hip, a snug tape through the crotch as a hull) pull more gently: 5 mm, and 8 mm for the
/// outseam (the catalogue's tape σ for them is 10–15 mm). Pulled to 2 mm they distort the body where the
/// definition, not the body, is uncertain (the 92 cm chart's outseam and shoulder length cannot both be met
/// with its crotch height). A target's larger declared measurement sigma supersedes this floor. This
/// numerical fitting policy is not a calibrated model-error distribution or a posterior confidence.
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

/// Separately calibrated occupied geometry. The original body parameters and evidence are unchanged.
#[derive(Clone, Debug)]
pub struct NappyOccupancy {
    /// Derived shape to reuse for collision/display and every requested pose, never body refitting.
    pub params: BodyParams,
    /// Computed occupied seat in the standard straight-arm 35-degree pose, in mm.
    pub seat_over_nappy_mm: f64,
    /// Source-bearing calibration, explicit local additions and occupied reference geometry identity.
    pub description: Value,
}

/// Apply an explicitly requested nappy occupancy to an otherwise unchanged fitted/accepted body.
/// The caller validates and fuses source records. A chart target remains a chart target; the result
/// is a geometric MODEL_FIT proxy and supplies no nappy mass, material law or measurement readiness.
/// Calibration always uses [`Pose::drape`], independently of later export and motion poses.
///
/// # Errors
/// Invalid wearer selection, missing/invalid fused target or sigma, an unreachable target, missing
/// model local changes or seat geometry, or calibration that cannot match within 0.01 mm.
pub fn nappy_occupancy(
    model: &AnnyModel,
    bare_params: &BodyParams,
    measurements: &Value,
) -> Result<Option<NappyOccupancy>, Refusal> {
    let bad = |message: &str| Refusal::new(RefusalCode::InvalidDocument, message);
    match measurements["wearer"].get("nappies") {
        None | Some(Value::Bool(false)) => return Ok(None),
        Some(Value::Bool(true)) => {}
        _ => return Err(bad("wearer.nappies must be a boolean")),
    }
    let source = &measurements["fused"]["seat_over_nappy"];
    let target = source["value_mm"]
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| bad("wearer.nappies requires a positive fused seat_over_nappy.value_mm target; no default bulk is inferred"))?;
    if source.get("unit").is_some_and(|unit| unit != "mm") {
        return Err(bad("seat_over_nappy must use millimetres"));
    }
    let sigma = source
        .get("sigma_mm")
        .map(|value| {
            value
                .as_f64()
                .filter(|v| v.is_finite() && *v > 0.0)
                .ok_or_else(|| bad("seat_over_nappy.sigma_mm must be finite and positive"))
        })
        .transpose()?;
    let pose = Pose::drape();
    let faces: Vec<_> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();
    let posed_body = |params: &BodyParams| {
        let rest = model.rest(params)?;
        let posed = model.pose(&rest, &pose_deltas(model, &rest, &pose, 1.0)?)?;
        let frames = BodyFrames::new(model, &posed)?;
        let seat = seat_girth(&faces, &posed, &frames)
            .filter(|g| g.is_finite() && *g > 0.0)
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    "nappy occupancy requires a valid seat section",
                )
            })?;
        Ok::<_, Refusal>((posed, seat))
    };
    let (_, bare_seat) = posed_body(bare_params)?;
    let (params, size, _) = calibrate_nappy(model, bare_params, &pose, target)?;
    let (posed, achieved) = posed_body(&params)?;
    if (achieved - target).abs() > 0.01 {
        return Err(Refusal::new(
            RefusalCode::NotConverged,
            "nappy occupancy could not match the supplied seat within 0.01 mm; no bare-body fallback",
        ));
    }
    let geometry = serde_json::to_vec(&(&posed.vertices, &faces))
        .map_err(|_| bad("cannot identify the occupied reference geometry"))?;
    let mut description = serde_json::json!({
        "model":"anny_seat_proxy/1","measurement_pose":"standard_straight_arm_drape_35_degrees",
        "target_mm":target,"target_source":source,"bare_seat_mm":bare_seat,
        "achieved_seat_mm":achieved,"residual_mm":achieved-target,"size_parameter":size,
        "local_change_additions":{"buttocks-volume-incr":size,"hip-scale-depth-incr":size/2.0},
        "occupied_reference_geometry_sha256":ftx_core::receipt::sha256_hex(&geometry),
        "provenance":"MODEL_FIT","physical_validation_claim":false,
        "no_claim":"Geometric occupancy proxy only. Bare body parameters, mass and fit evidence are unchanged. No nappy material, friction, thermal or vapor law, measured readiness or physical fit is established."
    });
    if let Some(sigma) = sigma {
        description["sigma_mm"] = serde_json::json!(sigma);
    }
    Ok(Some(NappyOccupancy {
        params,
        seat_over_nappy_mm: achieved,
        description,
    }))
}

/// What a tape reads on `posed` (mm, in [`TAPE_FIELDS`] order), or `None` when any
/// section is missing. Use [`partial_tape_readings`] to retain independently available sections.
#[must_use]
pub fn tape_readings(
    faces: &[[usize; 3]],
    rest_height_m: f64,
    posed: &PosedBody,
    frames: &BodyFrames,
) -> Option<[f64; N_TAPE]> {
    let partial = partial_tape_readings(faces, rest_height_m, posed, frames);
    let mut values = [0.0; N_TAPE];
    for (i, value) in partial.into_iter().enumerate() {
        values[i] = value?;
    }
    Some(values)
}

/// Independently available tape readings (mm, in [`TAPE_FIELDS`] order).
///
/// A missing section or landmark withdraws only the readings that depend on it. Nonfinite and
/// nonpositive values are unavailable, never zero or an inferred replacement. These are model
/// predictions; partial geometry does not establish measurement readiness or anatomical accuracy.
#[must_use]
pub fn partial_tape_readings(
    faces: &[[usize; 3]],
    rest_height_m: f64,
    posed: &PosedBody,
    frames: &BodyFrames,
) -> [Option<f64>; N_TAPE] {
    let lm = |n: &str| frames.landmarks.get(n).copied().filter(|v| v.is_finite());
    let mm: Vec<Vec3> = posed.vertices.iter().map(|v| *v * 1000.0).collect();
    // Torso sections retain their shared axis and the original tape arithmetic.
    let axis = [frames.torso.origin.x, frames.torso.origin.y];
    let torso = Slicer::new(&mm, faces, SLAB_MM);
    let g = |h: f64| torso.girth(frames.floor_z + h, axis);
    let hip = (|| {
        let (waist_h, crotch_h) = (lm("waist")?, lm("crotch")?);
        extreme(crotch_h + 0.2 * (waist_h - crotch_h), waist_h, true, g)
    })();
    let frame = |f: &crate::landmarks::Frame| in_frame(&mm, f.origin, f.lat, f.front, f.up);
    let (leg, arm) = (frame(&frames.leg_r), frame(&frames.arm_r));
    let (legs, arms) = (
        Slicer::new(&leg, faces, SLAB_MM),
        Slicer::new(&arm, faces, SLAB_MM),
    );
    // Limb axes can pass beside the surface; preserve the qualified nearest-loop selection.
    let gl = |h: f64| legs.girth_near(h, [0.0, 0.0]);
    let ga = |h: f64| arms.girth_near(h, [0.0, 0.0]);
    let lh = |n: &str| frames.landmark_h("leg_R", n).filter(|v| v.is_finite());
    let thigh = lh("crotch").and_then(|h| extreme(h - 40.0, h - 5.0, true, gl));
    let calf = (|| {
        let (knee, ankle) = (lh("knee")?, lh("ankle")?);
        let shin = knee - ankle;
        extreme(ankle + 0.3 * shin, knee - 0.1 * shin, true, gl)
    })();
    let ankle = (|| {
        let (knee, ankle) = (lh("knee")?, lh("ankle")?);
        extreme(ankle + 10.0, ankle + 0.25 * (knee - ankle), false, gl)
    })();
    let tip = frames.tip_h;
    let upper_arm = ga(tip - 0.275 * frames.arm_length);
    let wrist = ga(tip - frames.arm_length);
    // A missing spine section does not invalidate unrelated circumference readings.
    let back = (|| {
        let (waist_h, c7_h) = (lm("waist")?, lm("c7")?);
        let mut back = 0.0;
        let mut prev: Option<(f64, f64)> = None;
        let mut h = waist_h;
        loop {
            let y = torso.back_point(frames.floor_z + h, axis)?;
            if let Some((ph, py)) = prev {
                back += ftx_core::det::hypot(h - ph, y - py);
            }
            prev = Some((h, y));
            if h >= c7_h {
                break;
            }
            h = (h + SEARCH_STEP_MM).min(c7_h);
        }
        Some(back)
    })();
    let neck = crate::tape::neck_base(&mm, faces, frames);
    let shoulders =
        neck.and_then(|(_, snp)| crate::tape::shoulder_and_back(&mm, faces, &torso, frames, snp));
    [
        Some(rest_height_m * 1000.0),
        lm("chest").and_then(|h| g(h - CHEST_BELOW_ARMPIT_MM)),
        lm("waist").and_then(g),
        hip,
        lm("waist"),
        lm("crotch"),
        thigh,
        lh("knee").and_then(gl),
        calf,
        ankle,
        upper_arm,
        wrist,
        Some(frames.arm_length),
        lm("knee"),
        back,
        neck.map(|(girth, _)| girth),
        crate::tape::head_girth(&torso, frames),
        shoulders.map(|(length, _)| length),
        shoulders.map(|(_, back)| back),
        crate::tape::crotch_length(&mm, faces, frames),
        crate::tape::outseam(&torso, frames),
    ]
    .map(|value| value.filter(|v| v.is_finite() && *v > 0.0))
}

/// One measurement to fit: a name of [`TAPE_FIELDS`], value and optional source uncertainty (mm).
#[derive(Clone, Debug, PartialEq)]
pub struct TapeTarget {
    /// The measurement.
    pub name: String,
    /// Its value (mm).
    pub value_mm: f64,
    /// Reported measurement 1σ (mm), finite and positive when present. None uses [`fit_sigma`].
    pub sigma_mm: Option<f64>,
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
    /// The target's reported measurement 1σ, if supplied (mm).
    pub sigma_mm: Option<f64>,
    /// Actual residual scale: the larger of reported sigma and [`fit_sigma`] (mm).
    pub effective_sigma_mm: f64,
}

#[derive(Clone, Copy, Debug)]
struct TapeRow {
    field: usize,
    target: f64,
    sigma: f64,
    source_sigma: Option<f64>,
}

fn tape_rows(wearer: &Wearer, targets: &[TapeTarget]) -> Result<Vec<TapeRow>, Refusal> {
    let bad = |m: String| Refusal::new(RefusalCode::InvalidDocument, m);
    let mut rows = Vec::new();
    for t in targets {
        let field = TAPE_FIELDS
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
        if let Some(sigma) = t.sigma_mm
            && !(sigma.is_finite() && sigma > 0.0)
        {
            return Err(bad(format!(
                "{}.sigma_mm = {sigma} must be finite and positive",
                t.name
            )));
        }
        rows.push(TapeRow {
            field,
            target: t.value_mm,
            sigma: t.sigma_mm.unwrap_or(0.0).max(fit_sigma(&t.name)),
            source_sigma: t.sigma_mm,
        });
    }
    // The wearer's independent stature has no declared uncertainty in this file contract.
    if !rows.iter().any(|r| r.field == 0) {
        rows.insert(
            0,
            TapeRow {
                field: 0,
                target: wearer.stature_mm,
                sigma: FIT_SIGMA_LENGTH_MM,
                source_sigma: None,
            },
        );
    }
    // Weight retains its separate kg-scale policy, never a tape target's mm uncertainty.
    if let Some(kg) = wearer.weight_kg {
        if !(kg.is_finite() && kg > 0.0) {
            return Err(bad(format!("weight {kg} kg must be finite and positive")));
        }
        rows.push(TapeRow {
            field: N_TAPE,
            target: kg,
            sigma: (WEIGHT_SIGMA_FRACTION * kg).max(WEIGHT_SIGMA_MIN_KG),
            source_sigma: None,
        });
    }
    Ok(rows)
}

/// A body fitted to tape measurements.
#[derive(Clone, Debug)]
pub struct TapeFit {
    /// The body parameters.
    pub params: BodyParams,
    /// Independently available tape readings (mm, [`TAPE_FIELDS`] order), measured or not.
    /// Requested readings are always present; missing unrequested sections remain `None`.
    pub readings: [Option<f64>; N_TAPE],
    /// The fitted body's mass (kg; Anny's 980 kg/m³ × volume), fitted when the wearer's weight is known.
    pub mass_kg: f64,
    /// The fitted measurements, target against body.
    pub residuals: Vec<TapeResidual>,
    /// The free parameters' values ([`FREE`] order).
    pub free: [f64; N_FREE],
    /// Levenberg–Marquardt iterations taken.
    pub iterations: usize,
}

impl TapeFit {
    /// Unavailable diagnostics in [`TAPE_FIELDS`] order. None of these were requested fit targets.
    #[must_use]
    pub fn unavailable_fields(&self) -> Vec<&'static str> {
        TAPE_FIELDS
            .iter()
            .zip(self.readings)
            .filter_map(|(name, reading)| reading.is_none().then_some(*name))
            .collect()
    }
}

/// Fits the body of `wearer` (age and gender set; its stature fitted unless `targets` has one, its
/// weight when known) to `targets` on `pose`. Each residual is scaled by the larger of the target's
/// reported sigma and the explicit per-functional fitting floor. Missing sigma preserves the floor.
/// A stature outside the height-only initialization range starts from its nearest endpoint; the
/// full bounded fit can still use proportions and local lengths. Original targets and residuals
/// remain unchanged, including any discrepancy the complete model cannot resolve.
/// Only requested readings constrain admission; unavailable unrequested diagnostics remain absent.
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] for a target that is not a [`TAPE_FIELDS`] name or not finite and
/// positive, a supplied sigma that is not finite and positive, or a model without the [`FREE`] local
/// changes; invalid age/stature inputs and refusals of the model;
/// [`RefusalCode::DegenerateGeometry`] when the start body lacks a requested reading.
pub fn fit_tape(
    model: &AnnyModel,
    wearer: &Wearer,
    targets: &[TapeTarget],
    pose: &Pose,
) -> Result<TapeFit, Refusal> {
    let bad = |m: String| Refusal::new(RefusalCode::InvalidDocument, m);
    let rows = tape_rows(wearer, targets)?;
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
        .find(|r| r.field == 0)
        .map_or(wearer.stature_mm, |r| r.target);
    let (start, _) = stature_start(
        model,
        &Wearer {
            stature_mm: stature,
            ..*wearer
        },
        true,
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
    let needs_posed_tapes = rows.iter().any(|row| row.field > 0 && row.field < N_TAPE);
    // Height and mass do not use posed sections. Keep model/pose/frame admission on every
    // evaluation, and defer only the unused tape functionals until the final diagnostics.
    let read =
        |th: &[f64; N_FREE], include_tapes: bool| -> Result<[Option<f64>; N_TAPE + 1], Refusal> {
            let rest = model.rest(&params_of(th))?;
            let anthro = model.anthropometry(&rest);
            let posed = model.pose(&rest, &pose_deltas(model, &rest, pose, 1.0)?)?;
            let frames = BodyFrames::new(model, &posed)?;
            let t = if include_tapes {
                partial_tape_readings(&faces, anthro.height, &posed, &frames)
            } else {
                let mut t = [None; N_TAPE];
                t[0] = Some(anthro.height * 1000.0);
                t
            };
            Ok(std::array::from_fn(|k| {
                if k < N_TAPE { t[k] } else { Some(anthro.mass) }
            }))
        };
    let TapeSolution {
        free,
        mut readings,
        iterations,
    } = optimize_tape(&rows, base.height, |th| read(th, needs_posed_tapes))?;
    if !needs_posed_tapes {
        readings = read(&free, true)?;
    }
    let residuals = rows
        .iter()
        .filter(|r| r.field < N_TAPE)
        .map(|r| TapeResidual {
            name: TAPE_FIELDS[r.field].to_string(),
            target_mm: r.target,
            body_mm: readings[r.field].expect("requested readings are admitted"),
            sigma_mm: r.source_sigma,
            effective_sigma_mm: r.sigma,
        })
        .collect();
    Ok(TapeFit {
        params: params_of(&free),
        readings: std::array::from_fn(|k| readings[k]),
        mass_kg: readings[N_TAPE].expect("mass is always evaluated"),
        residuals,
        free,
        iterations,
    })
}

struct TapeSolution {
    free: [f64; N_FREE],
    readings: [Option<f64>; N_TAPE + 1],
    iterations: usize,
}

// Solve the damped tape-fit step inside the actual parameter box. Clipping an unconstrained
// least-squares answer discards the motion assigned to saturated controls without letting the
// remaining controls compensate. A small active set preserves the ordinary solve exactly when
// it is interior; hit bounds can be released when their reduced gradient points back inward.
fn bounded_tape_step(a: &[[f64; N_FREE]], b: &[f64], th: &[f64; N_FREE]) -> Option<[f64; N_FREE]> {
    let lower: [f64; N_FREE] = std::array::from_fn(|j| {
        if j < N_PHENOTYPE {
            -th[j]
        } else {
            -LOCAL_LIMIT - th[j]
        }
    });
    let upper: [f64; N_FREE] = std::array::from_fn(|j| {
        if j < N_PHENOTYPE {
            1.0 - th[j]
        } else {
            LOCAL_LIMIT - th[j]
        }
    });
    let mut step = [0.0; N_FREE];
    // -1: lower bound, 0: free, +1: upper bound.
    let mut active = [0_i8; N_FREE];
    for _ in 0..4 * N_FREE {
        let mut free_a = a.to_vec();
        let mut free_b = b.to_vec();
        for (i, row) in free_a.iter_mut().enumerate() {
            for j in 0..N_FREE {
                if active[j] != 0 {
                    free_b[i] -= row[j] * step[j];
                    row[j] = 0.0;
                }
            }
        }
        if free_b.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let mut trial = lstsq(&free_a, &free_b);
        for j in 0..N_FREE {
            if active[j] != 0 {
                trial[j] = step[j];
            }
        }
        if trial.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let mut first = None;
        let mut alpha = 1.0;
        for j in 0..N_FREE {
            if active[j] != 0 {
                continue;
            }
            let bound = if trial[j] < lower[j] {
                Some((lower[j], -1))
            } else if trial[j] > upper[j] {
                Some((upper[j], 1))
            } else {
                None
            };
            if let Some((value, side)) = bound {
                let fraction = (value - step[j]) / (trial[j] - step[j]);
                if fraction < alpha || (first.is_none() && fraction == alpha) {
                    alpha = fraction;
                    first = Some((j, value, side));
                }
            }
        }
        if let Some((j, value, side)) = first {
            for k in 0..N_FREE {
                step[k] = (step[k] + alpha * (trial[k] - step[k])).clamp(lower[k], upper[k]);
            }
            step[j] = value;
            active[j] = side;
            continue;
        }
        step = trial;
        let residuals: Vec<f64> = a
            .iter()
            .zip(b)
            .map(|(row, rhs)| row.iter().zip(step).map(|(a, x)| a * x).sum::<f64>() - rhs)
            .collect();
        let mut release = None;
        let mut largest = 0.0;
        for j in 0..N_FREE {
            if active[j] == 0 {
                continue;
            }
            let mut gradient = 0.0;
            let mut scale = 0.0;
            for (row, residual) in a.iter().zip(&residuals) {
                let contribution = row[j] * residual;
                gradient += contribution;
                scale += contribution.abs();
            }
            if !gradient.is_finite() || !scale.is_finite() {
                return None;
            }
            let violation = f64::from(active[j]) * gradient;
            if violation > 64.0 * f64::EPSILON * scale.max(1.0) && violation > largest {
                largest = violation;
                release = Some(j);
            }
        }
        if let Some(j) = release {
            active[j] = 0;
        } else {
            return Some(step);
        }
    }
    // Retry with stronger LM damping instead of publishing an unfinished constrained step.
    None
}

// The same bounded LM solve serves actual model readings and independent analytic controls. Keep
// target scales in both residual and Jacobian rows; scaling only one changes the fitted optimum.
fn optimize_tape(
    rows: &[TapeRow],
    initial_height: f64,
    read: impl Fn(&[f64; N_FREE]) -> Result<[Option<f64>; N_TAPE + 1], Refusal>,
) -> Result<TapeSolution, Refusal> {
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
    let available = |reading: Option<f64>| reading.is_some_and(|v| v.is_finite() && v > 0.0);
    let admitted = |r: &[Option<f64>; N_TAPE + 1]| rows.iter().all(|row| available(r[row.field]));
    let resid = |th: &[f64; N_FREE], r: &[Option<f64>; N_TAPE + 1]| -> Vec<f64> {
        rows.iter()
            .map(|row| {
                (r[row.field].expect("requested readings are admitted") - row.target) / row.sigma
            })
            .chain((0..N_FREE).map(|j| (th[j] - prior(j)) / tau(j)))
            .collect()
    };
    let cost = |r: &[f64]| r.iter().map(|x| x * x).sum::<f64>();
    let mut th: [f64; N_FREE] =
        std::array::from_fn(|j| if j == 0 { initial_height } else { prior(j) });
    let mut readings = read(&th)?;
    if !admitted(&readings) {
        let missing: Vec<_> = rows
            .iter()
            .filter(|row| !available(readings[row.field]))
            .map(|row| {
                if row.field < N_TAPE {
                    TAPE_FIELDS[row.field]
                } else {
                    "weight"
                }
            })
            .collect();
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            format!(
                "the start body cannot evaluate requested measurements: {} (a required section is \
                 missing or invalid; a body far outside the model's range?)",
                missing.join(", ")
            ),
        ));
    }
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
            // A probe missing a requested section falls back to a one-sided difference. Optional
            // diagnostics never invalidate a probe; both failing leaves this column at zero.
            let (ru, rd) = (read(&up)?, read(&dn)?);
            let (hi, lo, xh, xl) = match (admitted(&ru).then_some(ru), admitted(&rd).then_some(rd))
            {
                (Some(ru), Some(rd)) => (ru, rd, up[j], dn[j]),
                (Some(ru), None) => (ru, readings, up[j], th[j]),
                (None, Some(rd)) => (readings, rd, th[j], dn[j]),
                (None, None) => continue,
            };
            if xh == xl {
                continue;
            }
            for (i, row) in rows.iter().enumerate() {
                jac[i][j] = (hi[row.field].expect("requested readings are admitted")
                    - lo[row.field].expect("requested readings are admitted"))
                    / ((xh - xl) * row.sigma);
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
            let Some(step) = bounded_tape_step(&a, &b, &th) else {
                lambda *= 4.0;
                continue;
            };
            let trial = clamp(std::array::from_fn(|j| th[j] + step[j]));
            let rt = read(&trial)?;
            if admitted(&rt) {
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
    Ok(TapeSolution {
        free: th,
        readings,
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
        let t = tape_targets(&m).unwrap();
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
        assert!(t.iter().all(|target| target.sigma_mm.is_none()));
    }

    #[test]
    fn target_uncertainty_stays_with_its_selected_measurement() {
        let mut m = serde_json::json!({
            "wearer":{"age_months":30.0,"stature_mm":921.0},
            "fused":{"stature":{"value_mm":920.0,"sigma_mm":12.0},
                "waist":{"value_mm":530.0,"sigma_mm":18.0},
                "chest":{"value_mm":540.0}}
        });
        let wearer = Wearer::from_measurements(&m).unwrap();
        let targets = tape_targets(&m).unwrap();
        assert_eq!(targets[0].value_mm, 921.0);
        assert_eq!(targets[0].sigma_mm, None);
        assert_eq!(targets[1].name, "chest");
        assert_eq!(targets[1].sigma_mm, None);
        assert_eq!(targets[2].name, "waist");
        assert_eq!(targets[2].sigma_mm, Some(18.0));
        let rows = tape_rows(&wearer, &targets).unwrap();
        assert_eq!(rows[0].sigma, FIT_SIGMA_LENGTH_MM);
        assert_eq!(rows[1].sigma, FIT_SIGMA_GIRTH_MM);
        assert_eq!(rows[2].sigma, 18.0);

        // Numeric equality does not turn the independent wearer override into the fused evidence.
        m["wearer"]["stature_mm"] = serde_json::json!(920.0);
        assert_eq!(tape_targets(&m).unwrap()[0].sigma_mm, None);
        m["wearer"].as_object_mut().unwrap().remove("stature_mm");
        assert_eq!(tape_targets(&m).unwrap()[0].sigma_mm, Some(12.0));
    }

    #[test]
    fn malformed_uncertainty_is_not_treated_as_missing() {
        for sigma in [
            serde_json::Value::Null,
            serde_json::json!(0.0),
            serde_json::json!(-1.0),
            serde_json::json!("5.0"),
            serde_json::json!(true),
            serde_json::json!([]),
        ] {
            let m = serde_json::json!({"fused":{"waist":{"value_mm":530.0,"sigma_mm":sigma}}});
            let error = tape_targets(&m).unwrap_err();
            assert_eq!(error.code, RefusalCode::InvalidDocument);
            assert!(error.message.contains("waist.sigma_mm"));
        }
        let wearer = Wearer {
            years: 2.5,
            gender: 0.5,
            stature_mm: 920.0,
            weight_kg: None,
        };
        for sigma in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            let target = TapeTarget {
                name: "waist".into(),
                value_mm: 530.0,
                sigma_mm: Some(sigma),
            };
            let error = tape_rows(&wearer, &[target]).unwrap_err();
            assert_eq!(error.code, RefusalCode::InvalidDocument);
            assert!(error.message.contains("waist.sigma_mm"));
        }
    }

    fn stature_test_model() -> AnnyModel {
        use crate::AnnyData;
        use ftx_core::Mat3;
        use serde_json::json;

        // An independent four-point length model, not an anatomical body: stature in mm is
        // exactly 900 + 100 * height + 100 * local_length. Its 900..1000 height-only interval
        // is strictly smaller than the full 800..1100 interval of the bounded shape controls.
        let data = AnnyData {
            header: serde_json::from_value(json!({
                "schema":"ftxbody/1", "model":"anny", "anny_version":"0.6.1",
                "configuration":"synthetic length test", "licence":"synthetic", "package_sha256":{},
                "counts":{"vertices":4,"faces":4,"bones":1,"blendshapes":4,
                    "phenotype_blendshapes":2,"local_changes":1,"influences":1},
                "phenotype":{"features":["height"],"components":[["height","min"],["height","max"]],
                    "anchors":{"height":[0.0,1.0]},"exposed":["height"],"fixed_value":0.5},
                "labels":{"blendshapes":["min","max","length+","length-"],"bones":["root"],
                    "bone_parents":[-1],"local_changes":["measure-upperleg-height-incr"]},
                "y_axis":[0,1,0], "degenerate_rotation":[[1,0,0],[0,1,0],[0,0,1]],
                "waist_loop_final":[], "arrays":{}
            }))
            .unwrap(),
            template_vertices: vec![
                Vec3::ZERO,
                Vec3::new(0.1, 0.0, 0.0),
                Vec3::new(0.0, 0.1, 0.0),
                Vec3::new(0.0, 0.0, 0.9),
            ],
            faces: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            blendshape_row_ptr: vec![0, 0, 1, 2, 3],
            blendshape_vertex: vec![3, 3, 3],
            blendshape_delta: vec![
                Vec3::new(0.0, 0.0, 0.1),
                Vec3::new(0.0, 0.0, 0.1),
                Vec3::new(0.0, 0.0, -0.1),
            ],
            phenotype_mask: vec![1, 0, 0, 1],
            template_bone_heads: vec![Vec3::ZERO],
            template_bone_tails: vec![Vec3::new(0.0, 1.0, 0.0)],
            bone_heads_blendshapes: vec![Vec3::ZERO; 4],
            bone_tails_blendshapes: vec![Vec3::ZERO; 4],
            bone_rolls: vec![Mat3::IDENTITY],
            vertex_bone_indices: vec![0; 4],
            vertex_bone_weights: vec![1.0; 4],
            base_mesh_vertex_indices: vec![0, 1, 2, 3],
        };
        AnnyModel::from_data(data).unwrap()
    }

    #[test]
    fn full_tape_fit_reaches_lengths_outside_the_height_only_seed_range() {
        let model = stature_test_model();
        let local_index = FREE
            .iter()
            .position(|name| *name == "measure-upperleg-height-incr")
            .unwrap();
        for (target, endpoint) in [(850.0, 0.0), (1050.0, 1.0)] {
            let wearer = Wearer {
                years: 2.5,
                gender: 0.5,
                stature_mm: target,
                weight_kg: None,
            };
            assert_eq!(
                fit_stature(&model, &wearer).unwrap_err().code,
                RefusalCode::OutOfRange,
                "the standalone one-control fit must retain its strict contract"
            );
            let (start, achieved) = stature_start(&model, &wearer, true).unwrap();
            assert_eq!(start.phenotype.height, endpoint);
            assert_eq!(achieved, 900.0 + 100.0 * endpoint);
            let rows = tape_rows(&wearer, &[]).unwrap();
            let fit = optimize_tape(&rows, start.phenotype.height, |th| {
                let mut params = model.params(Phenotype {
                    height: th[0],
                    ..Phenotype::default()
                });
                params.local_changes[0] = th[local_index];
                let rest = model.rest(&params)?;
                let mut readings = [None; N_TAPE + 1];
                readings[0] = Some(model.anthropometry(&rest).height * 1000.0);
                Ok(readings)
            })
            .unwrap();
            // At this constrained quadratic minimum height is on its endpoint. The remaining
            // local parameter balances the unchanged 2 mm target against its 0.35 prior.
            let desired_local = (target - achieved) / 100.0;
            let exact_local = desired_local * 10_000.0
                / (10_000.0 + FIT_SIGMA_LENGTH_MM.powi(2) / PRIOR_SIGMA_LOCAL.powi(2));
            assert!((fit.free[0] - endpoint).abs() < 1e-8);
            assert!(
                (fit.free[local_index] - exact_local).abs() < 1e-5,
                "target {target}, local {} versus {exact_local}",
                fit.free[local_index]
            );
            assert!((fit.readings[0].unwrap() - target).abs() < 0.2);
            assert_eq!(
                rows[0].target, target,
                "the requested stature was not clipped"
            );
        }
    }

    #[test]
    fn tape_and_stature_initializers_match_inside_the_original_range() {
        let model = stature_test_model();
        for stature_mm in [900.0, 925.0, 950.0, 975.0, 1000.0] {
            let wearer = Wearer {
                years: 2.5,
                gender: 0.5,
                stature_mm,
                weight_kg: None,
            };
            assert_eq!(
                stature_start(&model, &wearer, true).unwrap(),
                fit_stature(&model, &wearer).unwrap()
            );
        }
        for stature_mm in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let wearer = Wearer {
                years: 2.5,
                gender: 0.5,
                stature_mm,
                weight_kg: None,
            };
            assert_eq!(
                stature_start(&model, &wearer, true).unwrap_err().code,
                RefusalCode::OutOfRange
            );
        }
    }

    #[test]
    fn bounded_tape_step_releases_and_reenters_correlated_bounds() {
        // AᵀA = [[1, -0.75], [-0.75, 1]]. Starting near x's bound makes x hit first,
        // but after y hits its bound the constrained solution requires releasing x.
        let mut a = [[0.0; N_FREE]; 2];
        a[0][0] = 1.0;
        a[0][1] = -0.75;
        a[1][1] = (1.0_f64 - 0.75 * 0.75).sqrt();
        for mirrored in [false, true] {
            let coordinate = |x: f64| if mirrored { 1.0 - x } else { x };
            let mut start = [0.5; N_FREE];
            start[0] = coordinate(0.9);
            start[1] = coordinate(0.0);
            for (target, expected) in [([2.0, 3.0], [0.5, 1.0]), ([10.0, 100.0], [0.0, 1.0])] {
                let dx = coordinate(target[0]) - start[0];
                let dy = coordinate(target[1]) - start[1];
                let b = [dx - 0.75 * dy, a[1][1] * dy];
                let step = bounded_tape_step(&a, &b, &start).unwrap();
                for j in 0..2 {
                    assert!(
                        (start[j] + step[j] - coordinate(expected[j])).abs() < 1e-12,
                        "target {target:?}, mirrored {mirrored}, step {step:?}"
                    );
                }
                // The second target requires the released x to hit the opposite bound.
                // Independently check optimality against every corner of the feasible square.
                let cost = |x: f64, y: f64| {
                    let dx = x - coordinate(target[0]);
                    let dy = y - coordinate(target[1]);
                    dx * dx - 1.5 * dx * dy + dy * dy
                };
                let value = cost(start[0] + step[0], start[1] + step[1]);
                for x in [0.0, 1.0] {
                    for y in [0.0, 1.0] {
                        assert!(value <= cost(x, y) + 1e-12);
                    }
                }
            }
        }
    }

    #[test]
    fn bounded_tape_step_preserves_the_interior_least_squares_answer() {
        let a: Vec<[f64; N_FREE]> = (0..N_FREE)
            .map(|j| {
                let mut row = [0.0; N_FREE];
                row[j] = 2.0;
                row[(j + 1) % N_FREE] = -0.25;
                row
            })
            .collect();
        let b: Vec<_> = (0..N_FREE).map(|j| (j % 3) as f64 * 0.1 - 0.1).collect();
        let start = std::array::from_fn(|j| if j < N_PHENOTYPE { 0.5 } else { 0.0 });
        assert_eq!(
            bounded_tape_step(&a, &b, &start).unwrap().map(f64::to_bits),
            lstsq(&a, &b).map(f64::to_bits)
        );
    }

    #[test]
    fn bounded_tape_step_matches_independent_rectangle_edge_minima() {
        // A convex quadratic in one phenotype and one local control has its constrained minimum
        // at the unconstrained point or at one of four analytically minimized rectangle edges.
        for (xx, yy, xy) in [
            (1.0_f64, 1.0_f64, -0.75_f64),
            (1.0, 1.0, 0.75),
            (10.0, 1.0, -2.0),
            (10.0, 1.0, 2.0),
            (1.0, 10.0, -2.0),
            (1.0, 10.0, 2.0),
        ] {
            let mut a = [[0.0; N_FREE]; 2];
            a[0][0] = xx.sqrt();
            a[0][N_PHENOTYPE] = xy / xx.sqrt();
            a[1][N_PHENOTYPE] = (yy - xy * xy / xx).sqrt();
            for tx in [-1.0, 0.4, 2.0] {
                for ty in [-3.0, 0.25, 3.0] {
                    let cost = |x: f64, y: f64| {
                        xx * (x - tx).powi(2)
                            + 2.0 * xy * (x - tx) * (y - ty)
                            + yy * (y - ty).powi(2)
                    };
                    let mut expected = f64::INFINITY;
                    for x in [0.0, 1.0] {
                        expected =
                            expected.min(cost(x, (ty - xy / yy * (x - tx)).clamp(-1.0, 1.0)));
                    }
                    for y in [-1.0, 1.0] {
                        expected = expected.min(cost((tx - xy / xx * (y - ty)).clamp(0.0, 1.0), y));
                    }
                    if (0.0..=1.0).contains(&tx) && (-1.0..=1.0).contains(&ty) {
                        expected = 0.0;
                    }
                    for x0 in [0.0, 0.37, 1.0] {
                        for y0 in [-1.0, 0.2, 1.0] {
                            let mut start =
                                std::array::from_fn(|j| if j < N_PHENOTYPE { 0.5 } else { 0.0 });
                            start[0] = x0;
                            start[N_PHENOTYPE] = y0;
                            let b = [
                                a[0][0] * (tx - x0) + a[0][N_PHENOTYPE] * (ty - y0),
                                a[1][N_PHENOTYPE] * (ty - y0),
                            ];
                            let step = bounded_tape_step(&a, &b, &start).unwrap();
                            let (x, y) = (x0 + step[0], y0 + step[N_PHENOTYPE]);
                            assert!((0.0..=1.0).contains(&x) && (-1.0..=1.0).contains(&y));
                            assert!((cost(x, y) - expected).abs() < 1e-10);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn reported_uncertainty_changes_the_actual_tape_optimizer() {
        let solve = |waist_sigma: Option<f64>, chest_sigma: Option<f64>| {
            let mut m = serde_json::json!({"wearer":{"age_months":30.0,"stature_mm":920.0},
                "fused":{"waist":{"value_mm":510.0},"chest":{"value_mm":560.0}}});
            for (name, sigma) in [("waist", waist_sigma), ("chest", chest_sigma)] {
                if let Some(sigma) = sigma {
                    m["fused"][name]["sigma_mm"] = serde_json::json!(sigma);
                }
            }
            let wearer = Wearer::from_measurements(&m).unwrap();
            let rows = tape_rows(&wearer, &tape_targets(&m).unwrap()).unwrap();
            // Analytic one-parameter body: chest and waist share a 500 + 100 q mm section.
            // Contradictory targets force the production LM solver to use both declared scales.
            // The exact optimum follows by differentiating the weighted quadratic plus q's prior.
            let result = optimize_tape(&rows, 0.5, |th| {
                let mut readings = [Some(500.0); N_TAPE + 1];
                readings[0] = Some(920.0);
                readings[1] = Some(500.0 + 100.0 * th[N_PHENOTYPE]);
                readings[2] = readings[1];
                Ok(readings)
            })
            .unwrap();
            let a = 1.0 / waist_sigma.unwrap_or(3.0).max(3.0).powi(2);
            let b = 1.0 / chest_sigma.unwrap_or(3.0).max(3.0).powi(2);
            let exact = 100.0 * (10.0 * a + 60.0 * b)
                / (10_000.0 * (a + b) + 1.0 / PRIOR_SIGMA_LOCAL.powi(2));
            assert!(
                (result.free[N_PHENOTYPE] - exact).abs() < 1e-7,
                "{} versus analytic {exact}",
                result.free[N_PHENOTYPE]
            );
            result
        };
        let precise_waist = solve(Some(3.0), Some(30.0));
        let precise_chest = solve(Some(30.0), Some(3.0));
        assert!(precise_waist.readings[2].unwrap() < 512.0);
        assert!(precise_chest.readings[1].unwrap() > 558.0);
        assert!(precise_chest.readings[2].unwrap() - precise_waist.readings[2].unwrap() > 45.0);

        let legacy = solve(None, None);
        let at_floor = solve(Some(3.0), Some(3.0));
        let below_floor = solve(Some(0.1), Some(0.2));
        assert_eq!(legacy.free, at_floor.free);
        assert_eq!(legacy.free, below_floor.free);
        let repeat = solve(Some(3.0), Some(30.0));
        assert_eq!(precise_waist.free, repeat.free);
        assert_eq!(precise_waist.readings, repeat.readings);
        assert_eq!(precise_waist.iterations, repeat.iterations);
    }

    #[test]
    fn optional_sections_cannot_block_a_requested_tape_fit() {
        let wearer = Wearer {
            years: 2.5,
            gender: 0.5,
            stature_mm: 920.0,
            weight_kg: None,
        };
        let waist = TapeTarget {
            name: "waist".into(),
            value_mm: 540.0,
            sigma_mm: None,
        };
        let rows = tape_rows(&wearer, std::slice::from_ref(&waist)).unwrap();
        let read = |th: &[f64; N_FREE], complete: bool| {
            let mut readings = [if complete { Some(500.0) } else { None }; N_TAPE + 1];
            readings[0] = Some(wearer.stature_mm);
            readings[2] = Some(500.0 + 100.0 * th[N_PHENOTYPE]);
            readings
        };
        let complete = optimize_tape(&rows, 0.5, |th| Ok(read(th, true))).unwrap();
        let partial = optimize_tape(&rows, 0.5, |th| Ok(read(th, false))).unwrap();
        assert_eq!(complete.free, partial.free);
        assert_eq!(complete.iterations, partial.iterations);
        assert_eq!(partial.readings[0], Some(wearer.stature_mm));
        assert!((partial.readings[2].unwrap() - waist.value_mm).abs() < 0.3);
        assert_eq!(partial.readings[16], None);

        let requested = tape_rows(
            &wearer,
            &[
                waist,
                TapeTarget {
                    name: "head".into(),
                    value_mm: 500.0,
                    sigma_mm: None,
                },
            ],
        )
        .unwrap();
        let error = optimize_tape(&requested, 0.5, |th| Ok(read(th, false)))
            .err()
            .expect("a missing requested head must refuse");
        assert_eq!(error.code, RefusalCode::DegenerateGeometry);
        assert!(error.message.contains("head"));
        assert!(!error.message.contains("waist"));

        let weighted_rows = tape_rows(
            &Wearer {
                weight_kg: Some(10.0),
                ..wearer
            },
            &[],
        )
        .unwrap();
        let error = optimize_tape(&weighted_rows, 0.5, |th| Ok(read(th, false)))
            .err()
            .expect("a supplied weight remains a required constraint");
        assert_eq!(error.code, RefusalCode::DegenerateGeometry);
        assert!(error.message.contains("weight"));

        // Invalid required values are unavailable; unrelated missing anatomy is still permitted.
        for value in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            let error = optimize_tape(&rows, 0.5, |th| {
                let mut readings = read(th, false);
                readings[2] = value;
                Ok(readings)
            })
            .err()
            .expect("an invalid requested waist must refuse");
            assert_eq!(error.code, RefusalCode::DegenerateGeometry);
            assert!(error.message.contains("waist"));
        }
    }

    #[test]
    fn probes_and_trials_cannot_drop_a_requested_section() {
        let wearer = Wearer {
            years: 2.5,
            gender: 0.5,
            stature_mm: 920.0,
            weight_kg: None,
        };
        let rows = tape_rows(
            &wearer,
            &[TapeTarget {
                name: "waist".into(),
                value_mm: 600.0,
                sigma_mm: None,
            }],
        )
        .unwrap();
        let solve = |upper_limit: f64| {
            optimize_tape(&rows, 0.5, |th| {
                let mut readings = [None; N_TAPE + 1];
                readings[0] = Some(wearer.stature_mm);
                let q = th[N_PHENOTYPE];
                // The negative initial probe lacks the requested section; the positive probe remains
                // usable. Trial steps beyond the supplied domain must keep, not drop, this constraint.
                readings[2] = (q >= 0.0 && q <= upper_limit).then_some(500.0 + 100.0 * q);
                Ok(readings)
            })
            .unwrap()
        };
        let open = solve(1.0);
        let exact = 10_000.0 / (10_000.0 + FIT_SIGMA_GIRTH_MM.powi(2) / PRIOR_SIGMA_LOCAL.powi(2));
        assert!((open.free[N_PHENOTYPE] - exact).abs() < 1e-7);
        let restricted = solve(0.25);
        assert!(restricted.free[N_PHENOTYPE] > 0.2);
        assert!(restricted.free[N_PHENOTYPE] <= 0.25);
        assert_eq!(
            restricted.readings[2],
            Some(500.0 + 100.0 * restricted.free[N_PHENOTYPE])
        );
        assert!(600.0 - restricted.readings[2].unwrap() >= 75.0);
        assert_eq!(restricted.readings[16], None);
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
        let mut frames = BodyFrames {
            floor_z: 0.0,
            landmarks,
            torso: dummy,
            arm_r: dummy,
            arm_l: dummy,
            leg_r: dummy,
            leg_l: dummy,
            tip_h: 0.0,
            tip_h_l: 0.0,
            upper_arm_radius: 0.0,
            upper_arm_radius_l: 0.0,
            arm_length: 0.0,
            arm_length_l: 0.0,
            neck: dummy,
            head_bone: Vec3::ZERO,
            shoulder_joint_r: Vec3::ZERO,
            torso_points: Vec::new(),
            leg_points: Vec::new(),
            leg_points_l: Vec::new(),
            leg_landmarks_l: [0.0; 3],
        };
        let girth = seat_girth(&faces, &posed, &frames).expect("seat girth");
        assert!((girth - 480.0).abs() < 1e-6, "got {girth}");

        // The square frustum has an independently known 104 mm side at z = 180 mm.
        // It has no head, neck or knee anatomy, which must not erase its valid torso sections.
        frames.neck.origin.z = 1000.0;
        let readings = partial_tape_readings(&faces, 0.2, &posed, &frames);
        assert_eq!(readings[0], Some(200.0));
        assert!((readings[2].unwrap() - 416.0).abs() < 1e-6);
        assert!((readings[3].unwrap() - 480.0).abs() < 1e-6);
        assert_eq!(readings[4], Some(180.0));
        assert_eq!(readings[5], Some(20.0));
        for index in [1, 7, 8, 9, 13, 14, 15, 16, 17, 18] {
            assert_eq!(
                readings[index], None,
                "unexpected field {}",
                TAPE_FIELDS[index]
            );
        }
        assert!(tape_readings(&faces, 0.2, &posed, &frames).is_none());
        let no_stature = partial_tape_readings(&faces, f64::NAN, &posed, &frames);
        assert_eq!(no_stature[0], None);
        assert_eq!(no_stature[2], readings[2]);
        assert_eq!(no_stature[3], readings[3]);

        frames.landmarks.remove("waist");
        let no_waist = partial_tape_readings(&faces, 0.2, &posed, &frames);
        assert_eq!(no_waist[2], None);
        assert_eq!(no_waist[3], None);
        assert_eq!(no_waist[4], None);
        assert_eq!(no_waist[5], Some(20.0));
    }
}
