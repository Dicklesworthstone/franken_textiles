//! FrankenTextiles in the browser (plan §15): the page's round (body, measured draft or someone else's
//! pattern, flat checks, measurement gate, every export with its acceptance checks, one zip) in [`page`],
//! testable natively against the reference's `webapp.py`, behind a thin `wasm-bindgen` boundary (`web`,
//! wasm32 only) so the numerical crates never see browser types.
#![forbid(unsafe_code)]

pub mod page;
#[cfg(target_arch = "wasm32")]
pub mod web;
pub mod zip;
