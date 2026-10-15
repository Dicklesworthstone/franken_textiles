//! FrankenTextiles in the browser (plan §15): the page's round (body, measured draft or someone else's
//! pattern, flat checks, measurement gate, every export with its acceptance checks, one zip) in [`page`],
//! testable natively against the reference's `webapp.py`, and the fold-up on a fitted body in [`fold`]
//! (`ftx drape` without files), behind a thin `wasm-bindgen` boundary (`web`, wasm32 only) so the numerical
//! crates never see browser types.
#![forbid(unsafe_code)]

pub mod fold;
pub mod manufacturing;
pub mod page;
pub mod tracing;
#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub mod web_tracing;
pub mod zip;
