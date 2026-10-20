//! Fresh-state drape and thermal evaluations for fixed-body material studies (plan §25.10).
//!
//! Manufacturing composition belongs to `ftx-export::material_study`, not to this numerical crate.
//! A session borrows one immutable body/pose, charges aggregate work before starting each drape,
//! and never uses the previous alternative's mesh, solver history or heat fields as its start.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{
    Refusal, RefusalCode, Vec2, curve::EDGE_DENSE, polyline::polyline_length, receipt::sha256_hex,
};
use ftx_pattern::{Pattern, truthy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    animation::{FOLD_FRAMES, ZIP_FRAMES},
    run::{DrapeBody, DrapeOptions, drape},
    scene::{casing_at_cut_length, elastic_casing},
    thermal::{
        steady::{coverage::request as coverage, request::SurfaceConditions},
        surface::projection::Settings,
    },
};

pub mod fitted;
pub mod heat;
pub mod motion;
mod options;

/// Coverage is anchored to triangle IDs on this session's collision body, never a saved old pose.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    /// Nonempty, unique collision-body triangle indices, in boundary-condition order.
    pub body_triangle_ids: Vec<usize>,
    /// Explicit physical layer identity for every retained drape instance.
    pub instance_layers: BTreeMap<String, String>,
    /// The existing normal-closure, interface and optional source-qualified gas-gap policy.
    pub settings: Settings,
    /// Existing prescribed reservoirs or qualified exterior exchange; never inferred weather.
    pub conditions: coverage::Conditions,
}

/// One numerical and environmental scenario shared by all material alternatives.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Must be `ftx-drape-scenario/1`.
    pub schema: String,
    /// Existing production drape settings. Omitted individual fields use their documented defaults.
    pub options: DrapeOptions,
    /// Existing flat fit contract used to interpret drape metrics; not an empirical fit claim.
    pub fit_spec: Value,
    /// Conservative mesh-point/lattice bound per alternative, in 3..32768.
    pub max_mesh_points: usize,
    /// Aggregate point × recorded-frame budget, in 1..64000000, applied independently to static
    /// draping and motion. Admitted attempts stay charged after refusal; no wall-time promise.
    pub max_vertex_frames: usize,
    /// Reserved motion trials across alternatives, in 1..32768; defaults to 4096.
    #[serde(
        default = "default_motion_trials",
        skip_serializing_if = "is_default_motion_trials"
    )]
    pub max_motion_trials: usize,
    /// Independent sheet-column heat exchange. Missing storage does not block steady transport.
    #[serde(default)]
    pub surface: Option<SurfaceConditions>,
    /// Optional actual body/layer overlap and series transport on the newly draped geometry.
    #[serde(default)]
    pub coverage: Option<Coverage>,
    /// Optional source-model motion, continuing the same sewn cloth and material history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion: Option<motion::Request>,
}

fn default_motion_trials() -> usize {
    4096
}
fn is_default_motion_trials(value: &usize) -> bool {
    *value == default_motion_trials()
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn budget(message: &str) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}
fn hash<T: Serialize>(value: &T) -> Result<String, Refusal> {
    Ok(sha256_hex(
        &serde_json::to_vec(value).map_err(|e| invalid(e.to_string()))?,
    ))
}
fn outcome(result: Result<Value, Refusal>) -> Value {
    match result {
        Ok(result) => json!({"status":"available","result":result}),
        Err(refusal) => json!({"status":"refused","refusal":refusal}),
    }
}
fn omitted() -> Value {
    json!({"status":"not_requested"})
}

impl Scenario {
    /// Validate numerical bounds before meshing, body fitting or starting any solver.
    /// Thermal validation stays in its independent consumer and cannot erase a valid drape.
    ///
    /// # Errors
    /// Unknown schema, invalid production options or an excessive per-run/aggregate budget.
    pub fn validate(&self) -> Result<(), Refusal> {
        self.options.validate()?;
        if let Some(motion) = &self.motion {
            motion.validate()?;
        }
        if self.schema != "ftx-drape-scenario/1" {
            return Err(invalid("expected ftx-drape-scenario/1"));
        }
        if !(3..=32_768).contains(&self.max_mesh_points)
            || !(1..=64_000_000).contains(&self.max_vertex_frames)
            || !(1..=32_768).contains(&self.max_motion_trials)
            || self
                .options
                .sew_steps
                .saturating_add(self.options.drape_steps)
                > 256
            || self.options.lag_rounds.iter().any(|&n| n > 4)
            || self.options.pcg_iters > 128
        {
            return Err(budget("scenario exceeds mesh/frame/solver work limits"));
        }
        Ok(())
    }
}

/// A single fixed body and scenario with an aggregate admission budget. No solver state is retained.
pub struct Session<'a, 'body> {
    scenario: &'a Scenario,
    body: &'a DrapeBody<'body>,
    body_sha256: String,
    scenario_sha256: String,
    charged_vertex_frames: usize,
    charged_motion_frames: usize,
    charged_motion_trials: usize,
    charged_motion_vertices: usize,
    motion_path: Option<Result<motion::Path, Refusal>>,
    heat_budget: heat::Budget,
}

impl<'a, 'body> Session<'a, 'body> {
    /// Bind one immutable posed body; material edits cannot change it inside this session.
    ///
    /// # Errors
    /// Invalid scenario, malformed or excessive body geometry, or non-finite pose angle.
    pub fn new(scenario: &'a Scenario, body: &'a DrapeBody<'body>) -> Result<Self, Refusal> {
        scenario.validate()?;
        for (vertices, triangles) in [body.collision, body.display] {
            if vertices.is_empty()
                || vertices.len() > 196_608
                || triangles.is_empty()
                || triangles.len() > 65_536
            {
                return Err(budget("scenario body exceeds geometry limits"));
            }
            ftx_core::intersection::validate_mesh(vertices, triangles)?;
        }
        Refusal::finite("scenario arm angle", body.arm_deg)?;
        let body_sha256 = hash(&(body.collision, body.display, body.arm_deg))?;
        let scenario_sha256 = hash(&(scenario, &body_sha256))?;
        Ok(Self {
            scenario,
            body,
            body_sha256,
            scenario_sha256,
            charged_vertex_frames: 0,
            charged_motion_frames: 0,
            charged_motion_trials: 0,
            charged_motion_vertices: 0,
            motion_path: None,
            heat_budget: heat::Budget::default(),
        })
    }

    /// Retained scenario and body identities for the application-level study request.
    #[must_use]
    pub fn descriptor(&self) -> Value {
        let mut descriptor = json!({"scenario":self.scenario,"body_geometry_sha256":self.body_sha256,
            "scenario_sha256":self.scenario_sha256,
            "body_policy":"one fixed caller-supplied posed body; no material-dependent refitting"});
        if let Some(path) = &self.motion_path {
            descriptor["motion_path"] = match path {
                Ok(path) => json!({"sha256":path.sha256()}),
                Err(refusal) => json!({"refusal":refusal}),
            };
        }
        descriptor
    }

    /// Attach endpoint geometry once, before any alternative runs. A setup refusal belongs only
    /// to motion, preserving independent static drape, heat and manufacturing results.
    ///
    /// # Errors
    /// No requested motion, a second attachment or an already-started session.
    pub fn set_motion_path(&mut self, path: Result<motion::Path, Refusal>) -> Result<(), Refusal> {
        let request = self
            .scenario
            .motion
            .as_ref()
            .ok_or_else(|| invalid("no motion requested"))?;
        if self.charged_vertex_frames != 0 || self.motion_path.is_some() {
            return Err(invalid(
                "motion path is immutable once attached or evaluation starts",
            ));
        }
        self.motion_path = Some(path.and_then(|p| {
            p.validate_for(request, self.body)?;
            Ok(p)
        }));
        self.scenario_sha256 = hash(&(
            self.scenario,
            &self.body_sha256,
            self.descriptor()["motion_path"].clone(),
        ))?;
        Ok(())
    }

    /// Run actual draping, then evaluate each requested heat consumer independently on its new result.
    /// Gating flags are retained: producing numerical output does not mean a settled or qualified fit.
    ///
    /// # Errors
    /// Invalid/budget-exceeding geometry or a production drape refusal. A failed drape produces no
    /// thermal output; independent manufacturing outcomes remain with the composing study.
    pub fn evaluate(
        &mut self,
        pattern: &Pattern,
        mut progress: impl FnMut(&str),
    ) -> Result<Value, Refusal> {
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(invalid(errors.join("; ")));
        }
        let simulated = elastic_casing(pattern, self.scenario.options.elastic_ea)
            .map(|c| casing_at_cut_length(pattern, &c));
        let points = mesh_bound(
            simulated.as_ref().unwrap_or(pattern),
            self.scenario.options.spacing,
            self.scenario.max_mesh_points,
        )?;
        let frames = self.scenario.options.sew_steps
            + self.scenario.options.drape_steps
            + FOLD_FRAMES
            + ZIP_FRAMES
            + 4;
        let charge = points
            .checked_mul(frames)
            .ok_or_else(|| budget("scenario work overflow"))?;
        self.charged_vertex_frames = self.charged_vertex_frames.saturating_add(charge);
        if self.charged_vertex_frames > self.scenario.max_vertex_frames {
            return Err(budget(
                "aggregate scenario vertex-frame budget exhausted; no drape started",
            ));
        }
        // Motion cannot spend the allowance needed by a later alternative's independent static
        // drape. Reserve its samples, endpoint geometry and trials separately before continuation.
        let motion_admission = self.scenario.motion.as_ref().map(|request| {
            let count = self.body.collision.0.len().saturating_add(points);
            if count > request.max_sample_vertices {
                return Err(budget("motion cannot retain its initial body and cloth sample"));
            }
            let samples = count.saturating_mul(request.max_trials + 1)
                .min(request.max_sample_vertices);
            let endpoint_copies = if self.scenario.surface.is_some() || self.scenario.coverage.is_some() {
                7 * request.keyframes.len()
            } else {
                0
            };
            // Endpoint heat receipts duplicate geometry; final velocities retain one more array.
            let retained = samples.saturating_add(count.saturating_mul(endpoint_copies + 1));
            let work = samples.saturating_mul(8).max(retained);
            let total_work = self.charged_motion_frames.saturating_add(work);
            let total_trials = self.charged_motion_trials.saturating_add(request.max_trials);
            let total_retained = self.charged_motion_vertices.saturating_add(retained);
            if total_work > self.scenario.max_vertex_frames
                || total_trials > self.scenario.max_motion_trials
                || total_retained > motion::MAX_RETAINED_VERTICES
            {
                return Err(budget("aggregate motion trial, work or retained-geometry budget exhausted; no motion started"));
            }
            self.charged_motion_frames = total_work;
            self.charged_motion_trials = total_trials;
            self.charged_motion_vertices = total_retained;
            Ok((work, retained))
        });
        let mut run = drape(
            pattern,
            &self.scenario.fit_spec,
            self.body,
            &self.scenario.options,
            &mut progress,
        )?;
        let surface = self
            .scenario
            .surface
            .as_ref()
            .map_or_else(omitted, |conditions| {
                outcome(conditions.evaluate(&run.report))
            });
        let coverage = self
            .scenario
            .coverage
            .as_ref()
            .map_or_else(omitted, |policy| {
                outcome(coverage_at(policy, &run.report, self.body.collision))
            });
        let positions_sha256 = hash(&run.scene.solver.x)?;
        // Exact resolved placement data disambiguates callers with equal skins but different frames.
        let placements: Vec<_> = run.scene.placements.iter().map(|p| json!({
            "frame":{"origin":p.frame.origin,"up":p.frame.up,"front":p.frame.front,"lat":p.frame.lat},
            "h0":p.h0,"radius":p.radius,"theta_deg":p.theta_deg,"x0":p.x0,
            "mirrored":p.mirrored,"own_side":p.own_side})).collect();
        let mut result = json!({"schema":"ftx-drape-scenario-result/1", "pattern_sha256":hash(pattern.raw())?,
            "scenario_sha256":self.scenario_sha256,"body_geometry_sha256":self.body_sha256,
            "placements_sha256":hash(&placements)?,"placements":placements,
            "positions_sha256":positions_sha256,"positions_m":run.scene.solver.x,
            "triangles":run.scene.solver.model.membranes.iter().map(|t| t.v).collect::<Vec<_>>(),
            "metrics":{"cloth_mass_kg":run.report["mass_kg"],
                "simulation_gating_clear":run.gating().is_empty(),
                "independent_surface_power_w":surface["result"]["steady"]["power_w"],
                "body_coverage_power_w":coverage["result"]["steady"]["power_w"]},
            "drape":run.report,"surface":surface,"coverage":coverage,
            "charged_vertex_frames":charge,"physical_validation_claim":false,
            "comfort_claim":false,"hardware_execution_authorized":false,
            "note":"Heat is evaluated on this final computed frame even when drape flags require review. No scalar ranking or empirical fit/whole-wearer claim is made."});
        if let Some(request) = &self.scenario.motion {
            result["motion"] = outcome((|| {
                let (work, retained) = motion_admission.expect("requested motion has admission")?;
                let mut moved = match &self.motion_path {
                    Some(Ok(path)) => motion::evaluate(
                        request,
                        path,
                        self.scenario,
                        self.body,
                        &mut run,
                        &mut self.heat_budget,
                        progress,
                    ),
                    Some(Err(refusal)) => Err(refusal.clone()),
                    None => Err(invalid(
                        "motion needs prepared endpoint meshes from the same body shape",
                    )),
                }?;
                moved["reserved_vertex_frames"] = json!(work);
                moved["reserved_retained_vertices"] = json!(retained);
                moved["reserved_trials"] = json!(request.max_trials);
                Ok(moved)
            })());
        }
        Ok(result)
    }
}

fn coverage_at(
    policy: &Coverage,
    report: &Value,
    body: (&[ftx_core::Vec3], &[[usize; 3]]),
) -> Result<Value, Refusal> {
    let mut seen = BTreeSet::new();
    if policy.body_triangle_ids.is_empty()
        || policy.body_triangle_ids.len() > 4096
        || policy
            .body_triangle_ids
            .iter()
            .any(|&i| i >= body.1.len() || !seen.insert(i))
    {
        return Err(invalid(
            "coverage needs 1..4096 unique current collision-body triangle IDs",
        ));
    }
    coverage::Request {
        schema: "ftx-steady-coverage-request/1".into(),
        report: report.clone(),
        map: coverage::Map {
            body_positions_m: body.0.to_vec(),
            body_triangles: policy
                .body_triangle_ids
                .iter()
                .map(|&i| body.1[i])
                .collect(),
            instance_layers: policy.instance_layers.clone(),
            settings: policy.settings.clone(),
        },
        conditions: policy.conditions.clone(),
    }
    .evaluate()
}

// Conservative admission bound BEFORE the reference mesher allocates its spacing-dependent arrays.
// Count all lattice candidates, not only interior hits. Seam subdivision is bounded by both side
// lengths plus junction/notch breaks; charge the full seam count to every participating edge.
fn mesh_bound(pattern: &Pattern, spacing: f64, limit: usize) -> Result<usize, Refusal> {
    let mut bounds = BTreeMap::new();
    let mut boundary = 0.0;
    let mut lattice = 0.0;
    let mut edge_count = 0usize;
    for inst in &pattern.instances {
        let piece = &pattern.raw()["pieces"][inst.piece_index];
        if piece["sim"] == false || !piece.get("placement").is_some_and(truthy) {
            continue;
        }
        edge_count = edge_count.saturating_add(inst.edges.len());
        if edge_count > 4096 || bounds.len() >= 128 {
            return Err(budget("scenario outline budget exceeded"));
        }
        let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
        let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for edge in &inst.edges {
            let pts = inst
                .edge_dense(&edge.id, EDGE_DENSE)
                .map_err(|e| invalid(e.to_string()))?;
            for p in &pts {
                if !p.is_finite() {
                    return Err(invalid("nonfinite scenario outline"));
                }
                lo.x = lo.x.min(p.x);
                lo.y = lo.y.min(p.y);
                hi.x = hi.x.max(p.x);
                hi.y = hi.y.max(p.y);
            }
            boundary += (polyline_length(&pts) / spacing).ceil().max(1.0) + 1.0;
        }
        // Resampled boundary points lie within these dense-polyline bounds.
        lattice += (((hi.x - lo.x) / spacing).ceil() + 2.0)
            * (((hi.y - lo.y) / (spacing * 0.866)).ceil() + 2.0);
        bounds.insert(inst.id.as_str(), inst.edges.len());
    }
    for seam in pattern.seams() {
        let (Some(a), Some(b)) = (seam.side_ref("a"), seam.side_ref("b")) else {
            continue;
        };
        let active = |s: &str| bounds.contains_key(s.split('/').next().unwrap_or(s));
        if !active(a) || !active(b) {
            continue;
        }
        let (pa, ja) = pattern
            .side_polyline(a)
            .map_err(|e| invalid(e.to_string()))?;
        let (pb, jb) = pattern
            .side_polyline(b)
            .map_err(|e| invalid(e.to_string()))?;
        let na = pattern
            .notch_fractions(a)
            .map_err(|e| invalid(e.to_string()))?;
        let nb = pattern
            .notch_fractions(b)
            .map_err(|e| invalid(e.to_string()))?;
        let (_, ea) = pattern.side(a).map_err(|e| invalid(e.to_string()))?;
        let (_, eb) = pattern.side(b).map_err(|e| invalid(e.to_string()))?;
        let count = ((polyline_length(&pa) + polyline_length(&pb)) / spacing).ceil()
            + (ja.len() + jb.len() + na.len() + nb.len() + 4) as f64;
        boundary += count * (ea.len() + eb.len()) as f64;
    }
    let bound = boundary + lattice;
    if !bound.is_finite() || bound < 3.0 || bound > limit as f64 {
        return Err(budget(
            "scenario conservative mesh bound exceeded; coarsen spacing or raise max_mesh_points",
        ));
    }
    Ok(bound.ceil() as usize)
}
