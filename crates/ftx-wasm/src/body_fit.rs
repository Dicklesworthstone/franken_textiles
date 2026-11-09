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
        self, BodyFitInput, BodyModel, CaptureImage, FitOptions, FitStart, FramePose, ModelLayout,
        Parameter,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartEdits {
    #[serde(default)]
    input_sha256: Option<String>,
    #[serde(default)]
    model_sha256: Option<String>,
    starts: Vec<StartValues>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartValues {
    shape: Vec<f64>,
    poses: Vec<PoseValues>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PoseValues {
    frame_index: usize,
    values: Vec<f64>,
}

/// Prepare named initialization controls, or replace starts in an otherwise unchanged input.
/// Empty edit text retains the original input bytes. Edits use original frame ordinals; native
/// identities never pass through JavaScript numbers. One start is an inspectable draft, while
/// the ordinary fitter still requires two to four distinct initializations.
/// Source hashes may be omitted together for older callers; when supplied, both must match the
/// exact original input and model bytes. The browser always supplies this binding.
///
/// # Errors
/// Oversized input/model/edits, wrong model identity or edit binding, malformed observations or fields,
/// incomplete frame poses, duplicate starts, or values outside the original exclusive bounds.
pub fn body_fit_starts(
    model_bytes: &[u8],
    input_text: &str,
    edits_text: &str,
) -> Result<Value, Refusal> {
    if input_text.len() > 16 * 1024 * 1024
        || model_bytes.len() > 512 * 1024 * 1024
        || edits_text.len() > 64 * 1024
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "initialization authoring supports at most 16 MiB input, 512 MiB model and 64 KiB edits",
        ));
    }
    let mut request: Request = serde_json::from_str(input_text)
        .map_err(|error| invalid(format!("invalid prepared initialization input: {error}")))?;
    if request.schema != "ftx-anny-body-fit-input/1" {
        return Err(invalid(
            "initialization authoring requires prepared ftx-anny-body-fit-input/1",
        ));
    }
    if let Some(images) = &request.capture_images {
        body_fit::validate_capture_images(images, &request.fit.frames)?;
    }
    let source_hash = sha256_hex(input_text.as_bytes());
    let hash = sha256_hex(model_bytes);
    if request.model.model_sha256 != hash {
        return Err(invalid(
            "model SHA-256 mismatch; no initialization was edited",
        ));
    }
    let model = AnnyModel::from_data(AnnyData::from_bytes(model_bytes, true)?)?;
    let adapter = Adapter::new(CorrespondenceModel::new(model, request.model, &hash)?);
    body_fit::prepare_start_authoring(&request.fit, adapter.layout())?;
    let input = if edits_text.trim().is_empty() {
        input_text.to_owned()
    } else {
        let edits: StartEdits = serde_json::from_str(edits_text)
            .map_err(|error| invalid(format!("invalid initialization edits: {error}")))?;
        match (edits.input_sha256.as_deref(), edits.model_sha256.as_deref()) {
            (None, None) => {}
            (Some(input), Some(model)) if input == source_hash && model == hash => {}
            (Some(_), Some(_)) => {
                return Err(invalid(
                    "initialization edits belong to different input or model bytes",
                ));
            }
            _ => {
                return Err(invalid(
                    "initialization edits must supply both input_sha256 and model_sha256",
                ));
            }
        }
        if !(1..=4).contains(&edits.starts.len()) {
            return Err(invalid("initialization edits need one to four starts"));
        }
        request.fit.starts = edits
            .starts
            .into_iter()
            .map(|start| {
                if start.poses.len() != request.fit.frames.len() {
                    return Err(invalid(
                        "each start needs exactly one pose per source frame",
                    ));
                }
                let mut values = vec![None; request.fit.frames.len()];
                for pose in start.poses {
                    let slot = values
                        .get_mut(pose.frame_index)
                        .ok_or_else(|| invalid("start pose frame_index is out of range"))?;
                    if slot.replace(pose.values).is_some() {
                        return Err(invalid("duplicate start pose frame_index"));
                    }
                }
                let poses = request
                    .fit
                    .frames
                    .iter()
                    .zip(values)
                    .map(|(frame, values)| {
                        Ok(FramePose {
                            frame: frame.frame.clone(),
                            values: values
                                .ok_or_else(|| invalid("missing start pose frame_index"))?,
                        })
                    })
                    .collect::<Result<_, Refusal>>()?;
                Ok(FitStart {
                    shape: start.shape,
                    poses,
                })
            })
            .collect::<Result<_, Refusal>>()?;
        body_fit::prepare_start_authoring(&request.fit, adapter.layout())?;
        // Preserve every other authored JSON value, including exact u64 source identities and
        // original frame order. Do not substitute the canonically sorted fitting representation.
        let mut value: Value = serde_json::from_str(input_text)
            .map_err(|error| invalid(format!("invalid initialization source: {error}")))?;
        value["fit"]["starts"] = json!(request.fit.starts);
        let edited = serde_json::to_string_pretty(&value).map_err(|error| {
            invalid(format!("cannot serialize edited initializations: {error}"))
        })?;
        if edited.len() > 16 * 1024 * 1024 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "edited prepared initialization input exceeds 16 MiB",
            ));
        }
        edited
    };
    let frames: Vec<_> = request
        .fit
        .frames
        .iter()
        .enumerate()
        .map(|(frame_index, frame)| {
            json!({"frame_index":frame_index,
                "label":format!("Frame {}: {} @ {} us", frame_index + 1,
                    frame.frame.stream, frame.frame.timestamp_us),
                "identity":{"stream":frame.frame.stream,
                    "timestamp_us":frame.frame.timestamp_us.to_string()}})
        })
        .collect();
    let starts = request
        .fit
        .starts
        .iter()
        .map(|start| {
            let poses = request
                .fit
                .frames
                .iter()
                .enumerate()
                .map(|(frame_index, frame)| {
                    let pose = start
                        .poses
                        .iter()
                        .find(|pose| pose.frame == frame.frame)
                        .ok_or_else(|| invalid("start is missing its source frame pose"))?;
                    Ok(json!({"frame_index":frame_index,"values":pose.values}))
                })
                .collect::<Result<Vec<_>, Refusal>>()?;
            Ok(json!({"shape":start.shape,"poses":poses}))
        })
        .collect::<Result<Vec<_>, Refusal>>()?;
    let shape: Vec<_> = adapter
        .layout
        .shape
        .iter()
        .map(|p| json!({"name":p.name,"lower":p.lower,"upper":p.upper,"unit":"dimensionless"}))
        .collect();
    let pose: Vec<_> = adapter
        .layout
        .pose
        .iter()
        .map(|p| {
            let unit = if matches!(p.name.as_str(), "root_x_m" | "root_y_m" | "root_z_m") {
                "m"
            } else {
                "rad"
            };
            json!({"name":p.name,"lower":p.lower,"upper":p.upper,"unit":unit})
        })
        .collect();
    Ok(
        json!({"schema":"ftx-body-starts-review/1","status":"INITIALIZATION_REVIEW",
        "body_fitted":false,"original_input_sha256":source_hash,
        "input_sha256":sha256_hex(input.as_bytes()),"model_sha256":hash,
        "shape":shape,"pose":pose,"frames":frames,"starts":starts,"input":input}),
    )
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

/// Inspect one declared initialization with the real model, without fitting or acceptance.
/// Frame ordinals address the original prepared request, while canonical admission joins poses
/// by exact frame identity. A one-start draft may be inspected; ordinary fitting still needs
/// two to four distinct starts. Failed projections and large residuals remain diagnostic data.
///
/// # Errors
/// Oversized inputs, invalid ordinals, wrong model identity, malformed observations/starts,
/// out-of-domain parameters or mesh bounds. A model pose refusal is retained in the preview.
pub fn review_body_fit_start(
    model_bytes: &[u8],
    input_text: &str,
    start_index: &str,
    frame_index: &str,
) -> Result<Value, Refusal> {
    if input_text.len() > 16 * 1024 * 1024 || model_bytes.len() > 512 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "initialization review supports at most 16 MiB input and 512 MiB converted model",
        ));
    }
    let ordinal = |text: &str, count, label| {
        text.parse::<usize>()
            .ok()
            .filter(|i| *i < count)
            .ok_or_else(|| {
                invalid(format!(
                    "initialization {label} index must be an integer in 0..{}",
                    count - 1
                ))
            })
    };
    let start_index = ordinal(start_index, 4, "start")?;
    let frame_index = ordinal(frame_index, 8, "frame")?;
    let request: Request = serde_json::from_str(input_text)
        .map_err(|error| invalid(format!("invalid prepared initialization input: {error}")))?;
    if request.schema != "ftx-anny-body-fit-input/1" {
        return Err(invalid(
            "initialization review requires prepared ftx-anny-body-fit-input/1",
        ));
    }
    if let Some(images) = &request.capture_images {
        body_fit::validate_capture_images(images, &request.fit.frames)?;
    }
    let selected_frame = request
        .fit
        .frames
        .get(frame_index)
        .ok_or_else(|| invalid("selected initialization frame is missing"))?
        .frame
        .clone();
    let hash = sha256_hex(model_bytes);
    if request.model.model_sha256 != hash {
        return Err(invalid(
            "model SHA-256 mismatch; no initialization was reviewed",
        ));
    }
    let model = AnnyModel::from_data(AnnyData::from_bytes(model_bytes, true)?)?;
    let counts = &model.data().header.counts;
    if counts.vertices == 0
        || counts.vertices > 100_000
        || counts.faces == 0
        || counts.faces > 200_000
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "initialization review needs 1..100000 vertices and 1..200000 faces",
        ));
    }
    let adapter = Adapter::new(CorrespondenceModel::new(model, request.model, &hash)?);
    let input = body_fit::prepare_start_authoring(&request.fit, adapter.layout())?;
    let canonical_index = input
        .frames
        .iter()
        .position(|frame| frame.frame == selected_frame)
        .ok_or_else(|| invalid("initialization frame identity was not preserved"))?;
    let frame = &input.frames[canonical_index];
    let start = input.starts.get(start_index).ok_or_else(|| {
        invalid("selected initialization start is missing; author a start before previewing")
    })?;
    let pose = &start.poses[canonical_index];
    let (mesh, targets) = match adapter.inner.posed_with_points(&start.shape, &pose.values) {
        Ok((body, targets)) => (
            Ok(SurfaceMesh {
                vertices_m: body.vertices,
                triangles: adapter
                    .inner
                    .model()
                    .data()
                    .faces
                    .iter()
                    .map(|face| face.map(|i| i as usize))
                    .collect(),
            }),
            Ok(targets),
        ),
        Err(error) => (Err(error.clone()), Err(error)),
    };
    let keypoints = body_fit::preview_keypoints(
        frame,
        &adapter.layout.keypoints,
        targets.as_ref().map(Vec::as_slice),
    )?;
    let mut diagnostics = Vec::new();
    if let Err(error) = &mesh {
        diagnostics.push(json!({"stage":"model_surface","refusal":error}));
    }
    if let Err(error) = &targets {
        diagnostics.push(json!({"stage":"model_targets","refusal":error}));
    }
    let mut mesh_obj = None;
    let mut pixels = Vec::new();
    let mut triangles = Vec::new();
    let mut omitted_faces = 0usize;
    if let Ok(mesh) = &mesh {
        if mesh.vertices_m.is_empty()
            || mesh.vertices_m.iter().any(|point| !point.is_finite())
            || mesh
                .triangles
                .iter()
                .flatten()
                .any(|&i| i >= mesh.vertices_m.len())
        {
            return Err(invalid(
                "initialization mesh has invalid vertices or face indices",
            ));
        }
        let projected: Vec<_> = mesh
            .vertices_m
            .iter()
            .map(|&point| frame.camera.project(point))
            .collect();
        if let Some(error) = projected.iter().find_map(|point| point.as_ref().err()) {
            diagnostics.push(json!({"stage":"surface_projection","refusal":error,
                "unprojectable_vertices":projected.iter().filter(|point| point.is_err()).count()}));
        }
        pixels = projected.into_iter().map(Result::ok).collect();
        triangles = mesh.triangles.clone();
        if silhouette::require_rectified(&frame.camera).is_ok() {
            omitted_faces = triangles
                .iter()
                .filter(|face| face.iter().any(|&i| pixels[i].is_none()))
                .count();
        }
        let mut obj = String::from(
            "# FrankenTextiles declared INITIALIZATION; metres, calibrated world frame, +Z up\n\
             # No optimization, accepted body, collision-skin admission or real-wearer fit claim\n",
        );
        for point in &mesh.vertices_m {
            writeln!(obj, "v {} {} {}", point.x, point.y, point.z).unwrap();
        }
        for face in &triangles {
            writeln!(obj, "f {} {} {}", face[0] + 1, face[1] + 1, face[2] + 1).unwrap();
        }
        mesh_obj = Some(obj);
    }
    let contour = frame.silhouette.as_ref().map(|source| {
        let comparisons = match &mesh {
            Ok(mesh) => {
                match silhouette::compare(mesh, &frame.camera, frame.image_size_px, source, None) {
                    Ok(comparisons) => Some(comparisons),
                    Err(error) => {
                        diagnostics.push(json!({"stage":"silhouette_projection","refusal":error}));
                        None
                    }
                }
            }
            Err(_) => None,
        };
        let lines: Vec<_> = source
            .lines
            .iter()
            .map(|observation| {
                let comparison = comparisons.as_ref().and_then(|comparisons| {
                    comparisons.iter().find(|line| {
                        line.axis == observation.axis && line.at_px == observation.at_px
                    })
                });
                json!({"observation":observation,"comparison":comparison})
            })
            .collect();
        json!({"evidence":source.evidence,"lines":lines})
    });
    let mut value = json!({"schema":"ftx-body-start-frame-review/1","status":"INITIALIZATION_REVIEW",
        "body_fitted":false,"pose_evaluations":usize::from(mesh.is_ok()),"input_sha256":sha256_hex(input_text.as_bytes()),
        "model_sha256":hash,"start_index":start_index,"frame_index":frame_index,
        "start_count":input.starts.len(),"frame_count":input.frames.len(),"frame":frame.frame,
        "frame_label":format!("{} @ {} us",frame.frame.stream,frame.frame.timestamp_us),
        "shape_parameters":adapter.layout.shape,"shape_values":start.shape,
        "pose_parameters":adapter.layout.pose,"pose_values":pose.values,
        "image_size_px":frame.image_size_px,"rotation_cw_deg":frame.rotation_cw_deg,"camera":frame.camera,
        "keypoints":keypoints,"silhouette":contour,"mesh_obj":mesh_obj,"triangles":triangles,
        "projected_vertices_px":pixels,"unprojectable_vertices":pixels.iter().filter(|point| point.is_none()).count(),
        "omitted_faces":omitted_faces,"diagnostics":diagnostics,
        "projection_kind":if silhouette::require_rectified(&frame.camera).is_ok(){"straight_edges"}else{"distorted_vertices"},
        "no_claim":"Declared INITIALIZATION inspected without optimization or acceptance; pose_evaluations counts complete requested poses. Large residuals and failed projections remain diagnostics. Keep held-out evidence fixed; do not tune starts to CHECK residuals. This is not an accepted body, collision skin, anatomical validation, READY measurement or physical garment-fit result."});
    if let Some(images) = request.capture_images {
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
            #[serde(default)]
            status: Option<String>,
            #[serde(default)]
            body_fitted: Option<bool>,
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
        let initialization = frame.schema == "ftx-body-start-frame-review/1"
            && frame.status.as_deref() == Some("INITIALIZATION_REVIEW")
            && frame.body_fitted == Some(false);
        if !(frame.schema == "ftx-body-frame-review/1" || initialization)
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
        let mut report = json!({"schema":"ftx-body-frame-image/1",
            "review_sha256":sha256_hex(review.as_bytes()),"source_image":source,
            "preview_sha256":sha256_hex(&preview),"orientation":"already_applied_stored_pixels",
            "no_claim":"Original encoded image bytes and dimensions match the retained frame. No refitting, camera calibration, anatomy, visibility or physical-fit acceptance was performed."});
        if initialization {
            report["review_kind"] = json!("INITIALIZATION_REVIEW");
        }
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
