//! Replay original stationary-sheet acquisitions before differentiating their camera poses.
//! The existing sheet-pose authority owns admission; its held-out points never steer a refit.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::{FrameId, Partition, TriangulationInput};
use crate::{
    camera::{Camera, Intrinsics},
    invalid,
    photo::intrinsics::{CalibrationInput, CalibrationReport, calibrate_camera},
};

const MAX_FIT_CORNERS: usize = 256;
const MAX_RETAINED_CORNERS: usize = 1024;

/// Complete original sheet-pose evidence for one fitting measurement frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameSheetPose {
    /// Exact FIT frame; held-out and unknown triangulation frames cannot be assigned.
    pub frame: FrameId,
    /// Accepted `ftx-sheet-pose/1` report, including its complete original input.
    /// Replay must reproduce the report and the supplied measurement camera exactly.
    pub report: Value,
}

// Parsed only after the complete source has passed Camera::sheet_pose_report. These fields
// are the retained fit inputs for that same estimator, not a second sheet-input authority.
#[derive(Deserialize)]
struct SheetPoint {
    sheet_xz_m: Vec2,
    pixel_px: Vec2,
    #[serde(default)]
    held_out: bool,
}

#[derive(Deserialize)]
struct SheetInput {
    reference_id: String,
    intrinsics: Intrinsics,
    image_size_px: [u32; 2],
    rotation_applied_degrees: u16,
    points: Vec<SheetPoint>,
}

// The native --calibration route retains this annotation beside the ordinary pose report.
// Its byte hashes identify separately retained files; without those bytes they are provenance,
// never the authority for accepting the lens. Recompute the complete stored calibration input.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntrinsicsSource {
    schema: String,
    report_sha256: String,
    reference_input_sha256: String,
    lens_id: String,
    recomputed: bool,
    input: CalibrationInput,
    no_claim: String,
}

fn replay_intrinsics_source(
    annotation: Value,
    sheet: &SheetInput,
    cache: &mut BTreeMap<Vec<u8>, CalibrationReport>,
) -> Result<(), Refusal> {
    let source: IntrinsicsSource = serde_json::from_value(annotation)
        .map_err(|error| invalid(format!("invalid sheet intrinsics source: {error}")))?;
    let hash = |value: &str| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit());
    if source.schema != "ftx-camera-calibration/1"
        || !source.recomputed
        || !hash(&source.report_sha256)
        || !hash(&source.reference_input_sha256)
        || source.lens_id != source.input.lens_id
        || sheet.image_size_px != source.input.image_size_px
        || sheet.rotation_applied_degrees != source.input.rotation_cw_deg
    {
        return Err(invalid(
            "sheet intrinsics source does not match its original lens/settings",
        ));
    }
    let key = serde_json::to_vec(&source.input).map_err(|error| invalid(error.to_string()))?;
    let checked = match cache.entry(key) {
        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(calibrate_camera(&source.input)?)
        }
    };
    // Legacy native reports made with separate --corner-noise retain this exact disclaimer,
    // but not that optional noise document. Their geometry can still be replayed here; no
    // missing covariance is inferred or used from this annotation.
    let separate_noise_claim = source.input.image_model == "brown5_zero_skew"
        && source
            .input
            .distortion_fit
            .as_ref()
            .is_some_and(|fit| fit.corner_noise.is_none())
        && source.no_claim == "Local nonlinear Brown calibration; conditional corner-noise precision is reported separately and does not establish total calibration accuracy, global optimality or real-wearer fit. Sheet-local poses are not a common world frame when the sheet moves.";
    if checked.status != "ACCEPTED"
        || (source.no_claim != checked.no_claim && !separate_noise_claim)
        || checked.solution.as_ref().is_none_or(|solution| {
            serde_json::to_value(solution.intrinsics).ok()
                != serde_json::to_value(sheet.intrinsics).ok()
        })
    {
        return Err(invalid(
            "sheet intrinsics must match the accepted recomputed calibration",
        ));
    }
    Ok(())
}

pub(super) struct PreparedSheetPose {
    pub(super) intrinsics: Intrinsics,
    pub(super) fit_point_indices: Vec<usize>,
    pub(super) sheet_xz_m: Vec<Vec2>,
    pub(super) pixels: Vec<Vec2>,
    image_size_px: [u32; 2],
    rotation_applied_degrees: u16,
}

impl PreparedSheetPose {
    pub(super) fn normalized_camera(
        &self,
        intrinsics: Intrinsics,
        pixels: &[Vec2],
        origin: Vec3,
        scale: f64,
    ) -> Result<Camera, Refusal> {
        if !origin.is_finite() || !scale.is_finite() || scale <= 0. {
            return Err(super::unavailable(
                "sheet pose needs a finite normalization origin and positive baseline",
            ));
        }
        // Preserve the actual source estimator. Its noisy homography-to-rigid projection
        // is not equivalent to fitting a translated sheet and translating its camera back.
        // Normalize the recovered camera before triangulation, so the final differential
        // never subtracts two large reconstructed landmark positions. If source-camera
        // cancellation is itself unresolved, the two differential steps fail closed.
        let mut camera = Camera::from_sheet(intrinsics, &self.sheet_xz_m, pixels)?;
        camera.center_m = (camera.center_m - origin) / scale;
        camera.validate()?;
        Ok(camera)
    }

    pub(super) fn check_pixel(&self, pixel: Vec2) -> Result<(), Refusal> {
        if !pixel.is_finite()
            || pixel.x < 0.
            || pixel.y < 0.
            || pixel.x > f64::from(self.image_size_px[0] - 1)
            || pixel.y > f64::from(self.image_size_px[1] - 1)
        {
            return Err(super::unavailable(
                "differential sheet corner leaves its original oriented image",
            ));
        }
        Ok(())
    }
}

pub(super) struct PreparedSheetPoses {
    poses: BTreeMap<FrameId, PreparedSheetPose>,
}

impl PreparedSheetPoses {
    pub(super) fn pose(&self, frame: &FrameId) -> Result<&PreparedSheetPose, Refusal> {
        self.poses
            .get(frame)
            .ok_or_else(|| invalid("sheet pose is missing its exact fitting frame"))
    }

    pub(super) fn image_size_px(&self, frame: &FrameId) -> Result<[u32; 2], Refusal> {
        Ok(self.pose(frame)?.image_size_px)
    }

    pub(super) fn rotation_applied_degrees(&self, frame: &FrameId) -> Result<u16, Refusal> {
        Ok(self.pose(frame)?.rotation_applied_degrees)
    }
}

/// Bound and replay every original sheet report before exposing its FIT-only refit inputs.
/// A common exact reference identifier is required; metric_scale_source remains the caller's
/// free-form evidence. Neither identifier proves that a physical sheet stayed stationary.
pub(super) fn prepare_sheet_poses(
    input: &TriangulationInput,
    poses: &[FrameSheetPose],
) -> Result<PreparedSheetPoses, Refusal> {
    if poses.is_empty() || poses.len() > 128 {
        return Err(invalid("sheet poses need 1–128 exact fitting frames"));
    }
    if serde_json::to_vec(poses)
        .map_err(|error| invalid(error.to_string()))?
        .len()
        > 16 * 1024 * 1024
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "retained sheet-pose evidence exceeds 16 MiB",
        ));
    }
    let fit: BTreeMap<_, _> = input
        .observations
        .iter()
        .filter(|view| view.partition == Partition::Fit)
        .map(|view| (&view.frame, &view.camera))
        .collect();
    let mut assigned = BTreeSet::new();
    let mut fit_corners = 0_usize;
    let mut all_corners = 0_usize;
    let mut parsed = Vec::with_capacity(poses.len());
    for pose in poses {
        if !fit.contains_key(&pose.frame) || !assigned.insert(&pose.frame) {
            return Err(invalid(
                "sheet poses must name unique FIT observations, never held-out or unknown frames",
            ));
        }
        let original = pose
            .report
            .get("input")
            .ok_or_else(|| invalid("sheet pose report is missing its original input"))?;
        let source: SheetInput = serde_json::from_value(original.clone())
            .map_err(|error| invalid(format!("invalid retained sheet-pose input: {error}")))?;
        fit_corners += source.points.iter().filter(|point| !point.held_out).count();
        all_corners += source.points.len();
        if fit_corners > MAX_FIT_CORNERS || all_corners > MAX_RETAINED_CORNERS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "sheet-pose propagation allows at most 256 FIT corners and 1024 total retained corners across all frames",
            ));
        }
        parsed.push((pose, source));
    }
    if assigned.len() != fit.len() {
        return Err(invalid(
            "sheet poses must cover every FIT frame exactly once",
        ));
    }
    parsed.sort_by(|(a, _), (b, _)| a.frame.cmp(&b.frame));
    let mut reference_id = None;
    let mut prepared = BTreeMap::new();
    let mut calibration_cache = BTreeMap::new();
    for (pose, source) in parsed {
        if reference_id
            .as_ref()
            .is_some_and(|reference| *reference != source.reference_id)
        {
            return Err(invalid(
                "sheet cameras must use the same exact stationary metric reference_id",
            ));
        }
        reference_id = Some(source.reference_id.clone());
        let replayed = Camera::sheet_pose_report(&pose.report["input"])?;
        let mut saved = pose.report.clone();
        if let Some(annotation) = saved
            .as_object_mut()
            .and_then(|report| report.remove("intrinsics_source"))
        {
            replay_intrinsics_source(annotation, &source, &mut calibration_cache)?;
        }
        if replayed != saved || replayed["ok"] != true {
            return Err(invalid(
                "sheet pose report is changed or unaccepted; replay its complete original acquisition",
            ));
        }
        if serde_json::to_value(fit[&pose.frame]).map_err(|error| invalid(error.to_string()))?
            != replayed["camera"]
        {
            return Err(invalid(
                "measurement camera must exactly match its replayed sheet pose, including the lens and metric frame",
            ));
        }
        let mut fit_point_indices = Vec::new();
        let mut sheet_xz_m = Vec::new();
        let mut pixels = Vec::new();
        for (index, point) in source.points.into_iter().enumerate() {
            if !point.held_out {
                fit_point_indices.push(index);
                sheet_xz_m.push(point.sheet_xz_m);
                pixels.push(point.pixel_px);
            }
        }
        prepared.insert(
            pose.frame.clone(),
            PreparedSheetPose {
                intrinsics: source.intrinsics,
                fit_point_indices,
                sheet_xz_m,
                pixels,
                image_size_px: source.image_size_px,
                rotation_applied_degrees: source.rotation_applied_degrees,
            },
        );
    }
    Ok(PreparedSheetPoses { poses: prepared })
}
