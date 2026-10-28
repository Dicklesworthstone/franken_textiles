//! One drape run end to end — what `ftx drape`, the examples and (later) the browser call: mesh the
//! pattern, place and start it on a body, sew and drape it, record the fold-up, and read the metrics and
//! flags into a `sim_report.json`-shaped report (spec 04 §6.11).

use ftx_core::np::percentile;
use ftx_core::py::round;
use ftx_core::{Refusal, RefusalCode, Vec3};
use ftx_pattern::{Pattern, Side, instance::physical_side};
use serde_json::{Value, json};

use crate::animation::{
    FOLD_FRAMES, FoldUp, SEAM_COLORS, ViewerExtras, ZIP_FRAMES, board_layout, fold_frames,
    viewer_html, viewer_payload, zip_frames,
};
use crate::contact::{Contact, ContactParams};
use crate::garment::{build_garment, simulation_scope};
use crate::metrics::{MIN_FRAMES, MetricsInput, metrics};
use crate::nearest::NearestTree;
use crate::scene::{
    Fabric, PartFrames, PhaseReport, Scene, casings_at_cut_length, elastic_casings, run_phase,
};
use crate::start::{edge_strain, face_edges};

/// Standard gravity (m/s²) of the drape phase.
pub const GRAVITY: f64 = 9.81;

/// The body a run drapes on.
pub struct DrapeBody<'a> {
    /// Part frames, landmarks and section radii.
    pub frames: &'a (dyn PartFrames + Sync),
    /// The surface the cloth collides with and the metrics measure against (vertices in m, triangles).
    pub collision: (&'a [Vec3], &'a [[usize; 3]]),
    /// The surface the viewer shows.
    pub display: (&'a [Vec3], &'a [[usize; 3]]),
    /// Arm abduction of the pose (degrees).
    pub arm_deg: f64,
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
    /// Report-only hold window: last N accepted gravity steps; zero means not requested.
    pub elastic_hold_steps: usize,
    /// Time step (s).
    pub step: f64,
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
}

impl Default for DrapeOptions {
    /// 12 mm mesh, 12 sewing and 36 draping steps of 1/24 s, Newton to 1e-3 m/s, compression as stiff
    /// as tension outside gather zones (the reference's `tension = compression = E_t·L`, spec 04 §5.3),
    /// friction lagged once a step while draping and not while sewing, a drafted casing's elastic at
    /// [`crate::scene::ELASTIC_EA`], ĝ 1 mm, the cloth's mid-surface 1.5 mm off the body, μ 0.4.
    fn default() -> Self {
        Self {
            spacing: 12.0,
            sew_steps: 12,
            drape_steps: 36,
            elastic_hold_steps: 0,
            step: 1.0 / 24.0,
            tol: 1e-3,
            compression: 1.0,
            lag_rounds: [0, 1],
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
        self.contact.validate()?;
        if let Some(p) = self.self_contact {
            p.validate()?;
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
        let total = self
            .sew_steps
            .checked_add(self.drape_steps)
            .filter(|n| n.checked_add(FOLD_FRAMES + ZIP_FRAMES).is_some())
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "animation frame count overflows",
                )
            })?;
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
    mut progress: impl FnMut(&str),
) -> Result<DrapeRun, Refusal> {
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
    let mesh = build_garment(pattern, opts.spacing)?;
    if mesh.verts2d.is_empty() {
        // a cone (fez) or a pattern whose pieces have no placement is judged flat, as `fab round` says
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "no piece has a fold-up placement: nothing to drape (the pattern is judged flat)",
        ));
    }
    let mut scene = crate::scene::build_scene_resolved(
        pattern,
        mesh,
        body.frames,
        body.collision.0,
        body.collision.1,
        &material,
        &elastics,
    )?;
    if let Some(p) = opts.self_contact {
        scene.solver = scene.solver.with_self_contact(p)?;
        progress("self-contact: start audit passed; cloth–cloth contact enabled");
    }
    scene.solver.tol = opts.tol;
    scene.solver.pcg_iters = opts.pcg_iters;
    progress(&format!(
        "start: {} vertices ({} after welding), relaxed {} iterations to p95 strain {:.3}",
        scene.mesh.verts2d.len(),
        scene.solver.x.len(),
        scene.start.relax_iterations,
        scene.start.block_p95.last().copied().unwrap_or(f64::NAN)
    ));
    let board = board_layout(&scene.mesh);
    let mut frames = fold_frames(&scene.mesh, &scene.placements, &board, FOLD_FRAMES);
    frames.extend(zip_frames(&scene.start.wrapped, &scene.start.relaxed));
    let initial_positions = scene.positions();
    let initial_contact = scene
        .solver
        .contact
        .as_ref()
        .map(|c| c.accepted_sample(&scene.solver.x));
    let mut phases = Vec::new();
    for (name, steps, g, lag) in [
        ("sew", opts.sew_steps, 0.0, opts.lag_rounds[0]),
        ("drape", opts.drape_steps, GRAVITY, opts.lag_rounds[1]),
    ] {
        scene.solver.friction_rounds = lag;
        let rep = run_phase(&mut scene, steps, opts.step, g, |_, x| {
            frames.push(x.to_vec());
        })?;
        progress(&format!(
            "{name}: {} steps, {} Newton iterations ({} unconverged), max speed {:.4} m/s",
            rep.steps, rep.iterations, rep.unconverged, rep.max_speed
        ));
        phases.push(rep);
    }
    let (sew, drape) = (phases[0].clone(), phases[1].clone());
    let fold = FoldUp {
        frames,
        fold: FOLD_FRAMES,
        zip: ZIP_FRAMES,
        sew: opts.sew_steps,
        drape: opts.drape_steps,
    };

    // Metrics against the collision body, levels from the body's frames.
    let true_body = NearestTree::new(body.collision.0.to_vec(), body.collision.1.to_vec())?;
    let level_y = |inst: &str, landmark: &str| metric_level_y(pattern, body.frames, inst, landmark);
    let seq = &fold.frames[FOLD_FRAMES + ZIP_FRAMES..];
    let mut report = metrics(
        &scene.mesh,
        &MetricsInput {
            pattern,
            seq,
            sew_end: opts.sew_steps.max(1),
            wrapped: &scene.start.wrapped,
            body: &true_body,
            level_y: &level_y,
            fit_spec,
            spacing: opts.spacing,
            arm_deg: body.arm_deg,
        },
    );
    let casing_mu: Vec<_> = elastics
        .iter()
        .map(|e| {
            (|| {
                let piece = pattern.instance(&e.instance)?.piece_index;
                let field = material.contact_materials.as_ref()?;
                Some(field.surfaces[field.piece_ids[piece]].friction_body)
            })()
            .unwrap_or(opts.contact.mu)
        })
        .collect();
    add_run_fields(
        &mut report,
        &scene,
        &fabric,
        opts,
        body.arm_deg,
        pattern,
        &casing_mu,
    );
    add_numerical_report(&mut report, &sew, &drape, opts);
    add_simulation_scope(&mut report, scope);
    report["material"] = material.report;
    add_casing_windows(&mut report, &scene, &fold, &initial_positions, &drape, opts);
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
                "rest_area_m2":area[i], "mass_kg":area[i]*fabrics[p].areal_kg_m2})
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
    Ok(DrapeRun {
        scene,
        fold,
        report,
        sew,
        drape,
        fabric,
    })
}

/// Preserve reference metrics; native acceptance additionally requires a quiet instantaneous tail.
fn add_numerical_report(
    report: &mut Value,
    sew: &PhaseReport,
    drape: &PhaseReport,
    opts: &DrapeOptions,
) {
    const SPEED_M_S: f64 = 1e-3;
    const QUIET_STEPS: usize = 3;
    let quiet = drape
        .samples
        .iter()
        .rev()
        .take_while(|s| s.solve.converged && s.max_speed_m_s <= SPEED_M_S)
        .count();
    let gravity_simulated = drape.steps > 0;
    let settled = gravity_simulated.then_some(quiet >= QUIET_STEPS);
    report["numerical"] = json!({
        "step_acceptance":"every Newton solve converged; stalled steps are refused without advancing the accepted state",
        "newton_correction_speed_tolerance_m_s":opts.tol,
        "time_step_s":opts.step,
        "velocity_tolerance_m_s":SPEED_M_S,
        "required_quiet_steps":QUIET_STEPS,
        "consecutive_quiet_steps":quiet,
        "gravity_simulated":gravity_simulated,
        "settled":settled,
        "sew":sew,
        "drape":drape,
        "basis":"Newton correction/h and instantaneous speed; reference settled remains the legacy window-drift metric, not static stress equilibrium or real fit"
    });
    if settled == Some(false) {
        let flag = format!(
            "not numerically settled (maximum speed {} m/s; need {QUIET_STEPS} consecutive steps <= {SPEED_M_S} m/s): rerun with more drape steps",
            drape.max_speed
        );
        for key in ["flags", "gating"] {
            if let Some(flags) = report[key].as_array_mut() {
                flags.push(json!(flag));
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
    let samples: Vec<_> = initial
        .into_iter()
        .chain(
            sew.samples
                .iter()
                .chain(&drape.samples)
                .filter_map(|s| s.contact.as_ref()),
        )
        .collect();
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
        "basis":"initial and accepted phase endpoints only; thickness-offset active-pair gaps, not an exact continuous-time minimum; no active pair means unavailable, not zero",
        "initial":initial,"accepted_samples":sew.steps+drape.steps,
        "minimum_sampled_active_body_gap_m":minimum(false),
        "minimum_sampled_active_self_gap_m":minimum(true),
        "friction_basis":"counts describe the frozen lag state, not a measured sticking/sliding verdict",
        "final_reactions":reactions});
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
    fold: &FoldUp,
    initial: &[Vec3],
    phase: &PhaseReport,
    opts: &DrapeOptions,
) {
    let start = FOLD_FRAMES + ZIP_FRAMES + opts.sew_steps;
    let initial = if opts.sew_steps == 0 {
        initial
    } else {
        &fold.frames[start - 1]
    };
    let frames = &fold.frames[start..];
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

fn add_run_fields(
    report: &mut Value,
    scene: &Scene,
    fabric: &Fabric,
    opts: &DrapeOptions,
    arm_deg: f64,
    pattern: &Pattern,
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
    let init: Vec<f64> = edge_strain(&scene.start.relaxed, &mesh.verts2d, &plain)
        .into_iter()
        .map(|e| e.abs() * 100.0)
        .collect();
    let style = pattern.raw()["style"].as_str().unwrap_or("");
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
            "p95": round(percentile(&init, 95.0), 1),
            "max": round(init.iter().copied().fold(0.0, f64::max), 1),
        }),
    );
    o.insert("presew".into(), json!(true));
    o.insert("arm_deg".into(), json!(arm_deg));
    o.insert(
        "gather_zone_vertices".into(),
        json!(w.iter().filter(|&&v| v < 1.0).count()),
    );
    o.insert("relax_iters".into(), json!(scene.start.relax_iterations));
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
    let explicit = pattern.raw()["meta"]["solved"]
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
    let payload = viewer_payload(
        &run.scene.mesh,
        &run.fold,
        &ViewerExtras {
            title: if title.is_empty() { "fold-up" } else { title },
            style,
            body: body.display,
            gating: run.gating(),
            advisory: run.advisory(),
            settled: run.report["settled"].as_bool().unwrap_or(false),
            arm_deg: body.arm_deg,
        },
        150,
    );
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
            });
            phase.steps += 1;
            phase.max_speed = speed;
        }
        let mut report = json!({"settled":true,"flags":[],"gating":[]});
        add_numerical_report(&mut report, &PhaseReport::default(), &phase, &options);
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
            });
            phase.steps += 1;
        }
        phase.max_speed = 0.0;
        let mut quiet = json!({"settled":true,"flags":[],"gating":[]});
        add_numerical_report(&mut quiet, &PhaseReport::default(), &phase, &options);
        assert_eq!(quiet["numerical"]["settled"], true);
        assert_eq!(quiet["numerical"]["consecutive_quiet_steps"], 3);
        assert_eq!(quiet["gating"], json!([]));
    }

    #[test]
    fn sewing_only_qualification_does_not_claim_gravity_settling() {
        let mut report = json!({"settled":true,"flags":[],"gating":[]});
        add_numerical_report(
            &mut report,
            &PhaseReport::default(),
            &PhaseReport::default(),
            &DrapeOptions::default(),
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
                self_contact: Some(ContactParams {
                    mu: -0.1,
                    ..default.contact
                }),
                ..default
            },
        ] {
            assert!(opts.validate().is_err(), "{opts:?}");
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
}
