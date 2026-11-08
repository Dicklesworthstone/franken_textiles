//! Prepared multi-frame observations fitted locally with the canonical Anny adapter and solver.
//! File-backed capture acquisition remains native; local review accepts explicit source bytes only.

use std::fmt::Write as _;

use ftx_body::{
    AnnyData, AnnyModel,
    accepted_shape::AcceptedShape,
    correspondence::{Configuration, CorrespondenceModel},
};
use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_measure::{
    body_fit::{
        self, BodyFitInput, BodyModel, CaptureImage, FitOptions, FramePose, ModelLayout, Parameter,
    },
    silhouette::{self, SurfaceMesh},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    model: Configuration,
    fit: BodyFitInput,
    #[serde(default)]
    capture_images: Option<Vec<CaptureImage>>,
}

struct Adapter {
    inner: CorrespondenceModel,
    layout: ModelLayout,
}

impl Adapter {
    fn new(inner: CorrespondenceModel) -> Self {
        let config = inner.configuration();
        let parameters = |source: &[ftx_body::correspondence::Parameter]| {
            source
                .iter()
                .map(|p| Parameter {
                    name: p.name.clone(),
                    lower: p.lower,
                    upper: p.upper,
                })
                .collect()
        };
        let layout = ModelLayout {
            model_id: format!("anny/0.6.1/sha256:{}", config.model_sha256),
            shape: parameters(&config.shape_parameters),
            pose: parameters(&config.pose_parameters),
            keypoints: config.keypoints.keys().cloned().collect(),
            measurements: config.measurements.clone(),
        };
        Self { inner, layout }
    }
}

impl BodyModel for Adapter {
    fn layout(&self) -> &ModelLayout {
        &self.layout
    }
    fn measurements(&self, shape: &[f64]) -> Result<Vec<f64>, Refusal> {
        self.inner.measurements(shape)
    }
    fn points(&self, shape: &[f64], pose: &[f64]) -> Result<Vec<Vec3>, Refusal> {
        self.inner.points(shape, pose)
    }
    fn surface(&self, shape: &[f64], pose: &[f64]) -> Result<SurfaceMesh, Refusal> {
        let posed = self.inner.posed(shape, pose)?;
        Ok(SurfaceMesh {
            vertices_m: posed.vertices,
            triangles: self
                .inner
                .model()
                .data()
                .faces
                .iter()
                .map(|t| t.map(|k| k as usize))
                .collect(),
        })
    }
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// Fit one shared shape and independent frame poses from original prepared JSON and model bytes.
/// The result is the ordinary `ftx-body-fit/1` report, including every failed/held-out/ambiguous
/// attempt and the exact input hash. Only accepted solutions contain reusable Anny parameters.
/// Textual work limits retain native integer parsing without JavaScript numeric truncation.
///
/// # Errors
/// More than 16 MiB input or 512 MiB model data, invalid prepared schema/controls, wrong model
/// identity/arrays, or the canonical solver's evidence and budget admission refusals. A solver
/// `BUDGET_STOP` is a diagnostic report, not an accepted partial shape.
pub fn fit_body(
    model_bytes: &[u8],
    input: &str,
    iterations: &str,
    max_evaluations: &str,
) -> Result<Value, Refusal> {
    if input.len() > 16 * 1024 * 1024 || model_bytes.len() > 512 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "body fit supports at most 16 MiB prepared input and 512 MiB converted model",
        ));
    }
    let request: Request = serde_json::from_str(input)
        .map_err(|e| invalid(format!("invalid Anny body-fit request: {e}")))?;
    if request.schema != "ftx-anny-body-fit-input/1" {
        return Err(invalid(
            "expected prepared ftx-anny-body-fit-input/1; file-backed capture acquisition requires native preparation",
        ));
    }
    if let Some(images) = &request.capture_images {
        body_fit::validate_capture_images(images, &request.fit.frames)?;
    }
    let iterations = iterations
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=100).contains(n))
        .ok_or_else(|| invalid("--iterations must be 1..100"))?;
    let max_model_evaluations = max_evaluations
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=1_000_000).contains(n))
        .ok_or_else(|| invalid("--max-evaluations must be 1..1000000"))?;
    let hash = sha256_hex(model_bytes);
    if hash != request.model.model_sha256 {
        return Err(invalid("model SHA-256 mismatch; no fit was run"));
    }
    let model = AnnyModel::from_data(AnnyData::from_bytes(model_bytes, true)?)?;
    let adapter = Adapter::new(CorrespondenceModel::new(model, request.model, &hash)?);
    let report = body_fit::fit_body(
        &request.fit,
        &adapter,
        &FitOptions {
            iterations,
            max_model_evaluations,
            ..FitOptions::default()
        },
    )?;
    let mut value = serde_json::to_value(&report).map_err(|e| invalid(e.to_string()))?;
    value["anny_configuration"] =
        serde_json::to_value(adapter.inner.configuration()).map_err(|e| invalid(e.to_string()))?;
    value["input_sha256"] = json!(sha256_hex(input.as_bytes()));
    if let Some(images) = request.capture_images {
        value["capture_images"] = json!(images);
    }
    if let Some(solution) = &report.solution {
        let p = adapter.inner.parameters(&solution.shape)?;
        let h = p.phenotype;
        value["solution"]["anny_parameters"] = json!({"provenance":"MODEL_FIT",
            "phenotype":{"gender":h.gender,"age":h.age,"muscle":h.muscle,"weight":h.weight,"height":h.height,"proportions":h.proportions},
            "local_changes":p.local_changes});
    }
    Ok(value)
}

/// Inspect one accepted image pose without fitting again or replacing its saved report.
/// The ordinal addresses the report's native frame order; full-width identities never pass
/// through a JavaScript number. Full-mesh keypoints and supplied silhouette slices are rechecked
/// through the same kernels as native mesh export, including all held-out observations.
///
/// # Errors
/// Oversized input/model, nonaccepted or inconsistent reports, wrong model, invalid frame/pose,
/// excessive mesh size, or a full-mesh projection/contour that fails its original gate.
pub fn review_body_fit_frame(
    model_bytes: &[u8],
    report_text: &str,
    frame_index: &str,
) -> Result<Value, Refusal> {
    if report_text.len() > 16 * 1024 * 1024 || model_bytes.len() > 512 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "body-frame review supports at most 16 MiB report and 512 MiB converted model",
        ));
    }
    let index = frame_index
        .parse::<usize>()
        .ok()
        .filter(|n| *n < 8)
        .ok_or_else(|| invalid("body-frame review index must be an integer in 0..7"))?;
    let report: Value = serde_json::from_str(report_text)
        .map_err(|e| invalid(format!("invalid accepted body-fit report: {e}")))?;
    if report["schema"] != "ftx-body-fit/1"
        || report["status"] != "ACCEPTED"
        || report["flags"].as_array().is_none_or(|v| !v.is_empty())
    {
        return Err(invalid(
            "only an intact ACCEPTED body-fit report can be reviewed",
        ));
    }
    let hash = sha256_hex(model_bytes);
    if report["anny_configuration"]["model_sha256"].as_str() != Some(hash.as_str()) {
        return Err(invalid("model SHA-256 mismatch; no frame was reviewed"));
    }
    let model = AnnyModel::from_data(AnnyData::from_bytes(model_bytes, true)?)?;
    let accepted = AcceptedShape::read(&report, &hash, &model.data().header.labels.local_changes)?;
    let counts = &model.data().header.counts;
    if counts.vertices == 0
        || counts.vertices > 100_000
        || counts.faces == 0
        || counts.faces > 200_000
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "body-frame review needs 1..100000 vertices and 1..200000 faces",
        ));
    }
    let config = serde_json::from_value(report["anny_configuration"].clone())
        .map_err(|e| invalid(format!("invalid accepted Anny configuration: {e}")))?;
    let adapter = Adapter::new(CorrespondenceModel::new(model, config, &hash)?);
    if serde_json::to_value(&adapter.layout).map_err(|e| invalid(e.to_string()))? != report["model"]
    {
        return Err(invalid(
            "accepted body-fit model layout disagrees with its configuration",
        ));
    }
    let input: BodyFitInput = serde_json::from_value(report["input"].clone())
        .map_err(|e| invalid(format!("invalid accepted body-fit observations: {e}")))?;
    body_fit::prepare_input(&input, &adapter.layout)?;
    let explicit_images: Option<Vec<CaptureImage>> = report
        .get("capture_images")
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|e| invalid(format!("invalid accepted capture images: {e}")))
        })
        .transpose()?;
    if let Some(images) = &explicit_images {
        body_fit::validate_capture_images(images, &input.frames)?;
    }
    let legacy_images = report
        .get("capture_input")
        .map(|capture| body_fit::capture_images_from_receipt(capture, &input.frames))
        .transpose()?;
    if let (Some(explicit), Some(legacy)) = (&explicit_images, &legacy_images)
        && explicit.iter().any(|image| !legacy.contains(image))
    {
        return Err(invalid(
            "accepted image bindings disagree with the original capture receipt",
        ));
    }
    let images = explicit_images.or(legacy_images);
    let poses: Vec<FramePose> = serde_json::from_value(report["solution"]["poses"].clone())
        .map_err(|e| invalid(format!("invalid accepted frame poses: {e}")))?;
    let spreads = report["pose_spread_fraction"]
        .as_array()
        .ok_or_else(|| invalid("accepted body-fit pose agreement is missing"))?;
    let limit = report["options"]["max_pose_spread_fraction"]
        .as_f64()
        .filter(|n| *n > 0.0 && *n <= 0.01)
        .ok_or_else(|| invalid("accepted body-fit pose agreement policy is missing or weakened"))?;
    if poses.len() != input.frames.len() || spreads.len() != poses.len() || index >= poses.len() {
        return Err(invalid(
            "accepted frame, pose or review index counts disagree",
        ));
    }
    for ((frame, pose), spread) in input.frames.iter().zip(&poses).zip(spreads) {
        let fractions = spread["fractions"].as_array();
        if frame.frame != pose.frame
            || spread["frame"] != json!(frame.frame)
            || pose.values.len() != adapter.layout.pose.len()
            || pose
                .values
                .iter()
                .zip(&adapter.layout.pose)
                .any(|(v, p)| !(*v > p.lower && *v < p.upper))
            || fractions.is_none_or(|v| {
                v.len() != adapter.layout.pose.len()
                    || v.iter()
                        .any(|f| f.as_f64().is_none_or(|n| !(0.0..=limit).contains(&n)))
            })
        {
            return Err(invalid(
                "accepted frame poses or their multistart agreement are inconsistent",
            ));
        }
    }
    let frame = &input.frames[index];
    let rest = adapter.inner.model().rest(&accepted.params)?;
    let body = adapter
        .inner
        .model()
        .pose(&rest, &adapter.inner.deltas(&poses[index].values)?)?;
    let targets = adapter.inner.points_on_body(&rest, &body)?;
    let keypoints = body_fit::review_keypoints(frame, &adapter.layout.keypoints, &targets)?;
    let faces = &adapter.inner.model().data().faces;
    let mesh = SurfaceMesh {
        vertices_m: body.vertices,
        triangles: faces.iter().map(|f| f.map(|v| v as usize)).collect(),
    };
    let contour = frame
        .silhouette
        .as_ref()
        .map(|source| {
            let comparisons =
                silhouette::compare(&mesh, &frame.camera, frame.image_size_px, source, None)?;
            if comparisons.iter().any(|line| !line.passed) {
                return Err(invalid(
                    "full mesh silhouette disagrees with an accepted fit/held-out scanline gate",
                ));
            }
            let lines: Vec<_> = comparisons
                .iter()
                .map(|comparison| {
                    let observation = source
                        .lines
                        .iter()
                        .find(|line| line.axis == comparison.axis && line.at_px == comparison.at_px)
                        .ok_or_else(|| invalid("missing source silhouette scanline"))?;
                    Ok(json!({"observation":observation,"comparison":comparison}))
                })
                .collect::<Result<_, Refusal>>()?;
            Ok(json!({"evidence":source.evidence,"lines":lines}))
        })
        .transpose()?;
    let mut obj = String::from(
        "# FrankenTextiles accepted MODEL_FIT; metres, common calibrated world frame, +Z up\n\
         # Complete model topology; NOT collision-skin admission or real-wearer fit evidence\n",
    );
    for point in &mesh.vertices_m {
        if !point.is_finite() {
            return Err(invalid("mesh review needs finite posed vertices"));
        }
        writeln!(obj, "v {} {} {}", point.x, point.y, point.z).unwrap();
    }
    for face in faces {
        if face.iter().any(|&i| i as usize >= mesh.vertices_m.len()) {
            return Err(invalid("mesh face index outside the posed vertex array"));
        }
        writeln!(
            obj,
            "f {} {} {}",
            u64::from(face[0]) + 1,
            u64::from(face[1]) + 1,
            u64::from(face[2]) + 1
        )
        .unwrap();
    }
    let pixels: Vec<_> = mesh
        .vertices_m
        .iter()
        .map(|&point| frame.camera.project(point).ok())
        .collect();
    let mut value = json!({"schema":"ftx-body-frame-review/1","report_sha256":sha256_hex(report_text.as_bytes()),
        "model_sha256":hash,"frame_index":index,"frame":frame.frame,
        "frame_label":format!("{} @ {} us",frame.frame.stream,frame.frame.timestamp_us),
        "image_size_px":frame.image_size_px,"rotation_cw_deg":frame.rotation_cw_deg,"camera":frame.camera,
        "keypoints":keypoints,"silhouette":contour,"mesh_obj":obj,"triangles":faces,"projected_vertices_px":pixels,
        "unprojectable_vertices":pixels.iter().filter(|p|p.is_none()).count(),
        "projection_kind":if silhouette::require_rectified(&frame.camera).is_ok(){"straight_edges"}else{"distorted_vertices"},
        "export_pose_evaluations":1,
        "no_claim":"Accepted MODEL_FIT pose reviewed without refitting. Complete topology, not a collision skin. Supplied keypoints and scanlines are rechecked, not full-mask, visibility, occlusion, anatomy or real-wearer fit. Source image identity, when retained, supports separate local inspection."});
    if let Some(images) = images {
        value["source_image"] = json!(
            images
                .iter()
                .find(|image| image.frame == frame.frame)
                .unwrap()
        );
    }
    Ok(value)
}

/// One exact capture photograph decoded for a previously checked frame review.
/// Pixels are kept only in this transient preview, never added to a fit report or garment handoff.
pub struct BodyFitImage {
    preview: Vec<u8>,
    report: Value,
}

impl BodyFitImage {
    /// Check source bytes before decoding and keep the original stored orientation.
    /// The review is an internally consistent local receipt, not an authenticity certificate.
    ///
    /// # Errors
    /// Missing or inconsistent source binding, wrong bytes/dimensions, nonidentity EXIF,
    /// unsupported encoding, more than 128 MiB image / 16 MiB review, or more than 40M pixels.
    pub fn new(bytes: &[u8], review: &str) -> Result<Self, Refusal> {
        #[derive(Deserialize)]
        struct Review {
            schema: String,
            frame: ftx_measure::triangulate::FrameId,
            image_size_px: [u32; 2],
            rotation_cw_deg: u16,
            source_image: CaptureImage,
        }
        if bytes.len() > 128 * 1024 * 1024 || review.len() > 16 * 1024 * 1024 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "capture image review supports at most 128 MiB image and 16 MiB review",
            ));
        }
        let frame: Review = serde_json::from_str(review).map_err(|e| {
            invalid(format!(
                "frame review has no valid capture image binding: {e}"
            ))
        })?;
        let source = &frame.source_image;
        source.validate()?;
        if frame.schema != "ftx-body-frame-review/1"
            || source.frame != frame.frame
            || source.image_size_px != frame.image_size_px
            || source.rotation_cw_deg != frame.rotation_cw_deg
        {
            return Err(invalid(
                "capture image binding disagrees with the selected frame review",
            ));
        }
        if sha256_hex(bytes) != source.image_sha256 {
            return Err(invalid(
                "capture image SHA-256 mismatch; select the original encoded frame",
            ));
        }
        let image = ftx_trace::raster::image::load_capture_image_with_limit(bytes, 40_000_000)?;
        if [image.w as u32, image.h as u32] != source.image_size_px {
            return Err(invalid(
                "capture image dimensions disagree with the original oriented frame",
            ));
        }
        let preview = ftx_trace::raster::image::encode_png_bgr_with_limit(&image, 40_000_000)
            .map_err(invalid)?;
        let report = json!({"schema":"ftx-body-frame-image/1",
            "review_sha256":sha256_hex(review.as_bytes()),"source_image":source,
            "preview_sha256":sha256_hex(&preview),"orientation":"already_applied_stored_pixels",
            "no_claim":"Original encoded image bytes and dimensions match the retained frame. No refitting, camera calibration, anatomy, visibility or physical-fit acceptance was performed."});
        Ok(Self { preview, report })
    }

    /// Lossless PNG of the original stored pixels, with no second rotation or lens transform.
    #[must_use]
    pub fn preview(&self) -> &[u8] {
        &self.preview
    }

    /// Exact image/review identities; contains no image bytes or file access instructions.
    #[must_use]
    pub fn report(&self) -> &Value {
        &self.report
    }
}
