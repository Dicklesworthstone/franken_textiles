//! Fit one Anny body once for an entire material study, using the existing production pose pipeline.
use std::collections::BTreeMap;

use ftx_body::{
    Affine, AnnyModel, BodyFrames, BodyParams, Legs, NappyOccupancy, Pose, RestBody, SkinMesh,
    TAPE_FIELDS, Wearer, accepted_shape::AcceptedShape, clothing_skin, fit_tape, joint_rotations,
    partial_tape_readings, pose_deltas, skin_mesh, tape_targets,
};
use ftx_core::{Mat3, Refusal, Vec3, det};
use ftx_pattern::{instance::PyError, validate::BodyMeasure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{budget, hash, invalid, motion};
use crate::run::DrapeBody;

/// Body evidence and an independently authored pose. No material can alter this request.
/// Legacy measurement documents still select a single tape fit at the standard measurement pose.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyRequest {
    /// Must be `ftx-scenario-body/1`.
    pub schema: String,
    /// Optional when reusing an accepted shape. Supplied tapes are compared, never used to refit it.
    #[serde(default = "empty_measurements")]
    pub measurements: Value,
    /// Intact `ftx-body-fit/1` report for the same model. Invalid receipts never fall back to tape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_body_fit: Option<Value>,
    /// Straight-arm abduction from vertical, degrees in 0..=180. Not a motion trajectory.
    #[serde(default = "default_arm_angle")]
    pub arm_abduction_deg: f64,
    /// Selected leg posture, independent of arm abduction. Omission preserves standing.
    #[serde(default, skip_serializing_if = "Legs::is_standing")]
    pub legs: Legs,
    /// Fraction of the selected leg rotations, 0..=1. Omission preserves the complete posture.
    #[serde(
        default = "full_leg_fraction",
        skip_serializing_if = "is_full_leg_fraction"
    )]
    pub leg_fraction: f64,
}

fn empty_measurements() -> Value {
    json!({})
}
fn default_arm_angle() -> f64 {
    Pose::A_POSE_DEG
}
fn full_leg_fraction() -> f64 {
    1.0
}
fn is_full_leg_fraction(fraction: &f64) -> bool {
    *fraction == 1.0
}

impl BodyRequest {
    /// Read a legacy measurement document or an explicit body request. A misspelled schema, null
    /// accepted report or unknown request field must not quietly select a different source.
    ///
    /// # Errors
    /// Malformed request, unsupported schema, invalid pose or nonpositive/non-finite tape values.
    pub fn read(input: &Value) -> Result<Self, Refusal> {
        if !input.is_object() {
            return Err(invalid("scenario body input must be a JSON object"));
        }
        let request = match input["schema"].as_str() {
            Some("ftx-scenario-body/1") => {
                if input.get("accepted_body_fit").is_some_and(Value::is_null) {
                    return Err(invalid(
                        "an explicit accepted_body_fit must be an intact report",
                    ));
                }
                serde_json::from_value::<Self>(input.clone()).map_err(|e| invalid(e.to_string()))?
            }
            None | Some("fabric-measurements/1") => {
                if [
                    "accepted_body_fit",
                    "arm_abduction_deg",
                    "legs",
                    "leg_fraction",
                ]
                .iter()
                .any(|k| input.get(k).is_some())
                    || input.get("schema").is_some_and(|v| !v.is_string())
                {
                    return Err(invalid(
                        "body selection and pose require ftx-scenario-body/1",
                    ));
                }
                Self {
                    schema: "ftx-scenario-body/1".into(),
                    measurements: input.clone(),
                    accepted_body_fit: None,
                    arm_abduction_deg: default_arm_angle(),
                    legs: Legs::Standing,
                    leg_fraction: 1.0,
                }
            }
            _ => {
                return Err(invalid(
                    "expected fabric-measurements/1 or ftx-scenario-body/1",
                ));
            }
        };
        request.validate()?;
        Ok(request)
    }

    /// Validate without fitting or evaluating model geometry.
    ///
    /// # Errors
    /// Invalid schema, angle, evidence document or supplied target.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-scenario-body/1" || !self.measurements.is_object() {
            return Err(invalid(
                "scenario body needs ftx-scenario-body/1 and a measurements object",
            ));
        }
        if !self.arm_abduction_deg.is_finite() || !(0.0..=180.0).contains(&self.arm_abduction_deg) {
            return Err(invalid(
                "scenario arm abduction must be finite and in 0..=180 degrees",
            ));
        }
        if !(0.0..=1.0).contains(&self.leg_fraction) {
            return Err(invalid("scenario leg fraction must be finite and in 0..=1"));
        }
        let m = &self.measurements;
        if m.get("schema")
            .is_some_and(|v| v != "fabric-measurements/1")
            || m.get("wearer").is_some_and(|v| !v.is_object())
            || m.get("fused").is_some_and(|v| !v.is_object())
        {
            return Err(invalid("invalid scenario measurement document"));
        }
        for name in TAPE_FIELDS {
            if let Some(v) = m["fused"].get(name) {
                positive(&v["value_mm"], name)?;
            }
        }
        for name in ["stature_mm", "weight_kg", "bmi"] {
            if let Some(v) = m["wearer"].get(name).filter(|v| !v.is_null()) {
                positive(v, name)?;
            }
        }
        if let Some(v) = m["wearer"].get("age_months").filter(|v| !v.is_null())
            && !v
                .as_f64()
                .is_some_and(|n| n.is_finite() && (0.0..=1320.0).contains(&n))
        {
            return Err(invalid(
                "scenario wearer age must be finite and in 0..=1320 months",
            ));
        }
        if let Some(v) = m["wearer"].get("sex").filter(|v| !v.is_null())
            && !v.is_string()
        {
            return Err(invalid("scenario wearer sex must be a string"));
        }
        Ok(())
    }
}

fn positive(value: &Value, name: &str) -> Result<f64, Refusal> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| invalid(format!("{name} must be a finite positive measurement")))
}

fn parameters(params: &BodyParams) -> Value {
    let p = params.phenotype;
    json!({"provenance":"MODEL_FIT", "phenotype":{"age":p.age,"gender":p.gender,
        "height":p.height,"weight":p.weight,"muscle":p.muscle,"proportions":p.proportions},
        "local_changes":params.local_changes})
}

/// An owned, fixed posed body shared by all study alternatives. No material enters body fitting.
pub struct FittedBody {
    frames: BodyFrames,
    collision: SkinMesh,
    display: SkinMesh,
    arm_deg: f64,
    // Motion begins at this exact selected pose, including legs; public report edits cannot alter it.
    initial_pose: Pose,
    params: BodyParams,
    // This geometric proxy never replaces the source shape or inherits edits to the public report.
    nappy_occupancy: Option<NappyOccupancy>,
    model_sha256: String,
    // Freeze source identity separately from the public display report. Lazy flat measurements
    // must not inherit later edits to report text or a different set of supplied tape targets.
    flat_source: Value,
    /// Actual body-fitting residuals and source identity; the fit is model-derived, not measured.
    pub report: Value,
}

/// Flat-pattern measurement authority from one selected model shape in its reference A-pose.
/// Values are model predictions in millimetres, not acquired wearer measurements. Missing tape
/// sections stay unavailable; no dress-form formula or supplied tape replaces the selected shape.
pub struct FlatFitBody {
    frames: BodyFrames,
    values: BTreeMap<String, f64>,
    identity: Value,
}

impl FlatFitBody {
    /// Exact model, selected shape, source evidence, reference geometry and measurement identities.
    #[must_use]
    pub fn identity(&self) -> &Value {
        &self.identity
    }
}

impl BodyMeasure for FlatFitBody {
    fn value(&self, name: &str) -> Result<f64, PyError> {
        self.values
            .get(name)
            .copied()
            .ok_or_else(|| PyError::Key(name.into()))
    }

    fn sigma(&self, _: &str) -> Option<f64> {
        // Input-tape uncertainty is not propagated through body fitting or geometric sections.
        None
    }

    fn landmark_h(&self, part_key: &str, name: &str) -> Result<f64, PyError> {
        crate::scene::PartFrames::landmark_h(&self.frames, part_key, name)
            .filter(|value| value.is_finite())
            .ok_or_else(|| PyError::Key(format!("{part_key}/{name}")))
    }
}

impl ftx_pattern::draft::DraftBody for FlatFitBody {
    fn get(&self, name: &str) -> Result<f64, Refusal> {
        let value = if name == "body_rise" {
            // Drafting rise is the vertical waist-to-crotch distance of this same reference
            // body, not the chart's analytic form or a supplied target that was not fitted.
            BodyMeasure::landmark_h(self, "torso", "waist").and_then(|waist| {
                BodyMeasure::landmark_h(self, "torso", "crotch").map(|crotch| waist - crotch)
            })
        } else {
            BodyMeasure::value(self, name)
        }
        .map_err(|_| {
            invalid(format!(
                "selected body has no drafting measurement '{name}'"
            ))
        })?;
        if !value.is_finite() || value <= 0.0 {
            return Err(invalid(format!(
                "selected body drafting measurement '{name}' must be finite and positive"
            )));
        }
        Ok(value)
    }

    fn landmark_h(&self, part: &str, name: &str) -> Result<f64, Refusal> {
        BodyMeasure::landmark_h(self, part, name)
            .map_err(|_| invalid(format!("selected body has no landmark '{part}/{name}'")))
    }

    fn frame_origin(&self, part: &str) -> Result<Vec3, Refusal> {
        self.frames
            .frame(part)
            .map(|frame| frame.origin)
            .filter(|origin| origin.is_finite())
            .ok_or_else(|| invalid(format!("selected body has no part frame '{part}'")))
    }

    fn section_radius(&self, part: &str, h_mm: f64) -> Result<f64, Refusal> {
        let radius = self.frames.section_radius(part, h_mm);
        if !h_mm.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return Err(invalid(format!(
                "selected body has no positive section radius for '{part}' at {h_mm} mm"
            )));
        }
        Ok(radius)
    }
}

impl FittedBody {
    /// Fit from tape once and prepare the standard straight-arm 35-degree drape pose.
    /// Uses the same model, fitting, trimming and landmark functions as the existing fold-up.
    ///
    /// # Errors
    /// Missing age, invalid measurements/model, failed fit, or refused body geometry.
    pub fn fit(model: &AnnyModel, measurements: &Value) -> Result<Self, Refusal> {
        let request = BodyRequest {
            schema: "ftx-scenario-body/1".into(),
            measurements: measurements.clone(),
            accepted_body_fit: None,
            arm_abduction_deg: default_arm_angle(),
            legs: Legs::Standing,
            leg_fraction: 1.0,
        };
        Self::prepare(model, "", &request)
    }

    /// Select the shared shape once, then pose that unchanged shape. The model hash must come from
    /// the exact decoded bytes. Accepted image-frame poses are deliberately never used for draping.
    ///
    /// # Errors
    /// Invalid evidence/model, failed fitting, inconsistent accepted receipt, or refused geometry.
    pub fn prepare(
        model: &AnnyModel,
        model_sha256: &str,
        request: &BodyRequest,
    ) -> Result<Self, Refusal> {
        request.validate()?;
        let (params, mut report) = if let Some(accepted) = &request.accepted_body_fit {
            accepted_shape(model, model_sha256, accepted, &request.measurements)?
        } else {
            tape_shape(model, &request.measurements)?
        };
        let occupancy = ftx_body::nappy_occupancy(model, &params, &request.measurements)?;
        let occupied_params = occupancy
            .as_ref()
            .map_or(&params, |occupied| &occupied.params);
        let mut pose = Pose::drape();
        pose.arm_abduction_deg = Some(request.arm_abduction_deg);
        pose.legs = request.legs;
        pose.leg_fraction = request.leg_fraction;
        let rest = model.rest(occupied_params)?;
        let rest_collision = clothing_skin(model, &rest.vertices)?;
        let posed = model.pose(&rest, &pose_deltas(model, &rest, &pose, 1.0)?)?;
        let collision = clothing_skin(model, &posed.vertices)?;
        let display = skin_mesh(&posed.vertices, &model.data().faces)?;
        let frames = BodyFrames::new(model, &posed)?;
        report["measurements_sha256"] = json!(hash(&request.measurements)?);
        report["shape_parameters"] = parameters(&params);
        report["shape_sha256"] = json!(hash(&(model_sha256, parameters(&params)))?);
        if let Some(occupied) = &occupancy {
            report["nappy_occupancy"] = occupied.description.clone();
            report["readings_mm"]["seat_over_nappy"] = json!(occupied.seat_over_nappy_mm);
            report["occupied_shape_sha256"] =
                json!(hash(&(model_sha256, parameters(&occupied.params)))?);
        }
        report["pose"] =
            json!({"kind":"standing_straight_arm", "arm_abduction_deg":request.arm_abduction_deg});
        if !request.legs.is_standing() {
            report["pose"]["kind"] = json!("straight_arm_with_leg_posture");
            report["pose"]["legs"] = json!(request.legs);
        }
        if request.leg_fraction != 1.0 {
            report["pose"]["leg_fraction"] = json!(request.leg_fraction);
        }
        report["body_provenance"] = json!("MODEL_FIT");
        report["body_contact_proxy"] = collision.contact_proxy_basis_with_rest(&rest_collision);
        report["physical_validation_claim"] = json!(false);
        let flat_source = json!({"shape_source":report["shape_source"],
            "input_measurements_sha256":report["measurements_sha256"]});
        Ok(Self {
            frames,
            collision,
            display,
            arm_deg: request.arm_abduction_deg,
            initial_pose: pose,
            params,
            nappy_occupancy: occupancy,
            model_sha256: model_sha256.into(),
            flat_source,
            report,
        })
    }

    /// Lazily measure this unchanged selected shape in the standard A-pose at 35 degrees.
    /// The ordinary scenario pose and any image-frame poses do not change flat-fit targets.
    /// Stature remains available from model anthropometry. Each valid tape reading survives
    /// unrelated missing sections; unavailable anatomy never receives an inferred replacement.
    ///
    /// # Errors
    /// The supplied model digest does not match the selected shape, or reference geometry,
    /// landmarks or model stature cannot be computed.
    pub fn flat_fit_body(
        &self,
        model: &AnnyModel,
        model_sha256: &str,
    ) -> Result<FlatFitBody, Refusal> {
        if self.model_sha256.is_empty() || self.model_sha256 != model_sha256 {
            return Err(invalid(
                "flat-fit measurements must use the source-identified selected body model",
            ));
        }
        let rest = model.rest(&self.params)?;
        let stature_m = model.anthropometry(&rest).height;
        let stature_mm = stature_m * 1000.0;
        if !stature_mm.is_finite() || stature_mm <= 0.0 {
            return Err(invalid("flat-fit model stature is unavailable"));
        }
        let posed = model.pose(&rest, &pose_deltas(model, &rest, &Pose::a_pose(35.0), 1.0)?)?;
        let frames = BodyFrames::new(model, &posed)?;
        let faces: Vec<_> = model
            .data()
            .faces
            .iter()
            .map(|face| face.map(|index| index as usize))
            .collect();
        let readings = partial_tape_readings(&faces, stature_m, &posed, &frames);
        let mut values = BTreeMap::from([("stature".into(), stature_mm)]);
        values.extend(
            TAPE_FIELDS
                .into_iter()
                .zip(readings)
                .filter_map(|(name, value)| value.map(|value| (name.into(), value))),
        );
        if let Some(occupied) = &self.nappy_occupancy {
            values.insert("seat_over_nappy".into(), occupied.seat_over_nappy_mm);
        }
        let mut identity = json!({"schema":"ftx-flat-fit-body/1","provenance":"MODEL_FIT",
            "model_sha256":model_sha256,
            "shape_sha256":hash(&(model_sha256, parameters(&self.params)))?,
            "shape_source":self.flat_source["shape_source"],
            "input_measurements_sha256":self.flat_source["input_measurements_sha256"],
            "measurement_pose":"a_pose_35",
            "measurement_geometry_sha256":hash(&(&posed.vertices, &faces))?,
            "measurement_values_sha256":hash(&values)?,
            "available_measurements":values.keys().collect::<Vec<_>>(),
            "complete_tape_sections":readings.iter().all(Option::is_some),
            "uncertainty_propagated":false,"fit_claim":false});
        if let Some(occupied) = &self.nappy_occupancy {
            identity["nappy_occupancy"] = occupied.description.clone();
            identity["occupied_shape_sha256"] =
                json!(hash(&(model_sha256, parameters(&occupied.params)))?);
        }
        Ok(FlatFitBody {
            frames,
            values,
            identity,
        })
    }

    /// Generate motion endpoints from the SAME selected shape, without fitting again. The caller
    /// supplies the model digest from its decoded bytes just as for `prepare`. Endpoint generation
    /// uses Anny poses and optional absolute world transforms. Requested intermediate source poses
    /// interpolate supported joint angles and shortest-arc root rotations, never skin vertices or
    /// rotation matrices. The solver checks linear chords between the resulting source surfaces.
    ///
    /// # Errors
    /// Invalid/excessive trajectory, changed model identity or nonmatching collision topology.
    pub fn motion_path(
        &self,
        model: &AnnyModel,
        model_sha256: &str,
        request: &motion::Request,
    ) -> Result<motion::Path, Refusal> {
        request.validate()?;
        if self.model_sha256.is_empty() || self.model_sha256 != model_sha256 {
            return Err(invalid(
                "motion must use the source-identified selected body model",
            ));
        }
        let steps = request.source_steps()?;
        if self
            .collision
            .vertices
            .len()
            .saturating_mul(steps.len() + 1)
            > 2_000_000
        {
            return Err(budget("motion endpoint body-vertex budget exceeded"));
        }
        let params = self
            .nappy_occupancy
            .as_ref()
            .map_or(&self.params, |occupied| &occupied.params);
        let rest = model.rest(params)?;
        let mut endpoints = Vec::with_capacity(steps.len());
        let mut previous_pose = self.initial_pose;
        let mut previous_rotation = None;
        let mut previous_translation = Vec3::ZERO;
        for (interval, keyframe) in request.keyframes.iter().enumerate() {
            let mut pose = Pose::drape();
            pose.arm_abduction_deg = Some(keyframe.arm_abduction_deg);
            pose.arm_pose = keyframe.arm_pose;
            if let Some(legs) = keyframe.leg_pose {
                pose.legs = match legs.posture {
                    motion::LegPosture::Seated => Legs::Seated,
                    motion::LegPosture::Squat => Legs::Squat,
                };
                // Scale only the source leg rotations; arm abduction and straightening stay fixed.
                pose.leg_fraction = legs.fraction;
            }
            let interval_steps: Vec<_> = steps
                .iter()
                .filter(|step| step.interval == interval)
                .collect();
            let rotations = if interval_steps.len() > 1 {
                Some((
                    source_rotvecs(model, &rest, &previous_pose)?,
                    source_rotvecs(model, &rest, &pose)?,
                ))
            } else {
                None
            };
            let translation = keyframe.root_translation_m.map_or(Vec3::ZERO, Vec3::from);
            for step in interval_steps {
                let exact = step.fraction == 1.0;
                let deltas = if exact {
                    // Keep the established arithmetic at every authored endpoint, including the
                    // absent-policy path, rather than reconstructing it from an interpolation.
                    pose_deltas(model, &rest, &pose, 1.0)?
                } else {
                    let (from, to) = rotations.as_ref().expect("intermediate source poses");
                    from.iter()
                        .zip(to)
                        .map(|(&from, &to)| {
                            Affine::from_rotvec(lerp_vec(from, to, step.fraction), Vec3::ZERO)
                        })
                        .collect()
                };
                let mut posed = model.pose(&rest, &deltas)?;
                let rotation = if exact || previous_rotation == keyframe.root_rotation_rad {
                    keyframe.root_rotation_matrix()?
                } else {
                    Some(root_rotation_between(
                        previous_rotation,
                        keyframe.root_rotation_rad,
                        step.fraction,
                    ))
                };
                if let Some(rotation) = rotation {
                    for vertex in &mut posed.vertices {
                        *vertex = rotation.mul_vec(*vertex);
                    }
                }
                let displacement = if exact {
                    translation
                } else {
                    lerp_vec(previous_translation, translation, step.fraction)
                };
                if displacement != Vec3::ZERO {
                    // Transform the complete source surface before deriving collision caps. All
                    // samples share the selected body's fixed root-relative source origin.
                    for vertex in &mut posed.vertices {
                        *vertex += displacement;
                    }
                }
                let skin = clothing_skin(model, &posed.vertices)?;
                if skin.triangles != self.collision.triangles
                    || skin.source_vertex != self.collision.source_vertex
                {
                    return Err(invalid(
                        "motion endpoints must preserve the selected collision source mapping and topology",
                    ));
                }
                endpoints.push(skin.vertices);
            }
            previous_pose = pose;
            previous_rotation = keyframe.root_rotation_rad;
            previous_translation = translation;
        }
        motion::Path::new(request, self.body().collision, endpoints)
    }

    /// Borrow the same immutable body for each fresh cloth simulation.
    #[must_use]
    pub fn body(&self) -> DrapeBody<'_> {
        DrapeBody {
            frames: &self.frames,
            collision: (&self.collision.vertices, &self.collision.triangles),
            display: (&self.display.vertices, &self.display.triangles),
            arm_deg: self.arm_deg,
        }
    }
}

// Existing supported pose controls each rotate a bone on a fixed rest-frame axis. Interpolate
// their scalar rotations in local coordinates, then run the ordinary source FK/LBS evaluation.
fn source_rotvecs(model: &AnnyModel, rest: &RestBody, pose: &Pose) -> Result<Vec<Vec3>, Refusal> {
    let mut rotations = vec![Vec3::ZERO; rest.bone_frames.len()];
    for joint in joint_rotations(model, rest, pose)? {
        rotations[joint.bone] = rest.bone_frames[joint.bone]
            .m
            .transpose()
            .mul_vec(joint.axis)
            * joint.angle;
    }
    Ok(rotations)
}

fn lerp_vec(from: Vec3, to: Vec3, fraction: f64) -> Vec3 {
    if from == to {
        from
    } else {
        from * (1.0 - fraction) + to * fraction
    }
}

// Unit quaternions avoid the collapsed halfway geometry of a matrix/vertex blend and choose the
// short orientation arc across the equivalent +pi/-pi rotation-vector boundary. Exact authored
// endpoints continue through Keyframe::root_rotation_matrix above.
fn root_rotation_between(from: Option<[f64; 3]>, to: Option<[f64; 3]>, fraction: f64) -> Mat3 {
    fn quaternion(rotation: Option<[f64; 3]>) -> (f64, Vec3) {
        let vector = rotation.map_or(Vec3::ZERO, Vec3::from);
        let scale = vector.x.abs().max(vector.y.abs()).max(vector.z.abs());
        if scale == 0.0 {
            return (1.0, Vec3::ZERO);
        }
        let scaled = vector / scale;
        let length = scaled.norm();
        let (sin, cos) = det::sin_cos(scale * length * 0.5);
        (cos, scaled * (sin / length))
    }
    let (a, av) = quaternion(from);
    let (mut b, mut bv) = quaternion(to);
    let mut dot = a * b + av.dot(bv);
    if dot < 0.0 {
        b = -b;
        bv = -bv;
        dot = -dot;
    }
    let (wa, wb) = if dot > 0.9995 {
        (1.0 - fraction, fraction)
    } else {
        let angle = det::acos(dot.clamp(0.0, 1.0));
        let denominator = det::sin_cos(angle).0;
        (
            det::sin_cos((1.0 - fraction) * angle).0 / denominator,
            det::sin_cos(fraction * angle).0 / denominator,
        )
    };
    let scalar = a * wa + b * wb;
    let vector = av * wa + bv * wb;
    let norm = (scalar * scalar + vector.dot(vector)).sqrt();
    let scalar = scalar / norm;
    let vector = vector / norm;
    let rotate = |point: Vec3| {
        let cross = vector.cross(point) * 2.0;
        point + cross * scalar + vector.cross(cross)
    };
    Mat3::from_cols(
        rotate(Vec3::new(1.0, 0.0, 0.0)),
        rotate(Vec3::new(0.0, 1.0, 0.0)),
        rotate(Vec3::new(0.0, 0.0, 1.0)),
    )
}

// Shape estimation always uses the existing reference pose, independent of the requested pose.
fn tape_shape(model: &AnnyModel, measurements: &Value) -> Result<(BodyParams, Value), Refusal> {
    if !measurements["wearer"]["age_months"].is_number() {
        return Err(invalid("scenario body fitting requires wearer.age_months"));
    }
    let pose = Pose::drape();
    let wearer = Wearer::from_measurements(measurements)?;
    let fit = fit_tape(model, &wearer, &tape_targets(measurements)?, &pose)?;
    let residuals: Vec<_> = fit
        .residuals
        .iter()
        .map(|r| {
            json!({"name":r.name,
            "target_mm":r.target_mm,"body_mm":r.body_mm,"residual_mm":r.body_mm-r.target_mm,
            "sigma_mm":r.sigma_mm,"effective_sigma_mm":r.effective_sigma_mm,
            "normalized_residual":(r.body_mm-r.target_mm)/r.effective_sigma_mm})
        })
        .collect();
    let readings: serde_json::Map<_, _> = TAPE_FIELDS
        .iter()
        .zip(fit.readings)
        .filter_map(|(name, value)| value.map(|value| (name.to_string(), json!(value))))
        .collect();
    let unavailable = fit.unavailable_fields();
    let report = json!({"measurement_pose":"standard_straight_arm_drape_35_degrees",
            "shape_source":{"kind":"tape_fit","refitted":true},"fit_iterations":fit.iterations,
            "body_mass_kg":fit.mass_kg,"residuals":residuals,"body_provenance":"MODEL_FIT",
            "readings_mm":readings,"tape_evaluation":{
                "status":if unavailable.is_empty() { "AVAILABLE" } else { "PARTIAL" },
                "pose":"a_pose_35","provenance":"MODEL_FIT","unavailable_fields":unavailable},
            "physical_validation_claim":false});
    Ok((fit.params, report))
}

fn accepted_shape(
    model: &AnnyModel,
    model_sha256: &str,
    accepted: &Value,
    measurements: &Value,
) -> Result<(BodyParams, Value), Refusal> {
    if model.data().header.model != "anny" || model.data().header.anny_version != "0.6.1" {
        return Err(invalid(
            "accepted scenario shapes require the pinned Anny 0.6.1 model",
        ));
    }
    let shape = AcceptedShape::read(
        accepted,
        model_sha256,
        &model.data().header.labels.local_changes,
    )?;
    let wearer = &measurements["wearer"];
    if let Some(age) = wearer["age_months"].as_f64()
        && (age / 12.0 - shape.years).abs() > 1e-10
    {
        return Err(invalid(
            "measurement wearer age differs from the accepted body fit; no refit",
        ));
    }
    if let Some(sex) = wearer["sex"].as_str()
        && Wearer::gender_of(Some(sex)) != shape.params.phenotype.gender
    {
        return Err(invalid(
            "measurement wearer sex differs from the accepted body fit; no refit",
        ));
    }
    let rest = model.rest(&shape.params)?;
    let dimensions = model.anthropometry(&rest);
    if !dimensions.height.is_finite()
        || dimensions.height <= 0.0
        || !dimensions.mass.is_finite()
        || dimensions.mass <= 0.0
    {
        return Err(invalid(
            "accepted scenario body has invalid recomputed dimensions",
        ));
    }
    // Match the existing accepted-body adapter's measurement pose, not its image poses or the
    // requested visualization angle. Missing sections are unavailable residuals, never zeros.
    let readings: Result<_, Refusal> = (|| {
        let posed = model.pose(&rest, &pose_deltas(model, &rest, &Pose::a_pose(35.0), 1.0)?)?;
        let frames = BodyFrames::new(model, &posed)?;
        let faces: Vec<_> = model
            .data()
            .faces
            .iter()
            .map(|f| f.map(|i| i as usize))
            .collect();
        Ok(partial_tape_readings(
            &faces,
            dimensions.height,
            &posed,
            &frames,
        ))
    })();
    let comparisons: Vec<_> = tape_targets(measurements)?
        .iter()
        .map(|target| {
            let body_mm = if target.name == "stature" {
                Some(dimensions.height * 1000.0)
            } else {
                readings.as_ref().ok().and_then(|values| {
                    TAPE_FIELDS
                        .iter()
                        .position(|&n| n == target.name)
                        .and_then(|i| values[i])
                })
            };
            json!({"name":target.name,"target_mm":target.value_mm,"body_mm":body_mm,
            "sigma_mm":target.sigma_mm,
            "residual_mm":body_mm.map(|v| v-target.value_mm),"used_to_refit":false})
        })
        .collect();
    let report = json!({"shape_source":{"kind":"accepted_multi_frame_fit",
        "report_sha256":hash(accepted)?,"input_sha256":shape.input_sha256,
        "refitted":false,"image_poses_reused":false}, "source_fit":accepted,
        "fit_iterations":0,"measurement_pose":"a_pose_35", "body_mass_kg":dimensions.mass,
        "residuals":comparisons,"tape_evaluation":match &readings {
            Ok(values) if values.iter().all(Option::is_some) => json!({"status":"available"}),
            Ok(values) => json!({"status":"partial","unavailable_fields":TAPE_FIELDS
                .iter().zip(values).filter_map(|(name,value)|value.is_none().then_some(*name))
                .collect::<Vec<_>>()}),
            Err(e) => json!({"status":"refused","refusal":e}),
        }, "no_claim":"Accepted shape reused without refitting; receipt consistency is not authenticity or physical fit. MODEL_FIT predictions are not new measurements."});
    Ok((shape.params, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_body::{AnnyData, Phenotype};
    use ftx_core::{Mat3, Vec3};

    // A closed prism with a tiny articulated rig, not an anatomical model or measured wearer.
    // Its lack of tape-fitting local changes also proves that accepted reuse cannot refit silently.
    fn model() -> AnnyModel {
        let labels = [
            "root",
            "upperarm01.R",
            "upperarm02.R",
            "lowerarm01.R",
            "wrist.R",
            "upperarm01.L",
            "upperarm02.L",
            "lowerarm01.L",
            "wrist.L",
            "upperleg01.R",
            "lowerleg01.R",
            "foot.R",
            "upperleg01.L",
            "lowerleg01.L",
            "foot.L",
            "neck01",
            "head",
        ];
        let parents = [-1, 0, 1, 2, 3, 0, 5, 6, 7, 0, 9, 10, 0, 12, 13, 0, 15];
        let mut vertices = Vec::new();
        let mut influences = Vec::new();
        for (r, z) in [0.0, 0.6, 1.2, 1.7].into_iter().enumerate() {
            for i in 0..16 {
                let (s, c) = ftx_core::det::sin_cos(i as f64 * std::f64::consts::TAU / 16.0);
                vertices.push(Vec3::new(0.14 * c, 0.14 * s, z));
                influences.push(if r == 3 && [15, 0, 1].contains(&i) {
                    1
                } else if r == 3 && [7, 8, 9].contains(&i) {
                    5
                } else if r == 0 && c > 0.5 {
                    9
                } else if r == 0 && c < -0.5 {
                    12
                } else {
                    0
                });
            }
        }
        vertices.extend([Vec3::ZERO, Vec3::new(0.0, 0.0, 1.7)]);
        influences.extend([0, 0]);
        let mut faces = Vec::new();
        for i in 0..16u32 {
            let j = (i + 1) % 16;
            faces.extend([[64, j, i], [65, 48 + i, 48 + j]]);
            for r in 0..3 {
                let a = 16 * r + i;
                let b = 16 * r + j;
                faces.extend([[a, b, b + 16], [a, b + 16, a + 16]]);
            }
        }
        let mut heads = vec![Vec3::ZERO];
        let mut tails = vec![Vec3::new(0.0, 0.0, 0.1)];
        for sign in [1.0, -1.0] {
            for j in 0..4 {
                let h = Vec3::new(sign * (0.12 + 0.05 * j as f64), 0.0, 1.2 - 0.08 * j as f64);
                heads.push(h);
                tails.push(h + Vec3::new(sign * 0.05, 0.0, -0.08));
            }
        }
        for sign in [1.0, -1.0] {
            for z in [0.6, 0.3, 0.05] {
                heads.push(Vec3::new(sign * 0.06, 0.0, z));
                tails.push(Vec3::new(sign * 0.06, 0.0, z - 0.05));
            }
        }
        heads.extend([Vec3::new(0.0, 0.0, 1.4), Vec3::new(0.0, 0.0, 1.5)]);
        tails.extend([Vec3::new(0.0, 0.0, 1.5), Vec3::new(0.0, 0.0, 1.6)]);
        let delta = |v: &Vec3| Vec3::new(0.0, 0.0, 0.5 * v.z);
        let mut components = vec![json!(["height", "low"]), json!(["height", "high"])];
        components.extend((0..24).map(|i| json!(["race", format!("r{i}")])));
        let mut mask = vec![0; 26];
        mask[1] = 1;
        let n = vertices.len();
        let b = labels.len();
        AnnyModel::from_data(AnnyData {
            header: serde_json::from_value(json!({"schema":"ftxbody/1","model":"anny","anny_version":"0.6.1",
                "configuration":"synthetic articulated prism","licence":"synthetic","package_sha256":{},
                "counts":{"vertices":n,"faces":faces.len(),"bones":b,"blendshapes":1,"phenotype_blendshapes":1,"local_changes":0,"influences":1},
                "phenotype":{"features":["height","race"],"components":components,"anchors":{"height":[0,1]},"exposed":["height"],"fixed_value":0.5},
                "labels":{"bones":labels,"bone_parents":parents,"blendshapes":["height-high"],"local_changes":[]},
                "y_axis":[0,1,0],"degenerate_rotation":[[1,0,0],[0,1,0],[0,0,1]],"waist_loop_final":(16..32).collect::<Vec<_>>(),"arrays":{}})).unwrap(),
            blendshape_delta:vertices.iter().map(delta).collect(),
            bone_heads_blendshapes:heads.iter().map(delta).collect(),
            bone_tails_blendshapes:tails.iter().map(delta).collect(),
            template_vertices:vertices,faces,blendshape_row_ptr:vec![0,n as u32],
            blendshape_vertex:(0..n as u32).collect(),phenotype_mask:mask,
            template_bone_heads:heads,template_bone_tails:tails,bone_rolls:vec![Mat3::IDENTITY;b],
            vertex_bone_indices:influences,vertex_bone_weights:vec![1.0;n],
            base_mesh_vertex_indices:(0..n as u32).collect(),
        }).unwrap()
    }

    fn request() -> BodyRequest {
        let model = model();
        let params = model.params(Phenotype {
            gender: 1.0,
            age: ftx_body::age_parameter_for_years(2.5),
            height: 0.6,
            ..Phenotype::default()
        });
        let hash = "a".repeat(64);
        let variable = json!([{"name":"height","lower":0.0,"upper":1.0}]);
        let attempt = json!({"status":"CONVERGED","flags":[],"checks":[{"unit":"px","residual":0.4,"limit":3.0,"passed":true}]});
        BodyRequest::read(&json!({"schema":"ftx-scenario-body/1","accepted_body_fit":{
            "schema":"ftx-body-fit/1","status":"ACCEPTED","input_sha256":"b".repeat(64),
            "input":{"starts":[{},{}],"frames":[{},{}]},"attempts":[attempt,attempt],
            "options":{"max_shape_spread_fraction":0.01},"shape_spread_fraction":[0.0],
            "model":{"model_id":format!("anny/0.6.1/sha256:{hash}"),"shape":variable},
            "anny_configuration":{"model_sha256":hash,"age_years":2.5,"gender":1.0,"fixed_shape":{},"shape_parameters":variable},
            "solution":{"shape":[0.6],"poses":[{"deliberately_not_a_drape_pose":1000}],"anny_parameters":parameters(&params)}}})).unwrap()
    }

    #[test]
    fn accepted_shape_reposes_without_refitting_or_reusing_image_poses() {
        let model = model();
        let hash = "a".repeat(64);
        let mut req = request();
        let a = FittedBody::prepare(&model, &hash, &req).unwrap();
        req.arm_abduction_deg = 50.0;
        let b = FittedBody::prepare(&model, &hash, &req).unwrap();
        assert_eq!(a.report["shape_sha256"], b.report["shape_sha256"]);
        assert_eq!(
            a.report["shape_parameters"],
            req.accepted_body_fit.as_ref().unwrap()["solution"]["anny_parameters"]
        );
        assert_eq!(a.report["shape_source"]["refitted"], false);
        assert_eq!(a.report["shape_source"]["image_poses_reused"], false);
        assert_eq!(a.report["residuals"], b.report["residuals"]);
        assert_ne!(a.collision.vertices, b.collision.vertices);
        assert_eq!(a.collision.triangles, b.collision.triangles);
        assert_eq!(b.body().arm_deg, 50.0);
        let again = FittedBody::prepare(&model, &hash, &req).unwrap();
        assert_eq!(b.collision.vertices, again.collision.vertices);
        assert_eq!(b.report, again.report);
    }

    #[test]
    fn flat_fit_body_uses_selected_model_values_and_keeps_missing_sections_unavailable() {
        let model = model();
        let digest = "a".repeat(64);
        let mut request = request();
        request.measurements = json!({"wearer":{"stature_mm":1000.0},
            "fused":{"waist":{"value_mm":500.0,"sigma_mm":5.0}}});
        let fitted = FittedBody::prepare(&model, &digest, &request).unwrap();
        let report = fitted.report.clone();
        let flat = fitted.flat_fit_body(&model, &digest).unwrap();
        // The fixture's 1.7 m rest height has one height blend: 1.7 * (1 + 0.5 * 0.6).
        // The disagreeing supplied tape is retained as evidence, never substituted or fitted.
        assert!((flat.value("stature").unwrap() - 2210.0).abs() < 1e-9);
        assert_eq!(flat.sigma("stature"), None);
        assert_eq!(flat.identity()["complete_tape_sections"], false);
        // The untouched waist is a regular 16-gon of radius 140 mm, independent of height.
        let waist = 32.0 * 140.0 * ftx_core::det::sin_cos(std::f64::consts::PI / 16.0).0;
        assert!((flat.value("waist").unwrap() - waist).abs() < 1e-9);
        assert_eq!(flat.sigma("waist"), None);
        assert!(
            flat.identity()["available_measurements"]
                .as_array()
                .unwrap()
                .contains(&json!("waist"))
        );
        assert_eq!(report["tape_evaluation"]["status"], "partial");
        let waist_residual = report["residuals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "waist")
            .unwrap();
        assert!((waist_residual["body_mm"].as_f64().unwrap() - waist).abs() < 1e-9);
        assert!((waist_residual["residual_mm"].as_f64().unwrap() - (waist - 500.0)).abs() < 1e-9);
        assert_eq!(waist_residual["used_to_refit"], false);
        let (_, description) = ftx_body::accepted_shape::prepare(
            &model,
            &digest,
            &serde_json::to_vec(request.accepted_body_fit.as_ref().unwrap()).unwrap(),
            Some(&request.measurements),
        )
        .unwrap();
        assert_eq!(description["tape_evaluation"]["status"], "PARTIAL");
        assert_eq!(description["tape_evaluation"]["provenance"], "MODEL_FIT");
        assert_eq!(
            description["tape_evaluation"]["unavailable_fields"],
            report["tape_evaluation"]["unavailable_fields"]
        );
        assert!((description["readings_mm"]["waist"].as_f64().unwrap() - waist).abs() < 1e-9);
        for name in report["tape_evaluation"]["unavailable_fields"]
            .as_array()
            .unwrap()
        {
            assert!(flat.value(name.as_str().unwrap()).is_err());
            assert!(
                description["readings_mm"]
                    .get(name.as_str().unwrap())
                    .is_none()
            );
        }
        assert!(flat.value("hand_pass").is_err());
        assert!(flat.landmark_h("torso", "unknown").is_err());
        assert!(flat.landmark_h("unknown", "waist").is_err());
        assert_eq!(flat.identity()["provenance"], "MODEL_FIT");
        assert_eq!(flat.identity()["measurement_pose"], "a_pose_35");
        assert_eq!(flat.identity()["shape_sha256"], report["shape_sha256"]);
        assert_eq!(
            flat.identity()["input_measurements_sha256"],
            json!(hash(&request.measurements).unwrap())
        );
        assert_eq!(flat.identity()["shape_source"], report["shape_source"]);
        assert_eq!(flat.identity()["uncertainty_propagated"], false);
        assert_eq!(flat.identity()["fit_claim"], false);
        assert_eq!(fitted.report, report);
        assert!(fitted.flat_fit_body(&model, &"b".repeat(64)).is_err());
        assert!(fitted.flat_fit_body(&model, "").is_err());

        let pattern = ftx_pattern::Pattern::from_json(include_str!(
            "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
        ))
        .unwrap();
        let point = json!({"id":"scaled_stature","kind":"vertical","instance":"band",
            "from":"a","to":"d","body":"stature","body_scale":0.01,
            "target_mm":[-3.0,-2.0,-1.0]});
        let row = ftx_pattern::validate::measure_point(&pattern, &point, &flat);
        assert_eq!(row.finished_mm, Some(20.0));
        assert_eq!(row.body_mm, Some(22.1));
        assert_eq!(row.ease_mm, Some(-2.1));
        assert_eq!(row.status, "IN_BAND");
        assert_eq!(row.body_sigma_mm, Some(None));
        let mut missing = point;
        missing["body"] = json!("hand_pass");
        assert_eq!(
            ftx_pattern::validate::measure_point(&pattern, &missing, &flat).status,
            "UNMEASURABLE"
        );
    }

    #[test]
    fn flat_fit_body_reference_identity_and_landmarks_do_not_follow_drape_or_report_edits() {
        let model = model();
        let digest = "a".repeat(64);
        let mut request = request();
        let mut fitted = FittedBody::prepare(&model, &digest, &request).unwrap();
        let flat = fitted.flat_fit_body(&model, &digest).unwrap();
        request.arm_abduction_deg = 50.0;
        request.legs = Legs::Seated;
        request.leg_fraction = 0.05;
        let reposed = FittedBody::prepare(&model, &digest, &request).unwrap();
        assert_ne!(fitted.collision.vertices, reposed.collision.vertices);
        let again = reposed.flat_fit_body(&model, &digest).unwrap();
        assert_eq!(flat.identity(), again.identity());

        let rest = model.rest(&fitted.params).unwrap();
        let posed = model
            .pose(
                &rest,
                &pose_deltas(&model, &rest, &Pose::a_pose(35.0), 1.0).unwrap(),
            )
            .unwrap();
        let reference = BodyFrames::new(&model, &posed).unwrap();
        for (part, reference_part, landmark) in [
            ("torso", "torso", "waist"),
            ("arm_R", "arm_R", "wrist"),
            ("arm_L", "arm_R", "wrist"),
            ("leg_R", "leg_R", "knee"),
            ("leg_L", "leg_R", "knee"),
        ] {
            let expected = reference.landmark_h(reference_part, landmark).unwrap();
            assert_eq!(flat.landmark_h(part, landmark).unwrap(), expected);
            assert_eq!(again.landmark_h(part, landmark).unwrap(), expected);
        }

        fitted.report["shape_source"] = json!({"kind":"edited-display-report"});
        fitted.report["measurements_sha256"] = json!("c".repeat(64));
        fitted.report["shape_sha256"] = json!("d".repeat(64));
        assert_eq!(
            fitted.flat_fit_body(&model, &digest).unwrap().identity(),
            flat.identity()
        );
        request.measurements = json!({"wearer":{"stature_mm":1000.0}});
        let extra_tape = FittedBody::prepare(&model, &digest, &request)
            .unwrap()
            .flat_fit_body(&model, &digest)
            .unwrap();
        assert_eq!(
            extra_tape.value("stature").unwrap(),
            flat.value("stature").unwrap()
        );
        assert_eq!(
            extra_tape.identity()["measurement_geometry_sha256"],
            flat.identity()["measurement_geometry_sha256"]
        );
        assert_ne!(
            extra_tape.identity()["input_measurements_sha256"],
            flat.identity()["input_measurements_sha256"]
        );
    }

    #[test]
    fn source_root_sampling_follows_rigid_shortest_arcs_without_chord_collapse() {
        let pi = std::f64::consts::PI;
        let point = Vec3::new(0.2, 0.0, 0.3);
        let half_turn = root_rotation_between(None, Some([0.0, 0.0, pi]), 0.5);
        let midpoint = half_turn.mul_vec(point);
        assert!((midpoint - Vec3::new(0.0, 0.2, 0.3)).norm() < 1e-15);
        assert!((midpoint.norm() - point.norm()).abs() < 1e-15);
        let old_chord_midpoint =
            (point + Mat3::rotation(Vec3::new(0.0, 0.0, 1.0), pi).mul_vec(point)) * 0.5;
        assert!((midpoint - old_chord_midpoint).norm() > 0.19);
        let equivalent = root_rotation_between(Some([0.0, 0.0, pi]), Some([0.0, 0.0, -pi]), 0.5);
        assert!((equivalent.mul_vec(point) - Vec3::new(-0.2, 0.0, 0.3)).norm() < 1e-15);
        // Independent quaternion midpoint of orthogonal quarter turns: (sqrt(2/3), 1/sqrt(6), 1/sqrt(6), 0).
        let mixed =
            root_rotation_between(Some([pi / 2.0, 0.0, 0.0]), Some([0.0, pi / 2.0, 0.0]), 0.5);
        assert!(
            (mixed.mul_vec(Vec3::new(1.0, 0.0, 0.0)) - Vec3::new(2.0 / 3.0, 1.0 / 3.0, -2.0 / 3.0))
                .norm()
                < 1e-14
        );
        for rotation in [
            half_turn,
            equivalent,
            mixed,
            root_rotation_between(None, Some([1e-9, -2e-9, 3e-9]), 0.5),
        ] {
            let x = rotation.mul_vec(Vec3::new(1.0, 0.0, 0.0));
            let y = rotation.mul_vec(Vec3::new(0.0, 1.0, 0.0));
            let z = rotation.mul_vec(Vec3::new(0.0, 0.0, 1.0));
            assert!((x.norm() - 1.0).abs() < 1e-14);
            assert!((y.norm() - 1.0).abs() < 1e-14);
            assert!(x.dot(y).abs() < 1e-14);
            assert!((x.cross(y) - z).norm() < 1e-14);
        }
    }

    #[test]
    fn sampled_source_poses_start_at_the_frozen_selected_legs_and_keep_exact_endpoints() {
        let model = model();
        let digest = "a".repeat(64);
        let mut selected = request();
        selected.legs = Legs::Seated;
        selected.leg_fraction = 0.06;
        let mut fitted = FittedBody::prepare(&model, &digest, &selected).unwrap();
        let initial_collision = fitted.collision.vertices.clone();
        let shape = fitted.report["shape_sha256"].clone();
        fitted.report["pose"] =
            json!({"legs":"standing","leg_fraction":0.0,"arm_abduction_deg":150.0});
        let mut motion: motion::Request =
            serde_json::from_value(json!({"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.004,"arm_abduction_deg":37.0,
                "leg_pose":{"posture":"seated","fraction":0.02},
                "root_rotation_rad":[0.0,0.0,std::f64::consts::FRAC_PI_2],
                "root_translation_m":[0.01,-0.02,0.03]}],
            "source_pose_sampling":{"max_step_s":0.002,"max_segments":2},
            "max_step_s":0.002,"max_trials":8,"max_pairs":100000,"max_sample_vertices":10000}))
            .unwrap();
        let path = fitted.motion_path(&model, &digest, &motion).unwrap();
        assert_eq!(path.source_vertices().len(), 2);
        let rest = model.rest(&fitted.params).unwrap();
        let mut midpoint = Pose::drape();
        midpoint.arm_abduction_deg = Some(36.0);
        midpoint.legs = Legs::Seated;
        midpoint.leg_fraction = 0.04;
        let posed = model
            .pose(&rest, &pose_deltas(&model, &rest, &midpoint, 1.0).unwrap())
            .unwrap();
        let rotation = Mat3::rotation(Vec3::new(0.0, 0.0, 1.0), std::f64::consts::FRAC_PI_4);
        let vertices: Vec<_> = posed
            .vertices
            .iter()
            .map(|p| rotation.mul_vec(*p) + Vec3::new(0.005, -0.01, 0.015))
            .collect();
        let expected = clothing_skin(&model, &vertices).unwrap();
        for (actual, expected) in path.source_vertices()[0].iter().zip(&expected.vertices) {
            assert!(
                (*actual - *expected).norm() < 1e-14,
                "{actual:?} != {expected:?}"
            );
        }
        // Exact endpoint generation remains byte-identical to the established endpoint-only path.
        motion.source_pose_sampling = None;
        let endpoint_only = fitted.motion_path(&model, &digest, &motion).unwrap();
        assert_eq!(
            path.source_vertices()[1],
            endpoint_only.source_vertices()[0]
        );
        assert_eq!(fitted.collision.vertices, initial_collision);
        assert_eq!(fitted.report["shape_sha256"], shape);
        midpoint.leg_fraction = 0.01; // A mistaken standing start would produce this midpoint.
        let wrong = model
            .pose(&rest, &pose_deltas(&model, &rest, &midpoint, 1.0).unwrap())
            .unwrap();
        let wrong: Vec<_> = wrong
            .vertices
            .iter()
            .map(|p| rotation.mul_vec(*p) + Vec3::new(0.005, -0.01, 0.015))
            .collect();
        let wrong = clothing_skin(&model, &wrong).unwrap();
        assert!(
            path.source_vertices()[0]
                .iter()
                .zip(wrong.vertices)
                .any(|(actual, wrong)| (*actual - wrong).norm() > 1e-5)
        );
    }

    #[test]
    fn independent_arm_motion_follows_source_sides_and_returns_to_each_symmetric_endpoint() {
        use ftx_body::drape::ArmPose;

        let model = model();
        let digest = "a".repeat(64);
        let fitted = FittedBody::prepare(&model, &digest, &request()).unwrap();
        let original_report = fitted.report.clone();
        let original_collision = fitted.collision.vertices.clone();
        let rest = model.rest(&fitted.params).unwrap();
        let mut asymmetric = Pose::drape();
        asymmetric.arm_pose = Some(ArmPose {
            left_abduction_deg: 70.0,
            right_abduction_deg: 25.0,
        });
        let posed = model
            .pose(
                &rest,
                &pose_deltas(&model, &rest, &asymmetric, 1.0).unwrap(),
            )
            .unwrap();
        // Independent direction check: each named source bone must reach its own authored angle.
        for (side, angle) in [("L", 70.0_f64), ("R", 25.0_f64)] {
            let bone = model.bone_index(&format!("upperarm01.{side}")).unwrap();
            let sign = rest.bone_frames[bone].m.col(1).x.signum();
            let (sin, cos) = det::sin_cos(angle.to_radians());
            let expected = Vec3::new(sign * sin, 0.0, -cos);
            assert!((posed.bone_poses[bone].m.col(1) - expected).norm() < 1e-14);
        }
        asymmetric.arm_pose.as_mut().unwrap().left_abduction_deg = 45.0;
        let changed = model
            .pose(
                &rest,
                &pose_deltas(&model, &rest, &asymmetric, 1.0).unwrap(),
            )
            .unwrap();
        let mut changed_left = false;
        let mut unchanged_right = false;
        for (i, &bone) in model.data().vertex_bone_indices.iter().enumerate() {
            if bone == 5 {
                changed_left |= changed.vertices[i] != posed.vertices[i];
            } else {
                assert_eq!(changed.vertices[i], posed.vertices[i]);
                unchanged_right |= bone == 1;
            }
        }
        assert!(changed_left && unchanged_right);

        let mut motion: motion::Request =
            serde_json::from_value(json!({"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.004,"arm_abduction_deg":35.0,
                "arm_pose":{"left_abduction_deg":37.0,"right_abduction_deg":35.0}},
                {"time_s":0.008,"arm_abduction_deg":36.0}],
            "source_pose_sampling":{"max_step_s":0.002,"max_segments":4},
            "max_step_s":0.002,"max_trials":8,"max_pairs":100000,"max_sample_vertices":10000}))
            .unwrap();
        let sampled = fitted.motion_path(&model, &digest, &motion).unwrap();
        assert_eq!(sampled.source_vertices().len(), 4);
        for (actual, (left, right)) in sampled.source_vertices().iter().zip([
            (36.0, 35.0),
            (37.0, 35.0),
            (36.5, 35.5),
            (36.0, 36.0),
        ]) {
            let mut expected = Pose::drape();
            expected.arm_pose = Some(ArmPose {
                left_abduction_deg: left,
                right_abduction_deg: right,
            });
            let posed = model
                .pose(&rest, &pose_deltas(&model, &rest, &expected, 1.0).unwrap())
                .unwrap();
            let skin = clothing_skin(&model, &posed.vertices).unwrap();
            assert_eq!(skin.triangles, fitted.collision.triangles);
            assert_eq!(skin.source_vertex, fitted.collision.source_vertex);
            for (actual, expected) in actual.iter().zip(skin.vertices) {
                assert!((*actual - expected).norm() < 1e-14);
            }
        }
        motion.source_pose_sampling = None;
        let endpoints = fitted.motion_path(&model, &digest, &motion).unwrap();
        assert_eq!(sampled.source_vertices()[1], endpoints.source_vertices()[0]);
        assert_eq!(sampled.source_vertices()[3], endpoints.source_vertices()[1]);
        let asymmetric_hash = endpoints.sha256().to_owned();
        motion.keyframes[0].arm_pose = None;
        let legacy = fitted.motion_path(&model, &digest, &motion).unwrap();
        assert_ne!(legacy.sha256(), asymmetric_hash);
        assert_eq!(legacy.source_vertices()[0], original_collision);
        assert_eq!(legacy.source_vertices()[1], endpoints.source_vertices()[1]);
        let mut explicit_null = serde_json::to_value(&motion).unwrap();
        explicit_null["keyframes"][0]["arm_pose"] = Value::Null;
        let explicit_null: motion::Request = serde_json::from_value(explicit_null).unwrap();
        assert_eq!(
            fitted
                .motion_path(&model, &digest, &explicit_null)
                .unwrap()
                .sha256(),
            legacy.sha256()
        );
        assert_eq!(fitted.report, original_report);
        assert_eq!(fitted.collision.vertices, original_collision);
    }

    #[test]
    fn motion_endpoints_reuse_exact_selected_shape_and_match_direct_source_poses() {
        let model = model();
        let digest = "a".repeat(64);
        let mut req = request();
        let fitted = FittedBody::prepare(&model, &digest, &req).unwrap();
        let original_report = fitted.report.clone();
        let motion: motion::Request = serde_json::from_value(json!({"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.01,"arm_abduction_deg":36.0,
                "leg_pose":{"posture":"seated","fraction":0.05}},
                {"time_s":0.02,"arm_abduction_deg":37.0,
                "leg_pose":{"posture":"squat","fraction":0.1}}],
            "max_step_s":0.01,"max_trials":16,"max_pairs":10000,"max_sample_vertices":10000}))
        .unwrap();
        let actual = fitted.motion_path(&model, &digest, &motion).unwrap();
        let mut endpoints = Vec::new();
        for keyframe in &motion.keyframes {
            req.arm_abduction_deg = keyframe.arm_abduction_deg;
            let legs = keyframe.leg_pose.unwrap();
            req.legs = match legs.posture {
                motion::LegPosture::Seated => Legs::Seated,
                motion::LegPosture::Squat => Legs::Squat,
            };
            req.leg_fraction = legs.fraction;
            let posed = FittedBody::prepare(&model, &digest, &req).unwrap();
            assert_eq!(posed.report["shape_sha256"], fitted.report["shape_sha256"]);
            assert_eq!(posed.report["pose"]["legs"], json!(req.legs));
            assert_eq!(posed.report["pose"]["leg_fraction"], legs.fraction);
            assert!(
                fitted
                    .collision
                    .source_vertex
                    .iter()
                    .enumerate()
                    .any(|(i, &source)| {
                        source < model.data().vertex_bone_indices.len()
                            && [9, 12].contains(&model.data().vertex_bone_indices[source])
                            && posed.collision.vertices[i] != fitted.collision.vertices[i]
                    }),
                "the fixture must move actual leg-skinned vertices"
            );
            endpoints.push(posed.collision.vertices);
        }
        let expected = motion::Path::new(&motion, fitted.body().collision, endpoints).unwrap();
        assert_eq!(actual.sha256(), expected.sha256());
        assert_eq!(
            actual.sha256(),
            fitted
                .motion_path(&model, &digest, &motion)
                .unwrap()
                .sha256()
        );
        assert_eq!(fitted.report, original_report);
        assert!(
            fitted
                .motion_path(&model, &"b".repeat(64), &motion)
                .is_err()
        );
        let mut changed = motion.clone();
        changed.keyframes[0].leg_pose.as_mut().unwrap().fraction = 0.06;
        assert_ne!(
            actual.sha256(),
            fitted
                .motion_path(&model, &digest, &changed)
                .unwrap()
                .sha256()
        );
    }

    #[test]
    fn translated_motion_endpoints_use_one_fixed_origin_and_keep_the_selected_body_unchanged() {
        let model = model();
        let digest = "a".repeat(64);
        let fitted = FittedBody::prepare(&model, &digest, &request()).unwrap();
        let original_report = fitted.report.clone();
        let original_collision = fitted.collision.vertices.clone();
        let original_display = fitted.display.vertices.clone();
        let motion: motion::Request = serde_json::from_value(json!({"schema":"ftx-drape-motion/1",
            "keyframes":[
                {"time_s":0.01,"arm_abduction_deg":35.0,"root_translation_m":[0.01,-0.02,0.03]},
                {"time_s":0.02,"arm_abduction_deg":35.0},
                {"time_s":0.03,"arm_abduction_deg":35.0,"root_translation_m":[0.0,0.0,0.0]}],
            "max_step_s":0.01,"max_trials":16,"max_pairs":10000,"max_sample_vertices":10000}))
        .unwrap();
        let actual = fitted.motion_path(&model, &digest, &motion).unwrap();
        let rest = model.rest(&fitted.params).unwrap();
        let mut posed = model
            .pose(
                &rest,
                &pose_deltas(&model, &rest, &Pose::drape(), 1.0).unwrap(),
            )
            .unwrap();
        let translation = Vec3::new(0.01, -0.02, 0.03);
        for vertex in &mut posed.vertices {
            *vertex += translation;
        }
        let translated = clothing_skin(&model, &posed.vertices).unwrap();
        for (&source, vertex) in translated.source_vertex.iter().zip(&translated.vertices) {
            if source < posed.vertices.len() {
                assert_eq!(*vertex, posed.vertices[source]);
            }
        }
        for (translated, original) in translated.vertices.iter().zip(&original_collision) {
            assert!((*translated - *original - translation).norm() < 1e-14);
        }
        let expected = motion::Path::new(
            &motion,
            fitted.body().collision,
            vec![
                translated.vertices,
                original_collision.clone(),
                original_collision.clone(),
            ],
        )
        .unwrap();
        assert_eq!(actual.sha256(), expected.sha256());
        let mut shifted = motion.clone();
        shifted.keyframes[1].root_translation_m = Some([0.01, -0.02, 0.03]);
        assert_ne!(
            actual.sha256(),
            fitted
                .motion_path(&model, &digest, &shifted)
                .unwrap()
                .sha256()
        );
        assert_eq!(fitted.collision.vertices, original_collision);
        assert_eq!(fitted.display.vertices, original_display);
        assert_eq!(fitted.report, original_report);
    }

    #[test]
    fn rotated_motion_endpoints_keep_absolute_source_origin_and_selected_shape() {
        let model = model();
        let digest = "a".repeat(64);
        let fitted = FittedBody::prepare(&model, &digest, &request()).unwrap();
        let original_report = fitted.report.clone();
        let original_collision = fitted.collision.vertices.clone();
        let original_display = fitted.display.vertices.clone();
        let rest = model.rest(&fitted.params).unwrap();
        let motion: motion::Request = serde_json::from_value(json!({"schema":"ftx-drape-motion/1",
            "keyframes":[
                {"time_s":0.01,"arm_abduction_deg":36.0,
                 "leg_pose":{"posture":"seated","fraction":0.05},
                 "root_rotation_rad":[0.0,0.0,0.125],"root_translation_m":[0.02,-0.04,0.01]},
                {"time_s":0.02,"arm_abduction_deg":37.0,
                 "root_translation_m":[-0.01,0.02,-0.03]},
                {"time_s":0.03,"arm_abduction_deg":35.0},
                {"time_s":0.04,"arm_abduction_deg":35.0,"root_rotation_rad":[-0.0,0.0,-0.0]}],
            "max_step_s":0.01,"max_trials":16,"max_pairs":10000,"max_sample_vertices":10000}))
        .unwrap();
        let mut expected = Vec::new();
        for (i, frame) in motion.keyframes.iter().enumerate() {
            let mut pose = Pose::drape();
            pose.arm_abduction_deg = Some(frame.arm_abduction_deg);
            if i == 0 {
                pose.legs = Legs::Seated;
                pose.leg_fraction = 0.05;
            }
            let mut posed = model
                .pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0).unwrap())
                .unwrap();
            let original = posed.vertices.clone();
            let angle = if i == 0 { 0.125 } else { 0.0 };
            let (s, c) = ftx_core::det::sin_cos(angle);
            if i == 0 {
                // Independent Z-axis matrix, rather than the candidate's vector-normalization
                // helper. The second frame has no rotation: it never inherits the first turn.
                let rotation = Mat3::from_rows([[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]);
                for vertex in &mut posed.vertices {
                    *vertex = rotation.mul_vec(*vertex);
                }
            }
            let translation = Vec3::from(frame.root_translation_m.unwrap_or([0.0; 3]));
            if i < 2 {
                for vertex in &mut posed.vertices {
                    *vertex += translation;
                }
            }
            for (before, after) in original.iter().zip(&posed.vertices) {
                let analytic = Vec3::new(
                    c * before.x - s * before.y,
                    s * before.x + c * before.y,
                    before.z,
                ) + translation;
                assert!((*after - analytic).norm() < 2e-14);
            }
            let skin = clothing_skin(&model, &posed.vertices).unwrap();
            assert_eq!(skin.triangles, fitted.collision.triangles);
            assert_eq!(skin.source_vertex, fitted.collision.source_vertex);
            expected.push(skin.vertices);
        }
        assert_eq!(expected[2], original_collision);
        assert_eq!(expected[3], original_collision);
        let independent = motion::Path::new(&motion, fitted.body().collision, expected).unwrap();
        let actual = fitted.motion_path(&model, &digest, &motion).unwrap();
        assert_eq!(actual.sha256(), independent.sha256());
        assert_eq!(fitted.report, original_report);
        assert_eq!(fitted.collision.vertices, original_collision);
        assert_eq!(fitted.display.vertices, original_display);
        let mut changed = motion.clone();
        changed.keyframes[0].root_rotation_rad.as_mut().unwrap()[2] += 0.01;
        assert_ne!(
            actual.sha256(),
            fitted
                .motion_path(&model, &digest, &changed)
                .unwrap()
                .sha256()
        );
        assert!(actual.validate_for(&changed, &fitted.body()).is_err());
    }

    #[test]
    fn seated_and_squat_paths_continue_real_cloth_and_restore_the_solver_after_a_late_refusal() {
        use crate::study::{Scenario, heat};
        use ftx_body::kinematics::Affine;

        let model = model();
        let digest = "a".repeat(64);
        let fitted = FittedBody::prepare(&model, &digest, &request()).unwrap();
        let original_body_report = fitted.report.clone();
        let body = fitted.body();
        let rest = model.rest(&fitted.params).unwrap();
        let mut pattern: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
        ))
        .unwrap();
        let width = std::f64::consts::TAU * 138.0;
        pattern["pieces"][0]["vertices"]["b"][0] = json!(width);
        pattern["pieces"][0]["vertices"]["c"][0] = json!(width);
        pattern["pieces"][0]["placement"]["radius_mm"] = json!(150.0);
        pattern["pieces"][0]["placement"]["x0"] = json!(width / 2.0);
        pattern["pieces"][0]["placement"]["y0"] =
            json!(body.frames.landmark_h("torso", "waist").unwrap() - 100.0);
        let pattern = ftx_pattern::Pattern::from_json(&pattern.to_string()).unwrap();
        let mut scenario: Scenario = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-scenario.json"
        ))
        .unwrap();
        scenario.options.spacing = 40.0;
        scenario.options.sew_steps = 12;
        scenario.options.drape_steps = 12;
        scenario.options.step = 0.0025;
        scenario.surface = None;
        scenario.coverage = None;
        let mut run = crate::run::drape(
            &pattern,
            &scenario.fit_spec,
            &body,
            &scenario.options,
            |_| {},
        )
        .unwrap();
        let original = run.scene.solver.clone();
        let count = body.collision.0.len() + original.x.len();
        for (posture, hip, knee, foot, translation, source_sampling) in [
            ("seated", -90.0_f64, 90.0_f64, 0.0_f64, None, false),
            ("squat", -100.0, 110.0, -10.0, None, false),
            (
                "seated",
                -90.0,
                90.0,
                0.0,
                Some([0.00001, -0.00002, 0.00003]),
                false,
            ),
            ("seated", -90.0, 90.0, 0.0, None, true),
        ] {
            let mut requested = json!({"schema":"ftx-drape-motion/1",
                "keyframes":[{"time_s":0.002,"arm_abduction_deg":35.0,
                    "leg_pose":{"posture":posture,"fraction":0.0001}},
                    {"time_s":0.004,"arm_abduction_deg":35.0,
                    "leg_pose":{"posture":posture,"fraction":0.0002}}],
                "max_step_s":0.001,"max_trials":16,"max_pairs":100000,
                "max_sample_vertices":10000});
            if let Some(translation) = translation {
                requested["keyframes"][0]["root_translation_m"] = json!(translation);
                requested["keyframes"][1]["root_translation_m"] = json!([0.0, 0.0, 0.0]);
            }
            if source_sampling {
                requested["source_pose_sampling"] = json!({"max_step_s":0.001,"max_segments":4});
                requested["keyframes"][0]["arm_pose"] =
                    json!({"left_abduction_deg":35.001,"right_abduction_deg":35.0});
            }
            let request: motion::Request = serde_json::from_value(requested).unwrap();
            let path = fitted.motion_path(&model, &digest, &request).unwrap();
            // Independently spell out the leg angles while retaining the requested arm deltas.
            let mut expected = Vec::new();
            for key in &request.keyframes {
                let mut arms = Pose::drape();
                arms.arm_pose = key.arm_pose;
                let mut deltas = pose_deltas(&model, &rest, &arms, 1.0).unwrap();
                for side in ["L", "R"] {
                    for (name, degrees) in
                        [("upperleg01", hip), ("lowerleg01", knee), ("foot", foot)]
                    {
                        let bone = model.bone_index(&format!("{name}.{side}")).unwrap();
                        let axis = rest.bone_frames[bone]
                            .m
                            .transpose()
                            .mul_vec(Vec3::new(1.0, 0.0, 0.0));
                        deltas[bone] = Affine::from_rotvec(
                            axis * (degrees.to_radians() * key.leg_pose.unwrap().fraction),
                            Vec3::ZERO,
                        );
                    }
                }
                let mut posed = model.pose(&rest, &deltas).unwrap();
                if let Some([x, y, z]) = key.root_translation_m
                    && [x, y, z] != [0.0; 3]
                {
                    for vertex in &mut posed.vertices {
                        *vertex += Vec3::new(x, y, z);
                    }
                }
                expected.push(clothing_skin(&model, &posed.vertices).unwrap().vertices);
            }
            if source_sampling {
                assert_eq!(path.source_vertices().len(), 4);
                assert_eq!(path.source_vertices()[1], expected[0]);
                assert_eq!(path.source_vertices()[3], expected[1]);
            } else {
                let independent =
                    motion::Path::new(&request, body.collision, expected.clone()).unwrap();
                assert_eq!(path.sha256(), independent.sha256());
            }
            assert_ne!(expected[0], body.collision.0);
            assert_ne!(expected[0], expected[1]);
            // Arm-only omission and an explicit zero leg fraction retain the original geometry.
            let mut standing = request.clone();
            for key in &mut standing.keyframes {
                key.arm_pose = None;
                key.leg_pose = None;
                key.root_translation_m = None;
            }
            let held = motion::Path::new(
                &standing,
                body.collision,
                vec![body.collision.0.to_vec(); standing.source_steps().unwrap().len()],
            )
            .unwrap();
            assert_eq!(
                fitted
                    .motion_path(&model, &digest, &standing)
                    .unwrap()
                    .sha256(),
                held.sha256()
            );

            run.scene.solver = original.clone();
            let continued = motion::evaluate(
                &request,
                &path,
                &scenario,
                &body,
                &mut run,
                &mut heat::Budget::default(),
                None,
                None,
                |_| {},
            )
            .unwrap();
            let samples = continued["samples"].as_array().unwrap();
            assert_eq!(samples.len(), 4);
            assert_eq!(samples[1]["body_positions_m"], json!(expected[0]));
            assert_eq!(samples[3]["body_positions_m"], json!(expected[1]));
            assert_eq!(
                continued["keyframes"][0]["leg_pose"],
                json!(request.keyframes[0].leg_pose)
            );
            assert_eq!(continued["continuous_contact_checked"], true);
            assert_eq!(
                continued["keyframes"][0]["arm_pose"],
                json!(request.keyframes[0].arm_pose)
            );
            assert!(continued["keyframes"][1].get("arm_pose").is_none());
            if source_sampling {
                assert_eq!(continued["source_pose_sampling"]["generated_segments"], 4);
                assert_eq!(
                    continued["source_pose_sampling"]["authored_interval_indices"],
                    json!([0, 0, 1, 1])
                );
                assert_eq!(continued["keyframes"].as_array().unwrap().len(), 2);
                assert_eq!(continued["keyframes"][0]["sample_index"], 1);
                assert_eq!(continued["keyframes"][1]["sample_index"], 3);
            } else {
                assert!(continued.get("source_pose_sampling").is_none());
            }
            assert_eq!(
                continued["keyframes"][0]["root_translation_m"],
                json!(translation)
            );
            assert_ne!(run.scene.solver.x, original.x);
            let moved = run.scene.solver.x.clone();
            if translation.is_some() {
                let mut fixed_root = request.clone();
                for key in &mut fixed_root.keyframes {
                    key.root_translation_m = None;
                }
                let fixed_path = fitted.motion_path(&model, &digest, &fixed_root).unwrap();
                run.scene.solver = original.clone();
                motion::evaluate(
                    &fixed_root,
                    &fixed_path,
                    &scenario,
                    &body,
                    &mut run,
                    &mut heat::Budget::default(),
                    None,
                    None,
                    |_| {},
                )
                .unwrap();
                assert_ne!(
                    run.scene.solver.x, moved,
                    "root translation must affect the contact-driven cloth response"
                );
            }
            run.scene.solver = original.clone();
            motion::evaluate(
                &standing,
                &held,
                &scenario,
                &body,
                &mut run,
                &mut heat::Budget::default(),
                None,
                None,
                |_| {},
            )
            .unwrap();
            assert_ne!(
                run.scene.solver.x, moved,
                "the leg collider must affect the sewn cloth"
            );

            // The first leg segment fits the budget and has already changed real cloth. A failure
            // before the second segment must restore every solver state and replay the same result.
            let mut limited = request.clone();
            limited.max_sample_vertices = 3 * count;
            let limited_path = fitted.motion_path(&model, &digest, &limited).unwrap();
            run.scene.solver = original.clone();
            let refusal = motion::evaluate(
                &limited,
                &limited_path,
                &scenario,
                &body,
                &mut run,
                &mut heat::Budget::default(),
                None,
                None,
                |_| {},
            )
            .unwrap_err();
            assert!(
                refusal
                    .message
                    .contains("whole-trajectory motion budget exhausted"),
                "{refusal:?}"
            );
            assert_eq!(run.scene.solver.x, original.x);
            assert_eq!(run.scene.solver.v, original.v);
            assert_eq!(
                run.scene.solver.contact.as_ref().unwrap().body.vertices,
                original.contact.as_ref().unwrap().body.vertices
            );
            assert_eq!(
                run.scene.solver.relaxation_report().unwrap(),
                original.relaxation_report().unwrap()
            );
            assert_eq!(
                run.scene.solver.bending_plasticity_report().unwrap(),
                original.bending_plasticity_report().unwrap()
            );
            assert_eq!(
                run.scene.solver.membrane_plasticity_report().unwrap(),
                original.membrane_plasticity_report().unwrap()
            );
            let replay = motion::evaluate(
                &request,
                &path,
                &scenario,
                &body,
                &mut run,
                &mut heat::Budget::default(),
                None,
                None,
                |_| {},
            )
            .unwrap();
            assert_eq!(replay, continued);
            assert_eq!(fitted.report, original_body_report);
        }
    }

    #[test]
    fn additional_tapes_report_disagreement_without_changing_the_accepted_shape() {
        let model = model();
        let hash = "a".repeat(64);
        let mut req = request();
        let a = FittedBody::prepare(&model, &hash, &req).unwrap();
        req.measurements = json!({"wearer":{"age_months":30,"sex":"F","stature_mm":1000},
            "fused":{"waist":{"value_mm":500}}});
        let b = FittedBody::prepare(&model, &hash, &req).unwrap();
        assert_eq!(a.report["shape_sha256"], b.report["shape_sha256"]);
        assert_eq!(a.collision.vertices, b.collision.vertices);
        assert_ne!(
            a.report["measurements_sha256"],
            b.report["measurements_sha256"]
        );
        let rows = b.report["residuals"].as_array().unwrap();
        assert!(rows[0]["residual_mm"].as_f64().unwrap() > 1000.0);
        assert!(rows.iter().all(|r| r["used_to_refit"] == false));
        assert_eq!(b.report["tape_evaluation"]["status"], "partial");
        // Missing head/limb sections do not erase the independently known regular-polygon waist.
        let waist = 32.0 * 140.0 * ftx_core::det::sin_cos(std::f64::consts::PI / 16.0).0;
        let row = rows.iter().find(|r| r["name"] == "waist").unwrap();
        assert!((row["body_mm"].as_f64().unwrap() - waist).abs() < 1e-9);
        assert!((row["residual_mm"].as_f64().unwrap() - (waist - 500.0)).abs() < 1e-9);
        let missing = b.report["tape_evaluation"]["unavailable_fields"]
            .as_array()
            .unwrap();
        assert!(!missing.is_empty());
        assert!(!missing.contains(&json!("waist")));
    }

    #[test]
    fn accepted_receipt_and_wearer_conflicts_never_fall_back_to_fitting() {
        let model = model();
        let hash = "a".repeat(64);
        for edit in 0..5 {
            let mut req = request();
            match edit {
                0 => req.accepted_body_fit.as_mut().unwrap()["status"] = json!("REJECTED"),
                1 => req.accepted_body_fit.as_mut().unwrap()["solution"]["shape"] = json!([0.7]),
                2 => req.measurements = json!({"wearer":{"age_months":31}}),
                3 => req.measurements = json!({"wearer":{"sex":"M"}}),
                _ => {
                    req.accepted_body_fit.as_mut().unwrap()["anny_configuration"]["model_sha256"] =
                        json!("c".repeat(64))
                }
            }
            let err = FittedBody::prepare(&model, &hash, &req).err().unwrap();
            assert!(
                !err.message.contains("local change"),
                "must not attempt the tape fitter: {err:?}"
            );
        }
    }

    #[test]
    fn invalid_pose_and_evidence_inputs_refuse_before_model_work() {
        for raw in [
            json!({"schema":"ftx-scenario-body/2"}),
            json!({"accepted_body_fit":{}}),
            json!({"schema":"ftx-scenario-body/1","accepted_body_fit":null}),
            json!({"schema":"ftx-scenario-body/1","arm_abduction_deg":181}),
            json!({"schema":"ftx-scenario-body/1","legs":"kneeling"}),
            json!({"schema":"ftx-scenario-body/1","leg_fraction":1.01}),
            json!({"schema":"ftx-scenario-body/1","leg_fraction":null}),
            json!({"legs":"seated"}),
            json!({"schema":"ftx-scenario-body/1","measurements":null}),
            json!({"wearer":{"age_months":-1}}),
            json!({"fused":{"waist":{"value_mm":"500"}}}),
        ] {
            assert!(BodyRequest::read(&raw).is_err(), "{raw}");
        }
        let legacy = json!({"wearer":{"age_months":30,"stature_mm":920}});
        let req = BodyRequest::read(&legacy).unwrap();
        assert_eq!(req.measurements, legacy);
        assert!(req.accepted_body_fit.is_none());
        assert_eq!(req.arm_abduction_deg, 35.0);
        assert_eq!(req.legs, Legs::Standing);
        assert_eq!(req.leg_fraction, 1.0);
        let replay = serde_json::to_value(&req).unwrap();
        assert!(replay.get("accepted_body_fit").is_none());
        assert!(replay.get("legs").is_none());
        assert!(replay.get("leg_fraction").is_none());
        assert_eq!(BodyRequest::read(&replay).unwrap().measurements, legacy);
    }

    #[test]
    fn accepted_body_runs_the_actual_drape_and_heat_consumers_with_fresh_pose_identity() {
        use crate::study::{Scenario, Session};
        use ftx_pattern::Pattern;
        let mut raw: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
        ))
        .unwrap();
        let width = std::f64::consts::TAU * 180.0;
        raw["pieces"][0]["vertices"]["b"][0] = json!(width);
        raw["pieces"][0]["vertices"]["c"][0] = json!(width);
        raw["pieces"][0]["placement"]["radius_mm"] = json!(180.0);
        raw["pieces"][0]["placement"]["x0"] = json!(width / 2.0);
        let pattern = Pattern::new(raw);
        let scenario: Scenario = serde_json::from_value(json!({"schema":"ftx-drape-scenario/1",
            "options":{"spacing":100.0,"sew_steps":3,"drape_steps":3,"step":0.001},
            "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":1000000,
            "surface":{"schema":"ftx-steady-surface-conditions/1","boundaries":[{
                "inside_c":30.0,"outside_c":10.0,"inside_resistance_m2_k_w":0.1,
                "outside_resistance_m2_k_w":0.2}]}}))
        .unwrap();
        let model = model();
        let hash = "a".repeat(64);
        let mut req = request();
        let a = FittedBody::prepare(&model, &hash, &req).unwrap();
        let body = a.body();
        let mut session = Session::new(&scenario, &body).unwrap();
        let first = session.evaluate(&pattern, |_| {}).unwrap();
        assert_eq!(first["surface"]["status"], "available");
        let ordinary = crate::run::drape(
            &pattern,
            &scenario.fit_spec,
            &body,
            &scenario.options,
            |_| {},
        )
        .unwrap();
        assert_eq!(first["drape"], ordinary.report);
        assert_eq!(first["positions_m"], json!(ordinary.scene.solver.x));
        // The prism's cap is not an anatomical shoulder: at 50 degrees its right and left
        // panels cross. Keep that previously accepted pose as a negative, not a weaker audit.
        req.arm_abduction_deg = 50.0;
        let invalid_body = FittedBody::prepare(&model, &hash, &req).unwrap();
        let invalid_body = invalid_body.body();
        let refusal = Session::new(&scenario, &invalid_body)
            .unwrap()
            .evaluate(&pattern, |_| {})
            .unwrap_err();
        assert_eq!(refusal.code, ftx_core::RefusalCode::IntersectingStart);
        assert!(refusal.message.contains("body triangles 6 and 63"));
        assert_eq!(first, session.evaluate(&pattern, |_| {}).unwrap());

        // At 36 degrees the cap panels remain on their own sides of the prism. Audit that
        // positive geometry independently of the ordinary drape and heat comparison below.
        req.arm_abduction_deg = 36.0;
        let b = FittedBody::prepare(&model, &hash, &req).unwrap();
        let body = b.body();
        crate::collider::audit_surface(body.collision.0, body.collision.1, 10_000).unwrap();
        let second = Session::new(&scenario, &body)
            .unwrap()
            .evaluate(&pattern, |_| {})
            .unwrap();
        assert_ne!(
            first["body_geometry_sha256"],
            second["body_geometry_sha256"]
        );
        assert_ne!(first["scenario_sha256"], second["scenario_sha256"]);
        assert_eq!(a.report["shape_sha256"], b.report["shape_sha256"]);
        assert_eq!(first, session.evaluate(&pattern, |_| {}).unwrap());
    }
}
