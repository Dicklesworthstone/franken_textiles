//! FrankenTextiles bodies: the Anny parametric body model (NAVER LABS Europe; Apache-2.0 code, CC0
//! MakeHuman/MPFB2 assets) from newborn to adult, evaluated in Rust from converted data
//! (`tools/body-model/convert_anny.py` → `.ftxbody`). Specified in `docs/spec/05-anny-forward-model.md`.
//!
//! - [`data`]: reading and checking `.ftxbody` files.
//! - [`model`]: phenotype and local-change coefficients, rest shape and skeleton, posing, anthropometry.
//! - [`kinematics`]: bone frames, forward kinematics, linear blend skinning.
//! - [`drape`]: poses and ramps for the drape, the nappy, and the checked skin mesh.
//! - [`landmarks`]: part frames, landmark heights and section radii for placing pattern pieces.
//! - [`fit`]: the body for a wearer's age, sex and stature.
//! - [`tape`]: girths the way a tape measures them (convex hulls of sections).
//!
//! Frame: metres, +Z up, the body faces −Y, +X is the body's left. Nothing is grounded: rest bodies sit in
//! the template frame (soles near z = −0.82 m), posed bodies are root-relative.
#![forbid(unsafe_code)]

pub mod accepted_shape;
pub mod correspondence;
pub mod data;
pub mod drape;
pub mod fit;
pub mod kinematics;
pub mod knit_shell;
pub mod landmarks;
pub mod model;
mod selected;
pub mod tape;

pub use data::{AnnyData, default_model_path};
pub use drape::{
    DEFAULT_POSE_AUDIT_PAIR_BUDGET, ElbowPose, HandCollider, JointRotation,
    LBS_VOLUME_CHANGE_ADVISORY_FRACTION, Legs, Pose, PoseGeometryReport, SkinIntersectionAudit,
    SkinMesh, SkinReport, ThumbPose, body_pose_geometry_json,
    body_pose_geometry_json_with_skinning, clothing_skin, clothing_skin_with_hands,
    joint_rotations, joint_rotations_with_thumbs, nappy_local_changes, pose_deltas,
    pose_deltas_with_thumbs, pose_rotation_vectors_with_thumbs, posed_skin,
    posed_skin_with_skinning, skin_mesh, trimmed_skin,
};
pub use fit::{
    FREE, MAX_NAPPY_SIZE, N_PHENOTYPE, N_TAPE, NappyOccupancy, TAPE_FIELDS, TapeFit, TapeResidual,
    TapeTarget, Wearer, calibrate_nappy, fit_sigma, fit_stature, fit_tape, is_girth,
    nappy_occupancy, partial_tape_readings, seat_girth, tape_readings, tape_targets,
};
pub use kinematics::{Affine, SkinningMode, dual_quaternion_skinning};
pub use landmarks::{BodyFrames, Frame};
pub use model::{
    AnnyModel, Anthropometry, BodyParams, Phenotype, PosedBody, RestBody, age_parameter_for_years,
    anchor_weights,
};
pub use selected::SelectedPose;
