//! One drape run end to end — what `ftx drape`, the examples and (later) the browser call: mesh the
//! pattern, place and start it on a body, sew and drape it, record the fold-up, and read the metrics and
//! flags into a `sim_report.json`-shaped report (spec 04 §6.11).

use ftx_core::np::percentile;
use ftx_core::py::round;
use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_pattern::{Pattern, Side, instance::physical_side};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::animation::{
    FOLD_FRAMES, FoldUp, SEAM_COLORS, ViewerExtras, ZIP_FRAMES, board_layout, fold_frames,
    viewer_html, viewer_payload, zip_frames,
};
use crate::cloth::NewtonStrategy;
use crate::contact::{AdaptiveBarrierPolicy, Contact, ContactParams, StickSlipFriction};
use crate::damping::DampingOptions;
use crate::elements::{BendingModel, CompressionModel};
use crate::garment::{
    ConstructionMechanicsOptions, GarmentMesh, MeshOptions, build_garment,
    build_garment_with_options, mesh_quality, simulation_scope,
};
use crate::integrator::TimeIntegrator;
use crate::metrics::{MIN_FRAMES, MetricsInput, metrics};
use crate::nearest::NearestTree;
use crate::scene::{
    Fabric, PartFrames, PhaseAudit, PhaseReport, RefusedSubstep, RejectedEndpoint, Scene,
    SceneBuildOptions, casings_at_cut_length, elastic_casings, run_phase_audited,
};
use crate::start::{SelfRepulsionOptions, edge_strain, face_edges};
use crate::stepping::SubdivisionOptions;

/// Standard gravity (m/s²) of the drape phase.
pub const GRAVITY: f64 = 9.81;

const SETTLE_SPEED_M_S: f64 = 1e-3;
const SETTLE_QUIET_STEPS: usize = 3;

/// An independent, frictionless static comparison from the final dynamic garment state.
/// It never changes the recorded trajectory, its elapsed time, metrics or subsequent motion.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StaticEquilibriumOptions {
    /// Stop when the maximum free Newton correction is below this distance in metres.
    /// This is distinct from the dynamic solver's correction-per-time tolerance in m/s.
    pub correction_tolerance_m: f64,
    /// Maximum Newton iterations, in 1..=1000; a refused solve publishes no trial geometry.
    pub max_newton_iterations: usize,
}

impl Default for StaticEquilibriumOptions {
    fn default() -> Self {
        Self {
            correction_tolerance_m: 1e-6,
            max_newton_iterations: 300,
        }
    }
}

impl StaticEquilibriumOptions {
    /// Validate the independent solve's explicit distance tolerance and work cap.
    ///
    /// # Errors
    /// Non-finite/nonpositive tolerance or a Newton cap outside 1..=1000.
    pub fn validate(&self) -> Result<(), Refusal> {
        Refusal::finite(
            "static-equilibrium correction tolerance",
            self.correction_tolerance_m,
        )?;
        if self.correction_tolerance_m <= 0.0 || !(1..=1000).contains(&self.max_newton_iterations) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "static equilibrium needs a positive correction tolerance in metres and 1..=1000 Newton iterations",
            ));
        }
        Ok(())
    }

    /// Conservative point-frame reservation for retained geometry, copied state and bounded
    /// solves, using the ordinary solver's 300-iteration cap as one solve-equivalent.
    pub(crate) fn vertex_frames(&self) -> usize {
        8 + self.max_newton_iterations.div_ceil(300)
    }
}

/// The body a run drapes on.
pub struct DrapeBody<'a> {
    /// Part frames, landmarks and section radii.
    pub frames: &'a (dyn PartFrames + Sync),
    /// The surface the cloth collides with and the metrics measure against (vertices in m, triangles).
    pub collision: (&'a [Vec3], &'a [[usize; 3]]),
    /// The surface the viewer shows.
    pub display: (&'a [Vec3], &'a [[usize; 3]]),
    /// Common arm abduction (degrees), overridden by `arm_pose` when present.
    pub arm_deg: f64,
    /// Exact independently authored initial arm angles, when supplied.
    pub arm_pose: Option<ftx_body::drape::ArmPose>,
}

/// Run settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrapeOptions {
    /// Mesh spacing (mm).
    pub spacing: f64,
    /// Steps of the zero-gravity sewing phase.
    pub sew_steps: usize,
    /// Steps under gravity.
    pub drape_steps: usize,
    /// Optional additional gravity steps, at the same time step, until the existing numerical
    /// quiet window and completed body-contact offset are reached. Zero preserves the fixed schedule;
    /// history laws refuse this tail.
    pub max_settle_steps: usize,
    /// Report-only hold window: last N accepted gravity steps; zero means not requested.
    pub elastic_hold_steps: usize,
    /// Optional conservative equilibrium comparison after the dynamic schedule. Normal contact
    /// is retained; friction and time-dependent/irreversible laws are not a static equilibrium law.
    /// A numerical or material refusal is reported independently and preserves the dynamic run.
    pub static_equilibrium: Option<StaticEquilibriumOptions>,
    /// Time step (s).
    pub step: f64,
    /// Optional convergence-driven time bisection, with one attempted-step budget shared by sewing,
    /// gravity and settling. Output frames keep the nominal `step`; actual partitions are reported.
    pub time_subdivision: Option<SubdivisionOptions>,
    /// Newton tolerance on ‖Δx‖∞/h (m/s). The default, 1e-3 (each step converged to 42 µm), halves the
    /// work of 1e-4: tighter, Newton descends into the cloth's tiny buckling modes, where the exact Hessian is
    /// indefinite and the projected one converges linearly (the 92 cm bolero 374 → 180 iterations, the
    /// trousers 563 → 334; hems and bands within 0.3 mm, seam strains within 0.05 %, the casing's 1 %).
    pub tol: f64,
    /// Compression stiffness as a fraction of tension, outside gather zones (inside them it falls to
    /// [`crate::start::GATHER_COMPRESSION`] at the gathered seam).
    pub compression: f64,
    /// Friction lag rounds per step (re-solves with the normal forces and contacts of the step's end)
    /// while sewing and while draping.
    pub lag_rounds: [usize; 2],
    /// Axial stiffness EA of a drafted casing's elastic (N): the casing is simulated at its cut length,
    /// drawn up by a tension-only elastic ring. 0: no elastic, the casing at its finished size
    /// (`sim_scale`) as the reference simulates it.
    pub elastic_ea: f64,
    /// Body contact and friction.
    pub contact: ContactParams,
    /// Optional cloth–cloth contact: independent friction and the sum of both half-thicknesses.
    /// Starts with intersecting layers are refused. `None` preserves body-only draping and is
    /// explicitly reported as self-contact not simulated.
    pub self_contact: Option<ContactParams>,
    /// Inexact Newton ([`crate::cloth::Solver::pcg_iters`]): conjugate-gradient iterations preconditioned
    /// by the last factorization before factorizing anew; 0 factorizes every iteration. The default, 20,
    /// halves the 92 cm trousers' CPU time (164 → 83 s; 10: 116 s, 40: 87 s, 80: 91 s) and takes the
    /// bolero's from 18.7 to 11.4 s, every reading within 0.2 mm: a factorization costs about 250
    /// conjugate-gradient iterations.
    pub pcg_iters: usize,
    /// Nonlinear globalization and linear-solve strategy (default: [`NewtonStrategy::ProjectedNewton`]).
    pub newton_strategy: NewtonStrategy,
    /// Structural time-integration scheme (default: [`TimeIntegrator::ImplicitEuler`]).
    pub integrator: TimeIntegrator,
    /// Hinge bending formulation (default: legacy [`BendingModel::Quadratic`]).
    pub bending_model: BendingModel,
    /// Membrane compression-softening formulation (default: legacy [`CompressionModel::AxisAligned`]).
    pub compression_model: CompressionModel,
    /// Opt-in aerodynamic drag and SE(3)-invariant Rayleigh internal damping (default: disabled).
    pub damping: DampingOptions,
    /// Opt-in Li et al. (2020) adaptive IPC barrier stiffness scaling (`None` preserves legacy fixed `kappa`).
    pub adaptive_barrier: Option<AdaptiveBarrierPolicy>,
    /// Opt-in smooth static/kinetic stick-slip friction for cloth–body contact (`None` preserves legacy Coulomb `mu`).
    pub stick_slip: Option<StickSlipFriction>,
    /// Opt-in smooth static/kinetic stick-slip friction for cloth–cloth self-contact (`None` preserves legacy Coulomb `mu`).
    pub self_stick_slip: Option<StickSlipFriction>,
    /// Opt-in curvature/gather-adaptive meshing and bounded interior Lloyd CVT smoothing (default: uniform spacing, 0 CVT passes).
    pub mesh: MeshOptions,
    /// Opt-in folded-hem and sewn seam-allowance mass and bending multipliers (default: 1.0× everywhere, 0 mm bands).
    pub construction: ConstructionMechanicsOptions,
    /// Opt-in bounded cloth–cloth and body-feature start separation (`None` preserves legacy relaxation).
    pub start_self_repulsion: Option<SelfRepulsionOptions>,
}

impl Default for DrapeOptions {
    /// 12 mm mesh, 12 sewing and 36 draping steps of 1/24 s, Newton to 1e-3 m/s, compression as stiff
    /// as tension outside gather zones (the reference's `tension = compression = E_t·L`, spec 04 §5.3),
    /// three friction refreshes while sewing and one under gravity, a drafted casing's elastic at
    /// [`crate::scene::ELASTIC_EA`], ĝ 1 mm, the cloth's mid-surface 1.5 mm off the body, μ 0.4.
    fn default() -> Self {
        Self {
            spacing: 12.0,
            sew_steps: 12,
            drape_steps: 36,
            max_settle_steps: 0,
            elastic_hold_steps: 0,
            static_equilibrium: None,
            step: 1.0 / 24.0,
            time_subdivision: None,
            tol: 1e-3,
            compression: 1.0,
            lag_rounds: [3, 1],
            elastic_ea: crate::scene::ELASTIC_EA,
            contact: ContactParams {
                dhat: 1e-3,
                thickness: 1.5e-3,
                kappa: 100.0,
                mu: 0.4,
                eps_v: 1e-3,
            },
            self_contact: None,
            pcg_iters: 20,
            newton_strategy: NewtonStrategy::ProjectedNewton,
            integrator: TimeIntegrator::ImplicitEuler,
            bending_model: BendingModel::Quadratic,
            compression_model: CompressionModel::AxisAligned,
            damping: DampingOptions::default(),
            adaptive_barrier: None,
            stick_slip: None,
            self_stick_slip: None,
            mesh: MeshOptions::default(),
            construction: ConstructionMechanicsOptions::default(),
            start_self_repulsion: None,
        }
    }
}

impl DrapeOptions {
    /// Validates the run before meshing or body fitting.
    ///
    /// # Errors
    /// Non-finite or out-of-range numerical settings, insufficient frames for metrics, or a frame
    /// count that overflows the animation budget.
    pub fn validate(&self) -> Result<(), Refusal> {
        self.newton_strategy.validate()?;
        self.integrator.validate()?;
        self.damping.validate()?;
        if let Some(policy) = &self.static_equilibrium {
            policy.validate()?;
        }
        if let Some(policy) = self.time_subdivision {
            policy.validate(self.step)?;
        }
        self.contact.validate()?;
        if let Some(p) = self.self_contact {
            p.validate()?;
        }
        if let Some(policy) = self.adaptive_barrier {
            policy.validate()?;
        }
        if let Some(friction) = self.stick_slip {
            friction.validate()?;
        }
        if let Some(friction) = self.self_stick_slip {
            friction.validate()?;
            if self.self_contact.is_none() {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "cloth-cloth stick-slip friction requires self-contact to be enabled",
                ));
            }
        }
        self.mesh.validate(self.spacing)?;
        self.construction.validate()?;
        if let Some(repulsion) = self.start_self_repulsion {
            repulsion.validate()?;
        }
        for (name, value) in [
            ("mesh spacing", self.spacing),
            ("time step", self.step),
            ("Newton tolerance", self.tol),
            ("compression factor", self.compression),
        ] {
            Refusal::finite(name, value)?;
            if value <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("{name} must be positive"),
                ));
            }
        }
        if self.compression > 1.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "compression factor must be at most 1",
            ));
        }
        Refusal::finite("elastic stiffness", self.elastic_ea)?;
        if self.elastic_hold_steps > self.drape_steps {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "elastic hold window cannot exceed the declared gravity steps",
            ));
        }
        if self.elastic_ea < 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "the elastic stiffness EA must be 0 (no elastic) or positive",
            ));
        }
        if self.max_settle_steps > 256 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "the optional settling tail is limited to 256 additional gravity steps",
            ));
        }
        let total = self
            .sew_steps
            .checked_add(self.drape_steps)
            .filter(|n| {
                n.checked_add(self.max_settle_steps)
                    .and_then(|n| n.checked_add(FOLD_FRAMES + ZIP_FRAMES))
                    .is_some()
            })
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "animation frame count overflows",
                )
            })?;
        if let Some(policy) = self.time_subdivision
            && policy.max_trials < total
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "time subdivision needs at least one trial per requested sewing and gravity interval",
            ));
        }
        if self.max_settle_steps > 0
            && !((self.drape_steps + self.max_settle_steps) as f64 * self.step).is_finite()
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "the declared gravity and settling duration must be finite",
            ));
        }
        if total < MIN_FRAMES {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "{} sewing and {} draping steps: the fit metrics need at least {MIN_FRAMES} in all \
                     (settledness compares the means of the last frames)",
                    self.sew_steps, self.drape_steps
                ),
            ).repair(format!("ask for at least {MIN_FRAMES} steps (--sew, --drape)")));
        }
        Ok(())
    }
}

/// A finished run.
pub struct DrapeRun {
    /// The scene at the end.
    pub scene: Scene,
    /// Every frame of the fold-up.
    pub fold: FoldUp,
    /// The report (metrics, flags and run fields).
    pub report: Value,
    /// The sewing phase.
    pub sew: PhaseReport,
    /// The draping phase.
    pub drape: PhaseReport,
    /// Default fabric; heterogeneous per-element laws live in `scene.solver.model` and the report.
    pub fabric: Fabric,
}

impl DrapeRun {
    /// The gating flags.
    #[must_use]
    pub fn gating(&self) -> Vec<String> {
        strings(&self.report["gating"])
    }

    /// The flags that are not gating.
    #[must_use]
    pub fn advisory(&self) -> Vec<String> {
        let gating = self.gating();
        strings(&self.report["flags"])
            .into_iter()
            .filter(|f| !gating.contains(f))
            .collect()
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// Keep metric levels on the same physical frame as placement and flat fit. Absent side retains
// the reference's right-frame convention, including generated mirrored instances.
fn metric_level_y(
    pattern: &Pattern,
    frames: &dyn PartFrames,
    inst: &str,
    landmark: &str,
) -> Option<f64> {
    let i = pattern.instance(inst)?;
    let piece = &pattern.raw()["pieces"][i.piece_index];
    let pl = &piece["placement"];
    let part = pl["part"].as_str()?;
    let side = physical_side(piece).ok()?.unwrap_or(Side::R);
    let key = if part == "torso" {
        part.to_string()
    } else {
        format!("{part}_{}", side.as_str())
    };
    let anchor = pl["y0_landmark"].as_str().unwrap_or("waist");
    Some(
        frames.landmark_h(&key, landmark)? - frames.landmark_h(&key, anchor)?
            + pl["y0"].as_f64().unwrap_or(0.0),
    )
}

fn add_simulation_scope(report: &mut Value, scope: Value) {
    if scope["basis"] == "declared_subset" {
        let excluded = scope["excluded_instances"].as_array().map_or(0, Vec::len);
        let seams = scope["excluded_seams"].as_array().map_or(0, Vec::len);
        let message = format!(
            "Declared simulation subset: {excluded} instance(s) and {seams} seam(s) excluded; \
             see simulation_scope. Results describe only the simulated subset."
        );
        report["flags"]
            .as_array_mut()
            .expect("metrics flags are an array")
            .push(json!(message));
    }
    report["simulation_scope"] = scope;
}

/// The cloth state a refusal is about, enough to draw it: the last accepted state when sewing, draping or
/// settling refuses, the untangled start rejected by the self-contact audit, or the recorded
/// frame that crosses the displayed wearer. It is not a drape, a fit or a result.
#[derive(Clone, Debug)]
pub struct RefusedRun {
    /// The garment mesh (faces, instances, flat coordinates).
    pub mesh: GarmentMesh,
    /// The positions of that state, one per mesh vertex, metres.
    pub positions: Vec<Vec3>,
    /// The phase the state belongs to: `start`, `sew`, `drape` or `settle`.
    pub phase: &'static str,
    /// Nominal steps of that phase accepted when the state was reached.
    pub accepted_steps: usize,
    /// Present when the retained geometry lies inside the next nominal interval.
    pub substep: Option<RefusedSubstep>,
    /// The first rejected pair at start admission, as source garment face indices before welding.
    pub start_crossing_faces: Option<[usize; 2]>,
    /// Set when the run began from an earlier relaxation checkpoint because its fully relaxed start
    /// failed cloth contact or displayed-surface admission: the relaxation iterations of the checkpoint
    /// used, then of the start that was not admitted. The checkpoint is the more strained.
    pub start_fallback: Option<(usize, usize)>,
    /// Set when the start's relaxation ran its full [`crate::start::RELAX_MAX_ITERS`] iterations and the
    /// start's plain edges are still above [`crate::start::RELAX_TARGET_P95`]: their p95 absolute strain,
    /// the value the run's `start:` line prints. `None` for a start that reached the target, for an earlier
    /// checkpoint (`start_fallback`, above the target for its own reason) and for a start that reached the
    /// target before the cap and was then stretched past it by the leg-separation repair. That repair
    /// replaces the last recorded value, so after a relaxation that ran to the cap this is the repaired
    /// start's strain, not the relaxation's own last one.
    pub start_unrelaxed_p95: Option<f64>,
}

fn start_unrelaxed_p95(scene: &Scene) -> Option<f64> {
    scene.start.block_p95.last().copied().filter(|&p95| {
        scene.start_backtracked_from.is_none()
            && scene.start.relax_iterations >= crate::start::RELAX_MAX_ITERS
            && p95 > crate::start::RELAX_TARGET_P95
    })
}

fn refused_run(
    scene: &Scene,
    positions: Vec<Vec3>,
    phase: &'static str,
    accepted_steps: usize,
    substep: Option<RefusedSubstep>,
) -> RefusedRun {
    RefusedRun {
        mesh: scene.mesh.clone(),
        positions,
        phase,
        accepted_steps,
        substep,
        start_crossing_faces: None,
        start_fallback: scene
            .start_backtracked_from
            .map(|rejected| (scene.start.relax_iterations, rejected)),
        start_unrelaxed_p95: start_unrelaxed_p95(scene),
    }
}

/// Drapes `pattern` on `body`, calling `progress` with a line per stage.
///
/// # Errors
/// [`RefusalCode::OutOfRange`] for fewer than [`MIN_FRAMES`] steps in all; refusals from meshing,
/// simulation scope, placement, the start state, the cloth model, contact or the solver.
pub fn drape(
    pattern: &Pattern,
    fit_spec: &Value,
    body: &DrapeBody,
    opts: &DrapeOptions,
    progress: impl FnMut(&str),
) -> Result<DrapeRun, Refusal> {
    drape_observed(pattern, fit_spec, body, opts, progress, &mut None)
}

fn effective_arm_deg(body: &DrapeBody) -> Result<f64, Refusal> {
    if let Some(arms) = body.arm_pose {
        arms.validate()?;
        if arms.left_abduction_deg == arms.right_abduction_deg {
            Ok(arms.left_abduction_deg + 0.0)
        } else if body.arm_deg.is_finite() && (0.0..=180.0).contains(&body.arm_deg) {
            Ok(body.arm_deg + 0.0)
        } else {
            Ok(0.5 * (arms.left_abduction_deg + arms.right_abduction_deg) + 0.0)
        }
    } else {
        Refusal::finite("arm angle", body.arm_deg)?;
        Ok(body.arm_deg + 0.0)
    }
}

/// [`drape`], and when it refuses once the start state has been admitted, `refused` receives the mesh and
/// the positions the refusal is about so that the caller can show them: the last accepted state when
/// sewing, draping or settling refuses (the admitted start if the first sewing step does), or the frame
/// that crosses a surface in the replay. It also retains the exact untangled start for a
/// self-contact refusal at admission. Other early refusals leave `refused` empty.
///
/// # Errors
/// As [`drape`]. The refusal is the same with or without an observer.
pub fn drape_observed(
    pattern: &Pattern,
    fit_spec: &Value,
    body: &DrapeBody,
    opts: &DrapeOptions,
    mut progress: impl FnMut(&str),
    refused: &mut Option<RefusedRun>,
) -> Result<DrapeRun, Refusal> {
    *refused = None;
    let arm_deg = effective_arm_deg(body)?;
    let scope = simulation_scope(pattern)?;
    let material = crate::material::resolve(pattern, opts)?;
    let opts = &material.options;
    let fabric = material.fabric;
    // an elastic casing is simulated at its cut length and drawn up by its elastic
    let elastics = elastic_casings(pattern, opts.elastic_ea)?;
    let simulated;
    let pattern = if elastics.is_empty() {
        pattern
    } else {
        simulated = casings_at_cut_length(pattern, &elastics);
        &simulated
    };
    let mesh = if opts.mesh.is_default() {
        build_garment(pattern, opts.spacing)?
    } else {
        build_garment_with_options(pattern, opts.spacing, opts.mesh)?
    };
    if mesh.verts2d.is_empty() {
        // a cone (fez) or a pattern whose pieces have no placement is judged flat, as `fab round` says
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "no piece has a fold-up placement: nothing to drape (the pattern is judged flat)",
        ));
    }
    let mut display_contact = opts
        .self_contact
        .map(|_| crate::animation::ReplayContact::new(body.display.0, body.display.1))
        .transpose()?;
    let scene_options = SceneBuildOptions {
        construction: opts.construction,
        start_self_repulsion: opts.start_self_repulsion,
    };
    let mut scene = crate::scene::build_scene_resolved_with_options(
        pattern,
        mesh,
        body.frames,
        body.collision,
        &material,
        &elastics,
        display_contact.as_mut(),
        scene_options,
    )?;
    if let Some(p) = opts.self_contact {
        let mut crossing = None;
        if let Err(mut refusal) = scene.solver.enable_self_contact_observed(p, &mut crossing) {
            if refusal.code == RefusalCode::IntersectingStart {
                let crossing_faces = crossing.and_then(|[a, b]| {
                    // The solver retains the source face order after removing collapsed welds.
                    let source_faces: Vec<_> = scene
                        .mesh
                        .faces
                        .iter()
                        .enumerate()
                        .filter_map(|(face, vertices)| {
                            let [i, j, k] = vertices.map(|v| scene.weld[v]);
                            (i != j && j != k && k != i).then_some(face)
                        })
                        .collect();
                    Some([*source_faces.get(a)?, *source_faces.get(b)?])
                });
                if let Some(faces) = crossing_faces {
                    let mut owners = Vec::new();
                    for face in faces {
                        if let Some(name) = scene
                            .mesh
                            .faces
                            .get(face)
                            .and_then(|tri| scene.mesh.owner.get(tri[0]))
                            .and_then(|&idx| scene.mesh.instances.get(idx))
                            && !owners.contains(&name.as_str())
                        {
                            owners.push(name.as_str());
                        }
                    }
                    if !owners.is_empty() {
                        refusal.repairs.push(format!(
                            "The reported cloth pair belongs to piece instance(s): {}.",
                            owners.join(" and ")
                        ));
                    }
                }
                let start_fallback = scene
                    .start_backtracked_from
                    .map(|rejected| (scene.start.relax_iterations, rejected));
                let start_unrelaxed_p95 = start_unrelaxed_p95(&scene);
                *refused = Some(RefusedRun {
                    positions: scene.positions(),
                    mesh: scene.mesh,
                    phase: "start",
                    accepted_steps: 0,
                    substep: None,
                    start_crossing_faces: crossing_faces,
                    start_fallback,
                    start_unrelaxed_p95,
                });
            }
            return Err(refusal);
        }
        progress("self-contact: start audit passed; cloth–cloth contact enabled");
    }
    if let Some(contact) = scene.solver.contact.as_mut() {
        if let Some(policy) = opts.adaptive_barrier {
            let n_verts = scene.solver.model.mass.len();
            let mean_mass = if n_verts > 0 {
                scene.solver.model.mass.iter().sum::<f64>() / n_verts as f64
            } else {
                0.0
            };
            let (stiff_area, total_area) = scene.solver.model.membranes.iter().enumerate().fold(
                (0.0, 0.0),
                |(sa, a), (i, e)| {
                    let q = scene.solver.model.material(i).q;
                    let et = 0.5 * (q[0][0] + q[1][1]);
                    (sa + et * e.area, a + e.area)
                },
            );
            let membrane_stiffness = if total_area > 0.0 {
                stiff_area / total_area
            } else {
                fabric.tensile_n_m
            };
            let ref_gravity = if opts.drape_steps > 0 || opts.max_settle_steps > 0 {
                GRAVITY
            } else {
                0.0
            };
            contact.set_adaptive_barrier(policy, mean_mass, membrane_stiffness, ref_gravity)?;
        }
        if let Some(friction) = opts.stick_slip {
            contact.set_stick_slip(friction)?;
        }
        if let Some(friction) = opts.self_stick_slip {
            contact.set_self_stick_slip(friction)?;
        }
    }
    scene.solver.tol = opts.tol;
    scene.solver.pcg_iters = opts.pcg_iters;
    scene.solver.newton_strategy = opts.newton_strategy;
    scene.solver.integrator = opts.integrator;
    scene.solver.model.bending_model = opts.bending_model;
    scene.solver.damping = opts.damping;
    scene.solver.check_constitutive_models()?;
    scene.solver.check_integrator_history()?;
    if opts.max_settle_steps > 0 {
        scene.solver.check_settling_history()?;
    }
    progress(&format!(
        "start: {} vertices ({} after welding), relaxed {} iterations to p95 strain {:.3}{}",
        scene.mesh.verts2d.len(),
        scene.solver.x.len(),
        scene.start.relax_iterations,
        scene.start.block_p95.last().copied().unwrap_or(f64::NAN),
        // Said here because a run that refuses never reaches the report that records it.
        scene
            .start_backtracked_from
            .map_or(String::new(), |rejected| format!(
                " (an earlier checkpoint: the start relaxed for {rejected} iterations failed cloth \
             contact or displayed-surface admission)"
            ))
    ));
    let initial_positions = scene.positions();
    let first_sewn = FOLD_FRAMES + ZIP_FRAMES;
    if let Some(display) = &mut display_contact {
        // The collider may omit fingers or feet retained by the displayed wearer. Check the exact
        // welded start before spending any work on a trajectory that must already be refused.
        if let Err(refusal) = display.check(&initial_positions, &scene.mesh.faces, first_sewn - 1) {
            *refused = Some(refused_run(&scene, initial_positions, "sew", 0, None));
            return Err(refusal);
        }
    }
    let board = board_layout(&scene.mesh);
    let mut frames = fold_frames(&scene.mesh, &scene.placements, &board, FOLD_FRAMES);
    if opts.self_contact.is_some() {
        for frame in &mut frames {
            crate::animation::separate_panels(frame, &scene.mesh.inst_range);
        }
        let held = frames.last().expect("fold has a final frame").clone();
        frames.extend(std::iter::repeat_n(held, ZIP_FRAMES - 1));
        frames.push(initial_positions.clone());
    } else {
        frames.extend(zip_frames(&scene.start.wrapped, &initial_positions));
    }
    let initial_contact = scene
        .solver
        .contact
        .as_ref()
        .map(|c| c.accepted_sample(&scene.solver.x));
    let mut phases = Vec::new();
    let mut subdivision_trials = 0;
    let mut advance = |scene: &mut Scene, name, steps, gravity, completed| {
        let first_frame = frames.len();
        let mut audit = display_contact
            .as_mut()
            .map(|display| PhaseAudit::new(display, first_frame, scene.positions()));
        let report = run_phase_audited(
            scene,
            steps,
            opts.step,
            gravity,
            opts.time_subdivision,
            &mut subdivision_trials,
            audit.as_mut(),
            |_, x| frames.push(x.to_vec()),
        );
        if report.is_err() {
            let endpoint = audit
                .and_then(|a| a.rejected)
                .unwrap_or_else(|| RejectedEndpoint {
                    positions: frames.last().expect("admitted start").clone(),
                    steps: frames.len() - first_frame,
                    substep: None,
                });
            *refused = Some(refused_run(
                scene,
                endpoint.positions,
                name,
                completed + endpoint.steps,
                endpoint.substep,
            ));
        }
        report
    };
    for (name, steps, g, lag) in [
        ("sew", opts.sew_steps, 0.0, opts.lag_rounds[0]),
        ("drape", opts.drape_steps, GRAVITY, opts.lag_rounds[1]),
    ] {
        scene.solver.friction_rounds = lag;
        let rep = advance(&mut scene, name, steps, g, 0)?;
        progress(&format!(
            "{name}: {} steps, {} Newton iterations ({} unconverged), max speed {:.4} m/s",
            rep.steps, rep.iterations, rep.unconverged, rep.max_speed
        ));
        phases.push(rep);
    }
    let (sew, mut drape) = (phases[0].clone(), phases[1].clone());
    if opts.max_settle_steps > 0 {
        for _ in 0..opts.max_settle_steps {
            if consecutive_quiet_steps(&drape) >= SETTLE_QUIET_STEPS
                && scene
                    .solver
                    .contact
                    .as_ref()
                    .is_none_or(|c| c.params.thickness >= scene.target_thickness)
            {
                break;
            }
            let step = advance(
                &mut scene,
                "settle",
                1,
                GRAVITY,
                drape.steps - opts.drape_steps,
            )?;
            drape.append(step);
        }
        progress(&format!(
            "settling: {} additional gravity steps, {} consecutive quiet endpoints, max speed {:.4} m/s",
            drape.steps - opts.drape_steps,
            consecutive_quiet_steps(&drape),
            drape.max_speed
        ));
    }
    let fold = FoldUp {
        frames,
        fold: FOLD_FRAMES,
        zip: ZIP_FRAMES,
        sew: opts.sew_steps,
        drape: drape.steps,
    };
    let executed_options = DrapeOptions {
        drape_steps: drape.steps,
        ..*opts
    };

    // Metrics against the collision body, levels from the body's frames.
    let true_body = NearestTree::new(body.collision.0.to_vec(), body.collision.1.to_vec())?;
    let level_y = |inst: &str, landmark: &str| metric_level_y(pattern, body.frames, inst, landmark);
    let seq = &fold.frames[FOLD_FRAMES + ZIP_FRAMES..];
    let (sewing_frames, gravity_frames) = seq.split_at(fold.sew);
    let after_sew = sewing_frames.last().unwrap_or(&initial_positions);
    let input = MetricsInput {
        pattern,
        seq,
        start: &initial_positions,
        after_sew,
        wrapped: &scene.start.wrapped,
        body: &true_body,
        level_y: &level_y,
        fit_spec,
        spacing: opts.spacing,
        arm_deg,
        arm_pose: body.arm_pose,
    };
    let mut report = metrics(&scene.mesh, &input);
    let casing_mu: Vec<_> = elastics
        .iter()
        .map(|e| {
            (|| {
                let piece = pattern.instance(&e.instance)?.piece_index;
                let field = material.contact_materials.as_ref()?;
                Some(field.surfaces[field.piece_ids[piece]?].friction_body)
            })()
            .unwrap_or_else(|| opts.stick_slip.map_or(opts.contact.mu, |s| s.mu_s))
        })
        .collect();
    add_run_fields(
        &mut report,
        &scene,
        &fabric,
        &executed_options,
        &input,
        &casing_mu,
    );
    add_numerical_report(
        &mut report,
        Some(&scene),
        &sew,
        &drape,
        opts,
        scene.solver.contact.as_ref().map(|c| c.params.thickness),
        scene.target_thickness,
    );
    add_simulation_scope(&mut report, scope);
    report["material"] = material.report;
    add_casing_windows(
        &mut report,
        &scene,
        gravity_frames,
        after_sew,
        &drape,
        &executed_options,
    );
    if let Some(fabrics) = &material.piece_fabrics {
        report["fabric"] = json!({"basis":"per_piece", "see":"material.records"});
        let mut area = vec![0.0; scene.mesh.instances.len()];
        for (e, &owner) in scene
            .solver
            .model
            .membranes
            .iter()
            .zip(&scene.triangle_owner)
        {
            area[owner] += e.area;
        }
        let instances: Vec<Value> = scene
            .mesh
            .instances
            .iter()
            .enumerate()
            .map(|(i, id)| {
                // The mesh and the materials were resolved against this same source-piece numbering.
                let p = pattern
                    .instance(id)
                    .expect("scene instance exists")
                    .piece_index;
                json!({"instance":id, "piece_index":p,
                "record_sha256":report["material"]["pieces"][p]["record_sha256"],
                "rest_area_m2":area[i], "mass_kg":area[i]*fabrics[p].expect("retained instance has an active sheet").areal_kg_m2})
            })
            .collect();
        report["material"]["instances"] = json!(instances);
        report["material"]["triangle_instance"] = json!(scene.triangle_owner);
        if material.contact_materials.is_some() {
            report["cloth_offset_basis"] = json!(
                "maximum broad-phase envelope; see material.records for physical sheet thicknesses"
            );
            if !report["elastic"].is_null() {
                report["elastic"]["friction_mu"] = json!(casing_mu[0]);
                report["elastic"]["grip_basis"] = json!(
                    "ideal ring estimate using casing/body friction; not integrated heterogeneous contact traction"
                );
            }
        }
    }
    // Bind reactions to the complete material receipt, including retained per-piece ownership.
    add_contact_report(&mut report, &scene, &sew, &drape, initial_contact.as_ref());
    if let Some(attached) = scene.solver.model.attached_mass_report()? {
        // Preserve the existing simulated-cloth metric. Actual solver/elastic gravity and inertia
        // use the augmented mass vector; thermal surfaces retain only characterized sheet mass.
        report["mass_kg"] = json!(attached.cloth_mass_kg);
        report["attached_mass"] = serde_json::to_value(attached)
            .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    } else {
        report["mass_kg"] = json!(scene.solver.model.mass.iter().sum::<f64>());
    }
    let model = &scene.solver.model;
    if !model.panel_bending_patches().is_empty() {
        let models: std::collections::BTreeSet<_> = model
            .panel_bending_patches()
            .iter()
            .map(crate::bending::BendingPatch::model)
            .collect();
        report["panel_bending"] = json!({
            "model":if models.len() == 1 { *models.first().expect("nonempty patches") } else { "per_patch" },
            "models":models,
            "patch_count":model.panel_bending_patches().len(),
            "material_triangles":model.panel_bending_triangles(),
            "panel_energy_j":scene.solver.panel_bending_energy()?,
            "active_scalar_hinges":model.hinges.iter().enumerate().filter(|(i,_)| model.hinge_rigidity(*i)>0.0).count(),
            "scope":"flat-reference source-panel curvature with declared per-record kinematics; scalar seam approximation retained; not conforming-shell or empirical seam validation"
        });
    }
    if let Some(history) = scene.solver.relaxation_report()? {
        report["membrane_history"] = history;
        report["material_settledness_note"] = json!(
            "Geometric settledness does not establish relaxed stress; history time includes sewing and draping steps."
        );
    }
    if let Some(history) = scene.solver.bending_plasticity_report()? {
        report["bending_history"] = history;
        report["panel_bending"]["energy_basis"] =
            json!("accepted elastic and hardening storage; excludes dissipated plastic work");
        report["material_settledness_note"] = json!(
            "Geometric settledness does not establish stress equilibrium or a stress-free shape; accepted history includes sewing and draping steps."
        );
    }
    if let Some(history) = scene.solver.membrane_plasticity_report()? {
        report["membrane_plasticity_history"] = history;
        report["material_settledness_note"] = json!(
            "Geometric settledness does not establish a stress-free material state; permanent membrane history includes accepted sewing and draping steps."
        );
    }
    // Missing thermal observations must not invalidate a supported mechanical drape. Legacy reports
    // stay byte-compatible; explicit materials carry a ready-to-evaluate exchange or a refusal.
    if report["material"]["basis"] != "legacy_uncertain" {
        report["thermal_surface"] = crate::thermal::surface::for_drape(&scene, &report["material"]);
        report["steady_thermal_surface"] =
            crate::thermal::steady::surface::for_drape(&scene, &report["material"]);
        report["vapor_surface"] = crate::thermal::vapor::for_drape(&scene, &report["material"]);
    }
    if let Some(policy) = &opts.static_equilibrium {
        let equilibrium = static_equilibrium(&scene, policy, &report["material"]);
        progress(&format!(
            "static equilibrium: {} (independent frictionless comparison; dynamic trajectory retained)",
            equilibrium["status"].as_str().unwrap_or("refused")
        ));
        report["static_equilibrium"] = equilibrium;
    }
    Ok(DrapeRun {
        scene,
        fold,
        report,
        sew,
        drape,
        fabric,
    })
}

// Keep this optional comparison downstream of all dynamic metrics and source-geometry receipts.
// The existing static solver omits friction, uses no duration and refuses material history laws.
fn static_equilibrium(scene: &Scene, policy: &StaticEquilibriumOptions, material: &Value) -> Value {
    let mut report = json!({
        "schema":"ftx-static-equilibrium/1", "request":policy,
        "basis":"independent_frictionless_equilibrium_from_dynamic_endpoint",
        "gravity_m_s2":scene.solver.gravity,
        "friction_included":false, "advances_simulated_time":false,
        "changes_dynamic_trajectory":false, "physical_validation_claim":false,
        "body_contact_simulated":scene.solver.contact.as_ref().is_some_and(|c| !c.body.triangles.is_empty()),
        "self_contact_simulated":scene.solver.contact.as_ref().is_some_and(|c| c.self_contact_params().is_some()),
        "no_claim":"A bounded conservative solve from one dynamic endpoint. Normal contact and elastic storage are retained; Coulomb friction, material history evolution and elapsed time are excluded. Dynamic geometry, velocities, frames, metrics, flags and subsequent motion remain authoritative. Convergence does not prove a unique equilibrium, friction-supported hold, physical garment fit or empirical validation."
    });
    let result = (|| -> Result<Value, Refusal> {
        policy.validate()?;
        if scene
            .solver
            .contact
            .as_ref()
            .is_some_and(|c| c.params.thickness < scene.target_thickness)
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "static equilibrium requires the completed physical contact-thickness ramp",
            )
            .repair("run more explicit draping steps before requesting the static comparison"));
        }
        let mut trial = scene.solver.clone();
        trial.tol = policy.correction_tolerance_m;
        trial.max_newton = policy.max_newton_iterations;
        let solve = trial.static_solve().map_err(|mut refusal| {
            if refusal.code == RefusalCode::NotConverged {
                refusal.repairs = vec![
                    "increase the explicit static Newton cap or review its distance tolerance".into(),
                    "check whether this garment has a supported frictionless equilibrium and remains in its material domain".into(),
                ];
            }
            refusal
        })?;
        let mut max_displacement_m = 0.0_f64;
        let mut sum_squared_displacement_m2 = 0.0;
        for (start, finish) in scene.solver.x.iter().zip(&trial.x) {
            let distance = (*finish - *start).norm();
            max_displacement_m = max_displacement_m.max(distance);
            sum_squared_displacement_m2 += distance * distance;
        }
        Refusal::finite(
            "static-equilibrium displacement",
            sum_squared_displacement_m2,
        )?;
        let digest = |value: &Value| {
            serde_json::to_vec(value)
                .map(|bytes| sha256_hex(&bytes))
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
        };
        let body_geometry_sha256 = scene
            .solver
            .contact
            .as_ref()
            .map(|c| digest(&json!([c.body.vertices, c.body.triangles])))
            .transpose()?;
        Ok(json!({
            "solve":solve, "vertex_basis":"welded solver vertices in the unchanged body frame",
            "initial_positions_m":scene.solver.x, "positions_m":trial.x,
            "triangles":trial.model.membranes.iter().map(|t| t.v).collect::<Vec<_>>(),
            "mesh_vertex_to_solver":scene.weld,
            "instances":scene.mesh.instances, "triangle_instance":scene.triangle_owner,
            "material_sha256":digest(material)?, "body_geometry_sha256":body_geometry_sha256,
            "max_displacement_m":max_displacement_m,
            "rms_displacement_m":(sum_squared_displacement_m2 / trial.x.len() as f64).sqrt(),
            "contact_thickness_m":trial.contact.as_ref().map(|c| c.params.thickness),
        }))
    })();
    match result {
        Ok(result) => {
            report["status"] = json!("available");
            report["result"] = result;
        }
        Err(refusal) => {
            report["status"] = json!("refused");
            report["refusal"] = json!(refusal);
        }
    }
    report
}

/// Preserve reference metrics; native acceptance additionally requires a quiet instantaneous tail.
fn consecutive_quiet_steps(phase: &PhaseReport) -> usize {
    phase
        .samples
        .iter()
        .rev()
        .take_while(|s| s.solve.converged && s.max_speed_m_s <= SETTLE_SPEED_M_S)
        .count()
}

fn add_numerical_report(
    report: &mut Value,
    scene: Option<&Scene>,
    sew: &PhaseReport,
    drape: &PhaseReport,
    opts: &DrapeOptions,
    body_thickness: Option<f64>,
    target_thickness: f64,
) {
    let quiet = consecutive_quiet_steps(drape);
    let gravity_simulated = drape.steps > 0;
    let settled = gravity_simulated.then_some(quiet >= SETTLE_QUIET_STEPS);
    report["numerical"] = json!({
        "step_acceptance":"every Newton solve converged; stalled steps are refused without advancing the accepted state",
        "newton_correction_speed_tolerance_m_s":opts.tol,
        "time_step_s":opts.step,
        "velocity_tolerance_m_s":SETTLE_SPEED_M_S,
        "required_quiet_steps":SETTLE_QUIET_STEPS,
        "consecutive_quiet_steps":quiet,
        "gravity_simulated":gravity_simulated,
        "settled":settled,
        "sew":sew,
        "drape":drape,
        "basis":"Newton correction/h and instantaneous speed; reference settled remains the legacy window-drift metric, not static stress equilibrium or real fit"
    });
    if !opts.newton_strategy.is_default() {
        report["numerical"]["newton_strategy"] = json!(opts.newton_strategy);
    }
    if !opts.integrator.is_default() {
        report["numerical"]["integrator"] = json!(opts.integrator);
    }
    if !opts.bending_model.is_default() {
        report["numerical"]["bending_model"] = json!(opts.bending_model);
    }
    if !opts.compression_model.is_default() {
        report["numerical"]["compression_model"] = json!(opts.compression_model);
    }
    if !opts.damping.is_default() {
        report["numerical"]["damping"] = json!(opts.damping);
    }
    if let Some(policy) = opts.adaptive_barrier {
        report["numerical"]["adaptive_barrier"] = json!(policy);
    }
    if let Some(friction) = opts.stick_slip {
        report["numerical"]["stick_slip"] = json!(friction);
    }
    if let Some(friction) = opts.self_stick_slip {
        report["numerical"]["self_stick_slip"] = json!(friction);
    }
    if !opts.mesh.is_default() {
        let mut entry = json!({
            "options": opts.mesh,
        });
        if let Some(scene) = scene {
            entry["quality"] = json!(mesh_quality(&scene.mesh));
        }
        report["numerical"]["mesh"] = entry;
    }
    if !opts.construction.is_default() {
        let mut entry = json!({
            "options": opts.construction,
            "basis": "uncalibrated structural multipliers on flat hem and seam-allowance bands; not measured laminate or stitch-class properties and not a fit claim",
        });
        if let Some(field) = scene.and_then(|s| s.construction_field.as_ref()) {
            entry["hem_edges"] = json!(field.hem_edges);
            entry["hem_triangles"] = json!(field.hem_triangles);
            entry["seam_band_triangles"] = json!(field.seam_band_triangles);
            entry["seam_hinges"] = json!(field.seam_hinges);
        }
        report["numerical"]["construction"] = entry;
    }
    if let Some(repulsion) = opts.start_self_repulsion {
        report["numerical"]["start_self_repulsion"] = json!(repulsion);
    }
    if let Some(count) = scene.map(|s| s.authored_crease_hinges).filter(|&n| n > 0) {
        report["numerical"]["authored_crease_hinges"] = json!(count);
    }
    if let Some(policy) = opts.time_subdivision {
        let intervals: Vec<_> = sew
            .samples
            .iter()
            .chain(&drape.samples)
            .filter_map(|sample| sample.time_subdivision.as_ref())
            .collect();
        report["numerical"]["time_subdivision"] = json!({
            "policy":policy,
            "trials":intervals.iter().map(|s| s.trials).sum::<usize>(),
            "accepted_substeps":intervals.iter().map(|s| s.accepted.len()).sum::<usize>(),
            "subdivided_nominal_intervals":intervals.iter().filter(|s| s.accepted.len() > 1).count(),
            "nominal_intervals":sew.steps+drape.steps,
            "basis":"Convergence-driven bisection only; every complete nominal interval advances its original duration. One attempted-step allowance covers sewing, gravity and settling. Nominal output frames remain equally spaced; each sample retains its actual accepted partition. Quietness includes every accepted substep speed. Solve energies refer to individual physical steps, never their sum. No time-error, continuous-contact-minimum or physical-fit certificate."
        });
    }
    if opts.max_settle_steps > 0 {
        let steps = drape.steps - opts.drape_steps;
        let complete = settled == Some(true)
            && body_thickness.is_none_or(|achieved| achieved >= target_thickness);
        report["numerical"]["settling"] = json!({
            "max_steps":opts.max_settle_steps,"steps":steps,"duration_s":steps as f64 * opts.step,
            "gravity_steps":drape.steps,"gravity_duration_s":drape.steps as f64 * opts.step,
            "stop_reason":if complete { "quiet_window" } else { "step_limit" },
            "cap_exhausted":!complete && steps == opts.max_settle_steps,
        });
    }
    if settled == Some(false) {
        let flag = format!(
            "not numerically settled (maximum speed {} m/s; need {SETTLE_QUIET_STEPS} consecutive steps <= {SETTLE_SPEED_M_S} m/s): rerun with more drape steps",
            drape.max_speed
        );
        for key in ["flags", "gating"] {
            if let Some(flags) = report[key].as_array_mut() {
                flags.push(json!(flag));
            }
        }
    }
    if body_thickness.is_some_and(|achieved| achieved < target_thickness) {
        for key in ["flags", "gating"] {
            if let Some(flags) = report[key].as_array_mut() {
                flags.push(json!("body thickness ramp did not reach its target"));
            }
        }
    }
}

/// The run fields of spec 04 §6.11 that a run knows (the CLI adds file hashes and timing).
fn add_contact_report(
    report: &mut Value,
    scene: &Scene,
    sew: &PhaseReport,
    drape: &PhaseReport,
    initial: Option<&crate::contact::ContactSample>,
) {
    let mut samples: Vec<_> = initial.into_iter().collect();
    let mut accepted_samples = 0;
    let mut subdivided = false;
    for sample in sew.samples.iter().chain(&drape.samples) {
        if let Some(partition) = &sample.time_subdivision {
            subdivided = true;
            accepted_samples += partition.accepted.len();
            samples.extend(partition.accepted.iter().filter_map(|s| s.contact.as_ref()));
        } else {
            accepted_samples += 1;
            samples.extend(sample.contact.iter());
        }
    }
    let minimum = |own: bool| {
        samples
            .iter()
            .filter_map(|s| {
                if own {
                    s.self_contact.as_ref()
                } else {
                    s.body.as_ref()
                }
            })
            .filter_map(|d| d.minimum_active_gap_m)
            .reduce(f64::min)
    };
    let reactions = scene
        .solver
        .contact
        .as_ref()
        .map(|c| match c.reaction_map(&scene.solver.x) {
            Ok(mut value) => {
                value["material_report_sha256"] = json!(ftx_core::receipt::sha256_hex(
                    &serde_json::to_vec(&report["material"]).expect("material report JSON")
                ));
                json!({"status":"available","result":value})
            }
            Err(e) => json!({"status":"refused","refusal":e}),
        });
    report["contact_diagnostics"] = json!({
        "basis":if subdivided {
            "initial and every accepted implicit substep endpoint; thickness-offset active-pair gaps, not an exact continuous-time minimum; no active pair means unavailable, not zero"
        } else {
            "initial and accepted phase endpoints only; thickness-offset active-pair gaps, not an exact continuous-time minimum; no active pair means unavailable, not zero"
        },
        "initial":initial,"accepted_samples":accepted_samples,
        "minimum_sampled_active_body_gap_m":minimum(false),
        "minimum_sampled_active_self_gap_m":minimum(true),
        "friction_basis":"counts describe the frozen lag state, not a measured sticking/sliding verdict",
        "final_reactions":reactions});
    if let Some(c) = scene.solver.contact.as_ref()
        && c.adaptive_barrier().is_some()
    {
        report["contact_diagnostics"]["final_body_kappa_n_m"] = json!(c.params.kappa);
        if let Some(sp) = c.self_contact_params() {
            report["contact_diagnostics"]["final_self_kappa_n_m"] = json!(sp.kappa);
        }
    }
}

fn band_window(initial: &[Vec3], frames: &[Vec<Vec3>], vertices: &[usize], step: f64) -> Value {
    if frames.is_empty() || vertices.is_empty() {
        return json!({"status":"not_simulated","basis":"no gravity observation window"});
    }
    let mean_z =
        |frame: &[Vec3]| vertices.iter().map(|&i| frame[i].z).sum::<f64>() / vertices.len() as f64;
    let start = mean_z(initial);
    let shifts: Vec<_> = frames.iter().map(|f| mean_z(f) - start).collect();
    json!({"status":"available","duration_s":frames.len() as f64*step,
        "samples":frames.len()+1,"signed_vertical_displacement_m":shifts.last(),
        "maximum_absolute_vertical_displacement_m":shifts.iter().map(|x| x.abs()).fold(0.,f64::max),
        "basis":"mean casing-vertex height relative to the static body over the declared gravity phase, including settling; sampled displacement, not contact-point slip, a stationary hold experiment, or physical fit"})
}

fn add_casing_windows(
    report: &mut Value,
    scene: &Scene,
    frames: &[Vec<Vec3>],
    initial: &[Vec3],
    phase: &PhaseReport,
    opts: &DrapeOptions,
) {
    let mut windows = Vec::new();
    for group in &scene.elastic_groups {
        // Fold frames use pattern mesh vertices, while elastic edges use welded solver vertices.
        let solver_vertices: std::collections::BTreeSet<_> = scene.elastic_rings
            [group.rings.clone()]
        .iter()
        .flat_map(|r| {
            scene.solver.model.elastics[r.clone()]
                .iter()
                .flat_map(|e| e.v)
        })
        .collect();
        let mut seen = std::collections::BTreeSet::new();
        let vertices: Vec<_> = scene
            .weld
            .iter()
            .enumerate()
            .filter_map(|(i, &v)| (solver_vertices.contains(&v) && seen.insert(v)).then_some(i))
            .collect();
        let hold = if opts.elastic_hold_steps == 0 {
            json!({"status":"not_requested"})
        } else {
            let offset = opts.drape_steps - opts.elastic_hold_steps;
            let anchor = if offset == 0 {
                initial
            } else {
                &frames[offset - 1]
            };
            let mut hold = band_window(anchor, &frames[offset..], &vertices, opts.step);
            hold["first_gravity_step"] = json!(offset + 1);
            hold["last_gravity_step"] = json!(opts.drape_steps);
            let samples = &phase.samples[offset..];
            hold["maximum_vertex_speed_m_s"] =
                json!(samples.iter().map(|s| s.max_speed_m_s).fold(0., f64::max));
            hold["quiet_at_1_mm_s"] = json!(
                samples
                    .iter()
                    .all(|s| s.solve.converged && s.max_speed_m_s <= 1e-3)
            );
            hold["basis"] = json!(
                "caller-declared last N accepted gravity steps on a static body; geometric mean-band displacement, not contact-point slip; quietness checks every cloth vertex and does not establish stress equilibrium, physical hold or fit"
            );
            hold
        };
        windows.push(json!({"instance":group.casing.instance,
            "window":band_window(initial,frames,&vertices,opts.step),"declared_hold":hold}));
    }
    report["elastic_hold_window"] = json!({"requested_steps":opts.elastic_hold_steps,
        "status":if opts.elastic_hold_steps == 0 {"not_requested"} else if windows.is_empty() {"unavailable_no_elastic_casings"} else {"available"}});
    report["elastic_observation_windows"] = json!(windows);
}

/// Record start-state repair receipts and advisory flags without clobbering an earlier repair when
/// both checkpoint backtracking and opposing-leg separation ran on the same start.
fn add_start_repair(
    report: &mut serde_json::Map<String, Value>,
    backtracked_from: Option<usize>,
    accepted_relax_iters: usize,
    leg_separation: Option<(usize, f64)>,
) {
    let backtrack_receipt = backtracked_from.map(|original| {
        json!({
            "basis": "earlier_contact_clear_relaxation",
            "rejected_relax_iters": original,
            "accepted_relax_iters": accepted_relax_iters,
        })
    });
    let leg_receipt = leg_separation.map(|(moved, max_mm)| {
        json!({
            "basis": "opposing_leg_placement",
            "moved_welded_vertices": moved,
            "maximum_displacement_mm": max_mm,
        })
    });
    match (backtrack_receipt, leg_receipt) {
        (None, None) => {}
        (Some(receipt), None) => {
            report.insert("start_repair".into(), receipt);
        }
        (None, Some(receipt)) => {
            report.insert("start_repair".into(), receipt);
        }
        (Some(backtrack), Some(leg)) => {
            let (Some(original), Some((moved, max_mm))) = (backtracked_from, leg_separation) else {
                unreachable!()
            };
            report.insert(
                "start_repair".into(),
                json!({
                    "basis": "earlier_contact_clear_relaxation_and_opposing_leg_placement",
                    "rejected_relax_iters": original,
                    "accepted_relax_iters": accepted_relax_iters,
                    "moved_welded_vertices": moved,
                    "maximum_displacement_mm": max_mm,
                    "steps": [backtrack, leg],
                }),
            );
        }
    }
    if backtracked_from.is_some() {
        report
            .get_mut("flags")
            .and_then(Value::as_array_mut)
            .expect("metrics flags exist")
            .push(json!(
                "start restored to an earlier contact-clear relaxation checkpoint"
            ));
    }
    if leg_separation.is_some() {
        report
            .get_mut("flags")
            .and_then(Value::as_array_mut)
            .expect("metrics flags exist")
            .push(json!(
                "opposing leg panels separated in the starting placement"
            ));
    }
}

fn add_run_fields(
    report: &mut Value,
    scene: &Scene,
    fabric: &Fabric,
    opts: &DrapeOptions,
    input: &MetricsInput,
    casing_mu: &[f64],
) {
    let mesh = &scene.mesh;
    let area: f64 = mesh
        .faces
        .iter()
        .map(|f| {
            let (a, b, c) = (mesh.verts2d[f[0]], mesh.verts2d[f[1]], mesh.verts2d[f[2]]);
            0.5 * ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs()
        })
        .sum::<f64>()
        * 1e-6;
    let w = &scene.start.gather_weights;
    let plain: Vec<(usize, usize)> = face_edges(&mesh.faces)
        .into_iter()
        .filter(|&(a, b)| w[a] >= 1.0 && w[b] >= 1.0)
        .collect();
    let init: Vec<f64> = edge_strain(input.start, &mesh.verts2d, &plain)
        .into_iter()
        .map(|e| e.abs() * 100.0)
        .collect();
    let style = input.pattern.raw()["style"].as_str().unwrap_or("");
    let family = |id: &str| id.rsplit_once('_').map_or(id, |(a, _)| a).to_string();
    let mut names: Vec<String> = mesh.seams.iter().map(|s| family(&s.id)).collect();
    names.sort();
    names.dedup();
    let legend: serde_json::Map<String, Value> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let c = SEAM_COLORS[i % SEAM_COLORS.len()];
            (n.clone(), json!(c.map(|v| round(v, 2))))
        })
        .collect();
    let o = report
        .as_object_mut()
        .expect("the metrics report is an object");
    o.insert("vertices".into(), json!(mesh.verts2d.len()));
    o.insert("faces".into(), json!(mesh.faces.len()));
    o.insert("sewing_pairs".into(), json!(mesh.pairs.len()));
    o.insert("area_m2".into(), json!(round(area, 4)));
    o.insert(
        "fabric".into(),
        json!({
            "areal_kg_m2": fabric.areal_kg_m2,
            "tensile_N_m": fabric.tensile_n_m,
            "bending_Nm": fabric.bending_nm,
        }),
    );
    o.insert(
        "initial_strain_pct".into(),
        json!({
            "p95": if init.is_empty() { 0.0 } else { round(percentile(&init, 95.0), 1) },
            "max": round(init.iter().copied().fold(0.0, f64::max), 1),
        }),
    );
    o.insert("presew".into(), json!(true));
    o.insert("arm_deg".into(), json!(input.arm_deg));
    if let Some(arms) = input.arm_pose {
        o.insert("arm_pose".into(), json!(arms));
    }
    o.insert(
        "gather_zone_vertices".into(),
        json!(w.iter().filter(|&&v| v < 1.0).count()),
    );
    // The reference's cloth resists compression as stiffly as tension (factor 1). A softer factor is a
    // different cloth model, in which cloth shorter than its pattern stands for folds finer than the
    // mesh, so a report made with one names it. Reports at the reference's factor are unchanged.
    if opts.compression < 1.0 {
        o.insert("compression_factor".into(), json!(opts.compression));
    }
    // A seam's declared band of pleats is likewise an authored model of folds the mesh does not resolve.
    let bands: Vec<Value> = mesh
        .seams
        .iter()
        .filter_map(|s| {
            s.gather_relief.map(|band| {
                json!({"seam": s.id, "relief_mm": band.relief_mm, "compression": band.compression})
            })
        })
        .collect();
    if !bands.is_empty() {
        o.insert("gather_relief".into(), json!(bands));
    }
    o.insert("relax_iters".into(), json!(scene.start.relax_iterations));
    add_start_repair(
        o,
        scene.start_backtracked_from,
        scene.start.relax_iterations,
        scene.start_leg_separation,
    );
    o.insert(
        "frames".into(),
        json!({"sew": opts.sew_steps, "drape": opts.drape_steps}),
    );
    o.insert(
        "animation".into(),
        json!({
            "pc2": "fold.pc2",
            "frames": FOLD_FRAMES + ZIP_FRAMES + opts.sew_steps + opts.drape_steps,
            "fold_frames": FOLD_FRAMES,
            "seam_legend": legend,
        }),
    );
    if opts.self_contact.is_some() {
        o["animation"]["first_physical_frame"] = json!(FOLD_FRAMES + ZIP_FRAMES - 1);
    }
    // FrankenTextiles: no collider inset; the cloth's mid-surface keeps this offset from the body.
    o.insert(
        "cloth_offset_mm".into(),
        json!(round(opts.contact.thickness * 1000.0, 2)),
    );
    o.insert("style".into(), json!(style));
    o.insert(
        "self_contact".into(),
        self_contact_report(scene.solver.contact.as_ref(), &scene.solver.x),
    );
    let elastics = &scene.solver.model.elastics;
    let mut casing_reports = Vec::new();
    let explicit = input.pattern.raw()["meta"]["solved"]
        .get("elastic_casings")
        .is_some();
    for (group, &mu) in scene.elastic_groups.iter().zip(casing_mu) {
        // FrankenTextiles: the casing's elastic at the end (the reference has none). Its rings (fold and
        // seam line) pull in parallel: the elastic's tension is their sum and it lies between them.
        let x = &scene.solver.x;
        let rings: Vec<(f64, f64, f64)> = scene.elastic_rings[group.rings.clone()]
            .iter()
            .map(|r| {
                let seg = &elastics[r.clone()];
                let rest: f64 = seg.iter().map(|e| e.rest).sum();
                let now: f64 = seg.iter().map(|e| (x[e.v[1]] - x[e.v[0]]).norm()).sum();
                let tension = seg.iter().map(|e| e.tension(x)).sum::<f64>() / seg.len() as f64;
                (rest, now, tension)
            })
            .collect();
        let n = rings.len() as f64;
        let relaxed = rings[0].0;
        let length = rings.iter().map(|r| r.1).sum::<f64>() / n;
        let tension: f64 = rings.iter().map(|r| r.2).sum();
        let stretch = (length / relaxed - 1.0) * 100.0;
        // KAT 4's ideal-ring estimate, compared with the declared source mass, not all mass on
        // shared seam degrees of freedom. This does not solve actual inter-panel load transfer.
        let grip = 2.0 * std::f64::consts::PI * mu * tension;
        let weight = group.loaded_mass_kg * GRAVITY;
        let mut row = json!({
            "relaxed_mm": round(relaxed * 1000.0, 1),
            "length_mm": round(length * 1000.0, 1),
            "rings_mm": rings.iter().map(|r| round(r.1 * 1000.0, 1)).collect::<Vec<_>>(),
            "stretch_pct": round(stretch, 1),
            "tension_n": round(tension, 3),
            "grip_n": round(grip, 3),
            "weight_n": round(weight, 3),
            "holds": grip >= weight,
            "ea_n": elastics[scene.elastic_rings[group.rings.start].start].ea * n,
        });
        if let Some(band) = &group.casing.material {
            row["band_material"] = json!(band);
            row["ea_basis"] = json!(
                "source-qualified finished band; split across casing rings, not multiplied by ring count"
            );
        }
        if explicit {
            row["instance"] = json!(group.casing.instance);
            row["load_instances"] = json!(group.casing.load_instances);
            row["load_mass"] = json!({
                "cloth_kg":group.loaded_cloth_mass_kg,
                "attached_kg":group.loaded_attached_mass_kg,
                "inertial_kg":group.loaded_mass_kg,
                "basis":"retained source-triangle nodal shares and source-target accessory allocations; no borrowing across seam welds"
            });
            row["friction_mu"] = json!(mu);
            row["grip_basis"] = json!(
                "independent ideal ring estimate using this casing/body friction and declared garment's simulated inertia; not integrated contact traction or combined support capacity"
            );
            casing_reports.push(row);
        } else {
            o.insert("elastic".into(), row);
        }
        if grip < weight {
            // advisory, as the flat `elastic_hold` row already judges the elastic
            if let Some(flags) = o.get_mut("flags").and_then(Value::as_array_mut) {
                let message = format!(
                    "the elastic does not grip: {} % against its relaxed length on the body, its friction \
                     holds {} N of the garment's {} N, so the waistband rests on the body instead (shorten \
                     the elastic)",
                    ftx_core::py::fixed(stretch, 1),
                    ftx_core::py::fixed(grip, 2),
                    ftx_core::py::fixed(weight, 2)
                );
                flags.push(json!(if explicit {
                    format!("{}: {message}", group.casing.instance)
                } else {
                    message
                }));
            }
        }
    }
    if explicit && !casing_reports.is_empty() {
        o.insert("elastics".into(), json!(casing_reports));
    }
}

fn self_contact_report(contact: Option<&Contact>, x: &[Vec3]) -> Value {
    let mut report = match contact.and_then(|c| c.self_contact_params().map(|p| (c, p))) {
        Some((c, p)) => json!({
            "enabled": true,
            "audit": "passed",
            "scope": "welded solver mesh, sewing and draping states (not the fold-up animation)",
            "thickness_mm": p.thickness * 1000.0,
            "friction_mu": p.mu,
            "activation_mm": p.dhat * 1000.0,
            "nearby_min_final_gap_mm": c.min_self_gap(x, p.dhat).map(|g| g * 1000.0),
            "gap_query_range_mm": p.dhat * 1000.0,
            "start_creases": c.start_creases().0,
            "start_crease_min_offset_mm": c.start_creases().1.map(|o| o * 1000.0),
        }),
        None => json!({"enabled": false, "audit": "not_simulated"}),
    };
    if contact.is_some_and(Contact::has_surface_materials) && report["enabled"] == true {
        report["coefficient_basis"] = json!(
            "thickness/friction fields are broad-phase upper envelopes; per-pair coefficients come from source-bound material interfaces"
        );
    }
    report
}

/// The fold-up viewer page of a run (`template` is `assets/fold_viewer_template.html`).
///
/// # Errors
/// When the template has no data placeholder.
pub fn viewer(
    run: &DrapeRun,
    pattern: &Pattern,
    body: &DrapeBody,
    template: &str,
) -> Result<String, String> {
    let raw = pattern.raw();
    let style = raw["style"].as_str().unwrap_or("");
    let title = raw["meta"]["title"].as_str().unwrap_or(style);
    let mut payload = viewer_payload(
        &run.scene.mesh,
        &run.fold,
        &ViewerExtras {
            contact_replay: run.report["self_contact"]["enabled"].as_bool() == Some(true),
            title: if title.is_empty() { "fold-up" } else { title },
            style,
            body: body.display,
            gating: run.gating(),
            advisory: run.advisory(),
            settled: run.report["settled"].as_bool().unwrap_or(false),
            arm_deg: run.report["arm_deg"].as_f64().unwrap_or(body.arm_deg),
        },
        150,
    );
    if let Some(arms) = body.arm_pose {
        payload["report"]["arm_pose"] = json!(arms);
    }
    viewer_html(template, &payload)
}

#[cfg(test)]
mod self_contact_tests {
    use super::*;
    use crate::cloth::SolveReport;
    use crate::contact::Body;
    use crate::scene::PhaseStep;

    fn scope_pattern() -> Value {
        json!({
            "schema":"fabric-pattern/1", "units":"mm",
            "pieces":[{
                "id":"panel", "cut":{"qty":1},
                "vertices":{"a":[0,0],"b":[100,0],"c":[100,200],"d":[0,200]},
                "edges":[
                    {"id":"bottom","from":"a","to":"b"},
                    {"id":"right","from":"b","to":"c"},
                    {"id":"top","from":"c","to":"d"},
                    {"id":"left","from":"d","to":"a"}
                ],
                "placement":{"part":"arm","y0_landmark":"anchor","y0":7}
            }],
            "seams":[]
        })
    }

    struct LevelFrames;
    impl PartFrames for LevelFrames {
        fn frame(&self, _: &str) -> Option<crate::start::PartFrame> {
            panic!("scope and level checks do not place geometry")
        }
        fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
            match (part, name) {
                ("arm_R" | "leg_R", "anchor") => Some(10.0),
                ("arm_R" | "leg_R", "cuff") => Some(30.0),
                ("arm_L" | "leg_L", "anchor") => Some(100.0),
                ("arm_L" | "leg_L", "cuff") => Some(220.0),
                _ => None,
            }
        }
        fn section_radius(&self, _: &str, _: f64) -> f64 {
            panic!("scope and level checks do not sample the body")
        }
    }

    #[test]
    fn metric_levels_follow_explicit_physical_limb_side() {
        let mut raw = scope_pattern();
        assert_eq!(
            metric_level_y(&Pattern::new(raw.clone()), &LevelFrames, "panel", "cuff"),
            Some(27.0)
        );
        for part in ["arm", "leg"] {
            for (side, expected) in [("R", 27.0), ("L", 127.0)] {
                raw["pieces"][0]["placement"]["part"] = json!(part);
                raw["pieces"][0]["placement"]["side"] = json!(side);
                let pattern = Pattern::new(raw.clone());
                assert!(pattern.structure_errors().is_empty());
                assert_eq!(
                    metric_level_y(&pattern, &LevelFrames, "panel", "cuff"),
                    Some(expected)
                );
                assert_eq!(
                    metric_level_y(&pattern, &LevelFrames, "panel", "unknown"),
                    None
                );
            }
        }
    }

    #[test]
    fn production_drape_refuses_missing_panels_before_body_or_solver_work() {
        let mut raw = scope_pattern();
        let mut missing = raw["pieces"][0].clone();
        missing["id"] = json!("unplaced");
        missing["placement"] = Value::Null;
        raw["pieces"].as_array_mut().unwrap().push(missing);
        raw["seams"] = json!([{"id":"attachment","a":"panel/right","b":"unplaced/left"}]);
        let pattern = Pattern::new(raw);
        let body = DrapeBody {
            frames: &LevelFrames,
            collision: (&[], &[]),
            display: (&[], &[]),
            arm_deg: 0.0,
            arm_pose: None,
        };
        let refusal = drape(
            &pattern,
            &json!({}),
            &body,
            &DrapeOptions::default(),
            |_| panic!("incomplete garments must refuse before progress"),
        )
        .err()
        .expect("incomplete garment must refuse");
        assert_eq!(refusal.code, RefusalCode::InvalidDocument);
        assert!(refusal.message.contains("unplaced"));
        assert!(refusal.message.contains("attachment"));
    }

    #[test]
    fn declared_subset_is_visible_without_erasing_existing_gates() {
        let mut raw = scope_pattern();
        let mut excluded = raw["pieces"][0].clone();
        excluded["id"] = json!("lining");
        excluded["sim"] = json!(false);
        raw["pieces"].as_array_mut().unwrap().push(excluded);
        raw["seams"] = json!([{"id":"lining_join","a":"panel/right","b":"lining/left"}]);
        let scope = simulation_scope(&Pattern::new(raw)).unwrap();
        let mut report = json!({"flags":["existing gate"],"gating":["existing gate"]});
        add_simulation_scope(&mut report, scope.clone());
        assert_eq!(report["simulation_scope"], scope);
        assert_eq!(report["flags"].as_array().unwrap().len(), 2);
        assert_eq!(report["gating"], json!(["existing gate"]));
        assert!(
            report["flags"][1]
                .as_str()
                .unwrap()
                .contains("1 instance(s) and 1 seam(s)")
        );
        let scope = simulation_scope(&Pattern::new(scope_pattern())).unwrap();
        let mut complete = json!({"flags":[],"gating":[]});
        add_simulation_scope(&mut complete, scope);
        assert_eq!(complete["simulation_scope"]["basis"], "complete_pattern");
        assert_eq!(complete["flags"], json!([]));
    }

    #[test]
    fn casing_window_retains_duration_signed_drift_and_sampled_peak() {
        let frame = |z| vec![Vec3::new(0., 0., z), Vec3::new(1., 0., z)];
        for (heights, end, peak) in [
            ([0., 0.00001, 0.], 0., 0.00001),
            ([0., -0.002, -0.01], -0.01, 0.01),
        ] {
            let frames: Vec<_> = heights.into_iter().map(frame).collect();
            let result = band_window(&frames[0], &frames[1..], &[0, 1], 0.01);
            assert_eq!(result["duration_s"], 0.02);
            assert_eq!(result["signed_vertical_displacement_m"], end);
            assert_eq!(result["maximum_absolute_vertical_displacement_m"], peak);
        }
        assert_eq!(
            band_window(&frame(0.), &[], &[0, 1], 0.01)["status"],
            "not_simulated"
        );
    }

    #[test]
    fn numerical_settledness_rejects_motion_hidden_by_window_means() {
        let options = DrapeOptions::default();
        let frames = [0.0_f64, 0.001, 0.0, 0.001, 0.0, 0.001, 0.0];
        assert_eq!(
            frames[..2].iter().sum::<f64>(),
            frames[4..6].iter().sum::<f64>()
        );
        let mut phase = PhaseReport::default();
        for pair in frames.windows(2) {
            let speed = (pair[1] - pair[0]).abs() / options.step;
            phase.samples.push(PhaseStep {
                solve: SolveReport {
                    converged: true,
                    ..SolveReport::default()
                },
                max_speed_m_s: speed,
                contact: None,
                time_subdivision: None,
            });
            phase.steps += 1;
            phase.max_speed = speed;
        }
        let mut report = json!({"settled":true,"flags":[],"gating":[]});
        add_numerical_report(
            &mut report,
            None,
            &PhaseReport::default(),
            &phase,
            &options,
            None,
            0.0,
        );
        assert_eq!(report["settled"], true, "reference metric is preserved");
        assert_eq!(report["numerical"]["settled"], false);
        assert_eq!(report["numerical"]["consecutive_quiet_steps"], 0);
        assert_eq!(report["gating"].as_array().unwrap().len(), 1);
        assert_eq!(report["flags"], report["gating"]);
        assert_eq!(
            report["numerical"]["drape"]["samples"]
                .as_array()
                .unwrap()
                .len(),
            6
        );

        for _ in 0..3 {
            phase.samples.push(PhaseStep {
                solve: SolveReport {
                    converged: true,
                    ..SolveReport::default()
                },
                max_speed_m_s: 0.0,
                contact: None,
                time_subdivision: None,
            });
            phase.steps += 1;
        }
        phase.max_speed = 0.0;
        let mut quiet = json!({"settled":true,"flags":[],"gating":[]});
        add_numerical_report(
            &mut quiet,
            None,
            &PhaseReport::default(),
            &phase,
            &options,
            None,
            0.0,
        );
        assert_eq!(quiet["numerical"]["settled"], true);
        assert_eq!(quiet["numerical"]["consecutive_quiet_steps"], 3);
        assert_eq!(quiet["gating"], json!([]));

        for (achieved, incomplete) in [
            (Some(0.0007366308626266741), true),
            (Some(options.contact.thickness), false),
            (Some(2e-3), false),
            (None, false),
        ] {
            let mut offset = json!({"settled":true,"flags":[],"gating":[]});
            add_numerical_report(
                &mut offset,
                None,
                &PhaseReport::default(),
                &phase,
                &options,
                achieved,
                options.contact.thickness,
            );
            assert_eq!(offset["numerical"]["settled"], true);
            assert_eq!(
                offset["gating"].as_array().unwrap().len(),
                usize::from(incomplete)
            );
            assert_eq!(offset["flags"], offset["gating"]);
        }
    }

    #[test]
    fn sewing_only_qualification_does_not_claim_gravity_settling() {
        let mut report = json!({"settled":true,"flags":[],"gating":[]});
        add_numerical_report(
            &mut report,
            None,
            &PhaseReport::default(),
            &PhaseReport::default(),
            &DrapeOptions::default(),
            None,
            0.0,
        );
        assert_eq!(report["numerical"]["gravity_simulated"], false);
        assert!(report["numerical"]["settled"].is_null());
        assert_eq!(report["gating"], json!([]));
    }

    #[test]
    fn options_validate_before_geometry_work() {
        let default = DrapeOptions::default();
        default.validate().unwrap();
        assert!(default.self_contact.is_none());
        for opts in [
            DrapeOptions {
                spacing: f64::NAN,
                ..default
            },
            DrapeOptions {
                step: 0.0,
                ..default
            },
            DrapeOptions {
                compression: 1.1,
                ..default
            },
            DrapeOptions {
                sew_steps: 0,
                drape_steps: 0,
                ..default
            },
            DrapeOptions {
                sew_steps: usize::MAX,
                ..default
            },
            DrapeOptions {
                max_settle_steps: 257,
                ..default
            },
            DrapeOptions {
                time_subdivision: Some(SubdivisionOptions {
                    max_halvings: 17,
                    max_trials: 128,
                }),
                ..default
            },
            DrapeOptions {
                time_subdivision: Some(SubdivisionOptions {
                    max_halvings: 4,
                    max_trials: 47,
                }),
                ..default
            },
            DrapeOptions {
                time_subdivision: Some(SubdivisionOptions::default()),
                step: f64::MIN_POSITIVE,
                ..default
            },
            DrapeOptions {
                step: f64::MAX,
                max_settle_steps: 1,
                ..default
            },
            DrapeOptions {
                self_contact: Some(ContactParams {
                    mu: -0.1,
                    ..default.contact
                }),
                ..default
            },
            DrapeOptions {
                adaptive_barrier: Some(AdaptiveBarrierPolicy {
                    target_gap_fraction: 1.0,
                    ..AdaptiveBarrierPolicy::default()
                }),
                ..default
            },
            DrapeOptions {
                stick_slip: Some(StickSlipFriction {
                    mu_s: 0.2,
                    mu_k: 0.4,
                    transition_v_m_s: 0.02,
                }),
                ..default
            },
            DrapeOptions {
                self_stick_slip: Some(StickSlipFriction {
                    mu_s: 0.5,
                    mu_k: 0.3,
                    transition_v_m_s: 0.02,
                }),
                ..default
            },
        ] {
            assert!(opts.validate().is_err(), "{opts:?}");
        }
        for policy in [
            StaticEquilibriumOptions {
                correction_tolerance_m: f64::NAN,
                ..Default::default()
            },
            StaticEquilibriumOptions {
                correction_tolerance_m: 0.0,
                ..Default::default()
            },
            StaticEquilibriumOptions {
                max_newton_iterations: 0,
                ..Default::default()
            },
            StaticEquilibriumOptions {
                max_newton_iterations: 1001,
                ..Default::default()
            },
        ] {
            assert!(
                DrapeOptions {
                    static_equilibrium: Some(policy),
                    ..default
                }
                .validate()
                .is_err()
            );
        }
    }

    #[test]
    fn report_distinguishes_unchecked_from_enabled_self_contact() {
        assert_eq!(
            self_contact_report(None, &[]),
            json!({"enabled": false, "audit": "not_simulated"})
        );
        let x = vec![
            Vec3::ZERO,
            Vec3::new(0.02, 0.0, 0.0),
            Vec3::new(0.0, 0.02, 0.0),
        ];
        let p = DrapeOptions::default().contact;
        let c = Contact::new(Body::empty(), p, vec![[0, 1, 2]], |a, b| {
            (x[a] - x[b]).norm()
        });
        assert_eq!(self_contact_report(Some(&c), &x)["audit"], "not_simulated");
        let c = c.with_self_contact(&x, p).unwrap();
        let report = self_contact_report(Some(&c), &x);
        assert_eq!(report["enabled"], true);
        assert_eq!(report["audit"], "passed");
        assert_eq!(report["thickness_mm"], 1.5);
        assert!(report["nearby_min_final_gap_mm"].is_null());
    }

    #[test]
    fn drape_options_wire_newton_strategy_and_generalized_alpha_end_to_end() {
        let mut opts = DrapeOptions::default();
        assert!(opts.validate().is_ok());

        // Invalid solver strategy and integrator parameters refuse up front.
        opts.newton_strategy = NewtonStrategy::AdaptiveIlu {
            pcg_iters: 0,
            max_shifts: 4,
        };
        assert!(opts.validate().is_err());
        opts.newton_strategy = NewtonStrategy::TrustRegionSteihaug {
            initial_radius: 0.5,
            max_radius: 0.1,
            min_radius: 1e-12,
            cg_iters: 0,
        };
        assert!(opts.validate().is_err());
        opts.newton_strategy = NewtonStrategy::ProjectedNewton;
        opts.integrator = TimeIntegrator::GeneralizedAlpha { rho_inf: 1.2 };
        assert!(opts.validate().is_err());

        // Valid opt-in configurations pass validation cleanly.
        opts.newton_strategy = NewtonStrategy::adaptive_ilu_default();
        opts.integrator = TimeIntegrator::GeneralizedAlpha { rho_inf: 0.8 };
        assert!(opts.validate().is_ok());
        opts.newton_strategy = NewtonStrategy::trust_region_default();
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn xd85_sewn_band_qualifies_generalized_alpha_rho08_operating_domain() {
        struct Form;
        impl PartFrames for Form {
            fn frame(&self, _: &str) -> Option<crate::start::PartFrame> {
                Some(crate::start::PartFrame {
                    origin: Vec3::ZERO,
                    up: Vec3::new(0., 0., 1.),
                    front: Vec3::new(0., -1., 0.),
                    lat: Vec3::new(1., 0., 0.),
                })
            }
            fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
                Some(10.0)
            }
            fn section_radius(&self, _: &str, _: f64) -> f64 {
                30.0
            }
        }
        let mut verts = Vec::new();
        for z in [-0.04, 0.08] {
            for i in 0..16 {
                let (s, c) = ftx_core::det::sin_cos(i as f64 * std::f64::consts::TAU / 16.0);
                verts.push(Vec3::new(0.03 * c, 0.03 * s, z));
            }
        }
        let mut tris = Vec::new();
        for i in 0..16 {
            let j = (i + 1) % 16;
            tris.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
        }
        let form = Form;
        let body = DrapeBody {
            frames: &form,
            collision: (&verts, &tris),
            display: (&verts, &tris),
            arm_deg: 35.0,
            arm_pose: None,
        };
        let w = (0.98 * 0.06 * std::f64::consts::PI * 1000.0).round();
        let pat = Pattern::new(json!({
            "schema": "fabric-pattern/1",
            "units": "mm",
            "pieces": [{
                "id": "band",
                "cut": {"qty": 1},
                "vertices": {"a": [0, 0], "b": [w, 0], "c": [w, 20], "d": [0, 20]},
                "edges": [
                    {"id": "bottom", "from": "a", "to": "b"},
                    {"id": "right", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"},
                    {"id": "left", "from": "d", "to": "a"}
                ],
                "notches": [],
                "grain": [[0, 0], [0, 20]],
                "placement": {"part": "torso", "radius_mm": 35, "x0": w / 2.0}
            }],
            "seams": [{"id": "join", "a": "band/right", "b": "band/left"}]
        }));

        // 1. Coarse 10 ms step at rho_inf=0.8 retains its honest structured refusal and offers a
        // targeted repair pointing to smaller time steps or lower rho_inf.
        let coarse_err = drape(
            &pat,
            &json!({}),
            &body,
            &DrapeOptions {
                sew_steps: 4,
                drape_steps: 6,
                step: 0.01,
                newton_strategy: NewtonStrategy::adaptive_ilu_default(),
                integrator: TimeIntegrator::GeneralizedAlpha { rho_inf: 0.8 },
                ..DrapeOptions::default()
            },
            |_| {},
        )
        .err()
        .expect("coarse 10 ms rho_inf=0.8 run should refuse");
        assert_eq!(coarse_err.code, RefusalCode::NotConverged);
        assert!(
            coarse_err
                .repairs
                .iter()
                .any(|r| r.contains("rho_inf") && r.contains("time step")),
            "{:?}",
            coarse_err.repairs
        );

        // 2. Resolving the exact same 40 ms sew + 60 ms drape schedule at 1 ms with rho_inf=0.8
        // converges, settles with zero gating flags, agrees between AdaptiveIlu and
        // ProjectedNewton, and passes an independent ReplayContact audit on every physical frame.
        let resolved_ilu = drape(
            &pat,
            &json!({}),
            &body,
            &DrapeOptions {
                sew_steps: 40,
                drape_steps: 60,
                step: 0.001,
                newton_strategy: NewtonStrategy::adaptive_ilu_default(),
                integrator: TimeIntegrator::GeneralizedAlpha { rho_inf: 0.8 },
                ..DrapeOptions::default()
            },
            |_| {},
        )
        .expect("1 ms resolved 40 ms + 60 ms schedule at rho_inf=0.8 must converge (AdaptiveIlu)");
        assert_eq!(resolved_ilu.report["numerical"]["settled"], json!(true));
        assert_eq!(resolved_ilu.report["gating"], json!([]));
        assert!(
            resolved_ilu.drape.max_speed <= SETTLE_SPEED_M_S,
            "max_speed={}",
            resolved_ilu.drape.max_speed
        );

        let mut replay = crate::animation::ReplayContact::new(body.display.0, body.display.1)
            .expect("cylinder display mesh");
        let first_sewn = resolved_ilu.fold.fold + resolved_ilu.fold.zip;
        for (idx, frame) in resolved_ilu
            .fold
            .frames
            .iter()
            .enumerate()
            .skip(first_sewn - 1)
        {
            replay
                .check(frame, &resolved_ilu.scene.mesh.faces, idx)
                .expect("every physical frame must remain crossing-free");
        }

        let resolved_pn = drape(
            &pat,
            &json!({}),
            &body,
            &DrapeOptions {
                sew_steps: 40,
                drape_steps: 60,
                step: 0.001,
                pcg_iters: 0,
                newton_strategy: NewtonStrategy::ProjectedNewton,
                integrator: TimeIntegrator::GeneralizedAlpha { rho_inf: 0.8 },
                ..DrapeOptions::default()
            },
            |_| {},
        )
        .expect("1 ms resolved schedule at rho_inf=0.8 must converge (ProjectedNewton)");
        let max_diff = resolved_ilu
            .scene
            .solver
            .x
            .iter()
            .zip(&resolved_pn.scene.solver.x)
            .map(|(a, b)| (*a - *b).norm())
            .fold(0.0_f64, f64::max);
        assert!(
            max_diff < 1e-4,
            "AdaptiveIlu and exact ProjectedNewton diverged at 1 ms rho_inf=0.8: max_diff={max_diff}"
        );
    }

    #[test]
    fn start_repair_preserves_single_schemas_and_retains_both_when_combined() {
        let mut none = json!({"flags": []});
        add_start_repair(none.as_object_mut().unwrap(), None, 200, None);
        assert!(none.get("start_repair").is_none());
        assert_eq!(none["flags"], json!([]));

        let mut backtrack_only = json!({"flags": []});
        add_start_repair(
            backtrack_only.as_object_mut().unwrap(),
            Some(600),
            400,
            None,
        );
        assert_eq!(
            backtrack_only["start_repair"],
            json!({
                "basis": "earlier_contact_clear_relaxation",
                "rejected_relax_iters": 600,
                "accepted_relax_iters": 400,
            })
        );
        assert_eq!(
            backtrack_only["flags"],
            json!(["start restored to an earlier contact-clear relaxation checkpoint"])
        );

        let mut leg_only = json!({"flags": []});
        add_start_repair(
            leg_only.as_object_mut().unwrap(),
            None,
            200,
            Some((14, 3.25)),
        );
        assert_eq!(
            leg_only["start_repair"],
            json!({
                "basis": "opposing_leg_placement",
                "moved_welded_vertices": 14,
                "maximum_displacement_mm": 3.25,
            })
        );
        assert_eq!(
            leg_only["flags"],
            json!(["opposing leg panels separated in the starting placement"])
        );

        let mut combined = json!({"flags": []});
        add_start_repair(
            combined.as_object_mut().unwrap(),
            Some(600),
            400,
            Some((14, 3.25)),
        );
        assert_eq!(
            combined["start_repair"]["basis"],
            "earlier_contact_clear_relaxation_and_opposing_leg_placement"
        );
        assert_eq!(combined["start_repair"]["rejected_relax_iters"], 600);
        assert_eq!(combined["start_repair"]["accepted_relax_iters"], 400);
        assert_eq!(combined["start_repair"]["moved_welded_vertices"], 14);
        assert_eq!(combined["start_repair"]["maximum_displacement_mm"], 3.25);
        assert_eq!(
            combined["start_repair"]["steps"],
            json!([backtrack_only["start_repair"], leg_only["start_repair"],])
        );
        assert_eq!(
            combined["flags"],
            json!([
                "start restored to an earlier contact-clear relaxation checkpoint",
                "opposing leg panels separated in the starting placement",
            ])
        );
    }

    #[test]
    fn drape_validates_arm_deg_and_resolves_symmetric_and_fallback_arm_pose() {
        let make_body = |arm_deg: f64, arm_pose: Option<ftx_body::drape::ArmPose>| DrapeBody {
            frames: &LevelFrames,
            collision: (&[], &[]),
            display: (&[], &[]),
            arm_deg,
            arm_pose,
        };
        let pattern = Pattern::new(scope_pattern());
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let body = make_body(bad, None);
            let err = drape(
                &pattern,
                &json!({}),
                &body,
                &DrapeOptions::default(),
                |_| panic!("non-finite arm_deg must refuse before progress or meshing"),
            )
            .err()
            .expect("non-finite arm_deg without arm_pose must refuse");
            assert_eq!(err.code, RefusalCode::NonFinite);
            assert!(err.message.contains("arm angle"));
        }

        // Canonicalizes -0.0 to +0.0 when arm_pose is absent.
        let zero = effective_arm_deg(&make_body(-0.0, None)).unwrap();
        assert_eq!(zero.to_bits(), 0.0_f64.to_bits());

        // Symmetric arm_pose overrides fallback arm_deg (even if stale or non-finite).
        let sym = ftx_body::drape::ArmPose {
            left_abduction_deg: 35.0,
            right_abduction_deg: 35.0,
        };
        assert_eq!(
            effective_arm_deg(&make_body(50.0, Some(sym))).unwrap(),
            35.0
        );
        assert_eq!(
            effective_arm_deg(&make_body(f64::NAN, Some(sym))).unwrap(),
            35.0
        );

        // Asymmetric arm_pose keeps a valid fallback arm_deg and falls back to midpoint otherwise.
        let asym = ftx_body::drape::ArmPose {
            left_abduction_deg: 20.0,
            right_abduction_deg: 80.0,
        };
        assert_eq!(
            effective_arm_deg(&make_body(35.0, Some(asym))).unwrap(),
            35.0
        );
        assert_eq!(
            effective_arm_deg(&make_body(f64::NAN, Some(asym))).unwrap(),
            50.0
        );
        assert_eq!(
            effective_arm_deg(&make_body(240.0, Some(asym))).unwrap(),
            50.0
        );

        // Invalid arm_pose still refuses regardless of fallback arm_deg.
        let invalid = ftx_body::drape::ArmPose {
            left_abduction_deg: -5.0,
            right_abduction_deg: 35.0,
        };
        assert!(effective_arm_deg(&make_body(35.0, Some(invalid))).is_err());
    }
}
