//! The fold-up in the page: a pattern draped on Anny fitted to the wearer's tape, the same pipeline as
//! `ftx drape` (tape fit, the drape pose, the trimmed collision skin, part frames, sewing then draping, the
//! fit report), with the body model read from bytes the page was given and the fold-up viewer page as text.

use ftx_body::{
    AnnyData, AnnyModel, BodyFrames, Legs, Pose, TAPE_FIELDS, TapeFit, Wearer, clothing_skin,
    fit_tape, pose_deltas, skin_mesh, tape_targets,
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
            "pose":"a_pose_35","provenance":"MODEL_FIT","unavailable_fields":unavailable},
        "readings_mm":readings,"fit":{"iterations":fit.iterations,"tape":tape,
            "weight_kg":wearer.weight_kg.map(|target| json!({"target_kg":target,
                "body_kg":fit.mass_kg,"residual_kg":fit.mass_kg-target}))},
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
    if data.get("records").is_some() || data.get("schema").is_some() {
        let errors = ftx_measure::validate(&data);
        if !errors.is_empty() {
            return Err(format!("body-check measurements: {}", errors.join("; ")));
        }
        data = ftx_measure::fuse(&data, true).map_err(|e| e.message)?;
    }
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
        (body_report(&wearer, &fit), fit.params)
    };
    if let Some(occupied) =
        ftx_body::nappy_occupancy(model, &params, &data).map_err(|e| e.message)?
    {
        body["readings_mm"]["seat_over_nappy"] = json!(occupied.seat_over_nappy_mm);
        body["nappy_occupancy"] = occupied.description;
    }
    body["model_sha256"] = json!(model_sha256);
    Ok(json!({"schema":"ftx-body-check/1", "body":body,
        "measurements_sha256":ftx_core::receipt::sha256_hex(measurements.as_bytes()),
        "cloth_simulated":false}))
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
    /// Cloth self-contact thickness in millimetres; zero disables self-contact.
    pub self_contact_mm: f64,
    /// Compare the default second sewing duration at the same placement.
    pub two_start: bool,
}

/// Explicit garment pose and bounded continuation, shared by native and browser adapters.
/// Defaults preserve the ordinary standing drape with no extra settling steps.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FoldSettings {
    /// Cloth self-contact thickness in millimetres; zero disables self-contact.
    pub self_contact_mm: f64,
    /// Compare the default second sewing duration at the same placement.
    pub two_start: bool,
    /// Arm angle independent of the leg posture and of the tape-fit reference pose.
    pub arm_abduction_deg: f64,
    /// Standing, seated or squat leg rotations on the same fitted shape.
    pub legs: Legs,
    /// Fraction of the selected leg rotations, from standing (0) to the full posture (1).
    pub leg_fraction: f64,
    /// Maximum extra gravity steps, stopping early only at the shared quiet-window criterion.
    pub max_settle_steps: usize,
    /// Independent frictionless static comparison, with no change to the dynamic fold or its time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_equilibrium: Option<StaticEquilibriumOptions>,
}

impl Default for FoldSettings {
    fn default() -> Self {
        Self {
            self_contact_mm: 0.0,
            two_start: false,
            arm_abduction_deg: Pose::A_POSE_DEG,
            legs: Legs::Standing,
            leg_fraction: 1.0,
            max_settle_steps: 0,
            static_equilibrium: None,
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
        max_settle_steps,
        static_equilibrium,
    } = options;
    let m = |r: ftx_core::Refusal| r.message;
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
    let mut opts = DrapeOptions {
        max_settle_steps,
        static_equilibrium,
        ..DrapeOptions::default()
    };
    if self_contact_mm > 0.0 {
        let mut p = opts.contact;
        p.thickness = self_contact_mm * 1e-3;
        opts.self_contact = Some(p);
    }
    opts.validate().map_err(m)?;
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
        let description = body_report(&wearer, &fit);
        (fit.params, description)
    };
    if !legs.is_standing() {
        description["legs"] = json!(legs);
    }
    if leg_fraction != 1.0 {
        description["leg_fraction"] = json!(leg_fraction);
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
        .pose(&rest, &pose_deltas(model, &rest, &pose, 1.0).map_err(m)?)
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
    let run = if two_start {
        ftx_drape::two_start::drape(
            &pat,
            fit_spec,
            &body,
            &opts,
            &TwoStartOptions::default(),
            &mut progress,
        )
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
        skin.contact_proxy_basis_with_rest(&rest_skin),
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
        let got = body_report(&wearer, &fit);
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
        wearer.weight_kg = None;
        assert!(body_report(&wearer, &fit)["fit"]["weight_kg"].is_null());
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
}
