//! The fold-up in the page: a pattern draped on Anny fitted to the wearer's tape, the same pipeline as
//! `ftx drape` (tape fit, the drape pose, the trimmed collision skin, part frames, sewing then draping, the
//! fit report), with the body model read from bytes the page was given and the fold-up viewer page as text.

use ftx_body::{
    AnnyData, AnnyModel, BodyFrames, Pose, Wearer, fit_tape, pose_deltas, skin_mesh, tape_targets,
    trimmed_skin,
};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape, viewer};
use ftx_pattern::Pattern;
use serde_json::Value;

/// The viewer page's template (Three.js from a CDN, the fold-up's frames embedded).
const TEMPLATE: &str = include_str!("../../../assets/fold_viewer_template.html");

/// A finished fold-up: `sim_report.json`'s content and the viewer page.
#[derive(Clone, Debug)]
pub struct Fold {
    /// The fit report (gating flags, advisories, settledness, hems, bands, seams, self-contact).
    pub report: Value,
    /// The fold-up viewer page (HTML).
    pub viewer: String,
}

/// Reads a converted body model (`.ftxbody`) from memory.
///
/// # Errors
/// Not a converted model, or a malformed one.
pub fn load_body(bytes: &[u8]) -> Result<AnnyModel, String> {
    AnnyData::from_bytes(bytes, false)
        .and_then(AnnyModel::from_data)
        .map_err(|r| r.message)
}

/// Drapes `pattern` on `model` fitted to `measurements`, judged by `fit_spec`. `self_contact_mm` > 0 turns
/// on cloth self-contact at that thickness (the skill passes Blender's 2 mm); progress lines go to
/// `progress`.
///
/// # Errors
/// A wearer or tape the fit cannot use, a pattern the drape refuses (no fold-up placement, an
/// intersecting start), a viewer template without its placeholder.
pub fn fold_up(
    model: &AnnyModel,
    pattern: &Value,
    measurements: &Value,
    fit_spec: &Value,
    self_contact_mm: f64,
    mut progress: impl FnMut(&str),
) -> Result<Fold, String> {
    let m = |r: ftx_core::Refusal| r.message;
    let wearer = Wearer::from_measurements(measurements).map_err(m)?;
    let pose = Pose::drape();
    let fit = fit_tape(model, &wearer, &tape_targets(measurements), &pose).map_err(m)?;
    progress(&format!(
        "body: fitted in {} iterations, {:.2} kg",
        fit.iterations, fit.mass_kg
    ));
    let rest = model.rest(&fit.params).map_err(m)?;
    let posed = model
        .pose(&rest, &pose_deltas(model, &rest, &pose, 1.0).map_err(m)?)
        .map_err(m)?;
    let skin = trimmed_skin(
        model,
        &posed.vertices,
        &["wrist.R", "wrist.L", "foot.R", "foot.L"],
    )
    .map_err(m)?;
    let full = skin_mesh(&posed.vertices, &model.data().faces).map_err(m)?;
    let frames = BodyFrames::new(model, &posed).map_err(m)?;
    let pat = Pattern::new(pattern.clone());
    let body = DrapeBody {
        frames: &frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: pose.arm_abduction_deg.unwrap_or(Pose::A_POSE_DEG),
    };
    let mut opts = DrapeOptions::default();
    if self_contact_mm > 0.0 {
        let mut p = opts.contact;
        p.thickness = self_contact_mm * 1e-3;
        opts.self_contact = Some(p);
    }
    let run = drape(&pat, fit_spec, &body, &opts, &mut progress).map_err(m)?;
    let html = viewer(&run, &pat, &body, TEMPLATE)?;
    Ok(Fold {
        report: run.report,
        viewer: html,
    })
}
