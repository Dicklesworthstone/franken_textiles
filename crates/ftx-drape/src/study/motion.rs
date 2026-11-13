//! Material-following continuation of an already sewn drape, not independent pose restarts.
#[cfg(test)]
use crate::motion::advance_body;
use ftx_core::{Mat3, Refusal, Vec3, intersection::validate_mesh};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    Scenario, budget, coverage_at, hash, heat, invalid, omitted, outcome, vapor_coverage_request,
};
use crate::{
    motion::{DissipationCapture, MotionOptions, advance_body_coupled, advance_body_with_work},
    run::{DrapeBody, DrapeRun},
    start::{edge_strain, face_edges},
};

pub(super) const MAX_RETAINED_VERTICES: usize = 2_000_000;

fn default_sample_vertices() -> usize {
    500_000
}

/// Existing source-model leg posture. This supplies joint rotations, not anatomical qualification.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegPosture {
    /// Hip −90° and knee +90° about the rest body's +X axis.
    Seated,
    /// Hip −100°, knee +110° and foot −10°; the trunk remains upright.
    Squat,
}

/// Absolute leg pose at one endpoint, independent of its arm abduction and straightening.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegPose {
    /// Which existing body posture to approach from standing.
    pub posture: LegPosture,
    /// Fraction of the posture's joint rotations, finite and in 0..=1.
    pub fraction: f64,
}

/// A source-model endpoint. The checked solver follows linear vertex chords, optionally between
/// additional evaluations of the source joint/root pose rather than only these authored endpoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    /// Cumulative physical seconds since the end of the initial drape, strictly increasing.
    pub time_s: f64,
    /// Straight-arm angle from vertical, in degrees, 0..=180.
    pub arm_abduction_deg: f64,
    /// Optional absolute left/right angles overriding the common arm angle. Omission/null
    /// returns both arms to this endpoint's common angle, never the previous override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arm_pose: Option<ftx_body::drape::ArmPose>,
    /// Optional seated/squat leg rotation. Omission keeps standing legs at this endpoint;
    /// this is not an increment from the previous endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leg_pose: Option<LegPose>,
    /// Absolute world-space displacement [x, y, z] in metres from the selected body's fixed
    /// origin, applied after source posing. Omission means zero, never the previous offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_translation_m: Option<[f64; 3]>,
    /// Absolute root rotation vector (axis times angle in radians), with norm at most pi.
    /// Rotate the posed source surface about its root-relative origin before world translation.
    /// Omission/null means identity at this endpoint, never the previous rotation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_rotation_rad: Option<[f64; 3]>,
}

impl Keyframe {
    // Keep admission and source posing on one deterministic law. Scaling avoids squaring tiny
    // rotation vectors to zero or overflowing their norm; exact zero bypasses the old geometry.
    pub(super) fn root_rotation_matrix(&self) -> Result<Option<Mat3>, Refusal> {
        let Some(rotation) = self.root_rotation_rad else {
            return Ok(None);
        };
        if rotation.iter().any(|value| !value.is_finite()) {
            return Err(invalid(
                "motion root rotation needs a finite rotation vector in radians",
            ));
        }
        let vector = Vec3::from(rotation);
        let scale = vector.x.abs().max(vector.y.abs()).max(vector.z.abs());
        if scale == 0.0 {
            return Ok(None);
        }
        let scaled = vector / scale;
        let length = scaled.norm();
        let angle = scale * length;
        if angle > std::f64::consts::PI {
            return Err(invalid(
                "motion root rotation vector norm must not exceed pi radians",
            ));
        }
        Ok(Some(Mat3::rotation(scaled / length, angle)))
    }
}

/// Explicit resolution of the source pose path, independent of mechanical solver subdivisions.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePoseSampling {
    /// Maximum source-pose interval in seconds, finite and in 1e-9..=1.
    pub max_step_s: f64,
    /// Maximum generated segments across the entire authored trajectory, 1..=512.
    pub max_segments: usize,
}

/// One generated endpoint, retaining its original thermal boundary-condition interval.
#[derive(Clone, Debug, Serialize)]
pub(super) struct SourceStep {
    pub interval: usize,
    pub time_s: f64,
    pub fraction: f64,
}

/// Bounded prescribed motion following the scenario's fresh initial drape.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-drape-motion/1`.
    pub schema: String,
    /// One to sixteen endpoints of the SAME fixed body shape, in time order.
    pub keyframes: Vec<Keyframe>,
    /// Maximum time per accepted implicit substep (s), not a rendering frame rate.
    pub max_step_s: f64,
    /// Total attempted substeps across the entire trajectory, including subdivisions, 1..=512.
    pub max_trials: usize,
    /// Maximum candidate pairs in each continuous-collision query, 1..=1000000.
    pub max_pairs: usize,
    /// Retained body/cloth vertices plus optional plastic-dissipation scalars, including the
    /// initial state, 1..=2000000. A retained scalar conservatively costs one vertex equivalent.
    /// Defaults to 500000; endpoint receipts also consume the session's geometry allowance.
    #[serde(default = "default_sample_vertices")]
    pub max_sample_vertices: usize,
    /// Optional evaluation of intermediate poses on the same source body. Joint angles follow
    /// their fixed source axes, root rotation follows the shortest quaternion arc, and root
    /// translation is linear. The solver still checks short vertex chords between these poses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_pose_sampling: Option<SourcePoseSampling>,
    /// Optional independent-sheet transient temperatures on every accepted mechanical sample.
    /// Its explicit initial temperature applies at motion time zero, not during initial draping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transient_surface: Option<heat::SurfaceRequest>,
    /// Optional transient layer/body-domain heat, with material enthalpy remapped on every
    /// accepted geometric sample. Independent of sheet heat and steady endpoint snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transient_coverage: Option<heat::coverage::Request>,
    /// Optional potential integral over every accepted mechanical sample, using the scenario's
    /// fixed `vapor_coverage` source/body/boundary policy. No moisture or latent heat is evolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vapor_coverage_history: Option<crate::vapor::coverage::history::Policy>,
}

impl Request {
    /// Validate finite, ordered time/pose inputs and whole-trajectory budgets before model work.
    ///
    /// # Errors
    /// Invalid schema, empty/excessive keyframes, nonfinite or unordered times, angles or budgets.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-drape-motion/1"
            || self.keyframes.is_empty()
            || self.keyframes.len() > 16
            || !self.max_step_s.is_finite()
            || !(1e-9..=1.0).contains(&self.max_step_s)
            || !(1..=512).contains(&self.max_trials)
            || !(1..=1_000_000).contains(&self.max_pairs)
            || !(1..=2_000_000).contains(&self.max_sample_vertices)
        {
            return Err(invalid(
                "invalid motion schema, keyframe count or work/sample limits",
            ));
        }
        let mut previous = 0.0;
        let mut minimum_trials = 0.0;
        for frame in &self.keyframes {
            if !frame.time_s.is_finite()
                || frame.time_s > 60.0
                || frame.time_s - previous < 1e-9
                || !frame.arm_abduction_deg.is_finite()
                || !(0.0..=180.0).contains(&frame.arm_abduction_deg)
            {
                return Err(invalid(
                    "motion needs increasing times through 60 s and finite standing angles in 0..=180 degrees",
                ));
            }
            if let Some(arms) = frame.arm_pose {
                arms.validate()?;
            }
            if frame.leg_pose.is_some_and(|pose| {
                !pose.fraction.is_finite() || !(0.0..=1.0).contains(&pose.fraction)
            }) {
                return Err(invalid(
                    "motion leg-pose fractions must be finite and in 0..=1",
                ));
            }
            if frame
                .root_translation_m
                .is_some_and(|translation| translation.iter().any(|value| !value.is_finite()))
            {
                return Err(invalid(
                    "motion root translation needs finite world-space coordinates in metres",
                ));
            }
            let _ = frame.root_rotation_matrix()?;
            minimum_trials += ((frame.time_s - previous) / self.max_step_s - 1e-12).ceil();
            previous = frame.time_s;
        }
        if self.source_pose_sampling.is_some() {
            previous = 0.0;
            minimum_trials = 0.0;
            for step in self.source_steps()? {
                minimum_trials += ((step.time_s - previous) / self.max_step_s - 1e-12).ceil();
                previous = step.time_s;
            }
        }
        if minimum_trials > self.max_trials as f64 {
            return Err(budget(
                "motion requires more nominal substeps than max_trials",
            ));
        }
        Ok(())
    }

    // Called after the authored times have been admitted by validate(). No geometry is allocated
    // until the global segment budget and representable duration of every sample are checked.
    pub(super) fn source_steps(&self) -> Result<Vec<SourceStep>, Refusal> {
        let Some(policy) = self.source_pose_sampling else {
            return Ok(self
                .keyframes
                .iter()
                .enumerate()
                .map(|(interval, frame)| SourceStep {
                    interval,
                    time_s: frame.time_s,
                    fraction: 1.0,
                })
                .collect());
        };
        if !policy.max_step_s.is_finite()
            || !(1e-9..=1.0).contains(&policy.max_step_s)
            || !(1..=512).contains(&policy.max_segments)
        {
            return Err(invalid(
                "source-pose sampling needs a finite step in 1e-9..=1 s and max_segments in 1..=512",
            ));
        }
        let mut steps = Vec::new();
        let mut start = 0.0;
        let mut previous = 0.0;
        for (interval, frame) in self.keyframes.iter().enumerate() {
            let count = ((frame.time_s - start) / policy.max_step_s - 1e-12).ceil();
            if !count.is_finite()
                || count < 1.0
                || count > (policy.max_segments - steps.len()) as f64
            {
                return Err(budget(
                    "source-pose sampling exceeds its whole-trajectory max_segments",
                ));
            }
            let count = count as usize;
            for k in 1..=count {
                let fraction = k as f64 / count as f64;
                let time_s = if k == count {
                    frame.time_s
                } else {
                    start + (frame.time_s - start) * fraction
                };
                if !time_s.is_finite() || time_s - previous < 1e-9 {
                    return Err(invalid(
                        "source-pose sampling produces an unrepresentable or sub-1e-9 s mechanical interval",
                    ));
                }
                steps.push(SourceStep {
                    interval,
                    time_s,
                    fraction,
                });
                previous = time_s;
            }
            start = frame.time_s;
        }
        Ok(steps)
    }
}

/// Prepared endpoint meshes with unchanged topology, tied to the exact initial collision mesh and
/// policy. The model adapter generates these once from the selected shape, before any material run.
pub struct Path {
    initial_sha256: String,
    request_sha256: String,
    vertices: Vec<Vec<Vec3>>,
    steps: Vec<SourceStep>,
    sha256: String,
}

impl Path {
    /// Bind endpoint geometry to an initial collision mesh. This is also the explicit seam for a
    /// caller with its own body model; source endpoint generation remains that caller's responsibility.
    /// With source-pose sampling enabled, supply every equally spaced generated source endpoint,
    /// in authored-interval order, rather than only the authored endpoints.
    ///
    /// # Errors
    /// Invalid policy, count, mesh or an excessive total of retained endpoint vertices.
    pub fn new(
        request: &Request,
        initial: (&[Vec3], &[[usize; 3]]),
        vertices: Vec<Vec<Vec3>>,
    ) -> Result<Self, Refusal> {
        request.validate()?;
        let steps = request.source_steps()?;
        if vertices.len() != steps.len()
            || initial.0.is_empty()
            || initial.0.len() > 196_608
            || initial.1.is_empty()
            || initial.1.len() > 65_536
            || initial.0.len().saturating_mul(vertices.len() + 1) > 2_000_000
        {
            return Err(budget(
                "motion path exceeds endpoint geometry limits or has the wrong count",
            ));
        }
        validate_mesh(initial.0, initial.1)?;
        for frame in &vertices {
            if frame.len() != initial.0.len() {
                return Err(invalid(
                    "motion endpoints must preserve collision-body vertex order/count",
                ));
            }
            validate_mesh(frame, initial.1)?;
        }
        let initial_sha256 = hash(&initial)?;
        let request_sha256 = hash(request)?;
        let sha256 = if request.source_pose_sampling.is_some() {
            hash(&(&initial_sha256, &request_sha256, &vertices, &steps))?
        } else {
            hash(&(&initial_sha256, &request_sha256, &vertices))?
        };
        Ok(Self {
            initial_sha256,
            request_sha256,
            vertices,
            steps,
            sha256,
        })
    }

    pub(super) fn validate_for(
        &self,
        request: &Request,
        body: &DrapeBody<'_>,
    ) -> Result<(), Refusal> {
        if self.initial_sha256 != hash(&body.collision)? || self.request_sha256 != hash(request)? {
            return Err(invalid(
                "motion path does not match this initial body and requested trajectory",
            ));
        }
        Ok(())
    }

    /// Identity includes the exact initial collision mesh, time/pose policy and all endpoint vertices.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    #[cfg(test)]
    pub(super) fn source_vertices(&self) -> &[Vec<Vec3>] {
        &self.vertices
    }
}

// Extend the kernel's per-segment transaction across the entire authored path, including velocity,
// contact caches and material memories. Reserved thermal work stays charged after a refusal.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate(
    request: &Request,
    path: &Path,
    scenario: &Scenario,
    body: &DrapeBody<'_>,
    run: &mut DrapeRun,
    heat_budget: &mut heat::Budget,
    material_strain: Option<Result<super::strain::History, Refusal>>,
    contact_loads: Option<Result<super::contact_loads::History, Refusal>>,
    progress: impl FnMut(&str),
) -> Result<Value, Refusal> {
    let initial = run.scene.solver.clone();
    let result = evaluate_path(
        request,
        path,
        scenario,
        body,
        run,
        heat_budget,
        material_strain,
        contact_loads,
        progress,
    );
    if result.is_err() {
        run.scene.solver = initial;
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn evaluate_path(
    request: &Request,
    path: &Path,
    scenario: &Scenario,
    body: &DrapeBody<'_>,
    run: &mut DrapeRun,
    heat_budget: &mut heat::Budget,
    mut material_strain: Option<Result<super::strain::History, Refusal>>,
    mut contact_loads: Option<Result<super::contact_loads::History, Refusal>>,
    mut progress: impl FnMut(&str),
) -> Result<Value, Refusal> {
    path.validate_for(request, body)?;
    if run
        .scene
        .solver
        .contact
        .as_ref()
        .is_some_and(|c| c.params.thickness < run.scene.target_thickness)
    {
        return Err(invalid(
            "finish the initial drape's physical thickness ramp before motion",
        ));
    }
    let geometry_count = body
        .collision
        .0
        .len()
        .checked_add(run.scene.solver.x.len())
        .ok_or_else(|| budget("motion sample count overflow"))?;
    let mut remaining_trials = request.max_trials;
    let initial_positions_sha256 = hash(&run.scene.solver.x)?;
    let mut initial_sample = json!({"time_s":0.0,"positions_m":run.scene.solver.x,
        "body_positions_m":body.collision.0});
    let initial_membrane_history = run.scene.solver.relaxation_report()?;
    let initial_bending_history = run.scene.solver.bending_plasticity_report()?;
    let initial_permanent_history = run.scene.solver.membrane_plasticity_report()?;
    // Freeze source/topology at the same sewn state as the mechanical path. Moving pressure fields
    // follow these verified body triangle identities; an old body hash is never silently cleared.
    let vapor_initial = scenario
        .vapor_coverage
        .as_ref()
        .map(|policy| vapor_coverage_request(policy, run, body.collision));
    let mut vapor_history = request.vapor_coverage_history.as_ref().map(|policy| {
        policy.validate()?;
        // This reservation survives an initial or later source/projection failure.
        heat_budget.reserve_vapor_geometry(policy.max_geometry_operations)?;
        let initial = vapor_initial
            .as_ref()
            .ok_or_else(|| invalid("moving vapor history requires scenario.vapor_coverage"))?
            .clone()?;
        crate::vapor::coverage::history::History::new(initial, policy.clone())
    });
    // One-way thermal failures remain independent. Opted-in liquid mass feedback makes the
    // surface history mandatory: no later mechanical state exists without its accepted mass.
    let coupled = request
        .transient_surface
        .as_ref()
        .is_some_and(heat::SurfaceRequest::has_mechanical_coupling);
    let mut surface_heat = request
        .transient_surface
        .as_ref()
        .map(|policy| policy.start(run, &request.keyframes, heat_budget));
    if coupled {
        let history = surface_heat
            .as_mut()
            .expect("coupling requires a surface request")
            .as_mut()
            .map_err(|e| e.clone())?;
        initial_sample["liquid_mechanics"] = history.bind_mechanics(run)?;
    }
    let mut coverage_heat = request
        .transient_coverage
        .as_ref()
        .map(|policy| policy.start(run, body.collision, request.keyframes.len(), heat_budget));
    let mut capture = surface_heat
        .as_ref()
        .and_then(|h| h.as_ref().ok())
        .map_or(DissipationCapture::default(), |h| h.dissipation_capture());
    let coverage_capture = coverage_heat
        .as_ref()
        .and_then(|h| h.as_ref().ok())
        .map_or(DissipationCapture::default(), |h| h.dissipation_capture());
    capture.plastic |= coverage_capture.plastic;
    capture.relaxation |= coverage_capture.relaxation;
    capture.bending |= coverage_capture.bending;
    let channels = usize::from(capture.plastic)
        + usize::from(capture.relaxation)
        + usize::from(capture.bending);
    let history_scalars = run
        .scene
        .solver
        .model
        .membranes
        .len()
        .checked_mul(channels)
        .ok_or_else(|| budget("motion dissipation count overflow"))?;
    let count = geometry_count
        .checked_add(history_scalars)
        .ok_or_else(|| budget("motion history count overflow"))?;
    let mut remaining_vertices = request
        .max_sample_vertices
        .checked_sub(count)
        .filter(|n| *n >= count)
        .ok_or_else(|| {
            budget("motion sample budget cannot retain initial and final body/cloth/history")
        })?;
    if capture.plastic {
        initial_sample["membrane_plastic_dissipation_j"] =
            json!(run.scene.solver.membrane_plastic_dissipation_j()?);
    }
    if capture.relaxation {
        initial_sample["membrane_relaxation_dissipation_j"] =
            json!(run.scene.solver.membrane_relaxation_dissipation_j()?);
    }
    if capture.bending {
        initial_sample["bending_plastic_dissipation_j"] =
            json!(run.scene.solver.bending_plastic_dissipation_j()?);
    }
    let faces: Vec<_> = run
        .scene
        .solver
        .model
        .membranes
        .iter()
        .map(|t| t.v)
        .collect();
    // The welded model has multiple material rest charts at a seam. Measure each original flat
    // panel edge against its own millimetre rest chart, not an arbitrary welded vertex's chart.
    let edges = face_edges(&run.scene.mesh.faces);
    let rest = &run.scene.mesh.verts2d;
    let weld = &run.scene.weld;
    let strain = |x: &[Vec3]| -> Result<f64, Refusal> {
        let expanded: Vec<_> = weld.iter().map(|&w| x[w]).collect();
        let values = edge_strain(&expanded, rest, &edges);
        if values.is_empty() || values.iter().any(|v| !v.is_finite()) {
            return Err(invalid(
                "motion edge strain unavailable on the retained solver mesh",
            ));
        }
        Ok(values.into_iter().fold(0.0_f64, |a, b| a.max(b.abs())) * 100.0)
    };
    let mut peak_strain = strain(&run.scene.solver.x)?;
    let mut samples = Vec::new();
    let mut endpoints = Vec::new();
    let mut pairs_tested = 0u64;
    let mut previous_time = 0.0;
    let mut accepted_time = 0.0;
    for (segment, step) in path.steps.iter().enumerate() {
        let i = step.interval;
        let frame = &request.keyframes[i];
        let time_s = step.time_s;
        let target = &path.vertices[segment];
        if remaining_trials == 0 || remaining_vertices < count {
            return Err(budget(
                "whole-trajectory motion budget exhausted; no partial motion published",
            ));
        }
        progress(&format!(
            "motion endpoint {}/{}",
            i + 1,
            request.keyframes.len()
        ));
        let options = MotionOptions {
            max_step_s: request.max_step_s,
            max_trials: remaining_trials,
            max_pairs: request.max_pairs,
            max_sample_vertices: remaining_vertices,
        };
        let report = if coupled {
            let history = surface_heat
                .as_mut()
                .expect("requested coupled history")
                .as_mut()
                .map_err(|e| e.clone())?;
            let mut index = samples.len();
            advance_body_coupled(
                &mut run.scene.solver,
                target,
                time_s - previous_time,
                options,
                capture,
                history,
                |solver, sample, history| {
                    let time = if sample.time_s == time_s - previous_time {
                        time_s
                    } else {
                        previous_time + sample.time_s
                    };
                    history.advance_coupled(solver, sample, time, i, index)?;
                    index += 1;
                    Ok(())
                },
            )?
        } else {
            advance_body_with_work(
                &mut run.scene.solver,
                target,
                time_s - previous_time,
                options,
                capture,
            )?
        };
        remaining_trials -= report.trials;
        remaining_vertices -= report.samples.len() * count;
        pairs_tested += report.pairs_tested;
        for sample in report.samples {
            peak_strain = peak_strain.max(strain(&sample.cloth)?);
            let time = if sample.time_s == time_s - previous_time {
                time_s
            } else {
                previous_time + sample.time_s
            };
            if !time.is_finite() || time <= accepted_time {
                return Err(invalid(
                    "motion timestamps lose physical ordering; no partial motion published",
                ));
            }
            accepted_time = time;
            if let Some(Ok(history)) = &mut material_strain
                && let Err(refusal) = history.advance(&sample.cloth, time)
            {
                material_strain = Some(Err(refusal));
            }
            if let Some(Ok(history)) = &mut contact_loads
                && let Err(refusal) = history.advance(&sample.cloth, &sample.body, time)
            {
                contact_loads = Some(Err(refusal));
            }
            if let Some(Ok(history)) = &mut vapor_history
                && let Err(refusal) = history.advance(crate::vapor::coverage::history::Sample {
                    time_s: time,
                    positions_m: sample.cloth.clone(),
                    body_positions_m: sample.body.clone(),
                })
            {
                vapor_history = Some(Err(refusal));
            }
            if !coupled
                && let Some(Ok(history)) = &mut surface_heat
                && let Err(refusal) = history.advance(
                    &sample.cloth,
                    time,
                    i,
                    samples.len(),
                    sample.membrane_plastic_dissipation_j.as_deref(),
                    sample.membrane_relaxation_dissipation_j.as_deref(),
                    sample.bending_plastic_dissipation_j.as_deref(),
                )
            {
                surface_heat = Some(Err(refusal));
            }
            if let Some(Ok(history)) = &mut coverage_heat
                && let Err(refusal) = history.advance(
                    &sample.cloth,
                    &sample.body,
                    time,
                    i,
                    samples.len(),
                    sample.membrane_plastic_dissipation_j.as_deref(),
                    sample.membrane_relaxation_dissipation_j.as_deref(),
                    sample.bending_plastic_dissipation_j.as_deref(),
                )
            {
                coverage_heat = Some(Err(refusal));
            }
            let mut retained = json!({"time_s":time,
                "positions_m":sample.cloth,"body_positions_m":sample.body,
                "solve":{"converged":sample.solve.converged,"iterations":sample.solve.iterations,
                    "residual_n":sample.solve.residual,"last_step_m":sample.solve.last_step,
                    "energy_start_j":sample.solve.energy_start,"energy_end_j":sample.solve.energy_end,
                    "factorizations":sample.solve.factorizations,"ccd_limited":sample.solve.ccd_limited}});
            if let Some(dissipation) = sample.membrane_plastic_dissipation_j {
                retained["membrane_plastic_dissipation_j"] = json!(dissipation);
            }
            if let Some(dissipation) = sample.membrane_relaxation_dissipation_j {
                retained["membrane_relaxation_dissipation_j"] = json!(dissipation);
            }
            if let Some(dissipation) = sample.bending_plastic_dissipation_j {
                retained["bending_plastic_dissipation_j"] = json!(dissipation);
            }
            samples.push(retained);
        }
        previous_time = time_s;
        if time_s != frame.time_s {
            continue;
        }
        // Rebuild source-locked heat input from the NEW solver geometry; never patch an old receipt.
        let mut snapshot = json!({"material":run.report["material"],
            "steady_thermal_surface":crate::thermal::steady::surface::for_drape(&run.scene, &run.report["material"])});
        if scenario.vapor_surface.is_some() {
            snapshot["vapor_surface"] =
                crate::thermal::vapor::for_drape(&run.scene, &run.report["material"]);
        }
        let surface = scenario
            .surface
            .as_ref()
            .map_or_else(omitted, |c| outcome(c.evaluate(&snapshot)));
        let coverage = scenario.coverage.as_ref().map_or_else(omitted, |c| {
            outcome(coverage_at(c, &snapshot, (target, body.collision.1)))
        });
        let mut endpoint = json!({"time_s":frame.time_s,"arm_abduction_deg":frame.arm_abduction_deg,
            "sample_index":samples.len()-1,"positions_sha256":hash(&run.scene.solver.x)?,
            "body_collision_sha256":hash(&(target,body.collision.1))?,
            "surface":surface,"coverage":coverage});
        if let Some(arms) = frame.arm_pose {
            endpoint["arm_pose"] = json!(arms);
        }
        if let Some(pose) = frame.leg_pose {
            endpoint["leg_pose"] = json!(pose);
        }
        if let Some(translation) = frame.root_translation_m {
            endpoint["root_translation_m"] = json!(translation);
        }
        if let Some(rotation) = frame.root_rotation_rad {
            endpoint["root_rotation_rad"] = json!(rotation);
        }
        if let Some(conditions) = &scenario.vapor_surface {
            endpoint["vapor_surface"] = outcome(conditions.evaluate(&snapshot));
        }
        if let Some(initial) = &vapor_initial {
            endpoint["vapor_coverage"] = outcome(
                initial
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|r| r.with_positions(&run.scene.solver.x, target))
                    .and_then(|r| r.evaluate()),
            );
        }
        endpoints.push(endpoint);
    }
    let mut result = json!({"schema":"ftx-drape-motion-result/1","path_sha256":path.sha256,
        "initial_positions_sha256":initial_positions_sha256,"initial_body_collision_sha256":path.initial_sha256,
        "initial_sample":initial_sample,
        "initial_membrane_history":initial_membrane_history,"initial_bending_history":initial_bending_history,
        "final_positions_sha256":hash(&run.scene.solver.x)?,
        "final_body_collision_sha256":hash(&(&run.scene.solver.contact.as_ref()
            .expect("motion retains collider").body.vertices,body.collision.1))?,
        "initial_drape_settled":run.report["settled"],"initial_drape_gating":run.report["gating"],
        "duration_s":previous_time,"trials":request.max_trials-remaining_trials,"pairs_tested":pairs_tested,
        "retained_vertex_samples":(samples.len()+1)*geometry_count,
        "samples":samples,"keyframes":endpoints,"triangles":faces,"body_triangles":body.collision.1,
        "final_velocities_m_s":run.scene.solver.v,
        "metrics":{"max_absolute_edge_strain_pct":peak_strain},
        "membrane_history":run.scene.solver.relaxation_report()?,
        "bending_history":run.scene.solver.bending_plasticity_report()?,
        "continuous_contact_checked":true,"physical_validation_claim":false,"comfort_claim":false,
        "fit_claim":false,"settledness_claim":false,
        "thermal_basis":"Independent steady endpoint snapshots, not transient temperatures or heat during motion.",
        "scope":"Prescribed piecewise-linear body-vertex motion from the computed initial drape. All accepted substeps are retained; no independent restarts or empirical ride-up/fit claim. Body self-intersection is not certified."});
    if let Some(policy) = request.source_pose_sampling {
        result["source_pose_sampling"] = json!({
            "max_step_s":policy.max_step_s,"max_segments":policy.max_segments,
            "generated_segments":path.steps.len(),
            "source_times_s":path.steps.iter().map(|step| step.time_s).collect::<Vec<_>>(),
            "authored_interval_indices":path.steps.iter().map(|step| step.interval).collect::<Vec<_>>()
        });
        result["scope"] = json!(
            "The same fixed source shape is evaluated at bounded, equally timed source poses: supported joint angles interpolate on their fixed source axes, root rotation follows the shortest quaternion arc, and translation is linear. Continuous contact is checked on the short vertex chords joining these poses, not on exact curved sweeps. All accepted substeps are retained; authored endpoints and thermal intervals remain unchanged. No anatomical, body self-intersection, empirical ride-up or fit qualification is inferred."
        );
    }
    if let Some(history) = material_strain {
        result["material_strain"] = outcome(history.map(|history| history.report(true)));
    }
    if let Some(history) = contact_loads {
        result["contact_loads"] = outcome(history.map(|history| history.report(true)));
    }
    if let Some(history) = initial_permanent_history {
        result["initial_membrane_plasticity_history"] = history;
    }
    if channels != 0 {
        result["retained_dissipation_scalars"] = json!(
            (result["samples"]
                .as_array()
                .expect("retained samples")
                .len()
                + 1)
                * history_scalars
        );
    }
    if let Some(history) = run.scene.solver.membrane_plasticity_report()? {
        result["membrane_plasticity_history"] = history;
    }
    if coupled {
        result["liquid_mechanics"] = json!(run.scene.solver.liquid_mass_report()?);
    }
    if let Some(history) = surface_heat {
        let finished =
            history.and_then(|h| h.finish(&path.sha256, &initial_positions_sha256, previous_time));
        result["transient_surface"] = if coupled {
            outcome(Ok(finished?))
        } else {
            outcome(finished)
        };
        result["thermal_basis"] = json!(
            "Steady endpoint snapshots and a separate requested independent-sheet transient history. These heat bases are not interchangeable."
        );
    }
    if let Some(history) = coverage_heat {
        result["transient_coverage"] = outcome(
            history.and_then(|h| h.finish(&path.sha256, &initial_positions_sha256, previous_time)),
        );
        result["thermal_basis"] = json!(
            "Steady endpoint snapshots and separately requested transient sheet/coverage histories. Their domains and energy totals are not interchangeable."
        );
    }
    if coupled {
        result["thermal_basis"] = json!(if result["liquid_mechanics"]["schema"]
            == "ftx-coupled-liquid-mechanics-result/6"
        {
            "Accepted-step heat and source-qualified free-liquid exchange update the next mechanical interval's inertia, gravity and velocity. Independently scheduled supply and condensation mix before evaporation and drainage leave with qualified velocities relative to the post-mixing cloth; retained cloth recoils to conserve momentum. Incoming mixing loss and outgoing separation work remain explicit mechanical boundary terms, separate from thermal sensible/latent enthalpy. No jet-pressure, runoff or wet constitutive law is inferred. Steady snapshots and optional coverage heat remain separate domains."
        } else if result["liquid_mechanics"]["drained_liquid_mass_kg"].is_number() {
            "Accepted-step heat and qualified free-liquid exchange update the next mechanical interval's inertia, gravity and velocity. Independently qualified incoming streams mix before evaporation and drainage leave at the common endpoint velocity. The outgoing phases retain distinct mass and sensible/latent labels; their combined bulk momentum and energy remain separate from thermal enthalpy. No mixing heat or runoff law is inferred. Steady snapshots and optional coverage heat remain separate domains."
        } else if matches!(
            result["liquid_mechanics"]["schema"].as_str(),
            Some(
                "ftx-coupled-liquid-mechanics-result/2"
                    | "ftx-coupled-liquid-mechanics-result/3"
                    | "ftx-coupled-liquid-mechanics-result/4"
                    | "ftx-coupled-liquid-mechanics-result/5"
            )
        ) {
            "Accepted-step sheet heat and source-qualified free-liquid exchange update the next mechanical interval's inertia, gravity and velocity. Prescribed incoming streams mix inelastically before outgoing evaporation leaves at the mixed velocity. Bulk momentum, kinetic/gravitational energy and nonnegative mixing dissipation remain separate from thermal sensible/latent enthalpy; no mixing heat is inferred. Steady snapshots and optional coverage heat remain separate domains."
        } else if result["liquid_mechanics"]["schema"] == "ftx-coupled-liquid-mechanics-result/1" {
            "Accepted-step sheet heat and source-qualified free-liquid exchange update the next mechanical interval's inertia and gravity. Incoming and outgoing mass are co-moving at accepted endpoints. Steady snapshots and optional coverage heat remain separate domains; carried bulk mechanical energy is separate from thermal latent/sensible enthalpy."
        } else {
            "Accepted-step sheet heat and free-liquid evaporation update the next mechanical interval's inertia and gravity under an independent source qualification. Steady snapshots and optional coverage heat remain separate domains; exported bulk mechanical energy is separate from thermal latent/sensible enthalpy."
        });
    }
    if let Some(history) = vapor_history {
        result["vapor_coverage_history"] =
            outcome(history.and_then(|h| h.finish(Some(path.sha256.clone()))));
        let integration = match request
            .vapor_coverage_history
            .as_ref()
            .expect("requested vapor history")
            .integration
        {
            crate::vapor::coverage::history::Integration::RightEndpoint => "right-endpoint",
            crate::vapor::coverage::history::Integration::CompositeTrapezoid => {
                "composite-trapezoid"
            }
        };
        result["vapor_basis"] = json!(format!(
            "Independent steady endpoints and a separately requested {integration} integral of covered prescribed-reservoir potential on every accepted sample. No stored moisture or latent cooling is modeled."
        ));
    } else if scenario.vapor_surface.is_some() || scenario.vapor_coverage.is_some() {
        result["vapor_basis"] = json!(
            "Independent isothermal potential-transfer snapshots on each actual motion endpoint. Layered coverage reprojects the current body and cloth; uncovered transfer remains unknown. No time-integrated evaporation, water supply, moisture storage or latent heat feedback is inferred."
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PartFrame, scene::PartFrames};

    #[test]
    fn source_pose_sampling_preserves_legacy_identity_and_bounds_all_generated_work() {
        let raw = json!({"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.002,"arm_abduction_deg":35.0},
                {"time_s":0.005,"arm_abduction_deg":36.0}],
            "max_step_s":0.003,"max_trials":2,"max_pairs":1000,
            "max_sample_vertices":10000});
        let legacy: Request = serde_json::from_value(raw.clone()).unwrap();
        legacy.validate().unwrap();
        let mut explicit_null = raw.clone();
        explicit_null["source_pose_sampling"] = Value::Null;
        let explicit_null: Request = serde_json::from_value(explicit_null).unwrap();
        assert_eq!(
            serde_json::to_vec(&legacy).unwrap(),
            serde_json::to_vec(&explicit_null).unwrap()
        );
        let vertices = vec![
            Vec3::ZERO,
            Vec3::new(0.1, 0.0, 0.0),
            Vec3::new(0.0, 0.1, 0.0),
        ];
        let triangles = vec![[0, 1, 2]];
        let initial = (vertices.as_slice(), triangles.as_slice());
        let endpoints = vec![vertices.clone(); 2];
        let old_digest = hash(&(
            &hash(&initial).unwrap(),
            &hash(&legacy).unwrap(),
            &endpoints,
        ))
        .unwrap();
        assert_eq!(
            Path::new(&legacy, initial, endpoints.clone())
                .unwrap()
                .sha256(),
            old_digest
        );
        assert_eq!(
            Path::new(&explicit_null, initial, endpoints)
                .unwrap()
                .sha256(),
            old_digest
        );

        let mut sampled = legacy.clone();
        sampled.source_pose_sampling = Some(SourcePoseSampling {
            max_step_s: 0.001,
            max_segments: 5,
        });
        assert!(
            sampled
                .validate()
                .unwrap_err()
                .message
                .contains("max_trials")
        );
        sampled.max_trials = 5;
        sampled.validate().unwrap();
        let steps = sampled.source_steps().unwrap();
        assert_eq!(
            steps.iter().map(|step| step.interval).collect::<Vec<_>>(),
            vec![0, 0, 1, 1, 1]
        );
        for (step, expected) in steps.iter().zip([0.001, 0.002, 0.003, 0.004, 0.005]) {
            assert!((step.time_s - expected).abs() < 1e-15);
        }
        assert_eq!(steps[1].time_s, sampled.keyframes[0].time_s);
        assert_eq!(steps[4].time_s, sampled.keyframes[1].time_s);
        assert!(Path::new(&sampled, initial, vec![vertices.clone(); 2]).is_err());
        Path::new(&sampled, initial, vec![vertices.clone(); 5]).unwrap();
        sampled.source_pose_sampling.as_mut().unwrap().max_segments = 4;
        assert!(
            sampled
                .validate()
                .unwrap_err()
                .message
                .contains("max_segments")
        );
        for invalid_step in [0.0, 1e-10, 1.01, f64::NAN, f64::INFINITY] {
            sampled.source_pose_sampling = Some(SourcePoseSampling {
                max_step_s: invalid_step,
                max_segments: 5,
            });
            assert!(sampled.validate().is_err());
        }
        for invalid_count in [0, 513] {
            sampled.source_pose_sampling = Some(SourcePoseSampling {
                max_step_s: 0.001,
                max_segments: invalid_count,
            });
            assert!(sampled.validate().is_err());
        }
        let mut unknown = raw;
        unknown["source_pose_sampling"] = json!({"max_step_s":0.001,"max_segments":5,"guess":true});
        assert!(serde_json::from_value::<Request>(unknown).is_err());

        sampled.keyframes.truncate(1);
        sampled.keyframes[0].time_s = 1.1e-9;
        sampled.source_pose_sampling = Some(SourcePoseSampling {
            max_step_s: 1e-9,
            max_segments: 2,
        });
        assert!(sampled.validate().unwrap_err().message.contains("sub-1e-9"));
        sampled.keyframes[0].time_s = 0.512;
        sampled.max_step_s = 0.001;
        sampled.max_trials = 512;
        sampled.source_pose_sampling = Some(SourcePoseSampling {
            max_step_s: 0.001,
            max_segments: 512,
        });
        sampled.validate().unwrap();
        let mut large_initial = vertices;
        large_initial.resize(4000, Vec3::ZERO);
        // Admission charges all generated source geometry before attempting to inspect it.
        let refusal = match Path::new(&sampled, (&large_initial, &triangles), vec![vec![]; 512]) {
            Ok(_) => panic!("excessive generated source geometry was admitted"),
            Err(refusal) => refusal,
        };
        assert!(refusal.message.contains("geometry limits"));
    }

    #[test]
    fn legacy_motion_pose_bytes_survive_and_leg_controls_are_bounded() {
        let legacy = json!({"time_s":0.1,"arm_abduction_deg":40.0});
        let frame: Keyframe = serde_json::from_value(legacy.clone()).unwrap();
        assert!(frame.leg_pose.is_none());
        assert_eq!(
            serde_json::to_string(&frame).unwrap(),
            r#"{"time_s":0.1,"arm_abduction_deg":40.0}"#
        );
        let explicit: Keyframe = serde_json::from_value(json!({"time_s":0.1,
            "arm_abduction_deg":40.0,"leg_pose":null}))
        .unwrap();
        assert_eq!(
            serde_json::to_vec(&frame).unwrap(),
            serde_json::to_vec(&explicit).unwrap()
        );
        for extra in [
            json!({"legs":"standing"}),
            json!({"leg_fraction":1.0}),
            json!({"leg_pose":{"posture":"seated","fraction":null}}),
        ] {
            let mut value = legacy.clone();
            value
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert!(serde_json::from_value::<Keyframe>(value).is_err());
        }
        let mut request: Request = serde_json::from_value(json!({"schema":"ftx-drape-motion/1",
            "keyframes":[legacy],"max_step_s":0.1,"max_trials":1,"max_pairs":100,
            "max_sample_vertices":100}))
        .unwrap();
        for fraction in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
            request.keyframes[0].leg_pose = Some(LegPose {
                posture: LegPosture::Seated,
                fraction,
            });
            assert!(request.validate().is_err());
        }
    }

    #[test]
    fn independent_arm_angles_preserve_legacy_bytes_and_require_both_bounded_sides() {
        let source = json!({"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.01,"arm_abduction_deg":35.0}],
            "max_step_s":0.01,"max_trials":16,"max_pairs":10000,"max_sample_vertices":10000});
        let legacy: Request = serde_json::from_value(source.clone()).unwrap();
        assert!(legacy.keyframes[0].arm_pose.is_none());
        let mut explicit_null = source.clone();
        explicit_null["keyframes"][0]["arm_pose"] = Value::Null;
        let explicit_null: Request = serde_json::from_value(explicit_null).unwrap();
        assert_eq!(serde_json::to_value(&legacy).unwrap(), source);
        assert_eq!(
            serde_json::to_vec(&legacy).unwrap(),
            serde_json::to_vec(&explicit_null).unwrap()
        );
        for (left, right) in [(0.0, 180.0), (180.0, 0.0), (70.0, 25.0)] {
            let mut value = source.clone();
            value["keyframes"][0]["arm_pose"] =
                json!({"left_abduction_deg":left,"right_abduction_deg":right});
            let request: Request = serde_json::from_value(value.clone()).unwrap();
            request.validate().unwrap();
            assert_eq!(serde_json::to_value(&request).unwrap(), value);
            for bad in [-0.01, 180.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                for side in ["left", "right"] {
                    let mut invalid = request.clone();
                    let arms = invalid.keyframes[0].arm_pose.as_mut().unwrap();
                    if side == "left" {
                        arms.left_abduction_deg = bad;
                    } else {
                        arms.right_abduction_deg = bad;
                    }
                    let refusal = invalid.validate().unwrap_err();
                    assert_eq!(refusal.code, ftx_core::RefusalCode::OutOfRange);
                    assert!(refusal.message.contains(side));
                }
            }
        }
        for arms in [
            json!({"left_abduction_deg":40.0}),
            json!({"right_abduction_deg":40.0}),
            json!({"left_abduction_deg":null,"right_abduction_deg":40.0}),
            json!({"left_abduction_deg":40.0,"right_abduction_deg":40.0,"elbow_deg":0.0}),
            json!("40,40"),
        ] {
            let mut value = source.clone();
            value["keyframes"][0]["arm_pose"] = arms;
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
    }

    #[test]
    fn leg_keyframes_preserve_legacy_serialization_and_refuse_invalid_postures() {
        let source = json!({"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.01,"arm_abduction_deg":35.0}],
            "max_step_s":0.01,"max_trials":16,"max_pairs":10000,"max_sample_vertices":10000});
        let legacy: Request = serde_json::from_value(source.clone()).unwrap();
        legacy.validate().unwrap();
        assert_eq!(serde_json::to_value(&legacy).unwrap(), source);
        for posture in ["seated", "squat"] {
            for fraction in [0.0, 0.25, 1.0] {
                let mut value = source.clone();
                value["keyframes"][0]["leg_pose"] = json!({"posture":posture,"fraction":fraction});
                let request: Request = serde_json::from_value(value.clone()).unwrap();
                request.validate().unwrap();
                assert_eq!(serde_json::to_value(&request).unwrap(), value);
                for bad in [-0.01, 1.01, f64::NAN, f64::INFINITY] {
                    let mut invalid = request.clone();
                    invalid.keyframes[0].leg_pose.as_mut().unwrap().fraction = bad;
                    assert!(invalid.validate().is_err());
                }
            }
        }
        for pose in [
            json!({"posture":"walking","fraction":0.5}),
            json!({"posture":"seated"}),
            json!({"posture":"seated","fraction":0.5,"root_drop_m":0.1}),
        ] {
            let mut value = source.clone();
            value["keyframes"][0]["leg_pose"] = pose;
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
    }

    #[test]
    fn root_translations_are_absolute_finite_world_coordinates_and_preserve_legacy_bytes() {
        let source = json!({"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.01,"arm_abduction_deg":35.0}],
            "max_step_s":0.01,"max_trials":16,"max_pairs":10000,"max_sample_vertices":10000});
        let legacy: Request = serde_json::from_value(source.clone()).unwrap();
        assert!(legacy.keyframes[0].root_translation_m.is_none());
        assert_eq!(serde_json::to_value(&legacy).unwrap(), source);
        for translation in [[0.01, -0.02, 0.03], [0.0; 3]] {
            let mut value = source.clone();
            value["keyframes"][0]["root_translation_m"] = json!(translation);
            let request: Request = serde_json::from_value(value.clone()).unwrap();
            request.validate().unwrap();
            assert_eq!(request.keyframes[0].root_translation_m, Some(translation));
            assert_eq!(serde_json::to_value(&request).unwrap(), value);
            for axis in 0..3 {
                for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    let mut invalid = request.clone();
                    invalid.keyframes[0].root_translation_m.as_mut().unwrap()[axis] = bad;
                    assert!(invalid.validate().is_err());
                }
            }
        }
        for malformed in [
            json!([]),
            json!([0., 0.]),
            json!([0., 0., 0., 0.]),
            json!("0,0,0"),
        ] {
            let mut value = source.clone();
            value["keyframes"][0]["root_translation_m"] = malformed;
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
        let mut omitted = source.clone();
        omitted["keyframes"][0]["root_translation_m"] = Value::Null;
        let request: Request = serde_json::from_value(omitted).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), source);
    }

    #[test]
    fn root_rotations_are_absolute_bounded_vectors_and_preserve_legacy_bytes() {
        let source = json!({"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.01,"arm_abduction_deg":35.0}],
            "max_step_s":0.01,"max_trials":16,"max_pairs":10000,"max_sample_vertices":10000});
        let legacy: Request = serde_json::from_value(source.clone()).unwrap();
        assert!(
            legacy.keyframes[0]
                .root_rotation_matrix()
                .unwrap()
                .is_none()
        );
        assert_eq!(serde_json::to_value(&legacy).unwrap(), source);
        for rotation in [[0.0; 3], [0.0, 0.0, std::f64::consts::PI], [0.1, -0.2, 0.3]] {
            let mut value = source.clone();
            value["keyframes"][0]["root_rotation_rad"] = json!(rotation);
            let request: Request = serde_json::from_value(value.clone()).unwrap();
            request.validate().unwrap();
            assert_eq!(serde_json::to_value(&request).unwrap(), value);
            for axis in 0..3 {
                for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    let mut invalid = request.clone();
                    invalid.keyframes[0].root_rotation_rad.as_mut().unwrap()[axis] = bad;
                    assert!(invalid.validate().is_err());
                }
            }
        }
        for rotation in [
            [std::f64::consts::PI + 1e-6, 0.0, 0.0],
            [2.3, 2.3, 0.0],
            [f64::MAX, f64::MAX, 0.0],
        ] {
            let mut invalid = legacy.clone();
            invalid.keyframes[0].root_rotation_rad = Some(rotation);
            assert!(invalid.validate().is_err());
        }
        for malformed in [
            json!([]),
            json!([0.0, 0.0]),
            json!([0.0, 0.0, 0.0, 0.0]),
            json!({"x":0.0,"y":0.0,"z":0.0}),
        ] {
            let mut value = source.clone();
            value["keyframes"][0]["root_rotation_rad"] = malformed;
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
        let mut omitted = source.clone();
        omitted["keyframes"][0]["root_rotation_rad"] = Value::Null;
        let request: Request = serde_json::from_value(omitted).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), source);
        let mut unknown = source;
        unknown["keyframes"][0]["root_rotation_deg"] = json!([0.0, 0.0, 90.0]);
        assert!(serde_json::from_value::<Request>(unknown).is_err());
    }

    #[test]
    fn root_rotation_retains_nonzero_angles_below_the_naive_norm_range() {
        let mut frame: Keyframe =
            serde_json::from_value(json!({"time_s":0.01,"arm_abduction_deg":35.0})).unwrap();
        for angle in [1e-200, 1e-300, f64::from_bits(1)] {
            frame.root_rotation_rad = Some([0.0, 0.0, angle]);
            let rotation = frame.root_rotation_matrix().unwrap().unwrap();
            assert_eq!(rotation.rows[1][0], angle);
            assert_eq!(rotation.rows[0][1], -angle);
            assert_eq!(rotation.mul_vec(Vec3::new(1.0, 0.0, 0.0)).y, angle);
        }
        frame.root_rotation_rad = Some([1e-300, -1e-300, 1e-300]);
        let rotation = frame.root_rotation_matrix().unwrap().unwrap();
        assert!(rotation.rows[0][1] < 0.0);
        assert!(rotation.rows[0][2] < 0.0);
        assert!(rotation.rows[1][2] < 0.0);
        frame.root_rotation_rad = Some([-0.0, 0.0, -0.0]);
        assert!(frame.root_rotation_matrix().unwrap().is_none());
    }

    struct Form;
    impl PartFrames for Form {
        fn frame(&self, _: &str) -> Option<PartFrame> {
            Some(PartFrame {
                origin: Vec3::ZERO,
                up: Vec3::new(0., 0., 1.),
                front: Vec3::new(0., -1., 0.),
                lat: Vec3::new(1., 0., 0.),
            })
        }
        fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
            Some(20.)
        }
        fn section_radius(&self, _: &str, _: f64) -> f64 {
            30.
        }
    }

    #[test]
    fn late_path_failure_restores_the_whole_solver_and_replays_the_same_first_step() {
        let pattern = ftx_pattern::Pattern::from_json(include_str!(
            "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
        ))
        .unwrap();
        let scenario: Scenario = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-scenario.json"
        ))
        .unwrap();
        let mut vertices = Vec::new();
        for z in [-0.02, 0.12] {
            for i in 0..16 {
                let (s, c) = ftx_core::det::sin_cos(i as f64 * std::f64::consts::TAU / 16.);
                vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
            }
        }
        let triangles: Vec<_> = (0..16)
            .flat_map(|i| {
                let j = (i + 1) % 16;
                [[i, j, j + 16], [i, j + 16, i + 16]]
            })
            .collect();
        let body = DrapeBody {
            frames: &Form,
            collision: (&vertices, &triangles),
            display: (&vertices, &triangles),
            arm_deg: 35.,
        };
        let mut run = crate::run::drape(
            &pattern,
            &scenario.fit_spec,
            &body,
            &scenario.options,
            |_| {},
        )
        .unwrap();
        let original = run.scene.solver.clone();
        let count = vertices.len() + original.x.len();
        let request: Request = serde_json::from_value(json!({"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.002,"arm_abduction_deg":35.0},
                {"time_s":0.004,"arm_abduction_deg":36.0}],
            "max_step_s":0.001,"max_trials":16,"max_pairs":100000,"max_sample_vertices":3 * count}))
        .unwrap();
        let endpoints: Vec<Vec<_>> = [0.0001, 0.0002]
            .into_iter()
            .map(|z| vertices.iter().map(|p| *p + Vec3::new(0., 0., z)).collect())
            .collect();
        let path = Path::new(&request, body.collision, endpoints).unwrap();
        let options = MotionOptions {
            max_step_s: 0.001,
            max_trials: 16,
            max_pairs: 100000,
            max_sample_vertices: 2 * count,
        };
        let mut expected = original.clone();
        let first = advance_body(&mut expected, &path.vertices[0], 0.002, options).unwrap();
        assert_eq!(first.samples.len(), 2);
        assert_ne!(expected.x, original.x);
        let refusal = evaluate(
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
        .unwrap_err();
        assert!(
            refusal
                .message
                .contains("whole-trajectory motion budget exhausted")
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
        let replay =
            advance_body(&mut run.scene.solver, &path.vertices[0], 0.002, options).unwrap();
        assert_eq!(format!("{replay:?}"), format!("{first:?}"));
    }
}
#[cfg(test)]
#[path = "motion_liquid_tests.rs"]
mod liquid_rollback_tests;
