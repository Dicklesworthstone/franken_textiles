//! Explicit Anny correspondences for articulated multi-frame fitting; no detector or guessed anatomy.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use ftx_body::{
    Affine, AnnyData, AnnyModel, BodyFrames, BodyParams, Phenotype, Pose, RestBody, TAPE_FIELDS,
    age_parameter_for_years, pose_deltas, tape_readings,
};
use ftx_core::{Mat3, Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_measure::body_fit::{
    BodyFitInput, BodyModel, FitOptions, ModelLayout, Parameter, fit_body,
};
use ftx_measure::{
    silhouette::{MaskSample, SurfaceMesh, require_rectified, sample_mask},
    triangulate::FrameId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{option, photo, single};
use crate::{Args, bad_input, known_options, read_json, required};

mod capture;
mod review;

pub(super) fn capture_review(args: &Args) -> Result<(Value, i32, String), Refusal> {
    capture::run(args)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    Vertex { index: usize },
    Surface { face: usize, barycentric: [f64; 3] },
    BoneHead { bone: String },
    BoneTail { bone: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Joint {
    axis: Vec3,
    #[serde(default)]
    angle_rad: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    model_sha256: String,
    age_years: f64,
    gender: f64,
    #[serde(default)]
    fixed_shape: BTreeMap<String, f64>,
    shape_parameters: Vec<Parameter>,
    #[serde(default)]
    root_translation_m: Vec3,
    #[serde(default)]
    root_rotation_rad: Vec3,
    #[serde(default)]
    joints: BTreeMap<String, Joint>,
    pose_parameters: Vec<Parameter>,
    keypoints: BTreeMap<String, Target>,
    measurements: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    model: Configuration,
    fit: BodyFitInput,
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

enum ResolvedTarget {
    Vertex(usize),
    Surface([usize; 3], [f64; 3]),
    Head(usize),
    Tail(usize),
}
enum Shape {
    Height,
    Weight,
    Muscle,
    Proportions,
    Local(usize),
}
enum PoseAxis {
    Translation(usize),
    Rotation(usize),
    Joint(usize),
}
struct Cache {
    shape: Vec<f64>,
    rest: RestBody,
    metrics: Vec<f64>,
}
struct Adapter {
    model: AnnyModel,
    layout: ModelLayout,
    config: Configuration,
    base: BodyParams,
    shapes: Vec<Shape>,
    pose_axes: Vec<PoseAxis>,
    joints: Vec<(usize, Vec3, f64)>,
    targets: Vec<ResolvedTarget>,
    selected_vertices: Vec<usize>,
    cache: RefCell<Option<Cache>>,
}

fn shape_axis(model: &AnnyModel, name: &str) -> Result<Shape, Refusal> {
    Ok(match name {
        "height" => Shape::Height,
        "weight" => Shape::Weight,
        "muscle" => Shape::Muscle,
        "proportions" => Shape::Proportions,
        _ => Shape::Local(
            model
                .local_change_index(name)
                .ok_or_else(|| bad_input(format!("unknown Anny shape parameter '{name}'")))?,
        ),
    })
}
fn set_shape(p: &mut BodyParams, axis: &Shape, value: f64) {
    match axis {
        Shape::Height => p.phenotype.height = value,
        Shape::Weight => p.phenotype.weight = value,
        Shape::Muscle => p.phenotype.muscle = value,
        Shape::Proportions => p.phenotype.proportions = value,
        Shape::Local(i) => p.local_changes[*i] = value,
    }
}
fn range(p: &Parameter, lo: f64, hi: f64) -> Result<(), Refusal> {
    if !p.lower.is_finite()
        || !p.upper.is_finite()
        || p.lower < lo
        || p.upper > hi
        || p.lower >= p.upper
    {
        return Err(bad_input(format!(
            "invalid fit bounds for '{}' (allowed {lo}..{hi})",
            p.name
        )));
    }
    Ok(())
}
fn rotation(v: Vec3) -> Mat3 {
    let angle = v.norm();
    if angle == 0.0 {
        Mat3::IDENTITY
    } else {
        Mat3::rotation(v / angle, angle)
    }
}

fn vertex_slot(selected: &mut Vec<usize>, index: usize) -> usize {
    if let Some(slot) = selected.iter().position(|&v| v == index) {
        slot
    } else {
        selected.push(index);
        selected.len() - 1
    }
}

// Canonical support catches a vertex expressed as a triangle corner and a shared-edge point
// expressed on either adjacent face. Zero weights carry no geometric information.
fn surface_identity(indices: [usize; 3], weights: [f64; 3]) -> Vec<(usize, u64)> {
    let mut support: Vec<_> = indices
        .into_iter()
        .zip(weights)
        .filter(|(_, w)| *w > 0.0)
        .map(|(i, w)| (i, w.to_bits()))
        .collect();
    support.sort_unstable();
    support
}

impl Adapter {
    fn new(model: AnnyModel, config: Configuration, hash: &str) -> Result<Self, Refusal> {
        if hash != config.model_sha256
            || hash.len() != 64
            || model.data().header.anny_version != "0.6.1"
            || model.data().header.counts.bones == 0
        {
            return Err(bad_input(
                "model SHA-256 or pinned Anny 0.6.1 identity does not match",
            ));
        }
        if !config.age_years.is_finite()
            || !(0.0..=110.0).contains(&config.age_years)
            || !config.gender.is_finite()
            || !(0.0..=1.0).contains(&config.gender)
            || !config.root_translation_m.is_finite()
            || !config.root_rotation_rad.is_finite()
            || config.root_rotation_rad.norm() > std::f64::consts::PI
            || config.keypoints.is_empty()
            || config.keypoints.len() > 128
            || config.measurements.len() > 32
            || config.joints.len() > 32
            || !(1..=12).contains(&config.shape_parameters.len())
            || !(1..=18).contains(&config.pose_parameters.len())
        {
            return Err(bad_input("invalid Anny fit configuration or size budget"));
        }
        let mut base = model.params(Phenotype {
            age: age_parameter_for_years(config.age_years),
            gender: config.gender,
            ..Phenotype::default()
        });
        for (name, &v) in &config.fixed_shape {
            let axis = shape_axis(&model, name)?;
            let lo = if matches!(axis, Shape::Local(_)) {
                -1.0
            } else {
                0.0
            };
            if !v.is_finite() || !(lo..=1.0).contains(&v) {
                return Err(bad_input(format!(
                    "fixed '{name}' is outside its modelled range"
                )));
            }
            set_shape(&mut base, &axis, v);
        }
        let mut shapes = Vec::new();
        let mut names = BTreeSet::new();
        for p in &config.shape_parameters {
            if !names.insert(&p.name) || config.fixed_shape.contains_key(&p.name) {
                return Err(bad_input(
                    "shape parameters must be unique and not also fixed",
                ));
            }
            let axis = shape_axis(&model, &p.name)?;
            range(
                p,
                if matches!(axis, Shape::Local(_)) {
                    -1.0
                } else {
                    0.0
                },
                1.0,
            )?;
            shapes.push(axis);
        }
        let mut joints = Vec::new();
        for (name, joint) in &config.joints {
            let k = model
                .bone_index(name)
                .filter(|&i| i > 0)
                .ok_or_else(|| bad_input(format!("unknown/non-articulated bone '{name}'")))?;
            if !joint.axis.is_finite()
                || (joint.axis.norm() - 1.0).abs() > 1e-10
                || !joint.angle_rad.is_finite()
                || joint.angle_rad.abs() > std::f64::consts::PI
            {
                return Err(bad_input(
                    "joint axes must be unit local-bone vectors; angles must be finite radians within +/-pi",
                ));
            }
            joints.push((k, joint.axis, joint.angle_rad));
        }
        let mut pose_axes = Vec::new();
        let mut names = BTreeSet::new();
        for p in &config.pose_parameters {
            if !names.insert(&p.name) {
                return Err(bad_input("duplicate pose parameter"));
            }
            let axis = match p.name.as_str() {
                "root_x_m" => PoseAxis::Translation(0),
                "root_y_m" => PoseAxis::Translation(1),
                "root_z_m" => PoseAxis::Translation(2),
                "root_rx_rad" => PoseAxis::Rotation(0),
                "root_ry_rad" => PoseAxis::Rotation(1),
                "root_rz_rad" => PoseAxis::Rotation(2),
                name => {
                    let bone = name
                        .strip_prefix("joint:")
                        .ok_or_else(|| bad_input(format!("unknown pose parameter '{name}'")))?;
                    let k = config
                        .joints
                        .keys()
                        .position(|n| n == bone)
                        .ok_or_else(|| {
                            bad_input(format!("joint '{bone}' has no explicit local axis"))
                        })?;
                    PoseAxis::Joint(k)
                }
            };
            if matches!(axis, PoseAxis::Translation(_)) {
                range(p, -1e6, 1e6)?;
            } else {
                range(p, -std::f64::consts::PI, std::f64::consts::PI)?;
            }
            pose_axes.push(axis);
        }
        let mut targets = Vec::new();
        let mut selected_vertices = Vec::new();
        let mut locations = BTreeSet::new();
        let mut surface_locations = BTreeSet::new();
        for (name, target) in &config.keypoints {
            if name.trim().is_empty() || name.len() > 256 {
                return Err(bad_input("invalid keypoint name"));
            }
            let (resolved, identity) = match target {
                Target::Vertex { index } => {
                    if *index >= model.data().header.counts.vertices {
                        return Err(bad_input("vertex correspondence is out of range"));
                    }
                    if !surface_locations.insert(vec![(*index, 1.0_f64.to_bits())]) {
                        return Err(bad_input("duplicate surface correspondence"));
                    }
                    let slot = vertex_slot(&mut selected_vertices, *index);
                    (ResolvedTarget::Vertex(slot), format!("vertex:{index}"))
                }
                Target::Surface { face, barycentric } => {
                    let ids = model
                        .data()
                        .faces
                        .get(*face)
                        .ok_or_else(|| bad_input("surface correspondence face is out of range"))?
                        .map(|i| i as usize);
                    let sum: f64 = barycentric.iter().sum();
                    if barycentric
                        .iter()
                        .any(|w| !w.is_finite() || !(0.0..=1.0).contains(w))
                        || (sum - 1.0).abs() > 1e-12
                        || ids[0] == ids[1]
                        || ids[0] == ids[2]
                        || ids[1] == ids[2]
                    {
                        return Err(bad_input(
                            "surface correspondence needs a triangle and nonnegative barycentric weights summing to one (1e-12 tolerance); weights are not normalized",
                        ));
                    }
                    if !surface_locations.insert(surface_identity(ids, *barycentric)) {
                        return Err(bad_input("duplicate surface correspondence"));
                    }
                    let slots = ids.map(|i| vertex_slot(&mut selected_vertices, i));
                    // The physical identity is checked above, including aliases on neighboring faces.
                    (
                        ResolvedTarget::Surface(slots, *barycentric),
                        format!("surface:{name}"),
                    )
                }
                Target::BoneHead { bone } | Target::BoneTail { bone } => {
                    let i = model
                        .bone_index(bone)
                        .ok_or_else(|| bad_input(format!("unknown keypoint bone '{bone}'")))?;
                    if matches!(target, Target::BoneHead { .. }) {
                        (ResolvedTarget::Head(i), format!("head:{i}"))
                    } else {
                        (ResolvedTarget::Tail(i), format!("tail:{i}"))
                    }
                }
            };
            if !locations.insert(identity) {
                return Err(bad_input(
                    "duplicate model location under multiple correspondence names",
                ));
            }
            targets.push(resolved);
        }
        if config
            .measurements
            .iter()
            .any(|n| !TAPE_FIELDS.contains(&n.as_str()))
            || config.measurements.iter().collect::<BTreeSet<_>>().len()
                != config.measurements.len()
        {
            return Err(bad_input("unknown or duplicate Anny tape functional"));
        }
        let layout = ModelLayout {
            model_id: format!("anny/0.6.1/sha256:{hash}"),
            shape: config.shape_parameters.clone(),
            pose: config.pose_parameters.clone(),
            keypoints: config.keypoints.keys().cloned().collect(),
            measurements: config.measurements.clone(),
        };
        Ok(Self {
            model,
            layout,
            config,
            base,
            shapes,
            pose_axes,
            joints,
            targets,
            selected_vertices,
            cache: RefCell::new(None),
        })
    }

    fn parameters(&self, shape: &[f64]) -> BodyParams {
        let mut p = self.base.clone();
        for (axis, &v) in self.shapes.iter().zip(shape) {
            set_shape(&mut p, axis, v);
        }
        p
    }

    fn prepare(&self, shape: &[f64]) -> Result<(), Refusal> {
        if self
            .cache
            .borrow()
            .as_ref()
            .is_some_and(|c| c.shape == shape)
        {
            return Ok(());
        }
        let rest = self.model.rest(&self.parameters(shape))?;
        let height = rest
            .vertices
            .iter()
            .map(|v| v.z)
            .fold(f64::NEG_INFINITY, f64::max)
            - rest
                .vertices
                .iter()
                .map(|v| v.z)
                .fold(f64::INFINITY, f64::min);
        let mut metrics = vec![height; self.layout.measurements.len()];
        if self.layout.measurements.iter().any(|n| n != "stature") {
            let posed = self.model.pose(
                &rest,
                &pose_deltas(&self.model, &rest, &Pose::a_pose(35.0), 1.0)?,
            )?;
            let frames = BodyFrames::new(&self.model, &posed)?;
            let faces: Vec<_> = self
                .model
                .data()
                .faces
                .iter()
                .map(|f| f.map(|v| v as usize))
                .collect();
            let tape = tape_readings(&faces, height, &posed, &frames)
                .ok_or_else(|| bad_input("Anny rest-body tape functional is unavailable"))?;
            for (name, value) in self.layout.measurements.iter().zip(&mut metrics) {
                *value = tape[TAPE_FIELDS.iter().position(|n| *n == name).unwrap()] / 1000.0;
            }
        }
        self.cache.replace(Some(Cache {
            shape: shape.to_vec(),
            rest,
            metrics,
        }));
        Ok(())
    }
    fn deltas(&self, pose: &[f64]) -> Vec<Affine> {
        let mut t: [f64; 3] = self.config.root_translation_m.into();
        let mut r: [f64; 3] = self.config.root_rotation_rad.into();
        let mut angles: Vec<_> = self.joints.iter().map(|j| j.2).collect();
        for (axis, &value) in self.pose_axes.iter().zip(pose) {
            match axis {
                PoseAxis::Translation(i) => t[*i] = value,
                PoseAxis::Rotation(i) => r[*i] = value,
                PoseAxis::Joint(i) => angles[*i] = value,
            }
        }
        let mut deltas = vec![Affine::IDENTITY; self.model.data().header.counts.bones];
        deltas[0] = Affine {
            m: rotation(Vec3::from(r)),
            t: Vec3::from(t),
        };
        for ((bone, axis, _), angle) in self.joints.iter().zip(angles) {
            deltas[*bone] = Affine {
                m: Mat3::rotation(*axis, angle),
                t: Vec3::ZERO,
            };
        }
        deltas
    }
}

impl BodyModel for Adapter {
    fn layout(&self) -> &ModelLayout {
        &self.layout
    }
    fn measurements(&self, shape: &[f64]) -> Result<Vec<f64>, Refusal> {
        self.prepare(shape)?;
        Ok(self.cache.borrow().as_ref().unwrap().metrics.clone())
    }
    fn points(&self, shape: &[f64], pose: &[f64]) -> Result<Vec<Vec3>, Refusal> {
        self.prepare(shape)?;
        let cache = self.cache.borrow();
        let rest = &cache.as_ref().unwrap().rest;
        let posed = self
            .model
            .pose_selected(rest, &self.deltas(pose), &self.selected_vertices)?;
        Ok(self
            .targets
            .iter()
            .map(|target| match target {
                ResolvedTarget::Vertex(i) => posed.vertices[*i],
                ResolvedTarget::Surface(ids, w) => {
                    posed.vertices[ids[0]] * w[0]
                        + posed.vertices[ids[1]] * w[1]
                        + posed.vertices[ids[2]] * w[2]
                }
                ResolvedTarget::Head(i) => posed.bone_poses[*i].t,
                ResolvedTarget::Tail(i) => posed.transforms[*i].apply(rest.bone_tails[*i]),
            })
            .collect())
    }
    fn surface(&self, shape: &[f64], pose: &[f64]) -> Result<SurfaceMesh, Refusal> {
        self.prepare(shape)?;
        let cache = self.cache.borrow();
        let rest = &cache.as_ref().unwrap().rest;
        let posed = self.model.pose(rest, &self.deltas(pose))?;
        Ok(SurfaceMesh {
            vertices_m: posed.vertices,
            triangles: self
                .model
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
        serde_json::to_value(&adapter.config).map_err(|e| bad_input(e.to_string()))?;
    value["input_sha256"] = json!(sha256_hex(&input_bytes));
    if let Some(capture_input) = capture_input {
        value["capture_input"] = capture_input;
    }
    if let Some(masks) = masks {
        value["silhouette_input"] = masks;
    }
    if let Some(solution) = &report.solution {
        let p = adapter.parameters(&solution.shape);
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
