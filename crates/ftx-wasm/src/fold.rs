//! The fold-up in the page: a pattern draped on Anny fitted to the wearer's tape, the same pipeline as
//! `ftx drape` (tape fit, the drape pose, the trimmed collision skin, part frames, sewing then draping, the
//! fit report), with the body model read from bytes the page was given and the fold-up viewer page as text.

use ftx_body::{
    AnnyData, AnnyModel, BodyFrames, Pose, TAPE_FIELDS, TapeFit, Wearer, fit_tape, pose_deltas,
    skin_mesh, tape_targets, trimmed_skin,
};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape, viewer};
use ftx_pattern::Pattern;
use serde_json::{Map, Value, json};

/// The viewer page's template (the pinned renderer and fold-up frames are embedded).
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

// Raw fitter outputs, in mm/kg. Supplied targets may be priors; predictions never become evidence.
fn body_report(wearer: &Wearer, fit: &TapeFit) -> Value {
    let tape: Map<String, Value> = fit
        .residuals
        .iter()
        .map(|r| {
            (
                r.name.clone(),
                json!({
                    "target_mm": r.target_mm, "body_mm": r.body_mm,
                    "residual_mm": r.body_mm - r.target_mm,
                }),
            )
        })
        .collect();
    let readings: Map<String, Value> = TAPE_FIELDS
        .iter()
        .zip(fit.readings)
        .map(|(name, value)| (name.to_string(), json!(value)))
        .collect();
    json!({"model":"anny 0.6.1", "provenance":"MODEL_FIT",
        "age_years":wearer.years,"gender":wearer.gender,"mass_kg":fit.mass_kg,
        "readings_mm":readings,"fit":{"iterations":fit.iterations,"tape":tape,
            "weight_kg":wearer.weight_kg.map(|target| json!({"target_kg":target,
                "body_kg":fit.mass_kg,"residual_kg":fit.mass_kg-target}))},
        "no_claim":"Predicted body dimensions are MODEL_FIT. Supplied targets may include priors; residuals do not establish measurement readiness or physical fit."})
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
    // the page's own words, before the body is fitted (the library would refuse later, in its terms)
    let placed = pattern["pieces"].as_array().is_some_and(|ps| {
        ps.iter()
            .any(|p| crate::page::truthy(&p["placement"]) && p["sim"] != false)
    });
    if !placed {
        return Err("no piece has a fold-up placement: the pattern is judged flat".into());
    }
    if !measurements["wearer"]["age_months"].is_number() {
        return Err("the fold-up fits a body of the wearer's age: fill in the age (months)".into());
    }
    if !self_contact_mm.is_finite() || self_contact_mm < 0.0 {
        return Err("self-contact thickness must be finite and nonnegative".into());
    }
    let pat = Pattern::new(pattern.clone());
    let mut opts = DrapeOptions::default();
    if self_contact_mm > 0.0 {
        let mut p = opts.contact;
        p.thickness = self_contact_mm * 1e-3;
        opts.self_contact = Some(p);
    }
    ftx_drape::material::resolve(&pat, &opts).map_err(m)?;
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
    let body = DrapeBody {
        frames: &frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: pose.arm_abduction_deg.unwrap_or(Pose::A_POSE_DEG),
    };
    let mut run = drape(&pat, fit_spec, &body, &opts, &mut progress).map_err(m)?;
    run.report["body"] = body_report(&wearer, &fit);
    let html = viewer(&run, &pat, &body, TEMPLATE)?;
    Ok(Fold {
        report: run.report,
        viewer: html,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_body::{BodyParams, TapeResidual};

    #[test]
    fn body_diagnostics_keep_raw_signed_residuals_and_unmeasured_predictions() {
        let mut wearer = Wearer {
            years: 2.5,
            gender: 0.5,
            stature_mm: 920.0,
            weight_kg: Some(10.0001),
        };
        let fit = TapeFit {
            params: BodyParams {
                phenotype: Default::default(),
                local_changes: vec![],
            },
            readings: std::array::from_fn(|i| 100.0004 + i as f64),
            mass_kg: 10.0003,
            residuals: vec![
                TapeResidual {
                    name: "chest".into(),
                    target_mm: 102.0,
                    body_mm: 100.0004,
                },
                TapeResidual {
                    name: "arm_length".into(),
                    target_mm: 200.0,
                    body_mm: 200.0000000003,
                },
            ],
            free: std::array::from_fn(|_| 0.0),
            iterations: 7,
        };
        let got = body_report(&wearer, &fit);
        assert_eq!(got["provenance"], "MODEL_FIT");
        assert_eq!(got["fit"]["iterations"], 7);
        for r in &fit.residuals {
            let row = &got["fit"]["tape"][&r.name];
            assert_eq!(row["target_mm"], r.target_mm);
            assert_eq!(row["body_mm"], r.body_mm);
            assert_eq!(row["residual_mm"], r.body_mm - r.target_mm);
        }
        assert!(got["fit"]["tape"]["chest"]["residual_mm"].as_f64().unwrap() < 0.0);
        assert!(
            got["fit"]["tape"]["arm_length"]["residual_mm"]
                .as_f64()
                .unwrap()
                > 0.0
        );
        assert_eq!(
            got["readings_mm"].as_object().unwrap().len(),
            TAPE_FIELDS.len()
        );
        for (name, value) in TAPE_FIELDS.iter().zip(fit.readings) {
            assert_eq!(got["readings_mm"][name], value);
        }
        assert_eq!(
            got["fit"]["weight_kg"]["residual_kg"],
            fit.mass_kg - 10.0001
        );
        wearer.weight_kg = None;
        assert!(body_report(&wearer, &fit)["fit"]["weight_kg"].is_null());
    }
}
