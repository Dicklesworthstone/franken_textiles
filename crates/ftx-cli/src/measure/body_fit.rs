//! Explicit Anny correspondences for articulated multi-frame fitting; no detector or guessed anatomy.

use std::{collections::BTreeSet, path::Path};

use ftx_body::{
    AnnyData, AnnyModel,
    correspondence::{Configuration, CorrespondenceModel, Target},
};
use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_measure::body_fit::{
    BodyFitInput, BodyModel, CaptureImage, FitOptions, ModelLayout, Parameter, fit_body,
    validate_capture_images,
};
use ftx_measure::{
    silhouette::{MaskSample, SurfaceMesh, require_rectified, sample_mask},
    triangulate::FrameId,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{option, photo, single};
use crate::{Args, bad_input, known_options, read_json, required};

mod capture;
mod review;
mod setup;
mod setup_edit;
mod start_edit;
mod start_review;
mod targets;

pub(super) fn capture_review(args: &Args) -> Result<(Value, i32, String), Refusal> {
    capture::run(args)
}

pub(super) fn target_review(args: &Args) -> Result<(Value, i32, String), Refusal> {
    targets::review(args)
}

pub(super) fn apply_targets(args: &Args) -> Result<(Value, i32, String), Refusal> {
    targets::apply(args)
}

pub(super) fn fit_start_review(args: &Args) -> Result<(Value, i32, String), Refusal> {
    start_review::run(args)
}

pub(super) fn apply_starts(args: &Args) -> Result<(Value, i32, String), Refusal> {
    start_edit::apply(args)
}

pub(super) fn capture_init_review(args: &Args) -> Result<(Value, i32, String), Refusal> {
    setup::review(args)
}

pub(super) fn capture_init(args: &Args) -> Result<(Value, i32, String), Refusal> {
    setup::apply(args)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    model: Configuration,
    fit: BodyFitInput,
    #[serde(default)]
    capture_images: Option<Vec<CaptureImage>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaskFrame {
    frame: FrameId,
    mask: String,
    samples: Vec<MaskSample>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaskManifest {
    schema: String,
    evidence: String,
    frames: Vec<MaskFrame>,
}

fn import_masks(path: &str, fit: &mut BodyFitInput) -> Result<Value, Refusal> {
    let (value, bytes) = read_json(path)?;
    let manifest: MaskManifest = serde_json::from_value(value.clone())
        .map_err(|e| bad_input(format!("invalid silhouette mask manifest: {e}")))?;
    if manifest.schema != "ftx-body-silhouettes/1"
        || manifest.evidence.trim().is_empty()
        || manifest.evidence.len() > 3000
        || !(1..=8).contains(&manifest.frames.len())
    {
        return Err(bad_input(
            "silhouettes need schema ftx-body-silhouettes/1, evidence and 1..8 frames",
        ));
    }
    let source = std::fs::canonicalize(path)
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
    let base = source
        .parent()
        .ok_or_else(|| bad_input("mask manifest needs a parent directory"))?;
    let mut ids = BTreeSet::new();
    let mut receipts = Vec::new();
    for entry in manifest.frames {
        if !ids.insert(entry.frame.clone()) {
            return Err(bad_input("duplicate silhouette frame identity"));
        }
        let frame = fit
            .frames
            .iter_mut()
            .find(|f| f.frame == entry.frame)
            .ok_or_else(|| bad_input("silhouette frame is absent from the body-fit request"))?;
        if frame.silhouette.is_some() {
            return Err(bad_input(
                "cannot replace inline silhouette evidence with a mask",
            ));
        }
        require_rectified(&frame.camera)?;
        let mask = super::video::load(base, &entry.mask, true)?;
        if [mask.w, mask.h] != frame.image_size_px.map(|v| v as usize) {
            return Err(bad_input(
                "silhouette mask dimensions differ from the oriented fit image; no resampling is performed",
            ));
        }
        let hash = sha256_hex(&mask.data);
        frame.silhouette = Some(sample_mask(
            frame.image_size_px,
            &mask.data,
            &entry.samples,
            &format!("{}; decoded_mask_sha256:{hash}", manifest.evidence),
        )?);
        receipts.push(json!({"frame":entry.frame,"mask":entry.mask,
            "image_size_px":frame.image_size_px,"decoded_mask_sha256":hash}));
    }
    Ok(json!({"manifest":value,"manifest_sha256":sha256_hex(&bytes),"masks":receipts}))
}

struct Adapter {
    inner: CorrespondenceModel,
    layout: ModelLayout,
}

impl std::ops::Deref for Adapter {
    type Target = CorrespondenceModel;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Adapter {
    fn new(model: AnnyModel, config: Configuration, hash: &str) -> Result<Self, Refusal> {
        Ok(Self::from_model(CorrespondenceModel::new(
            model, config, hash,
        )?))
    }

    // Target authoring must bootstrap an empty map without inventing a model correspondence.
    // Ordinary fitting and materialization still require at least one authored target.
    fn for_target_review(
        model: AnnyModel,
        config: Configuration,
        hash: &str,
    ) -> Result<Self, Refusal> {
        Ok(Self::from_model(CorrespondenceModel::for_target_review(
            model, config, hash,
        )?))
    }

    fn from_model(inner: CorrespondenceModel) -> Self {
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
                .model()
                .data()
                .faces
                .iter()
                .map(|t| t.map(|k| k as usize))
                .collect(),
        })
    }
}

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "measure fit-body",
        &[
            "model",
            "out",
            "iterations",
            "max-evaluations",
            "silhouettes",
            "mesh-out",
        ],
    )?;
    let mesh_out = option(args, "mesh-out").map(Path::new);
    if let Some(path) = mesh_out {
        if option(args, "out").is_some() {
            return Err(bad_input(
                "--mesh-out includes fit.json; use either --mesh-out or --out",
            ));
        }
        review::preflight(path)?;
    }
    let file = single(args, "fit-body")?;
    let (value, input_bytes) = super::intrinsics::read_document(file)?;
    let (mut request, capture_input) = if value["schema"] == "ftx-anny-capture-input/1" {
        let (request, receipt) = capture::prepare(Path::new(file), value)?;
        (request, Some(receipt))
    } else {
        (
            serde_json::from_value::<Request>(value)
                .map_err(|e| bad_input(format!("invalid Anny body-fit request: {e}")))?,
            None,
        )
    };
    if request.schema != "ftx-anny-body-fit-input/1" {
        return Err(bad_input("expected ftx-anny-body-fit-input/1"));
    }
    if let Some(images) = &request.capture_images {
        validate_capture_images(images, &request.fit.frames)?;
    }
    let iterations = option(args, "iterations")
        .map(|s| {
            s.parse::<usize>()
                .map_err(|_| bad_input("--iterations must be 1..100"))
        })
        .transpose()?
        .unwrap_or(50);
    if !(1..=100).contains(&iterations) {
        return Err(bad_input("--iterations must be 1..100"));
    }
    let max_model_evaluations = option(args, "max-evaluations")
        .map(|s| {
            s.parse::<usize>()
                .map_err(|_| bad_input("--max-evaluations must be 1..1000000"))
        })
        .transpose()?
        .unwrap_or(FitOptions::default().max_model_evaluations);
    if !(1..=1_000_000).contains(&max_model_evaluations) {
        return Err(bad_input("--max-evaluations must be 1..1000000"));
    }
    let path = Path::new(required(args, "model")?);
    let bytes = std::fs::read(path)
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display())))?;
    let hash = sha256_hex(&bytes);
    if hash != request.model.model_sha256 {
        return Err(bad_input("model SHA-256 mismatch; no fit was run"));
    }
    let model = AnnyModel::from_data(AnnyData::from_bytes(&bytes, true)?)?;
    let adapter = Adapter::new(model, request.model, &hash)?;
    let masks = option(args, "silhouettes")
        .map(|path| import_masks(path, &mut request.fit))
        .transpose()?;
    let report = fit_body(
        &request.fit,
        &adapter,
        &FitOptions {
            iterations,
            max_model_evaluations,
            ..FitOptions::default()
        },
    )?;
    let code = match report.status {
        "ACCEPTED" => 0,
        "BUDGET_STOP" => 4,
        _ => 2,
    };
    let mut text = format!(
        "BODY FIT: {} ({} frames, {} starts, {} model calls)",
        report.status,
        report.input.frames.len(),
        report.attempts.len(),
        report.model_evaluations
    );
    for (i, a) in report.attempts.iter().enumerate() {
        text.push_str(&format!("\n  start {i}: {}", a.status));
        for flag in &a.flags {
            text.push_str(&format!("\n    {flag}"));
        }
    }
    for flag in &report.flags {
        text.push_str(&format!("\n  {flag}"));
    }
    text.push_str(&format!("\n{}", report.no_claim));
    let mut value = serde_json::to_value(&report).map_err(|e| bad_input(e.to_string()))?;
    value["anny_configuration"] =
        serde_json::to_value(adapter.configuration()).map_err(|e| bad_input(e.to_string()))?;
    value["input_sha256"] = json!(sha256_hex(&input_bytes));
    if let Some(images) = request.capture_images {
        value["capture_images"] = json!(images);
    }
    if let Some(capture_input) = capture_input {
        value["capture_input"] = capture_input;
    }
    if let Some(masks) = masks {
        value["silhouette_input"] = masks;
    }
    if let Some(solution) = &report.solution {
        let p = adapter.parameters(&solution.shape)?;
        let h = p.phenotype;
        value["solution"]["anny_parameters"] = json!({"provenance":"MODEL_FIT",
            "phenotype":{"gender":h.gender,"age":h.age,"muscle":h.muscle,"weight":h.weight,"height":h.height,"proportions":h.proportions},
            "local_changes":p.local_changes});
    }
    if let Some(path) = mesh_out {
        if report.solution.is_some() {
            let bundle = review::prepare(&adapter, &report, value.get("capture_input"))?;
            value["mesh_review"] = bundle.manifest.clone();
            bundle.publish(path, &value)?;
            text.push_str(&format!("\nmesh review: {}/fit.json", path.display()));
        } else {
            text.push_str("\nmesh review skipped: no accepted solution");
        }
    }
    if let Some(path) = option(args, "out") {
        photo::save(path, &value)?;
        eprintln!("wrote {path}");
    }
    Ok((value, code, text))
}
