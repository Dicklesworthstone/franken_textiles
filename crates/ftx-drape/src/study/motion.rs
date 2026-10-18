//! Material-following continuation of an already sewn drape, not independent pose restarts.
use ftx_core::{Refusal, Vec3, intersection::validate_mesh};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Scenario, budget, coverage_at, hash, heat, invalid, omitted, outcome};
use crate::{
    motion::{MotionOptions, advance_body},
    run::{DrapeBody, DrapeRun},
    start::{edge_strain, face_edges},
};

/// A source-model endpoint. Between endpoints the checked solver follows linear vertex chords,
/// not an inferred anatomical joint trajectory. Use short pose increments to approximate a turn.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    /// Cumulative physical seconds since the end of the initial drape, strictly increasing.
    pub time_s: f64,
    /// Standing straight-arm angle from vertical, in degrees, 0..=180.
    pub arm_abduction_deg: f64,
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
    /// Total retained body plus cloth vertex samples, including the initial state, 1..=2000000.
    pub max_sample_vertices: usize,
    /// Optional independent-sheet transient temperatures on every accepted mechanical sample.
    /// Its explicit initial temperature applies at motion time zero, not during initial draping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transient_surface: Option<heat::SurfaceRequest>,
    /// Optional transient layer/body-domain heat, with material enthalpy remapped on every
    /// accepted geometric sample. Independent of sheet heat and steady endpoint snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transient_coverage: Option<heat::coverage::Request>,
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
            minimum_trials += ((frame.time_s - previous) / self.max_step_s - 1e-12).ceil();
            previous = frame.time_s;
        }
        if minimum_trials > self.max_trials as f64 {
            return Err(budget(
                "motion requires more nominal substeps than max_trials",
            ));
        }
        Ok(())
    }
}

/// Prepared endpoint meshes with unchanged topology, tied to the exact initial collision mesh and
/// policy. The model adapter generates these once from the selected shape, before any material run.
pub struct Path {
    initial_sha256: String,
    request_sha256: String,
    vertices: Vec<Vec<Vec3>>,
    sha256: String,
}

impl Path {
    /// Bind endpoint geometry to an initial collision mesh. This is also the explicit seam for a
    /// caller with its own body model; source endpoint generation remains that caller's responsibility.
    ///
    /// # Errors
    /// Invalid policy, count, mesh or an excessive total of retained endpoint vertices.
    pub fn new(
        request: &Request,
        initial: (&[Vec3], &[[usize; 3]]),
        vertices: Vec<Vec<Vec3>>,
    ) -> Result<Self, Refusal> {
        request.validate()?;
        if vertices.len() != request.keyframes.len()
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
        let sha256 = hash(&(&initial_sha256, &request_sha256, &vertices))?;
        Ok(Self {
            initial_sha256,
            request_sha256,
            vertices,
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
}

// This mutates only a private run after its static result was retained. On ANY late failure the
// composing consumer returns a refusal, never a partial trajectory or a changed static result.
pub(super) fn evaluate(
    request: &Request,
    path: &Path,
    scenario: &Scenario,
    body: &DrapeBody<'_>,
    run: &mut DrapeRun,
    heat_budget: &mut heat::Budget,
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
    let count = body
        .collision
        .0
        .len()
        .checked_add(run.scene.solver.x.len())
        .ok_or_else(|| budget("motion sample count overflow"))?;
    let mut remaining_vertices = request
        .max_sample_vertices
        .checked_sub(count)
        .filter(|n| *n >= count)
        .ok_or_else(|| budget("motion sample budget cannot retain initial and final body/cloth"))?;
    let mut remaining_trials = request.max_trials;
    let initial_positions_sha256 = hash(&run.scene.solver.x)?;
    // The thermal state is private to this material run. Its failure never aborts mechanics;
    // conversely a mechanical failure publishes neither a partial path nor its partial heat.
    let mut surface_heat = request
        .transient_surface
        .as_ref()
        .map(|policy| policy.start(run, &request.keyframes, heat_budget));
    let mut coverage_heat = request
        .transient_coverage
        .as_ref()
        .map(|policy| policy.start(run, body.collision, request.keyframes.len(), heat_budget));
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
    for (i, frame) in request.keyframes.iter().enumerate() {
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
        let report = advance_body(
            &mut run.scene.solver,
            &path.vertices[i],
            frame.time_s - previous_time,
            MotionOptions {
                max_step_s: request.max_step_s,
                max_trials: remaining_trials,
                max_pairs: request.max_pairs,
                max_sample_vertices: remaining_vertices,
            },
        )?;
        remaining_trials -= report.trials;
        remaining_vertices -= report.samples.len() * count;
        pairs_tested += report.pairs_tested;
        for sample in report.samples {
            peak_strain = peak_strain.max(strain(&sample.cloth)?);
            if let Some(Ok(history)) = &mut surface_heat
                && let Err(refusal) = history.advance(
                    &sample.cloth,
                    previous_time + sample.time_s,
                    i,
                    samples.len(),
                )
            {
                surface_heat = Some(Err(refusal));
            }
            if let Some(Ok(history)) = &mut coverage_heat
                && let Err(refusal) = history.advance(
                    &sample.cloth,
                    &sample.body,
                    previous_time + sample.time_s,
                    i,
                    samples.len(),
                )
            {
                coverage_heat = Some(Err(refusal));
            }
            samples.push(json!({"time_s":previous_time + sample.time_s,
                "positions_m":sample.cloth,"body_positions_m":sample.body,
                "solve":{"converged":sample.solve.converged,"iterations":sample.solve.iterations,
                    "residual_n":sample.solve.residual,"last_step_m":sample.solve.last_step,
                    "energy_start_j":sample.solve.energy_start,"energy_end_j":sample.solve.energy_end,
                    "factorizations":sample.solve.factorizations,"ccd_limited":sample.solve.ccd_limited}}));
        }
        // Rebuild source-locked heat input from the NEW solver geometry; never patch an old receipt.
        let snapshot = json!({"material":run.report["material"],
            "steady_thermal_surface":crate::thermal::steady::surface::for_drape(&run.scene, &run.report["material"])});
        let surface = scenario
            .surface
            .as_ref()
            .map_or_else(omitted, |c| outcome(c.evaluate(&snapshot)));
        let coverage = scenario.coverage.as_ref().map_or_else(omitted, |c| {
            outcome(coverage_at(
                c,
                &snapshot,
                (&path.vertices[i], body.collision.1),
            ))
        });
        endpoints.push(
            json!({"time_s":frame.time_s,"arm_abduction_deg":frame.arm_abduction_deg,
            "sample_index":samples.len()-1,"positions_sha256":hash(&run.scene.solver.x)?,
            "body_collision_sha256":hash(&(&path.vertices[i],body.collision.1))?,
            "surface":surface,"coverage":coverage}),
        );
        previous_time = frame.time_s;
    }
    let mut result = json!({"schema":"ftx-drape-motion-result/1","path_sha256":path.sha256,
        "initial_positions_sha256":initial_positions_sha256,"initial_body_collision_sha256":path.initial_sha256,
        "initial_drape_settled":run.report["settled"],"initial_drape_gating":run.report["gating"],
        "duration_s":previous_time,"trials":request.max_trials-remaining_trials,"pairs_tested":pairs_tested,
        "retained_vertex_samples":request.max_sample_vertices-remaining_vertices,
        "samples":samples,"keyframes":endpoints,"triangles":faces,"body_triangles":body.collision.1,
        "final_velocities_m_s":run.scene.solver.v,
        "metrics":{"max_absolute_edge_strain_pct":peak_strain},
        "membrane_history":run.scene.solver.relaxation_report()?,
        "bending_history":run.scene.solver.bending_plasticity_report()?,
        "continuous_contact_checked":true,"physical_validation_claim":false,"comfort_claim":false,
        "thermal_basis":"Independent steady endpoint snapshots, not transient temperatures or heat during motion.",
        "scope":"Prescribed piecewise-linear body-vertex motion from the computed initial drape. All accepted substeps are retained; no independent restarts or empirical ride-up/fit claim. Body self-intersection is not certified."});
    if let Some(history) = surface_heat {
        result["transient_surface"] = outcome(
            history.and_then(|h| h.finish(&path.sha256, &initial_positions_sha256, previous_time)),
        );
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
    Ok(result)
}
