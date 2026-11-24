//! Source-bound still photographs become the ordinary unfinished body-fit input.
//! Cameras use the native calibrated sheet authority; anatomy and starts remain explicit.

use std::collections::{BTreeMap, BTreeSet};

use ftx_body::{
    AnnyData, AnnyModel,
    correspondence::{
        CorrespondenceModel,
        capture::{ModelChoices, catalogue},
    },
};
use ftx_core::{Refusal, RefusalCode, Vec2, receipt::sha256_hex};
use ftx_measure::{
    body_fit::{self, BodyFitInput, BodyModel, CaptureImage, FitFrame, MetricObservation},
    camera::{Camera, Intrinsics, rectification::Rectification},
    photo::{
        MAX_PHOTO_PIXELS, capture::StationarySheet, detect_charuco, intrinsics::checked_calibration,
    },
    triangulate::FrameId,
};
use ftx_trace::raster::image::{Bgr, encode_png_bgr, load_photo_with_limit};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Adapter, invalid};

const MAX_JSON: usize = 16 * 1024 * 1024;
const MAX_MODEL: usize = 512 * 1024 * 1024;
const MAX_DECLARATION: usize = 256 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    images: Vec<String>,
    #[serde(default)]
    rectify: bool,
    lens_id: String,
    rotation_cw_deg: u16,
    reference_id: String,
    held_out_corner_ids: Vec<usize>,
    board_bottom_left_xz_m: Vec2,
    stationary_upright_evidence: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationOverrides {
    schema: String,
    overrides: Vec<CalibrationOverride>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationOverride {
    image: String,
    lens_id: String,
    rotation_cw_deg: u16,
    calibration_json: String,
}

#[derive(Clone)]
struct CaptureCalibration {
    lens_id: String,
    rotation_cw_deg: u16,
    calibration_sha256: String,
    source: &'static str,
    intrinsics: Intrinsics,
    image_size_px: [u32; 2],
    rectification: Option<Rectification>,
}

impl CaptureCalibration {
    fn checked(
        calibration: &Value,
        calibration_text: &str,
        lens_id: &str,
        rotation_cw_deg: u16,
        rectify: bool,
        source: &'static str,
    ) -> Result<Self, Refusal> {
        if lens_id.trim().is_empty()
            || lens_id.len() > 256
            || !matches!(rotation_cw_deg, 0 | 90 | 180 | 270)
        {
            return Err(invalid(
                "capture calibration needs an exact lens ID and already-applied rotation",
            ));
        }
        let checked = checked_calibration(calibration)?;
        if checked.input.lens_id != lens_id || checked.input.rotation_cw_deg != rotation_cw_deg {
            return Err(invalid(
                "capture lens and already-applied rotation must match the checked calibration",
            ));
        }
        let image_size_px = checked.input.image_size_px;
        if u64::from(image_size_px[0]) * u64::from(image_size_px[1]) > MAX_PHOTO_PIXELS as u64 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "capture permits 16 million pixels per image and 128 million pixels total",
            ));
        }
        let intrinsics = checked
            .solution
            .as_ref()
            .ok_or_else(|| invalid("checked calibration has no solution"))?
            .intrinsics;
        let rectification = rectify
            .then(|| Rectification::new(intrinsics, image_size_px))
            .transpose()?;
        Ok(Self {
            lens_id: lens_id.into(),
            rotation_cw_deg,
            calibration_sha256: sha256_hex(calibration_text.as_bytes()),
            source,
            intrinsics,
            image_size_px,
            rectification,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choices {
    schema: String,
    model_sha256: String,
    model: ModelChoices,
    metric_scale_source: String,
    observation_evidence: String,
    #[serde(default)]
    measurements: Vec<MetricObservation>,
}

fn parse<T: serde::de::DeserializeOwned>(text: &str, label: &str) -> Result<T, Refusal> {
    if text.len() > MAX_JSON {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("{label} JSON exceeds 16 MiB"),
        ));
    }
    serde_json::from_str(text).map_err(|error| invalid(format!("invalid {label}: {error}")))
}

fn model(bytes: &[u8]) -> Result<AnnyModel, Refusal> {
    if bytes.len() > MAX_MODEL {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "capture preparation supports at most 512 MiB converted model",
        ));
    }
    AnnyModel::from_data(AnnyData::from_bytes(bytes, true)?)
}

/// List actual model controls for explicit first-capture choices, without posing or fitting.
///
/// # Errors
/// Oversized, invalid or array-unverified converted model bytes.
pub fn body_capture_catalogue(model_bytes: &[u8]) -> Result<Value, Refusal> {
    let source = model(model_bytes)?;
    Ok(
        json!({"schema":"ftx-body-capture-catalogue/1","model_sha256":sha256_hex(model_bytes),
        "body_fitted":false,"catalogue":catalogue(&source)}),
    )
}

/// A frozen batch of two to eight still photos with checked lenses and one stationary board.
/// Successful additions retain source identities and camera diagnostics. Opt-in rectification
/// retains one derived PNG until it is taken; decoded pixels are released after each addition.
/// Each still image has its own source stream and zero clock origin; no capture time is inferred.
pub struct BodyCapture {
    manifest: Manifest,
    board: StationarySheet,
    sheet: Value,
    calibrations: Vec<CaptureCalibration>,
    pending_image: Option<Vec<u8>>,
    sources: Value,
    frames: Vec<FitFrame>,
    images: Vec<CaptureImage>,
    receipts: Vec<Value>,
    pixels: BTreeSet<String>,
    rectified_pixels: BTreeSet<String>,
}

impl BodyCapture {
    /// Freeze exact declarations, original calibration and sheet before the first detector call.
    /// The saved ACCEPTED calibration is recomputed from its original observations.
    ///
    /// # Errors
    /// Invalid/oversized JSON, unsupported schema, duplicate/path-like image names, lens or
    /// orientation mismatch, invalid board/CHECK declaration, or excessive decoded-pixel budget.
    pub fn new(
        manifest_text: &str,
        calibration_text: &str,
        sheet_text: &str,
    ) -> Result<Self, Refusal> {
        Self::prepare(manifest_text, calibration_text, sheet_text, None)
    }

    /// Freeze exact per-image calibration overrides alongside the declared shared default.
    /// Manifest `/2` and `ftx-body-photo-calibrations/1` explicitly select this path; every
    /// original ACCEPTED report is checked before any image is decoded. Unlisted images use
    /// the manifest's default lens. Invalid overrides never fall back to that default.
    ///
    /// # Errors
    /// Unknown/duplicate image names, empty overrides, lens/orientation mismatches, invalid
    /// original calibrations, unsupported schemas or aggregate source/pixel budgets.
    pub fn with_calibrations(
        manifest_text: &str,
        calibration_text: &str,
        sheet_text: &str,
        overrides_text: &str,
    ) -> Result<Self, Refusal> {
        Self::prepare(
            manifest_text,
            calibration_text,
            sheet_text,
            Some(overrides_text),
        )
    }

    fn prepare(
        manifest_text: &str,
        calibration_text: &str,
        sheet_text: &str,
        overrides_text: Option<&str>,
    ) -> Result<Self, Refusal> {
        if manifest_text.len() > MAX_DECLARATION
            || sheet_text.len() > MAX_DECLARATION
            || calibration_text.len() > MAX_JSON
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "capture manifest and sheet permit 256 KiB each; calibration permits 16 MiB",
            ));
        }
        if let Some(overrides) = overrides_text
            && (overrides.len() > MAX_JSON
                || manifest_text.len()
                    + calibration_text.len()
                    + sheet_text.len()
                    + overrides.len()
                    > MAX_JSON)
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "combined capture source documents exceed 16 MiB",
            ));
        }
        let manifest: Manifest = parse(manifest_text, "body photo capture")?;
        let calibration: Value = parse(calibration_text, "capture calibration")?;
        let sheet: Value = parse(sheet_text, "capture sheet")?;
        let mut names = BTreeSet::new();
        let schema = if overrides_text.is_some() {
            "ftx-body-photo-capture/2"
        } else {
            "ftx-body-photo-capture/1"
        };
        if manifest.schema != schema
            || !(2..=8).contains(&manifest.images.len())
            || manifest.images.iter().any(|name| {
                name.trim().is_empty()
                    || name.len() > 4096
                    || name.contains(['/', '\\', '\0'])
                    || matches!(name.as_str(), "." | "..")
                    || !names.insert(name.clone())
            })
            || manifest.lens_id.trim().is_empty()
            || manifest.lens_id.len() > 256
            || manifest.reference_id.trim().is_empty()
            || manifest.reference_id.len() > 256
            || !matches!(manifest.rotation_cw_deg, 0 | 90 | 180 | 270)
        {
            return Err(invalid(
                "capture needs schema /1 for a shared lens or /2 with explicit calibration overrides, two to eight unique image basenames, explicit lens/reference IDs and already-applied rotation",
            ));
        }
        let default = CaptureCalibration::checked(
            &calibration,
            calibration_text,
            &manifest.lens_id,
            manifest.rotation_cw_deg,
            manifest.rectify,
            "shared_default",
        )?;
        let mut overrides = BTreeMap::new();
        if let Some(text) = overrides_text {
            let declared: CalibrationOverrides = parse(text, "capture calibration overrides")?;
            if declared.schema != "ftx-body-photo-calibrations/1"
                || declared.overrides.is_empty()
                || declared.overrides.len() > manifest.images.len()
            {
                return Err(invalid(
                    "capture overrides need schema /1 and one to eight unique declared image bindings",
                ));
            }
            for entry in declared.overrides {
                if !names.contains(&entry.image) || overrides.contains_key(&entry.image) {
                    return Err(invalid(
                        "capture calibration overrides must name each selected image at most once, exactly",
                    ));
                }
                let calibration = parse(&entry.calibration_json, "image calibration")?;
                let checked = CaptureCalibration::checked(
                    &calibration,
                    &entry.calibration_json,
                    &entry.lens_id,
                    entry.rotation_cw_deg,
                    manifest.rectify,
                    "image_override",
                )?;
                overrides.insert(entry.image, checked);
            }
        }
        let calibrations: Vec<_> = manifest
            .images
            .iter()
            .map(|name| overrides.remove(name).unwrap_or_else(|| default.clone()))
            .collect();
        let pixel_count: u64 = calibrations
            .iter()
            .map(|calibration| {
                u64::from(calibration.image_size_px[0]) * u64::from(calibration.image_size_px[1])
            })
            .sum();
        if pixel_count > 128_000_000 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "capture permits 16 million pixels per image and 128 million pixels total",
            ));
        }
        let board = StationarySheet::new(
            &sheet,
            &manifest.held_out_corner_ids,
            manifest.board_bottom_left_xz_m,
            &manifest.stationary_upright_evidence,
        )?;
        let mut sources = json!({"manifest":parse::<Value>(manifest_text,"capture manifest")?,
            "manifest_sha256":sha256_hex(manifest_text.as_bytes()),
            "calibration":calibration,"calibration_sha256":sha256_hex(calibration_text.as_bytes()),
            "sheet":sheet,"sheet_sha256":sha256_hex(sheet_text.as_bytes())});
        if let Some(text) = overrides_text {
            sources["calibration_overrides"] = parse(text, "capture calibration overrides")?;
            sources["calibration_overrides_json"] = json!(text);
            sources["calibration_overrides_sha256"] = json!(sha256_hex(text.as_bytes()));
        }
        if serde_json::to_vec(&sources)
            .map_err(|error| invalid(error.to_string()))?
            .len()
            > MAX_JSON
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "combined capture source documents exceed 16 MiB",
            ));
        }
        Ok(Self {
            manifest,
            board,
            sheet,
            calibrations,
            pending_image: None,
            sources,
            frames: Vec::new(),
            images: Vec::new(),
            receipts: Vec::new(),
            pixels: BTreeSet::new(),
            rectified_pixels: BTreeSet::new(),
        })
    }

    /// Exact image labels in the original manifest order; every image must be supplied once.
    #[must_use]
    pub fn image_ids(&self) -> Vec<String> {
        self.manifest.images.clone()
    }

    /// Decode one original photo and reconstruct its camera from all detected board corners.
    /// Failed additions commit no state; the same next image can be retried.
    ///
    /// # Errors
    /// Unknown/repeated/out-of-order source, invalid image, unapplied EXIF rotation, wrong size,
    /// duplicate decoded pixels, detector or calibrated-pose refusal, a missing/failed CHECK,
    /// or a previous rectified PNG that has not been taken.
    pub fn add_image(&mut self, id: &str, bytes: &[u8]) -> Result<Value, Refusal> {
        if self.pending_image.is_some() {
            return Err(invalid(
                "take the previous rectified image before adding another capture image",
            ));
        }
        let next = self
            .manifest
            .images
            .get(self.frames.len())
            .ok_or_else(|| invalid("all declared capture images have already been added"))?;
        if next != id {
            return Err(invalid(format!(
                "expected capture image {next}, received {id}; use original manifest order"
            )));
        }
        let calibration = &self.calibrations[self.frames.len()];
        let image_size_px = calibration.image_size_px;
        let rotation_cw_deg = calibration.rotation_cw_deg;
        let intrinsics = calibration.intrinsics;
        let (image, decoded) = load_photo_with_limit(bytes, MAX_PHOTO_PIXELS)?;
        if [image.w, image.h] != image_size_px.map(|n| n as usize)
            || decoded["exif_orientation"]
                .as_u64()
                .is_some_and(|orientation| orientation != 1)
        {
            return Err(invalid(
                "capture images must retain the calibrated dimensions and already-oriented stored pixels; apply EXIF rotation to pixels and calibration before capture",
            ));
        }
        let pixels_sha256 = sha256_hex(&image.data);
        if self.pixels.contains(&pixels_sha256) {
            return Err(invalid(
                "duplicate decoded capture image: a renamed or re-encoded photo is not another view",
            ));
        }
        let detected =
            detect_charuco(&image.data, image.w, image.h, &self.sheet, Some(intrinsics))?;
        let mut reference = self.board.reference(
            &detected,
            &self.manifest.reference_id,
            id,
            [image.w, image.h],
            rotation_cw_deg,
        )?;
        reference["intrinsics"] = json!(intrinsics);
        let pose = Camera::sheet_pose_report(&reference)?;
        if pose["ok"] != true || pose["held_out_points"].as_u64().unwrap_or(0) == 0 {
            return Err(invalid(format!(
                "capture sheet pose needs passing FIT and predeclared CHECK points; reprojection diagnostics: {}",
                pose["reprojection"]
            )));
        }
        let original_camera: Camera = serde_json::from_value(pose["camera"].clone())
            .map_err(|error| invalid(format!("invalid checked sheet camera: {error}")))?;
        let image_sha256 = sha256_hex(bytes);
        let frame = FrameId {
            stream: format!("still-image:{image_sha256}"),
            timestamp_us: 0,
        };
        let mut source = CaptureImage {
            frame: frame.clone(),
            image_name: id.into(),
            image_sha256: image_sha256.clone(),
            image_size_px,
            rotation_cw_deg,
        };
        let mut receipt = json!({"frame":frame,"image":id,"image_sha256":image_sha256,
            "pixels_bgr_sha256":pixels_sha256,"image_input":decoded,
            "sheet_pose":pose,"detection":detected});
        if self.manifest.schema == "ftx-body-photo-capture/2" {
            receipt["calibration"] = json!({
                "lens_id":calibration.lens_id,"rotation_cw_deg":rotation_cw_deg,
                "image_size_px":image_size_px,"calibration_sha256":calibration.calibration_sha256,
                "source":calibration.source,
            });
        }
        // Board admission belongs to the original pixels and calibrated lens. Rectification
        // only changes image coordinates; it never refits the accepted metric camera pose.
        let mut camera = original_camera;
        let mut pending_image = None;
        let mut rectified_pixels_sha256 = None;
        if let Some(rectification) = &calibration.rectification {
            let size = rectification.image_size_px();
            let rectified = Bgr {
                w: size[0] as usize,
                h: size[1] as usize,
                data: rectification.rectify_bgr(&image.data)?,
            };
            let pixels = sha256_hex(&rectified.data);
            if self.rectified_pixels.contains(&pixels) {
                return Err(invalid(
                    "duplicate rectified capture pixels: differences outside the retained crop do not provide another authored view",
                ));
            }
            let png = encode_png_bgr(&rectified).map_err(invalid)?;
            source.image_name = format!("rectified-{image_sha256}.png");
            source.image_sha256 = sha256_hex(&png);
            source.image_size_px = size;
            camera.intrinsics = rectification.intrinsics();
            receipt["rectification"] = json!({
                "schema":"ftx-capture-rectification/1",
                "source_camera":original_camera,
                "source_image_size_px":image_size_px,
                "crop_origin_px":rectification.crop_origin_px(),
                "image_size_px":size,
                "camera":camera,
                "image":source,
                "pixels_bgr_sha256":pixels,
                "sampling":"Bilinear interpolation of encoded BGR samples at integer destination pixel centres; no extrapolation, padding, resizing or additional rotation.",
                "no_claim":"Derived coordinates retain the original accepted sheet pose and lens domain. No new independent camera evidence, anatomical contour, measurement accuracy or calibration-uncertainty propagation is established.",
            });
            pending_image = Some(png);
            rectified_pixels_sha256 = Some(pixels);
        }
        source.validate()?;
        self.frames.push(FitFrame {
            frame,
            camera,
            image_size_px: source.image_size_px,
            rotation_cw_deg,
            keypoints: Vec::new(),
            silhouette: None,
            fixed_pose: None,
        });
        self.images.push(source);
        self.receipts.push(receipt.clone());
        self.pixels.insert(pixels_sha256);
        if let Some(pixels) = rectified_pixels_sha256 {
            self.rectified_pixels.insert(pixels);
        }
        self.pending_image = pending_image;
        Ok(receipt)
    }

    /// Take the exact derived PNG for the most recently added image, releasing its retained bytes.
    /// The addition receipt's rectification image record supplies its name, hash and dimensions.
    /// Empty means rectification was disabled or this image has already been taken.
    #[must_use]
    pub fn take_image(&mut self) -> Vec<u8> {
        self.pending_image.take().unwrap_or_default()
    }

    /// Construct the ordinary prepared input with source-bound cameras and explicit model choices.
    /// Empty anatomical targets, observations and starts are unfinished authoring, never a fit.
    ///
    /// # Errors
    /// Incomplete capture, changed/invalid model bytes, malformed choices, out-of-domain controls,
    /// missing evidence, source-image mismatch or the existing body-fit authoring/budget guards.
    pub fn finish(&self, model_bytes: &[u8], choices_text: &str) -> Result<Value, Refusal> {
        if self.pending_image.is_some() {
            return Err(invalid(
                "take the final rectified image before constructing the prepared input",
            ));
        }
        if self.frames.len() != self.manifest.images.len() {
            return Err(invalid(
                "add every declared capture image before constructing the prepared input",
            ));
        }
        if choices_text.len() > MAX_DECLARATION || model_bytes.len() > MAX_MODEL {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "capture preparation permits 256 KiB choices and 512 MiB converted model",
            ));
        }
        let choices: Choices = parse(choices_text, "body photo choices")?;
        let model_sha256 = sha256_hex(model_bytes);
        if choices.schema != "ftx-body-photo-choices/1" || choices.model_sha256 != model_sha256 {
            return Err(invalid(
                "capture choices require schema /1 and the exact converted model SHA-256",
            ));
        }
        let measurements: Vec<_> = choices
            .measurements
            .iter()
            .map(|observation| observation.name.clone())
            .collect();
        let configuration = choices.model.configuration(&model_sha256, &measurements)?;
        let adapter = Adapter::new(CorrespondenceModel::for_target_review(
            model(model_bytes)?,
            configuration,
            &model_sha256,
        )?);
        let fit = BodyFitInput {
            schema: "ftx-body-fit-input/1".into(),
            metric_scale_source: choices.metric_scale_source,
            observation_evidence: choices.observation_evidence,
            frames: self.frames.clone(),
            measurements: choices.measurements,
            starts: Vec::new(),
        };
        body_fit::prepare_capture_authoring(&fit, adapter.layout())?;
        body_fit::validate_capture_images(&self.images, &fit.frames)?;
        let input = serde_json::to_string_pretty(&json!({"schema":"ftx-anny-body-fit-input/1",
            "model":adapter.inner.configuration(),"fit":fit,"capture_images":self.images}))
        .map_err(|error| invalid(error.to_string()))?;
        if input.len() > MAX_JSON {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "prepared body input exceeds 16 MiB",
            ));
        }
        let mut acquisition = self.sources.clone();
        acquisition["frames"] = json!(self.receipts);
        acquisition["world_frame"] = json!(
            "metres; upright stationary board at y=0; printed right/up are +x/+z; explicit outer bottom-left"
        );
        acquisition["frame_identity"] = json!(
            "Each encoded still image has stream still-image:<SHA-256> and its own zero clock origin. Timestamp zero is an identity sentinel, not capture time, simultaneity, synchronization or a video stillness assertion."
        );
        acquisition["corner_selection"] = json!(
            "all detected corners; every predeclared CHECK required; no pose-dependent trimming"
        );
        let schema = if self.manifest.schema == "ftx-body-photo-capture/2" {
            "ftx-body-photo-capture-result/3"
        } else if self.manifest.rectify {
            "ftx-body-photo-capture-result/2"
        } else {
            "ftx-body-photo-capture-result/1"
        };
        let report = json!({"schema":schema,"status":"CAPTURE_DRAFT",
            "body_fitted":false,"input_sha256":sha256_hex(input.as_bytes()),"input":input,
            "model_sha256":model_sha256,"start_count":0,"frame_count":self.frames.len(),
            "acquisition":acquisition,"choices":parse::<Value>(choices_text,"body photo choices")?,
            "choices_sha256":sha256_hex(choices_text.as_bytes()),
            "no_claim":"Cameras are conditional on the checked lens and caller-declared upright stationary board. Explicit model controls supply no anatomy, visibility, observability, accepted body, READY measurements or physical garment fit. Author model targets, photo observations and distinct starts before fitting. Calibration uncertainty is retained in its source report but is not propagated into body fitting."});
        if serde_json::to_vec(&report)
            .map_err(|error| invalid(error.to_string()))?
            .len()
            > MAX_JSON
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "capture preparation report exceeds 16 MiB",
            ));
        }
        Ok(report)
    }
}
