//! FrankenTextiles bodies: the Anny parametric body model (NAVER LABS Europe; Apache-2.0 code, CC0
//! MakeHuman/MPFB2 assets) from newborn to adult, evaluated in Rust from converted data
//! (`tools/body-model/convert_anny.py` → `.ftxbody`). Specified in `docs/spec/05-anny-forward-model.md`.
//!
//! - [`data`]: reading and checking `.ftxbody` files.
//! - [`model`]: phenotype and local-change coefficients, rest shape and skeleton, posing, anthropometry.
//! - [`kinematics`]: bone frames, forward kinematics, linear blend skinning.
//!
//! Frame: metres, +Z up, the body faces −Y, +X is the body's left. Nothing is grounded: rest bodies sit in
//! the template frame (soles near z = −0.82 m), posed bodies are root-relative.
#![forbid(unsafe_code)]

pub mod data;
pub mod kinematics;
pub mod model;

pub use data::{AnnyData, default_model_path};
pub use kinematics::Affine;
pub use model::{
    AnnyModel, Anthropometry, BodyParams, Phenotype, PosedBody, RestBody, age_parameter_for_years,
    anchor_weights,
};
