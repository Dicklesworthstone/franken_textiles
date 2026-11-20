//! The fold-up in the page: a pattern draped on Anny fitted to the wearer's tape, the same pipeline as
//! `ftx drape` (tape fit, the drape pose, the trimmed collision skin, part frames, sewing then draping, the
//! fit report), with the body model read from bytes the page was given and the fold-up viewer page as text.

use ftx_body::{
    AnnyData, AnnyModel, BodyFrames, Legs, Pose, SkinningMode, TAPE_FIELDS, TapeFit, Wearer,
    clothing_skin, fit_tape, pose_deltas, skin_mesh, tape_targets,
};
use ftx_drape::{
    run::{DrapeBody, DrapeOptions, StaticEquilibriumOptions, drape, viewer},
    two_start::TwoStartOptions,
};
use ftx_pattern::Pattern;
use serde::{Deserialize, Serialize};
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
    if bytes.len() > 512 * 1024 * 1024 {
        return Err("body model exceeds 512 MiB".into());
    }
    AnnyData::from_bytes(bytes, true)
        .and_then(AnnyModel::from_data)
        .map_err(|r| r.message)
}

fn model_fit_records(fit: &TapeFit, measurements: &Value) -> Vec<Value> {
    ftx_measure::model_fit_records(
        measurements,
        TAPE_FIELDS.into_iter().zip(fit.readings),
        fit.residuals.iter().map(|r| r.name.as_str()),
    )
}

// Raw fitter outputs, in mm/kg. Supplied targets may be priors; predictions never become evidence.
fn body_report(wearer: &Wearer, fit: &TapeFit, measurements: &Value) -> Value {
    let tape: Map<String, Value> = fit
        .residuals
        .iter()
        .map(|r| {
            (
                r.name.clone(),
                json!({
                    "target_mm": r.target_mm, "body_mm": r.body_mm,
                    "residual_mm": r.body_mm - r.target_mm,
                    "sigma_mm": r.sigma_mm,
                    "effective_sigma_mm": r.effective_sigma_mm,
                    "normalized_residual": (r.body_mm - r.target_mm) / r.effective_sigma_mm,
                }),
            )
        })
        .collect();
    let readings: Map<String, Value> = TAPE_FIELDS
        .iter()
        .zip(fit.readings)
        .filter_map(|(name, value)| value.map(|value| (name.to_string(), json!(value))))
        .collect();
    let unavailable = fit.unavailable_fields();
    json!({"model":"anny 0.6.1", "provenance":"MODEL_FIT",
        "age_years":wearer.years,"gender":wearer.gender,"mass_kg":fit.mass_kg,
        "tape_evaluation":{"status":if unavailable.is_empty() { "AVAILABLE" } else { "PARTIAL" },
            "pose":Pose::TAPE_FIT_POSE_NAME,"provenance":"MODEL_FIT","unavailable_fields":unavailable},
        "readings_mm":readings,"fit":{"iterations":fit.iterations,"tape":tape,
            "weight_kg":fit.mass_comparison(wearer, measurements)},
        "no_claim":"Predicted body dimensions are MODEL_FIT. Supplied targets may include priors; residuals do not establish measurement readiness or physical fit."})
}

/// Check the selected body against current targets without a garment or cloth simulation.
/// The model and digest must come from the same array-verified input bytes.
///
/// # Errors
/// Excessive/invalid measurements, missing tape-fit age, refused fitting or an inconsistent imported
/// report. A refused accepted report never falls back to tape fitting.
pub fn inspect_body(
    model: &AnnyModel,
    model_sha256: &str,
    measurements: &str,
    accepted_report: Option<&str>,
) -> Result<Value, String> {
    if measurements.len() > 16 * 1024 * 1024 {
        return Err("body-check measurements exceed 16 MiB".into());
    }
    let mut data: Value = serde_json::from_str(measurements).map_err(|e| e.to_string())?;
    if !data.is_object() {
        return Err("body-check measurements must be an object".into());
    }
    // A tape-only target file can carry the ordinary schema label without original records.
    // With original records the fused block is only a cache. Imported shapes retain their
    // existing source-admission rules.
    let fused_only = accepted_report.is_none()
        && data["schema"] == "fabric-measurements/1"
        && data["fused"].is_object()
        && data["wearer"].is_object();
    if data.get("records").is_some() || (data.get("schema").is_some() && !fused_only) {
        let errors = ftx_measure::validate(&data);
        if !errors.is_empty() {
            return Err(format!("body-check measurements: {}", errors.join("; ")));
        }
        data = ftx_measure::fuse(&data, true).map_err(|e| e.message)?;
    }
    let measurements_sha256 = ftx_core::receipt::sha256_hex(measurements.as_bytes());
    let mut report = json!({"schema":"ftx-body-check/1","measurements_sha256":measurements_sha256,
        "cloth_simulated":false});
    let (mut body, params) = if let Some(report) = accepted_report {
        let (shape, description) =
            ftx_body::accepted_shape::prepare(model, model_sha256, report.as_bytes(), Some(&data))
                .map_err(|e| e.message)?;
        (Value::Object(description), shape.params)
    } else {
        if !data["wearer"]["age_months"].is_number() {
            return Err("the body check needs the wearer's age (months)".into());
        }
        let wearer = Wearer::from_measurements(&data).map_err(|e| e.message)?;
        let targets = tape_targets(&data).map_err(|e| e.message)?;
        let fit = fit_tape(model, &wearer, &targets, &Pose::drape()).map_err(|e| e.message)?;
        let records = model_fit_records(&fit, &data);
        if !records.is_empty() {
            // Replaying predictions alone must never turn the source's stature into new evidence.
            report["model_fit_measurements"] = json!({"schema":"fabric-measurements/1",
                "wearer":{"reference":crate::page::truthy(&data["wearer"]["reference"])},
                "source_measurements_sha256":measurements_sha256,"model_sha256":model_sha256,
                "records":records});
        }
        (body_report(&wearer, &fit, &data), fit.params)
    };
    if let Some(occupied) =
        ftx_body::nappy_occupancy(model, &params, &data).map_err(|e| e.message)?
    {
        body["readings_mm"]["seat_over_nappy"] = json!(occupied.seat_over_nappy_mm);
        body["nappy_occupancy"] = occupied.description;
    }
    body["model_sha256"] = json!(model_sha256);
    report["body"] = body;
    Ok(report)
}

/// Drapes `pattern` on the fitted `model`. `self_contact_mm` sets the offset between cloth surfaces.
/// A zero offset still enforces contact. Progress lines go to `progress`.
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
    two_start: bool,
    progress: impl FnMut(&str),
) -> Result<Fold, String> {
    fold_on_source(
        model,
        pattern,
        measurements,
        fit_spec,
        FoldSettings {
            self_contact_mm,
            two_start,
            ..FoldSettings::default()
        },
        None,
        progress,
    )
}

/// Explicit imported shape and ordinary drape-pose choice; image-frame poses are never reused.
pub struct BodyFitSource<'a> {
    /// SHA-256 of the same array-verified model being evaluated.
    pub model_sha256: &'a str,
    /// Original saved report text; its exact bytes remain the source authority.
    pub report: &'a str,
    /// Ordinary drape arm abduction in degrees, independent of image-frame poses.
    pub arm_deg: f64,
}

/// Cloth controls shared by tape-fitted and explicitly accepted body sources.
#[derive(Clone, Copy, Debug)]
pub struct FoldOptions {
    /// Cloth self-contact offset in millimetres. Zero retains the crossing barrier.
    pub self_contact_mm: f64,
    /// Compare the default second sewing duration at the same placement.
    pub two_start: bool,
}

/// serde predicate for the default skinning mode (borrowed form).
fn skinning_is_lbs(mode: &SkinningMode) -> bool {
    mode.is_lbs()
}

/// Explicit garment pose and bounded continuation, shared by native and browser adapters.
/// Defaults preserve the ordinary standing drape with no extra settling steps.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FoldSettings {
    /// Cloth self-contact offset in millimetres. Zero retains the crossing barrier.
    pub self_contact_mm: f64,
    /// Compare the default second sewing duration at the same placement.
    pub two_start: bool,
    /// Arm angle independent of the leg posture and of the tape-fit reference pose.
    pub arm_abduction_deg: f64,
    /// Standing, seated or squat leg rotations on the same fitted shape.
    pub legs: Legs,
    /// Fraction of the selected leg rotations, from standing (0) to the full posture (1).
    pub leg_fraction: f64,
    /// Skinning operator (`"lbs"` default; `"dqs"` opt-in dual-quaternion linear blending).
    #[serde(default, skip_serializing_if = "skinning_is_lbs")]
    pub skinning: SkinningMode,
    /// Maximum extra gravity steps, stopping early only at the shared quiet-window criterion.
    pub max_settle_steps: usize,
    /// Optional nominal step duration in seconds (omitted when using the default `0.01 s`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_s: Option<f64>,
    /// Optional explicit sewing step count (omitted when using the default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sew_steps: Option<usize>,
    /// Optional explicit gravity drape step count (omitted when using the default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drape_steps: Option<usize>,
    /// Optional Newton convergence velocity tolerance in m/s (omitted when using the default `1e-3`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newton_tol_m_s: Option<f64>,
    /// Independent frictionless static comparison, with no change to the dynamic fold or its time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_equilibrium: Option<StaticEquilibriumOptions>,
    /// Explicit convergence-driven time bisection, sharing one trial budget across the whole drape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_subdivision: Option<ftx_drape::stepping::SubdivisionOptions>,
    /// Nonlinear globalization and linear-solve strategy (omitted when legacy `ProjectedNewton`).
    #[serde(default, skip_serializing_if = "ftx_drape::NewtonStrategy::is_default")]
    pub newton_strategy: ftx_drape::NewtonStrategy,
    /// Structural time-integration scheme (omitted when legacy `ImplicitEuler`).
    #[serde(default, skip_serializing_if = "ftx_drape::TimeIntegrator::is_default")]
    pub integrator: ftx_drape::TimeIntegrator,
}

impl Default for FoldSettings {
    fn default() -> Self {
        Self {
            self_contact_mm: 2.0,
            two_start: false,
            arm_abduction_deg: Pose::A_POSE_DEG,
            legs: Legs::Standing,
            leg_fraction: 1.0,
            skinning: SkinningMode::LinearBlend,
            max_settle_steps: 0,
            step_s: None,
            sew_steps: None,
            drape_steps: None,
            newton_tol_m_s: None,
            static_equilibrium: None,
            time_subdivision: None,
            newton_strategy: ftx_drape::NewtonStrategy::ProjectedNewton,
            integrator: ftx_drape::TimeIntegrator::ImplicitEuler,
        }
    }
}

/// Drape the exact accepted shape without tape fitting. The model must be array-verified.
///
/// # Errors
/// Invalid report/model/wearer identity, excessive input, invalid pose or refused cloth solve.
#[allow(clippy::too_many_arguments)] // The ordinary fold inputs plus the explicit accepted-shape source.
pub fn fold_up_with_body_fit(
    model: &AnnyModel,
    pattern: &Value,
    measurements: &Value,
    fit_spec: &Value,
    options: FoldOptions,
    source: &BodyFitSource<'_>,
    progress: impl FnMut(&str),
) -> Result<Fold, String> {
    fold_on_source(
        model,
        pattern,
        measurements,
        fit_spec,
        FoldSettings {
            self_contact_mm: options.self_contact_mm,
            two_start: options.two_start,
            arm_abduction_deg: source.arm_deg,
            ..FoldSettings::default()
        },
        Some(source),
        progress,
    )
}

/// Drape the selected shape with explicit pose and settling settings. If supplied, the accepted
/// source's arm angle must match the configured pose; its exact report remains authoritative.
///
/// # Errors
/// Invalid settings, an inconsistent accepted source, unavailable geometry or a refused solve.
pub fn fold_up_configured(
    model: &AnnyModel,
    pattern: &Value,
    measurements: &Value,
    fit_spec: &Value,
    settings: FoldSettings,
    source: Option<&BodyFitSource<'_>>,
    progress: impl FnMut(&str),
) -> Result<Fold, String> {
    if source.is_some_and(|source| source.arm_deg != settings.arm_abduction_deg) {
        return Err("accepted source arm angle must match the configured garment pose".into());
    }
    fold_on_source(
        model,
        pattern,
        measurements,
        fit_spec,
        settings,
        source,
        progress,
    )
}

fn validate_browser_schedule(
    options: &DrapeOptions,
    comparison: Option<&TwoStartOptions>,
) -> Result<(), String> {
    let longest_sewing = comparison.map_or(options.sew_steps, |policy| {
        options.sew_steps.max(policy.alternate_sew_steps)
    });
    if longest_sewing
        .saturating_add(options.drape_steps)
        .saturating_add(options.max_settle_steps)
        > 512
    {
        return Err("configured fold exceeds the 512-step browser work budget per sewing path (sew + drape + settle)".into());
    }
    match comparison {
        Some(policy) => policy.validate(options),
        None => options.validate(),
    }
    .map_err(|reason| reason.to_string())
}

#[allow(clippy::too_many_arguments)] // Shared adapter preserves the same inputs for both shape sources.
fn fold_on_source(
    model: &AnnyModel,
    pattern: &Value,
    measurements: &Value,
    fit_spec: &Value,
    options: FoldSettings,
    source: Option<&BodyFitSource<'_>>,
    mut progress: impl FnMut(&str),
) -> Result<Fold, String> {
    let FoldSettings {
        self_contact_mm,
        two_start,
        arm_abduction_deg,
        legs,
        leg_fraction,
        skinning,
        max_settle_steps,
        step_s,
        sew_steps,
        drape_steps,
        newton_tol_m_s,
        static_equilibrium,
        time_subdivision,
        newton_strategy,
        integrator,
    } = options;
    let m = |r: ftx_core::Refusal| r.to_string();
    let mut pose = Pose::drape();
    pose.arm_abduction_deg = Some(arm_abduction_deg);
    pose.legs = legs;
    pose.leg_fraction = leg_fraction;
    pose.validate().map_err(m)?;
    let pat = Pattern::new(pattern.clone());
    ftx_drape::garment::simulation_scope(&pat).map_err(m)?;
    if source.is_none() && !measurements["wearer"]["age_months"].is_number() {
        return Err("the fold-up fits a body of the wearer's age: fill in the age (months)".into());
    }
    if !self_contact_mm.is_finite() || self_contact_mm < 0.0 {
        return Err("self-contact thickness must be finite and nonnegative".into());
    }
    let defaults = DrapeOptions::default();
    let mut opts = DrapeOptions {
        step: step_s.unwrap_or(defaults.step),
        sew_steps: sew_steps.unwrap_or(defaults.sew_steps),
        drape_steps: drape_steps.unwrap_or(defaults.drape_steps),
        tol: newton_tol_m_s.unwrap_or(defaults.tol),
        max_settle_steps,
        static_equilibrium,
        time_subdivision,
        newton_strategy,
        integrator,
        ..defaults
    };
    opts.self_contact = Some(ftx_drape::contact::ContactParams {
        thickness: self_contact_mm * 1e-3,
        ..opts.contact
    });
    let comparison = two_start.then(TwoStartOptions::default);
    validate_browser_schedule(&opts, comparison.as_ref())?;
    let material = ftx_drape::material::resolve(&pat, &opts).map_err(m)?;
    // Occupancy and the bare fitter must consume the same current evidence, including in direct
    // portable calls that have not passed through a saved round.
    let data = if measurements.get("records").is_some()
        || (source.is_some() && measurements.get("schema").is_some())
    {
        let errors = ftx_measure::validate(measurements);
        if !errors.is_empty() {
            return Err(format!("fold-up measurements: {}", errors.join("; ")));
        }
        ftx_measure::fuse(measurements, true).map_err(m)?
    } else {
        measurements.clone()
    };
    let (params, mut description) = if let Some(source) = source {
        let (shape, mut description) = ftx_body::accepted_shape::prepare(
            model,
            source.model_sha256,
            source.report.as_bytes(),
            Some(&data),
        )
        .map_err(m)?;
        description.insert(
            "pose".into(),
            json!(format!(
                "drape: arms at {}° and straight, hands and feet left out of contact",
                arm_abduction_deg
            )),
        );
        progress("body: reusing accepted image-fit shape without tape refitting");
        (shape.params, Value::Object(description))
    } else {
        let wearer = Wearer::from_measurements(&data).map_err(m)?;
        let targets = tape_targets(&data).map_err(m)?;
        let fit = fit_tape(model, &wearer, &targets, &Pose::drape()).map_err(m)?;
        progress(&format!(
            "body: fitted in {} iterations, {:.2} kg",
            fit.iterations, fit.mass_kg
        ));
        let mut description = body_report(&wearer, &fit, &data);
        description["model_fit"] = Value::Array(model_fit_records(&fit, &data));
        (fit.params, description)
    };
    if !legs.is_standing() {
        description["legs"] = json!(legs);
    }
    if leg_fraction != 1.0 {
        description["leg_fraction"] = json!(leg_fraction);
    }
    if !skinning.is_lbs() {
        description["skinning"] = json!(skinning);
    }
    if source.is_none() && arm_abduction_deg != Pose::A_POSE_DEG {
        description["pose"] = json!(format!(
            "drape: arms at {arm_abduction_deg}° and straight, hands and feet left out of contact"
        ));
    }
    let occupancy = ftx_body::nappy_occupancy(model, &params, &data).map_err(m)?;
    let occupied_params = occupancy
        .as_ref()
        .map_or(&params, |occupied| &occupied.params);
    let rest = model.rest(occupied_params).map_err(m)?;
    if let Some(occupied) = occupancy {
        description["readings_mm"]["seat_over_nappy"] = json!(occupied.seat_over_nappy_mm);
        description["nappy_occupancy"] = occupied.description;
    }
    let rest_skin = clothing_skin(model, &rest.vertices).map_err(m)?;
    let posed = model
        .pose_with_skinning(
            &rest,
            &pose_deltas(model, &rest, &pose, 1.0).map_err(m)?,
            skinning,
        )
        .map_err(m)?;
    let skin = clothing_skin(model, &posed.vertices).map_err(m)?;
    let full = skin_mesh(&posed.vertices, &model.data().faces).map_err(m)?;
    let frames = BodyFrames::new(model, &posed).map_err(m)?;
    let body = DrapeBody {
        frames: &frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: pose.arm_abduction_deg.unwrap_or(Pose::A_POSE_DEG),
    };
    let run = if let Some(policy) = comparison {
        ftx_drape::two_start::drape(&pat, fit_spec, &body, &opts, &policy, &mut progress)
    } else {
        drape(&pat, fit_spec, &body, &opts, &mut progress)
    }
    .map_err(m)?;
    finalize_browser_fold(
        run,
        &pat,
        &body,
        &material,
        description,
        skin.contact_proxy_basis_with_rest_and_skinning(&rest_skin, skinning),
    )
}

fn finalize_browser_fold(
    mut run: ftx_drape::run::DrapeRun,
    pat: &Pattern,
    body: &DrapeBody<'_>,
    material: &ftx_drape::material::MaterialInputs,
    description: Value,
    contact_proxy_basis: Value,
) -> Result<Fold, String> {
    let m = |r: ftx_core::Refusal| r.message;
    run.report["body"] = description;
    run.report["body_contact_proxy"] = contact_proxy_basis;
    run.report["qualification_basis"] =
        ftx_drape::qualify::qualification_basis(body, material).map_err(m)?;
    ftx_drape::qualify::gate_tightness(
        &mut run.report,
        &ftx_drape::qualify::QualificationStatus::unqualified(
            "browser fold-up has no workspace qualification run (`ftx qualify`)",
        ),
    );
    let html = viewer(&run, pat, body, TEMPLATE)?;
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
    fn configured_fold_retains_bounded_time_subdivision_policy() {
        let mut value = serde_json::to_value(FoldSettings::default()).unwrap();
        assert!(value.get("time_subdivision").is_none());
        value["time_subdivision"] = json!({"max_halvings":3,"max_trials":200});
        let settings: FoldSettings = serde_json::from_value(value.clone()).unwrap();
        let options = DrapeOptions {
            time_subdivision: settings.time_subdivision,
            ..DrapeOptions::default()
        };
        options.validate().unwrap();
        assert_eq!(serde_json::to_value(settings).unwrap(), value);
        assert_eq!(options.step, DrapeOptions::default().step);
        value["time_subdivision"]["tolerance"] = json!(1.0);
        assert!(serde_json::from_value::<FoldSettings>(value).is_err());
    }

    #[test]
    fn configured_fold_preserves_explicit_static_distance_tolerance_without_changing_time() {
        let default = serde_json::to_value(FoldSettings::default()).unwrap();
        assert!(default.get("static_equilibrium").is_none());
        let mut configured = default.clone();
        configured["static_equilibrium"] = json!({
            "correction_tolerance_m":2e-6, "max_newton_iterations":120,
        });
        let settings: FoldSettings = serde_json::from_value(configured.clone()).unwrap();
        settings.static_equilibrium.unwrap().validate().unwrap();
        assert_eq!(serde_json::to_value(settings).unwrap(), configured);
        configured["static_equilibrium"]["time_s"] = json!(1.0);
        assert!(serde_json::from_value::<FoldSettings>(configured).is_err());
        assert!(
            serde_json::from_value::<FoldSettings>(default)
                .unwrap()
                .static_equilibrium
                .is_none()
        );
    }

    #[test]
    fn browser_step_budget_covers_both_paths_including_settling() {
        let comparison = TwoStartOptions::default();
        let mut options = DrapeOptions {
            sew_steps: 12,
            drape_steps: 493,
            max_settle_steps: 1,
            ..DrapeOptions::default()
        };
        assert!(validate_browser_schedule(&options, Some(&comparison)).is_ok());
        options.drape_steps += 1;
        assert!(validate_browser_schedule(&options, None).is_ok());
        assert!(
            validate_browser_schedule(&options, Some(&comparison))
                .unwrap_err()
                .contains("512")
        );
        options.sew_steps = usize::MAX;
        assert!(
            validate_browser_schedule(&options, None)
                .unwrap_err()
                .contains("512")
        );
        options.drape_steps = 60;
        options.sew_steps = comparison.alternate_sew_steps;
        assert!(
            validate_browser_schedule(&options, Some(&comparison))
                .unwrap_err()
                .contains("must differ")
        );
    }

    #[test]
    fn configured_fold_retains_opt_in_dqs_skinning_and_omits_default_lbs() {
        let default = serde_json::to_value(FoldSettings::default()).unwrap();
        assert!(default.get("skinning").is_none());
        let parsed_default: FoldSettings = serde_json::from_value(default.clone()).unwrap();
        assert_eq!(parsed_default.skinning, SkinningMode::LinearBlend);

        let mut explicit_lbs = default.clone();
        explicit_lbs["skinning"] = json!("lbs");
        let parsed_lbs: FoldSettings = serde_json::from_value(explicit_lbs).unwrap();
        assert_eq!(parsed_lbs.skinning, SkinningMode::LinearBlend);
        assert_eq!(serde_json::to_value(parsed_lbs).unwrap(), default);

        let mut configured_dqs = default.clone();
        configured_dqs["skinning"] = json!("dqs");
        let parsed_dqs: FoldSettings = serde_json::from_value(configured_dqs.clone()).unwrap();
        assert_eq!(parsed_dqs.skinning, SkinningMode::DualQuaternion);
        assert_eq!(serde_json::to_value(parsed_dqs).unwrap(), configured_dqs);

        let mut invalid = default;
        invalid["skinning"] = json!("bogus");
        assert!(serde_json::from_value::<FoldSettings>(invalid).is_err());
    }

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
            readings: std::array::from_fn(|i| {
                if i == 16 {
                    None
                } else {
                    Some(100.0004 + i as f64)
                }
            }),
            mass_kg: 10.0003,
            residuals: vec![
                TapeResidual {
                    name: "chest".into(),
                    target_mm: 102.0,
                    body_mm: 100.0004,
                    sigma_mm: Some(12.0),
                    effective_sigma_mm: 12.0,
                },
                TapeResidual {
                    name: "arm_length".into(),
                    target_mm: 200.0,
                    body_mm: 200.0000000003,
                    sigma_mm: None,
                    effective_sigma_mm: 2.0,
                },
            ],
            free: std::array::from_fn(|_| 0.0),
            iterations: 7,
        };
        let got = body_report(
            &wearer,
            &fit,
            &json!({"wearer":{"weight_kg":wearer.weight_kg}}),
        );
        assert_eq!(got["provenance"], "MODEL_FIT");
        assert_eq!(got["fit"]["iterations"], 7);
        for r in &fit.residuals {
            let row = &got["fit"]["tape"][&r.name];
            assert_eq!(row["target_mm"], r.target_mm);
            assert_eq!(row["body_mm"], r.body_mm);
            assert_eq!(row["residual_mm"], r.body_mm - r.target_mm);
            assert_eq!(row["sigma_mm"], json!(r.sigma_mm));
            assert_eq!(row["effective_sigma_mm"], r.effective_sigma_mm);
            assert_eq!(
                row["normalized_residual"],
                (r.body_mm - r.target_mm) / r.effective_sigma_mm
            );
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
            TAPE_FIELDS.len() - 1
        );
        assert_eq!(got["tape_evaluation"]["status"], "PARTIAL");
        assert_eq!(
            got["tape_evaluation"]["unavailable_fields"],
            json!(["head"])
        );
        for (name, value) in TAPE_FIELDS.iter().zip(fit.readings) {
            if let Some(value) = value {
                assert_eq!(got["readings_mm"][name], value);
            } else {
                assert!(got["readings_mm"].get(name).is_none());
            }
        }
        assert_eq!(
            got["fit"]["weight_kg"]["residual_kg"],
            fit.mass_kg - 10.0001
        );
        let records = model_fit_records(
            &fit,
            &json!({"wearer":{"stature_mm":1500,"age_months":120}}),
        );
        assert_eq!(records.len(), TAPE_FIELDS.len() - 3);
        assert!(records.iter().all(|r| {
            r["sigma_mm"] == 12.0
                && r["method"] == "ftx body: Anny 0.6.1 fitted to 2 tape measurements"
                && r["notes"].as_str().unwrap().contains("no chart prior")
                && !["head", "arm_length", "chest"].contains(&r["name"].as_str().unwrap())
        }));
        wearer.weight_kg = None;
        assert!(body_report(&wearer, &fit, &json!({}))["fit"]["weight_kg"].is_null());
    }

    #[test]
    fn browser_fold_demotes_unqualified_tightness_in_report_and_viewer() {
        use ftx_core::Vec3;
        use ftx_drape::{PartFrame, scene::PartFrames};

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
                Some(10.0)
            }
            fn section_radius(&self, _: &str, _: f64) -> f64 {
                30.0
            }
        }

        let mut verts = Vec::new();
        for z in [-0.02, 0.08] {
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
        };
        let w = (0.94 * 0.06 * std::f64::consts::PI * 1000.0).round();
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
        let fit_spec = json!({
            "schema": "fabric-fit/1",
            "points": [{"id": "chest", "kind": "girth", "instances": ["band"], "landmark": "chest", "body": "chest", "target_mm": [-20, 0, 20]}]
        });
        let opts = DrapeOptions {
            sew_steps: 20,
            drape_steps: 0,
            ..DrapeOptions::default()
        };
        let material = ftx_drape::material::resolve(&pat, &opts).unwrap();
        let run = drape(&pat, &fit_spec, &body, &opts, |_| {}).unwrap();
        let tight_flag = run.report["tightness_gating"][0]
            .as_str()
            .expect("tight ring should record tightness_gating")
            .to_string();
        assert!(
            run.report["gating"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f.as_str() == Some(&tight_flag))
        );

        let folded =
            finalize_browser_fold(run, &pat, &body, &material, json!({}), json!({})).unwrap();
        assert_eq!(folded.report["tightness_qualified"], false);
        assert_eq!(folded.report["collider_tightness_qualified"], false);
        assert!(
            !folded.report["gating"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f.as_str() == Some(&tight_flag))
        );
        assert!(
            folded.report["flags"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f.as_str() == Some(&tight_flag))
        );
        let (_, rest) = folded.viewer.split_once("\nconst D = ").unwrap();
        let (escaped, _) = rest.split_once(";\nconst $ = ").unwrap();
        let embedded: Value = serde_json::from_str(&escaped.replace("<\\/", "</")).unwrap();
        assert_eq!(embedded["report"]["gating"], folded.report["gating"]);
        assert!(
            embedded["report"]["advisory"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f.as_str() == Some(&tight_flag))
        );
    }

    #[test]
    fn fold_settings_and_sewn_band_drape_support_new_solver_strategies_and_generalized_alpha() {
        use ftx_core::Vec3;
        use ftx_drape::{NewtonStrategy, PartFrame, TimeIntegrator, scene::PartFrames};

        // Default FoldSettings omits newton_strategy, integrator, and explicit schedule overrides.
        let def_val = serde_json::to_value(FoldSettings::default()).unwrap();
        assert!(def_val.get("newton_strategy").is_none());
        assert!(def_val.get("integrator").is_none());
        assert!(def_val.get("step_s").is_none());
        assert!(def_val.get("sew_steps").is_none());
        assert!(def_val.get("drape_steps").is_none());
        assert!(def_val.get("newton_tol_m_s").is_none());

        let mut custom = def_val;
        custom["step_s"] = json!(0.001);
        custom["sew_steps"] = json!(40);
        custom["drape_steps"] = json!(60);
        custom["newton_tol_m_s"] = json!(1e-5);
        custom["newton_strategy"] =
            json!({"kind": "adaptive_ilu", "pcg_iters": 64, "max_shifts": 4});
        custom["integrator"] = json!({"kind": "generalized_alpha", "rho_inf": 0.8});
        let parsed: FoldSettings = serde_json::from_value(custom.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), custom);
        assert_eq!(
            (
                parsed.step_s,
                parsed.sew_steps,
                parsed.drape_steps,
                parsed.newton_tol_m_s
            ),
            (Some(0.001), Some(40), Some(60), Some(1e-5))
        );

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

        // Default run omits newton_strategy/integrator from report["numerical"].
        let base_opts = DrapeOptions {
            sew_steps: 4,
            drape_steps: 6,
            step: 0.01,
            ..DrapeOptions::default()
        };
        let base = drape(&pat, &json!({}), &body, &base_opts, |_| {}).unwrap();
        assert!(base.report["numerical"].get("newton_strategy").is_none());
        assert!(base.report["numerical"].get("integrator").is_none());

        // AdaptiveIlu completes real sewn garment drape cleanly.
        let ilu_opts = DrapeOptions {
            newton_strategy: NewtonStrategy::adaptive_ilu_default(),
            ..base_opts
        };
        let ilu = drape(&pat, &json!({}), &body, &ilu_opts, |_| {})
            .expect("AdaptiveIlu + ImplicitEuler should converge");
        assert_eq!(
            ilu.report["numerical"]["newton_strategy"]["kind"],
            "adaptive_ilu"
        );
        assert!(ilu.report["numerical"].get("integrator").is_none());

        // AdaptiveIlu + GeneralizedAlpha(0.0) (L-stable 2nd-order IPC default) completes both
        // zero-gravity sew and gravity drape phases with consistent acceleration reset.
        let ga_opts = DrapeOptions {
            newton_strategy: NewtonStrategy::adaptive_ilu_default(),
            integrator: TimeIntegrator::GeneralizedAlpha { rho_inf: 0.0 },
            ..base_opts
        };
        let ga = drape(&pat, &json!({}), &body, &ga_opts, |_| {})
            .expect("AdaptiveIlu + GeneralizedAlpha(0.0) should converge");
        assert_eq!(
            ga.report["numerical"]["newton_strategy"]["kind"],
            "adaptive_ilu"
        );
        assert_eq!(
            ga.report["numerical"]["integrator"]["kind"],
            "generalized_alpha"
        );
        assert_eq!(ga.drape.steps, 6);

        // The lightly damped band needs a finer interval from its initial prestressed state.
        // Keep the original physical durations and audit the sewn start and every accepted frame.
        let fine_opts = DrapeOptions {
            step: 0.001,
            tol: 1e-7,
            sew_steps: 40,
            drape_steps: 60,
            integrator: parsed.integrator,
            newton_strategy: parsed.newton_strategy,
            self_contact: Some(ftx_drape::contact::ContactParams {
                thickness: 0.002,
                ..base_opts.contact
            }),
            ..base_opts
        };
        let fine = drape(&pat, &json!({}), &body, &fine_opts, |_| {}).unwrap();
        assert_eq!((fine.sew.steps, fine.drape.steps), (40, 60));
        assert_eq!(fine.report["gating"], json!([]));
        for sample in fine.sew.samples.iter().chain(&fine.drape.samples) {
            assert!(sample.solve.converged && sample.solve.residual < 1e-3);
        }
        assert!(fine.drape.samples.last().unwrap().solve.residual < 1e-6);
        let first = fine.fold.fold + fine.fold.zip - 1;
        let physical = &fine.fold.frames[first..];
        assert_eq!(physical.len(), 101);
        let mut audit = ftx_drape::animation::ReplayContact::new(&verts, &tris).unwrap();
        for (i, positions) in physical.iter().enumerate() {
            audit
                .check(positions, &fine.scene.mesh.faces, first + i)
                .unwrap();
        }

        // TrustRegionSteihaug completes real sewn garment drape factorization-free.
        let tr_opts = DrapeOptions {
            newton_strategy: NewtonStrategy::trust_region_default(),
            ..base_opts
        };
        let tr = drape(&pat, &json!({}), &body, &tr_opts, |_| {})
            .expect("TrustRegionSteihaug should converge");
        assert_eq!(
            tr.report["numerical"]["newton_strategy"]["kind"],
            "trust_region_steihaug"
        );
        assert_eq!(tr.sew.factorizations, 0);
        assert_eq!(tr.drape.factorizations, 0);
    }

    #[test]
    fn portable_microclimate_and_lower_visco_match_native_bytes() {
        let micro_req = json!({
            "schema": ftx_drape::thermal::microclimate::MICROCLIMATE_REQUEST_SCHEMA,
            "air": { "temperature_c": 20.0, "pressure_pa": 101325.0, "relative_humidity": 0.5 },
            "convection": {
                "correlation": "churchill_chu_vertical_plate",
                "characteristic_length_m": 0.30,
                "delta_t_k": 10.0,
                "surface_temperature_range_c": [15.0, 40.0],
                "allow_estimates": true
            }
        })
        .to_string();
        assert_eq!(
            crate::thermal::microclimate(&micro_req).unwrap(),
            ftx_drape::thermal::microclimate::evaluate_json(&micro_req).unwrap()
        );

        let visco_req = json!({
            "schema": ftx_pattern::material_relaxation::exact_exp::LOWERING_REQUEST_SCHEMA,
            "e0": 200.0,
            "e_inf": 500.0,
            "alpha": 0.35,
            "tau_s": 1.0,
            "f_lo_hz": 0.01,
            "f_hi_hz": 100.0,
            "n_terms": 14,
            "rel_tol": 0.02
        })
        .to_string();
        assert_eq!(
            crate::materials::lower_visco(&visco_req).unwrap(),
            ftx_pattern::material_relaxation::exact_exp::evaluate_lowering_json(&visco_req)
                .unwrap()
        );
    }
}
