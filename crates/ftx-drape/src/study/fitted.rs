//! Fit one Anny body once for an entire material study, using the existing production pose pipeline.
use ftx_body::{
    AnnyModel, BodyFrames, Pose, SkinMesh, Wearer, fit_tape, pose_deltas, skin_mesh, tape_targets,
    trimmed_skin,
};
use ftx_core::Refusal;
use serde_json::{Value, json};

use super::{hash, invalid};
use crate::run::DrapeBody;

/// An owned, fixed posed body shared by all study alternatives. No material enters body fitting.
pub struct FittedBody {
    frames: BodyFrames,
    collision: SkinMesh,
    display: SkinMesh,
    /// Actual body-fitting residuals and source identity; the fit is model-derived, not measured.
    pub report: Value,
}

impl FittedBody {
    /// Fit from tape once and prepare the standard straight-arm 35-degree drape pose.
    /// Uses the same model, fitting, trimming and landmark functions as the existing fold-up.
    ///
    /// # Errors
    /// Missing age, invalid measurements/model, failed fit, or refused body geometry.
    pub fn fit(model: &AnnyModel, measurements: &Value) -> Result<Self, Refusal> {
        if !measurements["wearer"]["age_months"].is_number() {
            return Err(invalid("scenario body fitting requires wearer.age_months"));
        }
        let pose = Pose::drape();
        let wearer = Wearer::from_measurements(measurements)?;
        let fit = fit_tape(model, &wearer, &tape_targets(measurements), &pose)?;
        let rest = model.rest(&fit.params)?;
        let posed = model.pose(&rest, &pose_deltas(model, &rest, &pose, 1.0)?)?;
        let collision = trimmed_skin(
            model,
            &posed.vertices,
            &["wrist.R", "wrist.L", "foot.R", "foot.L"],
        )?;
        let display = skin_mesh(&posed.vertices, &model.data().faces)?;
        let frames = BodyFrames::new(model, &posed)?;
        let residuals: Vec<_> = fit
            .residuals
            .iter()
            .map(|r| {
                json!({"name":r.name,
            "target_mm":r.target_mm,"body_mm":r.body_mm,"residual_mm":r.body_mm-r.target_mm})
            })
            .collect();
        let report = json!({"measurements_sha256":hash(measurements)?,
            "pose":"standard_straight_arm_drape_35_degrees","fit_iterations":fit.iterations,
            "body_mass_kg":fit.mass_kg,"residuals":residuals,"body_provenance":"MODEL_FIT",
            "physical_validation_claim":false});
        Ok(Self {
            frames,
            collision,
            display,
            report,
        })
    }

    /// Borrow the same immutable body for each fresh cloth simulation.
    #[must_use]
    pub fn body(&self) -> DrapeBody<'_> {
        DrapeBody {
            frames: &self.frames,
            collision: (&self.collision.vertices, &self.collision.triangles),
            display: (&self.display.vertices, &self.display.triangles),
            arm_deg: 35.0,
        }
    }
}
