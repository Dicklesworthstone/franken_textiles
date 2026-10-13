//! Sparse calibrated-landmark filtering for reviewed still-window acquisition.

use std::collections::BTreeSet;

use ftx_core::{Refusal, Vec2, Vec3, det, linalg::lstsq};
use serde::{Deserialize, Serialize};

use crate::{
    camera::Camera,
    invalid,
    triangulate::{FrameId, Partition},
};

const MIN_PARALLAX_DEG: f64 = 3.0;
const MAX_REPROJECTION_PX: f64 = 3.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct StillLandmark {
    pub name: String,
    pub pixel_px: Vec2,
    pub partition: Partition,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct StillFrame {
    pub frame: FrameId,
    pub camera: Camera,
    pub landmarks: Vec<StillLandmark>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct StillWindowInput {
    pub schema: String,
    pub metric_scale_source: String,
    pub observation_evidence: String,
    pub max_motion_mm: f64,
    pub frames: Vec<StillFrame>,
}

#[derive(Clone, Debug, Serialize)]
#[allow(missing_docs)]
pub struct HalfGeometry {
    pub views: usize,
    pub max_parallax_deg: f64,
    pub point_m: Option<Vec3>,
    pub max_reprojection_px: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
#[allow(missing_docs)]
pub struct StillLandmarkCheck {
    pub name: String,
    pub partition: Partition,
    pub early: HalfGeometry,
    pub late: HalfGeometry,
    pub all: HalfGeometry,
    pub motion_mm: Option<f64>,
    pub passed: bool,
    pub flags: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[allow(missing_docs)]
pub struct StillWindowReport {
    pub schema: &'static str,
    pub status: &'static str,
    pub input: StillWindowInput,
    pub start_us: u64,
    pub end_us: u64,
    pub fit_landmarks: usize,
    pub held_out_landmarks: usize,
    pub checks: Vec<StillLandmarkCheck>,
    pub flags: Vec<String>,
    pub advisories: Vec<String>,
    pub no_claim: &'static str,
}

fn text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 4096
}

fn max_angle(cameras: &[&Camera], rays: &[Vec3]) -> f64 {
    let mut best: f64 = 0.0;
    for i in 0..rays.len() {
        for j in i + 1..rays.len() {
            if (cameras[i].center_m - cameras[j].center_m).norm() > 1e-8 {
                best = best.max(
                    det::atan2(rays[i].cross(rays[j]).norm(), rays[i].dot(rays[j]).abs()) * 180.0
                        / std::f64::consts::PI,
                );
            }
        }
    }
    best
}

fn fit(cameras: &[&Camera], pixels: &[Vec2]) -> Result<HalfGeometry, Refusal> {
    if cameras.len() < 2 {
        return Ok(HalfGeometry {
            views: cameras.len(),
            max_parallax_deg: 0.0,
            point_m: None,
            max_reprojection_px: None,
        });
    }
    let origin = cameras[0].center_m;
    let scale = cameras
        .iter()
        .map(|c| (c.center_m - origin).norm())
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale == 0.0 {
        return Ok(HalfGeometry {
            views: cameras.len(),
            max_parallax_deg: 0.0,
            point_m: None,
            max_reprojection_px: None,
        });
    }
    let rays: Vec<_> = cameras
        .iter()
        .zip(pixels)
        .map(|(c, &p)| c.ray(p))
        .collect::<Result<_, _>>()?;
    let angle = max_angle(cameras, &rays);
    if angle < MIN_PARALLAX_DEG {
        return Ok(HalfGeometry {
            views: cameras.len(),
            max_parallax_deg: angle,
            point_m: None,
            max_reprojection_px: None,
        });
    }
    let centers: Vec<_> = cameras
        .iter()
        .map(|c| (c.center_m - origin) / scale)
        .collect();
    if centers.iter().any(|c| !c.is_finite()) {
        return Err(invalid(
            "still-window camera baseline cannot be represented finitely",
        ));
    }
    let mut rows = Vec::with_capacity(3 * cameras.len());
    let mut rhs = Vec::with_capacity(3 * cameras.len());
    for (&d, &c) in rays.iter().zip(&centers) {
        rows.extend([[0.0, -d.z, d.y], [d.z, 0.0, -d.x], [-d.y, d.x, 0.0]]);
        rhs.extend(<[f64; 3]>::from(d.cross(c)));
    }
    let normalized = Vec3::from(lstsq(&rows, &rhs));
    let point = origin + normalized * scale;
    if !point.is_finite() {
        return Ok(HalfGeometry {
            views: cameras.len(),
            max_parallax_deg: angle,
            point_m: None,
            max_reprojection_px: None,
        });
    }
    let mut max_error: f64 = 0.0;
    for (camera, &pixel) in cameras.iter().zip(pixels) {
        let Ok(projected) = camera.project(point) else {
            return Ok(HalfGeometry {
                views: cameras.len(),
                max_parallax_deg: angle,
                point_m: Some(point),
                max_reprojection_px: None,
            });
        };
        let error = projected.dist(pixel);
        if !error.is_finite() {
            return Ok(HalfGeometry {
                views: cameras.len(),
                max_parallax_deg: angle,
                point_m: Some(point),
                max_reprojection_px: None,
            });
        }
        max_error = max_error.max(error);
    }
    Ok(HalfGeometry {
        views: cameras.len(),
        max_parallax_deg: angle,
        point_m: Some(point),
        max_reprojection_px: Some(max_error),
    })
}

fn geometry_flags(label: &str, g: &HalfGeometry, flags: &mut Vec<String>) {
    if g.max_parallax_deg < MIN_PARALLAX_DEG {
        flags.push(format!(
            "{label}: maximum ray angle {:.3} deg is below 3 deg",
            g.max_parallax_deg
        ));
    } else if g.point_m.is_none() {
        flags.push(format!("{label}: no finite triangulation solution"));
    } else if let Some(error) = g.max_reprojection_px {
        if error > MAX_REPROJECTION_PX {
            flags.push(format!(
                "{label}: maximum reprojection {error:.3} px exceeds 3 px"
            ));
        }
    } else {
        flags.push(format!(
            "{label}: projection unavailable for the reconstructed point"
        ));
    }
}

/// Reject moving or geometrically weak candidate intervals. Passing is still only a candidate for
/// explicit human review; this function never creates `triangulate::Capture::StillWindow` evidence.
pub fn assess_still_window(input: &StillWindowInput) -> Result<StillWindowReport, Refusal> {
    if input.schema != "ftx-still-window-input/1"
        || !text(&input.metric_scale_source)
        || !text(&input.observation_evidence)
        || !input.max_motion_mm.is_finite()
        || input.max_motion_mm <= 0.0
        || input.max_motion_mm > 100.0
        || !(4..=64).contains(&input.frames.len())
    {
        return Err(invalid(
            "still-window needs schema /1, evidence, a motion threshold in (0,100] mm and 4..64 frames",
        ));
    }
    let mut input = input.clone();
    input.frames.sort_by(|a, b| a.frame.cmp(&b.frame));
    let mut ids = BTreeSet::new();
    let streams: BTreeSet<_> = input
        .frames
        .iter()
        .map(|f| f.frame.stream.as_str())
        .collect();
    if streams.len() != 1 {
        return Err(invalid(
            "still-window frames must come from exactly one stream",
        ));
    }
    for frame in &mut input.frames {
        if !text(&frame.frame.stream)
            || !ids.insert(frame.frame.clone())
            || frame.landmarks.is_empty()
            || frame.landmarks.len() > 128
        {
            return Err(invalid("invalid/duplicate frame or landmark count"));
        }
        frame.camera.validate()?;
        frame.landmarks.sort_by(|a, b| a.name.cmp(&b.name));
        let mut names = BTreeSet::new();
        for p in &frame.landmarks {
            if !text(&p.name) || !p.pixel_px.is_finite() || !names.insert(p.name.as_str()) {
                return Err(invalid("invalid or duplicate landmark"));
            }
            frame.camera.ray(p.pixel_px)?;
        }
    }
    let expected: Vec<_> = input.frames[0]
        .landmarks
        .iter()
        .map(|p| (p.name.clone(), p.partition))
        .collect();
    for frame in &input.frames[1..] {
        if frame
            .landmarks
            .iter()
            .map(|p| (&p.name, p.partition))
            .ne(expected.iter().map(|(n, p)| (n, p.to_owned())))
        {
            return Err(invalid(
                "every frame must contain the same landmark names and partitions",
            ));
        }
    }
    let fit_landmarks = expected
        .iter()
        .filter(|(_, p)| *p == Partition::Fit)
        .count();
    let held_out_landmarks = expected.len() - fit_landmarks;
    if fit_landmarks < 3 {
        return Err(invalid(
            "still-window assessment needs at least three fit landmarks observed in every frame",
        ));
    }
    let split = input.frames.len() / 2;
    let mut checks = Vec::with_capacity(expected.len());
    for (i, (name, partition)) in expected.iter().enumerate() {
        let cameras: Vec<_> = input.frames.iter().map(|f| &f.camera).collect();
        let pixels: Vec<_> = input
            .frames
            .iter()
            .map(|f| f.landmarks[i].pixel_px)
            .collect();
        let early = fit(&cameras[..split], &pixels[..split])?;
        let late = fit(&cameras[split..], &pixels[split..])?;
        let all = fit(&cameras, &pixels)?;
        let motion_mm = early
            .point_m
            .zip(late.point_m)
            .map(|(a, b)| (a - b).norm() * 1000.0)
            .filter(|v| v.is_finite());
        let mut flags = Vec::new();
        geometry_flags("early half", &early, &mut flags);
        geometry_flags("late half", &late, &mut flags);
        geometry_flags("complete interval", &all, &mut flags);
        match motion_mm {
            Some(v) if v > input.max_motion_mm => flags.push(format!(
                "early/late displacement {v:.3} mm exceeds {:.3} mm",
                input.max_motion_mm
            )),
            None => flags.push("early/late displacement is unavailable".into()),
            _ => {}
        }
        checks.push(StillLandmarkCheck {
            name: name.clone(),
            partition: *partition,
            early,
            late,
            all,
            motion_mm,
            passed: flags.is_empty(),
            flags,
        });
    }
    let fit_failures = checks
        .iter()
        .filter(|c| c.partition == Partition::Fit && !c.passed)
        .count();
    let held_failures = checks
        .iter()
        .filter(|c| c.partition == Partition::HeldOut && !c.passed)
        .count();
    let mut flags = Vec::new();
    if fit_failures > 0 {
        flags.push(format!(
            "{fit_failures} fit landmarks fail stationarity gates"
        ));
    }
    if held_failures > 0 {
        flags.push(format!(
            "{held_failures} held-out landmarks independently fail stationarity gates"
        ));
    }
    let unobservable = checks
        .iter()
        .any(|c| c.early.point_m.is_none() || c.late.point_m.is_none() || c.all.point_m.is_none());
    let mut advisories = Vec::new();
    if held_out_landmarks == 0 {
        advisories.push(
            "No held-out landmarks: the same correspondence set defines and checks the candidate"
                .into(),
        );
    }
    advisories.push(format!("The {:.3} mm motion threshold is caller supplied; it is not a calibrated body-measurement accuracy bound", input.max_motion_mm));
    let status = if flags.is_empty() {
        "STILL_CANDIDATE"
    } else if unobservable {
        "UNOBSERVABLE"
    } else {
        "REJECTED"
    };
    Ok(StillWindowReport {
        schema: "ftx-still-window/1",
        status,
        start_us: input.frames.first().unwrap().frame.timestamp_us,
        end_us: input.frames.last().unwrap().frame.timestamp_us,
        input,
        fit_landmarks,
        held_out_landmarks,
        checks,
        flags,
        advisories,
        no_claim: "Sparse calibrated landmarks support only a still-window candidate. Human review must establish that the relevant body region is sufficiently still before using this interval as triangulation capture evidence; untracked motion, correspondence errors, camera/scale errors and rolling shutter remain possible.",
    })
}
