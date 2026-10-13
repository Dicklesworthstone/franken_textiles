//! One drape run end to end — what `ftx drape`, the examples and (later) the browser call: mesh the
//! pattern, place and start it on a body, sew and drape it, record the fold-up, and read the metrics and
//! flags into a `sim_report.json`-shaped report (spec 04 §6.11).

use ftx_body::BodyFrames;
use ftx_core::np::percentile;
use ftx_core::py::round;
use ftx_core::{Refusal, RefusalCode, Vec3};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

use crate::animation::{
    FOLD_FRAMES, FoldUp, SEAM_COLORS, ViewerExtras, ZIP_FRAMES, board_layout, fold_frames,
    viewer_html, viewer_payload, zip_frames,
};
use crate::contact::ContactParams;
use crate::garment::build_garment;
use crate::metrics::{MIN_FRAMES, MetricsInput, metrics};
use crate::nearest::NearestTree;
use crate::scene::{Fabric, PartFrames, PhaseReport, Scene, build_scene, run_phase};
use crate::start::{edge_strain, face_edges};

/// The body a run drapes on.
pub struct DrapeBody<'a> {
    /// Part frames, landmarks and section radii.
    pub frames: &'a BodyFrames,
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
    /// Time step (s).
    pub step: f64,
    /// Newton tolerance on ‖Δx‖∞/h (m/s).
    pub tol: f64,
    /// Compression stiffness as a fraction of tension, outside gather zones (inside them it falls to
    /// [`crate::start::GATHER_COMPRESSION`] at the gathered seam).
    pub compression: f64,
    /// Contact and friction.
    pub contact: ContactParams,
}

impl Default for DrapeOptions {
    /// 12 mm mesh, 12 sewing and 36 draping steps of 1/24 s, Newton to 1e-4 m/s, compression as stiff
    /// as tension outside gather zones (the reference's `tension = compression = E_t·L`, spec 04 §5.3),
    /// ĝ 1 mm, the cloth's mid-surface 1.5 mm off the body, μ 0.4.
    fn default() -> Self {
        Self {
            spacing: 12.0,
            sew_steps: 12,
            drape_steps: 36,
            step: 1.0 / 24.0,
            tol: 1e-4,
            compression: 1.0,
            contact: ContactParams {
                dhat: 1e-3,
                thickness: 1.5e-3,
                kappa: 100.0,
                mu: 0.4,
                eps_v: 1e-3,
            },
        }
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
    /// The fabric used.
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

/// Drapes `pattern` on `body`, calling `progress` with a line per stage.
///
/// # Errors
/// [`RefusalCode::OutOfRange`] for fewer than [`MIN_FRAMES`] steps in all; refusals from meshing,
/// placement, the start state, the cloth model, contact or the solver.
pub fn drape(
    pattern: &Pattern,
    fit_spec: &Value,
    body: &DrapeBody,
    opts: &DrapeOptions,
    mut progress: impl FnMut(&str),
) -> Result<DrapeRun, Refusal> {
    if opts.sew_steps + opts.drape_steps < MIN_FRAMES {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "{} sewing and {} draping steps: the fit metrics need at least {MIN_FRAMES} in all \
                 (settledness compares the means of the last frames)",
                opts.sew_steps, opts.drape_steps
            ),
        )
        .repair(format!(
            "ask for at least {MIN_FRAMES} steps (--sew, --drape)"
        )));
    }
    let mesh = build_garment(pattern, opts.spacing)?;
    let fabric = Fabric::of(pattern);
    let mut scene = build_scene(
        pattern,
        mesh,
        body.frames,
        body.collision.0,
        body.collision.1,
        fabric,
        opts.compression,
        opts.contact,
    )?;
    scene.solver.tol = opts.tol;
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
    let mut phases = Vec::new();
    for (name, steps, g) in [
        ("sew", opts.sew_steps, 0.0),
        ("drape", opts.drape_steps, 9.81),
    ] {
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
    let raw = pattern.raw();
    let level_y = |inst: &str, landmark: &str| -> Option<f64> {
        let i = pattern.instance(inst)?;
        let pl = &raw["pieces"][i.piece_index]["placement"];
        let part = pl["part"].as_str().unwrap_or("torso");
        let key = if part == "torso" {
            part.to_string()
        } else {
            format!("{part}_R")
        };
        let anchor = pl["y0_landmark"].as_str().unwrap_or("waist");
        Some(
            PartFrames::landmark_h(body.frames, &key, landmark)?
                - PartFrames::landmark_h(body.frames, &key, anchor)?
                + pl["y0"].as_f64().unwrap_or(0.0),
        )
    };
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
    add_run_fields(&mut report, &scene, &fabric, opts, body.arm_deg, pattern);
    Ok(DrapeRun {
        scene,
        fold,
        report,
        sew,
        drape,
        fabric,
    })
}

/// The run fields of spec 04 §6.11 that a run knows (the CLI adds file hashes and timing).
fn add_run_fields(
    report: &mut Value,
    scene: &Scene,
    fabric: &Fabric,
    opts: &DrapeOptions,
    arm_deg: f64,
    pattern: &Pattern,
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
