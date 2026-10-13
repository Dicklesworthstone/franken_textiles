//! FrankenTextiles exports (`docs/spec/03`): every deliverable as a projection of one pattern. So far the
//! text and number helpers (§2) and the seam-allowance cut outline (§3–4).
//!
//! Frame: millimetres, the pattern's own frame (x right, y up, loops counter-clockwise).
#![forbid(unsafe_code)]

pub mod outline;
pub mod text;
mod union;

pub use outline::{
    DEFAULT_SA, EdgeLoop, Method, clean_ring, cut_outline, drafted_instance, edge_loops,
    edge_points, edge_sa, ring_is_simple, sew_breaks, sew_ring,
};
pub use text::{
    Anchor, ascii_text, column_letters, fnum, page_list, pdf_string, safe_name, stroke_text,
    text_width, wrap, xml_escape,
};
