//! The reference's analytic body (`body.py`, spec 02 Part B): measurement values, landmark heights, the
//! girth-matched torso sections with the crotch ring, part frames, section radii and landmark heights in part
//! frames. The drafting programs read it; the stable default form (`ANATOMICAL = False`) only.

use std::f64::consts::PI;

use ftx_core::{Refusal, RefusalCode, Vec3};
use serde_json::Value;

const A_POSE_DEG: f64 = 35.0;
const LEG_SPREAD_DEG: f64 = 4.0;

/// A measurement document: `fabric-measurements/1` (values under `fused`) or a flat name → value object.
#[derive(Clone, Copy, Debug)]
pub struct Meas<'a>(pub &'a Value);

impl Meas<'_> {
    /// `mval`: the fused value (`value_mm`, else `value_deg`) or the flat number.
    ///
    /// # Errors
    /// `invalid_document` naming the measurement (the reference's `KeyError`) when it is missing or not a number.
    pub fn get(&self, name: &str) -> Result<f64, Refusal> {
        let v = match self.0.get("fused") {
            Some(f) => f
                .get(name)
                .map(|e| e.get("value_mm").unwrap_or_else(|| &e["value_deg"])),
            None => self.0.get(name),
        };
        v.and_then(Value::as_f64).ok_or_else(|| missing(name))
    }

    /// Whether the measurement is there (the reference catches its `KeyError` in two places).
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_ok()
    }
}

fn missing(name: &str) -> Refusal {
    Refusal::new(
        RefusalCode::InvalidDocument,
        format!("missing measurement '{name}'"),
    )
    .repair("measure it, or fuse the file with priors (ftx measure fuse)")
}

fn ramanujan(a: f64, b: f64) -> f64 {
    let r = (a - b) / (a + b);
    let h = r * r;
    PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()))
}

fn depth_for_perimeter(perimeter: f64, a: f64) -> f64 {
    let (mut lo, mut hi) = (1.0, a.max(perimeter / 2.0));
    if ramanujan(a, lo) > perimeter {
        return lo;
    }
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if ramanujan(a, mid) < perimeter {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Landmark heights (mm above the floor) and derived dimensions, `landmarks(meas)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Landmarks(pub Vec<(&'static str, f64)>);

impl Landmarks {
    /// A landmark by name.
    ///
    /// # Errors
    /// `invalid_document` for a name that is not a landmark.
    pub fn get(&self, name: &str) -> Result<f64, Refusal> {
        self.0
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| *v)
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!("no landmark '{name}'"),
                )
            })
    }

    fn at(&self, name: &str) -> f64 {
        self.get(name).unwrap_or(f64::NAN)
    }
}

/// `landmarks(meas)`.
///
/// # Errors
/// A missing measurement.
pub fn landmarks(m: Meas) -> Result<Landmarks, Refusal> {
    let z_waist = m.get("waist_height")?;
    let z_c7 = z_waist + m.get("back_neck_to_waist")?;
    let slope = m.get("shoulder_slope_deg")?.to_radians();
    let neck_r = m.get("neck_base")? / (2.0 * PI);
    let shoulder_half = neck_r + m.get("shoulder_length")? * slope.cos();
    let z_snp = z_c7 - 5.0;
    let z_shoulder = z_snp - m.get("shoulder_length")? * slope.sin();
    let z_chest = z_c7 - m.get("armscye_depth")?;
    let z_crotch = m.get("crotch_height")?;
    let z_hip = z_crotch + 0.45 * (z_waist - z_crotch);
    let head_r = m.get("head")? / (2.0 * PI);
    let knee = m.get("knee_height")?;
    let stature = m.get("stature")?;
    Ok(Landmarks(vec![
        ("floor", 0.0),
        ("ankle", 55.0),
        ("calf", 55.0 + 0.6 * (knee - 55.0)),
        ("knee", knee),
        ("crotch", z_crotch),
        ("hip", z_hip),
        ("waist", z_waist),
        ("chest", z_chest),
        ("across_back", 0.5 * (z_chest + z_shoulder)),
        ("shoulder", z_shoulder),
        ("snp", z_snp),
        ("c7", z_c7),
        ("head_top", stature),
        ("head_centre", stature - head_r),
        ("neck_r", neck_r),
        ("head_r", head_r),
        ("shoulder_half_width", shoulder_half),
        ("hip_joint_x", m.get("hip_width")? / 4.0),
        ("arm_length", m.get("arm_length")?),
    ]))
}

/// A torso section: height, half width, half depth, centre y.
pub type Section = (f64, f64, f64, f64);

fn sagittal_path(l: &Landmarks, ring: Section, hip: Section, waist: Section) -> f64 {
    let (z1, _, b1, cy1) = ring;
    let (zh, _, bh, cyh) = hip;
    let (zw, _, bw, cyw) = waist;
    let pts = [
        (cyw - bw, zw),
        (cyh - bh, zh),
        (cy1 - b1, z1),
        (cy1, l.at("crotch")),
        (cy1 + b1, z1),
        (cyh + bh, zh),
        (cyw + bw, zw),
    ];
    pts.windows(2)
        .map(|w| ftx_core::det::hypot(w[1].0 - w[0].0, w[1].1 - w[0].1))
        .fold(0.0, |a, b| a + b)
}

fn crotch_ring(m: Meas, l: &Landmarks, hip: Section, waist: Section) -> Section {
    let (zh, ah, bh, cyh) = hip;
    let crotch = l.at("crotch");
    let z1 = crotch + 25.0f64.min(0.5 * (zh - crotch));
    let ring = |b: f64| (z1, 0.92 * ah, b, cyh);
    let mut lo = bh * (z1 - crotch) / (zh - crotch);
    let Ok(target) = m.get("total_crotch_length") else {
        return ring(0.6 * bh);
    };
    if sagittal_path(l, ring(lo), hip, waist) >= target {
        return ring(lo);
    }
    let mut hi = bh;
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if sagittal_path(l, ring(mid), hip, waist) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    ring(0.5 * (lo + hi))
}

/// `torso_sections(meas)`: the crotch ring, hip, waist, chest, across back, shoulder and neck base.
///
/// # Errors
/// A missing measurement.
pub fn torso_sections(m: Meas) -> Result<Vec<Section>, Refusal> {
    let l = landmarks(m)?;
    let back_y = m.get("chest_depth")? / 2.0;
    let mut out: Vec<Section> = Vec::new();
    let sec = |out: &mut Vec<Section>, z: f64, girth: f64, width: f64| {
        let a = width / 2.0;
        let b = depth_for_perimeter(girth, a);
        out.push((z, a, b, back_y - b));
    };
    sec(&mut out, l.at("hip"), m.get("hip")?, m.get("hip_width")?);
    sec(
        &mut out,
        l.at("waist"),
        m.get("waist")?,
        m.get("waist_width")?,
    );
    let ring = crotch_ring(m, &l, out[0], out[1]);
    out.insert(0, ring);
    let (chest, chest_w) = (m.get("chest")?, m.get("chest_width")?);
    sec(&mut out, l.at("chest"), chest, chest_w);
    sec(&mut out, l.at("across_back"), 0.97 * chest, 0.98 * chest_w);
    let chest_cy = out[3].3;
    sec(
        &mut out,
        l.at("shoulder") - 15.0,
        0.80 * chest,
        2.0 * l.at("shoulder_half_width") - 30.0,
    );
    let neck_r = l.at("neck_r");
    sec(
        &mut out,
        l.at("snp") + 5.0,
        2.0 * PI * neck_r * 1.6,
        2.0 * neck_r * 2.2,
    );
    let n = out.len();
    for s in &mut out[n - 2..] {
        s.3 = chest_cy + 15.0;
    }
    Ok(out)
}

/// `crotch_path_mm(meas)`: the form's own total crotch length.
///
/// # Errors
/// A missing measurement.
pub fn crotch_path_mm(m: Meas) -> Result<f64, Refusal> {
    let s = torso_sections(m)?;
    Ok(sagittal_path(&landmarks(m)?, s[0], s[1], s[2]))
}

/// A part frame: `h` runs along `up` from `origin`; θ = 0 faces `front`, 90° faces `lat = up × front`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Origin (mm).
    pub origin: Vec3,
    /// Proximal direction.
    pub up: Vec3,
    /// Front.
    pub front: Vec3,
    /// `up × front`.
    pub lat: Vec3,
    /// Arms: the shoulder tip's h.
    pub tip_h: Option<f64>,
    /// Legs: hip joint to ankle along the leg.
    pub length: Option<f64>,
    /// Legs: hip joint to knee along the leg.
    pub knee_d: Option<f64>,
}

/// `frames(meas)[part]` for `torso`, `arm_R`, `arm_L`, `leg_R`, `leg_L`.
///
/// # Errors
/// A missing measurement; `invalid_document` for an unknown part.
pub fn frame(m: Meas, part: &str) -> Result<Frame, Refusal> {
    let l = landmarks(m)?;
    let front = Vec3::new(0.0, -1.0, 0.0);
    let cross = |u: Vec3, f: Vec3| {
        Vec3::new(
            u.y * f.z - u.z * f.y,
            u.z * f.x - u.x * f.z,
            u.x * f.y - u.y * f.x,
        )
    };
    let a = A_POSE_DEG.to_radians();
    let up_arm = m.get("upper_arm")? / (2.0 * PI);
    let _half_chest = m.get("chest_width")? / 2.0;
    let base = Frame {
        origin: Vec3::new(0.0, 0.0, 0.0),
        up: Vec3::new(0.0, 0.0, 1.0),
        front,
        lat: Vec3::ZERO,
        tip_h: None,
        length: None,
        knee_d: None,
    };
    let f = match part {
        "torso" => base,
        "arm_R" | "arm_L" | "leg_R" | "leg_L" => {
            let sx = if part.ends_with('R') { -1.0 } else { 1.0 };
            if part.starts_with("arm") {
                let xj = l.at("shoulder_half_width") - 0.6 * up_arm;
                Frame {
                    origin: Vec3::new(sx * xj, 0.0, l.at("shoulder") - 0.8 * up_arm),
                    up: Vec3::new(-sx * a.sin(), 0.0, a.cos()),
                    tip_h: Some(0.8 * up_arm),
                    ..base
                }
            } else {
                let b = LEG_SPREAD_DEG.to_radians();
                let hip = Vec3::new(sx * l.at("hip_joint_x"), 0.0, l.at("crotch") + 25.0);
                Frame {
                    origin: hip,
                    up: Vec3::new(-sx * b.sin(), 0.0, b.cos()),
                    length: Some((hip.z - l.at("ankle")) / b.cos()),
                    knee_d: Some((hip.z - l.at("knee")) / b.cos()),
                    ..base
                }
            }
        }
        _ => {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("no part '{part}'"),
            ));
        }
    };
    Ok(Frame {
        lat: cross(f.up, f.front),
        ..f
    })
}

fn interp_section(secs: &[Section], h: f64) -> (f64, f64, f64) {
    let zs: Vec<f64> = secs.iter().map(|s| s.0).collect();
    let col = |f: fn(&Section) -> f64| {
        let v: Vec<f64> = secs.iter().map(f).collect();
        ftx_core::np::interp(h, &zs, &v)
    };
    (col(|s| s.1), col(|s| s.2), col(|s| s.3))
}

/// `section_radius(meas, part, h)`: the largest distance from the part axis to the form near `h`.
///
/// # Errors
/// A missing measurement; `invalid_document` for an unknown part.
pub fn section_radius(m: Meas, part: &str, h: f64) -> Result<f64, Refusal> {
    if part == "torso" {
        let (a, b, cy) = interp_section(&torso_sections(m)?, h);
        return Ok(ftx_core::np::linspace(0.0, 2.0 * PI, 96)
            .into_iter()
            .map(|t| ftx_core::det::hypot(a * t.sin(), cy - b * t.cos()))
            .fold(f64::NEG_INFINITY, f64::max));
    }
    if part.starts_with("arm") {
        return Ok(m.get("upper_arm")? * 1.15 / (2.0 * PI));
    }
    if part.starts_with("leg") {
        let mut r = m.get("thigh")? * 1.05 / (2.0 * PI);
        let fr = frame(m, part)?;
        let z = fr.origin.z + h * fr.up.z;
        if z > fr.origin.z - 20.0 {
            let (a, b, cy) = interp_section(&torso_sections(m)?, z);
            let xj = fr.origin.x.abs();
            let reach = if a > xj {
                let q = xj / a;
                cy.abs() + b * (1.0 - q * q).max(0.0).sqrt()
            } else {
                b
            };
            r = r.max(reach).max(a - xj);
        }
        return Ok(r);
    }
    Err(Refusal::new(
        RefusalCode::InvalidDocument,
        format!("no part '{part}'"),
    ))
}

/// `landmark_h(meas, part, name)`: a landmark's h in a part frame.
///
/// # Errors
/// A missing measurement; `invalid_document` for an unknown part or landmark.
pub fn landmark_h(m: Meas, part: &str, name: &str) -> Result<f64, Refusal> {
    let l = landmarks(m)?;
    if part == "torso" {
        return l.get(name);
    }
    let fr = frame(m, part)?;
    if part.starts_with("arm") {
        let tip = fr.tip_h.unwrap_or(0.0);
        let arm = l.at("arm_length");
        return match name {
            "shoulder_tip" => Ok(tip),
            "elbow" => Ok(tip - 0.55 * arm),
            "wrist" => Ok(tip - arm),
            _ => Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("no arm landmark '{name}'"),
            )),
        };
    }
    let z = l.get(name)?;
    Ok(-(fr.origin.z - z) / fr.up.z)
}
