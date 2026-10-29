//! Fit one Anny body once for an entire material study, using the existing production pose pipeline.
use std::collections::BTreeMap;

use ftx_body::{
    AnnyModel, BodyFrames, BodyParams, Pose, SkinMesh, TAPE_FIELDS, Wearer,
    accepted_shape::AcceptedShape, fit_tape, pose_deltas, skin_mesh, tape_readings, tape_targets,
    trimmed_skin,
};
use ftx_core::Refusal;
use ftx_pattern::{instance::PyError, validate::BodyMeasure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{budget, hash, invalid, motion};
use crate::run::DrapeBody;

/// Body evidence and an independently authored standing pose. No material can alter this request.
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
    /// Standing straight-arm abduction from vertical, degrees in 0..=180. Not a motion trajectory.
    #[serde(default = "default_arm_angle")]
    pub arm_abduction_deg: f64,
}

fn empty_measurements() -> Value {
    json!({})
}
fn default_arm_angle() -> f64 {
    Pose::A_POSE_DEG
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
                if ["accepted_body_fit", "arm_abduction_deg"]
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
    params: BodyParams,
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
        let mut pose = Pose::drape();
        pose.arm_abduction_deg = Some(request.arm_abduction_deg);
        let rest = model.rest(&params)?;
        let posed = model.pose(&rest, &pose_deltas(model, &rest, &pose, 1.0)?)?;
        let collision = trimmed_skin(
            model,
            &posed.vertices,
            &["wrist.R", "wrist.L", "foot.R", "foot.L"],
        )?;
        let display = skin_mesh(&posed.vertices, &model.data().faces)?;
        let frames = BodyFrames::new(model, &posed)?;
        report["measurements_sha256"] = json!(hash(&request.measurements)?);
        report["shape_parameters"] = parameters(&params);
        report["shape_sha256"] = json!(hash(&(model_sha256, parameters(&params)))?);
        report["pose"] =
            json!({"kind":"standing_straight_arm", "arm_abduction_deg":request.arm_abduction_deg});
        report["body_provenance"] = json!("MODEL_FIT");
        report["physical_validation_claim"] = json!(false);
        let flat_source = json!({"shape_source":report["shape_source"],
            "input_measurements_sha256":report["measurements_sha256"]});
        Ok(Self {
            frames,
            collision,
            display,
            arm_deg: request.arm_abduction_deg,
            params,
            model_sha256: model_sha256.into(),
            flat_source,
            report,
        })
    }

    /// Lazily measure this unchanged selected shape in the standard A-pose at 35 degrees.
    /// The ordinary scenario pose and any image-frame poses do not change flat-fit targets.
    /// Stature remains available from model anthropometry when the complete tape-section
    /// evaluator cannot measure this shape; all other missing sections remain unavailable.
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
        let readings = tape_readings(&faces, stature_m, &posed, &frames)
            .filter(|values| values.iter().all(|value| value.is_finite() && *value > 0.0));
        let mut values = BTreeMap::from([("stature".into(), stature_mm)]);
        if let Some(readings) = readings {
            values.extend(
                TAPE_FIELDS
                    .into_iter()
                    .zip(readings)
                    .map(|(name, value)| (name.into(), value)),
            );
        }
        let identity = json!({"schema":"ftx-flat-fit-body/1","provenance":"MODEL_FIT",
            "model_sha256":model_sha256,
            "shape_sha256":hash(&(model_sha256, parameters(&self.params)))?,
            "shape_source":self.flat_source["shape_source"],
            "input_measurements_sha256":self.flat_source["input_measurements_sha256"],
            "measurement_pose":"a_pose_35",
            "measurement_geometry_sha256":hash(&(&posed.vertices, &faces))?,
            "measurement_values_sha256":hash(&values)?,
            "available_measurements":values.keys().collect::<Vec<_>>(),
            "complete_tape_sections":readings.is_some(),
            "uncertainty_propagated":false,"fit_claim":false});
        Ok(FlatFitBody {
            frames,
            values,
            identity,
        })
    }

    /// Generate motion endpoints from the SAME selected shape, without fitting again. The caller
    /// supplies the model digest from its decoded bytes just as for `prepare`. Endpoint generation
    /// uses Anny poses; the motion solver interpolates checked linear vertex chords between them.
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
        if self
            .collision
            .vertices
            .len()
            .saturating_mul(request.keyframes.len() + 1)
            > 2_000_000
        {
            return Err(budget("motion endpoint body-vertex budget exceeded"));
        }
        let rest = model.rest(&self.params)?;
        let mut endpoints = Vec::with_capacity(request.keyframes.len());
        for keyframe in &request.keyframes {
            let mut pose = Pose::drape();
            pose.arm_abduction_deg = Some(keyframe.arm_abduction_deg);
            let posed = model.pose(&rest, &pose_deltas(model, &rest, &pose, 1.0)?)?;
            let skin = trimmed_skin(
                model,
                &posed.vertices,
                &["wrist.R", "wrist.L", "foot.R", "foot.L"],
            )?;
            if skin.triangles != self.collision.triangles
                || skin.source_vertex != self.collision.source_vertex
            {
                return Err(invalid(
                    "motion endpoints must preserve the selected collision source mapping and topology",
                ));
            }
            endpoints.push(skin.vertices);
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

// Shape estimation always uses the existing reference pose, independent of the requested pose.
fn tape_shape(model: &AnnyModel, measurements: &Value) -> Result<(BodyParams, Value), Refusal> {
    if !measurements["wearer"]["age_months"].is_number() {
        return Err(invalid("scenario body fitting requires wearer.age_months"));
    }
    let pose = Pose::drape();
    let wearer = Wearer::from_measurements(measurements)?;
    let fit = fit_tape(model, &wearer, &tape_targets(measurements), &pose)?;
    let residuals: Vec<_> = fit
        .residuals
        .iter()
        .map(|r| {
            json!({"name":r.name,
            "target_mm":r.target_mm,"body_mm":r.body_mm,"residual_mm":r.body_mm-r.target_mm})
        })
        .collect();
    let report = json!({"measurement_pose":"standard_straight_arm_drape_35_degrees",
            "shape_source":{"kind":"tape_fit","refitted":true},"fit_iterations":fit.iterations,
            "body_mass_kg":fit.mass_kg,"residuals":residuals,"body_provenance":"MODEL_FIT",
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
    let readings = (|| {
        let posed = model.pose(&rest, &pose_deltas(model, &rest, &Pose::a_pose(35.0), 1.0)?)?;
        let frames = BodyFrames::new(model, &posed)?;
        let faces: Vec<_> = model
            .data()
            .faces
            .iter()
            .map(|f| f.map(|i| i as usize))
            .collect();
        tape_readings(&faces, dimensions.height, &posed, &frames)
            .filter(|a| a.iter().all(|v| v.is_finite() && *v > 0.0))
            .ok_or_else(|| invalid("accepted body tape sections are unavailable"))
    })();
    let comparisons: Vec<_> = tape_targets(measurements)
        .iter()
        .map(|target| {
            let body_mm = if target.name == "stature" {
                Some(dimensions.height * 1000.0)
            } else {
                readings.as_ref().ok().and_then(|values| {
                    TAPE_FIELDS
                        .iter()
                        .position(|&n| n == target.name)
                        .map(|i| values[i])
                })
            };
            json!({"name":target.name,"target_mm":target.value_mm,"body_mm":body_mm,
            "residual_mm":body_mm.map(|v| v-target.value_mm),"used_to_refit":false})
        })
        .collect();
    let report = json!({"shape_source":{"kind":"accepted_multi_frame_fit",
        "report_sha256":hash(accepted)?,"input_sha256":shape.input_sha256,
        "refitted":false,"image_poses_reused":false}, "source_fit":accepted,
        "fit_iterations":0,"measurement_pose":"a_pose_35", "body_mass_kg":dimensions.mass,
        "residuals":comparisons,"tape_evaluation":match &readings {
            Ok(_) => json!({"status":"available"}),
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
        assert_eq!(
            flat.identity()["available_measurements"],
            json!(["stature"])
        );
        assert!(flat.value("waist").is_err());
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
        missing["body"] = json!("waist");
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
    fn motion_endpoints_reuse_exact_selected_shape_and_match_direct_source_poses() {
        let model = model();
        let digest = "a".repeat(64);
        let mut req = request();
        let fitted = FittedBody::prepare(&model, &digest, &req).unwrap();
        let original_report = fitted.report.clone();
        let motion: motion::Request = serde_json::from_value(json!({"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.01,"arm_abduction_deg":36.0},
                {"time_s":0.02,"arm_abduction_deg":37.0}],
            "max_step_s":0.01,"max_trials":16,"max_pairs":10000,"max_sample_vertices":10000}))
        .unwrap();
        let actual = fitted.motion_path(&model, &digest, &motion).unwrap();
        let mut endpoints = Vec::new();
        for keyframe in &motion.keyframes {
            req.arm_abduction_deg = keyframe.arm_abduction_deg;
            let posed = FittedBody::prepare(&model, &digest, &req).unwrap();
            assert_eq!(posed.report["shape_sha256"], fitted.report["shape_sha256"]);
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
        changed.keyframes[0].arm_abduction_deg = 35.5;
        assert_ne!(
            actual.sha256(),
            fitted
                .motion_path(&model, &digest, &changed)
                .unwrap()
                .sha256()
        );
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
        assert_eq!(b.report["tape_evaluation"]["status"], "refused");
        assert!(rows[1]["body_mm"].is_null()); // the synthetic rig lacks complete tape sections
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
            json!({"schema":"ftx-scenario-body/1","legs":"seated"}),
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
        let replay = serde_json::to_value(&req).unwrap();
        assert!(replay.get("accepted_body_fit").is_none());
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
