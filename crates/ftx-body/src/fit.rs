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
//! | `waist_height`, `crotch_height` | the landmarks above the floor |
//!
//! A girth is [`crate::tape::girth`] around the torso axis.

use ftx_core::{Refusal, RefusalCode, linalg::lstsq};
use serde_json::Value;

use crate::drape::{Pose, pose_deltas};
use crate::landmarks::BodyFrames;
use crate::model::{AnnyModel, BodyParams, Phenotype, PosedBody, age_parameter_for_years};
use crate::tape::girth;

/// The wearer as the measurement file states them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wearer {
    /// Age (years).
    pub years: f64,
    /// Anny gender: 0 male, 1 female, 0.5 unknown.
    pub gender: f64,
    /// Stature (mm).
    pub stature_mm: f64,
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
    /// from `wearer.stature_mm` or else the fused stature, sex from `wearer.sex`.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] without an age or a stature.
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
        Ok(Self {
            years: months / 12.0,
            gender: Self::gender_of(w["sex"].as_str()),
            stature_mm: stature,
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
pub const TAPE_FIELDS: [&str; 6] = [
    "stature",
    "chest",
    "waist",
    "hip",
    "waist_height",
    "crotch_height",
];

/// How far below the armpit landmark the chest is read (mm).
pub const CHEST_BELOW_ARMPIT_MM: f64 = 10.0;

/// The fit's tolerance (1σ, mm) for a girth and for a length: the accuracy gate G1b asks of it.
pub const FIT_SIGMA_GIRTH_MM: f64 = 3.0;
/// See [`FIT_SIGMA_GIRTH_MM`].
pub const FIT_SIGMA_LENGTH_MM: f64 = 2.0;

/// The free parameters of [`fit_tape`]: the height and weight phenotype parameters, then local changes.
pub const FREE: [&str; 7] = [
    "height",
    "weight",
    "measure-bust-circ-incr",
    "measure-waist-circ-incr",
    "measure-hips-circ-incr",
    "measure-upperleg-height-incr",
    "measure-waisttohip-dist-incr",
];
const N_FREE: usize = FREE.len();
/// Phenotype parameters range over [0, 1]; local changes over [−1, 1], Anny's modelled range.
const LOCAL_LIMIT: f64 = 1.0;
/// Prior 1σ of every free parameter, about its population middle (0.5, or 0 for a local change).
const PRIOR_SIGMA: f64 = 1.0;
/// Central-difference step in the parameters.
const FD_STEP: f64 = 1e-3;
/// Levenberg–Marquardt iteration cap.
const MAX_ITERATIONS: usize = 40;

/// What a tape reads on `posed` (mm, in [`TAPE_FIELDS`] order), or `None` when a section around the
/// torso axis is missing (a body far outside the model's range).
#[must_use]
pub fn tape_readings(
    faces: &[[usize; 3]],
    rest_height_m: f64,
    posed: &PosedBody,
    frames: &BodyFrames,
) -> Option<[f64; 6]> {
    let lm = |n: &str| frames.landmarks.get(n).copied();
    let axis = [
        frames.torso.origin.x / 1000.0,
        frames.torso.origin.y / 1000.0,
    ];
    let g = |h: f64| {
        girth(&posed.vertices, faces, (frames.floor_z + h) / 1000.0, axis).map(|m| m * 1000.0)
    };
    let (waist_h, crotch_h, armpit_h) = (lm("waist")?, lm("crotch")?, lm("chest")?);
    let mut hip = f64::NEG_INFINITY;
    let mut h = crotch_h + 0.2 * (waist_h - crotch_h);
    while h <= waist_h {
        hip = hip.max(g(h)?);
        h += 2.0;
    }
    Some([
        rest_height_m * 1000.0,
        g(armpit_h - CHEST_BELOW_ARMPIT_MM)?,
        g(waist_h)?,
        hip,
        waist_h,
        crotch_h,
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
    pub readings: [f64; 6],
    /// The fitted measurements, target against body.
    pub residuals: Vec<TapeResidual>,
    /// The free parameters' values ([`FREE`] order).
    pub free: [f64; N_FREE],
    /// Levenberg–Marquardt iterations taken.
    pub iterations: usize,
}

/// Fits the body of `wearer` (age and gender; its stature is used when `targets` has none) to `targets`
/// on `pose`.
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] for a target that is not a [`TAPE_FIELDS`] name or not finite and
/// positive, or a model without the [`FREE`] local changes; refusals of [`fit_stature`] (the start) and of
/// the model; [`RefusalCode::DegenerateGeometry`] when the start body has no torso sections.
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
        let sigma = if matches!(t.name.as_str(), "chest" | "waist" | "hip") {
            FIT_SIGMA_GIRTH_MM
        } else {
            FIT_SIGMA_LENGTH_MM
        };
        rows.push((k, t.value_mm, sigma));
    }
    let lc: Vec<usize> = FREE[2..]
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
            ..base
        });
        for (j, &i) in lc.iter().enumerate() {
            p.local_changes[i] = th[2 + j];
        }
        p
    };
    let read = |th: &[f64; N_FREE]| -> Result<Option<[f64; 6]>, Refusal> {
        let rest = model.rest(&params_of(th))?;
        let height = model.anthropometry(&rest).height;
        let posed = model.pose(&rest, &pose_deltas(model, &rest, pose, 1.0)?)?;
        let frames = BodyFrames::new(model, &posed)?;
        Ok(tape_readings(&faces, height, &posed, &frames))
    };
    let prior = |j: usize| if j < 2 { 0.5 } else { 0.0 };
    let clamp = |th: [f64; N_FREE]| -> [f64; N_FREE] {
        std::array::from_fn(|j| {
            if j < 2 {
                th[j].clamp(0.0, 1.0)
            } else {
                th[j].clamp(-LOCAL_LIMIT, LOCAL_LIMIT)
            }
        })
    };
    // residual vector: the measurements, then the priors
    let resid = |th: &[f64; N_FREE], r: &[f64; 6]| -> Vec<f64> {
        rows.iter()
            .map(|&(k, v, s)| (r[k] - v) / s)
            .chain((0..N_FREE).map(|j| (th[j] - prior(j)) / PRIOR_SIGMA))
            .collect()
    };
    let cost = |r: &[f64]| r.iter().map(|x| x * x).sum::<f64>();
    let mut th: [f64; N_FREE] =
        std::array::from_fn(|j| if j == 0 { base.height } else { prior(j) });
    let mut readings = read(&th)?.ok_or_else(|| {
        Refusal::new(
            RefusalCode::DegenerateGeometry,
            "the start body has no torso section at the chest, waist or hip",
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
            let (Some(ru), Some(rd)) = (read(&up)?, read(&dn)?) else {
                continue;
            };
            for (i, &(k, _, s)) in rows.iter().enumerate() {
                jac[i][j] = (ru[k] - rd[k]) / ((up[j] - dn[j]) * s);
            }
        }
        let mut improved = false;
        while lambda < 1e10 {
            // [J; I/τ; √λ I] Δ = [−r_meas; −r_prior; 0]
            let mut a: Vec<[f64; N_FREE]> = jac.clone();
            a.extend((0..N_FREE).map(|j| {
                let mut row = [0.0; N_FREE];
                row[j] = 1.0 / PRIOR_SIGMA;
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
                    improved = moved > 1e-7 && gain > 1e-10 * c.max(1.0);
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
        .map(|&(k, v, _)| TapeResidual {
            name: TAPE_FIELDS[k].to_string(),
            target_mm: v,
            body_mm: readings[k],
        })
        .collect();
    Ok(TapeFit {
        params: params_of(&th),
        readings,
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
        let fused = serde_json::json!({"wearer": {"age_months": 24.0}, "fused": {"stature": {"value_mm": 870.0}}});
        assert_eq!(Wearer::from_measurements(&fused).unwrap().stature_mm, 870.0);
        assert!(Wearer::from_measurements(&serde_json::json!({"wearer": {}})).is_err());
    }

    #[test]
    fn targets_from_a_measurement_file() {
        let m = serde_json::json!({
            "wearer": {"age_months": 30.0, "stature_mm": 921.0},
            "fused": {"stature": {"value_mm": 920.0}, "waist": {"value_mm": 530.0},
                      "thigh": {"value_mm": 290.0}, "hip": {"value_deg": 3.0}},
        });
        let t = tape_targets(&m);
        let got: Vec<(&str, f64)> = t.iter().map(|t| (t.name.as_str(), t.value_mm)).collect();
        assert_eq!(got, [("stature", 921.0), ("waist", 530.0)]);
    }
}
