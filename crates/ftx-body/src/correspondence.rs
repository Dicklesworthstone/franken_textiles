//! Explicit Anny shape, pose and anatomical correspondences for calibrated image fitting.
//! This is model evaluation only: the measurement crate owns optimization and acceptance through
//! its body-model trait. Native and portable callers use this same adapter without a crate cycle.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

use ftx_core::{Mat3, Refusal, RefusalCode, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Affine, AnnyModel, BodyFrames, BodyParams, Phenotype, Pose, PosedBody, RestBody, TAPE_FIELDS,
    age_parameter_for_years, partial_tape_readings, pose_deltas,
};

/// One authored scalar control; fitted values stay strictly inside these bounds.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    /// Exact shape or pose control name.
    pub name: String,
    /// Exclusive lower bound.
    pub lower: f64,
    /// Exclusive upper bound.
    pub upper: f64,
}

/// Explicit anatomical correspondence, never guessed from a detector label.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    /// An original model vertex.
    Vertex {
        /// Index in the converted model.
        index: usize,
    },
    /// A point on the actual posed triangle, after skinning each vertex.
    Surface {
        /// Original model triangle index.
        face: usize,
        /// Nonnegative weights summing to one; never normalized by the adapter.
        barycentric: [f64; 3],
    },
    /// A named bone's posed head.
    BoneHead {
        /// Exact bone name.
        bone: String,
    },
    /// A named bone's posed tail.
    BoneTail {
        /// Exact bone name.
        bone: String,
    },
}

/// One explicitly authored joint axis in the bone's local frame.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Joint {
    /// Unit local-bone axis; no normalization is inferred.
    pub axis: Vec3,
    /// Fixed angle in radians unless selected as a pose parameter.
    #[serde(default)]
    pub angle_rad: f64,
}

/// The unchanged `model` block of `ftx-anny-body-fit-input/1`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    /// SHA-256 of the exact array-verified converted model.
    pub model_sha256: String,
    /// Authored age in years, not inferred from images.
    pub age_years: f64,
    /// Authored Anny gender coefficient.
    pub gender: f64,
    /// Explicit fixed shape controls; unselected controls retain model defaults.
    #[serde(default)]
    pub fixed_shape: BTreeMap<String, f64>,
    /// Shared fitted controls in vector order.
    pub shape_parameters: Vec<Parameter>,
    /// Fixed root translation in metres, overridden by selected pose controls.
    #[serde(default)]
    pub root_translation_m: Vec3,
    /// Fixed root rotation vector in radians, overridden by selected pose controls.
    #[serde(default)]
    pub root_rotation_rad: Vec3,
    /// Selected articulated joints; all other joints remain at rest.
    #[serde(default)]
    pub joints: BTreeMap<String, Joint>,
    /// Independently fitted controls in each frame, in vector order.
    pub pose_parameters: Vec<Parameter>,
    /// Named model locations in canonical key order.
    pub keypoints: BTreeMap<String, Target>,
    /// Requested independent rest-body tape functionals, in vector order.
    pub measurements: Vec<String>,
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
    metrics: Option<Vec<f64>>,
}

/// One checked correspondence configuration and its shared-shape evaluation cache.
/// Source identity and array verification remain the caller's responsibility when loading a model.
pub struct CorrespondenceModel {
    model: AnnyModel,
    config: Configuration,
    base: BodyParams,
    shapes: Vec<Shape>,
    pose_axes: Vec<PoseAxis>,
    joints: Vec<(usize, Vec3, f64)>,
    targets: Vec<ResolvedTarget>,
    selected_vertices: Vec<usize>,
    cache: RefCell<Option<Cache>>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
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
                .ok_or_else(|| invalid(format!("unknown Anny shape parameter '{name}'")))?,
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
        return Err(invalid(format!(
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

impl CorrespondenceModel {
    /// Check the exact model/configuration and resolve authored controls and correspondences.
    ///
    /// # Errors
    /// Model mismatch, unsupported or duplicate controls, invalid bounds/axes/targets or budgets.
    pub fn new(model: AnnyModel, config: Configuration, hash: &str) -> Result<Self, Refusal> {
        Self::checked(model, config, hash, false)
    }

    /// Prepare target authoring with an initially empty map, without inventing correspondences.
    /// Ordinary fitting still uses [`Self::new`] and requires an authored target.
    ///
    /// # Errors
    /// The same configuration refusals as [`Self::new`], except an empty target map is allowed.
    pub fn for_target_review(
        model: AnnyModel,
        config: Configuration,
        hash: &str,
    ) -> Result<Self, Refusal> {
        Self::checked(model, config, hash, true)
    }

    fn checked(
        model: AnnyModel,
        config: Configuration,
        hash: &str,
        allow_empty_targets: bool,
    ) -> Result<Self, Refusal> {
        if hash != config.model_sha256
            || hash.len() != 64
            || model.data().header.anny_version != "0.6.1"
            || model.data().header.counts.bones == 0
        {
            return Err(invalid(
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
            || (!allow_empty_targets && config.keypoints.is_empty())
            || config.keypoints.len() > 128
            || config.measurements.len() > 32
            || config.joints.len() > 32
            || !(1..=12).contains(&config.shape_parameters.len())
            || !(1..=18).contains(&config.pose_parameters.len())
        {
            return Err(invalid("invalid Anny fit configuration or size budget"));
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
                return Err(invalid(format!(
                    "fixed '{name}' is outside its modelled range"
                )));
            }
            set_shape(&mut base, &axis, v);
        }
        let mut shapes = Vec::new();
        let mut names = BTreeSet::new();
        for p in &config.shape_parameters {
            if !names.insert(&p.name) || config.fixed_shape.contains_key(&p.name) {
                return Err(invalid(
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
                .ok_or_else(|| invalid(format!("unknown/non-articulated bone '{name}'")))?;
            if !joint.axis.is_finite()
                || (joint.axis.norm() - 1.0).abs() > 1e-10
                || !joint.angle_rad.is_finite()
                || joint.angle_rad.abs() > std::f64::consts::PI
            {
                return Err(invalid(
                    "joint axes must be unit local-bone vectors; angles must be finite radians within +/-pi",
                ));
            }
            joints.push((k, joint.axis, joint.angle_rad));
        }
        let mut pose_axes = Vec::new();
        let mut names = BTreeSet::new();
        for p in &config.pose_parameters {
            if !names.insert(&p.name) {
                return Err(invalid("duplicate pose parameter"));
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
                        .ok_or_else(|| invalid(format!("unknown pose parameter '{name}'")))?;
                    let k = config
                        .joints
                        .keys()
                        .position(|n| n == bone)
                        .ok_or_else(|| {
                            invalid(format!("joint '{bone}' has no explicit local axis"))
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
                return Err(invalid("invalid keypoint name"));
            }
            let (resolved, identity) = match target {
                Target::Vertex { index } => {
                    if *index >= model.data().header.counts.vertices {
                        return Err(invalid("vertex correspondence is out of range"));
                    }
                    if !surface_locations.insert(vec![(*index, 1.0_f64.to_bits())]) {
                        return Err(invalid("duplicate surface correspondence"));
                    }
                    let slot = vertex_slot(&mut selected_vertices, *index);
                    (ResolvedTarget::Vertex(slot), format!("vertex:{index}"))
                }
                Target::Surface { face, barycentric } => {
                    let ids = model
                        .data()
                        .faces
                        .get(*face)
                        .ok_or_else(|| invalid("surface correspondence face is out of range"))?
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
                        return Err(invalid(
                            "surface correspondence needs a triangle and nonnegative barycentric weights summing to one (1e-12 tolerance); weights are not normalized",
                        ));
                    }
                    if !surface_locations.insert(surface_identity(ids, *barycentric)) {
                        return Err(invalid("duplicate surface correspondence"));
                    }
                    let slots = ids.map(|i| vertex_slot(&mut selected_vertices, i));
                    (
                        ResolvedTarget::Surface(slots, *barycentric),
                        format!("surface:{name}"),
                    )
                }
                Target::BoneHead { bone } | Target::BoneTail { bone } => {
                    let i = model
                        .bone_index(bone)
                        .ok_or_else(|| invalid(format!("unknown keypoint bone '{bone}'")))?;
                    if matches!(target, Target::BoneHead { .. }) {
                        (ResolvedTarget::Head(i), format!("head:{i}"))
                    } else {
                        (ResolvedTarget::Tail(i), format!("tail:{i}"))
                    }
                }
            };
            if !locations.insert(identity) {
                return Err(invalid(
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
            return Err(invalid("unknown or duplicate Anny tape functional"));
        }
        Ok(Self {
            model,
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

    /// The exact checked model.
    #[must_use]
    pub fn model(&self) -> &AnnyModel {
        &self.model
    }

    /// Immutable authored configuration and parameter order.
    #[must_use]
    pub fn configuration(&self) -> &Configuration {
        &self.config
    }

    /// Original rest topology and named rig endpoints for explicit correspondence authoring.
    /// Free shape controls use their declared midpoints; no pose, observation or tape is evaluated.
    ///
    /// # Errors
    /// Empty or excessive topology, invalid model evaluation or nonfinite rest geometry.
    pub fn target_preview(&self) -> Result<Value, Refusal> {
        let counts = &self.model.data().header.counts;
        if counts.vertices == 0 || counts.faces == 0 {
            return Err(invalid(
                "model target review requires a nonempty triangle mesh",
            ));
        }
        if counts.vertices > 100_000 || counts.faces > 200_000 || counts.bones > 1024 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "model target review supports at most 100000 vertices, 200000 triangles and 1024 bones",
            ));
        }
        let shape: Vec<_> = self
            .config
            .shape_parameters
            .iter()
            .map(|p| p.lower + 0.5 * (p.upper - p.lower))
            .collect();
        let rest = self.model.rest(&self.parameters(&shape)?)?;
        if rest
            .vertices
            .iter()
            .chain(&rest.bone_heads)
            .chain(&rest.bone_tails)
            .any(|p| !p.is_finite())
        {
            return Err(invalid("model target review needs finite rest geometry"));
        }
        let preview = json!({"status":"UNFITTED", "pose":"unposed_rest", "units":"m",
            "coordinates":"model_rest_z_up_faces_negative_y",
            "age_years":self.config.age_years,"gender":self.config.gender,
            "fixed_shape":self.config.fixed_shape,
            "shape_values":self.config.shape_parameters.iter().zip(&shape).map(|(p,v)|
                json!({"name":p.name,"value":v,"lower":p.lower,"upper":p.upper})).collect::<Vec<_>>(),
            "basis":"Free shape parameters are at the midpoint of their declared bounds. No observation, start, fitted shape, root transform or articulated pose was used."});
        let bones: Vec<_> = self.model.data().header.labels.bones.iter().enumerate()
            .map(|(i, bone)| json!({"name":bone,"head_m":rest.bone_heads[i],"tail_m":rest.bone_tails[i]}))
            .collect();
        Ok(
            json!({"model_sha256":self.config.model_sha256,"preview":preview,
            "mesh":{"vertices_m":rest.vertices,"triangles":self.model.data().faces},"bones":bones}),
        )
    }

    /// Expand shared shape values without changing fixed or unselected controls.
    ///
    /// # Errors
    /// A wrong-length or non-finite shape vector.
    pub fn parameters(&self, shape: &[f64]) -> Result<BodyParams, Refusal> {
        if shape.len() != self.shapes.len() || shape.iter().any(|v| !v.is_finite()) {
            return Err(invalid("invalid Anny shared-shape vector"));
        }
        let mut p = self.base.clone();
        for (axis, &v) in self.shapes.iter().zip(shape) {
            set_shape(&mut p, axis, v);
        }
        Ok(p)
    }

    fn prepare_rest(&self, shape: &[f64]) -> Result<(), Refusal> {
        if self
            .cache
            .borrow()
            .as_ref()
            .is_some_and(|c| c.shape == shape)
        {
            return Ok(());
        }
        let rest = self.model.rest(&self.parameters(shape)?)?;
        self.cache.replace(Some(Cache {
            shape: shape.to_vec(),
            rest,
            metrics: None,
        }));
        Ok(())
    }

    fn prepare(&self, shape: &[f64]) -> Result<(), Refusal> {
        self.prepare_rest(shape)?;
        let mut cache = self.cache.borrow_mut();
        let cache = cache.as_mut().unwrap();
        if cache.metrics.is_some() {
            return Ok(());
        }
        let rest = &cache.rest;
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
        let mut metrics = vec![height; self.config.measurements.len()];
        if self.config.measurements.iter().any(|n| n != "stature") {
            let posed = self.model.pose(
                rest,
                &pose_deltas(&self.model, rest, &Pose::a_pose(35.0), 1.0)?,
            )?;
            let frames = BodyFrames::new(&self.model, &posed)?;
            let faces: Vec<_> = self
                .model
                .data()
                .faces
                .iter()
                .map(|f| f.map(|v| v as usize))
                .collect();
            // Missing unrequested anatomy must not discard independent usable tape targets.
            let tape = partial_tape_readings(&faces, height, &posed, &frames);
            for (name, value) in self.config.measurements.iter().zip(&mut metrics) {
                *value =
                    tape[TAPE_FIELDS.iter().position(|n| *n == name).unwrap()].ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::DegenerateGeometry,
                            format!(
                                "requested Anny rest-body tape functional '{name}' is unavailable"
                            ),
                        )
                    })? / 1000.0;
            }
        }
        // A failed tape evaluation leaves only the reusable rest body cached. It must not
        // become an empty successful metric result or block independent image previews.
        cache.metrics = Some(metrics);
        Ok(())
    }

    /// Evaluate rest-body tape functionals in metres, independent of any image pose.
    ///
    /// # Errors
    /// Invalid shape/model geometry or a requested unavailable tape functional.
    pub fn measurements(&self, shape: &[f64]) -> Result<Vec<f64>, Refusal> {
        self.prepare(shape)?;
        Ok(self
            .cache
            .borrow()
            .as_ref()
            .unwrap()
            .metrics
            .as_ref()
            .unwrap()
            .clone())
    }

    /// Expand one frame's root and local-joint pose without changing its fixed controls.
    ///
    /// # Errors
    /// A wrong-length or non-finite pose vector.
    pub fn deltas(&self, pose: &[f64]) -> Result<Vec<Affine>, Refusal> {
        if pose.len() != self.pose_axes.len() || pose.iter().any(|v| !v.is_finite()) {
            return Err(invalid("invalid Anny per-frame pose vector"));
        }
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
        Ok(deltas)
    }

    /// Evaluate only vertices and rig endpoints used by the ordered correspondences.
    ///
    /// # Errors
    /// Invalid vectors, model geometry or unavailable requested tape functionals.
    pub fn points(&self, shape: &[f64], pose: &[f64]) -> Result<Vec<Vec3>, Refusal> {
        self.prepare(shape)?;
        let cache = self.cache.borrow();
        let rest = &cache.as_ref().unwrap().rest;
        let posed = self
            .model
            .pose_selected(rest, &self.deltas(pose)?, &self.selected_vertices)?;
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

    /// Evaluate the complete posed surface through the same cached shared shape.
    ///
    /// # Errors
    /// The same model/vector and requested-tape refusals as [`Self::points`].
    pub fn posed(&self, shape: &[f64], pose: &[f64]) -> Result<PosedBody, Refusal> {
        self.prepare(shape)?;
        let cache = self.cache.borrow();
        let rest = &cache.as_ref().unwrap().rest;
        self.model.pose(rest, &self.deltas(pose)?)
    }

    /// Evaluate a full surface and its ordered correspondences with exactly one pose evaluation.
    /// Initialization reviews use the same skinned vertices and rig endpoints for both displays.
    /// Independent tape functionals are not needed or evaluated for this image-pose review.
    ///
    /// # Errors
    /// Invalid vectors, model geometry or inconsistent posed surface/skeleton dimensions.
    pub fn posed_with_points(
        &self,
        shape: &[f64],
        pose: &[f64],
    ) -> Result<(PosedBody, Vec<Vec3>), Refusal> {
        self.prepare_rest(shape)?;
        let cache = self.cache.borrow();
        let rest = &cache.as_ref().unwrap().rest;
        let body = self.model.pose(rest, &self.deltas(pose)?)?;
        let points = self.points_on_body(rest, &body)?;
        Ok((body, points))
    }

    /// Evaluate correspondences on a complete already-posed mesh for independent export checks.
    /// The supplied rest/body must come from this model and the same requested shape and pose.
    ///
    /// # Errors
    /// Inconsistent full mesh or skeleton dimensions.
    pub fn points_on_body(&self, rest: &RestBody, body: &PosedBody) -> Result<Vec<Vec3>, Refusal> {
        let counts = &self.model.data().header.counts;
        if body.vertices.len() != counts.vertices
            || body.bone_poses.len() != counts.bones
            || body.transforms.len() != counts.bones
            || rest.bone_tails.len() != counts.bones
        {
            return Err(invalid(
                "full correspondence mesh has inconsistent dimensions",
            ));
        }
        let vertex = |i: usize| body.vertices[self.selected_vertices[i]];
        Ok(self
            .targets
            .iter()
            .map(|target| match target {
                ResolvedTarget::Vertex(i) => vertex(*i),
                ResolvedTarget::Surface(ids, w) => {
                    vertex(ids[0]) * w[0] + vertex(ids[1]) * w[1] + vertex(ids[2]) * w[2]
                }
                ResolvedTarget::Head(i) => body.bone_poses[*i].t,
                ResolvedTarget::Tail(i) => body.transforms[*i].apply(rest.bone_tails[*i]),
            })
            .collect())
    }
}
