//! Shared native/portable anatomical shell inputs. Geometry identity comes from the consumed
//! finite vertices and triangles; an optional expected digest prevents stale-source substitution.

use ftx_core::intersection::validate_mesh;
use ftx_core::receipt::sha256_hex;
use ftx_core::shaping::{EaseProfile, EaseShellMesh, KnitShellRegion, ShapingSurface};
use ftx_core::{Refusal, RefusalCode, Vec3};
use serde::{Deserialize, Serialize};

use super::{AnatomicalShellSpec, AnnyAnatomicalShell, build_anatomical_ease_shell};
use crate::{AnnyModel, Pose, SkinningMode, ThumbPose, pose_deltas, pose_deltas_with_thumbs};

/// Maximum explicit anatomical JSON request size (16 MiB).
pub const MAX_ANATOMICAL_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;

/// Explicit original closed skin and source-defined anatomical sections.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnatomicalShellRequest {
    /// Source vertices in metres, before collider trim caps.
    pub vertices_m: Vec<[f64; 3]>,
    /// Outward source triangles.
    pub triangles: Vec<[u32; 3]>,
    /// Actual section sources and requested normal displacements.
    pub spec: AnatomicalShellSpec,
    /// Optional binding to an earlier inspected source mesh. Never copied into the result
    /// without recomputing and comparing the identity of the actual consumed mesh.
    #[serde(default)]
    pub expected_body_mesh_sha256: Option<String>,
}

impl AnatomicalShellRequest {
    /// Extract the declared anatomical shell and bind it to the original source geometry.
    ///
    /// # Errors
    /// A changed expected source or any anatomical extraction refusal.
    pub fn build(&self) -> Result<EaseShellMesh, Refusal> {
        let vertices: Vec<_> = self.vertices_m.iter().copied().map(Vec3::from).collect();
        let triangles: Vec<_> = self
            .triangles
            .iter()
            .map(|t| t.map(|v| v as usize))
            .collect();
        let identity = body_mesh_sha256(&vertices, &triangles)?;
        if self
            .expected_body_mesh_sha256
            .as_ref()
            .is_some_and(|expected| expected != &identity)
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "anatomical source mesh SHA-256 mismatch; the selected body geometry changed",
            ));
        }
        build_anatomical_ease_shell(&vertices, &triangles, &self.spec, &identity)
    }
}

/// Canonical identity of the exact discrete body geometry, independent of garment cuts/ease.
/// Coordinates and index order are preserved; JSON whitespace is not part of the identity.
///
/// # Errors
/// Invalid/nonfinite geometry, source work bounds or serialization failures.
pub fn body_mesh_sha256(vertices: &[Vec3], triangles: &[[usize; 3]]) -> Result<String, Refusal> {
    if vertices.len() > super::MAX_SHELL_VERTICES || triangles.len() > super::MAX_SHELL_TRIANGLES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "body mesh exceeds anatomical shell work bounds",
        ));
    }
    validate_mesh(vertices, triangles)?;
    let positions: Vec<[f64; 3]> = vertices.iter().copied().map(Into::into).collect();
    let canonical = serde_json::to_vec(&("ftx-body-mesh/1", positions, triangles))
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    Ok(sha256_hex(&canonical))
}

/// Parse, extract and serialize explicit anatomy identically for native and portable callers.
///
/// # Errors
/// Input/output budgets, malformed JSON, stale source identity or geometric refusals.
pub fn anatomical_shell_json(request_json: &str) -> Result<String, Refusal> {
    if request_json.len() > MAX_ANATOMICAL_INPUT_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "anatomical shell input exceeds 16 MiB",
        ));
    }
    let request: AnatomicalShellRequest = serde_json::from_str(request_json).map_err(|e| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("anatomical shell input: {e}"),
        )
    })?;
    let shell = request.build()?;
    serialize_anatomical_shell(&shell)
}

/// Serialize a completed anatomical shell with the shared native/browser output budget.
///
/// # Errors
/// Serialization failure or output exceeding 32 MiB.
pub fn serialize_anatomical_shell(shell: &EaseShellMesh) -> Result<String, Refusal> {
    let output = serde_json::to_string(shell)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    if output.len() > MAX_OUTPUT_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "anatomical shell output exceeds 32 MiB",
        ));
    }
    Ok(output)
}

/// Existing body pose presets for source-backed anatomical extraction.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnatomicalBodyPose {
    /// Source rest pose.
    Rest,
    /// Standard standing drape pose.
    #[default]
    Drape,
    /// Existing arms-up preset.
    ArmsUp,
    /// Existing seated preset.
    Seated,
    /// Existing squat preset.
    Squat,
}

impl AnatomicalBodyPose {
    fn pose(self) -> Pose {
        match self {
            Self::Rest => Pose::rest(),
            Self::Drape => Pose::drape(),
            Self::ArmsUp => Pose::arms_up(),
            Self::Seated => Pose::seated(),
            Self::Squat => Pose::squat(),
        }
    }
}

/// Garment-region/ease request applied to a validated accepted Anny shape without refitting.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedAnatomicalShellRequest {
    /// Actual garment region; generic tubes need explicit source geometry.
    pub region: KnitShellRegion,
    /// Caller-authored wearing ease; no guessed wearer dimensions or clearance.
    pub ease: EaseProfile,
    /// Repose the accepted shape instead of reusing an image-frame pose.
    #[serde(default)]
    pub pose: AnatomicalBodyPose,
    /// Source LBS by default; DQS remains an explicit choice.
    #[serde(default)]
    pub skinning: SkinningMode,
    /// Optional authored source-local thumb rotations, as in the ordinary body route.
    #[serde(default)]
    pub thumb_pose: Option<ThumbPose>,
}

/// Reuse the existing accepted-shape admission and actual posed Anny rig, with no tape refit.
/// `model_sha256` must identify the same array-verified model bytes used to construct `model`.
///
/// # Errors
/// Unaccepted/inconsistent/wrong-model body fits, malformed pose/ease or geometry refusals.
pub fn build_accepted_anatomical_shell(
    model: &AnnyModel,
    model_sha256: &str,
    accepted_report: &[u8],
    request: &AcceptedAnatomicalShellRequest,
) -> Result<EaseShellMesh, Refusal> {
    super::validate_ease_profile(&request.ease)?;
    let (shape, _) = crate::accepted_shape::prepare(model, model_sha256, accepted_report, None)?;
    let rest = model.rest(&shape.params)?;
    let pose = request.pose.pose();
    let deltas = match request.thumb_pose {
        Some(thumbs) => pose_deltas_with_thumbs(model, &rest, &pose, 1.0, &thumbs),
        None => pose_deltas(model, &rest, &pose, 1.0),
    }?;
    let posed = model.pose_with_skinning(&rest, &deltas, request.skinning)?;
    let triangles: Vec<_> = model
        .data()
        .faces
        .iter()
        .map(|t| t.map(|v| v as usize))
        .collect();
    let source_body_sha256 = body_mesh_sha256(&posed.vertices, &triangles)?;
    AnnyAnatomicalShell {
        model,
        posed: &posed,
        source_body_sha256,
    }
    .build_ease_shell(request.region, &request.ease)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_hash_changes_with_consumed_geometry_and_rejects_nonfinite_coordinates() {
        let vertices = [
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let triangles = [[0, 1, 2]];
        let original = body_mesh_sha256(&vertices, &triangles).unwrap();
        let mut changed = vertices;
        changed[1].x = 1.1;
        assert_ne!(original, body_mesh_sha256(&changed, &triangles).unwrap());
        changed[1].x = f64::NAN;
        assert!(body_mesh_sha256(&changed, &triangles).is_err());
        assert_eq!(
            anatomical_shell_json("{\"vertices_m\":[]}")
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );
    }
}
